/// Key used with `pingora_limits::rate::Rate` for global gateway RPS tracking.
/// Shared between `MetricsServer` (main.rs) and the management API status handler.
pub use crab_metrics::GLOBAL_RATE_KEY;

pub mod config;
pub mod management;
pub mod pg_control_store;
pub mod webhook;
pub mod webhook_admin;
#[cfg(feature = "otel")]
pub mod otel;
