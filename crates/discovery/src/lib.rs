//! Read-only discovery.
//!
//! Every function here observes the machine and returns an
//! [`agasthy_core::IngestBatch`]. Nothing in this crate modifies files,
//! installs software, or talks to the network. External programs run only
//! through the guarded [`agasthy_platform::CommandRunner`].
//!
//! Discovered file contents and command output are untrusted input: they are
//! parsed with size limits and never executed or interpreted as instructions.

pub mod ai;
pub mod attrs;
pub mod catalog;
pub mod git;
pub mod manifests;
pub mod packages;
pub mod repos;
pub mod roots;
pub mod software;
pub mod version;

pub use ai::{AiScanOptions, scan_ai};
pub use packages::scan_packages;
pub use repos::{RepoScanOptions, RuntimeCopy, scan_repositories};
pub use roots::{RootError, validate_scan_root};
pub use software::{runtime_copies, scan_software};

/// Progress callback payload for long scans.
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct ScanProgress {
    pub phase: String,
    pub done: u32,
    pub total: Option<u32>,
    pub current: Option<String>,
}

pub type ProgressFn<'a> = &'a (dyn Fn(ScanProgress) + Sync);

/// A progress callback that ignores updates.
pub fn no_progress(_: ScanProgress) {}
