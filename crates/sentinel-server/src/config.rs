use anyhow::{Context, Result};
use measurement_core::DiagnosticConfig;
use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub quality_interval: Duration,
    pub full_interval: Duration,
    pub run_timeout: Duration,
    pub retention_days: i64,
    pub scheduler_enabled: bool,
    pub diagnostic: DiagnosticConfig,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let diagnostic = DiagnosticConfig {
            download_duration: Duration::from_secs(read("SENTINEL_DOWNLOAD_SECONDS", 10)?),
            upload_duration: Duration::from_secs(read("SENTINEL_UPLOAD_SECONDS", 10)?),
            idle_latency_duration: Duration::from_secs(read("SENTINEL_LATENCY_SECONDS", 2)?),
            udp_packets: read("SENTINEL_UDP_PACKETS", 50)?,
            concurrency: read("SENTINEL_CONCURRENCY", 6)?,
            ..DiagnosticConfig::default()
        };
        Ok(Self {
            bind: env::var("SENTINEL_BIND")
                .unwrap_or_else(|_| "0.0.0.0:8080".into())
                .parse()
                .context("invalid SENTINEL_BIND")?,
            data_dir: env::var_os("SENTINEL_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/data")),
            quality_interval: Duration::from_secs(read(
                "SENTINEL_QUALITY_INTERVAL_SECONDS",
                3_600,
            )?),
            full_interval: Duration::from_secs(read("SENTINEL_FULL_INTERVAL_SECONDS", 14_400)?),
            run_timeout: Duration::from_secs(read("SENTINEL_RUN_TIMEOUT_SECONDS", 90)?),
            retention_days: read("SENTINEL_RETENTION_DAYS", 90)?,
            scheduler_enabled: env::var("SENTINEL_SCHEDULER_ENABLED")
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
            diagnostic,
        })
    }
}

fn read<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => value.parse().with_context(|| format!("invalid {name}")),
        Err(_) => Ok(default),
    }
}
