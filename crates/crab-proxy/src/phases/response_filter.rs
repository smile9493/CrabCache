//! Phase: response_filter — upstream response header processing.
//!
//! Extracted from `proxy.rs` `ProxyHttp::response_filter`.

use crate::context::GatewayContext;
use crate::debug_agent_log;
use crate::proxy::GatewayProxy;
use crab_metrics::global_metrics;
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
    global_metrics().record_http_response(status);
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
                    ctx.upstream.key_guard = Some(new_guard);
                    global_metrics().record_upstream_key_retry("rate_limited_rotate");
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

    // Record success is deferred to logging phase (stream completion)
    // to avoid counting incomplete SSE streams as successes

    ctx.upstream.start = Some(std::time::Instant::now());

    Ok(())
}
