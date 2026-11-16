//! `agasthy` — headless discovery.
//!
//! Prints JSON to stdout. Read-only: it never modifies the machine. With
//! `--db <file>` the scan commands also write their results to an AGASTHY
//! database at that path. The query commands (`status`, `find`, `entity`,
//! `impact`, `shared`) read the desktop app's database read-only by default,
//! so agents and scripts can use what the app has already scanned.

use std::path::PathBuf;
use std::process::ExitCode;

use agasthy_core::{IngestBatch, Store};
use agasthy_discovery::{
    RepoScanOptions, catalog, scan_packages, scan_repositories, scan_software, validate_scan_root,
};
use agasthy_system_monitor::Monitor;

const USAGE: &str = "\
agasthy — read-only developer machine discovery

QUERY WHAT THE APP ALREADY SCANNED (reads the AGASTHY app's database read-only;
--db FILE reads another database instead):
    agasthy status                                (database, last scans and their age, counts)
    agasthy find [text] [--kind KIND] [--limit N] (search repositories, packages, tools, ...)
    agasthy entity <entity-id>                    (one entity with its evidence and relationships)
    agasthy impact <entity-id>                    (what if it were removed?)
    agasthy shared                                (dependencies shared by the most projects)

LIVE (nothing stored):
    agasthy agents                         (coding-agent sessions and what they left running)
    agasthy processes                      (processes, ports and the project each belongs to)
    agasthy usage [--days N] [--full]      (coding-agent token usage from their local logs; counters only)
    agasthy telemetry
    agasthy platform

SCANS (with --db FILE, results are also written to that database):
    agasthy scan <folder>... [--depth N] [--db FILE] [--full]
    agasthy software [--db FILE] [--full]
    agasthy ai [--db FILE] [--full]
    agasthy packages [--db FILE] [--full]
    agasthy disk [folder...]               (read-only cleanup analysis; folders act as approved roots)
    agasthy security --db FILE             (local checks only; never contacts OSV; writes findings to FILE)

    agasthy --version

Output is JSON. Without --full, a summary is printed instead of every entity.
KINDS: repository, application, tool, runtime, package, package_manager, ai_tool,
       ai_provider, mcp_server, ai_config_file, finding, ...
";

/// Where the desktop app keeps its database (Tauri's app data directory).
fn app_db_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("org.agasthy.desktop").join("agasthy.db"))
}

/// The database to read: `--db FILE` if given, otherwise the app's database.
fn read_store(db: &Option<PathBuf>) -> Result<Store, String> {
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
fn known_repos(db: &Option<PathBuf>) -> Vec<(String, String, PathBuf, u32)> {
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

fn print_json(v: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn take_opt(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}

fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    match args.iter().position(|a| a == name) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    }
}

fn run(raw: &[String]) -> Result<(), String> {
    let mut args = raw.to_vec();
    if args.is_empty() || take_flag(&mut args, "--help") || take_flag(&mut args, "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    if take_flag(&mut args, "--version") || take_flag(&mut args, "-V") {
        println!("agasthy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let db = take_opt(&mut args, "--db").map(PathBuf::from);
    let full = take_flag(&mut args, "--full");
    if args.is_empty() {
        return Err(format!("missing command\n\n{USAGE}"));
    }
    let cmd = args.remove(0);
    let platform = agasthy_platform::current();
    let runner = agasthy_platform::system_runner(platform.as_ref(), &catalog::tool_ids());

    let batch = match cmd.as_str() {
        "scan" => {
            let depth = take_opt(&mut args, "--depth")
                .map(|d| {
                    d.parse::<usize>()
                        .map_err(|_| "--depth must be a number".to_string())
                })
                .transpose()?
                .unwrap_or(6);
            if args.is_empty() {
                return Err("scan needs at least one folder".into());
            }
            let mut roots = Vec::new();
            for a in &args {
                let p =
                    validate_scan_root(platform.as_ref(), a).map_err(|e| format!("{a}: {e}"))?;
                roots.push((p, depth));
            }
            let mut opts = RepoScanOptions::new(roots);
            opts.runtimes = agasthy_discovery::runtime_copies(platform.as_ref());
            scan_repositories(&opts, &runner, &|p| {
                if p.phase == "searching" {
                    eprintln!("searching {}", p.current.unwrap_or_default());
                }
            })
        }
        "software" => scan_software(platform.as_ref(), &runner, &agasthy_discovery::no_progress),
        "packages" => scan_packages(platform.as_ref(), &runner, &agasthy_discovery::no_progress),
        "ai" => {
            let projects = known_repos(&db)
                .into_iter()
                .map(|(id, _, p, _)| (id, p))
                .collect();
            let home = platform.home_dir().ok_or("no home directory")?;
            let opts = agasthy_discovery::AiScanOptions::new(home, projects);
            agasthy_discovery::scan_ai(platform.as_ref(), &opts, &agasthy_discovery::no_progress)
        }
        "processes" | "disk" | "agents" => {
            let repos = known_repos(&db);
            let out = if cmd == "agents" {
                let mut m = agasthy_process::ProcessMonitor::new();
                let refs: Vec<agasthy_process::RepoRef> = repos
                    .iter()
                    .map(|(i, n, p, _)| (i.clone(), n.clone(), p.clone()))
                    .collect();
                let mut tracker = agasthy_process::agents::AgentTracker::new();
                tracker.update(&m.snapshot(&runner, &refs));
                std::thread::sleep(std::time::Duration::from_millis(400));
                serde_json::to_value(tracker.update(&m.snapshot(&runner, &refs)))
            } else if cmd == "processes" {
                let mut m = agasthy_process::ProcessMonitor::new();
                let refs: Vec<agasthy_process::RepoRef> = repos
                    .iter()
                    .map(|(i, n, p, _)| (i.clone(), n.clone(), p.clone()))
                    .collect();
                m.snapshot(&runner, &refs);
                std::thread::sleep(std::time::Duration::from_millis(400));
                serde_json::to_value(m.snapshot(&runner, &refs))
            } else {
                let mut roots = Vec::new();
                for a in &args {
                    roots.push(
                        validate_scan_root(platform.as_ref(), a)
                            .map_err(|e| format!("{a}: {e}"))?,
                    );
                }
                let input = agasthy_cleanup::AnalysisInput {
                    home: platform.home_dir().ok_or("no home directory")?,
                    scan_roots: roots,
                    repos: repos.iter().map(|(_, _, p, d)| (p.clone(), *d)).collect(),
                };
                serde_json::to_value(agasthy_cleanup::analyze(&input, &runner))
            }
            .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        "telemetry" => {
            let mut m = Monitor::new();
            m.sample();
            std::thread::sleep(std::time::Duration::from_millis(600));
            let s = m.sample();
            println!(
                "{}",
                serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        "usage" => {
            let days: u32 = take_opt(&mut args, "--days")
                .and_then(|d| d.parse().ok())
                .unwrap_or(30);
            let home = platform.home_dir().ok_or("no home directory")?;
            let r = agasthy_discovery::ai::usage::UsageIndex::new().refresh(
                &home,
                days.clamp(1, 90),
                agasthy_core::time::now_ms(),
            );
            let out = if full {
                serde_json::to_value(&r).map_err(|e| e.to_string())?
            } else {
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
                serde_json::json!({
                    "window_days": r.window_days,
                    "sources": r.sources,
                    "problems": r.problems,
                    "by_agent": by_agent.into_iter().map(|(a, (n, t, c))| serde_json::json!({"agent": a, "sessions": n, "tokens": t, "total_tokens": t.total(), "estimated_cost_usd": c.map(|c| (c * 100.0).round() / 100.0), "cost_note": if c.is_none() { "no price data for these models" } else { "API list prices; subscription plans are billed differently" }})).collect::<Vec<_>>(),
                })
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        "security" => {
            let db = db.ok_or("this command needs --db FILE")?;
            let tmp = std::env::temp_dir().join("agasthy-cli");
            let input = agasthy_security::SecurityInput {
                osv_enabled: false,
                queries: vec![],
                unknown_origin: vec![],
                work_dir: &tmp,
            };
            let (batch, report) = agasthy_security::scan(&input, &runner);
            let mut store = Store::open(&db).map_err(|e| e.to_string())?;
            let id = store
                .begin_scan(batch.scan_kind)
                .map_err(|e| e.to_string())?;
            store.ingest(id, &batch).map_err(|e| e.to_string())?;
            let names: Vec<&String> = batch.entities.iter().map(|e| &e.name).collect();
            let out = serde_json::json!({ "report": report, "findings": names });
            println!(
                "{}",
                serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        "status" => {
            let store = read_store(&db)?;
            let now = agasthy_core::time::now_ms();
            let scans: Vec<serde_json::Value> = store
                .latest_scans()
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|s| {
                    let at = s.finished_at.unwrap_or(s.started_at);
                    serde_json::json!({
                        "kind": s.kind,
                        "status": s.status,
                        "finished_at": s.finished_at,
                        "age_hours": ((now - at) as f64 / 3_600_000.0 * 10.0).round() / 10.0,
                        "entities_found": s.entities_found,
                    })
                })
                .collect();
            return print_json(&serde_json::json!({
                "database": store.path(),
                "last_scans": scans,
                "entities_by_kind": store.counts_by_kind().map_err(|e| e.to_string())?,
                "note": "Data is as of each scan's time. Scans run from the AGASTHY app.",
            }));
        }
        "find" => {
            let kind = match take_opt(&mut args, "--kind") {
                Some(k) => Some(
                    agasthy_core::EntityKind::parse(&k)
                        .ok_or_else(|| format!("unknown kind `{k}`"))?,
                ),
                None => None,
            };
            let limit = take_opt(&mut args, "--limit")
                .map(|n| {
                    n.parse::<u32>()
                        .map_err(|_| "--limit must be a number".to_string())
                })
                .transpose()?
                .unwrap_or(50)
                .clamp(1, 500);
            let text = (!args.is_empty()).then(|| args.join(" "));
            let store = read_store(&db)?;
            let found = store
                .list_entities(&agasthy_core::EntityQuery {
                    kind,
                    text,
                    limit: Some(limit),
                    offset: None,
                })
                .map_err(|e| e.to_string())?;
            let items: Vec<serde_json::Value> = found
                .iter()
                .map(|e| {
                    if full {
                        serde_json::to_value(e).unwrap_or_default()
                    } else {
                        serde_json::json!({
                            "id": e.id, "kind": e.kind, "name": e.name,
                            "version": e.version, "path": e.path, "source": e.source,
                        })
                    }
                })
                .collect();
            return print_json(
                &serde_json::json!({ "count": items.len(), "limit": limit, "entities": items }),
            );
        }
        "entity" => {
            let id = args
                .first()
                .ok_or("entity needs an entity id (from `agasthy find`)")?;
            let store = read_store(&db)?;
            let detail = store
                .entity_detail(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no entity with id `{id}`"))?;
            return print_json(&serde_json::to_value(detail).map_err(|e| e.to_string())?);
        }
        "impact" | "shared" => {
            let store = read_store(&db)?;
            let out = if cmd == "impact" {
                let id = args.first().ok_or("impact needs an entity id")?;
                serde_json::to_value(
                    agasthy_graph::removal_impact(&store, id).map_err(|e| e.to_string())?,
                )
            } else {
                serde_json::to_value(
                    agasthy_graph::shared_dependencies(&store, 25).map_err(|e| e.to_string())?,
                )
            }
            .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        "platform" => {
            let info = platform.info();
            let dirs: Vec<serde_json::Value> = platform
                .executable_search_dirs()
                .into_iter()
                .map(|d| serde_json::json!({"path": d.path, "origin": d.origin}))
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"platform": info, "search_dirs": dirs})
                )
                .map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        other => return Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    if let Some(path) = db {
        let mut store = Store::open(&path).map_err(|e| e.to_string())?;
        let id = store
            .begin_scan(batch.scan_kind)
            .map_err(|e| e.to_string())?;
        let res = store.ingest(id, &batch).map_err(|e| e.to_string())?;
        eprintln!(
            "stored in {}: {} written, {} removed",
            path.display(),
            res.entities_written,
            res.entities_removed
        );
    }
    print_batch(&batch, full)
}

fn print_batch(batch: &IngestBatch, full: bool) -> Result<(), String> {
    let out = if full {
        serde_json::json!({
            "scan_kind": batch.scan_kind,
            "entities": batch.entities.iter().map(|e| serde_json::json!({
                "id": e.id, "kind": e.kind, "name": e.name, "path": e.path,
                "version": e.version, "source": e.source, "attributes": e.attributes,
                "evidence": e.evidence,
            })).collect::<Vec<_>>(),
            "relationships": batch.relationships.iter().map(|r| serde_json::json!({
                "kind": r.kind, "from": r.from_id, "to": r.to_id, "confidence": r.confidence,
            })).collect::<Vec<_>>(),
            "issues": batch.issues,
        })
    } else {
        let mut by_kind = std::collections::BTreeMap::<String, usize>::new();
        for e in &batch.entities {
            *by_kind.entry(e.kind.as_str().to_string()).or_default() += 1;
        }
        let mut rels = std::collections::BTreeMap::<String, usize>::new();
        for r in &batch.relationships {
            *rels.entry(r.kind.as_str().to_string()).or_default() += 1;
        }
        let sample: Vec<serde_json::Value> = batch
            .entities
            .iter()
            .take(15)
            .map(|e| serde_json::json!({"kind": e.kind, "name": e.name, "version": e.version, "path": e.path}))
            .collect();
        serde_json::json!({
            "scan_kind": batch.scan_kind,
            "entities_by_kind": by_kind,
            "relationships_by_kind": rels,
            "issues": batch.issues,
            "first_entities": sample,
        })
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
    );
    Ok(())
}
