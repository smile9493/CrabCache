# CrabCache 可观测性

三层架构：**Prometheus（实时）**、**Admin Dashboard（运维）**、**影子追踪日志（离线调优）**。

## 指标定义

| 指标 / UI 字段 | 来源 | 含义 |
|---------------|------|------|
| `hit_rate_5m`（Dashboard） | Admin 指标环，5 分钟增量 | 网关 L0–L2 请求命中率（L2 路径先 `get_defer_miss` 再记 miss，避免 L2 命中双计；Coalesce Follower 回放计 hit） |
| `token_hit_rate_5m` | `gateway_deepseek_input_tokens_total` 增量 | 按 Token 加权的输入命中率（成本视角） |
| `hit_rate_cumulative` | Counter / 运行时间 | 自网关进程启动以来的累计值 |
| `prefix_cache_hit_ratio`（L3） | `gateway_upstream_prompt_cache_tokens_total` | DeepSeek 上游前缀缓存 |
| `cache_hit_ratio`（Trace 页面） | `trace.jsonl` 近 N 小时 | 影子日志实测命中率 |
| `semantic_hits/rejected/skipped` | `gateway_semantic_cache_requests_total` | 语义守卫状态（非 L2 层级命中） |
| `coalesced_total` / `coalesced_5m` | `gateway_coalesced_requests_total` | 合并的并发重复键数（5m 来自环增量） |
| `client_key_inflight` | `gateway_client_key_inflight{key_id,consumer}` | 客户端 Key 当前 in-flight 请求数 |
| `cost_saved_usd_total` / `cost_saved_usd_5m` | `gateway_cache_cost_saved_usd_total` | 网关估算的节省美元金额 |
| `rejected_total` / `rejected_5m` | `gateway_rejected_requests_total` | 被拒绝的请求数 |
| `ttft_ms` | `gateway_stream_first_token_latency_seconds` | 平均首字延迟（直方图） |
| `tier_deltas_5m` | 按层级 `gateway_cache_requests_total` | 5 分钟窗口内的 L0/L1/L2/未命中请求计数 |
| `trace_summary.cache_hit_ratio` | `trace.jsonl`（24h，缓存 60s） | Trace 对比横幅的影子日志命中率 |
| `suggestions[]` | `build_overview` 中的规则引擎 | 命中率和时序卡片下的可操作提示 |

## Admin Dashboard

- 概览每 **10 秒**轮询 **`GET /api/admin/overview/core`**（轻量指标，无时序数组）；时序与 24h Trace 分别由 **`GET /api/admin/overview/timeseries?window=1h|24h|7d`** 与 **`GET /api/admin/overview/trace`** 加载（每 **60 秒**自动刷新，与采样间隔和 Trace 缓存 TTL 对齐）。完整包 **`GET /api/admin/overview`** 保留向后兼容。`GET /api/admin/metrics` 保留向后兼容。
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

- 时序桶来自一个 **60 秒的指标采样器**（`CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS`，默认 60）。空图表表示"采集中"——启动后等待 1–2 分钟。时序窗口：**1h** 使用 **5 分钟**桶（最多 12 个，截断到最近 1 小时）；**24h** 使用 **1 小时**桶；**7d** 使用 **1 天**桶。
- 当 5 分钟窗口内请求数少于 5 时，`metrics_sample_insufficient` 为 true；UI 对窗口速率显示"—"。
- UI 图例区分 **L0–L2**（网关完整响应）和 **L3**（上游前缀 Token）。

环境变量：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CRABCACHE_GATEWAY_METRICS_URL` | `http://127.0.0.1:9090/metrics` | Prometheus 抓取目标 |
| `CRABCACHE_GATEWAY_METRICS_CACHE_TTL_SECS` | `2` | Admin 在此窗口内去重抓取（概览每 10s 轮询） |
| `CRABCACHE_GATEWAY_METRICS_STALE_SECS` | `30` | 抓取失败时，在此时长内返回上次成功数据而非 HTTP 503 |
| `CRABCACHE_METRICS_SAMPLE_INTERVAL_SECS` | `60` | 历史环采样间隔 |
| `CRABCACHE_TRACE_LOG_PATH` | `/app/logs/trace.jsonl` | Trace/日志页面的影子日志 |
| `CRABCACHE_LIVE_TRACE_CACHE_TTL_SECS` | `3` | Live 监控页 trace 缓存 TTL（秒）；增量 tail + 文件轮转检测 |
| `CRABCACHE_UPSTREAM_RECONCILE_INTERVAL_SECS` | `30` | `GET /upstream/config` 网关协调的最小间隔 |
| `CRABCACHE_GATEWAY_PROBE_TTL_SECS` | `3` | 概览中 `/v1/ready` + `/v1/status` 捆绑探针的缓存 TTL |
| `CRABCACHE_OVERVIEW_CORE_CACHE_TTL_SECS` | `10` | Overview Core 服务端缓存 TTL（秒）；客户端 10s 轮询与此对齐 |
| `CRABCACHE_OVERVIEW_CORE_REFRESH_SECS` | 与 `_CACHE_TTL` 相同 | Overview Core 后台预热间隔（秒）；SSE 广播触发源 |
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

### 客户端 Key 并发（in-flight）

每个 `sk-cc-*` 客户端 Key 可配置 `max_concurrent`（0 = 不限制）。超限时网关返回 **429**，`code: client_concurrency_exceeded`，`gateway_rejected_requests_total{reason="client_concurrency_exceeded"}` 递增。

**多副本部署：** 并发计数与 `max_concurrent`  enforcement 均在**单个网关进程内存**中完成，不跨实例共享。运行 N 个网关副本时，集群级有效并发上限约为 `N × max_concurrent`；Management API / Dashboard 返回的 `inflight` 也是**该实例**上的实时值。Prometheus 按实例抓取 `gateway_client_key_inflight` 后可在集群层求和观测。若需集群级硬限流，需另行引入 Redis 等共享计数（当前未实现）。

| 能力 | 说明 |
|------|------|
| Prometheus | `gateway_client_key_inflight{key_id, consumer}` — 当前 in-flight 数 |
| Management API | `GET /v1/keys` 返回 `inflight` + `max_concurrent`；`PATCH` 可热更新上限 |
| Dashboard Keys | **并发** 列显示 `inflight / max`（max=0 显示 ∞），每 5s 刷新 |

PromQL 按 consumer 聚合：

```promql
sum by (consumer) (gateway_client_key_inflight)
```

### DeepSeek per-`user_id` 并发软限（可选）

在 `gateway.toml` 启用 `[upstream.deepseek_user_concurrency]` 后，带 `project_id` 的 DeepSeek v4 请求受进程内 in-flight 限制（`v4_pro_per_user_id` / `v4_flash_per_user_id`）。超限时网关返回 **429**，`code: deepseek_user_concurrency_exceeded`（非上游 429，也不会触发同账号 Key 轮换）。

| 指标 | 说明 |
|------|------|
| `gateway_deepseek_user_id_concurrency_rejected_total{tier}` | 按 pro/flash 分桶的网关拒绝次数 |
| `gateway_deepseek_user_id_inflight{tier}` | 各 tier 聚合 in-flight（低基数，不含 project_id） |
| `gateway_rejected_requests_total{reason="deepseek_user_concurrency_exceeded"}` | 与其它拒绝原因统一计数 |

单 Key 精确查询（`key_id` 为 Management API 返回的 `id` 字段）：

```promql
gateway_client_key_inflight{key_id="..."}
```

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

### DeepSeek `user_id` 隔离审计字段

无需开启 `max_payload_bytes` 即可验收 `project_id` → 上游 `user_id` 注入：

| 字段 | 说明 |
|------|------|
| `project_id` | 网关解析的租户 ID（来自 `sk-cc-*` 绑定或匹配的 `X-Project-Id`） |
| `client_body_user_id` | 客户端原始 body 中的 `user_id`（若有） |
| `upstream_user_id` | **实际上游**请求体中的 `user_id`（权威） |
| `user_id_audit` | `injected` / `absent` / `stripped_client` / `mismatch` / `not_applicable` |
| `upstream_profile_id` / `pipeline` / `upstream_model` | 路由上下文 |

Admin Dashboard **Trace 分析**（`GET /api/admin/trace/analysis`）返回 `deepseek_user_id` 汇总：注入率、缺失 `project_id` 计数、`top_project_ids` 等。`isolation_ok=true` 表示近期 DeepSeek 请求基本均已正确注入。

当 `crab-composition` crate 启用时，每条日志条目还包含 `composition` 字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `composition` | `Option<RequestComposition>` | 请求组成结构的脱敏指纹（不含原始消息内容，仅含哈希与计数） |

Docker：网关写入 `gateway_logs` 卷；admin 以只读方式挂载。

## 请求组成分析（Request Composition）

`crab-composition` crate 从请求的 OpenAI-compatible JSON payload 中提取结构化指纹，存储在 `SanitizedLogEntry.composition` 字段中。**绝不存储原始消息内容**——仅存储哈希值与计数，适用于隐私合规的离线分析。

### RequestComposition 结构

```rust
pub struct RequestComposition {
    // ── 身份维度（来自 GatewayContext） ──
    consumer: String,           // API Key name / consumer 标签
    domain: String,             // 业务域（来自 API Key domain 或 "default"）
    project_id: Option<String>, // 多租户项目 ID（X-Project-Id）
    pipeline: String,           // 请求管线：cursor_deepseek_v4 / deepseek_light / generic_relay / mimo_relay / mimo_token_plan_relay / mimo_payg_relay
    user_agent: Option<String>, // User-Agent（超 128 字符时截断）

    // ── 模型 ──
    client_model: String,       // 客户端请求中的 model 字段
    upstream_model: Option<String>,  // 网关解析后的上游模型名

    // ── 系统前缀块 ──
    system_prefix_hash: Option<String>,  // 首个连续 system 消息 + tools JSON 的 SHA256
    system_message_count: u32,           // 连续 system 消息数量
    system_chars: u32,                   // system 消息内容总字符数

    // ── 工具 ──
    tool_count: u32,                    // tools 数组长度
    tool_names_hash: Option<String>,    // 排序后工具名称的 SHA256（无工具时为空）
    has_tools: bool,                    // 是否存在工具定义

    // ── 对话历史 ──
    message_count: u32,                 // messages 数组总长度
    roles: RoleCounts,                  // 各角色消息数 { system, user, assistant, tool }
    tool_turn_count: u32,               // role == "tool" 的轮次计数
    assistant_with_tool_calls_count: u32,  // 包含 tool_calls 的 assistant 消息数

    // ── Cursor Agent 组件 ──
    components: CursorComponents,       // 系统消息中检测到的 Cursor 构造
}

pub struct CursorComponents {
    rules:    ComponentFingerprint,  // workspace rules / .cursor/rules / always_applied_workspace_rules
    skills:   ComponentFingerprint,  // available_skills / SKILL.md / agent_skill
    mcp:      ComponentFingerprint,  // mcpServers / CallMcpTool / mcp_file_system
    subagent: ComponentFingerprint,  // subagent_type / Task tool / Launch.*agent
}

pub struct ComponentFingerprint {
    present: bool,                   // 是否检测到该组件
    fingerprint: Option<String>,     // 检测到的内容段的 SHA256 前缀指纹
}

pub struct RoleCounts {
    system: u32,
    user: u32,
    assistant: u32,
    tool: u32,
}
```

### Admin 组成分析 API

两个端点位于 `/api/admin/composition/*`，均需 `x-admin-key` 认证：

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/admin/composition/summary?hours=24&project_id=&consumer=` | 聚合组成统计——模型分布、工具直方图、组件检测率、租户/消费者分布 |
| GET | `/api/admin/composition/trends` | 过去 24 小时逐小时请求量趋势 |

**`GET /api/admin/composition/summary`** 查询参数：

| 参数 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `hours` | u32 | 24 | 回溯小时数 |
| `project_id` | string | — | 按项目 ID 筛选（可选） |
| `consumer` | string | — | 按消费者名称筛选（可选） |

响应：

```json
{
    "total_entries_in_window": 1847,
    "summary": {
        "total_entries": 1847,
        "tenant_count": 3,
        "consumer_count": 12,
        "model_distribution": [
            { "name": "deepseek-v4-pro", "count": 1200 },
            { "name": "deepseek-chat", "count": 400 }
        ],
        "project_distribution": [
            { "name": "project-alpha", "count": 900 }
        ],
        "consumer_distribution": [
            { "name": "cursor-user", "count": 800 }
        ],
        "tool_count_histogram": [
            { "bucket_label": "0", "count": 1400 },
            { "bucket_label": "1", "count": 200 },
            { "bucket_label": "2-5", "count": 150 },
            { "bucket_label": "6-10", "count": 50 },
            { "bucket_label": "11-20", "count": 30 },
            { "bucket_label": "20+", "count": 17 }
        ],
        "message_count_histogram": [
            { "bucket_label": "0-10", "count": 800 },
            { "bucket_label": "11-50", "count": 600 },
            { "bucket_label": "51-100", "count": 300 },
            { "bucket_label": "100+", "count": 147 }
        ],
        "component_rates": [
            { "component": "rules", "present_count": 500, "rate": 0.27 },
            { "component": "skills", "present_count": 200, "rate": 0.11 },
            { "component": "mcp", "present_count": 700, "rate": 0.38 },
            { "component": "subagent", "present_count": 100, "rate": 0.05 }
        ],
        "avg_latency_ms": 320.5,
        "avg_total_tokens": 4500
    }
}
```

**`GET /api/admin/composition/trends`** 响应：

```json
{
    "hours": 24,
    "points": [
        { "timestamp_ms": 1716480000000, "request_count": 85 },
        { "timestamp_ms": 1716483600000, "request_count": 120 }
    ]
}
```

### Dashboard 组成页面

在 **Monitor** 组新增 `/composition` 路由。页面功能：

| 组件 | 说明 |
|------|------|
| **汇总卡片** | 总条目数、平均延迟、平均 Token、租户数、消费者数 |
| **模型分布** | 垂直柱状图，展示 Top 10 模型请求量 |
| **工具直方图** | 按桶展示工具数量分布（0、1、2-5、6-10、11-20、20+） |
| **组件检测率** | rules / skills / mcp / subagent 的检测率条形图 |
| **消息直方图** | 按桶展示消息数量分布（0-10、11-50、51-100、100+） |
| **项目分布** | 按项目 ID 的请求量排名 |
| **消费者分布** | 按消费者的请求量排名 |
| **趋势图** | 过去 24 小时逐小时请求量折线图 |
| **时间窗口选择** | 1h / 6h / 24h / 72h / 168h |

页面每 **10 秒**轮询 summary 和 trends，浏览器标签页隐藏时自动暂停。

## 组成调试日志（Composition Debug Logging）

通过 `[trace_logging.composition_debug]` 配置项启用。启用后，网关会将**完整（未哈希的）system 消息文本和 tools 定义**写入独立的 JSONL 日志文件，用于离线组成分析。

**⚠️ 安全警告：此文件包含原始提示内容，不应在生产中长期启用，或应配置严格的日志轮转。**

### 配置

```toml
[trace_logging.composition_debug]
enabled = false
path = "/var/log/crabcache/trace-debug.jsonl"
max_lines = 5000
max_files = 3
```

环境变量：
- `CRABCACHE_COMPOSITION_DEBUG_PATH`：覆盖调试日志路径（默认从 trace 路径自动派生）

### CompositionDebugEntry 结构

| 字段 | 类型 | 说明 |
|------|------|------|
| `timestamp_ms` | u64 | Unix 毫秒时间戳 |
| `request_hash` | String | 与 trace 日志 `request_hash` 关联 |
| `consumer` | String | API Key name / consumer 标签 |
| `domain` | String | 业务域 |
| `project_id` | Option<String> | 多租户项目 ID |
| `model` | String | 请求模型 |
| `system_text` | Option<String> | 完整系统消息文本（最大 100K 字符，超长截断） |
| `tools_json` | Option<String> | 完整工具定义 JSON（最大 100K 字符，超长截断） |

### Admin 调试 API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/admin/composition/debug?hours=24&limit=100&request_hash=&consumer=&project_id=` | 列出调试条目，支持按哈希/消费者/项目筛选 |

### Dashboard 组成调试日志视图

组成页面底部显示"Composition Debug Log"区域，支持：
- 按 `request_hash` 搜索
- 按 `consumer` 筛选
- 点击条目打开侧边面板查看完整 system 文本和 tools 定义
- 显示 system/tools 存在标记（蓝色/绿色徽标）

### Prometheus 组成指标

| 指标 | 类型 | 标签 | 描述 |
|------|------|------|------|
| `gateway_composition_requests_total` | Counter | `project_id`, `pipeline`, `has_tools`, `msg_bucket` | 按项目、管线、工具状态和消息桶计数的请求组成统计 |
| `gateway_composition_component_total` | Counter | `component`, `present` | Cursor 组件检测计数（rules / skills / mcp / subagent） |
| `gateway_composition_tool_count` | Histogram | — | 每次请求的工具数量分布（桶：0, 1, 5, 10, 20, 50） |

## 原始请求捕获（Raw Capture）

独立于影子日志和组成分析的**抓包级**通道，记录完整 client/upstream JSON body 供离线结构对比分析。

### 配置

```toml
[raw_capture]
enabled = false
dir = "/var/log/crabcache/raw_capture"
max_index_lines = 5000
max_body_files = 5000
max_client_bytes = 0    # 0 = 仅受网关 64 MiB 限制
max_upstream_bytes = 0
mask_api_keys = false   # true = 脱敏 sk-* token
skip_paths = ["/health", "/healthz", "/ready"]
```

环境变量覆盖：`CRABCACHE_RAW_CAPTURE_DIR`。

### 存储布局

```
raw_capture/
  index.jsonl          # 每行一个 RawCaptureEntry（元数据 + 结构摘要 + body 文件路径）
  bodies/
    {request_id}.client.json
    {request_id}.upstream.json  # 仅当 upstream body != client body 时写入
```

- **index.jsonl** 自动轮转（`max_index_lines`），保留结构摘要用于列表和统计
- **bodies/** 目录超过 `max_body_files` 时自动删除最旧文件
- 不做额外截断（`max_client_bytes = 0`），受网关 `limits.max_request_body_bytes` 约束

### 安全警告

**Raw capture 默认关闭**，且不脱敏。启用前请注意：

1. **API Key 泄漏**：body 中包含 Bearer Token。`mask_api_keys = true` 可部分脱敏
2. **Prompt 内容**：完整对话历史包含敏感业务数据
3. **磁盘占用**：每个请求写入 ~2x body 大小。64 MiB body = ~128 MiB 磁盘
4. **部署建议**：仅在受控内网环境启用；目录权限设为 `chmod 600`；不暴露公网

### Admin API

| 方法 | 路径 | 行为 |
|------|------|------|
| GET | `/api/admin/capture/list?hours=&limit=&consumer=&project_id=&request_hash=&session_fingerprint=&backend_name=&client_key_fingerprint=&affinity_kind=` | 读 index.jsonl，返回摘要列表（含会话指纹、亲和键、后端、耗时等） |
| GET | `/api/admin/capture/{request_id}` | 读 index + 加载 bodies/*.json，返回 client/upstream 全文 + 结构 diff |
| GET | `/api/admin/capture/stats?hours=` | 聚合：平均 delta、reasoning 注入率、message_count P99、thinking 标记率 |

Dashboard **请求页 → 包捕获 Tab** 提供图形化访问。

每条 `index.jsonl` 记录（v2 字段，旧行缺失则为空）额外包含：

| 字段 | 用途 |
|------|------|
| `session_fingerprint` | 首条 `user` 消息哈希，**区分同 key 下不同 Cursor 会话** |
| `client_key_fingerprint` | 客户端 Bearer 密钥哈希，**区分不同 API key** |
| `affinity_key` / `affinity_kind` | Ketama 亲和键（`conv`/`pck`/`user`/`ip`） |
| `backend_name` / `upstream_host` | **负载均衡**落到的上游节点 |
| `duration_ms` / `ttft_ms` / `upstream_latency_ms` | **串行耗时**与流式首字 |
| `cache_hit` / `cache_tier` / `coalesce_leader` / `coalesced_follower` | 缓存与请求合并（并行去重） |

筛选示例：`session_fingerprint=97d0e3a8e50d` 只看同一会话；`backend_name=deepseek-1` 看 LB 分布。

### 思考模式上下文膨胀排查

1. 打开 Dashboard → Requests → Capture Tab
2. 查看 **Delta** 列：正数表示 upstream body 比 client 大
3. 查看 **Reasoning** 列：`Yes` 表示 upstream 获得了 reasoning_content
4. 点击行打开详情：
   - **Structure Table**：逐项对比 messages 字符数、reasoning 字符数、tool_calls 数
   - **Client/Upstream JSON**：左右分栏对比完整 body
5. 典型膨胀模式：
   - `delta_reasoning_chars` 远大于 0 → 思考链被注入到 assistant message
   - `delta_message_count` > 0 → upstream 多出 recovery 消息
   - `thinking_markup = Yes` → 内容包含 `<thinking>` 标签

### Docker 卷挂载

```yaml
# docker-compose.yml
services:
  admin:
    volumes:
      - gateway-logs:/var/log/crabcache:ro
volumes:
  gateway-logs:
```

Admin 以只读方式访问 raw_capture 目录（与 trace.jsonl 同卷）。

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

## 基础设施监控

Admin Dashboard 的「基础设施」页面显示与网关同 `docker-compose` 项目的容器资源占用与宿主机磁盘使用情况。

### 采集机制

- Admin 容器通过 **只读 Docker Engine API** (`/var/run/docker.sock`) 采集所有同项目容器的 CPU、内存与网络速率。
- 容器过滤依据 Compose 自动标签 `com.docker.compose.project=<project>`；项目名受环境变量 `CRABCACHE_COMPOSE_PROJECT`（或 `COMPOSE_PROJECT_NAME`）控制，默认 `crabcache`。
- **单一后台采集协程**周期性调用 Docker API，写入内存快照缓存；HTTP `GET /snapshot` 只读缓存，不在请求路径上采集。
- 采集间隔默认 `max(CRABCACHE_INFRA_CACHE_TTL_SECS, 5)`（可用 `CRABCACHE_INFRA_COLLECT_INTERVAL_SECS` 覆盖）；历史环采样间隔默认 60s（`CRABCACHE_INFRA_SAMPLE_INTERVAL_SECS`）。
- 前端每 10 秒轮询 `/api/admin/infra/snapshot`。

### 数据类型

| 字段 | 来源 | 含义 |
|------|------|------|
| `cpu_percent` | Docker stats CPU delta / system delta | 按 CPU 核数归一化的百分比（首次采样为 `null`） |
| `mem_usage_bytes` |`memory_stats.usage` | 当前 RSS 内存 |
| `mem_limit_bytes` | `memory_stats.limit` | 容器内存上限（无限制时为宿主机内存） |
| `net_rx_bps` / `net_tx_bps` | 网络接口累计值的差分速率 | 字节/秒（首次采样为 `null`） |
| `host_disks[].usage_percent` | `statvfs(/)` | 宿主机根分区使用率；若 `/var/lib/docker` 为独立挂载点则额外展示 |

### API

| 方法 | 路径 | 行为 |
|------|------|------|
| `GET` | `/api/admin/infra/snapshot` | 容器表 + 宿主机磁盘 + `collected_at` Unix 秒 |
| `GET` | `/api/admin/infra/status` | 轻量：`docker_connected`、`compose_project`、`history_sample_count`、`last_collected_at` |
| `GET` | `/api/admin/infra/timeseries` | 历史曲线（`window=1h\|24h`，`container_id` 可选） |
| `POST` | `/api/admin/infra/speed-test` | 异步带宽测试（`direction`: download/upload/both） |
| `POST` | `/api/admin/infra/speed-test/upload` | 上传探针（`job_id` + `token`，无 Admin Key；有任务 TTL 与次数限制） |

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CRABCACHE_COMPOSE_PROJECT` | `crabcache` | 用于过滤容器的 Docker Compose 项目名 |
| `CRABCACHE_DOCKER_HOST` | `unix:///var/run/docker.sock` | Docker Engine API 地址 |
| `CRABCACHE_DOCKER_ALLOW_TCP` | （未设置） | 设为 `1` 时 Unix 失败才回退 `127.0.0.1:2375` |
| `CRABCACHE_INFRA_CACHE_TTL_SECS` | `5` | 采集间隔下限参考（秒） |
| `CRABCACHE_INFRA_COLLECT_INTERVAL_SECS` | `max(TTL,5)` | Docker 采集周期（秒） |
| `CRABCACHE_INFRA_SAMPLE_INTERVAL_SECS` | `60` | 写入历史环的采样周期（秒） |
| `CRABCACHE_SPEED_TEST_*` | 见 `.env.example` | 测速 URL 白名单、字节数、任务 TTL、上传上限等 |

### 部署要求

- Admin 容器必须挂载 `docker.sock`（只读）：
  ```yaml
  volumes:
    - /var/run/docker.sock:/var/run/docker.sock:ro
  ```
- **安全**：`docker.sock` 等价宿主机 root 权限。仅 Admin 容器使用只读挂载，且 Admin API 已由 `X-Admin-Key` 保护。将 Admin 暴露到公网时必须使用强密钥。
- **裸机开发**：无 Docker Socket 时页面显示「Docker 不可用」提示，不影响其他 Admin 功能。

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
