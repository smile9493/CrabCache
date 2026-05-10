use crate::context::{ConnectionConfig, GatewayContext, GatewayState};
use crate::sse::{parse_sse_chunk, UsageData};
use crab_cache::{CacheEntry, UsageInfo};
use crab_metrics::global_metrics;
use crab_reasoning::{
    prepare_upstream_request, rewrite_response_body, rewrite_sse_chunk,
    CursorReasoningDisplayAdapter, StreamAccumulator,
};
use crab_route::extract_affinity_key;
use http::HeaderMap;
use pingora_core::prelude::*;
use pingora_core::protocols::l4::ext::TcpKeepalive;
use pingora_core::upstreams::peer::PeerOptions;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use sha2::{Sha256, Digest};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

pub struct GatewayProxy {
    state: Arc<GatewayState>,
}

impl GatewayProxy {
    pub fn new(state: Arc<GatewayState>) -> Self {
        Self { state }
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

        if req_header.uri.path() == "/health" || req_header.uri.path() == "/healthz" || req_header.uri.path() == "/v1/healthz" {
            let _ = session.respond_error(200).await;
            return Ok(true);
        }

        if is_models_endpoint(req_header.uri.path(), &req_header.method) {
            ctx.is_models_list = true;
            return Ok(false);
        }

        if req_header.uri.path() != "/v1/chat/completions" && req_header.uri.path() != "/chat/completions" {
            let _ = session.respond_error(404).await;
            return Ok(true);
        }

        if req_header.method != http::Method::POST {
            let _ = session.respond_error(405).await;
            return Ok(true);
        }

        let auth = req_header
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let provided_key = auth.strip_prefix("Bearer ").unwrap_or(auth);

        let (is_authorized, consumer_from_key) = if let Some(stored_key) = self.state.keys.get(provided_key) {
            let key = stored_key.value();
            (key.enabled, Some(key.name.clone()))
        } else {
            (auth.ends_with(&self.state.api_key), None)
        };

        if !is_authorized {
            let _ = session.respond_error(401).await;
            return Ok(true);
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
        loop {
            match session.downstream_session.read_request_body().await? {
                Some(data) => full_body.extend_from_slice(&data),
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

        ctx.model = payload.get("model").and_then(|m| m.as_str()).unwrap_or(&self.state.fallback_model).to_string();
        ctx.is_streaming = payload.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

        ctx.conversation_id = payload.get("conversation_id")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .or(conversation_id_from_header);

        let prepared = prepare_upstream_request(
            &payload,
            Some(&self.state.reasoning_store),
            &self.state.upstream_base_url,
            &self.state.fallback_model,
            &self.state.reasoning_config.thinking_mode,
            &self.state.reasoning_config.reasoning_effort,
            &self.state.reasoning_config.missing_reasoning_strategy,
            ctx.authorization.as_deref(),
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

        if let Ok(cache_key) = crab_cache::generate_cache_key(&full_body) {
            ctx.cache_key = Some(cache_key.clone());

            if let Some((entry, tier)) = self.state.tiered_cache.get(&cache_key).await {
                info!(
                    request_id = %ctx.request_id,
                    cache_key = %cache_key,
                    tier = ?tier,
                    "Cache hit, returning cached response"
                );

                ctx.cache_hit = Some(entry.clone());

                let response_body = entry.response_body.clone();
                let is_stream = ctx.is_streaming;

                if is_stream {
                    let sse_body = json_to_sse_stream(&response_body, &ctx.model);
                    let header = build_sse_response_header(sse_body.len());
                    let _ = session.downstream_session.write_response_header(Box::new(header)).await;
                    let _ = session.downstream_session.write_response_body(bytes::Bytes::from(sse_body), true).await;
                } else {
                    let header = build_json_response_header(response_body.len());
                    let _ = session.downstream_session.write_response_header(Box::new(header)).await;
                    let _ = session.downstream_session.write_response_body(bytes::Bytes::from(response_body), true).await;
                }

                return Ok(true);
            }

            if let Some(semantic_cache) = &self.state.semantic_cache {
                if let Some(payload_value) = serde_json::from_slice::<serde_json::Value>(&full_body).ok() {
                    if let Some(messages) = payload_value.get("messages").and_then(|m| m.as_array()) {
                        let last_message = messages.last().and_then(|m| m.get("content")).and_then(|c| c.as_str());
                        
                        if let Some(query_text) = last_message {
                            if let Some(entry) = semantic_cache.search(query_text).await {
                                info!(
                                    request_id = %ctx.request_id,
                                    query_len = query_text.len(),
                                    "Semantic cache hit, returning cached response"
                                );

                                ctx.cache_hit = Some(entry.clone());

                                let response_body = entry.response_body.clone();
                                let is_stream = ctx.is_streaming;

                                if is_stream {
                                    let sse_body = json_to_sse_stream(&response_body, &ctx.model);
                                    let header = build_sse_response_header(sse_body.len());
                                    let _ = session.downstream_session.write_response_header(Box::new(header)).await;
                                    let _ = session.downstream_session.write_response_body(bytes::Bytes::from(sse_body), true).await;
                                } else {
                                    let header = build_json_response_header(response_body.len());
                                    let _ = session.downstream_session.write_response_header(Box::new(header)).await;
                                    let _ = session.downstream_session.write_response_body(bytes::Bytes::from(response_body), true).await;
                                }

                                return Ok(true);
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

                            ctx.cache_hit = Some(entry.clone());

                            let response_body = entry.response_body.clone();
                            let is_stream = ctx.is_streaming;

                            if is_stream {
                                let sse_body = json_to_sse_stream(&response_body, &ctx.model);
                                let header = build_sse_response_header(sse_body.len());
                                let _ = session.downstream_session.write_response_header(Box::new(header)).await;
                                let _ = session.downstream_session.write_response_body(bytes::Bytes::from(sse_body), true).await;
                            } else {
                                let header = build_json_response_header(response_body.len());
                                let _ = session.downstream_session.write_response_header(Box::new(header)).await;
                                let _ = session.downstream_session.write_response_body(bytes::Bytes::from(response_body), true).await;
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

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        if ctx.is_models_list {
            let backend = self
                .state
                .router
                .backends()
                .first()
                .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?;

            let mut peer = HttpPeer::new(backend.addr, true, backend.tls_sni.clone());
            apply_connection_options(&self.state.conn_config, &mut peer.options);
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

        let backend = self
            .state
            .router
            .select(affinity_key.as_bytes())
            .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?;

        debug!(
            request_id = %ctx.request_id,
            backend = %backend.name,
            affinity_key = %affinity_key,
            "Selected upstream backend"
        );

        let mut peer = HttpPeer::new(backend.addr, true, backend.tls_sni.clone());
        apply_connection_options(&self.state.conn_config, &mut peer.options);

        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        upstream_request
            .insert_header("x-request-id", ctx.request_id.clone())
            .unwrap();

        if let Some(ref new_body) = ctx.new_request_body {
            upstream_request
                .insert_header(http::header::CONTENT_LENGTH, new_body.len().to_string())
                .unwrap();
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
        if ctx.is_models_list {
            return Ok(());
        }

        let status = upstream_response.status.as_u16();
        if status >= 400 {
            return Ok(());
        }

        upstream_response
            .insert_header("x-request-id", ctx.request_id.clone())
            .unwrap();

        upstream_response
            .insert_header("x-cache-status", "miss")
            .unwrap();

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
                        global_metrics().record_latency(
                            crab_metrics::LatencyKind::TTFT,
                            ctx.ttft.unwrap(),
                            Some(crab_metrics::CacheTier::Miss),
                        );
                    }
                }

                if let (Some(prepared), Some(accumulator)) = (&ctx.prepared_request, &mut ctx.stream_accumulator) {
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
                                prompt_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                                completion_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                                prompt_cache_hit_tokens: usage.get("prompt_cache_hit_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                                prompt_cache_miss_tokens: usage.get("prompt_cache_miss_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                            };
                            ctx.total_tokens += usage_data.prompt_tokens + usage_data.completion_tokens;
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
                    Some(crab_metrics::CacheTier::Miss),
                );
            }

            if let (Some(prepared), Some(accumulator)) = (&ctx.prepared_request, &mut ctx.stream_accumulator) {
                if let Some(store_reasoning) = Some(&self.state.reasoning_store) {
                    for (scope, prior_messages) in &prepared.record_response_contexts {
                        accumulator.store_reasoning(store_reasoning, scope, &prepared.cache_namespace, prior_messages);
                    }
                }
            }

            if let Some(ref prepared) = ctx.prepared_request {
                if let Some(rewritten) = rewrite_response_body(
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
                    *body = Some(bytes::Bytes::from(rewritten));
                }
            }

            if let Ok(body_value) = serde_json::from_slice::<serde_json::Value>(&ctx.accumulated_body) {
                if let Some(usage) = body_value.get("usage") {
                    let usage_data = UsageData {
                        prompt_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        completion_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        prompt_cache_hit_tokens: usage.get("prompt_cache_hit_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        prompt_cache_miss_tokens: usage.get("prompt_cache_miss_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                    };
                    ctx.total_tokens += usage_data.prompt_tokens + usage_data.completion_tokens;
                    record_usage(&usage_data, &ctx.model, ctx.consumer.as_deref());
                }

                if let Some(cache_key) = &ctx.cache_key {
                    let entry = CacheEntry {
                        response_body: ctx.accumulated_body.clone(),
                        model: ctx.model.clone(),
                        usage: UsageInfo::default(),
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        ttl_secs: 3600,
                    };

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
                            if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(original_body) {
                                if let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) {
                                    if let Some(query_text) = messages.last()
                                        .and_then(|m| m.get("content"))
                                        .and_then(|c| c.as_str())
                                    {
                                        let semantic_cache = semantic_cache.clone();
                                        let entry_clone = CacheEntry {
                                            response_body: ctx.accumulated_body.clone(),
                                            model: ctx.model.clone(),
                                            usage: UsageInfo::default(),
                                            created_at: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                            ttl_secs: 3600,
                                        };
                                        let query_text = query_text.to_string();
                                        
                                        tokio::spawn(async move {
                                            if let Err(e) = semantic_cache.insert(&query_text, &entry_clone).await {
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
                    Some(crab_metrics::CacheTier::Miss),
                );
            }

            if let (Some(cache_key), Some(accumulator)) = (&ctx.cache_key, &ctx.stream_accumulator) {
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
                    })).unwrap_or_default();

                    let tiered_cache = self.state.tiered_cache.clone();
                    let cache_key = cache_key.clone();
                    let model = ctx.model.clone();
                    let consumer = ctx.consumer.clone();
                    let entry_for_cache = CacheEntry {
                        response_body: response_json.clone().into_bytes(),
                        model: ctx.model.clone(),
                        usage: UsageInfo::default(),
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        ttl_secs: 3600,
                    };
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
                            if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(original_body) {
                                if let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) {
                                    if let Some(query_text) = messages.last()
                                        .and_then(|m| m.get("content"))
                                        .and_then(|c| c.as_str())
                                    {
                                        let semantic_cache = semantic_cache.clone();
                                        let entry_for_semantic = CacheEntry {
                                            response_body: response_json.into_bytes(),
                                            model: ctx.model.clone(),
                                            usage: UsageInfo::default(),
                                            created_at: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                            ttl_secs: 3600,
                                        };
                                        let query_text = query_text.to_string();
                                        
                                        tokio::spawn(async move {
                                            if let Err(e) = semantic_cache.insert(&query_text, &entry_for_semantic).await {
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
                cache_hit = ctx.cache_hit.is_some(),
                is_streaming = ctx.is_streaming,
                consumer = ?ctx.consumer,
                conversation_id = ?ctx.conversation_id,
                total_tokens = ctx.total_tokens,
                "Request completed"
            );
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

fn record_usage(usage: &UsageData, model: &str, consumer: Option<&str>) {
    global_metrics().record_upstream_usage(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.prompt_cache_hit_tokens,
        usage.prompt_cache_miss_tokens,
        model,
        consumer,
    );
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

fn build_json_response_header(body_len: usize) -> pingora_http::ResponseHeader {
    use pingora_http::ResponseHeader;
    let mut header = ResponseHeader::build(http::StatusCode::OK, Some(5)).unwrap();
    header.insert_header(http::header::CONTENT_TYPE, "application/json").unwrap();
    header.insert_header(http::header::CONTENT_LENGTH, body_len.to_string()).unwrap();
    header.insert_header("x-cache-status", "HIT").unwrap();
    header.insert_header(http::header::CONNECTION, "close").unwrap();
    header
}

fn build_sse_response_header(body_len: usize) -> pingora_http::ResponseHeader {
    use pingora_http::ResponseHeader;
    let mut header = ResponseHeader::build(http::StatusCode::OK, Some(5)).unwrap();
    header.insert_header(http::header::CONTENT_TYPE, "text/event-stream").unwrap();
    header.insert_header(http::header::CONTENT_LENGTH, body_len.to_string()).unwrap();
    header.insert_header(http::header::CACHE_CONTROL, "no-cache").unwrap();
    header.insert_header("x-cache-status", "HIT").unwrap();
    header.insert_header(http::header::CONNECTION, "close").unwrap();
    header
}

fn json_to_sse_stream(json_body: &[u8], model: &str) -> Vec<u8> {
    use serde_json::json;

    let value: serde_json::Value = match serde_json::from_slice(json_body) {
        Ok(v) => v,
        Err(_) => return json_body.to_vec(),
    };

    let choices = value.get("choices").and_then(|c| c.as_array()).cloned().unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_context_new() {
        let ctx = GatewayContext::new("test-id".to_string());
        assert_eq!(ctx.request_id, "test-id");
        assert!(!ctx.is_streaming);
        assert!(ctx.cache_key.is_none());
        assert!(ctx.cache_hit.is_none());
        assert!(ctx.original_request_body.is_none());
        assert!(ctx.prepared_request.is_none());
    }
}
