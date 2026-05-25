use crate::context::PricingConfig;
use crate::runtime::RuntimeConfig;
use crate::sse::UsageData;
use crab_metrics::global_metrics;

pub fn record_usage_metrics(
    usage: &UsageData,
    model: &str,
    consumer: Option<&str>,
    domain: Option<&str>,
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

    if usage.prompt_cache_hit_tokens > 0 {
        global_metrics().record_upstream_prompt_cache(
            "hit",
            usage.prompt_cache_hit_tokens,
            model,
            consumer,
            domain,
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
    }

    let total_tokens = usage.prompt_tokens.saturating_add(usage.completion_tokens);
    let spend = pricing.cost_saved_usd(model, usage.prompt_tokens, usage.completion_tokens);
    runtime.record_domain_usage(domain, total_tokens, spend);
}
