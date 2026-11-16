//! Processes, listening ports, and guarded termination.
//!
//! Termination rules:
//! * a process is addressed by PID **and** start time, so a reused PID is never hit;
//! * system processes, processes of other users, AGASTHY itself and its parent are refused;
//! * graceful (SIGTERM) and forced (SIGKILL) are separate requests; the UI only
//!   offers force after an explicit, separate confirmation;
//! * the outcome is verified by polling, never assumed.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agasthy_core::{Availability, time::now_ms};
use agasthy_discovery::ai::redact::redact_args;
use agasthy_platform::{CommandRunner, CommandSpec};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProcessKind {
    System,
    Application,
    DeveloperServer,
    Database,
    AiTool,
    Container,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ListeningPort {
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectLink {
    pub id: String,
    pub name: String,
    /// `working directory` or `executable`.
    pub via: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub exe: Option<String>,
    /// Command line with credential-like arguments redacted (at most 24 args).
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub cpu_percent: Availability<f32>,
    pub memory_bytes: u64,
    /// Seconds since the Unix epoch. Together with `pid` identifies the process.
    pub start_time: u64,
    pub run_time_secs: u64,
    pub status: String,
    pub kind: ProcessKind,
    /// Why AGASTHY will not terminate this process, if it will not.
    pub protected: Option<String>,
    pub ports: Vec<ListeningPort>,
    pub project: Option<ProjectLink>,
    /// The `.app` bundle the executable lives in.
    pub app_bundle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProcessSnapshot {
    pub taken_at: i64,
    pub processes: Vec<ProcessInfo>,
    /// Whether listening ports could be read (and why not).
    pub ports: Availability<u32>,
    pub own_pid: u32,
}

/// A known repository for linking processes to projects: (entity id, name, path).
pub type RepoRef = (String, String, PathBuf);

const CRITICAL_NAMES: &[&str] = &[
    "kernel_task",
    "launchd",
    "loginwindow",
    "WindowServer",
    "Dock",
    "Finder",
    "SystemUIServer",
    "ControlCenter",
    "NotificationCenter",
    "coreaudiod",
    "cfprefsd",
    "distnoted",
    "UserEventAgent",
    "securityd",
    "trustd",
    "opendirectoryd",
    "mds",
    "mds_stores",
    "logd",
    "systemd",
    "init",
    "explorer.exe",
    "csrss.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
];

const SYSTEM_PREFIXES: &[&str] = &[
    "/System/",
    "/usr/libexec/",
    "/usr/sbin/",
    "/sbin/",
    "/Library/Apple/",
    "/usr/lib/systemd/",
    "C:\\Windows\\",
];

const SERVER_NAMES: &[&str] = &[
    "node",
    "deno",
    "bun",
    "python",
    "python3",
    "ruby",
    "java",
    "go",
    "php",
    "php-fpm",
    "uvicorn",
    "gunicorn",
    "next-server",
    "vite",
    "esbuild",
    "rails",
    "puma",
    "dotnet",
    "beam.smp",
    "caddy",
    "nginx",
    "httpd",
];
const DATABASE_NAMES: &[&str] = &[
    "postgres",
    "mysqld",
    "mariadbd",
    "redis-server",
    "mongod",
    "clickhouse",
    "valkey-server",
    "memcached",
];
const AI_NAMES: &[&str] = &[
    "ollama",
    "claude",
    "codex",
    "gemini",
    "aider",
    "lms",
    "llama-server",
    "vllm",
    "local-ai",
    "openclaw",
];
const CONTAINER_NAMES: &[&str] = &[
    "com.docker.backend",
    "com.docker.vmnetd",
    "docker",
    "containerd",
    "colima",
    "limactl",
    "podman",
    "qemu-system-aarch64",
    "vfkit",
];

pub struct ProcessMonitor {
    sys: System,
    refreshed: Option<Instant>,
}

impl Default for ProcessMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_cwd(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet)
}

impl ProcessMonitor {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            refreshed: None,
        }
    }

    fn refresh(&mut self) -> bool {
        let had_previous = self
            .refreshed
            .is_some_and(|t| t.elapsed() >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
        self.refreshed = Some(Instant::now());
        had_previous
    }

    /// Take a snapshot of all visible processes.
    pub fn snapshot(&mut self, runner: &dyn CommandRunner, repos: &[RepoRef]) -> ProcessSnapshot {
        // CPU usage is a delta between two readings. On the very first call take
        // a baseline and wait the minimum interval, so the first view is complete.
        if self.refreshed.is_none() {
            self.refresh();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(50));
        }
        let cpu_ready = self.refresh();
        let (ports_by_pid, ports_state) = listening_ports(runner);
        let own = sysinfo::get_current_pid().ok();
        let own_uid = own
            .and_then(|p| self.sys.process(p))
            .and_then(|p| p.user_id())
            .cloned();
        let own_parent = own
            .and_then(|p| self.sys.process(p))
            .and_then(|p| p.parent());
        let mut out: Vec<ProcessInfo> = self
            .sys
            .processes()
            .values()
            .map(|p| {
                let pid = p.pid().as_u32();
                let name = p.name().to_string_lossy().to_string();
                let exe = p.exe().map(|e| e.display().to_string());
                let ports = ports_by_pid
                    .iter()
                    .filter(|(ppid, _)| *ppid == pid)
                    .map(|(_, port)| port.clone())
                    .collect::<Vec<_>>();
                let protected = protection_reason(
                    pid,
                    &name,
                    exe.as_deref(),
                    own.map(|o| o.as_u32()),
                    own_parent.map(|o| o.as_u32()),
                    match (own_uid.as_ref(), p.user_id()) {
                        (Some(a), Some(b)) => Some(a == b),
                        _ => None,
                    },
                );
                let cmd_raw: Vec<String> = p
                    .cmd()
                    .iter()
                    .take(24)
                    .map(|a| a.to_string_lossy().chars().take(200).collect())
                    .collect();
                let cwd = p.cwd().map(Path::to_path_buf);
                ProcessInfo {
                    pid,
                    parent_pid: p.parent().map(|x| x.as_u32()),
                    kind: classify(
                        &name,
                        exe.as_deref(),
                        !ports.is_empty(),
                        protected.is_some(),
                    ),
                    project: link_project(cwd.as_deref(), p.exe(), repos),
                    app_bundle: exe.as_deref().and_then(app_bundle),
                    name,
                    exe,
                    command: redact_args(&cmd_raw),
                    cwd: cwd.map(|c| c.display().to_string()),
                    cpu_percent: if cpu_ready {
                        Availability::available(p.cpu_usage())
                    } else {
                        Availability::unavailable("collecting the first CPU sample")
                    },
                    memory_bytes: p.memory(),
                    start_time: p.start_time(),
                    run_time_secs: p.run_time(),
                    status: p.status().to_string(),
                    protected,
                    ports,
                }
            })
            .collect();
        out.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
        ProcessSnapshot {
            taken_at: now_ms(),
            processes: out,
            ports: ports_state,
            own_pid: own.map(|p| p.as_u32()).unwrap_or(0),
        }
    }

    /// Re-read one process and check it is still the same one and may be terminated.
    pub fn check_target(
        &mut self,
        pid: u32,
        start_time: u64,
    ) -> Result<(String, Option<String>), String> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
            true,
            refresh_kind(),
        );
        let own = sysinfo::get_current_pid().ok();
        if let Some(o) = own {
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[o]),
                false,
                refresh_kind(),
            );
        }
        let p = self
            .sys
            .process(Pid::from_u32(pid))
            .ok_or_else(|| format!("process {pid} is no longer running"))?;
        if p.start_time() != start_time {
            return Err(format!(
                "PID {pid} now belongs to a different process; nothing was done"
            ));
        }
        let own_proc = own.and_then(|o| self.sys.process(o));
        let same_user = match (own_proc.and_then(|o| o.user_id()), p.user_id()) {
            (Some(a), Some(b)) => Some(a == b),
            _ => None,
        };
        let name = p.name().to_string_lossy().to_string();
        let exe = p.exe().map(|e| e.display().to_string());
        let reason = protection_reason(
            pid,
            &name,
            exe.as_deref(),
            own.map(|o| o.as_u32()),
            own_proc.and_then(|o| o.parent()).map(|x| x.as_u32()),
            same_user,
        );
        Ok((name, reason))
    }

    /// Send SIGTERM (graceful) or SIGKILL (force) and wait for the process to exit.
    pub fn terminate(
        &mut self,
        pid: u32,
        start_time: u64,
        force: bool,
        wait: Duration,
    ) -> Result<TerminateOutcome, String> {
        let (name, reason) = self.check_target(pid, start_time)?;
        if let Some(r) = reason {
            return Err(format!("{name} (PID {pid}) is protected: {r}"));
        }
        let p = self
            .sys
            .process(Pid::from_u32(pid))
            .ok_or("process vanished")?;
        let signal = if force { Signal::Kill } else { Signal::Term };
        match p.kill_with(signal) {
            Some(true) => {}
            Some(false) => {
                return Err(format!(
                    "the operating system refused to signal {name} (PID {pid})"
                ));
            }
            None => return Err("this signal is not supported on this platform".into()),
        }
        let deadline = Instant::now() + wait;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
                true,
                ProcessRefreshKind::nothing(),
            );
            let still = self.sys.process(Pid::from_u32(pid)).is_some_and(|p| {
                p.start_time() == start_time
                    && !matches!(p.status(), sysinfo::ProcessStatus::Zombie)
            });
            if !still {
                return Ok(TerminateOutcome { name, exited: true });
            }
            if Instant::now() >= deadline {
                return Ok(TerminateOutcome {
                    name,
                    exited: false,
                });
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminateOutcome {
    pub name: String,
    /// Verified gone. `false` means it was signalled but is still running.
    pub exited: bool,
}

/// Why a process must not be terminated, or `None` if AGASTHY may offer it.
pub fn protection_reason(
    pid: u32,
    name: &str,
    exe: Option<&str>,
    own_pid: Option<u32>,
    own_parent: Option<u32>,
    same_user: Option<bool>,
) -> Option<String> {
    if pid <= 1 {
        return Some("core operating-system process".into());
    }
    if Some(pid) == own_pid {
        return Some("this is AGASTHY".into());
    }
    if Some(pid) == own_parent {
        return Some("this process started AGASTHY".into());
    }
    if CRITICAL_NAMES.contains(&name) {
        return Some("essential system process".into());
    }
    if let Some(e) = exe
        && SYSTEM_PREFIXES.iter().any(|p| e.starts_with(p))
    {
        return Some("part of the operating system".into());
    }
    match same_user {
        Some(false) => {
            Some("owned by another user; AGASTHY never asks for administrator rights".into())
        }
        None => Some("owner could not be determined".into()),
        Some(true) => None,
    }
}

pub fn classify(name: &str, exe: Option<&str>, listening: bool, protected: bool) -> ProcessKind {
    let lower = name.to_ascii_lowercase();
    if DATABASE_NAMES
        .iter()
        .any(|d| lower == *d || lower.starts_with(d))
    {
        return ProcessKind::Database;
    }
    if AI_NAMES.contains(&lower.as_str())
        || exe.is_some_and(|e| e.contains("/Ollama.app/") || e.contains("LM Studio.app"))
    {
        return ProcessKind::AiTool;
    }
    if CONTAINER_NAMES.iter().any(|c| lower == *c)
        || exe.is_some_and(|e| e.contains("/Docker.app/"))
    {
        return ProcessKind::Container;
    }
    if listening
        && SERVER_NAMES
            .iter()
            .any(|s| lower == *s || lower.starts_with(&format!("{s}.")))
    {
        return ProcessKind::DeveloperServer;
    }
    if exe.is_some_and(|e| e.contains(".app/Contents/")) && !protected {
        return ProcessKind::Application;
    }
    if protected && exe.is_none_or(|e| SYSTEM_PREFIXES.iter().any(|p| e.starts_with(p))) {
        return ProcessKind::System;
    }
    ProcessKind::Other
}

fn app_bundle(exe: &str) -> Option<String> {
    let i = exe.find(".app/")?;
    Some(exe[..i + 4].to_string())
}

fn link_project(cwd: Option<&Path>, exe: Option<&Path>, repos: &[RepoRef]) -> Option<ProjectLink> {
    let best = |p: &Path| {
        repos
            .iter()
            .filter(|(_, _, root)| p.starts_with(root))
            .max_by_key(|(_, _, root)| root.as_os_str().len())
    };
    if let Some((id, name, _)) = cwd.and_then(best) {
        return Some(ProjectLink {
            id: id.clone(),
            name: name.clone(),
            via: "working directory".into(),
        });
    }
    exe.and_then(best).map(|(id, name, _)| ProjectLink {
        id: id.clone(),
        name: name.clone(),
        via: "executable".into(),
    })
}

/// Listening TCP sockets via `lsof` (macOS/Linux). Only the current user's
/// processes are visible without administrator rights; that is reported.
pub fn listening_ports(
    runner: &dyn CommandRunner,
) -> (Vec<(u32, ListeningPort)>, Availability<u32>) {
    if cfg!(windows) {
        return (Vec::new(), Availability::unsupported("windows"));
    }
    let spec = CommandSpec::new("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"])
        .timeout(Duration::from_secs(10));
    match runner.run(&spec) {
        // lsof exits 1 when nothing matches.
        Ok(o) if o.success() || (o.exit_code == Some(1) && o.stdout.trim().is_empty()) => {
            let v = parse_lsof(&o.stdout);
            let n = v.len() as u32;
            (v, Availability::available(n))
        }
        Ok(o) => (
            Vec::new(),
            Availability::unavailable(format!("lsof exited with {:?}", o.exit_code)),
        ),
        Err(e) => (Vec::new(), Availability::unavailable(e.to_string())),
    }
}

pub fn parse_lsof(out: &str) -> Vec<(u32, ListeningPort)> {
    let mut v = Vec::new();
    let mut pid: Option<u32> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.trim().parse().ok();
        } else if let (Some(n), Some(pid)) = (line.strip_prefix('n'), pid)
            && let Some((addr, port)) = n.rsplit_once(':')
            && let Ok(port) = port.parse::<u16>()
        {
            let item = (
                pid,
                ListeningPort {
                    address: addr.trim_matches(['[', ']']).to_string(),
                    port,
                },
            );
            if !v.contains(&item) {
                v.push(item);
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use agasthy_platform::SystemCommandRunner;

    #[test]
    fn protection_rules() {
        assert!(protection_reason(1, "launchd", None, Some(500), Some(400), Some(true)).is_some());
        assert!(
            protection_reason(500, "agasthy", None, Some(500), Some(400), Some(true))
                .unwrap()
                .contains("AGASTHY")
        );
        assert!(
            protection_reason(400, "zsh", None, Some(500), Some(400), Some(true))
                .unwrap()
                .contains("started AGASTHY")
        );
        assert!(protection_reason(42, "WindowServer", None, Some(500), None, Some(true)).is_some());
        assert!(
            protection_reason(
                42,
                "x",
                Some("/System/Library/x"),
                Some(500),
                None,
                Some(true)
            )
            .is_some()
        );
        assert!(
            protection_reason(
                42,
                "postgres",
                Some("/opt/homebrew/bin/postgres"),
                Some(500),
                None,
                Some(false)
            )
            .unwrap()
            .contains("another user")
        );
        assert!(
            protection_reason(
                42,
                "node",
                Some("/opt/homebrew/bin/node"),
                Some(500),
                None,
                None
            )
            .is_some()
        );
        assert!(
            protection_reason(
                42,
                "node",
                Some("/opt/homebrew/bin/node"),
                Some(500),
                None,
                Some(true)
            )
            .is_none()
        );
    }

    #[test]
    fn classification() {
        assert_eq!(
            classify("postgres", None, true, false),
            ProcessKind::Database
        );
        assert_eq!(
            classify("node", Some("/opt/homebrew/bin/node"), true, false),
            ProcessKind::DeveloperServer
        );
        assert_eq!(
            classify("node", Some("/opt/homebrew/bin/node"), false, false),
            ProcessKind::Other
        );
        assert_eq!(classify("ollama", None, true, false), ProcessKind::AiTool);
        assert_eq!(
            classify(
                "Cursor Helper",
                Some("/Applications/Cursor.app/Contents/MacOS/Cursor Helper"),
                false,
                false
            ),
            ProcessKind::Application
        );
        assert_eq!(
            classify("syslogd", Some("/usr/sbin/syslogd"), false, true),
            ProcessKind::System
        );
    }

    #[test]
    fn lsof_parsing() {
        let out = "p123\nn*:3000\nn[::1]:3000\np456\nn127.0.0.1:5432\n";
        let v = parse_lsof(out);
        assert_eq!(v.len(), 3);
        assert_eq!(
            v[0],
            (
                123,
                ListeningPort {
                    address: "*".into(),
                    port: 3000
                }
            )
        );
        assert_eq!(v[1].1.address, "::1");
        assert_eq!(
            v[2],
            (
                456,
                ListeningPort {
                    address: "127.0.0.1".into(),
                    port: 5432
                }
            )
        );
    }

    #[test]
    fn links_processes_to_the_deepest_repository() {
        let repos = vec![
            (
                "repository:/c".to_string(),
                "c".to_string(),
                PathBuf::from("/c"),
            ),
            (
                "repository:/c/inner".to_string(),
                "inner".to_string(),
                PathBuf::from("/c/inner"),
            ),
        ];
        let l = link_project(Some(Path::new("/c/inner/src")), None, &repos).unwrap();
        assert_eq!(l.name, "inner");
        assert_eq!(l.via, "working directory");
        assert!(link_project(Some(Path::new("/elsewhere")), None, &repos).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn terminates_only_its_own_child_and_verifies() {
        // A process this test starts, so nothing else on the machine is touched.
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut m = ProcessMonitor::new();
        let runner = SystemCommandRunner::new(&[], vec![], vec![]);
        let snap = m.snapshot(&runner, &[]);
        let info = snap
            .processes
            .iter()
            .find(|p| p.pid == pid)
            .expect("child is visible");
        assert!(info.protected.is_none(), "{:?}", info.protected);
        // A wrong start time is refused (PID-reuse protection).
        assert!(
            m.terminate(pid, info.start_time + 1, false, Duration::from_secs(2))
                .is_err()
        );
        let out = m
            .terminate(pid, info.start_time, false, Duration::from_secs(5))
            .unwrap();
        let _ = child.wait();
        assert!(out.exited, "graceful termination is verified");
        assert!(
            m.check_target(pid, info.start_time).is_err(),
            "gone afterwards"
        );
    }

    #[test]
    fn first_snapshot_already_has_cpu() {
        let runner = SystemCommandRunner::new(&[], vec![], vec![]);
        let snap = ProcessMonitor::new().snapshot(&runner, &[]);
        let me = snap
            .processes
            .iter()
            .find(|p| p.pid == snap.own_pid)
            .unwrap();
        assert!(me.cpu_percent.is_available());
    }

    #[test]
    fn refuses_itself() {
        let mut m = ProcessMonitor::new();
        let runner = SystemCommandRunner::new(&[], vec![], vec![]);
        let snap = m.snapshot(&runner, &[]);
        let me = snap
            .processes
            .iter()
            .find(|p| p.pid == snap.own_pid)
            .unwrap();
        assert!(
            m.terminate(me.pid, me.start_time, true, Duration::from_millis(100))
                .is_err()
        );
    }
}
