use crate::engine::{cloudflare::CloudflareClient, EngineControl, TestEngine};
use crate::model::{Phase, RunConfig, RunResult, TestEvent, TurnInfo};
use anyhow::{anyhow, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    Quality,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOutcome {
    Measured,
    Unavailable,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletedTests {
    pub latency: bool,
    pub download: bool,
    pub upload: bool,
    pub packet_loss: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartialMeasurement {
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_pct: Option<f64>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub loaded_latency_download_ms: Option<f64>,
    pub loaded_latency_upload_ms: Option<f64>,
    pub download_bufferbloat_ms: Option<f64>,
    pub upload_bufferbloat_ms: Option<f64>,
    pub completed: CompletedTests,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestSelection {
    pub latency: bool,
    pub download: bool,
    pub upload: bool,
    pub packet_loss: bool,
}

impl Default for TestSelection {
    fn default() -> Self {
        Self {
            latency: true,
            download: true,
            upload: true,
            packet_loss: true,
        }
    }
}

impl TestSelection {
    pub fn quality() -> Self {
        Self {
            latency: true,
            download: false,
            upload: false,
            packet_loss: true,
        }
    }

    pub fn is_empty(self) -> bool {
        !self.latency && !self.download && !self.upload && !self.packet_loss
    }

    pub fn kind(self) -> DiagnosticKind {
        if self.download || self.upload {
            DiagnosticKind::Full
        } else {
            DiagnosticKind::Quality
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticConfig {
    pub base_url: String,
    pub idle_latency_duration: Duration,
    pub download_duration: Duration,
    pub upload_duration: Duration,
    pub probe_interval_ms: u64,
    pub probe_timeout_ms: u64,
    pub udp_packets: u64,
    pub concurrency: usize,
}

impl Default for DiagnosticConfig {
    fn default() -> Self {
        Self {
            base_url: "https://speed.cloudflare.com".into(),
            idle_latency_duration: Duration::from_secs(2),
            download_duration: Duration::from_secs(10),
            upload_duration: Duration::from_secs(10),
            probe_interval_ms: 250,
            probe_timeout_ms: 2_000,
            udp_packets: 50,
            concurrency: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressEvent {
    Phase {
        name: String,
    },
    PacketLoss {
        sent: u64,
        received: u64,
        total: u64,
    },
    Partial {
        measurement: PartialMeasurement,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementReport {
    pub kind: DiagnosticKind,
    pub outcome: DiagnosticOutcome,
    pub outcome_detail: Option<String>,
    pub measurement_target: String,
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_pct: Option<f64>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub loaded_latency_download_ms: Option<f64>,
    pub loaded_latency_upload_ms: Option<f64>,
    pub download_bufferbloat_ms: Option<f64>,
    pub upload_bufferbloat_ms: Option<f64>,
    pub selected_tests: TestSelection,
    pub raw_result: serde_json::Value,
}

#[derive(Clone)]
pub struct Runner {
    config: DiagnosticConfig,
}

impl Runner {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self { config }
    }

    pub async fn run(
        &self,
        kind: DiagnosticKind,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<MeasurementReport> {
        let selection = match kind {
            DiagnosticKind::Quality => TestSelection::quality(),
            DiagnosticKind::Full => TestSelection::default(),
        };
        self.run_selected(selection, cancel, progress).await
    }

    pub async fn run_selected(
        &self,
        selection: TestSelection,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<MeasurementReport> {
        if selection.is_empty() {
            return Err(anyhow!("at least one diagnostic must be selected"));
        }
        self.run_custom(selection, cancel, progress).await
    }

    async fn run_full(
        &self,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<MeasurementReport> {
        let (event_tx, mut event_rx) = mpsc::channel(2_048);
        let (control_tx, control_rx) = mpsc::channel(8);
        let engine = TestEngine::new(self.upstream_config());
        let mut handle = tokio::spawn(async move { engine.run(event_tx, control_rx).await });
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = control_tx.send(EngineControl::Cancel).await;
                    let _ = tokio::time::timeout(Duration::from_secs(5), &mut handle).await;
                    return Err(anyhow!("diagnostic cancelled"));
                }
                event = event_rx.recv() => {
                    let Some(event) = event else { break };
                    forward_progress(&progress, &event).await;
                }
            }
        }
        let result = handle.await.context("measurement task failed")??;
        let latency = valid_latency(&result.idle_latency);
        let udp = result.experimental_udp.as_ref();
        let download = (result.download.bytes > 0).then_some(result.download.mbps);
        let upload = (result.upload.bytes > 0).then_some(result.upload.mbps);
        let loaded_download = valid_latency(&result.loaded_latency_download);
        let loaded_upload = valid_latency(&result.loaded_latency_upload);
        if latency.is_none() && download.is_none() && upload.is_none() {
            return Err(self.invalid_measurement_error(&result).await);
        }
        Ok(MeasurementReport {
            kind: DiagnosticKind::Full,
            outcome: DiagnosticOutcome::Measured,
            outcome_detail: None,
            measurement_target: "Cloudflare speed test edge; UDP loss to turn.cloudflare.com:3478"
                .into(),
            latency_ms: latency,
            jitter_ms: result
                .idle_latency
                .jitter_ms
                .filter(|_| result.idle_latency.received >= 2),
            packet_loss_pct: udp.map(|u| u.latency.loss * 100.0),
            download_mbps: download,
            upload_mbps: upload,
            loaded_latency_download_ms: loaded_download,
            loaded_latency_upload_ms: loaded_upload,
            download_bufferbloat_ms: added_latency(loaded_download, latency),
            upload_bufferbloat_ms: added_latency(loaded_upload, latency),
            selected_tests: TestSelection::default(),
            raw_result: serde_json::to_value(&result)?,
        })
    }

    async fn run_quality(
        &self,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<MeasurementReport> {
        let cfg = self.upstream_config();
        let client = CloudflareClient::new(&cfg, None).await?;
        let paused = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_flag = cancelled.clone();
        let cancel_watch = tokio::spawn(async move {
            cancel.cancelled().await;
            cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let _ = progress
            .send(ProgressEvent::Phase {
                name: "quality".into(),
            })
            .await;
        let (event_tx, mut event_rx) = mpsc::channel(256);
        let progress_clone = progress.clone();
        let event_forwarder = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                forward_progress(&progress_clone, &event).await;
            }
        });
        let latency = crate::engine::latency::run_latency_probes(
            &client,
            Phase::IdleLatency,
            None,
            self.config.idle_latency_duration,
            self.config.probe_interval_ms,
            self.config.probe_timeout_ms,
            &event_tx,
            paused,
            cancelled.clone(),
        )
        .await?;
        let addresses = tokio::net::lookup_host(("turn.cloudflare.com", 3478_u16))
            .await
            .map(|items| items.collect())
            .unwrap_or_default();
        let turn = TurnInfo {
            urls: vec!["stun:turn.cloudflare.com:3478".into()],
            username: None,
            credential: None,
        };
        let udp = crate::engine::turn_udp::run_udp_like_loss_probe(
            &turn, &cfg, &event_tx, addresses, None, &cancelled,
        )
        .await;
        drop(event_tx);
        let _ = event_forwarder.await;
        cancel_watch.abort();
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(anyhow!("diagnostic cancelled"));
        }
        let valid = valid_latency(&latency);
        if valid.is_none() && udp.is_err() {
            let probe = connectivity_probe(&cfg, self.config.probe_timeout_ms).await;
            return Err(anyhow!(
                "quality measurement returned no valid samples: HTTP latency {}/{} probes; UDP failed: {:#}; verification probe {}",
                latency.received,
                latency.sent,
                udp.as_ref().expect_err("UDP failure checked above"),
                probe,
            ));
        }
        let raw_result = serde_json::json!({
            "idle_latency": latency,
            "experimental_udp": udp.as_ref().ok(),
            "udp_error": udp.as_ref().err().map(|error| format!("{error:#}")),
        });
        Ok(MeasurementReport {
            kind: DiagnosticKind::Quality,
            outcome: DiagnosticOutcome::Measured,
            outcome_detail: None,
            measurement_target:
                "HTTP latency to speed.cloudflare.com; UDP loss to turn.cloudflare.com:3478".into(),
            latency_ms: valid,
            jitter_ms: latency.jitter_ms.filter(|_| latency.received >= 2),
            packet_loss_pct: udp.ok().map(|value| value.latency.loss * 100.0),
            download_mbps: None,
            upload_mbps: None,
            loaded_latency_download_ms: None,
            loaded_latency_upload_ms: None,
            download_bufferbloat_ms: None,
            upload_bufferbloat_ms: None,
            selected_tests: TestSelection::quality(),
            raw_result,
        })
    }

    async fn run_custom(
        &self,
        selection: TestSelection,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<MeasurementReport> {
        let cfg = self.upstream_config();
        let client = CloudflareClient::new(&cfg, None).await?;
        let paused = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_flag = cancelled.clone();
        let cancel_watch = tokio::spawn(async move {
            cancel.cancelled().await;
            cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let (event_tx, mut event_rx) = mpsc::channel(2_048);
        let progress_clone = progress.clone();
        let event_forwarder = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                forward_progress(&progress_clone, &event).await;
            }
        });

        let mut idle = crate::model::LatencySummary::default();
        let mut download = None;
        let mut upload = None;
        let mut loaded_download = crate::model::LatencySummary::default();
        let mut loaded_upload = crate::model::LatencySummary::default();
        let mut udp = None;
        let mut phase_errors = serde_json::Map::new();
        let mut partial = PartialMeasurement::default();

        if selection.latency {
            let _ = progress
                .send(ProgressEvent::Phase {
                    name: "idle_latency".into(),
                })
                .await;
            match crate::engine::latency::run_latency_probes(
                &client,
                Phase::IdleLatency,
                None,
                self.config.idle_latency_duration,
                self.config.probe_interval_ms,
                self.config.probe_timeout_ms,
                &event_tx,
                paused.clone(),
                cancelled.clone(),
            )
            .await
            {
                Ok(summary) => idle = summary,
                Err(error) => {
                    phase_errors.insert("latency".into(), format!("{error:#}").into());
                }
            }
            partial.latency_ms = valid_latency(&idle);
            partial.jitter_ms = idle.jitter_ms.filter(|_| idle.received >= 2);
            partial.completed.latency = true;
            emit_partial(&progress, &partial).await;
        }

        if selection.download {
            let _ = progress
                .send(ProgressEvent::Phase {
                    name: "download".into(),
                })
                .await;
            match crate::engine::throughput::run_download_with_loaded_latency(
                &client,
                &cfg,
                &event_tx,
                paused.clone(),
                cancelled.clone(),
            )
            .await
            {
                Ok((summary, loaded)) => {
                    download = (summary.bytes > 0).then_some(summary);
                    loaded_download = loaded;
                }
                Err(error) => {
                    phase_errors.insert("download".into(), format!("{error:#}").into());
                }
            }
            partial.download_mbps = download.as_ref().map(|value| value.mbps);
            partial.loaded_latency_download_ms = valid_latency(&loaded_download);
            partial.download_bufferbloat_ms =
                added_latency(partial.loaded_latency_download_ms, partial.latency_ms);
            partial.completed.download = true;
            emit_partial(&progress, &partial).await;
        }

        let turn_dns = if selection.packet_loss {
            Some(tokio::spawn(async {
                tokio::net::lookup_host(("turn.cloudflare.com", 3478_u16))
                    .await
                    .map(|items| items.collect::<Vec<_>>())
                    .unwrap_or_default()
            }))
        } else {
            None
        };

        if selection.upload {
            let _ = progress
                .send(ProgressEvent::Phase {
                    name: "upload".into(),
                })
                .await;
            match crate::engine::throughput::run_upload_with_loaded_latency(
                &client,
                &cfg,
                &event_tx,
                paused.clone(),
                cancelled.clone(),
            )
            .await
            {
                Ok((summary, loaded)) => {
                    upload = (summary.bytes > 0).then_some(summary);
                    loaded_upload = loaded;
                }
                Err(error) => {
                    phase_errors.insert("upload".into(), format!("{error:#}").into());
                }
            }
            partial.upload_mbps = upload.as_ref().map(|value| value.mbps);
            partial.loaded_latency_upload_ms = valid_latency(&loaded_upload);
            partial.upload_bufferbloat_ms =
                added_latency(partial.loaded_latency_upload_ms, partial.latency_ms);
            partial.completed.upload = true;
            emit_partial(&progress, &partial).await;
        }

        if selection.packet_loss {
            let _ = progress
                .send(ProgressEvent::Phase {
                    name: "packet_loss".into(),
                })
                .await;
            let addresses = match turn_dns {
                Some(handle) => handle.await.unwrap_or_default(),
                None => Vec::new(),
            };
            let turn = TurnInfo {
                urls: vec!["stun:turn.cloudflare.com:3478".into()],
                username: None,
                credential: None,
            };
            match crate::engine::turn_udp::run_udp_like_loss_probe(
                &turn, &cfg, &event_tx, addresses, None, &cancelled,
            )
            .await
            {
                Ok(value) => udp = Some(value),
                Err(error) => {
                    phase_errors.insert("packet_loss".into(), format!("{error:#}").into());
                }
            }
            partial.packet_loss_pct = udp.as_ref().map(|value| value.latency.loss * 100.0);
            partial.completed.packet_loss = true;
            emit_partial(&progress, &partial).await;
        }

        drop(event_tx);
        let _ = event_forwarder.await;
        cancel_watch.abort();
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(anyhow!("diagnostic cancelled"));
        }

        let measured = partial.latency_ms.is_some()
            || partial.download_mbps.is_some()
            || partial.upload_mbps.is_some()
            || partial.packet_loss_pct.is_some();
        let (outcome, outcome_detail) = if measured {
            (DiagnosticOutcome::Measured, None)
        } else {
            let probe = connectivity_probe(&cfg, self.config.probe_timeout_ms).await;
            phase_errors.insert("verification".into(), probe.clone().into());
            (
                DiagnosticOutcome::Unavailable,
                Some(format!(
                    "No valid samples were received from the configured measurement targets; {probe}"
                )),
            )
        };

        let raw_result = serde_json::json!({
            "selection": selection,
            "idle_latency": idle,
            "download": download,
            "upload": upload,
            "loaded_latency_download": loaded_download,
            "loaded_latency_upload": loaded_upload,
            "experimental_udp": udp,
            "phase_errors": phase_errors,
        });
        Ok(MeasurementReport {
            kind: selection.kind(),
            outcome,
            outcome_detail,
            measurement_target: "Selected diagnostics against Cloudflare speed test and TURN edges"
                .into(),
            latency_ms: partial.latency_ms,
            jitter_ms: partial.jitter_ms,
            packet_loss_pct: partial.packet_loss_pct,
            download_mbps: partial.download_mbps,
            upload_mbps: partial.upload_mbps,
            loaded_latency_download_ms: partial.loaded_latency_download_ms,
            loaded_latency_upload_ms: partial.loaded_latency_upload_ms,
            download_bufferbloat_ms: partial.download_bufferbloat_ms,
            upload_bufferbloat_ms: partial.upload_bufferbloat_ms,
            selected_tests: selection,
            raw_result,
        })
    }

    fn upstream_config(&self) -> RunConfig {
        let mut id = [0_u8; 8];
        rand::thread_rng().fill_bytes(&mut id);
        RunConfig {
            base_url: self.config.base_url.clone(),
            meas_id: u64::from_le_bytes(id).to_string(),
            comments: None,
            download_bytes_per_req: 10_000_000,
            upload_bytes_per_req: 5_000_000,
            concurrency: self.config.concurrency,
            idle_latency_duration: self.config.idle_latency_duration,
            download_duration: self.config.download_duration,
            upload_duration: self.config.upload_duration,
            probe_interval_ms: self.config.probe_interval_ms,
            probe_timeout_ms: self.config.probe_timeout_ms,
            user_agent: format!("hypernet-sentinel/{}", env!("CARGO_PKG_VERSION")),
            experimental: false,
            interface: None,
            source_ip: None,
            resolved_bind_ip: None,
            proxy: None,
            certificate_path: None,
            measure_dns: false,
            measure_tls: false,
            compare_ip_versions: false,
            traceroute: false,
            traceroute_max_hops: 30,
            ipv4_only: false,
            ipv6_only: false,
            udp_packets: self.config.udp_packets,
        }
    }

    async fn invalid_measurement_error(&self, result: &RunResult) -> anyhow::Error {
        let udp = match (&result.experimental_udp, &result.udp_error) {
            (Some(summary), _) => format!(
                "{}/{} packets received",
                summary.latency.received, summary.latency.sent
            ),
            (None, Some(error)) => format!("failed: {error}"),
            (None, None) => "unavailable".to_string(),
        };
        let cfg = self.upstream_config();
        let probe = connectivity_probe(&cfg, self.config.probe_timeout_ms).await;
        anyhow!(
            "measurement returned no valid samples: HTTP latency {}/{} probes; download {} bytes; upload {} bytes; UDP {}; verification probe {}",
            result.idle_latency.received,
            result.idle_latency.sent,
            result.download.bytes,
            result.upload.bytes,
            udp,
            probe,
        )
    }
}

async fn connectivity_probe(config: &RunConfig, timeout_ms: u64) -> String {
    let client = match CloudflareClient::new(config, None).await {
        Ok(client) => client,
        Err(error) => return format!("could not initialize HTTP client: {error:#}"),
    };
    match client
        .probe_latency_ms(None, timeout_ms.clamp(250, 3_000))
        .await
    {
        Ok((latency_ms, _)) => format!("succeeded in {latency_ms:.1} ms after the run"),
        Err(error) => format!("to {} failed: {error:#}", config.base_url),
    }
}

async fn emit_partial(sender: &mpsc::Sender<ProgressEvent>, partial: &PartialMeasurement) {
    let _ = sender
        .send(ProgressEvent::Partial {
            measurement: partial.clone(),
        })
        .await;
}

fn valid_latency(summary: &crate::model::LatencySummary) -> Option<f64> {
    (summary.received > 0)
        .then_some(summary.median_ms)
        .flatten()
}

fn added_latency(loaded: Option<f64>, idle: Option<f64>) -> Option<f64> {
    Some((loaded? - idle?).max(0.0))
}

async fn forward_progress(sender: &mpsc::Sender<ProgressEvent>, event: &TestEvent) {
    let progress = match event {
        TestEvent::PhaseStarted { phase } => Some(ProgressEvent::Phase {
            name: format!("{phase:?}").to_lowercase(),
        }),
        TestEvent::UdpLossProgress {
            sent,
            received,
            total,
            ..
        } => Some(ProgressEvent::PacketLoss {
            sent: *sent,
            received: *received,
            total: *total,
        }),
        _ => None,
    };
    if let Some(progress) = progress {
        let _ = sender.send(progress).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_match_policy() {
        let config = DiagnosticConfig::default();
        assert_eq!(config.download_duration, Duration::from_secs(10));
        assert_eq!(config.upload_duration, Duration::from_secs(10));
        assert_eq!(config.udp_packets, 50);
    }
    #[test]
    fn zero_sample_latency_is_missing() {
        assert_eq!(
            valid_latency(&crate::model::LatencySummary::default()),
            None
        );
    }
    #[test]
    fn added_latency_clamps_measurement_noise_to_zero() {
        assert_eq!(added_latency(Some(18.0), Some(20.0)), Some(0.0));
        assert_eq!(added_latency(Some(24.5), Some(20.0)), Some(4.5));
        assert_eq!(added_latency(None, Some(20.0)), None);
        assert_eq!(added_latency(Some(20.0), None), None);
    }

    #[tokio::test]
    async fn invalid_full_measurement_reports_phase_evidence() {
        let runner = Runner::new(DiagnosticConfig {
            base_url: "not a url".into(),
            ..DiagnosticConfig::default()
        });
        let result = crate::model::empty_run_result();
        let message = format!("{:#}", runner.invalid_measurement_error(&result).await);
        assert!(message.contains("HTTP latency 0/0 probes"));
        assert!(message.contains("download 0 bytes"));
        assert!(message.contains("upload 0 bytes"));
        assert!(message.contains("UDP unavailable"));
        assert!(message.contains("could not initialize HTTP client: invalid base_url"));
    }
    #[test]
    fn selections_map_to_storage_kinds() {
        assert_eq!(TestSelection::default().kind(), DiagnosticKind::Full);
        assert_eq!(TestSelection::quality().kind(), DiagnosticKind::Quality);
        assert!(TestSelection {
            latency: false,
            download: false,
            upload: false,
            packet_loss: false,
        }
        .is_empty());
    }
}
