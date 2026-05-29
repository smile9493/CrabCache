//! Phase: upstream request header and body rewriting.
//!
//! Extracted from `proxy.rs` `upstream_request_filter` and `request_body_filter`.

use crate::debug_agent_log;
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crate::upstream_body::apply_prepared_upstream_body;
use crate::upstream_headers::{
    normalize_replaced_body_headers, smooth_upstream_client_headers, upstream_header_names,
};
use bytes::Bytes;
use pingora_http::RequestHeader;
use pingora_core::prelude::*;
use pingora_proxy::Session;
use std::time::Instant;
use tracing::debug;

use crate::context::GatewayContext;

/// Run the `upstream_request_filter` phase: inject host/id headers, normalize body framing,
/// inject upstream API key, disable keepalive if configured.
pub(crate) async fn run_upstream_request_filter(
    proxy: &GatewayProxy,
    _session: &mut Session,
    upstream_request: &mut RequestHeader,
    ctx: &mut GatewayContext,
) -> Result<()> {
    let host = ctx.upstream.host.as_deref().unwrap_or("api.deepseek.com");
    let _ = upstream_request.insert_header("host", host);
    let _ = upstream_request.insert_header("x-request-id", ctx.request_id.clone());
    if let Some(ref new_body) = ctx.new_request_body {
        let had_transfer_encoding = upstream_request
            .headers
            .get(http::header::TRANSFER_ENCODING)
            .is_some();
        normalize_replaced_body_headers(upstream_request, new_body.len());
        smooth_upstream_client_headers(upstream_request, ctx.is_streaming);
        // #region agent log
        debug_agent_log(
            "H3",
            "proxy.rs:upstream_request_filter",
            "upstream body framing headers normalized",
            serde_json::json!({
                "request_id": ctx.request_id,
                "body_len": new_body.len(),
                "had_transfer_encoding": had_transfer_encoding,
                "is_streaming": ctx.is_streaming,
                "header_names": upstream_header_names(upstream_request),
            }),
        );
        // #endregion
    }

    // #region agent log
    let had_auth_before = upstream_request
        .headers
        .get(http::header::AUTHORIZATION)
        .is_some();
    debug_agent_log(
        "UPAUTH1",
        "proxy.rs:upstream_request_filter",
        "upstream authorization header state (before upstream key inject)",
        serde_json::json!({
            "request_id": ctx.request_id,
            "had_auth_header_before": had_auth_before,
            "has_upstream_key_guard": ctx.upstream.key_guard.is_some(),
            "key_id": ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
        }),
    );
    // #endregion

    if let Some(guard) = ctx.upstream.key_guard.as_ref() {
        // Xiaomi MiMo Token Plan uses `api-key: tp-...` (not Bearer).
        // Pay-as-you-go keys may still use Bearer semantics depending on upstream.
        let secret = guard.bearer_secret();
        if secret.starts_with("tp-") {
            let _ = upstream_request.remove_header(&http::header::AUTHORIZATION);
            let _ = upstream_request.insert_header("api-key", secret);
        } else {
            let bearer = format!("Bearer {}", secret);
            let _ = upstream_request.insert_header(http::header::AUTHORIZATION, bearer);
        }
    }
    // #region agent log
    let had_auth_after = upstream_request
        .headers
        .get(http::header::AUTHORIZATION)
        .is_some();
    debug_agent_log(
        "UPAUTH2",
        "proxy.rs:upstream_request_filter",
        "upstream authorization header state (after upstream key inject)",
        serde_json::json!({
            "request_id": ctx.request_id,
            "had_auth_header_after": had_auth_after,
            "has_upstream_key_guard": ctx.upstream.key_guard.is_some(),
            "key_id": ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
        }),
    );
    // #endregion

    let conn_config = proxy.state.runtime.conn_config.read().clone();
    if conn_config.upstream_disable_keepalive {
        ctx.upstream.connection_close = true;
        let _ = upstream_request.insert_header(http::header::CONNECTION, "close");
    }

    ctx.upstream_headers_prepared_at = Some(Instant::now());
    timeline_stamp(&mut ctx.timeline.upstream_headers_sent);

    if ctx.new_request_body.is_some() {
        upstream_request.set_send_end_stream(false);
    }

    debug!(
        request_id = %ctx.request_id,
        method = %upstream_request.method,
        uri = %upstream_request.uri,
        headers = ?upstream_request.headers,
        new_body_len = ctx.new_request_body.as_ref().map(|b| b.len()),
        send_end_stream = upstream_request.send_end_stream(),
        "Upstream request debug"
    );

    Ok(())
}

/// Run the `request_body_filter` phase: replace upstream body with prepared payload.
pub(crate) async fn run_request_body_filter(
    _proxy: &GatewayProxy,
    _session: &mut Session,
    body: &mut Option<Bytes>,
    end_of_stream: bool,
    ctx: &mut GatewayContext,
) -> Result<()> {
    if let Some(new_body) = ctx.new_request_body.take() {
        let emit_now = end_of_stream || ctx.upstream.retry_buffer_truncated;
        if emit_now {
            timeline_stamp(&mut ctx.timeline.upstream_body_sent);
            let header_to_body_ms = ctx
                .upstream_headers_prepared_at
                .map(|t| t.elapsed().as_millis())
                .unwrap_or(0);
            debug_agent_log(
                "R1",
                "proxy.rs:request_body_filter",
                "upstream body send after headers",
                serde_json::json!({
                    "request_id": ctx.request_id,
                    "body_len": new_body.len(),
                    "header_to_body_ms": header_to_body_ms,
                    "connection_close": ctx.upstream.connection_close,
                    "retry_buffer_truncated": ctx.upstream.retry_buffer_truncated,
                }),
            );
            debug!(
                request_id = %ctx.request_id,
                body_len = new_body.len(),
                body_preview = %String::from_utf8_lossy(&new_body[..new_body.len().min(500)]),
                header_to_body_ms,
                connection_close = ctx.upstream.connection_close,
                retry_buffer_truncated = ctx.upstream.retry_buffer_truncated,
                "Setting upstream request body"
            );
        }
        ctx.new_request_body = apply_prepared_upstream_body(
            new_body,
            body,
            end_of_stream,
            ctx.upstream.retry_buffer_truncated,
        );
    } else {
        debug!(
            request_id = %ctx.request_id,
            body_len = body.as_ref().map(|b| b.len()),
            end_of_stream,
            "request_body_filter: no modified body"
        );
    }
    Ok(())
}
