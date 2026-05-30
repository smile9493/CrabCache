//! Phase: upstream peer selection — Ketama affinity routing + connection pre-warm.

use crate::connection_prewarm::spawn_direct_prewarm_if_new_session;
use crate::context::GatewayContext;
use crate::context::BackendRouteStrategy;
use crate::debug_agent_log;
use crate::metrics_helpers::timeline_stamp;
use crate::proxy::GatewayProxy;
use crab_metrics::global_metrics;
use crab_route::extract_affinity_key;
use http::HeaderMap;
use pingora_core::prelude::*;
use pingora_proxy::Session;
use sha2::{Digest, Sha256};
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

    let client_ip = ctx
        .client_ip
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            session
                .client_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        });

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
            ctx.conversation_id.as_deref(),
            body_pck,
            ctx.project_id.as_deref(),
            ctx.session_fingerprint.as_deref(),
        )
    });

    let profile = proxy.active_upstream_profile(ctx);
    let router = &profile.router;

    let preferred_backend = if proxy.state.features.read().affinity_prompt_cache_feedback {
        ctx.upstream
            .affinity_key
            .as_deref()
            .and_then(|k| proxy.state.affinity_backend_hints.get(k))
    } else {
        None
    };

    let features = proxy.state.features.read().clone();
    let selected = router.select_with_hint(affinity_key.as_bytes(), preferred_backend.as_deref());
    let max_inflight = features.default_max_inflight_per_backend.max(1);
    let score_weights: crate::backend_state::ScoreWeights = (&features.score_weights).into();
    let mut ranked_backends = rank_backends(
        route_strategy_for(&features),
        &router.ready_backends(),
        &affinity_key,
        selected.as_ref().map(|b| b.name),
        &proxy.state.backend_load,
        &profile.id,
        max_inflight,
        &score_weights,
    );
    if ranked_backends.is_empty()
        && let Some(selected) = selected
    {
        ranked_backends.push(crab_route::Backend::new(
            selected.name.to_string(),
            selected.addr,
            1,
            selected.tls_sni.to_string(),
        ));
    }
    if ranked_backends.is_empty() {
        warn!(
            request_id = %ctx.request_id,
            "No healthy upstream backend available, returning 503"
        );
        return Err(Error::new(ErrorType::ConnectProxyFailure));
    }

    let mut selected_addr = ranked_backends[0].addr;
    let mut selected_name = ranked_backends[0].name.clone();
    let mut selected_tls_sni = ranked_backends[0].tls_sni.clone();

    let prefill_threshold_ms = features.backend_prefill_overload_threshold_ms;
    let mut backend_permit = None;
    let mut overload_state = "ready";
    let mut selected_found = false;
    for candidate in ranked_backends {
        // Skip backends whose circuit breaker is OPEN (not allowing requests).
        if !proxy.state.circuit_breakers.can_execute(&candidate.name).await {
            debug!(
                request_id = %ctx.request_id,
                backend = %candidate.name,
                "Skipping backend with OPEN circuit breaker"
            );
            continue;
        }
        let candidate_state = proxy.state.backend_load.overload_state(
            &profile.id,
            &candidate.name,
            max_inflight,
            prefill_threshold_ms,
        );
        if features.backend_load_aware_routing_enabled && candidate_state != "ready" {
            continue;
        }
        if features.backend_concurrency_limit_enabled {
            let Some(permit) = proxy
                .state
                .backend_load
                .try_acquire(&profile.id, &candidate.name, max_inflight)
            else {
                continue;
            };
            backend_permit = Some(permit);
        }
        selected_addr = candidate.addr;
        selected_tls_sni = candidate.tls_sni;
        selected_name = candidate.name;
        overload_state = candidate_state;
        selected_found = true;
        break;
    }

    if !selected_found {
        if features.backend_concurrency_limit_enabled {
            global_metrics().record_rejected("backend_concurrency_exceeded");
            global_metrics().record_rejection_by_source("backend");
        }
        warn!(
            request_id = %ctx.request_id,
            profile = %profile.id,
            "All ready upstream backends are at concurrency limit"
        );
        return Err(Error::new(ErrorType::ConnectProxyFailure));
    }

    debug!(
        request_id = %ctx.request_id,
        backend = %selected_name,
        affinity_key = %affinity_key,
        "Selected upstream backend"
    );

    global_metrics().set_backend_overload_state(&profile.id, &selected_name, overload_state);
    ctx.backend_permit = backend_permit;
    ctx.upstream.backend_name = Some(selected_name.clone());
    ctx.upstream.backend_overload_state = Some(overload_state.to_string());
    ctx.upstream.host = Some(selected_tls_sni.clone());
    if ctx.upstream.start.is_none() {
        ctx.upstream.start = Some(Instant::now());
    }
    timeline_stamp(&mut ctx.timeline.upstream_connect_done);
    let peer = proxy.create_upstream_peer(selected_addr, &selected_tls_sni, ctx);

    // Trigger direct pool pre-warm for new session fingerprints (same peer options as upstream).
    if proxy.state.features.read().connection_prewarm {
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

fn route_strategy_for(features: &crate::context::FeaturesConfig) -> BackendRouteStrategy {
    features.backend_route_strategy
}

fn rank_backends(
    strategy: BackendRouteStrategy,
    ready: &[crab_route::Backend],
    affinity_key: &str,
    selected_name: Option<&str>,
    backend_load: &crate::backend_state::BackendLoadRegistry,
    profile_id: &str,
    max_inflight: usize,
    score_weights: &crate::backend_state::ScoreWeights,
) -> Vec<crab_route::Backend> {
    let mut backends = ready.to_vec();
    if backends.is_empty() {
        return backends;
    }

    let mut ranked = Vec::with_capacity(backends.len());

    match strategy {
        BackendRouteStrategy::Ketama => {
            if let Some(name) = selected_name
                && let Some(idx) = backends.iter().position(|b| b.name == name)
            {
                ranked.push(backends.remove(idx));
            }
            backends.sort_by(|a, b| {
                let a_key = stable_backend_hash(affinity_key, &a.name);
                let b_key = stable_backend_hash(affinity_key, &b.name);
                a_key.cmp(&b_key).then_with(|| a.name.cmp(&b.name))
            });
            ranked.extend(backends);
        }
        BackendRouteStrategy::P2c => {
            if backends.len() == 1 {
                return backends;
            }
            let first = stable_backend_hash(&format!("{affinity_key}:p2c:first"), affinity_key)
                as usize
                % backends.len();
            let mut second = stable_backend_hash(&format!("{affinity_key}:p2c:second"), affinity_key)
                as usize
                % backends.len();
            if second == first {
                second = (second + 1) % backends.len();
            }
            let a = backends[first].clone();
            let b = backends[second].clone();
            let first_score = backend_score(backend_load, profile_id, &a);
            let second_score = backend_score(backend_load, profile_id, &b);
            let winner = if first_score <= second_score { a } else { b };
            let winner_name = winner.name.clone();
            ranked.push(winner);
            backends.sort_by(|lhs, rhs| {
                backend_score(backend_load, profile_id, lhs)
                    .partial_cmp(&backend_score(backend_load, profile_id, rhs))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| lhs.name.cmp(&rhs.name))
            });
            for backend in backends {
                if backend.name != winner_name {
                    ranked.push(backend);
                }
            }
        }
        BackendRouteStrategy::LeastUsed => {
            backends.sort_by(|a, b| {
                backend_score(backend_load, profile_id, a)
                    .partial_cmp(&backend_score(backend_load, profile_id, b))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.weight.cmp(&a.weight))
                    .then_with(|| a.name.cmp(&b.name))
            });
            ranked.extend(backends);
        }
        BackendRouteStrategy::CostOptimized => {
            backends.sort_by(|a, b| {
                backend_cost_score(backend_load, profile_id, a)
                    .partial_cmp(&backend_cost_score(backend_load, profile_id, b))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.name.cmp(&b.name))
            });
            ranked.extend(backends);
        }
        BackendRouteStrategy::WeightedKetama => {
            backends.sort_by(|a, b| {
                let sa = backend_load.score_backend(profile_id, &a.name, max_inflight, 0, score_weights, true, 0.5);
                let sb = backend_load.score_backend(profile_id, &b.name, max_inflight, 0, score_weights, true, 0.5);
                sb.score
                    .partial_cmp(&sa.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.name.cmp(&b.name))
            });
            ranked.extend(backends);
        }
    }

    ranked
}

fn stable_backend_hash(key: &str, backend: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.update(b"\n");
    hasher.update(backend.as_bytes());
    let digest = hasher.finalize();
    u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]))
}

fn backend_score(
    backend_load: &crate::backend_state::BackendLoadRegistry,
    profile_id: &str,
    backend: &crab_route::Backend,
) -> f64 {
    let inflight = backend_load.inflight(profile_id, &backend.name) as f64;
    inflight / backend.weight.max(1) as f64
}

fn backend_cost_score(
    backend_load: &crate::backend_state::BackendLoadRegistry,
    profile_id: &str,
    backend: &crab_route::Backend,
) -> f64 {
    backend_score(backend_load, profile_id, backend) * 0.9
        + (1.0 / backend.weight.max(1) as f64) * 0.1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(name: &str, port: u16, weight: u32) -> crab_route::Backend {
        crab_route::Backend::new(
            name.to_string(),
            format!("127.0.0.1:{port}").parse().unwrap(),
            weight,
            "localhost".to_string(),
        )
    }

    #[test]
    fn least_used_prefers_lower_inflight_backend() {
        let load = crate::backend_state::BackendLoadRegistry::default();
        let _permit = load.try_acquire("profile", "b1", 8);
        let ranked = rank_backends(
            BackendRouteStrategy::LeastUsed,
            &[backend("b1", 8001, 1), backend("b2", 8002, 4)],
            "affinity-key",
            None,
            &load,
            "profile",
            8,
            &crate::backend_state::DEFAULT_SCORE_WEIGHTS,
        );
        assert_eq!(ranked.first().map(|b| b.name.as_str()), Some("b2"));
    }

    #[test]
    fn cost_optimized_prefers_lower_cost_backend() {
        let load = crate::backend_state::BackendLoadRegistry::default();
        let _permit = load.try_acquire("profile", "b1", 8);
        let ranked = rank_backends(
            BackendRouteStrategy::CostOptimized,
            &[backend("b1", 8001, 1), backend("b2", 8002, 8)],
            "affinity-key",
            None,
            &load,
            "profile",
            8,
            &crate::backend_state::DEFAULT_SCORE_WEIGHTS,
        );
        assert_eq!(ranked.first().map(|b| b.name.as_str()), Some("b2"));
    }
}
