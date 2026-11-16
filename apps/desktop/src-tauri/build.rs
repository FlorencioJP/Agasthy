/// App commands exposed to the webview. Each one must also be granted in
/// `capabilities/default.json` (as `allow-<command-name>`), so nothing is
/// callable by accident.
const COMMANDS: &[&str] = &[
    "app_info",
    "telemetry",
    "gpus",
    "overview",
    "list_scan_roots",
    "add_scan_root",
    "remove_scan_root",
    "set_scan_root_enabled",
    "run_scan",
    "running_scans",
    "list_entities",
    "entity_detail",
    "list_scans",
    "list_audit",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
