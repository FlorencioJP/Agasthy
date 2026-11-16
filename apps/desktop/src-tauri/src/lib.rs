//! AGASTHY desktop shell.
//!
//! This crate is deliberately thin: it owns application state, exposes typed
//! commands to the webview and forwards events. All product logic lives in the
//! `crates/` workspace members.

mod agents;
mod commands;
mod intel;
mod operator;
mod ops;
mod state;

use tauri::Manager;

pub use commands::{AppInfo, Overview, ScanEvent, SearchDirInfo};
pub use intel::SecuritySettings;
pub use operator::OperatorSettings;

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
            agents::start_sampler(state.0.clone());
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
            agents::agent_report,
            agents::agent_worktrees,
            agents::agent_usage,
            ops::processes,
            ops::disk_analysis,
            ops::cached_disk_analysis,
            ops::quarantine_list,
            ops::plan_action,
            ops::execute_action,
            intel::removal_impact,
            intel::snapshots_list,
            intel::snapshot_create,
            intel::snapshot_compare,
            intel::snapshot_reconstruction,
            intel::security_settings,
            intel::set_osv_enabled,
            intel::labs_list,
            operator::operator_settings,
            operator::operator_set_provider,
            operator::operator_set_key,
            operator::operator_delete_key,
            operator::operator_set_cloud_consent,
            operator::operator_ask,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AGASTHY");
}
