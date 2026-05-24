use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Snapshot of all monitored containers and host disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraSnapshot {
    pub containers: Vec<ContainerStats>,
    pub host_disks: Vec<HostDisk>,
    pub volumes: Vec<VolumeDisk>,
    pub collected_at: u64,
    pub compose_project: String,
    #[serde(default)]
    pub docker_connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_error: Option<String>,
}

/// Per-container stats snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub name: String,
    pub container_id: String,
    /// CPU usage percent (null on first sample).
    pub cpu_percent: Option<f64>,
    pub mem_usage_bytes: u64,
    pub mem_limit_bytes: u64,
    pub mem_percent: f64,
    /// Network receive bytes/sec (null on first sample).
    pub net_rx_bps: Option<f64>,
    /// Network transmit bytes/sec (null on first sample).
    pub net_tx_bps: Option<f64>,
    pub status: String,
}

/// Host disk usage for a single mount point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDisk {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

/// Lightweight status endpoint response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraStatus {
    pub docker_connected: bool,
    pub compose_project: String,
    #[serde(default = "default_poll_hint")]
    pub poll_hint_secs: u64,
    pub history_sample_count: usize,
    pub last_collected_at: Option<u64>,
}

fn default_poll_hint() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraChartPoint {
    pub timestamp: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraTimeseriesResponse {
    pub window: String,
    pub container_id: String,
    pub cpu: Vec<InfraChartPoint>,
    pub memory: Vec<InfraChartPoint>,
    pub net_rx: Vec<InfraChartPoint>,
    pub net_tx: Vec<InfraChartPoint>,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpeedTestRequest {
    pub direction: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpeedTestAccepted {
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_token: Option<String>,
}

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

/// Docker named volume disk usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeDisk {
    pub volume_name: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}
