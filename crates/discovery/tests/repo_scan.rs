//! End-to-end repository discovery against real `git init` repositories in a
//! temporary directory, persisted through the real store.

use std::path::Path;
use std::process::Command;

use agasthy_core::{EntityKind, EntityQuery, RelationKind, Store};
use agasthy_discovery::{RepoScanOptions, attrs::RepositoryAttrs, scan_repositories};
use agasthy_platform::SystemCommandRunner;

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git must be installed to run this test");
    assert!(st.success(), "git {args:?} failed");
}

fn commit(dir: &Path, file: &str) {
    std::fs::write(dir.join(file), "content\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

fn runner() -> SystemCommandRunner {
    let dirs: Vec<std::path::PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect();
    let suffixes = if cfg!(windows) {
        vec![".exe".to_string()]
    } else {
        vec![String::new()]
    };
    SystemCommandRunner::new(&["git"], dirs, suffixes)
}

#[test]
fn discovers_real_repositories() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git not installed; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = agasthy_core::paths::canonical(tmp.path()).unwrap();

    // clean repo with origin on GitHub
    let clean = root.join("projects/clean");
    std::fs::create_dir_all(&clean).unwrap();
    git(&clean, &["init", "-q"]);
    std::fs::write(clean.join(".gitignore"), "node_modules/\n").unwrap();
    commit(&clean, "README.md");
    git(
        &clean,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/Example/Widget.git",
        ],
    );
    git(
        &clean,
        &[
            "remote",
            "add",
            "upstream",
            "git@github.com:upstream/widget.git",
        ],
    );

    // duplicate clone of the same project, dirty, with a credential in its URL
    let dup = root.join("other/widget-copy");
    std::fs::create_dir_all(&dup).unwrap();
    git(&dup, &["init", "-q"]);
    commit(&dup, "a.rs");
    git(
        &dup,
        &[
            "remote",
            "add",
            "origin",
            "https://user:secret-token@github.com/example/widget",
        ],
    );
    std::fs::write(dup.join("a.rs"), "changed\n").unwrap();
    std::fs::write(dup.join("untracked.txt"), "x\n").unwrap();

    // unborn repo without remotes
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q"]);

    // detached HEAD
    let det = root.join("detached");
    std::fs::create_dir_all(&det).unwrap();
    git(&det, &["init", "-q"]);
    commit(&det, "x.py");
    git(&det, &["checkout", "-q", "--detach"]);

    // a repo inside node_modules must not be found
    let nm = root.join("projects/clean/node_modules/dep");
    std::fs::create_dir_all(&nm).unwrap();
    git(&nm, &["init", "-q"]);

    // a repo outside the root, reachable only via a symlink, must not be found
    let outside_tmp = tempfile::tempdir().unwrap();
    let outside = outside_tmp.path().join("outside-repo");
    std::fs::create_dir_all(&outside).unwrap();
    git(&outside, &["init", "-q"]);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("link-to-outside")).unwrap();

    let r = runner();
    let batch = scan_repositories(
        &RepoScanOptions::new(vec![(root.clone(), 8)]),
        &r,
        &agasthy_discovery::no_progress,
    );

    let repos: Vec<_> = batch
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Repository)
        .collect();
    let names: Vec<&str> = repos.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(repos.len(), 4, "found: {names:?}");
    assert!(!names.contains(&"dep"), "node_modules must be skipped");
    assert!(
        !names.contains(&"outside-repo"),
        "symlinks must not be followed"
    );

    let attrs = |name: &str| -> RepositoryAttrs {
        let e = repos.iter().find(|e| e.name == name).unwrap();
        serde_json::from_value(e.attributes.clone()).unwrap()
    };

    let c = attrs("clean");
    let head = c.head.value().unwrap();
    assert_eq!(head.branch.as_deref(), Some("main"));
    assert!(head.commit_sha.as_ref().unwrap().len() == 40);
    assert!(c.working_tree.value().unwrap().clean);
    assert_eq!(c.remotes.len(), 2);
    assert_eq!(c.remotes[0].name, "origin");
    assert_eq!(
        c.primary_remote.as_ref().unwrap().key(),
        "github.com/example/widget"
    );
    assert_eq!(c.clones_of_same_remote, 2);
    assert_eq!(*c.local_branches.value().unwrap(), 1);

    let d = attrs("widget-copy");
    let wt = d.working_tree.value().unwrap();
    assert!(!wt.clean);
    assert_eq!(wt.modified, 1);
    assert_eq!(wt.untracked, 1);
    assert!(d.remotes[0].credentials_redacted);
    let serialized = serde_json::to_string(&batch.entities).unwrap()
        + &serde_json::to_string(&batch.relationships).unwrap();
    assert!(
        !serialized.contains("secret-token"),
        "credentials must never be stored"
    );

    let e = attrs("empty");
    assert!(e.head.value().unwrap().unborn);
    assert!(e.remotes.is_empty());

    let dt = attrs("detached");
    assert!(dt.head.value().unwrap().detached);
    assert_eq!(dt.primary_language.as_deref(), Some("Python"));

    // Persist and query through the real store.
    let mut store = Store::open_in_memory().unwrap();
    let scan = store.begin_scan(batch.scan_kind).unwrap();
    store.ingest(scan, &batch).unwrap();
    assert_eq!(
        store
            .count_relationships(RelationKind::DuplicateClone)
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count_relationships(RelationKind::OriginRemote)
            .unwrap(),
        2
    );
    let hits = store
        .list_entities(&EntityQuery {
            kind: Some(EntityKind::Repository),
            text: Some("widget".into()),
            limit: None,
            offset: None,
        })
        .unwrap();
    assert!(hits.len() >= 2, "FTS finds repos by remote name");
    let remote = store
        .entity_detail("remote:github.com/example/widget")
        .unwrap()
        .unwrap();
    assert_eq!(remote.incoming.len(), 2);

    // Rescan after deleting a repo: it is pruned from AGASTHY's database.
    std::fs::remove_dir_all(&empty).unwrap();
    let batch2 = scan_repositories(
        &RepoScanOptions::new(vec![(root.clone(), 8)]),
        &r,
        &agasthy_discovery::no_progress,
    );
    let scan2 = store.begin_scan(batch2.scan_kind).unwrap();
    let res = store.ingest(scan2, &batch2).unwrap();
    assert_eq!(res.entities_removed, 1);
}
