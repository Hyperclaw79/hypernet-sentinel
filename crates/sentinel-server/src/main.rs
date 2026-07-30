mod api;
mod config;
mod coordinator;
mod db;
mod mcp;
mod models;
mod scheduler;

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use config::Config;
use coordinator::Coordinator;
use db::Database;
use measurement_core::Runner;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use scheduler::SchedulerHealth;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct AppState {
    db: Database,
    coordinator: Arc<Coordinator>,
    scheduler_health: Arc<SchedulerHealth>,
    scheduler_enabled: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|argument| argument == "--healthcheck") {
        let bind = std::env::var("SENTINEL_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let port = bind.rsplit(':').next().unwrap_or("8080");
        let response = reqwest::get(format!("http://127.0.0.1:{port}/healthz")).await?;
        anyhow::ensure!(
            response.status().is_success(),
            "health endpoint returned {}",
            response.status()
        );
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sentinel_server=info,tower_http=info".into()),
        )
        .init();
    let config = Config::from_env()?;
    let shutdown = CancellationToken::new();
    let db = Database::open(&config.data_dir).await?;
    let coordinator = Coordinator::new(
        db.clone(),
        Arc::new(Runner::new(config.diagnostic.clone())),
        config.run_timeout,
        shutdown.clone(),
    );
    let scheduler_health = SchedulerHealth::new();
    let state = AppState {
        db: db.clone(),
        coordinator: coordinator.clone(),
        scheduler_health: scheduler_health.clone(),
        scheduler_enabled: config.scheduler_enabled,
    };
    if config.scheduler_enabled {
        scheduler::spawn(
            db,
            coordinator,
            config.quality_interval,
            config.full_interval,
            config.retention_days,
            shutdown.clone(),
            scheduler_health,
        );
    }

    let mcp_state = state.clone();
    let mcp_service = StreamableHttpService::new(
        move || Ok(mcp::SentinelMcp::new(mcp_state.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .disable_allowed_hosts()
            .with_cancellation_token(shutdown.child_token()),
    );
    let app = Router::new()
        .route("/", get(api::index))
        .route("/manifest.webmanifest", get(api::manifest))
        .route("/icons/{name}", get(api::icon))
        .route("/assets/chart.umd.min.js", get(api::chart))
        .route("/assets/hammer.min.js", get(api::hammer))
        .route("/assets/chartjs-plugin-zoom.min.js", get(api::chart_zoom))
        .route("/styles/app.css", get(api::styles))
        .route("/scripts/app.js", get(api::script))
        .route("/healthz", get(api::health))
        .route("/api/summary", get(api::summary))
        .route("/api/results", get(api::results))
        .route("/api/current", get(api::current))
        .route("/api/run", post(api::run))
        .nest_service("/mcp", mcp_service)
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(address=%config.bind, "Hypernet Sentinel ready");
    let stop = shutdown.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            stop.cancel();
        })
        .await?;
    Ok(())
}
