UPDATE test_runs
SET download_bufferbloat_ms = MAX(download_bufferbloat_ms, 0)
WHERE download_bufferbloat_ms < 0;

UPDATE test_runs
SET upload_bufferbloat_ms = MAX(upload_bufferbloat_ms, 0)
WHERE upload_bufferbloat_ms < 0;

INSERT INTO schema_migrations(version, applied_at)
VALUES (3, strftime('%Y-%m-%dT%H:%M:%fZ','now'));
