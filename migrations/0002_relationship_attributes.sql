-- Schema v2: relationships carry kind-specific attributes (e.g. the version
-- constraint on a `depends_on` or `requires_runtime` edge).
ALTER TABLE relationships ADD COLUMN attributes TEXT NOT NULL DEFAULT 'null';
CREATE INDEX relationships_kind ON relationships(kind);
