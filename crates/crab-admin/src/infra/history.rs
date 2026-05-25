//! Ring buffer for infrastructure metric history (CPU, memory, disk, network rates).
//!
//! Each tick samples current snapshot values into a ring of `InfraSamplePoint` entries.
//! The ring is in-memory only (unlike `metrics_history` which persists to SQLite).

use crate::infra::types::InfraSnapshot;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const MAX_SAMPLES: usize = 1500;
const DEFAULT_INTERVAL_SECS: u64 = 60;

/// A single infrastructure history sample point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraSamplePoint {
    pub sampled_at: u64,
    /// container_id -> aggregated metrics
    pub containers: HashMap<String, ContainerHistoryEntry>,
    /// Host disk usage summary (root /)
    pub host_disk_used_bytes: u64,
    pub host_disk_total_bytes: u64,
    /// Sum of net rx/tx across all containers (bps)
    pub total_net_rx_bps: f64,
    pub total_net_tx_bps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerHistoryEntry {
    pub cpu_percent: f64,
    pub mem_percent: f64,
    pub mem_usage_bytes: u64,
    pub net_rx_bps: f64,
    pub net_tx_bps: f64,
}

/// In-memory ring buffer of infra samples.
#[derive(Debug, Clone, Default)]
pub struct InfraHistoryRing {
    samples: Vec<InfraSamplePoint>,
}

impl InfraHistoryRing {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, snapshot: &InfraSnapshot) {
        let containers: HashMap<String, ContainerHistoryEntry> = snapshot
            .containers
            .iter()
            .map(|c| {
                (
                    c.container_id.clone(),
                    ContainerHistoryEntry {
                        cpu_percent: c.cpu_percent.unwrap_or(0.0),
                        mem_percent: c.mem_percent,
                        mem_usage_bytes: c.mem_usage_bytes,
                        net_rx_bps: c.net_rx_bps.unwrap_or(0.0),
                        net_tx_bps: c.net_tx_bps.unwrap_or(0.0),
                    },
                )
            })
            .collect();

        let host_disk = snapshot
            .host_disks
            .first()
            .map(|d| (d.used_bytes, d.total_bytes))
            .unwrap_or((0, 0));

        let total_net_rx_bps: f64 = containers.values().map(|c| c.net_rx_bps).sum();
        let total_net_tx_bps: f64 = containers.values().map(|c| c.net_tx_bps).sum();

        let point = InfraSamplePoint {
            sampled_at: snapshot.collected_at,
            containers,
            host_disk_used_bytes: host_disk.0,
            host_disk_total_bytes: host_disk.1,
            total_net_rx_bps,
            total_net_tx_bps,
        };

        if let Some(last) = self.samples.last()
            && point.sampled_at <= last.sampled_at
        {
            return;
        }

        self.samples.push(point);
        self.trim();
    }

    fn trim(&mut self) {
        if self.samples.len() > MAX_SAMPLES {
            let drop = self.samples.len() - MAX_SAMPLES;
            self.samples.drain(0..drop);
        }
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn oldest_at(&self) -> u64 {
        self.samples.first().map(|s| s.sampled_at).unwrap_or(0)
    }

    /// Get points within a time window (seconds before `now`).
    pub fn points_since(&self, window_secs: u64) -> Vec<&InfraSamplePoint> {
        let cutoff = self
            .samples
            .last()
            .map(|s| s.sampled_at.saturating_sub(window_secs))
            .unwrap_or(0);
        self.samples
            .iter()
            .filter(|s| s.sampled_at >= cutoff)
            .collect()
    }

    /// Get all points.
    pub fn all_points(&self) -> &[InfraSamplePoint] {
        &self.samples
    }
}

pub fn sample_interval_secs() -> u64 {
    std::env::var("CRABCACHE_INFRA_SAMPLE_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

/// Convert infra history to a timeline of time-series points compatible with the
/// existing LineChart component.
pub fn to_timeseries(
    points: &[InfraSamplePoint],
    container_id: &str,
) -> (
    Vec<ChartPoint>,
    Vec<ChartPoint>,
    Vec<ChartPoint>,
    Vec<ChartPoint>,
) {
    let mut cpu_series = Vec::new();
    let mut mem_series = Vec::new();
    let mut rx_series = Vec::new();
    let mut tx_series = Vec::new();

    for p in points {
        let ts = DateTime::from_timestamp(p.sampled_at as i64, 0)
            .map(|dt| dt.format("%H:%M").to_string())
            .unwrap_or_else(|| p.sampled_at.to_string());

        cpu_series.push(ChartPoint {
            timestamp: ts.clone(),
            value: p
                .containers
                .get(container_id)
                .map(|c| c.cpu_percent)
                .unwrap_or(0.0),
        });
        mem_series.push(ChartPoint {
            timestamp: ts.clone(),
            value: p
                .containers
                .get(container_id)
                .map(|c| c.mem_percent)
                .unwrap_or(0.0),
        });
        let entry = p.containers.get(container_id);
        rx_series.push(ChartPoint {
            timestamp: ts.clone(),
            value: entry.map(|c| c.net_rx_bps).unwrap_or(0.0),
        });
        tx_series.push(ChartPoint {
            timestamp: ts,
            value: entry.map(|c| c.net_tx_bps).unwrap_or(0.0),
        });
    }

    (cpu_series, mem_series, rx_series, tx_series)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartPoint {
    pub timestamp: String,
    pub value: f64,
}
