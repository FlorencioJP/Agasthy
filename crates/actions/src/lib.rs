//! The only way AGASTHY changes the machine.
//!
//! Every action is a typed [`ActionRequest`]. It is first **planned** (a dry
//! run: exact targets, sizes, risks, recovery), which returns a single-use
//! token that expires. Only [`ActionEngine::execute`] with that token performs
//! the action, after re-validating every target, and then **verifies** the
//! outcome and writes an **audit** entry. There is no way to run arbitrary
//! commands through this crate.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use agasthy_cleanup::docker::{self, DockerPrune};
use agasthy_cleanup::quarantine::MoveRequest;
use agasthy_cleanup::{CleanupCandidate, DiskAnalysis, Quarantine, QuarantineState, safety};
use agasthy_core::Availability;
use agasthy_platform::CommandRunner;
use agasthy_process::ProcessMonitor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ActionRequest {
    /// Stop a process. `force` sends SIGKILL instead of SIGTERM.
    TerminateProcess {
        pid: u32,
        start_time: u64,
        force: bool,
    },
    /// Move cleanup candidates (from the latest disk analysis) into quarantine.
    QuarantinePaths { paths: Vec<String> },
    /// Move a quarantine operation's items back to where they were.
    RestoreQuarantine { op_id: String },
    /// Permanently delete a quarantine operation's items.
    PurgeQuarantine { op_id: String },
    /// Prune Docker build cache or dangling images. Never volumes.
    DockerPrune { target: DockerPrune },
}

impl ActionRequest {
    pub fn name(&self) -> &'static str {
        match self {
            ActionRequest::TerminateProcess { force: false, .. } => "process.terminate",
            ActionRequest::TerminateProcess { force: true, .. } => "process.force_terminate",
            ActionRequest::QuarantinePaths { .. } => "cleanup.quarantine",
            ActionRequest::RestoreQuarantine { .. } => "cleanup.restore",
            ActionRequest::PurgeQuarantine { .. } => "cleanup.purge",
            ActionRequest::DockerPrune { .. } => "docker.prune",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanItem {
    pub label: String,
    pub detail: Option<String>,
    pub bytes: Option<u64>,
    /// Why this item would not be acted on.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActionPlan {
    /// Single-use confirmation token. `None` when the plan cannot be executed.
    pub token: Option<String>,
    pub request: ActionRequest,
    pub title: String,
    pub summary: String,
    pub items: Vec<PlanItem>,
    pub total_bytes: Option<u64>,
    pub warnings: Vec<String>,
    pub can_execute: bool,
    pub reversible: bool,
    pub recovery: String,
    /// Seconds until the token expires.
    pub expires_in_secs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ActionOutcome {
    Succeeded,
    PartiallySucceeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActionResult {
    pub title: String,
    pub outcome: ActionOutcome,
    pub message: String,
    pub details: Vec<String>,
    /// How the outcome was checked.
    pub verification: String,
    pub recovery: Option<String>,
    pub bytes: Option<u64>,
    pub quarantine_op: Option<String>,
}

/// What the engine needs from the application. Borrowed per call.
pub struct ActionContext<'a> {
    pub runner: &'a dyn CommandRunner,
    pub monitor: &'a Mutex<ProcessMonitor>,
    pub quarantine: &'a Quarantine,
    pub home: PathBuf,
    pub scan_roots: Vec<PathBuf>,
    pub repos: Vec<PathBuf>,
    pub analysis: Option<DiskAnalysis>,
}

/// Receives one audit record per execution: (action, target, outcome, detail).
pub type Auditor<'a> = &'a mut dyn FnMut(&str, &str, &str, &str);

const PLAN_TTL: Duration = Duration::from_secs(600);

#[derive(Default)]
pub struct ActionEngine {
    pending: Mutex<HashMap<String, (ActionRequest, Instant)>>,
}

fn token() -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(Instant::now().elapsed().as_nanos() ^ agasthy_core::time::now_ms() as u128);
    h.write_u32(std::process::id());
    format!(
        "{:016x}{:016x}",
        h.finish(),
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    )
}

fn candidate<'a>(analysis: &'a Option<DiskAnalysis>, path: &str) -> Option<&'a CleanupCandidate> {
    analysis
        .as_ref()?
        .candidates
        .iter()
        .find(|c| c.path == path)
}

/// Re-check one cleanup target right now. Returns its size on success.
fn validate_cleanup(ctx: &ActionContext<'_>, path: &str) -> Result<(u64, String), String> {
    let c =
        candidate(&ctx.analysis, path).ok_or("not in the latest disk analysis; analyse again")?;
    if !c.eligible {
        return Err(c
            .blocked_by
            .clone()
            .unwrap_or_else(|| "not eligible".into()));
    }
    let p = PathBuf::from(path);
    safety::check_path(&p, &ctx.home, &ctx.scan_roots, &ctx.repos)?;
    if let Some(repo) = &c.project {
        safety::check_untracked(ctx.runner, Path::new(repo), &p)?;
    }
    if !ctx.quarantine.same_volume(&p) {
        return Err("on a different volume from AGASTHY's quarantine; not supported yet".into());
    }
    let size = c.size_bytes.value().copied().unwrap_or(0);
    Ok((size, format!("{:?}", c.category).to_lowercase()))
}

impl ActionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Produce a dry-run plan. Nothing is changed.
    pub fn plan(&self, ctx: &ActionContext<'_>, request: ActionRequest) -> ActionPlan {
        let mut plan = match &request {
            ActionRequest::TerminateProcess {
                pid,
                start_time,
                force,
            } => {
                let check = ctx
                    .monitor
                    .lock()
                    .map_err(|_| "process monitor unavailable".to_string())
                    .and_then(|mut m| m.check_target(*pid, *start_time));
                let (name, blocked) = match check {
                    Ok((name, reason)) => (name, reason),
                    Err(e) => (format!("PID {pid}"), Some(e)),
                };
                let mut warnings = Vec::new();
                if *force {
                    warnings.push("Force quit ends the process immediately. Unsaved work in it is lost, and it cannot clean up (temporary files, locks, child processes).".into());
                } else {
                    warnings.push("The process is asked to exit. Well-behaved programs save state and exit; some may ignore the request.".into());
                }
                ActionPlan {
                    token: None,
                    request: request.clone(),
                    title: format!("{} {name}", if *force { "Force quit" } else { "Stop" }),
                    summary: format!("Send {} to {name} (PID {pid}).", if *force { "SIGKILL" } else { "SIGTERM" }),
                    items: vec![PlanItem { label: name, detail: Some(format!("PID {pid}")), bytes: None, blocked: blocked.clone() }],
                    total_bytes: None,
                    warnings,
                    can_execute: blocked.is_none(),
                    reversible: false,
                    recovery: "A stopped process can be started again the way it was started originally. AGASTHY does not restart it.".into(),
                    expires_in_secs: PLAN_TTL.as_secs() as u32,
                }
            }
            ActionRequest::QuarantinePaths { paths } => {
                let mut items = Vec::new();
                let mut total = 0u64;
                for p in paths {
                    match validate_cleanup(ctx, p) {
                        Ok((size, cat)) => {
                            total += size;
                            items.push(PlanItem {
                                label: p.clone(),
                                detail: Some(cat),
                                bytes: Some(size),
                                blocked: None,
                            });
                        }
                        Err(e) => items.push(PlanItem {
                            label: p.clone(),
                            detail: None,
                            bytes: None,
                            blocked: Some(e),
                        }),
                    }
                }
                let blocked = items.iter().filter(|i| i.blocked.is_some()).count();
                ActionPlan {
                    token: None,
                    request: request.clone(),
                    title: format!("Move {} item{} to quarantine", items.len(), if items.len() == 1 { "" } else { "s" }),
                    summary: "Each folder is moved (not copied or deleted) into AGASTHY's quarantine on the same disk. You can restore it, or delete it permanently to free the space.".into(),
                    items,
                    total_bytes: Some(total),
                    warnings: if blocked > 0 {
                        vec![format!("{blocked} item(s) failed a safety check. Remove them from the selection to continue.")]
                    } else {
                        vec!["Disk space is freed only when you permanently delete the quarantined items. This does not free memory (RAM).".into()]
                    },
                    can_execute: blocked == 0 && !paths.is_empty(),
                    reversible: true,
                    recovery: "Restore from Storage & Cleanup → Quarantine; items return to their original paths.".into(),
                    expires_in_secs: PLAN_TTL.as_secs() as u32,
                }
            }
            ActionRequest::RestoreQuarantine { op_id }
            | ActionRequest::PurgeQuarantine { op_id } => {
                let purge = matches!(request, ActionRequest::PurgeQuarantine { .. });
                match ctx.quarantine.load(op_id) {
                    Ok(op) => {
                        let pending: Vec<_> = op.items.iter().filter(|i| !i.restored).collect();
                        let blocked = if op.state == QuarantineState::Purged {
                            Some("already permanently deleted".to_string())
                        } else if pending.is_empty() {
                            Some("nothing left in quarantine for this entry".to_string())
                        } else {
                            None
                        };
                        let total: u64 = pending.iter().map(|i| i.size_bytes).sum();
                        ActionPlan {
                            token: None,
                            request: request.clone(),
                            title: if purge { "Permanently delete quarantined items".into() } else { "Restore quarantined items".into() },
                            summary: if purge {
                                "These items are deleted from disk. This cannot be undone.".into()
                            } else {
                                "Items are moved back to their original locations. Anything that now exists at an original location is left untouched.".into()
                            },
                            items: pending
                                .iter()
                                .map(|i| PlanItem {
                                    label: i.original_path.clone(),
                                    detail: Some(i.category.clone()),
                                    bytes: Some(i.size_bytes),
                                    blocked: (!purge && Path::new(&i.original_path).exists()).then(|| "something now exists at this path; it will be left in quarantine".to_string()),
                                })
                                .collect(),
                            total_bytes: Some(total),
                            warnings: if purge { vec!["Permanent: there is no undo.".into()] } else { vec![] },
                            can_execute: blocked.is_none(),
                            reversible: !purge,
                            recovery: if purge {
                                "None from AGASTHY. Caches and dependency folders can be regenerated with the commands shown when they were quarantined.".into()
                            } else {
                                "You can clean them up again later.".into()
                            },
                            expires_in_secs: PLAN_TTL.as_secs() as u32,
                        }
                    }
                    Err(e) => ActionPlan {
                        token: None,
                        request: request.clone(),
                        title: "Quarantine entry not found".into(),
                        summary: e,
                        items: vec![],
                        total_bytes: None,
                        warnings: vec![],
                        can_execute: false,
                        reversible: false,
                        recovery: String::new(),
                        expires_in_secs: 0,
                    },
                }
            }
            ActionRequest::DockerPrune { target } => {
                let usage = docker::usage(ctx.runner);
                let (blocked, items) = match &usage {
                    Availability::Available { value } => {
                        let kind = match target {
                            DockerPrune::BuildCache => "Build Cache",
                            DockerPrune::DanglingImages => "Images",
                        };
                        let row = value.rows.iter().find(|r| r.kind == kind);
                        (
                            None,
                            vec![PlanItem {
                                label: match target {
                                    DockerPrune::BuildCache => "Docker build cache".into(),
                                    DockerPrune::DanglingImages => {
                                        format!("{} dangling image(s)", value.dangling_images)
                                    }
                                },
                                detail: row.map(|r| format!("reclaimable now: {}", r.reclaimable)),
                                bytes: None,
                                blocked: None,
                            }],
                        )
                    }
                    other => (
                        Some(match other {
                            Availability::Unavailable { reason } => reason.clone(),
                            _ => "Docker is unavailable".into(),
                        }),
                        vec![],
                    ),
                };
                ActionPlan {
                    token: None,
                    request: request.clone(),
                    title: match target {
                        DockerPrune::BuildCache => "Prune Docker build cache".into(),
                        DockerPrune::DanglingImages => "Remove dangling Docker images".into(),
                    },
                    summary: format!("Runs `docker {}`. Volumes, containers and tagged images are not touched.", target.args().join(" ")),
                    items,
                    total_bytes: None,
                    warnings: vec!["Docker deletes these directly; they do not go to quarantine. Images and cache are rebuilt or pulled again when needed.".into()],
                    can_execute: blocked.is_none(),
                    reversible: false,
                    recovery: match target {
                        DockerPrune::BuildCache => "The next `docker build` recreates cache layers (slower first build).".into(),
                        DockerPrune::DanglingImages => "Dangling images are untagged leftovers; rebuild or pull an image if you need it.".into(),
                    },
                    expires_in_secs: PLAN_TTL.as_secs() as u32,
                }
                .with_block(blocked)
            }
        };
        if plan.can_execute {
            let t = token();
            if let Ok(mut p) = self.pending.lock() {
                p.retain(|_, (_, at)| at.elapsed() < PLAN_TTL);
                p.insert(t.clone(), (request, Instant::now()));
                plan.token = Some(t);
            } else {
                plan.can_execute = false;
            }
        }
        plan
    }

    /// Execute a previously planned action. Every target is re-validated first.
    pub fn execute(
        &self,
        ctx: &ActionContext<'_>,
        token: &str,
        audit: Auditor<'_>,
    ) -> ActionResult {
        let request = {
            let looked_up = self
                .pending
                .lock()
                .map_err(|_| "internal state unavailable")
                .map(|mut p| p.remove(token));
            let refusal = match looked_up {
                Ok(Some((r, at))) if at.elapsed() < PLAN_TTL => Ok(r),
                Ok(Some(_)) => Err("this confirmation expired; review the plan again"),
                Ok(None) => Err("unknown or already used confirmation; review the plan again"),
                Err(e) => Err(e),
            };
            match refusal {
                Ok(r) => r,
                Err(msg) => {
                    // Refused attempts are audited too.
                    audit("action.execute", "confirmation token", "refused", msg);
                    return failed("Action", msg);
                }
            }
        };
        let name = request.name();
        let result = match &request {
            ActionRequest::TerminateProcess {
                pid,
                start_time,
                force,
            } => {
                let r = ctx
                    .monitor
                    .lock()
                    .map_err(|_| "process monitor unavailable".to_string())
                    .and_then(|mut m| {
                        m.terminate(
                            *pid,
                            *start_time,
                            *force,
                            Duration::from_secs(if *force { 3 } else { 8 }),
                        )
                    });
                match r {
                    Ok(o) if o.exited => ActionResult {
                        title: format!("{} {}", if *force { "Force quit" } else { "Stopped" }, o.name),
                        outcome: ActionOutcome::Succeeded,
                        message: format!("{} (PID {pid}) has exited.", o.name),
                        details: vec![],
                        verification: "Checked the process table: no process with this PID and start time remains.".into(),
                        recovery: None,
                        bytes: None,
                        quarantine_op: None,
                    },
                    Ok(o) => ActionResult {
                        title: format!("{} is still running", o.name),
                        outcome: ActionOutcome::Failed,
                        message: format!("{} (PID {pid}) received the signal but has not exited.", o.name),
                        details: vec![if *force { "It may be stuck in the kernel; try again later.".into() } else { "It may be ignoring the request. You can choose Force quit separately.".into() }],
                        verification: "Polled the process table after sending the signal.".into(),
                        recovery: None,
                        bytes: None,
                        quarantine_op: None,
                    },
                    Err(e) => failed("Stop process", &e),
                }
            }
            ActionRequest::QuarantinePaths { paths } => {
                let mut requests = Vec::new();
                let mut problems = Vec::new();
                for p in paths {
                    match validate_cleanup(ctx, p) {
                        Ok((size, cat)) => requests.push(MoveRequest {
                            path: PathBuf::from(p),
                            size_bytes: size,
                            category: cat,
                        }),
                        Err(e) => problems.push(format!("{p}: {e}")),
                    }
                }
                if !problems.is_empty() {
                    let mut r = failed(
                        "Cleanup",
                        "Nothing was moved: a target changed since the plan and failed a safety check.",
                    );
                    r.details = problems;
                    r
                } else {
                    match ctx.quarantine.move_in(requests) {
                        Ok(rep) => {
                            let moved = rep.op.items.len();
                            ActionResult {
                                title: format!("Moved {moved} item{} to quarantine", if moved == 1 { "" } else { "s" }),
                                outcome: if rep.failed.is_empty() { ActionOutcome::Succeeded } else if moved > 0 { ActionOutcome::PartiallySucceeded } else { ActionOutcome::Failed },
                                message: if rep.failed.is_empty() {
                                    "Delete them permanently from the Quarantine list to free the space, or restore them.".into()
                                } else {
                                    "Stopped at the first item that could not be moved; nothing after it was attempted.".into()
                                },
                                details: rep.failed.iter().map(|(p, e)| format!("{p}: {e}")).collect(),
                                verification: "Each original path was confirmed gone and each quarantined copy confirmed present before it was recorded.".into(),
                                recovery: (moved > 0).then(|| "Restore from Storage & Cleanup → Quarantine.".into()),
                                bytes: Some(rep.op.total_bytes),
                                quarantine_op: (moved > 0).then(|| rep.op.id.clone()),
                            }
                        }
                        Err(e) => failed("Cleanup", &e),
                    }
                }
            }
            ActionRequest::RestoreQuarantine { op_id } => match ctx.quarantine.restore(op_id) {
                Ok((op, problems)) => ActionResult {
                    title: "Restored from quarantine".into(),
                    outcome: if problems.is_empty() {
                        ActionOutcome::Succeeded
                    } else if op.items.iter().any(|i| i.restored) {
                        ActionOutcome::PartiallySucceeded
                    } else {
                        ActionOutcome::Failed
                    },
                    message: format!(
                        "{} of {} items are back in place.",
                        op.items.iter().filter(|i| i.restored).count(),
                        op.items.len()
                    ),
                    details: problems,
                    verification: "Each original path was confirmed present after the move.".into(),
                    recovery: None,
                    bytes: None,
                    quarantine_op: Some(op.id),
                },
                Err(e) => failed("Restore", &e),
            },
            ActionRequest::PurgeQuarantine { op_id } => match ctx.quarantine.purge(op_id) {
                Ok((op, freed)) => ActionResult {
                    title: "Permanently deleted".into(),
                    outcome: ActionOutcome::Succeeded,
                    message: "The quarantined items were deleted from disk.".into(),
                    details: vec![],
                    verification: "Each quarantined path was removed and confirmed absent.".into(),
                    recovery: None,
                    bytes: Some(freed),
                    quarantine_op: Some(op.id),
                },
                Err(e) => failed("Delete permanently", &e),
            },
            ActionRequest::DockerPrune { target } => match docker::prune(ctx.runner, *target) {
                Ok(summary) => ActionResult {
                    title: "Docker pruned".into(),
                    outcome: ActionOutcome::Succeeded,
                    message: summary,
                    details: vec![],
                    verification:
                        "Docker reported completion; usage is re-read on the next analysis.".into(),
                    recovery: None,
                    bytes: None,
                    quarantine_op: None,
                },
                Err(e) => failed("Docker prune", &e),
            },
        };
        let target = match &request {
            ActionRequest::TerminateProcess { pid, .. } => format!("pid {pid}"),
            ActionRequest::QuarantinePaths { paths } => paths.join(", "),
            ActionRequest::RestoreQuarantine { op_id }
            | ActionRequest::PurgeQuarantine { op_id } => op_id.clone(),
            ActionRequest::DockerPrune { target } => format!("{target:?}"),
        };
        let outcome = match result.outcome {
            ActionOutcome::Succeeded => "ok",
            ActionOutcome::PartiallySucceeded => "partial",
            ActionOutcome::Failed => "failed",
        };
        let mut detail = result.message.clone();
        if let Some(op) = &result.quarantine_op {
            detail.push_str(&format!(" [quarantine {op}]"));
        }
        if !result.details.is_empty() {
            detail.push_str(&format!(" | {}", result.details.join("; ")));
        }
        audit(name, &target, outcome, &detail);
        result
    }
}

trait WithBlock {
    fn with_block(self, blocked: Option<String>) -> Self;
}

impl WithBlock for ActionPlan {
    fn with_block(mut self, blocked: Option<String>) -> Self {
        if let Some(b) = blocked {
            self.can_execute = false;
            self.items.push(PlanItem {
                label: "Docker".into(),
                detail: None,
                bytes: None,
                blocked: Some(b),
            });
        }
        self
    }
}

fn failed(title: &str, msg: &str) -> ActionResult {
    ActionResult {
        title: format!("{title} failed"),
        outcome: ActionOutcome::Failed,
        message: msg.to_string(),
        details: vec![],
        verification: "Nothing was changed after the failure was detected.".into(),
        recovery: None,
        bytes: None,
        quarantine_op: None,
    }
}

#[cfg(test)]
mod tests;
