//! Things that run while AGASTHY is open: keeping scan data fresh, and the
//! menu-bar item. Both only read; scans are the same read-only scans the user
//! can start, and are recorded in the audit log as started by `scheduler`.

use std::sync::Arc;
use std::time::Duration;

use agasthy_core::{ScanKind, ScanStatus};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

use crate::insights::InsightSeverity;
use crate::state::{AppState, Inner};

type CmdResult<T> = Result<T, String>;

pub const AUTO_REFRESH_SETTING: &str = "scans.auto_refresh";
/// Data older than this is refreshed in the background.
pub const REFRESH_AFTER_HOURS: u32 = 24;
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(3 * 60);

// ----- scan freshness -----

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScanSchedule {
    pub auto_refresh: bool,
    pub refresh_after_hours: u32,
    /// Scans that are refreshed automatically. Security is never: it may contact OSV.dev.
    pub kinds: Vec<ScanKind>,
}

const AUTO_KINDS: [ScanKind; 4] = [
    ScanKind::Packages,
    ScanKind::Software,
    ScanKind::Repositories,
    ScanKind::Ai,
];

fn auto_refresh_enabled(inner: &Inner) -> bool {
    inner
        .store
        .lock()
        .ok()
        .and_then(|s| s.get_setting(AUTO_REFRESH_SETTING).ok().flatten())
        .is_none_or(|v| v != "false")
}

#[tauri::command]
pub fn scan_schedule(state: State<'_, AppState>) -> CmdResult<ScanSchedule> {
    Ok(ScanSchedule {
        auto_refresh: auto_refresh_enabled(&state.0),
        refresh_after_hours: REFRESH_AFTER_HOURS,
        kinds: AUTO_KINDS.to_vec(),
    })
}

#[tauri::command]
pub fn set_auto_refresh(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    let s = state.0.store.lock().map_err(|_| "database lock poisoned")?;
    s.set_setting(AUTO_REFRESH_SETTING, if enabled { "true" } else { "false" })
        .map_err(|e| e.to_string())?;
    let _ = s.audit(
        "user",
        if enabled {
            "settings.auto_refresh_on"
        } else {
            "settings.auto_refresh_off"
        },
        "scans",
        "ok",
        None,
    );
    Ok(())
}

/// Which scans are due: run successfully before (the user has opted into
/// that area) and finished more than `REFRESH_AFTER_HOURS` ago.
fn due(inner: &Inner) -> Vec<ScanKind> {
    let Ok(s) = inner.store.lock() else {
        return vec![];
    };
    let latest = s.latest_scans().unwrap_or_default();
    let roots = s
        .list_scan_roots()
        .unwrap_or_default()
        .iter()
        .filter(|r| r.enabled)
        .count();
    let now = agasthy_core::time::now_ms();
    AUTO_KINDS
        .into_iter()
        .filter(|k| *k != ScanKind::Repositories || roots > 0)
        .filter(|k| {
            latest
                .iter()
                .find(|r| {
                    r.kind == *k
                        && matches!(
                            r.status,
                            ScanStatus::Completed | ScanStatus::CompletedWithIssues
                        )
                })
                .and_then(|r| r.finished_at)
                .is_some_and(|t| now - t > i64::from(REFRESH_AFTER_HOURS) * 3_600_000)
        })
        .collect()
}

pub fn start_scheduler(app: AppHandle, inner: Arc<Inner>) {
    std::thread::Builder::new()
        .name("agasthy-scan-scheduler".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            loop {
                crate::state::guarded(&inner, "scan scheduler", || {
                    if auto_refresh_enabled(&inner) {
                        // In dependency order (packages before software, repositories before AI).
                        for kind in due(&inner) {
                            let _ = crate::commands::scan_now(&app, &inner, kind, "scheduler");
                        }
                    }
                });
                std::thread::sleep(CHECK_EVERY);
            }
        })
        .ok();
}

// ----- menu bar -----

const TRAY_ID: &str = "agasthy";
const REFRESH_TRAY: Duration = Duration::from_secs(60);

/// Show the main window, optionally on a route (`/agents`, …).
pub fn show_main(app: &AppHandle, route: Option<&str>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if let Some(r) = route {
        let _ = app.emit("navigate", r.to_string());
    }
}

struct TrayLines {
    memory: MenuItem<tauri::Wry>,
    disk: MenuItem<tauri::Wry>,
    agents: MenuItem<tauri::Wry>,
    tokens: MenuItem<tauri::Wry>,
    insights: MenuItem<tauri::Wry>,
}

pub fn build_tray(app: &AppHandle, inner: Arc<Inner>) -> tauri::Result<()> {
    let info = |id: &str, text: &str| MenuItem::with_id(app, id, text, false, None::<&str>);
    let lines = TrayLines {
        memory: info("i-memory", "Memory: measuring…")?,
        disk: info("i-disk", "Disk: measuring…")?,
        agents: info("i-agents", "Agents: looking…")?,
        tokens: info("i-tokens", "Tokens today: reading…")?,
        insights: MenuItem::with_id(app, "nav:/", "Insights: checking…", true, None::<&str>)?,
    };
    let open = MenuItem::with_id(app, "open", "Open AGASTHY", true, None::<&str>)?;
    let agents = MenuItem::with_id(app, "nav:/agents", "Agents…", true, None::<&str>)?;
    let usage = MenuItem::with_id(app, "nav:/usage", "Token Usage…", true, None::<&str>)?;
    let operator = MenuItem::with_id(
        app,
        "nav:/operator",
        "Ask the Operator…",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit AGASTHY", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &lines.insights,
            &sep1,
            &lines.memory,
            &lines.disk,
            &lines.agents,
            &lines.tokens,
            &sep2,
            &open,
            &agents,
            &usage,
            &operator,
            &sep3,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("AGASTHY")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if id == "quit" {
                app.exit(0);
            } else if id == "open" {
                show_main(app, None);
            } else if let Some(route) = id.strip_prefix("nav:") {
                show_main(app, Some(route));
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    let app2 = app.clone();
    std::thread::Builder::new()
        .name("agasthy-tray".into())
        .spawn(move || {
            loop {
                crate::state::guarded(&inner, "menu bar", || update_tray(&app2, &inner, &lines));
                std::thread::sleep(REFRESH_TRAY);
            }
        })
        .ok();
    Ok(())
}

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1024.0 / 1024.0 / 1024.0)
}

fn update_tray(app: &AppHandle, inner: &Inner, l: &TrayLines) {
    let Ok(report) = crate::insights::compute(inner) else {
        return;
    };
    let attention = report
        .insights
        .iter()
        .filter(|i| i.severity != InsightSeverity::Info)
        .count();
    let _ = l.insights.set_text(match attention {
        0 => "✓ Nothing needs attention".to_string(),
        n => format!(
            "⚠ {n} thing{} need{} attention",
            if n == 1 { "" } else { "s" },
            if n == 1 { "s" } else { "" }
        ),
    });
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_title(if attention > 0 {
            Some(attention.to_string())
        } else {
            None::<String>
        });
    }
    if let Ok(mut m) = inner.monitor.lock() {
        let t = m.sample();
        if let Some(mem) = t.memory.value() {
            let swap = if mem.swap_used_bytes > 0 && !agasthy_system_monitor::SWAP_IS_COMMIT_CHARGE
            {
                format!(" · swap {}", gb(mem.swap_used_bytes))
            } else {
                String::new()
            };
            let _ = l.memory.set_text(format!(
                "Memory: {} of {} used{swap}",
                gb(mem.used_bytes),
                gb(mem.total_bytes)
            ));
        }
        if let Some(d) = t
            .disks
            .value()
            .and_then(|d| d.iter().find(|x| x.primary).cloned())
        {
            let _ = l
                .disk
                .set_text(format!("Disk: {} free", gb(d.available_bytes)));
        }
    }
    if let Ok(a) = crate::agents::latest(inner) {
        let left = if a.leftovers.is_empty() {
            String::new()
        } else {
            format!(" · {} left running", a.leftovers.len())
        };
        let _ = l.agents.set_text(format!(
            "Agents: {} session{} · {}{left}",
            a.sessions.len(),
            if a.sessions.len() == 1 { "" } else { "s" },
            gb(a.total_memory_bytes)
        ));
    }
    let today = report
        .insights
        .iter()
        .find(|i| i.id == "tokens-today")
        .map(|i| i.detail.split(" (").next().unwrap_or("").to_string());
    let _ = l.tokens.set_text(match today {
        Some(t) => format!("Tokens: {t}"),
        None => "Tokens today: none recorded".to_string(),
    });
}
