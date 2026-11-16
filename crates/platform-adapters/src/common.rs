//! Helpers shared by several platform adapters.

use std::path::{Path, PathBuf};

use crate::types::{SearchDir, SearchDirOrigin};

/// Directories from this process's `PATH`, de-duplicated, order preserved.
pub(crate) fn process_path_dirs() -> Vec<SearchDir> {
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&path) {
            push_unique(&mut out, p, SearchDirOrigin::ProcessPath);
        }
    }
    out
}

pub(crate) fn push_unique(out: &mut Vec<SearchDir>, path: PathBuf, origin: SearchDirOrigin) {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return;
    }
    if out.iter().any(|d| d.path == path) {
        return;
    }
    out.push(SearchDir { path, origin });
}

/// Add conventional per-user tool directories that exist on disk.
pub(crate) fn push_well_known_user_dirs(out: &mut Vec<SearchDir>, home: Option<&Path>) {
    let Some(home) = home else { return };
    for rel in [
        ".cargo/bin",
        ".local/bin",
        ".bun/bin",
        ".deno/bin",
        "go/bin",
        ".volta/bin",
        ".pyenv/shims",
        ".rbenv/shims",
        ".asdf/shims",
        ".local/share/mise/shims",
    ] {
        let p = home.join(rel);
        if p.is_dir() {
            push_unique(out, p, SearchDirOrigin::WellKnown);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_unique_ignores_relative_and_duplicates() {
        let mut v = Vec::new();
        push_unique(
            &mut v,
            PathBuf::from("relative/bin"),
            SearchDirOrigin::ProcessPath,
        );
        push_unique(&mut v, PathBuf::from(""), SearchDirOrigin::ProcessPath);
        let abs = std::env::temp_dir();
        push_unique(&mut v, abs.clone(), SearchDirOrigin::ProcessPath);
        push_unique(&mut v, abs, SearchDirOrigin::WellKnown);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].origin, SearchDirOrigin::ProcessPath);
    }
}
