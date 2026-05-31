//! Phase: request_filter — full upstream request validation, preparation, and caching.
//!
//! Extracted from `proxy.rs` `ProxyHttp::request_filter` to keep the trait impl manageable.

use crate::body_quick_parse::quick_parse_request_fields;
use crate::client_key_limiter::ClientKeyLimitError;
use crate::context::GatewayContext;
use crate::error_jsons::{
    client_concurrency_exceeded_error_json, deepseek_user_concurrency_exceeded_error_json,
    missing_reasoning_error_json, upstream_pool_exhausted_error_details,
    upstream_pool_exhausted_error_json,
};
use crate::helper_fns::{
    client_session_from_authorization, extract_client_endpoint_addrs, fingerprint_client_key,
    is_models_endpoint, last_user_message_fingerprint, stable_session_log_fields,
};
use crate::{evaluate_request_guardrails, maybe_handle_cursor_bypass};
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
use std::sync::atomic::Ordering;
use std::time::Instant;
use tracing::{debug, info, warn};

fn mimo_direct_passthrough(pipeline: RequestPipeline) -> bool {
    GatewayProxy::is_mimo_pipeline(pipeline)
}

fn mimo_uses_direct_body_relay(ctx: &GatewayContext, pipeline: RequestPipeline) -> bool {
    mimo_direct_passthrough(pipeline) && !crate::responses_wire::needs_responses_wire_translate(ctx)
}

fn request_passthrough_allowed_pipeline(pipeline: RequestPipeline) -> bool {
    mimo_direct_passthrough(pipeline)
}

fn can_arm_mimo_request_passthrough(stream: Option<bool>) -> bool {
    matches!(stream, Some(true))
}

/// Codex and other Responses API clients require Chat↔Responses translation.
/// Upgrade Chat-Completions upstream pipelines automatically when the client uses `/v1/responses`.
fn upgrade_pipeline_for_responses_client(
    client_wire_api: crate::context::ClientWireApi,
    pipeline: RequestPipeline,
    provider: UpstreamProvider,
) -> RequestPipeline {
    if client_wire_api != crate::context::ClientWireApi::Responses {
        return pipeline;
    }

    match (provider, pipeline) {
        (
            UpstreamProvider::Deepseek,
            RequestPipeline::DeepSeekLight | RequestPipeline::CursorDeepSeekV4,
        ) => RequestPipeline::CodexDeepSeek,
        (
            UpstreamProvider::Mimo,
            RequestPipeline::MimoTokenPlanRelay | RequestPipeline::MimoPaygRelay,
        ) => RequestPipeline::CodexMimo,
        _ => pipeline,
    }
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
    ctx.model = crab_pipeline::canonicalize_client_model(
        &quick
            .model
            .filter(|m| !m.is_empty())
            .unwrap_or(fallback_model),
    );
    ctx.is_streaming = quick.stream.unwrap_or(false);

    ctx.conversation_id = quick.conversation_id.or(conversation_id_from_header);

    ctx.prompt_cache_key = quick.prompt_cache_key;

    let client_ip = ctx
        .client_ip
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            session
                .client_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        });
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

    let mut selection = if !skip_early_pipeline_select {
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
        if proxy.state.features.read().pipeline_overload_degrade_enabled
            && GatewayProxy::is_mimo_pipeline(selection.pipeline)
        {
            let profile = proxy.active_upstream_profile(ctx);
            let features = proxy.state.features.read().clone();
            let ready = profile.router.ready_backends();
            let all_overloaded = !ready.is_empty()
                && ready.iter().all(|b| {
                    !proxy.state.backend_load.is_available(
                        &profile.id,
                        &b.name,
                        features.default_max_inflight_per_backend.max(1),
                        features.backend_prefill_overload_threshold_ms,
                    )
                });
            if all_overloaded {
                global_metrics().record_pipeline_backpressure(
                    selection.pipeline.as_str(),
                    &profile.id,
                    "mimo_backend_overloaded",
                );
                global_metrics().record_rejected("mimo_backend_overloaded");
                global_metrics().record_rejection_by_source("backend");
                let body = serde_json::json!({
                    "error": {
                        "message": "MiMo upstream backends are overloaded. Retry after the backpressure window.",
                        "type": "overloaded",
                        "code": "mimo_backend_overloaded"
                    }
                });
                let body_str = body.to_string();
                if !send_json_error_with_retry_after(
                    session,
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    body_str.as_bytes(),
                    30,
                )
                .await
                {
                    let _ = session.respond_error(503).await;
                }
                return Ok(true);
            }
        }
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

    if let Some(pipe) = ctx.request_pipeline {
        let upgraded =
            upgrade_pipeline_for_responses_client(ctx.client_wire_api, pipe, selection.provider);
        if upgraded != pipe {
            tracing::debug!(
                request_id = %ctx.request_id,
                from = %pipe.as_str(),
                to = %upgraded.as_str(),
                "Upgraded pipeline for Responses API client"
            );
            ctx.request_pipeline = Some(upgraded);
            selection.pipeline = upgraded;
        }
    }

    if maybe_handle_cursor_bypass(proxy, session, ctx).await? {
        return Ok(true);
    }

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

    // Stable session id (ReasoningStore + session store):
    //   conv_id > pck > session_fingerprint > sk-cc > req_hash
    // session_fingerprint is not yet available here (computed after body parse below);
    // it will be injected into stable_session_buf after body parsing completes.
    let client_session = client_session_from_authorization(ctx.authorization.as_deref());
    let client_session_for_log = client_session.clone();
    let mut stable_session_buf = ctx
        .conversation_id
        .clone()
        .or(ctx.prompt_cache_key.clone())
        // client_session and session_fingerprint are populated post-parse below.
        .or_else(|| {
            ctx.req_hash
                .as_ref()
                .map(|h| format!("req:{}", &h[..h.len().min(16)]))
        });
    #[allow(unused_assignments)]
    let mut stable_session = stable_session_buf.as_deref();

    // ─── Phase 4: Preparation (reasoning preprocessing, composition extraction) ───
    let active_profile = proxy.active_upstream_profile(ctx);
    let upstream_base_url = active_profile.base_url.clone();
    let profile_fallback = active_profile.fallback_model.clone();
    let reasoning_cfg = proxy.reasoning_config();
    ctx.cached_reasoning_config = reasoning_cfg.clone();

    let (stable_session_kind, stable_session_prefix) = stable_session_log_fields(
        ctx.conversation_id.as_deref(),
        ctx.prompt_cache_key.as_deref(),
        ctx.session_fingerprint.as_deref(),
        client_session_for_log.as_deref(),
        ctx.req_hash.as_deref(),
    );
    ctx.stable_session_kind = Some(stable_session_kind.to_string());

    let direct_mimo = mimo_uses_direct_body_relay(ctx, selection.pipeline);
    let guardrail_payload = serde_json::from_slice::<serde_json::Value>(full_body.as_ref()).ok();
    let mut parse_elapsed = std::time::Duration::default();
    let mut reject_missing = false;
    let mut patched = 0usize;
    let mut missing = 0usize;
    let mut recovered = 0usize;
    let mut retired_prefix = 0usize;
    #[allow(unused_assignments)]
    let mut upstream_model_log = ctx.model.clone();
    let mut namespace_preview = String::new();
    let mut responses_tool_audit_payload: Option<serde_json::Value> = None;
    let effective_user_id = ctx.project_id.clone();

    let prepare_start = Instant::now();
    if direct_mimo {
        if let Some(payload) = guardrail_payload.as_ref() {
            let guardrail = evaluate_request_guardrails(payload, &session.req_header().headers);
            ctx.guardrail_hits = guardrail.labels.clone();
            ctx.guardrail_blocked = guardrail.blocked;
            if !guardrail.labels.is_empty() {
            }
            if guardrail.blocked {
                let body = serde_json::json!({
                    "error": {
                        "message": guardrail.message.unwrap_or_else(|| "Request rejected by guardrails".to_string()),
                        "type": "invalid_request_error",
                        "code": "guardrail_blocked"
                    }
                });
                let body_str = body.to_string();
                warn!(
                    request_id = %ctx.request_id,
                    model = %ctx.model,
                    "Rejecting request: guardrail block"
                );
                if !send_json_error(session, http::StatusCode::BAD_REQUEST, body_str.as_bytes()).await {
                    let _ = session.respond_error(400).await;
                }
                return Ok(true);
            }
        }
        let mut body = if ctx.is_streaming {
            Bytes::from(
                crate::phases::upstream_request::inject_stream_options_include_usage(
                    full_body.to_vec(),
                    false,
                ),
            )
        } else {
            full_body.clone()
        };
        if crate::responses_wire::needs_responses_wire_translate(ctx) {
            if let Ok(mut payload) = serde_json::from_slice::<serde_json::Value>(&body) {
                let chain_ns = crate::responses_wire::responses_chain_namespace(ctx);
                let wire_target = crate::responses_wire::responses_wire_target(ctx);
                crate::responses_wire::apply_responses_chain(
                    &mut payload,
                    &proxy.state.responses_chain_store,
                    chain_ns,
                )
                .await;
                let chat = crate::responses_wire::responses_payload_to_chat_completions_for(
                    &payload,
                    wire_target,
                );
                ctx.parsed_request_payload = Some(Arc::new(chat.clone()));
                body = Bytes::from(serde_json::to_vec(&chat).unwrap_or_default());
            }
        }
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
        // Refresh stable_session now that session_fingerprint is available.
        // Priority: conv_id > pck > session_fingerprint > client_key > req_hash.
        if stable_session_buf.is_none() {
            if let Some(sfp) = ctx.session_fingerprint.as_deref() {
                stable_session_buf = Some(format!("sfp:{}", sfp));
            }
        }
        stable_session = stable_session_buf.as_deref();
        refresh_affinity_key(
            ctx,
            &affinity_headers,
            &client_ip,
            body_user_from_payload(parsed_payload.as_ref()),
        );
        if ctx.guardrail_hits.is_empty() && !ctx.guardrail_blocked
            && let Some(payload) = guardrail_payload.as_ref()
        {
            let guardrail = evaluate_request_guardrails(payload, &session.req_header().headers);
            ctx.guardrail_hits = guardrail.labels.clone();
            ctx.guardrail_blocked = guardrail.blocked;
            if !guardrail.labels.is_empty() {
            }
            if guardrail.blocked {
                let body = serde_json::json!({
                    "error": {
                        "message": guardrail.message.unwrap_or_else(|| "Request rejected by guardrails".to_string()),
                        "type": "invalid_request_error",
                        "code": "guardrail_blocked"
                    }
                });
                let body_str = body.to_string();
                warn!(
                    request_id = %ctx.request_id,
                    model = %ctx.model,
                    "Rejecting request: guardrail block"
                );
                if !send_json_error(session, http::StatusCode::BAD_REQUEST, body_str.as_bytes()).await {
                    let _ = session.respond_error(400).await;
                }
                return Ok(true);
            }
        }

        // ─── Responses wire (DeepSeek / MiMo only; Codex OAuth passthrough skips this) ───
        if crate::responses_wire::needs_responses_wire_translate(ctx) {
            let chain_ns = crate::responses_wire::responses_chain_namespace(ctx);
            let wire_target = crate::responses_wire::responses_wire_target(ctx);
            let mut wire_payload = parsed_payload.as_ref().clone();
            crate::responses_wire::apply_responses_chain(
                &mut wire_payload,
                &proxy.state.responses_chain_store,
                chain_ns,
            )
            .await;
            responses_tool_audit_payload = Some(wire_payload.clone());
            parsed_payload = Arc::new(
                crate::responses_wire::responses_payload_to_chat_completions_for(
                    &wire_payload,
                    wire_target,
                ),
            );
            ctx.parsed_request_payload = Some(parsed_payload.clone());
        }

        // MiMo Chat Completions clients only — Codex Responses uses ResponsesChainStore.
        if GatewayProxy::mimo_session_store_applies(ctx)
            && let Some(store) = &proxy.state.session_store
        {
            let cache_namespace = effective_cache_namespace(
                proxy.state.cache_key_namespace.as_deref(),
                ctx.project_id.as_deref(),
            );
            let features = proxy.state.features.read().clone();
            crate::session_store::apply_mimo_session_store(
                store.clone(),
                &features,
                ctx,
                &mut parsed_payload,
                stable_session,
                cache_namespace.as_deref(),
            )
            .await;
            ctx.parsed_request_payload = Some(parsed_payload.clone());
        }

        let payload = parsed_payload.as_ref();

        let pipeline = ctx
            .request_pipeline
            .expect("request_pipeline set before upstream prepare");

        match pipeline {
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
                reject_missing =
                    missing > 0 && reasoning_cfg.missing_reasoning_strategy == "reject";
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
            | RequestPipeline::MimoPaygRelay
            | RequestPipeline::CodexMimo => {
                // `parsed_payload` already converted from Responses API when needed (above).
                let features = proxy.state.features.read();
                let mimo = prepare_mimo_request(
                    payload,
                    &profile_fallback,
                    features.mimo_retire_prefix_messages,
                    features.mimo_keep_recent_turns,
                );
                retired_prefix = mimo.retired_prefix_messages;
                ctx.retired_prefix_messages = Some(mimo.retired_prefix_messages);
                upstream_model_log = mimo.model.clone();
                ctx.parsed_upstream_payload = Some(Arc::new(mimo.payload.clone()));
                ctx.new_request_body = mimo.serialized_body.map(Bytes::from);
                let registry_audit = responses_tool_audit_payload
                    .as_ref()
                    .map(crate::responses_tool_registry::audit_responses_tool_registry)
                    .unwrap_or_else(|| {
                        crate::responses_tool_registry::audit_chat_tool_registry(payload)
                    });
                let mimo_audit =
                    crate::responses_tool_registry::audit_mimo_tool_pipeline(payload, &mimo.payload);
                crate::responses_tool_registry::log_mimo_codex_tool_registry_warnings(
                    &ctx.request_id,
                    &ctx.model,
                    &registry_audit,
                    Some(&mimo_audit),
                );
                ctx.stream.responses_exec_only_surface = registry_audit.exec_only_surface;
                ctx.stream.client_responses_tool_names =
                    registry_audit.registered_tool_names.clone();
            }
            RequestPipeline::CodexRelay => {
                let model = alias_upstream_model
                    .filter(|m| !m.is_empty())
                    .unwrap_or(ctx.model.as_str());
                let opts = crate::codex::CodexPrepareOptions {
                    conversation_id: ctx.conversation_id.as_deref(),
                    prompt_cache_key: ctx.prompt_cache_key.as_deref(),
                    stable_session_id: stable_session,
                };
                let prepared = if ctx.client_wire_api == crate::context::ClientWireApi::Responses {
                    crate::codex::CodexTranslator::default()
                        .prepare_client_responses(payload, model, opts)
                } else {
                    crate::codex::CodexTranslator::default()
                        .prepare_chat_request(payload, model, opts)
                };
                upstream_model_log = prepared.model.clone();
                ctx.parsed_upstream_payload = Some(Arc::new(prepared.payload.clone()));
                ctx.new_request_body = Some(Bytes::from(
                    serde_json::to_vec(&prepared.payload).unwrap_or_default(),
                ));
            }
            RequestPipeline::CodexDeepSeek => {
                // DeepSeek upstream for Codex CLI: converted above with DeepSeek wire target.
                let light = prepare_light_request(
                    parsed_payload.as_ref(),
                    &profile_fallback,
                    alias_upstream_model,
                    effective_user_id.as_deref(),
                );
                upstream_model_log = light.upstream_model.clone();
                ctx.new_request_body = Some(Bytes::from(
                    serde_json::to_vec(&light.payload).unwrap_or_default(),
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
                global_metrics().record_rejection_by_source("client");
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


    if reject_missing {
        warn!(
            request_id = %ctx.request_id,
            missing = missing,
            "Strict missing-reasoning mode rejected request"
        );
        let body = missing_reasoning_error_json(missing);
        if !send_json_error(session, http::StatusCode::CONFLICT, &body).await {
            let _ = session.respond_error(409).await;
        }
        return Ok(true);
    }


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
    ctx.upstream.prepared_body_for_retry = Some(new_body.clone());
    ctx.new_request_body = Some(new_body.clone());
    ctx.upstream_body_for_capture = Some(new_body);
    ctx.upstream.retry_buffer_truncated = session.retry_buffer_truncated();
    if ctx.upstream.retry_buffer_truncated {
    }

    // ─── Phase 4.5: Quota Preflight ──────────────────────────────────────
    // Quick profile-level health gate: if every backend in the active profile
    // is in cooldown (recent 429 streaks), short-circuit before cache lookup + coalescing.
    {
        let features = proxy.state.features.read().clone();
        let preflight = &features.preflight;
        if preflight.enabled {
            let profile = proxy.active_upstream_profile(ctx);
            let ready = profile.router.ready_backends();
            if ready.is_empty() {
                global_metrics().record_rejected("preflight_no_ready_backends");
                debug!(
                    request_id = %ctx.request_id,
                    profile = %profile.id,
                    "Preflight: no ready backends in profile router"
                );
                let retry_after = (preflight.cooldown_ms / 1000).max(1);
                let body = r#"{"error":{"message":"upstream_backend_unavailable","type":"server_error","code":503}}"#;
                if !send_json_error_with_retry_after(
                    session,
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    body.as_bytes(),
                    retry_after,
                )
                .await
                {
                    let _ = session.respond_error(503).await;
                }
                return Ok(true);
            }
        }
    }

    // ─── Phase 4.6: MiMo Key Binding Pre-acquire ───────────────────────────
    // Pre-acquire the bound upstream key asynchronously. If the bound key is at
    // capacity (inflight >= max_inflight), wait up to `mimo_key_overflow_wait_ms`
    // before spilling to another key. This protects prefix-cache affinity for
    // Cursor's concurrent agentic requests.
    if ctx.upstream.key_guard.is_none()
        && ctx.request_pipeline.map_or(false, |p| GatewayProxy::is_mimo_pipeline(p))
    {
        // Extract binding info while holding the features read lock, then drop it
        // before the async await (parking_lot RwLockReadGuard is !Send).
        let binding_info: Option<(Arc<crate::key_binding::KeyBindingStore>, String, String, u64)> = {
            let features = proxy.state.features.read();
            if features.mimo_key_binding {
                proxy.state.key_binding_store.as_ref().and_then(|store| {
                    stable_session.and_then(|sid| {
                        store.get(sid).map(|binding| {
                            let timeout_ms = features.mimo_key_overflow_wait_ms;
                            (Arc::clone(store), sid.to_string(), binding.key_id, timeout_ms)
                        })
                    })
                })
            } else {
                None
            }
        };

        if let Some((binding_store, sid, bound_key_id, timeout_ms)) = binding_info {
            let profile = proxy.active_upstream_profile(ctx);
            let pool = profile.resolve_upstream_pool();
            let timeout = std::time::Duration::from_millis(timeout_ms);
            if let Some(guard) =
                pool.acquire_with_binding_async(&bound_key_id, timeout).await
            {
                binding_store.touch(&sid);
                ctx.upstream.key_guard = Some(guard);
                global_metrics().record_key_binding_event("pre_acquire");
                tracing::debug!(
                    request_id = %ctx.request_id,
                    session_id = %sid,
                    key_id = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                    "MiMo key binding pre-acquired in request_filter"
                );
            }
        }
    }

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


    // ── Connection pre-warm for new session fingerprints ──────────
    // Connection pre-warm trigger is now in upstream_peer (after Ketama selection).
    // This ensures we construct the correct HttpPeer with the real backend address.

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{
        can_arm_mimo_request_passthrough, mimo_direct_passthrough,
        request_passthrough_allowed_pipeline,
    };
    use crab_pipeline::RequestPipeline;

    #[test]
    fn mimo_pipelines_use_direct_passthrough() {
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
            RequestPipeline::MimoPaygRelay
        ));
        assert!(!request_passthrough_allowed_pipeline(
            RequestPipeline::GenericRelay
        ));
    }

    #[test]
    fn mimo_passthrough_requires_explicit_stream_true() {
        assert!(can_arm_mimo_request_passthrough(Some(true)));
        assert!(!can_arm_mimo_request_passthrough(Some(false)));
        assert!(!can_arm_mimo_request_passthrough(None));
    }
}

/// Early pipeline select on a partial body so direct MiMo relay can start before EOS.
///
/// # Exact-cache limitation
///
/// The passthrough path skips the exact L0/L1 cache probe because only a small prefix
/// (typically 1024 bytes) is available at this point — not enough to compute a full
/// request-body hash or `cache_key`.  Non-passthrough paths (small bodies that arrive
/// completely in the first read) still go through `run_post_body_phases` → exact cache.
///
/// Future work: at passthrough EOS, when the full body hash is available, we could
/// retroactively probe the cache and short-circuit if a hit is found (unlikely for
/// streaming, but possible for short conversations).
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
    if !crate::streaming_body_forward::feature_enabled(proxy) {
        return false;
    }
    if !crate::streaming_body_forward::path_eligible(req_path, req_method) {
        return false;
    }
    if crate::context::is_client_responses_path(req_path) {
        return false;
    }
    let min_prefix = proxy.state.features.read().passthrough_prefix_bytes;
    if session.is_body_done() || partial_body.len() < min_prefix {
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
    if !can_arm_mimo_request_passthrough(quick.stream) {
        return false;
    }
    ctx.model = crab_pipeline::canonicalize_client_model(&model);
    ctx.is_streaming = true;
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
        ctx.session_fingerprint.as_deref(),
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
    ctx.request_passthrough.prefix_emitted = false;
    ctx.request_passthrough.finalized = false;
    ctx.request_passthrough.body_hasher = Some(Sha256::new());
    crate::helper_fns::passthrough_hash_update(ctx, &partial_body);
    ctx.original_request_body = Some(Bytes::from(partial_body.clone()));
    let before_len = partial_body.len();
    let outbound_prefix =
        crate::phases::upstream_request::inject_stream_options_include_usage(partial_body, true);
    ctx.request_passthrough.outbound_extra_bytes =
        outbound_prefix.len().saturating_sub(before_len);
    ctx.request_passthrough.buffer = outbound_prefix;
    ctx.request_passthrough.armed_prefix_len = ctx.request_passthrough.buffer.len();
    ctx.upstream.retry_budget = 0;
    global_metrics().record_request_passthrough_total();
    ctx.content_length = ctx
        .request_passthrough
        .inbound_content_length
        .map(|len| len.saturating_add(ctx.request_passthrough.outbound_extra_bytes))
        .unwrap_or(ctx.request_passthrough.buffer.len());
    ctx.upstream_outbound_body_len = 0;
    // Exact-cache is intentionally disabled on this path: we only have a 1KB+ prefix,
    // not the full body, so we cannot compute the complete cache key or body hash.
    // Non-passthrough paths (small bodies, full body arrives in first read) still run
    // run_post_body_phases → exact cache.  A future optimisation could retroactively
    // probe the cache at passthrough EOS when the full body hash is available.
    // Store prefix body for trace logging (request_messages_snapshot will contain first 1KB+).
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

    if proxy.state.cors_enabled.load(Ordering::Relaxed)
        && session.req_header().method == http::Method::OPTIONS
        && send_cors_preflight(session).await
    {
        return Ok(true);
    }

    let req_path = session.req_header().uri.path().to_string();
    let req_method = session.req_header().method.clone();

    let client_addrs = extract_client_endpoint_addrs(session);
    ctx.client_ip = Some(client_addrs.client_ip.clone());
    ctx.client_peer_addr = Some(client_addrs.peer_addr);

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
        // Client lockout pre-check for models endpoint.
        let lockout = proxy.state.client_lockouts.check_lockout(&provided_key);
        if lockout.locked {
            let retry_after_secs = (lockout.remaining_ms / 1000).max(1);
            let body = serde_json::json!({
                "error": {
                    "message": format!(
                        "Client locked out due to too many failed attempts. Retry after {}s.",
                        retry_after_secs
                    ),
                    "type": "rate_limit_error",
                    "code": "client_lockout"
                }
            });
            if !send_json_error_with_retry_after(
                session,
                http::StatusCode::TOO_MANY_REQUESTS,
                body.to_string().as_bytes(),
                retry_after_secs,
            )
            .await
            {
                let _ = session.respond_error(429).await;
            }
            return Ok(true);
        }

        let (is_authorized, consumer_from_key, domain_from_key, _, _, key_profile) =
            proxy.authorize_client(&provided_key, &auth);
        if !is_authorized {
            let status = proxy.state.client_lockouts.record_failed_attempt(&provided_key);
            if status.locked {
                global_metrics().record_client_lockout();
                let retry_after_secs = (status.remaining_ms / 1000).max(1);
                let body = serde_json::json!({
                    "error": {
                        "message": format!(
                            "Client locked out due to too many failed attempts. Retry after {}s.",
                            retry_after_secs
                        ),
                        "type": "rate_limit_error",
                        "code": "client_lockout"
                    }
                });
                if !send_json_error_with_retry_after(
                    session,
                    http::StatusCode::TOO_MANY_REQUESTS,
                    body.to_string().as_bytes(),
                    retry_after_secs,
                )
                .await
                {
                    let _ = session.respond_error(429).await;
                }
            } else {
                let _ = session.respond_error(401).await;
            }
            return Ok(true);
        }
        proxy.state.client_lockouts.record_success(&provided_key);
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

    if req_path != "/v1/chat/completions"
        && req_path != "/chat/completions"
        && !crate::context::is_client_responses_path(&req_path)
    {
        let _ = session.respond_error(404).await;
        return Ok(true);
    }

    ctx.client_wire_api = crate::context::ClientWireApi::from_request_path(&req_path);

    if req_method != http::Method::POST {
        let _ = session.respond_error(405).await;
        return Ok(true);
    }

    // ─── Phase 2: Auth & Limits (key validation, RPM, concurrency, domain quota) ───

    // Client lockout pre-check: reject brute-force clients before auth.
    let lockout_status = proxy.state.client_lockouts.check_lockout(&provided_key);
    if lockout_status.locked {
        let retry_after_secs = (lockout_status.remaining_ms / 1000).max(1);
        let body = serde_json::json!({
            "error": {
                "message": format!(
                    "Client locked out due to too many failed attempts. Retry after {}s.",
                    retry_after_secs
                ),
                "type": "rate_limit_error",
                "code": "client_lockout"
            }
        });
        if !send_json_error_with_retry_after(
            session,
            http::StatusCode::TOO_MANY_REQUESTS,
            body.to_string().as_bytes(),
            retry_after_secs,
        )
        .await
        {
            let _ = session.respond_error(429).await;
        }
        return Ok(true);
    }

    let (
        is_authorized,
        consumer_from_key,
        domain_from_key,
        key_project_id,
        key_pipeline,
        key_upstream_profile,
    ) = proxy.authorize_client(&provided_key, &auth);


    if !is_authorized {
        // Record failed attempt for brute-force protection.
        let status = proxy.state.client_lockouts.record_failed_attempt(&provided_key);
        if status.locked {
            global_metrics().record_client_lockout();
            let retry_after_secs = (status.remaining_ms / 1000).max(1);
            let body = serde_json::json!({
                "error": {
                    "message": format!(
                        "Client locked out due to too many failed attempts. Retry after {}s.",
                        retry_after_secs
                    ),
                    "type": "rate_limit_error",
                    "code": "client_lockout"
                }
            });
            if !send_json_error_with_retry_after(
                session,
                http::StatusCode::TOO_MANY_REQUESTS,
                body.to_string().as_bytes(),
                retry_after_secs,
            )
            .await
            {
                let _ = session.respond_error(429).await;
            }
        } else {
            let _ = session.respond_error(401).await;
        }
        return Ok(true);
    }

    // Auth succeeded — clear any failed-attempt history for this client.
    proxy.state.client_lockouts.record_success(&provided_key);

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
                global_metrics().record_rejection_by_source("client");
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
            global_metrics().record_rejection_by_source("client");
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
        global_metrics().record_rejection_by_source("client");
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
    let max_body = proxy.state.max_request_body_bytes.load(Ordering::Relaxed);
    let mut hasher = Sha256::new();
    loop {
        match session.downstream_session.read_request_body().await? {
            Some(data) => {
                if full_body.len() + data.len() > max_body {
                    global_metrics().record_rejected("body_too_large");
                    global_metrics().record_rejection_by_source("client");
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
