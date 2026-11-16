//! AI ecosystem discovery: assistants, providers, local models, MCP servers
//! and AI instruction/configuration files.
//!
//! Evidence comes from the filesystem (executables, app bundles, editor
//! extensions, configuration files) and, for local inference engines only, a
//! TCP connection to their port on 127.0.0.1. Nothing is sent anywhere.
//! Credential files are checked for existence only; environment variables for
//! their names only.

pub mod catalog;
pub mod files;
pub mod mcp;
pub mod models;
pub mod redact;
pub mod usage;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use agasthy_core::{
    Availability, Confidence, EntityInput, EntityKind, EvidenceInput, IngestBatch, IssueKind,
    PruneScope, RelationKind, RelationshipInput, ScanIssue, ScanKind,
};
use agasthy_platform::{Platform, is_executable};

use crate::attrs::{
    AiConfigFileAttrs, AiProviderAttrs, AiSignal, AiToolAttrs, Directive, LocalEndpoint,
    LocalModelAttrs, McpServerAttrs, ProviderLevel, ProviderSignal,
};
use crate::{ProgressFn, ScanProgress};
use catalog::{FileType, PROVIDERS, TOOLS, provider_from_name, provider_spec};
use files::{FoundFile, read_file, resolve_import};

#[derive(Debug, Clone)]
pub struct AiScanOptions {
    pub home: PathBuf,
    /// Repositories already discovered in user-approved folders: (entity id, path).
    pub projects: Vec<(String, PathBuf)>,
    /// Names (never values) of environment variables visible to this process.
    pub env_names: BTreeSet<String>,
    /// Try a TCP connection to local inference engines' ports on 127.0.0.1.
    pub check_local_ports: bool,
}

impl AiScanOptions {
    pub fn new(home: PathBuf, projects: Vec<(String, PathBuf)>) -> Self {
        Self {
            home,
            projects,
            env_names: std::env::vars_os()
                .filter_map(|(k, _)| k.into_string().ok())
                .collect(),
            check_local_ports: true,
        }
    }
}

fn ev(
    kind: &str,
    detail: impl Into<String>,
    source: impl Into<String>,
    c: Confidence,
) -> EvidenceInput {
    EvidenceInput::new(kind, detail, source, c)
}

fn rel(
    from: &str,
    to: &str,
    kind: RelationKind,
    c: Confidence,
    evidence: Vec<EvidenceInput>,
) -> RelationshipInput {
    RelationshipInput {
        from_id: from.into(),
        to_id: to.into(),
        kind,
        confidence: c,
        attributes: serde_json::Value::Null,
        evidence,
    }
}

pub fn tool_id(id: &str) -> String {
    format!("ai_tool:{id}")
}

pub fn provider_id(id: &str) -> String {
    format!("ai_provider:{id}")
}

pub fn scan_ai(
    platform: &dyn Platform,
    opts: &AiScanOptions,
    progress: ProgressFn<'_>,
) -> IngestBatch {
    let mut batch = IngestBatch::new(ScanKind::Ai);
    let home = &opts.home;
    let step = |phase: &str| {
        progress(ScanProgress {
            phase: phase.into(),
            done: 0,
            total: None,
            current: None,
        })
    };

    // ----- tools -----
    step("ai-tools");
    let apps = match platform.list_applications().apps {
        Availability::Available { value } => value,
        _ => Vec::new(),
    };
    let dirs = platform.executable_search_dirs();
    let extensions = editor_extensions(home);
    let mut detected: BTreeMap<&'static str, AiToolAttrs> = BTreeMap::new();
    let mut tool_versions: HashMap<&'static str, String> = HashMap::new();
    for spec in TOOLS {
        let mut signals = Vec::new();
        for b in spec.binaries {
            'dirs: for d in &dirs {
                for f in platform.executable_file_names(b) {
                    let p = d.path.join(&f);
                    if is_executable(&p) {
                        signals.push(AiSignal {
                            kind: "binary".into(),
                            path: p.display().to_string(),
                            detail: None,
                        });
                        break 'dirs;
                    }
                }
            }
        }
        for app in &apps {
            let file = Path::new(&app.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let by_id = app
                .bundle_id
                .as_deref()
                .is_some_and(|id| spec.bundle_ids.contains(&id));
            // A bundle identifier is stronger evidence than a file name: a bundle whose
            // identifier belongs to another tool is that tool, whatever it is called.
            let claimed_elsewhere = app.bundle_id.as_deref().is_some_and(|id| {
                TOOLS
                    .iter()
                    .any(|t| t.id != spec.id && t.bundle_ids.contains(&id))
            });
            let by_name = spec.app_names.contains(&file.as_str()) && !claimed_elsewhere;
            if by_id || by_name {
                signals.push(AiSignal {
                    kind: "application".into(),
                    path: app.path.clone(),
                    detail: Some(match &app.bundle_id {
                        Some(id) => format!("{} ({id})", app.name),
                        None => app.name.clone(),
                    }),
                });
                if let Some(v) = &app.version {
                    tool_versions.entry(spec.id).or_insert_with(|| v.clone());
                }
            }
        }
        for (editor, ext_id, version, path) in &extensions {
            if spec.extensions.contains(&ext_id.as_str()) {
                signals.push(AiSignal {
                    kind: "extension".into(),
                    path: path.clone(),
                    detail: Some(format!("{editor} extension {ext_id} {version}")),
                });
                tool_versions
                    .entry(spec.id)
                    .or_insert_with(|| version.clone());
            }
        }
        let installed = !signals.is_empty();
        for c in spec.config_paths {
            let p = home.join(c);
            if p.exists() {
                signals.push(AiSignal {
                    kind: "config_dir".into(),
                    path: p.display().to_string(),
                    detail: None,
                });
            }
        }
        if signals.is_empty() {
            continue;
        }
        let local_endpoint = match (spec.local_port, installed && opts.check_local_ports) {
            (Some(port), true) => {
                let addr = SocketAddr::from(([127, 0, 0, 1], port));
                Some(LocalEndpoint {
                    address: addr.to_string(),
                    listening: TcpStream::connect_timeout(&addr, Duration::from_millis(250))
                        .is_ok(),
                })
            }
            _ => None,
        };
        detected.insert(
            spec.id,
            AiToolAttrs {
                tool: spec.id.into(),
                vendor: spec.vendor.into(),
                category: spec.category.into(),
                installed,
                signals,
                local_endpoint,
            },
        );
    }

    // ----- configuration files -----
    step("ai-files");
    let mut found: Vec<FoundFile> = files::global_files(home);
    for (rid, path) in &opts.projects {
        found.extend(files::project_files(rid, path));
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);
    let mut contents: Vec<(FoundFile, files::FileContent)> = Vec::new();
    for f in found {
        match read_file(&f) {
            Ok(c) => contents.push((f, c)),
            Err(e) => batch.issues.push(ScanIssue::new(
                IssueKind::Other,
                f.path.display().to_string(),
                format!("could not read: {e}"),
            )),
        }
    }

    // ----- providers -----
    let mut provider_signals: BTreeMap<&'static str, Vec<ProviderSignal>> = BTreeMap::new();
    let mut tool_provider_links: Vec<(&'static str, &'static str, String)> = Vec::new();
    for (id, attrs) in &detected {
        let spec = catalog::tool_spec(id).expect("detected tools come from the catalogue");
        if let (Some(p), true) = (spec.provider, attrs.installed) {
            provider_signals.entry(p).or_default().push(ProviderSignal {
                level: ProviderLevel::Referenced,
                source: spec.name.into(),
                detail: format!("{} is built for this provider", spec.name),
            });
            tool_provider_links.push((id, p, format!("{} is built for this provider", spec.name)));
        }
        if let (Some(p), Some(ep)) = (spec.provider, &attrs.local_endpoint)
            && ep.listening
        {
            provider_signals.entry(p).or_default().push(ProviderSignal {
                level: ProviderLevel::Reachable,
                source: ep.address.clone(),
                detail: format!("{} accepted a TCP connection on {}", spec.name, ep.address),
            });
        }
    }
    for p in PROVIDERS {
        for name in p.credential_env {
            if opts.env_names.contains(*name) {
                provider_signals
                    .entry(p.id)
                    .or_default()
                    .push(ProviderSignal {
                        level: ProviderLevel::CredentialPresent,
                        source: "environment".into(),
                        detail: format!(
                            "`{name}` is set in AGASTHY's environment (value not read)"
                        ),
                    });
            }
        }
        for name in p.endpoint_env {
            if opts.env_names.contains(*name) {
                provider_signals
                    .entry(p.id)
                    .or_default()
                    .push(ProviderSignal {
                        level: ProviderLevel::EndpointConfigured,
                        source: "environment".into(),
                        detail: format!("`{name}` is set in AGASTHY's environment"),
                    });
            }
        }
        for f in p.credential_files {
            let path = home.join(f);
            if path.is_file() {
                provider_signals
                    .entry(p.id)
                    .or_default()
                    .push(ProviderSignal {
                        level: ProviderLevel::CredentialPresent,
                        source: path.display().to_string(),
                        detail: "credential file exists (contents not read)".into(),
                    });
            }
        }
    }
    // Provider references inside tool settings (names only).
    for (f, c) in &contents {
        let Some(raw) = &c.raw else { continue };
        if f.file_type != FileType::Settings && f.file_type != FileType::McpConfig {
            continue;
        }
        for (pid, level, detail) in provider_refs_in_config(&f.rel, raw) {
            provider_signals
                .entry(pid)
                .or_default()
                .push(ProviderSignal {
                    level,
                    source: f.path.display().to_string(),
                    detail,
                });
            if let Some(tool) = f.tools.first()
                && detected.contains_key(tool)
            {
                tool_provider_links.push((
                    tool,
                    pid,
                    format!("configured in {}", f.path.display()),
                ));
            }
        }
    }
    for (pid, mut signals) in provider_signals {
        let Some(spec) = provider_spec(pid) else {
            continue;
        };
        signals.sort_by(|a, b| b.level.cmp(&a.level).then(a.source.cmp(&b.source)));
        signals.dedup_by(|a, b| a.level == b.level && a.detail == b.detail);
        let highest = signals
            .first()
            .map(|s| s.level)
            .unwrap_or(ProviderLevel::Referenced);
        batch.entities.push(EntityInput {
            id: provider_id(pid),
            kind: EntityKind::AiProvider,
            name: spec.name.into(),
            path: None,
            version: None,
            source: "ai".into(),
            evidence: signals
                .iter()
                .map(|s| {
                    ev(
                        "provider_signal",
                        s.detail.clone(),
                        s.source.clone(),
                        match s.level {
                            ProviderLevel::Referenced => Confidence::Strong,
                            _ => Confidence::Verified,
                        },
                    )
                })
                .collect(),
            attributes: serde_json::to_value(AiProviderAttrs {
                provider: pid.into(),
                kind: spec.kind.into(),
                highest,
                signals,
            })
            .unwrap_or_default(),
        });
    }
    let mut seen_links = BTreeSet::new();
    for (tool, pid, detail) in tool_provider_links {
        if seen_links.insert((tool, pid)) {
            batch.relationships.push(rel(
                &tool_id(tool),
                &provider_id(pid),
                RelationKind::UsesProvider,
                Confidence::Strong,
                vec![ev("tool_provider", detail, tool, Confidence::Strong)],
            ));
        }
    }

    // ----- tool entities -----
    for (id, attrs) in &detected {
        let spec = catalog::tool_spec(id).expect("catalogue tool");
        let evidence = attrs
            .signals
            .iter()
            .map(|s| {
                let (detail, conf) = match s.kind.as_str() {
                    "binary" => ("executable found".to_string(), Confidence::Verified),
                    "application" => (
                        "application bundle installed".to_string(),
                        Confidence::Verified,
                    ),
                    "extension" => (s.detail.clone().unwrap_or_default(), Confidence::Verified),
                    _ => (
                        "configuration present (may remain after uninstalling)".to_string(),
                        Confidence::Probable,
                    ),
                };
                ev(&format!("ai_{}", s.kind), detail, s.path.clone(), conf)
            })
            .collect();
        batch.entities.push(EntityInput {
            id: tool_id(id),
            kind: EntityKind::AiTool,
            name: spec.name.into(),
            path: attrs.signals.first().map(|s| s.path.clone()),
            version: tool_versions.get(id).cloned(),
            source: "ai".into(),
            attributes: serde_json::to_value(attrs).unwrap_or_default(),
            evidence,
        });
    }

    // ----- MCP servers -----
    step("ai-mcp");
    for (f, c) in &contents {
        if f.file_type != FileType::McpConfig && f.file_type != FileType::Settings {
            continue;
        }
        let Some(raw) = &c.raw else { continue };
        let file_name = f
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let servers = match mcp::parse(&file_name, raw) {
            Ok(s) => s,
            Err(e) => {
                batch.issues.push(ScanIssue::new(
                    IssueKind::ParseError,
                    f.path.display().to_string(),
                    format!("MCP configuration not parsed: {e}"),
                ));
                continue;
            }
        };
        let file_id = file_entity_id(&f.path);
        let tool = f.tools.first().copied();
        for s in servers {
            let id = format!("mcp:{}#{}#{}", f.path.display(), s.scope, s.name);
            batch.relationships.push(rel(
                &file_id,
                &id,
                RelationKind::DeclaresMcpServer,
                Confidence::Verified,
                vec![ev(
                    "mcp_declaration",
                    format!("server `{}` declared", s.name),
                    f.path.display().to_string(),
                    Confidence::Verified,
                )],
            ));
            if let Some(t) = tool.filter(|t| detected.contains_key(t)) {
                batch.relationships.push(rel(
                    &tool_id(t),
                    &id,
                    RelationKind::UsesMcpServer,
                    Confidence::Strong,
                    vec![ev(
                        "mcp_owner",
                        format!("declared in {}'s configuration", t),
                        f.path.display().to_string(),
                        Confidence::Strong,
                    )],
                ));
            }
            let fingerprint = s.fingerprint();
            batch.entities.push(EntityInput {
                id,
                kind: EntityKind::McpServer,
                name: s.name.clone(),
                path: Some(f.path.display().to_string()),
                version: None,
                source: "ai".into(),
                evidence: vec![ev(
                    "mcp_declaration",
                    format!("declared in {}", f.rel),
                    f.path.display().to_string(),
                    Confidence::Verified,
                )],
                attributes: serde_json::to_value(McpServerAttrs {
                    server: s.name,
                    transport: s.transport,
                    command: s.command,
                    args: s.args,
                    url: s.url,
                    env_names: s.env_names,
                    config_file: f.path.display().to_string(),
                    tool: tool.map(str::to_string),
                    scope: s.scope,
                    disabled: s.disabled,
                    fingerprint,
                })
                .unwrap_or_default(),
            });
        }
    }

    // ----- local models -----
    step("ai-models");
    for m in models::ollama(home)
        .into_iter()
        .chain(models::lm_studio(home))
        .chain(models::huggingface_cache(home))
    {
        let id = format!(
            "local_model:{}:{}{}",
            m.store,
            m.model,
            m.tag
                .as_deref()
                .map(|t| format!(":{t}"))
                .unwrap_or_default()
        );
        let engine = match m.store {
            "ollama" => Some("ollama"),
            "lm-studio" => Some("lm-studio"),
            _ => None,
        };
        if let Some(e) = engine.filter(|e| detected.contains_key(e)) {
            batch.relationships.push(rel(
                &tool_id(e),
                &id,
                RelationKind::ServesModel,
                Confidence::Verified,
                vec![ev(
                    "model_store",
                    format!("in {}'s model directory", e),
                    m.path.clone(),
                    Confidence::Verified,
                )],
            ));
        }
        batch.entities.push(EntityInput {
            id,
            kind: EntityKind::LocalModel,
            name: m.model.clone(),
            path: Some(m.path.clone()),
            version: m.tag.clone(),
            source: "ai".into(),
            evidence: vec![ev(
                "model_store",
                format!("found in the {} store", m.store),
                m.path.clone(),
                Confidence::Verified,
            )],
            attributes: serde_json::to_value(LocalModelAttrs {
                store: m.store.into(),
                model: m.model,
                tag: m.tag,
                format: m.format,
                size_bytes: m.size_bytes,
            })
            .unwrap_or_default(),
        });
    }

    // ----- file analysis: duplicates, overlaps, conflicts, imports -----
    step("ai-analysis");
    let ids: Vec<String> = contents
        .iter()
        .map(|(f, _)| file_entity_id(&f.path))
        .collect();
    let mut by_hash: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, (f, c)) in contents.iter().enumerate() {
        if f.file_type.indexable() && c.sha256.len() == 64 {
            by_hash.entry(c.sha256.as_str()).or_default().push(i);
        }
    }
    let mut duplicates: Vec<Vec<String>> = vec![Vec::new(); contents.len()];
    for group in by_hash.values().filter(|g| g.len() > 1) {
        for &i in group {
            duplicates[i] = group
                .iter()
                .filter(|&&j| j != i)
                .map(|&j| contents[j].0.path.display().to_string())
                .collect();
        }
        for &j in &group[1..] {
            batch.relationships.push(rel(
                &ids[j],
                &ids[group[0]],
                RelationKind::SameContent,
                Confidence::Verified,
                vec![ev(
                    "sha256",
                    "identical SHA-256 of contents",
                    contents[j].0.path.display().to_string(),
                    Confidence::Verified,
                )],
            ));
        }
    }
    // Overlaps: several instruction files that apply to the same directory.
    let mut by_dir: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (i, (f, _)) in contents.iter().enumerate() {
        if matches!(f.file_type, FileType::Instructions | FileType::Rules) && f.scope != "global" {
            let dir = applies_to_dir(f);
            by_dir.entry(dir).or_default().push(i);
        }
    }
    let mut overlaps: Vec<Vec<String>> = vec![Vec::new(); contents.len()];
    for group in by_dir.values().filter(|g| g.len() > 1) {
        for &i in group {
            overlaps[i] = group
                .iter()
                .filter(|&&j| j != i)
                .map(|&j| contents[j].0.rel.clone())
                .collect();
        }
    }
    // Conflicts among files that apply together: globals + each project's files.
    let mut chains: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
    for (i, (f, _)) in contents.iter().enumerate() {
        if f.file_type.indexable() {
            chains.entry(f.project.clone()).or_default().push(i);
        }
    }
    let globals: Vec<usize> = chains.get(&None).cloned().unwrap_or_default();
    let mut conflict_pairs = BTreeSet::new();
    for (project, members) in &chains {
        let mut set = members.clone();
        if project.is_some() {
            set.extend(globals.iter().copied());
        }
        for (a_idx, &a) in set.iter().enumerate() {
            for &b in &set[a_idx + 1..] {
                for (topic, va, vb, la, lb) in
                    conflicts(&contents[a].1.directives, &contents[b].1.directives)
                {
                    let key = (a.min(b), a.max(b), topic.clone());
                    if !conflict_pairs.insert(key) {
                        continue;
                    }
                    batch.relationships.push(RelationshipInput {
                        from_id: ids[a].clone(),
                        to_id: ids[b].clone(),
                        kind: RelationKind::PotentialConflict,
                        confidence: Confidence::Probable,
                        attributes: serde_json::json!({ "topic": topic, "values": [va, vb] }),
                        evidence: vec![
                            ev(
                                "directive",
                                format!("{topic}: {va} — line {}: {}", la.line, la.text),
                                contents[a].0.path.display().to_string(),
                                Confidence::Probable,
                            ),
                            ev(
                                "directive",
                                format!("{topic}: {vb} — line {}: {}", lb.line, lb.text),
                                contents[b].0.path.display().to_string(),
                                Confidence::Probable,
                            ),
                        ],
                    });
                }
            }
        }
    }
    // Imports.
    let path_index: HashMap<PathBuf, usize> = contents
        .iter()
        .enumerate()
        .map(|(i, (f, _))| {
            (
                agasthy_core::paths::canonical(&f.path).unwrap_or_else(|_| f.path.clone()),
                i,
            )
        })
        .collect();
    let mut references: Vec<Vec<String>> = vec![Vec::new(); contents.len()];
    for (i, (f, c)) in contents.iter().enumerate() {
        for imp in &c.imports {
            let target = resolve_import(&f.path, imp, home);
            if !target.is_file() {
                continue;
            }
            let canon = agasthy_core::paths::canonical(&target).unwrap_or(target.clone());
            references[i].push(canon.display().to_string());
            if let Some(&j) = path_index.get(&canon) {
                batch.relationships.push(rel(
                    &ids[i],
                    &ids[j],
                    RelationKind::References,
                    Confidence::Verified,
                    vec![ev(
                        "import",
                        format!("`@{imp}`"),
                        f.path.display().to_string(),
                        Confidence::Verified,
                    )],
                ));
            }
        }
    }

    // ----- file entities -----
    for (i, (f, c)) in contents.into_iter().enumerate() {
        let installed_readers: Vec<&str> = f
            .tools
            .iter()
            .copied()
            .filter(|t| detected.get(t).is_some_and(|a| a.installed))
            .collect();
        for t in f.tools.iter().filter(|t| detected.contains_key(*t)) {
            batch.relationships.push(rel(
                &ids[i],
                &tool_id(t),
                RelationKind::ConfiguresTool,
                Confidence::Strong,
                vec![ev(
                    "naming_convention",
                    format!("`{}` is read by {t} by convention", f.rel),
                    f.path.display().to_string(),
                    Confidence::Strong,
                )],
            ));
        }
        if let Some(project) = &f.project {
            batch.relationships.push(rel(
                project,
                &ids[i],
                RelationKind::Contains,
                Confidence::Verified,
                vec![ev(
                    "file_location",
                    format!("at {}", f.rel),
                    f.path.display().to_string(),
                    Confidence::Verified,
                )],
            ));
        }
        let file_name = f
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // Skills are identified by their folder (`pdf/SKILL.md`), not the file name.
        let name = match (f.file_type, f.path.parent().and_then(|p| p.file_name())) {
            (FileType::Skill, Some(dir)) => format!("{}/{}", dir.to_string_lossy(), file_name),
            _ => file_name,
        };
        batch.entities.push(EntityInput {
            id: ids[i].clone(),
            kind: EntityKind::AiConfigFile,
            name,
            path: Some(f.path.display().to_string()),
            version: None,
            source: "ai".into(),
            evidence: vec![ev(
                "ai_file",
                format!("{} file ({} scope)", f.file_type.as_str(), f.scope),
                f.path.display().to_string(),
                Confidence::Verified,
            )],
            attributes: serde_json::to_value(AiConfigFileAttrs {
                file_type: f.file_type.as_str().into(),
                scope: f.scope.into(),
                tools: f.tools.iter().map(|t| t.to_string()).collect(),
                project: f.project_path.as_ref().map(|p| p.display().to_string()),
                relative_path: f.rel.clone(),
                size_bytes: c.size,
                modified_at: c.modified_at,
                sha256: c.sha256,
                lines: c.lines,
                headings: c.headings,
                text: c.text,
                text_truncated: c.text_truncated,
                redactions: c.redactions,
                orphaned: installed_readers.is_empty(),
                duplicates: duplicates[i].clone(),
                overlaps: overlaps[i].clone(),
                directives: c.directives,
                references: references[i].clone(),
            })
            .unwrap_or_default(),
        });
    }

    for kind in [
        EntityKind::AiTool,
        EntityKind::AiProvider,
        EntityKind::LocalModel,
        EntityKind::McpServer,
        EntityKind::AiConfigFile,
    ] {
        batch.prune.push(PruneScope { kind, source: None });
    }
    step("done");
    batch
}

/// Installed editor extensions: (editor, `publisher.name`, version, path).
/// Read from extension folder names (`publisher.name-1.2.3[-platform]`).
fn editor_extensions(home: &Path) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for (editor, dir) in [
        ("VS Code", ".vscode/extensions"),
        ("VS Code Insiders", ".vscode-insiders/extensions"),
        ("Cursor", ".cursor/extensions"),
        ("Windsurf", ".windsurf/extensions"),
    ] {
        let Ok(rd) = std::fs::read_dir(home.join(dir)) else {
            continue;
        };
        for e in rd.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            // Split at the first "-<digit>" which starts the version.
            let split = name
                .char_indices()
                .find(|(i, c)| *c == '-' && name[i + 1..].starts_with(|d: char| d.is_ascii_digit()))
                .map(|(i, _)| i);
            let Some(i) = split else { continue };
            let id = name[..i].to_string();
            let version = name[i + 1..].split('-').next().unwrap_or("").to_string();
            out.push((
                editor.to_string(),
                id,
                version,
                e.path().display().to_string(),
            ));
        }
    }
    out
}

pub fn file_entity_id(path: &Path) -> String {
    format!("ai_file:{}", path.display())
}

/// The directory an instruction file applies to.
fn applies_to_dir(f: &FoundFile) -> PathBuf {
    let root = f.project_path.clone().unwrap_or_default();
    let first = f.rel.split('/').next().unwrap_or("");
    if catalog::AI_DIRS.contains(&first) {
        root
    } else {
        f.path.parent().map(Path::to_path_buf).unwrap_or(root)
    }
}

/// Topics where each file states exactly one value and the values differ.
fn conflicts<'a>(
    a: &'a [Directive],
    b: &'a [Directive],
) -> Vec<(String, String, String, &'a Directive, &'a Directive)> {
    let single = |ds: &'a [Directive]| -> BTreeMap<&'a str, Option<&'a Directive>> {
        let mut m: BTreeMap<&str, Option<&Directive>> = BTreeMap::new();
        for d in ds {
            m.entry(d.topic.as_str())
                .and_modify(|cur| {
                    if cur.is_some_and(|c| c.value != d.value) {
                        *cur = None;
                    }
                })
                .or_insert(Some(d));
        }
        m
    };
    let (sa, sb) = (single(a), single(b));
    let mut out = Vec::new();
    for (topic, da) in &sa {
        if let (Some(da), Some(Some(db))) = (da, sb.get(topic))
            && da.value != db.value
        {
            out.push((
                topic.to_string(),
                da.value.clone(),
                db.value.clone(),
                *da,
                *db,
            ));
        }
    }
    out
}

/// Provider names referenced in a tool's settings file. Only names and keys
/// are inspected; values of credential-like keys are never used.
fn provider_refs_in_config(rel: &str, raw: &str) -> Vec<(&'static str, ProviderLevel, String)> {
    let mut out = Vec::new();
    if rel.ends_with("config.toml") {
        if let Ok(t) = raw.parse::<toml::Table>() {
            if let Some(p) = t
                .get("model_provider")
                .and_then(|v| v.as_str())
                .and_then(provider_from_name)
            {
                out.push((
                    p,
                    ProviderLevel::Configured,
                    "`model_provider` names this provider".to_string(),
                ));
            }
            if let Some(mp) = t.get("model_providers").and_then(|v| v.as_table()) {
                for (name, cfg) in mp {
                    let pid = provider_from_name(name).or_else(|| {
                        cfg.get("name")
                            .and_then(|n| n.as_str())
                            .and_then(provider_from_name)
                    });
                    if let Some(pid) = pid {
                        let level = if cfg.get("base_url").is_some() {
                            ProviderLevel::EndpointConfigured
                        } else {
                            ProviderLevel::Configured
                        };
                        out.push((pid, level, format!("`[model_providers.{name}]` configured")));
                    }
                }
            }
        }
    } else if rel.ends_with(".json")
        && !rel.ends_with(".claude.json")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(raw)
        && let Some(env) = v.get("env").and_then(|e| e.as_object())
    {
        for key in env.keys() {
            for p in PROVIDERS {
                if p.credential_env.contains(&key.as_str()) {
                    out.push((
                        p.id,
                        ProviderLevel::CredentialPresent,
                        format!("`{key}` set in the tool's env settings (value not stored)"),
                    ));
                } else if p.endpoint_env.contains(&key.as_str()) {
                    out.push((
                        p.id,
                        ProviderLevel::EndpointConfigured,
                        format!("`{key}` set in the tool's env settings"),
                    ));
                }
            }
        }
    }
    out
}

// These tests build executable stubs in a temporary home, so they are Unix-only.
#[cfg(all(test, unix))]
mod tests;
