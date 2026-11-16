//! Linux adapter (skeleton).
//!
//! Path resolution is implemented. Application inventory (`.desktop` files,
//! Flatpak, Snap) and GPU information return `Unsupported` for now.

use std::path::{Path, PathBuf};

use agasthy_core::{Availability, IssueKind, ScanIssue};

use crate::command::CommandRunner;
use crate::common::{process_path_dirs, push_unique, push_well_known_user_dirs};
use crate::types::*;
use crate::{AppInventory, AppScan, Platform, ProbePolicy, SystemPaths};

pub struct Linux;

impl Linux {
    pub fn new() -> Self {
        Self
    }
}

impl SystemPaths for Linux {
    fn executable_search_dirs(&self) -> Vec<SearchDir> {
        let mut out = process_path_dirs();
        for p in [
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ] {
            let p = PathBuf::from(p);
            if p.is_dir() {
                push_unique(&mut out, p, SearchDirOrigin::WellKnown);
            }
        }
        push_well_known_user_dirs(&mut out, dirs::home_dir().as_deref());
        out
    }

    fn executable_file_names(&self, tool: &str) -> Vec<String> {
        vec![tool.to_string()]
    }

    fn is_protected_scan_root(&self, path: &Path) -> bool {
        let s = path.to_string_lossy();
        let s = s.trim_end_matches('/');
        let s = if s.is_empty() { "/" } else { s };
        matches!(
            s,
            "/" | "/proc"
                | "/sys"
                | "/dev"
                | "/usr"
                | "/bin"
                | "/sbin"
                | "/etc"
                | "/boot"
                | "/var"
                | "/run"
                | "/lib"
                | "/lib64"
                | "/tmp"
        ) || ["/proc/", "/sys/", "/dev/", "/usr/", "/etc/", "/boot/"]
            .iter()
            .any(|p| s.starts_with(p))
    }
}

impl AppInventory for Linux {
    fn list_applications(&self) -> AppScan {
        AppScan {
            apps: Availability::unsupported("linux"),
            issues: vec![ScanIssue::new(
                IssueKind::Unsupported,
                "applications",
                "Linux application inventory is not implemented in this build",
            )],
        }
    }
}

impl ProbePolicy for Linux {
    fn may_probe_version(&self, _tool: &str, _path: &Path) -> Result<(), String> {
        Ok(())
    }
}

impl Platform for Linux {
    fn info(&self) -> PlatformInfo {
        PlatformInfo {
            os: "linux".into(),
            os_version: None,
            arch: std::env::consts::ARCH.into(),
            support_level: SupportLevel::Skeleton,
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

    fn gpus(&self, _runner: &dyn CommandRunner) -> Availability<Vec<GpuInfo>> {
        Availability::unsupported("linux")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_roots() {
        let l = Linux::new();
        assert!(l.is_protected_scan_root(Path::new("/")));
        assert!(l.is_protected_scan_root(Path::new("/proc/1")));
        assert!(!l.is_protected_scan_root(Path::new("/home/dev/code")));
    }
}
