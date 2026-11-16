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
    agasthy packages [--db FILE] [--full]
    agasthy telemetry
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
            scan_repositories(&RepoScanOptions::new(roots), &runner, &|p| {
                if p.phase == "searching" {
                    eprintln!("searching {}", p.current.unwrap_or_default());
                }
            })
        }
        "software" => scan_software(platform.as_ref(), &runner, &agasthy_discovery::no_progress),
        "packages" => scan_packages(platform.as_ref(), &runner, &agasthy_discovery::no_progress),
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
