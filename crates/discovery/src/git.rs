//! Reading Git metadata through the `git` CLI.
//!
//! Each call passes a fixed argument vector. Repository-local configuration can
//! name programs for git to run (for example `core.fsmonitor`), so those hooks
//! are disabled on the command line for every invocation.

use std::path::Path;
use std::time::Duration;

use agasthy_core::Availability;
use agasthy_platform::{CommandError, CommandRunner, CommandSpec};
use agasthy_provenance::{parse_remote, redact_url};

use crate::attrs::{HeadInfo, RemoteInfo, WorkingTreeStatus};

/// Point Git's hooks folder at the null device so no repository hook can run.
/// (`/dev/null` does not exist on Windows; Git for Windows understands `NUL`.)
#[cfg(windows)]
pub const HOOKS_OFF: &str = "core.hooksPath=NUL";
#[cfg(not(windows))]
pub const HOOKS_OFF: &str = "core.hooksPath=/dev/null";

/// Options placed before every git subcommand.
const SAFE_GIT_OPTS: &[&str] = &[
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=false",
    "-c",
    HOOKS_OFF,
    "-c",
    "diff.external=",
    "-c",
    "color.ui=false",
];

fn git_spec(repo: &Path, args: &[&str], timeout: Duration) -> CommandSpec {
    let repo_s = repo.display().to_string();
    let mut all: Vec<&str> = vec!["-C", &repo_s];
    all.extend_from_slice(SAFE_GIT_OPTS);
    all.extend_from_slice(args);
    CommandSpec::new("git", &all)
        .timeout(timeout)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
}

fn run_git(
    runner: &dyn CommandRunner,
    repo: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<agasthy_platform::CommandOutput, CommandError> {
    runner.run(&git_spec(repo, args, timeout))
}

fn err_to_availability<T>(e: CommandError) -> Availability<T> {
    match e {
        CommandError::NotFound { .. } => Availability::unavailable("git is not installed"),
        CommandError::PermissionDenied { .. } => {
            Availability::permission_denied("permission denied running git")
        }
        other => Availability::unavailable(other.to_string()),
    }
}

pub fn git_available(runner: &dyn CommandRunner) -> bool {
    runner.resolve("git").is_some()
}

pub fn read_head(runner: &dyn CommandRunner, repo: &Path) -> Availability<HeadInfo> {
    let t = Duration::from_secs(10);
    let branch = match run_git(
        runner,
        repo,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        t,
    ) {
        Ok(o) if o.success() => Some(o.stdout.trim().to_string()).filter(|s| !s.is_empty()),
        Ok(_) => None, // detached HEAD
        Err(e) => return err_to_availability(e),
    };
    match run_git(runner, repo, &["log", "-1", "--format=%H%x1f%ct"], t) {
        Ok(o) if o.success() => {
            let line = o.stdout.trim();
            let mut parts = line.split('\u{1f}');
            let sha = parts
                .next()
                .filter(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_string);
            let at = parts
                .next()
                .and_then(|s| s.trim().parse::<i64>().ok())
                .map(|secs| secs * 1000);
            Availability::available(HeadInfo {
                detached: branch.is_none(),
                branch,
                unborn: false,
                commit_sha: sha,
                last_commit_at: at,
            })
        }
        Ok(o) => {
            let stderr = o.stderr.to_lowercase();
            if stderr.contains("does not have any commits")
                || stderr.contains("bad default revision")
            {
                Availability::available(HeadInfo {
                    detached: false,
                    branch,
                    unborn: true,
                    commit_sha: None,
                    last_commit_at: None,
                })
            } else if stderr.contains("dubious ownership") {
                Availability::permission_denied(
                    "git refused to read this repository: it is owned by another user (safe.directory)",
                )
            } else {
                Availability::unavailable(first_line(&o.stderr, "git log failed"))
            }
        }
        Err(e) => err_to_availability(e),
    }
}

pub fn read_status(runner: &dyn CommandRunner, repo: &Path) -> Availability<WorkingTreeStatus> {
    match run_git(
        runner,
        repo,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=dirty",
        ],
        Duration::from_secs(30),
    ) {
        Ok(o) if o.success() => Availability::available(parse_porcelain_z(&o.stdout)),
        Ok(o) => {
            if o.stderr.contains("must be run in a work tree") {
                Availability::unavailable("bare repository: no working tree")
            } else {
                Availability::unavailable(first_line(&o.stderr, "git status failed"))
            }
        }
        Err(e) => err_to_availability(e),
    }
}

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct WorktreeEntry {
    pub path: String,
    pub head: Option<String>,
    /// Short branch name (`main`), or `None` when detached.
    pub branch: Option<String>,
    pub is_main: bool,
    pub locked: bool,
    /// Git reports the worktree's folder is gone; `git worktree prune` would remove the record.
    pub prunable: bool,
}

pub fn read_worktrees(runner: &dyn CommandRunner, repo: &Path) -> Availability<Vec<WorktreeEntry>> {
    match run_git(
        runner,
        repo,
        &["worktree", "list", "--porcelain"],
        Duration::from_secs(15),
    ) {
        Ok(o) if o.success() => Availability::available(parse_worktrees(&o.stdout)),
        Ok(o) => Availability::unavailable(first_line(&o.stderr, "git worktree list failed")),
        Err(e) => err_to_availability(e),
    }
}

pub fn parse_worktrees(out: &str) -> Vec<WorktreeEntry> {
    let mut v: Vec<WorktreeEntry> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            v.push(WorktreeEntry {
                path: path.to_string(),
                head: None,
                branch: None,
                is_main: v.is_empty(),
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(cur) = v.last_mut() else { continue };
        if let Some(h) = line.strip_prefix("HEAD ") {
            cur.head = Some(h.chars().take(12).collect());
        } else if let Some(b) = line.strip_prefix("branch ") {
            cur.branch = Some(b.trim_start_matches("refs/heads/").to_string());
        } else if line == "locked" || line.starts_with("locked ") {
            cur.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            cur.prunable = true;
        } else if line == "bare" {
            cur.is_main = true;
        }
    }
    v
}

pub fn parse_porcelain_z(out: &str) -> WorkingTreeStatus {
    let mut s = WorkingTreeStatus {
        clean: true,
        staged: 0,
        modified: 0,
        untracked: 0,
        conflicted: 0,
    };
    let mut entries = out.split('\0').filter(|e| !e.is_empty());
    while let Some(entry) = entries.next() {
        let b = entry.as_bytes();
        if b.len() < 3 {
            continue;
        }
        let (x, y) = (b[0] as char, b[1] as char);
        // Renames and copies are followed by the original path as its own entry.
        if x == 'R' || x == 'C' {
            entries.next();
        }
        if x == '?' && y == '?' {
            s.untracked += 1;
        } else if x == '!' {
            continue;
        } else if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
            s.conflicted += 1;
        } else {
            if x != ' ' {
                s.staged += 1;
            }
            if y != ' ' {
                s.modified += 1;
            }
        }
    }
    s.clean = s.staged == 0 && s.modified == 0 && s.untracked == 0 && s.conflicted == 0;
    s
}

pub fn read_branch_count(runner: &dyn CommandRunner, repo: &Path) -> Availability<u32> {
    match run_git(
        runner,
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        Duration::from_secs(10),
    ) {
        Ok(o) if o.success() => {
            Availability::available(o.stdout.lines().filter(|l| !l.trim().is_empty()).count() as u32)
        }
        Ok(o) => Availability::unavailable(first_line(&o.stderr, "git for-each-ref failed")),
        Err(e) => err_to_availability(e),
    }
}

/// Remotes as configured in the repository. URLs are redacted before they leave this function.
pub fn read_remotes(runner: &dyn CommandRunner, repo: &Path) -> Result<Vec<RemoteInfo>, String> {
    match run_git(
        runner,
        repo,
        &["config", "--get-regexp", r"^remote\..*\.url$"],
        Duration::from_secs(10),
    ) {
        Ok(o) if o.success() => Ok(parse_remote_config(&o.stdout)),
        // Exit code 1: no matching keys, i.e. no remotes.
        Ok(o) if o.exit_code == Some(1) => Ok(Vec::new()),
        Ok(o) => Err(first_line(&o.stderr, "git config failed")),
        Err(e) => Err(e.to_string()),
    }
}

pub fn parse_remote_config(out: &str) -> Vec<RemoteInfo> {
    let mut v: Vec<RemoteInfo> = out
        .lines()
        .filter_map(|line| {
            let (key, url) = line.split_once(' ')?;
            let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
            let (safe, redacted) = redact_url(url);
            Some(RemoteInfo {
                name: name.to_string(),
                parsed: parse_remote(&safe),
                url: safe,
                credentials_redacted: redacted,
            })
        })
        .collect();
    // origin first, then alphabetical.
    v.sort_by(|a, b| (a.name != "origin", &a.name).cmp(&(b.name != "origin", &b.name)));
    v.dedup_by(|a, b| a.name == b.name);
    v
}

fn first_line(s: &str, fallback: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(240).collect())
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn worktree_porcelain() {
        let out = "worktree /u/web\nHEAD 0123456789abcdef0123\nbranch refs/heads/main\n\nworktree /u/web/.claude/worktrees/fix-a\nHEAD fedcba9876543210\ndetached\nlocked\n\nworktree /tmp/gone\nHEAD aaaa\nbranch refs/heads/feat/x\nprunable gitdir file points to non-existent location\n";
        let w = super::parse_worktrees(out);
        assert_eq!(w.len(), 3);
        assert!(w[0].is_main && !w[1].is_main);
        assert_eq!(w[0].branch.as_deref(), Some("main"));
        assert_eq!(w[0].head.as_deref(), Some("0123456789ab"));
        assert!(w[1].branch.is_none() && w[1].locked);
        assert!(w[2].prunable);
        assert_eq!(w[2].branch.as_deref(), Some("feat/x"));
    }

    use super::*;
    use agasthy_platform::{CommandOutput, MockCommandRunner};

    #[test]
    fn porcelain_counts() {
        let out = " M src/a.rs\0M  src/b.rs\0MM src/c.rs\0?? new.txt\0R  new\0old\0UU conflict\0";
        let s = parse_porcelain_z(out);
        assert_eq!(s.modified, 2);
        assert_eq!(s.staged, 3);
        assert_eq!(s.untracked, 1);
        assert_eq!(s.conflicted, 1);
        assert!(!s.clean);
        assert!(parse_porcelain_z("").clean);
    }

    #[test]
    fn remote_config_parsing_redacts_and_orders() {
        let out = "remote.upstream.url https://github.com/up/proj.git\n\
                   remote.origin.url https://me:ghp_abc@github.com/me/proj.git\n\
                   remote.my.fork.url git@github.com:fork/proj.git\n";
        let r = parse_remote_config(out);
        assert_eq!(r[0].name, "origin");
        assert_eq!(r[0].url, "https://github.com/me/proj.git");
        assert!(r[0].credentials_redacted);
        assert_eq!(r[1].name, "my.fork");
        assert_eq!(r[2].name, "upstream");
        assert_eq!(r[0].parsed.as_ref().unwrap().owner, "me");
        assert!(!format!("{r:?}").contains("ghp_abc"));
    }

    #[test]
    fn git_missing_is_unavailable_not_empty() {
        let m = MockCommandRunner::new();
        let h = read_head(&m, Path::new("/tmp/x"));
        assert!(matches!(h, Availability::Unavailable { .. }));
    }

    #[test]
    fn every_invocation_disables_repo_configured_programs() {
        let m = MockCommandRunner::new();
        m.on("git", &[], Ok(CommandOutput::ok("")));
        let _ = read_status(&m, Path::new("/tmp/x"));
        let call = &m.calls()[0];
        let joined = call.args.join(" ");
        assert!(joined.contains("core.fsmonitor=false"));
        assert!(joined.contains(HOOKS_OFF));
        assert!(call.env.iter().any(|(k, _)| k == "GIT_CONFIG_NOSYSTEM"));
    }
}
