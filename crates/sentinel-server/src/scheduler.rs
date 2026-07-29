use crate::{
    coordinator::{Coordinator, StartError},
    db::Database,
};
use measurement_core::DiagnosticKind;
use std::{
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub struct SchedulerHealth {
    heartbeat: AtomicI64,
}
impl SchedulerHealth {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            heartbeat: AtomicI64::new(chrono::Utc::now().timestamp()),
        })
    }
    pub fn healthy(&self) -> bool {
        chrono::Utc::now().timestamp() - self.heartbeat.load(Ordering::Relaxed) < 120
    }
}

pub fn spawn(
    db: Database,
    coordinator: Arc<Coordinator>,
    quality_interval: Duration,
    full_interval: Duration,
    retention_days: i64,
    shutdown: CancellationToken,
    health: Arc<SchedulerHealth>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = ticker.tick() => {
                    health.heartbeat.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
                    if let Err(error) = db.enforce_retention(retention_days).await { tracing::warn!(%error, "retention cleanup failed"); }
                    let full_due = db.is_due(DiagnosticKind::Full, full_interval).await.unwrap_or(false);
                    let quality_due = db.is_due(DiagnosticKind::Quality, quality_interval).await.unwrap_or(false);
                    let kind = due_kind(full_due, quality_due);
                    if let Some(kind) = kind {
                        match coordinator.start(kind, "scheduled").await {
                            Ok(active) => tracing::info!(id=%active.id, ?kind, "scheduled diagnostic started"),
                            Err(StartError::Conflict) => {},
                            Err(error) => tracing::error!(%error, "scheduled diagnostic could not start"),
                        }
                    }
                }
            }
        }
    });
}

fn due_kind(full_due: bool, quality_due: bool) -> Option<DiagnosticKind> {
    if full_due {
        Some(DiagnosticKind::Full)
    } else if quality_due {
        Some(DiagnosticKind::Quality)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_run_takes_priority_when_both_are_due() {
        assert_eq!(due_kind(true, true), Some(DiagnosticKind::Full));
        assert_eq!(due_kind(false, true), Some(DiagnosticKind::Quality));
        assert_eq!(due_kind(false, false), None);
    }
}
