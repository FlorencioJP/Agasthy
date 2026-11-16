//! Typed commands exposed to the webview.
//!
//! Everything here is read-only with respect to the user's machine. The only
//! writes go to AGASTHY's own database (scan results, scan roots, audit log).

use std::path::PathBuf;

use agasthy_core::{
    AuditEntry, Availability, Edge, Entity, EntityDetail, EntityKind, EntityQuery, IssueKind,
    KindCount, RelationCount, RelationKind, ScanIssue, ScanKind, ScanRecord, ScanRoot,
};
use agasthy_discovery::{
    AiScanOptions, RepoScanOptions, ScanProgress, runtime_copies, scan_ai, scan_packages,
    scan_repositories, scan_software, validate_scan_root,
};
use agasthy_graph::{GraphView, ImpactReport, OverviewKind, SharedDependency};
use agasthy_platform::{GpuInfo, PlatformInfo, SearchDirOrigin};
use agasthy_system_monitor::TelemetrySnapshot;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use ts_rs::TS;

use crate::state::{AppState, Inner};

type CmdResult<T> = Result<T, String>;

const DEFAULT_SCAN_DEPTH: u32 = 6;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn with_store<T>(
    inner: &Inner,
    f: impl FnOnce(&mut agasthy_core::Store) -> agasthy_core::CoreResult<T>,
) -> CmdResult<T> {
    let mut store = inner
        .store
        .lock()
        .map_err(|_| "database lock poisoned; restart AGASTHY".to_string())?;
    f(&mut store).map_err(err)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SearchDirInfo {
    pub path: String,
    pub origin: SearchDirOrigin,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    pub platform: PlatformInfo,
    pub data_dir: String,
    pub db_path: String,
    pub schema_version: u32,
    pub search_dirs: Vec<SearchDirInfo>,
    /// Programs AGASTHY is permitted to run. Everything else is refused.
    pub command_allowlist: Vec<String>,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let inner = &state.0;
    let schema_version = with_store(inner, |s| s.schema_version())?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        platform: inner.platform.info(),
        data_dir: inner.data_dir.display().to_string(),
        db_path: inner.db_path.display().to_string(),
        schema_version,
        search_dirs: inner
            .platform
            .executable_search_dirs()
            .into_iter()
            .map(|d| SearchDirInfo {
                path: d.path.display().to_string(),
                origin: d.origin,
            })
            .collect(),
        command_allowlist: inner.runner.allowed_programs(),
    })
}

#[tauri::command]
pub async fn telemetry(state: State<'_, AppState>) -> CmdResult<TelemetrySnapshot> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut m =
            st.0.monitor
                .lock()
                .map_err(|_| "telemetry lock poisoned".to_string())?;
        Ok(m.sample())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn gpus(state: State<'_, AppState>) -> CmdResult<Availability<Vec<GpuInfo>>> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        st.0.gpus
            .get_or_init(|| st.0.platform.gpus(&st.0.runner))
            .clone()
    })
    .await
    .map_err(err)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Overview {
    pub counts: Vec<KindCount>,
    pub recent: Vec<Entity>,
    pub latest_scans: Vec<ScanRecord>,
    pub scan_roots: u32,
    pub dirty_repositories: u32,
    pub duplicate_clone_links: u32,
    pub binaries_installed_by_package: u32,
    pub relationship_counts: Vec<RelationCount>,
}

#[tauri::command]
pub fn overview(state: State<'_, AppState>) -> CmdResult<Overview> {
    with_store(&state.0, |s| {
        Ok(Overview {
            counts: s.counts_by_kind()?,
            recent: s.recent_entities(10)?,
            latest_scans: s.latest_scans()?,
            scan_roots: s.list_scan_roots()?.len() as u32,
            dirty_repositories: s.count_where_attr(
                EntityKind::Repository,
                "$.working_tree.value.clean",
                &serde_json::Value::Bool(false),
            )?,
            duplicate_clone_links: s.count_relationships(RelationKind::DuplicateClone)?,
            binaries_installed_by_package: s.count_relationships(RelationKind::InstalledBy)?,
            relationship_counts: s.relationship_counts()?,
        })
    })
}

#[tauri::command]
pub fn list_scan_roots(state: State<'_, AppState>) -> CmdResult<Vec<ScanRoot>> {
    with_store(&state.0, |s| s.list_scan_roots())
}

#[tauri::command]
pub fn add_scan_root(state: State<'_, AppState>, path: String) -> CmdResult<ScanRoot> {
    let inner = &state.0;
    let canonical = match validate_scan_root(inner.platform.as_ref(), &path) {
        Ok(p) => p,
        Err(e) => {
            let _ = with_store(inner, |s| {
                s.audit(
                    "user",
                    "scan_root.add",
                    &path,
                    "rejected",
                    Some(&e.to_string()),
                )
            });
            return Err(e.to_string());
        }
    };
    let canonical = canonical.display().to_string();
    with_store(inner, |s| {
        let root = s.add_scan_root(&canonical, DEFAULT_SCAN_DEPTH)?;
        s.audit("user", "scan_root.add", &canonical, "ok", None)?;
        Ok(root)
    })
}

#[tauri::command]
pub fn remove_scan_root(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_store(&state.0, |s| {
        let path = s.remove_scan_root(id)?;
        s.audit(
            "user",
            "scan_root.remove",
            &path,
            "ok",
            Some("repositories under this folder are removed from AGASTHY at the next repository scan"),
        )?;
        Ok(())
    })
}

#[tauri::command]
pub fn set_scan_root_enabled(state: State<'_, AppState>, id: i64, enabled: bool) -> CmdResult<()> {
    with_store(&state.0, |s| {
        s.set_scan_root_enabled(id, enabled)?;
        s.audit(
            "user",
            if enabled {
                "scan_root.enable"
            } else {
                "scan_root.disable"
            },
            &id.to_string(),
            "ok",
            None,
        )?;
        Ok(())
    })
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScanEvent {
    pub kind: ScanKind,
    /// `progress`, `finished` or `failed`.
    pub stage: String,
    pub progress: Option<ScanProgress>,
    pub message: Option<String>,
}

/// Removes a scan kind from the running set even if the scan panics.
struct RunningGuard<'a> {
    inner: &'a Inner,
    kind: ScanKind,
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut r) = self.inner.running.lock() {
            r.remove(&self.kind);
        }
    }
}

#[tauri::command]
pub async fn run_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: ScanKind,
) -> CmdResult<ScanRecord> {
    let st = state.inner().clone();
    {
        let mut running = st.0.running.lock().map_err(|_| "state lock poisoned")?;
        if !running.insert(kind) {
            return Err(format!("a {} scan is already running", kind.as_str()));
        }
    }
    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = RunningGuard { inner: &st.0, kind };
        execute_scan(&app2, &st.0, kind)
    })
    .await
    .map_err(err)?;
    let stage = if result.is_ok() { "finished" } else { "failed" };
    let _ = app.emit(
        "scan-event",
        ScanEvent {
            kind,
            stage: stage.into(),
            progress: None,
            message: result.as_ref().err().cloned(),
        },
    );
    result
}

fn execute_scan(app: &AppHandle, inner: &Inner, kind: ScanKind) -> CmdResult<ScanRecord> {
    let scan_id = with_store(inner, |s| {
        let id = s.begin_scan(kind)?;
        s.audit("user", "scan.start", kind.as_str(), "started", None)?;
        Ok(id)
    })?;

    let emit = |p: ScanProgress| {
        let _ = app.emit(
            "scan-event",
            ScanEvent {
                kind,
                stage: "progress".into(),
                progress: Some(p),
                message: None,
            },
        );
    };

    let batch = match kind {
        ScanKind::Repositories => {
            let roots = with_store(inner, |s| s.list_scan_roots())?;
            let mut issues = Vec::new();
            let mut valid: Vec<(PathBuf, usize)> = Vec::new();
            for r in roots.iter().filter(|r| r.enabled) {
                match validate_scan_root(inner.platform.as_ref(), &r.path) {
                    Ok(p) => valid.push((p, r.max_depth as usize)),
                    Err(e) => issues.push(ScanIssue::new(
                        IssueKind::NotFound,
                        r.path.clone(),
                        e.to_string(),
                    )),
                }
            }
            if valid.is_empty() {
                let message = if roots.is_empty() {
                    "no folders to scan: add a folder in Settings first".to_string()
                } else {
                    "none of the enabled folders could be scanned".to_string()
                };
                issues.push(ScanIssue::new(IssueKind::Skipped, "scan roots", &message));
                with_store(inner, |s| {
                    s.fail_scan(scan_id, &issues)?;
                    s.audit(
                        "user",
                        "scan.finish",
                        kind.as_str(),
                        "failed",
                        Some(&message),
                    )
                })?;
                return Err(message);
            }
            let mut opts = RepoScanOptions::new(valid);
            // Link projects to the runtime copies on PATH (ids match the software scan).
            opts.runtimes = runtime_copies(inner.platform.as_ref());
            let mut batch = scan_repositories(&opts, &inner.runner, &emit);
            batch.issues.extend(issues);
            batch
        }
        ScanKind::Software => scan_software(inner.platform.as_ref(), &inner.runner, &emit),
        ScanKind::Packages => scan_packages(inner.platform.as_ref(), &inner.runner, &emit),
        ScanKind::Ai => {
            // Project-level AI files are read only inside repositories already
            // discovered in user-approved folders.
            let projects = with_store(inner, |s| {
                s.list_entities(&EntityQuery {
                    kind: Some(EntityKind::Repository),
                    text: None,
                    limit: Some(5000),
                    offset: None,
                })
            })?
            .into_iter()
            .filter_map(|e| e.path.map(|p| (e.id, PathBuf::from(p))))
            .collect();
            let home = inner
                .platform
                .home_dir()
                .ok_or_else(|| "could not determine the home directory".to_string())?;
            let opts = AiScanOptions::new(home, projects);
            scan_ai(inner.platform.as_ref(), &opts, &emit)
        }
    };

    with_store(inner, |s| {
        let res = s.ingest(scan_id, &batch)?;
        s.audit(
            "user",
            "scan.finish",
            kind.as_str(),
            if batch.issues.is_empty() {
                "ok"
            } else {
                "ok_with_issues"
            },
            Some(&format!(
                "{} entities recorded, {} no longer present removed from AGASTHY, {} issues",
                res.entities_written,
                res.entities_removed,
                batch.issues.len()
            )),
        )?;
        let rec = s
            .list_scans(50)?
            .into_iter()
            .find(|r| r.id == scan_id)
            .ok_or_else(|| agasthy_core::CoreError::NotFound(format!("scan {scan_id}")))?;
        Ok(rec)
    })
}

#[tauri::command]
pub fn running_scans(state: State<'_, AppState>) -> CmdResult<Vec<ScanKind>> {
    let r = state.0.running.lock().map_err(|_| "state lock poisoned")?;
    let mut v: Vec<ScanKind> = r.iter().copied().collect();
    v.sort_by_key(|k| k.as_str());
    Ok(v)
}

#[tauri::command]
pub fn list_entities(state: State<'_, AppState>, query: EntityQuery) -> CmdResult<Vec<Entity>> {
    with_store(&state.0, |s| s.list_entities(&query))
}

#[tauri::command]
pub fn entity_detail(state: State<'_, AppState>, id: String) -> CmdResult<Option<EntityDetail>> {
    with_store(&state.0, |s| s.entity_detail(&id))
}

#[tauri::command]
pub fn list_scans(state: State<'_, AppState>, limit: Option<u32>) -> CmdResult<Vec<ScanRecord>> {
    with_store(&state.0, |s| s.list_scans(limit.unwrap_or(50).min(500)))
}

#[tauri::command]
pub fn list_audit(state: State<'_, AppState>, limit: Option<u32>) -> CmdResult<Vec<AuditEntry>> {
    with_store(&state.0, |s| s.list_audit(limit.unwrap_or(100).min(1000)))
}

#[tauri::command]
pub fn list_edges(state: State<'_, AppState>, kind: RelationKind) -> CmdResult<Vec<Edge>> {
    with_store(&state.0, |s| s.edges_of_kind(kind, None))
}

// ----- graph -----

#[tauri::command]
pub fn graph_neighborhood(
    state: State<'_, AppState>,
    center: String,
    depth: Option<u32>,
    kinds: Option<Vec<EntityKind>>,
) -> CmdResult<GraphView> {
    with_store(&state.0, |s| {
        agasthy_graph::neighborhood(
            s,
            &center,
            depth.unwrap_or(1).clamp(1, 3),
            &kinds.unwrap_or_default(),
            250,
        )
    })
}

#[tauri::command]
pub fn graph_overview(state: State<'_, AppState>, kind: OverviewKind) -> CmdResult<GraphView> {
    with_store(&state.0, |s| agasthy_graph::overview(s, kind, 400))
}

#[tauri::command]
pub fn runtime_impact(state: State<'_, AppState>, id: String) -> CmdResult<Option<ImpactReport>> {
    with_store(&state.0, |s| agasthy_graph::runtime_impact(s, &id))
}

#[tauri::command]
pub fn shared_dependencies(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> CmdResult<Vec<SharedDependency>> {
    with_store(&state.0, |s| {
        agasthy_graph::shared_dependencies(s, limit.unwrap_or(25).min(200))
    })
}
