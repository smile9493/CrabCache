//! Phase: logging — request completion, trace logging, raw capture, reasoning flush.
//!
//! Extracted from `proxy.rs` `ProxyHttp::logging`.

use crate::context::GatewayContext;
use crate::trace_logger::composition_debug_tx;
use crab_composition::{
    CompositionDebugEntry, CompositionHints, extract_composition, extract_system_text,
    extract_tools_json,
};
use crate::debug_agent_log;
use crate::helper_fns::{build_capture_request_meta, sanitize_for_trace};
use crate::metrics_helpers::{
    finalize_affinity_backend_hint, observe_request_timeline, timeline_stamp,
};
use crate::proxy::{GatewayProxy, build_response_preview, flush_streaming_reasoning};
use crate::sse_pipeline::SsePipeline;
use crate::trace_logger::SanitizedLogEntry;
use crate::user_id_audit::apply_user_id_audit_to_entry;
use crab_capture::affinity_kind_from_key;
use crab_metrics::global_metrics;
use crab_pipeline::RequestPipeline;
use hex;
use pingora_proxy::Session;
use sha2::Digest;
use tracing::{debug, info, warn};

/// Run the logging phase: structured log, trace logger, raw capture, partial reasoning flush.
pub(crate) async fn run(
    proxy: &GatewayProxy,
    session: &mut Session,
    error: Option<&pingora_core::Error>,
    ctx: &mut GatewayContext,
) {
    let duration = ctx.request_start.elapsed();
    let latency_ms = duration.as_millis() as u64;
    record_request_composition(ctx);
    timeline_stamp(&mut ctx.timeline.logging_done);
    if proxy.state.features.affinity_prompt_cache_feedback {
        finalize_affinity_backend_hint(&proxy.state.affinity_backend_hints, ctx);
    }
    observe_request_timeline(ctx);

    if let Some(e) = error {
        warn!(
            request_id = %ctx.request_id,
            error = %e,
            duration_ms = latency_ms,
            model = %ctx.model,
            "Request failed"
        );
        if let Some(guard) = &ctx.coalesce_guard
            && guard.is_leader()
        {
            guard.mark_failed();
        }

        // #region agent log
        debug_agent_log(
            "F",
            "proxy.rs:logging",
            "upstream proxy error",
            serde_json::json!({
                "request_id": ctx.request_id,
                "error": e.to_string(),
                "duration_ms": latency_ms,
                "outbound_bytes": ctx.upstream_outbound_body_len,
                "is_streaming": ctx.is_streaming,
                "coalesce_leader": ctx.coalesce_guard.as_ref().map(|g| g.is_leader()),
            }),
        );
        // #endregion
    } else {
        // #region agent log
        debug_agent_log(
            "OK",
            "proxy.rs:logging",
            "request completed without proxy error",
            serde_json::json!({
                "request_id": ctx.request_id,
                "duration_ms": latency_ms,
                "upstream_status": ctx.upstream.http_status,
                "cache_tier": ctx.cache_tier.map(|t| t.as_str()),
                "is_streaming": ctx.is_streaming,
                "total_tokens": ctx.tokens.total,
                "client_sse_bytes": ctx.stream.client_sse_body.len(),
                "accumulated_body_bytes": ctx.accumulated_body.len(),
                "has_prepared": ctx.prepared_request.is_some(),
                "ttft_ms": ctx.ttft.map(|d| d.as_millis()),
            }),
        );
        // #endregion
        info!(
            request_id = %ctx.request_id,
            request_hash = %ctx.req_hash.as_ref().unwrap_or(&"missing".to_string()),
            content_length = ctx.content_length,
            upstream_outbound_bytes = ctx.upstream_outbound_body_len,
            upstream_status = ?ctx.upstream.http_status,
            latency_ms = latency_ms,
            model = %ctx.model,
            cache_hit = ctx.cache_tier.is_some(),
            cache_tier = ?ctx.cache_tier,
            is_streaming = ctx.is_streaming,
            streaming_defer = ctx.streaming_body.active,
            consumer = ?sanitize_for_trace(ctx.consumer.as_deref()),
            conversation_id = ?sanitize_for_trace(ctx.conversation_id.as_deref()),
            total_tokens = ctx.tokens.total,
            upstream_key_id = ?ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
            upstream_profile = ?ctx.upstream_profile_id,
            pipeline = ?ctx.request_pipeline,
            "Request completed"
        );

        if let Some(trace_logger) = &proxy.state.trace_logger {
            let max_payload = trace_logger.max_payload_bytes();
            let max_resp = trace_logger.max_response_preview_bytes();
            if let Some(body) = &ctx.original_request_body {
                let mut entry = SanitizedLogEntry::from_request(
                    body,
                    ctx.conversation_id.clone(),
                    ctx.consumer.clone(),
                    ctx.domain.clone(),
                    ctx.project_id.clone(),
                    &ctx.model,
                    ctx.tokens.total as usize,
                    duration.as_secs_f64() * 1000.0,
                    ctx.cache_tier.is_some(),
                    ctx.cache_tier.map(|t| t.as_str().to_string()),
                    ctx.request_composition.clone(),
                    max_payload,
                );
                entry.retired_prefix_messages = ctx.retired_prefix_messages;
                entry.reasoning_strategy =
                    if ctx.request_pipeline == Some(RequestPipeline::CursorDeepSeekV4) {
                        Some(
                            ctx.cached_reasoning_config
                                .missing_reasoning_strategy
                                .clone(),
                        )
                    } else {
                        Some("none".to_string())
                    };
                let hit = ctx.tokens.last_prompt_cache_hit;
                let miss = ctx.tokens.last_prompt_cache_miss;
                if hit + miss > 0 {
                    entry.prompt_cache_hit_ratio = Some(hit as f64 / (hit + miss) as f64);
                }
                entry.upstream_latency_ms = ctx.upstream.latency_ms;
                let (prefill_ms, sse_ttft_ms) = crate::helper_fns::request_timing_ms(ctx);
                entry.prefill_ms = prefill_ms;
                // Legacy trace field: time before upstream stream segment (≈ body read + upload + prefill).
                entry.pre_header_ms = ctx.upstream.latency_ms.map(|up| {
                    (latency_ms as f64 - up).max(0.0)
                });
                entry.ttft_ms = sse_ttft_ms;
                if ctx.tokens.last_input > 0 || ctx.tokens.last_output > 0 {
                    entry.input_tokens = Some(ctx.tokens.last_input);
                    entry.output_tokens = Some(ctx.tokens.last_output);
                    entry.prompt_tokens =
                        ctx.tokens.last_input.saturating_add(ctx.tokens.last_output) as usize;
                }
                // Populate response_preview from best available source
                if max_resp > 0 {
                    entry.response_preview = build_response_preview(ctx, max_resp);
                }
                entry.upstream_key_id = ctx
                    .upstream
                    .key_guard
                    .as_ref()
                    .map(|g| g.key_id().to_string());
                apply_user_id_audit_to_entry(
                    &mut entry,
                    ctx.request_pipeline,
                    ctx.upstream_profile_id.as_deref(),
                    ctx.upstream_model.as_deref(),
                    ctx.project_id.as_deref(),
                    ctx.original_request_body.as_deref(),
                    ctx.upstream_body_for_capture
                        .as_ref()
                        .map(|b| b.as_ref())
                        .or(ctx.new_request_body.as_deref()),
                );
                entry.affinity_key = ctx.upstream.affinity_key.clone();
                entry.affinity_kind = ctx
                    .upstream
                    .affinity_key
                    .as_deref()
                    .map(|k| affinity_kind_from_key(k).to_string());
                entry.backend_name = ctx.upstream.backend_name.clone();
                entry.is_coalesced = ctx.is_coalesced_follower;
                entry.client_key_id = ctx
                    .upstream
                    .key_guard
                    .as_ref()
                    .map(|g| g.key_id().to_string());
                entry.session_fingerprint = ctx.session_fingerprint.clone();
                if let Some(backend) = &ctx.upstream.backend_name {
                    let result = if ctx.upstream.http_status.is_some_and(|s| s >= 400) {
                        "error"
                    } else {
                        "ok"
                    };
                    global_metrics().record_backend_request(backend, result);
                }
                trace_logger.log(entry);
            }
        }
    }

    // ── Raw capture (with sampling) ───────────────────────────────
    if let Some(raw_logger) = &proxy.state.raw_capture_logger {
        let req_path = session.req_header().uri.path();
        if !raw_logger.should_skip(req_path) {
            let has_error = ctx
                .upstream
                .http_status
                .is_some_and(|s| s >= 400);
            let body_bytes = ctx.content_length.max(ctx.upstream_outbound_body_len);
            if raw_logger.should_sample(has_error, body_bytes)
                == crate::raw_capture::SampleDecision::Capture
            {
                let reasoning_strategy = ctx
                    .cached_reasoning_config
                    .missing_reasoning_strategy
                    .as_str();
                let capture_meta = build_capture_request_meta(session, ctx, latency_ms);
                raw_logger.capture(
                    &ctx.request_id,
                    ctx.req_hash.as_deref(),
                    &ctx.model,
                    ctx.consumer.as_deref(),
                    ctx.project_id.as_deref(),
                    ctx.request_pipeline.as_ref().map(|p| p.as_str()),
                    ctx.is_streaming,
                    ctx.retired_prefix_messages,
                    Some(reasoning_strategy),
                    ctx.original_request_body.as_deref(),
                    ctx.upstream_body_for_capture
                        .as_ref()
                        .map(|b| b.as_ref())
                        .or(ctx.new_request_body.as_deref()),
                    ctx.parsed_request_payload.as_deref(),
                    ctx.parsed_upstream_payload.as_deref(),
                    capture_meta,
                );
            }
        }
    }

    if let Some(ttft) = ctx.ttft {
        debug!(
            request_id = %ctx.request_id,
            ttft_ms = ttft.as_millis() as u64,
            "Time to first token recorded"
        );
    }

    if let Some(cache_key) = &ctx.cache_key
        && ctx.cache_hit.is_none()
    {
        debug!(
            request_id = %ctx.request_id,
            cache_key = %cache_key,
            "Cache miss for request"
        );
    }

    if ctx.is_streaming && !ctx.stream.reasoning_finalized {
        let stored = if let Some(pipeline) = ctx.stream.stream_pipeline.as_mut() {
            pipeline.store_partial_reasoning(&proxy.state.reasoning_store)
        } else {
            flush_streaming_reasoning(ctx, &proxy.state.reasoning_store)
        };
        if stored > 0 {
            debug!(
                request_id = %ctx.request_id,
                stored,
                "Stored partial streaming reasoning before request exit"
            );
        }
    }
}

/// Composition trace (off hot path — see `latency_optimizations.md` optimization B).
fn record_request_composition(ctx: &mut GatewayContext) {
    if ctx.request_composition.is_some() {
        return;
    }
    let Some(payload) = ctx.parsed_request_payload.as_ref() else {
        return;
    };
    let hints = CompositionHints {
        consumer: ctx.consumer.clone().unwrap_or_default(),
        domain: ctx.domain.clone().unwrap_or_default(),
        project_id: ctx.project_id.clone(),
        pipeline: ctx
            .request_pipeline
            .map(|p| p.as_str().to_string())
            .unwrap_or_default(),
        user_agent: None,
        upstream_model: ctx.upstream_model.clone(),
    };
    let comp = extract_composition(payload, &hints);
    global_metrics().record_composition_metrics(&comp);
    ctx.request_composition = Some(comp);

    if let Some(debug_tx) = composition_debug_tx() {
        let request_hash = ctx.req_hash.clone().unwrap_or_else(|| {
            let mut hasher = sha2::Sha256::new();
            if let Some(body) = ctx.original_request_body.as_deref() {
                hasher.update(body);
            } else {
                hasher.update(payload.to_string().as_bytes());
            }
            let h = hex::encode(hasher.finalize());
            h[..h.len().min(16)].to_string()
        });
        let system_text = extract_system_text(payload, 100_000);
        let tools_json = extract_tools_json(payload, 100_000);
        if system_text.is_some() || tools_json.is_some() {
            let debug_entry = CompositionDebugEntry {
                timestamp_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                request_hash,
                consumer: ctx.consumer.clone().unwrap_or_default(),
                domain: ctx.domain.clone().unwrap_or_default(),
                project_id: ctx.project_id.clone(),
                model: ctx.model.clone(),
                system_text,
                tools_json,
            };
            debug_tx.send(debug_entry).ok();
        }
    }
}
