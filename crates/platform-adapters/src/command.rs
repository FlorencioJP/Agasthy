//! The only way AGASTHY runs external programs.
//!
//! * Programs must be on an allowlist (matched by file name, also for absolute paths).
//! * Shells and privilege-escalation tools are always refused.
//! * Arguments are passed as an array; nothing is ever interpreted by a shell.
//! * stdin is closed, so a program cannot block on a prompt.
//! * Every run has a timeout and an output size cap.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use thiserror::Error;
use wait_timeout::ChildExt;

/// Programs that can never be run, whatever the allowlist says.
const ALWAYS_DENIED: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "nu",
    "cmd",
    "powershell",
    "pwsh",
    "osascript",
    "env",
    "sudo",
    "doas",
    "su",
    "runas",
    "xargs",
    "eval",
];

#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    /// A bare program name (resolved via the runner's search dirs) or an absolute path.
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub max_output_bytes: usize,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            timeout: Duration::from_secs(10),
            cwd: None,
            env: Vec::new(),
            max_output_bytes: 8 * 1024 * 1024,
        }
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn max_output(mut self, bytes: usize) -> Self {
        self.max_output_bytes = bytes;
        self
    }

    /// Display form for logs and evidence (`program arg1 arg2`).
    pub fn display(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            s.push_str(a);
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub truncated: bool,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    pub fn ok(stdout: &str) -> Self {
        Self {
            exit_code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
            duration_ms: 0,
            truncated: false,
        }
    }

    pub fn failed(code: i32, stderr: &str) -> Self {
        Self {
            exit_code: Some(code),
            stdout: String::new(),
            stderr: stderr.into(),
            duration_ms: 0,
            truncated: false,
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error("`{program}` is not on AGASTHY's command allowlist")]
    NotAllowed { program: String },
    #[error("`{program}` was not found")]
    NotFound { program: String },
    #[error("`{program}` did not finish within {after_ms} ms and was stopped")]
    TimedOut { program: String, after_ms: u64 },
    #[error("could not start `{program}`: {message}")]
    Spawn { program: String, message: String },
    #[error("permission denied running `{program}`")]
    PermissionDenied { program: String },
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CommandError>;

    /// Resolve a bare program name to the executable that `run` would use.
    fn resolve(&self, program: &str) -> Option<PathBuf>;
}

/// Base name of a program without directory or Windows executable suffix.
fn base_name(program: &str) -> String {
    // Split on both separators so Windows paths are handled on every host.
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let lower = name.to_ascii_lowercase();
    for suffix in [".exe", ".cmd", ".bat", ".com"] {
        if let Some(stripped) = lower.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    lower
}

pub struct SystemCommandRunner {
    allow: HashSet<String>,
    search_dirs: Vec<PathBuf>,
    exe_suffixes: Vec<String>,
}

impl SystemCommandRunner {
    /// `exe_suffixes` are appended when resolving bare names (`""` for Unix,
    /// `".exe"`, `".cmd"` … on Windows).
    pub fn new(allowed: &[&str], search_dirs: Vec<PathBuf>, exe_suffixes: Vec<String>) -> Self {
        let allow = allowed
            .iter()
            .map(|p| base_name(p))
            .filter(|p| !ALWAYS_DENIED.contains(&p.as_str()))
            .collect();
        let exe_suffixes = if exe_suffixes.is_empty() {
            vec![String::new()]
        } else {
            exe_suffixes
        };
        Self {
            allow,
            search_dirs,
            exe_suffixes,
        }
    }

    /// Allow additional program names (used for version probes of catalogued tools).
    pub fn allow(&mut self, programs: &[&str]) {
        for p in programs {
            let b = base_name(p);
            if !ALWAYS_DENIED.contains(&b.as_str()) {
                self.allow.insert(b);
            }
        }
    }

    /// The allowlist, sorted, for display on the privacy page.
    pub fn allowed_programs(&self) -> Vec<String> {
        let mut v: Vec<String> = self.allow.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn is_allowed(&self, program: &str) -> bool {
        let b = base_name(program);
        !ALWAYS_DENIED.contains(&b.as_str()) && self.allow.contains(&b)
    }

    fn path_env(&self) -> Option<std::ffi::OsString> {
        std::env::join_paths(&self.search_dirs).ok()
    }
}

impl CommandRunner for SystemCommandRunner {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        let p = Path::new(program);
        if p.is_absolute() {
            return p.is_file().then(|| p.to_path_buf());
        }
        if program.contains('/') || program.contains('\\') {
            return None;
        }
        for dir in &self.search_dirs {
            for suffix in &self.exe_suffixes {
                let candidate = dir.join(format!("{program}{suffix}"));
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CommandError> {
        if !self.is_allowed(&spec.program) {
            return Err(CommandError::NotAllowed {
                program: spec.program.clone(),
            });
        }
        let exe = self.resolve(&spec.program).ok_or(CommandError::NotFound {
            program: spec.program.clone(),
        })?;

        let mut cmd = Command::new(&exe);
        cmd.args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0");
        if let Some(path) = self.path_env() {
            cmd.env("PATH", path);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }

        let start = Instant::now();
        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => CommandError::PermissionDenied {
                program: spec.program.clone(),
            },
            std::io::ErrorKind::NotFound => CommandError::NotFound {
                program: spec.program.clone(),
            },
            _ => CommandError::Spawn {
                program: spec.program.clone(),
                message: e.to_string(),
            },
        })?;

        let cap = spec.max_output_bytes;
        let out_handle = child.stdout.take().map(|s| read_capped(s, cap));
        let err_handle = child.stderr.take().map(|s| read_capped(s, cap));

        let status = match child.wait_timeout(spec.timeout) {
            Ok(Some(status)) => status,
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CommandError::TimedOut {
                    program: spec.program.clone(),
                    after_ms: spec.timeout.as_millis() as u64,
                });
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CommandError::Spawn {
                    program: spec.program.clone(),
                    message: e.to_string(),
                });
            }
        };

        let (stdout, t1) = out_handle
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        let (stderr, t2) = err_handle
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();

        Ok(CommandOutput {
            exit_code: status.code(),
            stdout,
            stderr,
            duration_ms: start.elapsed().as_millis() as u64,
            truncated: t1 || t2,
        })
    }
}

fn read_capped<R: Read + Send + 'static>(
    mut r: R,
    cap: usize,
) -> std::thread::JoinHandle<(String, bool)> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        let mut truncated = false;
        loop {
            match r.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len() < cap {
                        let take = n.min(cap - buf.len());
                        buf.extend_from_slice(&chunk[..take]);
                        if take < n {
                            truncated = true;
                        }
                    } else {
                        // Keep draining so the child is not blocked on a full pipe.
                        truncated = true;
                    }
                }
                Err(_) => break,
            }
        }
        (String::from_utf8_lossy(&buf).into_owned(), truncated)
    })
}

/// Whether `p` is a regular file the current user may execute.
#[cfg(unix)]
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Whether `p` is a regular file (Windows decides executability by extension).
#[cfg(not(unix))]
pub fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false)
}

/// Test double: returns scripted outputs and records every call.
///
/// Only for automated tests and explicitly marked development fixtures.
#[derive(Default)]
pub struct MockCommandRunner {
    responses: Mutex<HashMap<String, Result<CommandOutput, CommandError>>>,
    resolvable: Mutex<HashMap<String, PathBuf>>,
    calls: Mutex<Vec<CommandSpec>>,
}

impl MockCommandRunner {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(program: &str, args: &[String]) -> String {
        let mut k = base_name(program);
        for a in args {
            k.push('\u{1f}');
            k.push_str(a);
        }
        k
    }

    /// Script a response for `program args…`. Also makes `program` resolvable.
    pub fn on(&self, program: &str, args: &[&str], result: Result<CommandOutput, CommandError>) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.responses
            .lock()
            .unwrap()
            .insert(Self::key(program, &args), result);
        self.resolvable
            .lock()
            .unwrap()
            .entry(base_name(program))
            .or_insert_with(|| PathBuf::from(format!("/mock/bin/{}", base_name(program))));
    }

    pub fn calls(&self) -> Vec<CommandSpec> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for MockCommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CommandError> {
        self.calls.lock().unwrap().push(spec.clone());
        let b = base_name(&spec.program);
        if ALWAYS_DENIED.contains(&b.as_str()) {
            return Err(CommandError::NotAllowed {
                program: spec.program.clone(),
            });
        }
        self.responses
            .lock()
            .unwrap()
            .get(&Self::key(&spec.program, &spec.args))
            .cloned()
            .unwrap_or(Err(CommandError::NotFound {
                program: spec.program.clone(),
            }))
    }

    fn resolve(&self, program: &str) -> Option<PathBuf> {
        self.resolvable
            .lock()
            .unwrap()
            .get(&base_name(program))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix_dirs() -> Vec<PathBuf> {
        ["/usr/bin", "/bin"].iter().map(PathBuf::from).collect()
    }

    #[test]
    fn refuses_programs_not_on_allowlist() {
        let r = SystemCommandRunner::new(&["git"], unix_dirs(), vec![]);
        let err = r.run(&CommandSpec::new("ls", &[])).unwrap_err();
        assert!(matches!(err, CommandError::NotAllowed { .. }));
        // Absolute paths are checked by file name too.
        let err = r.run(&CommandSpec::new("/bin/ls", &[])).unwrap_err();
        assert!(matches!(err, CommandError::NotAllowed { .. }));
    }

    #[test]
    fn shells_are_denied_even_if_allowlisted() {
        let r = SystemCommandRunner::new(&["sh", "bash", "zsh", "sudo"], unix_dirs(), vec![]);
        for p in ["sh", "/bin/bash", "zsh", "sudo", "SH"] {
            assert!(!r.is_allowed(p), "{p} must be denied");
            assert!(matches!(
                r.run(&CommandSpec::new(p, &["-c", "echo hi"])),
                Err(CommandError::NotAllowed { .. })
            ));
        }
    }

    #[test]
    fn relative_paths_are_not_resolved() {
        let r = SystemCommandRunner::new(&["git"], unix_dirs(), vec![]);
        assert!(r.resolve("./git").is_none());
        assert!(r.resolve("../bin/git").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn runs_a_real_allowlisted_command_without_a_shell() {
        let r = SystemCommandRunner::new(&["echo"], unix_dirs(), vec![]);
        // `$HOME` and `;` must arrive literally: no shell interpretation.
        let out = r
            .run(&CommandSpec::new("echo", &["$HOME", ";", "done"]))
            .unwrap();
        assert!(out.success());
        assert_eq!(out.stdout.trim(), "$HOME ; done");
    }

    #[cfg(unix)]
    #[test]
    fn enforces_timeout() {
        let r = SystemCommandRunner::new(&["sleep"], unix_dirs(), vec![]);
        let start = Instant::now();
        let err = r
            .run(&CommandSpec::new("sleep", &["5"]).timeout(Duration::from_millis(200)))
            .unwrap_err();
        assert!(matches!(err, CommandError::TimedOut { .. }));
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn caps_output() {
        let r = SystemCommandRunner::new(&["yes"], unix_dirs(), vec![]);
        // `yes` never exits; we get the cap and a timeout, not unbounded memory.
        let res = r.run(
            &CommandSpec::new("yes", &[])
                .timeout(Duration::from_millis(300))
                .max_output(1024),
        );
        assert!(matches!(res, Err(CommandError::TimedOut { .. })));
    }

    #[test]
    fn mock_records_calls_and_scripts_results() {
        let m = MockCommandRunner::new();
        m.on(
            "git",
            &["--version"],
            Ok(CommandOutput::ok("git version 2.52.0\n")),
        );
        let out = m.run(&CommandSpec::new("git", &["--version"])).unwrap();
        assert_eq!(out.stdout, "git version 2.52.0\n");
        assert!(m.run(&CommandSpec::new("git", &["status"])).is_err());
        assert!(m.run(&CommandSpec::new("bash", &["-c", "x"])).is_err());
        assert_eq!(m.calls().len(), 3);
        assert!(m.resolve("git").is_some());
        assert!(m.resolve("brew").is_none());
    }

    #[test]
    fn base_name_strips_windows_suffixes() {
        assert_eq!(base_name("C:\\Tools\\NPM.CMD"), "npm");
        assert_eq!(base_name("/opt/homebrew/bin/brew"), "brew");
        assert_eq!(base_name("git.exe"), "git");
    }
}
