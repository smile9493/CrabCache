use crate::context::{GatewayContext, PricingConfig};
use crate::runtime::RuntimeConfig;
use crate::sse::UsageData;
use crab_metrics::global_metrics;
use std::time::Instant;

/// Invalidate affinity hint after this many consecutive pure prompt-cache misses.
pub const AFFINITY_MISS_STREAK_THRESHOLD: u32 = 3;

pub fn record_usage_metrics(
    usage: &UsageData,
    model: &str,
    consumer: Option<&str>,
    domain: Option<&str>,
    upstream_key_id: Option<&str>,
    affinity_kind: Option<&str>,
    runtime: &RuntimeConfig,
    pricing: &PricingConfig,
) {
    global_metrics().record_upstream_usage(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.prompt_cache_hit_tokens,
        usage.prompt_cache_miss_tokens,
        model,
        consumer,
        domain,
    );

    if let Some(kid) = upstream_key_id {
        global_metrics().record_upstream_key_usage(
            kid,
            model,
            usage.prompt_tokens,
            usage.completion_tokens,
        );
    }

    if usage.prompt_cache_hit_tokens > 0 {
        global_metrics().record_upstream_prompt_cache(
            "hit",
            usage.prompt_cache_hit_tokens,
            model,
            consumer,
            domain,
        );
        global_metrics().record_session_prompt_cache(
            affinity_kind.unwrap_or("none"),
            "hit",
            usage.prompt_cache_hit_tokens,
            model,
        );
    }
    if usage.prompt_cache_miss_tokens > 0 {
        global_metrics().record_upstream_prompt_cache(
            "miss",
            usage.prompt_cache_miss_tokens,
            model,
            consumer,
            domain,
        );
        global_metrics().record_session_prompt_cache(
            affinity_kind.unwrap_or("none"),
            "miss",
            usage.prompt_cache_miss_tokens,
            model,
        );
    }

    let total_tokens = usage.prompt_tokens.saturating_add(usage.completion_tokens);
    let spend = pricing.cost_saved_usd(model, usage.prompt_tokens, usage.completion_tokens);
    runtime.record_domain_usage(domain, total_tokens, spend);
}

/// Accumulate upstream prompt-cache usage for end-of-request affinity hint updates.
pub fn accumulate_affinity_prompt_cache_usage(
    ctx: &mut GatewayContext,
    prompt_cache_hit_tokens: u64,
    prompt_cache_miss_tokens: u64,
) {
    if prompt_cache_hit_tokens > 0 {
        ctx.affinity_pure_miss_streak = 0;
    } else if prompt_cache_miss_tokens > 0 {
        ctx.affinity_pure_miss_streak = ctx.affinity_pure_miss_streak.saturating_add(1);
    }
    ctx.affinity_prompt_cache_hits = ctx
        .affinity_prompt_cache_hits
        .saturating_add(prompt_cache_hit_tokens);
    ctx.affinity_prompt_cache_misses = ctx
        .affinity_prompt_cache_misses
        .saturating_add(prompt_cache_miss_tokens);
}

/// Apply affinity hint once per request (avoids per-chunk invalidation on transient misses).
pub fn finalize_affinity_backend_hint(
    hints: &moka::sync::Cache<String, String>,
    ctx: &GatewayContext,
) {
    let Some(key) = ctx
        .upstream
        .affinity_key
        .as_deref()
        .filter(|k| !k.is_empty())
    else {
        return;
    };
    if ctx.affinity_prompt_cache_hits > 0 {
        if let Some(backend) = ctx
            .upstream
            .backend_name
            .as_deref()
            .filter(|b| !b.is_empty())
        {
            hints.insert(key.to_string(), backend.to_string());
        }
    } else if ctx.affinity_prompt_cache_misses > 0
        && ctx.affinity_pure_miss_streak >= AFFINITY_MISS_STREAK_THRESHOLD
    {
        hints.invalidate(key);
    }
}

/// Emit phase latency histograms from request-start watermarks.
pub fn observe_request_timeline(ctx: &GatewayContext) {
    let pipeline = ctx.request_pipeline.map(|p| p.as_str());
    let model = ctx.model.as_str();
    let start = ctx.request_start;

    let mark = |phase: &str, at: Option<Instant>| {
        if let Some(t) = at {
            global_metrics().record_request_phase(phase, t.duration_since(start), pipeline, model);
        }
    };

    let timeline = &ctx.timeline;
    mark("body_read_start", timeline.body_read_start);
    mark("body_read_done", timeline.body_read_done);
    mark("json_parse_done", timeline.json_parse_done);
    mark("pipeline_select_done", timeline.pipeline_select_done);
    mark("cache_lookup_done", timeline.cache_lookup_done);
    mark("upstream_connect_done", timeline.upstream_connect_done);
    mark("upstream_headers_sent", timeline.upstream_headers_sent);
    mark("upstream_body_sent", timeline.upstream_body_sent);
    mark("ttft", timeline.ttft);
    mark("upstream_body_done", timeline.upstream_body_done);
    mark("cache_write_done", timeline.cache_write_done);
    mark(
        "logging_done",
        timeline.logging_done.or(Some(Instant::now())),
    );
}

/// Stamp a timeline watermark if not yet set.
pub fn timeline_stamp(field: &mut Option<Instant>) {
    if field.is_none() {
        *field = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{GatewayContext, UpstreamState};
    use std::time::Instant;

    #[test]
    fn finalize_affinity_inserts_on_request_hits_only() {
        let hints: moka::sync::Cache<String, String> =
            moka::sync::Cache::builder().max_capacity(8).build();
        let mut ctx = GatewayContext::new("req".into());
        ctx.upstream = UpstreamState {
            affinity_key: Some("aff-1".into()),
            backend_name: Some("backend-a".into()),
            ..UpstreamState::default()
        };
        ctx.affinity_prompt_cache_hits = 100;
        finalize_affinity_backend_hint(&hints, &ctx);
        assert_eq!(hints.get("aff-1").as_deref(), Some("backend-a"));
    }

    #[test]
    fn finalize_affinity_invalidates_after_miss_streak() {
        let hints: moka::sync::Cache<String, String> =
            moka::sync::Cache::builder().max_capacity(8).build();
        hints.insert("aff-2".to_string(), "stale".to_string());
        let mut ctx = GatewayContext::new("req".into());
        ctx.upstream.affinity_key = Some("aff-2".into());
        ctx.affinity_prompt_cache_misses = 50;
        ctx.affinity_pure_miss_streak = AFFINITY_MISS_STREAK_THRESHOLD;
        finalize_affinity_backend_hint(&hints, &ctx);
        assert!(hints.get("aff-2").is_none());
    }

    #[test]
    fn finalize_affinity_keeps_hint_on_transient_miss_streak() {
        let hints: moka::sync::Cache<String, String> =
            moka::sync::Cache::builder().max_capacity(8).build();
        hints.insert("aff-3".to_string(), "backend-x".to_string());
        let mut ctx = GatewayContext::new("req".into());
        ctx.upstream.affinity_key = Some("aff-3".into());
        ctx.affinity_prompt_cache_misses = 10;
        ctx.affinity_pure_miss_streak = AFFINITY_MISS_STREAK_THRESHOLD - 1;
        finalize_affinity_backend_hint(&hints, &ctx);
        assert_eq!(hints.get("aff-3").as_deref(), Some("backend-x"));
    }

    #[test]
    fn accumulate_does_not_touch_hints_cache() {
        let hints: moka::sync::Cache<String, String> =
            moka::sync::Cache::builder().max_capacity(8).build();
        let mut ctx = GatewayContext::new("req".into());
        ctx.request_start = Instant::now();
        accumulate_affinity_prompt_cache_usage(&mut ctx, 0, 10);
        assert!(hints.get("any").is_none());
        assert_eq!(ctx.affinity_prompt_cache_misses, 10);
    }
}
