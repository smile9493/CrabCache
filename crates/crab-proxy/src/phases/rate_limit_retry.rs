//! Shared upstream rate-limit detection + same-request key rotation (response headers).

use crate::codex_rate_limit::{self, RateLimitClassification};
use crate::context::GatewayContext;
use crate::proxy::GatewayProxy;
use crate::upstream_pool::UpstreamKeyPool;
use crab_metrics::global_metrics;
use http::header;
use pingora_core::ErrorType;
use pingora_http::ResponseHeader;
use std::sync::Arc;

fn model_scope(ctx: &GatewayContext) -> &'static str {
    let model = ctx.upstream_model.as_deref().unwrap_or(ctx.model.as_str());
    codex_rate_limit::codex_model_scope(model)
}

fn cooldown_secs(classification: &RateLimitClassification, pool: &UpstreamKeyPool) -> u64 {
    classification
        .cooldown
        .map(|d| d.as_secs().max(1))
        .unwrap_or_else(|| pool.default_cooldown_secs().max(1))
}

/// Classify upstream status (header stage; body usually unavailable) and rotate keys when possible.
pub(crate) fn try_upstream_rate_limit_rotation(
    proxy: &GatewayProxy,
    ctx: &mut GatewayContext,
    upstream_response: &mut ResponseHeader,
    status: u16,
    body: Option<&str>,
    pool: &Arc<UpstreamKeyPool>,
) -> pingora_core::Result<()> {
    let codex = codex_rate_limit::is_codex_upstream_pipeline(ctx.request_pipeline);
    let mimo = ctx
        .request_pipeline
        .is_some_and(GatewayProxy::is_mimo_pipeline);
    let retry_hdr = upstream_response
        .headers
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok());
    let classification = codex_rate_limit::classify_upstream_rate_limit(
        status,
        body,
        retry_hdr,
        codex,
        mimo,
        pool.default_cooldown_secs(),
    );
    if !classification.is_rate_limit {
        return Ok(());
    }

    let key_id = ctx
        .upstream
        .key_guard
        .as_ref()
        .map(|g| g.key_id().to_string());
    let Some(id) = key_id else {
        return Ok(());
    };

    let scope = if codex || mimo {
        Some(model_scope(ctx))
    } else {
        None
    };
    let fill_first = proxy.state.features.read().codex_acquire_fill_first;
    let cooldown = cooldown_secs(&classification, pool);
    pool.report_rate_limited_for(&id, cooldown, scope);
    global_metrics().record_upstream_key_request(&id, "rate_limited");

    // Codex quota feedback: mark key exhausted in quota cache on 429 usage limit
    if codex && status == 429 && crate::codex_rate_limit::body_indicates_codex_rate_limit(body.unwrap_or("")) {
        if let Some(cache) = pool.quota_cache() {
            cache.mark_exhausted_from_429(&id, "codex");
        }
        // Clear session affinity so next request picks a different key (OmniRoute: deleteSessionAccountAffinity)
        if let Some(ref binding_store) = proxy.state.key_binding_store {
            let features = proxy.state.features.read();
            if features.codex_key_binding {
                if let Some(sid) = ctx
                    .conversation_id
                    .as_deref()
                    .or(ctx.prompt_cache_key.as_deref())
                    .or(ctx.session_fingerprint.as_deref())
                    .or(ctx.client_key_fingerprint.as_deref())
                {
                    let bind_key = crate::key_binding::KeyBindingStore::codex_session_key(sid);
                    binding_store.remove(&bind_key);
                    tracing::info!(
                        request_id = %ctx.request_id,
                        key_id = %id,
                        session_id = %sid,
                        "codex quota: cleared session binding after 429 usage limit"
                    );
                }
            }
        }
    }

    tracing::info!(
        request_id = %ctx.request_id,
        key_preview = %id,
        upstream_status = status,
        effective_status = classification.effective_status,
        retry_budget = ctx.upstream.retry_budget,
        cooldown_secs = cooldown,
        scope = ?scope,
        "upstream rate limited, attempting key rotation"
    );

    if ctx.upstream.retry_budget > 0 {
        ctx.upstream.retry_budget -= 1;
        let excluded = pool
            .list_status()
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.account_id);
        let new_guard = if codex {
            pool.acquire_codex_for_model(
                ctx.upstream_model.as_deref().unwrap_or(&ctx.model),
                excluded.as_deref(),
                fill_first,
            )
        } else {
            pool.acquire_excluding_account(excluded.as_deref())
        };

        if let Some(new_guard) = new_guard {
            let new_key_id = new_guard.key_id().to_string();
            ctx.upstream.key_guard = Some(new_guard);
            global_metrics().record_upstream_key_retry("rate_limited_rotate");

            ctx.new_request_body = ctx.upstream.prepared_body_for_retry.clone();
            ctx.upstream.error_passthrough = false;
            ctx.upstream.error_body_logged = false;

            tracing::info!(
                request_id = %ctx.request_id,
                new_key_id = %new_key_id,
                upstream_status = status,
                "retrying rate limit with new upstream key"
            );

            let mut e = pingora_core::Error::create(
                ErrorType::HTTPStatus(classification.effective_status),
                pingora_core::ErrorSource::Upstream,
                Some(
                    format!("upstream rate limited (status {status}), retrying with new key")
                        .into(),
                ),
                None,
            );
            e.set_retry(true);
            return Err(e);
        }
        global_metrics().record_upstream_key_retry("cooldown_only");
    }

    if classification.effective_status == 429 {
        if let Some(ref model) = ctx.upstream_model {
            let decision = crate::fallback_policy::check_fallback_error(429, body, retry_hdr);
            let profile_id = ctx.upstream_profile_id.as_deref().unwrap_or("default");
            if let Some(ref backend_name) = ctx.upstream.backend_name {
                proxy.state.model_lockouts.record_failure(
                    profile_id,
                    backend_name,
                    model,
                    &decision.reason,
                    decision.cooldown,
                    4,
                );
                global_metrics().record_model_lockout(profile_id, backend_name, model);
            }
        }
        if let Some(guard) = &ctx.coalesce_guard
            && guard.is_leader()
        {
            guard.mark_failed();
        }
    }

    Ok(())
}
