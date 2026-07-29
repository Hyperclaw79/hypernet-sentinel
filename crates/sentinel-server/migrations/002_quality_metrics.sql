ALTER TABLE test_runs ADD COLUMN loaded_latency_download_ms REAL;
ALTER TABLE test_runs ADD COLUMN loaded_latency_upload_ms REAL;
ALTER TABLE test_runs ADD COLUMN download_bufferbloat_ms REAL;
ALTER TABLE test_runs ADD COLUMN upload_bufferbloat_ms REAL;
ALTER TABLE test_runs ADD COLUMN selected_tests_json TEXT;

UPDATE test_runs
SET loaded_latency_download_ms = json_extract(raw_result_json, '$.loaded_latency_download.median_ms'),
    loaded_latency_upload_ms = json_extract(raw_result_json, '$.loaded_latency_upload.median_ms')
WHERE raw_result_json IS NOT NULL;

UPDATE test_runs
SET download_bufferbloat_ms = loaded_latency_download_ms - latency_ms,
    upload_bufferbloat_ms = loaded_latency_upload_ms - latency_ms
WHERE latency_ms IS NOT NULL;

UPDATE test_runs
SET selected_tests_json = CASE test_kind
    WHEN 'full' THEN '{"latency":true,"download":true,"upload":true,"packet_loss":true}'
    ELSE '{"latency":true,"download":false,"upload":false,"packet_loss":true}'
END
WHERE selected_tests_json IS NULL;

INSERT INTO schema_migrations(version, applied_at)
VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ','now'));
