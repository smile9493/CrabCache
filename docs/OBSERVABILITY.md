# CrabCache Observability

Three layers: **Prometheus (real-time)**, **Admin Dashboard (operations)**, **shadow trace log (offline tuning)**.

## Metric definitions

| Metric / UI field | Source | Meaning |
|-------------------|--------|---------|
| `hit_rate_5m` (Dashboard) | Admin metrics ring, 5m delta | Gateway L0–L2 request hit rate |
| `token_hit_rate_5m` | `gateway_deepseek_input_tokens_total` delta | Token-weighted input hit rate (cost view) |
| `hit_rate_cumulative` | Counter / uptime | Since gateway process start |
| `prefix_cache_hit_ratio` (L3) | `gateway_upstream_prompt_cache_tokens_total` | DeepSeek upstream prefix cache |
| `cache_hit_ratio` (Trace page) | `trace.jsonl` last N hours | Shadow log measured hits |
| `semantic_hits/rejected/skipped` | `gateway_semantic_cache_requests_total` | Semantic guard status (not L2 tier hits) |
| `coalesced_total` / `coalesced_5m` | `gateway_coalesced_requests_total` | Merged concurrent duplicate keys (5m from ring delta) |
| `cost_saved_usd_total` / `cost_saved_usd_5m` | `gateway_cache_cost_saved_usd_total` | Gateway-estimated USD saved |
| `rejected_total` / `rejected_5m` | `gateway_rejected_requests_total` | Rejected requests |
| `ttft_ms` | `gateway_stream_first_token_latency_seconds` | Average TTFT (histogram) |
| `tier_deltas_5m` | `gateway_cache_requests_total` by tier | L0/L1/L2/miss request counts in 5m window |
| `trace_summary.cache_hit_ratio` | `trace.jsonl` (24h, cached 60s) | Shadow log hit rate for Trace compare banner |
| `suggestions[]` | Rule engine in `build_overview` | Actionable hints under hit-rate and time-series cards |

## Admin Dashboard

- Overview polls **`GET /api/admin/overview`** every 5s (single bundle). `GET /api/admin/metrics` remains for backward compatibility.
- Overview bundle fields:

| Field | Description |
|-------|-------------|
| `metrics` | Extended `MetricsSnapshot` (5m rates, tier deltas, history meta, time series) |
| `health` | Gateway reachability, stream cache, upstream key pool counts |
| `prefix_cache` | L3 global + `by_model` table |
| `semantic` | `enabled` + `similarity_threshold` |
| `trace_summary` | 24h shadow log summary (60s server cache) |
| `ops` | Cost saved, coalescing/rejected 5m, TTFT, prefix_break, reasoning store, SSE omitted |
| `suggestions` | Rule-based ops hints (`severity`, `target`, `message`) for Overview cards |

- Time series buckets come from a **60s metrics sampler** (`CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS`, default 60). Empty charts mean “collecting” — wait 1–2 minutes after startup.
- `metrics_sample_insufficient` is true when the 5m window has fewer than 5 requests; UI shows “—” for window rates.
- UI legend separates **L0–L2** (gateway full response) from **L3** (upstream prefix tokens).

Environment:

| Variable | Default | Description |
|----------|---------|-------------|
| `CRABCACHE_GATEWAY_METRICS_URL` | `http://127.0.0.1:9090/metrics` | Prometheus scrape target |
| `CRABCACHE_GATEWAY_METRICS_CACHE_TTL_SECS` | `2` | Admin dedupes scrapes within this window (overview polls every 5s) |
| `CRABCACHE_GATEWAY_METRICS_STALE_SECS` | `30` | On scrape failure, serve last good body up to this age instead of HTTP 503 |
| `CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS` | `60` | History ring sample interval |
| `CRABCACHE_TRACE_LOG_PATH` | `/app/logs/trace.jsonl` | Shadow log for Trace/Logs pages |
| `CRABCACHE_UPSTREAM_RECONCILE_INTERVAL_SECS` | `30` | Min interval for `GET /upstream/config` gateway reconcile |
| `CRABCACHE_GATEWAY_PROBE_TTL_SECS` | `3` | Cache TTL for bundled `/v1/ready` + `/v1/status` in overview |

Trace analysis: `GET /api/admin/trace/analysis?hours=24` (default 24; `hours=0` = full file).

### Live client monitor (`/live`)

Dashboard page **实时监控 / Live** polls **`GET /api/admin/live-metrics`** every **2s** (per selected Consumer).

| Query | Default | Description |
|-------|---------|-------------|
| `consumer` | (required) | API key `name` / trace `consumer` |
| `window_secs` | `300` | Last 5 minutes (clamped 60–900) |
| `bucket_secs` | `5` | Aggregation bucket width |

Response: `buckets[]` (avg e2e / upstream / TTFT latency, token sums per bucket), `summary`, optional `latest` point.

| UI series | Trace field | Notes |
|-----------|-------------|-------|
| E2E latency | `latency_ms` | Client → gateway wall time |
| Upstream latency | `upstream_latency_ms` | Gateway → upstream body EOS; **miss only** |
| TTFT | `ttft_ms` | Streaming first token |
| Tokens | `input_tokens` / `output_tokens` | From upstream `usage`; cache hits use cached entry usage |

Requires `[trace_logging] enabled = true` and API keys with a **name** (consumer label). Admin tails the last **2MB** of `trace.jsonl` via **seek** (not full-file read).

**Performance:** parsed tail is cached in `crab-admin` for **1s** (invalidated on file mtime or window change). Dashboard polls every **2s** (5m window) or **3s** (15m window) and pauses when the browser tab is hidden.

**Response fields:** `available_consumers` (up to 50 names from trace), `buckets[].upstream_latency_ms` / `ttft_ms` as `null` when the bucket has no upstream/TTFT samples (cache hits).

## Shadow log

Configured in `gateway.toml`:

```toml
[trace_logging]
enabled = true
path = "/app/logs/trace.jsonl"
max_lines = 10000
max_files = 5
```

Each line is a `SanitizedLogEntry` (no raw body). Includes `consumer` (API key name), `cache_tier`, `prompt_cache_hit_ratio`, and (current gateway builds) `upstream_latency_ms`, `ttft_ms`, `input_tokens`, `output_tokens`.

Docker: gateway writes to `gateway_logs` volume; admin mounts it read-only.

## Prometheus / Grafana (optional)

```bash
docker compose -f docker-compose.yml -f docker-compose.observability.yml --profile observability up -d
```

- Prometheus: http://127.0.0.1:9091
- Grafana: http://127.0.0.1:3001 (add Prometheus data source `http://prometheus:9090`)

Example PromQL (matches Dashboard 5m hit rate):

```promql
sum(rate(gateway_cache_requests_total{result="hit"}[5m]))
/ sum(rate(gateway_cache_requests_total[5m]))
```

Token-weighted:

```promql
sum(rate(gateway_deepseek_input_tokens_total{cache_status="hit"}[5m]))
/ sum(rate(gateway_deepseek_input_tokens_total[5m]))
```

Alert rules: `config/prometheus/alerts.yml`.

### Domain label

API keys may set `domain` (business line / project). Metrics use label `domain`; unset keys record as `unclassified`. Dashboard **Domains** page and Overview domain table read `domain_buckets` from `GET /api/admin/overview`. Domain policies: `PUT /v1/domains/policies` (synced from admin `data/admin-state.json`).

Example PromQL by domain:

```promql
sum by (domain) (rate(gateway_deepseek_input_tokens_total{cache_status="hit"}[5m]))
/ sum by (domain) (rate(gateway_deepseek_input_tokens_total[5m]))
```

## Client verification

Log response headers from the gateway:

- `x-cache-status`: `HIT` / `MISS`
- `x-request-id`: correlate with gateway logs

## Dashboard 502 / 503 troubleshooting

| Symptom | Likely cause | What to check |
|---------|----------------|---------------|
| Browser **502** on the whole page (document request) | OpenResty cannot reach `crab-admin` | `docker compose --profile admin up -d`; `curl -sf http://127.0.0.1:18001/`; `proxy_pass` must be `127.0.0.1:18001` (not `8080` / wrong port). OpenResty `error.log`: `connect() failed (111: Connection refused)`. |
| Overview card **HTTP 503** | `GET /api/admin/overview` failed building metrics | Gateway `:9090/metrics` and `CRABCACHE_GATEWAY_METRICS_URL`; `docker compose ps` gateway health. |
| Overview card **HTTP 502** (API, not HTML) | Rare; upstream proxy reset while admin was blocked | Admin logs; large `trace.jsonl` (tail-read capped at 32MB); reduce concurrent refreshes. |
| Intermittent failures on refresh | Metrics scrape timeout or gateway `:9090` blocked under load | Overview polls `/api/admin/overview` only; admin caches metrics for 2s (`CRABCACHE_GATEWAY_METRICS_CACHE_TTL_SECS`) and serves stale up to 30s (`CRABCACHE_GATEWAY_METRICS_STALE_SECS`) on transient errors. Confirm `CRABCACHE_GATEWAY_METRICS_URL` (Docker: `http://gateway:9090/metrics`). |

Quick checks:

```bash
curl -sf http://127.0.0.1:18001/
curl -sf -H "x-admin-key: $CRABCACHE_ADMIN_KEY" http://127.0.0.1:18001/api/admin/overview | head -c 200
curl -sf http://127.0.0.1:9090/metrics | head
```

## Limitations

- Admin metrics history is **in-memory**; restarting `crab-admin` clears time series (counters on gateway are unchanged).
- Low-traffic deployments may show unstable 5m window rates until enough samples exist.
- Overview `trace_summary` is cached for 60s on the admin server (up to 1 minute lag vs Trace page).

## Overview acceptance checklist

1. Start `crab-gateway` and `crab-admin`; after 1–2 minutes, time series bars are non-empty and 5m hit rate is not “—”.
2. L3 **by model** table shows rows when traffic exists; `cost_saved_usd_total` matches `gateway_cache_cost_saved_usd_total` on `:9090/metrics`.
3. With `semantic.enabled = false` in gateway config, Overview shows the disabled badge; Trace banner `cache_hit_ratio` matches Trace page 24h value.
4. Under load, Ops row TTFT / coalescing / rejected **5m** values change within the 5m window.
5. After restarting `crab-admin`, `HistoryMetaHint` indicates the metrics ring was reset.

**Admin 指标时序环**为进程内存，重启 `crab-admin` 会清空 Dashboard 历史曲线；Prometheus 网关计数器不受影响。Key 配额等扩展字段见 `data/admin-state.json`（[PERSISTENCE.md](./PERSISTENCE.md)）。
