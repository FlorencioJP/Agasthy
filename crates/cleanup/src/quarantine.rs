//! AGASTHY's quarantine: removed items are moved (renamed) here on the same
//! volume, recorded in a manifest, and can be restored. Space is only freed
//! when an operation is explicitly purged.

use std::path::{Path, PathBuf};

use agasthy_core::time::now_ms;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::safety::device_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum QuarantineState {
    Quarantined,
    PartiallyRestored,
    Restored,
    Purged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QuarantineItem {
    pub original_path: String,
    pub quarantined_path: String,
    pub size_bytes: u64,
    pub category: String,
    pub restored: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QuarantineOp {
    pub id: String,
    pub created_at: i64,
    pub state: QuarantineState,
    pub items: Vec<QuarantineItem>,
    pub total_bytes: u64,
    pub updated_at: i64,
}

pub struct Quarantine {
    root: PathBuf,
}

/// One path to move, already validated by the caller.
pub struct MoveRequest {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoveReport {
    pub op: QuarantineOp,
    /// Items that could not be moved, with the reason. Nothing after the first
    /// failure is attempted.
    pub failed: Vec<(String, String)>,
}

impl Quarantine {
    pub fn new(root: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn op_dir(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("invalid operation id".into());
        }
        Ok(self.root.join(id))
    }

    fn save(&self, op: &QuarantineOp) -> Result<(), String> {
        let dir = self.op_dir(&op.id)?;
        let tmp = dir.join("manifest.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(op).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join("manifest.json")).map_err(|e| e.to_string())
    }

    pub fn load(&self, id: &str) -> Result<QuarantineOp, String> {
        let p = self.op_dir(id)?.join("manifest.json");
        let text =
            std::fs::read_to_string(&p).map_err(|e| format!("no such quarantine entry: {e}"))?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<QuarantineOp> {
        let mut v: Vec<QuarantineOp> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| self.load(&e.file_name().to_string_lossy()).ok())
            .collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    /// Whether `path` can be moved here with a rename (same volume).
    pub fn same_volume(&self, path: &Path) -> bool {
        matches!((device_of(path), device_of(&self.root)), (Some(a), Some(b)) if a == b)
    }

    /// Move validated paths into a new quarantine operation.
    pub fn move_in(&self, requests: Vec<MoveRequest>) -> Result<MoveReport, String> {
        let id = format!("op-{}-{}", now_ms(), std::process::id());
        let dir = self.op_dir(&id)?;
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        let mut op = QuarantineOp {
            id: id.clone(),
            created_at: now_ms(),
            updated_at: now_ms(),
            state: QuarantineState::Quarantined,
            items: Vec::new(),
            total_bytes: 0,
        };
        let mut failed = Vec::new();
        for (i, r) in requests.into_iter().enumerate() {
            if !self.same_volume(&r.path) {
                failed.push((
                    r.path.display().to_string(),
                    "on a different volume from the quarantine; not moved".into(),
                ));
                break;
            }
            let base = r
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "item".into());
            let target = dir.join(format!("{i:03}-{base}"));
            if let Err(e) = std::fs::rename(&r.path, &target) {
                failed.push((r.path.display().to_string(), e.to_string()));
                break;
            }
            // Verify the move before recording it as done.
            if r.path.exists() || !target.exists() {
                failed.push((
                    r.path.display().to_string(),
                    "move could not be verified".into(),
                ));
                break;
            }
            op.total_bytes += r.size_bytes;
            op.items.push(QuarantineItem {
                original_path: r.path.display().to_string(),
                quarantined_path: target.display().to_string(),
                size_bytes: r.size_bytes,
                category: r.category,
                restored: false,
            });
            // Persist after each item so a crash never loses track of moved data.
            self.save(&op)?;
        }
        if op.items.is_empty() {
            let _ = std::fs::remove_dir(&dir);
        } else {
            self.save(&op)?;
        }
        Ok(MoveReport { op, failed })
    }

    /// Move every item of an operation back. Items whose original location is
    /// occupied again are left in quarantine and reported.
    pub fn restore(&self, id: &str) -> Result<(QuarantineOp, Vec<String>), String> {
        let mut op = self.load(id)?;
        if op.state == QuarantineState::Purged {
            return Err("this entry was permanently deleted and cannot be restored".into());
        }
        let mut problems = Vec::new();
        for item in op.items.iter_mut().filter(|i| !i.restored) {
            let from = PathBuf::from(&item.quarantined_path);
            let to = PathBuf::from(&item.original_path);
            if !from.starts_with(&self.root) {
                problems.push(format!("{}: not inside the quarantine", item.original_path));
                continue;
            }
            if to.exists() {
                problems.push(format!(
                    "{}: something already exists there; left in quarantine",
                    item.original_path
                ));
                continue;
            }
            if let Some(parent) = to.parent()
                && !parent.exists()
            {
                problems.push(format!(
                    "{}: its parent folder no longer exists",
                    item.original_path
                ));
                continue;
            }
            match std::fs::rename(&from, &to) {
                Ok(()) if to.exists() => item.restored = true,
                Ok(()) => problems.push(format!(
                    "{}: restore could not be verified",
                    item.original_path
                )),
                Err(e) => problems.push(format!("{}: {e}", item.original_path)),
            }
        }
        op.state = if op.items.iter().all(|i| i.restored) {
            QuarantineState::Restored
        } else if op.items.iter().any(|i| i.restored) {
            QuarantineState::PartiallyRestored
        } else {
            op.state
        };
        op.updated_at = now_ms();
        self.save(&op)?;
        Ok((op, problems))
    }

    /// Permanently delete the quarantined (not restored) items of an operation.
    pub fn purge(&self, id: &str) -> Result<(QuarantineOp, u64), String> {
        let mut op = self.load(id)?;
        if op.state == QuarantineState::Purged {
            return Ok((op, 0));
        }
        let dir = self.op_dir(id)?;
        let mut freed = 0;
        for item in op.items.iter().filter(|i| !i.restored) {
            let p = PathBuf::from(&item.quarantined_path);
            // Only ever delete inside this operation's quarantine folder.
            if !p.starts_with(&dir) || p == dir {
                return Err(format!(
                    "refusing to delete {} outside the quarantine",
                    p.display()
                ));
            }
            if p.exists() {
                if let Err(e) = std::fs::remove_dir_all(&p) {
                    make_writable(&p);
                    std::fs::remove_dir_all(&p)
                        .map_err(|e2| format!("{}: {e}; retry failed: {e2}", p.display()))?;
                }
                freed += item.size_bytes;
            }
        }
        op.state = QuarantineState::Purged;
        op.updated_at = now_ms();
        self.save(&op)?;
        Ok((op, freed))
    }
}

fn make_writable(p: &Path) {
    for e in walkdir::WalkDir::new(p)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if let Ok(m) = e.metadata() {
            let mut perms = m.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(e.path(), perms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_restore_purge() {
        let d = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(d.path()).unwrap();
        let q = Quarantine::new(root.join("q")).unwrap();
        let a = root.join("proj/node_modules");
        let b = root.join("proj/target");
        for p in [&a, &b] {
            std::fs::create_dir_all(p.join("x")).unwrap();
            std::fs::write(p.join("x/f"), "data").unwrap();
        }
        let rep = q
            .move_in(vec![
                MoveRequest {
                    path: a.clone(),
                    size_bytes: 4,
                    category: "node_modules".into(),
                },
                MoveRequest {
                    path: b.clone(),
                    size_bytes: 4,
                    category: "rust_target".into(),
                },
            ])
            .unwrap();
        assert!(rep.failed.is_empty());
        assert!(!a.exists() && !b.exists());
        assert_eq!(rep.op.total_bytes, 8);
        assert_eq!(q.list().len(), 1);

        // Restore refuses to overwrite something new at the original path.
        std::fs::create_dir_all(&b).unwrap();
        let (op, problems) = q.restore(&rep.op.id).unwrap();
        assert!(a.join("x/f").exists(), "restored with contents");
        assert_eq!(problems.len(), 1);
        assert_eq!(op.state, QuarantineState::PartiallyRestored);

        // Purge deletes only what is still quarantined.
        let (op, freed) = q.purge(&rep.op.id).unwrap();
        assert_eq!(freed, 4);
        assert_eq!(op.state, QuarantineState::Purged);
        assert!(a.exists(), "restored item untouched");
        assert!(q.restore(&rep.op.id).is_err());
    }

    #[test]
    fn rejects_bad_ids() {
        let d = tempfile::tempdir().unwrap();
        let q = Quarantine::new(d.path().join("q")).unwrap();
        assert!(q.load("../etc").is_err());
        assert!(q.purge("../../x").is_err());
        assert!(q.restore("").is_err());
    }
}
