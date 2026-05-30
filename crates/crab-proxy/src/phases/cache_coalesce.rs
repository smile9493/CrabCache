//! Phase 5: Cache & Coalesce — key generation, L0/L1/L2 lookup, request coalescing.
//!
//! Extracted from `proxy.rs` `request_filter` to keep `ProxyHttp` impl manageable.

use crate::cache_helpers::cache_entry_matches_stream_mode;
use crate::cache_response::send_cached_response;
use crate::context::GatewayContext;
use crate::error_jsons::coalesce_leader_failed_error_json;
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crate::send_helpers::send_client_error;
use crate::tenant::effective_cache_namespace;
use crab_cache::CoalesceError;
use crab_metrics::global_metrics;
use crab_pipeline::RequestPipeline;
use pingora_proxy::Session;
use tracing::{debug, info, warn};

/// Outcome of the cache/coalesce phase.
pub(crate) enum CachePhaseOutcome {
    /// Request was fully handled (cache hit, coalesce follower, or error).
    /// The inner `bool` matches the `request_filter` return value semantics.
    Return(bool),
    /// No cache hit; proceed to upstream key acquisition.
    Continue,
}

const GLOBAL_RATE_KEY: &str = "__global_gateway_rps__";

/// Run Phase 5: generate cache key, probe L0/L1/L2, acquire coalesce guard.
pub(crate) async fn run(
    proxy: &GatewayProxy,
    session: &mut Session,
    ctx: &mut GatewayContext,
) -> Result<CachePhaseOutcome, pingora_core::Error> {
    if ctx.client_wire_api == crate::context::ClientWireApi::Responses {
        return Ok(CachePhaseOutcome::Continue);
    }
    let fingerprint = proxy.state.runtime.fingerprint.read().clone();
    // Observe global RPS via pingora-limits
    proxy.state.global_rate.observe(&GLOBAL_RATE_KEY, 1);

    let cache_namespace = effective_cache_namespace(
        proxy.state.cache_key_namespace.as_deref(),
        ctx.project_id.as_deref(),
    );
    let cache_key_result = if let Some(payload) = ctx.parsed_request_payload.as_ref() {
        crab_cache::generate_namespaced_cache_key_with_fingerprint_from_value(
            payload,
            cache_namespace.as_deref(),
            &fingerprint,
        )
    } else {
        let cache_key_body = ctx
            .original_request_body
            .as_deref()
            .expect("original_request_body set");
        crab_cache::generate_namespaced_cache_key_with_fingerprint(
            cache_key_body,
            cache_namespace.as_deref(),
            &fingerprint,
        )
    };
    if let Ok(cache_key) = cache_key_result {
        ctx.cache_key = Some(cache_key.clone());

        let may_try_l2 = ctx.request_pipeline == Some(RequestPipeline::CursorDeepSeekV4);
        let tiered_exact = if ctx.exact_cache_probed {
            None
        } else if may_try_l2 {
            proxy
                .state
                .tiered_cache
                .get_defer_miss(&cache_key, ctx.consumer.as_deref(), ctx.domain.as_deref())
                .await
        } else {
            proxy
                .state
                .tiered_cache
                .get(&cache_key, ctx.consumer.as_deref(), ctx.domain.as_deref())
                .await
        };
        let tiered_was_absent = tiered_exact.is_none();
        if !ctx.exact_cache_probed {
            timeline_stamp(&mut ctx.timeline.cache_lookup_done);
        }
        if let Some((entry, tier)) = tiered_exact {
            if cache_entry_matches_stream_mode(&entry, ctx.is_streaming) {
                info!(
                    request_id = %ctx.request_id,
                    cache_key = %cache_key,
                    tier = ?tier,
                    "Cache hit, returning cached response"
                );

                let sent_ok = send_cached_response(
                    session,
                    &entry,
                    &ctx.model,
                    ctx.is_streaming,
                    tier,
                    ctx.cached_reasoning_config.display_reasoning,
                )
                .await;


                if sent_ok {
                    ctx.cache_tier = Some(tier);
                    ctx.cache_hit = Some(entry.clone());
                    ctx.tokens.last_input = entry.usage.prompt_tokens;
                    ctx.tokens.last_output = entry.usage.completion_tokens;
                    // Spawn stale-while-revalidate if entry is stale.
                    if let Some(body) = &ctx.original_request_body {
                        crate::cache_revalidate::maybe_spawn_swr(session, &cache_key, &entry, body);
                    }
                    global_metrics().record_latency(
                        crab_metrics::LatencyKind::CacheFetch,
                        ctx.request_start.elapsed(),
                        &ctx.model,
                        Some(tier),
                    );
                    let cost = proxy.state.pricing.read().cost_saved_usd(
                        &ctx.model,
                        entry.usage.prompt_tokens,
                        entry.usage.completion_tokens,
                    );
                    global_metrics().record_cost_saved(
                        &ctx.model,
                        ctx.consumer.as_deref(),
                        ctx.domain.as_deref(),
                        tier,
                        cost,
                    );
                    global_metrics()
                        .record_full_response_cache_hit(tier.as_str(), ctx.domain.as_deref());
                    global_metrics().record_request_saved_by_cache(ctx.domain.as_deref());
                    return Ok(CachePhaseOutcome::Return(true));
                }

                warn!(
                    request_id = %ctx.request_id,
                    cache_key = %cache_key,
                    tier = ?tier,
                    "Hollow cache entry (no client-visible content); treating as miss"
                );
            } else {
                debug!(
                    request_id = %ctx.request_id,
                    cache_key = %cache_key,
                    entry_is_stream = entry.is_stream,
                    request_is_streaming = ctx.is_streaming,
                    "Exact cache hit ignored: stream mode mismatch"
                );
            }
        }

        // ── Prefix-aware L0 lookup ───────────────────────────────
        // Enabled by feature flag, and default-on for MiMo relay pipelines.
        let prefix_aware_enabled = proxy.state.features.read().prefix_aware_cache
            || ctx
                .request_pipeline
                .is_some_and(GatewayProxy::is_mimo_pipeline);
        if tiered_was_absent && prefix_aware_enabled {
            if let Some(payload) = ctx.parsed_request_payload.as_ref() {
                let prefix_key = crab_cache::generate_composite_cache_key_from_value(
                    payload,
                    cache_namespace.as_deref(),
                    &fingerprint,
                );
                // prefix_key format: "{ns}:{fp_ver}:{system_hash}:{tools_hash}:{msgs_hash}"
                // Extract prefix (everything before the last ':' = messages hash).
                if let Some(prefix_end) = prefix_key.rfind(':') {
                    let prefix_hash = &prefix_key[..prefix_end];
                    if let Some(entry) =
                        proxy.state.tiered_cache.prefix_l0_lookup(prefix_hash).await
                    {
                        if cache_entry_matches_stream_mode(&entry, ctx.is_streaming) {
                            global_metrics().record_prefix_index_warmup();
                            debug!(
                                request_id = %ctx.request_id,
                                prefix_hash = %prefix_hash,
                                full_key = %cache_key,
                                "Prefix-aware L0 index warm-up (request continues upstream)"
                            );
                        }
                    }
                    // Register prefix → full key for exact-cache and future prefix lookups.
                    proxy
                        .state
                        .tiered_cache
                        .update_prefix_index(prefix_hash, &cache_key);
                }
            }
        }

        if may_try_l2 {
            if proxy.try_l2_semantic_cache(session, ctx).await {
                return Ok(CachePhaseOutcome::Return(true));
            }
            if tiered_was_absent {
                proxy
                    .state
                    .tiered_cache
                    .record_absent_miss(ctx.domain.as_deref());
            }
        }

        match proxy.state.coalescer.acquire(&cache_key).await {
            Ok(guard) => {
                if !guard.is_leader() {
                    ctx.is_coalesced_follower = true;

                    if let Some((entry, tier)) =
                        proxy.state.tiered_cache.get_silent(&cache_key).await
                    {
                        if !cache_entry_matches_stream_mode(&entry, ctx.is_streaming) {
                            debug!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                entry_is_stream = entry.is_stream,
                                request_is_streaming = ctx.is_streaming,
                                "Follower cache hit ignored: stream mode mismatch"
                            );
                        } else {
                            info!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                tier = ?tier,
                                "Follower found cached response after leader completed"
                            );
                            global_metrics().record_coalesce_follower("hit");


                            let sent_ok = send_cached_response(
                                session,
                                &entry,
                                &ctx.model,
                                ctx.is_streaming,
                                tier,
                                ctx.cached_reasoning_config.display_reasoning,
                            )
                            .await;

                            if sent_ok {
                                ctx.cache_tier = Some(tier);
                                ctx.cache_hit = Some(entry.clone());
                                ctx.tokens.last_input = entry.usage.prompt_tokens;
                                ctx.tokens.last_output = entry.usage.completion_tokens;
                                // Spawn stale-while-revalidate if entry is stale.
                                if let Some(body) = &ctx.original_request_body {
                                    crate::cache_revalidate::maybe_spawn_swr(
                                        session, &cache_key, &entry, body,
                                    );
                                }
                                global_metrics().record_coalesced_request();
                                global_metrics().record_cache_hit(
                                    tier,
                                    &ctx.model,
                                    ctx.consumer.as_deref(),
                                    ctx.domain.as_deref(),
                                );
                                global_metrics().record_latency(
                                    crab_metrics::LatencyKind::CacheFetch,
                                    ctx.request_start.elapsed(),
                                    &ctx.model,
                                    Some(tier),
                                );
                                let cost = proxy.state.pricing.read().cost_saved_usd(
                                    &ctx.model,
                                    entry.usage.prompt_tokens,
                                    entry.usage.completion_tokens,
                                );
                                global_metrics().record_cost_saved(
                                    &ctx.model,
                                    ctx.consumer.as_deref(),
                                    ctx.domain.as_deref(),
                                    tier,
                                    cost,
                                );
                                global_metrics().record_full_response_cache_hit(
                                    tier.as_str(),
                                    ctx.domain.as_deref(),
                                );
                                global_metrics()
                                    .record_request_saved_by_cache(ctx.domain.as_deref());
                                return Ok(CachePhaseOutcome::Return(true));
                            }

                            warn!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                "Follower hollow cache entry; waiting for upstream path"
                            );
                        }
                    } else if guard.leader_failed() {
                        warn!(
                            request_id = %ctx.request_id,
                            cache_key = %cache_key,
                            "Follower: leader upstream failed, not retrying upstream"
                        );
                        let body = coalesce_leader_failed_error_json();
                        if !send_client_error(
                            session,
                            ctx.is_streaming,
                            http::StatusCode::BAD_GATEWAY,
                            &body,
                            &ctx.model,
                            ctx.client_wire_api,
                        )
                        .await
                        {
                            let _ = session.respond_error(502).await;
                        }
                        return Ok(CachePhaseOutcome::Return(true));
                    } else {
                        warn!(
                            request_id = %ctx.request_id,
                            cache_key = %cache_key,
                            "Follower did not find cached response, falling through to upstream"
                        );
                        global_metrics().record_coalesce_follower("fallthrough");
                    }
                } else {
                    ctx.coalesce_guard = Some(guard);
                    global_metrics().record_coalesce_leader("elected");
                }
            }
            Err(CoalesceError::CapacityExceeded) => {
                global_metrics().record_rejected("coalesce_capacity");
                global_metrics().record_rejection_by_source("client");
                let _ = session.respond_error(503).await;
                return Ok(CachePhaseOutcome::Return(true));
            }
            Err(e) => {
                warn!(
                    request_id = %ctx.request_id,
                    error = %e,
                    "Coalescing failed, proceeding without coalescing"
                );
            }
        }
    }

    Ok(CachePhaseOutcome::Continue)
}
