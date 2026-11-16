//! Windows application inventory from the registry's Uninstall keys, read with
//! `reg.exe query` (read-only) through a runner that can start nothing else.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use crate::command::{CommandRunner, CommandSpec, SystemCommandRunner};
use crate::types::{AppLocation, InstalledApp};

pub const UNINSTALL_ROOTS: &[(&str, AppLocation)] = &[
    (
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        AppLocation::Machine,
    ),
    (
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        AppLocation::Machine,
    ),
    (
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        AppLocation::User,
    ),
];

/// A runner limited to `reg` in the Windows system directory.
pub fn registry_runner() -> SystemCommandRunner {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    SystemCommandRunner::new(&["reg"], vec![root.join("System32")], vec![".exe".into()])
}

pub fn query_spec(root: &str) -> CommandSpec {
    CommandSpec::new("reg", &["query", root, "/s"])
        .timeout(Duration::from_secs(30))
        .max_output(32 * 1024 * 1024)
}

/// Read every root. Returns apps and the roots that could not be read.
pub fn read_all(runner: &dyn CommandRunner) -> (Vec<InstalledApp>, Vec<String>) {
    let mut apps = Vec::new();
    let mut failed = Vec::new();
    for (root, loc) in UNINSTALL_ROOTS {
        match runner.run(&query_spec(root)) {
            Ok(o) if o.success() => apps.extend(parse_reg_uninstall(&o.stdout, *loc)),
            // A missing key (e.g. no WOW6432Node on 32-bit Windows) exits 1 with no output.
            Ok(o) if o.stdout.trim().is_empty() && o.exit_code == Some(1) => {}
            Ok(o) => failed.push(format!(
                "{root}: {}",
                o.stderr.lines().next().unwrap_or("reg query failed")
            )),
            Err(e) => failed.push(format!("{root}: {e}")),
        }
    }
    let mut seen = HashSet::new();
    apps.retain(|a| seen.insert((a.name.to_lowercase(), a.version.clone())));
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    (apps, failed)
}

/// Parse `reg query <Uninstall key> /s` output.
///
/// ```text
/// HKEY_LOCAL_MACHINE\SOFTWARE\...\Uninstall\Git_is1
///     DisplayName    REG_SZ    Git
///     DisplayVersion    REG_SZ    2.45.1
///     InstallLocation    REG_SZ    C:\Program Files\Git\
/// ```
pub fn parse_reg_uninstall(out: &str, location: AppLocation) -> Vec<InstalledApp> {
    let mut apps = Vec::new();
    let mut key: Option<String> = None;
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    let mut flush = |key: &Option<String>, values: &mut BTreeMap<String, String>| {
        if let Some(k) = key
            && let Some(app) = to_app(k, values, location)
        {
            apps.push(app);
        }
        values.clear();
    };
    for raw in out.lines() {
        let line = raw.trim_end_matches('\r');
        if line.starts_with("HKEY_") {
            flush(&key, &mut values);
            key = Some(line.trim().to_string());
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || key.is_none() {
            continue;
        }
        // name    REG_TYPE    data   (columns separated by 4 spaces)
        let Some(idx) = trimmed.find("    REG_") else {
            continue;
        };
        let name = trimmed[..idx].trim().to_string();
        let rest = &trimmed[idx + 4..];
        let (_ty, data) = match rest.find("    ") {
            Some(i) => (&rest[..i], rest[i..].trim()),
            None => (rest.trim(), ""),
        };
        values.insert(name, data.to_string());
    }
    flush(&key, &mut values);
    apps
}

fn to_app(key: &str, v: &BTreeMap<String, String>, location: AppLocation) -> Option<InstalledApp> {
    let name = v
        .get("DisplayName")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())?;
    // Components and updates are not user-facing applications.
    if v.get("SystemComponent").is_some_and(|s| s == "0x1") || v.contains_key("ParentKeyName") {
        return None;
    }
    if v.get("ReleaseType").is_some_and(|r| {
        let r = r.to_lowercase();
        r.contains("update") || r.contains("hotfix")
    }) {
        return None;
    }
    let install = v
        .get("InstallLocation")
        .map(|s| {
            s.trim()
                .trim_end_matches('\\')
                .trim_matches('"')
                .to_string()
        })
        .filter(|s| !s.is_empty());
    let icon_dir = v.get("DisplayIcon").and_then(|s| {
        let p = s.trim().trim_matches('"');
        let p = p
            .rsplit_once(',')
            .map(|(a, _)| a)
            .unwrap_or(p)
            .trim_matches('"');
        p.rsplit_once('\\').map(|(dir, _)| dir.to_string())
    });
    let location = if key.starts_with("HKEY_CURRENT_USER") {
        AppLocation::User
    } else {
        location
    };
    Some(InstalledApp {
        name: name.to_string(),
        // The key is unique; the folder is used when the installer records it.
        path: install.or(icon_dir).unwrap_or_else(|| key.to_string()),
        bundle_id: key.rsplit('\\').next().map(str::to_string),
        version: v
            .get("DisplayVersion")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        build: None,
        location,
        metadata_source: key.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, MockCommandRunner};

    const SAMPLE: &str = "\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Git_is1\r
    DisplayName    REG_SZ    Git\r
    DisplayVersion    REG_SZ    2.45.1\r
    Publisher    REG_SZ    The Git Development Community\r
    InstallLocation    REG_SZ    C:\\Program Files\\Git\\\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{ABC-123}\r
    DisplayName    REG_SZ    Microsoft Visual C++ Runtime\r
    SystemComponent    REG_DWORD    0x1\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\KB5005\r
    DisplayName    REG_SZ    Security Update\r
    ParentKeyName    REG_SZ    OperatingSystem\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Code\r
    DisplayName    REG_SZ    Microsoft Visual Studio Code\r
    DisplayIcon    REG_SZ    \"C:\\Users\\dev\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe\",0\r
    DisplayVersion    REG_SZ    1.90.0\r
\r
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\NoName\r
    InstallLocation    REG_SZ    C:\\x\r
";

    #[test]
    fn parses_uninstall_entries_and_skips_components() {
        let apps = parse_reg_uninstall(SAMPLE, AppLocation::Machine);
        assert_eq!(apps.len(), 2, "{apps:?}");
        let git = &apps[0];
        assert_eq!(git.name, "Git");
        assert_eq!(git.version.as_deref(), Some("2.45.1"));
        assert_eq!(git.path, r"C:\Program Files\Git");
        assert_eq!(git.bundle_id.as_deref(), Some("Git_is1"));
        let code = &apps[1];
        assert_eq!(
            code.path, r"C:\Users\dev\AppData\Local\Programs\Microsoft VS Code",
            "falls back to the icon's folder"
        );
    }

    #[test]
    fn reads_every_root_and_reports_failures() {
        let r = MockCommandRunner::new();
        let [(m, _), (wow, _), (cu, _)] = UNINSTALL_ROOTS else {
            unreachable!()
        };
        r.on("reg", &["query", m, "/s"], Ok(CommandOutput::ok(SAMPLE)));
        r.on(
            "reg",
            &["query", wow, "/s"],
            Ok(CommandOutput::failed(1, "")),
        );
        r.on(
            "reg",
            &["query", cu, "/s"],
            Ok(CommandOutput::failed(5, "ERROR: Access is denied.")),
        );
        let (apps, failed) = read_all(&r);
        assert_eq!(apps.len(), 2);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("Access is denied"));
    }
}
