//! `agasthy` — headless discovery.
//!
//! Prints JSON to stdout. Read-only: it never modifies the machine. With
//! `--db <file>` results are also written to an AGASTHY database at that path.

use std::path::PathBuf;
use std::process::ExitCode;

use agasthy_core::{IngestBatch, Store};
use agasthy_discovery::{
    RepoScanOptions, catalog, scan_packages, scan_repositories, scan_software, validate_scan_root,
};
use agasthy_system_monitor::Monitor;

const USAGE: &str = "\
agasthy — read-only developer machine discovery

USAGE:
    agasthy scan <folder>... [--depth N] [--db FILE] [--full]
    agasthy software [--db FILE] [--full]
    agasthy ai [--db FILE] [--full]        (projects come from repositories in --db)
    agasthy impact <entity-id> --db FILE          (what if it were removed?)
    agasthy security --db FILE                    (local checks only; never contacts OSV)
    agasthy shared --db FILE
    agasthy packages [--db FILE] [--full]
    agasthy telemetry
    agasthy usage [--days N] [--full]      (coding-agent token usage from their local logs; counters only)
    agasthy processes [--db FILE]          (read-only; --db links processes to repositories)
    agasthy disk [folder...] [--db FILE]   (read-only cleanup analysis; folders act as approved roots)
    agasthy platform

Output is JSON. Without --full, a summary is printed instead of every entity.
";

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
    let cmd = args.remove(0);
    let db = take_opt(&mut args, "--db").map(PathBuf::from);
    let full = take_flag(&mut args, "--full");
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
            let projects = match &db {
                Some(path) => Store::open(path)
                    .and_then(|s| {
                        s.list_entities(&agasthy_core::EntityQuery {
                            kind: Some(agasthy_core::EntityKind::Repository),
                            text: None,
                            limit: Some(5000),
                            offset: None,
                        })
                    })
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter_map(|e| e.path.map(|p| (e.id, PathBuf::from(p))))
                    .collect(),
                None => Vec::new(),
            };
            let home = platform.home_dir().ok_or("no home directory")?;
            let opts = agasthy_discovery::AiScanOptions::new(home, projects);
            agasthy_discovery::scan_ai(platform.as_ref(), &opts, &agasthy_discovery::no_progress)
        }
        "processes" | "disk" => {
            let repos: Vec<(String, String, PathBuf, u32)> = match &db {
                Some(path) => Store::open(path)
                    .and_then(|s| {
                        s.list_entities(&agasthy_core::EntityQuery {
                            kind: Some(agasthy_core::EntityKind::Repository),
                            text: None,
                            limit: Some(5000),
                            offset: None,
                        })
                    })
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter_map(|e| {
                        let d = e
                            .attributes
                            .get("clones_of_same_remote")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(1) as u32;
                        e.path.map(|p| (e.id, e.name, PathBuf::from(p), d))
                    })
                    .collect(),
                None => Vec::new(),
            };
            let out = if cmd == "processes" {
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
        "impact" | "shared" => {
            let db = db.ok_or("this command needs --db FILE")?;
            let store = Store::open(&db).map_err(|e| e.to_string())?;
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
