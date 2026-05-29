//! Contract tests for MiMo `streaming_body_forward` (defer path).
//!
//! Full Pingora e2e requires a live gateway; these tests lock the invariants that
//! prevent empty/truncated upstream bodies and document suppress-on-handled behavior.

mod common;

use bytes::Bytes;
use crab_cache::{FingerprintConfig, generate_cache_key_with_fingerprint};
use crab_proxy::upstream_body::{apply_prepared_upstream_body, should_emit_prepared_upstream_body};
use crab_proxy::{
    StreamingBodyState, StreamingDeferCircuitBreaker, defer_body_incomplete,
    defer_partial_ready_for_arm, hash_body_chunks,
    should_skip_upstream_trailing_empty_eos,
};
use crab_reasoning::prepare_mimo_request;

fn large_mimo_body(content_len: usize) -> Vec<u8> {
    let content = "x".repeat(content_len);
    format!(
        r#"{{"model":"mimo-v2.5-pro","messages":[{{"role":"user","content":"{content}"}}],"stream":true}}"#
    )
    .into_bytes()
}

#[test]
fn defer_assembled_body_matches_incremental_hash() {
    let body = large_mimo_body(80_000);
    const SPLIT: usize = 64 * 1024;
    assert!(body.len() > SPLIT);

    let mut state = StreamingBodyState::default();
    state.active = true;
    state.deferred_partial_len = SPLIT;
    state.buffer = body[..SPLIT].to_vec();
    for chunk in body[SPLIT..].chunks(8192) {
        state.append_chunk(chunk);
    }

    assert_eq!(state.buffer.len(), body.len());
    assert!(!defer_body_incomplete(&state.buffer, Some(body.len())));

    let chunks: Vec<&[u8]> = vec![&body[..SPLIT], &body[SPLIT..]];
    assert_eq!(
        hash_body_chunks(&chunks),
        hash_body_chunks(&[state.buffer.as_slice()])
    );
}

#[test]
fn defer_partial_arm_allows_large_prefix_before_full_content_length() {
    let body = large_mimo_body(40_000);
    let split = 32 * 1024;
    let partial = &body[..split];
    assert!(partial.len() >= 32 * 1024);
    assert!(defer_partial_ready_for_arm(partial));
    assert!(defer_body_incomplete(&body, Some(body.len() + 10_000)));
}

#[test]
fn defer_truncated_json_detected_at_finalize_gate() {
    let body = large_mimo_body(50_000);
    let split = 32 * 1024;
    let truncated = &body[..split];
    assert!(defer_body_incomplete(truncated, None));
    assert!(!defer_body_incomplete(&body, Some(body.len())));
}

#[test]
fn defer_finalize_prepare_mimo_produces_nonempty_upstream() {
    let body = large_mimo_body(60_000);
    let split = 32 * 1024;
    let mut state = StreamingBodyState::default();
    state.deferred_partial_len = split;
    state.buffer = body[..split].to_vec();
    state.append_chunk(&body[split..]);

    let payload: serde_json::Value = serde_json::from_slice(&state.buffer).expect("assembled JSON");
    let prepared = prepare_mimo_request(&payload, "xiaomi/mimo-v2.5-pro", true, 6);
    let upstream = serde_json::to_vec(&prepared.payload).expect("upstream serialize");
    assert!(!upstream.is_empty());
    assert!(serde_json::from_slice::<serde_json::Value>(&upstream).is_ok());
}

#[test]
fn cache_hit_suppress_skips_trailing_empty_eos() {
    let mut ctx = crab_proxy::GatewayContext::new("defer-cache-hit".into());
    ctx.streaming_body.active = true;
    ctx.streaming_body.suppress_upstream = true;
    assert!(should_skip_upstream_trailing_empty_eos(&ctx));
}

#[test]
fn handled_finalize_without_body_suppresses_upstream_emit() {
    let mut ctx = crab_proxy::GatewayContext::new("defer-handled".into());
    ctx.streaming_body.active = true;
    ctx.streaming_body.finalized = true;
    ctx.streaming_body.suppress_upstream = true;
    assert!(ctx.new_request_body.is_none());
    assert!(should_skip_upstream_trailing_empty_eos(&ctx));
}

#[test]
fn defer_emit_at_eos_waits_for_client_eos() {
    assert!(!should_emit_prepared_upstream_body(false, false));
    assert!(should_emit_prepared_upstream_body(true, false));
}

#[test]
fn prepared_body_replaces_client_chunk_at_eos() {
    let prepared = Bytes::from_static(br#"{"model":"mimo","messages":[]}"#);
    let mut chunk = Some(Bytes::from_static(b"partial"));
    let rest = apply_prepared_upstream_body(prepared.clone(), &mut chunk, true, false);
    assert!(rest.is_none());
    assert_eq!(chunk.unwrap(), prepared);
}

#[test]
fn defer_cache_key_requires_complete_json() {
    let body = large_mimo_body(50_000);
    let split = 32 * 1024;
    let partial = &body[..split];
    let fp = FingerprintConfig::default_v1();
    let key_full = generate_cache_key_with_fingerprint(&body, &fp).expect("full key");
    assert!(generate_cache_key_with_fingerprint(partial, &fp).is_err());
    assert_ne!(
        key_full,
        hash_body_chunks(&[partial]),
        "partial bytes must not match full-body cache key"
    );
}

#[test]
fn circuit_breaker_blocks_after_threshold() {
    let cb = StreamingDeferCircuitBreaker::new(2);
    cb.record_failure();
    assert!(cb.defer_allowed());
    cb.record_failure();
    assert!(!cb.defer_allowed());
}
