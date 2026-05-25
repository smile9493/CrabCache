use crate::client_key_limiter::ClientKeyLimitError;
use crate::context::{ConnectionConfig, GatewayContext, GatewayState, ReasoningConfig};
use crate::send_helpers::{send_cors_preflight, send_json_error, send_json_error_with_retry_after, send_json_ok};
use crate::error_jsons::{
    client_concurrency_exceeded_error_json, deepseek_user_concurrency_exceeded_error_json,
    upstream_pool_exhausted_error_details,
};
use crate::upstream_user_id_limiter::{
    DeepSeekUserIdLimitError, classify_deepseek_v4_tier,
};
use crate::tenant::{
    ProjectResolveError, derive_project_id_from_client_key, effective_cache_namespace,
    resolve_project_id,
};
use crate::runtime::RuntimeConfig;
use crate::debug_agent_log;
use crate::upstream_body::apply_prepared_upstream_body;
use crate::upstream_headers::{
    normalize_replaced_body_headers, smooth_upstream_client_headers, upstream_header_names,
};
use crate::upstream_pool::UpstreamKeyPool;
use pingora_core::upstreams::peer::ALPN;
use crate::sse::{UsageData, parse_sse_chunk};
use crate::trace_logger::SanitizedLogEntry;
use crate::trace_logger::composition_debug_tx;
use crate::user_id_audit::apply_user_id_audit_to_entry;
use crab_cache::CoalesceError;
use crab_composition::{
    extract_composition, extract_system_text, extract_tools_json, CompositionDebugEntry,
    CompositionHints,
};
use crab_metrics::{CacheTier, global_metrics};
use crab_pipeline::{
    PipelineOverride, PipelineRequestContext, RequestPipeline, UpstreamProvider,
    select_request_pipeline, validate_pipeline_override,
};
use crab_reasoning::{
    CursorReasoningDisplayAdapter, ReasoningBackend, StreamAccumulator,
    prepare_generic_request, prepare_light_request, prepare_upstream_request,
    rewrite_response_body, rewrite_sse_chunk, sanitize_client_completion,
};
use crate::cache_helpers::{
    build_cache_entry, build_cache_entry_with_sse, build_semantic_query_text,
    cache_entry_matches_stream_mode, prepare_response_body_for_cache, should_store_sse_body,
};
use crate::cache_response::{
    completion_json_has_visible_client_content, send_cached_response,
};
use crate::sse_rewrite::apply_silent_strip_to_sse_chunk;
use crab_route::{CircuitState, extract_affinity_key};
use crab_semantic::{GateDecision, evaluate_semantic_gate};
use http::HeaderMap;
use pingora_core::prelude::*;
use pingora_core::protocols::l4::ext::TcpKeepalive;
use pingora_core::upstreams::peer::PeerOptions;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

fn sanitize_for_trace(value: Option<&str>) -> Option<String> {
    value.map(|s| {
        if s.len() > 64 {
            format!("{}...<truncated>", &s[..32])
        } else {
            s.to_string()
        }
    })
}

/// Labels which stable ReasoningStore scope source is active (for ops / Cursor sub-agent debugging).
fn last_user_message_fingerprint(payload: &serde_json::Value) -> Option<String> {
    let messages = payload.get("messages")?.as_array()?;
    let content = messages.iter().rev().find_map(|m| {
        if m.get("role")?.as_str()? != "user" {
            return None;
        }
        match m.get("content") {
            Some(serde_json::Value::String(s)) => Some(s.as_str()),
            _ => None,
        }
    })?;
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = hex::encode(hasher.finalize());
    Some(hash[..hash.len().min(8)].to_string())
}

fn client_session_from_authorization(authorization: Option<&str>) -> Option<String> {
    let auth = authorization?;
    let token = auth.strip_prefix("Bearer ").unwrap_or(auth).trim();
    if token.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let hash = hex::encode(hasher.finalize());
    Some(format!("client:{}", &hash[..hash.len().min(16)]))
}

fn stable_session_log_fields(
    conversation_id: Option<&str>,
    prompt_cache_key: Option<&str>,
    client_session: Option<&str>,
    req_hash: Option<&str>,
) -> (&'static str, Option<String>) {
    fn prefix8(s: &str) -> String {
        s.chars().take(8).collect()
    }
    if conversation_id.is_some_and(|s| !s.trim().is_empty()) {
        return ("conversation", conversation_id.map(prefix8));
    }
    if prompt_cache_key.is_some_and(|s| !s.trim().is_empty()) {
        return ("prompt_cache_key", prompt_cache_key.map(prefix8));
    }
    if client_session.is_some_and(|s| !s.trim().is_empty()) {
        return ("client_key", client_session.map(prefix8));
    }
    if let Some(hash) = req_hash.filter(|s| !s.trim().is_empty()) {
        let short: String = hash.chars().take(8).collect();
        return ("req_hash", Some(short));
    }
    ("message_scope", None)
}

pub struct GatewayProxy {
    state: Arc<GatewayState>,
}

impl GatewayProxy {
    pub fn new(state: Arc<GatewayState>) -> Self {
        Self { state }
    }

    fn reasoning_config(&self) -> ReasoningConfig {
        self.state.reasoning_config.read().clone()
    }

    fn authorize_client(
        &self,
        provided_key: &str,
        auth: &str,
    ) -> (
        bool,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        if let Some(stored_key) = self.state.runtime.keys.get(provided_key) {
            let key = stored_key.value();
            (
                key.enabled,
                Some(key.name.clone()),
                key.domain.clone(),
                key.project_id.clone(),
                key.pipeline.clone(),
                key.upstream_profile.clone(),
            )
        } else if self
            .state
            .runtime
            .is_legacy_client_token(provided_key, auth)
        {
            (true, None, None, None, None, None)
        } else {
            (false, None, None, None, None, None)
        }
    }

    fn domain_policy_fields(&self, domain: Option<&str>) -> (Option<String>, Option<String>) {
        let label = RuntimeConfig::effective_domain_label(domain);
        self.state
            .runtime
            .domain_policies
            .read()
            .get(label)
            .cloned()
            .map(|p| (p.pipeline.clone(), p.upstream_profile.clone()))
            .unwrap_or((None, None))
    }

    fn active_upstream_profile(&self, ctx: &GatewayContext) -> Arc<crate::upstream_profile::UpstreamProfileRuntime> {
        ctx.upstream_profile_id
            .as_deref()
            .and_then(|id| self.state.runtime.profile(id))
            .unwrap_or_else(|| self.state.runtime.default_profile())
    }

    fn create_upstream_peer(
        &self,
        backend: &crab_route::Backend,
        ctx: &mut GatewayContext,
    ) -> HttpPeer {
        ctx.upstream.host = Some(backend.tls_sni.clone());
        let mut peer = HttpPeer::new(backend.addr, true, backend.tls_sni.clone());
        let conn_config = self.state.runtime.conn_config.read().clone();
        apply_connection_options(&conn_config, &mut peer.options);
        peer
    }

    fn try_acquire_upstream_key(&self, ctx: &mut GatewayContext) -> bool {
        if ctx.upstream.key_guard.is_some() {
            return true;
        }
        let pool = self.active_upstream_profile(ctx).resolve_upstream_pool();
        match pool.acquire() {
            Some(guard) => {
                ctx.upstream.miss = true;
                ctx.upstream.key_guard = Some(guard);
                true
            }
            None => {
                global_metrics().record_rejected("upstream_key_exhausted");
                false
            }
        }
    }

    /// Attempt L2 semantic cache lookup. Returns `true` if the response was served
    /// from the semantic cache (including error responses), `false` if no hit.
    async fn try_l2_semantic_cache(
        &self,
        session: &mut Session,
        ctx: &mut GatewayContext,
        cache_key_body: &[u8],
    ) -> bool {
        let gate = {
            let runtime = self.state.semantic_runtime.read();
            if !runtime.enabled {
                return false;
            }
            runtime.gate.clone()
        };
        let Some(semantic_cache) = &self.state.semantic_cache else {
            return false;
        };
        let Ok(payload_value) = serde_json::from_slice::<serde_json::Value>(cache_key_body) else {
            return false;
        };
        let Some(messages) = payload_value.get("messages").and_then(|m| m.as_array()) else {
            return false;
        };
        let Some(query_text) = build_semantic_query_text(messages) else {
            return false;
        };

        // Apply semantic gate before L2 search
        let gate_decision = evaluate_semantic_gate(
            &gate,
            &query_text,
            ctx.cache_hit.is_some(),
        );
        match gate_decision {
            GateDecision::Pass => {}
            ref reason => {
                let reason_str = match reason {
                    GateDecision::TooShort => "too_short",
                    GateDecision::TooLong => "too_long",
                    GateDecision::NotExactMiss => "not_exact_miss",
                    _ => "unknown",
                };
                global_metrics().record_semantic_skipped(reason_str);
                debug!(
                    request_id = %ctx.request_id,
                    reason = reason_str,
                    query_len = query_text.len(),
                    "Semantic gate blocked L2 search",
                );
            }
        }
        if gate_decision != GateDecision::Pass {
            return false;
        }

        let Some(entry) = semantic_cache.search(&query_text, ctx.project_id.as_deref()).await else {
            return false;
        };

        // Model guard: verify the cached entry's model matches
        if entry.model != ctx.model {
            global_metrics().record_semantic_cache_rejected();
            debug!(
                request_id = %ctx.request_id,
                cached_model = %entry.model,
                request_model = %ctx.model,
                "Semantic cache candidate rejected by model guard",
            );
            return false;
        }
        if !cache_entry_matches_stream_mode(&entry, ctx.is_streaming) {
            debug!(
                request_id = %ctx.request_id,
                entry_is_stream = entry.is_stream,
                request_is_streaming = ctx.is_streaming,
                "Semantic cache hit ignored: stream mode mismatch"
            );
            return false;
        }

        info!(
            request_id = %ctx.request_id,
            query_len = query_text.len(),
            "Semantic cache hit, returning cached response"
        );

        if send_cached_response(
            session,
            &entry,
            &ctx.model,
            ctx.is_streaming,
            CacheTier::L2Semantic,
            ctx.cached_reasoning_config.display_reasoning,
        )
        .await
        {
            ctx.cache_tier = Some(CacheTier::L2Semantic);
            ctx.cache_hit = Some(entry.clone());
            ctx.tokens.last_input = entry.usage.prompt_tokens;
            ctx.tokens.last_output = entry.usage.completion_tokens;
            global_metrics().record_cache_hit(
                CacheTier::L2Semantic,
                &ctx.model,
                ctx.consumer.as_deref(),
                ctx.domain.as_deref(),
            );
            global_metrics().record_latency(
                crab_metrics::LatencyKind::CacheFetch,
                ctx.request_start.elapsed(),
                &ctx.model,
                Some(CacheTier::L2Semantic),
            );
            let cost = self.state.pricing.cost_saved_usd(
                &ctx.model,
                entry.usage.prompt_tokens,
                entry.usage.completion_tokens,
            );
            global_metrics().record_cost_saved(
                &ctx.model,
                ctx.consumer.as_deref(),
                ctx.domain.as_deref(),
                CacheTier::L2Semantic,
                cost,
            );
            return true;
        }

        debug!(
            request_id = %ctx.request_id,
            "Semantic cache hit refused (hollow payload); continuing as miss"
        );
        false
    }
}

#[async_trait::async_trait]
impl ProxyHttp for GatewayProxy {
    type CTX = GatewayContext;

    fn new_ctx(&self) -> Self::CTX {
        GatewayContext::new(uuid::Uuid::new_v4().to_string())
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        if self.state.cors_enabled && session.req_header().method == http::Method::OPTIONS {
            if send_cors_preflight(session).await {
                return Ok(true);
            }
        }

        let req_path = session.req_header().uri.path().to_string();
        let req_method = session.req_header().method.clone();
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
            let redis_ok = self.state.tiered_cache.ping().await;
            // Readiness includes upstream key availability: if all keys are exhausted
            // (e.g. rate-limited or disabled), the gateway cannot serve requests.
            // This intentionally reports 503 so load balancers route traffic elsewhere.
            let default_profile = self.state.runtime.default_profile();
            let pool = default_profile.resolve_upstream_pool();
            let keys_available = pool.available_count() > 0;
            let status = if redis_ok && keys_available {
                200
            } else {
                503
            };
            let _ = session.respond_error(status).await;
            return Ok(true);
        }

        if is_models_endpoint(&req_path, &req_method) {
            let (is_authorized, consumer_from_key, domain_from_key, _, _, key_profile) =
                self.authorize_client(&provided_key, &auth);
            if !is_authorized {
                let _ = session.respond_error(401).await;
                return Ok(true);
            }
            ctx.consumer = consumer_from_key;
            ctx.domain = domain_from_key;
            ctx.upstream_profile_id = key_profile
                .or_else(|| Some(self.state.runtime.default_upstream_profile_id()));
            ctx.is_models_list = true;
            let cursor_models = self.state.runtime.pipeline_globals().cursor_models;
            if cursor_models.synthetic_models_enabled && !cursor_models.aliases.is_empty() {
                let body = crab_pipeline::synthetic_models_list_json(&cursor_models);
                if send_json_ok(session, &body).await {
                    return Ok(true);
                }
                let _ = session.respond_error(500).await;
                return Ok(true);
            }
            if !self.try_acquire_upstream_key(ctx) {
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

        let (
            is_authorized,
            consumer_from_key,
            domain_from_key,
            key_project_id,
            key_pipeline,
            key_upstream_profile,
        ) = self.authorize_client(&provided_key, &auth);

        if !is_authorized {
            let _ = session.respond_error(401).await;
            return Ok(true);
        }

        // Per-key RPM rate limiting
        if let Some(stored_key) = self.state.runtime.keys.get(&provided_key) {
            let key = stored_key.value();
            if key.rpm_limit > 0
                && !self.state.client_key_rate_limiter.check_and_consume(
                    &provided_key,
                    key.rpm_limit,
                )
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
                    && self.state.runtime.auto_project_id_from_client_key
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
                if !send_json_error(session, http::StatusCode::FORBIDDEN, body_str.as_bytes()).await
                {
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
                if !send_json_error(session, http::StatusCode::BAD_REQUEST, body_str.as_bytes()).await
                {
                    let _ = session.respond_error(400).await;
                }
                return Ok(true);
            }
        }

        if let Some(stored_key) = self.state.runtime.keys.get(&provided_key) {
            match self.state.client_key_limiter.try_acquire(&provided_key, stored_key.value()) {
                Ok(guard) => ctx.client_key_guard = Some(guard),
                Err(ClientKeyLimitError::Exceeded) => {
                    global_metrics().record_rejected("client_concurrency_exceeded");
                    let body = client_concurrency_exceeded_error_json();
                    if !send_json_error(session, http::StatusCode::TOO_MANY_REQUESTS, body.as_slice()).await {
                        let _ = session.respond_error(429).await;
                    }
                    return Ok(true);
                }
            }
        }

        match self.state.request_semaphore.clone().try_acquire_owned() {
            Ok(permit) => ctx.request_permit = Some(permit),
            Err(_) => {
                global_metrics().record_rejected("overloaded");
                let _ = session.respond_error(503).await;
                return Ok(true);
            }
        }

        ctx.authorization = Some(auth);
        ctx.consumer = consumer_from_key
            .or(consumer_from_header)
            .or_else(|| ctx.project_id.clone());
        ctx.domain = domain_from_key;
        let (domain_pipeline, domain_upstream_profile) =
            self.domain_policy_fields(ctx.domain.as_deref());

        if !self.state.runtime.domain_within_quota(ctx.domain.as_deref()) {
            global_metrics().record_rejected("domain_quota_exceeded");
            let _ = session.respond_error(429).await;
            return Ok(true);
        }

        session.as_mut().enable_retry_buffering();

        let mut full_body = Vec::new();
        let max_body = self.state.max_request_body_bytes;
        loop {
            match session.downstream_session.read_request_body().await? {
                Some(data) => {
                    if full_body.len() + data.len() > max_body {
                        global_metrics().record_rejected("body_too_large");
                        let _ = session.respond_error(413).await;
                        return Ok(true);
                    }
                    full_body.extend_from_slice(&data);
                }
                None => break,
            }
            if session.is_body_done() {
                break;
            }
        }

        if full_body.is_empty() {
            let _ = session.respond_error(400).await;
            return Ok(true);
        }

        ctx.original_request_body = Some(full_body.clone());

        let mut hasher = Sha256::new();
        hasher.update(&full_body);
        let req_hash = hex::encode(hasher.finalize());
        ctx.req_hash = Some(req_hash);
        ctx.content_length = full_body.len();

        let payload: serde_json::Value = match serde_json::from_slice(&full_body) {
            Ok(v) => v,
            Err(_) => {
                let _ = session.respond_error(400).await;
                return Ok(true);
            }
        };

        let profile = self.state.runtime.default_profile();
        let fallback_model = profile.fallback_model.clone();
        ctx.model = payload
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&fallback_model)
            .to_string();
        ctx.is_streaming = payload
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        ctx.conversation_id = payload
            .get("conversation_id")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .or(conversation_id_from_header);

        ctx.prompt_cache_key = payload
            .get("prompt_cache_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let pipeline_globals = self.state.runtime.pipeline_globals();
        let model_alias_entry = pipeline_globals.cursor_models.resolve(&ctx.model);
        let alias_upstream_model = model_alias_entry.map(|e| e.upstream.as_str());
        let model_alias_pipeline = model_alias_entry.map(|e| e.pipeline);
        let alias_hit = model_alias_entry.is_some();

        let pipe_ctx = PipelineRequestContext {
            model: &ctx.model,
            payload: Some(&payload),
            key_pipeline: key_pipeline
                .as_deref()
                .map(PipelineOverride::from_str),
            key_upstream_profile: key_upstream_profile.as_deref(),
            domain_pipeline: domain_pipeline
                .as_deref()
                .map(PipelineOverride::from_str),
            domain_upstream_profile: domain_upstream_profile.as_deref(),
            conversation_id_header: ctx.conversation_id.as_deref(),
            user_agent: user_agent.as_deref(),
            alias_upstream_model,
            model_alias_pipeline,
        };
        let selection = select_request_pipeline(
            &pipeline_globals,
            &self.state.runtime.profile_descriptors(),
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

        let active_profile = self.active_upstream_profile(ctx);
        let upstream_base_url = active_profile.base_url.clone();
        let profile_fallback = active_profile.fallback_model.clone();
        let reasoning_cfg = self.reasoning_config();
        ctx.cached_reasoning_config = reasoning_cfg.clone();

        // Stable ReasoningStore scope (deepseek-cursor-proxy style): header id > client sk-cc > req hash.
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

        let (stable_session_kind, stable_session_prefix) = stable_session_log_fields(
            ctx.conversation_id.as_deref(),
            ctx.prompt_cache_key.as_deref(),
            client_session_for_log.as_deref(),
            ctx.req_hash.as_deref(),
        );

        let mut reject_missing = false;
        let mut patched = 0usize;
        let mut missing = 0usize;
        let mut recovered = 0usize;
        let mut retired_prefix = 0usize;
        let mut upstream_model_log = ctx.model.clone();
        let mut namespace_preview = String::new();
        let effective_user_id = ctx.project_id.as_deref();

        match selection.pipeline {
            RequestPipeline::CursorDeepSeekV4 => {
                let prepared = prepare_upstream_request(
                    &payload,
                    Some(&self.state.reasoning_store),
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
                    effective_user_id,
                );
                patched = prepared.patched_reasoning_messages;
                missing = prepared.missing_reasoning_messages;
                recovered = prepared.recovered_reasoning_messages;
                retired_prefix = prepared.retired_prefix_messages;
                upstream_model_log = prepared.upstream_model.clone();
                namespace_preview = prepared.cache_namespace.chars().take(8).collect();
                reject_missing = missing > 0 && reasoning_cfg.missing_reasoning_strategy == "reject";
                ctx.stream.pending_recovery_notice = prepared.recovery_notice.clone();
                ctx.prepared_request = Some(prepared.clone());
                ctx.new_request_body = Some(serde_json::to_vec(&prepared.payload).unwrap_or_default());
                if ctx.is_streaming {
                    ctx.stream.accumulator = Some(StreamAccumulator::new());
                    ctx.stream.display_adapter = reasoning_cfg.display_reasoning.then(|| {
                        CursorReasoningDisplayAdapter::new(reasoning_cfg.collapsible_reasoning)
                    });
                }
            }
            RequestPipeline::DeepSeekLight => {
                let light = prepare_light_request(
                    &payload,
                    &profile_fallback,
                    alias_upstream_model,
                    effective_user_id,
                );
                upstream_model_log = light.upstream_model.clone();
                ctx.new_request_body = Some(serde_json::to_vec(&light.payload).unwrap_or_default());
            }
            RequestPipeline::GenericRelay | RequestPipeline::MimoRelay => {
                let generic = prepare_generic_request(&payload);
                upstream_model_log = generic.model.clone();
                ctx.new_request_body = Some(serde_json::to_vec(&generic.payload).unwrap_or_default());
            }
        }
        ctx.upstream_model = Some(upstream_model_log.clone());

        if selection.provider == UpstreamProvider::Deepseek {
            if let Some(user_id) = ctx.project_id.as_deref() {
                if let Some(tier) = classify_deepseek_v4_tier(&upstream_model_log) {
                    match self
                        .state
                        .deepseek_user_id_limiter
                        .try_acquire(user_id, tier)
                    {
                        Ok(guard) => ctx.deepseek_user_id_guard = Some(guard),
                        Err(DeepSeekUserIdLimitError::Exceeded) => {
                            global_metrics()
                                .record_deepseek_user_id_concurrency_rejected(tier.as_str());
                            global_metrics()
                                .record_rejected("deepseek_user_concurrency_exceeded");
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
        let message_count = payload
            .get("messages")
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
                "last_user_fp": last_user_message_fingerprint(&payload),
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

        // ---- Extract request composition for trace analysis ----
        if let Some(body) = &ctx.original_request_body {
            if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(body) {
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
                ctx.request_composition = Some(extract_composition(&payload, &hints));
                if let Some(ref comp) = ctx.request_composition {
                    global_metrics().record_composition_metrics(comp);
                }

                // ---- Write composition debug entry if debug logging is enabled ----
                if let Some(debug_tx) = composition_debug_tx() {
                    let request_hash = ctx.req_hash.clone().unwrap_or_else(|| {
                        let mut hasher = sha2::Sha256::new();
                        hasher.update(body);
                        let h = hex::encode(hasher.finalize());
                        h[..h.len().min(16)].to_string()
                    });
                    let system_text = extract_system_text(&payload, 100_000);
                    let tools_json = extract_tools_json(&payload, 100_000);
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
        }

        let new_body = ctx.new_request_body.clone().unwrap_or(full_body);
        ctx.upstream_outbound_body_len = new_body.len();
        // #region agent log
        let outbound_fp: String = {
            let mut hasher = Sha256::new();
            hasher.update(&new_body);
            let h = hex::encode(hasher.finalize());
            h[..h.len().min(8)].to_string()
        };
        let upstream_msg_count = serde_json::from_slice::<serde_json::Value>(&new_body)
            .ok()
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
                "last_user_fp": last_user_message_fingerprint(&payload),
                "recovered": recovered,
                "retired_prefix": retired_prefix,
            }),
        );
        // #endregion
        ctx.new_request_body = Some(new_body);
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
        // #endregion

        let fingerprint = self.state.runtime.fingerprint.read().clone();
        let cache_key_body = ctx
            .original_request_body
            .as_deref()
            .expect("original_request_body set");
        let cache_namespace = effective_cache_namespace(
            self.state.cache_key_namespace.as_deref(),
            ctx.project_id.as_deref(),
        );
        if let Ok(cache_key) = crab_cache::generate_namespaced_cache_key_with_fingerprint(
            cache_key_body,
            cache_namespace.as_deref(),
            &fingerprint,
        ) {
            ctx.cache_key = Some(cache_key.clone());

            if let Some((entry, tier)) = self
                .state
                .tiered_cache
                .get(
                    &cache_key,
                    ctx.consumer.as_deref(),
                    ctx.domain.as_deref(),
                )
                .await
            {
                if cache_entry_matches_stream_mode(&entry, ctx.is_streaming) {
                    info!(
                        request_id = %ctx.request_id,
                        cache_key = %cache_key,
                        tier = ?tier,
                        "Cache hit, returning cached response"
                    );
                    // #region agent log
                    let req_hash_short = ctx
                        .req_hash
                        .as_deref()
                        .map(|h| h.chars().take(8).collect::<String>());
                    debug_agent_log(
                        "H4",
                        "proxy.rs:request_filter",
                        "exact cache hit before send_cached_response",
                        serde_json::json!({
                            "request_id": ctx.request_id,
                            "req_hash": req_hash_short,
                            "tier": tier.as_str(),
                            "is_streaming": ctx.is_streaming,
                            "entry_is_stream": entry.is_stream,
                            "response_body_len": entry.response_body.len(),
                            "sse_body_len": entry.sse_body.as_ref().map(|s| s.len()),
                            "elapsed_ms": ctx.request_start.elapsed().as_millis(),
                        }),
                    );
                    // #endregion

                    let sent_ok = send_cached_response(
                        session,
                        &entry,
                        &ctx.model,
                        ctx.is_streaming,
                        tier,
                        ctx.cached_reasoning_config.display_reasoning,
                    )
                    .await;

                    // #region agent log
                    debug_agent_log(
                        "H5",
                        "proxy.rs:request_filter",
                        "send_cached_response result",
                        serde_json::json!({
                            "request_id": ctx.request_id,
                            "req_hash": req_hash_short,
                            "sent_ok": sent_ok,
                            "tier": tier.as_str(),
                        }),
                    );
                    // #endregion

                    if sent_ok {
                        ctx.cache_tier = Some(tier);
                        ctx.cache_hit = Some(entry.clone());
                        ctx.tokens.last_input = entry.usage.prompt_tokens;
                        ctx.tokens.last_output = entry.usage.completion_tokens;
                        global_metrics().record_latency(
                            crab_metrics::LatencyKind::CacheFetch,
                            ctx.request_start.elapsed(),
                            &ctx.model,
                            Some(tier),
                        );
                        let cost = self.state.pricing.cost_saved_usd(
                            &ctx.model,
                            entry.usage.prompt_tokens,
                            entry.usage.completion_tokens,
                        );
                        global_metrics().record_cost_saved(
                            &ctx.model,
                            ctx.consumer.as_deref(),
                            ctx.domain.as_deref(),
                            tier,
                            cost,
                        );
                        return Ok(true);
                    }

                    warn!(
                        request_id = %ctx.request_id,
                        cache_key = %cache_key,
                        tier = ?tier,
                        "Hollow cache entry (no client-visible content); treating as miss"
                    );
                    // #region agent log
                    debug_agent_log(
                        "H1",
                        "proxy.rs:request_filter",
                        "hollow cache fallthrough to upstream",
                        serde_json::json!({
                            "request_id": ctx.request_id,
                            "req_hash": req_hash_short,
                            "tier": tier.as_str(),
                        }),
                    );
                    // #endregion
                } else {
                    // #region agent log
                    debug_agent_log(
                        "H2",
                        "proxy.rs:request_filter",
                        "cache hit stream mode mismatch",
                        serde_json::json!({
                            "request_id": ctx.request_id,
                            "is_streaming": ctx.is_streaming,
                            "entry_is_stream": entry.is_stream,
                            "tier": tier.as_str(),
                        }),
                    );
                    // #endregion
                    debug!(
                        request_id = %ctx.request_id,
                        cache_key = %cache_key,
                        entry_is_stream = entry.is_stream,
                        request_is_streaming = ctx.is_streaming,
                        "Exact cache hit ignored: stream mode mismatch"
                    );
                }
            }

            if ctx.request_pipeline == Some(RequestPipeline::CursorDeepSeekV4) {
                let cache_key_body_owned = cache_key_body.to_vec();
                if self.try_l2_semantic_cache(session, ctx, &cache_key_body_owned).await {
                    return Ok(true);
                }
            }

            match self.state.coalescer.acquire(&cache_key).await {
                Ok(guard) => {
                    // #region agent log
                    let cache_key_short: String = cache_key.chars().take(8).collect();
                    debug_agent_log(
                        "H-E",
                        "proxy.rs:request_filter",
                        "coalesce acquire",
                        serde_json::json!({
                            "request_id": ctx.request_id,
                            "req_hash": ctx.req_hash.as_deref().map(|h| h.chars().take(8).collect::<String>()),
                            "is_leader": guard.is_leader(),
                            "leader_failed": guard.leader_failed(),
                            "cache_key_prefix": cache_key_short,
                        }),
                    );
                    // #endregion
                    if !guard.is_leader() {
                        ctx.is_coalesced_follower = true;

                        if let Some((entry, tier)) = self
                .state
                .tiered_cache
                .get(
                    &cache_key,
                    ctx.consumer.as_deref(),
                    ctx.domain.as_deref(),
                )
                .await
            {
                            if !cache_entry_matches_stream_mode(&entry, ctx.is_streaming) {
                                debug!(
                                    request_id = %ctx.request_id,
                                    cache_key = %cache_key,
                                    entry_is_stream = entry.is_stream,
                                    request_is_streaming = ctx.is_streaming,
                                    "Follower cache hit ignored: stream mode mismatch"
                                );
                            } else {
                            info!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                tier = ?tier,
                                "Follower found cached response after leader completed"
                            );

                            // #region agent log
                            debug_agent_log(
                                "H-A",
                                "proxy.rs:request_filter",
                                "coalesce follower cache replay",
                                serde_json::json!({
                                    "request_id": ctx.request_id,
                                    "req_hash": ctx.req_hash.as_deref().map(|h| h.chars().take(8).collect::<String>()),
                                    "tier": tier.as_str(),
                                    "response_body_len": entry.response_body.len(),
                                    "sse_body_len": entry.sse_body.as_ref().map(|s| s.len()),
                                }),
                            );
                            // #endregion

                            let sent_ok = send_cached_response(
                                session,
                                &entry,
                                &ctx.model,
                                ctx.is_streaming,
                                tier,
                                ctx.cached_reasoning_config.display_reasoning,
                            )
                            .await;

                            if sent_ok {
                                ctx.cache_tier = Some(tier);
                                ctx.cache_hit = Some(entry.clone());
                                ctx.tokens.last_input = entry.usage.prompt_tokens;
                                ctx.tokens.last_output = entry.usage.completion_tokens;
                                global_metrics().record_coalesced_request();
                                global_metrics().record_latency(
                                    crab_metrics::LatencyKind::CacheFetch,
                                    ctx.request_start.elapsed(),
                                    &ctx.model,
                                    Some(tier),
                                );
                                let cost = self.state.pricing.cost_saved_usd(
                                    &ctx.model,
                                    entry.usage.prompt_tokens,
                                    entry.usage.completion_tokens,
                                );
                                global_metrics().record_cost_saved(
                                    &ctx.model,
                                    ctx.consumer.as_deref(),
                                    ctx.domain.as_deref(),
                                    tier,
                                    cost,
                                );
                                return Ok(true);
                            }

                            warn!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                "Follower hollow cache entry; waiting for upstream path"
                            );
                            }
                        } else if guard.leader_failed() {
                            warn!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                "Follower: leader upstream failed, not retrying upstream"
                            );
                            let body = coalesce_leader_failed_error_json();
                            if !send_json_error(session, http::StatusCode::BAD_GATEWAY, &body)
                                .await
                            {
                                let _ = session.respond_error(502).await;
                            }
                            return Ok(true);
                        } else {
                            // #region agent log
                            debug_agent_log(
                                "H-F",
                                "proxy.rs:request_filter",
                                "follower no cache entry, falling through to upstream",
                                serde_json::json!({
                                    "request_id": ctx.request_id,
                                    "elapsed_since_start_ms": ctx.request_start.elapsed().as_millis(),
                                }),
                            );
                            // #endregion
                            warn!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                "Follower did not find cached response, falling through to upstream"
                            );
                        }
                    } else {
                        ctx.coalesce_guard = Some(guard);
                    }
                }
                Err(CoalesceError::CapacityExceeded) => {
                    global_metrics().record_rejected("coalesce_capacity");
                    let _ = session.respond_error(503).await;
                    return Ok(true);
                }
                Err(e) => {
                    warn!(
                        request_id = %ctx.request_id,
                        error = %e,
                        "Coalescing failed, proceeding without coalescing"
                    );
                }
            }
        }

        if !self.try_acquire_upstream_key(ctx) {
            let pool = self.active_upstream_profile(ctx).resolve_upstream_pool();
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
            if !send_json_error_with_retry_after(session, http::StatusCode::SERVICE_UNAVAILABLE, &body, retry_after)
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

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        if ctx.is_models_list {
            let profile = self.active_upstream_profile(ctx);
            let router = &profile.router;
            let backend = router
                .backends()
                .first()
                .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?;

            ctx.upstream.backend_name = Some(backend.name.clone());
            ctx.upstream.host = Some(backend.tls_sni.clone());
            let peer = self.create_upstream_peer(backend, ctx);
            let conn_config = self.state.runtime.conn_config.read().clone();
            // #region agent log
            debug_agent_log(
                "H1",
                "proxy.rs:upstream_peer",
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

        let headers = HeaderMap::from_iter(
            req_header
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );

        let body_pck = ctx.prompt_cache_key.as_deref();
        let affinity_key =
            extract_affinity_key(&headers, &client_ip, body_pck, ctx.project_id.as_deref());

        let profile = self.active_upstream_profile(ctx);
        let router = &profile.router;

        // Transition timed-out open circuits to half-open before backend selection.
        // Read-first: only acquire write lock if there are open circuits.
        let has_open = {
            let health = self.state.runtime.backend_health.read();
            health
                .values()
                .any(|h| matches!(h.circuit_state, CircuitState::Open))
        };
        if has_open {
            let mut health = self.state.runtime.backend_health.write();
            let circuit_cfg = &self.state.runtime.circuit_breaker_config;
            for h in health.values_mut() {
                h.check_open_circuit(circuit_cfg);
            }
        }

        // Check if we have health information to filter by
        let backend = {
            let health = self.state.runtime.backend_health.read();
            router
                .select_healthy(affinity_key.as_bytes(), |name| {
                    health.get(name).map(|h| h.healthy).unwrap_or(true)
                })
                .cloned()
                .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?
        };

        debug!(
            request_id = %ctx.request_id,
            backend = %backend.name,
            affinity_key = %affinity_key,
            "Selected upstream backend"
        );

        ctx.upstream.backend_name = Some(backend.name.clone());
        ctx.upstream.host = Some(backend.tls_sni.clone());
        let peer = self.create_upstream_peer(&backend, ctx);
        let conn_config = self.state.runtime.conn_config.read().clone();
        // #region agent log
        debug_agent_log(
            "H1",
            "proxy.rs:upstream_peer",
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

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
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

        if let Some(guard) = ctx.upstream.key_guard.as_ref() {
            let bearer = format!("Bearer {}", guard.bearer_secret());
            let _ = upstream_request.insert_header(http::header::AUTHORIZATION, bearer);
        }

        let conn_config = self.state.runtime.conn_config.read().clone();
        if conn_config.upstream_disable_keepalive {
            ctx.upstream.connection_close = true;
            let _ = upstream_request.insert_header(http::header::CONNECTION, "close");
        }

        ctx.upstream_headers_prepared_at = Some(Instant::now());

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

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(new_body) = ctx.new_request_body.take() {
            let emit_now = end_of_stream || ctx.upstream.retry_buffer_truncated;
            if emit_now {
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
            ctx.new_request_body =
                apply_prepared_upstream_body(new_body, body, end_of_stream, ctx.upstream.retry_buffer_truncated);
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

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        let status = upstream_response.status.as_u16();
        ctx.upstream.http_status = Some(status);
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
        let pool = self.active_upstream_profile(ctx).resolve_upstream_pool();
        let key_id = ctx
            .upstream.key_guard
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
                    if let Some(new_guard) =
                        pool.acquire_excluding_account(
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
                        )
                    {
                        ctx.upstream.key_guard = Some(new_guard);
                        global_metrics().record_upstream_key_retry("rate_limited_rotate");
                    } else {
                        global_metrics().record_upstream_key_retry("cooldown_only");
                    }
                }
            }
            if let Some(guard) = &ctx.coalesce_guard {
                if guard.is_leader() {
                    guard.mark_failed();
                }
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
            if let Some(guard) = &ctx.coalesce_guard {
                if guard.is_leader() {
                    guard.mark_failed();
                }
            }
            if status >= 500 {
                if let Some(ref backend_name) = ctx.upstream.backend_name {
                    let mut health = self.state.runtime.backend_health.write();
                    if let Some(h) = health.get_mut(backend_name) {
                        h.record_failure(&self.state.runtime.circuit_breaker_config);
                    }
                }
            }
            return Ok(());
        }

        let _ = upstream_response.insert_header("x-request-id", ctx.request_id.clone());
        let _ = upstream_response.insert_header("x-cache-status", "miss");

        if let Some(ref backend_name) = ctx.upstream.backend_name {
            let mut health = self.state.runtime.backend_health.write();
            if let Some(h) = health.get_mut(backend_name) {
                h.record_success(&self.state.runtime.circuit_breaker_config);
            }
        }

        ctx.upstream.start = Some(std::time::Instant::now());

        Ok(())
    }

    fn upstream_response_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<Option<Duration>> {
        if ctx.is_models_list {
            return Ok(None);
        }

        if !ctx.upstream.error_body_logged {
            if let Some(status) = ctx.upstream.http_status {
                if status >= 400 {
                    if let Some(chunk) = body.as_ref() {
                        let preview = upstream_error_preview(chunk.as_ref());
                        let has_reasoning_err = preview.contains("reasoning_content");
                        // #region agent log
                        debug_agent_log(
                            "UP4B",
                            "proxy.rs:upstream_response_body_filter",
                            "upstream error body preview",
                            serde_json::json!({
                                "request_id": ctx.request_id,
                                "status": status,
                                "preview": preview,
                                "has_reasoning_content_msg": has_reasoning_err,
                                "body_len": chunk.len(),
                                "end_of_stream": end_of_stream,
                            }),
                        );
                        // #endregion
                        ctx.upstream.error_body_logged = true;
                    }
                }
            }
        }

        if let Some(data) = body.take() {
            // #region agent log
            if !ctx.upstream.first_body_chunk_logged {
                ctx.upstream.first_body_chunk_logged = true;
                debug_agent_log(
                    "UP-BODY",
                    "proxy.rs:upstream_response_body_filter",
                    "first upstream body chunk",
                    serde_json::json!({
                        "request_id": ctx.request_id,
                        "chunk_len": data.len(),
                        "upstream_status": ctx.upstream.http_status,
                        "is_streaming": ctx.is_streaming,
                        "end_of_stream": end_of_stream,
                        "elapsed_since_start_ms": ctx.request_start.elapsed().as_millis(),
                        "pipeline": ctx.request_pipeline.map(|p| p.as_str()),
                        "has_prepared": ctx.prepared_request.is_some(),
                    }),
                );
            }
            // #endregion
            if !ctx.is_streaming {
                ctx.accumulated_body.extend_from_slice(&data);
                ctx.response_body_preview.extend_from_slice(&data);
            }

            if ctx.is_streaming {
                if ctx.ttft.is_none() {
                    if let Some(upstream_start) = ctx.upstream.start {
                        ctx.ttft = Some(upstream_start.elapsed());
                        if let Some(ttft) = ctx.ttft {
                            global_metrics().record_latency(
                                crab_metrics::LatencyKind::TTFT,
                                ttft,
                                &ctx.model,
                                Some(crab_metrics::CacheTier::Miss),
                            );
                        }
                    }
                }

                let downstream_chunk = if let (Some(prepared), Some(accumulator)) = (
                    ctx.prepared_request.as_ref(),
                    ctx.stream.accumulator.as_mut(),
                ) {
                    let (rewritten, finalized) = rewrite_upstream_sse_bytes(
                        &data,
                        &mut ctx.stream.sse_remainder,
                        prepared,
                        accumulator,
                        ctx.cached_reasoning_config.display_reasoning,
                        &mut ctx.stream.display_adapter,
                        &mut ctx.stream.pending_recovery_notice,
                        &self.state.reasoning_store,
                        false,
                    );
                    if finalized {
                        ctx.stream.reasoning_finalized = true;
                    }
                    if !rewritten.is_empty() {
                        ctx.stream.client_sse_body.extend_from_slice(&rewritten);
                    }
                    if rewritten.is_empty() {
                        None
                    } else {
                        Some(bytes::Bytes::from(rewritten))
                    }
                } else {
                    let client_bytes = if ctx.request_pipeline
                        == Some(RequestPipeline::CursorDeepSeekV4)
                        && !ctx.cached_reasoning_config.display_reasoning
                    {
                        if !ctx.stream.reasoning_bypass_warned {
                            ctx.stream.reasoning_bypass_warned = true;
                            warn!(
                                request_id = %ctx.request_id,
                                "CursorDeepSeekV4 stream without prepared_request; applying silent reasoning strip"
                            );
                        }
                        apply_silent_strip_to_sse_chunk(&data)
                    } else {
                        data.to_vec()
                    };
                    ctx.stream.client_sse_body.extend_from_slice(&client_bytes);
                    ctx.accumulated_body.extend_from_slice(&data);
                    Some(bytes::Bytes::from(client_bytes))
                };

                let parse_src = downstream_chunk.as_ref().unwrap_or(&data);
                let events = parse_sse_chunk(parse_src);
                for event in &events {
                    if let Some(usage) = event.parse_usage() {
                        ctx.tokens.total += usage.prompt_tokens + usage.completion_tokens;
                        ctx.tokens.last_input = usage.prompt_tokens;
                        ctx.tokens.last_output = usage.completion_tokens;
                        ctx.tokens.last_prompt_cache_hit = usage.prompt_cache_hit_tokens;
                        ctx.tokens.last_prompt_cache_miss = usage.prompt_cache_miss_tokens;
                        record_usage_metrics(
                            &usage,
                            &ctx.model,
                            ctx.consumer.as_deref(),
                            ctx.domain.as_deref(),
                            &self.state.runtime,
                            &self.state.pricing,
                        );
                    }
                }

                *body = downstream_chunk;
            } else {
                *body = Some(data);
            }
        }

        // Non-streaming reasoning rewrite: buffer upstream chunks; only emit rewritten body on EOS.
        if !ctx.is_streaming && ctx.prepared_request.is_some() && !end_of_stream {
            *body = None;
            return Ok(None);
        }

        if end_of_stream && !ctx.is_streaming {
            if let Some(guard) = &ctx.coalesce_guard {
                guard.mark_completed();
            }

            if let Some(upstream_start) = ctx.upstream.start {
                let latency = upstream_start.elapsed();
                ctx.upstream.latency_ms = Some(latency.as_secs_f64() * 1000.0);
                global_metrics().record_latency(
                    crab_metrics::LatencyKind::Upstream,
                    latency,
                    &ctx.model,
                    Some(crab_metrics::CacheTier::Miss),
                );
            }

            // Non-streaming: reasoning is stored inside rewrite_response_body via
            // record_response_reasoning. The accumulator is not populated for
            // non-streaming responses, so skip the redundant store call.
            if ctx.is_streaming {
                if let (Some(prepared), Some(accumulator)) =
                    (&ctx.prepared_request, &mut ctx.stream.accumulator)
                {
                    for (scope, prior_messages) in &prepared.record_response_contexts {
                        accumulator.store_reasoning(
                            &self.state.reasoning_store,
                            scope,
                            &prepared.cache_namespace,
                            prior_messages,
                        );
                    }
                }
            }

            let client_body: Vec<u8> = if let Some(ref prepared) = ctx.prepared_request {
                match rewrite_response_body(
                    &ctx.accumulated_body,
                    &prepared.original_model,
                    Some(&self.state.reasoning_store),
                    &prepared.record_response_messages,
                    &prepared.cache_namespace,
                    ctx.stream.pending_recovery_notice.as_deref(),
                    &prepared.record_response_contexts,
                    ctx.cached_reasoning_config.display_reasoning,
                    ctx.cached_reasoning_config.collapsible_reasoning,
                ) {
                    Some(rewritten) => {
                        *body = Some(bytes::Bytes::from(rewritten.clone()));
                        rewritten
                    }
                    None => ctx.accumulated_body.clone(),
                }
            } else {
                ctx.accumulated_body.clone()
            };

            if let Ok(body_value) = serde_json::from_slice::<serde_json::Value>(&client_body) {
                if let Some(usage) = body_value.get("usage") {
                    let usage_data = UsageData {
                        prompt_tokens: usage
                            .get("prompt_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        completion_tokens: usage
                            .get("completion_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        prompt_cache_hit_tokens: usage
                            .get("prompt_cache_hit_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        prompt_cache_miss_tokens: usage
                            .get("prompt_cache_miss_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    };
                    ctx.tokens.total += usage_data.prompt_tokens + usage_data.completion_tokens;
                    ctx.tokens.last_input = usage_data.prompt_tokens;
                    ctx.tokens.last_output = usage_data.completion_tokens;
                    ctx.tokens.last_prompt_cache_hit = usage_data.prompt_cache_hit_tokens;
                    ctx.tokens.last_prompt_cache_miss = usage_data.prompt_cache_miss_tokens;
                    record_usage_metrics(
                        &usage_data,
                        &ctx.model,
                        ctx.consumer.as_deref(),
                        ctx.domain.as_deref(),
                        &self.state.runtime,
                        &self.state.pricing,
                    );
                }

                if let Some(cache_key) = &ctx.cache_key {
                    let ttl_secs = self
                        .state
                        .tiered_cache
                        .resolve_ttl(&ctx.model, ctx.consumer.as_deref());
                    let display_reasoning = ctx.cached_reasoning_config.display_reasoning;

                    let cache_body =
                        prepare_response_body_for_cache(client_body.clone(), display_reasoning);
                    let entry = build_cache_entry(
                        cache_body,
                        ctx.model.clone(),
                        ttl_secs,
                        ctx.is_streaming,
                        display_reasoning,
                    );

                    let tiered_cache = self.state.tiered_cache.clone();
                    let cache_key = cache_key.clone();
                    let model = ctx.model.clone();
                    let consumer = ctx.consumer.clone();
                    tokio::spawn(async move {
                        if let Err(e) = tiered_cache
                            .put(&cache_key, entry, &model, consumer.as_deref())
                            .await
                        {
                            warn!(error = %e, "Failed to cache response");
                        }
                    });

                    if let Some(semantic_cache) = &self.state.semantic_cache {
                        if let Some(original_body) = &ctx.original_request_body {
                            if let Ok(payload) =
                                serde_json::from_slice::<serde_json::Value>(original_body)
                            {
                                if let Some(messages) =
                                    payload.get("messages").and_then(|m| m.as_array())
                                {
                                    if let Some(query_text) = build_semantic_query_text(messages) {
                                        let semantic_cache = semantic_cache.clone();
                                        let entry_clone = build_cache_entry(
                                            prepare_response_body_for_cache(
                                                client_body.clone(),
                                                display_reasoning,
                                            ),
                                            ctx.model.clone(),
                                            ttl_secs,
                                            ctx.is_streaming,
                                            display_reasoning,
                                        );

                                        let project_id = ctx.project_id.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = semantic_cache
                                                .insert(
                                                    &query_text,
                                                    &entry_clone,
                                                    project_id.as_deref(),
                                                )
                                                .await
                                            {
                                                warn!(error = %e, "Failed to insert into semantic cache");
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if end_of_stream && ctx.is_streaming {
            if let (Some(prepared), Some(accumulator)) = (
                ctx.prepared_request.as_ref(),
                ctx.stream.accumulator.as_mut(),
            ) {
                let (rewritten, finalized) = rewrite_upstream_sse_bytes(
                    b"",
                    &mut ctx.stream.sse_remainder,
                    prepared,
                    accumulator,
                    ctx.cached_reasoning_config.display_reasoning,
                    &mut ctx.stream.display_adapter,
                    &mut ctx.stream.pending_recovery_notice,
                    &self.state.reasoning_store,
                    true,
                );
                if finalized {
                    ctx.stream.reasoning_finalized = true;
                }
                if !rewritten.is_empty() {
                    ctx.stream.client_sse_body.extend_from_slice(&rewritten);
                    *body = Some(bytes::Bytes::from(rewritten));
                }
            }

            if let Some(guard) = &ctx.coalesce_guard {
                guard.mark_completed();
            }

            if let Some(upstream_start) = ctx.upstream.start {
                let latency = upstream_start.elapsed();
                ctx.upstream.latency_ms = Some(latency.as_secs_f64() * 1000.0);
                global_metrics().record_latency(
                    crab_metrics::LatencyKind::Upstream,
                    latency,
                    &ctx.model,
                    Some(crab_metrics::CacheTier::Miss),
                );
            }

            if let (Some(cache_key), Some(accumulator)) = (&ctx.cache_key, &ctx.stream.accumulator)
            {
                let messages = accumulator.messages();
                if !messages.is_empty() {
                    let reasoning_cfg = &ctx.cached_reasoning_config;
                    let mut response_value = serde_json::json!({
                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                        "object": "chat.completion",
                        "created": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        "model": ctx.model,
                        "choices": messages.iter().enumerate().map(|(idx, msg)| {
                            serde_json::json!({
                                "index": idx,
                                "message": msg,
                                "finish_reason": "stop"
                            })
                        }).collect::<Vec<_>>(),
                        "usage": {
                            "prompt_tokens": 0,
                            "completion_tokens": 0,
                            "total_tokens": 0
                        }
                    });
                    sanitize_client_completion(
                        &mut response_value,
                        reasoning_cfg.display_reasoning,
                        reasoning_cfg.collapsible_reasoning,
                    );
                    let response_bytes =
                        serde_json::to_vec(&response_value).unwrap_or_default();
                    // Store synthesized completion JSON as response preview for trace logging.
                    ctx.response_body_preview = response_bytes.clone();

                    let ttl_secs = self
                        .state
                        .tiered_cache
                        .resolve_ttl(&ctx.model, ctx.consumer.as_deref());

                    if self.state.runtime.stream_cache_enabled()
                        && completion_json_has_visible_client_content(
                            &response_bytes,
                            reasoning_cfg.display_reasoning,
                        )
                    {
                        let sse_body = ctx.stream.client_sse_body.clone();
                        let max_sse = self.state.max_sse_cache_bytes;
                        let entry_for_cache = if should_store_sse_body(sse_body.len(), max_sse) {
                            build_cache_entry_with_sse(
                                response_bytes.clone(),
                                sse_body,
                                ctx.model.clone(),
                                ttl_secs,
                                true,
                                reasoning_cfg.display_reasoning,
                            )
                        } else {
                            warn!(
                                sse_len = sse_body.len(),
                                limit = max_sse,
                                "SSE body exceeds max_sse_cache_bytes; caching JSON only"
                            );
                            global_metrics().record_stream_cache_sse_omitted("over_limit");
                            build_cache_entry(
                                response_bytes.clone(),
                                ctx.model.clone(),
                                ttl_secs,
                                true,
                                reasoning_cfg.display_reasoning,
                            )
                        };
                        let tiered_cache = self.state.tiered_cache.clone();
                        let cache_key = cache_key.clone();
                        let model = ctx.model.clone();
                        let consumer = ctx.consumer.clone();
                        tokio::spawn(async move {
                            if let Err(e) = tiered_cache
                                .put(&cache_key, entry_for_cache, &model, consumer.as_deref())
                                .await
                            {
                                warn!(error = %e, "Failed to cache streaming response");
                            }
                        });

                    } else if self.state.runtime.stream_cache_enabled() {
                        warn!(
                            request_id = %ctx.request_id,
                            cache_key = ?ctx.cache_key,
                            "Skipping stream cache write: no client-visible content after sanitize"
                        );
                        // #region agent log
                        debug_agent_log(
                            "H1",
                            "proxy.rs:upstream_response_body_filter",
                            "skipped hollow stream cache write",
                            serde_json::json!({
                                "request_id": ctx.request_id,
                                "response_body_len": response_bytes.len(),
                                "client_sse_len": ctx.stream.client_sse_body.len(),
                                "display_reasoning": reasoning_cfg.display_reasoning,
                            }),
                        );
                        // #endregion
                    }

                    if completion_json_has_visible_client_content(
                        &response_bytes,
                        reasoning_cfg.display_reasoning,
                    ) {
                        if let Some(semantic_cache) = &self.state.semantic_cache {
                            if let Some(original_body) = &ctx.original_request_body {
                                if let Ok(payload) =
                                    serde_json::from_slice::<serde_json::Value>(original_body)
                                {
                                    if let Some(messages) =
                                        payload.get("messages").and_then(|m| m.as_array())
                                    {
                                        if let Some(query_text) =
                                            build_semantic_query_text(messages)
                                        {
                                            let semantic_cache = semantic_cache.clone();
                                            let entry_for_semantic = build_cache_entry(
                                                response_bytes.clone(),
                                                ctx.model.clone(),
                                                ttl_secs,
                                                true,
                                                reasoning_cfg.display_reasoning,
                                            );
                                            let query_text = query_text.to_string();
                                            let project_id = ctx.project_id.clone();

                                            tokio::spawn(async move {
                                                if let Err(e) = semantic_cache
                                                    .insert(
                                                        &query_text,
                                                        &entry_for_semantic,
                                                        project_id.as_deref(),
                                                    )
                                                    .await
                                                {
                                                    warn!(error = %e, "Failed to insert streaming response into semantic cache");
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    async fn logging(
        &self,
        session: &mut Session,
        error: Option<&pingora_core::Error>,
        ctx: &mut Self::CTX,
    ) {
        let duration = ctx.request_start.elapsed();
        let latency_ms = duration.as_millis() as u64;

        if let Some(e) = error {
            warn!(
                request_id = %ctx.request_id,
                error = %e,
                duration_ms = latency_ms,
                model = %ctx.model,
                "Request failed"
            );
            if let Some(guard) = &ctx.coalesce_guard {
                if guard.is_leader() {
                    guard.mark_failed();
                }
            }
            // #region agent log
            debug_agent_log(
                "F",
                "proxy.rs:logging",
                "upstream proxy error",
                serde_json::json!({
                    "request_id": ctx.request_id,
                    "error": e.to_string(),
                    "duration_ms": latency_ms,
                    "outbound_bytes": ctx.upstream_outbound_body_len,
                    "is_streaming": ctx.is_streaming,
                    "coalesce_leader": ctx.coalesce_guard.as_ref().map(|g| g.is_leader()),
                }),
            );
            // #endregion
        } else {
            // #region agent log
            debug_agent_log(
                "OK",
                "proxy.rs:logging",
                "request completed without proxy error",
                serde_json::json!({
                    "request_id": ctx.request_id,
                    "duration_ms": latency_ms,
                    "upstream_status": ctx.upstream.http_status,
                    "cache_tier": ctx.cache_tier.map(|t| t.as_str()),
                    "is_streaming": ctx.is_streaming,
                    "total_tokens": ctx.tokens.total,
                    "client_sse_bytes": ctx.stream.client_sse_body.len(),
                    "accumulated_body_bytes": ctx.accumulated_body.len(),
                    "has_prepared": ctx.prepared_request.is_some(),
                    "ttft_ms": ctx.ttft.map(|d| d.as_millis()),
                }),
            );
            // #endregion
            info!(
                request_id = %ctx.request_id,
                request_hash = %ctx.req_hash.as_ref().unwrap_or(&"missing".to_string()),
                content_length = ctx.content_length,
                latency_ms = latency_ms,
                model = %ctx.model,
                cache_hit = ctx.cache_tier.is_some(),
                cache_tier = ?ctx.cache_tier,
                is_streaming = ctx.is_streaming,
                consumer = ?sanitize_for_trace(ctx.consumer.as_deref()),
                conversation_id = ?sanitize_for_trace(ctx.conversation_id.as_deref()),
                total_tokens = ctx.tokens.total,
                upstream_key_id = ?ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                "Request completed"
            );
            if let Some(trace_logger) = &self.state.trace_logger {
                let max_payload = trace_logger.max_payload_bytes();
                let max_resp = trace_logger.max_response_preview_bytes();
                if let Some(body) = &ctx.original_request_body {
                    let mut entry = SanitizedLogEntry::from_request(
                        body,
                        ctx.conversation_id.clone(),
                        ctx.consumer.clone(),
                        ctx.domain.clone(),
                        ctx.project_id.clone(),
                        &ctx.model,
                        ctx.tokens.total as usize,
                        duration.as_secs_f64() * 1000.0,
                        ctx.cache_tier.is_some(),
                        ctx.cache_tier.map(|t| t.as_str().to_string()),
                        ctx.request_composition.clone(),
                        max_payload,
                    );
                    if let Some(prepared) = &ctx.prepared_request {
                        entry.retired_prefix_messages = Some(prepared.retired_prefix_messages);
                    }
                    entry.reasoning_strategy =
                        Some(ctx.cached_reasoning_config.missing_reasoning_strategy.clone());
                    let hit = ctx.tokens.last_prompt_cache_hit;
                    let miss = ctx.tokens.last_prompt_cache_miss;
                    if hit + miss > 0 {
                        entry.prompt_cache_hit_ratio = Some(hit as f64 / (hit + miss) as f64);
                    }
                    entry.upstream_latency_ms = ctx.upstream.latency_ms;
                    entry.ttft_ms = ctx.ttft.map(|d| d.as_secs_f64() * 1000.0);
                    if ctx.tokens.last_input > 0 || ctx.tokens.last_output > 0 {
                        entry.input_tokens = Some(ctx.tokens.last_input);
                        entry.output_tokens = Some(ctx.tokens.last_output);
                        entry.prompt_tokens = ctx
                            .tokens.last_input
                            .saturating_add(ctx.tokens.last_output) as usize;
                    }
                    // Populate response_preview from best available source
                    if max_resp > 0 {
                        entry.response_preview = build_response_preview(ctx, max_resp);
                    }
                    apply_user_id_audit_to_entry(
                        &mut entry,
                        ctx.request_pipeline,
                        ctx.upstream_profile_id.as_deref(),
                        ctx.upstream_model.as_deref(),
                        ctx.project_id.as_deref(),
                        ctx.original_request_body.as_deref(),
                        ctx.new_request_body.as_deref(),
                    );
                    trace_logger.log(entry);
                }
            }
        }

        // ── Raw capture ──────────────────────────────────────────────
        if let Some(raw_logger) = &self.state.raw_capture_logger {
            let req_path = session.req_header().uri.path();
            if !raw_logger.should_skip(req_path) {
                let reasoning_strategy = ctx
                    .cached_reasoning_config
                    .missing_reasoning_strategy
                    .as_str();
                raw_logger.capture(
                    &ctx.request_id,
                    ctx.req_hash.as_deref(),
                    &ctx.model,
                    ctx.consumer.as_deref(),
                    ctx.project_id.as_deref(),
                    ctx.request_pipeline.as_ref().map(|p| p.as_str()),
                    ctx.is_streaming,
                    ctx.prepared_request.as_ref().map(|p| p.retired_prefix_messages),
                    Some(reasoning_strategy),
                    ctx.original_request_body.as_deref(),
                    ctx.new_request_body.as_deref(),
                );
            }
        }

        if let Some(ttft) = ctx.ttft {
            debug!(
                request_id = %ctx.request_id,
                ttft_ms = ttft.as_millis() as u64,
                "Time to first token recorded"
            );
        }

        if let Some(cache_key) = &ctx.cache_key {
            if ctx.cache_hit.is_none() {
                debug!(
                    request_id = %ctx.request_id,
                    cache_key = %cache_key,
                    "Cache miss for request"
                );
            }
        }

        if ctx.is_streaming && !ctx.stream.reasoning_finalized {
            let stored = flush_streaming_reasoning(ctx, &self.state.reasoning_store);
            if stored > 0 {
                debug!(
                    request_id = %ctx.request_id,
                    stored,
                    "Stored partial streaming reasoning before request exit"
                );
            }
        }
    }
}

/// Persist accumulated streaming reasoning when the client disconnects or stops before `[DONE]`.
pub fn flush_streaming_reasoning(
    ctx: &mut GatewayContext,
    store: &crab_reasoning::ReasoningBackend,
) -> usize {
    if ctx.stream.reasoning_finalized {
        return 0;
    }
    let (prepared, accumulator) = match (&ctx.prepared_request, &mut ctx.stream.accumulator) {
        (Some(p), Some(a)) => (p, a),
        _ => return 0,
    };
    if accumulator.messages().is_empty() {
        return 0;
    }
    prepared
        .record_response_contexts
        .iter()
        .map(|(scope, prior_messages)| {
            accumulator.store_reasoning(store, scope, &prepared.cache_namespace, prior_messages)
        })
        .sum()
}

fn upstream_pool_exhausted_error_json() -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": "All upstream DeepSeek API keys are rate-limited or disabled. Retry after cooldown or add keys via CRABCACHE_UPSTREAM_KEYS.",
            "type": "upstream_key_exhausted",
            "code": "upstream_key_exhausted",
        }
    });
    serde_json::to_vec(&body).unwrap_or_default()
}

fn coalesce_leader_failed_error_json() -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": "Upstream request failed while coalesced peers were waiting; no duplicate upstream call was made.",
            "type": "upstream_error",
            "code": "coalesce_leader_failed",
        }
    });
    serde_json::to_vec(&body).unwrap_or_default()
}

/// Sanitized upstream error snippet for debug logs (no secrets).
fn upstream_error_preview(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
        if let Some(msg) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return msg.chars().take(300).collect();
        }
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            return msg.chars().take(300).collect();
        }
    }
    s.chars().take(300).collect()
}

fn missing_reasoning_error_json(missing_count: usize) -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": format!(
                "CrabCache cannot satisfy DeepSeek thinking-mode requirements: reasoning_content is still \
                 missing for {missing_count} assistant message(s) after ReasoningStore fill and history recover. \
                 Clear the reasoning cache (DELETE /v1/reasoning/cache), retry once so the gateway can store \
                 reasoning from a successful response, or temporarily set missing_reasoning_strategy to \"recover\". \
                 Ensure [reasoning].backend = \"redis\" for multi-instance/sub-agent retries."
            ),
            "type": "missing_reasoning_content",
            "code": "missing_reasoning_content",
            "missing_reasoning_messages": missing_count,
        }
    });
    serde_json::to_vec(&body).unwrap_or_default()
}

/// Build a response preview string from the best available source in the context.
///
/// Priority:
/// 1. Cache hit → `entry.response_body`
/// 2. Non-streaming accumulated body → `ctx.response_body_preview`
/// 3. Streaming SSE body → `ctx.stream.client_sse_body`
///
/// Returns `None` when no data is available or all sources are empty.
fn build_response_preview(ctx: &GatewayContext, max_bytes: usize) -> Option<String> {
    let source: &[u8] = if let Some(ref cache_entry) = ctx.cache_hit {
        &cache_entry.response_body
    } else if !ctx.response_body_preview.is_empty() {
        &ctx.response_body_preview
    } else if ctx.is_streaming && !ctx.stream.client_sse_body.is_empty() {
        &ctx.stream.client_sse_body
    } else {
        return None;
    };

    if source.is_empty() {
        return None;
    }

    let raw = String::from_utf8_lossy(source);
    let truncated = if raw.len() > max_bytes {
        format!("{}...<truncated {}>", &raw[..max_bytes], raw.len() - max_bytes)
    } else {
        raw.to_string()
    };

    Some(truncated)
}

fn record_usage_metrics(
    usage: &UsageData,
    model: &str,
    consumer: Option<&str>,
    domain: Option<&str>,
    runtime: &crate::runtime::RuntimeConfig,
    pricing: &crate::context::PricingConfig,
) {
    global_metrics().record_upstream_usage(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.prompt_cache_hit_tokens,
        usage.prompt_cache_miss_tokens,
        model,
        consumer,
        domain,
    );

    if usage.prompt_cache_hit_tokens > 0 {
        global_metrics().record_upstream_prompt_cache(
            "hit",
            usage.prompt_cache_hit_tokens,
            model,
            consumer,
            domain,
        );
    }
    if usage.prompt_cache_miss_tokens > 0 {
        global_metrics().record_upstream_prompt_cache(
            "miss",
            usage.prompt_cache_miss_tokens,
            model,
            consumer,
            domain,
        );
    }

    let total_tokens = usage.prompt_tokens.saturating_add(usage.completion_tokens);
    let spend = pricing.cost_saved_usd(model, usage.prompt_tokens, usage.completion_tokens);
    runtime.record_domain_usage(domain, total_tokens, spend);
}

/// Rewrite upstream SSE lines for OpenAI-compatible clients (mirror reasoning into `content`).
fn rewrite_upstream_sse_bytes(
    chunk: &[u8],
    remainder: &mut Vec<u8>,
    prepared: &crab_reasoning::PreparedRequest,
    accumulator: &mut StreamAccumulator,
    display_reasoning: bool,
    display_adapter: &mut Option<CursorReasoningDisplayAdapter>,
    pending_recovery_notice: &mut Option<String>,
    store: &ReasoningBackend,
    flush_remainder: bool,
) -> (Vec<u8>, bool) {
    remainder.extend_from_slice(chunk);
    let mut out = Vec::new();
    let mut finalized = false;

    while let Some(pos) = remainder.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = remainder.drain(..=pos).collect();
        if line.iter().all(|&b| b == b'\n' || b == b'\r') {
            out.extend_from_slice(&line);
            continue;
        }
        let result = rewrite_sse_chunk(
            &line,
            &prepared.original_model,
            accumulator,
            &prepared.cache_namespace,
            &prepared.record_response_contexts,
            display_reasoning,
            display_adapter,
            pending_recovery_notice.as_deref(),
            Some(store),
        );
        *pending_recovery_notice = result.pending_recovery_notice;
        if result.finalized {
            finalized = true;
        }
        out.extend_from_slice(&result.rewritten_line);
    }

    if flush_remainder && !remainder.is_empty() {
        let mut tail = std::mem::take(remainder);
        if !tail.ends_with(b"\n") {
            tail.push(b'\n');
        }
        let result = rewrite_sse_chunk(
            &tail,
            &prepared.original_model,
            accumulator,
            &prepared.cache_namespace,
            &prepared.record_response_contexts,
            display_reasoning,
            display_adapter,
            pending_recovery_notice.as_deref(),
            Some(store),
        );
        *pending_recovery_notice = result.pending_recovery_notice;
        if result.finalized {
            finalized = true;
        }
        out.extend_from_slice(&result.rewritten_line);
    }

    (out, finalized)
}

fn is_models_endpoint(path: &str, method: &http::Method) -> bool {
    *method == http::Method::GET && (path == "/models" || path == "/v1/models")
}

fn apply_connection_options(config: &ConnectionConfig, options: &mut PeerOptions) {
    if config.upstream_force_http1 {
        options.set_http_version(1, 1);
        options.alpn = ALPN::H1;
        options.h2_ping_interval = None;
        options.max_h2_streams = 1;
    } else if let Some(ping_secs) = config.h2_ping_interval_secs
        && ping_secs > 0
    {
        options.h2_ping_interval = Some(Duration::from_secs(ping_secs));
    }

    if !config.upstream_tls_curves.is_empty() {
        use std::sync::OnceLock;
        static CACHED_CURVES: OnceLock<&'static str> = OnceLock::new();
        let curves: &'static str = *CACHED_CURVES.get_or_init(|| {
            Box::leak(config.upstream_tls_curves.clone().into_boxed_str())
        });
        options.curves = Some(curves);
    }

    if let (Some(idle), Some(interval), Some(count)) = (
        config.tcp_keepalive_idle_secs,
        config.tcp_keepalive_interval_secs,
        config.tcp_keepalive_count,
    ) {
        options.tcp_keepalive = Some(TcpKeepalive {
            idle: Duration::from_secs(idle),
            interval: Duration::from_secs(interval),
            count,
            user_timeout: Duration::from_secs(0),
        });
    }

    if config.upstream_disable_keepalive {
        options.idle_timeout = Some(Duration::from_secs(0));
    } else if let Some(idle_secs) = config.idle_timeout_secs {
        options.idle_timeout = Some(Duration::from_secs(idle_secs));
    }

    if let Some(secs) = config.upstream_connection_timeout_secs {
        if secs > 0 {
            options.connection_timeout = Some(Duration::from_secs(secs));
        }
    }

    if let Some(secs) = config.upstream_write_timeout_secs {
        if secs > 0 {
            options.write_timeout = Some(Duration::from_secs(secs));
        }
    }

    if let Some(secs) = config.upstream_request_timeout_secs {
        if secs > 0 {
            options.read_timeout = Some(Duration::from_secs(secs));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_response::{cached_sse_has_nonempty_content, json_to_sse_stream};
    use serde_json::json;

    #[test]
    fn test_gateway_context_new() {
        let ctx = GatewayContext::new("test-id".to_string());
        assert_eq!(ctx.request_id, "test-id");
        assert!(!ctx.is_streaming);
        assert!(ctx.cache_key.is_none());
        assert!(ctx.cache_hit.is_none());
        assert!(ctx.cache_tier.is_none());
        assert!(ctx.original_request_body.is_none());
        assert!(ctx.prepared_request.is_none());
    }

    #[test]
    fn test_build_semantic_query_text_single_user() {
        let messages = vec![json!({"role": "user", "content": "Hello"})];
        let result = build_semantic_query_text(&messages);
        assert_eq!(result, Some("Hello".to_string()));
    }

    #[test]
    fn test_build_semantic_query_text_system_and_user() {
        let messages = vec![
            json!({"role": "system", "content": "You are a helpful assistant."}),
            json!({"role": "user", "content": "What is Rust?"}),
        ];
        let result = build_semantic_query_text(&messages);
        assert_eq!(
            result,
            Some("You are a helpful assistant.\nWhat is Rust?".to_string())
        );
    }

    #[test]
    fn test_build_semantic_query_text_conversation() {
        let messages = vec![
            json!({"role": "system", "content": "Be concise."}),
            json!({"role": "user", "content": "Hi"}),
            json!({"role": "assistant", "content": "Hello!"}),
            json!({"role": "user", "content": "Explain caching."}),
        ];
        let result = build_semantic_query_text(&messages);
        assert!(result.unwrap().contains("Explain caching."));
    }

    #[test]
    fn test_build_semantic_query_text_array_content() {
        let messages = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "Hello from array"}]}),
        ];
        let result = build_semantic_query_text(&messages);
        assert_eq!(result, Some("Hello from array".to_string()));
    }

    #[test]
    fn test_build_semantic_query_text_empty_messages() {
        let result = build_semantic_query_text(&[]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_build_semantic_query_text_no_text_content() {
        let messages = vec![
            json!({"role": "user", "content": [{"type": "image_url", "url": "http://example.com/img.png"}]}),
        ];
        let result = build_semantic_query_text(&messages);
        assert_eq!(result, None);
    }

    #[test]
    fn test_build_semantic_query_text_tool_role_skipped() {
        let messages = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "tool", "content": "tool result"}),
        ];
        let result = build_semantic_query_text(&messages);
        assert_eq!(result, Some("Hello".to_string()));
    }

    #[test]
    fn cache_hit_sse_with_reasoning_content_should_regenerate() {
        let bad_sse = b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"x\"}}]}\n\n";
        assert!(bad_sse.windows(b"reasoning_content".len()).any(|w| w == b"reasoning_content"));
        let body = serde_json::json!({
            "choices": [{
                "message": {"role": "assistant", "content": "ok", "reasoning_content": "hidden"},
                "finish_reason": "stop"
            }]
        });
        let regen = json_to_sse_stream(body.to_string().as_bytes(), "deepseek-v4-pro", true);
        let text = String::from_utf8(regen).unwrap();
        assert!(!text.contains("reasoning_content"));
    }

    #[test]
    fn json_to_sse_stream_omits_reasoning_content() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "answer",
                    "reasoning_content": "hidden"
                },
                "finish_reason": "stop"
            }]
        });
        let sse = json_to_sse_stream(body.to_string().as_bytes(), "deepseek-v4-pro", true);
        let text = String::from_utf8(sse).unwrap();
        assert!(!text.contains("reasoning_content"));
        assert!(text.contains("[DONE]"));
        assert!(text.contains("answer"));
    }

    #[test]
    fn json_to_sse_stream_folds_reasoning_when_content_empty() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "thought"
                },
                "finish_reason": "stop"
            }]
        });
        let sse = json_to_sse_stream(body.to_string().as_bytes(), "deepseek-v4-pro", true);
        assert!(cached_sse_has_nonempty_content(&sse));
        let text = String::from_utf8(sse).unwrap();
        assert!(!text.contains("reasoning_content"));
        assert!(text.contains("thought"));
    }

    #[test]
    fn json_to_sse_stream_silent_mode_omits_reasoning_text() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "thought"
                },
                "finish_reason": "stop"
            }]
        });
        let sse = json_to_sse_stream(body.to_string().as_bytes(), "deepseek-v4-pro", false);
        let text = String::from_utf8(sse).unwrap();
        assert!(!text.contains("reasoning_content"));
        assert!(!text.contains("thought"));
    }

    #[test]
    fn cache_entry_stream_mode_guard() {
        let entry = build_cache_entry(b"{}".to_vec(), "m".into(), 60, false, true);
        assert!(cache_entry_matches_stream_mode(&entry, false));
        assert!(!cache_entry_matches_stream_mode(&entry, true));
        let stream_entry = build_cache_entry(b"{}".to_vec(), "m".into(), 60, true, false);
        assert!(cache_entry_matches_stream_mode(&stream_entry, true));
    }

    #[test]
    fn prepare_response_body_for_cache_strips_reasoning_field() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "<details>\n<summary>Thinking</summary>\n\nthink\n</details>\n\nhi",
                    "reasoning_content": "secret"
                },
                "finish_reason": "stop"
            }]
        });
        let out = prepare_response_body_for_cache(body.to_string().into_bytes(), false);
        let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let msg = &parsed["choices"][0]["message"];
        assert!(msg.get("reasoning_content").is_none());
        assert_eq!(msg["content"].as_str(), Some("hi"));
    }

    #[test]
    fn client_sse_rewrite_omits_reasoning_content_when_silent() {
        use crab_reasoning::{PreparedRequest, ReasoningBackend, StreamAccumulator};

        let store =
            ReasoningBackend::open_sqlite(":memory:", Some(3600), Some(1000)).expect("memory");
        let prepared = PreparedRequest {
            payload: serde_json::json!({}),
            original_model: "deepseek-v4-pro".into(),
            upstream_model: "deepseek-v4-pro".into(),
            cache_namespace: "ns".into(),
            patched_reasoning_messages: 0,
            missing_reasoning_messages: 0,
            recovered_reasoning_messages: 0,
            recovery_dropped_messages: 0,
            retired_prefix_messages: 0,
            recovery_notice: None,
            record_response_scope: "scope".into(),
            record_response_messages: vec![],
            record_response_contexts: vec![("scope".into(), vec![])],
        };
        let line = br#"data: {"choices":[{"index":0,"delta":{"reasoning_content":"secret think","role":"assistant"}}]}

"#;
        let mut remainder = Vec::new();
        let mut acc = StreamAccumulator::new();
        let mut adapter = None;
        let mut notice = None;
        let (out, _) = crate::sse_rewrite::rewrite_upstream_sse_bytes(
            line,
            &mut remainder,
            &prepared,
            &mut acc,
            false,
            &mut adapter,
            &mut notice,
            &store,
            true,
        );
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("reasoning_content"));
        assert!(!text.contains("secret think"));
    }

    #[test]
    fn test_should_store_sse_body() {
        assert!(should_store_sse_body(100, 4_194_304));
        assert!(!should_store_sse_body(4_194_305, 4_194_304));
        assert!(!should_store_sse_body(100, 0));
        assert!(should_store_sse_body(0, 1024));
    }

    #[test]
    fn test_flush_streaming_reasoning_skips_when_finalized() {
        let store =
            crab_reasoning::ReasoningBackend::open_sqlite(":memory:", Some(3600), Some(1000))
                .expect("store");
        let mut ctx = GatewayContext::new("req-1".to_string());
        ctx.stream.reasoning_finalized = true;
        ctx.prepared_request = Some(crab_reasoning::PreparedRequest {
            payload: serde_json::json!({}),
            original_model: "deepseek-v4-pro".into(),
            upstream_model: "deepseek-v4-pro".into(),
            cache_namespace: "ns".into(),
            patched_reasoning_messages: 0,
            missing_reasoning_messages: 0,
            recovered_reasoning_messages: 0,
            recovery_dropped_messages: 0,
            retired_prefix_messages: 0,
            recovery_notice: None,
            record_response_scope: "scope".into(),
            record_response_messages: vec![],
            record_response_contexts: vec![("scope".into(), vec![])],
        });
        ctx.stream.accumulator = Some(StreamAccumulator::new());
        assert_eq!(flush_streaming_reasoning(&mut ctx, &store), 0);
    }

    #[test]
    fn test_missing_reasoning_error_json_shape() {
        let body = missing_reasoning_error_json(2);
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value["error"]["code"].as_str(),
            Some("missing_reasoning_content")
        );
        assert_eq!(
            value["error"]["missing_reasoning_messages"].as_u64(),
            Some(2)
        );
    }
}
