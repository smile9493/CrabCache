# `streaming_body_forward` — phase order and behavior

## Pingora phase order (spike)

For a cache-miss upstream request, Pingora runs:

1. `early_request_filter`
2. **`request_filter`** — CrabCache auth, optional **partial** body read
3. `proxy_upstream_filter`
4. **`upstream_peer`** — Ketama + **TCP/TLS connect** (can overlap with remaining client body)
5. **`upstream_request_filter`** — Host, API key, body framing
6. **`request_body_filter`** — Remaining client chunks; at EOS: parse, cache, `prepare_mimo`, emit upstream body
7. `response_filter` — `upstream.start` / `upstream_response_headers` timeline
8. `upstream_response_body_filter` / `logging`

Mount point: **do not** read the full body in `request_filter` when deferring; finish read in `request_body_filter` so step 4 can start while the client is still uploading.

## Constraints

| Topic | Behavior |
|-------|----------|
| Exact cache key | Incremental SHA-256 while reading; L0/L1/coalesce at **body EOS** |
| MiMo early exact cache | Skipped while defer active; same lookup at EOS |
| `prepare_mimo_request` | Runs at EOS (needs full JSON) |
| Default | `[features] streaming_body_forward = false` |
| Retry buffer | Partial reads in `request_filter` must **not** use `enable_retry_buffering()` (would send incomplete JSON upstream); body ships at EOS only |
| Pipelines | `MimoTokenPlanRelay`（含 `MimoRelay` / `MimoPaygRelay` 别名） only |
| Cache hit at EOS | `send_cached_response`; upstream may have connected idle (acceptable waste) |
| Defer finalize early exact | At EOS, exact L0/L1 lookup on full body **before** full JSON parse / prepare (Phase 3.1) |
| Trailing empty upstream EOS | Skipped when `prepared_upstream_body_emitted` or `suppress_upstream` (Pingora PATCH) |

## Latency fields (trace / raw_capture)

| Field | Definition |
|-------|------------|
| `prefill_ms` | Request start → upstream **response headers** (MiMo prefill SLO) |
| `ttft_ms` | Response headers → first upstream body chunk |
| `pre_header_ms` (trace legacy) | `latency_ms - upstream_latency_ms` (first-byte window, not the same as `prefill_ms`) |

`upstream.start` is connect time only; `upstream.headers_at` is set once in `response_filter` (not overwritten).

## Regression / gray gate (wuming)

[`config/gateway.docker.toml`](../config/gateway.docker.toml) enables `streaming_body_forward = true` after Phase 1 safety fixes (incomplete-body gate, suppress trailing empty EOS, circuit breaker). Re-disable immediately if metrics or logs regress.

```bash
# After hot-update:
ssh wuming 'docker exec crabcache-gateway-1 tail -10000 /app/logs/raw_capture/index.jsonl' \
  | python3 scripts/analyze_downstream_latency.py -
# 或用 crab-cli（推荐）：
crab-cli trace analyze --target wuming --tail 10000

# Logs must have zero:
#   "client JSON parse failed" with streaming_defer=true
#   Upstream 400 Param Incorrect with outbound_bytes=0
```

Success targets (MiMo stream miss, ≥10 requests):

| Check | Target |
|-------|--------|
| `gap` p50 | ≤3.5s (Phase 0+1), ≤3s (+ streaming gray) |
| `gap/e2e` | ≤45% |
| Parse / empty upstream | **0** failures |

Tests: unit — [`streaming_body_forward.rs`](../crates/crab-proxy/src/streaming_body_forward.rs); contract — [`streaming_defer.rs`](../crates/crab-gateway/tests/streaming_defer.rs), [`request_passthrough.rs`](../crates/crab-gateway/tests/request_passthrough.rs).

## MiMo direct request passthrough

MiMo pipelines (`MimoRelay`, `MimoTokenPlanRelay`, `MimoPaygRelay`) use **direct passthrough** instead of streaming defer. The gateway sniffs a prefix (≥1024 B with `"model"` in JSON), selects the pipeline, acquires an upstream key, and returns from `request_filter` before the client body is complete. The armed prefix and subsequent client chunks are **relayed incrementally** in `request_body_filter` (no upstream EOS until `session.is_body_done()`).

| Topic | Passthrough | Streaming defer (non-MiMo) |
|-------|-------------|----------------------------|
| Arm threshold | ≥1024 B prefix | ≥32 KiB prefix |
| Body rewrite | None (client JSON as-is) | `prepare_mimo_request` at EOS |
| Exact cache / coalesce | Skipped | At EOS |
| Upstream headers | H2 (default): strip `Content-Length` / `TE`; DATA frames. H1: `TE: chunked` | Strip CL/TE until EOS |
| Same-request retry | `retry_budget = 0` | Normal retry budget |
| Trace | `request_passthrough=true`, `request_passthrough_prefix_len` | `streaming_defer=true` |
| Metric | `gateway_request_passthrough_total` | `gateway_streaming_defer_*` |

Both paths require `[features] streaming_body_forward = true`. Upstream defaults to HTTP/2 (`[connection] upstream_force_http1 = false`). Passthrough uses `prepare_passthrough_upstream_headers` (H2: no framing headers; H1 fallback: chunked).
