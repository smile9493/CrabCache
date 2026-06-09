use crate::cache_helpers::{
    build_semantic_query_text, cache_entry_matches_stream_mode, tiered_exact_lookup,
};
use crate::cache_response::send_cached_response;
use crate::connection_helpers::apply_connection_options;
use crate::context::{GatewayContext, GatewayState, ReasoningConfig};
use crate::metrics_helpers::timeline_stamp;
use crate::runtime::RuntimeConfig;
use crate::upstream_pool::UpstreamKeyPool;
use crab_metrics::{CacheTier, global_metrics};
use crab_pipeline::RequestPipeline;
use crab_semantic::{GateDecision, evaluate_semantic_gate};
use pingora_core::prelude::*;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

pub struct GatewayProxy {
    pub(crate) state: Arc<GatewayState>,
}

impl GatewayProxy {
    pub(crate) fn is_mimo_pipeline(p: RequestPipeline) -> bool {
        matches!(
            p,
            RequestPipeline::MimoTokenPlanRelay | RequestPipeline::CodexMimo
        )
    }

    /// Redis session merge applies only to Chat Completions MiMo clients.
    ///
    /// Codex / Responses API clients carry continuity via `previous_response_id` +
    /// [`ResponsesChainStore`]. Running session store merge on converted `messages[]` fights
    /// that chain and causes prefix-break misalignment (not intentional turn truncation).
    pub(crate) fn mimo_session_store_applies(ctx: &GatewayContext) -> bool {
        ctx.request_pipeline.is_some_and(Self::is_mimo_pipeline)
            && ctx.client_wire_api == crate::context::ClientWireApi::ChatCompletions
    }

    pub(crate) fn is_codex_relay_pipeline(p: RequestPipeline) -> bool {
        matches!(p, RequestPipeline::CodexRelay)
    }

    pub(crate) fn is_codex_upstream_pipeline(p: RequestPipeline) -> bool {
        crate::codex_rate_limit::is_codex_upstream_pipeline(Some(p))
    }

    /// Pool-sized same-request retry budget (Codex OAuth/DeepSeek bridge + MiMo relays).
    pub(crate) fn uses_pool_scaled_retry_budget(p: RequestPipeline) -> bool {
        Self::is_codex_upstream_pipeline(p) || Self::is_mimo_pipeline(p)
    }

    pub(crate) fn is_deepseek_upstream_pipeline(p: RequestPipeline) -> bool {
        matches!(
            p,
            RequestPipeline::CodexDeepSeek
                | RequestPipeline::CursorDeepSeekV4
                | RequestPipeline::DeepSeekLight
        )
    }

    pub fn new(state: Arc<GatewayState>) -> Self {
        Self { state }
    }

    pub(crate) fn reasoning_config(&self) -> Arc<ReasoningConfig> {
        Arc::clone(&self.state.reasoning_config.read())
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

    /// Seed the per-request fallback chain from the active profile config.
    pub(crate) fn initialize_profile_fallback_state(&self, ctx: &mut GatewayContext) {
        ctx.profile_fallback_chain.clear();
        ctx.profile_fallback_attempt = 0;

        let Some(mut current_id) = ctx.upstream_profile_id.clone() else {
            return;
        };
        let mut remaining_hops = self
            .state
            .runtime
            .profile(&current_id)
            .map(|profile| profile.fallback_max_retries as usize)
            .unwrap_or(0);
        let mut visited = HashSet::new();
        while visited.insert(current_id.clone()) {
            ctx.profile_fallback_chain.push(current_id.clone());
            if remaining_hops == 0 {
                break;
            }
            let Some(next_id) = self
                .state
                .runtime
                .profile(&current_id)
                .and_then(|profile| profile.fallback_profile_id.clone())
            else {
                break;
            };
            let next_id = next_id.trim().to_string();
            if next_id.is_empty() {
                break;
            }
            remaining_hops = remaining_hops.saturating_sub(1);
            current_id = next_id;
        }
    }

    fn refresh_retry_budget_for_active_profile(&self, ctx: &mut GatewayContext) {
        if !ctx
            .request_pipeline
            .is_some_and(Self::uses_pool_scaled_retry_budget)
        {
            return;
        }
        let profile = self.active_upstream_profile(ctx);
        let pool = profile.resolve_upstream_pool();
        let max_budget = self.state.features.read().codex_retry_budget_max;
        ctx.upstream.retry_budget =
            crate::codex_rate_limit::pool_scaled_retry_budget(pool.len(), max_budget);
    }

    fn reset_upstream_state_for_retry(&self, ctx: &mut GatewayContext) {
        ctx.upstream.reset_for_retry();
        ctx.backend_permit = None;
        ctx.upstream_outbound_body_len = 0;
        ctx.upstream_headers_prepared_at = None;
        ctx.ttft = None;
        ctx.response_body_preview.clear();
        ctx.stream.pending_recovery_notice = None;
        ctx.new_request_body = ctx.upstream.prepared_body_for_retry.clone();
    }

    /// Switch the current request to the next configured fallback profile, if any.
    pub(crate) fn try_profile_fallback_retry(
        &self,
        ctx: &mut GatewayContext,
        status: u16,
        reason: &str,
    ) -> Option<Box<pingora_core::Error>> {
        if ctx.upstream.prepared_body_for_retry.is_none() || ctx.upstream.retry_buffer_truncated {
            return None;
        }
        let current_id = ctx.upstream_profile_id.clone()?;
        let current_profile = self.state.runtime.profile(&current_id)?;
        let next_index = ctx.profile_fallback_attempt as usize + 1;
        let next_id = ctx.profile_fallback_chain.get(next_index)?.clone();
        if next_id == current_id {
            return None;
        }
        let next_profile = self.state.runtime.profile(&next_id)?;
        if next_profile.provider != current_profile.provider {
            warn!(
                request_id = %ctx.request_id,
                current_profile = %current_id,
                fallback_profile = %next_id,
                current_provider = %current_profile.provider.as_str(),
                fallback_provider = %next_profile.provider.as_str(),
                "Skipping profile fallback because provider mismatch"
            );
            global_metrics()
                .profile_fallback_total
                .with_label_values(&[&current_id, &next_id, "provider_mismatch"])
                .inc();
            return None;
        }

        ctx.profile_fallback_attempt = ctx.profile_fallback_attempt.saturating_add(1);
        ctx.upstream_profile_id = Some(next_id.clone());
        self.reset_upstream_state_for_retry(ctx);
        self.refresh_retry_budget_for_active_profile(ctx);

        global_metrics()
            .profile_fallback_total
            .with_label_values(&[&current_id, &next_id, "triggered"])
            .inc();

        let mut e = Error::create(
            ErrorType::HTTPStatus(status),
            pingora_core::ErrorSource::Upstream,
            Some(
                format!(
                    "profile fallback from '{}' to '{}' after {}",
                    current_id, next_id, reason
                )
                .into(),
            ),
            None,
        );
        e.set_retry(true);
        Some(e)
    }

    /// Sync MiMo profile pool semaphores with `mimo_key_max_inflight` when configured.
    /// Skips the rebuild if the pool already has an explicit `max_inflight` from profile config
    /// (`max_inflight_per_key`), per the compatibility rules in `config.rs`.
    pub(crate) fn ensure_mimo_pool_inflight_cap(
        &self,
        profile: &crate::upstream_profile::UpstreamProfileRuntime,
        max_inflight: usize,
    ) {
        if max_inflight == 0 {
            return;
        }
        let current = profile.resolve_upstream_pool();
        if current.max_inflight() == max_inflight {
            return;
        }
        // Pool already has an explicit cap from profile.max_inflight_per_key — don't downgrade.
        if current.max_inflight() > 0 {
            return;
        }
        let updated = UpstreamKeyPool::rebuild_with_max_inflight(&current, max_inflight);
        *profile.upstream_pool.write() = updated;
        if profile.id == self.state.runtime.default_upstream_profile_id() {
            self.state.runtime.replace_upstream_pool(profile.resolve_upstream_pool());
        }
    }

    pub(crate) fn create_upstream_peer(
        &self,
        addr: std::net::SocketAddr,
        tls_sni: &str,
        ctx: &mut GatewayContext,
    ) -> HttpPeer {
        ctx.upstream.host = Some(tls_sni.to_string());
        let mut peer = HttpPeer::new(addr, true, tls_sni.to_string());
        // Per-profile connection overrides take precedence over the global config.
        // NOTE: pooled connections use TLS params from when they were established;
        //       new params take effect only for new connections after idle expiry.
        let profile = self.active_upstream_profile(ctx);
        if let Some(ref profile_conn) = profile.connection {
            apply_connection_options(profile_conn, &mut peer.options);
        } else {
            let conn_config = self.state.runtime.conn_config.read().clone();
            apply_connection_options(&conn_config, &mut peer.options);
        }
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
        ctx.stream.stream_completion =
            Some(crate::context::StreamCompletion::CacheHit);
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
        let canonical_model = ctx
            .upstream_model
            .as_deref()
            .map(crab_pipeline::canonicalize_client_model)
            .unwrap_or_else(|| crab_pipeline::canonicalize_client_model(&ctx.model));
        let upstream_model = if profile.provider == crab_pipeline::UpstreamProvider::Codex {
            crate::codex::resolve_codex_upstream_model(&canonical_model).to_string()
        } else {
            canonical_model
        };

        // MiMo conversation-level key binding: try bound key first.
        let features = self.state.features.read();
        if ctx.request_pipeline.map_or(false, Self::is_mimo_pipeline) && features.mimo_key_binding {
            if features.mimo_key_max_inflight > 0 {
                self.ensure_mimo_pool_inflight_cap(&profile, features.mimo_key_max_inflight);
            }
            let pool = profile.resolve_upstream_pool();
            if let Some(ref binding_store) = self.state.key_binding_store {
                if let Some(bind_key) = crate::key_binding::resolve_mimo_binding_key(ctx) {
                    if let Some(binding) = binding_store.get(&bind_key) {
                        let bound_id = binding.key_id;
                        let max_inflight = features.mimo_key_max_inflight;
                        let current_inflight = pool.inflight_of(&bound_id);
                        if max_inflight == 0 || current_inflight < max_inflight {
                            if let Some(guard) = pool.acquire_specific(&bound_id) {
                                binding_store.touch(&bind_key);
                                ctx.upstream.miss = true;
                                ctx.upstream.key_guard = Some(guard);
                                global_metrics().record_key_binding_event("hit");
                                tracing::debug!(
                                    request_id = %ctx.request_id,
                                    bind_key = %bind_key,
                                    key_id = bound_id.as_str(),
                                    binding_key_kind = crate::key_binding::mimo_binding_key_kind(&bind_key),
                                    "MiMo key binding hit"
                                );
                                return true;
                            }
                        }

                        let spill_reason = if max_inflight > 0
                            && current_inflight >= max_inflight
                        {
                            "inflight"
                        } else {
                            "cooldown"
                        };
                        if let Some(guard) = pool
                            .acquire_excluding_key(&bound_id)
                            .or_else(|| pool.acquire_for_upstream_model(&upstream_model, false))
                            .or_else(|| pool.acquire())
                        {
                            ctx.upstream.miss = true;
                            ctx.upstream.key_guard = Some(guard);
                            global_metrics().record_key_binding_event("spill");
                            tracing::info!(
                                request_id = %ctx.request_id,
                                bind_key = %bind_key,
                                bound_key_id = %bound_id,
                                spill_reason,
                                binding_key_kind = crate::key_binding::mimo_binding_key_kind(&bind_key),
                                "MiMo key binding spill to alternate upstream key"
                            );
                            return true;
                        }
                        global_metrics().record_rejected("upstream_key_exhausted");
                        global_metrics().record_rejection_by_source("upstream");
                        return false;
                    }

                    let max_sessions = features.mimo_key_max_sessions_per_key;
                    let available_ids = pool.available_key_ids();
                    let available_refs: Vec<&str> =
                        available_ids.iter().map(|s| s.as_str()).collect();
                    let preferred =
                        binding_store.least_loaded_key(&available_refs, max_sessions);
                    let guard = preferred
                        .as_deref()
                        .and_then(|kid| pool.acquire_specific(kid))
                        .or_else(|| pool.acquire_for_upstream_model(&upstream_model, false))
                        .or_else(|| pool.acquire());
                    if let Some(guard) = guard {
                        let kid = guard.key_id().to_string();
                        binding_store.put(bind_key.clone(), kid);
                        ctx.upstream.miss = true;
                        ctx.upstream.key_guard = Some(guard);
                        global_metrics().record_key_binding_event("miss");
                        tracing::debug!(
                            request_id = %ctx.request_id,
                            bind_key = %bind_key,
                            binding_key_kind = crate::key_binding::mimo_binding_key_kind(&bind_key),
                            key_id = ctx.upstream.key_guard.as_ref().map(|g| g.key_id()),
                            "MiMo key binding created"
                        );
                        return true;
                    }
                    tracing::warn!(
                        request_id = %ctx.request_id,
                        bind_key = %bind_key,
                        "MiMo key binding: no upstream keys available for new binding"
                    );
                    global_metrics().record_rejected("upstream_key_exhausted");
                    global_metrics().record_rejection_by_source("upstream");
                    return false;
                }
                // No resolvable binding key: fall through to default pool acquire.
            }
        }

        // Codex conversation-level key binding (session → account affinity + overflow spill).
        if ctx
            .request_pipeline
            .map_or(false, Self::is_codex_upstream_pipeline)
            && features.codex_key_binding
        {
            if let Some(ref binding_store) = self.state.key_binding_store {
                let stable_session = ctx
                    .conversation_id
                    .as_deref()
                    .or(ctx.prompt_cache_key.as_deref())
                    .or(ctx.session_fingerprint.as_deref())
                    .or(ctx.client_key_fingerprint.as_deref());

                if let Some(sid) = stable_session {
                    let bind_key = crate::key_binding::KeyBindingStore::codex_session_key(sid);
                    let fill_first = features.codex_acquire_fill_first;
                    let scope = crate::codex_rate_limit::codex_model_scope(&upstream_model);
                    if let Some(binding) = binding_store.get(&bind_key) {
                        let max_inflight = features.codex_key_max_inflight;
                        if max_inflight == 0 || pool.inflight_of(&binding.key_id) < max_inflight {
                            if let Some(guard) =
                                pool.acquire_specific_scoped(&binding.key_id, Some(scope))
                            {
                                binding_store.touch(&bind_key);
                                ctx.upstream.miss = true;
                                ctx.upstream.key_guard = Some(guard);
                                global_metrics().record_key_binding_event("codex_hit");
                                return true;
                            }
                        }
                        global_metrics().record_rejected("upstream_key_exhausted");
                        global_metrics().record_rejection_by_source("upstream");
                        return false;
                    }
                    if let Some(guard) = pool
                        .acquire_codex_for_model(&upstream_model, None, fill_first)
                        .or_else(|| pool.acquire_codex_oauth())
                    {
                        let kid = guard.key_id().to_string();
                        binding_store.put(bind_key, kid);
                        ctx.upstream.miss = true;
                        ctx.upstream.key_guard = Some(guard);
                        global_metrics().record_key_binding_event("codex_miss");
                        return true;
                    }
                    global_metrics().record_rejected("upstream_key_exhausted");
                    global_metrics().record_rejection_by_source("upstream");
                    return false;
                }
            }
        }

        // Default acquire logic (Codex, non-MiMo, or pipelines without key binding).
        let guard = if profile.provider == crab_pipeline::UpstreamProvider::Codex {
            pool.acquire_codex_for_model(&upstream_model, None, features.codex_acquire_fill_first)
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
                true
            }
            None => {
                global_metrics().record_rejected("upstream_key_exhausted");
                global_metrics().record_rejection_by_source("upstream");
                false
            }
        }
    }

    /// Codex quota-aware upstream key selection with preflight WHAM check.
    ///
    /// Mirrors OmniRoute `getProviderCredentialsWithQuotaPreflight`:
    /// 1. Try key binding first (hit/spill/miss) but skip exhausted keys
    /// 2. Acquire a key → fetch WHAM → if remaining <= threshold, exclude & retry
    /// 3. All blocked → return false (caller sends 503)
    pub(crate) async fn try_acquire_codex_with_preflight(&self, ctx: &mut GatewayContext) -> bool {
        if ctx.upstream.key_guard.is_some() {
            return true;
        }

        let profile = self.active_upstream_profile(ctx);
        let pool = profile.resolve_upstream_pool();
        let base_url = profile.base_url.clone();

        let canonical_model = ctx
            .upstream_model
            .as_deref()
            .map(crab_pipeline::canonicalize_client_model)
            .unwrap_or_else(|| crab_pipeline::canonicalize_client_model(&ctx.model));
        let upstream_model = crate::codex::resolve_codex_upstream_model(&canonical_model);

        let min_remaining;
        {
            let f = self.state.features.read();
            min_remaining = f.codex_quota_min_remaining_percent;
        }

        let quota_cache = match pool.quota_cache() {
            Some(cache) => Arc::clone(cache),
            None => {
                // No quota cache configured — fall through to sync path
                return self.try_acquire_upstream_key(ctx);
            }
        };

        // Step 1: Try key binding (but skip exhausted keys)
        let codex_key_binding;
        {
            let f = self.state.features.read();
            codex_key_binding = f.codex_key_binding;
        }
        if ctx
            .request_pipeline
            .map_or(false, Self::is_codex_upstream_pipeline)
            && codex_key_binding
        {
            if let Some(ref binding_store) = self.state.key_binding_store {
                let stable_session = ctx
                    .conversation_id
                    .as_deref()
                    .or(ctx.prompt_cache_key.as_deref())
                    .or(ctx.session_fingerprint.as_deref())
                    .or(ctx.client_key_fingerprint.as_deref());

                if let Some(sid) = stable_session {
                    let bind_key = crate::key_binding::KeyBindingStore::codex_session_key(sid);
                    if let Some(binding) = binding_store.get(&bind_key) {
                        // Skip if bound key is exhausted
                        if !quota_cache.is_exhausted(&binding.key_id, min_remaining) {
                            let scope = crate::codex_rate_limit::codex_model_scope(upstream_model);
                            if let Some(guard) =
                                pool.acquire_specific_scoped(&binding.key_id, Some(scope))
                            {
                                binding_store.touch(&bind_key);
                                ctx.upstream.miss = true;
                                ctx.upstream.key_guard = Some(guard);
                                global_metrics().record_key_binding_event("codex_hit");
                                return true;
                            }
                        }
                        // Bound key exhausted or unavailable — will fall through to preflight loop
                    }
                }
            }
        }

        // Step 2: Preflight loop — acquire → WHAM check → exclude if blocked
        let mut exclude_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut exclude_account: Option<String> = None;
        let fill_first;
        {
            let f = self.state.features.read();
            fill_first = f.codex_acquire_fill_first;
        }

        loop {
            let guard = if let Some(ref excl_acct) = exclude_account {
                pool.acquire_codex_for_model_excluding(
                    upstream_model,
                    Some(excl_acct),
                    fill_first,
                    &exclude_keys,
                )
            } else {
                pool.acquire_codex_for_model(upstream_model, None, fill_first)
            };

            let guard = match guard {
                Some(g) => g,
                None => {
                    // No more keys available
                    global_metrics().record_rejected("upstream_key_exhausted");
                    global_metrics().record_rejection_by_source("upstream");
                    return false;
                }
            };

            let key_id = guard.key_id().to_string();
            let account_id = guard.account_id().to_string();

            // Check cache first (avoid WHAM fetch if already cached)
            let cached_headroom = quota_cache.headroom_percent(&key_id);
            if let Some(headroom) = cached_headroom {
                if headroom <= min_remaining {
                    tracing::info!(
                        request_id = %ctx.request_id,
                        key_id = %key_id,
                        headroom = headroom,
                        min_remaining = min_remaining,
                        "codex quota preflight: key blocked by cache (low headroom)"
                    );
                    global_metrics().record_codex_quota_preflight("blocked");
                    exclude_keys.insert(key_id);
                    if !account_id.is_empty()
                        && account_id != crate::upstream_pool::DEFAULT_UPSTREAM_ACCOUNT_ID
                    {
                        exclude_account = Some(account_id);
                    }
                    continue;
                }
                // Cached data shows sufficient headroom — use this key
                tracing::debug!(
                    request_id = %ctx.request_id,
                    key_id = %key_id,
                    headroom = headroom,
                    "codex quota preflight: key accepted (cached headroom)"
                );
                ctx.upstream.miss = true;
                ctx.upstream.key_guard = Some(guard);
                global_metrics().record_codex_quota_preflight("proceed");
                return true;
            }

            // No cached data — fetch WHAM
            let secret = guard.bearer_secret().to_string();
            let acct_id = account_id.clone();

            // Release the guard before async WHAM fetch (we'll re-acquire if it passes)
            drop(guard);

            let snapshot = quota_cache
                .fetch_and_update(&key_id, &base_url, &secret, &acct_id)
                .await;

            let snapshot = match snapshot {
                Some(s) => s,
                None => {
                    // WHAM fetch failed — fail-open (proceed with this key)
                    tracing::warn!(
                        request_id = %ctx.request_id,
                        key_id = %key_id,
                        "codex quota preflight: WHAM fetch failed, proceeding (fail-open)"
                    );
                    global_metrics().record_codex_quota_preflight("fail_open");
                    // Re-acquire the key
                    if let Some(guard) = pool.acquire_specific(&key_id) {
                        ctx.upstream.miss = true;
                        ctx.upstream.key_guard = Some(guard);
                        return true;
                    }
                    // Re-acquire failed (race condition) — skip this key
                    exclude_keys.insert(key_id);
                    continue;
                }
            };

            let remaining = snapshot.min_remaining_percent().unwrap_or(100.0);
            if snapshot.is_exhausted(min_remaining) || remaining <= min_remaining {
                tracing::info!(
                    request_id = %ctx.request_id,
                    key_id = %key_id,
                    remaining = remaining,
                    limit_reached = snapshot.limit_reached,
                    "codex quota preflight: key blocked (low remaining from WHAM)"
                );
                global_metrics().record_codex_quota_preflight("blocked");
                exclude_keys.insert(key_id);
                if !acct_id.is_empty()
                    && acct_id != crate::upstream_pool::DEFAULT_UPSTREAM_ACCOUNT_ID
                {
                    exclude_account = Some(acct_id);
                }
                continue;
            }

            // WHAM check passed — re-acquire and use this key
            tracing::debug!(
                request_id = %ctx.request_id,
                key_id = %key_id,
                remaining = remaining,
                "codex quota preflight: key accepted (WHAM check passed)"
            );
            if let Some(guard) = pool.acquire_specific(&key_id) {
                ctx.upstream.miss = true;
                ctx.upstream.key_guard = Some(guard);
                global_metrics().record_codex_quota_preflight("proceed");
                return true;
            }

            // Re-acquire failed — skip and retry
            exclude_keys.insert(key_id);
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
            ctx.stream.stream_completion =
                Some(crate::context::StreamCompletion::CacheHit);
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

    pub(crate) fn rotate_upstream_key_for_same_request_retry(
        &self,
        ctx: &mut GatewayContext,
        reason: &'static str,
        disable_current: bool,
        cooldown_current_secs: Option<u64>,
    ) -> Option<String> {
        let prepared_body = ctx.upstream.prepared_body_for_retry.clone()?;
        if ctx.upstream.retry_budget == 0 {
            global_metrics().record_upstream_key_retry("budget_exhausted");
            return None;
        }

        let profile = self.active_upstream_profile(ctx);
        let pool = profile.resolve_upstream_pool();
        let old_key = ctx
            .upstream
            .key_guard
            .as_ref()
            .map(|g| (g.key_id().to_string(), g.account_id().to_string()));
        let bind_key = crate::key_binding::resolve_mimo_binding_key(ctx);
        let transient_failures = old_key
            .as_ref()
            .map(|(old_id, _)| {
                self.record_mimo_transient_failure(ctx, old_id, "same_request_retry")
            })
            .unwrap_or(0);

        if let Some((ref old_id, _)) = old_key {
            if disable_current {
                pool.report_unauthorized(old_id);
                global_metrics().record_upstream_key_request(old_id, "error");
            } else if transient_failures >= 3
                && let Some(cooldown) = cooldown_current_secs
            {
                let codex =
                    crate::codex_rate_limit::is_codex_upstream_pipeline(ctx.request_pipeline);
                let mimo = ctx
                    .request_pipeline
                    .is_some_and(Self::is_mimo_pipeline);
                let retry_scope = crate::codex_rate_limit::upstream_rate_limit_scope(
                    codex,
                    mimo,
                    ctx.upstream_model.as_deref().unwrap_or(&ctx.model),
                );
                pool.report_rate_limited_for(old_id, cooldown.max(1), retry_scope);
            }
        }

        // Drop the old guard before re-acquiring so per-key inflight is released.
        ctx.upstream.key_guard = None;
        let new_guard = if !disable_current && transient_failures < 3 {
            old_key
                .as_ref()
                .and_then(|(old_id, _)| pool.acquire_specific(old_id))
        } else {
            let excluded_account = old_key.as_ref().map(|(_, account)| account.as_str());
            pool.acquire_excluding_account(excluded_account)
                .or_else(|| {
                    if disable_current {
                        None
                    } else {
                        pool.acquire()
                    }
                })
        }?;
        let new_key_id = new_guard.key_id().to_string();

        ctx.upstream.retry_budget -= 1;
        ctx.upstream.key_guard = Some(new_guard);
        ctx.new_request_body = Some(prepared_body);
        ctx.upstream.prepared_upstream_body_emitted = false;
        ctx.upstream.error_passthrough = false;
        ctx.upstream.error_body_logged = false;
        ctx.upstream.first_body_chunk_logged = false;
        ctx.upstream.http_status = None;
        ctx.upstream.headers_at = None;
        ctx.upstream.response_decompress.reset();
        ctx.upstream_outbound_body_len = 0;
        ctx.upstream.backend_name = None;
        ctx.upstream.backend_overload_state = None;
        ctx.upstream.host = None;
        ctx.backend_permit = None;

        if let Some(ref binding_store) = self.state.key_binding_store {
            if let Some(ref key) = bind_key {
                if disable_current || transient_failures >= 3 {
                    binding_store.remove(key);
                }
            }
            if let Some(sid) = ctx
                .conversation_id
                .as_deref()
                .or(ctx.prompt_cache_key.as_deref())
                .or(ctx.session_fingerprint.as_deref())
            {
                if disable_current || transient_failures >= 3 {
                    binding_store
                        .remove(&crate::key_binding::KeyBindingStore::codex_session_key(sid));
                }
            }
        }

        global_metrics().record_upstream_key_retry(reason);
        Some(new_key_id)
    }

    fn is_mimo_upload_pipe_closed(e: &pingora_core::Error) -> bool {
        let text = e.to_string();
        text.contains("Failed to send upstream task Body to pipe cause: channel closed")
            || text
                .contains("Failed to send upstream task Body (end) to pipe cause: channel closed")
    }

    fn record_mimo_transient_failure(
        &self,
        ctx: &mut GatewayContext,
        key_id: &str,
        reason: &'static str,
    ) -> u32 {
        if ctx
            .request_pipeline
            .is_none_or(|pipeline| !Self::is_mimo_pipeline(pipeline))
        {
            return 0;
        }

        let mut transient_failures = 0u32;
        if let (Some(binding_store), Some(bind_key)) = (
            self.state.key_binding_store.as_ref(),
            crate::key_binding::resolve_mimo_binding_key(ctx),
        ) {
            transient_failures = binding_store
                .record_failure(&bind_key, key_id)
                .unwrap_or(0);
        }

        if transient_failures >= 3 {
            let pool = self.active_upstream_profile(ctx).resolve_upstream_pool();
            pool.report_rate_limited_for(key_id, 30, None);

            if let (Some(binding_store), Some(bind_key)) = (
                self.state.key_binding_store.as_ref(),
                crate::key_binding::resolve_mimo_binding_key(ctx),
            ) {
                binding_store.remove(&bind_key);
                global_metrics().record_key_binding_event("unbind_429");
            }

            if let (Some(model), Some(backend_name)) = (
                ctx.upstream_model.as_deref(),
                ctx.upstream.backend_name.as_deref(),
            ) {
                let profile_id = ctx.upstream_profile_id.as_deref().unwrap_or("default");
                self.state.model_lockouts.record_failure(
                    profile_id,
                    backend_name,
                    model,
                    reason,
                    Duration::from_secs(30),
                    1,
                );
                global_metrics().record_model_lockout(profile_id, backend_name, model);
            }
        }

        transient_failures
    }

    fn should_retry_mimo_upload_error(
        &self,
        session: &Session,
        e: &pingora_core::Error,
        ctx: &GatewayContext,
    ) -> bool {
        if session.response_written().is_some() {
            return false;
        }
        if ctx
            .request_pipeline
            .is_none_or(|pipeline| !Self::is_mimo_pipeline(pipeline))
        {
            return false;
        }
        if ctx.upstream.prepared_body_for_retry.is_none() || ctx.upstream.retry_budget == 0 {
            return false;
        }
        Self::is_mimo_upload_pipe_closed(e)
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

    /// Synthesize `response.completed` when MiMo/upstream aborts mid-stream (before Pingora EOS).
    async fn finalize_aborted_upstream_stream(
        &self,
        session: &Session,
        ctx: &mut Self::CTX,
    ) -> Result<Option<bytes::Bytes>> {
        if session.response_written().is_none() {
            return Ok(None);
        }
        crab_metrics::global_metrics().record_upstream_abort_after_headers();
        let tail = crate::responses_wire::build_graceful_responses_stream_tail(
            ctx,
            self.state.responses_chain_store.as_ref(),
        );
        if tail.is_some() {
            crab_metrics::global_metrics().record_synthetic_response_completed("upstream_abort");
            ctx.stream.stream_completion =
                Some(crate::context::StreamCompletion::SyntheticAbortTail);
        }
        Ok(tail.map(bytes::Bytes::from))
    }

    fn take_force_downstream_body_end_of_stream(&self, ctx: &mut Self::CTX) -> bool {
        if !ctx.stream.responses_wire_force_downstream_eos {
            return false;
        }
        ctx.stream.responses_wire_force_downstream_eos = false;
        crab_metrics::global_metrics().record_force_downstream_eos();
        true
    }

    /// Prefill keepalive: send Responses bootstrap right after upstream 200 headers.
    async fn initial_downstream_response_body(
        &self,
        _session: &Session,
        ctx: &mut Self::CTX,
    ) -> Result<Option<bytes::Bytes>> {
        Ok(crate::responses_wire::take_early_responses_wire_bootstrap(ctx).map(bytes::Bytes::from))
    }

    /// Idle upstream: push `response.in_progress` heartbeats so Codex does not drop the SSE socket.
    async fn poll_downstream_stream_keepalive(
        &self,
        _session: &Session,
        ctx: &mut Self::CTX,
    ) -> Result<Option<bytes::Bytes>> {
        Ok(crate::responses_wire::poll_responses_wire_keepalive(ctx).map(bytes::Bytes::from))
    }

    fn error_while_proxy(
        &self,
        peer: &HttpPeer,
        session: &mut Session,
        e: Box<pingora_core::Error>,
        ctx: &mut Self::CTX,
        client_reused: bool,
    ) -> Box<pingora_core::Error> {
        if e.esource == pingora_core::ErrorSource::Upstream
            && session.response_written().is_none()
            && let Some(retry_err) =
                self.try_profile_fallback_retry(ctx, 502, "upstream connection failure")
        {
            warn!(
                request_id = %ctx.request_id,
                upstream_profile = ctx.upstream_profile_id.as_deref().unwrap_or(""),
                "Upstream connection failed; retrying via fallback profile"
            );
            return retry_err;
        }
        let should_retry = self.should_retry_mimo_upload_error(session, e.as_ref(), ctx);
        let is_mimo_upload_pipe_closed = Self::is_mimo_upload_pipe_closed(e.as_ref())
            && ctx.request_pipeline.is_some_and(Self::is_mimo_pipeline);
        let mut e = e.more_context(format!("Peer: {}", peer));
        if should_retry {
            if let Some(new_key_id) = self.rotate_upstream_key_for_same_request_retry(
                ctx,
                "mimo_upload_pipe_closed_rotate",
                false,
                Some(30),
            ) {
                e.set_retry(true);
                warn!(
                    request_id = %ctx.request_id,
                    new_key_id = %new_key_id,
                    "retrying MiMo upload after upstream body pipe closed"
                );
                return e;
            }
        }
        if is_mimo_upload_pipe_closed
            && let Some(key_id) = ctx
                .upstream
                .key_guard
                .as_ref()
                .map(|guard| guard.key_id().to_string())
        {
            let transient_failures =
                self.record_mimo_transient_failure(ctx, &key_id, "mimo_upload_pipe_closed");
            warn!(
                request_id = %ctx.request_id,
                key_id = %key_id,
                transient_failures,
                response_written = session.response_written().is_some(),
                retry_budget = ctx.upstream.retry_budget,
                "recorded MiMo upload pipe failure without retry"
            );
        }
        e.retry
            .decide_reuse(client_reused && !session.as_ref().retry_buffer_truncated());
        e
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
        let safe_end = raw.floor_char_boundary(max_bytes);
        format!(
            "{}...<truncated {}>",
            &raw[..safe_end],
            raw.len() - safe_end
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
