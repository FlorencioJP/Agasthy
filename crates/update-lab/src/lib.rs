//! Safe Update Lab.
//!
//! A lab is a detached Git worktree of a project at its current commit, kept
//! under AGASTHY's data folder. The proposed dependency change is written only
//! there. Installing and testing run only there, and only when requested as
//! separate approved actions. Results can be applied to the original project
//! (manifest and lockfile only, and only if those files are unchanged there),
//! or the lab is discarded. The original working tree is never modified
//! during validation.

use std::path::{Path, PathBuf};
use std::time::Duration;

use agasthy_core::time::now_ms;
use agasthy_platform::{CommandRunner, CommandSpec};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LabEcosystem {
    Npm,
    Pnpm,
    Cargo,
}

impl LabEcosystem {
    pub fn manifest(&self) -> &'static str {
        match self {
            LabEcosystem::Npm | LabEcosystem::Pnpm => "package.json",
            LabEcosystem::Cargo => "Cargo.toml",
        }
    }
    pub fn lockfile(&self) -> &'static str {
        match self {
            LabEcosystem::Npm => "package-lock.json",
            LabEcosystem::Pnpm => "pnpm-lock.yaml",
            LabEcosystem::Cargo => "Cargo.lock",
        }
    }
    pub fn program(&self) -> &'static str {
        match self {
            LabEcosystem::Npm => "npm",
            LabEcosystem::Pnpm => "pnpm",
            LabEcosystem::Cargo => "cargo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LabStep {
    Install,
    Test,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LabSpec {
    pub repo_id: String,
    pub repo_path: String,
    pub ecosystem: LabEcosystem,
    pub package: String,
    pub target_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StepRecord {
    pub step: LabStep,
    pub command: String,
    pub allow_scripts: bool,
    pub exit_code: Option<i32>,
    pub ok: bool,
    /// Last part of the combined output.
    pub output_tail: String,
    pub duration_ms: u64,
    pub at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LabState {
    Ready,
    Applied,
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Lab {
    pub id: String,
    pub spec: LabSpec,
    pub worktree: String,
    pub base_commit: String,
    pub created_at: i64,
    pub old_constraint: String,
    pub new_constraint: String,
    pub steps: Vec<StepRecord>,
    /// `git diff --stat` and the lockfile diff inside the worktree after install.
    pub changes: Option<String>,
    pub state: LabState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreatePreview {
    pub head: String,
    /// Uncommitted changes in the original; they are NOT part of the lab.
    pub dirty_files: Vec<String>,
    pub manifest: String,
    pub old_constraint: String,
    pub new_constraint: String,
}

fn git(runner: &dyn CommandRunner, repo: &Path, args: &[&str]) -> Result<String, String> {
    let r = repo.display().to_string();
    let mut all = vec![
        "-C",
        r.as_str(),
        "-c",
        "core.fsmonitor=false",
        "-c",
        if cfg!(windows) {
            "core.hooksPath=NUL"
        } else {
            "core.hooksPath=/dev/null"
        },
    ];
    all.extend_from_slice(args);
    let out = runner
        .run(&CommandSpec::new("git", &all).timeout(Duration::from_secs(60)))
        .map_err(|e| e.to_string())?;
    if !out.success() {
        // Git prints progress first ("Preparing worktree …"); the reason is the
        // `fatal:`/`error:` line, or else the last line.
        let lines: Vec<&str> = out
            .stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let reason = lines
            .iter()
            .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
            .or(lines.last())
            .copied()
            .unwrap_or("git failed");
        return Err(reason.to_string());
    }
    Ok(out.stdout)
}

/// Keep the operator style the project uses (`^`, `~`, `>=` or exact).
pub fn new_constraint(old: &str, target: &str) -> String {
    let op: String = old.chars().take_while(|c| "^~>=<".contains(*c)).collect();
    let target = target.trim().trim_start_matches(['^', '~', '=', 'v']);
    match op.as_str() {
        "^" | "~" | ">=" => format!("{op}{target}"),
        _ => target.to_string(),
    }
}

/// Replace the version of `package` in a package.json without reformatting it.
pub fn edit_package_json(
    text: &str,
    package: &str,
    target: &str,
) -> Result<(String, String, String), String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("package.json: {e}"))?;
    let section = ["dependencies", "devDependencies", "optionalDependencies"]
        .into_iter()
        .find(|s| v.get(s).and_then(|d| d.get(package)).is_some())
        .ok_or_else(|| format!("`{package}` is not a direct dependency in package.json"))?;
    let old = v[section][package]
        .as_str()
        .ok_or("unsupported dependency value")?
        .to_string();
    if old.starts_with("workspace:")
        || old.starts_with("file:")
        || old.contains("://")
        || old.starts_with("git")
    {
        return Err(format!("`{package}` is not a registry version ({old})"));
    }
    let new = new_constraint(&old, target);
    // Locate `"section": { ... "package": "old"` textually.
    let sec_at = text
        .find(&format!("\"{section}\""))
        .ok_or("section not found")?;
    let key = format!("\"{package}\"");
    let key_at = text[sec_at..]
        .find(&key)
        .map(|i| sec_at + i)
        .ok_or("dependency not found")?;
    let after = &text[key_at + key.len()..];
    let colon = after.find(':').ok_or("malformed")?;
    let rest = &after[colon + 1..];
    let q1 = rest.find('"').ok_or("malformed")?;
    let q2 = rest[q1 + 1..].find('"').ok_or("malformed")? + q1 + 1;
    if rest[q1 + 1..q2] != old {
        return Err("could not locate the version text safely".into());
    }
    let start = key_at + key.len() + colon + 1 + q1 + 1;
    let end = key_at + key.len() + colon + 1 + q2;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(&new);
    out.push_str(&text[end..]);
    Ok((out, old, new))
}

/// Replace the version of `package` in Cargo.toml's dependency tables, line-wise.
pub fn edit_cargo_toml(
    text: &str,
    package: &str,
    target: &str,
) -> Result<(String, String, String), String> {
    let mut section = String::new();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for line in lines.iter_mut() {
        let t = line.trim_start();
        if t.starts_with('[') {
            section = t.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        if !matches!(
            section.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies" | "workspace.dependencies"
        ) {
            continue;
        }
        let Some(rest) = t.strip_prefix(package) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let (prefix_len, quoted) = if value.starts_with('"') {
            (0, value)
        } else if value.starts_with('{') {
            let i = value
                .find("version")
                .ok_or_else(|| format!("`{package}` has no version (path or git dependency)"))?;
            let v = &value[i..];
            let q = v.find('"').ok_or("malformed")?;
            (i + q, &v[q..])
        } else {
            continue;
        };
        let end = quoted[1..].find('"').ok_or("malformed")? + 1;
        let old = quoted[1..end].to_string();
        let new = new_constraint(&old, target);
        let value_start = line.len() - value.len();
        let abs = value_start + prefix_len;
        line.replace_range(abs + 1..abs + end, &new);
        let mut out = lines.join("\n");
        if text.ends_with('\n') {
            out.push('\n');
        }
        return Ok((out, old, new));
    }
    Err(format!(
        "`{package}` is not a dependency with a version in Cargo.toml"
    ))
}

fn edit_manifest(
    eco: LabEcosystem,
    text: &str,
    package: &str,
    target: &str,
) -> Result<(String, String, String), String> {
    match eco {
        LabEcosystem::Npm | LabEcosystem::Pnpm => edit_package_json(text, package, target),
        LabEcosystem::Cargo => edit_cargo_toml(text, package, target),
    }
}

fn validate_spec(spec: &LabSpec) -> Result<(), String> {
    let ok_name = |s: &str| {
        !s.is_empty()
            && s.len() < 214
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.@/".contains(c))
    };
    if !ok_name(&spec.package) {
        return Err("invalid package name".into());
    }
    if spec.target_version.is_empty()
        || !spec
            .target_version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-+^~=".contains(c))
    {
        return Err("invalid target version".into());
    }
    Ok(())
}

pub fn preview(runner: &dyn CommandRunner, spec: &LabSpec) -> Result<CreatePreview, String> {
    validate_spec(spec)?;
    let repo = Path::new(&spec.repo_path);
    let head = git(runner, repo, &["rev-parse", "HEAD"])?
        .trim()
        .to_string();
    let dirty: Vec<String> = git(runner, repo, &["status", "--porcelain"])?
        .lines()
        .map(|l| l.get(3..).unwrap_or(l).to_string())
        .collect();
    let manifest = spec.ecosystem.manifest();
    let committed = git(runner, repo, &["show", &format!("HEAD:{manifest}")])?;
    let (_, old, new) = edit_manifest(
        spec.ecosystem,
        &committed,
        &spec.package,
        &spec.target_version,
    )?;
    if old == new {
        return Err(format!("`{}` is already at {old}", spec.package));
    }
    Ok(CreatePreview {
        head,
        dirty_files: dirty,
        manifest: manifest.into(),
        old_constraint: old,
        new_constraint: new,
    })
}

pub struct LabStore {
    dir: PathBuf,
}

impl LabStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir: std::fs::canonicalize(&dir)?,
        })
    }

    fn lab_dir(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("invalid lab id".into());
        }
        Ok(self.dir.join(id))
    }

    pub fn save(&self, lab: &Lab) -> Result<(), String> {
        let p = self.lab_dir(&lab.id)?.join("lab.json");
        std::fs::write(
            &p,
            serde_json::to_vec_pretty(lab).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn load(&self, id: &str) -> Result<Lab, String> {
        let text = std::fs::read_to_string(self.lab_dir(id)?.join("lab.json"))
            .map_err(|e| format!("lab not found: {e}"))?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<Lab> {
        let mut v: Vec<Lab> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| self.load(&e.file_name().to_string_lossy()).ok())
            .collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    /// Create the worktree and write the proposed change into it.
    pub fn create(&self, runner: &dyn CommandRunner, spec: LabSpec) -> Result<Lab, String> {
        let p = preview(runner, &spec)?;
        let id = format!("lab-{}", now_ms());
        let dir = self.lab_dir(&id)?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let wt = dir.join("worktree");
        let repo = Path::new(&spec.repo_path);
        git(
            runner,
            repo,
            &[
                "worktree",
                "add",
                "--detach",
                &wt.display().to_string(),
                &p.head,
            ],
        )?;
        let manifest_path = wt.join(spec.ecosystem.manifest());
        let text = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
        let (edited, old, new) =
            edit_manifest(spec.ecosystem, &text, &spec.package, &spec.target_version)?;
        std::fs::write(&manifest_path, edited).map_err(|e| e.to_string())?;
        let lab = Lab {
            id,
            spec,
            worktree: wt.display().to_string(),
            base_commit: p.head,
            created_at: now_ms(),
            old_constraint: old,
            new_constraint: new,
            steps: vec![],
            changes: None,
            state: LabState::Ready,
        };
        self.save(&lab)?;
        Ok(lab)
    }

    pub fn step_command(lab: &Lab, step: LabStep, allow_scripts: bool) -> CommandSpec {
        let eco = lab.spec.ecosystem;
        let args: Vec<String> = match (eco, step) {
            (LabEcosystem::Npm, LabStep::Install) => {
                let mut a = vec!["install".into(), "--no-audit".into(), "--no-fund".into()];
                if !allow_scripts {
                    a.push("--ignore-scripts".into());
                }
                a
            }
            (LabEcosystem::Pnpm, LabStep::Install) => {
                let mut a = vec!["install".into(), "--no-frozen-lockfile".into()];
                if !allow_scripts {
                    a.push("--ignore-scripts".into());
                }
                a
            }
            (LabEcosystem::Cargo, LabStep::Install) => vec![
                "update".into(),
                "--package".into(),
                lab.spec.package.clone(),
            ],
            (LabEcosystem::Npm | LabEcosystem::Pnpm, LabStep::Test) => vec!["test".into()],
            (LabEcosystem::Cargo, LabStep::Test) => vec!["test".into()],
        };
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        CommandSpec::new(eco.program(), &refs)
            .cwd(&lab.worktree)
            .timeout(Duration::from_secs(1200))
            .max_output(4 * 1024 * 1024)
            .env("CI", "1")
            .env("NO_UPDATE_NOTIFIER", "1")
            .env("npm_config_audit", "false")
    }

    /// Run install or test inside the worktree and record the result.
    pub fn run_step(
        &self,
        runner: &dyn CommandRunner,
        id: &str,
        step: LabStep,
        allow_scripts: bool,
    ) -> Result<Lab, String> {
        let mut lab = self.load(id)?;
        if lab.state != LabState::Ready {
            return Err("this lab is no longer active".into());
        }
        if !Path::new(&lab.worktree).starts_with(&self.dir) {
            return Err("lab worktree is outside AGASTHY's lab folder".into());
        }
        let spec = Self::step_command(&lab, step, allow_scripts);
        let started = now_ms();
        let (exit_code, ok, output) = match runner.run(&spec) {
            Ok(o) => (
                o.exit_code,
                o.success(),
                format!("{}\n{}", o.stdout, o.stderr),
            ),
            Err(e) => (None, false, e.to_string()),
        };
        let tail: String = {
            let chars: Vec<char> = output.chars().collect();
            chars[chars.len().saturating_sub(4000)..].iter().collect()
        };
        lab.steps.push(StepRecord {
            step,
            command: spec.display(),
            allow_scripts,
            exit_code,
            ok,
            output_tail: tail,
            duration_ms: (now_ms() - started).max(0) as u64,
            at: now_ms(),
        });
        if step == LabStep::Install {
            let wt = Path::new(&lab.worktree);
            let stat = git(runner, wt, &["diff", "--stat"]).unwrap_or_default();
            let lock =
                git(runner, wt, &["diff", "--", lab.spec.ecosystem.lockfile()]).unwrap_or_default();
            let mut changes = stat;
            if !lock.is_empty() {
                changes.push('\n');
                changes.push_str(&lock.chars().take(20_000).collect::<String>());
            }
            lab.changes = Some(changes);
        }
        self.save(&lab)?;
        Ok(lab)
    }

    /// Why applying is not possible right now, if it is not.
    pub fn apply_blockers(&self, runner: &dyn CommandRunner, lab: &Lab) -> Vec<String> {
        let mut b = Vec::new();
        if lab.state != LabState::Ready {
            b.push("the lab is no longer active".into());
            return b;
        }
        let repo = Path::new(&lab.spec.repo_path);
        match git(runner, repo, &["rev-parse", "HEAD"]) {
            Ok(h) if h.trim() == lab.base_commit => {}
            Ok(_) => b.push("the project has new commits since the lab was created".into()),
            Err(e) => b.push(format!("cannot read the project: {e}")),
        }
        let files = [lab.spec.ecosystem.manifest(), lab.spec.ecosystem.lockfile()];
        match git(
            runner,
            repo,
            &["status", "--porcelain", "--", files[0], files[1]],
        ) {
            Ok(s) if s.trim().is_empty() => {}
            Ok(_) => b.push(format!(
                "{} or {} has uncommitted changes in the project",
                files[0], files[1]
            )),
            Err(e) => b.push(format!("cannot read the project status: {e}")),
        }
        if !lab.steps.iter().any(|s| s.step == LabStep::Install && s.ok) {
            b.push("run a successful install in the lab first, so the lockfile matches".into());
        }
        b
    }

    /// Copy the manifest and lockfile from the lab into the project.
    pub fn apply(
        &self,
        runner: &dyn CommandRunner,
        id: &str,
    ) -> Result<(Lab, Vec<String>), String> {
        let mut lab = self.load(id)?;
        let blockers = self.apply_blockers(runner, &lab);
        if !blockers.is_empty() {
            return Err(blockers.join("; "));
        }
        let mut copied = Vec::new();
        for f in [lab.spec.ecosystem.manifest(), lab.spec.ecosystem.lockfile()] {
            let from = Path::new(&lab.worktree).join(f);
            let to = Path::new(&lab.spec.repo_path).join(f);
            if !from.is_file() {
                continue;
            }
            let bytes = std::fs::read(&from).map_err(|e| e.to_string())?;
            std::fs::write(&to, &bytes).map_err(|e| e.to_string())?;
            if std::fs::read(&to).map_err(|e| e.to_string())? != bytes {
                return Err(format!("{f} could not be verified after writing"));
            }
            copied.push(to.display().to_string());
        }
        lab.state = LabState::Applied;
        self.save(&lab)?;
        Ok((lab, copied))
    }

    /// Remove the worktree (it lives in AGASTHY's lab folder) and mark the lab discarded.
    pub fn discard(&self, runner: &dyn CommandRunner, id: &str) -> Result<Lab, String> {
        let mut lab = self.load(id)?;
        let wt = PathBuf::from(&lab.worktree);
        if !wt.starts_with(&self.dir) {
            return Err("lab worktree is outside AGASTHY's lab folder".into());
        }
        if wt.exists() {
            git(
                runner,
                Path::new(&lab.spec.repo_path),
                &["worktree", "remove", "--force", &wt.display().to_string()],
            )?;
        }
        let _ = git(
            runner,
            Path::new(&lab.spec.repo_path),
            &["worktree", "prune"],
        );
        lab.state = LabState::Discarded;
        self.save(&lab)?;
        Ok(lab)
    }
}

#[cfg(test)]
mod tests;
