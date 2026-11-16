//! Shared domain model: entities, relationships and the evidence behind them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What kind of thing an entity is.
///
/// The kinds deliberately keep provenance classes apart: a cloned repository,
/// a package-manager install, a binary on `PATH` and an installed application
/// are different kinds and are never merged by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EntityKind {
    /// A local Git working tree found in a user-approved scan root.
    Repository,
    /// An upstream project identified from a Git remote or package metadata.
    RemoteProject,
    /// An application bundle installed on the machine.
    Application,
    /// A command-line tool found on `PATH`.
    Binary,
    /// A language runtime found on `PATH`.
    Runtime,
    /// A package installed through a package manager.
    Package,
    /// A package manager whose inventory AGASTHY read.
    PackageManager,
    /// A dependency declared in a project manifest (`ecosystem:name`).
    Dependency,
    /// An AI coding assistant, desktop AI app or local inference engine.
    AiTool,
    /// An AI model provider (cloud API or local endpoint).
    AiProvider,
    /// A model stored on disk for local inference.
    LocalModel,
    /// A Model Context Protocol server declared in a configuration file.
    McpServer,
    /// An AI instruction, rules, agent, skill, prompt, settings or MCP file.
    AiConfigFile,
}

impl EntityKind {
    pub const ALL: [EntityKind; 13] = [
        EntityKind::Repository,
        EntityKind::RemoteProject,
        EntityKind::Application,
        EntityKind::Binary,
        EntityKind::Runtime,
        EntityKind::Package,
        EntityKind::PackageManager,
        EntityKind::Dependency,
        EntityKind::AiTool,
        EntityKind::AiProvider,
        EntityKind::LocalModel,
        EntityKind::McpServer,
        EntityKind::AiConfigFile,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Repository => "repository",
            EntityKind::RemoteProject => "remote_project",
            EntityKind::Application => "application",
            EntityKind::Binary => "binary",
            EntityKind::Runtime => "runtime",
            EntityKind::Package => "package",
            EntityKind::PackageManager => "package_manager",
            EntityKind::Dependency => "dependency",
            EntityKind::AiTool => "ai_tool",
            EntityKind::AiProvider => "ai_provider",
            EntityKind::LocalModel => "local_model",
            EntityKind::McpServer => "mcp_server",
            EntityKind::AiConfigFile => "ai_config_file",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// How strongly the evidence supports a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Confidence {
    Unknown,
    Probable,
    Strong,
    Verified,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::Verified => "verified",
            Confidence::Strong => "strong",
            Confidence::Probable => "probable",
            Confidence::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "verified" => Confidence::Verified,
            "strong" => Confidence::Strong,
            "probable" => Confidence::Probable,
            _ => Confidence::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RelationKind {
    /// Repository has this project configured as its `origin` remote.
    OriginRemote,
    /// Repository has this project configured as a non-origin remote.
    TracksRemote,
    /// Package was installed by this package manager.
    ManagedBy,
    /// Package metadata names this project as its upstream.
    UpstreamProject,
    /// Package installed this application or binary.
    Provides,
    /// Two local repositories are clones of the same upstream project.
    DuplicateClone,
    /// A binary on PATH resolves into files owned by this package.
    InstalledBy,
    /// A project's manifest declares this dependency.
    DependsOn,
    /// A project declares a runtime version requirement this runtime may satisfy.
    RequiresRuntime,
    /// A repository contains this AI configuration file.
    Contains,
    /// An AI configuration file is read by this tool.
    ConfiguresTool,
    /// A configuration file declares this MCP server.
    DeclaresMcpServer,
    /// An AI tool uses this MCP server (via its configuration).
    UsesMcpServer,
    /// An AI tool uses this provider.
    UsesProvider,
    /// A local inference engine serves this model.
    ServesModel,
    /// Two AI configuration files have identical content.
    SameContent,
    /// Two applicable instruction files state different values for the same setting.
    PotentialConflict,
    /// An instruction file imports or references another.
    References,
}

impl RelationKind {
    pub const ALL: [RelationKind; 18] = [
        RelationKind::OriginRemote,
        RelationKind::TracksRemote,
        RelationKind::ManagedBy,
        RelationKind::UpstreamProject,
        RelationKind::Provides,
        RelationKind::DuplicateClone,
        RelationKind::InstalledBy,
        RelationKind::DependsOn,
        RelationKind::RequiresRuntime,
        RelationKind::Contains,
        RelationKind::ConfiguresTool,
        RelationKind::DeclaresMcpServer,
        RelationKind::UsesMcpServer,
        RelationKind::UsesProvider,
        RelationKind::ServesModel,
        RelationKind::SameContent,
        RelationKind::PotentialConflict,
        RelationKind::References,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            RelationKind::OriginRemote => "origin_remote",
            RelationKind::TracksRemote => "tracks_remote",
            RelationKind::ManagedBy => "managed_by",
            RelationKind::UpstreamProject => "upstream_project",
            RelationKind::Provides => "provides",
            RelationKind::DuplicateClone => "duplicate_clone",
            RelationKind::InstalledBy => "installed_by",
            RelationKind::DependsOn => "depends_on",
            RelationKind::RequiresRuntime => "requires_runtime",
            RelationKind::Contains => "contains",
            RelationKind::ConfiguresTool => "configures_tool",
            RelationKind::DeclaresMcpServer => "declares_mcp_server",
            RelationKind::UsesMcpServer => "uses_mcp_server",
            RelationKind::UsesProvider => "uses_provider",
            RelationKind::ServesModel => "serves_model",
            RelationKind::SameContent => "same_content",
            RelationKind::PotentialConflict => "potential_conflict",
            RelationKind::References => "references",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// A single observation supporting an entity or relationship.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EvidenceInput {
    /// Machine-readable evidence type, e.g. `git_config_remote` or `info_plist`.
    pub kind: String,
    /// Human-readable description of what was observed. Never contains secrets.
    pub detail: String,
    /// Where it was observed: a file path or the command that was run.
    pub source: String,
    pub confidence: Confidence,
}

impl EvidenceInput {
    pub fn new(
        kind: impl Into<String>,
        detail: impl Into<String>,
        source: impl Into<String>,
        confidence: Confidence,
    ) -> Self {
        Self {
            kind: kind.into(),
            detail: detail.into(),
            source: source.into(),
            confidence,
        }
    }
}

/// An entity as produced by a discovery pass, before persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityInput {
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Discovery source, e.g. `git`, `applications_dir`, `path`, `homebrew`.
    pub source: String,
    /// Kind-specific attributes (typed structs serialized to JSON).
    pub attributes: serde_json::Value,
    pub evidence: Vec<EvidenceInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipInput {
    pub from_id: String,
    pub to_id: String,
    pub kind: RelationKind,
    pub confidence: Confidence,
    pub evidence: Vec<EvidenceInput>,
    /// Kind-specific data, e.g. a version constraint. `Null` when unused.
    #[serde(default)]
    pub attributes: serde_json::Value,
}

impl RelationshipInput {
    pub fn id(&self) -> String {
        format!("{}:{}->{}", self.kind.as_str(), self.from_id, self.to_id)
    }
}

/// Which previously stored entities a scan is authoritative for.
///
/// After ingesting, entities in scope that were not seen in this scan are
/// removed from AGASTHY's database (never from the machine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PruneScope {
    pub kind: EntityKind,
    /// Restrict to one discovery source; `None` means every source of that kind.
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScanKind {
    Repositories,
    Software,
    Packages,
    Ai,
}

impl ScanKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScanKind::Repositories => "repositories",
            ScanKind::Software => "software",
            ScanKind::Packages => "packages",
            ScanKind::Ai => "ai",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "repositories" => Some(ScanKind::Repositories),
            "software" => Some(ScanKind::Software),
            "packages" => Some(ScanKind::Packages),
            "ai" => Some(ScanKind::Ai),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum IssueKind {
    PermissionDenied,
    NotFound,
    Timeout,
    CommandFailed,
    ParseError,
    Unsupported,
    Skipped,
    Other,
}

/// A non-fatal problem encountered during a scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScanIssue {
    pub kind: IssueKind,
    pub subject: String,
    pub message: String,
}

impl ScanIssue {
    pub fn new(kind: IssueKind, subject: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            subject: subject.into(),
            message: message.into(),
        }
    }
}

/// Everything a single scan produced, written atomically by the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestBatch {
    pub scan_kind: ScanKind,
    pub entities: Vec<EntityInput>,
    pub relationships: Vec<RelationshipInput>,
    pub prune: Vec<PruneScope>,
    pub issues: Vec<ScanIssue>,
}

impl IngestBatch {
    pub fn new(scan_kind: ScanKind) -> Self {
        Self {
            scan_kind,
            entities: Vec::new(),
            relationships: Vec::new(),
            prune: Vec::new(),
            issues: Vec::new(),
        }
    }
}

// ----- Read models returned to the UI -----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Entity {
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub source: String,
    pub attributes: serde_json::Value,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Evidence {
    pub id: i64,
    pub kind: String,
    pub detail: String,
    pub source: String,
    pub confidence: Confidence,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Relationship {
    pub id: String,
    pub kind: RelationKind,
    pub from_id: String,
    pub to_id: String,
    pub confidence: Confidence,
    pub attributes: serde_json::Value,
    pub evidence: Vec<Evidence>,
    /// The entity on the other side of the relationship, from the viewer's side.
    pub other: Option<EntityRef>,
}

/// A relationship without its evidence, for graph traversal and rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Edge {
    pub id: String,
    pub kind: RelationKind,
    pub from_id: String,
    pub to_id: String,
    pub confidence: Confidence,
    pub attributes: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EntityRef {
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EntityDetail {
    pub entity: Entity,
    pub evidence: Vec<Evidence>,
    pub outgoing: Vec<Relationship>,
    pub incoming: Vec<Relationship>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScanRoot {
    pub id: i64,
    pub path: String,
    pub enabled: bool,
    pub max_depth: u32,
    pub added_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScanStatus {
    Running,
    Completed,
    CompletedWithIssues,
    Failed,
}

impl ScanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScanStatus::Running => "running",
            ScanStatus::Completed => "completed",
            ScanStatus::CompletedWithIssues => "completed_with_issues",
            ScanStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => ScanStatus::Running,
            "completed" => ScanStatus::Completed,
            "completed_with_issues" => ScanStatus::CompletedWithIssues,
            _ => ScanStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScanRecord {
    pub id: i64,
    pub kind: ScanKind,
    pub status: ScanStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub entities_found: u32,
    pub entities_removed: u32,
    pub issues: Vec<ScanIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KindCount {
    pub kind: EntityKind,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RelationCount {
    pub kind: RelationKind,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AuditEntry {
    pub id: i64,
    pub at: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub outcome: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EntityQuery {
    pub kind: Option<EntityKind>,
    /// Full-text search over name, path and attributes.
    pub text: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
