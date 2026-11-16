-- AGASTHY schema v1.
-- Generic entity / relationship / evidence tables so later phases (graph,
-- impact analysis) build on the same data rather than re-modelling it.

CREATE TABLE entities (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    path         TEXT,
    version      TEXT,
    source       TEXT NOT NULL,
    attributes   TEXT NOT NULL DEFAULT '{}',
    first_seen   INTEGER NOT NULL,
    last_seen    INTEGER NOT NULL,
    last_scan_id INTEGER
);
CREATE INDEX entities_kind ON entities(kind);
CREATE INDEX entities_kind_source ON entities(kind, source);
CREATE INDEX entities_first_seen ON entities(first_seen);

CREATE TABLE relationships (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    from_id      TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    to_id        TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    confidence   TEXT NOT NULL,
    last_seen    INTEGER NOT NULL,
    last_scan_id INTEGER
);
CREATE INDEX relationships_from ON relationships(from_id);
CREATE INDEX relationships_to ON relationships(to_id);

CREATE TABLE evidence (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_id       TEXT REFERENCES entities(id) ON DELETE CASCADE,
    relationship_id TEXT REFERENCES relationships(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    detail          TEXT NOT NULL,
    source          TEXT NOT NULL,
    confidence      TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    CHECK ((entity_id IS NULL) <> (relationship_id IS NULL))
);
CREATE INDEX evidence_entity ON evidence(entity_id);
CREATE INDEX evidence_relationship ON evidence(relationship_id);

CREATE TABLE scan_roots (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    path      TEXT NOT NULL UNIQUE,
    enabled   INTEGER NOT NULL DEFAULT 1,
    max_depth INTEGER NOT NULL DEFAULT 6,
    added_at  INTEGER NOT NULL
);

CREATE TABLE scans (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    kind             TEXT NOT NULL,
    status           TEXT NOT NULL,
    started_at       INTEGER NOT NULL,
    finished_at      INTEGER,
    entities_found   INTEGER NOT NULL DEFAULT 0,
    entities_removed INTEGER NOT NULL DEFAULT 0,
    issues           TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX scans_kind_started ON scans(kind, started_at);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Append-only record of every action AGASTHY takes on the user's behalf.
CREATE TABLE audit_log (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    at      INTEGER NOT NULL,
    actor   TEXT NOT NULL,
    action  TEXT NOT NULL,
    target  TEXT NOT NULL,
    outcome TEXT NOT NULL,
    detail  TEXT
);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;

-- Full-text index over entities, kept in sync by triggers.
CREATE VIRTUAL TABLE entities_fts USING fts5(
    id UNINDEXED,
    kind UNINDEXED,
    name,
    path,
    attributes,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER entities_fts_insert AFTER INSERT ON entities BEGIN
    INSERT INTO entities_fts(id, kind, name, path, attributes)
    VALUES (new.id, new.kind, new.name, coalesce(new.path, ''), new.attributes);
END;
CREATE TRIGGER entities_fts_delete AFTER DELETE ON entities BEGIN
    DELETE FROM entities_fts WHERE id = old.id;
END;
CREATE TRIGGER entities_fts_update AFTER UPDATE OF name, path, attributes ON entities BEGIN
    DELETE FROM entities_fts WHERE id = old.id;
    INSERT INTO entities_fts(id, kind, name, path, attributes)
    VALUES (new.id, new.kind, new.name, coalesce(new.path, ''), new.attributes);
END;
