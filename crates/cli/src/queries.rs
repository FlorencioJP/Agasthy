//! Read-only questions about the machine, answered as JSON. Shared by the
//! command line and the MCP server so both give the same answers.

use std::path::PathBuf;

use agasthy_core::Store;
use serde_json::{Value, json};

/// Where the desktop app keeps its database (Tauri's app data directory).
pub fn app_db_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("org.agasthy.desktop").join("agasthy.db"))
}

/// The database to read: `db` if given, otherwise the app's database.
pub fn read_store(db: &Option<PathBuf>) -> Result<Store, String> {
    let path = match db {
        Some(p) => p.clone(),
        None => app_db_path().ok_or("cannot find the user data directory")?,
    };
    Store::open_read_only(&path).map_err(|e| {
        if db.is_none() {
            format!("{e}. Open the AGASTHY app and run a scan first, or pass --db FILE.")
        } else {
            e.to_string()
        }
    })
}

/// Repositories from the database, for linking processes to projects.
/// Missing or unreadable database: no links, not an error.
pub fn known_repos(db: &Option<PathBuf>) -> Vec<(String, String, PathBuf, u32)> {
    let Ok(store) = read_store(db) else {
        return Vec::new();
    };
    store
        .list_entities(&agasthy_core::EntityQuery {
            kind: Some(agasthy_core::EntityKind::Repository),
            text: None,
            limit: Some(5000),
            offset: None,
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let d = e
                .attributes
                .get("clones_of_same_remote")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            e.path.map(|p| (e.id, e.name, PathBuf::from(p), d))
        })
        .collect()
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn repo_refs(db: &Option<PathBuf>) -> Vec<agasthy_process::RepoRef> {
    known_repos(db)
        .into_iter()
        .map(|(i, n, p, _)| (i, n, p))
        .collect()
}

fn runner() -> agasthy_platform::SystemCommandRunner {
    let platform = agasthy_platform::current();
    agasthy_platform::system_runner(platform.as_ref(), &agasthy_discovery::catalog::tool_ids())
}

pub fn status(db: &Option<PathBuf>) -> Result<Value, String> {
    let store = read_store(db)?;
    let now = agasthy_core::time::now_ms();
    let scans: Vec<Value> = store
        .latest_scans()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| {
            let at = s.finished_at.unwrap_or(s.started_at);
            json!({
                "kind": s.kind,
                "status": s.status,
                "finished_at": s.finished_at,
                "age_hours": ((now - at) as f64 / 3_600_000.0 * 10.0).round() / 10.0,
                "entities_found": s.entities_found,
            })
        })
        .collect();
    Ok(json!({
        "database": store.path(),
        "last_scans": scans,
        "entities_by_kind": store.counts_by_kind().map_err(|e| e.to_string())?,
        "note": "Data is as of each scan's time. Scans run from the AGASTHY app.",
    }))
}

pub fn find(
    db: &Option<PathBuf>,
    text: Option<String>,
    kind: Option<&str>,
    limit: u32,
    full: bool,
) -> Result<Value, String> {
    let kind = match kind {
        Some(k) => {
            Some(agasthy_core::EntityKind::parse(k).ok_or_else(|| format!("unknown kind `{k}`"))?)
        }
        None => None,
    };
    let limit = limit.clamp(1, 500);
    let store = read_store(db)?;
    let found = store
        .list_entities(&agasthy_core::EntityQuery {
            kind,
            text: text.filter(|t| !t.trim().is_empty()),
            limit: Some(limit),
            offset: None,
        })
        .map_err(|e| e.to_string())?;
    let items: Vec<Value> = found
        .iter()
        .map(|e| {
            if full {
                serde_json::to_value(e).unwrap_or_default()
            } else {
                json!({
                    "id": e.id, "kind": e.kind, "name": e.name,
                    "version": e.version, "path": e.path, "source": e.source,
                })
            }
        })
        .collect();
    Ok(json!({ "count": items.len(), "limit": limit, "entities": items }))
}

pub fn entity(db: &Option<PathBuf>, id: &str) -> Result<Value, String> {
    let store = read_store(db)?;
    let detail = store
        .entity_detail(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no entity with id `{id}`; find ids with `find`"))?;
    to_value(detail)
}

pub fn impact(db: &Option<PathBuf>, id: &str) -> Result<Value, String> {
    let store = read_store(db)?;
    to_value(agasthy_graph::removal_impact(&store, id).map_err(|e| e.to_string())?)
}

pub fn shared(db: &Option<PathBuf>) -> Result<Value, String> {
    let store = read_store(db)?;
    to_value(agasthy_graph::shared_dependencies(&store, 25).map_err(|e| e.to_string())?)
}

/// Live coding-agent sessions and processes they left running.
pub fn agents(db: &Option<PathBuf>) -> Result<Value, String> {
    let runner = runner();
    let refs = repo_refs(db);
    let mut m = agasthy_process::ProcessMonitor::new();
    let mut tracker = agasthy_process::agents::AgentTracker::new();
    tracker.update(&m.snapshot(&runner, &refs));
    std::thread::sleep(std::time::Duration::from_millis(400));
    to_value(tracker.update(&m.snapshot(&runner, &refs)))
}

/// Processes with their ports and projects. With `limit`, only the largest
/// by memory are listed (and the total is reported).
pub fn processes(db: &Option<PathBuf>, limit: Option<usize>) -> Result<Value, String> {
    let runner = runner();
    let refs = repo_refs(db);
    let mut m = agasthy_process::ProcessMonitor::new();
    m.snapshot(&runner, &refs);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let mut snap = m.snapshot(&runner, &refs);
    let total = snap.processes.len();
    if let Some(n) = limit {
        snap.processes
            .sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
        snap.processes.truncate(n);
    }
    let mut v = to_value(snap)?;
    if let Some(o) = v.as_object_mut() {
        o.insert("total_processes".into(), json!(total));
    }
    Ok(v)
}

/// Token usage from coding agents' local logs (counters only).
pub fn usage(days: u32, full: bool) -> Result<Value, String> {
    let platform = agasthy_platform::current();
    let home = platform.home_dir().ok_or("no home directory")?;
    let r = agasthy_discovery::ai::usage::UsageIndex::new().refresh(
        &home,
        days.clamp(1, 90),
        agasthy_core::time::now_ms(),
    );
    if full {
        return to_value(&r);
    }
    let mut by_agent: std::collections::BTreeMap<
        String,
        (u64, agasthy_discovery::ai::usage::TokenCounts, Option<f64>),
    > = Default::default();
    for s in &r.sessions {
        let e = by_agent.entry(s.agent.clone()).or_default();
        e.0 += 1;
        e.1.add(&s.tokens);
        if let Some(c) = s.estimated_cost_usd {
            e.2 = Some(e.2.unwrap_or(0.0) + c);
        }
    }
    Ok(json!({
        "window_days": r.window_days,
        "sources": r.sources,
        "problems": r.problems,
        "by_agent": by_agent.into_iter().map(|(a, (n, t, c))| json!({
            "agent": a,
            "sessions": n,
            "tokens": t,
            "total_tokens": t.total(),
            "estimated_cost_usd": c.map(|c| (c * 100.0).round() / 100.0),
            "cost_note": if c.is_none() { "no price data for these models" } else { "API list prices; subscription plans are billed differently" },
        })).collect::<Vec<_>>(),
    }))
}

/// Memory, CPU, load and disk right now.
pub fn telemetry() -> Result<Value, String> {
    let mut m = agasthy_system_monitor::Monitor::new();
    m.sample();
    std::thread::sleep(std::time::Duration::from_millis(600));
    to_value(m.sample())
}
