//! Phase: upstream_response_body_filter — streaming/non-streaming body processing.
//!
//! Extracted from `proxy.rs` `ProxyHttp::upstream_response_body_filter`.
//! The SSE pipeline loop is already delegated to `sse_pipeline/`; only the outer state machine,
//! cache write, and metric recording live here.

use crate::cache_helpers::{
    build_cache_entry, build_cache_entry_with_sse, build_semantic_query_text,
    prepare_response_body_for_cache, should_store_sse_body,
};
use crate::cache_response::completion_json_has_visible_client_content;
use crate::context::GatewayContext;
use crate::debug_agent_log;
use crate::error_jsons::upstream_error_preview;
use crate::metrics_helpers::{
    accumulate_affinity_prompt_cache_usage, record_usage_metrics, timeline_stamp,
};
use crate::proxy::GatewayProxy;
use crate::sse::UsageData;
use crate::sse::parse_sse_chunk;
use crate::sse_pipeline::{SsePipeline, select_sse_pipeline};
use crate::upstream_response_decompress::decompress_upstream_chunk;
use crab_metrics::{CacheTier, LatencyKind, global_metrics};
use crab_reasoning::{rewrite_response_body, sanitize_client_completion};
use pingora_core::prelude::*;
use pingora_proxy::Session;
use std::time::Duration;
use tracing::warn;

/// Run the `upstream_response_body_filter` phase.
pub(crate) fn run(
    proxy: &GatewayProxy,
    _session: &mut Session,
    body: &mut Option<bytes::Bytes>,
    end_of_stream: bool,
    ctx: &mut GatewayContext,
) -> Result<Option<Duration>> {
    if ctx.is_models_list {
        return Ok(None);
    }

    if !ctx.upstream.error_body_logged
        && let Some(status) = ctx.upstream.http_status
        && status >= 400
        && let Some(chunk) = body.as_ref()
    {
        let preview = upstream_error_preview(chunk.as_ref());
        let has_reasoning_err = preview.contains("reasoning_content");
        warn!(
            request_id = %ctx.request_id,
            status,
            body_len = chunk.len(),
            preview = %preview,
            "Upstream error response body"
        );
        // #region agent log
        debug_agent_log(
            "UP4B",
            "proxy.rs:upstream_response_body_filter",
            "upstream error body preview",
            serde_json::json!({
                "request_id": ctx.request_id,
                "status": status,
                "preview": preview,
                "has_reasoning_content_msg": has_reasoning_err,
                "body_len": chunk.len(),
                "end_of_stream": end_of_stream,
            }),
        );
        // #endregion
        ctx.upstream.error_body_logged = true;
    }

    if let Some(mut data) = body.take() {
        if ctx.upstream.response_decompress.encoding.is_some() {
            data = match decompress_upstream_chunk(
                &mut ctx.upstream.response_decompress,
                data,
                end_of_stream,
            ) {
                Ok(d) => d,
                Err(e) => {
                    warn!(
                        request_id = %ctx.request_id,
                        error = %e,
                        "upstream response decompress chunk failed"
                    );
                    return Ok(None);
                }
            };
        }
        if data.is_empty() && !end_of_stream {
            *body = None;
            return Ok(None);
        }

        // #region agent log
        if !ctx.upstream.first_body_chunk_logged {
            ctx.upstream.first_body_chunk_logged = true;
            debug_agent_log(
                "UP-BODY",
                "proxy.rs:upstream_response_body_filter",
                "first upstream body chunk",
                serde_json::json!({
                    "request_id": ctx.request_id,
                    "chunk_len": data.len(),
                    "upstream_status": ctx.upstream.http_status,
                    "is_streaming": ctx.is_streaming,
                    "end_of_stream": end_of_stream,
                    "elapsed_since_start_ms": ctx.request_start.elapsed().as_millis(),
                    "pipeline": ctx.request_pipeline.map(|p| p.as_str()),
                    "has_prepared": ctx.prepared_request.is_some(),
                }),
            );
        }
        // #endregion
        if !ctx.is_streaming {
            ctx.accumulated_body.extend_from_slice(&data);
            ctx.response_body_preview.extend_from_slice(&data);
        }

        if ctx.is_streaming {
            if ctx.ttft.is_none()
                && let Some(headers_at) = ctx.upstream.headers_at
            {
                let sse_ttft = headers_at.elapsed();
                ctx.ttft = Some(sse_ttft);
                timeline_stamp(&mut ctx.timeline.ttft);
                global_metrics().record_latency(
                    LatencyKind::TTFT,
                    sse_ttft,
                    &ctx.model,
                    Some(CacheTier::Miss),
                );
            }

            // Initialize SSE pipeline on first streaming chunk.
            if ctx.stream.stream_pipeline.is_none() {
                ctx.stream.stream_pipeline =
                    select_sse_pipeline(ctx, proxy.state.reasoning_store.clone());
            }

            // Detect rate-limit errors embedded in SSE data chunks.
            // Must run before the mutable borrow of stream_pipeline below.
            if !ctx.upstream.sse_rate_limited {
                let sse_events = parse_sse_chunk(&data);
                let rate_limited = sse_events.iter().any(|e| e.is_rate_limit_error());
                if rate_limited {
                    ctx.upstream.sse_rate_limited = true;
                    let old_key_id = ctx
                        .upstream
                        .key_guard
                        .as_ref()
                        .map(|g| g.key_id().to_string());
                    if let Some(ref key_id) = old_key_id {
                        let pool = proxy.active_upstream_profile(ctx).resolve_upstream_pool();
                        pool.report_rate_limited(key_id);
                        crab_metrics::global_metrics()
                            .record_upstream_key_request(key_id, "rate_limited");
                        let account_id = ctx
                            .upstream
                            .key_guard
                            .as_ref()
                            .and_then(|g| {
                                pool.list_status()
                                    .into_iter()
                                    .find(|s| s.id == g.key_id())
                                    .map(|s| s.account_id)
                            });
                        if let Some(new_guard) =
                            pool.acquire_excluding_account(account_id.as_deref())
                        {
                            ctx.upstream.key_guard = Some(new_guard);
                            crab_metrics::global_metrics()
                                .record_upstream_key_retry("sse_rate_limited_rotate");
                        }
                        warn!(
                            request_id = %ctx.request_id,
                            key_id = key_id,
                            model = %ctx.model,
                            "SSE stream contains rate-limit error from upstream; key cooled down"
                        );
                    }
                }
            }

            if let Some(pipeline) = ctx.stream.stream_pipeline.as_mut() {
                ctx.accumulated_body.extend_from_slice(&data);
                let result = pipeline.process_chunk(data, &mut ctx.stream.client_sse_body);
                if let Some(usage) = result.usage {
                    ctx.tokens.total += usage.prompt_tokens + usage.completion_tokens;
                    ctx.tokens.last_input = usage.prompt_tokens;
                    ctx.tokens.last_output = usage.completion_tokens;
                    ctx.tokens.last_prompt_cache_hit = usage.prompt_cache_hit_tokens;
                    ctx.tokens.last_prompt_cache_miss = usage.prompt_cache_miss_tokens;
                    record_usage_metrics(
                        &usage,
                        &ctx.model,
                        ctx.consumer.as_deref(),
                        ctx.domain.as_deref(),
                        ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                        ctx.upstream
                            .affinity_key
                            .as_deref()
                            .map(crab_capture::affinity_kind_from_key),
                        &proxy.state.runtime,
                        &proxy.state.pricing,
                    );
                    if proxy.state.features.affinity_prompt_cache_feedback {
                        accumulate_affinity_prompt_cache_usage(
                            ctx,
                            usage.prompt_cache_hit_tokens,
                            usage.prompt_cache_miss_tokens,
                        );
                    }
                }
                *body = result.client_bytes;
            } else {
                *body = Some(data);
            }
        } else {
            *body = Some(data);
        }
    }

    // Non-streaming reasoning rewrite: buffer upstream chunks; only emit rewritten body on EOS.
    if !ctx.is_streaming && ctx.prepared_request.is_some() && !end_of_stream {
        *body = None;
        return Ok(None);
    }

    if end_of_stream && !ctx.is_streaming {
        if let Some(guard) = &ctx.coalesce_guard {
            guard.mark_completed();
        }

        if let Some(headers_at) = ctx.upstream.headers_at {
            let latency = headers_at.elapsed();
            ctx.upstream.latency_ms = Some(latency.as_secs_f64() * 1000.0);
            timeline_stamp(&mut ctx.timeline.upstream_body_done);
            global_metrics().record_latency(
                LatencyKind::Upstream,
                latency,
                &ctx.model,
                Some(CacheTier::Miss),
            );
            if let Some(kid) = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()) {
                global_metrics().record_upstream_key_latency(kid, latency);
            }
        }

        let client_body: Vec<u8> = if let Some(ref prepared) = ctx.prepared_request {
            match rewrite_response_body(
                &ctx.accumulated_body,
                &prepared.original_model,
                Some(&proxy.state.reasoning_store),
                &prepared.record_response_messages,
                &prepared.cache_namespace,
                ctx.stream.pending_recovery_notice.as_deref(),
                &prepared.record_response_contexts,
                ctx.cached_reasoning_config.display_reasoning,
                ctx.cached_reasoning_config.collapsible_reasoning,
            ) {
                Some(rewritten) => {
                    *body = Some(bytes::Bytes::from(rewritten.clone()));
                    rewritten
                }
                None => ctx.accumulated_body.clone(),
            }
        } else {
            ctx.accumulated_body.clone()
        };

        if let Ok(body_value) = serde_json::from_slice::<serde_json::Value>(&client_body) {
            if let Some(usage) = body_value.get("usage") {
                let usage_data = UsageData {
                    prompt_tokens: usage
                        .get("prompt_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    completion_tokens: usage
                        .get("completion_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    prompt_cache_hit_tokens: usage
                        .get("prompt_cache_hit_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    prompt_cache_miss_tokens: usage
                        .get("prompt_cache_miss_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                };
                ctx.tokens.total += usage_data.prompt_tokens + usage_data.completion_tokens;
                ctx.tokens.last_input = usage_data.prompt_tokens;
                ctx.tokens.last_output = usage_data.completion_tokens;
                ctx.tokens.last_prompt_cache_hit = usage_data.prompt_cache_hit_tokens;
                ctx.tokens.last_prompt_cache_miss = usage_data.prompt_cache_miss_tokens;
                record_usage_metrics(
                    &usage_data,
                    &ctx.model,
                    ctx.consumer.as_deref(),
                    ctx.domain.as_deref(),
                    ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                    ctx.upstream
                        .affinity_key
                        .as_deref()
                        .map(crab_capture::affinity_kind_from_key),
                    &proxy.state.runtime,
                    &proxy.state.pricing,
                );
                if proxy.state.features.affinity_prompt_cache_feedback {
                    accumulate_affinity_prompt_cache_usage(
                        ctx,
                        usage_data.prompt_cache_hit_tokens,
                        usage_data.prompt_cache_miss_tokens,
                    );
                }
            }

            if let Some(cache_key) = &ctx.cache_key {
                let ttl_secs = proxy
                    .state
                    .tiered_cache
                    .resolve_ttl(&ctx.model, ctx.consumer.as_deref());
                let display_reasoning = ctx.cached_reasoning_config.display_reasoning;

                let cache_body =
                    prepare_response_body_for_cache(client_body.clone(), display_reasoning);
                let entry = build_cache_entry(
                    cache_body,
                    ctx.model.clone(),
                    ttl_secs,
                    ctx.is_streaming,
                    display_reasoning,
                );

                timeline_stamp(&mut ctx.timeline.cache_write_done);
                let tiered_cache = proxy.state.tiered_cache.clone();
                let cache_key = cache_key.clone();
                let model = ctx.model.clone();
                let consumer = ctx.consumer.clone();
                tokio::spawn(async move {
                    if let Err(e) = tiered_cache
                        .put(&cache_key, entry, &model, consumer.as_deref())
                        .await
                    {
                        warn!(error = %e, "Failed to cache response");
                    }
                });

                if let Some(semantic_cache) = &proxy.state.semantic_cache
                    && let Some(original_body) = &ctx.original_request_body
                    && let Ok(payload) = serde_json::from_slice::<serde_json::Value>(original_body)
                    && let Some(messages) = payload.get("messages").and_then(|m| m.as_array())
                    && let Some(query_text) = build_semantic_query_text(messages)
                {
                    let semantic_cache = semantic_cache.clone();
                    let entry_clone = build_cache_entry(
                        prepare_response_body_for_cache(client_body.clone(), display_reasoning),
                        ctx.model.clone(),
                        ttl_secs,
                        ctx.is_streaming,
                        display_reasoning,
                    );

                    let project_id = ctx.project_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = semantic_cache
                            .insert(&query_text, &entry_clone, project_id.as_deref())
                            .await
                        {
                            warn!(error = %e, "Failed to insert into semantic cache");
                        }
                    });
                }
            }
        }
    }

    if end_of_stream && ctx.is_streaming {
        if let Some(pipeline) = ctx.stream.stream_pipeline.as_mut() {
            let flush = pipeline.flush_remainder(&mut ctx.stream.client_sse_body);
            if pipeline.reasoning_finalized() {
                ctx.stream.reasoning_finalized = true;
            }
            if let Some(bytes) = flush.client_bytes {
                *body = Some(bytes);
            }
        }

        if let Some(guard) = &ctx.coalesce_guard {
            guard.mark_completed();
        }

        if let Some(headers_at) = ctx.upstream.headers_at {
            let latency = headers_at.elapsed();
            ctx.upstream.latency_ms = Some(latency.as_secs_f64() * 1000.0);
            timeline_stamp(&mut ctx.timeline.upstream_body_done);
            global_metrics().record_latency(
                LatencyKind::Upstream,
                latency,
                &ctx.model,
                Some(CacheTier::Miss),
            );
            if let Some(kid) = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()) {
                global_metrics().record_upstream_key_latency(kid, latency);
            }
        }

        let stream_messages = ctx
            .stream
            .stream_pipeline
            .as_ref()
            .map(|p| p.messages())
            .unwrap_or_default();
        if let Some(cache_key) = &ctx.cache_key {
            let messages = stream_messages;
            if !messages.is_empty() {
                let reasoning_cfg = &ctx.cached_reasoning_config;
                let mut response_value = serde_json::json!({
                    "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion",
                    "created": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    "model": ctx.model,
                    "choices": messages.iter().enumerate().map(|(idx, msg)| {
                        serde_json::json!({
                            "index": idx,
                            "message": msg,
                            "finish_reason": "stop"
                        })
                    }).collect::<Vec<_>>(),
                    "usage": {
                        "prompt_tokens": 0,
                        "completion_tokens": 0,
                        "total_tokens": 0
                    }
                });
                sanitize_client_completion(
                    &mut response_value,
                    reasoning_cfg.display_reasoning,
                    reasoning_cfg.collapsible_reasoning,
                );
                let response_bytes = serde_json::to_vec(&response_value).unwrap_or_default();
                // Store synthesized completion JSON as response preview for trace logging.
                ctx.response_body_preview = response_bytes.clone();

                let ttl_secs = proxy
                    .state
                    .tiered_cache
                    .resolve_ttl(&ctx.model, ctx.consumer.as_deref());

                if proxy.state.runtime.stream_cache_enabled()
                    && completion_json_has_visible_client_content(
                        &response_bytes,
                        reasoning_cfg.display_reasoning,
                    )
                {
                    let sse_body = std::mem::take(&mut ctx.stream.client_sse_body);
                    let max_sse = proxy.state.max_sse_cache_bytes;
                    let entry_for_cache = if should_store_sse_body(sse_body.len(), max_sse) {
                        build_cache_entry_with_sse(
                            response_bytes.clone(),
                            sse_body,
                            ctx.model.clone(),
                            ttl_secs,
                            true,
                            reasoning_cfg.display_reasoning,
                        )
                    } else {
                        warn!(
                            sse_len = sse_body.len(),
                            limit = max_sse,
                            "SSE body exceeds max_sse_cache_bytes; caching JSON only"
                        );
                        global_metrics().record_stream_cache_sse_omitted("over_limit");
                        build_cache_entry(
                            response_bytes.clone(),
                            ctx.model.clone(),
                            ttl_secs,
                            true,
                            reasoning_cfg.display_reasoning,
                        )
                    };
                    timeline_stamp(&mut ctx.timeline.cache_write_done);
                    let tiered_cache = proxy.state.tiered_cache.clone();
                    let cache_key = cache_key.clone();
                    let model = ctx.model.clone();
                    let consumer = ctx.consumer.clone();
                    tokio::spawn(async move {
                        if let Err(e) = tiered_cache
                            .put(&cache_key, entry_for_cache, &model, consumer.as_deref())
                            .await
                        {
                            warn!(error = %e, "Failed to cache streaming response");
                        }
                    });
                } else if proxy.state.runtime.stream_cache_enabled() {
                    warn!(
                        request_id = %ctx.request_id,
                        cache_key = ?ctx.cache_key,
                        "Skipping stream cache write: no client-visible content after sanitize"
                    );
                    // #region agent log
                    debug_agent_log(
                        "H1",
                        "proxy.rs:upstream_response_body_filter",
                        "skipped hollow stream cache write",
                        serde_json::json!({
                            "request_id": ctx.request_id,
                            "response_body_len": response_bytes.len(),
                            "client_sse_len": ctx.stream.client_sse_body.len(),
                            "display_reasoning": reasoning_cfg.display_reasoning,
                        }),
                    );
                    // #endregion
                }

                if completion_json_has_visible_client_content(
                    &response_bytes,
                    reasoning_cfg.display_reasoning,
                ) && let Some(semantic_cache) = &proxy.state.semantic_cache
                    && let Some(original_body) = &ctx.original_request_body
                    && let Ok(payload) = serde_json::from_slice::<serde_json::Value>(original_body)
                    && let Some(messages) = payload.get("messages").and_then(|m| m.as_array())
                    && let Some(query_text) = build_semantic_query_text(messages)
                {
                    let semantic_cache = semantic_cache.clone();
                    let entry_for_semantic = build_cache_entry(
                        response_bytes.clone(),
                        ctx.model.clone(),
                        ttl_secs,
                        true,
                        reasoning_cfg.display_reasoning,
                    );
                    let query_text = query_text.to_string();
                    let project_id = ctx.project_id.clone();

                    tokio::spawn(async move {
                        if let Err(e) = semantic_cache
                            .insert(&query_text, &entry_for_semantic, project_id.as_deref())
                            .await
                        {
                            warn!(error = %e, "Failed to insert streaming response into semantic cache");
                        }
                    });
                }
            }
        }
    }

    Ok(None)
}
