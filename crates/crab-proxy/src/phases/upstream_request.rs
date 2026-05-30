//! Phase: upstream request header and body rewriting.
//!
//! Extracted from `proxy.rs` `upstream_request_filter` and `request_body_filter`.

use crate::debug_agent_log;
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crate::upstream_body::apply_prepared_upstream_body;
use crate::upstream_body_compress::maybe_gzip_request_body;
use crate::upstream_headers::{
    apply_upstream_request_content_encoding, normalize_replaced_body_headers,
    prepare_passthrough_upstream_headers, smooth_upstream_client_headers, upstream_header_names,
};
use bytes::Bytes;
use memchr::memmem;
use pingora_core::prelude::*;
use pingora_http::RequestHeader;
use pingora_proxy::Session;
use std::time::Instant;
use tracing::debug;

use crate::codex::apply_codex_upstream_request;
use crate::context::GatewayContext;
use crab_pipeline::RequestPipeline;

/// Run the `upstream_request_filter` phase: inject host/id headers, normalize body framing,
/// inject upstream API key, disable keepalive if configured.
pub(crate) async fn run_upstream_request_filter(
    proxy: &GatewayProxy,
    _session: &mut Session,
    upstream_request: &mut RequestHeader,
    ctx: &mut GatewayContext,
) -> Result<()> {
    let conn_config = proxy.state.runtime.conn_config.read().clone();
    let host = ctx.upstream.host.as_deref().unwrap_or("api.deepseek.com");
    let _ = upstream_request.insert_header("host", host);
    let _ = upstream_request.insert_header("x-request-id", ctx.request_id.clone());
    if ctx.request_passthrough.active {
        prepare_passthrough_upstream_headers(
            upstream_request,
            ctx.is_streaming,
            conn_config.upstream_force_http1,
        );
        // Forward the known Content-Length from the client request to the upstream
        // so the upstream can size the body in advance (avoids chunked transfer-encoding
        // which some upstreams may not handle well for passthrough connections).
        if let Some(len) = ctx.request_passthrough.inbound_content_length {
            let _ = upstream_request.insert_header("content-length", len.to_string());
        }
        upstream_request.set_send_end_stream(false);
    } else if ctx.new_request_body.is_some() {
        let features = &proxy.state.features;
        let raw = ctx.new_request_body.as_ref().unwrap();
        let (payload, gzipped) = maybe_gzip_request_body(
            raw,
            features.upstream_request_gzip,
            features.upstream_request_gzip_min_bytes,
        );
        if gzipped {
            ctx.new_request_body = Some(Bytes::from(payload));
        }
        let new_body = ctx.new_request_body.as_ref().unwrap();
        let had_transfer_encoding = upstream_request
            .headers
            .get(http::header::TRANSFER_ENCODING)
            .is_some();
        normalize_replaced_body_headers(upstream_request, new_body.len());
        if gzipped {
            apply_upstream_request_content_encoding(upstream_request, "gzip");
        }
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

    if ctx.request_pipeline == Some(RequestPipeline::CodexRelay) {
        let account_id = ctx
            .upstream
            .key_guard
            .as_ref()
            .map(|g| g.account_id())
            .unwrap_or("");
        if !account_id.is_empty() && account_id != crate::upstream_pool::DEFAULT_UPSTREAM_ACCOUNT_ID
        {
            let session_id = ctx
                .parsed_upstream_payload
                .as_ref()
                .and_then(|p| p.get("prompt_cache_key"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or(ctx.conversation_id.as_deref())
                .or(ctx.prompt_cache_key.as_deref());
            apply_codex_upstream_request(upstream_request, account_id, ctx.is_streaming, session_id);
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

    if conn_config.upstream_disable_keepalive && conn_config.upstream_force_http1 {
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

/// Inject `stream_options.include_usage: true` into a streaming request body so the upstream
/// API returns token usage in the final SSE chunk. Used by MiMo direct-mimo and request-passthrough
/// paths that relay raw client bodies without going through `prepare_upstream_request`.
///
/// Uses substring-based detection (memchr) instead of a full JSON parse to stay compatible
/// with the zero-buffer passthrough design.
pub(crate) fn inject_stream_options_include_usage(mut body: Vec<u8>) -> Vec<u8> {
    // 1. Fast-path: bail if "stream_options" already present
    if memmem::find(&body, b"\"stream_options\"").is_some() {
        return body;
    }
    // 2. Locate "stream": true
    let stream_key = b"\"stream\":";
    let Some(pos) = memmem::find(&body, stream_key) else {
        return body;
    };
    let after = &body[pos + stream_key.len()..];
    let trimmed = after
        .iter()
        .copied()
        .skip_while(|&b| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r')
        .collect::<Vec<_>>();
    if !trimmed.starts_with(b"true") {
        return body;
    }
    // 3. Find first '{' (skip leading whitespace) and inject after it
    let brace_pos = body
        .iter()
        .position(|&b| b == b'{')
        .unwrap_or(body.len().saturating_sub(1));
    if brace_pos >= body.len().saturating_sub(1) {
        return body;
    }
    let insert_at = brace_pos + 1;
    let injection = b"\"stream_options\":{\"include_usage\":true},";
    body.reserve(injection.len());
    body.splice(insert_at..insert_at, injection.iter().copied());
    body
}

/// Run the `request_body_filter` phase: replace upstream body with prepared payload.
pub(crate) async fn run_request_body_filter(
    _proxy: &GatewayProxy,
    session: &mut Session,
    body: &mut Option<Bytes>,
    end_of_stream: bool,
    ctx: &mut GatewayContext,
) -> Result<()> {
    if ctx.request_passthrough.active && !ctx.request_passthrough.finalized {
        let client_done = session.is_body_done();
        if !ctx.request_passthrough.prefix_emitted {
            if let Some(chunk) = body.take() {
                crate::helper_fns::passthrough_hash_update(ctx, &chunk);
                ctx.request_passthrough.buffer.extend_from_slice(&chunk);
            }
            if ctx.request_passthrough.buffer.is_empty() {
                *body = None;
                return Ok(());
            }
            let mut prefix = std::mem::take(&mut ctx.request_passthrough.buffer);
            if ctx.is_streaming {
                prefix = inject_stream_options_include_usage(prefix);
            }
            let prefix = Bytes::from(prefix);
            ctx.request_passthrough.prefix_emitted = true;
            ctx.upstream_outbound_body_len += prefix.len();
            ctx.upstream.prepared_upstream_body_emitted = true;
            if client_done {
                timeline_stamp(&mut ctx.timeline.body_read_done);
                timeline_stamp(&mut ctx.timeline.upstream_body_sent);
                ctx.content_length = ctx
                    .request_passthrough
                    .inbound_content_length
                    .unwrap_or(ctx.upstream_outbound_body_len);
                crate::helper_fns::passthrough_hash_finalize(ctx);
                ctx.request_passthrough.finalized = true;
                ctx.request_passthrough.active = false;
            }
            *body = Some(prefix);
            return Ok(());
        }
        if let Some(chunk) = body.take() {
            crate::helper_fns::passthrough_hash_update(ctx, &chunk);
            ctx.upstream_outbound_body_len += chunk.len();
            // Zero-copy capture for Raw Capture (O(1) refcount bump)
            ctx.request_passthrough
                .captured_client_chunks
                .push(chunk.clone());
            if client_done {
                timeline_stamp(&mut ctx.timeline.body_read_done);
                timeline_stamp(&mut ctx.timeline.upstream_body_sent);
                ctx.content_length = ctx
                    .request_passthrough
                    .inbound_content_length
                    .unwrap_or(ctx.upstream_outbound_body_len);
                crate::helper_fns::passthrough_hash_finalize(ctx);
                ctx.request_passthrough.finalized = true;
                ctx.request_passthrough.active = false;
            }
            *body = Some(chunk);
            return Ok(());
        }
        if client_done {
            timeline_stamp(&mut ctx.timeline.body_read_done);
            crate::helper_fns::passthrough_hash_finalize(ctx);
            ctx.request_passthrough.finalized = true;
            ctx.request_passthrough.active = false;
        }
        *body = None;
        return Ok(());
    }

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
        let force_emit = ctx.upstream.retry_buffer_truncated;
        ctx.new_request_body =
            apply_prepared_upstream_body(new_body, body, end_of_stream, force_emit);
        // #region debug-point D:prepared-body-emit
        debug_agent_log(
            "D",
            "upstream_request.rs:run_request_body_filter",
            "[DEBUG] prepared upstream body emission decision",
            serde_json::json!({
                "request_id": ctx.request_id,
                "end_of_stream": end_of_stream,
                "force_emit": force_emit,
                "retry_buffer_truncated": ctx.upstream.retry_buffer_truncated,
                "prepared_body_now": body.as_ref().map(|b| b.len()),
                "prepared_body_rest": ctx.new_request_body.as_ref().map(|b| b.len()),
                "prepared_body_emitted": ctx.upstream.prepared_upstream_body_emitted,
            }),
        );
        // #endregion
        if emit_now && body.as_ref().is_some_and(|b| !b.is_empty()) {
            ctx.upstream.prepared_upstream_body_emitted = true;
        }
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
