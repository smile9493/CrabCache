//! Infra monitoring API types (shared) and internal collector state.

pub use crab_admin_types::{
    ContainerStats, HostDisk, InfraChartPoint, InfraSnapshot, InfraStatus, InfraTimeseriesResponse,
    SpeedTestAccepted, SpeedTestRequest, VolumeDisk,
};

use std::time::Instant;

/// Raw per-container counters stored between polls for rate computation.
#[derive(Debug, Clone)]
pub struct ContainerRawSample {
    pub cpu_total_usage: u64,
    pub system_cpu_usage: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub online_cpus: u32,
    pub sampled_at: Instant,
}
