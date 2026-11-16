//! Validation of user-approved scan roots.

use std::path::{Path, PathBuf};

use agasthy_platform::SystemPaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    NotAbsolute,
    DoesNotExist,
    NotADirectory,
    PermissionDenied,
    Protected(String),
    Io(String),
}

impl std::fmt::Display for RootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RootError::NotAbsolute => write!(f, "the path must be absolute"),
            RootError::DoesNotExist => write!(f, "the folder does not exist"),
            RootError::NotADirectory => write!(f, "the path is not a folder"),
            RootError::PermissionDenied => write!(f, "permission to read this folder was denied"),
            RootError::Protected(p) => write!(
                f,
                "{p} is a system location or too broad to scan; choose a folder that contains your projects"
            ),
            RootError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RootError {}

/// Validate a folder the user selected and return its canonical form.
///
/// Canonicalisation resolves symlinks once, here, so a root cannot silently
/// point somewhere else later. The walker itself never follows symlinks.
pub fn validate_scan_root(paths: &dyn SystemPaths, raw: &str) -> Result<PathBuf, RootError> {
    let p = Path::new(raw.trim());
    if !p.is_absolute() {
        return Err(RootError::NotAbsolute);
    }
    let canonical = match std::fs::canonicalize(p) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(RootError::DoesNotExist),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(RootError::PermissionDenied);
        }
        Err(e) => return Err(RootError::Io(e.to_string())),
    };
    if !canonical.is_dir() {
        return Err(RootError::NotADirectory);
    }
    if paths.is_protected_scan_root(&canonical) || paths.is_protected_scan_root(p) {
        return Err(RootError::Protected(canonical.display().to_string()));
    }
    if let Err(e) = std::fs::read_dir(&canonical) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return Err(RootError::PermissionDenied);
        }
        return Err(RootError::Io(e.to_string()));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPaths;
    impl SystemPaths for TestPaths {
        fn executable_search_dirs(&self) -> Vec<agasthy_platform::SearchDir> {
            vec![]
        }
        fn executable_file_names(&self, tool: &str) -> Vec<String> {
            vec![tool.into()]
        }
        fn is_protected_scan_root(&self, path: &Path) -> bool {
            path == Path::new("/")
        }
    }

    #[test]
    fn validates_roots() {
        let dir = tempfile::tempdir().unwrap();
        let ok = validate_scan_root(&TestPaths, dir.path().to_str().unwrap()).unwrap();
        assert!(ok.is_absolute());
        assert_eq!(
            validate_scan_root(&TestPaths, "relative/dir"),
            Err(RootError::NotAbsolute)
        );
        let missing = dir.path().join("nope");
        assert_eq!(
            validate_scan_root(&TestPaths, missing.to_str().unwrap()),
            Err(RootError::DoesNotExist)
        );
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            validate_scan_root(&TestPaths, file.to_str().unwrap()),
            Err(RootError::NotADirectory)
        );
        #[cfg(unix)]
        assert!(matches!(
            validate_scan_root(&TestPaths, "/"),
            Err(RootError::Protected(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn traversal_segments_are_resolved_before_checks() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("a");
        std::fs::create_dir(&sub).unwrap();
        // "/tmp/x/a/../../../../.." canonicalises to "/" and must be refused.
        let sneaky = format!("{}/a/../../../../../../../../..", dir.path().display());
        assert!(matches!(
            validate_scan_root(&TestPaths, &sneaky),
            Err(RootError::Protected(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_root_resolves_to_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let v = validate_scan_root(&TestPaths, link.to_str().unwrap()).unwrap();
        assert_eq!(v, std::fs::canonicalize(&target).unwrap());
    }
}
