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

- Time series buckets come from a **60s metrics sampler** (`CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS`, default 60). Empty charts mean “collecting” — wait 1–2 minutes after startup.
- `metrics_sample_insufficient` is true when the 5m window has fewer than 5 requests; UI shows “—” for window rates.
- UI legend separates **L0–L2** (gateway full response) from **L3** (upstream prefix tokens).

Environment:

| Variable | Default | Description |
|----------|---------|-------------|
| `CRABCACHE_GATEWAY_METRICS_URL` | `http://127.0.0.1:9090/metrics` | Prometheus scrape target |
| `CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS` | `60` | History ring sample interval |
| `CRABCACHE_TRACE_LOG_PATH` | `/app/logs/trace.jsonl` | Shadow log for Trace/Logs pages |

Trace analysis: `GET /api/admin/trace/analysis?hours=24` (default 24; `hours=0` = full file).

## Shadow log

Configured in `gateway.toml`:

```toml
[trace_logging]
enabled = true
path = "/app/logs/trace.jsonl"
max_lines = 10000
max_files = 5
```

Each line is a `SanitizedLogEntry` (no raw body). Includes `consumer` (API key name), `cache_tier`, `prompt_cache_hit_ratio`.

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
