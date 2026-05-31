//! Phase: logging — request completion, trace logging, raw capture, reasoning flush.
//!
//! Extracted from `proxy.rs` `ProxyHttp::logging`.

use crate::context::GatewayContext;
use crate::helper_fns::{build_capture_request_meta, sanitize_for_trace};
use crate::metrics_helpers::{
    finalize_affinity_backend_hint, observe_request_timeline, timeline_stamp,
};
use crate::proxy::{GatewayProxy, build_response_preview, flush_streaming_reasoning};
use crate::sse_pipeline::SsePipeline;
use crate::trace_logger::SanitizedLogEntry;
use crate::trace_logger::composition_debug_tx;
use crate::user_id_audit::apply_user_id_audit_to_entry;
use crab_capture::affinity_kind_from_key;
use crab_composition::{
    CompositionDebugEntry, CompositionHints, extract_composition, extract_system_text,
    extract_tools_json,
};
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
    if proxy.state.features.read().affinity_prompt_cache_feedback {
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
        // Persist partial Responses chain even when the client already disconnected.
        let graceful_tail = crate::responses_wire::build_graceful_responses_stream_tail(
            ctx,
            proxy.state.responses_chain_store.as_ref(),
        );
        let _ = graceful_tail;
        if let Some(guard) = &ctx.coalesce_guard
            && guard.is_leader()
        {
            guard.mark_failed();
        }
    } else {
        info!(
            request_id = %ctx.request_id,
            request_hash = %ctx.req_hash.as_ref().unwrap_or(&"missing".to_string()),
            content_length = ctx.content_length,
            client_ip = ?ctx.client_ip,
            client_peer_addr = ?ctx.client_peer_addr,
            upstream_outbound_bytes = ctx.upstream_outbound_body_len,
            upstream_status = ?ctx.upstream.http_status,
            latency_ms = latency_ms,
            model = %ctx.model,
            cache_hit = ctx.cache_tier.is_some(),
            cache_tier = ?ctx.cache_tier,
            is_streaming = ctx.is_streaming,
            session_store = ?ctx.session_store_outcome,
            consumer = ?sanitize_for_trace(ctx.consumer.as_deref()),
            conversation_id = ?sanitize_for_trace(ctx.conversation_id.as_deref()),
            total_tokens = ctx.tokens.total,
            upstream_key_id = ?ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
            upstream_profile = ?ctx.upstream_profile_id,
            pipeline = ?ctx.request_pipeline,
            "Request completed"
        );

        // ── Idempotency save ──
        // If this request carried an idempotency key, save the response so subsequent
        // retries with the same key get an instant reply.
        if let Some(idem_key) = ctx.idempotency_key.as_deref() {
            if let Some(status) = ctx.upstream.http_status {
                proxy.state.idempotency.save(idem_key, status, ctx.accumulated_body.clone());
            }
        }

        // ── Event bus emission (non-blocking) ──
        if proxy.state.event_bus.subscriber_count() > 0 {
            use crate::event_bus::{GatewayEvent, RequestCompletedEvent, now_secs};
            let event = GatewayEvent::RequestCompleted(RequestCompletedEvent {
                request_id: ctx.request_id.clone(),
                model: ctx.model.clone(),
                consumer: ctx.consumer.clone(),
                project_id: ctx.project_id.clone(),
                latency_ms,
                cache_hit: ctx.cache_tier.is_some(),
                cache_tier: ctx.cache_tier.map(|t| t.as_str().to_string()),
                is_streaming: ctx.is_streaming,
                input_tokens: Some(ctx.tokens.last_input),
                output_tokens: Some(ctx.tokens.last_output),
                upstream_status: ctx.upstream.http_status,
                pipeline: ctx
                    .request_pipeline
                    .as_ref()
                    .map(|p| p.as_str().to_string()),
                backend_name: ctx.upstream.backend_name.clone(),
                client_ip: ctx.client_ip.clone(),
                timestamp: now_secs(),
            });
            proxy.state.event_bus.publish(event);
        }

        if crate::responses_wire::needs_responses_wire_translate(ctx) {
            let (tool_names, output_item_types) = ctx
                .stream
                .responses_translator
                .as_ref()
                .map(|t| {
                    let output = t.completed_output();
                    let types: Vec<String> = output
                        .iter()
                        .filter_map(|item| {
                            item.get("type")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        })
                        .collect();
                    let tools: Vec<String> = output
                        .iter()
                        .filter(|item| {
                            item.get("type").and_then(|v| v.as_str()) == Some("function_call")
                        })
                        .filter_map(|item| {
                            item.get("name")
                                .and_then(|n| n.as_str())
                                .map(str::to_string)
                        })
                        .collect();
                    (tools, types)
                })
                .unwrap_or((Vec::new(), Vec::new()));
            let msg_text_len = ctx
                .stream
                .responses_translator
                .as_ref()
                .map(|t| {
                    t.completed_output()
                        .iter()
                        .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("message"))
                        .and_then(|item| {
                            item.get("content")
                                .and_then(|c| c.as_array())
                                .and_then(|a| a.first())
                                .and_then(|p| p.get("text"))
                                .and_then(|t| t.as_str())
                                .map(str::len)
                        })
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            let unregistered: Vec<String> = tool_names
                .iter()
                .filter(|n| {
                    !ctx.stream
                        .client_responses_tool_names
                        .iter()
                        .any(|r| r == *n)
                })
                .cloned()
                .collect();
            // #region agent log
            crate::debug_log::debug_agent_log(
                "K",
                "logging.rs:run",
                "responses wire success summary",
                serde_json::json!({
                    "request_id": ctx.request_id,
                    "duration_ms": latency_ms,
                    "exec_only_surface": ctx.stream.responses_exec_only_surface,
                    "client_tools": ctx.stream.client_responses_tool_names,
                    "downstream_tool_names": tool_names,
                    "output_item_types": output_item_types,
                    "unregistered_tools": unregistered,
                    "message_text_len": msg_text_len,
                    "has_response_completed": crate::sse::sse_bytes_contains_event(
                        &ctx.stream.client_sse_body,
                        "response.completed",
                    ),
                    "has_done_marker": ctx.stream.client_sse_body.windows(6).any(|w| w == b"[DONE]"),
                    "client_sse_len": ctx.stream.client_sse_body.len(),
                }),
            );
            // #endregion
        }

        if let Some(pipeline) = ctx.request_pipeline
            && proxy.state.features.read().mimo_session_store
            && GatewayProxy::mimo_session_store_applies(ctx)
            && ctx.cache_tier.is_none()
            && ctx.upstream.http_status.is_none_or(|s| s < 400)
            && let (Some(store), Some(redis_key), Some(base)) = (
                proxy.state.session_store.as_ref(),
                ctx.session_store_redis_key.clone(),
                ctx.session_persist_base.clone(),
            )
        {
            let assistant = crate::session_store::extract_assistant_content(
                ctx.is_streaming,
                &ctx.accumulated_body,
                &ctx.stream.client_sse_body,
            );
            crate::session_store::spawn_session_persist(
                store.clone(),
                redis_key,
                base,
                assistant,
                proxy.state.features.read().mimo_session_store_ttl_secs,
                proxy.state.features.read().mimo_session_store_max_messages,
            );
        }

        if let Some(trace_logger) = &proxy.state.trace_logger {
            let max_payload = trace_logger.max_payload_bytes();
            let max_resp = trace_logger.max_response_preview_bytes();
            let passthrough_trace = ctx.request_passthrough.armed_prefix_len > 0;
            if ctx.original_request_body.is_some() || passthrough_trace {
                let body_bytes = ctx.original_request_body.as_deref().unwrap_or(&[]);
                let mut entry = SanitizedLogEntry::from_request(
                    body_bytes,
                    ctx.conversation_id.clone(),
                    ctx.consumer.clone(),
                    ctx.domain.clone(),
                    ctx.project_id.clone(),
                    &ctx.model,
                    // prompt_tokens = last upstream input_tokens when usage available, else request total
                    ctx.tokens.total as usize,
                    duration.as_secs_f64() * 1000.0,
                    ctx.cache_tier.is_some(),
                    ctx.cache_tier.map(|t| t.as_str().to_string()),
                    ctx.request_composition.clone(),
                    max_payload,
                );
                if passthrough_trace {
                    entry.content_length = ctx.content_length;
                    if let Some(full_hash) = ctx.req_hash.as_deref() {
                        entry.request_hash =
                            crate::helper_fns::trace_request_hash_prefix(full_hash);
                    }
                }
                entry.retired_prefix_messages = ctx.retired_prefix_messages;
                entry.reasoning_strategy =
                    if ctx.request_pipeline == Some(RequestPipeline::CursorDeepSeekV4) {
                        Some(
                            ctx.cached_reasoning_config
                                .missing_reasoning_strategy
                                .clone(),
                        )
                    } else if passthrough_trace {
                        Some("passthrough".to_string())
                    } else {
                        Some("none".to_string())
                    };
                let hit = ctx.tokens.last_prompt_cache_hit;
                let miss = ctx.tokens.last_prompt_cache_miss;
                if hit + miss > 0 {
                    entry.prompt_cache_hit_ratio = Some(hit as f64 / (hit + miss) as f64);
                }
                entry.session_store = ctx.session_store_outcome.clone();
                entry.stable_session_kind = ctx.stable_session_kind.clone();
                entry.upstream_outbound_bytes = Some(ctx.upstream_outbound_body_len);
                entry.upstream_latency_ms = ctx.upstream.latency_ms;
                let (prefill_ms, sse_ttft_ms) = crate::helper_fns::request_timing_ms(ctx);
                entry.prefill_ms = prefill_ms;
                // Legacy trace field: time before upstream stream segment (≈ body read + upload + prefill).
                entry.pre_header_ms = ctx
                    .upstream
                    .latency_ms
                    .map(|up| (latency_ms as f64 - up).max(0.0));
                entry.ttft_ms = sse_ttft_ms;
                if ctx.tokens.last_input > 0 || ctx.tokens.last_output > 0 {
                    entry.input_tokens = Some(ctx.tokens.last_input);
                    entry.output_tokens = Some(ctx.tokens.last_output);
                    entry.prompt_tokens = ctx.tokens.last_input as usize;
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
                entry.selected_backend_name = ctx.upstream.backend_name.clone();
                entry.backend_overload_state = ctx.upstream.backend_overload_state.clone();
                entry.backend_route_strategy = Some(
                    proxy
                        .state
                        .features
                        .read()
                        .backend_route_strategy
                        .as_str()
                        .to_string(),
                );
                entry.is_coalesced = ctx.is_coalesced_follower;
                entry.request_passthrough = passthrough_trace;
                entry.request_passthrough_prefix_len = if passthrough_trace {
                    Some(ctx.request_passthrough.armed_prefix_len)
                } else {
                    None
                };
                entry.guardrail_blocked = ctx.guardrail_blocked;
                entry.guardrail_labels = ctx.guardrail_hits.clone();
                entry.client_ip = ctx.client_ip.clone();
                entry.client_peer_addr = ctx.client_peer_addr.clone();
                entry.body_read_duration_ms =
                    match (ctx.timeline.body_read_start, ctx.timeline.body_read_done) {
                        (Some(start), Some(done)) => {
                            Some(done.duration_since(start).as_secs_f64() * 1000.0)
                        }
                        _ => None,
                    };
                entry.upload_bytes_per_sec = entry.body_read_duration_ms.and_then(|ms| {
                    if ms > 0.0 {
                        Some((ctx.content_length as f64) / (ms / 1000.0))
                    } else {
                        None
                    }
                });
                entry.upstream_send_duration_ms = match (
                    ctx.timeline.upstream_headers_sent,
                    ctx.timeline.upstream_body_sent,
                ) {
                    (Some(start), Some(done)) => {
                        Some(done.duration_since(start).as_secs_f64() * 1000.0)
                    }
                    _ => None,
                };
                entry.pipeline_degraded = false;
                entry.client_key_id = ctx
                    .upstream
                    .key_guard
                    .as_ref()
                    .map(|g| g.key_id().to_string());
                entry.session_fingerprint = ctx.session_fingerprint.clone();
                // ── New diagnostic trace fields ──────────────────────────
                entry.status_code = ctx.upstream.http_status;
                // Determine error_code from upstream status and context
                let error_code = if let Some(status) = ctx.upstream.http_status {
                    match status {
                        429 if ctx.upstream.key_guard.is_some() => Some("rate_limited".to_string()),
                        s if s >= 500 => Some("upstream_error".to_string()),
                        s if s >= 400 => Some("upstream_client_error".to_string()),
                        _ => None,
                    }
                } else {
                    None
                };
                entry.error_code = error_code;
                // limit_source: derive from upstream status and context
                entry.limit_source = if ctx.upstream.http_status == Some(429) {
                    Some("upstream_rate_limit".to_string())
                } else {
                    None
                };
                // cache_decision: summarize the cache outcome
                let cache_decision = if ctx.cache_tier.is_some() {
                    "hit"
                } else if ctx.is_coalesced_follower {
                    "skip_coalesced"
                } else if ctx.request_pipeline
                    == Some(crab_pipeline::RequestPipeline::MimoTokenPlanRelay)
                    && ctx.request_passthrough.armed_prefix_len > 0
                {
                    "skip_passthrough"
                } else {
                    "miss"
                };
                entry.cache_decision = Some(cache_decision.to_string());
                // upstream_result: classify upstream response
                entry.upstream_result = ctx.upstream.http_status.map(|status| {
                    match status {
                        200..=299 => "success",
                        429 => "429_rate_limited",
                        401 => "401_unauthorized",
                        s if s >= 500 => "5xx_error",
                        s if s >= 400 => "4xx_error",
                        _ => "unknown",
                    }
                    .to_string()
                });
                // phase_durations_ms: compute from timeline watermarks
                if let Some(phases) =
                    crate::trace_logger::compute_phase_durations(&ctx.request_start, &ctx.timeline)
                {
                    entry.phase_durations_ms = Some(phases);
                }
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

    // ── Rebuild full client body for passthrough Raw Capture ──
    if ctx.request_passthrough.armed_prefix_len > 0
        && !ctx.request_passthrough.captured_client_chunks.is_empty()
    {
        let prefix = ctx.original_request_body.take().unwrap_or_default();
        let tail_size: usize = ctx
            .request_passthrough
            .captured_client_chunks
            .iter()
            .map(|c| c.len())
            .sum();
        let mut full = Vec::with_capacity(prefix.len() + tail_size);
        full.extend_from_slice(&prefix);
        for chunk in &ctx.request_passthrough.captured_client_chunks {
            full.extend_from_slice(chunk);
        }
        ctx.original_request_body = Some(bytes::Bytes::from(full));
    }

    // ── Raw capture (with sampling) ───────────────────────────────
    if let Some(raw_logger) = &proxy.state.raw_capture_logger {
        let req_path = session.req_header().uri.path();
        if !raw_logger.should_skip(req_path) {
            let has_error = ctx.upstream.http_status.is_some_and(|s| s >= 400);
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
