use crate::models::{Summary, TestRun};
use anyhow::{Context, Result};
use measurement_core::{DiagnosticKind, MeasurementReport, TestSelection, UPSTREAM_REVISION};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub async fn open(data_dir: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(data_dir)
            .await
            .context("create data directory")?;
        let db = Self {
            path: data_dir.join("sentinel.db"),
        };
        db.call(|connection| {
            connection.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            let version: i64 = connection.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )?;
            if version < 2 {
                connection.execute_batch(include_str!("../migrations/002_quality_metrics.sql"))?;
            }
            if version < 3 {
                connection.execute_batch(include_str!(
                    "../migrations/003_nonnegative_bufferbloat.sql"
                ))?;
            }
            Ok(())
        })
        .await?;
        db.mark_interrupted().await?;
        Ok(db)
    }

    async fn call<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = Connection::open(path)?;
            connection.busy_timeout(Duration::from_secs(5))?;
            connection.pragma_update(None, "foreign_keys", "ON")?;
            connection.pragma_update(None, "journal_mode", "WAL")?;
            connection.pragma_update(None, "synchronous", "NORMAL")?;
            operation(&mut connection)
        })
        .await
        .context("database worker panicked")?
    }

    pub async fn check(&self) -> Result<()> {
        self.call(|connection| {
            connection.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        })
        .await
    }

    pub async fn insert_running(
        &self,
        id: String,
        kind: DiagnosticKind,
        trigger: String,
        started: String,
        selection: TestSelection,
    ) -> Result<()> {
        self.call(move |connection| {
            connection.execute("INSERT INTO test_runs (id,test_kind,trigger,status,started_at,upstream_engine_revision,selected_tests_json) VALUES (?1,?2,?3,'running',?4,?5,?6)",
                params![id, kind_text(kind), trigger, started, UPSTREAM_REVISION.trim(), serde_json::to_string(&selection)?])?;
            Ok(())
        }).await
    }

    pub async fn complete(
        &self,
        id: String,
        report: MeasurementReport,
        completed: String,
    ) -> Result<()> {
        self.call(move |connection| {
            connection.execute("UPDATE test_runs SET status='completed',completed_at=?2,measurement_target=?3,latency_ms=?4,jitter_ms=?5,packet_loss_pct=?6,download_mbps=?7,upload_mbps=?8,raw_result_json=?9,loaded_latency_download_ms=?10,loaded_latency_upload_ms=?11,download_bufferbloat_ms=?12,upload_bufferbloat_ms=?13,selected_tests_json=?14 WHERE id=?1",
                params![id, completed, report.measurement_target, report.latency_ms, report.jitter_ms, report.packet_loss_pct,
                    report.download_mbps, report.upload_mbps, serde_json::to_string(&report.raw_result)?, report.loaded_latency_download_ms,
                    report.loaded_latency_upload_ms, report.download_bufferbloat_ms, report.upload_bufferbloat_ms,
                    serde_json::to_string(&report.selected_tests)?])?;
            Ok(())
        }).await
    }

    pub async fn fail(&self, id: String, error: String, completed: String) -> Result<()> {
        self.call(move |connection| {
            connection.execute(
                "UPDATE test_runs SET status='failed',completed_at=?2,error=?3 WHERE id=?1",
                params![id, completed, error],
            )?;
            Ok(())
        })
        .await
    }

    async fn mark_interrupted(&self) -> Result<()> {
        self.call(|connection| {
            connection.execute("UPDATE test_runs SET status='failed',completed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),error='Application restarted before the diagnostic completed' WHERE status='running'", [])?;
            Ok(())
        }).await
    }

    pub async fn results(
        &self,
        since_hours: i64,
        limit: usize,
        include_raw: bool,
    ) -> Result<Vec<TestRun>> {
        self.call(move |connection| {
            let mut statement = connection.prepare("SELECT id,test_kind,trigger,status,started_at,completed_at,measurement_target,latency_ms,jitter_ms,packet_loss_pct,download_mbps,upload_mbps,error,raw_result_json,upstream_engine_revision,loaded_latency_download_ms,loaded_latency_upload_ms,download_bufferbloat_ms,upload_bufferbloat_ms,selected_tests_json FROM test_runs WHERE unixepoch(started_at) >= unixepoch('now') - (?1 * 3600) ORDER BY started_at DESC LIMIT ?2")?;
            let rows = statement.query_map(params![since_hours, limit as i64], |row| map_run(row, include_raw))?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
        }).await
    }

    pub async fn latest(&self) -> Result<Option<TestRun>> {
        self.call(|connection| {
            connection.query_row("SELECT id,test_kind,trigger,status,started_at,completed_at,measurement_target,latency_ms,jitter_ms,packet_loss_pct,download_mbps,upload_mbps,error,raw_result_json,upstream_engine_revision,loaded_latency_download_ms,loaded_latency_upload_ms,download_bufferbloat_ms,upload_bufferbloat_ms,selected_tests_json FROM test_runs WHERE status != 'running' ORDER BY started_at DESC LIMIT 1", [], |row| map_run(row, true)).optional().map_err(Into::into)
        }).await
    }

    pub async fn summary(&self) -> Result<Summary> {
        let runs = self.results(24 * 30, 10_000, false).await?;
        Ok(Summary {
            latest: runs.first().cloned(),
            latest_download_mbps: runs.iter().find_map(|r| {
                (r.status == "completed")
                    .then_some(r.download_mbps)
                    .flatten()
            }),
            latest_upload_mbps: runs
                .iter()
                .find_map(|r| (r.status == "completed").then_some(r.upload_mbps).flatten()),
            latest_latency_ms: runs
                .iter()
                .find_map(|r| (r.status == "completed").then_some(r.latency_ms).flatten()),
            latest_jitter_ms: runs
                .iter()
                .find_map(|r| (r.status == "completed").then_some(r.jitter_ms).flatten()),
            latest_download_bufferbloat_ms: runs.iter().find_map(|r| {
                (r.status == "completed")
                    .then_some(r.download_bufferbloat_ms)
                    .flatten()
            }),
            latest_upload_bufferbloat_ms: runs.iter().find_map(|r| {
                (r.status == "completed")
                    .then_some(r.upload_bufferbloat_ms)
                    .flatten()
            }),
            latest_bufferbloat_ms: runs.iter().find_map(|r| {
                if r.status != "completed" {
                    return None;
                }
                match (r.download_bufferbloat_ms, r.upload_bufferbloat_ms) {
                    (Some(download), Some(upload)) => Some(download.max(upload)),
                    (Some(download), None) => Some(download),
                    (None, Some(upload)) => Some(upload),
                    (None, None) => None,
                }
            }),
            latest_packet_loss_pct: runs.iter().find_map(|r| {
                (r.status == "completed")
                    .then_some(r.packet_loss_pct)
                    .flatten()
            }),
            failures_24h: runs
                .iter()
                .filter(|r| {
                    r.status == "failed"
                        && chrono::DateTime::parse_from_rfc3339(&r.started_at)
                            .map(|t| t > chrono::Utc::now() - chrono::Duration::hours(24))
                            .unwrap_or(false)
                })
                .count(),
        })
    }

    pub async fn is_due(&self, kind: DiagnosticKind, interval: Duration) -> Result<bool> {
        self.call(move |connection| {
            let last: Option<i64> = connection.query_row("SELECT MAX(unixepoch(started_at)) FROM test_runs WHERE test_kind=?1 AND status IN ('running','completed')", [kind_text(kind)], |r| r.get(0))?;
            Ok(last.map(|timestamp| chrono::Utc::now().timestamp() - timestamp >= interval.as_secs() as i64).unwrap_or(true))
        }).await
    }

    pub async fn enforce_retention(&self, days: i64) -> Result<usize> {
        self.call(move |connection| Ok(connection.execute("DELETE FROM test_runs WHERE status != 'running' AND unixepoch(started_at) < unixepoch('now') - (?1 * 86400)", [days])?)).await
    }
}

fn kind_text(kind: DiagnosticKind) -> &'static str {
    match kind {
        DiagnosticKind::Quality => "quality",
        DiagnosticKind::Full => "full",
    }
}
fn parse_kind(value: String) -> DiagnosticKind {
    if value == "quality" {
        DiagnosticKind::Quality
    } else {
        DiagnosticKind::Full
    }
}

fn map_run(row: &Row<'_>, include_raw: bool) -> rusqlite::Result<TestRun> {
    let raw: Option<String> = row.get(13)?;
    let test_kind = parse_kind(row.get(1)?);
    let selected_tests = row
        .get::<_, Option<String>>(19)?
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_else(|| match test_kind {
            DiagnosticKind::Quality => TestSelection::quality(),
            DiagnosticKind::Full => TestSelection::default(),
        });
    Ok(TestRun {
        id: row.get(0)?,
        test_kind,
        trigger: row.get(2)?,
        status: row.get(3)?,
        started_at: row.get(4)?,
        completed_at: row.get(5)?,
        measurement_target: row.get(6)?,
        latency_ms: row.get(7)?,
        jitter_ms: row.get(8)?,
        packet_loss_pct: row.get(9)?,
        download_mbps: row.get(10)?,
        upload_mbps: row.get(11)?,
        error: row.get(12)?,
        raw_result_json: if include_raw {
            raw.and_then(|value| serde_json::from_str(&value).ok())
        } else {
            None
        },
        upstream_engine_revision: row.get(14)?,
        loaded_latency_download_ms: row.get(15)?,
        loaded_latency_upload_ms: row.get(16)?,
        download_bufferbloat_ms: row.get(17)?,
        upload_bufferbloat_ms: row.get(18)?,
        selected_tests,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn migrations_and_measurement_round_trip_preserve_nulls_and_raw_data() {
        let directory = tempdir().unwrap();
        let db = Database::open(directory.path()).await.unwrap();
        let started = chrono::Utc::now().to_rfc3339();
        db.insert_running(
            "run-1".into(),
            DiagnosticKind::Quality,
            "scheduled".into(),
            started,
            TestSelection::quality(),
        )
        .await
        .unwrap();
        db.complete(
            "run-1".into(),
            MeasurementReport {
                kind: DiagnosticKind::Quality,
                measurement_target: "HTTP target; UDP target".into(),
                latency_ms: Some(12.5),
                jitter_ms: Some(1.2),
                packet_loss_pct: None,
                download_mbps: None,
                upload_mbps: None,
                loaded_latency_download_ms: None,
                loaded_latency_upload_ms: None,
                download_bufferbloat_ms: None,
                upload_bufferbloat_ms: None,
                selected_tests: TestSelection::quality(),
                raw_result: serde_json::json!({"probe":"ok"}),
            },
            chrono::Utc::now().to_rfc3339(),
        )
        .await
        .unwrap();
        let rows = db.results(24, 10, true).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].latency_ms, Some(12.5));
        assert_eq!(rows[0].packet_loss_pct, None);
        assert_eq!(rows[0].download_mbps, None);
        assert_eq!(
            rows[0].raw_result_json,
            Some(serde_json::json!({"probe":"ok"}))
        );
        assert!(db.check().await.is_ok());
    }

    #[tokio::test]
    async fn startup_marks_abandoned_runs_failed() {
        let directory = tempdir().unwrap();
        let db = Database::open(directory.path()).await.unwrap();
        db.insert_running(
            "run-2".into(),
            DiagnosticKind::Full,
            "manual".into(),
            chrono::Utc::now().to_rfc3339(),
            TestSelection::default(),
        )
        .await
        .unwrap();
        drop(db);
        let reopened = Database::open(directory.path()).await.unwrap();
        let latest = reopened.latest().await.unwrap().unwrap();
        assert_eq!(latest.status, "failed");
        assert!(latest.error.unwrap().contains("restarted"));
    }

    #[tokio::test]
    async fn migration_backfills_bufferbloat_from_legacy_raw_results() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("sentinel.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        let raw = serde_json::json!({
            "loaded_latency_download": {"median_ms": 48.0},
            "loaded_latency_upload": {"median_ms": 61.5}
        });
        connection.execute(
            "INSERT INTO test_runs (id,test_kind,trigger,status,started_at,completed_at,latency_ms,jitter_ms,raw_result_json,upstream_engine_revision) VALUES ('legacy','full','manual','completed',strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'),20.0,1.5,?1,'revision')",
            [serde_json::to_string(&raw).unwrap()],
        ).unwrap();
        drop(connection);

        let db = Database::open(directory.path()).await.unwrap();
        let run = db.latest().await.unwrap().unwrap();
        assert_eq!(run.loaded_latency_download_ms, Some(48.0));
        assert_eq!(run.loaded_latency_upload_ms, Some(61.5));
        assert_eq!(run.download_bufferbloat_ms, Some(28.0));
        assert_eq!(run.upload_bufferbloat_ms, Some(41.5));
        assert_eq!(run.selected_tests, TestSelection::default());
    }

    #[tokio::test]
    async fn migration_clamps_negative_bufferbloat_without_changing_latencies() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("sentinel.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!("../migrations/002_quality_metrics.sql"))
            .unwrap();
        connection.execute(
            "INSERT INTO test_runs (id,test_kind,trigger,status,started_at,completed_at,latency_ms,loaded_latency_download_ms,loaded_latency_upload_ms,download_bufferbloat_ms,upload_bufferbloat_ms,upstream_engine_revision) VALUES ('negative','full','manual','completed',strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'),20.0,17.0,18.5,-3.0,-1.5,'revision')",
            [],
        ).unwrap();
        drop(connection);

        let db = Database::open(directory.path()).await.unwrap();
        let run = db.latest().await.unwrap().unwrap();
        assert_eq!(run.latency_ms, Some(20.0));
        assert_eq!(run.loaded_latency_download_ms, Some(17.0));
        assert_eq!(run.loaded_latency_upload_ms, Some(18.5));
        assert_eq!(run.download_bufferbloat_ms, Some(0.0));
        assert_eq!(run.upload_bufferbloat_ms, Some(0.0));
    }
}
