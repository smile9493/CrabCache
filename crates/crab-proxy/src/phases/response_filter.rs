//! Phase: response_filter — upstream response header processing.
//!
//! Extracted from `proxy.rs` `ProxyHttp::response_filter`.

use crate::context::GatewayContext;
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crate::upstream_response_decompress::parse_content_encoding;
use crab_metrics::global_metrics;
use http::header;
use pingora_http::ResponseHeader;
use pingora_proxy::Session;
use tracing::{debug, warn};

/// Run the response_filter phase: handle status codes, key rotation, coalesce failure marking.
pub(crate) async fn run(
    proxy: &GatewayProxy,
    _session: &mut Session,
    upstream_response: &mut ResponseHeader,
    ctx: &mut GatewayContext,
) -> pingora_core::Result<()> {
    let status = upstream_response.status.as_u16();
    ctx.upstream.http_status = Some(status);
    ctx.upstream.response_decompress.reset();
    if !ctx.is_streaming
        && status == 200
        && upstream_response
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("text/event-stream"))
    {
        // Passthrough may arm before `"stream": true` appears in the request prefix.
        ctx.is_streaming = true;
    }
    if let Some(ce) = upstream_response.headers.get(header::CONTENT_ENCODING) {
        if let Ok(value) = ce.to_str() {
            let parsed = parse_content_encoding(value);
            ctx.upstream.response_decompress.encoding = Some(parsed);
            let _ = upstream_response.remove_header(&header::CONTENT_ENCODING);
        }
    }
    global_metrics().record_http_response(status);
    global_metrics()
        .record_upstream_response_status(status, ctx.request_pipeline.map(|p| p.as_str()));
    if status == 401 {
        if let Some(new_key_id) =
            proxy.rotate_upstream_key_for_same_request_retry(ctx, "unauthorized_rotate", true, None)
        {
            warn!(
                request_id = %ctx.request_id,
                new_key_id = %new_key_id,
                "upstream key rejected with 401; retrying with replacement key"
            );
            let mut e = pingora_core::Error::create(
                pingora_core::ErrorType::HTTPStatus(401),
                pingora_core::ErrorSource::Upstream,
                Some("upstream key unauthorized (401), retrying with replacement key".into()),
                None,
            );
            e.set_retry(true);
            return Err(e);
        }
    }
    if status >= 400 {
        // Record failures toward backend circuit breaker (NOT for 429 — connection-scoped)
        if matches!(status, 408 | 500 | 502 | 503 | 504) {
            if let Some(ref backend_name) = ctx.upstream.backend_name {
                let kind = crate::circuit_breaker::classify_failure(status, None);
                proxy
                    .state
                    .circuit_breakers
                    .on_failure(backend_name, kind)
                    .await;

                // Fallback policy: classify error for structured cooldown decision.
                let retry_after_hdr = upstream_response
                    .headers
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok());
                let decision =
                    crate::fallback_policy::check_fallback_error(status, None, retry_after_hdr);
                if decision.should_fallback {
                    debug!(
                        request_id = %ctx.request_id,
                        status,
                        reason = %decision.reason,
                        cooldown_ms = decision.cooldown.as_millis(),
                        "Fallback decision from upstream error"
                    );
                    global_metrics().record_fallback_decision(match decision.failure_kind {
                        crate::circuit_breaker::FailureKind::RateLimit => "rate_limit",
                        crate::circuit_breaker::FailureKind::QuotaExhausted => "quota_exhausted",
                        crate::circuit_breaker::FailureKind::Transient => "transient",
                    });
                }

                // Model-level lockout: exponential backoff for transient failures.
                if let Some(ref model) = ctx.upstream_model {
                    let profile_id = ctx.upstream_profile_id.as_deref().unwrap_or("default");
                    proxy.state.model_lockouts.record_failure(
                        profile_id,
                        backend_name,
                        model,
                        &decision.reason,
                        decision.cooldown,
                        4,
                    );
                    global_metrics().record_model_lockout(profile_id, backend_name, model);
                }
            }
        }

        warn!(
            request_id = %ctx.request_id,
            status,
            is_streaming = ctx.is_streaming,
            outbound_bytes = ctx.upstream_outbound_body_len,
            model = %ctx.model,
            pipeline = ?ctx.request_pipeline,
            upstream_profile = ?ctx.upstream_profile_id,
            "Upstream returned error status"
        );
        if ctx.is_streaming {
            // Client asked for SSE; upstream may return JSON with Content-Length.
            // Rewrite body as SSE in body_filter; use 200 + event-stream so Cursor surfaces the error.
            ctx.upstream.error_passthrough = true;
            upstream_response.status = http::StatusCode::OK;
            let _ = upstream_response.remove_header(&header::CONTENT_LENGTH);
            let _ = upstream_response.remove_header(&header::TRANSFER_ENCODING);
            let _ = upstream_response.insert_header(header::CONTENT_TYPE, "text/event-stream");
            let _ = upstream_response.insert_header(header::CACHE_CONTROL, "no-cache");
        }
    }
    let pool = proxy.active_upstream_profile(ctx).resolve_upstream_pool();
    if let Err(retry_err) = crate::phases::rate_limit_retry::try_upstream_rate_limit_rotation(
        proxy,
        ctx,
        upstream_response,
        status,
        None,
        &pool,
    ) {
        return Err(retry_err);
    }
    let key_id = ctx
        .upstream
        .key_guard
        .as_ref()
        .map(|g| g.key_id().to_string());

    if status == 429 {
        return Ok(());
    }
    if status == 401 {
        if let Some(ref id) = key_id {
            pool.report_unauthorized(id);
            global_metrics().record_upstream_key_request(id, "error");
            warn!(upstream_key_id = %id, "Upstream key rejected with 401; disabled");
        }
        // Model-level lockout for 401: long cooldown (unauthorized = key invalid for this model).
        if let Some(ref model) = ctx.upstream_model {
            let profile_id = ctx.upstream_profile_id.as_deref().unwrap_or("default");
            if let Some(ref backend_name) = ctx.upstream.backend_name {
                proxy.state.model_lockouts.record_failure(
                    profile_id,
                    backend_name,
                    model,
                    "upstream key unauthorized",
                    std::time::Duration::from_secs(86400),
                    4,
                );
                global_metrics().record_model_lockout(profile_id, backend_name, model);
            }
        }
        return Ok(());
    }

    if let Some(ref id) = key_id {
        global_metrics().record_upstream_key_request(id, "ok");
        pool.record_key_success(id);
        if let Some(ref binding_store) = proxy.state.key_binding_store {
            let stable_session = ctx
                .conversation_id
                .as_deref()
                .or(ctx.prompt_cache_key.as_deref())
                .or(ctx.session_fingerprint.as_deref())
                .or(ctx.client_key_fingerprint.as_deref());
            if let Some(sid) = stable_session {
                binding_store.reset_failures(sid, id);
                binding_store.reset_failures(
                    &crate::key_binding::KeyBindingStore::codex_session_key(sid),
                    id,
                );
            }
        }
        let _ = upstream_response.insert_header("x-upstream-key-id", id.clone());
    }

    if ctx.is_models_list {
        return Ok(());
    }

    if status >= 400 {
        if let Some(ref id) = key_id {
            global_metrics().record_upstream_key_request(id, "error");
        }
        if let Some(guard) = &ctx.coalesce_guard
            && guard.is_leader()
        {
            guard.mark_failed();
        }
        return Ok(());
    }

    let _ = upstream_response.insert_header("x-request-id", ctx.request_id.clone());
    let _ = upstream_response.insert_header("x-cache-status", "miss");
    if ctx.is_streaming {
        // OpenResty/nginx: disable response buffering so Codex sees SSE keepalives immediately.
        let _ = upstream_response.insert_header("X-Accel-Buffering", "no");
    }

    // Record circuit breaker success for non-streaming responses (status < 400).
    // Streaming success is deferred to the logging phase to avoid counting
    // incomplete SSE streams as successes.
    if !ctx.is_streaming {
        if let Some(ref backend_name) = ctx.upstream.backend_name {
            proxy.state.circuit_breakers.on_success(backend_name).await;
        }
    }

    let now = std::time::Instant::now();
    if ctx.upstream.headers_at.is_none() {
        ctx.upstream.headers_at = Some(now);
    }
    timeline_stamp(&mut ctx.timeline.upstream_response_headers);
    timeline_stamp(&mut ctx.timeline.prefill_done);

    if status == 200
        && ctx.is_streaming
        && crate::responses_wire::needs_responses_wire_translate(ctx)
    {
        let model = ctx.model.clone();
        crate::responses_wire::arm_responses_wire_stream(ctx, &model);
    }

    Ok(())
}
