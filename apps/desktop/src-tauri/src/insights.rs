//! Insights: a short, ranked list of things worth doing now, derived from data
//! AGASTHY already has. Nothing here changes the machine; an insight can only
//! open a page, start a (read-only) scan, or open the review dialog for an action.

use std::collections::HashMap;

use agasthy_actions::{ActionRequest, ProcessTarget};
use agasthy_core::{EntityKind, EntityQuery, ScanKind, ScanStatus};
use serde::Serialize;
use tauri::State;
use ts_rs::TS;

use crate::state::{AppState, Inner};

type CmdResult<T> = Result<T, String>;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum InsightSeverity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum InsightAction {
    /// Open a page (hash route, e.g. `/agents`).
    Open { route: String, label: String },
    /// Open the review dialog for this action. Nothing runs without confirmation.
    Review {
        request: ActionRequest,
        label: String,
    },
    /// Start a read-only scan.
    Scan { scan: ScanKind, label: String },
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Insight {
    pub id: String,
    pub severity: InsightSeverity,
    /// Short area label: Memory, Disk, Agents, …
    pub area: String,
    pub title: String,
    pub detail: String,
    pub action: Option<InsightAction>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct InsightsReport {
    pub generated_at: i64,
    pub insights: Vec<Insight>,
}

fn gb(b: f64) -> String {
    if b >= GIB {
        format!("{:.1} GB", b / GIB)
    } else {
        format!("{:.0} MB", b / 1024.0 / 1024.0)
    }
}

fn tokens(n: u64) -> String {
    match n {
        n if n >= 1_000_000_000 => format!("{:.1}B", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.1}K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

fn open(route: &str, label: &str) -> Option<InsightAction> {
    Some(InsightAction::Open {
        route: route.into(),
        label: label.into(),
    })
}

/// Build the insights. Reads only in-memory state and the database, plus one
/// incremental read of agents' usage logs.
pub fn compute(inner: &Inner) -> CmdResult<InsightsReport> {
    let now = agasthy_core::time::now_ms();
    let mut out: Vec<Insight> = Vec::new();
    let telemetry = inner
        .monitor
        .lock()
        .map_err(|_| "telemetry lock poisoned")?
        .sample();
    let agents = crate::agents::latest(inner).ok();
    let agent_mem: u64 = agents.as_ref().map(|a| a.total_memory_bytes).unwrap_or(0);

    // ----- memory -----
    if let Some(m) = telemetry.memory.value() {
        let avail = m.available_bytes as f64 / m.total_bytes.max(1) as f64;
        // Windows reports commit charge beyond RAM as "swap"; that is not paging.
        let swap = if agasthy_system_monitor::SWAP_IS_COMMIT_CHARGE {
            0.0
        } else {
            m.swap_used_bytes as f64
        };
        if avail < 0.10 || swap > 4.0 * GIB {
            let mut detail = format!(
                "{} of {} available",
                gb(m.available_bytes as f64),
                gb(m.total_bytes as f64)
            );
            if swap > 0.0 {
                detail.push_str(&format!(", {} swap in use", gb(swap)));
            }
            let route = if agent_mem as f64 > 1.0 * GIB {
                detail.push_str(&format!(
                    ". Coding-agent sessions hold {}.",
                    gb(agent_mem as f64)
                ));
                "/agents"
            } else {
                detail.push('.');
                "/processes"
            };
            out.push(Insight {
                id: "memory".into(),
                severity: if avail < 0.05 {
                    InsightSeverity::Critical
                } else {
                    InsightSeverity::Warning
                },
                area: "Memory".into(),
                title: if swap > 4.0 * GIB {
                    "Memory is swapping heavily".into()
                } else {
                    "Memory is nearly full".into()
                },
                detail,
                action: open(
                    route,
                    if route == "/agents" {
                        "See agent sessions"
                    } else {
                        "See processes"
                    },
                ),
            });
        }
    }

    // ----- disk -----
    if let Some(disks) = telemetry.disks.value() {
        let main = disks.iter().find(|d| d.primary);
        if let Some(d) = main {
            let free = d.available_bytes as f64;
            if free < 15.0 * GIB {
                let reclaim = inner
                    .analysis
                    .lock()
                    .ok()
                    .and_then(|a| a.as_ref().map(|a| a.eligible_bytes));
                let detail = match reclaim {
                    Some(r) if r > 0 => format!(
                        "{} free. {} of rebuildable folders and caches passed AGASTHY's safety checks.",
                        gb(free),
                        gb(r as f64)
                    ),
                    _ => format!(
                        "{} free. Analyse disk usage to see what can be cleaned safely.",
                        gb(free)
                    ),
                };
                out.push(Insight {
                    id: "disk".into(),
                    severity: if free < 5.0 * GIB {
                        InsightSeverity::Critical
                    } else {
                        InsightSeverity::Warning
                    },
                    area: "Disk".into(),
                    title: "Disk space is low".into(),
                    detail,
                    action: open("/storage", "Review cleanup"),
                });
            }
        }
    }

    // ----- agents -----
    if let Some(a) = &agents {
        let stoppable: Vec<_> = a
            .leftovers
            .iter()
            .filter(|l| l.process.protected.is_none())
            .collect();
        if !stoppable.is_empty() {
            let mem: u64 = stoppable.iter().map(|l| l.process.memory_bytes).sum();
            out.push(Insight {
                id: "agent-leftovers".into(),
                severity: InsightSeverity::Warning,
                area: "Agents".into(),
                title: format!(
                    "{} process{} left running by exited agents",
                    stoppable.len(),
                    if stoppable.len() == 1 { "" } else { "es" }
                ),
                detail: format!(
                    "Dev servers, MCP servers or watchers holding {} and their ports.",
                    gb(mem as f64)
                ),
                action: Some(InsightAction::Review {
                    request: ActionRequest::TerminateProcesses {
                        targets: stoppable
                            .iter()
                            .map(|l| ProcessTarget {
                                pid: l.process.pid,
                                start_time: l.process.start_time,
                            })
                            .collect(),
                    },
                    label: "Review stopping them".into(),
                }),
            });
        }
    }

    // ----- token usage -----
    if let Ok(home) = crate::ops::home(inner)
        && let Ok(mut idx) = inner.usage.lock()
    {
        let r = idx.refresh(&home, 8, now);
        let mut per_day: HashMap<String, (u64, f64)> = HashMap::new();
        for d in &r.days {
            let e = per_day.entry(d.day.clone()).or_default();
            e.0 += d.tokens.input + d.tokens.output + d.tokens.cache_read + d.tokens.cache_write;
            e.1 += d.estimated_cost_usd.unwrap_or(0.0);
        }
        let today = date_of(now);
        if let Some(&(t, cost)) = per_day.get(&today) {
            let prior: Vec<u64> = (1..=7)
                .filter_map(|i| per_day.get(&date_of(now - i * DAY_MS)).map(|x| x.0))
                .collect();
            let avg = if prior.is_empty() {
                0.0
            } else {
                prior.iter().sum::<u64>() as f64 / prior.len() as f64
            };
            let ratio = if avg > 0.0 { t as f64 / avg } else { 0.0 };
            let high = ratio >= 2.0 && t > 50_000_000;
            out.push(Insight {
                id: "tokens-today".into(),
                severity: if high {
                    InsightSeverity::Warning
                } else {
                    InsightSeverity::Info
                },
                area: "Tokens".into(),
                title: if high {
                    format!("Token use today is {ratio:.1}× your recent daily average")
                } else {
                    format!("Agents used {} tokens today", tokens(t))
                },
                detail: format!(
                    "{} tokens today (≈${:.2} at API list prices){}.",
                    tokens(t),
                    cost,
                    if avg > 0.0 {
                        format!("; 7-day average {}", tokens(avg as u64))
                    } else {
                        String::new()
                    }
                ),
                action: open("/usage", "Open Token Usage"),
            });
        }
    }

    // ----- data from the store -----
    {
        let s = inner.store.lock().map_err(|_| "database lock poisoned")?;
        let all = |k: EntityKind| {
            s.list_entities(&EntityQuery {
                kind: Some(k),
                text: None,
                limit: Some(5000),
                offset: None,
            })
            .unwrap_or_default()
        };

        // Security
        let findings = all(EntityKind::Finding);
        let serious = findings
            .iter()
            .filter(|f| {
                matches!(
                    f.attributes.get("severity").and_then(|x| x.as_str()),
                    Some("critical" | "high")
                )
            })
            .count();
        if serious > 0 {
            out.push(Insight {
                id: "security".into(),
                severity: InsightSeverity::Warning,
                area: "Security".into(),
                title: format!(
                    "{serious} critical or high security finding{}",
                    if serious == 1 { "" } else { "s" }
                ),
                detail: "Known vulnerabilities in installed packages or project dependencies."
                    .into(),
                action: open("/security", "Review findings"),
            });
        }

        // Uncommitted work left alone
        let idle: Vec<String> = all(EntityKind::Repository)
            .iter()
            .filter(|r| {
                r.attributes
                    .pointer("/working_tree/value/clean")
                    .and_then(|v| v.as_bool())
                    == Some(false)
            })
            .filter(|r| {
                r.attributes
                    .pointer("/head/value/last_commit_at")
                    .and_then(|v| v.as_i64())
                    .is_some_and(|t| now - t > 30 * DAY_MS)
            })
            .map(|r| r.name.clone())
            .collect();
        if !idle.is_empty() {
            out.push(Insight {
                id: "idle-uncommitted".into(),
                severity: InsightSeverity::Info,
                area: "Repositories".into(),
                title: format!(
                    "{} repositor{} with uncommitted work untouched for 30+ days",
                    idle.len(),
                    if idle.len() == 1 { "y" } else { "ies" }
                ),
                detail: format!(
                    "{}. Commit or stash it before cleaning up or switching machines.",
                    idle.iter().take(4).cloned().collect::<Vec<_>>().join(", ")
                ),
                action: open("/discovery", "Open repositories"),
            });
        }

        // AI configuration hygiene
        let orphaned = all(EntityKind::AiConfigFile)
            .iter()
            .filter(|f| f.attributes.get("orphaned").and_then(|v| v.as_bool()) == Some(true))
            .count();
        let mcp = all(EntityKind::McpServer);
        let mut fp: HashMap<String, u32> = HashMap::new();
        for m in &mcp {
            if let Some(f) = m.attributes.get("fingerprint").and_then(|v| v.as_str()) {
                *fp.entry(f.to_string()).or_default() += 1;
            }
        }
        let dup_mcp = fp.values().filter(|n| **n > 1).count();
        if orphaned > 0 || dup_mcp > 0 {
            let mut parts = Vec::new();
            if orphaned > 0 {
                parts.push(format!(
                    "{orphaned} instruction file{} no installed tool reads",
                    if orphaned == 1 { "" } else { "s" }
                ));
            }
            if dup_mcp > 0 {
                parts.push(format!(
                    "{dup_mcp} MCP server{} configured in more than one place",
                    if dup_mcp == 1 { "" } else { "s" }
                ));
            }
            out.push(Insight {
                id: "ai-hygiene".into(),
                severity: InsightSeverity::Info,
                area: "AI setup".into(),
                title: "AI configuration could be tidied".into(),
                detail: format!("{}.", parts.join("; ")),
                action: open("/ai", "Open AI Ecosystem"),
            });
        }

        // Freshness
        let latest = s.latest_scans().unwrap_or_default();
        let roots = s
            .list_scan_roots()
            .unwrap_or_default()
            .iter()
            .filter(|r| r.enabled)
            .count();
        for kind in [
            ScanKind::Packages,
            ScanKind::Software,
            ScanKind::Ai,
            ScanKind::Repositories,
        ] {
            if kind == ScanKind::Repositories && roots == 0 {
                continue;
            }
            let last = latest
                .iter()
                .find(|r| r.kind == kind && r.status != ScanStatus::Running);
            let (title, stale) = match last.and_then(|r| r.finished_at) {
                None => (format!("{} not scanned yet", scan_label(kind)), true),
                Some(t) if now - t > 3 * DAY_MS => (
                    format!(
                        "{} data is {} days old",
                        scan_label(kind),
                        (now - t) / DAY_MS
                    ),
                    true,
                ),
                _ => (String::new(), false),
            };
            if stale {
                out.push(Insight {
                    id: format!("stale-{}", kind.as_str()),
                    severity: InsightSeverity::Info,
                    area: "Freshness".into(),
                    title,
                    detail: "Answers about this area (impact analysis, the Operator) are only as current as the last scan.".into(),
                    action: Some(InsightAction::Scan { scan: kind, label: "Scan now".into() }),
                });
            }
        }
    }

    out.sort_by_key(|i| i.severity);
    out.truncate(8);
    Ok(InsightsReport {
        generated_at: now,
        insights: out,
    })
}

pub fn scan_label(k: ScanKind) -> &'static str {
    match k {
        ScanKind::Repositories => "Repository",
        ScanKind::Software => "Installed software",
        ScanKind::Packages => "Package",
        ScanKind::Ai => "AI ecosystem",
        ScanKind::Security => "Security",
    }
}

/// `YYYY-MM-DD` (UTC).
pub fn date_of(ms: i64) -> String {
    let z = ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y}-{m:02}-{d:02}")
}

#[tauri::command]
pub async fn insights(state: State<'_, AppState>) -> CmdResult<InsightsReport> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || compute(&st.0))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_and_labels() {
        assert_eq!(super::date_of(1_794_790_000_000), "2026-11-16");
        assert_eq!(super::tokens(2_500_000_000), "2.5B");
        assert_eq!(super::gb(3.0 * super::GIB), "3.0 GB");
    }
}
