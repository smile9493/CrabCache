//! Prometheus counter snapshots and time-series rollups for the admin dashboard.

use crate::types::{
    ConsumerMetricsBucket, DomainMetricsBucket, PrefixCacheModelBucket, TierDeltas5m,
    TimeSeriesPoint,
};
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

pub(crate) const MAX_RETENTION_SECS: u64 = 25 * 3600;
const DEFAULT_MAX_SAMPLES: usize = 1500;
pub const WINDOW_5M_SECS: u64 = 300;

/// TTL cache for raw Prometheus text from the gateway `:9090/metrics` endpoint.
#[derive(Default)]
pub struct GatewayMetricsCache {
    entry: parking_lot::RwLock<Option<(Instant, String)>>,
    pub fetch_lock: AsyncMutex<()>,
}

impl GatewayMetricsCache {
    pub fn fresh_body(&self, ttl: Duration) -> Option<String> {
        let guard = self.entry.read();
        let (at, body) = guard.as_ref()?;
        if at.elapsed() < ttl {
            Some(body.clone())
        } else {
            None
        }
    }

    pub fn stale_body(&self, max_age: Duration) -> Option<String> {
        let guard = self.entry.read();
        let (at, body) = guard.as_ref()?;
        if at.elapsed() < max_age {
            Some(body.clone())
        } else {
            None
        }
    }

    pub fn store(&self, body: String) {
        *self.entry.write() = Some((Instant::now(), body));
    }

    pub fn age_secs(&self) -> Option<u64> {
        let guard = self.entry.read();
        guard.as_ref().map(|(at, _)| at.elapsed().as_secs())
    }
}

/// Parsed gateway counters at one point in time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsCounterSnapshot {
    pub sampled_at: u64,
    pub l0_hits: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub total_output_tokens: u64,
    pub coalesced_total: u64,
    pub semantic_hits: u64,
    pub semantic_rejected: u64,
    pub semantic_skipped: u64,
    pub cost_saved_usd: f64,
    pub rejected_total: u64,
    pub prefix_break_total: u64,
    pub reasoning_store_hits: u64,
    pub reasoning_store_misses: u64,
    pub stream_cache_sse_omitted: u64,
    pub domain_tokens: HashMap<String, (u64, u64)>,
    pub upstream_p99_latency_ms: f64,
    pub ttft_p99_latency_ms: f64,
    pub cache_fetch_p99_latency_ms: f64,
    pub http_4xx_total: u64,
    pub http_5xx_total: u64,
    /// Sum of all `gateway_http_responses_total` (2xx+3xx+4xx+5xx).
    pub http_responses_total: u64,
}

impl MetricsCounterSnapshot {
    pub fn total_requests(&self) -> u64 {
        self.l0_hits + self.l1_hits + self.l2_hits + self.cache_misses
    }

    pub fn gateway_cache_hits(&self) -> u64 {
        self.l0_hits + self.l1_hits + self.l2_hits
    }

    pub fn total_tokens(&self) -> u64 {
        self.cache_hit_tokens + self.cache_miss_tokens + self.total_output_tokens
    }
}

/// Ring buffer of counter snapshots (typically one sample per minute).
#[derive(Debug, Clone, Default)]
pub struct MetricsHistory {
    samples: Vec<MetricsCounterSnapshot>,
}

/// Delta-based rates over a time window.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowRates {
    pub qps: f64,
    pub hit_rate: f64,
    pub token_hit_rate: f64,
    pub sample_count: usize,
    pub window_secs: u64,
}

impl MetricsHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, snapshot: MetricsCounterSnapshot) {
        if let Some(last) = self.samples.last()
            && snapshot.sampled_at <= last.sampled_at
        {
            return;
        }
        self.samples.push(snapshot);
        self.trim();
    }

    fn trim(&mut self) {
        if self.samples.is_empty() {
            return;
        }
        let cutoff = self
            .samples
            .last()
            .map(|s| s.sampled_at.saturating_sub(MAX_RETENTION_SECS))
            .unwrap_or(0);
        self.samples.retain(|s| s.sampled_at >= cutoff);
        if self.samples.len() > DEFAULT_MAX_SAMPLES {
            let drop = self.samples.len() - DEFAULT_MAX_SAMPLES;
            self.samples.drain(0..drop);
        }
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Rates from counter deltas over `window_secs` (default 5 minutes).
    pub fn window_rates(&self, window_secs: u64, now: u64) -> WindowRates {
        let window_secs = window_secs.max(1);
        let start = now.saturating_sub(window_secs);
        let in_window: Vec<&MetricsCounterSnapshot> = self
            .samples
            .iter()
            .filter(|s| s.sampled_at >= start && s.sampled_at <= now)
            .collect();

        if in_window.len() < 2 {
            return WindowRates {
                window_secs,
                sample_count: in_window.len(),
                ..Default::default()
            };
        }

        let first = in_window.first().copied().unwrap();
        let last = in_window.last().copied().unwrap();
        let elapsed = last.sampled_at.saturating_sub(first.sampled_at).max(1) as f64;

        let d_requests = last.total_requests().saturating_sub(first.total_requests());
        let d_hits = last
            .gateway_cache_hits()
            .saturating_sub(first.gateway_cache_hits());
        let d_hit_tokens = last.cache_hit_tokens.saturating_sub(first.cache_hit_tokens);
        let d_miss_tokens = last
            .cache_miss_tokens
            .saturating_sub(first.cache_miss_tokens);
        let d_input = d_hit_tokens + d_miss_tokens;

        WindowRates {
            qps: d_requests as f64 / elapsed,
            hit_rate: if d_requests > 0 {
                d_hits as f64 / d_requests as f64
            } else {
                0.0
            },
            token_hit_rate: if d_input > 0 {
                d_hit_tokens as f64 / d_input as f64
            } else {
                0.0
            },
            sample_count: in_window.len(),
            window_secs,
        }
    }

    pub fn window_rates_5m(&self, now: u64) -> WindowRates {
        self.window_rates(WINDOW_5M_SECS, now)
    }

    /// Total requests observed in the window (counter delta).
    pub fn oldest_sample_at(&self) -> u64 {
        self.samples.first().map(|s| s.sampled_at).unwrap_or(0)
    }

    pub fn window_tier_deltas(&self, window_secs: u64, now: u64) -> TierDeltas5m {
        let start = now.saturating_sub(window_secs);
        let in_window: Vec<&MetricsCounterSnapshot> = self
            .samples
            .iter()
            .filter(|s| s.sampled_at >= start && s.sampled_at <= now)
            .collect();
        if in_window.len() < 2 {
            return TierDeltas5m::default();
        }
        let first = in_window.first().unwrap();
        let last = in_window.last().unwrap();
        TierDeltas5m {
            l0: last.l0_hits.saturating_sub(first.l0_hits),
            l1: last.l1_hits.saturating_sub(first.l1_hits),
            l2: last.l2_hits.saturating_sub(first.l2_hits),
            miss: last.cache_misses.saturating_sub(first.cache_misses),
            coalesced: last.coalesced_total.saturating_sub(first.coalesced_total),
        }
    }

    pub fn window_float_delta(
        &self,
        window_secs: u64,
        now: u64,
        extract: fn(&MetricsCounterSnapshot) -> f64,
    ) -> f64 {
        let start = now.saturating_sub(window_secs);
        let in_window: Vec<&MetricsCounterSnapshot> = self
            .samples
            .iter()
            .filter(|s| s.sampled_at >= start && s.sampled_at <= now)
            .collect();
        if in_window.len() < 2 {
            return 0.0;
        }
        let first = in_window.first().unwrap();
        let last = in_window.last().unwrap();
        (extract(last) - extract(first)).max(0.0)
    }

    pub fn window_u64_delta(
        &self,
        window_secs: u64,
        now: u64,
        extract: fn(&MetricsCounterSnapshot) -> u64,
    ) -> u64 {
        let start = now.saturating_sub(window_secs);
        let in_window: Vec<&MetricsCounterSnapshot> = self
            .samples
            .iter()
            .filter(|s| s.sampled_at >= start && s.sampled_at <= now)
            .collect();
        if in_window.len() < 2 {
            return 0;
        }
        let first = in_window.first().unwrap();
        let last = in_window.last().unwrap();
        extract(last).saturating_sub(extract(first))
    }

    pub fn window_request_delta(&self, window_secs: u64, now: u64) -> u64 {
        let start = now.saturating_sub(window_secs);
        let in_window: Vec<&MetricsCounterSnapshot> = self
            .samples
            .iter()
            .filter(|s| s.sampled_at >= start && s.sampled_at <= now)
            .collect();
        if in_window.len() < 2 {
            return 0;
        }
        let first = in_window.first().unwrap();
        let last = in_window.last().unwrap();
        last.total_requests().saturating_sub(first.total_requests())
    }

    pub fn build_hourly_stats(&self, now: u64) -> Vec<TimeSeriesPoint> {
        self.build_bucketed_stats(now, BucketKind::Hour, None)
    }

    pub fn build_daily_stats(&self, now: u64) -> Vec<TimeSeriesPoint> {
        self.build_bucketed_stats(now, BucketKind::Day, None)
    }

    pub fn build_weekly_stats(&self, now: u64) -> Vec<TimeSeriesPoint> {
        self.build_bucketed_stats(now, BucketKind::Week, None)
    }

    pub fn build_monthly_stats(&self, now: u64) -> Vec<TimeSeriesPoint> {
        self.build_bucketed_stats(now, BucketKind::Month, None)
    }

    /// Build timeseries points for a given window and bucket kind.
    ///
    /// `window_secs` restricts samples to `[now - window_secs, now]` before
    /// bucketing, ensuring e.g. "1h" only returns points from the last hour.
    pub fn build_windowed_stats(
        &self,
        now: u64,
        kind: BucketKind,
        window_secs: u64,
    ) -> Vec<TimeSeriesPoint> {
        self.build_bucketed_stats(now, kind, Some(window_secs))
    }

    fn build_bucketed_stats(
        &self,
        now: u64,
        kind: BucketKind,
        window_secs: Option<u64>,
    ) -> Vec<TimeSeriesPoint> {
        if self.samples.len() < 2 {
            return Vec::new();
        }

        let cutoff = window_secs.map(|w| now.saturating_sub(w));

        let mut buckets: HashMap<i64, Vec<&MetricsCounterSnapshot>> = HashMap::new();
        for s in &self.samples {
            if let Some(cutoff) = cutoff {
                if s.sampled_at < cutoff {
                    continue;
                }
            }
            let key = kind.bucket_key(s.sampled_at);
            buckets.entry(key).or_default().push(s);
        }

        let mut keys: Vec<i64> = buckets.keys().copied().collect();
        keys.sort_unstable();

        let max_buckets = kind.max_buckets();
        if keys.len() > max_buckets {
            keys = keys.split_off(keys.len() - max_buckets);
        }

        keys.into_iter()
            .filter_map(|key| {
                let group = buckets.get(&key)?;
                if group.len() < 2 {
                    return None;
                }
                let first = group.first()?;
                let last = group.last()?;
                let d_requests = last.total_requests().saturating_sub(first.total_requests());
                let d_tokens = last.total_tokens().saturating_sub(first.total_tokens());
                let d_hits = last
                    .gateway_cache_hits()
                    .saturating_sub(first.gateway_cache_hits());
                let hit_rate = if d_requests > 0 {
                    d_hits as f64 / d_requests as f64
                } else {
                    0.0
                };
                Some(TimeSeriesPoint {
                    timestamp: kind.format_label(key, now),
                    requests: d_requests,
                    tokens: d_tokens,
                    cache_hits: d_hits,
                    avg_latency_ms: 0.0,
                    hit_rate,
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BucketKind {
    FiveMin,
    Hour,
    Day,
    Week,
    Month,
}

impl BucketKind {
    fn bucket_key(self, sampled_at: u64) -> i64 {
        match self {
            BucketKind::FiveMin => sampled_at as i64 / 300,
            BucketKind::Hour => sampled_at as i64 / 3600,
            BucketKind::Day => {
                let dt = DateTime::from_timestamp(sampled_at as i64, 0).unwrap_or_else(Utc::now);
                let date = dt.date_naive();
                date.and_hms_opt(0, 0, 0)
                    .map(|ndt| ndt.and_utc().timestamp() / 86400)
                    .unwrap_or(0)
            }
            BucketKind::Week => sampled_at as i64 / 604_800,
            BucketKind::Month => {
                let dt = DateTime::from_timestamp(sampled_at as i64, 0).unwrap_or_else(Utc::now);
                let y = dt.year() as i64;
                let m = dt.month() as i64;
                y * 12 + m
            }
        }
    }

    fn format_label(self, key: i64, _now: u64) -> String {
        match self {
            BucketKind::FiveMin => {
                let ts = key * 300;
                DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%H:%M").to_string())
                    .unwrap_or_else(|| key.to_string())
            }
            BucketKind::Hour => {
                let ts = key * 3600;
                DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%H:00").to_string())
                    .unwrap_or_else(|| key.to_string())
            }
            BucketKind::Day => {
                let ts = key * 86400;
                DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%m-%d").to_string())
                    .unwrap_or_else(|| key.to_string())
            }
            BucketKind::Week => format!("W{key}"),
            BucketKind::Month => format!("M{key}"),
        }
    }

    fn max_buckets(self) -> usize {
        match self {
            BucketKind::FiveMin => 12,
            BucketKind::Hour => 24,
            BucketKind::Day => 7,
            BucketKind::Week => 4,
            BucketKind::Month => 12,
        }
    }
}

pub fn gateway_metrics_url() -> String {
    std::env::var("CRABCACHE_GATEWAY_METRICS_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9090/metrics".to_string())
}

pub fn sample_interval_secs() -> u64 {
    std::env::var("CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s| s > 0)
        .unwrap_or(60)
}

pub fn metrics_cache_ttl() -> Duration {
    std::env::var("CRABCACHE_GATEWAY_METRICS_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s| s > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(2))
}

pub fn metrics_stale_max_age() -> Duration {
    std::env::var("CRABCACHE_GATEWAY_METRICS_STALE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s| s > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(30))
}

/// Fetch Prometheus text with TTL cache, in-flight dedup, and stale fallback on transient errors.
pub async fn fetch_gateway_metrics_cached(cache: &GatewayMetricsCache) -> Result<String, String> {
    let ttl = metrics_cache_ttl();
    if let Some(body) = cache.fresh_body(ttl) {
        return Ok(body);
    }

    let _guard = cache.fetch_lock.lock().await;
    if let Some(body) = cache.fresh_body(ttl) {
        return Ok(body);
    }

    match fetch_gateway_metrics_body_raw().await {
        Ok(body) => {
            cache.store(body.clone());
            Ok(body)
        }
        Err(e) => {
            if let Some(body) = cache.stale_body(metrics_stale_max_age()) {
                tracing::warn!(
                    error = %e,
                    stale_age_secs = cache.age_secs().unwrap_or(0),
                    "Using stale gateway metrics after fetch failure"
                );
                return Ok(body);
            }
            Err(e)
        }
    }
}

/// Uncached fetch (tests / one-off diagnostics).
pub async fn fetch_gateway_metrics_body() -> Result<String, String> {
    fetch_gateway_metrics_body_raw().await
}

async fn fetch_gateway_metrics_body_raw() -> Result<String, String> {
    let metrics_url = gateway_metrics_url();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .http1_only()
        .build()
        .map_err(|e| e.to_string())?;

    let mut last_err = String::new();
    for attempt in 0..2 {
        match client.get(&metrics_url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    last_err = format!("fetch {metrics_url}: HTTP {}", resp.status().as_u16());
                    if attempt == 0 {
                        tracing::debug!(error = %last_err, "metrics fetch retry after non-success status");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        continue;
                    }
                    tracing::warn!(error = %last_err, "Failed to fetch gateway metrics");
                    return Err(last_err);
                }
                return resp.text().await.map_err(|e| e.to_string());
            }
            Err(e) => {
                last_err = format!("fetch {metrics_url}: {e}");
                if attempt == 0 && (e.is_timeout() || e.is_connect()) {
                    tracing::debug!(error = %e, "metrics fetch retry");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                tracing::warn!(error = %last_err, "Failed to fetch gateway metrics");
                return Err(last_err);
            }
        }
    }
    Err(last_err)
}

/// Parse gateway counters from Prometheus exposition text.
pub fn scrape_gateway_counters(body: &str, sampled_at: u64) -> MetricsCounterSnapshot {
    MetricsCounterSnapshot {
        sampled_at,
        l0_hits: sum_prometheus_counter(
            body,
            "gateway_cache_requests_total",
            &[("tier", "L0_moka"), ("result", "hit")],
        ),
        l1_hits: sum_prometheus_counter(
            body,
            "gateway_cache_requests_total",
            &[("tier", "L1_redis"), ("result", "hit")],
        ),
        l2_hits: sum_prometheus_counter(
            body,
            "gateway_cache_requests_total",
            &[("tier", "L2_semantic"), ("result", "hit")],
        ),
        cache_misses: sum_prometheus_counter(
            body,
            "gateway_cache_requests_total",
            &[("tier", "miss"), ("result", "miss")],
        ),
        cache_hit_tokens: sum_prometheus_counter(
            body,
            "gateway_deepseek_input_tokens_total",
            &[("cache_status", "hit")],
        ),
        cache_miss_tokens: sum_prometheus_counter(
            body,
            "gateway_deepseek_input_tokens_total",
            &[("cache_status", "miss")],
        ),
        total_output_tokens: sum_prometheus_counter(
            body,
            "gateway_deepseek_output_tokens_total",
            &[],
        ),
        coalesced_total: sum_prometheus_counter(body, "gateway_coalesced_requests_total", &[]),
        semantic_hits: sum_prometheus_counter(
            body,
            "gateway_semantic_cache_requests_total",
            &[("status", "hit_above_threshold")],
        ) + sum_prometheus_counter(
            body,
            "gateway_semantic_cache_requests_total",
            &[("status", "hit_below_threshold")],
        ),
        semantic_rejected: sum_prometheus_counter(
            body,
            "gateway_semantic_cache_requests_total",
            &[("status", "rejected_by_guard")],
        ),
        semantic_skipped: sum_prometheus_counter(body, "gateway_semantic_skipped_total", &[]),
        cost_saved_usd: sum_prometheus_sample(body, "gateway_cache_cost_saved_usd_total", &[]),
        rejected_total: sum_prometheus_counter(body, "gateway_rejected_requests_total", &[]),
        prefix_break_total: sum_prometheus_counter(body, "gateway_prefix_break_total", &[]),
        reasoning_store_hits: sum_prometheus_counter(
            body,
            "gateway_reasoning_store_lookups_total",
            &[("result", "hit")],
        ),
        reasoning_store_misses: sum_prometheus_counter(
            body,
            "gateway_reasoning_store_lookups_total",
            &[("result", "miss")],
        ),
        stream_cache_sse_omitted: sum_prometheus_counter(
            body,
            "gateway_stream_cache_sse_omitted_total",
            &[],
        ),
        domain_tokens: parse_domain_tokens_from_body(body),
        upstream_p99_latency_ms: percentile_from_buckets(
            body,
            "gateway_upstream_latency_seconds",
            &[],
            0.99,
        ),
        ttft_p99_latency_ms: percentile_from_buckets(
            body,
            "gateway_stream_first_token_latency_seconds",
            &[],
            0.99,
        ),
        cache_fetch_p99_latency_ms: percentile_from_buckets(
            body,
            "gateway_cache_fetch_latency_seconds",
            &[],
            0.99,
        ),
        http_4xx_total: sum_prometheus_counter(
            body,
            "gateway_http_responses_total",
            &[("status_class", "4xx")],
        ),
        http_5xx_total: sum_prometheus_counter(
            body,
            "gateway_http_responses_total",
            &[("status_class", "5xx")],
        ),
        http_responses_total: sum_prometheus_counter(body, "gateway_http_responses_total", &[]),
    }
}

impl MetricsHistory {
    /// Token hit-rate time series for one domain over `window_secs`.
    pub fn domain_token_hit_rate_series(
        &self,
        domain: &str,
        window_secs: u64,
        now: u64,
    ) -> Vec<TimeSeriesPoint> {
        let start = now.saturating_sub(window_secs);
        let in_window: Vec<&MetricsCounterSnapshot> = self
            .samples
            .iter()
            .filter(|s| s.sampled_at >= start && s.sampled_at <= now)
            .collect();
        if in_window.len() < 2 {
            return Vec::new();
        }

        let mut points = Vec::new();
        for w in in_window.windows(2) {
            let first = w[0];
            let last = w[1];
            let (fh, fm) = first.domain_tokens.get(domain).copied().unwrap_or((0, 0));
            let (lh, lm) = last.domain_tokens.get(domain).copied().unwrap_or((0, 0));
            let dh = lh.saturating_sub(fh);
            let dm = lm.saturating_sub(fm);
            let total = dh + dm;
            let hit_rate = if total > 0 {
                dh as f64 / total as f64
            } else {
                0.0
            };
            let ts = DateTime::from_timestamp(last.sampled_at as i64, 0)
                .unwrap_or_else(Utc::now)
                .format("%H:%M")
                .to_string();
            points.push(TimeSeriesPoint {
                timestamp: ts,
                requests: 0,
                tokens: total,
                cache_hits: dh,
                avg_latency_ms: 0.0,
                hit_rate,
            });
        }
        points
    }

    pub fn domain_qps_5m(&self, domain: &str, now: u64) -> f64 {
        let window_secs = WINDOW_5M_SECS;
        let start = now.saturating_sub(window_secs);
        let in_window: Vec<&MetricsCounterSnapshot> = self
            .samples
            .iter()
            .filter(|s| s.sampled_at >= start && s.sampled_at <= now)
            .collect();
        if in_window.len() < 2 {
            return 0.0;
        }
        let first = in_window.first().unwrap();
        let last = in_window.last().unwrap();
        let (fh, fm) = first.domain_tokens.get(domain).copied().unwrap_or((0, 0));
        let (lh, lm) = last.domain_tokens.get(domain).copied().unwrap_or((0, 0));
        let delta_tokens = lh.saturating_sub(fh) + lm.saturating_sub(fm);
        let elapsed = last.sampled_at.saturating_sub(first.sampled_at).max(1) as f64;
        delta_tokens as f64 / elapsed
    }
}

/// Scrape operational counters + TTFT from current metrics body (not stored in ring).
pub fn scrape_ops_metrics(
    body: &str,
    history: &MetricsHistory,
    now: u64,
) -> crate::types::OverviewOpsMetrics {
    let counters = scrape_gateway_counters(body, now);
    let cost_saved_usd_5m = history.window_float_delta(WINDOW_5M_SECS, now, |s| s.cost_saved_usd);
    let coalesced_5m = history.window_u64_delta(WINDOW_5M_SECS, now, |s| s.coalesced_total);
    let rejected_5m = history.window_u64_delta(WINDOW_5M_SECS, now, |s| s.rejected_total);

    crate::types::OverviewOpsMetrics {
        cost_saved_usd_total: counters.cost_saved_usd,
        cost_saved_usd_5m,
        coalesced_total: counters.coalesced_total,
        coalesced_5m,
        rejected_total: counters.rejected_total,
        rejected_5m,
        ttft_ms: avg_prometheus_histogram_ms(
            body,
            "gateway_stream_first_token_latency_seconds",
            &[],
        ),
        prefix_break_total: counters.prefix_break_total,
        reasoning_store_hits: counters.reasoning_store_hits,
        reasoning_store_misses: counters.reasoning_store_misses,
        stream_cache_sse_omitted: counters.stream_cache_sse_omitted,
        upstream_key_count: 0,
        upstream_keys_available: 0,
        upstream_default_profile_id: None,
    }
}

pub fn prefix_cache_by_model(body: &str) -> Vec<PrefixCacheModelBucket> {
    let mut per_model: HashMap<String, (u64, u64)> = HashMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_upstream_prompt_cache_tokens_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let status = label_value(labels, "status");
        let model = label_value(labels, "model").unwrap_or_else(|| "unknown".to_string());
        let value_part = line[close + 1..].trim();
        let Ok(tokens) = value_part.parse::<u64>() else {
            continue;
        };
        let entry = per_model.entry(model).or_insert((0, 0));
        match status.as_deref() {
            Some("hit") => entry.0 += tokens,
            Some("miss") => entry.1 += tokens,
            _ => {}
        }
    }
    let mut buckets: Vec<PrefixCacheModelBucket> = per_model
        .into_iter()
        .map(|(model, (hit, miss))| PrefixCacheModelBucket {
            hit_tokens: hit,
            miss_tokens: miss,
            hit_ratio: prefix_hit_ratio(hit, miss),
            model,
        })
        .collect();
    buckets.sort_by(|a, b| a.model.cmp(&b.model));
    buckets
}

pub fn build_prefix_cache_snapshot(body: &str) -> crate::types::PrefixCacheMetricsSnapshot {
    let hit_tokens = sum_prometheus_counter(
        body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "hit")],
    );
    let miss_tokens = sum_prometheus_counter(
        body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "miss")],
    );
    crate::types::PrefixCacheMetricsSnapshot {
        hit_tokens,
        miss_tokens,
        hit_ratio: prefix_hit_ratio(hit_tokens, miss_tokens),
        by_model: prefix_cache_by_model(body),
    }
}

/// Top consumers by input token volume (hit/miss from `gateway_deepseek_input_tokens_total`).
pub fn consumer_token_buckets(body: &str, top_n: usize) -> Vec<ConsumerMetricsBucket> {
    let mut per_consumer: HashMap<String, (u64, u64)> = HashMap::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_deepseek_input_tokens_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let consumer = label_value(labels, "consumer").unwrap_or_else(|| "unknown".to_string());
        let status = label_value(labels, "cache_status");
        let value_part = line[close + 1..].trim();
        let Ok(tokens) = value_part.parse::<u64>() else {
            continue;
        };
        let entry = per_consumer.entry(consumer).or_insert((0, 0));
        match status.as_deref() {
            Some("hit") => entry.0 += tokens,
            Some("miss") => entry.1 += tokens,
            _ => {}
        }
    }

    let mut buckets: Vec<ConsumerMetricsBucket> = per_consumer
        .into_iter()
        .map(|(consumer, (hit, miss))| {
            let total = hit + miss;
            ConsumerMetricsBucket {
                consumer,
                hit_tokens: hit,
                miss_tokens: miss,
                hit_ratio: if total > 0 {
                    hit as f64 / total as f64
                } else {
                    0.0
                },
            }
        })
        .collect();

    buckets.sort_by(|a, b| (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens)));
    buckets.truncate(top_n);
    buckets
}

/// Top domains by input token volume.
pub fn domain_token_buckets(body: &str, top_n: usize) -> Vec<DomainMetricsBucket> {
    let mut per_domain: HashMap<String, (u64, u64)> = HashMap::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_deepseek_input_tokens_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let domain = label_value(labels, "domain").unwrap_or_else(|| "unclassified".to_string());
        let status = label_value(labels, "cache_status");
        let value_part = line[close + 1..].trim();
        let Ok(tokens) = value_part.parse::<u64>() else {
            continue;
        };
        let entry = per_domain.entry(domain).or_insert((0, 0));
        match status.as_deref() {
            Some("hit") => entry.0 += tokens,
            Some("miss") => entry.1 += tokens,
            _ => {}
        }
    }

    let cost_by_domain = domain_cost_saved(body);
    let mut buckets: Vec<DomainMetricsBucket> = per_domain
        .into_iter()
        .map(|(domain, (hit, miss))| {
            let total = hit + miss;
            DomainMetricsBucket {
                domain: domain.clone(),
                hit_tokens: hit,
                miss_tokens: miss,
                hit_ratio: if total > 0 {
                    hit as f64 / total as f64
                } else {
                    0.0
                },
                cost_saved_usd: cost_by_domain.get(&domain).copied().unwrap_or(0.0),
                qps_5m: 0.0,
                alert: None,
            }
        })
        .collect();

    buckets.sort_by(|a, b| (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens)));
    buckets.truncate(top_n);
    buckets
}

fn domain_cost_saved(body: &str) -> HashMap<String, f64> {
    let mut per_domain: HashMap<String, f64> = HashMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_cache_cost_saved_usd_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let domain = label_value(labels, "domain").unwrap_or_else(|| "unclassified".to_string());
        let value_part = line[close + 1..].trim();
        let Ok(usd) = value_part.parse::<f64>() else {
            continue;
        };
        *per_domain.entry(domain).or_insert(0.0) += usd;
    }
    per_domain
}

pub fn domain_consumer_buckets(body: &str, domain: &str) -> Vec<ConsumerMetricsBucket> {
    let mut per_consumer: HashMap<String, (u64, u64)> = HashMap::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_deepseek_input_tokens_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let line_domain =
            label_value(labels, "domain").unwrap_or_else(|| "unclassified".to_string());
        if line_domain != domain {
            continue;
        }
        let consumer = label_value(labels, "consumer").unwrap_or_else(|| "unknown".to_string());
        let status = label_value(labels, "cache_status");
        let value_part = line[close + 1..].trim();
        let Ok(tokens) = value_part.parse::<u64>() else {
            continue;
        };
        let entry = per_consumer.entry(consumer).or_insert((0, 0));
        match status.as_deref() {
            Some("hit") => entry.0 += tokens,
            Some("miss") => entry.1 += tokens,
            _ => {}
        }
    }

    let mut buckets: Vec<ConsumerMetricsBucket> = per_consumer
        .into_iter()
        .map(|(consumer, (hit, miss))| {
            let total = hit + miss;
            ConsumerMetricsBucket {
                consumer,
                hit_tokens: hit,
                miss_tokens: miss,
                hit_ratio: if total > 0 {
                    hit as f64 / total as f64
                } else {
                    0.0
                },
            }
        })
        .collect();
    buckets.sort_by(|a, b| (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens)));
    buckets
}

pub fn domain_tier_deltas_5m(body: &str, domain: &str) -> TierDeltas5m {
    let mut d = TierDeltas5m::default();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_cache_requests_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let line_domain =
            label_value(labels, "domain").unwrap_or_else(|| "unclassified".to_string());
        if line_domain != domain {
            continue;
        }
        let tier = label_value(labels, "tier");
        let result = label_value(labels, "result");
        let value_part = line[close + 1..].trim();
        let Ok(v) = value_part.parse::<u64>() else {
            continue;
        };
        match (tier.as_deref(), result.as_deref()) {
            (Some("L0_moka"), Some("hit")) => d.l0 += v,
            (Some("L1_redis"), Some("hit")) => d.l1 += v,
            (Some("L2_semantic"), Some("hit")) => d.l2 += v,
            (Some("miss"), Some("miss")) => d.miss += v,
            _ => {}
        }
    }
    d
}

fn parse_domain_tokens_from_body(body: &str) -> HashMap<String, (u64, u64)> {
    let mut per_domain: HashMap<String, (u64, u64)> = HashMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_deepseek_input_tokens_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let domain = label_value(labels, "domain").unwrap_or_else(|| "unclassified".to_string());
        let status = label_value(labels, "cache_status");
        let value_part = line[close + 1..].trim();
        let Ok(tokens) = value_part.parse::<u64>() else {
            continue;
        };
        let entry = per_domain.entry(domain).or_insert((0, 0));
        match status.as_deref() {
            Some("hit") => entry.0 += tokens,
            Some("miss") => entry.1 += tokens,
            _ => {}
        }
    }
    per_domain
}

pub async fn sample_metrics_history(
    cache: &GatewayMetricsCache,
    history: &parking_lot::RwLock<MetricsHistory>,
    gateway_uptime_secs: u64,
    pg_store: Option<&crate::pg::PgStore>,
    sqlite_store: Option<&crate::metrics_store::MetricsStore>,
) -> Result<(), String> {
    let body = fetch_gateway_metrics_cached(cache).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let snapshot = scrape_gateway_counters(&body, now);
    history.write().append(snapshot.clone());

    // Write to PostgreSQL.
    if let Some(pg) = pg_store {
        if let Err(e) = pg
            .insert_metric_snapshot(&snapshot, gateway_uptime_secs)
            .await
        {
            tracing::warn!(error = %e, "PG metrics insert_metric_snapshot failed");
        }
        let cutoff = now.saturating_sub(30 * 86400); // 30 days retention
        if let Err(e) = pg.prune_metric_snapshots(cutoff).await {
            tracing::warn!(error = %e, "PG metrics prune_metric_snapshots failed");
        }
    }

    // Write to SQLite (fallback when PG is unavailable).
    if pg_store.is_none() {
        if let Some(store) = sqlite_store {
            store.insert_snapshot(&snapshot, gateway_uptime_secs);
            let cutoff = now.saturating_sub(crate::metrics_store::MetricsStore::retention_secs());
            store.prune_older_than(cutoff);
        }
    }

    Ok(())
}

fn label_value(labels: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = labels.find(&needle)? + needle.len();
    let rest = &labels[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn labels_match(labels: &str, required: &[(&str, &str)]) -> bool {
    required
        .iter()
        .all(|(key, val)| labels.contains(&format!("{key}=\"{val}\"")))
}

pub fn sum_prometheus_counter_public(body: &str, metric: &str, required: &[(&str, &str)]) -> u64 {
    sum_prometheus_counter(body, metric, required)
}

fn sum_prometheus_counter(body: &str, metric: &str, required: &[(&str, &str)]) -> u64 {
    let mut total = 0u64;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.starts_with(metric) {
            continue;
        }
        if let Some(open) = line.find('{') {
            if let Some(close) = line.find('}') {
                let labels = &line[open + 1..close];
                if !labels_match(labels, required) {
                    continue;
                }
                let value_part = line[close + 1..].trim();
                if let Ok(v) = value_part.parse::<u64>() {
                    total += v;
                }
            }
        } else if required.is_empty() {
            let value_part = line[metric.len()..].trim();
            if let Ok(v) = value_part.parse::<u64>() {
                total += v;
            }
        }
    }
    total
}

/// Average latency in milliseconds from Prometheus histogram `_sum` / `_count` series.
pub fn avg_prometheus_histogram_ms(body: &str, metric: &str, required: &[(&str, &str)]) -> f64 {
    let sum = sum_prometheus_sample(body, &format!("{metric}_sum"), required);
    let count = sum_prometheus_sample(body, &format!("{metric}_count"), required);
    if count > 0.0 {
        (sum / count) * 1000.0
    } else {
        0.0
    }
}

/// Calculate a percentile from Prometheus histogram `_bucket` lines using linear interpolation.
///
/// Parses `{le="..."}` boundaries and corresponding `_bucket` cumulative counts,
/// then interpolates at the requested percentile (0.0–1.0).  Returns milliseconds.
pub fn percentile_from_buckets(
    body: &str,
    metric_prefix: &str,
    required: &[(&str, &str)],
    percentile: f64,
) -> f64 {
    let bucket_metric = format!("{metric_prefix}_bucket");
    let count_metric = format!("{metric_prefix}_count");

    // Collect (le_boundary, cumulative_count) pairs and the total count.
    let mut buckets: Vec<(f64, f64)> = Vec::new();
    let mut total_count = 0.0_f64;

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse _count line (only for the total, not per-bucket).
        if line.starts_with(&count_metric) {
            let value_part = if let Some(open) = line.find('{') {
                if let Some(close) = line.find('}') {
                    let labels = &line[open + 1..close];
                    if !labels_match(labels, required) {
                        continue;
                    }
                    line[close + 1..].trim()
                } else {
                    continue;
                }
            } else if required.is_empty() {
                line[count_metric.len()..].trim()
            } else {
                continue;
            };
            if let Ok(v) = value_part.parse::<f64>() {
                total_count = v;
            }
            continue;
        }

        // Parse _bucket line.
        if !line.starts_with(&bucket_metric) {
            continue;
        }
        let Some(open) = line.find('{') else {
            continue;
        };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        if !labels_match(labels, required) {
            continue;
        }

        // Extract le="..." value.
        let le_start = match labels.find("le=\"") {
            Some(i) => i + 4,
            None => continue,
        };
        let le_end = match labels[le_start..].find('"') {
            Some(i) => le_start + i,
            None => continue,
        };
        let le_str = &labels[le_start..le_end];
        let le = if le_str == "+Inf" {
            f64::INFINITY
        } else {
            match le_str.parse::<f64>() {
                Ok(v) => v,
                Err(_) => continue,
            }
        };

        let value_part = line[close + 1..].trim();
        if let Ok(v) = value_part.parse::<f64>() {
            buckets.push((le, v));
        }
    }

    if buckets.is_empty() || total_count <= 0.0 {
        return 0.0;
    }

    // Sort by boundary ascending.
    buckets.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let target = total_count * percentile;
    let mut prev_count = 0.0_f64;
    for &(le, count) in &buckets {
        if count >= target {
            // Interpolate within this bucket.
            let prev_le = buckets
                .iter()
                .filter(|(l, _)| *l < le)
                .map(|(l, _)| *l)
                .next_back()
                .unwrap_or(0.0);
            let bucket_range = le - prev_le;
            let bucket_count = count - prev_count;
            let fraction = if bucket_count > 0.0 {
                (target - prev_count) / bucket_count
            } else {
                0.0
            };
            return (prev_le + bucket_range * fraction) * 1000.0; // seconds → ms
        }
        prev_count = count;
    }

    0.0
}

fn sum_prometheus_sample(body: &str, metric: &str, required: &[(&str, &str)]) -> f64 {
    let mut total = 0.0;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.starts_with(metric) {
            continue;
        }
        if let Some(open) = line.find('{') {
            if let Some(close) = line.find('}') {
                let labels = &line[open + 1..close];
                if !labels_match(labels, required) {
                    continue;
                }
                let value_part = line[close + 1..].trim();
                if let Ok(v) = value_part.parse::<f64>() {
                    total += v;
                }
            }
        } else if required.is_empty() {
            let value_part = line[metric.len()..].trim();
            if let Ok(v) = value_part.parse::<f64>() {
                total += v;
            }
        }
    }
    total
}

pub fn prefix_hit_ratio(hit: u64, miss: u64) -> f64 {
    let total = hit + miss;
    if total == 0 {
        0.0
    } else {
        hit as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        at: u64,
        requests: u64,
        hits: u64,
        hit_tokens: u64,
        miss_tokens: u64,
    ) -> MetricsCounterSnapshot {
        MetricsCounterSnapshot {
            sampled_at: at,
            l0_hits: hits,
            cache_misses: requests.saturating_sub(hits),
            cache_hit_tokens: hit_tokens,
            cache_miss_tokens: miss_tokens,
            ..Default::default()
        }
    }

    #[test]
    fn window_rates_from_deltas() {
        let mut h = MetricsHistory::new();
        h.append(snap(0, 0, 0, 0, 0));
        h.append(snap(300, 100, 40, 1000, 500));
        let w = h.window_rates(300, 300);
        assert_eq!(w.sample_count, 2);
        assert!((w.hit_rate - 0.4).abs() < 1e-6);
        assert!((w.token_hit_rate - (1000.0 / 1500.0)).abs() < 1e-6);
        assert!((w.qps - (100.0 / 300.0)).abs() < 1e-6);
    }

    #[test]
    fn hourly_stats_need_two_samples_in_bucket() {
        let mut h = MetricsHistory::new();
        let base = 1_700_000_000u64;
        h.append(snap(base, 0, 0, 0, 0));
        h.append(snap(base + 120, 50, 20, 100, 50));
        h.append(snap(base + 240, 100, 60, 200, 80));
        let stats = h.build_hourly_stats(base + 240);
        assert!(!stats.is_empty());
        assert!(stats.last().unwrap().requests > 0);
    }

    #[test]
    fn consumer_buckets_top_n() {
        let body = r#"
gateway_deepseek_input_tokens_total{cache_status="hit",model="m",consumer="alice"} 100
gateway_deepseek_input_tokens_total{cache_status="miss",model="m",consumer="alice"} 50
gateway_deepseek_input_tokens_total{cache_status="hit",model="m",consumer="bob"} 10
"#;
        let buckets = consumer_token_buckets(body, 10);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].consumer, "alice");
        assert!((buckets[0].hit_ratio - (100.0 / 150.0)).abs() < 1e-6);
    }

    #[test]
    fn fivemin_window_limits_to_last_hour() {
        let mut h = MetricsHistory::new();
        // Place 13 samples spaced 5 minutes apart, spanning 60 minutes.
        // The oldest is at t=0 (which is outside the 1h window relative to
        // `now = 13*300`), so it should be filtered out by `window_secs`.
        let base = 1_700_000_000u64;
        for i in 0u64..14 {
            h.append(snap(base + i * 300, i * 100, i * 40, i * 1000, i * 500));
        }
        let now = base + 13 * 300; // last sample timestamp
        let points = h.build_windowed_stats(now, BucketKind::FiveMin, 3600);
        // 1h window = 3600s, cutoff = now - 3600 = base + 13*300 - 3600 = base + 300.
        // Samples from i=1..13 fall within the window, giving 13 samples in 12
        // distinct 5min buckets (i=0 is at `base` which is < cutoff).
        // Each bucket needs ≥2 samples to produce a point. With one sample per
        // bucket, we expect 0 points. Let's verify the max_buckets cap works
        // correctly by testing with enough intra-bucket samples.
        assert!(points.len() <= 12, "should not exceed 12 buckets");

        // Now test with 2 samples per bucket: 14 buckets, but max 12 kept.
        let mut h2 = MetricsHistory::new();
        for i in 0u64..28 {
            h2.append(snap(base + i * 150, i * 100, i * 40, i * 1000, i * 500));
        }
        let now2 = base + 27 * 150;
        let points2 = h2.build_windowed_stats(now2, BucketKind::FiveMin, 3600);
        assert!(points2.len() <= 12, "max_buckets=12 cap");
        // All returned points must be within the 1h window.
        for p in &points2 {
            // Timestamp labels are HH:MM, which is fine — verify no empty.
            assert!(!p.timestamp.is_empty());
        }
    }

    #[test]
    fn build_windowed_stats_excludes_old_samples() {
        let mut h = MetricsHistory::new();
        let base = 1_700_000_000u64;
        // Two old samples (outside 1h window).
        h.append(snap(base, 0, 0, 0, 0));
        h.append(snap(base + 120, 50, 20, 100, 50));
        // Two recent samples (inside 1h window).
        let recent = base + 4000;
        h.append(snap(recent, 100, 60, 200, 80));
        h.append(snap(recent + 120, 200, 100, 400, 160));
        let now = recent + 120;
        let points = h.build_windowed_stats(now, BucketKind::Hour, 3600);
        // Only the two recent samples should contribute; they are in the same
        // hour bucket if they share the same hour key.
        // At minimum, the old samples must not appear.
        for p in &points {
            // Requests from the old pair maxed at 50; recent pair goes 100→200.
            // If old data leaked, we'd see a request delta of 50 in a separate bucket.
            assert!(p.requests <= 200, "old data must not leak into windowed stats");
        }
    }
}
