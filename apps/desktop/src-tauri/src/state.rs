use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use agasthy_core::{Availability, ScanKind, Store};
use agasthy_platform::{GpuInfo, Platform, SystemCommandRunner};
use agasthy_system_monitor::Monitor;

pub struct Inner {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub store: Mutex<Store>,
    pub platform: Box<dyn Platform>,
    pub runner: SystemCommandRunner,
    pub monitor: Mutex<Monitor>,
    pub gpus: OnceLock<Availability<Vec<GpuInfo>>>,
    pub running: Mutex<HashSet<ScanKind>>,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl AppState {
    pub fn open(data_dir: PathBuf) -> Result<Self, String> {
        let db_path = data_dir.join("agasthy.db");
        let store = Store::open(&db_path)
            .map_err(|e| format!("could not open database at {}: {e}", db_path.display()))?;
        store
            .mark_interrupted_scans()
            .map_err(|e| format!("database error: {e}"))?;
        let platform = agasthy_platform::current();
        let runner = agasthy_platform::system_runner(
            platform.as_ref(),
            &agasthy_discovery::catalog::tool_ids(),
        );
        Ok(Self(Arc::new(Inner {
            data_dir,
            db_path,
            store: Mutex::new(store),
            platform,
            runner,
            monitor: Mutex::new(Monitor::new()),
            gpus: OnceLock::new(),
            running: Mutex::new(HashSet::new()),
        })))
    }
}
