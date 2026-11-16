//! Git repository discovery inside user-approved scan roots.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use agasthy_core::{
    Availability, Confidence, EntityInput, EntityKind, EvidenceInput, IngestBatch, IssueKind,
    PruneScope, RelationKind, RelationshipInput, ScanIssue, ScanKind,
};
use agasthy_platform::CommandRunner;
use agasthy_provenance::{ProvenanceSource, RemoteRef, confidence_for};
use walkdir::WalkDir;

use crate::attrs::{LanguageShare, LicenseInfo, RepositoryAttrs};
use crate::{ProgressFn, ScanProgress, git};

/// Directory names never descended into while looking for repositories.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".hg",
    ".svn",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".gradle",
    ".m2",
    ".npm",
    ".pnpm-store",
    ".yarn",
    ".cache",
    ".cargo",
    ".rustup",
    ".nvm",
    ".Trash",
    "DerivedData",
    "Pods",
    ".next",
    ".nuxt",
    ".turbo",
    "dist",
    "build",
    "vendor",
    "Library",
];

/// Directory names excluded from language statistics (still counted in size).
const NON_SOURCE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    "vendor",
    ".next",
    "Pods",
    "DerivedData",
];

#[derive(Debug, Clone)]
pub struct RepoScanOptions {
    pub roots: Vec<(PathBuf, usize)>,
    /// Upper bound on directories visited per root, to keep scans bounded.
    pub max_dirs_per_root: usize,
    /// Upper bound on files examined when measuring a single repository.
    pub max_files_per_repo: usize,
    pub workers: usize,
}

impl RepoScanOptions {
    pub fn new(roots: Vec<(PathBuf, usize)>) -> Self {
        Self {
            roots,
            max_dirs_per_root: 250_000,
            max_files_per_repo: 300_000,
            workers: 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundRepo {
    pub path: PathBuf,
    pub root: PathBuf,
    pub git_is_file: bool,
}

/// Walk `root` (never following symlinks) and return directories containing `.git`.
pub fn find_repositories(
    root: &Path,
    max_depth: usize,
    max_dirs: usize,
    issues: &mut Vec<ScanIssue>,
) -> Vec<FoundRepo> {
    let mut found = Vec::new();
    let mut visited = 0usize;
    let mut it = WalkDir::new(root)
        .follow_links(false)
        .max_depth(max_depth)
        .into_iter();
    loop {
        let entry = match it.next() {
            None => break,
            Some(Ok(e)) => e,
            Some(Err(e)) => {
                let path = e
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| root.display().to_string());
                let kind = match e.io_error().map(|io| io.kind()) {
                    Some(std::io::ErrorKind::PermissionDenied) => IssueKind::PermissionDenied,
                    Some(std::io::ErrorKind::NotFound) => IssueKind::NotFound,
                    _ => IssueKind::Other,
                };
                issues.push(ScanIssue::new(kind, path, e.to_string()));
                continue;
            }
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        visited += 1;
        if visited > max_dirs {
            issues.push(ScanIssue::new(
                IssueKind::Skipped,
                root.display().to_string(),
                format!("stopped after visiting {max_dirs} folders; choose a narrower folder"),
            ));
            break;
        }
        let name = entry.file_name().to_string_lossy();
        if entry.depth() > 0 && SKIP_DIRS.contains(&name.as_ref()) {
            it.skip_current_dir();
            continue;
        }
        let dot_git = entry.path().join(".git");
        if let Ok(meta) = std::fs::symlink_metadata(&dot_git)
            && (meta.is_dir() || meta.is_file())
        {
            found.push(FoundRepo {
                path: entry.path().to_path_buf(),
                root: root.to_path_buf(),
                git_is_file: meta.is_file(),
            });
            // Nested repositories inside a working tree are not reported.
            it.skip_current_dir();
        }
    }
    found
}

/// Discover repositories under every root and build an ingest batch.
pub fn scan_repositories(
    opts: &RepoScanOptions,
    runner: &dyn CommandRunner,
    progress: ProgressFn<'_>,
) -> IngestBatch {
    let mut batch = IngestBatch::new(ScanKind::Repositories);
    if !git::git_available(runner) {
        batch.issues.push(ScanIssue::new(
            IssueKind::NotFound,
            "git",
            "git was not found; repository details (branch, status, remotes) are unavailable",
        ));
    }

    let mut found: Vec<FoundRepo> = Vec::new();
    for (root, depth) in &opts.roots {
        progress(ScanProgress {
            phase: "searching".into(),
            done: found.len() as u32,
            total: None,
            current: Some(root.display().to_string()),
        });
        found.extend(find_repositories(
            root,
            *depth,
            opts.max_dirs_per_root,
            &mut batch.issues,
        ));
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);

    let total = found.len() as u32;
    let done = AtomicU32::new(0);
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(FoundRepo, RepositoryAttrs, Vec<ScanIssue>)>> =
        Mutex::new(Vec::with_capacity(found.len()));
    let workers = opts.workers.clamp(1, 16).min(found.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(repo) = found.get(i) else { break };
                    let (attrs, issues) = inspect_repository(repo, runner, opts.max_files_per_repo);
                    results.lock().unwrap().push((repo.clone(), attrs, issues));
                    let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(ScanProgress {
                        phase: "inspecting".into(),
                        done: d,
                        total: Some(total),
                        current: Some(repo.path.display().to_string()),
                    });
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.0.path.cmp(&b.0.path));

    // Duplicate clones: normal clones (not worktrees) sharing a primary remote.
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, (repo, attrs, _)) in results.iter().enumerate() {
        if let Some(r) = &attrs.primary_remote
            && !repo.git_is_file
        {
            groups.entry(r.key()).or_default().push(i);
        }
    }
    for idxs in groups.values() {
        let n = idxs.len() as u32;
        for &i in idxs {
            results[i].1.clones_of_same_remote = n;
        }
    }

    let mut remotes: HashMap<String, RemoteRef> = HashMap::new();
    for (repo, attrs, issues) in &results {
        batch.issues.extend(issues.iter().cloned());
        let id = repo_id(&repo.path);
        for remote in &attrs.remotes {
            let Some(parsed) = &remote.parsed else {
                continue;
            };
            remotes
                .entry(parsed.entity_id())
                .or_insert_with(|| parsed.clone());
            let kind = if remote.name == "origin" {
                RelationKind::OriginRemote
            } else {
                RelationKind::TracksRemote
            };
            batch.relationships.push(RelationshipInput {
                from_id: id.clone(),
                to_id: parsed.entity_id(),
                kind,
                confidence: confidence_for(ProvenanceSource::GitConfigRemote),
                evidence: vec![EvidenceInput::new(
                    "git_config_remote",
                    format!("remote.{}.url = {}", remote.name, remote.url),
                    format!("git config ({})", repo.path.join(".git").display()),
                    Confidence::Verified,
                )],
            });
        }
        let mut evidence = vec![EvidenceInput::new(
            "git_directory",
            if repo.git_is_file {
                "`.git` is a file: a linked worktree or submodule"
            } else {
                "`.git` directory present"
            },
            repo.path.join(".git").display().to_string(),
            Confidence::Verified,
        )];
        if let Some(l) = &attrs.license {
            evidence.push(EvidenceInput::new(
                "license",
                format!(
                    "license {} read from {}",
                    l.spdx.as_deref().unwrap_or("(unrecognised text)"),
                    l.source
                ),
                repo.path.join(&l.source).display().to_string(),
                if l.spdx.is_some() {
                    Confidence::Strong
                } else {
                    Confidence::Unknown
                },
            ));
        }
        for m in &attrs.manifests {
            evidence.push(EvidenceInput::new(
                "manifest",
                format!("{m} present"),
                repo.path.join(m).display().to_string(),
                Confidence::Verified,
            ));
        }
        let name = repo
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| repo.path.display().to_string());
        batch.entities.push(EntityInput {
            id,
            kind: EntityKind::Repository,
            name,
            path: Some(repo.path.display().to_string()),
            version: attrs.head.value().and_then(|h| {
                h.commit_sha
                    .as_ref()
                    .map(|s| s[..s.len().min(12)].to_string())
            }),
            source: "git".into(),
            attributes: serde_json::to_value(attrs).unwrap_or_default(),
            evidence,
        });
    }

    for (key, idxs) in &groups {
        if idxs.len() < 2 {
            continue;
        }
        let first = &results[idxs[0]].0.path;
        for &i in &idxs[1..] {
            let other = &results[i].0.path;
            batch.relationships.push(RelationshipInput {
                from_id: repo_id(other),
                to_id: repo_id(first),
                kind: RelationKind::DuplicateClone,
                confidence: Confidence::Verified,
                evidence: vec![EvidenceInput::new(
                    "shared_remote",
                    format!("both repositories name {key} as their primary remote"),
                    "git config",
                    Confidence::Verified,
                )],
            });
        }
    }

    for (id, r) in remotes {
        batch.entities.push(remote_entity(&id, &r, "git"));
    }

    batch.prune.push(PruneScope {
        kind: EntityKind::Repository,
        source: None,
    });
    progress(ScanProgress {
        phase: "done".into(),
        done: total,
        total: Some(total),
        current: None,
    });
    batch
}

pub fn repo_id(path: &Path) -> String {
    format!("repository:{}", path.display())
}

pub fn remote_entity(id: &str, r: &RemoteRef, source: &str) -> EntityInput {
    EntityInput {
        id: id.to_string(),
        kind: EntityKind::RemoteProject,
        name: r.display(),
        path: None,
        version: None,
        source: source.into(),
        attributes: serde_json::to_value(r).unwrap_or_default(),
        evidence: vec![],
    }
}

pub fn inspect_repository(
    repo: &FoundRepo,
    runner: &dyn CommandRunner,
    max_files: usize,
) -> (RepositoryAttrs, Vec<ScanIssue>) {
    let mut issues = Vec::new();
    let head = git::read_head(runner, &repo.path);
    let working_tree = git::read_status(runner, &repo.path);
    let local_branches = git::read_branch_count(runner, &repo.path);
    let remotes = match git::read_remotes(runner, &repo.path) {
        Ok(r) => r,
        Err(e) => {
            if git::git_available(runner) {
                issues.push(ScanIssue::new(
                    IssueKind::CommandFailed,
                    repo.path.display().to_string(),
                    format!("could not read remotes: {e}"),
                ));
            }
            Vec::new()
        }
    };
    let primary_remote = remotes
        .iter()
        .find(|r| r.name == "origin")
        .and_then(|r| r.parsed.clone())
        .or_else(|| remotes.iter().find_map(|r| r.parsed.clone()));

    let manifests = detect_manifests(&repo.path);
    let (languages, size_bytes) = measure(&repo.path, max_files);
    let primary_language = languages.first().map(|l| l.language.clone());
    let frameworks = detect_frameworks(&repo.path, &manifests);
    let license = detect_license(&repo.path);

    (
        RepositoryAttrs {
            scan_root: repo.root.display().to_string(),
            git_dir_kind: if repo.git_is_file {
                "file"
            } else {
                "directory"
            }
            .into(),
            head,
            working_tree,
            local_branches,
            remotes,
            primary_remote,
            manifests,
            languages,
            primary_language,
            frameworks,
            license,
            size_bytes,
            clones_of_same_remote: 1,
        },
        issues,
    )
}

const MANIFESTS: &[&str] = &[
    "package.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "package-lock.json",
    "bun.lockb",
    "Cargo.toml",
    "Cargo.lock",
    "pyproject.toml",
    "requirements.txt",
    "uv.lock",
    "poetry.lock",
    "Pipfile",
    "setup.py",
    "go.mod",
    "Gemfile",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "composer.json",
    "Package.swift",
    "Podfile",
    "mix.exs",
    "deno.json",
    "CMakeLists.txt",
    "Makefile",
    "Dockerfile",
    "docker-compose.yml",
    "compose.yaml",
    ".tool-versions",
    ".nvmrc",
    ".python-version",
    "rust-toolchain.toml",
];

fn detect_manifests(repo: &Path) -> Vec<String> {
    MANIFESTS
        .iter()
        .filter(|m| {
            std::fs::symlink_metadata(repo.join(m))
                .map(|md| md.is_file())
                .unwrap_or(false)
        })
        .map(|m| m.to_string())
        .collect()
}

fn language_for(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "rb" => "Ruby",
        "php" => "PHP",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "m" | "mm" => "Objective-C",
        "scala" => "Scala",
        "ex" | "exs" => "Elixir",
        "erl" => "Erlang",
        "hs" => "Haskell",
        "lua" => "Lua",
        "dart" => "Dart",
        "zig" => "Zig",
        "sh" | "bash" | "zsh" => "Shell",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "sql" => "SQL",
        "r" => "R",
        "jl" => "Julia",
        "ml" | "mli" => "OCaml",
        "clj" | "cljs" => "Clojure",
        "tf" => "HCL",
        _ => return None,
    })
}

/// One walk of the repository: total size (everything, including `.git`)
/// and language counts (source files only). Never follows symlinks.
fn measure(repo: &Path, max_files: usize) -> (Vec<LanguageShare>, Availability<u64>) {
    let mut total: u64 = 0;
    let mut files = 0usize;
    let mut counts: HashMap<&'static str, u32> = HashMap::new();
    let mut denied = false;
    let mut capped = false;
    for entry in WalkDir::new(repo).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                if e.io_error().map(|io| io.kind()) == Some(std::io::ErrorKind::PermissionDenied) {
                    denied = true;
                }
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        files += 1;
        if files > max_files {
            capped = true;
            break;
        }
        if let Ok(md) = entry.metadata() {
            total += md.len();
        }
        let rel = entry.path().strip_prefix(repo).unwrap_or(entry.path());
        let in_non_source = rel
            .components()
            .any(|c| NON_SOURCE_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()));
        if in_non_source {
            continue;
        }
        if let Some(lang) = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .and_then(|e| language_for(&e))
        {
            *counts.entry(lang).or_default() += 1;
        }
    }
    let mut langs: Vec<LanguageShare> = counts
        .into_iter()
        .map(|(language, files)| LanguageShare {
            language: language.to_string(),
            files,
        })
        .collect();
    langs.sort_by(|a, b| b.files.cmp(&a.files).then(a.language.cmp(&b.language)));
    let size = if capped {
        Availability::unavailable(format!(
            "more than {max_files} files; size not measured to keep the scan bounded"
        ))
    } else if denied && total == 0 {
        Availability::permission_denied("files in this repository could not be read")
    } else {
        Availability::available(total)
    };
    (langs, size)
}

/// Read at most `cap` bytes of a regular file (not a symlink).
pub(crate) fn read_small(path: &Path, cap: u64) -> Option<String> {
    let md = std::fs::symlink_metadata(path).ok()?;
    if !md.is_file() || md.len() > cap {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn detect_frameworks(repo: &Path, manifests: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let has = |m: &str| manifests.iter().any(|x| x == m);
    if has("package.json")
        && let Some(text) = read_small(&repo.join("package.json"), 1024 * 1024)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
    {
        let mut deps: Vec<String> = Vec::new();
        for key in ["dependencies", "devDependencies", "peerDependencies"] {
            if let Some(obj) = v.get(key).and_then(|d| d.as_object()) {
                deps.extend(obj.keys().cloned());
            }
        }
        for (dep, fw) in [
            ("next", "Next.js"),
            ("react", "React"),
            ("vue", "Vue"),
            ("svelte", "Svelte"),
            ("@angular/core", "Angular"),
            ("nuxt", "Nuxt"),
            ("astro", "Astro"),
            ("express", "Express"),
            ("fastify", "Fastify"),
            ("@nestjs/core", "NestJS"),
            ("electron", "Electron"),
            ("@tauri-apps/api", "Tauri"),
            ("react-native", "React Native"),
            ("vite", "Vite"),
        ] {
            if deps.iter().any(|d| d == dep) {
                out.push(fw.into());
            }
        }
    }
    let scan_lines = |file: &str, pairs: &[(&str, &str)], out: &mut Vec<String>| {
        if let Some(text) = read_small(&repo.join(file), 512 * 1024) {
            for (needle, fw) in pairs {
                let hit = text.lines().any(|l| {
                    let l = l.trim_start().to_ascii_lowercase();
                    l.starts_with(needle)
                        && l[needle.len()..]
                            .chars()
                            .next()
                            .map(|c| {
                                matches!(
                                    c,
                                    ' ' | '=' | '>' | '<' | '~' | '[' | '"' | '\'' | '.' | ';'
                                )
                            })
                            .unwrap_or(true)
                });
                if hit && !out.iter().any(|f| f == fw) {
                    out.push((*fw).into());
                }
            }
        }
    };
    if has("Cargo.toml") {
        scan_lines(
            "Cargo.toml",
            &[
                ("tauri", "Tauri"),
                ("axum", "Axum"),
                ("actix-web", "Actix Web"),
                ("rocket", "Rocket"),
                ("bevy", "Bevy"),
                ("tokio", "Tokio"),
            ],
            &mut out,
        );
    }
    for f in ["pyproject.toml", "requirements.txt"] {
        if has(f) {
            scan_lines(
                f,
                &[
                    ("django", "Django"),
                    ("fastapi", "FastAPI"),
                    ("flask", "Flask"),
                    ("torch", "PyTorch"),
                    ("\"django", "Django"),
                    ("\"fastapi", "FastAPI"),
                    ("\"flask", "Flask"),
                    ("\"torch", "PyTorch"),
                ],
                &mut out,
            );
        }
    }
    if has("go.mod") {
        scan_lines(
            "go.mod",
            &[
                ("github.com/gin-gonic/gin", "Gin"),
                ("github.com/gofiber/fiber", "Fiber"),
            ],
            &mut out,
        );
    }
    if has("Gemfile") {
        scan_lines(
            "Gemfile",
            &[("gem 'rails'", "Rails"), ("gem \"rails\"", "Rails")],
            &mut out,
        );
    }
    out.sort();
    out.dedup();
    out
}

pub(crate) fn spdx_from_text(text: &str) -> Option<&'static str> {
    let head: String = text.chars().take(4000).collect::<String>().to_lowercase();
    let head = head.split_whitespace().collect::<Vec<_>>().join(" ");
    let checks: &[(&str, &str)] = &[
        ("apache license version 2.0", "Apache-2.0"),
        ("apache license, version 2.0", "Apache-2.0"),
        ("gnu affero general public license version 3", "AGPL-3.0"),
        ("gnu lesser general public license version 3", "LGPL-3.0"),
        ("gnu lesser general public license version 2.1", "LGPL-2.1"),
        ("gnu general public license version 3", "GPL-3.0"),
        ("gnu general public license version 2", "GPL-2.0"),
        ("mozilla public license version 2.0", "MPL-2.0"),
        ("mozilla public license, version 2.0", "MPL-2.0"),
        ("permission is hereby granted, free of charge", "MIT"),
        (
            "this is free and unencumbered software released into the public domain",
            "Unlicense",
        ),
        ("isc license", "ISC"),
        ("boost software license", "BSL-1.0"),
        ("eclipse public license - v 2.0", "EPL-2.0"),
    ];
    for (needle, id) in checks {
        if head.contains(needle) {
            return Some(id);
        }
    }
    if head.contains("redistribution and use in source and binary forms") {
        return Some(if head.contains("neither the name") {
            "BSD-3-Clause"
        } else {
            "BSD-2-Clause"
        });
    }
    None
}

fn detect_license(repo: &Path) -> Option<LicenseInfo> {
    for name in [
        "LICENSE",
        "LICENSE.md",
        "LICENSE.txt",
        "LICENCE",
        "LICENCE.md",
        "COPYING",
        "LICENSE-APACHE",
        "LICENSE-MIT",
    ] {
        if let Some(text) = read_small(&repo.join(name), 256 * 1024) {
            return Some(LicenseInfo {
                spdx: spdx_from_text(&text).map(str::to_string),
                source: name.into(),
            });
        }
    }
    if let Some(text) = read_small(&repo.join("package.json"), 1024 * 1024)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(l) = v.get("license").and_then(|l| l.as_str())
    {
        let l: String = l.chars().take(64).collect();
        return Some(LicenseInfo {
            spdx: Some(l),
            source: "package.json".into(),
        });
    }
    for file in ["Cargo.toml", "pyproject.toml"] {
        if let Some(text) = read_small(&repo.join(file), 512 * 1024) {
            for line in text.lines() {
                let l = line.trim();
                if let Some(rest) = l.strip_prefix("license") {
                    let rest = rest.trim_start();
                    if let Some(val) = rest.strip_prefix('=') {
                        let val = val.trim().trim_matches('"').trim_matches('\'');
                        if !val.is_empty() && !val.starts_with('{') && val.len() <= 64 {
                            return Some(LicenseInfo {
                                spdx: Some(val.to_string()),
                                source: file.into(),
                            });
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_detection() {
        assert_eq!(
            spdx_from_text(
                "MIT License\n\nPermission is hereby granted, free of charge, to any person"
            ),
            Some("MIT")
        );
        assert_eq!(
            spdx_from_text(
                "                                 Apache License\n                           Version 2.0, January 2004"
            ),
            Some("Apache-2.0")
        );
        assert_eq!(spdx_from_text("All rights reserved."), None);
    }

    #[test]
    fn languages_and_frameworks_from_a_temp_repo() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("src")).unwrap();
        std::fs::create_dir_all(r.join("node_modules/dep")).unwrap();
        std::fs::write(r.join("src/a.ts"), "export {}").unwrap();
        std::fs::write(r.join("src/b.tsx"), "export {}").unwrap();
        std::fs::write(r.join("src/c.rs"), "fn main(){}").unwrap();
        std::fs::write(r.join("node_modules/dep/index.js"), "x").unwrap();
        std::fs::write(
            r.join("package.json"),
            r#"{"license":"MIT","dependencies":{"react":"19"},"devDependencies":{"vite":"8"}}"#,
        )
        .unwrap();
        let manifests = detect_manifests(r);
        assert_eq!(manifests, vec!["package.json".to_string()]);
        let (langs, size) = measure(r, 1000);
        assert_eq!(langs[0].language, "TypeScript");
        assert_eq!(langs[0].files, 2);
        assert!(
            !langs.iter().any(|l| l.language == "JavaScript"),
            "node_modules excluded"
        );
        assert!(size.value().copied().unwrap() > 0);
        assert_eq!(detect_frameworks(r, &manifests), vec!["React", "Vite"]);
        assert_eq!(detect_license(r).unwrap().spdx.as_deref(), Some("MIT"));
        // Size measurement is bounded.
        let (_, capped) = measure(r, 2);
        assert!(!capped.is_available());
    }
}
