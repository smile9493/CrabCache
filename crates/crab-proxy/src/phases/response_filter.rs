//! Phase: response_filter — upstream response header processing.
//!
//! Extracted from `proxy.rs` `ProxyHttp::response_filter`.

use crate::context::GatewayContext;
use crate::debug_agent_log;
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crate::upstream_response_decompress::parse_content_encoding;
use crab_metrics::global_metrics;
use http::header;
use pingora_core::ErrorType;
use pingora_http::ResponseHeader;
use pingora_proxy::Session;
use tracing::warn;

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
    global_metrics().record_upstream_response_status(status, ctx.request_pipeline.map(|p| p.as_str()));
    // #region agent log
    debug_agent_log(
        "UP-SEEN",
        "proxy.rs:response_filter",
        "upstream response header received",
        serde_json::json!({
            "request_id": ctx.request_id,
            "status": status,
            "is_streaming": ctx.is_streaming,
            "elapsed_since_start_ms": ctx.request_start.elapsed().as_millis(),
        }),
    );
    // #endregion
    if status >= 400 {
        // Record failures toward backend circuit breaker (NOT for 429 — connection-scoped)
        if matches!(status, 408 | 500 | 502 | 503 | 504) {
            if let Some(ref backend_name) = ctx.upstream.backend_name {
                let kind = crate::circuit_breaker::classify_failure(status, None);
                proxy.state.circuit_breakers.on_failure(backend_name, kind).await;
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
        // #region agent log
        debug_agent_log(
            "UP4",
            "proxy.rs:response_filter",
            "upstream non-success status",
            serde_json::json!({
                "request_id": ctx.request_id,
                "status": status,
                "is_streaming": ctx.is_streaming,
                "outbound_bytes": ctx.upstream_outbound_body_len,
            }),
        );
        // #endregion
    }
    let pool = proxy.active_upstream_profile(ctx).resolve_upstream_pool();
    let key_id = ctx
        .upstream
        .key_guard
        .as_ref()
        .map(|g| g.key_id().to_string());

    if status == 429 {
        if let Some(ref id) = key_id {
            // Set cooldown for the 429'd key exactly once.
            pool.report_rate_limited(id);
            global_metrics().record_upstream_key_request(id, "rate_limited");

            tracing::info!(
                request_id = %ctx.request_id,
                key_preview = %id,
                status = 429,
                retry_budget = ctx.upstream.retry_budget,
                "upstream rate limited, attempting key rotation"
            );

            if ctx.upstream.retry_budget > 0 {
                ctx.upstream.retry_budget -= 1;
                // Try a key from a different account_id (cooldown already set above).
                if let Some(new_guard) = pool.acquire_excluding_account(
                    ctx.upstream
                        .key_guard
                        .as_ref()
                        .and_then(|g| {
                            pool.list_status()
                                .into_iter()
                                .find(|s| s.id == g.key_id())
                                .map(|s| s.account_id)
                        })
                        .as_deref(),
                ) {
                    let new_key_id = new_guard.key_id().to_string();
                    ctx.upstream.key_guard = Some(new_guard);
                    global_metrics().record_upstream_key_retry("rate_limited_rotate");

                    // Restore prepared body so request_body_filter can re-emit it on retry.
                    ctx.new_request_body = ctx.upstream.prepared_body_for_retry.clone();
                    // The >= 400 block may have set error_passthrough; clear it for retry.
                    ctx.upstream.error_passthrough = false;
                    // Reset state from the failed attempt so the retry logs cleanly.
                    ctx.upstream.error_body_logged = false;

                    tracing::info!(
                        request_id = %ctx.request_id,
                        new_key_id = %new_key_id,
                        "retrying 429 with new upstream key"
                    );

                    // Return a retryable error — Pingora's retry loop will re-run
                    // upstream_peer → upstream_request_filter → request_body_filter
                    // → response_filter with a fresh upstream connection.
                    let mut e = pingora_core::Error::create(
                        ErrorType::HTTPStatus(429),
                        pingora_core::ErrorSource::Upstream,
                        Some("upstream rate limited, retrying with new key".into()),
                        None,
                    );
                    e.set_retry(true);
                    return Err(e);
                } else {
                    global_metrics().record_upstream_key_retry("cooldown_only");
                }
            }
        }
        if let Some(guard) = &ctx.coalesce_guard
            && guard.is_leader()
        {
            guard.mark_failed();
        }
        return Ok(());
    }
    if status == 401 {
        if let Some(ref id) = key_id {
            pool.report_unauthorized(id);
            global_metrics().record_upstream_key_request(id, "error");
            warn!(upstream_key_id = %id, "Upstream key rejected with 401; disabled");
        }
        return Ok(());
    }

    if let Some(ref id) = key_id {
        global_metrics().record_upstream_key_request(id, "ok");
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

    Ok(())
}
