//! Operating-system adapters.
//!
//! Everything platform-specific lives behind the traits in this crate. Shared
//! product logic (discovery, persistence, UI) only sees these traits, so adding
//! an OS means implementing them, not forking the product.
//!
//! * macOS: implemented and validated.
//! * Windows, Linux: implemented and unit-tested against recorded output, not
//!   yet validated on real hardware. Their modules compile (and their tests
//!   run) on every OS; [`current`] picks the one for the running OS. Anything
//!   still missing returns [`Availability::Unsupported`], never an empty result.

pub mod command;
pub mod types;

pub mod linux;
#[cfg(target_os = "macos")]
mod macos;
pub mod windows;

mod common;

use std::path::{Path, PathBuf};

use agasthy_core::{Availability, ScanIssue};

pub use command::{
    CommandError, CommandOutput, CommandRunner, CommandSpec, MockCommandRunner,
    SystemCommandRunner, is_executable,
};
pub use types::*;

/// Filesystem locations and executable-resolution rules.
pub trait SystemPaths: Send + Sync {
    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }

    /// Directories searched for executables, in resolution order, with the
    /// reason each is included.
    fn executable_search_dirs(&self) -> Vec<SearchDir>;

    /// File names under which `tool` may exist (e.g. adds `.exe` on Windows).
    fn executable_file_names(&self, tool: &str) -> Vec<String>;

    /// Paths that must never be accepted as a scan root (too broad or OS-owned).
    fn is_protected_scan_root(&self, path: &Path) -> bool;
}

/// Installed application discovery.
pub trait AppInventory: Send + Sync {
    fn list_applications(&self) -> AppScan;
}

/// Rules about which discovered executables may be run to read a version.
pub trait ProbePolicy: Send + Sync {
    /// `Ok(())` when running `path --version` is expected to be side-effect free.
    /// `Err(reason)` when it could, for example, open an installer dialog.
    fn may_probe_version(&self, tool: &str, path: &Path) -> Result<(), String>;
}

/// The full adapter for one operating system.
pub trait Platform: SystemPaths + AppInventory + ProbePolicy {
    fn info(&self) -> PlatformInfo;

    /// Package managers whose inventory this build can read on this OS.
    fn supported_package_managers(&self) -> Vec<PackageManagerSupport>;

    fn gpus(&self, runner: &dyn CommandRunner) -> Availability<Vec<GpuInfo>>;
}

/// Result of application discovery.
#[derive(Debug, Clone)]
pub struct AppScan {
    pub apps: Availability<Vec<InstalledApp>>,
    pub issues: Vec<ScanIssue>,
}

/// The adapter for the OS this binary was compiled for.
pub fn current() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOs::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::Windows::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::Linux::new())
    }
}

/// Programs AGASTHY may run in this build. Shells and privilege-escalation
/// tools are refused by the runner even if added here.
pub const DEFAULT_COMMAND_ALLOWLIST: &[&str] = &[
    // version control / inventory
    "git",
    "brew",
    "npm",
    "pnpm",
    "uv",
    "python3",
    "cargo",
    // macOS hardware info
    "system_profiler",
    // listening ports (read-only: `lsof -nP -iTCP -sTCP:LISTEN -Fpn`)
    "lsof",
    // Docker usage and the two prune operations in agasthy-cleanup::docker
    "docker",
    // OSV.dev advisory lookups, only when the user enables network checks;
    // fixed HTTPS URL and arguments in agasthy-security.
    "curl",
];

/// Build the command runner used in production for this platform.
pub fn system_runner(platform: &dyn Platform, extra_allowed: &[&str]) -> SystemCommandRunner {
    let dirs = platform
        .executable_search_dirs()
        .into_iter()
        .map(|d| d.path)
        .collect();
    let mut allow: Vec<&str> = DEFAULT_COMMAND_ALLOWLIST.to_vec();
    allow.extend_from_slice(extra_allowed);
    // This OS's package managers (read-only inventory commands only).
    for m in platform.supported_package_managers() {
        if m.inventory_implemented {
            allow.push(m.id.binary());
        }
    }
    if platform.info().os == "windows" {
        // Listening ports: `netstat -ano` (read-only).
        allow.push("netstat");
    }
    let suffixes = platform
        .executable_file_names("x")
        .into_iter()
        .map(|n| n[1..].to_string())
        .collect();
    SystemCommandRunner::new(&allow, dirs, suffixes)
}
