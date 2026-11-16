//! Path helpers that behave the same on every OS.

use std::path::{Path, PathBuf};

/// `std::fs::canonicalize`, but without Windows' verbatim prefix.
///
/// On Windows, canonicalising returns `\\?\C:\…` (or `\\?\UNC\server\share\…`).
/// Many programs, Git among them, cannot use such paths, and they never compare
/// equal to the ordinary form of the same path. This returns `C:\…` /
/// `\\server\share\…` instead. On other systems it is plain `canonicalize`.
pub fn canonical(p: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(p).map(strip_verbatim)
}

/// Remove a `\\?\` verbatim prefix from a Windows path, when the result is
/// still an ordinary absolute path. Other paths are returned unchanged.
pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    let Some(s) = p.to_str() else { return p };
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        // Only drive paths such as `C:\…`; leave device paths alone.
        if b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/')
        {
            return PathBuf::from(rest);
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_removed() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\Users\dev\code")),
            PathBuf::from(r"C:\Users\dev\code")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\server\share\x")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\GLOBALROOT\Device\X")),
            PathBuf::from(r"\\?\GLOBALROOT\Device\X")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from("/Users/dev/code")),
            PathBuf::from("/Users/dev/code")
        );
    }

    #[test]
    fn canonical_resolves_like_std() {
        let d = std::env::temp_dir();
        assert!(canonical(&d).unwrap().is_absolute());
        assert!(
            !canonical(&d)
                .unwrap()
                .to_string_lossy()
                .starts_with(r"\\?\")
        );
    }
}
