//! Overview / metrics API types (batch A).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GatewayHealth {
    pub healthy: bool,
    #[serde(default)]
    pub uptime_secs: u64,
    #[serde(default)]
    pub active_keys: u64,
    #[serde(default)]
    pub backend_count: usize,
    #[serde(default)]
    pub stream_cache_enabled: bool,
    #[serde(default)]
    pub upstream_key_count: u32,
    #[serde(default)]
    pub upstream_keys_available: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub redis_connected: bool,
    #[serde(default)]
    pub qdrant_connected: bool,
    /// Number of healthy backends (default profile).
    #[serde(default)]
    pub backends_healthy: usize,
    /// Total number of backends (default profile).
    #[serde(default)]
    pub backends_total: usize,
    /// Number of unhealthy backends (managed by Pingora health checks).
    #[serde(default)]
    pub backends_unhealthy: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub similarity_threshold: f64,
    #[serde(default)]
    pub ttl_secs: u64,
    #[serde(default)]
    pub min_query_chars: usize,
    #[serde(default)]
    pub max_query_chars: usize,
    #[serde(default)]
    pub max_concurrent_embeds: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: String,
    pub requests: u64,
    pub tokens: u64,
    pub cache_hits: u64,
    #[serde(default)]
    pub avg_latency_ms: f64,
    #[serde(default)]
    pub hit_rate: f64,
    /// Per-tier hit rate (% of total requests in bucket), 0–100.
    #[serde(default)]
    pub l0_hit_rate: f64,
    #[serde(default)]
    pub l1_hit_rate: f64,
    #[serde(default)]
    pub l2_hit_rate: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MetricsHistoryMeta {
    pub sample_count: usize,
    pub oldest_sample_at_secs: u64,
    pub sampling_interval_secs: u64,
    #[serde(default)]
    pub gateway_counter_reset: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TierDeltas5m {
    pub l0: u64,
    pub l1: u64,
    pub l2: u64,
    pub miss: u64,
    pub coalesced: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsumerMetricsBucket {
    pub consumer: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    #[serde(default)]
    pub hit_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainMetricsBucket {
    pub domain: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    #[serde(default)]
    pub hit_ratio: f64,
    #[serde(default)]
    pub cost_saved_usd: f64,
    #[serde(default)]
    pub qps_5m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    #[serde(default)]
    pub qps: f64,
    #[serde(default)]
    pub tps: f64,
    pub l0_hits: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub latency_l0_ms: f64,
    #[serde(default)]
    pub latency_l1_ms: f64,
    #[serde(default)]
    pub latency_l2_ms: f64,
    #[serde(default)]
    pub latency_upstream_ms: f64,
    pub active_keys: u64,
    pub uptime_hours: u64,
    #[serde(default)]
    pub uptime_secs: u64,
    pub hourly_stats: Vec<TimeSeriesPoint>,
    pub daily_stats: Vec<TimeSeriesPoint>,
    pub weekly_stats: Vec<TimeSeriesPoint>,
    pub monthly_stats: Vec<TimeSeriesPoint>,
    #[serde(default)]
    pub semantic_hits: u64,
    #[serde(default)]
    pub semantic_rejected: u64,
    #[serde(default)]
    pub semantic_skipped: u64,
    #[serde(default)]
    pub prefix_cache_hit_tokens: u64,
    #[serde(default)]
    pub prefix_cache_miss_tokens: u64,
    #[serde(default)]
    pub prefix_cache_hit_ratio: f64,
    #[serde(default)]
    pub hit_rate_cumulative: f64,
    #[serde(default)]
    pub hit_rate_5m: f64,
    #[serde(default)]
    pub token_hit_rate_5m: f64,
    #[serde(default)]
    pub qps_5m: f64,
    #[serde(default)]
    pub coalesced_total: u64,
    #[serde(default)]
    pub consumer_buckets: Vec<ConsumerMetricsBucket>,
    #[serde(default)]
    pub domain_buckets: Vec<DomainMetricsBucket>,
    #[serde(default)]
    pub metrics_sample_insufficient: bool,
    #[serde(default)]
    pub history_meta: MetricsHistoryMeta,
    #[serde(default)]
    pub tier_deltas_5m: TierDeltas5m,
    #[serde(default)]
    pub latency_upstream_p99_ms: f64,
    #[serde(default)]
    pub latency_ttft_p99_ms: f64,
    #[serde(default)]
    pub latency_prefill_p99_ms: f64,
    #[serde(default)]
    pub latency_cache_fetch_p99_ms: f64,
    #[serde(default)]
    pub error_rate_5m: f64,
    #[serde(default)]
    pub http_4xx_5m: u64,
    #[serde(default)]
    pub http_5xx_5m: u64,
    #[serde(default)]
    pub qps_prev_1h: f64,
    #[serde(default)]
    pub hit_rate_prev_1h: f64,
    #[serde(default)]
    pub pg_total_input_tokens: u64,
    #[serde(default)]
    pub pg_total_output_tokens: u64,
    #[serde(default)]
    pub pg_total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsSnapshotCore {
    #[serde(default)]
    pub qps: f64,
    #[serde(default)]
    pub tps: f64,
    pub l0_hits: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub latency_l0_ms: f64,
    #[serde(default)]
    pub latency_l1_ms: f64,
    #[serde(default)]
    pub latency_l2_ms: f64,
    #[serde(default)]
    pub latency_upstream_ms: f64,
    pub active_keys: u64,
    pub uptime_hours: u64,
    #[serde(default)]
    pub uptime_secs: u64,
    #[serde(default)]
    pub semantic_hits: u64,
    #[serde(default)]
    pub semantic_rejected: u64,
    #[serde(default)]
    pub semantic_skipped: u64,
    #[serde(default)]
    pub prefix_cache_hit_tokens: u64,
    #[serde(default)]
    pub prefix_cache_miss_tokens: u64,
    #[serde(default)]
    pub prefix_cache_hit_ratio: f64,
    #[serde(default)]
    pub hit_rate_cumulative: f64,
    #[serde(default)]
    pub hit_rate_5m: f64,
    #[serde(default)]
    pub token_hit_rate_5m: f64,
    #[serde(default)]
    pub qps_5m: f64,
    #[serde(default)]
    pub coalesced_total: u64,
    #[serde(default)]
    pub consumer_buckets: Vec<ConsumerMetricsBucket>,
    #[serde(default)]
    pub domain_buckets: Vec<DomainMetricsBucket>,
    #[serde(default)]
    pub metrics_sample_insufficient: bool,
    #[serde(default)]
    pub history_meta: MetricsHistoryMeta,
    #[serde(default)]
    pub tier_deltas_5m: TierDeltas5m,
    #[serde(default)]
    pub latency_upstream_p99_ms: f64,
    #[serde(default)]
    pub latency_ttft_p99_ms: f64,
    #[serde(default)]
    pub latency_prefill_p99_ms: f64,
    #[serde(default)]
    pub latency_cache_fetch_p99_ms: f64,
    #[serde(default)]
    pub error_rate_5m: f64,
    #[serde(default)]
    pub http_4xx_5m: u64,
    #[serde(default)]
    pub http_5xx_5m: u64,
    #[serde(default)]
    pub qps_prev_1h: f64,
    #[serde(default)]
    pub hit_rate_prev_1h: f64,
    #[serde(default)]
    pub pg_total_input_tokens: u64,
    #[serde(default)]
    pub pg_total_output_tokens: u64,
    #[serde(default)]
    pub pg_total_tokens: u64,
}

fn fin(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

impl MetricsSnapshotCore {
    pub fn sanitize_finite(&mut self) {
        self.qps = fin(self.qps);
        self.tps = fin(self.tps);
        self.latency_l0_ms = fin(self.latency_l0_ms);
        self.latency_l1_ms = fin(self.latency_l1_ms);
        self.latency_l2_ms = fin(self.latency_l2_ms);
        self.latency_upstream_ms = fin(self.latency_upstream_ms);
        self.prefix_cache_hit_ratio = fin(self.prefix_cache_hit_ratio);
        self.hit_rate_cumulative = fin(self.hit_rate_cumulative);
        self.hit_rate_5m = fin(self.hit_rate_5m);
        self.token_hit_rate_5m = fin(self.token_hit_rate_5m);
        self.qps_5m = fin(self.qps_5m);
        self.latency_upstream_p99_ms = fin(self.latency_upstream_p99_ms);
        self.latency_ttft_p99_ms = fin(self.latency_ttft_p99_ms);
        self.latency_prefill_p99_ms = fin(self.latency_prefill_p99_ms);
        self.latency_cache_fetch_p99_ms = fin(self.latency_cache_fetch_p99_ms);
        self.error_rate_5m = fin(self.error_rate_5m);
        self.qps_prev_1h = fin(self.qps_prev_1h);
        self.hit_rate_prev_1h = fin(self.hit_rate_prev_1h);
    }
}

impl MetricsSnapshot {
    pub fn sanitize_finite(&mut self) {
        self.qps = fin(self.qps);
        self.tps = fin(self.tps);
        self.latency_l0_ms = fin(self.latency_l0_ms);
        self.latency_l1_ms = fin(self.latency_l1_ms);
        self.latency_l2_ms = fin(self.latency_l2_ms);
        self.latency_upstream_ms = fin(self.latency_upstream_ms);
        self.prefix_cache_hit_ratio = fin(self.prefix_cache_hit_ratio);
        self.hit_rate_cumulative = fin(self.hit_rate_cumulative);
        self.hit_rate_5m = fin(self.hit_rate_5m);
        self.token_hit_rate_5m = fin(self.token_hit_rate_5m);
        self.qps_5m = fin(self.qps_5m);
        self.latency_upstream_p99_ms = fin(self.latency_upstream_p99_ms);
        self.latency_ttft_p99_ms = fin(self.latency_ttft_p99_ms);
        self.latency_prefill_p99_ms = fin(self.latency_prefill_p99_ms);
        self.latency_cache_fetch_p99_ms = fin(self.latency_cache_fetch_p99_ms);
        self.error_rate_5m = fin(self.error_rate_5m);
        self.qps_prev_1h = fin(self.qps_prev_1h);
        self.hit_rate_prev_1h = fin(self.hit_rate_prev_1h);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverviewOpsMetrics {
    #[serde(default)]
    pub cost_saved_usd_total: f64,
    #[serde(default)]
    pub cost_saved_usd_5m: f64,
    pub coalesced_total: u64,
    pub coalesced_5m: u64,
    pub rejected_total: u64,
    pub rejected_5m: u64,
    #[serde(default)]
    pub ttft_ms: f64,
    pub prefix_break_total: u64,
    pub reasoning_store_hits: u64,
    pub reasoning_store_misses: u64,
    pub stream_cache_sse_omitted: u64,
    pub upstream_key_count: u32,
    pub upstream_keys_available: u32,
    /// Default upstream profile id (keys above are for this profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_default_profile_id: Option<String>,
}

impl OverviewOpsMetrics {
    pub fn sanitize_finite(&mut self) {
        self.cost_saved_usd_total = fin(self.cost_saved_usd_total);
        self.cost_saved_usd_5m = fin(self.cost_saved_usd_5m);
        self.ttft_ms = fin(self.ttft_ms);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceSummary {
    pub hours: u32,
    pub total_requests: usize,
    #[serde(default)]
    pub cache_hit_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewSuggestion {
    pub severity: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrefixCacheModelBucket {
    pub model: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    #[serde(default)]
    pub hit_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrefixCacheMetricsSnapshot {
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    #[serde(default)]
    pub hit_ratio: f64,
    pub by_model: Vec<PrefixCacheModelBucket>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewCore {
    pub metrics: MetricsSnapshotCore,
    pub health: GatewayHealth,
    pub prefix_cache: PrefixCacheMetricsSnapshot,
    pub semantic: SemanticConfig,
    pub ops: OverviewOpsMetrics,
    #[serde(default)]
    pub suggestions: Vec<OverviewSuggestion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewTimeseriesResponse {
    pub window: String,
    pub points: Vec<TimeSeriesPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewBundle {
    pub metrics: MetricsSnapshot,
    pub health: GatewayHealth,
    pub prefix_cache: PrefixCacheMetricsSnapshot,
    pub semantic: SemanticConfig,
    pub trace_summary: TraceSummary,
    pub ops: OverviewOpsMetrics,
    #[serde(default)]
    pub suggestions: Vec<OverviewSuggestion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_core_roundtrip() {
        let core = OverviewCore {
            metrics: MetricsSnapshotCore {
                qps: 1.0,
                tps: 2.0,
                l0_hits: 1,
                l1_hits: 2,
                l2_hits: 3,
                cache_misses: 4,
                cache_hit_tokens: 5,
                cache_miss_tokens: 6,
                total_input_tokens: 7,
                total_output_tokens: 8,
                total_tokens: 15,
                latency_l0_ms: 0.1,
                latency_l1_ms: 1.0,
                latency_l2_ms: 10.0,
                latency_upstream_ms: 100.0,
                active_keys: 2,
                uptime_hours: 1,
                uptime_secs: 3600,
                semantic_hits: 0,
                semantic_rejected: 0,
                semantic_skipped: 0,
                prefix_cache_hit_tokens: 0,
                prefix_cache_miss_tokens: 0,
                prefix_cache_hit_ratio: 0.0,
                hit_rate_cumulative: 0.5,
                hit_rate_5m: 0.6,
                token_hit_rate_5m: 0.7,
                qps_5m: 1.0,
                coalesced_total: 0,
                consumer_buckets: vec![],
                domain_buckets: vec![],
                metrics_sample_insufficient: false,
                history_meta: MetricsHistoryMeta::default(),
                tier_deltas_5m: TierDeltas5m::default(),
                latency_upstream_p99_ms: 0.0,
                latency_ttft_p99_ms: 0.0,
                latency_prefill_p99_ms: 0.0,
                latency_cache_fetch_p99_ms: 0.0,
                error_rate_5m: 0.0,
                http_4xx_5m: 0,
                http_5xx_5m: 0,
                qps_prev_1h: 0.0,
                hit_rate_prev_1h: 0.0,
            },
            health: GatewayHealth::default(),
            prefix_cache: PrefixCacheMetricsSnapshot {
                hit_tokens: 0,
                miss_tokens: 0,
                hit_ratio: 0.0,
                by_model: vec![],
            },
            semantic: SemanticConfig {
                enabled: true,
                similarity_threshold: 0.95,
            },
            ops: OverviewOpsMetrics::default(),
            suggestions: vec![],
        };
        let json = serde_json::to_string(&core).expect("serialize");
        let back: OverviewCore = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(core, back);
    }
}
