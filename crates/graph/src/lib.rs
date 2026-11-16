//! The machine intelligence graph.
//!
//! Built directly from the entities and evidence-backed relationships in the
//! store, never from a separate, hand-maintained model. This crate provides:
//!
//! * graph views for the UI ([`neighborhood`], [`overview`]),
//! * "what depends on this?" lists ([`shared_dependencies`]),
//! * runtime impact analysis ([`runtime_impact`]): what changes for each
//!   project if a runtime copy is removed, with evidence and explicit unknowns.

pub mod version_req;

use std::collections::{BTreeSet, HashMap, VecDeque};

use agasthy_core::{
    CoreResult, Edge, Entity, EntityKind, EntityQuery, EntityRef, RelationKind, Store,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use version_req::satisfies;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphNode {
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    /// Short secondary label: version, ecosystem or path.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphView {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<Edge>,
    /// The view hit its node limit; some neighbours are not shown.
    pub truncated: bool,
    pub center: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OverviewKind {
    /// Repositories, the runtimes they require, shared dependencies, shared upstreams.
    Projects,
    /// Package managers, their packages, and the binaries and apps those provide.
    Software,
    /// AI assistants, providers, MCP servers, local models and instruction files.
    Ai,
}

/// Infer an entity kind from its id prefix (ids are deterministic).
pub fn kind_of_id(id: &str) -> Option<EntityKind> {
    let prefix = id.split(':').next()?;
    Some(match prefix {
        "repository" => EntityKind::Repository,
        "remote" => EntityKind::RemoteProject,
        "application" => EntityKind::Application,
        "binary" => EntityKind::Binary,
        "runtime" => EntityKind::Runtime,
        "package" => EntityKind::Package,
        "package_manager" => EntityKind::PackageManager,
        "dependency" => EntityKind::Dependency,
        "ai_tool" => EntityKind::AiTool,
        "ai_provider" => EntityKind::AiProvider,
        "local_model" => EntityKind::LocalModel,
        "mcp" => EntityKind::McpServer,
        "ai_file" => EntityKind::AiConfigFile,
        _ => return None,
    })
}

fn node_from(e: &Entity) -> GraphNode {
    let detail = match e.kind {
        EntityKind::Dependency => e
            .attributes
            .get("ecosystem")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        EntityKind::Repository => e
            .attributes
            .get("primary_language")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => e.version.clone(),
    };
    GraphNode {
        id: e.id.clone(),
        kind: e.kind,
        name: e.name.clone(),
        detail,
    }
}

fn build_view(
    store: &Store,
    ids: Vec<String>,
    truncated: bool,
    center: Option<String>,
) -> CoreResult<GraphView> {
    let mut nodes: Vec<GraphNode> = store.entities_by_ids(&ids)?.iter().map(node_from).collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    let present: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
    let mut edges = store.edges_among(&present)?;
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(GraphView {
        nodes,
        edges,
        truncated,
        center,
    })
}

/// Nodes within `depth` hops of `center`, optionally restricted to `kinds`
/// (the center is always included). Package managers are not expanded through
/// unless they are the center, because they connect to every package.
pub fn neighborhood(
    store: &Store,
    center: &str,
    depth: u32,
    kinds: &[EntityKind],
    max_nodes: usize,
) -> CoreResult<GraphView> {
    let allowed = |id: &str| kinds.is_empty() || kind_of_id(id).is_some_and(|k| kinds.contains(&k));
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    let mut truncated = false;
    seen.insert(center.to_string());
    order.push(center.to_string());
    queue.push_back((center.to_string(), 0));
    while let Some((id, d)) = queue.pop_front() {
        if d >= depth {
            continue;
        }
        if id != center && kind_of_id(&id) == Some(EntityKind::PackageManager) {
            continue;
        }
        let mut edges = store.edges_touching(&id)?;
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        for e in edges {
            let other = if e.from_id == id { e.to_id } else { e.from_id };
            if seen.contains(&other) || !allowed(&other) {
                continue;
            }
            if order.len() >= max_nodes {
                truncated = true;
                break;
            }
            seen.insert(other.clone());
            order.push(other.clone());
            queue.push_back((other, d + 1));
        }
    }
    build_view(store, order, truncated, Some(center.to_string()))
}

/// A whole-machine view of one aspect, bounded by `max_nodes`.
pub fn overview(store: &Store, kind: OverviewKind, max_nodes: usize) -> CoreResult<GraphView> {
    let mut ids: Vec<String> = Vec::new();
    let all = |k: EntityKind| -> CoreResult<Vec<String>> {
        Ok(store
            .list_entities(&EntityQuery {
                kind: Some(k),
                text: None,
                limit: Some(max_nodes as u32),
                offset: None,
            })?
            .into_iter()
            .map(|e| e.id)
            .collect())
    };
    match kind {
        OverviewKind::Projects => {
            ids.extend(all(EntityKind::Repository)?);
            // Runtimes that at least one project requires.
            let required: BTreeSet<String> = store
                .edges_of_kind(RelationKind::RequiresRuntime, None)?
                .into_iter()
                .map(|e| e.to_id)
                .collect();
            ids.extend(required);
            // Dependencies shared by two or more projects.
            ids.extend(
                store
                    .most_referenced(RelationKind::DependsOn, 80)?
                    .into_iter()
                    .filter(|(_, n)| *n >= 2)
                    .map(|(id, _)| id),
            );
            // Upstream projects with more than one local link (duplicates, forks).
            let mut remote_links: HashMap<String, u32> = HashMap::new();
            for k in [RelationKind::OriginRemote, RelationKind::TracksRemote] {
                for e in store.edges_of_kind(k, None)? {
                    *remote_links.entry(e.to_id).or_default() += 1;
                }
            }
            ids.extend(
                remote_links
                    .into_iter()
                    .filter(|(_, n)| *n >= 2)
                    .map(|(id, _)| id),
            );
        }
        OverviewKind::Ai => {
            for k in [
                EntityKind::AiTool,
                EntityKind::AiProvider,
                EntityKind::McpServer,
                EntityKind::LocalModel,
            ] {
                ids.extend(all(k)?);
            }
            // Instruction files and the projects that contain them.
            for k in [
                RelationKind::ConfiguresTool,
                RelationKind::Contains,
                RelationKind::DeclaresMcpServer,
            ] {
                for e in store.edges_of_kind(k, None)? {
                    ids.push(e.from_id);
                    ids.push(e.to_id);
                }
            }
        }
        OverviewKind::Software => {
            ids.extend(all(EntityKind::PackageManager)?);
            ids.extend(all(EntityKind::Runtime)?);
            let linked: BTreeSet<String> = [RelationKind::InstalledBy, RelationKind::Provides]
                .into_iter()
                .map(|k| store.edges_of_kind(k, None))
                .collect::<CoreResult<Vec<_>>>()?
                .into_iter()
                .flatten()
                .flat_map(|e| [e.from_id, e.to_id])
                .collect();
            ids.extend(linked);
        }
    }
    let mut dedup = BTreeSet::new();
    ids.retain(|id| dedup.insert(id.clone()));
    let truncated = ids.len() > max_nodes;
    ids.truncate(max_nodes);
    build_view(store, ids, truncated, None)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SharedDependency {
    pub dependency: EntityRef,
    pub ecosystem: Option<String>,
    pub projects: u32,
}

/// Dependencies declared by the most projects.
pub fn shared_dependencies(store: &Store, limit: u32) -> CoreResult<Vec<SharedDependency>> {
    let top = store.most_referenced(RelationKind::DependsOn, limit)?;
    let ids: Vec<String> = top.iter().map(|(id, _)| id.clone()).collect();
    let by_id: HashMap<String, Entity> = store
        .entities_by_ids(&ids)?
        .into_iter()
        .map(|e| (e.id.clone(), e))
        .collect();
    Ok(top
        .into_iter()
        .filter_map(|(id, n)| {
            let e = by_id.get(&id)?;
            Some(SharedDependency {
                dependency: EntityRef {
                    id: e.id.clone(),
                    kind: e.kind,
                    name: e.name.clone(),
                    path: None,
                },
                ecosystem: e
                    .attributes
                    .get("ecosystem")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                projects: n,
            })
        })
        .collect())
}

// ----- runtime impact -----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeCopyView {
    pub id: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub active: bool,
    pub rank: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ImpactOutcome {
    /// After removal, nothing on PATH satisfies the project's requirement.
    Breaks,
    /// Another copy takes over and satisfies the requirement.
    FallsBack,
    /// Resolution would not change (the copy is shadowed).
    Unaffected,
    /// This copy does not satisfy the requirement today either.
    AlreadyIncompatible,
    /// Not enough information: constraint or version cannot be evaluated.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImpactItem {
    pub project: EntityRef,
    /// Constraints as declared (manifest: constraint).
    pub requirements: Vec<String>,
    pub outcome: ImpactOutcome,
    pub explanation: String,
    pub fallback: Option<RuntimeCopyView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImpactReport {
    pub target: RuntimeCopyView,
    pub runtime: String,
    pub other_copies: Vec<RuntimeCopyView>,
    pub items: Vec<ImpactItem>,
    pub breaks: u32,
    pub falls_back: u32,
    pub unaffected: u32,
    pub unknown: u32,
    /// What this analysis cannot see.
    pub limitations: Vec<String>,
}

fn family_of_tool(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "node" => "node",
        "python3" | "python" => "python",
        "rustc" => "rust",
        "go" => "go",
        "ruby" => "ruby",
        _ => return None,
    })
}

fn copy_view(e: &Entity) -> RuntimeCopyView {
    let active = e
        .attributes
        .get("active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let rank = e
        .attributes
        .get("rank")
        .and_then(|v| v.as_u64())
        .map(|r| r as u32)
        .unwrap_or(if active { 0 } else { 1 });
    RuntimeCopyView {
        id: e.id.clone(),
        path: e.path.clone(),
        version: e.version.clone(),
        active,
        rank,
    }
}

/// `Some(true)` if every explicit constraint is met, `Some(false)` if any is
/// not, `None` if any cannot be evaluated. `Ok(None)` inner = no explicit constraint.
fn evaluate(version: Option<&str>, constraints: &[String]) -> Option<Option<bool>> {
    if constraints.is_empty() {
        return Some(None);
    }
    let v = version?;
    let mut all = true;
    for c in constraints {
        match satisfies(v, c, false) {
            Some(true) => {}
            Some(false) => all = false,
            None => return None,
        }
    }
    Some(Some(all))
}

/// What happens to each project that requires this runtime family if the copy
/// `runtime_id` is removed from PATH.
pub fn runtime_impact(store: &Store, runtime_id: &str) -> CoreResult<Option<ImpactReport>> {
    let Some(target) = store.get_entity(runtime_id)? else {
        return Ok(None);
    };
    let tool = target
        .attributes
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let Some(family) = family_of_tool(tool) else {
        return Ok(None);
    };
    let mut copies: Vec<RuntimeCopyView> = store
        .list_entities(&EntityQuery {
            kind: Some(EntityKind::Runtime),
            text: None,
            limit: Some(1000),
            offset: None,
        })?
        .iter()
        .filter(|e| {
            e.attributes
                .get("tool")
                .and_then(|v| v.as_str())
                .and_then(family_of_tool)
                == Some(family)
        })
        .map(copy_view)
        .collect();
    copies.sort_by_key(|c| (c.rank, c.id.clone()));
    let target_view = copy_view(&target);
    let others: Vec<RuntimeCopyView> = copies
        .iter()
        .filter(|c| c.id != target.id)
        .cloned()
        .collect();
    // Which copy would run after removal: the best-ranked remaining one.
    let successor = others.first().cloned();
    // Is the target the copy that runs today (first in order among all copies)?
    let target_runs = copies.first().map(|c| c.id == target.id).unwrap_or(false);

    let edges = store.edges_of_kind(RelationKind::RequiresRuntime, Some(runtime_id))?;
    let project_ids: Vec<String> = edges.iter().map(|e| e.from_id.clone()).collect();
    let projects: HashMap<String, Entity> = store
        .entities_by_ids(&project_ids)?
        .into_iter()
        .map(|e| (e.id.clone(), e))
        .collect();

    let mut items = Vec::new();
    for edge in &edges {
        let Some(p) = projects.get(&edge.from_id) else {
            continue;
        };
        let decls = edge
            .attributes
            .get("declarations")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        let constraints: Vec<String> = decls
            .iter()
            .filter_map(|d| {
                d.get("constraint")
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
            })
            .collect();
        let requirements: Vec<String> = decls
            .iter()
            .map(|d| {
                let m = d.get("manifest").and_then(|v| v.as_str()).unwrap_or("?");
                match d.get("constraint").and_then(|c| c.as_str()) {
                    Some(c) => format!("{m}: {c}"),
                    None => format!("{m}: any version (implied)"),
                }
            })
            .collect();

        let target_ok = evaluate(target_view.version.as_deref(), &constraints);
        let (outcome, explanation, fallback) = if !target_runs {
            (
                ImpactOutcome::Unaffected,
                format!(
                    "This copy is shadowed{}; removing it does not change which {family} runs.",
                    copies
                        .first()
                        .and_then(|c| c.path.as_ref())
                        .map(|p| format!(" by {p}"))
                        .unwrap_or_default()
                ),
                None,
            )
        } else if target_ok == Some(Some(false)) {
            (
                ImpactOutcome::AlreadyIncompatible,
                format!(
                    "The current {family} {} does not satisfy this project's requirement today.",
                    target_view.version.as_deref().unwrap_or("?")
                ),
                None,
            )
        } else {
            match &successor {
                None => (
                    ImpactOutcome::Breaks,
                    format!("No other {family} is on PATH; the project could not run its {family} tooling."),
                    None,
                ),
                Some(next) => match evaluate(next.version.as_deref(), &constraints) {
                    Some(Some(true)) => (
                        ImpactOutcome::FallsBack,
                        format!(
                            "{} {} would run instead and satisfies the requirement.",
                            next.path.as_deref().unwrap_or("Another copy"),
                            next.version.as_deref().unwrap_or("")
                        ),
                        Some(next.clone()),
                    ),
                    Some(Some(false)) => (
                        ImpactOutcome::Breaks,
                        format!(
                            "{} {} would run instead but does not satisfy the requirement.",
                            next.path.as_deref().unwrap_or("Another copy"),
                            next.version.as_deref().unwrap_or("")
                        ),
                        Some(next.clone()),
                    ),
                    Some(None) => (
                        ImpactOutcome::Unknown,
                        format!(
                            "{} {} would run instead. The project declares no version, so compatibility is unknown.",
                            next.path.as_deref().unwrap_or("Another copy"),
                            next.version.as_deref().unwrap_or("")
                        ),
                        Some(next.clone()),
                    ),
                    None => (
                        ImpactOutcome::Unknown,
                        "The requirement or the fallback's version cannot be evaluated (for example `lts/*` or `stable`).".to_string(),
                        Some(next.clone()),
                    ),
                },
            }
        };
        items.push(ImpactItem {
            project: EntityRef {
                id: p.id.clone(),
                kind: p.kind,
                name: p.name.clone(),
                path: p.path.clone(),
            },
            requirements,
            outcome,
            explanation,
            fallback,
        });
    }
    let order = |o: ImpactOutcome| match o {
        ImpactOutcome::Breaks => 0,
        ImpactOutcome::Unknown => 1,
        ImpactOutcome::FallsBack => 2,
        ImpactOutcome::AlreadyIncompatible => 3,
        ImpactOutcome::Unaffected => 4,
    };
    items.sort_by(|a, b| {
        order(a.outcome)
            .cmp(&order(b.outcome))
            .then(a.project.name.cmp(&b.project.name))
    });
    let count = |o: ImpactOutcome| items.iter().filter(|i| i.outcome == o).count() as u32;
    Ok(Some(ImpactReport {
        breaks: count(ImpactOutcome::Breaks),
        falls_back: count(ImpactOutcome::FallsBack),
        unaffected: count(ImpactOutcome::Unaffected) + count(ImpactOutcome::AlreadyIncompatible),
        unknown: count(ImpactOutcome::Unknown),
        target: target_view,
        runtime: family.into(),
        other_copies: others,
        items,
        limitations: vec![
            "Only projects in your scanned folders are considered.".into(),
            "Resolution order is AGASTHY's search order; a shell with a different PATH, or a version manager such as nvm or pyenv, may pick a different copy.".into(),
            "Global tools, editors and services that use this runtime are not yet analysed.".into(),
        ],
    }))
}

#[cfg(test)]
mod tests;
