//! Typed, kind-specific attributes stored in `entities.attributes`.
//!
//! These are exported to TypeScript so the UI reads the same shapes the
//! backend writes.

use agasthy_core::Availability;
use agasthy_platform::{PackageManagerId, SearchDirOrigin};
use agasthy_provenance::RemoteRef;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RepositoryAttrs {
    /// The approved scan root this repository was found under.
    pub scan_root: String,
    /// `directory` for a normal clone, `file` for a linked worktree or submodule.
    pub git_dir_kind: String,
    pub head: Availability<HeadInfo>,
    pub working_tree: Availability<WorkingTreeStatus>,
    pub local_branches: Availability<u32>,
    pub remotes: Vec<RemoteInfo>,
    /// The `origin` remote if configured, otherwise the first parseable remote.
    pub primary_remote: Option<RemoteRef>,
    pub manifests: Vec<String>,
    pub languages: Vec<LanguageShare>,
    pub primary_language: Option<String>,
    pub frameworks: Vec<String>,
    pub license: Option<LicenseInfo>,
    pub size_bytes: Availability<u64>,
    /// Number of local clones sharing `primary_remote` (including this one).
    pub clones_of_same_remote: u32,
    /// Distinct external dependencies declared across the project's manifests.
    pub dependency_count: u32,
    /// Runtime families the project declares or implies (`node`, `python`, …).
    pub runtime_requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HeadInfo {
    pub branch: Option<String>,
    pub detached: bool,
    /// The repository has no commits yet.
    pub unborn: bool,
    pub commit_sha: Option<String>,
    pub last_commit_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorkingTreeStatus {
    pub clean: bool,
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
    pub conflicted: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteInfo {
    pub name: String,
    /// URL with any embedded credentials removed.
    pub url: String,
    pub credentials_redacted: bool,
    pub parsed: Option<RemoteRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LanguageShare {
    pub language: String,
    pub files: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LicenseInfo {
    /// SPDX identifier when recognised.
    pub spdx: Option<String>,
    /// Where it was read from, e.g. `LICENSE` or `package.json`.
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToolCategory {
    Runtime,
    PackageManager,
    VersionControl,
    Build,
    Container,
    Cloud,
    Database,
    Editor,
    AiTool,
    Utility,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BinaryAttrs {
    /// Catalogue id, e.g. `node`.
    pub tool: String,
    pub display_name: String,
    pub category: ToolCategory,
    pub file_name: String,
    pub directory: String,
    pub directory_origin: SearchDirOrigin,
    /// Symlinks resolved, when different from `path`.
    pub resolved_path: Option<String>,
    /// First match in AGASTHY's search order. A terminal with a different
    /// `PATH` may resolve a different copy.
    pub active: bool,
    /// Position in resolution order among copies of this tool (0 = active).
    pub rank: u32,
    pub shadowed_by: Option<String>,
    pub version: Availability<String>,
    pub version_command: Option<String>,
    /// How the binary appears to have been installed, from its resolved path.
    pub install_hint: Option<InstallHint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstallHint {
    /// e.g. `homebrew`, `nvm`, `rustup`, `npm`, `system`.
    pub installer: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PackageAttrs {
    pub manager: PackageManagerId,
    /// `formula`, `cask`, `global`, `tool`, `distribution` or `crate`.
    pub package_kind: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    /// Homebrew: installed explicitly rather than as a dependency.
    pub installed_on_request: Option<bool>,
    pub upstream: Option<RemoteRef>,
    /// Executables or application bundles the manager says this package provides.
    pub provides: Vec<String>,
    pub install_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export)]
pub enum ManagerState {
    /// Inventory read successfully.
    Read {
        binary: String,
        version: Option<String>,
    },
    /// The manager's executable was not found.
    NotInstalled,
    /// Installed, but this build cannot read its inventory yet.
    NotImplemented { binary: String },
    /// Installed, but reading the inventory failed.
    Failed { binary: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PackageManagerAttrs {
    pub manager: PackageManagerId,
    pub display_name: String,
    pub state: ManagerState,
    pub package_count: Option<u32>,
    pub inventory_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DependencyAttrs {
    /// `npm`, `cargo`, `pypi` or `go`.
    pub ecosystem: String,
    pub package: String,
}

/// One place a project declares a dependency or runtime requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Declaration {
    /// Path of the manifest relative to the repository root.
    pub manifest: String,
    /// The constraint as written, e.g. `^18.2.0`, `>=3.10`. `None` when the
    /// requirement is only implied (e.g. a `package.json` with no `engines`).
    pub constraint: Option<String>,
    /// `runtime`, `dev`, `build`, `optional`, `peer`, or for runtimes the field name.
    pub scope: String,
    /// Version pinned by a lockfile, when one was read.
    pub resolved: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DependsOnAttrs {
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RequiresRuntimeAttrs {
    /// Runtime family: `node`, `python`, `rust`, `go`, `ruby`.
    pub runtime: String,
    pub declarations: Vec<Declaration>,
}
