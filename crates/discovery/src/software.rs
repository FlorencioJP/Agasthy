//! Installed applications and catalogued command-line tools on `PATH`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use agasthy_core::{
    Availability, Confidence, EntityInput, EntityKind, EvidenceInput, IngestBatch, IssueKind,
    PruneScope, RelationKind, RelationshipInput, ScanIssue, ScanKind,
};
use agasthy_platform::{
    CommandError, CommandRunner, CommandSpec, Platform, SearchDir, SearchDirOrigin, is_executable,
};

use crate::attrs::{BinaryAttrs, InstallHint, ToolCategory};
use crate::catalog::{PROBE_ENV, TOOLS, ToolSpec};
use crate::version::extract_version;
use crate::{ProgressFn, ScanProgress};

/// At most this many copies of one tool are version-probed.
const MAX_COPIES_PROBED: usize = 3;

pub fn app_id(path: &str) -> String {
    format!("application:{path}")
}

pub fn binary_id(kind: EntityKind, path: &Path) -> String {
    let prefix = if kind == EntityKind::Runtime {
        "runtime"
    } else {
        "binary"
    };
    format!("{prefix}:{}", path.display())
}

/// Discover installed applications and catalogued tools.
pub fn scan_software(
    platform: &dyn Platform,
    runner: &dyn CommandRunner,
    progress: ProgressFn<'_>,
) -> IngestBatch {
    let mut batch = IngestBatch::new(ScanKind::Software);

    progress(ScanProgress {
        phase: "applications".into(),
        done: 0,
        total: None,
        current: None,
    });
    let apps = platform.list_applications();
    batch.issues.extend(apps.issues);
    match apps.apps {
        Availability::Available { value } => {
            for app in value {
                batch.entities.push(EntityInput {
                    id: app_id(&app.path),
                    kind: EntityKind::Application,
                    name: app.name.clone(),
                    path: Some(app.path.clone()),
                    version: app.version.clone(),
                    source: "applications_dir".into(),
                    evidence: vec![EvidenceInput::new(
                        "bundle_metadata",
                        format!(
                            "bundle {} read from Info.plist",
                            app.bundle_id.as_deref().unwrap_or("(no identifier)")
                        ),
                        app.metadata_source.clone(),
                        Confidence::Verified,
                    )],
                    attributes: serde_json::to_value(&app).unwrap_or_default(),
                });
            }
            batch.prune.push(PruneScope {
                kind: EntityKind::Application,
                source: None,
            });
        }
        other => {
            let msg = match other {
                Availability::Unsupported { platform } => {
                    format!("application inventory is not implemented on {platform}")
                }
                Availability::PermissionDenied { detail } => detail,
                Availability::Unavailable { reason } => reason,
                Availability::Available { .. } => unreachable!(),
            };
            batch
                .issues
                .push(ScanIssue::new(IssueKind::Unsupported, "applications", msg));
        }
    }

    // Tools on PATH.
    let dirs = platform.executable_search_dirs();
    let found = locate_tools(platform, &dirs);
    let total = found.len() as u32;
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let out: Mutex<Vec<(EntityInput, Option<RelationshipInput>)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..6 {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(f) = found.get(i) else { break };
                    let e = tool_entity(platform, runner, f);
                    out.lock().unwrap().push(e);
                    let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(ScanProgress {
                        phase: "tools".into(),
                        done: d as u32,
                        total: Some(total),
                        current: Some(f.spec.id.to_string()),
                    });
                }
            });
        }
    });
    let mut out = out.into_inner().unwrap();
    out.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    for (entity, rel) in out {
        batch.entities.push(entity);
        if let Some(r) = rel {
            batch.relationships.push(r);
        }
    }
    batch.prune.push(PruneScope {
        kind: EntityKind::Binary,
        source: Some("path".into()),
    });
    batch.prune.push(PruneScope {
        kind: EntityKind::Runtime,
        source: Some("path".into()),
    });
    batch
}

/// Runtime family for a catalogued runtime tool id.
pub fn runtime_family(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "node" => "node",
        "python3" | "python" => "python",
        "rustc" => "rust",
        "go" => "go",
        "ruby" => "ruby",
        _ => return None,
    })
}

/// Runtime executables on PATH, with the entity ids the software scan gives them.
/// Cheap: no version probing.
pub fn runtime_copies(platform: &dyn Platform) -> Vec<crate::RuntimeCopy> {
    let dirs = platform.executable_search_dirs();
    locate_tools(platform, &dirs)
        .into_iter()
        .filter(|f| f.spec.category == ToolCategory::Runtime)
        .filter_map(|f| {
            runtime_family(f.spec.id).map(|family| crate::RuntimeCopy {
                family: family.into(),
                entity_id: binary_id(EntityKind::Runtime, &f.path),
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct FoundTool {
    pub spec: ToolSpec,
    pub path: PathBuf,
    pub dir: SearchDir,
    /// 0 for the copy that wins resolution.
    pub rank: usize,
    pub first_path: PathBuf,
}

/// Find every catalogued tool in the search directories, in resolution order.
pub fn locate_tools(platform: &dyn Platform, dirs: &[SearchDir]) -> Vec<FoundTool> {
    let mut out = Vec::new();
    for spec in TOOLS {
        let mut rank = 0usize;
        let mut first: Option<PathBuf> = None;
        let mut seen_canonical: Vec<PathBuf> = Vec::new();
        for dir in dirs {
            for file in platform.executable_file_names(spec.id) {
                let p = dir.path.join(&file);
                if !is_executable(&p) {
                    continue;
                }
                // The same file reachable through two PATH entries counts once.
                let canonical = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
                if seen_canonical.contains(&canonical) && first.is_some() {
                    continue;
                }
                seen_canonical.push(canonical);
                let first_path = first.get_or_insert_with(|| p.clone()).clone();
                out.push(FoundTool {
                    spec: *spec,
                    path: p,
                    dir: dir.clone(),
                    rank,
                    first_path,
                });
                rank += 1;
                break;
            }
        }
    }
    out
}

fn tool_entity(
    platform: &dyn Platform,
    runner: &dyn CommandRunner,
    f: &FoundTool,
) -> (EntityInput, Option<RelationshipInput>) {
    let kind = if f.spec.category == ToolCategory::Runtime {
        EntityKind::Runtime
    } else {
        EntityKind::Binary
    };
    let resolved = std::fs::canonicalize(&f.path).ok().filter(|r| r != &f.path);
    let effective = resolved.clone().unwrap_or_else(|| f.path.clone());

    let mut evidence = vec![EvidenceInput::new(
        "executable_on_path",
        format!(
            "executable `{}` found in {} ({})",
            f.path.file_name().unwrap_or_default().to_string_lossy(),
            f.dir.path.display(),
            origin_label(f.dir.origin)
        ),
        f.path.display().to_string(),
        Confidence::Verified,
    )];

    let mut version_command = None;
    let version: Availability<String> = match (f.spec.version_args, f.rank < MAX_COPIES_PROBED) {
        (None, _) => Availability::unavailable("version is not probed for this tool"),
        (Some(_), false) => Availability::unavailable("additional copy; version not probed"),
        (Some(args), true) => match platform.may_probe_version(f.spec.id, &f.path) {
            Err(reason) => Availability::unavailable(reason),
            Ok(()) => {
                let mut spec = CommandSpec::new(f.path.display().to_string(), args)
                    .timeout(Duration::from_secs(8))
                    .max_output(64 * 1024);
                for (k, v) in PROBE_ENV {
                    spec = spec.env(k, v);
                }
                version_command = Some(format!("{} {}", f.spec.id, args.join(" ")));
                match runner.run(&spec) {
                    Ok(o) => {
                        let text = format!("{}\n{}", o.stdout, o.stderr);
                        match extract_version(&text) {
                            Some(v) => {
                                let first_line = text
                                    .lines()
                                    .map(str::trim)
                                    .find(|l| !l.is_empty())
                                    .unwrap_or("")
                                    .chars()
                                    .take(160)
                                    .collect::<String>();
                                evidence.push(EvidenceInput::new(
                                    "version_probe",
                                    format!("reported: {first_line}"),
                                    spec.display(),
                                    Confidence::Verified,
                                ));
                                Availability::available(v)
                            }
                            None => Availability::unavailable(if o.success() {
                                "no version number in output".to_string()
                            } else {
                                format!("exited with {:?} and no version", o.exit_code)
                            }),
                        }
                    }
                    Err(CommandError::TimedOut { after_ms, .. }) => {
                        Availability::unavailable(format!("no response within {after_ms} ms"))
                    }
                    Err(CommandError::PermissionDenied { .. }) => {
                        Availability::permission_denied("not permitted to execute")
                    }
                    Err(e) => Availability::unavailable(e.to_string()),
                }
            }
        },
    };

    let (install_hint, relationship) = infer_installer(&effective);
    let id = binary_id(kind, &f.path);
    let relationship = relationship.map(|(pkg_id, detail, confidence)| RelationshipInput {
        attributes: serde_json::Value::Null,
        from_id: id.clone(),
        to_id: pkg_id,
        kind: RelationKind::InstalledBy,
        confidence,
        evidence: vec![EvidenceInput::new(
            "resolved_path",
            detail,
            effective.display().to_string(),
            confidence,
        )],
    });

    let attrs = BinaryAttrs {
        tool: f.spec.id.into(),
        display_name: f.spec.display_name.into(),
        category: f.spec.category,
        file_name: f
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        directory: f.dir.path.display().to_string(),
        directory_origin: f.dir.origin,
        resolved_path: resolved.map(|r| r.display().to_string()),
        active: f.rank == 0,
        rank: f.rank as u32,
        shadowed_by: (f.rank > 0).then(|| f.first_path.display().to_string()),
        version: version.clone(),
        version_command,
        install_hint,
    };
    (
        EntityInput {
            id,
            kind,
            name: f.spec.display_name.into(),
            path: Some(f.path.display().to_string()),
            version: version.into_value(),
            source: "path".into(),
            attributes: serde_json::to_value(&attrs).unwrap_or_default(),
            evidence,
        },
        relationship,
    )
}

fn origin_label(o: SearchDirOrigin) -> &'static str {
    match o {
        SearchDirOrigin::ProcessPath => "on this process's PATH",
        SearchDirOrigin::SystemPathConfig => "listed in /etc/paths",
        SearchDirOrigin::WellKnown => "conventional location, not on this process's PATH",
    }
}

/// Infer how a binary was installed from where its resolved path lives.
///
/// Returns a hint for display and, where the location is owned by a package
/// manager we inventory, a relationship target with its confidence.
pub fn infer_installer(
    resolved: &Path,
) -> (Option<InstallHint>, Option<(String, String, Confidence)>) {
    let s = resolved.to_string_lossy().replace('\\', "/");
    let seg_after = |marker: &str| -> Option<String> {
        let i = s.find(marker)?;
        let rest = &s[i + marker.len()..];
        rest.split('/')
            .next()
            .filter(|x| !x.is_empty())
            .map(str::to_string)
    };
    let hint = |installer: &str, detail: String| {
        Some(InstallHint {
            installer: installer.into(),
            detail,
        })
    };

    if let Some(formula) = seg_after("/Cellar/") {
        let detail = format!("resolves into the Homebrew keg for `{formula}`");
        return (
            hint("homebrew", detail.clone()),
            Some((
                format!("package:homebrew:{formula}"),
                detail,
                Confidence::Verified,
            )),
        );
    }
    if let Some(cask) = seg_after("/Caskroom/") {
        let detail = format!("resolves into the Homebrew Caskroom for `{cask}`");
        return (
            hint("homebrew", detail.clone()),
            Some((
                format!("package:homebrew-cask:{cask}"),
                detail,
                Confidence::Verified,
            )),
        );
    }
    if let Some(tool) = seg_after("/uv/tools/") {
        let detail = format!("resolves into the uv tool environment `{tool}`");
        return (
            hint("uv", detail.clone()),
            Some((format!("package:uv:{tool}"), detail, Confidence::Strong)),
        );
    }
    if s.contains("/pnpm/global/")
        && let Some(pkg) = node_package_after(&s, "/node_modules/")
    {
        let detail = format!("resolves into pnpm's global store for `{pkg}`");
        return (
            hint("pnpm", detail.clone()),
            Some((format!("package:pnpm:{pkg}"), detail, Confidence::Strong)),
        );
    }
    if s.contains("/lib/node_modules/")
        && !s.contains("/.nvm/")
        && let Some(pkg) = node_package_after(&s, "/lib/node_modules/")
    {
        let detail = format!("resolves into npm's global node_modules for `{pkg}`");
        return (
            hint("npm", detail.clone()),
            Some((format!("package:npm:{pkg}"), detail, Confidence::Strong)),
        );
    }
    let path_hints: &[(&str, &str, &str)] = &[
        (
            "/.nvm/versions/",
            "nvm",
            "inside an nvm-managed Node.js version",
        ),
        ("/.volta/", "volta", "inside a Volta-managed toolchain"),
        (
            "/.rustup/toolchains/",
            "rustup",
            "inside a rustup toolchain",
        ),
        (
            "/.cargo/bin/",
            "cargo",
            "in ~/.cargo/bin (rustup proxy or cargo install)",
        ),
        ("/.pyenv/", "pyenv", "inside a pyenv-managed Python"),
        ("/.rbenv/", "rbenv", "inside an rbenv-managed Ruby"),
        ("/.asdf/", "asdf", "inside an asdf-managed tool"),
        ("/mise/", "mise", "inside a mise-managed tool"),
        ("/.bun/", "bun", "inside Bun's install directory"),
        ("/.deno/", "deno", "inside Deno's install directory"),
        (
            "/Applications/",
            "application",
            "inside an application bundle",
        ),
        (
            "/Library/Developer/CommandLineTools/",
            "xcode-clt",
            "part of the Xcode Command Line Tools",
        ),
        ("/Applications/Xcode.app/", "xcode", "part of Xcode"),
    ];
    for (marker, installer, detail) in path_hints {
        if s.contains(marker) {
            return (hint(installer, (*detail).into()), None);
        }
    }
    if s.starts_with("/usr/bin/") || s.starts_with("/bin/") || s.starts_with("/usr/sbin/") {
        return (
            hint("system", "shipped with the operating system".into()),
            None,
        );
    }
    (None, None)
}

fn node_package_after(s: &str, marker: &str) -> Option<String> {
    let i = s.rfind(marker)?;
    let rest = &s[i + marker.len()..];
    let mut parts = rest.split('/');
    let first = parts.next()?;
    if first.starts_with('@') {
        Some(format!("{first}/{}", parts.next()?))
    } else if first.is_empty() {
        None
    } else {
        Some(first.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_inference() {
        let (h, r) = infer_installer(Path::new("/opt/homebrew/Cellar/jq/1.7.1/bin/jq"));
        assert_eq!(h.unwrap().installer, "homebrew");
        let (id, _, c) = r.unwrap();
        assert_eq!(id, "package:homebrew:jq");
        assert_eq!(c, Confidence::Verified);

        let (_, r) = infer_installer(Path::new(
            "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js",
        ));
        assert_eq!(r.unwrap().0, "package:npm:@anthropic-ai/claude-code");

        let (h, r) = infer_installer(Path::new("/Users/u/.nvm/versions/node/v20.1.0/bin/node"));
        assert_eq!(h.unwrap().installer, "nvm");
        assert!(r.is_none());

        let (_, r) = infer_installer(Path::new("/Users/u/.local/share/uv/tools/ruff/bin/ruff"));
        assert_eq!(r.unwrap().0, "package:uv:ruff");

        let (h, r) = infer_installer(Path::new("/usr/bin/git"));
        assert_eq!(h.unwrap().installer, "system");
        assert!(r.is_none());

        let (h, r) = infer_installer(Path::new("/some/random/place/tool"));
        assert!(h.is_none() && r.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn locates_tools_in_order_and_marks_shadowed_copies() {
        use std::os::unix::fs::PermissionsExt;
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        for d in [a.path(), b.path()] {
            let p = d.join("jq");
            std::fs::write(&p, "#!/bin/false\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // A non-executable file with a catalogued name is ignored.
        std::fs::write(a.path().join("rg"), "x").unwrap();
        let dirs = vec![
            SearchDir {
                path: a.path().to_path_buf(),
                origin: SearchDirOrigin::ProcessPath,
            },
            SearchDir {
                path: b.path().to_path_buf(),
                origin: SearchDirOrigin::WellKnown,
            },
        ];
        let platform = agasthy_platform::current();
        let found = locate_tools(platform.as_ref(), &dirs);
        let jq: Vec<_> = found.iter().filter(|f| f.spec.id == "jq").collect();
        assert_eq!(jq.len(), 2);
        assert_eq!(jq[0].rank, 0);
        assert_eq!(jq[1].rank, 1);
        assert_eq!(jq[1].first_path, a.path().join("jq"));
        assert!(!found.iter().any(|f| f.spec.id == "rg"));
    }
}
