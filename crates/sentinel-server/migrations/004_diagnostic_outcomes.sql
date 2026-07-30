ALTER TABLE test_runs ADD COLUMN outcome TEXT CHECK(outcome IN ('measured','unavailable'));
ALTER TABLE test_runs ADD COLUMN outcome_detail TEXT;

UPDATE test_runs
SET outcome = 'measured'
WHERE status = 'completed' AND outcome IS NULL;

UPDATE test_runs
SET status = 'completed',
    outcome = 'unavailable',
    outcome_detail = error,
    error = NULL
WHERE status = 'failed'
  AND lower(error) LIKE '%measurement returned no valid samples%';

INSERT INTO schema_migrations(version, applied_at)
VALUES (4, strftime('%Y-%m-%dT%H:%M:%fZ','now'));
