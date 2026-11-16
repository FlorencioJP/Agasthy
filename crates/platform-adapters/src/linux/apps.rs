//! Linux application inventory from freedesktop `.desktop` entries
//! (distribution packages, Flatpak and Snap exports, per-user entries).
//! Files are read, never executed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::types::{AppLocation, InstalledApp};

pub fn entry_dirs(home: Option<&Path>) -> Vec<(PathBuf, AppLocation)> {
    let mut v = vec![
        (
            PathBuf::from("/usr/share/applications"),
            AppLocation::Machine,
        ),
        (
            PathBuf::from("/usr/local/share/applications"),
            AppLocation::Machine,
        ),
        (
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            AppLocation::Machine,
        ),
        (
            PathBuf::from("/var/lib/snapd/desktop/applications"),
            AppLocation::Machine,
        ),
    ];
    if let Some(h) = home {
        v.push((h.join(".local/share/applications"), AppLocation::User));
        v.push((
            h.join(".local/share/flatpak/exports/share/applications"),
            AppLocation::User,
        ));
    }
    v
}

/// Parse the `[Desktop Entry]` group of one file. Returns `None` for entries
/// that are hidden, not applications, or have no name.
pub fn parse_desktop_entry(text: &str, file: &Path, location: AppLocation) -> Option<InstalledApp> {
    let mut in_main = false;
    let (mut name, mut kind, mut hidden, mut version, mut flatpak, mut snap) =
        (None, None, false, None, None, None);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            // Unlocalized name only (`Name[de]=` is skipped by the exact match).
            "Name" => name = Some(v.to_string()),
            "Type" => kind = Some(v.to_string()),
            "NoDisplay" | "Hidden" if v.eq_ignore_ascii_case("true") => hidden = true,
            "X-AppImage-Version" => version = Some(v.to_string()),
            "X-Flatpak" => flatpak = Some(v.to_string()),
            "X-SnapInstanceName" => snap = Some(v.to_string()),
            _ => {}
        }
    }
    if hidden || kind.as_deref() != Some("Application") {
        return None;
    }
    let name = name.filter(|n| !n.is_empty())?;
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned());
    Some(InstalledApp {
        name,
        path: file.display().to_string(),
        bundle_id: flatpak.or(snap).or(stem),
        version,
        build: None,
        location,
        metadata_source: if file.to_string_lossy().contains("/flatpak/") {
            "Flatpak export (.desktop)".into()
        } else if file.to_string_lossy().contains("/snapd/") {
            "Snap export (.desktop)".into()
        } else {
            ".desktop entry".into()
        },
    })
}

/// Read every entry directory. Later (user) entries with the same desktop id
/// override system ones, as the freedesktop spec says.
pub fn read_all(dirs: &[(PathBuf, AppLocation)]) -> (Vec<InstalledApp>, Vec<String>) {
    let mut by_id: Vec<(String, InstalledApp)> = Vec::new();
    let mut issues = Vec::new();
    for (dir, loc) in dirs {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                issues.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            // Small files only; never follow into anything else.
            if e.metadata().map(|m| m.len() > 256 * 1024).unwrap_or(true) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            if let Some(app) = parse_desktop_entry(&text, &p, *loc) {
                let id = e.file_name().to_string_lossy().into_owned();
                by_id.retain(|(i, _)| *i != id);
                by_id.push((id, app));
            }
        }
    }
    let mut seen = HashSet::new();
    let mut apps: Vec<InstalledApp> = by_id
        .into_iter()
        .map(|(_, a)| a)
        .filter(|a| seen.insert(a.path.clone()))
        .collect();
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    (apps, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_skips_hidden() {
        let code = "[Desktop Entry]\nName=Visual Studio Code\nName[de]=Code\nType=Application\nExec=/usr/share/code/code %F\n\n[Desktop Action new-window]\nName=New Window\n";
        let a = parse_desktop_entry(
            code,
            Path::new("/usr/share/applications/code.desktop"),
            AppLocation::Machine,
        )
        .unwrap();
        assert_eq!(a.name, "Visual Studio Code");
        assert_eq!(a.bundle_id.as_deref(), Some("code"));
        assert!(
            parse_desktop_entry(
                "[Desktop Entry]\nName=x\nType=Application\nNoDisplay=true\n",
                Path::new("/a/x.desktop"),
                AppLocation::Machine
            )
            .is_none()
        );
        assert!(
            parse_desktop_entry(
                "[Desktop Entry]\nName=x\nType=Link\n",
                Path::new("/a/x.desktop"),
                AppLocation::Machine
            )
            .is_none()
        );
        let fp = parse_desktop_entry(
            "[Desktop Entry]\nName=Zed\nType=Application\nX-Flatpak=dev.zed.Zed\n",
            Path::new("/var/lib/flatpak/exports/share/applications/dev.zed.Zed.desktop"),
            AppLocation::Machine,
        )
        .unwrap();
        assert_eq!(fp.bundle_id.as_deref(), Some("dev.zed.Zed"));
        assert!(fp.metadata_source.contains("Flatpak"));
    }

    #[test]
    fn user_entries_override_system_ones() {
        let sys = tempfile::tempdir().unwrap();
        let usr = tempfile::tempdir().unwrap();
        std::fs::write(
            sys.path().join("a.desktop"),
            "[Desktop Entry]\nName=System A\nType=Application\n",
        )
        .unwrap();
        std::fs::write(
            sys.path().join("b.desktop"),
            "[Desktop Entry]\nName=B\nType=Application\n",
        )
        .unwrap();
        std::fs::write(sys.path().join("notes.txt"), "x").unwrap();
        std::fs::write(
            usr.path().join("a.desktop"),
            "[Desktop Entry]\nName=User A\nType=Application\n",
        )
        .unwrap();
        let dirs = vec![
            (sys.path().to_path_buf(), AppLocation::Machine),
            (usr.path().to_path_buf(), AppLocation::User),
            (PathBuf::from("/definitely/missing"), AppLocation::Machine),
        ];
        let (apps, issues) = read_all(&dirs);
        assert!(issues.is_empty());
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["B", "User A"]);
    }
}
