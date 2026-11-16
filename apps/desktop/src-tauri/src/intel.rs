//! Phase 5 commands: removal impact, snapshots, security, Update Lab.

use std::collections::BTreeSet;

use agasthy_core::{EntityKind, EntityQuery, Store};
use agasthy_graph::RemovalImpact;
use agasthy_security::{OsvQuery, SecurityInput, SecurityReport, osv_ecosystem};
use agasthy_snapshots::{ReconstructionPlan, SnapshotDiff, SnapshotSummary};
use agasthy_update_lab::Lab;
use serde::Serialize;
use tauri::State;
use ts_rs::TS;

use crate::state::{AppState, Inner};

type CmdResult<T> = Result<T, String>;

const OSV_SETTING: &str = "security.osv_enabled";
const REPORT_SETTING: &str = "security.last_report";

fn with_store<T>(
    inner: &Inner,
    f: impl FnOnce(&Store) -> agasthy_core::CoreResult<T>,
) -> CmdResult<T> {
    let store = inner.store.lock().map_err(|_| "database lock poisoned")?;
    f(&store).map_err(|e| e.to_string())
}

fn all(store: &Store, kind: EntityKind) -> agasthy_core::CoreResult<Vec<agasthy_core::Entity>> {
    store.list_entities(&EntityQuery {
        kind: Some(kind),
        text: None,
        limit: Some(10_000),
        offset: None,
    })
}

// ----- impact -----

#[tauri::command]
pub fn removal_impact(state: State<'_, AppState>, id: String) -> CmdResult<Option<RemovalImpact>> {
    with_store(&state.0, |s| agasthy_graph::removal_impact(s, &id))
}

// ----- snapshots -----

fn current_snapshot(inner: &Inner, name: &str) -> CmdResult<agasthy_snapshots::Snapshot> {
    let env: BTreeSet<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .collect();
    let info = inner.platform.info();
    let host = sysinfo_hostname();
    with_store(inner, |s| {
        agasthy_snapshots::capture(
            s,
            name,
            host.clone(),
            Some(format!(
                "{} {}",
                info.os,
                info.os_version.clone().unwrap_or_default()
            )),
            &env,
        )
    })
}

/// Hostname from the OS (no external program is run).
fn sysinfo_hostname() -> Option<String> {
    sysinfo::System::host_name()
}

#[tauri::command]
pub fn snapshots_list(state: State<'_, AppState>) -> CmdResult<Vec<SnapshotSummary>> {
    Ok(state.0.snapshots.list())
}

#[tauri::command]
pub fn snapshot_create(state: State<'_, AppState>, name: String) -> CmdResult<SnapshotSummary> {
    let name = if name.trim().is_empty() {
        "Snapshot".to_string()
    } else {
        name
    };
    let snap = current_snapshot(&state.0, &name)?;
    let summary = state.0.snapshots.save(&snap)?;
    with_store(&state.0, |s| {
        s.audit(
            "user",
            "snapshot.create",
            &summary.path,
            "ok",
            Some(&format!("{} items", snap.items.len())),
        )
    })?;
    Ok(summary)
}

/// Compare snapshot `from` with snapshot `to`, or with the current state when `to` is `None`.
#[tauri::command]
pub fn snapshot_compare(
    state: State<'_, AppState>,
    from: String,
    to: Option<String>,
) -> CmdResult<SnapshotDiff> {
    let a = state.0.snapshots.load(&from)?;
    let b = match to {
        Some(id) => state.0.snapshots.load(&id)?,
        None => current_snapshot(&state.0, "Now")?,
    };
    Ok(agasthy_snapshots::diff(&a, &b))
}

#[tauri::command]
pub fn snapshot_reconstruction(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<ReconstructionPlan> {
    let snap = state.0.snapshots.load(&id)?;
    let now = current_snapshot(&state.0, "Now")?;
    Ok(agasthy_snapshots::reconstruction(&snap, &now))
}

// ----- security -----

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SecuritySettings {
    pub osv_enabled: bool,
    /// Exactly what an OSV check would send: ecosystem, name and version only.
    pub preview: Vec<OsvQuery>,
    pub preview_total: u32,
    pub last_report: Option<SecurityReport>,
}

fn osv_queries(store: &Store) -> agasthy_core::CoreResult<Vec<OsvQuery>> {
    let mut q = Vec::new();
    for p in all(store, EntityKind::Package)? {
        let manager = p
            .attributes
            .get("manager")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let (Some(eco), Some(v)) = (osv_ecosystem(manager), p.version.clone()) {
            q.push(OsvQuery {
                entity_id: p.id.clone(),
                ecosystem: eco.into(),
                name: p.name.clone(),
                version: v,
            });
        }
    }
    // Project dependencies, when a lockfile pinned the version.
    for dep in all(store, EntityKind::Dependency)? {
        let eco = dep
            .attributes
            .get("ecosystem")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Some(osv) = osv_ecosystem(eco) else {
            continue;
        };
        let mut versions = BTreeSet::new();
        for e in store.edges_touching(&dep.id)? {
            if e.to_id != dep.id {
                continue;
            }
            for d in e
                .attributes
                .get("declarations")
                .and_then(|d| d.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(r) = d.get("resolved").and_then(|r| r.as_str()) {
                    versions.insert(r.to_string());
                }
            }
        }
        for v in versions {
            q.push(OsvQuery {
                entity_id: dep.id.clone(),
                ecosystem: osv.into(),
                name: dep.name.clone(),
                version: v,
            });
        }
    }
    q.sort();
    q.dedup();
    Ok(q)
}

#[tauri::command]
pub fn security_settings(state: State<'_, AppState>) -> CmdResult<SecuritySettings> {
    with_store(&state.0, |s| {
        let queries = osv_queries(s)?;
        Ok(SecuritySettings {
            osv_enabled: s.get_setting(OSV_SETTING)?.as_deref() == Some("true"),
            preview_total: queries.len() as u32,
            preview: queries.into_iter().take(50).collect(),
            last_report: s
                .get_setting(REPORT_SETTING)?
                .and_then(|t| serde_json::from_str(&t).ok()),
        })
    })
}

#[tauri::command]
pub fn set_osv_enabled(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    with_store(&state.0, |s| {
        s.set_setting(OSV_SETTING, if enabled { "true" } else { "false" })?;
        s.audit(
            "user",
            if enabled {
                "settings.osv_on"
            } else {
                "settings.osv_off"
            },
            "osv.dev",
            "ok",
            None,
        )?;
        Ok(())
    })
}

/// Build and run the security scan. Called from the generic scan runner.
pub fn security_batch(inner: &Inner) -> CmdResult<agasthy_core::IngestBatch> {
    let (enabled, queries, unknown) = with_store(inner, |s| {
        let enabled = s.get_setting(OSV_SETTING)?.as_deref() == Some("true");
        let queries = osv_queries(s)?;
        let mut unknown = Vec::new();
        for kind in [EntityKind::Binary, EntityKind::Runtime] {
            for e in all(s, kind)? {
                let active = e
                    .attributes
                    .get("active")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let hint = e.attributes.get("install_hint").filter(|h| !h.is_null());
                if active && hint.is_none() {
                    unknown.push((
                        e.id.clone(),
                        e.name.clone(),
                        e.path.clone().unwrap_or_default(),
                    ));
                }
            }
        }
        Ok((enabled, queries, unknown))
    })?;
    let work = inner.data_dir.join("tmp");
    let input = SecurityInput {
        osv_enabled: enabled,
        queries,
        unknown_origin: unknown,
        work_dir: &work,
    };
    let (batch, report) = agasthy_security::scan(&input, &inner.runner);
    with_store(inner, |s| {
        s.set_setting(
            REPORT_SETTING,
            &serde_json::to_string(&report).unwrap_or_default(),
        )?;
        if report.sent_to_osv > 0 {
            s.audit(
                "user",
                "security.osv_query",
                agasthy_security::OSV_QUERY_URL,
                "ok",
                Some(&format!(
                    "{} package name/version pairs sent; {} advisories matched",
                    report.sent_to_osv, report.vulnerabilities
                )),
            )?;
        }
        Ok(())
    })?;
    Ok(batch)
}

// ----- update lab -----

#[tauri::command]
pub fn labs_list(state: State<'_, AppState>) -> CmdResult<Vec<Lab>> {
    Ok(state.0.labs.list())
}
