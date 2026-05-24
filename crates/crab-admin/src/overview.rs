//! Aggregated overview payload for the dashboard (single poll endpoint).

use crate::metrics_history::{
    self, avg_prometheus_histogram_ms, build_prefix_cache_snapshot, consumer_token_buckets,
    domain_token_buckets, scrape_gateway_counters, scrape_ops_metrics, WINDOW_5M_SECS,
};
use crate::state::{AppState, GatewayProbe};
use crate::suggestions::build_overview_suggestions;
use crate::trace_log;
use crate::trace_summary;
use crate::types::{
    GatewayHealthView, MetricsHistoryMeta, MetricsSnapshot, MetricsSnapshotCore, OverviewBundle,
    OverviewCore, OverviewTimeseriesResponse, SemanticConfig, TimeSeriesPoint, TraceSummary,
};
use crab_control::GatewayStatus;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TRACE_SUMMARY_TTL: Duration = Duration::from_secs(60);
const GATEWAY_PROBE_TTL: Duration = Duration::from_secs(3);
const DEFAULT_OVERVIEW_CORE_CACHE_TTL: Duration = Duration::from_secs(10);

pub fn overview_core_cache_ttl() -> Duration {
    std::env::var("CRABCACHE_OVERVIEW_CORE_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s| s > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_OVERVIEW_CORE_CACHE_TTL)
}

/// Background refresh interval (defaults to cache TTL, aligned with dashboard 10s core poll).
pub fn overview_core_background_interval() -> Duration {
    std::env::var("CRABCACHE_OVERVIEW_CORE_REFRESH_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s| s > 0)
        .map(Duration::from_secs)
        .unwrap_or(overview_core_cache_ttl())
}

fn read_fresh_overview_core_cache(
    state: &AppState,
    ttl: Duration,
) -> Option<(OverviewCore, String)> {
    let guard = state.overview_core_cache.read();
    let (at, core, etag) = guard.as_ref()?;
    if at.elapsed() < ttl {
        Some((core.clone(), etag.clone()))
    } else {
        None
    }
}

fn store_overview_core_cache(state: &AppState, core: OverviewCore, etag: String) {
    *state.overview_core_cache.write() = Some((Instant::now(), core, etag));
}

/// Stable ETag for serialized overview core (matches `get_overview_core` hashing).
pub fn overview_core_etag(core: &OverviewCore) -> Result<String, String> {
    let json_bytes =
        serde_json::to_vec(core).map_err(|e| format!("overview core serialize: {e}"))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json_bytes.hash(&mut hasher);
    Ok(format!("\"{:x}\"", hasher.finish()))
}

/// Serve overview core from TTL cache when fresh; rebuild with in-flight dedup on miss.
pub async fn get_overview_core_cached(
    state: &Arc<AppState>,
) -> Result<(OverviewCore, String), String> {
    let ttl = overview_core_cache_ttl();
    if let Some(pair) = read_fresh_overview_core_cache(state, ttl) {
        return Ok(pair);
    }

    let _guard = state.overview_core_build_lock.lock().await;
    if let Some(pair) = read_fresh_overview_core_cache(state, ttl) {
        return Ok(pair);
    }

    let core = build_overview_core(state).await?;
    let etag = overview_core_etag(&core)?;
    store_overview_core_cache(state, core.clone(), etag.clone());
    Ok((core, etag))
}

/// Pre-warm overview core cache (background sampler and startup).
pub async fn refresh_overview_core_cache(state: &Arc<AppState>) -> Result<(), String> {
    let _guard = state.overview_core_build_lock.lock().await;
    let core = build_overview_core(state).await?;
    let etag = overview_core_etag(&core)?;
    store_overview_core_cache(state, core, etag);
    Ok(())
}

pub fn gateway_probe_ttl() -> Duration {
    std::env::var("CRABCACHE_GATEWAY_PROBE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&s| s > 0)
        .map(Duration::from_secs)
        .unwrap_or(GATEWAY_PROBE_TTL)
}

/// Fetch `/v1/ready` and `/v1/status` at most once per TTL window.
pub async fn fetch_gateway_probe_cached(state: &Arc<AppState>) -> GatewayProbe {
    {
        let cache = state.gateway_probe_cache.read();
        if let Some((at, probe)) = cache.as_ref() {
            if at.elapsed() < gateway_probe_ttl() {
                return probe.clone();
            }
        }
    }

    let ready = state.gateway.ready().await;
    let ready_ok = ready.is_ok();
    let ready_error = ready.err().map(|e| e.to_string());

    let status_result = state.gateway.status().await;
    let (status, status_error) = match status_result {
        Ok(s) => (Some(s), None),
        Err(e) => (None, Some(e.to_string())),
    };

    let probe = GatewayProbe {
        ready_ok,
        ready_error,
        status,
        status_error,
    };
    *state.gateway_probe_cache.write() = Some((Instant::now(), probe.clone()));
    probe
}

async fn refresh_metrics_history_sample(state: &Arc<AppState>, body: &str, now: u64) {
    let counters = scrape_gateway_counters(body, now);
    let mut history = state.metrics_history.write();
    if history.sample_count() == 0 {
        history.append(counters);
    }
}

pub async fn build_overview_core(state: &Arc<AppState>) -> Result<OverviewCore, String> {
    let body = state.fetch_gateway_metrics().await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    refresh_metrics_history_sample(state, &body, now).await;

    let probe = fetch_gateway_probe_cached(state).await;
    let metrics = build_metrics_snapshot_core(&body, state, now, probe.status.as_ref()).await?;
    let health = build_gateway_health_from_probe(&probe);
    let prefix_cache = build_prefix_cache_snapshot(&body);

    let semantic_cfg = state.semantic_config.read().clone();
    let semantic = SemanticConfig {
        enabled: semantic_cfg.enabled,
        similarity_threshold: semantic_cfg.similarity_threshold as f64,
    };

    let mut ops = scrape_ops_metrics(&body, &state.metrics_history.read(), now);
    if let Some(ref status) = probe.status {
        ops.upstream_key_count = status.upstream_key_count as u32;
        ops.upstream_keys_available = status.upstream_keys_available as u32;
    }

    let bundle_for_suggestions = OverviewBundle {
        metrics: metrics_snapshot_from_core(&metrics, &[]),
        health: health.clone(),
        prefix_cache: prefix_cache.clone(),
        semantic: semantic.clone(),
        trace_summary: TraceSummary {
            hours: 24,
            total_requests: 0,
            cache_hit_ratio: 0.0,
        },
        ops: ops.clone(),
        suggestions: vec![],
    };
    let suggestions = build_overview_suggestions(&bundle_for_suggestions);

    Ok(OverviewCore {
        metrics,
        health,
        prefix_cache,
        semantic,
        ops,
        suggestions,
    })
}

pub async fn build_overview_timeseries(
    state: &Arc<AppState>,
    window: &str,
) -> Result<OverviewTimeseriesResponse, String> {
    let body = state.fetch_gateway_metrics().await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    refresh_metrics_history_sample(state, &body, now).await;

    let points = {
        let history = state.metrics_history.read();
        match window {
            "7d" => history.build_daily_stats(now),
            "24h" => history.build_hourly_stats(now),
            _ => history.build_hourly_stats(now),
        }
    };

    Ok(OverviewTimeseriesResponse {
        window: window.to_string(),
        points,
    })
}

pub async fn build_overview_trace(state: &Arc<AppState>) -> TraceSummary {
    cached_trace_summary(state, 24).await
}

pub async fn build_overview(state: &Arc<AppState>) -> Result<OverviewBundle, String> {
    let body = state.fetch_gateway_metrics().await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    refresh_metrics_history_sample(state, &body, now).await;

    let probe = fetch_gateway_probe_cached(state).await;
    let metrics = build_metrics_snapshot(&body, state, now, probe.status.as_ref()).await?;
    let health = build_gateway_health_from_probe(&probe);
    let prefix_cache = build_prefix_cache_snapshot(&body);

    let semantic_cfg = state.semantic_config.read().clone();
    let semantic = SemanticConfig {
        enabled: semantic_cfg.enabled,
        similarity_threshold: semantic_cfg.similarity_threshold as f64,
    };

    let mut ops = scrape_ops_metrics(&body, &state.metrics_history.read(), now);
    if let Some(ref status) = probe.status {
        ops.upstream_key_count = status.upstream_key_count as u32;
        ops.upstream_keys_available = status.upstream_keys_available as u32;
    }

    let trace_summary = cached_trace_summary(state, 24).await;

    let mut bundle = OverviewBundle {
        metrics,
        health,
        prefix_cache,
        semantic,
        trace_summary,
        ops,
        suggestions: vec![],
    };
    bundle.suggestions = build_overview_suggestions(&bundle);
    Ok(bundle)
}

fn metrics_snapshot_from_core(
    core: &MetricsSnapshotCore,
    points: &[TimeSeriesPoint],
) -> MetricsSnapshot {
    MetricsSnapshot {
        qps: core.qps,
        tps: core.tps,
        l0_hits: core.l0_hits,
        l1_hits: core.l1_hits,
        l2_hits: core.l2_hits,
        cache_misses: core.cache_misses,
        cache_hit_tokens: core.cache_hit_tokens,
        cache_miss_tokens: core.cache_miss_tokens,
        total_input_tokens: core.total_input_tokens,
        total_output_tokens: core.total_output_tokens,
        total_tokens: core.total_tokens,
        latency_l0_ms: core.latency_l0_ms,
        latency_l1_ms: core.latency_l1_ms,
        latency_l2_ms: core.latency_l2_ms,
        latency_upstream_ms: core.latency_upstream_ms,
        active_keys: core.active_keys,
        uptime_hours: core.uptime_hours,
        uptime_secs: core.uptime_secs,
        hourly_stats: points.to_vec(),
        daily_stats: vec![],
        weekly_stats: vec![],
        monthly_stats: vec![],
        semantic_hits: core.semantic_hits,
        semantic_rejected: core.semantic_rejected,
        semantic_skipped: core.semantic_skipped,
        prefix_cache_hit_tokens: core.prefix_cache_hit_tokens,
        prefix_cache_miss_tokens: core.prefix_cache_miss_tokens,
        prefix_cache_hit_ratio: core.prefix_cache_hit_ratio,
        hit_rate_cumulative: core.hit_rate_cumulative,
        hit_rate_5m: core.hit_rate_5m,
        token_hit_rate_5m: core.token_hit_rate_5m,
        qps_5m: core.qps_5m,
        coalesced_total: core.coalesced_total,
        consumer_buckets: core.consumer_buckets.clone(),
        domain_buckets: core.domain_buckets.clone(),
        metrics_sample_insufficient: core.metrics_sample_insufficient,
        history_meta: core.history_meta.clone(),
        tier_deltas_5m: core.tier_deltas_5m,
    }
}

pub async fn build_metrics_snapshot_core(
    body: &str,
    state: &Arc<AppState>,
    now: u64,
    gateway_status: Option<&GatewayStatus>,
) -> Result<MetricsSnapshotCore, String> {
    let counters = scrape_gateway_counters(body, now);

    let prefix_cache_hit_tokens = metrics_history::sum_prometheus_counter_public(
        body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "hit")],
    );
    let prefix_cache_miss_tokens = metrics_history::sum_prometheus_counter_public(
        body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "miss")],
    );
    let prefix_cache_hit_ratio =
        metrics_history::prefix_hit_ratio(prefix_cache_hit_tokens, prefix_cache_miss_tokens);

    let mut uptime_secs = now.saturating_sub(state.start_time);
    let mut active_keys = state.keys_meta.len() as u64;
    if let Some(status) = gateway_status {
        if status.uptime_secs > 0 {
            uptime_secs = status.uptime_secs;
        }
        active_keys = status.active_keys;
    }

    let total_requests = counters.total_requests();
    let total_hits = counters.gateway_cache_hits();
    let hit_rate_cumulative = if total_requests > 0 {
        total_hits as f64 / total_requests as f64
    } else {
        0.0
    };

    let qps = if uptime_secs > 0 {
        total_requests as f64 / uptime_secs as f64
    } else {
        0.0
    };
    let tps = if uptime_secs > 0 {
        counters.total_tokens() as f64 / uptime_secs as f64
    } else {
        0.0
    };

    let history = state.metrics_history.read();
    let window = history.window_rates_5m(now);
    let d_requests_window = history.window_request_delta(window.window_secs, now);
    let metrics_sample_insufficient = window.sample_count < 2 || d_requests_window < 5;
    let tier_deltas_5m = history.window_tier_deltas(WINDOW_5M_SECS, now);
    let history_meta = MetricsHistoryMeta {
        sample_count: history.sample_count(),
        oldest_sample_at_secs: history.oldest_sample_at(),
        sampling_interval_secs: metrics_history::sample_interval_secs(),
        gateway_counter_reset: {
            // Gateway restart detection: if the gateway uptime is shorter than
            // the oldest persisted sample, the Prometheus counters have been reset.
            let g_uptime = gateway_status.map(|s| s.uptime_secs).unwrap_or(0);
            let oldest_ts = history.oldest_sample_at();
            if g_uptime > 0 && oldest_ts > 0 && oldest_ts > now.saturating_sub(g_uptime) {
                true
            } else {
                false
            }
        },
    };

    let consumer_buckets = consumer_token_buckets(body, 10);
    let mut domain_buckets = domain_token_buckets(body, 20);
    for bucket in &mut domain_buckets {
        bucket.qps_5m = history.domain_qps_5m(&bucket.domain, now);
        for policy in state.domain_policies.read().iter() {
            if policy.domain != bucket.domain || !policy.enabled {
                continue;
            }
            if policy.min_hit_rate > 0.0 && bucket.hit_ratio < policy.min_hit_rate {
                bucket.alert = Some("hit_rate_low".to_string());
            }
            let total_tokens = bucket.hit_tokens + bucket.miss_tokens;
            if policy.monthly_token_budget > 0 && total_tokens >= policy.monthly_token_budget {
                bucket.alert = Some("budget_exceeded".to_string());
            }
            if policy.monthly_cost_budget_usd > 0.0
                && bucket.cost_saved_usd >= policy.monthly_cost_budget_usd
            {
                bucket.alert = Some("budget_exceeded".to_string());
            }
        }
    }
    drop(history);

    Ok(MetricsSnapshotCore {
        qps,
        tps,
        l0_hits: counters.l0_hits,
        l1_hits: counters.l1_hits,
        l2_hits: counters.l2_hits,
        cache_misses: counters.cache_misses,
        cache_hit_tokens: counters.cache_hit_tokens,
        cache_miss_tokens: counters.cache_miss_tokens,
        total_input_tokens: counters.cache_hit_tokens + counters.cache_miss_tokens,
        total_output_tokens: counters.total_output_tokens,
        total_tokens: counters.cache_hit_tokens
            + counters.cache_miss_tokens
            + counters.total_output_tokens,
        latency_l0_ms: avg_prometheus_histogram_ms(
            body,
            "gateway_cache_fetch_latency_seconds",
            &[("tier", "L0_moka")],
        ),
        latency_l1_ms: avg_prometheus_histogram_ms(
            body,
            "gateway_cache_fetch_latency_seconds",
            &[("tier", "L1_redis")],
        ),
        latency_l2_ms: avg_prometheus_histogram_ms(
            body,
            "gateway_cache_fetch_latency_seconds",
            &[("tier", "L2_semantic")],
        ),
        latency_upstream_ms: avg_prometheus_histogram_ms(
            body,
            "gateway_upstream_latency_seconds",
            &[],
        ),
        active_keys,
        uptime_hours: uptime_secs / 3600,
        uptime_secs,
        semantic_hits: counters.semantic_hits,
        semantic_rejected: counters.semantic_rejected,
        semantic_skipped: counters.semantic_skipped,
        prefix_cache_hit_tokens,
        prefix_cache_miss_tokens,
        prefix_cache_hit_ratio,
        hit_rate_cumulative,
        hit_rate_5m: window.hit_rate,
        token_hit_rate_5m: window.token_hit_rate,
        qps_5m: window.qps,
        coalesced_total: counters.coalesced_total,
        consumer_buckets,
        domain_buckets,
        metrics_sample_insufficient,
        history_meta,
        tier_deltas_5m,
    })
}

pub async fn build_metrics_snapshot(
    body: &str,
    state: &Arc<AppState>,
    now: u64,
    gateway_status: Option<&GatewayStatus>,
) -> Result<MetricsSnapshot, String> {
    let core = build_metrics_snapshot_core(body, state, now, gateway_status).await?;
    let history = state.metrics_history.read();
    let hourly_stats = history.build_hourly_stats(now);
    let daily_stats = history.build_daily_stats(now);
    let weekly_stats = history.build_weekly_stats(now);
    let monthly_stats = history.build_monthly_stats(now);
    drop(history);

    Ok(MetricsSnapshot {
        qps: core.qps,
        tps: core.tps,
        l0_hits: core.l0_hits,
        l1_hits: core.l1_hits,
        l2_hits: core.l2_hits,
        cache_misses: core.cache_misses,
        cache_hit_tokens: core.cache_hit_tokens,
        cache_miss_tokens: core.cache_miss_tokens,
        total_input_tokens: core.total_input_tokens,
        total_output_tokens: core.total_output_tokens,
        total_tokens: core.total_tokens,
        latency_l0_ms: core.latency_l0_ms,
        latency_l1_ms: core.latency_l1_ms,
        latency_l2_ms: core.latency_l2_ms,
        latency_upstream_ms: core.latency_upstream_ms,
        active_keys: core.active_keys,
        uptime_hours: core.uptime_hours,
        uptime_secs: core.uptime_secs,
        hourly_stats,
        daily_stats,
        weekly_stats,
        monthly_stats,
        semantic_hits: core.semantic_hits,
        semantic_rejected: core.semantic_rejected,
        semantic_skipped: core.semantic_skipped,
        prefix_cache_hit_tokens: core.prefix_cache_hit_tokens,
        prefix_cache_miss_tokens: core.prefix_cache_miss_tokens,
        prefix_cache_hit_ratio: core.prefix_cache_hit_ratio,
        hit_rate_cumulative: core.hit_rate_cumulative,
        hit_rate_5m: core.hit_rate_5m,
        token_hit_rate_5m: core.token_hit_rate_5m,
        qps_5m: core.qps_5m,
        coalesced_total: core.coalesced_total,
        consumer_buckets: core.consumer_buckets,
        domain_buckets: core.domain_buckets,
        metrics_sample_insufficient: core.metrics_sample_insufficient,
        history_meta: core.history_meta,
        tier_deltas_5m: core.tier_deltas_5m,
    })
}

pub async fn build_gateway_health(state: &Arc<AppState>) -> GatewayHealthView {
    build_gateway_health_from_probe(&fetch_gateway_probe_cached(state).await)
}

fn build_gateway_health_from_probe(probe: &GatewayProbe) -> GatewayHealthView {
    if !probe.ready_ok {
        return GatewayHealthView {
            healthy: false,
            uptime_secs: 0,
            active_keys: 0,
            backend_count: 0,
            stream_cache_enabled: false,
            upstream_key_count: 0,
            upstream_keys_available: 0,
            error: probe.ready_error.clone(),
        };
    }
    match &probe.status {
        Some(s) => gateway_health_from_status(s.clone(), probe.status_error.clone()),
        None => gateway_health_from_status(
            empty_gateway_status(),
            probe.status_error.clone(),
        ),
    }
}

fn gateway_health_from_status(s: GatewayStatus, error: Option<String>) -> GatewayHealthView {
    GatewayHealthView {
        healthy: true,
        uptime_secs: s.uptime_secs,
        active_keys: s.active_keys,
        backend_count: s.backend_count,
        stream_cache_enabled: s.stream_cache_enabled,
        upstream_key_count: s.upstream_key_count as u32,
        upstream_keys_available: s.upstream_keys_available as u32,
        error,
    }
}

fn empty_gateway_status() -> GatewayStatus {
    GatewayStatus {
        uptime_secs: 0,
        active_keys: 0,
        backend_count: 0,
        stream_cache_enabled: false,
        upstream_key_count: 0,
        upstream_keys_available: 0,
        upstream_base_url: None,
        upstream_model: None,
    }
}

async fn cached_trace_summary(state: &Arc<AppState>, hours: u32) -> TraceSummary {
    {
        let cache = state.trace_summary_cache.read();
        if let Some((at, summary)) = cache.as_ref() {
            if at.elapsed() < TRACE_SUMMARY_TTL && summary.hours == hours {
                return summary.clone();
            }
        }
    }

    let path = trace_log::trace_log_path();
    let entries = trace_log::load_trace_entries_async(&path, hours).await;
    let summary = trace_summary::compute_trace_summary(&entries, hours);
    *state.trace_summary_cache.write() = Some((Instant::now(), summary.clone()));
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics_history::build_prefix_cache_snapshot;
    use crate::state::GatewayProbe;
    use crate::types::{
        MetricsHistoryMeta, MetricsSnapshotCore, OverviewCore, OverviewOpsMetrics, SemanticConfig,
        TimeSeriesPoint, TierDeltas5m,
    };

    fn empty_core() -> MetricsSnapshotCore {
        MetricsSnapshotCore {
            qps: 0.0,
            tps: 0.0,
            l0_hits: 0,
            l1_hits: 0,
            l2_hits: 0,
            cache_misses: 0,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            latency_l0_ms: 0.0,
            latency_l1_ms: 0.0,
            latency_l2_ms: 0.0,
            latency_upstream_ms: 0.0,
            active_keys: 0,
            uptime_hours: 0,
            uptime_secs: 0,
            semantic_hits: 0,
            semantic_rejected: 0,
            semantic_skipped: 0,
            prefix_cache_hit_tokens: 0,
            prefix_cache_miss_tokens: 0,
            prefix_cache_hit_ratio: 0.0,
            hit_rate_cumulative: 0.0,
            hit_rate_5m: 0.0,
            token_hit_rate_5m: 0.0,
            qps_5m: 0.0,
            coalesced_total: 0,
            consumer_buckets: vec![],
            domain_buckets: vec![],
            metrics_sample_insufficient: false,
            history_meta: MetricsHistoryMeta::default(),
            tier_deltas_5m: TierDeltas5m::default(),
        }
    }

    #[test]
    fn overview_core_etag_is_stable_for_same_payload() {
        let core = OverviewCore {
            metrics: empty_core(),
            health: build_gateway_health_from_probe(&GatewayProbe {
                ready_ok: true,
                ready_error: None,
                status: None,
                status_error: None,
            }),
            prefix_cache: build_prefix_cache_snapshot(""),
            semantic: SemanticConfig {
                enabled: false,
                similarity_threshold: 0.95,
            },
            ops: OverviewOpsMetrics::default(),
            suggestions: vec![],
        };
        let a = overview_core_etag(&core).expect("etag");
        let b = overview_core_etag(&core).expect("etag");
        assert_eq!(a, b);
        assert!(a.starts_with('"') && a.ends_with('"'));
    }

    #[test]
    fn metrics_snapshot_from_core_maps_points_to_hourly() {
        let core = empty_core();
        let points = vec![TimeSeriesPoint {
            timestamp: "12:00".into(),
            requests: 1,
            tokens: 2,
            cache_hits: 0,
            avg_latency_ms: 0.0,
            hit_rate: 0.0,
        }];
        let snap = metrics_snapshot_from_core(&core, &points);
        assert_eq!(snap.hourly_stats.len(), 1);
        assert_eq!(snap.hourly_stats[0].tokens, 2);
        assert!(snap.daily_stats.is_empty());
    }
}
