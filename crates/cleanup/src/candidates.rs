//! Finding cleanup candidates. Read-only.

use std::path::{Path, PathBuf};

use agasthy_core::time::system_time_ms;
use agasthy_core::{Availability, IssueKind, ScanIssue};
use agasthy_platform::CommandRunner;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use walkdir::WalkDir;

use crate::docker::{self, DockerUsage};
use crate::{dir_size, safety};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CleanupCategory {
    NodeModules,
    RustTarget,
    PythonVenv,
    BuildOutput,
    PackageCache,
    XcodeDerivedData,
    DeveloperCache,
    ModelCache,
    DuplicateClone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SafetyLevel {
    /// Regenerated automatically or by one documented command.
    Safe,
    /// Regenerable, but at a cost (large re-download, device re-sync).
    Caution,
    /// Shown for information; AGASTHY will not remove it.
    ReviewOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CleanupCandidate {
    /// Stable id: the canonical path.
    pub id: String,
    pub category: CleanupCategory,
    pub path: String,
    pub size_bytes: Availability<u64>,
    pub last_modified: Option<i64>,
    pub safety: SafetyLevel,
    pub reason: String,
    /// How to get it back if needed.
    pub regenerate: Option<String>,
    /// Repository the artifact belongs to.
    pub project: Option<String>,
    /// Checks passed; can be included in a cleanup plan.
    pub eligible: bool,
    /// Why it is not eligible.
    pub blocked_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CategoryTotal {
    pub category: CleanupCategory,
    pub count: u32,
    pub bytes: u64,
    pub eligible_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiskAnalysis {
    pub analysed_at: i64,
    pub candidates: Vec<CleanupCandidate>,
    pub totals: Vec<CategoryTotal>,
    pub eligible_bytes: u64,
    pub docker: Availability<DockerUsage>,
    pub issues: Vec<ScanIssue>,
}

pub struct AnalysisInput {
    pub home: PathBuf,
    pub scan_roots: Vec<PathBuf>,
    /// (repository path, duplicate group size) for every known repository.
    pub repos: Vec<(PathBuf, u32)>,
}

const MAX_ENTRIES: usize = 400_000;

struct GlobalCache {
    rel: &'static str,
    category: CleanupCategory,
    safety: SafetyLevel,
    reason: &'static str,
    regenerate: &'static str,
}

const GLOBAL: &[GlobalCache] = &[
    GlobalCache {
        rel: "Library/Caches/Homebrew",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "Homebrew's download cache",
        regenerate: "Homebrew downloads again when needed",
    },
    GlobalCache {
        rel: ".npm/_cacache",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "npm's package cache",
        regenerate: "npm downloads again on the next install",
    },
    GlobalCache {
        rel: "Library/Caches/Yarn",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "Yarn's package cache",
        regenerate: "Yarn downloads again on the next install",
    },
    GlobalCache {
        rel: "Library/Caches/pip",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "pip's download and wheel cache",
        regenerate: "pip downloads again on the next install",
    },
    GlobalCache {
        rel: ".cache/uv",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "uv's package cache",
        regenerate: "uv downloads again on the next sync",
    },
    GlobalCache {
        rel: "Library/Caches/pnpm",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "pnpm's metadata cache (not the content store)",
        regenerate: "pnpm fetches metadata again",
    },
    GlobalCache {
        rel: ".cargo/registry/cache",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "downloaded crate archives",
        regenerate: "cargo downloads again on the next build",
    },
    GlobalCache {
        rel: ".cargo/registry/src",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "extracted crate sources",
        regenerate: "cargo extracts again on the next build",
    },
    GlobalCache {
        rel: ".cargo/git/checkouts",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "checkouts of Git dependencies",
        regenerate: "cargo checks out again on the next build",
    },
    GlobalCache {
        rel: ".gradle/caches",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "Gradle's dependency and build cache",
        regenerate: "Gradle downloads again on the next build",
    },
    GlobalCache {
        rel: "Library/Caches/CocoaPods",
        category: CleanupCategory::PackageCache,
        safety: SafetyLevel::Safe,
        reason: "CocoaPods' download cache",
        regenerate: "pod install downloads again",
    },
    GlobalCache {
        rel: "Library/Developer/Xcode/DerivedData",
        category: CleanupCategory::XcodeDerivedData,
        safety: SafetyLevel::Safe,
        reason: "Xcode build products and indexes",
        regenerate: "Xcode rebuilds on the next build",
    },
    GlobalCache {
        rel: "Library/Developer/CoreSimulator/Caches",
        category: CleanupCategory::DeveloperCache,
        safety: SafetyLevel::Safe,
        reason: "iOS Simulator caches",
        regenerate: "the Simulator recreates them",
    },
    GlobalCache {
        rel: "Library/Caches/ms-playwright",
        category: CleanupCategory::DeveloperCache,
        safety: SafetyLevel::Safe,
        reason: "browsers downloaded by Playwright",
        regenerate: "`npx playwright install` downloads them again",
    },
    GlobalCache {
        rel: "Library/Caches/node-gyp",
        category: CleanupCategory::DeveloperCache,
        safety: SafetyLevel::Safe,
        reason: "Node.js headers for native modules",
        regenerate: "node-gyp downloads again when building a native module",
    },
    GlobalCache {
        rel: "Library/Developer/Xcode/iOS DeviceSupport",
        category: CleanupCategory::DeveloperCache,
        safety: SafetyLevel::Caution,
        reason: "debug symbols copied from connected iOS devices",
        regenerate: "Xcode copies them again when a device is connected (slow)",
    },
    GlobalCache {
        rel: ".cache/huggingface/hub",
        category: CleanupCategory::ModelCache,
        safety: SafetyLevel::Caution,
        reason: "models and datasets downloaded from Hugging Face",
        regenerate: "downloaded again on first use (can be large)",
    },
];

fn mtime(p: &Path) -> Option<i64> {
    std::fs::symlink_metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(system_time_ms)
}

/// Find artifact directories inside a repository.
fn repo_artifacts(repo: &Path) -> Vec<(PathBuf, CleanupCategory, &'static str, &'static str)> {
    let mut out = Vec::new();
    let mut it = WalkDir::new(repo)
        .follow_links(false)
        .max_depth(5)
        .into_iter();
    while let Some(Ok(e)) = it.next() {
        if !e.file_type().is_dir() || e.depth() == 0 {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let parent = e.path().parent().unwrap_or(repo);
        let found = match name.as_str() {
            ".git" => {
                it.skip_current_dir();
                None
            }
            "node_modules" if parent.join("package.json").is_file() => Some((
                CleanupCategory::NodeModules,
                "installed npm dependencies",
                "reinstall with your package manager (npm/pnpm/yarn/bun install)",
            )),
            "target"
                if parent.join("Cargo.toml").is_file()
                    || e.path().join("CACHEDIR.TAG").is_file() =>
            {
                Some((
                    CleanupCategory::RustTarget,
                    "Rust build output",
                    "`cargo build` rebuilds it",
                ))
            }
            ".venv" | "venv" if e.path().join("pyvenv.cfg").is_file() => Some((
                CleanupCategory::PythonVenv,
                "Python virtual environment",
                "recreate it and reinstall from your requirements or lockfile",
            )),
            ".next" | ".nuxt" | ".turbo" | ".parcel-cache" | ".svelte-kit" | ".angular"
            | ".gradle" | "DerivedData" => Some((
                CleanupCategory::BuildOutput,
                "framework build cache",
                "rebuilt on the next build",
            )),
            "dist" | "build" | "out" => Some((
                CleanupCategory::BuildOutput,
                "build output",
                "rebuilt on the next build",
            )),
            _ => None,
        };
        if let Some((cat, reason, regen)) = found {
            out.push((e.path().to_path_buf(), cat, reason, regen));
            it.skip_current_dir();
        } else if name == "node_modules" || name.starts_with('.') && name != ".github" {
            it.skip_current_dir();
        }
    }
    out
}

pub fn analyze(input: &AnalysisInput, runner: &dyn CommandRunner) -> DiskAnalysis {
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let repo_paths: Vec<PathBuf> = input.repos.iter().map(|(p, _)| p.clone()).collect();
    let roots: Vec<PathBuf> = input
        .scan_roots
        .iter()
        .map(|r| std::fs::canonicalize(r).unwrap_or_else(|_| r.clone()))
        .collect();

    for (repo, dupes) in &input.repos {
        if !repo.is_dir() {
            continue;
        }
        let repo_name = repo.file_name().map(|n| n.to_string_lossy().to_string());
        for (path, category, reason, regen) in repo_artifacts(repo) {
            let (bytes, complete) = dir_size(&path, MAX_ENTRIES);
            let mut blocked = safety::check_path(&path, &input.home, &roots, &repo_paths).err();
            if blocked.is_none() {
                blocked = safety::check_untracked(runner, repo, &path).err();
            }
            candidates.push(CleanupCandidate {
                id: path.display().to_string(),
                category,
                path: path.display().to_string(),
                size_bytes: if complete {
                    Availability::available(bytes)
                } else {
                    Availability::unavailable(format!("more than {MAX_ENTRIES} entries"))
                },
                last_modified: mtime(&path),
                safety: SafetyLevel::Safe,
                reason: format!(
                    "{reason} in {}",
                    repo_name.as_deref().unwrap_or("a project")
                ),
                regenerate: Some(regen.to_string()),
                project: Some(repo.display().to_string()),
                eligible: blocked.is_none() && complete,
                blocked_by: blocked
                    .or_else(|| (!complete).then(|| "too large to measure reliably".to_string())),
            });
        }
        if *dupes > 1 {
            let (bytes, complete) = dir_size(repo, MAX_ENTRIES);
            candidates.push(CleanupCandidate {
                id: format!("duplicate:{}", repo.display()),
                category: CleanupCategory::DuplicateClone,
                path: repo.display().to_string(),
                size_bytes: if complete { Availability::available(bytes) } else { Availability::unavailable("too large to measure") },
                last_modified: mtime(repo),
                safety: SafetyLevel::ReviewOnly,
                reason: format!("one of {dupes} local clones of the same project"),
                regenerate: None,
                project: Some(repo.display().to_string()),
                eligible: false,
                blocked_by: Some("repositories are never removed by AGASTHY; review and remove it yourself if it is unused".into()),
            });
        }
    }

    for g in GLOBAL {
        let p = input.home.join(g.rel);
        if !p.is_dir() {
            continue;
        }
        let (bytes, complete) = dir_size(&p, MAX_ENTRIES);
        if complete && bytes == 0 {
            continue;
        }
        let blocked = safety::check_path(&p, &input.home, &roots, &repo_paths).err();
        candidates.push(CleanupCandidate {
            id: p.display().to_string(),
            category: g.category,
            path: p.display().to_string(),
            size_bytes: if complete {
                Availability::available(bytes)
            } else {
                Availability::unavailable(format!("more than {MAX_ENTRIES} entries"))
            },
            last_modified: mtime(&p),
            safety: g.safety,
            reason: g.reason.to_string(),
            regenerate: Some(g.regenerate.to_string()),
            project: None,
            eligible: blocked.is_none() && complete,
            blocked_by: blocked
                .or_else(|| (!complete).then(|| "too large to measure reliably".to_string())),
        });
    }

    let docker = docker::usage(runner);
    if let Availability::Unavailable { reason } = &docker {
        issues.push(ScanIssue::new(
            IssueKind::Unsupported,
            "docker",
            reason.clone(),
        ));
    }

    candidates.sort_by(|a, b| {
        let sa = a.size_bytes.value().copied().unwrap_or(0);
        let sb = b.size_bytes.value().copied().unwrap_or(0);
        sb.cmp(&sa)
    });
    let mut totals: Vec<CategoryTotal> = Vec::new();
    for c in &candidates {
        let bytes = c.size_bytes.value().copied().unwrap_or(0);
        match totals.iter_mut().find(|t| t.category == c.category) {
            Some(t) => {
                t.count += 1;
                t.bytes += bytes;
                if c.eligible {
                    t.eligible_bytes += bytes;
                }
            }
            None => totals.push(CategoryTotal {
                category: c.category,
                count: 1,
                bytes,
                eligible_bytes: if c.eligible { bytes } else { 0 },
            }),
        }
    }
    totals.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    let eligible_bytes = totals.iter().map(|t| t.eligible_bytes).sum();
    DiskAnalysis {
        analysed_at: agasthy_core::time::now_ms(),
        candidates,
        totals,
        eligible_bytes,
        docker,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agasthy_platform::{CommandOutput, MockCommandRunner};

    fn mk(p: &Path, files: &[(&str, usize)]) {
        for (f, n) in files {
            let full = p.join(f);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, vec![b'x'; *n]).unwrap();
        }
    }

    #[test]
    fn finds_artifacts_and_applies_git_checks() {
        let d = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(d.path()).unwrap();
        let home = root.join("home");
        let code = root.join("code");
        let repo = code.join("web");
        mk(
            &repo,
            &[
                ("package.json", 2),
                ("node_modules/a/index.js", 1000),
                (".git/HEAD", 5),
                ("src/app.ts", 10),
                ("dist/app.js", 300),
            ],
        );
        mk(
            &home,
            &[
                (".npm/_cacache/x", 50),
                ("Library/Developer/Xcode/DerivedData/P/x", 70),
            ],
        );
        let m = MockCommandRunner::new();
        let r = repo.display().to_string();
        let git = |args: &[&str]| {
            let mut v = vec!["-C", r.as_str(), "-c", "core.fsmonitor=false"];
            v.extend_from_slice(args);
            v.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        };
        fn strs(v: &[String]) -> Vec<&str> {
            v.iter().map(String::as_str).collect()
        }
        // node_modules: ignored, nothing tracked → eligible
        m.on(
            "git",
            &strs(&git(&["ls-files", "-z", "--", "node_modules"])),
            Ok(CommandOutput::ok("")),
        );
        m.on(
            "git",
            &strs(&git(&["check-ignore", "-q", "--", "node_modules"])),
            Ok(CommandOutput::ok("")),
        );
        // dist: tracked file inside → blocked
        m.on(
            "git",
            &strs(&git(&["ls-files", "-z", "--", "dist"])),
            Ok(CommandOutput::ok("dist/app.js\0")),
        );

        let a = analyze(
            &AnalysisInput {
                home: home.clone(),
                scan_roots: vec![code.clone()],
                repos: vec![(repo.clone(), 2)],
            },
            &m,
        );
        let find = |suffix: &str| {
            a.candidates
                .iter()
                .find(|c| c.path.ends_with(suffix))
                .unwrap_or_else(|| panic!("{suffix}"))
        };
        let nm = find("web/node_modules");
        assert!(nm.eligible, "{:?}", nm.blocked_by);
        assert_eq!(nm.size_bytes.value(), Some(&1000));
        let dist = find("web/dist");
        assert!(!dist.eligible);
        assert!(dist.blocked_by.as_ref().unwrap().contains("tracked"));
        assert!(find(".npm/_cacache").eligible);
        assert!(find("DerivedData").eligible);
        let dup = a
            .candidates
            .iter()
            .find(|c| c.category == CleanupCategory::DuplicateClone)
            .unwrap();
        assert_eq!(dup.safety, SafetyLevel::ReviewOnly);
        assert!(!dup.eligible, "repositories are never eligible");
        assert!(
            !a.candidates.iter().any(|c| c.path.ends_with("src")),
            "source is never a candidate"
        );
        assert_eq!(a.eligible_bytes, 1000 + 50 + 70);
        assert!(
            matches!(a.docker, Availability::Unavailable { .. }),
            "docker absent in mock"
        );
    }

    #[test]
    fn without_git_project_artifacts_are_not_eligible() {
        let d = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(d.path()).unwrap();
        let repo = root.join("code/app");
        mk(
            &repo,
            &[("Cargo.toml", 1), ("target/debug/x", 10), (".git/HEAD", 1)],
        );
        let a = analyze(
            &AnalysisInput {
                home: root.join("home"),
                scan_roots: vec![root.join("code")],
                repos: vec![(repo, 1)],
            },
            &MockCommandRunner::new(),
        );
        let t = &a.candidates[0];
        assert_eq!(t.category, CleanupCategory::RustTarget);
        assert!(!t.eligible);
        assert!(t.blocked_by.as_ref().unwrap().contains("cannot verify"));
    }
}
