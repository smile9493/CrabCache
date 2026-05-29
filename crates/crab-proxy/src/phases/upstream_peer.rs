//! Phase: upstream peer selection — Ketama affinity routing + connection pre-warm.

use crate::connection_prewarm::spawn_direct_prewarm_if_new_session;
use crate::context::GatewayContext;
use crate::debug_agent_log;
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crab_route::extract_affinity_key;
use http::HeaderMap;
use pingora_core::prelude::*;
use pingora_proxy::Session;
use std::time::Instant;
use tracing::{debug, warn};

pub(crate) async fn run(
    proxy: &GatewayProxy,
    session: &mut Session,
    ctx: &mut GatewayContext,
) -> Result<Box<HttpPeer>> {
    if ctx.is_models_list {
        let profile = proxy.active_upstream_profile(ctx);
        let router = &profile.router;
        let selected = router
            .select(b"models")
            .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?;

        ctx.upstream.affinity_key = Some("models".to_string());
        ctx.upstream.backend_name = Some(selected.name.to_string());
        ctx.upstream.host = Some(selected.tls_sni.to_string());
        let peer = proxy.create_upstream_peer(selected.addr, selected.tls_sni, ctx);
        let conn_config = proxy.state.runtime.conn_config.read().clone();
        // #region agent log
        debug_agent_log(
            "H1",
            "phases/upstream_peer",
            "upstream peer options (models)",
            serde_json::json!({
                "request_id": ctx.request_id,
                "alpn": format!("{}", peer.options.alpn),
                "force_http1": conn_config.upstream_force_http1,
                "read_timeout_secs": conn_config.upstream_request_timeout_secs,
                "write_timeout_secs": conn_config.upstream_write_timeout_secs,
            }),
        );
        // #endregion
        return Ok(Box::new(peer));
    }

    let req_header = session.req_header();

    let client_ip = session
        .client_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();

    // Build minimal HeaderMap for affinity fallback (only needed headers).
    // Headers must match extract_affinity_key() — see its doc comment.
    let mut affinity_headers_fallback = HeaderMap::with_capacity(3);
    for hdr in ["x-conversation-id", "x-prompt-cache-key", "x-user-id"] {
        if let Some(v) = req_header.headers.get(hdr) {
            affinity_headers_fallback.insert(hdr, v.clone());
        }
    }

    // Reuse affinity key from request_filter if available (avoids redundant SHA-256 hash)
    let body_pck = ctx.prompt_cache_key.as_deref();
    let affinity_key = ctx.upstream.affinity_key.clone().unwrap_or_else(|| {
        extract_affinity_key(
            &affinity_headers_fallback,
            &client_ip,
            body_pck,
            ctx.project_id.as_deref(),
            ctx.session_fingerprint.as_deref(),
        )
    });

    let profile = proxy.active_upstream_profile(ctx);
    let router = &profile.router;

    let preferred_backend = if proxy.state.features.affinity_prompt_cache_feedback {
        ctx.upstream
            .affinity_key
            .as_deref()
            .and_then(|k| proxy.state.affinity_backend_hints.get(k))
    } else {
        None
    };

    let selected = router
        .select_with_hint(affinity_key.as_bytes(), preferred_backend.as_deref())
        .ok_or_else(|| {
            warn!(
                request_id = %ctx.request_id,
                "No healthy upstream backend available, returning 503"
            );
            Error::new(ErrorType::ConnectProxyFailure)
        })?;

    debug!(
        request_id = %ctx.request_id,
        backend = %selected.name,
        affinity_key = %affinity_key,
        "Selected upstream backend"
    );

    ctx.upstream.backend_name = Some(selected.name.to_string());
    ctx.upstream.host = Some(selected.tls_sni.to_string());
    if ctx.upstream.start.is_none() {
        ctx.upstream.start = Some(Instant::now());
    }
    timeline_stamp(&mut ctx.timeline.upstream_connect_done);
    let backend_addr = selected.addr;
    let backend_tls_sni = selected.tls_sni.to_string();
    let peer = proxy.create_upstream_peer(backend_addr, &backend_tls_sni, ctx);

    // Trigger direct pool pre-warm for new session fingerprints (same peer options as upstream).
    if proxy.state.features.connection_prewarm {
        if let Some(sfp) = ctx.session_fingerprint.as_deref() {
            spawn_direct_prewarm_if_new_session(
                &proxy.state.upstream_connector.read().clone(),
                &proxy.state.seen_session_fingerprints,
                sfp,
                peer.clone(),
                proxy.state.prewarm_semaphore.clone(),
            );
        }
    }

    let conn_config = proxy.state.runtime.conn_config.read().clone();
    // #region agent log
    debug_agent_log(
        "H1",
        "phases/upstream_peer",
        "upstream peer options (chat)",
        serde_json::json!({
            "request_id": ctx.request_id,
            "alpn": format!("{}", peer.options.alpn),
            "force_http1": conn_config.upstream_force_http1,
            "disable_keepalive": conn_config.upstream_disable_keepalive,
            "tls_curves": conn_config.upstream_tls_curves,
            "outbound_bytes": ctx.upstream_outbound_body_len,
            "read_timeout_secs": conn_config.upstream_request_timeout_secs,
            "write_timeout_secs": conn_config.upstream_write_timeout_secs,
        }),
    );
    // #endregion

    Ok(Box::new(peer))
}
