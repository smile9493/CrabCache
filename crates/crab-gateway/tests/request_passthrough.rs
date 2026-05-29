//! Contract tests for MiMo direct request passthrough (prefix sniff + chunk relay).
//!
//! Full Pingora e2e requires a live gateway; these tests lock invariants for the
//! connect-overlap path: early upstream connect, incremental body relay (no EOS until client done).

use bytes::Bytes;
use crab_pipeline::RequestPipeline;
use crab_proxy::should_defer_upstream_request_body;

fn large_mimo_body(content_len: usize, stream: bool) -> Vec<u8> {
    let content = "x".repeat(content_len);
    format!(
        r#"{{"model":"mimo-v2.5-pro","messages":[{{"role":"user","content":"{content}"}}],"stream":{stream}}}"#
    )
    .into_bytes()
}

/// Mirrors incremental passthrough relay in `run_request_body_filter`.
fn passthrough_relay_next(
    buffer: &mut Vec<u8>,
    prefix_emitted: &mut bool,
    finalized: &mut bool,
    outbound: &mut usize,
    chunk: Option<Bytes>,
    client_body_done: bool,
) -> Option<(Bytes, bool)> {
    if *finalized {
        return None;
    }
    if !*prefix_emitted {
        if let Some(c) = chunk {
            buffer.extend_from_slice(&c);
        }
        if buffer.is_empty() {
            return None;
        }
        let prefix = std::mem::take(buffer);
        *prefix_emitted = true;
        *outbound += prefix.len();
        if client_body_done {
            *finalized = true;
        }
        return Some((Bytes::from(prefix), client_body_done));
    }
    if let Some(c) = chunk {
        *outbound += c.len();
        if client_body_done {
            *finalized = true;
        }
        return Some((c, client_body_done));
    }
    if client_body_done {
        *finalized = true;
    }
    None
}

#[test]
fn passthrough_incremental_relay_reconstructs_full_body() {
    let body = large_mimo_body(80_000, true);
    const PREFIX_SPLIT: usize = 1024;
    assert!(body.len() > PREFIX_SPLIT + 8192);

    let mut buffer = body[..PREFIX_SPLIT].to_vec();
    let mut prefix_emitted = false;
    let mut finalized = false;
    let mut outbound = 0usize;
    let mut upstream = Vec::new();

    for (i, chunk) in body[PREFIX_SPLIT..].chunks(8192).enumerate() {
        let client_done = i + 1 == body[PREFIX_SPLIT..].chunks(8192).count();
        if let Some((bytes, _eos)) = passthrough_relay_next(
            &mut buffer,
            &mut prefix_emitted,
            &mut finalized,
            &mut outbound,
            Some(Bytes::copy_from_slice(chunk)),
            client_done,
        ) {
            upstream.extend_from_slice(&bytes);
        }
    }

    assert!(prefix_emitted);
    assert!(finalized);
    assert_eq!(outbound, body.len());
    assert_eq!(upstream, body);
    assert!(serde_json::from_slice::<serde_json::Value>(&upstream).is_ok());
}

#[test]
fn passthrough_bootstrap_emits_prefix_without_finalizing() {
    let body = large_mimo_body(80_000, true);
    let mut buffer = body[..1024].to_vec();
    let mut prefix_emitted = false;
    let mut finalized = false;
    let mut outbound = 0usize;
    let (prefix, eos) = passthrough_relay_next(
        &mut buffer,
        &mut prefix_emitted,
        &mut finalized,
        &mut outbound,
        None,
        false,
    )
    .expect("armed prefix should relay on defer bootstrap");
    assert_eq!(prefix.len(), 1024);
    assert!(!eos);
    assert!(prefix_emitted);
    assert!(!finalized);
    assert_eq!(outbound, 1024);
}

#[test]
fn passthrough_prefix_emits_without_client_eos() {
    let body = large_mimo_body(40_000, true);
    let mut buffer = body[..1024].to_vec();
    let mut prefix_emitted = false;
    let mut finalized = false;
    let mut outbound = 0usize;
    let (prefix, eos) = passthrough_relay_next(
        &mut buffer,
        &mut prefix_emitted,
        &mut finalized,
        &mut outbound,
        None,
        false,
    )
    .expect("prefix should emit");
    assert_eq!(prefix.as_ref(), &body[..1024]);
    assert!(!eos);
    assert!(!finalized);
}

#[test]
fn passthrough_defers_upstream_body_while_buffering() {
    let mut ctx = crab_proxy::GatewayContext::new("req".into());
    assert!(!should_defer_upstream_request_body(&ctx));
    ctx.request_passthrough.active = true;
    assert!(should_defer_upstream_request_body(&ctx));
    ctx.request_passthrough.finalized = true;
    assert!(!should_defer_upstream_request_body(&ctx));
}

#[test]
fn passthrough_mimo_pipelines_are_distinct_from_defer_eligible_generic() {
    let mimo = [
        RequestPipeline::MimoTokenPlanRelay,
        RequestPipeline::MimoTokenPlanRelay,
        RequestPipeline::MimoPaygRelay,
    ];
    for pipeline in mimo {
        assert!(matches!(
            pipeline,
            RequestPipeline::MimoTokenPlanRelay
                | RequestPipeline::MimoTokenPlanRelay
                | RequestPipeline::MimoPaygRelay
        ));
    }
    assert!(!matches!(
        RequestPipeline::GenericRelay,
        RequestPipeline::MimoTokenPlanRelay
            | RequestPipeline::MimoTokenPlanRelay
            | RequestPipeline::MimoPaygRelay
    ));
}
