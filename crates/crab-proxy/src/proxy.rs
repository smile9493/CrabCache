const GLOBAL_RATE_KEY: &str = "__global_gateway_rps__";

use crate::cache_helpers::{
    build_semantic_query_text, cache_entry_matches_stream_mode, tiered_exact_lookup,
};
use crate::cache_response::send_cached_response;
use crate::connection_helpers::apply_connection_options;
use crate::context::{GatewayContext, GatewayState, ReasoningConfig};
use crate::debug_agent_log;
use crate::metrics_helpers::timeline_stamp;
use crate::runtime::RuntimeConfig;
use crab_metrics::{CacheTier, global_metrics};
use crab_pipeline::RequestPipeline;
use crab_semantic::{GateDecision, evaluate_semantic_gate};
use pingora_core::prelude::*;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

pub struct GatewayProxy {
    pub(crate) state: Arc<GatewayState>,
}

impl GatewayProxy {
    pub(crate) fn is_mimo_pipeline(p: RequestPipeline) -> bool {
        matches!(
            p,
            RequestPipeline::MimoTokenPlanRelay | RequestPipeline::MimoPaygRelay
        )
    }

    pub fn new(state: Arc<GatewayState>) -> Self {
        Self { state }
    }

    pub(crate) fn reasoning_config(&self) -> ReasoningConfig {
        self.state.reasoning_config.read().clone()
    }

    pub(crate) fn authorize_client(
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

    pub(crate) fn domain_policy_fields(
        &self,
        domain: Option<&str>,
    ) -> (Option<String>, Option<String>) {
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

    pub(crate) fn active_upstream_profile(
        &self,
        ctx: &GatewayContext,
    ) -> Arc<crate::upstream_profile::UpstreamProfileRuntime> {
        ctx.upstream_profile_id
            .as_deref()
            .and_then(|id| self.state.runtime.profile(id))
            .unwrap_or_else(|| self.state.runtime.default_profile())
    }

    pub(crate) fn create_upstream_peer(
        &self,
        addr: std::net::SocketAddr,
        tls_sni: &str,
        ctx: &mut GatewayContext,
    ) -> HttpPeer {
        ctx.upstream.host = Some(tls_sni.to_string());
        let mut peer = HttpPeer::new(addr, true, tls_sni.to_string());
        let conn_config = self.state.runtime.conn_config.read().clone();
        apply_connection_options(&conn_config, &mut peer.options);
        peer
    }

    /// Full JSON parse (deferred for MiMo until cache miss or prepare).
    pub(crate) fn ensure_client_payload(
        &self,
        ctx: &mut GatewayContext,
        body: &[u8],
        pipeline: Option<&str>,
    ) -> Result<Arc<serde_json::Value>, serde_json::Error> {
        if let Some(p) = &ctx.parsed_request_payload {
            return Ok(p.clone());
        }
        let parse_start = Instant::now();
        let parsed = match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(v) => Arc::new(v),
            Err(e) => return Err(e),
        };
        let parse_elapsed = parse_start.elapsed();
        global_metrics().record_request_body_stage(
            "json_parse_client",
            parse_elapsed,
            body.len(),
            pipeline,
        );
        ctx.parsed_request_payload = Some(parsed.clone());
        timeline_stamp(&mut ctx.timeline.json_parse_done);
        Ok(parsed)
    }

    /// Exact tiered cache lookup + response before full JSON parse (MiMo / GenericRelay).
    pub(crate) async fn try_early_exact_cache(
        &self,
        session: &mut Session,
        ctx: &mut GatewayContext,
        cache_key: &str,
        display_reasoning: bool,
    ) -> Result<bool, pingora_core::Error> {
        ctx.exact_cache_probed = true;
        let tiered_exact = tiered_exact_lookup(
            &self.state.tiered_cache,
            cache_key,
            ctx.consumer.as_deref(),
            ctx.domain.as_deref(),
            ctx.is_streaming,
        )
        .await;
        timeline_stamp(&mut ctx.timeline.cache_lookup_done);

        let Some((entry, tier)) = tiered_exact else {
            return Ok(false);
        };

        let sent_ok = send_cached_response(
            session,
            &entry,
            &ctx.model,
            ctx.is_streaming,
            tier,
            display_reasoning,
        )
        .await;

        if !sent_ok {
            return Ok(false);
        }

        ctx.cache_tier = Some(tier);
        ctx.cache_hit = Some(entry.clone());
        ctx.tokens.last_input = entry.usage.prompt_tokens;
        ctx.tokens.last_output = entry.usage.completion_tokens;
        if let Some(body) = &ctx.original_request_body {
            crate::cache_revalidate::maybe_spawn_swr(session, cache_key, &entry, body);
        }
        global_metrics().record_latency(
            crab_metrics::LatencyKind::CacheFetch,
            ctx.request_start.elapsed(),
            &ctx.model,
            Some(tier),
        );
        let cost = self.state.pricing.read().cost_saved_usd(
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
        global_metrics().record_full_response_cache_hit(tier.as_str(), ctx.domain.as_deref());
        global_metrics().record_request_saved_by_cache(ctx.domain.as_deref());
        Ok(true)
    }

    pub(crate) fn try_acquire_upstream_key(&self, ctx: &mut GatewayContext) -> bool {
        if ctx.upstream.key_guard.is_some() {
            return true;
        }
        let profile = self.active_upstream_profile(ctx);
        let pool = profile.resolve_upstream_pool();
        let available_before = pool.available_count();
        let total = pool.len();
        let canonical_model = crab_pipeline::canonicalize_client_model(&ctx.model);
        let upstream_model = if profile.provider == crab_pipeline::UpstreamProvider::Codex {
            crate::codex::resolve_codex_upstream_model(&canonical_model).to_string()
        } else {
            canonical_model
        };

        // MiMo conversation-level key binding: try bound key first.
        let features = self.state.features.read();
        if ctx.request_pipeline.map_or(false, Self::is_mimo_pipeline) && features.mimo_key_binding {
            if let Some(ref binding_store) = self.state.key_binding_store {
                let stable_session = ctx
                    .conversation_id
                    .as_deref()
                    .or(ctx.prompt_cache_key.as_deref())
                    .or(ctx.session_fingerprint.as_deref())
                    .or(ctx.client_key_fingerprint.as_deref());

                if let Some(sid) = stable_session {
                    if let Some(binding) = binding_store.get(sid) {
                        let max_inflight = features.mimo_key_max_inflight;
                        // Bound key available and under concurrency limit?
                        if max_inflight == 0 || pool.inflight_of(&binding.key_id) < max_inflight {
                            if let Some(guard) = pool.acquire_specific(&binding.key_id) {
                                binding_store.touch(sid);
                                ctx.upstream.miss = true;
                                ctx.upstream.key_guard = Some(guard);
                                global_metrics().record_key_binding_event("hit");
                                tracing::debug!(
                                    request_id = %ctx.request_id,
                                    session_id = %sid,
                                    key_id = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                                    "MiMo key binding hit: reusing bound key"
                                );
                                return true;
                            }
                        }
                        // Bound key full or unavailable: overflow to another key.
                        if let Some(guard) = pool.acquire_excluding_key(&binding.key_id) {
                            ctx.upstream.miss = true;
                            ctx.upstream.key_guard = Some(guard);
                            global_metrics().record_key_binding_event("spill");
                            tracing::debug!(
                                request_id = %ctx.request_id,
                                session_id = %sid,
                                bound_key_id = %binding.key_id,
                                overflow_key_id = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                                "MiMo key binding overflow: bound key full, using alternative"
                            );
                            return true;
                        }
                        // All other keys exhausted too.
                        tracing::warn!(
                            request_id = %ctx.request_id,
                            session_id = %sid,
                            "MiMo key binding: all keys exhausted (bound + overflow)"
                        );
                        global_metrics().record_rejected("upstream_key_exhausted");
                        global_metrics().record_rejection_by_source("upstream");
                        return false;
                    }
                    // No binding for this session: create one.
                    if let Some(guard) = pool.acquire_for_upstream_model(&upstream_model, false)
                        .or_else(|| pool.acquire())
                    {
                        let kid = guard.key_id().to_string();
                        binding_store.put(sid.to_string(), kid);
                        ctx.upstream.miss = true;
                        ctx.upstream.key_guard = Some(guard);
                        global_metrics().record_key_binding_event("miss");
                        tracing::debug!(
                            request_id = %ctx.request_id,
                            session_id = %sid,
                            key_id = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                            "MiMo key binding: new binding created"
                        );
                        return true;
                    }
                    // No keys available at all.
                    tracing::warn!(
                        request_id = %ctx.request_id,
                        session_id = %sid,
                        "MiMo key binding: no upstream keys available for new binding"
                    );
                    global_metrics().record_rejected("upstream_key_exhausted");
                    global_metrics().record_rejection_by_source("upstream");
                    return false;
                }
                // No stable session id available: fall through to default logic.
            }
        }

        // Default acquire logic (Codex, non-MiMo, or MiMo without key binding).
        let guard = if profile.provider == crab_pipeline::UpstreamProvider::Codex {
            pool.acquire_for_upstream_model(&upstream_model, true)
                .or_else(|| pool.acquire_codex_oauth())
        } else {
            pool.acquire_for_upstream_model(&upstream_model, false)
                .or_else(|| pool.acquire())
        };
        match guard {
            Some(guard) => {
                ctx.upstream.miss = true;
                ctx.upstream.key_guard = Some(guard);
                if profile.provider == crab_pipeline::UpstreamProvider::Codex {
                    tracing::info!(
                        request_id = %ctx.request_id,
                        client_model = %ctx.model,
                        upstream_model = %upstream_model,
                        key_id = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                        pool_available = pool.available_count(),
                        "Codex upstream key selected for model"
                    );
                }
                // #region agent log
                debug_agent_log(
                    "UPKEY1",
                    "proxy.rs:try_acquire_upstream_key",
                    "acquired upstream key guard",
                    serde_json::json!({
                        "request_id": ctx.request_id,
                        "upstream_profile": ctx.upstream_profile_id,
                        "pool_total": total,
                        "pool_available_before": available_before,
                        "pool_available_after": pool.available_count(),
                        "key_id": ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                    }),
                );
                // #endregion
                true
            }
            None => {
                global_metrics().record_rejected("upstream_key_exhausted");
                global_metrics().record_rejection_by_source("upstream");
                // #region agent log
                debug_agent_log(
                    "UPKEY2",
                    "proxy.rs:try_acquire_upstream_key",
                    "failed to acquire upstream key guard",
                    serde_json::json!({
                        "request_id": ctx.request_id,
                        "upstream_profile": ctx.upstream_profile_id,
                        "pool_total": total,
                        "pool_available": available_before,
                    }),
                );
                // #endregion
                false
            }
        }
    }

    /// Attempt L2 semantic cache lookup. Returns `true` if the response was served
    /// from the semantic cache (including error responses), `false` if no hit.
    pub(crate) async fn try_l2_semantic_cache(
        &self,
        session: &mut Session,
        ctx: &mut GatewayContext,
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
        let Some(payload_value) = ctx.parsed_request_payload.as_ref() else {
            return false;
        };
        let Some(messages) = payload_value.get("messages").and_then(|m| m.as_array()) else {
            return false;
        };
        let Some(query_text) = build_semantic_query_text(messages) else {
            return false;
        };

        // Apply semantic gate before L2 search
        let gate_decision = evaluate_semantic_gate(&gate, &query_text, ctx.cache_hit.is_some());
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

        let Some(entry) = semantic_cache
            .search(&query_text, ctx.project_id.as_deref())
            .await
        else {
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
            // Spawn stale-while-revalidate if entry is stale.
            if let (Some(body), Some(cache_key)) =
                (&ctx.original_request_body, ctx.cache_key.as_deref())
            {
                crate::cache_revalidate::maybe_spawn_swr(session, cache_key, &entry, body);
            }
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
            let cost = self.state.pricing.read().cost_saved_usd(
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
            global_metrics().record_full_response_cache_hit("L2_semantic", ctx.domain.as_deref());
            global_metrics().record_request_saved_by_cache(ctx.domain.as_deref());
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

    /// Enable subrequest spawning for stale-while-revalidate cache refresh.
    fn allow_spawning_subrequest(&self, _session: &Session, _ctx: &Self::CTX) -> bool {
        true
    }

    fn defer_upstream_request_body(&self, _session: &Session, ctx: &Self::CTX) -> bool {
        should_defer_upstream_request_body(ctx)
    }

    fn skip_upstream_trailing_empty_eos(&self, _session: &Session, ctx: &Self::CTX) -> bool {
        should_skip_upstream_trailing_empty_eos(ctx)
    }

    fn defer_upstream_body_end_stream(&self, session: &mut Session, ctx: &Self::CTX) -> bool {
        should_upstream_body_end_stream(session, ctx)
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        crate::phases::request_filter::run(self, session, ctx).await
    }

    #[tracing::instrument(skip_all, fields(request_id = %ctx.request_id))]
    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        crate::phases::upstream_peer::run(self, session, ctx).await
    }

    #[tracing::instrument(skip_all, fields(request_id = %ctx.request_id))]
    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        crate::phases::upstream_request::run_upstream_request_filter(
            self,
            session,
            upstream_request,
            ctx,
        )
        .await
    }

    async fn request_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        crate::phases::upstream_request::run_request_body_filter(
            self,
            session,
            body,
            end_of_stream,
            ctx,
        )
        .await
    }

    #[tracing::instrument(skip_all, fields(request_id = %ctx.request_id))]
    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        crate::phases::response_filter::run(self, session, upstream_response, ctx).await
    }

    fn upstream_response_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<Option<Duration>> {
        crate::phases::response_body::run(self, session, body, end_of_stream, ctx)
    }

    #[tracing::instrument(skip_all, fields(request_id = %ctx.request_id))]
    async fn logging(
        &self,
        session: &mut Session,
        error: Option<&pingora_core::Error>,
        ctx: &mut Self::CTX,
    ) {
        crate::phases::logging::run(self, session, error, ctx).await
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

/// Build a response preview string from the best available source in the context.
///
/// Priority:
/// 1. Cache hit → `entry.response_body`
/// 2. Non-streaming accumulated body → `ctx.response_body_preview`
/// 3. Streaming SSE body → `ctx.stream.client_sse_body`
///
/// Returns `None` when no data is available or all sources are empty.
pub(crate) fn build_response_preview(ctx: &GatewayContext, max_bytes: usize) -> Option<String> {
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
        format!(
            "{}...<truncated {}>",
            &raw[..max_bytes],
            raw.len() - max_bytes
        )
    } else {
        raw.to_string()
    };

    Some(truncated)
}

/// Whether Pingora should defer upstream body I/O until `request_body_filter` supplies bytes.
///
/// MiMo direct passthrough: prefix sniff + chunk relay. Without this, H1/H2 may skip the
/// initial body pipe or send an empty END_STREAM before the armed prefix is forwarded —
/// upstream sees invalid/empty JSON (`400 Param Incorrect`).
pub fn should_defer_upstream_request_body(ctx: &GatewayContext) -> bool {
    ctx.request_passthrough.active && !ctx.request_passthrough.finalized
}

/// Whether the current defer-path upstream chunk should end the request body.
pub fn should_upstream_body_end_stream(session: &mut Session, ctx: &GatewayContext) -> bool {
    if ctx.request_passthrough.active && !ctx.request_passthrough.finalized {
        return false;
    }
    session.is_body_done()
}

/// Skip Pingora's trailing empty upstream EOS after defer cache hit / suppress (see PATCH.md).
/// Also skip the defer-bootstrap empty pipe while passthrough is still buffering (otherwise
/// upstream sees headers + 0-byte body → MiMo `400 Invalid JSON`).
pub fn should_skip_upstream_trailing_empty_eos(ctx: &GatewayContext) -> bool {
    ctx.upstream.prepared_upstream_body_emitted
        || (ctx.request_passthrough.active && !ctx.request_passthrough.finalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crab_reasoning::StreamAccumulator;

    #[test]
    fn flush_streaming_reasoning_skips_when_finalized() {
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
            prefix_tokens_before: 0,
            prefix_tokens_after: 0,
            recovery_notice: None,
            record_response_scope: "scope".into(),
            record_response_messages: vec![],
            record_response_contexts: vec![("scope".into(), vec![])],
        });
        ctx.stream.accumulator = Some(StreamAccumulator::new());
        assert_eq!(flush_streaming_reasoning(&mut ctx, &store), 0);
    }

    #[test]
    fn passthrough_blocks_upstream_end_stream_until_finalized() {
        let mut ctx = GatewayContext::new("req".into());
        ctx.request_passthrough.active = true;
        assert!(ctx.request_passthrough.active && !ctx.request_passthrough.finalized);
        ctx.request_passthrough.finalized = true;
        ctx.request_passthrough.active = false;
        assert!(!ctx.request_passthrough.active || ctx.request_passthrough.finalized);
    }

    #[test]
    fn skip_trailing_empty_eos_while_passthrough_buffering() {
        let mut ctx = GatewayContext::new("req".into());
        ctx.request_passthrough.active = true;
        assert!(should_skip_upstream_trailing_empty_eos(&ctx));
        ctx.request_passthrough.finalized = true;
        assert!(!should_skip_upstream_trailing_empty_eos(&ctx));
    }
}
