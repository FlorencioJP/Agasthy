//! macOS adapter.

use std::path::{Path, PathBuf};
use std::time::Duration;

use agasthy_core::{Availability, IssueKind, ScanIssue};

use crate::command::{CommandRunner, CommandSpec};
use crate::common::{process_path_dirs, push_unique, push_well_known_user_dirs};
use crate::types::*;
use crate::{AppInventory, AppScan, Platform, ProbePolicy, SystemPaths};

pub struct MacOs {
    home: Option<PathBuf>,
}

impl MacOs {
    pub fn new() -> Self {
        Self {
            home: dirs::home_dir(),
        }
    }

    fn app_dirs(&self) -> Vec<(PathBuf, AppLocation)> {
        let mut v = vec![
            (PathBuf::from("/Applications"), AppLocation::Machine),
            (
                PathBuf::from("/Applications/Utilities"),
                AppLocation::Machine,
            ),
            (PathBuf::from("/System/Applications"), AppLocation::System),
            (
                PathBuf::from("/System/Applications/Utilities"),
                AppLocation::System,
            ),
        ];
        if let Some(h) = &self.home {
            v.push((h.join("Applications"), AppLocation::User));
        }
        v
    }
}

impl Default for MacOs {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemPaths for MacOs {
    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    /// Apps launched from Finder get a minimal `PATH`
    /// (`/usr/bin:/bin:/usr/sbin:/sbin`). To see what a terminal sees, we also
    /// read the files macOS's `path_helper` uses, then conventional locations.
    /// Each directory records where it came from.
    fn executable_search_dirs(&self) -> Vec<SearchDir> {
        let mut out = process_path_dirs();
        for line in read_lines(Path::new("/etc/paths")) {
            push_unique(
                &mut out,
                PathBuf::from(line),
                SearchDirOrigin::SystemPathConfig,
            );
        }
        if let Ok(rd) = std::fs::read_dir("/etc/paths.d") {
            let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            for f in files {
                for line in read_lines(&f) {
                    push_unique(
                        &mut out,
                        PathBuf::from(line),
                        SearchDirOrigin::SystemPathConfig,
                    );
                }
            }
        }
        for p in [
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
        ] {
            let p = PathBuf::from(p);
            if p.is_dir() {
                push_unique(&mut out, p, SearchDirOrigin::WellKnown);
            }
        }
        push_well_known_user_dirs(&mut out, self.home.as_deref());
        out
    }

    fn executable_file_names(&self, tool: &str) -> Vec<String> {
        vec![tool.to_string()]
    }

    fn is_protected_scan_root(&self, path: &Path) -> bool {
        const PROTECTED: &[&str] = &[
            "/",
            "/System",
            "/Library",
            "/usr",
            "/bin",
            "/sbin",
            "/private",
            "/dev",
            "/etc",
            "/var",
            "/tmp",
            "/cores",
            "/opt",
            "/Volumes",
            "/Applications",
        ];
        let s = path.to_string_lossy();
        let s = s.trim_end_matches('/');
        let s = if s.is_empty() { "/" } else { s };
        PROTECTED.contains(&s)
            || s.starts_with("/System/")
            || s.starts_with("/private/")
            || s.starts_with("/usr/")
            || self
                .home
                .as_ref()
                .map(|h| path == h.join("Library") || path.starts_with(h.join("Library")))
                .unwrap_or(false)
    }
}

impl AppInventory for MacOs {
    fn list_applications(&self) -> AppScan {
        let mut apps = Vec::new();
        let mut issues = Vec::new();
        for (dir, location) in self.app_dirs() {
            collect_apps(&dir, location, 0, &mut apps, &mut issues);
        }
        apps.sort_by_cached_key(|a| a.name.to_lowercase());
        apps.dedup_by(|a, b| a.path == b.path);
        AppScan {
            apps: Availability::available(apps),
            issues,
        }
    }
}

fn collect_apps(
    dir: &Path,
    location: AppLocation,
    depth: usize,
    out: &mut Vec<InstalledApp>,
    issues: &mut Vec<ScanIssue>,
) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            issues.push(ScanIssue::new(
                IssueKind::PermissionDenied,
                dir.display().to_string(),
                "macOS denied access to this folder",
            ));
            return;
        }
        Err(e) => {
            issues.push(ScanIssue::new(
                IssueKind::Other,
                dir.display().to_string(),
                e.to_string(),
            ));
            return;
        }
    };
    for entry in rd.flatten() {
        let path = entry.path();
        // Do not follow symlinks out of the application folders.
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let is_app = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("app"))
            .unwrap_or(false);
        if is_app {
            match read_bundle(&path, location) {
                Ok(app) => out.push(app),
                Err(msg) => issues.push(ScanIssue::new(
                    IssueKind::ParseError,
                    path.display().to_string(),
                    msg,
                )),
            }
        } else if depth == 0 && !path.ends_with("Utilities") {
            // Vendor folders such as "/Applications/Some Vendor/Tool.app".
            collect_apps(&path, location, depth + 1, out, issues);
        }
    }
}

fn read_bundle(path: &Path, location: AppLocation) -> Result<InstalledApp, String> {
    let plist_path = path.join("Contents/Info.plist");
    let meta = std::fs::metadata(&plist_path).map_err(|e| format!("no Info.plist: {e}"))?;
    if meta.len() > 4 * 1024 * 1024 {
        return Err("Info.plist larger than 4 MiB; skipped".into());
    }
    let value = plist::Value::from_file(&plist_path).map_err(|e| e.to_string())?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| "Info.plist is not a dictionary".to_string())?;
    let get = |k: &str| {
        dict.get(k)
            .and_then(|v| v.as_string())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let file_stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = get("CFBundleDisplayName")
        .or_else(|| get("CFBundleName"))
        .unwrap_or(file_stem);
    Ok(InstalledApp {
        name,
        path: path.display().to_string(),
        bundle_id: get("CFBundleIdentifier"),
        version: get("CFBundleShortVersionString"),
        build: get("CFBundleVersion"),
        location,
        metadata_source: plist_path.display().to_string(),
    })
}

/// Tools in `/usr/bin` that are stubs for the Xcode Command Line Tools.
/// Running them without the tools installed opens an installer dialog.
const XCODE_SHIMS: &[&str] = &[
    "git",
    "python3",
    "pip3",
    "make",
    "clang",
    "clang++",
    "gcc",
    "g++",
    "swift",
    "swiftc",
    "cc",
    "c++",
    "ld",
    "lldb",
    "xcodebuild",
    "xcrun",
    "svn",
];

impl ProbePolicy for MacOs {
    fn may_probe_version(&self, tool: &str, path: &Path) -> Result<(), String> {
        let in_usr_bin = path.parent() == Some(Path::new("/usr/bin"));
        if in_usr_bin && XCODE_SHIMS.contains(&tool) {
            let clt = Path::new("/Library/Developer/CommandLineTools/usr/bin").is_dir();
            let xcode = Path::new("/Applications/Xcode.app").is_dir();
            if !clt && !xcode {
                return Err(
                    "this /usr/bin stub would prompt to install the Xcode Command Line Tools"
                        .into(),
                );
            }
        }
        if in_usr_bin && matches!(tool, "java" | "javac" | "jar") {
            let has_jdk = std::fs::read_dir("/Library/Java/JavaVirtualMachines")
                .map(|mut rd| rd.next().is_some())
                .unwrap_or(false);
            if !has_jdk {
                return Err(
                    "no JDK installed; the /usr/bin stub would open an install prompt".into(),
                );
            }
        }
        Ok(())
    }
}

impl Platform for MacOs {
    fn info(&self) -> PlatformInfo {
        let os_version =
            plist::Value::from_file("/System/Library/CoreServices/SystemVersion.plist")
                .ok()
                .and_then(|v| {
                    v.as_dictionary()
                        .and_then(|d| d.get("ProductVersion"))
                        .and_then(|v| v.as_string())
                        .map(str::to_string)
                });
        PlatformInfo {
            os: "macos".into(),
            os_version,
            arch: std::env::consts::ARCH.into(),
            support_level: SupportLevel::Implemented,
        }
    }

    fn supported_package_managers(&self) -> Vec<PackageManagerSupport> {
        use PackageManagerId::*;
        [Homebrew, Npm, Pnpm, Uv, Pip, Cargo]
            .into_iter()
            .map(|id| PackageManagerSupport {
                id,
                inventory_implemented: true,
            })
            .chain(std::iter::once(PackageManagerSupport {
                id: Yarn,
                inventory_implemented: false,
            }))
            .collect()
    }

    fn gpus(&self, runner: &dyn CommandRunner) -> Availability<Vec<GpuInfo>> {
        let spec = CommandSpec::new("system_profiler", &["SPDisplaysDataType", "-json"])
            .timeout(Duration::from_secs(20));
        let out = match runner.run(&spec) {
            Ok(o) if o.success() => o,
            Ok(o) => {
                return Availability::unavailable(format!(
                    "system_profiler exited with {:?}",
                    o.exit_code
                ));
            }
            Err(e) => return Availability::unavailable(e.to_string()),
        };
        match parse_system_profiler_displays(&out.stdout) {
            Some(g) if !g.is_empty() => Availability::available(g),
            Some(_) => Availability::unavailable("system_profiler reported no graphics devices"),
            None => Availability::unavailable("could not parse system_profiler output"),
        }
    }
}

pub(crate) fn parse_system_profiler_displays(json: &str) -> Option<Vec<GpuInfo>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let items = v.get("SPDisplaysDataType")?.as_array()?;
    Some(
        items
            .iter()
            .filter_map(|g| {
                let model = g
                    .get("sppci_model")
                    .or_else(|| g.get("_name"))?
                    .as_str()?
                    .to_string();
                let vendor = g
                    .get("spdisplays_vendor")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim_start_matches("sppci_vendor_").to_string());
                let cores = g
                    .get("sppci_cores")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok());
                let vram = g
                    .get("spdisplays_vram")
                    .or_else(|| g.get("spdisplays_vram_shared"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                Some(GpuInfo {
                    model,
                    vendor,
                    cores,
                    vram,
                })
            })
            .collect(),
    )
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_roots() {
        let m = MacOs::new();
        for p in [
            "/",
            "/System",
            "/System/Library",
            "/usr/local",
            "/private/var",
            "/Volumes",
        ] {
            assert!(m.is_protected_scan_root(Path::new(p)), "{p}");
        }
        if let Some(h) = dirs::home_dir() {
            assert!(!m.is_protected_scan_root(&h));
            assert!(m.is_protected_scan_root(&h.join("Library/Caches")));
            assert!(!m.is_protected_scan_root(&h.join("code")));
        }
    }

    #[test]
    fn parses_system_profiler_fixture() {
        let json = r#"{"SPDisplaysDataType":[{"_name":"kHW_AppleM2Item","sppci_model":"Apple M2","sppci_cores":"10","spdisplays_vendor":"sppci_vendor_Apple"}]}"#;
        let g = parse_system_profiler_displays(json).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].model, "Apple M2");
        assert_eq!(g[0].cores, Some(10));
        assert_eq!(g[0].vendor.as_deref(), Some("Apple"));
    }

    #[test]
    fn reads_a_real_bundle_from_a_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Demo.app/Contents");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            app.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Demo</string>
<key>CFBundleIdentifier</key><string>org.example.demo</string>
<key>CFBundleShortVersionString</key><string>1.2.3</string>
</dict></plist>"#,
        )
        .unwrap();
        let mut out = Vec::new();
        let mut issues = Vec::new();
        collect_apps(dir.path(), AppLocation::User, 0, &mut out, &mut issues);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bundle_id.as_deref(), Some("org.example.demo"));
        assert_eq!(out[0].version.as_deref(), Some("1.2.3"));
        assert!(issues.is_empty());
    }

    #[test]
    fn real_application_inventory_returns_data() {
        // On any macOS host, /System/Applications contains at least one bundle.
        let scan = MacOs::new().list_applications();
        let apps = scan.apps.into_value().unwrap();
        assert!(apps.iter().any(|a| a.location == AppLocation::System));
    }
}
