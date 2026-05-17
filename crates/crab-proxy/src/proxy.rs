use crate::context::{ConnectionConfig, GatewayContext, GatewayState};
use crate::sse::{UsageData, parse_sse_chunk};
use crate::trace_logger::SanitizedLogEntry;
use crate::upstream_pool::REASONING_NAMESPACE_AUTH;
use crab_cache::{CacheEntry, CoalesceError, UsageInfo};
use crab_metrics::{CacheTier, global_metrics};
use crab_reasoning::{
    CursorReasoningDisplayAdapter, StreamAccumulator, prepare_upstream_request,
    rewrite_response_body, rewrite_sse_chunk,
};
use crab_route::extract_affinity_key;
use crab_semantic::{GateDecision, evaluate_semantic_gate};
use http::HeaderMap;
use pingora_core::prelude::*;
use pingora_core::protocols::l4::ext::TcpKeepalive;
use pingora_core::upstreams::peer::PeerOptions;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
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

pub struct GatewayProxy {
    state: Arc<GatewayState>,
}

impl GatewayProxy {
    pub fn new(state: Arc<GatewayState>) -> Self {
        Self { state }
    }

    fn authorize_client(&self, provided_key: &str, auth: &str) -> (bool, Option<String>) {
        if let Some(stored_key) = self.state.runtime.keys.get(provided_key) {
            let key = stored_key.value();
            (key.enabled, Some(key.name.clone()))
        } else if self
            .state
            .runtime
            .is_legacy_client_token(provided_key, auth)
        {
            (true, None)
        } else {
            (false, None)
        }
    }

    fn try_acquire_upstream_key(&self, ctx: &mut GatewayContext) -> bool {
        if ctx.upstream_key_guard.is_some() {
            return true;
        }
        match self.state.runtime.upstream_pool().acquire() {
            Some(guard) => {
                ctx.upstream_miss = true;
                ctx.upstream_key_guard = Some(guard);
                true
            }
            None => {
                global_metrics().record_rejected("upstream_key_exhausted");
                false
            }
        }
    }
}

#[async_trait::async_trait]
impl ProxyHttp for GatewayProxy {
    type CTX = GatewayContext;

    fn new_ctx(&self) -> Self::CTX {
        GatewayContext::new(uuid::Uuid::new_v4().to_string())
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let req_header = session.req_header();

        if req_header.uri.path() == "/health"
            || req_header.uri.path() == "/healthz"
            || req_header.uri.path() == "/v1/healthz"
        {
            let _ = session.respond_error(200).await;
            return Ok(true);
        }

        if req_header.uri.path() == "/ready" {
            let status = if self.state.tiered_cache.ping().await {
                200
            } else {
                503
            };
            let _ = session.respond_error(status).await;
            return Ok(true);
        }

        let auth = req_header
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let provided_key = auth.strip_prefix("Bearer ").unwrap_or(auth);

        if is_models_endpoint(req_header.uri.path(), &req_header.method) {
            let (is_authorized, consumer_from_key) = self.authorize_client(provided_key, auth);
            if !is_authorized {
                let _ = session.respond_error(401).await;
                return Ok(true);
            }
            ctx.consumer = consumer_from_key;
            ctx.is_models_list = true;
            if !self.try_acquire_upstream_key(ctx) {
                let _ = session.respond_error(503).await;
                return Ok(true);
            }
            return Ok(false);
        }

        if req_header.uri.path() != "/v1/chat/completions"
            && req_header.uri.path() != "/chat/completions"
        {
            let _ = session.respond_error(404).await;
            return Ok(true);
        }

        if req_header.method != http::Method::POST {
            let _ = session.respond_error(405).await;
            return Ok(true);
        }

        let (is_authorized, consumer_from_key) = self.authorize_client(provided_key, auth);

        if !is_authorized {
            let _ = session.respond_error(401).await;
            return Ok(true);
        }

        match self.state.request_semaphore.clone().try_acquire_owned() {
            Ok(permit) => ctx.request_permit = Some(permit),
            Err(_) => {
                global_metrics().record_rejected("overloaded");
                let _ = session.respond_error(503).await;
                return Ok(true);
            }
        }

        ctx.authorization = Some(auth.to_string());
        ctx.consumer = consumer_from_key.or_else(|| {
            req_header
                .headers
                .get("x-consumer")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        });

        let conversation_id_from_header = req_header
            .headers
            .get("x-conversation-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

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

        let fallback_model = self
            .state
            .runtime
            .fallback_model
            .read()
            .map(|m| m.clone())
            .unwrap_or_else(|_| "deepseek-v4-pro".to_string());
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

        let upstream_base_url = self
            .state
            .runtime
            .upstream_base_url
            .read()
            .map(|u| u.clone())
            .unwrap_or_else(|_| "https://api.deepseek.com".to_string());
        let prepared = prepare_upstream_request(
            &payload,
            Some(&self.state.reasoning_store),
            &upstream_base_url,
            &fallback_model,
            &self.state.reasoning_config.thinking_mode,
            &self.state.reasoning_config.reasoning_effort,
            &self.state.reasoning_config.missing_reasoning_strategy,
            Some(REASONING_NAMESPACE_AUTH),
        );

        info!(
            request_id = %ctx.request_id,
            model = %prepared.original_model,
            upstream_model = %prepared.upstream_model,
            patched = prepared.patched_reasoning_messages,
            missing = prepared.missing_reasoning_messages,
            recovered = prepared.recovered_reasoning_messages,
            "Prepared upstream request"
        );

        if ctx.is_streaming {
            ctx.stream_accumulator = Some(StreamAccumulator::new());
            if self.state.reasoning_config.display_reasoning {
                ctx.display_adapter = Some(CursorReasoningDisplayAdapter::new(
                    self.state.reasoning_config.collapsible_reasoning,
                ));
            }
        }

        ctx.pending_recovery_notice = prepared.recovery_notice.clone();

        let new_body = serde_json::to_vec(&prepared.payload).unwrap_or_default();
        ctx.new_request_body = Some(new_body);

        let fingerprint = self
            .state
            .runtime
            .fingerprint
            .read()
            .map(|f| f.clone())
            .unwrap_or_default();
        if let Ok(cache_key) = crab_cache::generate_namespaced_cache_key_with_fingerprint(
            &full_body,
            self.state.cache_key_namespace.as_deref(),
            &fingerprint,
        ) {
            ctx.cache_key = Some(cache_key.clone());

            if let Some((entry, tier)) = self.state.tiered_cache.get(&cache_key).await {
                info!(
                    request_id = %ctx.request_id,
                    cache_key = %cache_key,
                    tier = ?tier,
                    "Cache hit, returning cached response"
                );

                ctx.cache_tier = Some(tier);
                ctx.cache_hit = Some(entry.clone());
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
                global_metrics().record_cost_saved(&ctx.model, ctx.consumer.as_deref(), tier, cost);

                if !send_cached_response(session, &entry, &ctx.model, ctx.is_streaming, tier).await
                {
                    let _ = session.respond_error(500).await;
                }

                return Ok(true);
            }

            if let Some(semantic_cache) = &self.state.semantic_cache {
                if let Some(payload_value) =
                    serde_json::from_slice::<serde_json::Value>(&full_body).ok()
                {
                    if let Some(messages) = payload_value.get("messages").and_then(|m| m.as_array())
                    {
                        if let Some(query_text) = build_semantic_query_text(messages) {
                            // Apply semantic gate before L2 search
                            let gate_decision = evaluate_semantic_gate(
                                &self.state.semantic_gate,
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

                            if gate_decision == GateDecision::Pass {
                                if let Some(entry) = semantic_cache.search(&query_text).await {
                                    // Model guard: verify the cached entry's model matches
                                    if entry.model != ctx.model {
                                        global_metrics().record_semantic_cache_rejected();
                                        debug!(
                                            request_id = %ctx.request_id,
                                            cached_model = %entry.model,
                                            request_model = %ctx.model,
                                            "Semantic cache candidate rejected by model guard",
                                        );
                                    } else {
                                        info!(
                                            request_id = %ctx.request_id,
                                            query_len = query_text.len(),
                                            "Semantic cache hit, returning cached response"
                                        );

                                        ctx.cache_tier = Some(CacheTier::L2Semantic);
                                        ctx.cache_hit = Some(entry.clone());
                                        global_metrics().record_cache_hit(
                                            CacheTier::L2Semantic,
                                            &ctx.model,
                                            ctx.consumer.as_deref(),
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
                                            CacheTier::L2Semantic,
                                            cost,
                                        );

                                        if !send_cached_response(
                                            session,
                                            &entry,
                                            &ctx.model,
                                            ctx.is_streaming,
                                            CacheTier::L2Semantic,
                                        )
                                        .await
                                        {
                                            let _ = session.respond_error(500).await;
                                        }

                                        return Ok(true);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            match self.state.coalescer.acquire(&cache_key).await {
                Ok(guard) => {
                    if !guard.is_leader() {
                        ctx.is_coalesced_follower = true;

                        if let Some((entry, tier)) = self.state.tiered_cache.get(&cache_key).await {
                            info!(
                                request_id = %ctx.request_id,
                                cache_key = %cache_key,
                                tier = ?tier,
                                "Follower found cached response after leader completed"
                            );

                            ctx.cache_tier = Some(tier);
                            ctx.cache_hit = Some(entry.clone());
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
                                tier,
                                cost,
                            );

                            if !send_cached_response(
                                session,
                                &entry,
                                &ctx.model,
                                ctx.is_streaming,
                                tier,
                            )
                            .await
                            {
                                let _ = session.respond_error(500).await;
                            }

                            return Ok(true);
                        } else {
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

        ctx.prepared_request = Some(prepared);

        if !self.try_acquire_upstream_key(ctx) {
            let _ = session.respond_error(503).await;
            return Ok(true);
        }

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        if ctx.is_models_list {
            let router = self
                .state
                .runtime
                .router
                .read()
                .map_err(|_| Error::new(ErrorType::InternalError))?;
            let backend = router
                .backends()
                .first()
                .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?;

            let mut peer = HttpPeer::new(backend.addr, true, backend.tls_sni.clone());
            let conn_config = self
                .state
                .runtime
                .conn_config
                .read()
                .map_err(|_| Error::new(ErrorType::InternalError))?
                .clone();
            apply_connection_options(&conn_config, &mut peer.options);
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

        let affinity_key = extract_affinity_key(&headers, &client_ip);

        let router = self
            .state
            .runtime
            .router
            .read()
            .map_err(|_| Error::new(ErrorType::InternalError))?;

        // Check if we have health information to filter by
        let backend = {
            let health = self
                .state
                .runtime
                .backend_health
                .read()
                .map_err(|_| Error::new(ErrorType::InternalError))?;
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

        let mut peer = HttpPeer::new(backend.addr, true, backend.tls_sni);
        let conn_config = self
            .state
            .runtime
            .conn_config
            .read()
            .map_err(|_| Error::new(ErrorType::InternalError))?
            .clone();
        apply_connection_options(&conn_config, &mut peer.options);

        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        let _ = upstream_request.insert_header("x-request-id", ctx.request_id.clone());
        if let Some(ref new_body) = ctx.new_request_body {
            let _ = upstream_request
                .insert_header(http::header::CONTENT_LENGTH, new_body.len().to_string());
        }

        if let Some(guard) = ctx.upstream_key_guard.as_ref() {
            let bearer = format!("Bearer {}", guard.bearer_secret());
            let _ = upstream_request.insert_header(http::header::AUTHORIZATION, bearer);
        }

        Ok(())
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if ctx.new_request_body.is_some() {
            if end_of_stream {
                if let Some(new_body) = ctx.new_request_body.take() {
                    *body = Some(bytes::Bytes::from(new_body));
                }
            } else {
                *body = None;
            }
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
        let pool = self.state.runtime.upstream_pool();
        let key_id = ctx
            .upstream_key_guard
            .as_ref()
            .map(|g| g.key_id().to_string());

        if status == 429 {
            if let Some(ref id) = key_id {
                pool.report_rate_limited(id);
                global_metrics().record_upstream_key_request(id, "rate_limited");
                if !ctx.is_streaming && ctx.upstream_retry_budget > 0 {
                    ctx.upstream_retry_budget = 0;
                    global_metrics().record_upstream_key_retry("cooldown_only");
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
            return Ok(());
        }

        let _ = upstream_response.insert_header("x-request-id", ctx.request_id.clone());
        let _ = upstream_response.insert_header("x-cache-status", "miss");

        ctx.upstream_start = Some(std::time::Instant::now());

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

        if let Some(data) = body.as_ref() {
            ctx.accumulated_body.extend_from_slice(data);

            if ctx.is_streaming {
                if ctx.ttft.is_none() {
                    if let Some(upstream_start) = ctx.upstream_start {
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

                if let (Some(prepared), Some(accumulator)) =
                    (&ctx.prepared_request, &mut ctx.stream_accumulator)
                {
                    let text = String::from_utf8_lossy(data);
                    for line in text.lines() {
                        let line_bytes = line.as_bytes();
                        let result = rewrite_sse_chunk(
                            line_bytes,
                            &prepared.original_model,
                            accumulator,
                            &prepared.cache_namespace,
                            &prepared.record_response_contexts,
                            &mut ctx.display_adapter,
                            ctx.pending_recovery_notice.as_deref(),
                            Some(&self.state.reasoning_store),
                        );

                        ctx.pending_recovery_notice = result.pending_recovery_notice;

                        if let Some(usage) = &result.chunk_usage {
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
                            ctx.total_tokens +=
                                usage_data.prompt_tokens + usage_data.completion_tokens;
                            record_usage(&usage_data, &ctx.model, ctx.consumer.as_deref());
                        }
                    }
                }

                let events = parse_sse_chunk(data);
                for event in &events {
                    if let Some(usage) = event.parse_usage() {
                        ctx.total_tokens += usage.prompt_tokens + usage.completion_tokens;
                        record_usage(&usage, &ctx.model, ctx.consumer.as_deref());
                    }
                }
            }
        }

        if end_of_stream && !ctx.is_streaming {
            if let Some(guard) = &ctx.coalesce_guard {
                guard.mark_completed();
            }

            if let Some(upstream_start) = ctx.upstream_start {
                let latency = upstream_start.elapsed();
                global_metrics().record_latency(
                    crab_metrics::LatencyKind::Upstream,
                    latency,
                    &ctx.model,
                    Some(crab_metrics::CacheTier::Miss),
                );
            }

            if let (Some(prepared), Some(accumulator)) =
                (&ctx.prepared_request, &mut ctx.stream_accumulator)
            {
                if let Some(store_reasoning) = Some(&self.state.reasoning_store) {
                    for (scope, prior_messages) in &prepared.record_response_contexts {
                        accumulator.store_reasoning(
                            store_reasoning,
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
                    ctx.pending_recovery_notice.as_deref(),
                    &prepared.record_response_contexts,
                    self.state.reasoning_config.display_reasoning,
                    self.state.reasoning_config.collapsible_reasoning,
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
                    ctx.total_tokens += usage_data.prompt_tokens + usage_data.completion_tokens;
                    record_usage(&usage_data, &ctx.model, ctx.consumer.as_deref());
                }

                if let Some(cache_key) = &ctx.cache_key {
                    let ttl_secs = self
                        .state
                        .tiered_cache
                        .resolve_ttl(&ctx.model, ctx.consumer.as_deref());

                    let entry = build_cache_entry(client_body.clone(), ctx.model.clone(), ttl_secs);

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
                                            client_body.clone(),
                                            ctx.model.clone(),
                                            ttl_secs,
                                        );

                                        tokio::spawn(async move {
                                            if let Err(e) = semantic_cache
                                                .insert(&query_text, &entry_clone)
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
            if let Some(guard) = &ctx.coalesce_guard {
                guard.mark_completed();
            }

            if let Some(upstream_start) = ctx.upstream_start {
                let latency = upstream_start.elapsed();
                global_metrics().record_latency(
                    crab_metrics::LatencyKind::Upstream,
                    latency,
                    &ctx.model,
                    Some(crab_metrics::CacheTier::Miss),
                );
            }

            if let (Some(cache_key), Some(accumulator)) = (&ctx.cache_key, &ctx.stream_accumulator)
            {
                let messages = accumulator.messages();
                if !messages.is_empty() {
                    let response_json = serde_json::to_string(&serde_json::json!({
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
                    }))
                    .unwrap_or_default();

                    if self.state.runtime.stream_cache_enabled() {
                        let ttl_secs = self
                            .state
                            .tiered_cache
                            .resolve_ttl(&ctx.model, ctx.consumer.as_deref());

                        let sse_body = ctx.accumulated_body.clone();
                        let max_sse = self.state.max_sse_cache_bytes;
                        let response_bytes = response_json.clone().into_bytes();
                        let entry_for_cache = if should_store_sse_body(sse_body.len(), max_sse) {
                            build_cache_entry_with_sse(
                                response_bytes.clone(),
                                sse_body,
                                ctx.model.clone(),
                                ttl_secs,
                            )
                        } else {
                            warn!(
                                sse_len = sse_body.len(),
                                limit = max_sse,
                                "SSE body exceeds max_sse_cache_bytes; caching JSON only"
                            );
                            global_metrics().record_stream_cache_sse_omitted("over_limit");
                            build_cache_entry(response_bytes, ctx.model.clone(), ttl_secs)
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
                                                response_json.into_bytes(),
                                                ctx.model.clone(),
                                                ttl_secs,
                                            );
                                            let query_text = query_text.to_string();

                                            tokio::spawn(async move {
                                                if let Err(e) = semantic_cache
                                                    .insert(&query_text, &entry_for_semantic)
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
        _session: &mut Session,
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
        } else {
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
                total_tokens = ctx.total_tokens,
                upstream_key_id = ?ctx.upstream_key_guard.as_ref().map(|g| g.key_id()),
                "Request completed"
            );

            if let Some(trace_logger) = &self.state.trace_logger {
                if let Some(body) = &ctx.original_request_body {
                    let entry = SanitizedLogEntry::from_request(
                        body,
                        ctx.conversation_id.clone(),
                        &ctx.model,
                        ctx.total_tokens as usize,
                        duration.as_secs_f64() * 1000.0,
                        ctx.cache_tier.is_some(),
                        ctx.cache_tier.map(|t| t.as_str().to_string()),
                    );
                    trace_logger.log(entry);
                }
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
    }
}

async fn send_cached_response(
    session: &mut Session,
    entry: &CacheEntry,
    model: &str,
    is_streaming: bool,
    cache_tier: CacheTier,
) -> bool {
    let response_body = &entry.response_body;
    if is_streaming {
        let sse_body = if let Some(ref saved) = entry.sse_body {
            saved.clone()
        } else {
            json_to_sse_stream(response_body, model)
        };
        let Some(header) = build_sse_response_header(sse_body.len(), cache_tier) else {
            warn!("Failed to build SSE cache response header");
            return false;
        };
        let _ = session
            .downstream_session
            .write_response_header(Box::new(header))
            .await;
        let _ = session
            .downstream_session
            .write_response_body(bytes::Bytes::from(sse_body), true)
            .await;
    } else {
        let Some(header) = build_json_response_header(response_body.len(), cache_tier) else {
            warn!("Failed to build JSON cache response header");
            return false;
        };
        let _ = session
            .downstream_session
            .write_response_header(Box::new(header))
            .await;
        let _ = session
            .downstream_session
            .write_response_body(bytes::Bytes::from(response_body.clone()), true)
            .await;
    }
    true
}

/// Whether to persist raw SSE bytes alongside the synthesized JSON completion.
pub fn should_store_sse_body(sse_len: usize, max_sse_cache_bytes: usize) -> bool {
    max_sse_cache_bytes > 0 && sse_len <= max_sse_cache_bytes
}

fn build_cache_entry(response_body: Vec<u8>, model: String, ttl_secs: u64) -> CacheEntry {
    CacheEntry {
        response_body,
        model,
        usage: UsageInfo::default(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        ttl_secs,
        sse_body: None,
    }
}

fn build_cache_entry_with_sse(
    response_body: Vec<u8>,
    sse_body: Vec<u8>,
    model: String,
    ttl_secs: u64,
) -> CacheEntry {
    CacheEntry {
        response_body,
        sse_body: Some(sse_body),
        model,
        usage: UsageInfo::default(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        ttl_secs,
    }
}

fn record_usage(usage: &UsageData, model: &str, consumer: Option<&str>) {
    global_metrics().record_upstream_usage(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.prompt_cache_hit_tokens,
        usage.prompt_cache_miss_tokens,
        model,
        consumer,
    );

    if usage.prompt_cache_hit_tokens > 0 {
        global_metrics().record_upstream_prompt_cache(
            "hit",
            usage.prompt_cache_hit_tokens,
            model,
            consumer,
        );
    }
    if usage.prompt_cache_miss_tokens > 0 {
        global_metrics().record_upstream_prompt_cache(
            "miss",
            usage.prompt_cache_miss_tokens,
            model,
            consumer,
        );
    }
}

fn is_models_endpoint(path: &str, method: &http::Method) -> bool {
    *method == http::Method::GET && (path == "/models" || path == "/v1/models")
}

fn apply_connection_options(config: &ConnectionConfig, options: &mut PeerOptions) {
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

    if let Some(idle_secs) = config.idle_timeout_secs {
        options.idle_timeout = Some(Duration::from_secs(idle_secs));
    }

    if let Some(ping_secs) = config.h2_ping_interval_secs
        && ping_secs > 0
    {
        options.h2_ping_interval = Some(Duration::from_secs(ping_secs));
    }
}

fn cache_status_header(tier: CacheTier) -> &'static str {
    match tier {
        CacheTier::L0Moka => "HIT_L0",
        CacheTier::L1Redis => "HIT_L1",
        CacheTier::L2Semantic => "HIT_L2",
        CacheTier::Miss => "miss",
    }
}

fn insert_response_header(
    header: &mut pingora_http::ResponseHeader,
    name: &'static str,
    value: impl ToString,
) -> Option<()> {
    header.insert_header(name, value.to_string()).ok()
}

fn build_json_response_header(
    body_len: usize,
    cache_tier: CacheTier,
) -> Option<pingora_http::ResponseHeader> {
    use pingora_http::ResponseHeader;
    let mut header = ResponseHeader::build(http::StatusCode::OK, Some(5)).ok()?;
    insert_response_header(&mut header, "content-type", "application/json")?;
    insert_response_header(&mut header, "content-length", body_len.to_string())?;
    insert_response_header(
        &mut header,
        "x-cache-status",
        cache_status_header(cache_tier),
    )?;
    insert_response_header(&mut header, "connection", "close")?;
    Some(header)
}

fn build_sse_response_header(
    body_len: usize,
    cache_tier: CacheTier,
) -> Option<pingora_http::ResponseHeader> {
    use pingora_http::ResponseHeader;
    let mut header = ResponseHeader::build(http::StatusCode::OK, Some(5)).ok()?;
    insert_response_header(&mut header, "content-type", "text/event-stream")?;
    insert_response_header(&mut header, "content-length", body_len.to_string())?;
    insert_response_header(&mut header, "cache-control", "no-cache")?;
    insert_response_header(
        &mut header,
        "x-cache-status",
        cache_status_header(cache_tier),
    )?;
    insert_response_header(&mut header, "connection", "close")?;
    Some(header)
}

fn json_to_sse_stream(json_body: &[u8], model: &str) -> Vec<u8> {
    use serde_json::json;

    let value: serde_json::Value = match serde_json::from_slice(json_body) {
        Ok(v) => v,
        Err(_) => return json_body.to_vec(),
    };

    let choices = value
        .get("choices")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let usage = value.get("usage").cloned();

    let mut sse_output = Vec::new();

    for (idx, choice) in choices.iter().enumerate() {
        let delta = json!({
            "index": idx,
            "delta": choice.get("message").cloned().unwrap_or(json!({})),
            "finish_reason": choice.get("finish_reason").cloned().unwrap_or(serde_json::Value::Null)
        });

        let event_data = json!({
            "id": format!("chatcmpl-cache-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion.chunk",
            "created": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "model": model,
            "choices": [delta]
        });

        sse_output.extend_from_slice(format!("data: {}\n\n", event_data).as_bytes());
    }

    if let Some(usage_data) = usage {
        let usage_event = json!({
            "id": format!("chatcmpl-cache-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion.chunk",
            "created": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "model": model,
            "choices": [],
            "usage": usage_data
        });
        sse_output.extend_from_slice(format!("data: {}\n\n", usage_event).as_bytes());
    }

    sse_output.extend_from_slice(b"data: [DONE]\n\n");

    sse_output
}

/// Build a stable concatenated query text for semantic cache from request messages.
///
/// Extracts system message (if present) and all user/assistant message text content,
/// joining them with newlines. If any content field is a non-string array, it is
/// serialized as a JSON subset. Returns None if no usable text is found.
fn build_semantic_query_text(messages: &[serde_json::Value]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "system" || role == "user" || role == "assistant" {
            let content = msg.get("content");
            let text = match content {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(arr)) => {
                    // Array content: extract text parts or serialize as JSON subset
                    let sub: Vec<String> = arr
                        .iter()
                        .filter_map(|part| {
                            part.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect();
                    if sub.is_empty() {
                        // No text parts found, skip this message
                        continue;
                    }
                    sub.join(" ")
                }
                _ => continue,
            };
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_should_store_sse_body() {
        assert!(should_store_sse_body(100, 4_194_304));
        assert!(!should_store_sse_body(4_194_305, 4_194_304));
        assert!(!should_store_sse_body(100, 0));
        assert!(should_store_sse_body(0, 1024));
    }

    #[test]
    fn test_build_json_response_header_ok() {
        let header = build_json_response_header(42, CacheTier::L0Moka).expect("header");
        assert_eq!(header.status.as_u16(), 200);
    }

    #[test]
    fn test_build_sse_response_header_ok() {
        let header = build_sse_response_header(100, CacheTier::L1Redis).expect("header");
        assert_eq!(header.status.as_u16(), 200);
    }
}
