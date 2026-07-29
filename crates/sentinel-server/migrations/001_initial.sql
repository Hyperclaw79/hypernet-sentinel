PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

INSERT OR IGNORE INTO schema_migrations(version, applied_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ','now'));

CREATE TABLE IF NOT EXISTS test_runs (
    id TEXT PRIMARY KEY,
    test_kind TEXT NOT NULL CHECK(test_kind IN ('quality','full')),
    trigger TEXT NOT NULL CHECK(trigger IN ('manual','scheduled','mcp')),
    status TEXT NOT NULL CHECK(status IN ('running','completed','failed')),
    started_at TEXT NOT NULL,
    completed_at TEXT,
    measurement_target TEXT,
    latency_ms REAL,
    jitter_ms REAL,
    packet_loss_pct REAL,
    download_mbps REAL,
    upload_mbps REAL,
    error TEXT,
    raw_result_json TEXT,
    upstream_engine_revision TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_test_runs_started_at ON test_runs(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_test_runs_kind_status ON test_runs(test_kind, status, started_at DESC);
