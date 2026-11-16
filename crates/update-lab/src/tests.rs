use std::path::Path;
use std::process::Command;

use agasthy_platform::{CommandError, CommandOutput, SystemCommandRunner};

use super::*;

#[test]
fn edits_package_json_without_reformatting() {
    let text = "{\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"react\":   \"^18.2.0\",\n    \"lodash\": \"4.17.0\"\n  },\n  \"devDependencies\": { \"vite\": \"~5.0.0\" }\n}\n";
    let (out, old, new) = edit_package_json(text, "react", "19.1.0").unwrap();
    assert_eq!((old.as_str(), new.as_str()), ("^18.2.0", "^19.1.0"));
    assert_eq!(
        out,
        text.replace("^18.2.0", "^19.1.0"),
        "only the version changes"
    );
    let (_, _, new) = edit_package_json(text, "lodash", "4.17.21").unwrap();
    assert_eq!(new, "4.17.21");
    let (_, _, new) = edit_package_json(text, "vite", "5.4.0").unwrap();
    assert_eq!(new, "~5.4.0");
    assert!(edit_package_json(text, "missing", "1").is_err());
    assert!(edit_package_json("{\"dependencies\":{\"a\":\"workspace:*\"}}", "a", "1").is_err());
}

#[test]
fn edits_cargo_toml_lines() {
    let text = "[package]\nname = \"serde\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = { version = \"1.0.100\", features = [\"derive\"] }\nanyhow = \"1\"\nlocal = { path = \"../local\" }\n";
    let (out, old, new) = edit_cargo_toml(text, "serde", "1.0.200").unwrap();
    assert_eq!((old.as_str(), new.as_str()), ("1.0.100", "1.0.200"));
    assert!(out.contains("serde = { version = \"1.0.200\", features = [\"derive\"] }"));
    assert!(
        out.contains("name = \"serde\""),
        "the [package] name line is untouched"
    );
    let (out, _, _) = edit_cargo_toml(text, "anyhow", "1.0.90").unwrap();
    assert!(out.contains("anyhow = \"1.0.90\""));
    assert!(edit_cargo_toml(text, "local", "2").is_err());
}

/// Real git; a stub for the package manager so no network or project code runs.
struct TestRunner {
    git: SystemCommandRunner,
}

impl CommandRunner for TestRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CommandError> {
        if spec.program == "npm" {
            let dir = spec.cwd.clone().unwrap();
            std::fs::write(dir.join("package-lock.json"), "{\"lockfileVersion\":3,\"packages\":{\"node_modules/lodash\":{\"version\":\"4.17.21\"}}}\n").unwrap();
            return Ok(CommandOutput::ok("added 1 package"));
        }
        self.git.run(spec)
    }
    fn resolve(&self, program: &str) -> Option<std::path::PathBuf> {
        self.git.resolve(program)
    }
}

fn sh(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn full_lab_lifecycle_never_touches_the_original_until_apply() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let root = agasthy_core::paths::canonical(d.path()).unwrap();
    let repo = root.join("app");
    std::fs::create_dir_all(&repo).unwrap();
    let pkg =
        "{\n  \"name\": \"app\",\n  \"dependencies\": {\n    \"lodash\": \"4.17.0\"\n  }\n}\n";
    std::fs::write(repo.join("package.json"), pkg).unwrap();
    std::fs::write(repo.join("package-lock.json"), "{}\n").unwrap();
    sh(&repo, &["init", "-q"]);
    sh(&repo, &["add", "."]);
    sh(&repo, &["commit", "-q", "-m", "init"]);
    std::fs::write(repo.join("notes.txt"), "uncommitted work\n").unwrap();

    let dirs: Vec<_> = std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect();
    let runner = TestRunner {
        git: SystemCommandRunner::new(
            &["git"],
            dirs,
            if cfg!(windows) {
                vec![".exe".into()]
            } else {
                vec![String::new()]
            },
        ),
    };
    let store = LabStore::new(root.join("labs")).unwrap();
    let spec = LabSpec {
        repo_id: "repository:app".into(),
        repo_path: repo.display().to_string(),
        ecosystem: LabEcosystem::Npm,
        package: "lodash".into(),
        target_version: "4.17.21".into(),
    };

    let p = preview(&runner, &spec).unwrap();
    assert_eq!(p.dirty_files, vec!["notes.txt"]);
    assert_eq!(
        (p.old_constraint.as_str(), p.new_constraint.as_str()),
        ("4.17.0", "4.17.21")
    );

    let lab = store.create(&runner, spec).unwrap();
    assert!(Path::new(&lab.worktree).join("package.json").exists());
    assert_eq!(
        std::fs::read_to_string(repo.join("package.json")).unwrap(),
        pkg,
        "original untouched"
    );
    assert!(
        std::fs::read_to_string(Path::new(&lab.worktree).join("package.json"))
            .unwrap()
            .contains("4.17.21")
    );
    assert!(
        !Path::new(&lab.worktree).join("notes.txt").exists(),
        "uncommitted work is not copied into the lab"
    );

    // Apply is blocked before a successful install.
    assert!(!store.apply_blockers(&runner, &lab).is_empty());
    let lab = store
        .run_step(&runner, &lab.id, LabStep::Install, false)
        .unwrap();
    assert!(lab.steps[0].ok);
    assert!(
        lab.steps[0].command.contains("--ignore-scripts"),
        "scripts are off unless allowed"
    );
    assert!(lab.changes.as_ref().unwrap().contains("package-lock.json"));
    assert_eq!(
        std::fs::read_to_string(repo.join("package-lock.json")).unwrap(),
        "{}\n",
        "original lockfile untouched"
    );

    // A local edit to the manifest blocks apply.
    std::fs::write(repo.join("package.json"), format!("{pkg} ")).unwrap();
    assert!(
        store
            .apply_blockers(&runner, &lab)
            .iter()
            .any(|b| b.contains("uncommitted"))
    );
    std::fs::write(repo.join("package.json"), pkg).unwrap();

    let (lab, copied) = store.apply(&runner, &lab.id).unwrap();
    assert_eq!(copied.len(), 2);
    assert!(
        std::fs::read_to_string(repo.join("package.json"))
            .unwrap()
            .contains("4.17.21")
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("notes.txt")).unwrap(),
        "uncommitted work\n"
    );
    assert_eq!(lab.state, LabState::Applied);

    let lab = store.discard(&runner, &lab.id).unwrap();
    assert_eq!(lab.state, LabState::Discarded);
    assert!(!Path::new(&lab.worktree).exists());
    assert!(
        repo.join("package.json").exists(),
        "discarding never touches the project"
    );
}
