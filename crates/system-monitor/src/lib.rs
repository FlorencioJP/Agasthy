//! Live CPU, memory, disk and process telemetry.
//!
//! [`Monitor`] keeps sampler state between calls because CPU usage is a delta:
//! the first sample after start-up reports CPU as unavailable instead of an
//! invented number.

use std::time::Instant;

use agasthy_core::{Availability, time::now_ms};
use serde::{Deserialize, Serialize};
use sysinfo::{
    CpuRefreshKind, DiskKind, Disks, MINIMUM_CPU_UPDATE_INTERVAL, MemoryRefreshKind,
    ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HostInfo {
    pub hostname: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub cpu_arch: String,
    pub uptime_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CpuSample {
    pub usage_percent: f32,
    pub per_core_percent: Vec<f32>,
    pub brand: Option<String>,
    pub logical_cores: u32,
    pub physical_cores: Option<u32>,
    pub frequency_mhz: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemorySample {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiskSample {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
    pub read_only: bool,
    /// `ssd`, `hdd` or `unknown`.
    pub kind: String,
    /// An OS-internal volume (e.g. macOS `/System/Volumes/*`) that shares space
    /// with a user-visible volume. Hidden by default in the UI.
    pub system_volume: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProcessSample {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: Availability<f32>,
    pub memory_bytes: u64,
    pub exe: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TelemetrySnapshot {
    pub taken_at: i64,
    pub host: HostInfo,
    pub cpu: Availability<CpuSample>,
    pub memory: Availability<MemorySample>,
    pub load_average: Availability<LoadAverage>,
    pub disks: Availability<Vec<DiskSample>>,
    pub process_count: Availability<u32>,
    pub top_by_memory: Availability<Vec<ProcessSample>>,
    pub top_by_cpu: Availability<Vec<ProcessSample>>,
    /// GPU utilisation is not available through portable APIs on macOS without
    /// elevated privileges; reported explicitly rather than as zero.
    pub gpu_utilisation: Availability<f32>,
}

pub struct Monitor {
    sys: System,
    disks: Disks,
    last_cpu_refresh: Instant,
    /// Per-process CPU is also a delta: it needs a previous process refresh.
    last_process_refresh: Option<Instant>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::everything()),
        );
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
            last_cpu_refresh: Instant::now(),
            last_process_refresh: None,
        }
    }

    /// Take a sample. Call repeatedly (e.g. every 2 s) for meaningful CPU data.
    pub fn sample(&mut self) -> TelemetrySnapshot {
        let host = host_info();
        let gpu_utilisation = Availability::unavailable(
            "GPU utilisation is not exposed through unprivileged system APIs in this build",
        );
        let sys = &mut self.sys;

        // CPU: usage is relative to the previous refresh.
        let had_previous = self.last_cpu_refresh.elapsed() >= MINIMUM_CPU_UPDATE_INTERVAL;
        sys.refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
        self.last_cpu_refresh = Instant::now();
        let cpu = if had_previous && !sys.cpus().is_empty() {
            let cpus = sys.cpus();
            Availability::available(CpuSample {
                usage_percent: sys.global_cpu_usage(),
                per_core_percent: cpus.iter().map(|c| c.cpu_usage()).collect(),
                brand: cpus
                    .first()
                    .map(|c| c.brand().trim().to_string())
                    .filter(|b| !b.is_empty()),
                logical_cores: cpus.len() as u32,
                physical_cores: System::physical_core_count().map(|n| n as u32),
                frequency_mhz: cpus.first().map(|c| c.frequency()).filter(|f| *f > 0),
            })
        } else if sys.cpus().is_empty() {
            Availability::unavailable("the operating system reported no CPUs")
        } else {
            Availability::unavailable("collecting the first CPU sample")
        };

        sys.refresh_memory();
        let total = sys.total_memory();
        let memory = if total == 0 {
            Availability::unavailable("the operating system reported 0 bytes of memory")
        } else {
            Availability::available(MemorySample {
                total_bytes: total,
                used_bytes: sys.used_memory(),
                available_bytes: sys.available_memory(),
                swap_total_bytes: sys.total_swap(),
                swap_used_bytes: sys.used_swap(),
            })
        };

        let load_average = if cfg!(windows) {
            Availability::unsupported("windows")
        } else {
            let l = System::load_average();
            Availability::available(LoadAverage {
                one: l.one,
                five: l.five,
                fifteen: l.fifteen,
            })
        };

        let disks = {
            let d = &mut self.disks;
            {
                d.refresh(true);
                let mut v: Vec<DiskSample> = d
                    .list()
                    .iter()
                    .map(|disk| {
                        let mount = disk.mount_point().display().to_string();
                        DiskSample {
                            name: disk.name().to_string_lossy().to_string(),
                            system_volume: is_system_volume(&mount),
                            mount_point: mount,
                            file_system: disk.file_system().to_string_lossy().to_string(),
                            total_bytes: disk.total_space(),
                            available_bytes: disk.available_space(),
                            removable: disk.is_removable(),
                            read_only: disk.is_read_only(),
                            kind: match disk.kind() {
                                DiskKind::SSD => "ssd".into(),
                                DiskKind::HDD => "hdd".into(),
                                _ => "unknown".into(),
                            },
                        }
                    })
                    .collect();
                v.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
                if v.is_empty() {
                    Availability::unavailable("the operating system reported no disks")
                } else {
                    Availability::available(v)
                }
            }
        };

        let proc_cpu_ready = self
            .last_process_refresh
            .is_some_and(|t| t.elapsed() >= MINIMUM_CPU_UPDATE_INTERVAL);
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        self.last_process_refresh = Some(Instant::now());
        let procs = sys.processes();
        let process_count = if procs.is_empty() {
            Availability::unavailable("process list is empty (access may be restricted)")
        } else {
            Availability::available(procs.len() as u32)
        };
        let to_sample = |p: &sysinfo::Process| ProcessSample {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().to_string(),
            cpu_percent: if proc_cpu_ready {
                Availability::available(p.cpu_usage())
            } else {
                Availability::unavailable("collecting the first CPU sample")
            },
            memory_bytes: p.memory(),
            exe: p.exe().map(|e| e.display().to_string()),
        };
        let (top_by_memory, top_by_cpu) = if procs.is_empty() {
            (
                Availability::unavailable("process list unavailable"),
                Availability::unavailable("process list unavailable"),
            )
        } else {
            let mut by_mem: Vec<&sysinfo::Process> = procs.values().collect();
            by_mem.sort_by_key(|p| std::cmp::Reverse(p.memory()));
            let mem: Vec<ProcessSample> = by_mem.iter().take(8).map(|p| to_sample(p)).collect();
            let cpu_top = if proc_cpu_ready {
                let mut by_cpu: Vec<&sysinfo::Process> = procs.values().collect();
                by_cpu.sort_by(|a, b| b.cpu_usage().total_cmp(&a.cpu_usage()));
                Availability::available(by_cpu.iter().take(8).map(|p| to_sample(p)).collect())
            } else {
                Availability::unavailable("collecting the first CPU sample")
            };
            (Availability::available(mem), cpu_top)
        };

        TelemetrySnapshot {
            taken_at: now_ms(),
            host,
            cpu,
            memory,
            load_average,
            disks,
            process_count,
            top_by_memory,
            top_by_cpu,
            gpu_utilisation,
        }
    }
}

fn host_info() -> HostInfo {
    HostInfo {
        hostname: System::host_name(),
        os_name: System::name(),
        os_version: System::os_version(),
        kernel_version: System::kernel_version(),
        cpu_arch: System::cpu_arch(),
        uptime_secs: Some(System::uptime()).filter(|u| *u > 0),
    }
}

fn is_system_volume(mount: &str) -> bool {
    // macOS APFS volumes that share the container with "/".
    mount.starts_with("/System/Volumes/")
        || mount.starts_with("/private/var/vm")
        || mount.starts_with("/Library/Developer/CoreSimulator/Volumes/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_cpu_sample_is_unavailable_not_zero() {
        let mut m = Monitor::new();
        let first = m.sample();
        assert!(
            !first.cpu.is_available(),
            "first sample must not report CPU"
        );
        std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL + std::time::Duration::from_millis(50));
        let second = m.sample();
        let cpu = second.cpu.value().expect("second sample reports CPU");
        assert!(cpu.logical_cores > 0);
        assert!((0.0..=100.0 * cpu.logical_cores as f32).contains(&cpu.usage_percent));
    }

    #[test]
    fn real_memory_and_disks() {
        let mut m = Monitor::new();
        let s = m.sample();
        let mem = s.memory.value().expect("memory");
        assert!(mem.total_bytes > 0);
        assert!(mem.used_bytes <= mem.total_bytes);
        let disks = s.disks.value().expect("disks");
        assert!(disks.iter().any(|d| d.total_bytes > 0));
        assert!(!s.gpu_utilisation.is_available());
    }

    #[test]
    fn system_volumes_are_flagged() {
        assert!(is_system_volume("/System/Volumes/VM"));
        assert!(is_system_volume("/System/Volumes/Data"));
        assert!(!is_system_volume("/"));
        assert!(!is_system_volume("/Volumes/External"));
    }
}
