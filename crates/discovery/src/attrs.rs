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

// ----- AI ecosystem -----

/// One piece of on-disk evidence that an AI tool is present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiSignal {
    /// `binary`, `application`, `extension`, `config_dir`.
    pub kind: String,
    pub path: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiToolAttrs {
    pub tool: String,
    pub vendor: String,
    /// `cli`, `desktop_app`, `ide`, `ide_extension`, `local_inference`.
    pub category: String,
    /// True when an executable, application or extension was found. False
    /// when only configuration remains (possibly left over after uninstalling).
    pub installed: bool,
    pub signals: Vec<AiSignal>,
    /// For local inference engines: whether their local port accepted a TCP connection.
    pub local_endpoint: Option<LocalEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LocalEndpoint {
    pub address: String,
    pub listening: bool,
}

/// How far AGASTHY could confirm a provider, weakest to strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProviderLevel {
    /// A tool on this machine is built for this provider.
    Referenced,
    /// A configuration file names this provider.
    Configured,
    /// An endpoint for it is configured.
    EndpointConfigured,
    /// A credential appears to be present (only presence is checked).
    CredentialPresent,
    /// A local endpoint accepted a connection.
    Reachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProviderSignal {
    pub level: ProviderLevel,
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiProviderAttrs {
    pub provider: String,
    /// `cloud` or `local`.
    pub kind: String,
    pub highest: ProviderLevel,
    pub signals: Vec<ProviderSignal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LocalModelAttrs {
    /// `ollama`, `lm-studio`, `huggingface-cache`.
    pub store: String,
    pub model: String,
    pub tag: Option<String>,
    pub format: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpServerAttrs {
    pub server: String,
    /// `stdio`, `http`, `sse` or `unknown`.
    pub transport: String,
    pub command: Option<String>,
    /// Arguments with anything credential-like redacted.
    pub args: Vec<String>,
    /// URL with credentials and query string removed.
    pub url: Option<String>,
    /// Names of environment variables passed to the server. Values are never read.
    pub env_names: Vec<String>,
    pub config_file: String,
    pub tool: Option<String>,
    /// `global` or the project path the server is scoped to.
    pub scope: String,
    pub disabled: bool,
    /// Same command + args or URL: identifies the same server configured in several places.
    pub fingerprint: String,
}

/// A line that states a setting, used for conflict detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Directive {
    /// `package_manager`, `indentation`, `quotes`, `semicolons`.
    pub topic: String,
    pub value: String,
    pub line: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiConfigFileAttrs {
    /// `instructions`, `rules`, `agent`, `skill`, `command`, `mcp_config`, `settings`.
    pub file_type: String,
    /// `global`, `project` or `directory`.
    pub scope: String,
    pub tools: Vec<String>,
    pub project: Option<String>,
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_at: Option<i64>,
    pub sha256: String,
    pub lines: u32,
    pub headings: Vec<String>,
    /// Redacted text for search and preview (instruction-like files only, capped).
    pub text: Option<String>,
    pub text_truncated: bool,
    pub redactions: u32,
    /// No tool that reads this file was found on this machine.
    pub orphaned: bool,
    pub duplicates: Vec<String>,
    /// Other instruction files that apply to the same directory.
    pub overlaps: Vec<String>,
    pub directives: Vec<Directive>,
    /// Files this one imports (`@path`), resolved.
    pub references: Vec<String>,
}
