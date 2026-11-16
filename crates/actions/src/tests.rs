//! Engine tests. Every filesystem target lives in a temporary directory and
//! the only process signalled is one the test starts itself.

use std::sync::Mutex;

use agasthy_cleanup::{CleanupCategory, SafetyLevel};
use agasthy_platform::MockCommandRunner;

use super::*;

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    cache: PathBuf,
    quarantine: Quarantine,
    monitor: Mutex<ProcessMonitor>,
    runner: MockCommandRunner,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = agasthy_core::paths::canonical(dir.path()).unwrap();
    let home = root.join("home");
    let cache = home.join(".npm/_cacache");
    std::fs::create_dir_all(cache.join("x")).unwrap();
    std::fs::write(cache.join("x/blob"), vec![0u8; 2048]).unwrap();
    let quarantine = Quarantine::new(root.join("data/quarantine")).unwrap();
    Fixture {
        _dir: dir,
        home,
        cache,
        quarantine,
        monitor: Mutex::new(ProcessMonitor::new()),
        runner: MockCommandRunner::new(),
    }
}

fn ctx<'a>(f: &'a Fixture, eligible: bool) -> ActionContext<'a> {
    let c = CleanupCandidate {
        id: f.cache.display().to_string(),
        category: CleanupCategory::PackageCache,
        path: f.cache.display().to_string(),
        size_bytes: Availability::available(2048),
        last_modified: None,
        safety: SafetyLevel::Safe,
        reason: "npm's package cache".into(),
        regenerate: None,
        project: None,
        eligible,
        blocked_by: (!eligible).then(|| "blocked in analysis".into()),
    };
    ActionContext {
        runner: &f.runner,
        monitor: &f.monitor,
        quarantine: &f.quarantine,
        home: f.home.clone(),
        scan_roots: vec![],
        repos: vec![],
        labs: None,
        repo_index: vec![],
        analysis: Some(DiskAnalysis {
            analysed_at: 0,
            candidates: vec![c],
            totals: vec![],
            eligible_bytes: 2048,
            docker: Availability::unavailable("test"),
            issues: vec![],
        }),
    }
}

#[test]
fn plan_execute_restore_purge_with_audit() {
    let f = fixture();
    let engine = ActionEngine::new();
    let c = ctx(&f, true);
    let mut log: Vec<(String, String, String)> = Vec::new();
    let mut audit = |a: &str, t: &str, o: &str, _d: &str| log.push((a.into(), t.into(), o.into()));

    let plan = engine.plan(
        &c,
        ActionRequest::QuarantinePaths {
            paths: vec![f.cache.display().to_string()],
        },
    );
    assert!(plan.can_execute, "{plan:?}");
    assert_eq!(plan.total_bytes, Some(2048));
    assert!(plan.reversible);
    assert!(f.cache.exists(), "planning changes nothing");

    let token = plan.token.clone().unwrap();
    let r = engine.execute(&c, &token, &mut audit);
    assert_eq!(r.outcome, ActionOutcome::Succeeded, "{r:?}");
    assert!(!f.cache.exists());
    let op = r.quarantine_op.clone().unwrap();

    // Tokens are single use.
    let again = engine.execute(&c, &token, &mut audit);
    assert_eq!(again.outcome, ActionOutcome::Failed);

    let restore = engine.plan(&c, ActionRequest::RestoreQuarantine { op_id: op.clone() });
    let r = engine.execute(&c, restore.token.as_ref().unwrap(), &mut audit);
    assert_eq!(r.outcome, ActionOutcome::Succeeded);
    assert!(f.cache.join("x/blob").exists(), "restored intact");

    // Nothing left to purge for a fully restored operation.
    let purge = engine.plan(&c, ActionRequest::PurgeQuarantine { op_id: op });
    assert!(!purge.can_execute);

    assert_eq!(
        log.len(),
        3,
        "every execution is audited, including the refused reuse"
    );
    assert_eq!(log[0].0, "cleanup.quarantine");
    assert_eq!(log[0].2, "ok");
    assert_eq!(
        log[1],
        (
            "action.execute".into(),
            "confirmation token".into(),
            "refused".into()
        )
    );
}

#[test]
fn ineligible_or_changed_targets_are_refused() {
    let f = fixture();
    let engine = ActionEngine::new();
    let blocked = engine.plan(
        &ctx(&f, false),
        ActionRequest::QuarantinePaths {
            paths: vec![f.cache.display().to_string()],
        },
    );
    assert!(!blocked.can_execute);
    assert!(blocked.token.is_none());

    let unknown = engine.plan(
        &ctx(&f, true),
        ActionRequest::QuarantinePaths {
            paths: vec!["/etc".into()],
        },
    );
    assert!(!unknown.can_execute);

    // Planned, then the target is replaced by a symlink before execution: refused.
    let c = ctx(&f, true);
    let plan = engine.plan(
        &c,
        ActionRequest::QuarantinePaths {
            paths: vec![f.cache.display().to_string()],
        },
    );
    std::fs::remove_dir_all(&f.cache).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&f.home, &f.cache).unwrap();
    let mut audit = |_: &str, _: &str, _: &str, _: &str| {};
    let r = engine.execute(&c, plan.token.as_ref().unwrap(), &mut audit);
    assert_eq!(r.outcome, ActionOutcome::Failed);
    assert!(f.home.exists(), "nothing was moved");
}

#[cfg(unix)]
#[test]
fn terminates_a_child_and_refuses_itself() {
    let f = fixture();
    let engine = ActionEngine::new();
    let c = ctx(&f, true);
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();
    let snap = f.monitor.lock().unwrap().snapshot(&f.runner, &[]);
    let info = snap
        .processes
        .iter()
        .find(|p| p.pid == pid)
        .unwrap()
        .clone();

    let me = snap
        .processes
        .iter()
        .find(|p| p.pid == snap.own_pid)
        .unwrap();
    let self_plan = engine.plan(
        &c,
        ActionRequest::TerminateProcess {
            pid: me.pid,
            start_time: me.start_time,
            force: true,
        },
    );
    assert!(!self_plan.can_execute);

    let plan = engine.plan(
        &c,
        ActionRequest::TerminateProcess {
            pid,
            start_time: info.start_time,
            force: false,
        },
    );
    assert!(plan.can_execute, "{plan:?}");
    let mut audit = |_: &str, _: &str, _: &str, _: &str| {};
    let r = engine.execute(&c, plan.token.as_ref().unwrap(), &mut audit);
    let _ = child.wait();
    assert_eq!(r.outcome, ActionOutcome::Succeeded, "{r:?}");
}

#[test]
fn docker_unavailable_is_not_executable() {
    let f = fixture();
    let plan = ActionEngine::new().plan(
        &ctx(&f, true),
        ActionRequest::DockerPrune {
            target: DockerPrune::BuildCache,
        },
    );
    assert!(!plan.can_execute);
    assert!(plan.summary.contains("Volumes"));
}

#[test]
fn lab_actions_need_a_lab_store_and_known_repo() {
    let f = fixture();
    let engine = ActionEngine::new();
    let plan = engine.plan(
        &ctx(&f, true),
        ActionRequest::CreateUpdateLab {
            repo_id: "repository:/nope".into(),
            ecosystem: LabEcosystem::Npm,
            package: "x".into(),
            target_version: "1".into(),
        },
    );
    assert!(!plan.can_execute);
    assert!(plan.token.is_none());
}

#[cfg(unix)]
#[test]
fn stops_a_batch_and_skips_protected_targets() {
    let f = fixture();
    let engine = ActionEngine::new();
    let c = ctx(&f, true);
    let mut a = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let mut b = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let snap = f.monitor.lock().unwrap().snapshot(&f.runner, &[]);
    let target = |pid: u32| {
        let p = snap.processes.iter().find(|p| p.pid == pid).unwrap();
        ProcessTarget {
            pid: p.pid,
            start_time: p.start_time,
        }
    };
    let me = target(snap.own_pid);
    let request = ActionRequest::TerminateProcesses {
        targets: vec![target(a.id()), target(b.id()), me, target(a.id())],
    };
    let plan = engine.plan(&c, request);
    assert!(plan.can_execute, "{plan:?}");
    assert_eq!(plan.items.len(), 3, "duplicates collapsed");
    assert_eq!(
        plan.items.iter().filter(|i| i.blocked.is_some()).count(),
        1,
        "AGASTHY itself is refused"
    );
    let mut audited = Vec::new();
    let mut audit = |action: &str, target: &str, outcome: &str, _: &str| {
        audited.push(format!("{action} {target} {outcome}"))
    };
    let r = engine.execute(&c, plan.token.as_ref().unwrap(), &mut audit);
    let _ = a.wait();
    let _ = b.wait();
    assert_eq!(r.outcome, ActionOutcome::PartiallySucceeded, "{r:?}");
    assert!(r.title.starts_with("Stopped 2 of 3"), "{}", r.title);
    assert!(
        audited[0].starts_with("process.terminate_batch"),
        "{audited:?}"
    );
}
