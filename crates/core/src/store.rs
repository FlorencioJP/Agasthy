//! SQLite persistence.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::time::now_ms;

/// Ordered list of embedded migrations. Index + 1 is the schema version.
const MIGRATIONS: &[&str] = &[
    include_str!("../../../migrations/0001_init.sql"),
    include_str!("../../../migrations/0002_relationship_attributes.sql"),
];

pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

pub struct Store {
    conn: Connection,
    path: Option<PathBuf>,
}

/// Outcome of writing an [`IngestBatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestResult {
    pub scan_id: i64,
    pub entities_written: u32,
    pub entities_removed: u32,
}

impl Store {
    /// Open (or create) the database at `path` and apply pending migrations.
    pub fn open(path: &Path) -> CoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let mut store = Self {
            conn,
            path: Some(path.to_path_buf()),
        };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> CoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        let mut store = Self { conn, path: None };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn configure(&self) -> CoreResult<()> {
        self.conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    }

    pub fn schema_version(&self) -> CoreResult<u32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?)
    }

    fn migrate(&mut self) -> CoreResult<()> {
        let current = self.schema_version()?;
        if current > SCHEMA_VERSION {
            return Err(CoreError::SchemaTooNew {
                found: current,
                supported: SCHEMA_VERSION,
            });
        }
        for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let version = idx as u32 + 1;
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql).map_err(|e| CoreError::Migration {
                version,
                message: e.to_string(),
            })?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
        Ok(())
    }

    // ----- scans and ingest -----

    pub fn begin_scan(&self, kind: ScanKind) -> CoreResult<i64> {
        self.conn.execute(
            "INSERT INTO scans (kind, status, started_at) VALUES (?1, ?2, ?3)",
            params![kind.as_str(), ScanStatus::Running.as_str(), now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Mark a scan as failed without writing any entities.
    pub fn fail_scan(&self, scan_id: i64, issues: &[ScanIssue]) -> CoreResult<()> {
        self.conn.execute(
            "UPDATE scans SET status = ?1, finished_at = ?2, issues = ?3 WHERE id = ?4",
            params![
                ScanStatus::Failed.as_str(),
                now_ms(),
                serde_json::to_string(issues)?,
                scan_id
            ],
        )?;
        Ok(())
    }

    /// Write a completed scan atomically: upsert entities, replace their
    /// evidence, upsert relationships, prune stale rows in scope, close the scan.
    pub fn ingest(&mut self, scan_id: i64, batch: &IngestBatch) -> CoreResult<IngestResult> {
        let now = now_ms();
        let tx = self.conn.transaction()?;
        let mut written = 0u32;
        for e in &batch.entities {
            upsert_entity(&tx, scan_id, now, e)?;
            written += 1;
        }
        for r in &batch.relationships {
            // A relationship to an entity another scan has not produced yet
            // (e.g. a package before the package scan ran) is skipped, not invented.
            if entity_exists(&tx, &r.from_id)? && entity_exists(&tx, &r.to_id)? {
                upsert_relationship(&tx, scan_id, now, r)?;
            }
        }

        let mut removed = 0u32;
        for scope in &batch.prune {
            removed += match &scope.source {
                Some(source) => tx.execute(
                    "DELETE FROM entities WHERE kind = ?1 AND source = ?2
                     AND (last_scan_id IS NULL OR last_scan_id <> ?3)",
                    params![scope.kind.as_str(), source, scan_id],
                )?,
                None => tx.execute(
                    "DELETE FROM entities WHERE kind = ?1
                     AND (last_scan_id IS NULL OR last_scan_id <> ?2)",
                    params![scope.kind.as_str(), scan_id],
                )?,
            } as u32;
        }
        // A relationship of a kind this scan is authoritative for is stale when
        // its source entity was re-observed but the relationship was not.
        // (Relationships of pruned entities go via ON DELETE CASCADE.)
        for kind in scan_relationship_kinds(batch.scan_kind) {
            tx.execute(
                "DELETE FROM relationships WHERE kind = ?1
                 AND (last_scan_id IS NULL OR last_scan_id <> ?2)
                 AND from_id IN (SELECT id FROM entities WHERE last_scan_id = ?2)",
                params![kind.as_str(), scan_id],
            )?;
        }
        // Remote projects are shared between scans (a Git remote and a Homebrew
        // homepage can name the same project). Keep them while anything links to them.
        tx.execute(
            "DELETE FROM entities WHERE kind IN ('remote_project', 'dependency')
             AND id NOT IN (SELECT from_id FROM relationships UNION SELECT to_id FROM relationships)",
            [],
        )?;

        let status = if batch.issues.is_empty() {
            ScanStatus::Completed
        } else {
            ScanStatus::CompletedWithIssues
        };
        tx.execute(
            "UPDATE scans SET status = ?1, finished_at = ?2, entities_found = ?3,
             entities_removed = ?4, issues = ?5 WHERE id = ?6",
            params![
                status.as_str(),
                now_ms(),
                written,
                removed,
                serde_json::to_string(&batch.issues)?,
                scan_id
            ],
        )?;
        tx.commit()?;
        Ok(IngestResult {
            scan_id,
            entities_written: written,
            entities_removed: removed,
        })
    }

    pub fn list_scans(&self, limit: u32) -> CoreResult<Vec<ScanRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, status, started_at, finished_at, entities_found,
                    entities_removed, issues
             FROM scans ORDER BY started_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], scan_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?.into_iter().collect()
    }

    /// The most recent scan of each kind.
    pub fn latest_scans(&self) -> CoreResult<Vec<ScanRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, status, started_at, finished_at, entities_found,
                    entities_removed, issues
             FROM scans s
             WHERE id = (SELECT max(id) FROM scans WHERE kind = s.kind)
             ORDER BY kind",
        )?;
        let rows = stmt.query_map([], scan_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?.into_iter().collect()
    }

    /// Scans left in `running` state by a previous process that exited mid-scan.
    pub fn mark_interrupted_scans(&self) -> CoreResult<usize> {
        let issue = vec![ScanIssue::new(
            IssueKind::Other,
            "scan",
            "AGASTHY exited before this scan finished",
        )];
        Ok(self.conn.execute(
            "UPDATE scans SET status = ?1, finished_at = ?2, issues = ?3 WHERE status = ?4",
            params![
                ScanStatus::Failed.as_str(),
                now_ms(),
                serde_json::to_string(&issue)?,
                ScanStatus::Running.as_str()
            ],
        )?)
    }

    // ----- entity reads -----

    pub fn list_entities(&self, query: &EntityQuery) -> CoreResult<Vec<Entity>> {
        let limit = query.limit.unwrap_or(500).min(5000);
        let offset = query.offset.unwrap_or(0);
        let kind = query.kind.map(|k| k.as_str());
        let text = query
            .text
            .as_deref()
            .and_then(fts_query)
            .filter(|t| !t.is_empty());

        let mut out = Vec::new();
        if let Some(fts) = text {
            let mut stmt = self.conn.prepare(
                "SELECT e.id, e.kind, e.name, e.path, e.version, e.source, e.attributes,
                        e.first_seen, e.last_seen
                 FROM entities_fts f JOIN entities e ON e.id = f.id
                 WHERE entities_fts MATCH ?1 AND (?2 IS NULL OR e.kind = ?2)
                 ORDER BY bm25(entities_fts), e.name COLLATE NOCASE
                 LIMIT ?3 OFFSET ?4",
            )?;
            let rows = stmt.query_map(params![fts, kind, limit, offset], entity_from_row)?;
            for r in rows {
                out.push(r??);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, kind, name, path, version, source, attributes, first_seen, last_seen
                 FROM entities WHERE (?1 IS NULL OR kind = ?1)
                 ORDER BY name COLLATE NOCASE, id LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt.query_map(params![kind, limit, offset], entity_from_row)?;
            for r in rows {
                out.push(r??);
            }
        }
        Ok(out)
    }

    pub fn recent_entities(&self, limit: u32) -> CoreResult<Vec<Entity>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, path, version, source, attributes, first_seen, last_seen
             FROM entities WHERE kind NOT IN ('remote_project', 'dependency', 'ai_config_file', 'mcp_server', 'finding')
             ORDER BY first_seen DESC, name COLLATE NOCASE LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], entity_from_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    pub fn get_entity(&self, id: &str) -> CoreResult<Option<Entity>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, kind, name, path, version, source, attributes, first_seen, last_seen
                 FROM entities WHERE id = ?1",
                [id],
                entity_from_row,
            )
            .optional()?;
        row.transpose()
    }

    pub fn entity_detail(&self, id: &str) -> CoreResult<Option<EntityDetail>> {
        let Some(entity) = self.get_entity(id)? else {
            return Ok(None);
        };
        let evidence = self.evidence_for("entity_id", id)?;
        let outgoing = self.relationships_for(id, true)?;
        let incoming = self.relationships_for(id, false)?;
        Ok(Some(EntityDetail {
            entity,
            evidence,
            outgoing,
            incoming,
        }))
    }

    fn evidence_for(&self, column: &str, id: &str) -> CoreResult<Vec<Evidence>> {
        let sql = format!(
            "SELECT id, kind, detail, source, confidence, observed_at
             FROM evidence WHERE {column} = ?1 ORDER BY id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([id], |r| {
            Ok(Evidence {
                id: r.get(0)?,
                kind: r.get(1)?,
                detail: r.get(2)?,
                source: r.get(3)?,
                confidence: Confidence::parse(&r.get::<_, String>(4)?),
                observed_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn relationships_for(&self, id: &str, outgoing: bool) -> CoreResult<Vec<Relationship>> {
        let (mine, other) = if outgoing {
            ("from_id", "to_id")
        } else {
            ("to_id", "from_id")
        };
        let sql = format!(
            "SELECT r.id, r.kind, r.from_id, r.to_id, r.confidence,
                    o.id, o.kind, o.name, o.path, r.attributes
             FROM relationships r LEFT JOIN entities o ON o.id = r.{other}
             WHERE r.{mine} = ?1 ORDER BY r.kind, o.name"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([id], |r| {
            let other_id: Option<String> = r.get(5)?;
            let other_kind: Option<String> = r.get(6)?;
            let other = match (other_id, other_kind.as_deref().and_then(EntityKind::parse)) {
                (Some(oid), Some(kind)) => Some(EntityRef {
                    id: oid,
                    kind,
                    name: r.get(7)?,
                    path: r.get(8)?,
                }),
                _ => None,
            };
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                other,
                r.get::<_, String>(9)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (rid, kind, from_id, to_id, confidence, other, attrs) = row?;
            let Some(kind) = RelationKind::parse(&kind) else {
                continue;
            };
            let evidence = self.evidence_for("relationship_id", &rid)?;
            out.push(Relationship {
                id: rid,
                kind,
                from_id,
                to_id,
                confidence: Confidence::parse(&confidence),
                attributes: serde_json::from_str(&attrs).unwrap_or(serde_json::Value::Null),
                evidence,
                other,
            });
        }
        Ok(out)
    }

    pub fn counts_by_kind(&self) -> CoreResult<Vec<KindCount>> {
        let mut out: Vec<KindCount> = EntityKind::ALL
            .into_iter()
            .map(|kind| KindCount { kind, count: 0 })
            .collect();
        let mut stmt = self
            .conn
            .prepare("SELECT kind, count(*) FROM entities GROUP BY kind")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?;
        for row in rows {
            let (kind, count) = row?;
            if let Some(slot) = out.iter_mut().find(|c| c.kind.as_str() == kind) {
                slot.count = count;
            }
        }
        Ok(out)
    }

    pub fn relationship_counts(&self) -> CoreResult<Vec<RelationCount>> {
        RelationKind::ALL
            .into_iter()
            .map(|kind| {
                Ok(RelationCount {
                    kind,
                    count: self.count_relationships(kind)?,
                })
            })
            .collect()
    }

    pub fn count_relationships(&self, kind: RelationKind) -> CoreResult<u32> {
        Ok(self.conn.query_row(
            "SELECT count(*) FROM relationships WHERE kind = ?1",
            [kind.as_str()],
            |r| r.get(0),
        )?)
    }

    /// Count entities of a kind whose JSON attribute at `json_path` equals `value`.
    pub fn count_where_attr(
        &self,
        kind: EntityKind,
        json_path: &str,
        value: &serde_json::Value,
    ) -> CoreResult<u32> {
        let v = serde_json::to_string(value)?;
        Ok(self.conn.query_row(
            "SELECT count(*) FROM entities
             WHERE kind = ?1 AND json_extract(attributes, ?2) = json_extract(?3, '$')",
            params![kind.as_str(), json_path, v],
            |r| r.get(0),
        )?)
    }

    // ----- graph queries -----

    /// Lightweight edges (no evidence) touching `id`, in either direction.
    pub fn edges_touching(&self, id: &str) -> CoreResult<Vec<Edge>> {
        self.query_edges(
            "SELECT id, kind, from_id, to_id, confidence, attributes FROM relationships
             WHERE from_id = ?1 OR to_id = ?1",
            params![id],
        )
    }

    /// Edges whose endpoints are both in `ids`.
    pub fn edges_among(&self, ids: &[String]) -> CoreResult<Vec<Edge>> {
        let json = serde_json::to_string(ids)?;
        self.query_edges(
            "SELECT id, kind, from_id, to_id, confidence, attributes FROM relationships
             WHERE from_id IN (SELECT value FROM json_each(?1))
               AND to_id IN (SELECT value FROM json_each(?1))",
            params![json],
        )
    }

    /// Edges of the given kinds, optionally limited to those ending at `to_id`.
    pub fn edges_of_kind(&self, kind: RelationKind, to_id: Option<&str>) -> CoreResult<Vec<Edge>> {
        self.query_edges(
            "SELECT id, kind, from_id, to_id, confidence, attributes FROM relationships
             WHERE kind = ?1 AND (?2 IS NULL OR to_id = ?2)",
            params![kind.as_str(), to_id],
        )
    }

    fn query_edges(&self, sql: &str, p: impl rusqlite::Params) -> CoreResult<Vec<Edge>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(p, |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, from_id, to_id, conf, attrs) = row?;
            let Some(kind) = RelationKind::parse(&kind) else {
                continue;
            };
            out.push(Edge {
                id,
                kind,
                from_id,
                to_id,
                confidence: Confidence::parse(&conf),
                attributes: serde_json::from_str(&attrs).unwrap_or(serde_json::Value::Null),
            });
        }
        Ok(out)
    }

    pub fn entities_by_ids(&self, ids: &[String]) -> CoreResult<Vec<Entity>> {
        let json = serde_json::to_string(ids)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, path, version, source, attributes, first_seen, last_seen
             FROM entities WHERE id IN (SELECT value FROM json_each(?1))",
        )?;
        let rows = stmt.query_map([json], entity_from_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r??);
        }
        Ok(out)
    }

    /// Targets with the most incoming edges of `kind`, e.g. the most shared dependencies.
    pub fn most_referenced(
        &self,
        kind: RelationKind,
        limit: u32,
    ) -> CoreResult<Vec<(String, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT to_id, count(*) AS n FROM relationships WHERE kind = ?1
             GROUP BY to_id ORDER BY n DESC, to_id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![kind.as_str(), limit], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ----- scan roots -----

    pub fn list_scan_roots(&self) -> CoreResult<Vec<ScanRoot>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, enabled, max_depth, added_at FROM scan_roots ORDER BY path",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ScanRoot {
                id: r.get(0)?,
                path: r.get(1)?,
                enabled: r.get::<_, i64>(2)? != 0,
                max_depth: r.get(3)?,
                added_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Add a scan root. The caller is responsible for validating the path.
    pub fn add_scan_root(&self, path: &str, max_depth: u32) -> CoreResult<ScanRoot> {
        if path.is_empty() {
            return Err(CoreError::InvalidInput("empty path".into()));
        }
        self.conn.execute(
            "INSERT INTO scan_roots (path, max_depth, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET enabled = 1",
            params![path, max_depth, now_ms()],
        )?;
        self.conn
            .query_row(
                "SELECT id, path, enabled, max_depth, added_at FROM scan_roots WHERE path = ?1",
                [path],
                |r| {
                    Ok(ScanRoot {
                        id: r.get(0)?,
                        path: r.get(1)?,
                        enabled: r.get::<_, i64>(2)? != 0,
                        max_depth: r.get(3)?,
                        added_at: r.get(4)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn remove_scan_root(&self, id: i64) -> CoreResult<String> {
        let path: Option<String> = self
            .conn
            .query_row("SELECT path FROM scan_roots WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(path) = path else {
            return Err(CoreError::NotFound(format!("scan root {id}")));
        };
        self.conn
            .execute("DELETE FROM scan_roots WHERE id = ?1", [id])?;
        Ok(path)
    }

    pub fn set_scan_root_enabled(&self, id: i64, enabled: bool) -> CoreResult<()> {
        let n = self.conn.execute(
            "UPDATE scan_roots SET enabled = ?1 WHERE id = ?2",
            params![enabled as i64, id],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("scan root {id}")));
        }
        Ok(())
    }

    // ----- settings -----

    pub fn get_setting(&self, key: &str) -> CoreResult<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> CoreResult<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ----- audit log -----

    pub fn audit(
        &self,
        actor: &str,
        action: &str,
        target: &str,
        outcome: &str,
        detail: Option<&str>,
    ) -> CoreResult<i64> {
        self.conn.execute(
            "INSERT INTO audit_log (at, actor, action, target, outcome, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now_ms(), actor, action, target, outcome, detail],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_audit(&self, limit: u32) -> CoreResult<Vec<AuditEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, at, actor, action, target, outcome, detail
             FROM audit_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(AuditEntry {
                id: r.get(0)?,
                at: r.get(1)?,
                actor: r.get(2)?,
                action: r.get(3)?,
                target: r.get(4)?,
                outcome: r.get(5)?,
                detail: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

/// Relationship kinds each scan is authoritative for.
fn scan_relationship_kinds(kind: ScanKind) -> &'static [RelationKind] {
    match kind {
        ScanKind::Repositories => &[
            RelationKind::OriginRemote,
            RelationKind::TracksRemote,
            RelationKind::DuplicateClone,
            RelationKind::DependsOn,
            RelationKind::RequiresRuntime,
        ],
        ScanKind::Software => &[RelationKind::InstalledBy],
        ScanKind::Packages => &[
            RelationKind::ManagedBy,
            RelationKind::UpstreamProject,
            RelationKind::Provides,
            RelationKind::DependsOn,
        ],
        ScanKind::Security => &[RelationKind::HasFinding],
        ScanKind::Ai => &[
            RelationKind::Contains,
            RelationKind::ConfiguresTool,
            RelationKind::DeclaresMcpServer,
            RelationKind::UsesMcpServer,
            RelationKind::UsesProvider,
            RelationKind::ServesModel,
            RelationKind::SameContent,
            RelationKind::PotentialConflict,
            RelationKind::References,
        ],
    }
}

fn entity_exists(tx: &Transaction<'_>, id: &str) -> CoreResult<bool> {
    Ok(tx
        .query_row("SELECT 1 FROM entities WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn upsert_entity(tx: &Transaction<'_>, scan_id: i64, now: i64, e: &EntityInput) -> CoreResult<()> {
    let attrs = serde_json::to_string(&e.attributes)?;
    tx.execute(
        "INSERT INTO entities (id, kind, name, path, version, source, attributes,
                               first_seen, last_seen, last_scan_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
             kind = excluded.kind, name = excluded.name, path = excluded.path,
             version = excluded.version, source = excluded.source,
             attributes = excluded.attributes, last_seen = excluded.last_seen,
             last_scan_id = excluded.last_scan_id",
        params![
            e.id,
            e.kind.as_str(),
            e.name,
            e.path,
            e.version,
            e.source,
            attrs,
            now,
            scan_id
        ],
    )?;
    tx.execute("DELETE FROM evidence WHERE entity_id = ?1", [&e.id])?;
    for ev in &e.evidence {
        tx.execute(
            "INSERT INTO evidence (entity_id, kind, detail, source, confidence, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                e.id,
                ev.kind,
                ev.detail,
                ev.source,
                ev.confidence.as_str(),
                now
            ],
        )?;
    }
    Ok(())
}

fn upsert_relationship(
    tx: &Transaction<'_>,
    scan_id: i64,
    now: i64,
    r: &RelationshipInput,
) -> CoreResult<()> {
    let id = r.id();
    tx.execute(
        "INSERT INTO relationships
             (id, kind, from_id, to_id, confidence, last_seen, last_scan_id, attributes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET confidence = excluded.confidence,
             last_seen = excluded.last_seen, last_scan_id = excluded.last_scan_id,
             attributes = excluded.attributes",
        params![
            id,
            r.kind.as_str(),
            r.from_id,
            r.to_id,
            r.confidence.as_str(),
            now,
            scan_id,
            serde_json::to_string(&r.attributes)?
        ],
    )?;
    tx.execute("DELETE FROM evidence WHERE relationship_id = ?1", [&id])?;
    for ev in &r.evidence {
        tx.execute(
            "INSERT INTO evidence (relationship_id, kind, detail, source, confidence, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                ev.kind,
                ev.detail,
                ev.source,
                ev.confidence.as_str(),
                now
            ],
        )?;
    }
    Ok(())
}

fn entity_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CoreResult<Entity>> {
    let kind: String = r.get(1)?;
    let attrs: String = r.get(6)?;
    let Some(kind) = EntityKind::parse(&kind) else {
        return Ok(Err(CoreError::InvalidInput(format!(
            "unknown entity kind in database: {kind}"
        ))));
    };
    let attributes = match serde_json::from_str(&attrs) {
        Ok(v) => v,
        Err(e) => return Ok(Err(e.into())),
    };
    Ok(Ok(Entity {
        id: r.get(0)?,
        kind,
        name: r.get(2)?,
        path: r.get(3)?,
        version: r.get(4)?,
        source: r.get(5)?,
        attributes,
        first_seen: r.get(7)?,
        last_seen: r.get(8)?,
    }))
}

fn scan_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CoreResult<ScanRecord>> {
    let kind: String = r.get(1)?;
    let issues: String = r.get(7)?;
    let Some(kind) = ScanKind::parse(&kind) else {
        return Ok(Err(CoreError::InvalidInput(format!(
            "unknown scan kind in database: {kind}"
        ))));
    };
    let issues = match serde_json::from_str(&issues) {
        Ok(v) => v,
        Err(e) => return Ok(Err(e.into())),
    };
    Ok(Ok(ScanRecord {
        id: r.get(0)?,
        kind,
        status: ScanStatus::parse(&r.get::<_, String>(2)?),
        started_at: r.get(3)?,
        finished_at: r.get(4)?,
        entities_found: r.get(5)?,
        entities_removed: r.get(6)?,
        issues,
    }))
}

/// Turn free text into a safe FTS5 query: each token is quoted (so FTS syntax
/// in user input is inert) and prefix-matched.
pub fn fts_query(text: &str) -> Option<String> {
    let tokens: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'))
        .map(|t| t.trim_matches(|c: char| c == '.' || c == '-'))
        .filter(|t| !t.is_empty())
        .take(12)
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entity(id: &str, kind: EntityKind, name: &str, source: &str) -> EntityInput {
        EntityInput {
            id: id.into(),
            kind,
            name: name.into(),
            path: Some(format!("/tmp/{name}")),
            version: None,
            source: source.into(),
            attributes: json!({ "note": format!("about {name}") }),
            evidence: vec![EvidenceInput::new(
                "test",
                "observed in test",
                "unit-test",
                Confidence::Verified,
            )],
        }
    }

    #[test]
    fn migrates_from_empty() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn reopening_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        {
            let s = Store::open(&path).unwrap();
            s.set_setting("k", "v").unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(s.get_setting("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn upgrades_a_v1_database_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(MIGRATIONS[0]).unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
            c.execute(
                "INSERT INTO entities (id, kind, name, source, first_seen, last_seen)
                 VALUES ('repository:/a', 'repository', 'a', 'git', 1, 1),
                        ('remote:x/y/z', 'remote_project', 'y/z', 'git', 1, 1)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO relationships (id, kind, from_id, to_id, confidence, last_seen)
                 VALUES ('r1', 'origin_remote', 'repository:/a', 'remote:x/y/z', 'verified', 1)",
                [],
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        let d = s.entity_detail("repository:/a").unwrap().unwrap();
        assert_eq!(d.outgoing.len(), 1, "existing rows survive the migration");
        assert_eq!(d.outgoing[0].attributes, serde_json::Value::Null);
        assert_eq!(s.edges_touching("repository:/a").unwrap().len(), 1);
    }

    #[test]
    fn rejects_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", SCHEMA_VERSION + 5)
                .unwrap();
        }
        assert!(matches!(
            Store::open(&path),
            Err(CoreError::SchemaTooNew { .. })
        ));
    }

    #[test]
    fn ingest_search_and_prune() {
        let mut store = Store::open_in_memory().unwrap();
        let scan = store.begin_scan(ScanKind::Repositories).unwrap();
        let mut batch = IngestBatch::new(ScanKind::Repositories);
        batch.entities.push(entity(
            "repository:/tmp/alpha",
            EntityKind::Repository,
            "alpha",
            "git",
        ));
        batch.entities.push(entity(
            "repository:/tmp/beta",
            EntityKind::Repository,
            "beta-service",
            "git",
        ));
        batch.entities.push(entity(
            "remote:github.com/o/alpha",
            EntityKind::RemoteProject,
            "o/alpha",
            "git",
        ));
        batch.relationships.push(RelationshipInput {
            attributes: serde_json::Value::Null,
            from_id: "repository:/tmp/alpha".into(),
            to_id: "remote:github.com/o/alpha".into(),
            kind: RelationKind::OriginRemote,
            confidence: Confidence::Verified,
            evidence: vec![EvidenceInput::new(
                "git_config_remote",
                "origin",
                ".git/config",
                Confidence::Verified,
            )],
        });
        batch.prune.push(PruneScope {
            kind: EntityKind::Repository,
            source: None,
        });
        let res = store.ingest(scan, &batch).unwrap();
        assert_eq!(res.entities_written, 3);
        assert_eq!(res.entities_removed, 0);

        // FTS finds by prefix of a hyphenated name.
        let found = store
            .list_entities(&EntityQuery {
                kind: Some(EntityKind::Repository),
                text: Some("beta".into()),
                limit: None,
                offset: None,
            })
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "beta-service");

        // FTS syntax in user input is inert: no syntax error, and operators are
        // treated as ordinary (AND-ed) words rather than as query operators.
        let weird = store.list_entities(&EntityQuery {
            kind: None,
            text: Some("alpha\" OR * NEAR(".into()),
            limit: None,
            offset: None,
        });
        assert!(weird.unwrap().is_empty());
        let quoted = store
            .list_entities(&EntityQuery {
                kind: None,
                text: Some("\"alpha".into()),
                limit: None,
                offset: None,
            })
            .unwrap();
        assert!(!quoted.is_empty());

        let detail = store
            .entity_detail("repository:/tmp/alpha")
            .unwrap()
            .unwrap();
        assert_eq!(detail.evidence.len(), 1);
        assert_eq!(detail.outgoing.len(), 1);
        assert_eq!(detail.outgoing[0].evidence.len(), 1);
        assert_eq!(
            detail.outgoing[0].other.as_ref().unwrap().kind,
            EntityKind::RemoteProject
        );

        // Second scan sees only alpha: beta is pruned, first_seen preserved.
        let first_seen = store
            .get_entity("repository:/tmp/alpha")
            .unwrap()
            .unwrap()
            .first_seen;
        let scan2 = store.begin_scan(ScanKind::Repositories).unwrap();
        let mut batch2 = IngestBatch::new(ScanKind::Repositories);
        batch2.entities.push(entity(
            "repository:/tmp/alpha",
            EntityKind::Repository,
            "alpha",
            "git",
        ));
        batch2.prune.push(PruneScope {
            kind: EntityKind::Repository,
            source: None,
        });
        let res2 = store.ingest(scan2, &batch2).unwrap();
        assert_eq!(res2.entities_removed, 1);
        assert!(store.get_entity("repository:/tmp/beta").unwrap().is_none());
        let alpha = store.get_entity("repository:/tmp/alpha").unwrap().unwrap();
        assert_eq!(alpha.first_seen, first_seen);
        // The origin relationship was not re-observed, so it is gone.
        let d2 = store
            .entity_detail("repository:/tmp/alpha")
            .unwrap()
            .unwrap();
        assert!(d2.outgoing.is_empty());

        let scans = store.list_scans(10).unwrap();
        assert_eq!(scans.len(), 2);
        assert_eq!(scans[0].status, ScanStatus::Completed);
    }

    #[test]
    fn prune_respects_source() {
        let mut store = Store::open_in_memory().unwrap();
        let s1 = store.begin_scan(ScanKind::Packages).unwrap();
        let mut b = IngestBatch::new(ScanKind::Packages);
        b.entities.push(entity(
            "package:homebrew:jq",
            EntityKind::Package,
            "jq",
            "homebrew",
        ));
        b.entities.push(entity(
            "package:npm:typescript",
            EntityKind::Package,
            "typescript",
            "npm",
        ));
        store.ingest(s1, &b).unwrap();

        // npm failed this time; only homebrew is authoritative.
        let s2 = store.begin_scan(ScanKind::Packages).unwrap();
        let mut b2 = IngestBatch::new(ScanKind::Packages);
        b2.prune.push(PruneScope {
            kind: EntityKind::Package,
            source: Some("homebrew".into()),
        });
        b2.issues.push(ScanIssue::new(
            IssueKind::Timeout,
            "npm",
            "npm ls timed out",
        ));
        store.ingest(s2, &b2).unwrap();
        assert!(store.get_entity("package:homebrew:jq").unwrap().is_none());
        assert!(
            store
                .get_entity("package:npm:typescript")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store.latest_scans().unwrap()[0].status,
            ScanStatus::CompletedWithIssues
        );
    }

    #[test]
    fn audit_log_is_append_only() {
        let store = Store::open_in_memory().unwrap();
        store
            .audit("user", "scan_root.add", "/tmp/x", "ok", None)
            .unwrap();
        assert!(store.conn.execute("DELETE FROM audit_log", []).is_err());
        assert!(
            store
                .conn
                .execute("UPDATE audit_log SET outcome = 'x'", [])
                .is_err()
        );
        assert_eq!(store.list_audit(10).unwrap().len(), 1);
    }

    #[test]
    fn scan_roots_crud() {
        let store = Store::open_in_memory().unwrap();
        let r = store.add_scan_root("/tmp/code", 6).unwrap();
        assert!(r.enabled);
        store.set_scan_root_enabled(r.id, false).unwrap();
        assert!(!store.list_scan_roots().unwrap()[0].enabled);
        // Re-adding re-enables instead of duplicating.
        store.add_scan_root("/tmp/code", 6).unwrap();
        let roots = store.list_scan_roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert!(roots[0].enabled);
        assert_eq!(store.remove_scan_root(r.id).unwrap(), "/tmp/code");
        assert!(store.list_scan_roots().unwrap().is_empty());
        assert!(store.remove_scan_root(r.id).is_err());
    }

    #[test]
    fn interrupted_scans_are_marked_failed() {
        let store = Store::open_in_memory().unwrap();
        store.begin_scan(ScanKind::Software).unwrap();
        assert_eq!(store.mark_interrupted_scans().unwrap(), 1);
        assert_eq!(store.list_scans(1).unwrap()[0].status, ScanStatus::Failed);
    }

    #[test]
    fn fts_query_quotes_tokens() {
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("foo bar").unwrap(), "\"foo\"* \"bar\"*");
        assert_eq!(fts_query("a\"b").unwrap(), "\"a\"* \"b\"*");
    }
}
