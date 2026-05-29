//! MiMo streaming body forward: overlap client body read with upstream TCP/TLS connect.
//!
//! Pingora phase order (see `docs/STREAMING_BODY_FORWARD.md`):
//! `request_filter` (partial body read) → `upstream_peer` → `upstream_request_filter` →
//! `request_body_filter` (remaining chunks + finalize) → upstream response.
//!
//! Cache/coalesce and `prepare_mimo_request` run at client body EOS so exact keys stay correct.

use crate::context::GatewayContext;
use crate::proxy::GatewayProxy;
use crab_pipeline::RequestPipeline;
use http::HeaderMap;
use sha2::{Digest, Sha256};

/// POST chat completions only (body carries `model`).
pub fn path_eligible(path: &str, method: &http::Method) -> bool {
    method == http::Method::POST
        && (path == "/v1/chat/completions" || path.ends_with("/v1/chat/completions"))
}

pub fn feature_enabled(proxy: &GatewayProxy) -> bool {
    proxy.state.features.streaming_body_forward
}

pub fn pipeline_eligible(pipeline: RequestPipeline) -> bool {
    GatewayProxy::is_mimo_pipeline(pipeline)
}

/// Recompute Ketama affinity after `session_fingerprint` / body `user` are known.
pub fn refresh_affinity_key(
    ctx: &mut GatewayContext,
    affinity_headers: &HeaderMap,
    client_ip: &str,
    body_user_id: Option<&str>,
) {
    ctx.upstream.affinity_key = Some(crab_route::extract_affinity_key(
        affinity_headers,
        client_ip,
        ctx.conversation_id.as_deref(),
        ctx.prompt_cache_key.as_deref(),
        body_user_id,
        ctx.session_fingerprint.as_deref(),
    ));
}

pub fn body_user_from_payload(payload: &serde_json::Value) -> Option<&str> {
    payload
        .get("user")
        .and_then(|u| u.as_str())
        .filter(|s| !s.is_empty())
}

/// Incremental SHA-256 over streamed chunks (must match one-shot body hash).
pub fn hash_body_chunks(chunks: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for c in chunks {
        hasher.update(c);
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_hash_matches_one_shot() {
        let body = br#"{"model":"mimo","messages":[],"stream":true}"#;
        let chunks: Vec<&[u8]> = vec![&body[..20], &body[20..]];
        let inc = hash_body_chunks(&chunks);
        let mut hasher = Sha256::new();
        hasher.update(body);
        let full = hex::encode(hasher.finalize());
        assert_eq!(inc, full);
    }

    #[test]
    fn refresh_affinity_prefers_sfp_without_user() {
        let mut ctx = crate::context::GatewayContext::new("req".into());
        ctx.session_fingerprint = Some("abc123def456".into());
        let headers = http::HeaderMap::new();
        refresh_affinity_key(&mut ctx, &headers, "192.168.1.1", None);
        assert_eq!(
            ctx.upstream.affinity_key.as_deref(),
            Some("sfp:abc123def456")
        );
    }

    #[test]
    fn path_eligible_chat_completions() {
        assert!(path_eligible("/v1/chat/completions", &http::Method::POST));
        assert!(!path_eligible("/v1/models", &http::Method::GET));
    }
}
