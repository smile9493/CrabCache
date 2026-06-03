# pingora-proxy 0.8.0 local patch

## 1. Retry buffer truncated fix

Upstream CrabCache reads the full request body in `request_filter` (often >64KiB for Cursor).
Pingora's retry buffer is capped at 64KiB; when truncated, the proxy skipped the initial
`send_body_to_pipe` call, so replaced upstream bodies never reached DeepSeek.

- `src/proxy_h1.rs`: also call `send_body_to_pipe` when `session.retry_buffer_truncated()`
- `src/proxy_h2.rs`: same for H2 upstream path

CrabCache `request_body_filter` injects `new_request_body` on that path.

## 1c. Graceful Responses stream finalize on upstream abort (CrabCache)

When MiMo/upstream closes mid-SSE, Pingora may reach `finish_body` or emit `HttpTask::Failed`
without running CrabCache EOS synthesis. `ProxyHttp::finalize_aborted_upstream_stream` lets
`GatewayProxy` append synthetic `response.completed` (+ chain store) before the downstream
connection is torn down.

- `src/proxy_trait.rs`: new hook default `None`
- `src/proxy_h1.rs` / `src/proxy_h2.rs`: call hook before `finish_body`; convert `Failed` → `Body` when tail returned

## 1d. H2 pipe drain after downstream early finish (CrabCache Responses)

When CrabCache forces downstream EOS after Responses `[DONE]`, `bidirection_down_to_up` may
close the H2→downstream pipe while MiMo upstream still has trailing DATA frames.
`pipe_up_to_down_response` treats any failed `Body` send on a closed channel as success (drain
and exit) instead of surfacing `InternalError: channel closed`.

## 1b. Streaming defer + trailing empty EOS (CrabCache)

`ProxyHttp::defer_upstream_request_body` / `skip_upstream_trailing_empty_eos` (implemented on
`GatewayProxy`):

- H2: do not send empty END_STREAM DATA before deferred body; skip duplicate empty EOS after
  prepared JSON was written.
- H1: same via `send_body_to_pipe` startup path and trailing-empty guard.

## 2. Arc-wrapped Connector for shared connection pool pre-warm

`HttpProxy.client_upstream` changed from `Connector<C>` to `Arc<Connector<C>>`.

**Why**: CrabCache needs to share the same `Connector` (and its internal TCP/TLS
connection pool) between the proxy hot path and runtime pre-warm tasks. Without
`Arc`, the `Connector` is owned exclusively by `HttpProxy` and inaccessible after
`Service::new()` takes ownership.

**API additions**:
- `HttpProxy::connector_arc() -> Arc<Connector<C>>` — clone the `Arc` for injection
  into `GatewayState` before `Service::new()` takes the proxy.
- `HttpProxy::connector_ref()` — kept for backward compat, returns `&Connector<C>`.

**Usage (CrabCache main.rs)**:
```rust
let mut proxy = http_proxy(&server.conf, GatewayProxy::new(state.clone()));
*state.upstream_connector.write() = Some(proxy.connector_arc());
server.add_service(Service::new("crab-gateway", proxy));
```

Runtime pre-warm then calls `connector.get_http_session(&peer)` followed by
`connector.release_http_session(session, &peer, idle_timeout)` to populate the
pool without sending any HTTP traffic.

---

## Regression test index

Each patch maps to test functions in `crates/crab-gateway/tests/pingora_lifecycle.rs`
(and existing test modules). Run with `cargo test -p crab-gateway --test pingora_lifecycle`.

| Patch | Test name | What it verifies |
|-------|-----------|------------------|
| 1 | `patch1_retry_buffer_truncated_emits_prepared_body` | Prepared body emitted immediately when `retry_buffer_truncated=true` |
| 1 | `patch1_large_body_over_64kib_triggers_truncated_path` | Large body context flag triggers emission path |
| 1 | `patch1_normal_body_not_truncated_waits_for_eos` | Normal (non-truncated) body held until EOS |
| 1b | `patch1b_defer_body_active_when_passthrough_armed` | `defer_upstream_request_body` returns true only while passthrough is active and not finalized |
| 1b | `patch1b_skip_trailing_empty_eos_while_passthrough_buffering` | `skip_upstream_trailing_empty_eos` returns true during passthrough buffering or after prepared body emitted |
| 1b | `patch1b_defer_body_end_stream_follows_passthrough_state` | End-stream flag follows passthrough finalization state |
| 1c | `patch1c_finalize_aborted_returns_none_when_no_response_written` | `finalize_aborted_upstream_stream` returns None when no response written |
| 1c | `patch1c_responses_wire_force_downstream_eos` | `responses_wire_force_downstream_eos` flag can be set/consumed |
| 1d | `patch1d_force_eos_does_not_cause_panic` | Force downstream EOS flag is a valid operation |
| 2 | `patch2_connector_arc_compiles` | Arc-wrapped Connector API compiles (compilation = contract) |
| StreamCapture | `stream_capture_under_limit_appends_fully` | Under-limit data appended fully |
| StreamCapture | `stream_capture_over_limit_retains_tail` | Over-limit data retains tail window |
| StreamCapture | `stream_capture_sliding_tail_window` | Tail window slides as more data arrives |
| StreamCapture | `stream_capture_take_returns_vec_and_resets` | `take()` returns Vec and resets state |
| StreamCapture | `stream_capture_reconfigure_applies_limit` | `reconfigure()` applies new limit retroactively |
| StreamCapture | `stream_capture_zero_max_means_unbounded` | `max_bytes=0` means unbounded (backward compat) |
| StreamCapture | `stream_capture_deref_allows_slice_operations` | `Deref<Target=[u8]>` enables slice operations |
| StreamCapture | `streaming_accumulated_body_enforcement_truncates_to_tail` | `accumulated_body` enforcement in streaming path |
