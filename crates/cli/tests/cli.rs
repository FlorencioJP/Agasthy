//! The command-line tool as agents and scripts use it: JSON out, read-only queries.

use std::process::Command;

fn agasthy(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_agasthy"))
        .args(args)
        .output()
        .expect("run agasthy");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s}"))
}

#[test]
fn version_matches_the_package() {
    let (ok, out, _) = agasthy(&["--version"]);
    assert!(ok);
    assert_eq!(out.trim(), format!("agasthy {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn scan_then_query_the_database() {
    // Scans refuse system folders such as /var, where tempdir() lives on macOS.
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let work = dir.path().join("code");
    let repo = work.join("demo-app");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let s = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            s.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&s.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(repo.join("package.json"), r#"{"name":"demo-app"}"#).unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    std::fs::write(repo.join("notes.txt"), "uncommitted").unwrap();

    let db = dir.path().join("agasthy.db");
    let db = db.to_str().unwrap();
    let (ok, _, err) = agasthy(&["scan", work.to_str().unwrap(), "--db", db]);
    assert!(ok, "{err}");

    // Options work before or after the command.
    let (ok, out, err) = agasthy(&["--db", db, "status"]);
    assert!(ok, "{err}");
    let status = json(&out);
    assert_eq!(status["last_scans"][0]["kind"], "repositories");

    let (ok, out, err) = agasthy(&["find", "demo-app", "--kind", "repository", "--db", db]);
    assert!(ok, "{err}");
    let found = json(&out);
    assert_eq!(found["count"], 1);
    let id = found["entities"][0]["id"].as_str().unwrap().to_string();

    let (ok, out, err) = agasthy(&["entity", &id, "--db", db]);
    assert!(ok, "{err}");
    let detail = json(&out);
    assert_eq!(detail["entity"]["name"], "demo-app");
    assert_eq!(
        detail["entity"]["attributes"]["working_tree"]["value"]["untracked"],
        1
    );

    // Querying never writes: the database is byte-for-byte the same afterwards.
    let before = std::fs::read(db).unwrap();
    for args in [
        vec!["status"],
        vec!["find"],
        vec!["entity", &id],
        vec!["shared"],
    ] {
        let mut a = args.clone();
        a.extend(["--db", db]);
        assert!(agasthy(&a).0, "{a:?}");
    }
    assert_eq!(before, std::fs::read(db).unwrap());
}

#[test]
fn clear_errors_for_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("none.db");
    let (ok, _, err) = agasthy(&["status", "--db", missing.to_str().unwrap()]);
    assert!(!ok);
    assert!(err.contains("no AGASTHY database"), "{err}");
    assert!(!missing.exists(), "a query must not create a database");

    let (ok, _, err) = agasthy(&[
        "find",
        "--kind",
        "nonsense",
        "--db",
        missing.to_str().unwrap(),
    ]);
    assert!(!ok);
    assert!(err.contains("unknown kind"), "{err}");
}
