# [OPEN] Prefill Latency Debug

## Session
- session_id: `prefill-latency`
- started_at: `2026-05-29`
- scope: `mimo_relay prefill latency remains high after direct passthrough`

## Symptoms
- `prefill` median remains high on `wuming` after `mimo` direct passthrough.
- Recent analysis shows `prefill p50` is much higher than `upstream p50`.
- `streaming_defer` is effectively disabled, so the old defer bug path is no longer the main suspect.

## Falsifiable Hypotheses
1. Large request bodies dominate pre-header time, so `prefill` grows primarily with inbound body size before upstream responds.
2. Upstream-side queueing or provider-side scheduling dominates `prefill`, independent of gateway body processing.
3. Some upstream keys or accounts are slower than others, inflating `prefill` through routing bias.
4. Some upstream backends are slower before first header, inflating `prefill` even when `upstream_latency_ms` stays moderate.
5. Gateway-side body read / buffering / request assembly still contributes material latency before the upstream request is fully issued.

## Evidence Collected
- `prefill_ms` recent windows stay much higher than `upstream_latency_ms` on `wuming`.
- Latest `tail=200` analysis:
  - `prefill p50 ≈ 30.8s`
  - `upstream p50 ≈ 5.7s`
  - `e2e p50 ≈ 39.5s`
- Phase metrics (`gateway_request_phase_latency_seconds`) imply:
  - `body_read_done avg ≈ 26.96s`
  - `upstream_headers_sent avg ≈ 27.96s`
  - `upstream_body_sent avg ≈ 27.96s`
  - `upstream_response_headers/prefill_done avg ≈ 33.73s`
- Therefore:
  - request start -> full body read dominates the first ~27s
  - body-ready -> upstream body sent is only ~1s
  - upstream body sent -> upstream response headers is ~5.8s
- Body size strongly correlates with `prefill`:
  - `<150KB`: `prefill p50 ≈ 15.8s`
  - `150-250KB`: `prefill p50 ≈ 21.7s`
  - `250-350KB`: `prefill p50 ≈ 29.6s`
  - `>=350KB`: `prefill p50 ≈ 39.0s`
- Backend differences exist but are secondary to body size:
  - fastest sampled backend median prefill is still ~27s
  - slowest sampled backend medians are ~35-38s
- Key differences exist but are modest:
  - `key-1..key-4` prefill medians are all roughly `28-33s`
  - `upstream` medians differ more than `prefill`
- Current key pool is multi-key but not multi-account:
  - all visible `account_id` values are `default`

## Hypothesis Status
1. Large request bodies dominate pre-header time before upstream responds.
   - Supported.
2. Upstream-side queueing or provider-side scheduling dominates `prefill`.
   - Partially supported; it contributes ~5-6s on average after upstream body send, but does not explain the whole ~30s median alone.
3. Some upstream keys or accounts are slower than others, inflating `prefill` through routing bias.
   - Weak support for key-level variance; not enough evidence for account-level impact because current pool is not multi-account.
4. Some upstream backends are slower before first header, inflating `prefill` even when `upstream_latency_ms` stays moderate.
   - Supported, but secondary to body size / pre-header upload time.
5. Gateway-side body read / buffering / request assembly still contributes material latency before the upstream request is fully issued.
   - Partially supported only for body-read time itself; request assembly/sending after body-ready appears small (~1s), so gateway compute/transform is not the main bottleneck.

## Next Steps
1. Re-read latest trace/log aggregates for `prefill`, `body_read_done`, `upstream_connect_done`, and `prefill_done`.
2. Compare `prefill` against body size buckets.
3. Compare `prefill` against `backend_name`.
4. Compare `prefill` against `upstream_key_id` and `account_id`.
5. Decide whether more instrumentation is needed or existing trace is already sufficient.

## Fix Deployed (2026-05-30)
- MiMo **direct request passthrough**: prefix sniff (≥1024 B) → early upstream connect → chunk relay without full-body buffering.
- Trace fields: `request_passthrough`, `request_passthrough_prefix_len`.
- Metric: `gateway_request_passthrough_total`.
- Same-request retry disabled (`retry_budget = 0`); exact cache / coalesce / full raw capture skipped on this path.

## Verification Criteria (wuming gray)

| Check | Baseline (pre-passthrough) | Target (passthrough hit) |
|-------|---------------------------|--------------------------|
| `body_read_done` avg | ~27s | Seconds (prefix read), not linear with body size |
| `prefill_ms` p50 (≥350KB) | ~39s | Significantly lower; ~5–6s upstream queue remains |
| `gateway_request_passthrough_total` | 0 | Increments with large MiMo requests |
| Trace `request_passthrough` rate | 0% | >0% on stream miss MiMo traffic |
| Upstream 400 + `outbound_bytes=0` | 0 | 0 (rollback if not) |
| `upstream_outbound_bytes ≈ content_length` | — | No systematic under-count |

```bash
ssh wuming 'curl -s http://127.0.0.1:9090/metrics' | grep request_passthrough
ssh wuming 'docker exec crabcache-gateway-1 tail -10000 /app/logs/raw_capture/index.jsonl' \
  | python3 scripts/analyze_gateway_logs.py -
```

## wuming Results (2026-05-30 hot-update)

- Gateway ready: `GET /v1/ready` → 200
- `gateway_request_passthrough_total`: **7** (within ~15 min post-deploy)
- Phase latency (passthrough sample, n=7 MiMo):
  - `upstream_headers_sent` avg **~1482ms** (baseline was ~27s)
  - `upstream_body_sent` avg **~1580ms**
  - `pipeline_select_done` avg **~817ms**
- Trace schema: `request_passthrough` field present in `/app/logs/trace.jsonl`
- Regressions observed (monitor):
  - Several upstream **400 Param Incorrect** with non-zero `outbound_bytes` (32–65KB) — likely upstream JSON validation, not empty-body
  - **1** request with `outbound_bytes=0` + 400 (`ded22ce5`) — watch for recurrence
  - No `incomplete_body` / `streaming_defer` parse failures in last 15m logs

**Conclusion:** Passthrough arm is active and phase metrics confirm the primary SLO win (headers/body to upstream in ~1.5s vs ~27s). Continue monitoring 400 rate on passthrough path; rollback if `outbound_bytes=0` 400s become frequent.
