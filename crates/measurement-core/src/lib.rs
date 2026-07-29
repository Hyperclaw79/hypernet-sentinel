#![allow(dead_code)]
//! Sentinel-owned adapter around the selected GPL-3.0 upstream measurement engine.
//! The `upstream` directory is mechanically refreshed; application behavior belongs here.

#[path = "upstream/constants.rs"]
pub(crate) mod constants;
#[path = "upstream/engine/mod.rs"]
pub(crate) mod engine;
#[path = "upstream/metrics.rs"]
pub(crate) mod metrics;
#[path = "upstream/model.rs"]
pub(crate) mod model;
#[path = "upstream/stats.rs"]
pub(crate) mod stats;

mod adapter;

pub use adapter::{
    DiagnosticConfig, DiagnosticKind, MeasurementReport, ProgressEvent, Runner, TestSelection,
};

pub const UPSTREAM_REVISION: &str = include_str!("../UPSTREAM_REVISION");

// Upstream's socket binding module needs only this predicate from its broader
// CLI network-information module, which is intentionally not imported.
pub(crate) mod network {
    pub fn is_link_local_v6(ip: &std::net::Ipv6Addr) -> bool {
        (ip.segments()[0] & 0xffc0) == 0xfe80
    }
}
