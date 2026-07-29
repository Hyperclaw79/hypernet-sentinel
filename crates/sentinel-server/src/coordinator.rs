use crate::{
    db::Database,
    models::{ActiveTest, CurrentStatus},
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use measurement_core::{DiagnosticKind, MeasurementReport, ProgressEvent, Runner, TestSelection};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("a diagnostic is already running")]
    Conflict,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[async_trait]
pub trait MeasurementEngine: Send + Sync {
    async fn run(
        &self,
        selection: TestSelection,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<MeasurementReport>;
}

#[async_trait]
impl MeasurementEngine for Runner {
    async fn run(
        &self,
        selection: TestSelection,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<MeasurementReport> {
        Runner::run_selected(self, selection, cancel, progress).await
    }
}

pub struct Coordinator {
    db: Database,
    engine: Arc<dyn MeasurementEngine>,
    busy: AtomicBool,
    active: RwLock<Option<ActiveTest>>,
    timeout: Duration,
    shutdown: CancellationToken,
}

impl Coordinator {
    pub fn new(
        db: Database,
        engine: Arc<dyn MeasurementEngine>,
        timeout: Duration,
        shutdown: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            engine,
            busy: AtomicBool::new(false),
            active: RwLock::new(None),
            timeout,
            shutdown,
        })
    }

    pub async fn start(
        self: &Arc<Self>,
        kind: DiagnosticKind,
        trigger: &str,
    ) -> std::result::Result<ActiveTest, StartError> {
        let selection = match kind {
            DiagnosticKind::Quality => TestSelection::quality(),
            DiagnosticKind::Full => TestSelection::default(),
        };
        self.start_selected(selection, trigger).await
    }

    pub async fn start_selected(
        self: &Arc<Self>,
        selection: TestSelection,
        trigger: &str,
    ) -> std::result::Result<ActiveTest, StartError> {
        if selection.is_empty() {
            return Err(StartError::Internal(anyhow!(
                "at least one diagnostic must be selected"
            )));
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(StartError::Conflict);
        }
        let active = ActiveTest {
            id: Uuid::new_v4().to_string(),
            test_kind: selection.kind(),
            trigger: trigger.to_string(),
            started_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            phase: "starting".into(),
            selected_tests: selection,
        };
        if let Err(error) = self
            .db
            .insert_running(
                active.id.clone(),
                selection.kind(),
                active.trigger.clone(),
                active.started_at.clone(),
                selection,
            )
            .await
        {
            self.busy.store(false, Ordering::Release);
            return Err(StartError::Internal(error));
        }
        *self.active.write().await = Some(active.clone());
        let coordinator = Arc::clone(self);
        let id = active.id.clone();
        tokio::spawn(async move {
            coordinator.execute(id, selection).await;
        });
        Ok(active)
    }

    async fn execute(self: Arc<Self>, id: String, selection: TestSelection) {
        let cancel = self.shutdown.child_token();
        let (progress_tx, mut progress_rx) = mpsc::channel(128);
        let future = self.engine.run(selection, cancel.clone(), progress_tx);
        tokio::pin!(future);
        let deadline = tokio::time::sleep(self.timeout);
        tokio::pin!(deadline);
        let outcome = loop {
            tokio::select! {
                biased;
                _ = self.shutdown.cancelled() => { cancel.cancel(); break Err(anyhow!("application is shutting down")); }
                result = &mut future => break result,
                _ = &mut deadline => { cancel.cancel(); break Err(anyhow!("diagnostic exceeded {} second timeout", self.timeout.as_secs())); }
                progress = progress_rx.recv() => {
                    if let Some(progress) = progress { self.update_progress(progress).await; }
                }
            }
        };
        let completed = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let result = match outcome {
            Ok(report) => self.db.complete(id.clone(), report, completed).await,
            Err(error) => {
                tracing::error!(%id, error=%format!("{error:#}"), "diagnostic failed");
                self.db
                    .fail(id.clone(), format!("{error:#}"), completed)
                    .await
            }
        };
        if let Err(error) = result {
            tracing::error!(%id, %error, "failed to persist diagnostic terminal state");
        }
        *self.active.write().await = None;
        self.busy.store(false, Ordering::Release);
    }

    async fn update_progress(&self, event: ProgressEvent) {
        let phase = match event {
            ProgressEvent::Phase { name } => name,
            ProgressEvent::PacketLoss { sent, total, .. } => format!("packet_loss {sent}/{total}"),
        };
        if let Some(active) = self.active.write().await.as_mut() {
            active.phase = phase;
        }
    }

    pub async fn status(&self) -> CurrentStatus {
        CurrentStatus {
            running: self.busy.load(Ordering::Acquire),
            active_test: self.active.read().await.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use measurement_core::DiagnosticKind;
    use tempfile::tempdir;

    struct BlockingEngine;
    #[async_trait]
    impl MeasurementEngine for BlockingEngine {
        async fn run(
            &self,
            selection: TestSelection,
            cancel: CancellationToken,
            _progress: mpsc::Sender<ProgressEvent>,
        ) -> Result<MeasurementReport> {
            cancel.cancelled().await;
            Err(anyhow!("cancelled {:?}", selection.kind()))
        }
    }

    struct SlowEngine;
    #[async_trait]
    impl MeasurementEngine for SlowEngine {
        async fn run(
            &self,
            _selection: TestSelection,
            _cancel: CancellationToken,
            _progress: mpsc::Sender<ProgressEvent>,
        ) -> Result<MeasurementReport> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            unreachable!()
        }
    }

    #[tokio::test]
    async fn concurrent_requests_conflict() {
        let directory = tempdir().unwrap();
        let db = Database::open(directory.path()).await.unwrap();
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            db,
            Arc::new(BlockingEngine),
            Duration::from_secs(5),
            shutdown.clone(),
        );
        coordinator
            .start(DiagnosticKind::Full, "manual")
            .await
            .unwrap();
        assert!(matches!(
            coordinator.start(DiagnosticKind::Full, "manual").await,
            Err(StartError::Conflict)
        ));
        shutdown.cancel();
    }

    #[tokio::test]
    async fn timeout_is_persisted_and_releases_lock() {
        let directory = tempdir().unwrap();
        let db = Database::open(directory.path()).await.unwrap();
        let coordinator = Coordinator::new(
            db.clone(),
            Arc::new(SlowEngine),
            Duration::from_millis(20),
            CancellationToken::new(),
        );
        coordinator
            .start(DiagnosticKind::Quality, "scheduled")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!coordinator.status().await.running);
        let latest = db.latest().await.unwrap().unwrap();
        assert_eq!(latest.status, "failed");
        assert!(latest.error.unwrap().contains("timeout"));
        assert!(coordinator
            .start(DiagnosticKind::Full, "manual")
            .await
            .is_ok());
    }
}
