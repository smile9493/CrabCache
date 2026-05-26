//! Rule-based operational suggestions for the Overview dashboard.

use crate::types::{OverviewBundle, OverviewSuggestion};

pub fn build_overview_suggestions(bundle: &OverviewBundle) -> Vec<OverviewSuggestion> {
    let mut out = Vec::new();
    let m = &bundle.metrics;
    let ops = &bundle.ops;
    let semantic = &bundle.semantic;
    let trace = &bundle.trace_summary;

    if m.metrics_sample_insufficient {
        out.push(OverviewSuggestion {
            severity: "info".into(),
            target: "hit_rate".into(),
            message: "5-minute window has too few requests; wait 1–2 minutes for metrics sampling or increase traffic.".into(),
        });
    }

    if m.hourly_stats.is_empty() {
        out.push(OverviewSuggestion {
            severity: "info".into(),
            target: "timeseries".into(),
            message: "Time series is empty; ensure crab-admin metrics sampler is running (CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS, default 60s).".into(),
        });
    }

    if !m.metrics_sample_insufficient && trace.total_requests > 0 {
        let gw_pct = m.hit_rate_5m * 100.0;
        let trace_pct = trace.cache_hit_ratio * 100.0;
        if trace_pct - gw_pct > 15.0 {
            out.push(OverviewSuggestion {
                severity: "warn".into(),
                target: "hit_rate".into(),
                message: format!(
                    "Gateway 5m hit rate ({gw_pct:.1}%) is much lower than 24h trace ({trace_pct:.1}%); check cache fingerprint version and namespace."
                ),
            });
        }
    }

    if !semantic.enabled {
        let total_tier =
            m.tier_deltas_5m.l0 + m.tier_deltas_5m.l1 + m.tier_deltas_5m.l2 + m.tier_deltas_5m.miss;
        if total_tier > 0 && m.tier_deltas_5m.miss as f64 / total_tier as f64 > 0.5 {
            out.push(OverviewSuggestion {
                severity: "action".into(),
                target: "semantic".into(),
                message: "Semantic cache (L2) is disabled and miss share is high; consider enabling L2 or tuning TTL.".into(),
            });
        }
    }

    if ops.prefix_break_total > 0 || ops.reasoning_store_misses > ops.reasoning_store_hits {
        out.push(OverviewSuggestion {
            severity: "warn".into(),
            target: "ops".into(),
            message: "Prefix breaks or reasoning store misses are elevated; review Reasoning recovery and x-conversation-id affinity (see docs/REASONING_STORE.md).".into(),
        });
    }

    if m.prefix_cache_hit_ratio < 0.3
        && m.prefix_cache_hit_tokens + m.prefix_cache_miss_tokens > 1000
    {
        out.push(OverviewSuggestion {
            severity: "warn".into(),
            target: "l3".into(),
            message: "L3 upstream prefix cache hit ratio is low; ensure stable conversation routing (Ketama + x-conversation-id).".into(),
        });
    }

    for bucket in &m.domain_buckets {
        let Some(alert) = bucket.alert.as_ref() else {
            continue;
        };
        let message = match alert.as_str() {
            "hit_rate_low" => format!(
                "Domain \"{}\" hit rate {:.1}% is below policy minimum.",
                bucket.domain,
                bucket.hit_ratio * 100.0
            ),
            "budget_exceeded" => format!(
                "Domain \"{}\" exceeded monthly budget policy.",
                bucket.domain
            ),
            other => format!("Domain \"{}\": alert {other}.", bucket.domain),
        };
        out.push(OverviewSuggestion {
            severity: "warn".into(),
            target: "hit_rate".into(),
            message,
        });
    }

    if !bundle.health.healthy {
        out.push(OverviewSuggestion {
            severity: "action".into(),
            target: "ops".into(),
            message: format!(
                "Gateway health check failed: {}",
                bundle.health.error.as_deref().unwrap_or("not ready")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        GatewayHealthView, MetricsSnapshot, OverviewOpsMetrics, PrefixCacheMetricsSnapshot,
        SemanticConfig, TierDeltas5m, TraceSummary,
    };

    fn test_metrics(insufficient: bool, hourly_empty: bool) -> MetricsSnapshot {
        MetricsSnapshot {
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
            hourly_stats: if hourly_empty {
                vec![]
            } else {
                vec![crate::types::TimeSeriesPoint {
                    timestamp: "12:00".into(),
                    requests: 100,
                    tokens: 200,
                    cache_hits: 50,
                    avg_latency_ms: 0.0,
                    hit_rate: 0.5,
                }]
            },
            daily_stats: vec![],
            weekly_stats: vec![],
            monthly_stats: vec![],
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
            metrics_sample_insufficient: insufficient,
            history_meta: Default::default(),
            tier_deltas_5m: TierDeltas5m {
                miss: 10,
                ..Default::default()
            },
            latency_upstream_p99_ms: 0.0,
            latency_ttft_p99_ms: 0.0,
            latency_cache_fetch_p99_ms: 0.0,
            error_rate_5m: 0.0,
            http_4xx_5m: 0,
            http_5xx_5m: 0,
            qps_prev_1h: 0.0,
            hit_rate_prev_1h: 0.0,
        }
    }

    #[test]
    fn insufficient_sample_emits_info() {
        let bundle = OverviewBundle {
            metrics: test_metrics(true, true),
            health: GatewayHealthView {
                healthy: true,
                uptime_secs: 0,
                active_keys: 0,
                backend_count: 0,
                stream_cache_enabled: false,
                upstream_key_count: 0,
                upstream_keys_available: 0,
                error: None,
                redis_connected: false,
                qdrant_connected: false,
                backends_healthy: 0,
                backends_total: 0,
                circuit_open_count: 0,
            },
            prefix_cache: PrefixCacheMetricsSnapshot {
                hit_tokens: 0,
                miss_tokens: 0,
                hit_ratio: 0.0,
                by_model: vec![],
            },
            semantic: SemanticConfig {
                enabled: false,
                similarity_threshold: 0.95,
            },
            trace_summary: TraceSummary {
                hours: 24,
                total_requests: 0,
                cache_hit_ratio: 0.0,
            },
            ops: OverviewOpsMetrics::default(),
            suggestions: vec![],
        };
        let s = build_overview_suggestions(&bundle);
        assert!(
            s.iter()
                .any(|x| x.target == "hit_rate" && x.severity == "info")
        );
        assert!(s.iter().any(|x| x.target == "timeseries"));
    }

    #[test]
    fn trace_hit_rate_divergence_emits_warn() {
        let mut m = test_metrics(false, false);
        m.hit_rate_5m = 0.40; // 40% gateway 5m hit rate
        let bundle = OverviewBundle {
            metrics: m,
            health: GatewayHealthView {
                healthy: true,
                ..Default::default()
            },
            prefix_cache: PrefixCacheMetricsSnapshot {
                hit_tokens: 0,
                miss_tokens: 0,
                hit_ratio: 0.0,
                by_model: vec![],
            },
            semantic: SemanticConfig {
                enabled: false,
                similarity_threshold: 0.95,
            },
            trace_summary: TraceSummary {
                hours: 24,
                total_requests: 1000,
                cache_hit_ratio: 0.90, // 90% trace hit rate → 50% gap > 15%
            },
            ops: OverviewOpsMetrics::default(),
            suggestions: vec![],
        };
        let s = build_overview_suggestions(&bundle);
        assert!(
            s.iter()
                .any(|x| x.target == "hit_rate" && x.severity == "warn"),
            "should emit warn when trace hit rate far exceeds gateway 5m hit rate"
        );
    }
}
