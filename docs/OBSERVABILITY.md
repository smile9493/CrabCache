# CrabCache 可观测性

三层架构：**Prometheus（实时）**、**Admin Dashboard（运维）**、**影子追踪日志（离线调优）**。

## 指标定义

| 指标 / UI 字段 | 来源 | 含义 |
|---------------|------|------|
| `hit_rate_5m`（Dashboard） | Admin 指标环，5 分钟增量 | 网关 L0–L2 请求命中率 |
| `token_hit_rate_5m` | `gateway_deepseek_input_tokens_total` 增量 | 按 Token 加权的输入命中率（成本视角） |
| `hit_rate_cumulative` | Counter / 运行时间 | 自网关进程启动以来的累计值 |
| `prefix_cache_hit_ratio`（L3） | `gateway_upstream_prompt_cache_tokens_total` | DeepSeek 上游前缀缓存 |
| `cache_hit_ratio`（Trace 页面） | `trace.jsonl` 近 N 小时 | 影子日志实测命中率 |
| `semantic_hits/rejected/skipped` | `gateway_semantic_cache_requests_total` | 语义守卫状态（非 L2 层级命中） |
| `coalesced_total` / `coalesced_5m` | `gateway_coalesced_requests_total` | 合并的并发重复键数（5m 来自环增量） |
| `cost_saved_usd_total` / `cost_saved_usd_5m` | `gateway_cache_cost_saved_usd_total` | 网关估算的节省美元金额 |
| `rejected_total` / `rejected_5m` | `gateway_rejected_requests_total` | 被拒绝的请求数 |
| `ttft_ms` | `gateway_stream_first_token_latency_seconds` | 平均首字延迟（直方图） |
| `tier_deltas_5m` | 按层级 `gateway_cache_requests_total` | 5 分钟窗口内的 L0/L1/L2/未命中请求计数 |
| `trace_summary.cache_hit_ratio` | `trace.jsonl`（24h，缓存 60s） | Trace 对比横幅的影子日志命中率 |
| `suggestions[]` | `build_overview` 中的规则引擎 | 命中率和时序卡片下的可操作提示 |

## Admin Dashboard

- 概览每 **5 秒**轮询 **`GET /api/admin/overview`**（单次请求包）。`GET /api/admin/metrics` 保留向后兼容。
- 概览包字段：

| 字段 | 说明 |
|------|------|
| `metrics` | 扩展的 `MetricsSnapshot`（5m 速率、层级增量、历史元信息、时序数据） |
| `health` | 网关可达性、流缓存、上游 Key 池计数 |
| `prefix_cache` | L3 全局 + `by_model` 表格 |
| `semantic` | `enabled` + `similarity_threshold` |
| `trace_summary` | 24h 影子日志摘要（60s 服务端缓存） |
| `ops` | 节省成本、合并/拒绝 5m、TTFT、prefix_break、reasoning 存储、SSE 省略 |
| `suggestions` | 基于规则的运维提示（`severity`、`target`、`message`），供 Overview 卡片使用 |

- 时序桶来自一个 **60 秒的指标采样器**（`CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS`，默认 60）。空图表表示"采集中"——启动后等待 1–2 分钟。
- 当 5 分钟窗口内请求数少于 5 时，`metrics_sample_insufficient` 为 true；UI 对窗口速率显示"—"。
- UI 图例区分 **L0–L2**（网关完整响应）和 **L3**（上游前缀 Token）。

环境变量：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CRABCACHE_GATEWAY_METRICS_URL` | `http://127.0.0.1:9090/metrics` | Prometheus 抓取目标 |
| `CRABCACHE_GATEWAY_METRICS_CACHE_TTL_SECS` | `2` | Admin 在此窗口内去重抓取（概览每 5s 轮询） |
| `CRABCACHE_GATEWAY_METRICS_STALE_SECS` | `30` | 抓取失败时，在此时长内返回上次成功数据而非 HTTP 503 |
| `CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS` | `60` | 历史环采样间隔 |
| `CRABCACHE_TRACE_LOG_PATH` | `/app/logs/trace.jsonl` | Trace/日志页面的影子日志 |
| `CRABCACHE_LIVE_TRACE_CACHE_TTL_SECS` | `3` | Live 监控页 trace 缓存 TTL（秒）；增量 tail + 文件轮转检测 |
| `CRABCACHE_UPSTREAM_RECONCILE_INTERVAL_SECS` | `30` | `GET /upstream/config` 网关协调的最小间隔 |
| `CRABCACHE_GATEWAY_PROBE_TTL_SECS` | `3` | 概览中 `/v1/ready` + `/v1/status` 捆绑探针的缓存 TTL |
| `CRABCACHE_ADMIN_METRICS_DB_PATH` | `data/metrics.sqlite` | Admin 指标采样 SQLite 数据库路径 |
| `CRABCACHE_METRICS_DB_RETENTION_SECS` | `2592000`（30 天） | 指标采样保留时长 |
| `CRABCACHE_KEY_USAGE_SYNC_INTERVAL_SECS` | `60` | Key 月度用量同步周期（秒）；设为 0 禁用 |

Trace 分析：`GET /api/admin/trace/analysis?hours=24`（默认 24；`hours=0` = 全文）。

### 实时客户端监控（`/live`）

Dashboard **实时监控 / Live** 页面每 **2 秒**轮询 **`GET /api/admin/live-metrics`**（按选择的 Consumer）。

| 查询参数 | 默认值 | 说明 |
|---------|--------|------|
| `consumer` |（必填）| API Key `name` / trace `consumer`；另可通过 `GET /api/admin/live-metrics/consumers?window_secs=300` 获取可选 consumer 列表 |
| `window_secs` | `300` | 最近 5 分钟（限制 60–900） |
| `bucket_secs` | `5` | 聚合桶宽度 |

响应：`buckets[]`（每个桶的平均 e2e / 上游 / TTFT 延迟、Token 总和，`upstream_sample_count` / `ttft_sample_count` 为桶中实际样本数）、`summary`（加权平均，上游/TTFT 按实际样本数而非桶数加权）、可选的 `latest` 点、`available_consumers`。

| UI 时序 | Trace 字段 | 备注 |
|---------|-----------|------|
| E2E 延迟 | `latency_ms` | 客户端 → 网关端到端时间（含完整流式响应） |
| 上游延迟 | `upstream_latency_ms` | 网关 → 上游 body EOS 总时长；**仅未命中**且仅含上游样本的加权平 |
| TTFT | `ttft_ms` | 流式首字延迟（加权平均） |
| Token | `input_tokens` / `output_tokens` | 来自上游 `usage`；缓存命中使用缓存条目的用量 |

需要 `[trace_logging] enabled = true` 且 API Key 具有 **name**（consumer 标签）。Admin 使用**增量 tail 读取** `trace.jsonl`，支持文件轮转检测（inode/mtime），避免全文件重解析。

**性能：** `LiveTraceCache` 在 `crab-admin` 中缓存 **3 秒**（可通过 `CRABCACHE_LIVE_TRACE_CACHE_TTL_SECS` 配置，默认 3），文件追加时仅读取新字节（增量 tail）。文件轮转时自动全量重建。Dashboard 每 **2 秒**（5m 窗口）或 **3 秒**（15m 窗口）轮询，浏览器标签页隐藏时暂停。

**响应字段：** `available_consumers`（最多 50 个来自 trace 的名称，Dashboard 优先从此端点获取以移除 keys 硬依赖）、`buckets[].upstream_latency_ms` / `ttft_ms` 在桶中没有上游/TTFT 样本时（缓存命中）为 `null`，桶中还包含 `upstream_sample_count` / `ttft_sample_count` 以支持加权聚合。

## 影子日志

在 `gateway.toml` 中配置：

```toml
[trace_logging]
enabled = true
path = "/app/logs/trace.jsonl"
max_lines = 10000
max_files = 5
```

每行是一个 `SanitizedLogEntry`（不含原始 body）。包含 `consumer`（API Key name）、`cache_tier`、`prompt_cache_hit_ratio`，以及（当前网关构建版本）`upstream_latency_ms`、`ttft_ms`、`input_tokens`、`output_tokens`。

Docker：网关写入 `gateway_logs` 卷；admin 以只读方式挂载。

## Prometheus / Grafana（可选）

```bash
docker compose -f docker-compose.yml -f docker-compose.observability.yml --profile observability up -d
```

- Prometheus：http://127.0.0.1:9091
- Grafana：http://127.0.0.1:3001（添加 Prometheus 数据源 `http://prometheus:9090`）

示例 PromQL（匹配 Dashboard 5 分钟命中率）：

```promql
sum(rate(gateway_cache_requests_total{result="hit"}[5m]))
/ sum(rate(gateway_cache_requests_total[5m]))
```

按 Token 加权：

```promql
sum(rate(gateway_deepseek_input_tokens_total{cache_status="hit"}[5m]))
/ sum(rate(gateway_deepseek_input_tokens_total[5m]))
```

告警规则：`config/prometheus/alerts.yml`。

### 域名标签

API Key 可以设置 `domain`（业务线/项目）。指标使用 `domain` 标签；未设置的密钥记录为 `unclassified`。Dashboard **域名/Domains** 页面和 Overview 域名表格从 `GET /api/admin/overview` 读取 `domain_buckets`。域策略：`PUT /v1/domains/policies`（从 admin `data/admin-state.json` 同步）。

按域名查询示例 PromQL：

```promql
sum by (domain) (rate(gateway_deepseek_input_tokens_total{cache_status="hit"}[5m]))
/ sum by (domain) (rate(gateway_deepseek_input_tokens_total[5m]))
```

## 客户端验证

从网关响应头中获取信息：

- `x-cache-status`：`HIT` / `MISS`
- `x-request-id`：与网关日志关联

## Dashboard 502 / 503 故障排查

| 症状 | 可能原因 | 检查内容 |
|------|----------|---------|
| 浏览器 **502**（整个页面文档请求） | OpenResty 无法连接 `crab-admin` | `docker compose --profile admin up -d`；`curl -sf http://127.0.0.1:18001/`；`proxy_pass` 必须为 `127.0.0.1:18001`（非 `8080`/错误端口）。OpenResty `error.log`：`connect() failed (111: Connection refused)`。 |
| Overview 卡片 **HTTP 503** | `GET /api/admin/overview` 构建指标失败 | 网关 `:9090/metrics` 和 `CRABCACHE_GATEWAY_METRICS_URL`；`docker compose ps` 网关健康检查。 |
| Overview 卡片 **HTTP 502**（API，非 HTML） | 罕见；admin 被阻塞时上游代理重置 | Admin 日志；`trace.jsonl` 过大（tail 读取上限 32MB）；减少并发刷新。 |
| 刷新时间歇性失败 | 指标抓取超时或负载下网关 `:9090` 被阻塞 | Overview 仅轮询 `/api/admin/overview`；admin 缓存指标 2 秒（`CRABCACHE_GATEWAY_METRICS_CACHE_TTL_SECS`），临时错误时可提供最多 30 秒的过期数据（`CRABCACHE_GATEWAY_METRICS_STALE_SECS`）。确认 `CRABCACHE_GATEWAY_METRICS_URL`（Docker：`http://gateway:9090/metrics`）。 |

快速检查：

```bash
curl -sf http://127.0.0.1:18001/
curl -sf -H "x-admin-key: $CRABCACHE_ADMIN_KEY" http://127.0.0.1:18001/api/admin/overview | head -c 200
curl -sf http://127.0.0.1:9090/metrics | head
```

## 限制

- Admin 指标历史现由 SQLite `data/metrics.sqlite` 持久化；重启 `crab-admin` 后时序数据从数据库恢复（网关上的 Counter 不受影响）。
- 低流量部署可能显示不稳定的 5 分钟窗口速率，直到积累足够的样本。
- Overview `trace_summary` 在 Admin 服务器上缓存 60 秒（与 Trace 页面相比最多 1 分钟延迟）。

## Overview 验收清单

1. 启动 `crab-gateway` 和 `crab-admin`；1–2 分钟后，时序条非空且 5 分钟命中率不显示"—"。
2. L3 **按模型** 表格在有流量时显示行；`cost_saved_usd_total` 与 `:9090/metrics` 上的 `gateway_cache_cost_saved_usd_total` 匹配。
3. 网关配置中 `semantic.enabled = false` 时，Overview 显示已禁用徽标；Trace 横幅 `cache_hit_ratio` 与 Trace 页面 24h 值匹配。
4. 有负载时，Ops 行 TTFT/coalescing/rejected **5m** 值在 5 分钟窗口内变化。
5. 重启 `crab-admin` 后，时序曲线在 1–2 分钟内恢复；若网关同时重启，概览将显示 `HistoryMetaHint` 提示。

**Admin 指标时序环**现由 SQLite `data/metrics.sqlite` 持久化，重启 `crab-admin` 后从数据库恢复；若网关同时重启，累计值可能归零但历史曲线尚存。Key 配额等扩展字段见 `data/admin-state.json`（[PERSISTENCE.md](./PERSISTENCE.md)）。
