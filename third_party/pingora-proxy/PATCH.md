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
