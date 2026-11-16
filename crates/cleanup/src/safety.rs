//! Checks that run before any path is moved. They are deliberately strict:
//! when something cannot be verified, the answer is "no".

use std::path::{Path, PathBuf};
use std::time::Duration;

use agasthy_platform::{CommandRunner, CommandSpec};

/// Locations under the home directory that hold only regenerable caches.
/// Global cache candidates must be exactly one of these (or inside one).
pub const CACHE_ROOTS: &[&str] = &[
    "Library/Caches/Homebrew",
    ".npm/_cacache",
    "Library/Caches/Yarn",
    "Library/Caches/pip",
    ".cache/uv",
    "Library/Caches/pnpm",
    ".cargo/registry/cache",
    ".cargo/registry/src",
    ".cargo/git/checkouts",
    ".gradle/caches",
    "Library/Caches/CocoaPods",
    "Library/Developer/Xcode/DerivedData",
    "Library/Developer/CoreSimulator/Caches",
    "Library/Caches/ms-playwright",
    "Library/Caches/node-gyp",
    ".cache/huggingface/hub",
    "Library/Developer/Xcode/iOS DeviceSupport",
];

/// Why a path may not be touched, or `None` if it passes every check.
///
/// `allowed_roots`: user-approved scan folders (for project artifacts).
/// `repos`: known repository roots; none may be removed or contain the candidate's removal.
pub fn check_path(
    path: &Path,
    home: &Path,
    allowed_roots: &[PathBuf],
    repos: &[PathBuf],
) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("path is not absolute".into());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("cannot read: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err("is a symbolic link; AGASTHY does not follow or remove links".into());
    }
    if !meta.is_dir() {
        return Err("is not a directory".into());
    }
    let canonical = agasthy_core::paths::canonical(path).map_err(|e| e.to_string())?;
    if canonical != path {
        return Err("path resolves elsewhere (a parent is a symbolic link)".into());
    }
    if canonical.components().any(|c| c.as_os_str() == ".git") {
        return Err("is inside a Git directory".into());
    }
    if canonical.join(".git").exists() {
        return Err("is a Git repository".into());
    }
    for r in repos {
        if r.starts_with(&canonical) {
            return Err(format!("contains the repository {}", r.display()));
        }
    }
    let home = agasthy_core::paths::canonical(home).unwrap_or_else(|_| home.to_path_buf());
    if canonical == home || home.starts_with(&canonical) {
        return Err("is your home folder or one of its parents".into());
    }
    let in_cache = CACHE_ROOTS.iter().any(|c| {
        let root = home.join(c);
        canonical == root || canonical.starts_with(&root)
    });
    let in_scan_root = allowed_roots
        .iter()
        .any(|r| canonical.starts_with(r) && canonical != *r);
    if !in_cache && !in_scan_root {
        return Err("is outside your project folders and AGASTHY's list of cache locations".into());
    }
    Ok(canonical)
}

/// For an artifact inside a repository: it must be ignored by Git and contain
/// no tracked files. Without Git the answer is "cannot verify".
pub fn check_untracked(runner: &dyn CommandRunner, repo: &Path, path: &Path) -> Result<(), String> {
    let rel = path
        .strip_prefix(repo)
        .map_err(|_| "not inside the repository".to_string())?
        .to_string_lossy()
        .to_string();
    let git = |args: &[&str]| {
        let mut all = vec![
            "-C",
            repo.to_str().unwrap_or("."),
            "-c",
            "core.fsmonitor=false",
        ];
        all.extend_from_slice(args);
        runner.run(
            &CommandSpec::new("git", &all)
                .timeout(Duration::from_secs(20))
                .env("GIT_OPTIONAL_LOCKS", "0"),
        )
    };
    let tracked =
        git(&["ls-files", "-z", "--", &rel]).map_err(|e| format!("cannot verify with git: {e}"))?;
    if !tracked.success() {
        return Err(format!(
            "cannot verify with git (exit {:?})",
            tracked.exit_code
        ));
    }
    if !tracked.stdout.trim_matches('\0').trim().is_empty() {
        return Err("contains files tracked by Git".into());
    }
    let ignored = git(&["check-ignore", "-q", "--", &rel])
        .map_err(|e| format!("cannot verify with git: {e}"))?;
    match ignored.exit_code {
        Some(0) => Ok(()),
        Some(1) => Err("is not ignored by Git (it may hold work you have not committed)".into()),
        other => Err(format!("cannot verify with git (exit {other:?})")),
    }
}

/// Device id, to keep moves on one volume (a rename, never a copy).
#[cfg(unix)]
pub fn device_of(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
pub fn device_of(p: &Path) -> Option<u64> {
    // Same drive letter is the best cheap approximation on Windows.
    p.components().next().map(|c| {
        let s = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        s.bytes()
            .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rules() {
        let d = tempfile::tempdir().unwrap();
        let root = agasthy_core::paths::canonical(d.path()).unwrap();
        let home = root.join("home");
        let code = root.join("code");
        let repo = code.join("app");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(home.join(".npm/_cacache/x")).unwrap();
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        let roots = vec![code.clone()];
        let repos = vec![repo.clone()];

        assert!(check_path(&repo.join("node_modules"), &home, &roots, &repos).is_ok());
        assert!(check_path(&home.join(".npm/_cacache"), &home, &roots, &repos).is_ok());
        assert!(
            check_path(&repo, &home, &roots, &repos)
                .unwrap_err()
                .contains("Git repository")
        );
        assert!(
            check_path(&code, &home, &roots, &repos).is_err(),
            "a scan root itself is never removed"
        );
        assert!(check_path(&repo.join(".git"), &home, &roots, &repos).is_err());
        assert!(
            check_path(&home.join("Documents"), &home, &roots, &repos)
                .unwrap_err()
                .contains("outside")
        );
        assert!(check_path(&home, &home, &roots, &repos).is_err());
        assert!(check_path(Path::new("relative"), &home, &roots, &repos).is_err());
        assert!(check_path(&repo.join("missing"), &home, &roots, &repos).is_err());
        // traversal: resolves somewhere else
        let sneaky = repo.join("node_modules/../../../home/Documents");
        assert!(check_path(&sneaky, &home, &roots, &repos).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.join("Documents"), repo.join("link")).unwrap();
            assert!(
                check_path(&repo.join("link"), &home, &roots, &repos)
                    .unwrap_err()
                    .contains("symbolic link")
            );
        }
    }
}
