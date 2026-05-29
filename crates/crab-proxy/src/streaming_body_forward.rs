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
use pingora_proxy::Session;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingFinalizeOutcome {
    /// Response already sent (cache hit / error).
    Handled,
    /// Prepared upstream body; caller should emit via `request_body_filter`.
    ContinueUpstream,
}

/// State for deferred MiMo body processing.
#[derive(Default)]
pub struct StreamingBodyState {
    pub active: bool,
    pub buffer: Vec<u8>,
    pub finalized: bool,
    /// Emit prepared upstream body at client EOS (do not piggyback on `retry_buffer_truncated`).
    pub streaming_defer_emit_at_eos: bool,
    /// When true, suppress upstream chunks (cache hit or error during finalize).
    pub suppress_upstream: bool,
    /// Body read in `request_filter` before defer (remaining chunks append in `request_body_filter`).
    pub deferred_partial_len: usize,
    /// Inbound `Content-Length` when present (defer only safe once partial >= this).
    pub inbound_content_length: Option<usize>,
    /// Bytes appended in `request_body_filter` after the partial handoff (diagnostics).
    pub append_tail_bytes: usize,
    pub key_pipeline: Option<String>,
    pub key_upstream_profile: Option<String>,
    pub domain_pipeline: Option<String>,
    pub domain_upstream_profile: Option<String>,
}

impl StreamingBodyState {
    pub fn append_chunk(&mut self, data: &[u8]) {
        if self.active && self.deferred_partial_len > 0 && self.buffer.len() >= self.deferred_partial_len {
            self.append_tail_bytes += data.len();
        }
        self.buffer.extend_from_slice(data);
    }
}

/// Opens after consecutive parse/empty-body failures; half-open after 60s.
pub struct StreamingDeferCircuitBreaker {
    threshold: u32,
    inner: parking_lot::Mutex<CbInner>,
}

struct CbInner {
    consecutive_failures: u32,
    open_until: Option<std::time::Instant>,
    half_open: bool,
}

impl StreamingDeferCircuitBreaker {
    pub fn new(threshold: u32) -> Self {
        Self {
            threshold,
            inner: parking_lot::Mutex::new(CbInner {
                consecutive_failures: 0,
                open_until: None,
                half_open: false,
            }),
        }
    }

    /// Whether a new defer handoff is allowed.
    pub fn defer_allowed(&self) -> bool {
        if self.threshold == 0 {
            return true;
        }
        let mut g = self.inner.lock();
        let now = std::time::Instant::now();
        if let Some(until) = g.open_until {
            if now < until {
                return false;
            }
            g.open_until = None;
            g.half_open = true;
        }
        if g.half_open {
            g.half_open = false;
        }
        true
    }

    pub fn record_success(&self) {
        let mut g = self.inner.lock();
        g.consecutive_failures = 0;
        g.open_until = None;
        g.half_open = false;
    }

    pub fn record_failure(&self) {
        if self.threshold == 0 {
            return;
        }
        let mut g = self.inner.lock();
        if g.half_open {
            g.open_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
            g.half_open = false;
            g.consecutive_failures = 0;
            return;
        }
        g.consecutive_failures = g.consecutive_failures.saturating_add(1);
        if g.consecutive_failures >= self.threshold {
            g.open_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
            g.consecutive_failures = 0;
        }
    }
}

/// Parse inbound `Content-Length` when the client/proxy sent a fixed body size.
pub fn inbound_content_length(session: &Session) -> Option<usize> {
    session
        .req_header()
        .headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

/// Whether a partial buffer must not be treated as a complete JSON body yet.
pub fn defer_body_incomplete(body: &[u8], inbound_cl: Option<usize>) -> bool {
    if inbound_cl.is_some_and(|cl| body.len() < cl) {
        return true;
    }
    if serde_json::from_slice::<serde_json::Value>(body).is_ok() {
        return false;
    }
    let Ok(text) = std::str::from_utf8(body) else {
        return true;
    };
    let trimmed = text.trim_end();
    trimmed.is_empty() || !(trimmed.ends_with('}') || trimmed.ends_with(']'))
}

/// Try to arm streaming defer after `model` is known from the partial buffer.
pub fn try_arm_defer(
    proxy: &GatewayProxy,
    path: &str,
    method: &http::Method,
    pipeline: RequestPipeline,
    session: &mut Session,
) -> bool {
    feature_enabled(proxy)
        && path_eligible(path, method)
        && pipeline_eligible(pipeline)
        && !session.is_body_done()
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

    #[test]
    fn deferred_buffer_assembly_parses_valid_json() {
        let body = br#"{"model":"mimo-v2.5-pro","messages":[{"role":"user","content":"hi"}],"stream":true}"#;
        const SPLIT: usize = 48;
        let mut state = StreamingBodyState::default();
        state.deferred_partial_len = SPLIT;
        state.buffer = body[..SPLIT].to_vec();
        state.append_chunk(&body[SPLIT..]);
        assert_eq!(state.buffer.len(), body.len());
        let parsed: serde_json::Value =
            serde_json::from_slice(&state.buffer).expect("assembled body must be valid JSON");
        assert_eq!(
            parsed.get("model").and_then(|m| m.as_str()),
            Some("mimo-v2.5-pro")
        );
    }

    #[test]
    fn append_chunk_tracks_tail_after_defer() {
        let body = b"0123456789";
        let mut state = StreamingBodyState::default();
        state.active = true;
        state.deferred_partial_len = 4;
        state.buffer = body[..4].to_vec();
        state.append_chunk(&body[4..]);
        assert_eq!(state.append_tail_bytes, 6);
        assert_eq!(state.buffer.len(), body.len());
    }

    #[test]
    fn circuit_breaker_opens_after_threshold_failures() {
        let cb = StreamingDeferCircuitBreaker::new(3);
        assert!(cb.defer_allowed());
        cb.record_failure();
        cb.record_failure();
        assert!(cb.defer_allowed());
        cb.record_failure();
        assert!(!cb.defer_allowed());
    }

    #[test]
    fn circuit_breaker_success_resets_failures() {
        let cb = StreamingDeferCircuitBreaker::new(3);
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        cb.record_failure();
        cb.record_failure();
        assert!(cb.defer_allowed());
    }

    #[test]
    fn defer_body_incomplete_respects_content_length() {
        let partial = br#"{"model":"mimo","messages":[]}"#;
        assert!(defer_body_incomplete(partial, Some(5000)));
        assert!(!defer_body_incomplete(partial, Some(partial.len())));
    }

    #[test]
    fn defer_body_incomplete_detects_truncated_json() {
        let partial = br#"{"model":"mimo","messages":[{"role":"user","content":"hello"#;
        assert!(defer_body_incomplete(partial, None));
        let complete = br#"{"model":"mimo","messages":[]}"#;
        assert!(!defer_body_incomplete(complete, None));
    }

    #[test]
    fn deferred_buffer_large_payload_roundtrip() {
        let content = "x".repeat(80_000);
        let body = format!(
            r#"{{"model":"mimo-v2.5-pro","messages":[{{"role":"user","content":"{content}"}}],"stream":true}}"#
        );
        let bytes = body.as_bytes();
        let split = 64 * 1024;
        let mut state = StreamingBodyState::default();
        state.deferred_partial_len = split;
        state.buffer = bytes[..split].to_vec();
        for chunk in bytes[split..].chunks(8192) {
            state.append_chunk(chunk);
        }
        assert_eq!(state.buffer.len(), bytes.len());
        serde_json::from_slice::<serde_json::Value>(&state.buffer)
            .expect("large assembled body must parse");
    }
}
