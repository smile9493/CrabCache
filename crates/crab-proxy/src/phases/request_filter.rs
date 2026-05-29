//! Phase: request_filter — full upstream request validation, preparation, and caching.
//!
//! Extracted from `proxy.rs` `ProxyHttp::request_filter` to keep the trait impl manageable.

use crate::body_quick_parse::quick_parse_request_fields;
use crate::client_key_limiter::ClientKeyLimitError;
use crate::context::GatewayContext;
use crate::debug_agent_log;
use crate::error_jsons::{
    client_concurrency_exceeded_error_json, deepseek_user_concurrency_exceeded_error_json,
    missing_reasoning_error_json, upstream_pool_exhausted_error_details,
    upstream_pool_exhausted_error_json,
};
use crate::helper_fns::{
    client_session_from_authorization, fingerprint_client_key, is_models_endpoint,
    last_user_message_fingerprint, stable_session_log_fields,
};
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crate::send_helpers::{
    send_cors_preflight, send_json_error, send_json_error_with_retry_after, send_json_ok,
};
use crate::streaming_body_forward::{body_user_from_payload, refresh_affinity_key};
use crate::tenant::{
    ProjectResolveError, derive_project_id_from_client_key, effective_cache_namespace,
    resolve_project_id,
};
use crate::upstream_user_id_limiter::{DeepSeekUserIdLimitError, classify_deepseek_v4_tier};
use bytes::Bytes;
use crab_metrics::global_metrics;
use crab_pipeline::{
    PipelineOverride, PipelineRequestContext, PipelineSelection, RequestPipeline, UpstreamProvider,
    select_request_pipeline, validate_pipeline_override,
};
use crab_reasoning::{
    CursorReasoningDisplayAdapter, StreamAccumulator, prepare_generic_request,
    prepare_light_request, prepare_mimo_request, prepare_upstream_request,
};
use crab_route::extract_affinity_key;
use http::HeaderMap;
use pingora_core::prelude::*;
use pingora_proxy::Session;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

fn mimo_direct_passthrough(pipeline: RequestPipeline) -> bool {
    GatewayProxy::is_mimo_pipeline(pipeline)
}

fn request_passthrough_allowed_pipeline(pipeline: RequestPipeline) -> bool {
    mimo_direct_passthrough(pipeline)
}

/// Shared path after the full client body is available (normal read or streaming finalize).
async fn run_post_body_phases(
    proxy: &GatewayProxy,
    session: &mut Session,
    ctx: &mut GatewayContext,
    full_body: Bytes,
    conversation_id_from_header: Option<String>,
    user_agent: Option<String>,
    key_pipeline: Option<String>,
    key_upstream_profile: Option<String>,
    domain_pipeline: Option<String>,
    domain_upstream_profile: Option<String>,
) -> Result<bool> {
    let skip_early_pipeline_select = ctx.request_pipeline.is_some();
    let mut body_hasher = Sha256::new();
    body_hasher.update(full_body.as_ref());
    let req_hash = hex::encode(body_hasher.finalize());
    ctx.req_hash = Some(req_hash);
    ctx.content_length = full_body.len();
    ctx.original_request_body = Some(full_body.clone());
    timeline_stamp(&mut ctx.timeline.body_read_done);

    let quick = quick_parse_request_fields(&full_body);
    let profile = proxy.state.runtime.default_profile();
    let fallback_model = profile.fallback_model.clone();
    ctx.model = quick
        .model
        .filter(|m| !m.is_empty())
        .unwrap_or(fallback_model);
    ctx.is_streaming = quick.stream.unwrap_or(false);

    ctx.conversation_id = quick.conversation_id.or(conversation_id_from_header);

    ctx.prompt_cache_key = quick.prompt_cache_key;

    let client_ip = session
        .client_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    // Build a minimal HeaderMap with only the headers needed for affinity extraction,
    // avoiding a full HeaderMap clone of all request headers.
    // Headers must match extract_affinity_key() — see its doc comment.
    let mut affinity_headers = HeaderMap::with_capacity(3);
    let req_hdrs = &session.req_header().headers;
    for hdr in ["x-conversation-id", "x-prompt-cache-key", "x-user-id"] {
        if let Some(v) = req_hdrs.get(hdr) {
            affinity_headers.insert(hdr, v.clone());
        }
    }
    ctx.upstream.affinity_key = Some(extract_affinity_key(
        &affinity_headers,
        &client_ip,
        ctx.conversation_id.as_deref(),
        ctx.prompt_cache_key.as_deref(),
        ctx.project_id.as_deref(),
        ctx.session_fingerprint.as_deref(),
    ));

    let pipeline_globals = proxy.state.runtime.pipeline_globals();
    let model_alias_entry = pipeline_globals.cursor_models.resolve(&ctx.model);
    let alias_upstream_model = model_alias_entry.map(|e| e.upstream.as_str());
    let model_alias_pipeline = model_alias_entry.map(|e| e.pipeline);
    let alias_hit = model_alias_entry.is_some();

    let selection = if !skip_early_pipeline_select {
        let pipe_ctx = PipelineRequestContext {
            model: &ctx.model,
            payload: ctx.parsed_request_payload.as_deref(),
            key_pipeline: key_pipeline.as_deref().map(PipelineOverride::from_str),
            key_upstream_profile: key_upstream_profile.as_deref(),
            domain_pipeline: domain_pipeline.as_deref().map(PipelineOverride::from_str),
            domain_upstream_profile: domain_upstream_profile.as_deref(),
            conversation_id_header: ctx.conversation_id.as_deref(),
            user_agent: user_agent.as_deref(),
            alias_upstream_model,
            model_alias_pipeline,
        };
        let selection = select_request_pipeline(
            &pipeline_globals,
            &proxy.state.runtime.profile_descriptors(),
            &pipe_ctx,
        );

        // #region agent log
        debug_agent_log(
            "SEL1",
            "proxy.rs:request_filter",
            "pipeline/profile selection",
            serde_json::json!({
                "request_id": ctx.request_id,
                "model": ctx.model,
                "alias_hit": alias_hit,
                "alias_upstream_model": alias_upstream_model,
                "alias_pipeline": model_alias_pipeline.map(|p| p.as_str()),
                "key_pipeline": pipe_ctx.key_pipeline.map(|p| p.as_str()),
                "key_upstream_profile": pipe_ctx.key_upstream_profile,
                "domain_pipeline": pipe_ctx.domain_pipeline.map(|p| p.as_str()),
                "domain_upstream_profile": pipe_ctx.domain_upstream_profile,
                "selected_profile": selection.upstream_profile_id,
                "selected_provider": selection.provider.as_str(),
                "selected_pipeline": selection.pipeline.as_str(),
                "selected_reason": selection.reason.as_str(),
            }),
        );
        // #endregion

        if let Some(msg) = validate_pipeline_override(
            pipe_ctx
                .key_pipeline
                .or(pipe_ctx.domain_pipeline)
                .unwrap_or(PipelineOverride::Auto),
            selection.provider,
        ) {
            let body = serde_json::json!({
                "error": { "message": msg, "type": "invalid_pipeline", "code": "invalid_pipeline" }
            });
            let body_str = body.to_string();
            warn!(
                request_id = %ctx.request_id,
                model = %ctx.model,
                message = %msg,
                "Rejecting request: invalid pipeline override for upstream profile"
            );
            if !send_json_error(session, http::StatusCode::BAD_REQUEST, body_str.as_bytes()).await {
                let _ = session.respond_error(400).await;
            }
            return Ok(true);
        }

        ctx.request_pipeline = Some(selection.pipeline);
        ctx.pipeline_reason = Some(selection.reason);
        ctx.upstream_profile_id = Some(selection.upstream_profile_id.clone());

        global_metrics().record_pipeline_selected(
            selection.pipeline.as_str(),
            &selection.upstream_profile_id,
            selection.reason.as_str(),
        );
        timeline_stamp(&mut ctx.timeline.pipeline_select_done);
        selection
    } else {
        let profile = proxy.active_upstream_profile(ctx);
        PipelineSelection {
            pipeline: ctx
                .request_pipeline
                .expect("streaming defer sets request_pipeline"),
            upstream_profile_id: profile.id.clone(),
            provider: profile.provider,
            reason: ctx
                .pipeline_reason
                .expect("streaming defer sets pipeline_reason"),
        }
    };

    let reasoning_cfg_early = proxy.reasoning_config();
    let skip_early_exact = skip_early_pipeline_select;
    if !skip_early_exact
        && (GatewayProxy::is_mimo_pipeline(selection.pipeline)
            || matches!(selection.pipeline, RequestPipeline::GenericRelay))
    {
        let fingerprint_early = proxy.state.runtime.fingerprint.read().clone();
        let cache_namespace_early = effective_cache_namespace(
            proxy.state.cache_key_namespace.as_deref(),
            ctx.project_id.as_deref(),
        );
        if let Ok(early_key) = crab_cache::generate_namespaced_cache_key_with_fingerprint(
            &full_body,
            cache_namespace_early.as_deref(),
            &fingerprint_early,
        ) {
            ctx.cache_key = Some(early_key.clone());
            if proxy
                .try_early_exact_cache(
                    session,
                    ctx,
                    &early_key,
                    reasoning_cfg_early.display_reasoning,
                )
                .await?
            {
                return Ok(true);
            }
        }
    }

    // Stable session id (ReasoningStore + session store): conv > pck > sk-cc > req hash.
    let client_session = client_session_from_authorization(ctx.authorization.as_deref());
    let client_session_for_log = client_session.clone();
    let stable_session_buf = ctx
        .conversation_id
        .clone()
        .or(ctx.prompt_cache_key.clone())
        .or(client_session)
        .or_else(|| {
            ctx.req_hash
                .as_ref()
                .map(|h| format!("req:{}", &h[..h.len().min(16)]))
        });
    let stable_session = stable_session_buf.as_deref();

    // ─── Phase 4: Preparation (reasoning preprocessing, composition extraction) ───
    let active_profile = proxy.active_upstream_profile(ctx);
    let upstream_base_url = active_profile.base_url.clone();
    let profile_fallback = active_profile.fallback_model.clone();
    let reasoning_cfg = proxy.reasoning_config();
    ctx.cached_reasoning_config = reasoning_cfg.clone();

    let (stable_session_kind, stable_session_prefix) = stable_session_log_fields(
        ctx.conversation_id.as_deref(),
        ctx.prompt_cache_key.as_deref(),
        client_session_for_log.as_deref(),
        ctx.req_hash.as_deref(),
    );
    ctx.stable_session_kind = Some(stable_session_kind.to_string());

    let direct_mimo = mimo_direct_passthrough(selection.pipeline);
    let mut parse_elapsed = std::time::Duration::default();
    let mut reject_missing = false;
    let mut patched = 0usize;
    let mut missing = 0usize;
    let mut recovered = 0usize;
    let mut retired_prefix = 0usize;
    #[allow(unused_assignments)]
    let mut upstream_model_log = ctx.model.clone();
    let mut namespace_preview = String::new();
    let effective_user_id = ctx.project_id.clone();

    let prepare_start = Instant::now();
    if direct_mimo {
        let body = if ctx.is_streaming {
            Bytes::from(
                crate::phases::upstream_request::inject_stream_options_include_usage(
                    full_body.to_vec(),
                ),
            )
        } else {
            full_body.clone()
        };
        ctx.new_request_body = Some(body);
        ctx.upstream_body_for_capture = Some(full_body.clone());
    } else {
        let mut parsed_payload =
            match proxy.ensure_client_payload(ctx, &full_body, Some(selection.pipeline.as_str())) {
                Ok(p) => p,
                Err(parse_err) => {
                    let preview: String = full_body
                        .iter()
                        .take(32)
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    warn!(
                        request_id = %ctx.request_id,
                        model = %ctx.model,
                        body_len = full_body.len(),
                        body_prefix_hex = %preview,
                        parse_error = %parse_err,
                        "Rejecting request: client JSON parse failed"
                    );
                    let body = serde_json::json!({
                        "error": {
                            "message": format!("Invalid JSON in request body: {parse_err}"),
                            "type": "invalid_request_error",
                            "code": "invalid_json"
                        }
                    });
                    let body_str = body.to_string();
                    if !send_json_error(session, http::StatusCode::BAD_REQUEST, body_str.as_bytes())
                        .await
                    {
                        let _ = session.respond_error(400).await;
                    }
                    return Ok(true);
                }
            };
        parse_elapsed = ctx
            .timeline
            .json_parse_done
            .map(|t| t.duration_since(ctx.request_start))
            .unwrap_or_default();
        if ctx.session_fingerprint.is_none() {
            ctx.session_fingerprint =
                crab_capture::session_fingerprint_from_payload(parsed_payload.as_ref());
        }
        refresh_affinity_key(
            ctx,
            &affinity_headers,
            &client_ip,
            body_user_from_payload(parsed_payload.as_ref()),
        );

        if GatewayProxy::is_mimo_pipeline(selection.pipeline)
            && let Some(store) = &proxy.state.session_store
        {
            let cache_namespace = effective_cache_namespace(
                proxy.state.cache_key_namespace.as_deref(),
                ctx.project_id.as_deref(),
            );
            crate::session_store::apply_mimo_session_store(
                store,
                &proxy.state.features,
                ctx,
                &mut parsed_payload,
                stable_session,
                cache_namespace.as_deref(),
            )
            .await;
        }

        let payload = parsed_payload.as_ref();

        match selection.pipeline {
            RequestPipeline::CursorDeepSeekV4 => {
                let prepared = prepare_upstream_request(
                    payload,
                    Some(&proxy.state.reasoning_store),
                    &upstream_base_url,
                    &profile_fallback,
                    &reasoning_cfg.thinking_mode,
                    &reasoning_cfg.reasoning_effort,
                    &reasoning_cfg.missing_reasoning_strategy,
                    reasoning_cfg.context_summary_message_threshold,
                    reasoning_cfg.prefix_validate,
                    ctx.authorization.as_deref(),
                    stable_session,
                    alias_upstream_model,
                    effective_user_id.as_deref(),
                );
                patched = prepared.patched_reasoning_messages;
                missing = prepared.missing_reasoning_messages;
                recovered = prepared.recovered_reasoning_messages;
                retired_prefix = prepared.retired_prefix_messages;
                upstream_model_log = prepared.upstream_model.clone();
                namespace_preview = prepared.cache_namespace.chars().take(8).collect();
                reject_missing = missing > 0 && reasoning_cfg.missing_reasoning_strategy == "reject";
                ctx.stream.pending_recovery_notice = prepared.recovery_notice.clone();
                ctx.retired_prefix_messages = Some(prepared.retired_prefix_messages);
                ctx.prepared_request = Some(prepared.clone());
                ctx.new_request_body = Some(Bytes::from(
                    serde_json::to_vec(&prepared.payload).unwrap_or_default(),
                ));
                if ctx.is_streaming {
                    ctx.stream.accumulator = Some(StreamAccumulator::new());
                    ctx.stream.display_adapter = reasoning_cfg.display_reasoning.then(|| {
                        CursorReasoningDisplayAdapter::new(reasoning_cfg.collapsible_reasoning)
                    });
                }
            }
            RequestPipeline::DeepSeekLight => {
                let light = prepare_light_request(
                    payload,
                    &profile_fallback,
                    alias_upstream_model,
                    effective_user_id.as_deref(),
                );
                upstream_model_log = light.upstream_model.clone();
                ctx.new_request_body = Some(Bytes::from(
                    serde_json::to_vec(&light.payload).unwrap_or_default(),
                ));
            }
            RequestPipeline::GenericRelay => {
                let generic = prepare_generic_request(payload);
                upstream_model_log = generic.model.clone();
                ctx.new_request_body = Some(Bytes::from(
                    serde_json::to_vec(&generic.payload).unwrap_or_default(),
                ));
            }
            RequestPipeline::MimoTokenPlanRelay
            | RequestPipeline::MimoPaygRelay => {
                let features = &proxy.state.features;
                let mimo = prepare_mimo_request(
                    payload,
                    &profile_fallback,
                    features.mimo_retire_prefix_messages,
                    features.mimo_keep_recent_turns,
                );
                retired_prefix = mimo.retired_prefix_messages;
                ctx.retired_prefix_messages = Some(mimo.retired_prefix_messages);
                upstream_model_log = mimo.model.clone();
                ctx.parsed_upstream_payload = Some(Arc::new(mimo.payload));
                ctx.new_request_body = mimo.serialized_body.map(Bytes::from);
            }
            RequestPipeline::CodexRelay => {
                let model = alias_upstream_model
                    .filter(|m| !m.is_empty())
                    .unwrap_or(ctx.model.as_str());
                let prepared = crate::codex::prepare_codex_request(payload, model);
                upstream_model_log = prepared.model.clone();
                ctx.parsed_upstream_payload = Some(Arc::new(prepared.payload.clone()));
                ctx.new_request_body = Some(Bytes::from(
                    serde_json::to_vec(&prepared.payload).unwrap_or_default(),
                ));
            }
        }
    }
    let prepare_elapsed = prepare_start.elapsed();
    global_metrics().record_request_body_stage(
        "prepare_upstream_body",
        prepare_elapsed,
        full_body.len(),
        Some(selection.pipeline.as_str()),
    );
    ctx.upstream_model = Some(upstream_model_log.clone());
    if selection.provider == UpstreamProvider::Deepseek
        && let Some(user_id) = ctx.project_id.as_deref()
        && let Some(tier) = classify_deepseek_v4_tier(&upstream_model_log)
    {
        match proxy
            .state
            .deepseek_user_id_limiter
            .try_acquire(user_id, tier)
        {
            Ok(guard) => ctx.deepseek_user_id_guard = Some(guard),
            Err(DeepSeekUserIdLimitError::Exceeded) => {
                global_metrics().record_deepseek_user_id_concurrency_rejected(tier.as_str());
                global_metrics().record_rejected("deepseek_user_concurrency_exceeded");
                let body = deepseek_user_concurrency_exceeded_error_json();
                if !send_json_error(
                    session,
                    http::StatusCode::TOO_MANY_REQUESTS,
                    body.as_slice(),
                )
                .await
                {
                    let _ = session.respond_error(429).await;
                }
                return Ok(true);
            }
        }
    }

    info!(
        request_id = %ctx.request_id,
        pipeline = %selection.pipeline.as_str(),
        upstream_profile = %selection.upstream_profile_id,
        pipeline_reason = %selection.reason.as_str(),
        client_model = %ctx.model,
        model = %ctx.model,
        upstream_model = %upstream_model_log,
        alias_hit = alias_hit,
        patched = patched,
        missing = missing,
        recovered = recovered,
        retired_prefix = retired_prefix,
        reasoning_strategy = %reasoning_cfg.missing_reasoning_strategy,
        cache_namespace = %namespace_preview,
        stable_session_kind = %stable_session_kind,
        stable_session_prefix = ?stable_session_prefix,
        consumer = ?ctx.consumer,
        "Prepared upstream request"
    );

    // #region agent log
    let req_hash_short = ctx
        .req_hash
        .as_deref()
        .map(|h| h.chars().take(8).collect::<String>());
    let payload_for_logs = ctx.parsed_request_payload.as_deref();
    let message_count = payload_for_logs
        .and_then(|payload| payload.get("messages"))
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    debug_agent_log(
        "P1",
        "proxy.rs:request_filter",
        "reasoning prepare summary",
        serde_json::json!({
            "request_id": ctx.request_id,
            "req_hash": req_hash_short,
            "message_count": message_count,
            "last_user_fp": payload_for_logs.and_then(last_user_message_fingerprint),
            "stable_session_kind": stable_session_kind,
            "stable_session_prefix": stable_session_prefix,
            "missing": missing,
            "patched": patched,
            "recovered": recovered,
            "retired_prefix": retired_prefix,
            "recovery_notice_prepared": ctx.stream.pending_recovery_notice.is_some(),
            "strategy": reasoning_cfg.missing_reasoning_strategy,
            "reject_missing": reject_missing,
            "pipeline": selection.pipeline.as_str(),
            "retry_buffer_truncated": ctx.upstream.retry_buffer_truncated,
        }),
    );
    // #endregion

    if reject_missing {
        warn!(
            request_id = %ctx.request_id,
            missing = missing,
            "Strict missing-reasoning mode rejected request"
        );
        let body = missing_reasoning_error_json(missing);
        // #region agent log
        debug_agent_log(
            "RM",
            "proxy.rs:request_filter",
            "rejected missing reasoning before upstream",
            serde_json::json!({
                "request_id": ctx.request_id,
                "missing": missing,
                "status": 409,
            }),
        );
        // #endregion
        if !send_json_error(session, http::StatusCode::CONFLICT, &body).await {
            let _ = session.respond_error(409).await;
        }
        return Ok(true);
    }

    // #region agent log
    debug_agent_log(
        "H-B",
        "proxy.rs:request_filter",
        "recovery notice gate",
        serde_json::json!({
            "request_id": ctx.request_id,
            "req_hash": req_hash_short,
            "recovery_notice_prepared": ctx.stream.pending_recovery_notice.is_some(),
            "pending_recovery_notice": ctx.stream.pending_recovery_notice.is_some(),
        }),
    );
    // #endregion

    let new_body = ctx
        .new_request_body
        .clone()
        .unwrap_or_else(|| full_body.clone());
    ctx.upstream_outbound_body_len = new_body.len();
    if ctx.parsed_upstream_payload.is_none() && !direct_mimo {
        let upstream_parse_start = Instant::now();
        ctx.parsed_upstream_payload =
            serde_json::from_slice::<serde_json::Value>(new_body.as_ref())
                .ok()
                .map(Arc::new);
        global_metrics().record_request_body_stage(
            "json_parse_upstream",
            upstream_parse_start.elapsed(),
            ctx.upstream_outbound_body_len,
            Some(selection.pipeline.as_str()),
        );
    }
    // #region agent log
    if crate::is_debug_agent_log_enabled() {
        let outbound_fp: String = {
            let mut hasher = Sha256::new();
            hasher.update(new_body.as_ref());
            let h = hex::encode(hasher.finalize());
            h[..h.len().min(8)].to_string()
        };
        let upstream_msg_count = ctx
            .parsed_upstream_payload
            .as_ref()
            .and_then(|v| {
                v.get("messages")
                    .and_then(|m| m.as_array())
                    .map(|a| a.len())
            })
            .unwrap_or(0);
        debug_agent_log(
            "H-G",
            "proxy.rs:request_filter",
            "upstream context size (stagnation check)",
            serde_json::json!({
                "request_id": ctx.request_id,
                "req_hash": req_hash_short,
                "message_count": message_count,
                "upstream_msg_count": upstream_msg_count,
                "inbound_bytes": ctx.content_length,
                "outbound_bytes": ctx.upstream_outbound_body_len,
                "outbound_fp": outbound_fp,
                "last_user_fp": payload_for_logs.and_then(last_user_message_fingerprint),
                "recovered": recovered,
                "retired_prefix": retired_prefix,
            }),
        );
    }
    // #endregion
    ctx.new_request_body = Some(new_body.clone());
    ctx.upstream_body_for_capture = Some(new_body);
    ctx.upstream.retry_buffer_truncated = session.retry_buffer_truncated();
    if ctx.upstream.retry_buffer_truncated {
        debug_agent_log(
            "H4",
            "proxy.rs:request_filter",
            "retry buffer truncated; upstream body will use request_body_filter",
            serde_json::json!({
                "request_id": ctx.request_id,
                "inbound_bytes": ctx.content_length,
                "outbound_bytes": ctx.upstream_outbound_body_len,
            }),
        );
    }
    // #region agent log
    if ctx.upstream_outbound_body_len > 50_000 {
        debug_agent_log(
            "E",
            "proxy.rs:request_filter",
            "large upstream outbound body",
            serde_json::json!({
                "request_id": ctx.request_id,
                "inbound_bytes": ctx.content_length,
                "outbound_bytes": ctx.upstream_outbound_body_len,
                "model": ctx.model,
                "is_streaming": ctx.is_streaming,
            }),
        );
    }
    if ctx.upstream_outbound_body_len >= 256 * 1024 {
        debug_agent_log(
            "PB",
            "proxy.rs:request_filter",
            "request body performance sample",
            serde_json::json!({
                "request_id": ctx.request_id,
                "pipeline": selection.pipeline.as_str(),
                "inbound_bytes": ctx.content_length,
                "outbound_bytes": ctx.upstream_outbound_body_len,
                "client_parse_ms": parse_elapsed.as_secs_f64() * 1000.0,
                "prepare_ms": prepare_elapsed.as_secs_f64() * 1000.0,
            }),
        );
    }
    // #endregion

    // ─── Phase 5: Cache & Coalesce (key generation, L0/L1/L2 lookup, coalescing) ───
    match crate::phases::cache_coalesce::run(proxy, session, ctx).await? {
        crate::phases::cache_coalesce::CachePhaseOutcome::Return(done) => {
            return Ok(done);
        }
        crate::phases::cache_coalesce::CachePhaseOutcome::Continue => {}
    }

    if !proxy.try_acquire_upstream_key(ctx) {
        let pool = proxy.active_upstream_profile(ctx).resolve_upstream_pool();
        let failure = pool.diagnose_acquire_failure();
        let (body, error_code, retry_after) = upstream_pool_exhausted_error_details(failure);
        // #region agent log
        debug_agent_log(
            "H-K",
            "proxy.rs:request_filter",
            "upstream key pool exhausted",
            serde_json::json!({
                "request_id": ctx.request_id,
                "elapsed_since_start_ms": ctx.request_start.elapsed().as_millis(),
                "upstream_profile": ctx.upstream_profile_id,
                "pool_total": pool.len(),
                "pool_available": pool.available_count(),
                "failure_reason": format!("{:?}", failure),
                "error_code": error_code,
                "retry_after_secs": retry_after,
                "project_id_set": ctx.project_id.is_some(),
            }),
        );
        // #endregion
        if !send_json_error_with_retry_after(
            session,
            http::StatusCode::SERVICE_UNAVAILABLE,
            &body,
            retry_after,
        )
        .await
        {
            let _ = session.respond_error(503).await;
        }
        return Ok(true);
    }

    // #region agent log
    debug_agent_log(
        "H-OUT",
        "proxy.rs:request_filter",
        "request_filter returning false (proxy upstream)",
        serde_json::json!({
            "request_id": ctx.request_id,
            "elapsed_since_start_ms": ctx.request_start.elapsed().as_millis(),
            "pipeline": ctx.request_pipeline.map(|p| p.as_str()),
            "has_prepared": ctx.prepared_request.is_some(),
            "new_body_len": ctx.new_request_body.as_ref().map(|b| b.len()),
            "outbound_bytes": ctx.upstream_outbound_body_len,
        }),
    );
    // #endregion

    // ── Connection pre-warm for new session fingerprints ──────────
    // Connection pre-warm trigger is now in upstream_peer (after Ketama selection).
    // This ensures we construct the correct HttpPeer with the real backend address.

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{
        mimo_direct_passthrough, request_passthrough_allowed_pipeline,
    };
    use crab_pipeline::RequestPipeline;

    #[test]
    fn mimo_pipelines_use_direct_passthrough() {
        assert!(mimo_direct_passthrough(RequestPipeline::MimoTokenPlanRelay));
        assert!(mimo_direct_passthrough(RequestPipeline::MimoTokenPlanRelay));
        assert!(mimo_direct_passthrough(RequestPipeline::MimoPaygRelay));
        assert!(!mimo_direct_passthrough(RequestPipeline::GenericRelay));
        assert!(!mimo_direct_passthrough(RequestPipeline::CursorDeepSeekV4));
    }

    #[test]
    fn mimo_pipelines_allow_request_passthrough() {
        assert!(request_passthrough_allowed_pipeline(
            RequestPipeline::MimoTokenPlanRelay
        ));
        assert!(request_passthrough_allowed_pipeline(
            RequestPipeline::MimoTokenPlanRelay
        ));
        assert!(request_passthrough_allowed_pipeline(
            RequestPipeline::MimoPaygRelay
        ));
        assert!(!request_passthrough_allowed_pipeline(
            RequestPipeline::GenericRelay
        ));
    }
}

/// Early pipeline select on a partial body so direct MiMo relay can start before EOS.
fn try_arm_mimo_request_passthrough_on_partial_body(
    proxy: &GatewayProxy,
    session: &mut Session,
    ctx: &mut GatewayContext,
    partial_body: &[u8],
    req_path: &str,
    req_method: &http::Method,
    user_agent: &Option<String>,
    key_pipeline: &Option<String>,
    key_upstream_profile: &Option<String>,
    domain_pipeline: &Option<String>,
    domain_upstream_profile: &Option<String>,
) -> bool {
    const MIN_PASSTHROUGH_PREFIX_BYTES: usize = 1024;
    if !crate::streaming_body_forward::feature_enabled(proxy) {
        return false;
    }
    if !crate::streaming_body_forward::path_eligible(req_path, req_method) {
        return false;
    }
    if session.is_body_done() || partial_body.len() < MIN_PASSTHROUGH_PREFIX_BYTES {
        return false;
    }
    {
        let Ok(text) = std::str::from_utf8(partial_body) else {
            return false;
        };
        if text.trim().is_empty() || !text.contains("\"model\"") {
            return false;
        }
    }
    let quick = quick_parse_request_fields(partial_body);
    let Some(model) = quick.model.filter(|m| !m.is_empty()) else {
        return false;
    };
    ctx.model = model;
    ctx.is_streaming = quick.stream.unwrap_or(false);
    ctx.conversation_id = quick
        .conversation_id
        .clone()
        .or_else(|| ctx.conversation_id.clone());
    ctx.prompt_cache_key = quick.prompt_cache_key.or(ctx.prompt_cache_key.clone());

    let pipeline_globals = proxy.state.runtime.pipeline_globals();
    let model_alias_entry = pipeline_globals.cursor_models.resolve(&ctx.model);
    let pipe_ctx = PipelineRequestContext {
        model: &ctx.model,
        payload: None,
        key_pipeline: key_pipeline.as_deref().map(PipelineOverride::from_str),
        key_upstream_profile: key_upstream_profile.as_deref(),
        domain_pipeline: domain_pipeline.as_deref().map(PipelineOverride::from_str),
        domain_upstream_profile: domain_upstream_profile.as_deref(),
        conversation_id_header: ctx.conversation_id.as_deref(),
        user_agent: user_agent.as_deref(),
        alias_upstream_model: model_alias_entry.map(|e| e.upstream.as_str()),
        model_alias_pipeline: model_alias_entry.map(|e| e.pipeline),
    };
    let selection = select_request_pipeline(
        &pipeline_globals,
        &proxy.state.runtime.profile_descriptors(),
        &pipe_ctx,
    );
    if !request_passthrough_allowed_pipeline(selection.pipeline) {
        return false;
    }
    ctx.request_pipeline = Some(selection.pipeline);
    ctx.pipeline_reason = Some(selection.reason);
    ctx.upstream_profile_id = Some(selection.upstream_profile_id);
    ctx.upstream_model = Some(ctx.model.clone());
    global_metrics().record_pipeline_selected(
        selection.pipeline.as_str(),
        ctx.upstream_profile_id.as_deref().unwrap_or("default"),
        selection.reason.as_str(),
    );
    let (stable_kind, _stable_prefix) = stable_session_log_fields(
        ctx.conversation_id.as_deref(),
        ctx.prompt_cache_key.as_deref(),
        None,
        None,
    );
    ctx.stable_session_kind = Some(stable_kind.to_string());
    timeline_stamp(&mut ctx.timeline.pipeline_select_done);
    true
}

/// Direct MiMo body relay: only the sniffed prefix is buffered locally, later chunks pass through.
async fn request_passthrough_handoff(
    proxy: &GatewayProxy,
    session: &mut Session,
    ctx: &mut GatewayContext,
    partial_body: Vec<u8>,
) -> Result<bool> {
    if partial_body.is_empty() {
        let _ = session.respond_error(400).await;
        return Ok(true);
    }
    ctx.request_passthrough.active = true;
    ctx.request_passthrough.armed_prefix_len = partial_body.len();
    ctx.request_passthrough.prefix_emitted = false;
    ctx.request_passthrough.finalized = false;
    ctx.request_passthrough.body_hasher = Some(Sha256::new());
    crate::helper_fns::passthrough_hash_update(ctx, &partial_body);
    ctx.request_passthrough.buffer = partial_body;
    ctx.upstream.retry_budget = 0;
    global_metrics().record_request_passthrough_total();
    ctx.content_length = ctx
        .request_passthrough
        .inbound_content_length
        .unwrap_or(ctx.request_passthrough.buffer.len());
    ctx.upstream_outbound_body_len = 0;
    // Exact-cache is intentionally disabled on this path; bodies are not buffered.
    // Store prefix body for trace logging (request_messages_snapshot will contain first 1KB+).
    ctx.original_request_body = Some(Bytes::from(ctx.request_passthrough.buffer.clone()));
    ctx.parsed_request_payload = None;
    ctx.upstream_body_for_capture = None;
    ctx.parsed_upstream_payload = None;
    if !proxy.try_acquire_upstream_key(ctx) {
        let pool = proxy.active_upstream_profile(ctx).resolve_upstream_pool();
        let failure = pool.diagnose_acquire_failure();
        let (body, _code, retry_after) = upstream_pool_exhausted_error_details(failure);
        if !send_json_error_with_retry_after(
            session,
            http::StatusCode::SERVICE_UNAVAILABLE,
            &body,
            retry_after,
        )
        .await
        {
            let _ = session.respond_error(503).await;
        }
        return Ok(true);
    }
    // #region agent log
    info!(
        request_id = %ctx.request_id,
        pipeline = ?ctx.request_pipeline,
        upstream_profile = ctx.upstream_profile_id.as_deref().unwrap_or("default"),
        pipeline_reason = ctx.pipeline_reason.map(|r| r.as_str()).unwrap_or(""),
        client_model = %ctx.model,
        upstream_model = ctx.upstream_model.as_deref().unwrap_or(""),
        consumer = ctx.consumer.as_deref().unwrap_or(""),
        prefix_len = ctx.request_passthrough.armed_prefix_len,
        is_streaming = ctx.is_streaming,
        "MiMo passthrough handoff armed"
    );
    debug_agent_log(
        "PT-HANDOFF",
        "request_filter.rs:request_passthrough_handoff",
        "MiMo passthrough handoff armed — prefix relay to upstream",
        serde_json::json!({
            "request_id": ctx.request_id,
            "pipeline": ctx.request_pipeline,
            "upstream_profile": ctx.upstream_profile_id,
            "pipeline_reason": ctx.pipeline_reason,
            "model": ctx.model,
            "upstream_model": ctx.upstream_model,
            "consumer": ctx.consumer,
            "prefix_len": ctx.request_passthrough.armed_prefix_len,
            "is_streaming": ctx.is_streaming,
            "content_length": ctx.content_length,
        }),
    );
    // #endregion
    Ok(false)
}

/// Run the full request_filter phase (Phases 1–5 + upstream key acquisition).
///
/// Returns `Ok(true)` if the request was fully handled (e.g. cache hit, error response),
/// `Ok(false)` if the request should proceed to upstream.
pub(crate) async fn run(
    proxy: &GatewayProxy,
    session: &mut Session,
    ctx: &mut GatewayContext,
) -> Result<bool> {
    // ─── Phase 1: Routing Gate (CORS, health, models, path/method validation) ───

    // Detect SWR revalidation subrequests: skip cache lookup, force upstream.
    if crate::cache_revalidate::is_revalidation_subrequest(session).is_some() {
        debug!(
            request_id = %ctx.request_id,
            "SWR revalidation subrequest detected, skipping cache lookup"
        );
        ctx.cache_key = None;
        // Fall through to normal request processing (auth already in session headers).
    }

    if proxy.state.cors_enabled
        && session.req_header().method == http::Method::OPTIONS
        && send_cors_preflight(session).await
    {
        return Ok(true);
    }

    let req_path = session.req_header().uri.path().to_string();
    let req_method = session.req_header().method.clone();

    {
        let hdr = session.req_header();
        if let Some(url) = crab_client_endpoint::url_from_forwarded_headers(
            hdr.headers.get("host").and_then(|v| v.to_str().ok()),
            hdr.headers
                .get("x-forwarded-host")
                .and_then(|v| v.to_str().ok()),
            hdr.headers
                .get("x-forwarded-proto")
                .and_then(|v| v.to_str().ok()),
            hdr.headers
                .get("x-forwarded-port")
                .and_then(|v| v.to_str().ok()),
        ) {
            let mut snap = proxy.state.client_endpoint.write();
            snap.gateway_url_public = Some(url);
            snap.public_source = Some(crab_client_endpoint::PublicUrlSource::Observed);
        }
    }

    let auth = session
        .req_header()
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let provided_key = auth.strip_prefix("Bearer ").unwrap_or(&auth).to_string();
    // #region agent log
    debug_agent_log(
        "AUTH1",
        "proxy.rs:request_filter",
        "auth header extracted",
        serde_json::json!({
            "request_id": ctx.request_id,
            "path": req_path,
            "method": format!("{}", req_method),
            "auth_present": !auth.is_empty(),
            "is_bearer": auth.starts_with("Bearer "),
            "provided_key_len": provided_key.len(),
            "looks_like_sk_cc": provided_key.starts_with("sk-cc-"),
        }),
    );
    // #endregion
    let conversation_id_from_header = session
        .req_header()
        .headers
        .get("x-conversation-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let user_agent = session
        .req_header()
        .headers
        .get(http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let consumer_from_header = session
        .req_header()
        .headers
        .get("x-consumer")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if req_path == "/health" || req_path == "/healthz" || req_path == "/v1/healthz" {
        let _ = session.respond_error(200).await;
        return Ok(true);
    }

    if req_path == "/ready" {
        let redis_ok = proxy.state.tiered_cache.ping().await;
        // Readiness includes upstream key availability: if all keys are exhausted
        // (e.g. rate-limited or disabled), the gateway cannot serve requests.
        // This intentionally reports 503 so load balancers route traffic elsewhere.
        let default_profile = proxy.state.runtime.default_profile();
        let pool = default_profile.resolve_upstream_pool();
        let keys_available = pool.available_count() > 0;
        let status = if redis_ok && keys_available { 200 } else { 503 };
        let _ = session.respond_error(status).await;
        return Ok(true);
    }

    if is_models_endpoint(&req_path, &req_method) {
        let (is_authorized, consumer_from_key, domain_from_key, _, _, key_profile) =
            proxy.authorize_client(&provided_key, &auth);
        if !is_authorized {
            let _ = session.respond_error(401).await;
            return Ok(true);
        }
        ctx.consumer = consumer_from_key;
        ctx.domain = domain_from_key;
        ctx.upstream_profile_id =
            key_profile.or_else(|| Some(proxy.state.runtime.default_upstream_profile_id()));
        ctx.is_models_list = true;
        let cursor_models = proxy.state.runtime.pipeline_globals().cursor_models;
        if cursor_models.synthetic_models_enabled && !cursor_models.aliases.is_empty() {
            let body = crab_pipeline::synthetic_models_list_json(&cursor_models);
            if send_json_ok(session, &body).await {
                return Ok(true);
            }
            let _ = session.respond_error(500).await;
            return Ok(true);
        }
        if !proxy.try_acquire_upstream_key(ctx) {
            let body = upstream_pool_exhausted_error_json();
            if !send_json_error_with_retry_after(
                session,
                http::StatusCode::SERVICE_UNAVAILABLE,
                &body,
                60,
            )
            .await
            {
                let _ = session.respond_error(503).await;
            }
            return Ok(true);
        }
        return Ok(false);
    }

    if req_path != "/v1/chat/completions" && req_path != "/chat/completions" {
        let _ = session.respond_error(404).await;
        return Ok(true);
    }

    if req_method != http::Method::POST {
        let _ = session.respond_error(405).await;
        return Ok(true);
    }

    // ─── Phase 2: Auth & Limits (key validation, RPM, concurrency, domain quota) ───
    let (
        is_authorized,
        consumer_from_key,
        domain_from_key,
        key_project_id,
        key_pipeline,
        key_upstream_profile,
    ) = proxy.authorize_client(&provided_key, &auth);

    // #region agent log
    debug_agent_log(
        "AUTH2",
        "proxy.rs:request_filter",
        "authorize_client decision",
        serde_json::json!({
            "request_id": ctx.request_id,
            "authorized": is_authorized,
            "stored_key_present": proxy.state.runtime.keys.contains_key(&provided_key),
            "legacy_match_enabled": proxy.state.runtime.legacy_api_key_as_client_auth,
            "key_has_consumer": consumer_from_key.is_some(),
            "key_has_domain": domain_from_key.is_some(),
            "key_has_project_id": key_project_id.is_some(),
            "key_has_pipeline": key_pipeline.is_some(),
            "key_has_upstream_profile": key_upstream_profile.is_some(),
        }),
    );
    // #endregion

    if !is_authorized {
        let _ = session.respond_error(401).await;
        return Ok(true);
    }

    // Per-key RPM rate limiting
    if let Some(stored_key) = proxy.state.runtime.keys.get(&provided_key) {
        let key = stored_key.value();
        if key.rpm_limit > 0
            && !proxy
                .state
                .client_key_rate_limiter
                .check_and_consume(&provided_key, key.rpm_limit)
        {
            let body = serde_json::json!({
                "error": {
                    "message": "Rate limit exceeded for this API key. Please retry after the rate limit resets.",
                    "type": "rate_limit_error",
                    "code": "rate_limit_exceeded"
                }
            });
            let body_str = body.to_string();
            if !send_json_error_with_retry_after(
                session,
                http::StatusCode::TOO_MANY_REQUESTS,
                body_str.as_bytes(),
                60,
            )
            .await
            {
                let _ = session.respond_error(429).await;
            }
            return Ok(true);
        }
    }

    let project_id_header = session
        .req_header()
        .headers
        .get("x-project-id")
        .and_then(|v| v.to_str().ok());

    match resolve_project_id(key_project_id.as_deref(), project_id_header) {
        Ok(mut project_id) => {
            if project_id.is_none()
                && proxy.state.runtime.auto_project_id_from_client_key
                && !provided_key.is_empty()
            {
                match derive_project_id_from_client_key(&provided_key) {
                    Ok(derived) => {
                        debug_agent_log(
                            "H-UID",
                            "proxy.rs:request_filter",
                            "auto project_id from client key",
                            serde_json::json!({
                                "request_id": ctx.request_id,
                                "project_id_prefix": derived.chars().take(12).collect::<String>(),
                            }),
                        );
                        project_id = Some(derived);
                    }
                    Err(e) => {
                        warn!(
                            request_id = %ctx.request_id,
                            error = %e,
                            "auto_project_id_from_client_key failed"
                        );
                    }
                }
            }
            ctx.project_id = project_id;
        }
        Err(ProjectResolveError::Mismatch) => {
            let body = serde_json::json!({
                "error": {
                    "message": "X-Project-Id does not match the project_id bound to this API key",
                    "type": "project_mismatch",
                    "code": "project_mismatch"
                }
            });
            let body_str = body.to_string();
            if !send_json_error(session, http::StatusCode::FORBIDDEN, body_str.as_bytes()).await {
                let _ = session.respond_error(403).await;
            }
            return Ok(true);
        }
        Err(ProjectResolveError::InvalidHeader(msg)) => {
            let body = serde_json::json!({
                "error": {
                    "message": msg,
                    "type": "invalid_project_id",
                    "code": "invalid_project_id"
                }
            });
            let body_str = body.to_string();
            if !send_json_error(session, http::StatusCode::BAD_REQUEST, body_str.as_bytes()).await {
                let _ = session.respond_error(400).await;
            }
            return Ok(true);
        }
    }

    if let Some(stored_key) = proxy.state.runtime.keys.get(&provided_key) {
        match proxy
            .state
            .client_key_limiter
            .try_acquire(&provided_key, stored_key.value())
        {
            Ok(guard) => ctx.client_key_guard = Some(guard),
            Err(ClientKeyLimitError::Exceeded) => {
                global_metrics().record_rejected("client_concurrency_exceeded");
                let body = client_concurrency_exceeded_error_json();
                if !send_json_error(
                    session,
                    http::StatusCode::TOO_MANY_REQUESTS,
                    body.as_slice(),
                )
                .await
                {
                    let _ = session.respond_error(429).await;
                }
                return Ok(true);
            }
        }
    }

    match proxy.state.request_semaphore.clone().try_acquire_owned() {
        Ok(permit) => ctx.request_permit = Some(permit),
        Err(_) => {
            global_metrics().record_rejected("overloaded");
            let _ = session.respond_error(503).await;
            return Ok(true);
        }
    }

    ctx.authorization = Some(auth);
    if !provided_key.is_empty() {
        ctx.client_key_fingerprint = Some(fingerprint_client_key(&provided_key));
    }
    ctx.consumer = consumer_from_key
        .or(consumer_from_header)
        .or_else(|| ctx.project_id.clone());
    ctx.domain = domain_from_key;
    let (domain_pipeline, domain_upstream_profile) =
        proxy.domain_policy_fields(ctx.domain.as_deref());

    if !proxy
        .state
        .runtime
        .domain_within_quota(ctx.domain.as_deref())
    {
        global_metrics().record_rejected("domain_quota_exceeded");
        let _ = session.respond_error(429).await;
        return Ok(true);
    }

    // ─── Phase 3: Request Parse (body read, JSON parse, pipeline selection) ───
    // Do not enable retry buffering here: partial reads for `streaming_body_forward` must not
    // land in Pingora's 64KiB retry buffer (would send incomplete JSON upstream). Pingora
    // enables retry buffering again when the upstream connection starts; full bodies use
    // `request_body_filter` at EOS (see third_party/pingora-proxy/PATCH.md).
    timeline_stamp(&mut ctx.timeline.body_read_start);
    ctx.request_passthrough.inbound_content_length = session
        .req_header()
        .headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let mut full_body = Vec::new();
    let max_body = proxy.state.max_request_body_bytes;
    let mut hasher = Sha256::new();
    loop {
        match session.downstream_session.read_request_body().await? {
            Some(data) => {
                if full_body.len() + data.len() > max_body {
                    global_metrics().record_rejected("body_too_large");
                    let _ = session.respond_error(413).await;
                    return Ok(true);
                }
                hasher.update(&data);
                full_body.extend_from_slice(&data);
            }
            None => break,
        }
        if !ctx.request_passthrough.active
            && try_arm_mimo_request_passthrough_on_partial_body(
                proxy,
                session,
                ctx,
                &full_body,
                &req_path,
                &req_method,
                &user_agent,
                &key_pipeline,
                &key_upstream_profile,
                &domain_pipeline,
                &domain_upstream_profile,
            )
        {
            return request_passthrough_handoff(proxy, session, ctx, full_body).await;
        }
        if session.is_body_done() {
            break;
        }
    }

    if full_body.is_empty() {
        let _ = session.respond_error(400).await;
        return Ok(true);
    }

    let full_body = Bytes::from(full_body);
    return run_post_body_phases(
        proxy,
        session,
        ctx,
        full_body,
        conversation_id_from_header,
        user_agent,
        key_pipeline,
        key_upstream_profile,
        domain_pipeline,
        domain_upstream_profile,
    )
    .await;
}
