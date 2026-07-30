use crate::{coordinator::StartError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use measurement_core::TestSelection;
use serde::Deserialize;
#[cfg(debug_assertions)]
use std::path::Path as FilePath;

#[derive(Deserialize)]
pub struct ResultsQuery {
    range: Option<String>,
    limit: Option<usize>,
}

pub async fn health(State(state): State<AppState>) -> Response {
    let database = state.db.check().await;
    let scheduler = !state.scheduler_enabled || state.scheduler_health.healthy();
    let healthy = database.is_ok() && scheduler;
    let status = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(serde_json::json!({
            "status": if healthy { "ready" } else { "not_ready" },
            "database": database.is_ok(), "scheduler": scheduler,
            "upstream_engine_revision": measurement_core::UPSTREAM_REVISION.trim(),
        })),
    )
        .into_response()
}

pub async fn summary(State(state): State<AppState>) -> Response {
    match state.db.summary().await {
        Ok(value) => Json(value).into_response(),
        Err(error) => internal(error),
    }
}

pub async fn results(State(state): State<AppState>, Query(query): Query<ResultsQuery>) -> Response {
    let hours = match query.range.as_deref().unwrap_or("24h") {
        "24h" => 24,
        "7d" => 24 * 7,
        "30d" => 24 * 30,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":"range must be 24h, 7d, or 30d"})),
            )
                .into_response()
        }
    };
    match state.db.results(hours, query.limit.unwrap_or(250).min(1_000), false).await {
        Ok(value) => Json(serde_json::json!({"range":query.range.unwrap_or_else(|| "24h".into()),"results":value})).into_response(),
        Err(error) => internal(error),
    }
}

pub async fn current(State(state): State<AppState>) -> Json<crate::models::CurrentStatus> {
    Json(state.coordinator.status().await)
}

pub async fn run(
    State(state): State<AppState>,
    selection: Option<Json<TestSelection>>,
) -> Response {
    let selection = selection.map(|Json(value)| value).unwrap_or_default();
    if selection.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"select at least one diagnostic"})),
        )
            .into_response();
    }
    match state.coordinator.start_selected(selection, "manual").await {
        Ok(active) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"accepted":true,"test":active})),
        )
            .into_response(),
        Err(StartError::Conflict) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error":"a diagnostic is already running"})),
        )
            .into_response(),
        Err(StartError::Internal(error)) => internal(error),
    }
}

pub async fn index() -> Response {
    web_asset(
        "index.html",
        include_str!("../../../web/index.html"),
        "text/html; charset=utf-8",
    )
    .await
}
pub async fn manifest() -> Response {
    web_asset(
        "manifest.webmanifest",
        include_str!("../../../web/manifest.webmanifest"),
        "application/manifest+json; charset=utf-8",
    )
    .await
}
pub async fn icon(Path(name): Path<String>) -> Response {
    let bytes: &'static [u8] = match name.as_str() {
        "icon-192.png" => include_bytes!("../../../web/icons/icon-192.png"),
        "icon-512.png" => include_bytes!("../../../web/icons/icon-512.png"),
        "icon-maskable-192.png" => include_bytes!("../../../web/icons/icon-maskable-192.png"),
        "icon-maskable-512.png" => include_bytes!("../../../web/icons/icon-maskable-512.png"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    ([(header::CONTENT_TYPE, "image/png")], bytes).into_response()
}
pub async fn styles() -> Response {
    web_asset(
        "styles/app.css",
        include_str!("../../../web/styles/app.css"),
        "text/css; charset=utf-8",
    )
    .await
}
pub async fn script() -> Response {
    web_asset(
        "scripts/app.js",
        include_str!("../../../web/scripts/app.js"),
        "text/javascript; charset=utf-8",
    )
    .await
}
pub async fn chart() -> Response {
    web_asset(
        "assets/chart.umd.min.js",
        include_str!("../../../web/assets/chart.umd.min.js"),
        "text/javascript; charset=utf-8",
    )
    .await
}
pub async fn hammer() -> Response {
    web_asset(
        "assets/hammer.min.js",
        include_str!("../../../web/assets/hammer.min.js"),
        "text/javascript; charset=utf-8",
    )
    .await
}
pub async fn chart_zoom() -> Response {
    web_asset(
        "assets/chartjs-plugin-zoom.min.js",
        include_str!("../../../web/assets/chartjs-plugin-zoom.min.js"),
        "text/javascript; charset=utf-8",
    )
    .await
}

async fn web_asset(
    relative_path: &str,
    embedded: &'static str,
    content_type: &'static str,
) -> Response {
    #[cfg(not(debug_assertions))]
    let _ = relative_path;
    #[cfg(debug_assertions)]
    let body = if let Some(root) = std::env::var_os("SENTINEL_WEB_DIR") {
        match tokio::fs::read_to_string(FilePath::new(&root).join(relative_path)).await {
            Ok(contents) => contents,
            Err(error) => {
                return internal(
                    anyhow::anyhow!(error)
                        .context(format!("read development web asset {relative_path}")),
                )
            }
        }
    } else {
        embedded.to_owned()
    };
    #[cfg(not(debug_assertions))]
    let body = embedded.to_owned();

    ([(axum::http::header::CONTENT_TYPE, content_type)], body).into_response()
}

fn internal(error: anyhow::Error) -> Response {
    tracing::error!(%error, "request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error":"internal application error"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    #[test]
    fn dashboard_contains_required_controls_and_chart_regions() {
        let html = include_str!("../../../web/index.html");
        assert!(html.contains("Run all tests"));
        assert!(html.contains("throughput-chart"));
        assert!(html.contains("latency-chart"));
        assert!(html.contains("jitter-chart"));
        assert!(html.contains("bufferbloat-chart"));
        assert!(html.contains("chartjs-plugin-zoom"));
        assert!(html.contains("Recent runs"));
    }
}
