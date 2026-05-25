use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerStats {
    pub name: String,
    pub container_id: String,
    pub cpu_percent: Option<f64>,
    pub mem_usage_bytes: u64,
    pub mem_limit_bytes: u64,
    pub mem_percent: f64,
    pub net_rx_bps: Option<f64>,
    pub net_tx_bps: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostDisk {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeDisk {
    pub volume_name: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InfraChartPoint {
    pub timestamp: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InfraTimeseriesResponse {
    pub window: String,
    pub container_id: String,
    pub cpu: Vec<InfraChartPoint>,
    pub memory: Vec<InfraChartPoint>,
    pub net_rx: Vec<InfraChartPoint>,
    pub net_tx: Vec<InfraChartPoint>,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestRequest {
    pub direction: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeedTestAccepted {
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeedTestJobView {
    pub job_id: String,
    pub status: String,
    pub direction: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub error: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub upload_token: Option<String>,
    #[serde(default)]
    pub upload_bytes: Option<u64>,
}
