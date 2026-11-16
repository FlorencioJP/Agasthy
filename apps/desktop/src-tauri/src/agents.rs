//! Agent Control Center: coding-agent sessions, what they left running, and
//! the Git worktrees they work in.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agasthy_core::{EntityKind, EntityQuery};
use agasthy_discovery::git::{read_status, read_worktrees};
use agasthy_process::RepoRef;
use agasthy_process::agents::AgentReport;
use serde::Serialize;
use tauri::State;
use ts_rs::TS;

use crate::ops;
use crate::state::{AppState, Inner};

type CmdResult<T> = Result<T, String>;

/// How often the background sampler looks at processes, so processes left
/// behind by an agent are recognised even when the Agents page is closed.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(45);

/// Take a process snapshot and update the agent tracker.
pub fn sample(inner: &Inner) -> CmdResult<AgentReport> {
    let repos: Vec<RepoRef> = ops::repositories(inner)?
        .into_iter()
        .map(|(id, name, path, _)| (id, name, path))
        .collect();
    let snap = inner
        .processes
        .lock()
        .map_err(|_| "process monitor lock poisoned")?
        .snapshot(&inner.runner, &repos);
    Ok(inner
        .agents
        .lock()
        .map_err(|_| "agent tracker lock poisoned")?
        .update(&snap))
}

/// Start the background sampler (a plain thread; it only reads).
pub fn start_sampler(inner: Arc<Inner>) {
    std::thread::Builder::new()
        .name("agasthy-agent-sampler".into())
        .spawn(move || {
            loop {
                let _ = sample(&inner);
                std::thread::sleep(SAMPLE_EVERY);
            }
        })
        .ok();
}

#[tauri::command]
pub async fn agent_report(state: State<'_, AppState>) -> CmdResult<AgentReport> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sample(&st.0))
        .await
        .map_err(|e| e.to_string())?
}

/// Token usage from agents' own logs (Claude Code, Codex, Hermes) over the
/// last `window_days` (default 30, at most 90). Only counters are read.
#[tauri::command]
pub async fn agent_usage(
    state: State<'_, AppState>,
    window_days: Option<u32>,
) -> CmdResult<agasthy_discovery::ai::usage::UsageReport> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let home = ops::home(&st.0)?;
        let days = window_days.unwrap_or(30).clamp(1, 90);
        let mut idx = st.0.usage.lock().map_err(|_| "usage index lock poisoned")?;
        Ok(idx.refresh(&home, days, agasthy_core::time::now_ms()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct WorktreeView {
    pub repo_id: String,
    pub repo_name: String,
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub locked: bool,
    /// The folder is gone; only Git's record remains.
    pub prunable: bool,
    /// Which tool's folder convention the path follows, if any.
    pub created_by: Option<String>,
    /// Number of changed or untracked files; `None` when it could not be read.
    pub changed_files: Option<u32>,
    /// Agent sessions currently working inside it.
    pub active_agents: Vec<String>,
    pub modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct WorktreeReport {
    pub worktrees: Vec<WorktreeView>,
    pub repos_checked: u32,
    pub problems: Vec<String>,
}

/// Which tool a worktree path belongs to, by folder convention only.
pub fn worktree_owner(path: &str) -> Option<&'static str> {
    let p = path.replace('\\', "/");
    if p.contains("/.claude/worktrees/") {
        Some("Claude Code")
    } else if p.contains("/.codex/worktrees/") || p.contains("/codex/worktrees/") {
        Some("Codex")
    } else if p.contains("/.cursor/worktrees/") || p.contains("/cursor/worktrees/") {
        Some("Cursor")
    } else if p.contains("/org.agasthy.desktop/labs/") {
        Some("AGASTHY Update Lab")
    } else if p.contains("/conductor/") {
        Some("Conductor")
    } else {
        None
    }
}

#[tauri::command]
pub async fn agent_worktrees(state: State<'_, AppState>) -> CmdResult<WorktreeReport> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || worktrees(&st.0))
        .await
        .map_err(|e| e.to_string())?
}

pub fn worktrees(inner: &Inner) -> CmdResult<WorktreeReport> {
    let repos = {
        let s = inner.store.lock().map_err(|_| "database lock poisoned")?;
        s.list_entities(&EntityQuery {
            kind: Some(EntityKind::Repository),
            text: None,
            limit: Some(500),
            offset: None,
        })
        .map_err(|e| e.to_string())?
    };
    let sessions = inner
        .agents
        .lock()
        .map_err(|_| "agent tracker lock poisoned")?
        .last_sessions();
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut checked = 0;
    for r in &repos {
        let Some(path) = r.path.as_deref() else {
            continue;
        };
        checked += 1;
        let list = match read_worktrees(&inner.runner, Path::new(path)) {
            agasthy_core::Availability::Available { value } => value,
            other => {
                problems.push(format!(
                    "{}: {}",
                    r.name,
                    match other {
                        agasthy_core::Availability::Unavailable { reason } => reason,
                        _ => "not readable".into(),
                    }
                ));
                continue;
            }
        };
        for w in list.into_iter().filter(|w| !w.is_main) {
            let p = Path::new(&w.path);
            let exists = p.is_dir();
            let changed_files = if exists {
                read_status(&inner.runner, p)
                    .value()
                    .map(|s| s.staged + s.modified + s.untracked + s.conflicted)
            } else {
                None
            };
            let active_agents = sessions
                .iter()
                .filter(|(_, cwd)| cwd.as_deref().is_some_and(|c| Path::new(c).starts_with(p)))
                .map(|(label, _)| label.clone())
                .collect();
            let modified_at = std::fs::metadata(p)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(agasthy_core::time::system_time_ms);
            out.push(WorktreeView {
                repo_id: r.id.clone(),
                repo_name: r.name.clone(),
                created_by: worktree_owner(&w.path).map(str::to_string),
                path: w.path,
                branch: w.branch,
                head: w.head,
                locked: w.locked,
                prunable: w.prunable || !exists,
                changed_files,
                active_agents,
                modified_at,
            });
        }
    }
    out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(WorktreeReport {
        worktrees: out,
        repos_checked: checked,
        problems,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn worktree_owners() {
        use super::worktree_owner as o;
        assert_eq!(o("/u/web/.claude/worktrees/fix"), Some("Claude Code"));
        assert_eq!(o("/u/.codex/worktrees/abc/web"), Some("Codex"));
        assert_eq!(
            o("/u/Library/Application Support/org.agasthy.desktop/labs/x"),
            Some("AGASTHY Update Lab")
        );
        assert_eq!(o("/u/web-feature"), None);
    }
}
