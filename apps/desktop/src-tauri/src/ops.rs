//! System operations: processes, disk analysis, and the action engine.
//!
//! Read commands (`processes`, `disk_analysis`, `quarantine_list`) observe.
//! Changes happen only through `plan_action` → `execute_action`, which goes
//! through `agasthy-actions` (typed, re-validated, verified, audited).

use std::path::PathBuf;

use agasthy_actions::{ActionContext, ActionPlan, ActionRequest, ActionResult};
use agasthy_cleanup::{AnalysisInput, DiskAnalysis, QuarantineOp};
use agasthy_core::{EntityKind, EntityQuery};
use agasthy_process::{ProcessSnapshot, RepoRef};
use tauri::State;

use crate::state::{AppState, Inner};

type CmdResult<T> = Result<T, String>;

pub(crate) fn repositories(inner: &Inner) -> CmdResult<Vec<(String, String, PathBuf, u32)>> {
    let store = inner.store.lock().map_err(|_| "database lock poisoned")?;
    let repos = store
        .list_entities(&EntityQuery {
            kind: Some(EntityKind::Repository),
            text: None,
            limit: Some(5000),
            offset: None,
        })
        .map_err(|e| e.to_string())?;
    Ok(repos
        .into_iter()
        .filter_map(|e| {
            let dupes = e
                .attributes
                .get("clones_of_same_remote")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            e.path.map(|p| (e.id, e.name, PathBuf::from(p), dupes))
        })
        .collect())
}

pub(crate) fn scan_roots(inner: &Inner) -> CmdResult<Vec<PathBuf>> {
    let store = inner.store.lock().map_err(|_| "database lock poisoned")?;
    Ok(store
        .list_scan_roots()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| r.enabled)
        .map(|r| PathBuf::from(r.path))
        .collect())
}

pub(crate) fn home(inner: &Inner) -> CmdResult<PathBuf> {
    inner
        .platform
        .home_dir()
        .ok_or_else(|| "could not determine the home directory".to_string())
}

#[tauri::command]
pub async fn processes(state: State<'_, AppState>) -> CmdResult<ProcessSnapshot> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let repos: Vec<RepoRef> = repositories(&st.0)?
            .into_iter()
            .map(|(id, name, path, _)| (id, name, path))
            .collect();
        let mut m =
            st.0.processes
                .lock()
                .map_err(|_| "process monitor lock poisoned")?;
        let snap = m.snapshot(&st.0.runner, &repos);
        drop(m);
        if let Ok(mut a) = st.0.agents.lock() {
            a.update(&snap);
        }
        Ok(snap)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn disk_analysis(state: State<'_, AppState>) -> CmdResult<DiskAnalysis> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let input = AnalysisInput {
            home: home(&st.0)?,
            scan_roots: scan_roots(&st.0)?,
            repos: repositories(&st.0)?
                .into_iter()
                .map(|(_, _, p, d)| (p, d))
                .collect(),
        };
        let a = agasthy_cleanup::analyze(&input, &st.0.runner);
        *st.0.analysis.lock().map_err(|_| "analysis lock poisoned")? = Some(a.clone());
        Ok(a)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn cached_disk_analysis(state: State<'_, AppState>) -> CmdResult<Option<DiskAnalysis>> {
    Ok(state
        .0
        .analysis
        .lock()
        .map_err(|_| "analysis lock poisoned")?
        .clone())
}

#[tauri::command]
pub fn quarantine_list(state: State<'_, AppState>) -> CmdResult<Vec<QuarantineOp>> {
    Ok(state.0.quarantine.list())
}

pub(crate) fn context(inner: &Inner) -> CmdResult<ActionContext<'_>> {
    Ok(ActionContext {
        runner: &inner.runner,
        monitor: &inner.processes,
        quarantine: &inner.quarantine,
        home: home(inner)?,
        scan_roots: scan_roots(inner)?
            .into_iter()
            .map(|r| agasthy_core::paths::canonical(&r).unwrap_or(r))
            .collect(),
        repos: repositories(inner)?
            .into_iter()
            .map(|(_, _, p, _)| p)
            .collect(),
        labs: Some(&inner.labs),
        repo_index: repositories(inner)?
            .into_iter()
            .map(|(id, _, p, _)| (id, p))
            .collect(),
        analysis: inner
            .analysis
            .lock()
            .map_err(|_| "analysis lock poisoned")?
            .clone(),
    })
}

/// Dry run: what would happen. Changes nothing.
#[tauri::command]
pub async fn plan_action(
    state: State<'_, AppState>,
    request: ActionRequest,
) -> CmdResult<ActionPlan> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = context(&st.0)?;
        Ok(st.0.engine.plan(&ctx, request))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Perform a planned action identified by its single-use token.
#[tauri::command]
pub async fn execute_action(state: State<'_, AppState>, token: String) -> CmdResult<ActionResult> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = context(&st.0)?;
        let mut audit = |action: &str, target: &str, outcome: &str, detail: &str| {
            if let Ok(s) = st.0.store.lock() {
                let _ = s.audit("user", action, target, outcome, Some(detail));
            }
        };
        let result = st.0.engine.execute(&ctx, &token, &mut audit);
        // A cleanup changes the disk: the cached analysis is no longer accurate.
        if result.quarantine_op.is_some()
            && let Ok(mut a) = st.0.analysis.lock()
        {
            *a = None;
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
