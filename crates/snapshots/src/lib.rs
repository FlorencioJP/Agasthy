//! Environment snapshots.
//!
//! A snapshot is metadata, not a backup: which applications, tools, runtimes,
//! packages, repositories (with commit and branch), AI tools and MCP servers
//! exist, plus the *names* of environment variables. It never contains file
//! contents, credentials or environment values.
//!
//! A reconstruction plan lists commands that would bring back supported items
//! (packages, global tools, repository clones). AGASTHY shows them; it does not
//! run them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use agasthy_core::time::now_ms;
use agasthy_core::{CoreResult, EntityKind, EntityQuery, Store};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SnapshotItem {
    /// `application`, `tool`, `runtime`, `package`, `repository`, `ai_tool`, `mcp_server`, `env_var`.
    pub section: String,
    /// Stable identity within the section (bundle id, package id, path…).
    pub key: String,
    pub name: String,
    pub version: Option<String>,
    /// Section-specific facts used for reconstruction (manager, remote, branch…).
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub items: Vec<SnapshotItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SnapshotSummary {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub hostname: Option<String>,
    pub counts: BTreeMap<String, u32>,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ItemChange {
    pub section: String,
    pub key: String,
    pub name: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SnapshotDiff {
    pub from: String,
    pub to: String,
    pub added: Vec<SnapshotItem>,
    pub removed: Vec<SnapshotItem>,
    /// Same item, different version / commit / branch.
    pub changed: Vec<ItemChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReconstructionStep {
    pub item: SnapshotItem,
    /// Command to run yourself, when one is supported.
    pub command: Option<String>,
    /// Why there is no command, or caveats.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReconstructionPlan {
    pub snapshot: String,
    /// Items in the snapshot that are missing now.
    pub steps: Vec<ReconstructionStep>,
    pub supported: u32,
    pub unsupported: u32,
}

/// Capture a snapshot from AGASTHY's database plus environment variable names.
pub fn capture(
    store: &Store,
    name: &str,
    hostname: Option<String>,
    os: Option<String>,
    env_names: &BTreeSet<String>,
) -> CoreResult<Snapshot> {
    let all = |k: EntityKind| {
        store.list_entities(&EntityQuery {
            kind: Some(k),
            text: None,
            limit: Some(5000),
            offset: None,
        })
    };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let mut items = Vec::new();
    for e in all(EntityKind::Application)? {
        items.push(SnapshotItem {
            section: "application".into(),
            key: s(&e.attributes, "bundle_id")
                .unwrap_or_else(|| e.path.clone().unwrap_or_default()),
            name: e.name,
            version: e.version,
            details: BTreeMap::from([("path".to_string(), e.path.unwrap_or_default())]),
        });
    }
    for (kind, section) in [
        (EntityKind::Binary, "tool"),
        (EntityKind::Runtime, "runtime"),
    ] {
        for e in all(kind)? {
            let mut d = BTreeMap::new();
            d.insert("path".into(), e.path.clone().unwrap_or_default());
            if let Some(h) = e
                .attributes
                .get("install_hint")
                .and_then(|h| h.get("installer"))
                .and_then(|v| v.as_str())
            {
                d.insert("installer".into(), h.into());
            }
            items.push(SnapshotItem {
                section: section.into(),
                key: e.path.clone().unwrap_or(e.id.clone()),
                name: e.name,
                version: e.version,
                details: d,
            });
        }
    }
    for e in all(EntityKind::Package)? {
        let mut d = BTreeMap::new();
        if let Some(m) = s(&e.attributes, "manager") {
            d.insert("manager".into(), m);
        }
        if let Some(k) = s(&e.attributes, "package_kind") {
            d.insert("kind".into(), k);
        }
        items.push(SnapshotItem {
            section: "package".into(),
            key: e.id.clone(),
            name: e.name,
            version: e.version,
            details: d,
        });
    }
    for e in all(EntityKind::Repository)? {
        let a = &e.attributes;
        let mut d = BTreeMap::new();
        d.insert("path".into(), e.path.clone().unwrap_or_default());
        if let Some(r) = a
            .get("remotes")
            .and_then(|r| r.as_array())
            .and_then(|r| r.first())
            && let Some(u) = r.get("url").and_then(|u| u.as_str())
        {
            d.insert("remote".into(), u.into());
        }
        if let Some(h) = a.get("head").and_then(|h| h.get("value"))
            && let Some(b) = h.get("branch").and_then(|b| b.as_str())
        {
            d.insert("branch".into(), b.into());
        }
        if let Some(clean) = a
            .get("working_tree")
            .and_then(|w| w.get("value"))
            .and_then(|w| w.get("clean"))
            .and_then(|c| c.as_bool())
        {
            d.insert(
                "working_tree".into(),
                if clean {
                    "clean".into()
                } else {
                    "uncommitted changes".into()
                },
            );
        }
        items.push(SnapshotItem {
            section: "repository".into(),
            key: e.path.clone().unwrap_or(e.id.clone()),
            name: e.name,
            version: a
                .get("head")
                .and_then(|h| h.get("value"))
                .and_then(|h| h.get("commit_sha"))
                .and_then(|c| c.as_str())
                .map(str::to_string),
            details: d,
        });
    }
    for e in all(EntityKind::AiTool)? {
        if e.attributes.get("installed").and_then(|v| v.as_bool()) != Some(true) {
            continue;
        }
        items.push(SnapshotItem {
            section: "ai_tool".into(),
            key: e.id.clone(),
            name: e.name,
            version: e.version,
            details: BTreeMap::new(),
        });
    }
    for e in all(EntityKind::McpServer)? {
        let mut d = BTreeMap::new();
        if let Some(t) = s(&e.attributes, "tool") {
            d.insert("tool".into(), t);
        }
        if let Some(f) = s(&e.attributes, "fingerprint") {
            d.insert("fingerprint".into(), f);
        }
        items.push(SnapshotItem {
            section: "mcp_server".into(),
            key: e.id.clone(),
            name: e.name,
            version: None,
            details: d,
        });
    }
    for n in env_names {
        items.push(SnapshotItem {
            section: "env_var".into(),
            key: n.clone(),
            name: n.clone(),
            version: None,
            details: BTreeMap::new(),
        });
    }
    items.sort();
    let created_at = now_ms();
    Ok(Snapshot {
        id: format!("snap-{created_at}"),
        name: name.trim().chars().take(80).collect::<String>(),
        created_at,
        hostname,
        os,
        items,
    })
}

pub fn summary(s: &Snapshot, path: &Path) -> SnapshotSummary {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for i in &s.items {
        *counts.entry(i.section.clone()).or_default() += 1;
    }
    SnapshotSummary {
        id: s.id.clone(),
        name: s.name.clone(),
        created_at: s.created_at,
        hostname: s.hostname.clone(),
        counts,
        path: path.display().to_string(),
    }
}

/// Snapshots stored as JSON files in one directory.
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("invalid snapshot id".into());
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    pub fn save(&self, s: &Snapshot) -> Result<SnapshotSummary, String> {
        let p = self.path(&s.id)?;
        let tmp = p.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
        Ok(summary(s, &p))
    }

    pub fn load(&self, id: &str) -> Result<Snapshot, String> {
        let text = std::fs::read_to_string(self.path(id)?)
            .map_err(|e| format!("snapshot not found: {e}"))?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<SnapshotSummary> {
        let mut v: Vec<SnapshotSummary> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let text = std::fs::read_to_string(e.path()).ok()?;
                let s: Snapshot = serde_json::from_str(&text).ok()?;
                Some(summary(&s, &e.path()))
            })
            .collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }
}

fn index(s: &Snapshot) -> BTreeMap<(String, String), &SnapshotItem> {
    s.items
        .iter()
        .map(|i| ((i.section.clone(), i.key.clone()), i))
        .collect()
}

fn fingerprint(i: &SnapshotItem) -> Option<String> {
    let mut v = i.version.clone().unwrap_or_default();
    if let Some(b) = i.details.get("branch") {
        v.push_str(&format!(" on {b}"));
    }
    (!v.is_empty()).then_some(v)
}

pub fn diff(a: &Snapshot, b: &Snapshot) -> SnapshotDiff {
    let (ia, ib) = (index(a), index(b));
    let added = ib
        .iter()
        .filter(|(k, _)| !ia.contains_key(*k))
        .map(|(_, v)| (*v).clone())
        .collect();
    let removed = ia
        .iter()
        .filter(|(k, _)| !ib.contains_key(*k))
        .map(|(_, v)| (*v).clone())
        .collect();
    let changed = ia
        .iter()
        .filter_map(|(k, va)| {
            let vb = ib.get(k)?;
            let (fa, fb) = (fingerprint(va), fingerprint(vb));
            (fa != fb).then(|| ItemChange {
                section: k.0.clone(),
                key: k.1.clone(),
                name: vb.name.clone(),
                before: fa,
                after: fb,
            })
        })
        .collect();
    SnapshotDiff {
        from: a.id.clone(),
        to: b.id.clone(),
        added,
        removed,
        changed,
    }
}

/// Commands that would bring back items present in `snapshot` but missing in `current`.
pub fn reconstruction(snapshot: &Snapshot, current: &Snapshot) -> ReconstructionPlan {
    let missing = diff(snapshot, current).removed;
    let quote = |s: &str| {
        if s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./@:+=".contains(c))
        {
            s.to_string()
        } else {
            format!("'{}'", s.replace('\'', "'\\''"))
        }
    };
    let mut steps = Vec::new();
    for item in missing {
        let (command, note) = match item.section.as_str() {
            "package" => {
                let manager = item
                    .details
                    .get("manager")
                    .map(String::as_str)
                    .unwrap_or("");
                let ver = item.version.clone().unwrap_or_default();
                match manager {
                    "homebrew" if item.key.starts_with("package:homebrew-cask:") => (Some(format!("brew install --cask {}", quote(&item.name))), None),
                    "homebrew" => (Some(format!("brew install {}", quote(&item.name))), (!ver.is_empty()).then(|| format!("Homebrew installs the current version; the snapshot had {ver}."))),
                    "npm" => (Some(format!("npm install --global {}{}", quote(&item.name), if ver.is_empty() { String::new() } else { format!("@{ver}") })), None),
                    "pnpm" => (Some(format!("pnpm add --global {}{}", quote(&item.name), if ver.is_empty() { String::new() } else { format!("@{ver}") })), None),
                    "uv" => (Some(format!("uv tool install {}{}", quote(&item.name), if ver.is_empty() { String::new() } else { format!("=={ver}") })), None),
                    "cargo" => (Some(format!("cargo install {}{}", quote(&item.name), if ver.is_empty() { String::new() } else { format!(" --version {ver}") })), None),
                    "pip" => (None, Some("Packages in the default Python environment are not reinstalled automatically; use a requirements file per project.".into())),
                    _ => (None, Some("No supported reinstall command for this package manager.".into())),
                }
            }
            "repository" => match (item.details.get("remote"), item.details.get("path")) {
                (Some(remote), Some(path)) => {
                    let mut c = format!("git clone {} {}", quote(remote), quote(path));
                    if let Some(sha) = &item.version {
                        c.push_str(&format!(
                            " && git -C {} checkout {}",
                            quote(path),
                            quote(sha)
                        ));
                    }
                    (
                        Some(c),
                        item.details
                            .get("working_tree")
                            .filter(|w| *w != "clean")
                            .map(|_| {
                                "Uncommitted changes at snapshot time cannot be restored."
                                    .to_string()
                            }),
                    )
                }
                _ => (
                    None,
                    Some("The repository had no remote; it cannot be re-cloned.".into()),
                ),
            },
            "env_var" => (
                None,
                Some("Only the name was recorded; set the value yourself.".into()),
            ),
            "application" => (
                None,
                Some("Reinstall the application from its vendor or the App Store.".into()),
            ),
            "tool" | "runtime" => (
                None,
                item.details
                    .get("installer")
                    .map(|i| format!("Was installed via {i}."))
                    .or(Some("Installation source unknown.".into())),
            ),
            _ => (None, Some("Not restorable from a snapshot.".into())),
        };
        steps.push(ReconstructionStep {
            item,
            command,
            note,
        });
    }
    let supported = steps.iter().filter(|s| s.command.is_some()).count() as u32;
    let unsupported = steps.len() as u32 - supported;
    ReconstructionPlan {
        snapshot: snapshot.id.clone(),
        steps,
        supported,
        unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        section: &str,
        key: &str,
        version: Option<&str>,
        details: &[(&str, &str)],
    ) -> SnapshotItem {
        SnapshotItem {
            section: section.into(),
            key: key.into(),
            name: key.rsplit(':').next().unwrap().into(),
            version: version.map(Into::into),
            details: details
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }

    fn snap(id: &str, items: Vec<SnapshotItem>) -> Snapshot {
        Snapshot {
            id: id.into(),
            name: id.into(),
            created_at: 0,
            hostname: None,
            os: None,
            items,
        }
    }

    #[test]
    fn diff_and_reconstruction() {
        let before = snap(
            "a",
            vec![
                item(
                    "package",
                    "package:homebrew:jq",
                    Some("1.7.1"),
                    &[("manager", "homebrew")],
                ),
                item(
                    "package",
                    "package:npm:typescript",
                    Some("5.9.3"),
                    &[("manager", "npm")],
                ),
                item(
                    "repository",
                    "/code/x",
                    Some("abc123"),
                    &[
                        ("remote", "https://github.com/o/x.git"),
                        ("path", "/code/x"),
                        ("branch", "main"),
                        ("working_tree", "uncommitted changes"),
                    ],
                ),
                item("env_var", "OPENAI_API_KEY", None, &[]),
                item(
                    "package",
                    "package:pip:requests",
                    Some("2.0"),
                    &[("manager", "pip")],
                ),
            ],
        );
        let after = snap(
            "b",
            vec![
                item(
                    "package",
                    "package:homebrew:jq",
                    Some("1.8.0"),
                    &[("manager", "homebrew")],
                ),
                item(
                    "package",
                    "package:homebrew:ripgrep",
                    Some("15"),
                    &[("manager", "homebrew")],
                ),
            ],
        );
        let d = diff(&before, &after);
        assert_eq!(d.added.len(), 1);
        assert_eq!(d.removed.len(), 4);
        assert_eq!(d.changed.len(), 1);
        assert_eq!(d.changed[0].before.as_deref(), Some("1.7.1"));

        let plan = reconstruction(&before, &after);
        let cmd = |key: &str| plan.steps.iter().find(|s| s.item.key == key).unwrap();
        assert_eq!(
            cmd("package:npm:typescript").command.as_deref(),
            Some("npm install --global typescript@5.9.3")
        );
        assert_eq!(
            cmd("/code/x").command.as_deref(),
            Some("git clone https://github.com/o/x.git /code/x && git -C /code/x checkout abc123")
        );
        assert!(
            cmd("/code/x")
                .note
                .as_ref()
                .unwrap()
                .contains("Uncommitted")
        );
        assert!(
            cmd("OPENAI_API_KEY").command.is_none(),
            "env values are never stored"
        );
        assert!(cmd("package:pip:requests").command.is_none());
        assert_eq!(plan.supported, 2);
        assert_eq!(plan.unsupported, 2);
    }

    #[test]
    fn shell_quoting_in_commands() {
        let before = snap(
            "a",
            vec![item(
                "repository",
                "/code/my repo",
                None,
                &[
                    ("remote", "https://h/o/r.git"),
                    ("path", "/code/my repo; rm -rf ~"),
                ],
            )],
        );
        let plan = reconstruction(&before, &snap("b", vec![]));
        let c = plan.steps[0].command.clone().unwrap();
        assert!(c.contains("'/code/my repo; rm -rf ~'"), "{c}");
    }

    #[test]
    fn store_roundtrip_and_capture_from_db() {
        let d = tempfile::tempdir().unwrap();
        let st = SnapshotStore::new(d.path().join("snaps")).unwrap();
        let db = Store::open_in_memory().unwrap();
        let env = BTreeSet::from(["PATH".to_string(), "OPENAI_API_KEY".to_string()]);
        let s = capture(
            &db,
            "Before upgrade",
            Some("host".into()),
            Some("macos".into()),
            &env,
        )
        .unwrap();
        assert_eq!(s.items.len(), 2, "empty database: only env var names");
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("sk-"), "no values");
        let sum = st.save(&s).unwrap();
        assert_eq!(sum.counts.get("env_var"), Some(&2));
        assert_eq!(st.list().len(), 1);
        assert_eq!(st.load(&s.id).unwrap(), s);
        assert!(st.load("../x").is_err());
    }
}
