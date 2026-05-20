//! Aggregated overview payload for the dashboard (single poll endpoint).

use crate::metrics_history::{
    self, avg_prometheus_histogram_ms, build_prefix_cache_snapshot, consumer_token_buckets,
    scrape_gateway_counters, scrape_ops_metrics, WINDOW_5M_SECS,
};
use crate::state::AppState;
use crate::trace_log;
use crate::trace_summary;
use crate::types::{
    GatewayHealthView, MetricsHistoryMeta, MetricsSnapshot, OverviewBundle, SemanticConfig,
    TraceSummary,
};
use crab_control::GatewayStatus;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TRACE_SUMMARY_TTL: Duration = Duration::from_secs(60);

pub async fn build_overview(state: &Arc<AppState>) -> Result<OverviewBundle, String> {
    let body = metrics_history::fetch_gateway_metrics_body().await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let counters = scrape_gateway_counters(&body, now);
    {
        let mut history = state.metrics_history.write();
        if history.sample_count() == 0 {
            history.append(counters);
        }
    }

    let metrics = build_metrics_snapshot(&body, state, now).await?;
    let health = build_gateway_health(state).await;
    let prefix_cache = build_prefix_cache_snapshot(&body);

    let semantic_cfg = state.semantic_config.read().clone();
    let semantic = SemanticConfig {
        enabled: semantic_cfg.enabled,
        similarity_threshold: semantic_cfg.similarity_threshold as f64,
    };

    let mut ops = scrape_ops_metrics(&body, &state.metrics_history.read(), now);
    if let Ok(status) = state.gateway.status().await {
        ops.upstream_key_count = status.upstream_key_count as u32;
        ops.upstream_keys_available = status.upstream_keys_available as u32;
    }

    let trace_summary = cached_trace_summary(state, 24);

    Ok(OverviewBundle {
        metrics,
        health,
        prefix_cache,
        semantic,
        trace_summary,
        ops,
    })
}

pub async fn build_metrics_snapshot(
    body: &str,
    state: &Arc<AppState>,
    now: u64,
) -> Result<MetricsSnapshot, String> {
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
    if let Ok(status) = state.gateway.status().await {
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
    };

    let hourly_stats = history.build_hourly_stats(now);
    let daily_stats = history.build_daily_stats(now);
    let weekly_stats = history.build_weekly_stats(now);
    let monthly_stats = history.build_monthly_stats(now);
    drop(history);

    let consumer_buckets = consumer_token_buckets(body, 10);

    Ok(MetricsSnapshot {
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
        hourly_stats,
        daily_stats,
        weekly_stats,
        monthly_stats,
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
        metrics_sample_insufficient,
        history_meta,
        tier_deltas_5m,
    })
}

pub async fn build_gateway_health(state: &Arc<AppState>) -> GatewayHealthView {
    match state.gateway.ready().await {
        Ok(()) => match state.gateway.status().await {
            Ok(s) => gateway_health_from_status(s, None),
            Err(e) => gateway_health_from_status(empty_gateway_status(), Some(e.to_string())),
        },
        Err(e) => GatewayHealthView {
            healthy: false,
            uptime_secs: 0,
            active_keys: 0,
            backend_count: 0,
            stream_cache_enabled: false,
            upstream_key_count: 0,
            upstream_keys_available: 0,
            error: Some(e.to_string()),
        },
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

fn cached_trace_summary(state: &Arc<AppState>, hours: u32) -> TraceSummary {
    {
        let cache = state.trace_summary_cache.read();
        if let Some((at, summary)) = cache.as_ref() {
            if at.elapsed() < TRACE_SUMMARY_TTL && summary.hours == hours {
                return summary.clone();
            }
        }
    }

    let path = trace_log::trace_log_path();
    let entries = trace_log::filter_trace_by_hours(trace_log::load_trace_entries(&path), hours);
    let summary = trace_summary::compute_trace_summary(&entries, hours);
    *state.trace_summary_cache.write() = Some((Instant::now(), summary.clone()));
    summary
}
