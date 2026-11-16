//! AGASTHY desktop shell.
//!
//! This crate is deliberately thin: it owns application state, exposes typed
//! commands to the webview and forwards events. All product logic lives in the
//! `crates/` workspace members.

mod commands;
mod state;

use tauri::Manager;

pub use commands::{AppInfo, Overview, ScanEvent, SearchDirInfo};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("could not resolve the app data directory: {e}"))?;
            let state = state::AppState::open(data_dir)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::telemetry,
            commands::gpus,
            commands::overview,
            commands::list_scan_roots,
            commands::add_scan_root,
            commands::remove_scan_root,
            commands::set_scan_root_enabled,
            commands::run_scan,
            commands::running_scans,
            commands::list_entities,
            commands::entity_detail,
            commands::list_scans,
            commands::list_audit,
            commands::graph_neighborhood,
            commands::graph_overview,
            commands::runtime_impact,
            commands::shared_dependencies,
            commands::list_edges,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AGASTHY");
}
