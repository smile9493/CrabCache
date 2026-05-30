use crab_capture::affinity_kind_from_key;
use crab_route::extract_affinity_key;
use http::HeaderMap;
use pingora_proxy::Session;
use sha2::{Digest, Sha256};

use crate::context::GatewayContext;

/// SHA-256 hex prefix (16 chars) of the client API key token (Bearer value).
pub fn fingerprint_client_key(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let hash = hex::encode(hasher.finalize());
    hash[..hash.len().min(16)].to_string()
}

/// Labels which stable ReasoningStore scope source is active (for ops / Cursor sub-agent debugging).
pub fn last_user_message_fingerprint(payload: &serde_json::Value) -> Option<String> {
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

pub fn client_session_from_authorization(authorization: Option<&str>) -> Option<String> {
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

pub fn stable_session_log_fields(
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

/// `prefill_ms`: request start → upstream response headers (MiMo prefill SLO).
/// `sse_ttft_ms`: response headers → first upstream body chunk.
pub fn request_timing_ms(ctx: &GatewayContext) -> (Option<f64>, Option<f64>) {
    let prefill_ms = ctx
        .upstream
        .headers_at
        .map(|h| h.duration_since(ctx.request_start).as_secs_f64() * 1000.0);
    let sse_ttft_ms = ctx.ttft.map(|d| d.as_secs_f64() * 1000.0);
    (prefill_ms, sse_ttft_ms)
}

/// Metadata for raw capture: session grouping, load balancing, and latency.
pub fn build_capture_request_meta(
    session: &Session,
    ctx: &GatewayContext,
    duration_ms: u64,
) -> crab_capture::CaptureRequestMeta {
    let req_header = session.req_header();
    let headers = HeaderMap::from_iter(
        req_header
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    let client_ip = session
        .client_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();

    let affinity_key = ctx.upstream.affinity_key.clone().unwrap_or_else(|| {
        extract_affinity_key(
            &headers,
            &client_ip,
            ctx.conversation_id.as_deref(),
            ctx.prompt_cache_key.as_deref(),
            ctx.project_id.as_deref(),
            ctx.session_fingerprint.as_deref(),
        )
    });
    let affinity_kind = Some(affinity_kind_from_key(&affinity_key).to_string());
    let (prefill_ms, sse_ttft_ms) = request_timing_ms(ctx);

    crab_capture::CaptureRequestMeta {
        conversation_id: ctx.conversation_id.clone(),
        prompt_cache_key: ctx.prompt_cache_key.clone(),
        session_fingerprint: None,
        body_user: None,
        affinity_kind,
        affinity_key: Some(affinity_key),
        backend_name: ctx.upstream.backend_name.clone(),
        upstream_host: ctx.upstream.host.clone(),
        client_key_fingerprint: ctx.client_key_fingerprint.clone(),
        upstream_key_id: ctx
            .upstream
            .key_guard
            .as_ref()
            .map(|g| g.key_id().to_string()),
        upstream_profile_id: ctx.upstream_profile_id.clone(),
        domain: ctx.domain.clone(),
        cache_tier: ctx.cache_tier.map(|t| t.as_str().to_string()),
        cache_hit: ctx.cache_tier.is_some(),
        coalesced_follower: ctx.is_coalesced_follower,
        coalesce_leader: ctx.coalesce_guard.as_ref().map(|g| g.is_leader()),
        duration_ms,
        prefill_ms: prefill_ms.map(|v| v.round() as u64),
        ttft_ms: sse_ttft_ms.map(|v| v.round() as u64),
        upstream_latency_ms: ctx.upstream.latency_ms,
    }
}

#[inline]
pub fn is_models_endpoint(path: &str, method: &http::Method) -> bool {
    *method == http::Method::GET && (path == "/models" || path == "/v1/models")
}

pub fn sanitize_for_trace(value: Option<&str>) -> Option<String> {
    value.map(|s| {
        if s.len() > 64 {
            format!("{}...<truncated>", &s[..32])
        } else {
            s.to_string()
        }
    })
}

/// Feed passthrough relay bytes into the incremental body hasher.
pub fn passthrough_hash_update(ctx: &mut GatewayContext, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    use sha2::Digest;
    let hasher = ctx
        .request_passthrough
        .body_hasher
        .get_or_insert_with(Sha256::new);
    hasher.update(data);
}

/// Finalize passthrough body hash into `ctx.req_hash`.
pub fn passthrough_hash_finalize(ctx: &mut GatewayContext) {
    use sha2::Digest;
    if let Some(hasher) = ctx.request_passthrough.body_hasher.take() {
        ctx.req_hash = Some(hex::encode(hasher.finalize()));
    }
}

/// Trace-safe prefix (16 hex chars) of a full request hash.
pub fn trace_request_hash_prefix(full_hash: &str) -> String {
    if full_hash.len() >= 16 {
        full_hash[..16].to_string()
    } else {
        full_hash.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::GatewayContext;

    #[test]
    fn passthrough_incremental_hash_matches_full_body() {
        let prefix = b"{\"model\":\"mimo\"";
        let suffix = b",\"messages\":[]}";
        let full: Vec<u8> = [prefix.as_slice(), suffix.as_slice()].concat();
        let mut expected = Sha256::new();
        expected.update(&full);
        let expected_hex = hex::encode(expected.finalize());

        let mut ctx = GatewayContext::new("test".to_string());
        ctx.request_passthrough.body_hasher = Some(Sha256::new());
        passthrough_hash_update(&mut ctx, prefix);
        passthrough_hash_update(&mut ctx, suffix);
        passthrough_hash_finalize(&mut ctx);

        assert_eq!(ctx.req_hash.as_deref(), Some(expected_hex.as_str()));
        assert_ne!(
            ctx.req_hash.as_deref(),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        assert_eq!(trace_request_hash_prefix(&expected_hex), expected_hex[..16]);
    }
}
