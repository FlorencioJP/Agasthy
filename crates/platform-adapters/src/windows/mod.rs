//! Windows adapter (unvalidated).
//!
//! Implemented and unit-tested against recorded output, not yet run on
//! Windows hardware: path resolution, application inventory (registry
//! Uninstall keys via `reg query`), Scoop and Chocolatey inventory, and
//! listening ports (`netstat`). GPU information and WinGet inventory return
//! `Unsupported`: WinGet only prints a localized table.

pub mod apps;

use std::path::Path;

use agasthy_core::{Availability, IssueKind, ScanIssue};

use crate::command::CommandRunner;
use crate::common::{process_path_dirs, push_well_known_user_dirs};
use crate::types::*;
use crate::{AppInventory, AppScan, Platform, ProbePolicy, SystemPaths};

#[derive(Default)]
pub struct Windows;

impl Windows {
    pub fn new() -> Self {
        Self
    }
}

impl SystemPaths for Windows {
    fn executable_search_dirs(&self) -> Vec<SearchDir> {
        let mut out = process_path_dirs();
        push_well_known_user_dirs(&mut out, dirs::home_dir().as_deref());
        out
    }

    fn executable_file_names(&self, tool: &str) -> Vec<String> {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| format!("{tool}{}", e.to_ascii_lowercase()))
            .collect()
    }

    fn is_protected_scan_root(&self, path: &Path) -> bool {
        let s = path.to_string_lossy().to_ascii_lowercase();
        let s = s.trim_end_matches(['\\', '/']);
        // Drive roots such as "c:" and OS-owned folders.
        (s.len() <= 2 && s.ends_with(':'))
            || s.ends_with(":\\windows")
            || s.contains(":\\windows\\")
            || s.ends_with(":\\program files")
            || s.ends_with(":\\program files (x86)")
            || s.ends_with(":\\programdata")
    }
}

impl AppInventory for Windows {
    fn list_applications(&self) -> AppScan {
        let (found, failed) = apps::read_all(&apps::registry_runner());
        let issues = failed
            .into_iter()
            .map(|f| ScanIssue::new(IssueKind::CommandFailed, "applications", f))
            .collect();
        AppScan {
            apps: Availability::available(found),
            issues,
        }
    }
}

impl ProbePolicy for Windows {
    fn may_probe_version(&self, _tool: &str, path: &Path) -> Result<(), String> {
        // App-execution aliases (e.g. the Store `python.exe` stub) open the
        // Microsoft Store instead of printing a version.
        if path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\microsoft\\windowsapps\\")
        {
            return Err("Windows app-execution alias; may open the Microsoft Store".into());
        }
        Ok(())
    }
}

impl Platform for Windows {
    fn info(&self) -> PlatformInfo {
        PlatformInfo {
            os: "windows".into(),
            os_version: None,
            arch: std::env::consts::ARCH.into(),
            support_level: SupportLevel::Unvalidated,
        }
    }

    fn supported_package_managers(&self) -> Vec<PackageManagerSupport> {
        use PackageManagerId::*;
        [Npm, Pnpm, Uv, Cargo, Scoop, Chocolatey]
            .into_iter()
            .map(|id| PackageManagerSupport {
                id,
                inventory_implemented: true,
            })
            .chain([Winget, Yarn].into_iter().map(|id| PackageManagerSupport {
                id,
                inventory_implemented: false,
            }))
            .collect()
    }

    fn gpus(&self, _runner: &dyn CommandRunner) -> Availability<Vec<GpuInfo>> {
        Availability::unsupported("windows")
    }
}
