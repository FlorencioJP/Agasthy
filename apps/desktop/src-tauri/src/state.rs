use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use agasthy_actions::ActionEngine;
use agasthy_cleanup::{DiskAnalysis, Quarantine};
use agasthy_core::{Availability, ScanKind, Store};
use agasthy_platform::{GpuInfo, Platform, SystemCommandRunner};
use agasthy_process::ProcessMonitor;
use agasthy_snapshots::SnapshotStore;
use agasthy_system_monitor::Monitor;
use agasthy_update_lab::LabStore;

pub struct Inner {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub store: Mutex<Store>,
    pub platform: Box<dyn Platform>,
    pub runner: SystemCommandRunner,
    pub monitor: Mutex<Monitor>,
    pub gpus: OnceLock<Availability<Vec<GpuInfo>>>,
    pub running: Mutex<HashSet<ScanKind>>,
    pub processes: Mutex<ProcessMonitor>,
    /// Remembers agent sessions' processes to recognise what they leave behind.
    pub agents: Mutex<agasthy_process::agents::AgentTracker>,
    pub last_agents: Mutex<Option<agasthy_process::agents::AgentReport>>,
    /// Incremental reader over coding agents' local usage logs (counters only).
    pub usage: Mutex<agasthy_discovery::ai::usage::UsageIndex>,
    pub quarantine: Quarantine,
    pub engine: ActionEngine,
    /// The latest disk analysis; cleanup plans may only target its candidates.
    pub analysis: Mutex<Option<DiskAnalysis>>,
    pub snapshots: SnapshotStore,
    pub labs: LabStore,
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
        let data_dir_snap = data_dir.join("snapshots");
        let data_dir_labs = data_dir.join("labs");
        let quarantine = Quarantine::new(data_dir.join("quarantine"))
            .map_err(|e| format!("could not create the quarantine folder: {e}"))?;
        Ok(Self(Arc::new(Inner {
            data_dir,
            db_path,
            store: Mutex::new(store),
            platform,
            runner,
            monitor: Mutex::new(Monitor::new()),
            gpus: OnceLock::new(),
            running: Mutex::new(HashSet::new()),
            processes: Mutex::new(ProcessMonitor::new()),
            agents: Mutex::new(agasthy_process::agents::AgentTracker::new()),
            last_agents: Mutex::new(None),
            usage: Mutex::new(agasthy_discovery::ai::usage::UsageIndex::new()),
            quarantine,
            engine: ActionEngine::new(),
            analysis: Mutex::new(None),
            snapshots: SnapshotStore::new(data_dir_snap)
                .map_err(|e| format!("could not create the snapshots folder: {e}"))?,
            labs: LabStore::new(data_dir_labs)
                .map_err(|e| format!("could not create the Update Lab folder: {e}"))?,
        })))
    }
}
