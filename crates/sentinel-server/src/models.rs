use measurement_core::{DiagnosticKind, TestSelection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRun {
    pub id: String,
    pub test_kind: DiagnosticKind,
    pub trigger: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub measurement_target: Option<String>,
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
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_result_json: Option<serde_json::Value>,
    pub upstream_engine_revision: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveTest {
    pub id: String,
    pub test_kind: DiagnosticKind,
    pub trigger: String,
    pub started_at: String,
    pub phase: String,
    pub selected_tests: TestSelection,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub latest: Option<TestRun>,
    pub latest_download_mbps: Option<f64>,
    pub latest_upload_mbps: Option<f64>,
    pub latest_latency_ms: Option<f64>,
    pub latest_jitter_ms: Option<f64>,
    pub latest_download_bufferbloat_ms: Option<f64>,
    pub latest_upload_bufferbloat_ms: Option<f64>,
    pub latest_bufferbloat_ms: Option<f64>,
    pub latest_packet_loss_pct: Option<f64>,
    pub failures_24h: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CurrentStatus {
    pub running: bool,
    pub active_test: Option<ActiveTest>,
}
