# 日志管理运维手册

CrabCache 日志子系统包含 **5 个独立模块**：结构化运行日志、Trace 请求追踪日志、Raw Capture 原始抓包、审计日志、以及调试日志。本文档覆盖配置、运维操作、故障排查和架构细节。

**相关文档**：[可观测性](OBSERVABILITY.md) · [持久化](PERSISTENCE.md) · [运维手册](OPS_RUNBOOK.md)

---

## 1. 子系统总览

| 子系统 | 存储格式 | 写入方 | 读取方 | 用途 |
|--------|----------|--------|--------|------|
| **运行日志** | `./logs/gateway.log.YYYY-MM-DD`（JSON，按天轮转） | `tracing-appender` | `journalctl` / 日志采集 | 网关运行状态、错误、警告 |
| **Trace 日志** | `trace.jsonl`（JSONL，按行数轮转） + PostgreSQL `trace_logs` | `TraceLogger`（mpsc 异步线程） | Admin Dashboard / API | 每请求脱敏元数据（60+ 字段） |
| **Raw Capture** | `index.jsonl` + `bodies/*.json` | `RawCaptureLogger`（mpsc 异步线程） | Admin Capture 页面 | 完整请求/响应体抓包 |
| **审计日志** | PostgreSQL `audit_log` | Admin API handler | Dashboard 审计页面 | 管理操作记录 |
| **调试日志** | NDJSON 文件（`CRABCACHE_DEBUG_LOG_PATH`） | `debug_log::debug_agent_log` | 人工分析 | 假设验证式调试 |

---

## 2. 配置参考

### 2.1 Trace 日志（`[trace_logging]`）

```toml
[trace_logging]
enabled = false                          # 启用 Trace 日志
path = "/var/log/crabcache/trace.jsonl"  # JSONL 文件路径
max_lines = 10000                        # 单文件最大行数（触发轮转）
max_files = 5                            # 保留的轮转文件数
max_payload_bytes = 0                    # 请求体快照字节数（0=禁用，建议 4096）
max_response_preview_bytes = 0           # 响应预览字节数（0=禁用，建议 2048）
pg_url = ""                             # PG 双写 URL（可选）

[trace_logging.composition_debug]
enabled = false                          # Composition 调试日志
path = "/var/log/crabcache/trace-debug.jsonl"
max_lines = 5000
max_files = 3
```

**环境变量覆盖**：

| 变量 | 作用 | 默认值 |
|------|------|--------|
| `CRABCACHE_TRACE_LOG_PATH` | Trace 日志路径 | `/app/logs/trace.jsonl` |
| `CRABCACHE_LIVE_TRACE_CACHE_TTL_SECS` | Live 缓存 TTL（秒） | `3` |
| `CRABCACHE_LIVE_TRACE_SOURCE` | 设为 `pg` 强制从 PG 读取 | — |
| `CRABCACHE_ADMIN_TRACE_PG_SYNC` | 设为 `false` 禁用 JSONL→PG 同步 | `true` |
| `CRADMIN_TRACE_RETENTION_DAYS` | PG trace_logs 保留天数覆盖 | 策略值 |
| `CRABCACHE_ADMIN_LOG_LIST_PREVIEW_CHARS` | 列表 API 响应预览截断字符数 | `200` |

### 2.2 Raw Capture（`[raw_capture]`）

```toml
[raw_capture]
enabled = false
dir = "/var/log/crabcache/raw_capture"
max_index_lines = 5000                   # index.jsonl 最大行数
max_body_files = 5000                    # bodies/ 最大文件数
max_client_bytes = 0                     # 客户端 body 最大字节（0=无限制）
max_upstream_bytes = 0                   # 上游 body 最大字节
mask_api_keys = false                    # body 中是否脱敏 sk-* 密钥
sample_rate = 1.0                        # 采样率（0.0~1.0）
sample_always_on_error = true            # 错误请求必采
sample_always_on_large_body = false      # 大请求必采
min_body_bytes_for_large = 2097152       # 大请求阈值（2 MiB）
skip_paths = ["/health", "/healthz", "/ready"]
```

### 2.3 运行日志（tracing-subscriber）

网关启动时自动初始化，无 TOML 配置项。通过环境变量控制：

| 变量 | 作用 | 默认值 |
|------|------|--------|
| `RUST_LOG` | 文件日志级别过滤 | `info` |
| `RUST_LOG_STDOUT` | 控制台日志级别过滤 | `warn` |

文件输出：`./logs/gateway.log`，按天轮转，JSON 格式，由 `tracing-appender` 管理。

### 2.4 调试日志

| 变量 | 作用 |
|------|------|
| `CRABCACHE_DEBUG_LOG_PATH` | NDJSON 调试日志文件路径（不设置=禁用） |
| `CRABCACHE_DEBUG_RUN_ID` | 调试运行标识（写入每行 JSON） |
| `CRABCACHE_DEBUG_SESSION_ID` | 调试会话标识 |

---

## 3. 数据写入链路

### 3.1 Trace 日志生命周期

```
请求完成 (ProxyHttp::logging)
  │
  ├─ SanitizedLogEntry::from_request()
  │   ├─ SHA256(body)[:16] → request_hash
  │   ├─ SHA256(body)[:8] % 100 → semantic_cluster
  │   └─ mask_api_keys(truncate(body)) → request_messages_snapshot
  │
  ├─ 填充 60+ 字段（延迟/Token/路由/审计/阶段耗时）
  │   ├─ apply_user_id_audit_to_entry()
  │   ├─ compute_phase_durations()
  │   └─ build_response_preview()
  │
  └─ TraceLogger::log(entry)
      │
      ├─ mpsc::channel → crab-trace-writer 线程
      │   └─ RotatingJsonlWriter → trace.jsonl
      │       ├─ 每 100 行 flush
      │       └─ 达 max_lines → rotate（rename + cleanup）
      │
      └─ pg_sink.try_send() → PG 批量写入
          └─ INSERT INTO trace_logs ... ON CONFLICT DO NOTHING
```

### 3.2 JSONL → PG 同步（Admin 侧）

当 Gateway 未直接写 PG（`trace_logging.pg_url` 未配置）时，Admin 后台任务 `pg_sync` 每 **10 秒**增量读取 `trace.jsonl` 并写入 PG：

- 通过 byte offset + inode 追踪读取位置
- 轮转检测：inode 变化或文件缩小 → 从头读取新文件
- 环境变量：`CRABCACHE_ADMIN_TRACE_PG_SYNC=false` 可禁用

**注意**：如果 Gateway 已配置 `trace_logging.pg_url` 直接写 PG，Admin 侧同步应禁用以避免重复插入（`ON CONFLICT DO NOTHING` 会兜底，但浪费 IO）。

### 3.3 Raw Capture 采样决策

```
should_skip(path)?  →  跳过 /health 等
should_sample(error, body_bytes)?
  ├─ AlwaysCapture: upstream 4xx/5xx 错误（sample_always_on_error=true）
  ├─ AlwaysCapture: body > min_body_bytes_for_large（sample_always_on_large_body=true）
  ├─ Capture: random < sample_rate
  └─ Skip
```

---

## 4. PG 表结构

### 4.1 trace_logs

主键：`(request_hash, timestamp_ms)`

```sql
CREATE TABLE trace_logs (
    request_hash    TEXT NOT NULL,
    timestamp_ms    BIGINT NOT NULL,
    content_length  INTEGER NOT NULL,
    semantic_cluster INTEGER NOT NULL,
    model           TEXT NOT NULL,
    prompt_tokens   INTEGER NOT NULL,
    latency_ms      DOUBLE PRECISION NOT NULL,
    cache_hit       BOOLEAN NOT NULL,
    conversation_id TEXT,
    consumer        TEXT,
    domain          TEXT,
    project_id      TEXT,
    upstream_latency_ms DOUBLE PRECISION,
    ttft_ms         DOUBLE PRECISION,
    input_tokens    BIGINT,
    output_tokens   BIGINT,
    cache_tier      TEXT,
    composition     JSONB,
    request_messages_snapshot TEXT,
    response_preview TEXT,
    retired_prefix_messages INTEGER,
    reasoning_strategy TEXT,
    prompt_cache_hit_ratio DOUBLE PRECISION,
    upstream_profile_id TEXT,
    pipeline        TEXT,
    upstream_model  TEXT,
    client_body_user_id TEXT,
    upstream_user_id TEXT,
    user_id_audit   TEXT,
    upstream_key_id TEXT,
    session_store   TEXT,
    stable_session_kind TEXT,
    upstream_outbound_bytes INTEGER,
    prefill_ms      DOUBLE PRECISION,
    pre_header_ms   DOUBLE PRECISION,
    affinity_key    TEXT,
    affinity_kind   TEXT,
    backend_name    TEXT,
    session_fingerprint TEXT,
    is_coalesced    BOOLEAN NOT NULL DEFAULT false,
    client_key_id   TEXT,
    request_passthrough BOOLEAN NOT NULL DEFAULT false,
    request_passthrough_prefix_len INTEGER,
    status_code     INTEGER,
    error_code      TEXT,
    limit_source    TEXT,
    cache_decision  TEXT,
    upstream_result TEXT,
    phase_durations_ms JSONB,
    PRIMARY KEY (request_hash, timestamp_ms)
);
```

**索引**：
- `idx_trace_ts` — `timestamp_ms DESC`
- `idx_trace_consumer_ts` — `(consumer, timestamp_ms DESC)` WHERE consumer IS NOT NULL
- `idx_trace_model_ts` — `(model, timestamp_ms DESC)`
- `idx_trace_cache_tier` — `(cache_tier, timestamp_ms DESC)` WHERE cache_tier IS NOT NULL

### 4.2 request_logs

```sql
CREATE TABLE request_logs (
    id              TEXT PRIMARY KEY,
    timestamp_ms    BIGINT NOT NULL,
    model           TEXT NOT NULL DEFAULT '',
    consumer        TEXT NOT NULL DEFAULT '',
    duration_ms     DOUBLE PRECISION NOT NULL DEFAULT 0,
    input_tokens    BIGINT NOT NULL DEFAULT 0,
    output_tokens   BIGINT NOT NULL DEFAULT 0,
    cache_status    TEXT NOT NULL DEFAULT '',
    cache_tier      TEXT NOT NULL DEFAULT '',
    status_code     INTEGER NOT NULL DEFAULT 200,
    conversation_id TEXT NOT NULL DEFAULT '',
    route_backend   TEXT NOT NULL DEFAULT '',
    request_payload JSONB,
    response_body   TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 4.3 audit_log

```sql
CREATE TABLE audit_log (
    id          BIGSERIAL PRIMARY KEY,
    timestamp   TIMESTAMPTZ DEFAULT NOW(),
    action      TEXT NOT NULL,
    actor       TEXT NOT NULL,
    target      TEXT,
    detail      JSONB,
    ip_address  TEXT
);
```

---

## 5. 后台任务

| 任务 | 间隔 | 功能 | 配置 |
|------|------|------|------|
| `log_retention_loop` | 10 分钟 | JSONL 文件清理 + PG 行修剪 + Prometheus 指标更新 | `RetentionPolicy`（Dashboard 可调） |
| `pg_sync` | 10 秒 | JSONL → PG 增量同步 | `CRABCACHE_ADMIN_TRACE_PG_SYNC` |
| `key_usage_sync` | 60 秒 | 从 trace 日志同步 Key 月度 Token 用量 | `CRABCACHE_KEY_USAGE_SYNC_INTERVAL_SECS` |
| `peak_hours_aggregator` | 5 分钟 | 模型高峰时段聚合写入 PG `model_peak_hours` | — |
| `domain_usage_sync` | 60 秒 | Gateway 域名用量 → PG 同步 | `CRABCACHE_DOMAIN_USAGE_SYNC_INTERVAL_SECS` |

---

## 6. 日常运维操作

### 6.1 查看日志磁盘用量

```bash
# API 查询
curl -s http://127.0.0.1:18001/api/admin/logs/disk-usage \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq

# 直接查看文件
ls -lh /var/log/crabcache/trace.jsonl*
ls -lh /var/log/crabcache/trace-debug.jsonl*
du -sh /var/log/crabcache/raw_capture/
```

### 6.2 查看 PG 日志表行数

```bash
# API（通过 Prometheus 指标）
curl -s http://127.0.0.1:9090/metrics | grep admin_log_pg_rows

# 直连 PG
psql "$CRADMIN_PG_URL" -c "
  SELECT 'trace_logs' AS tbl, count(*) FROM trace_logs
  UNION ALL
  SELECT 'request_logs', count(*) FROM request_logs
  UNION ALL
  SELECT 'audit_log', count(*) FROM audit_log;
"
```

### 6.3 手动清理日志

```bash
# 清理所有轮转文件（保留活跃文件）
curl -X POST http://127.0.0.1:18001/api/admin/logs/clear \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"target": "all"}'

# 仅清理 7 天前的 trace 轮转文件
curl -X POST http://127.0.0.1:18001/api/admin/logs/clear \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"target": "trace_rotated", "older_than_hours": 168}'
```

**清理目标**：`trace_rotated` | `debug_rotated` | `capture` | `all`

### 6.4 更新保留策略

```bash
# 查看当前策略
curl -s http://127.0.0.1:18001/api/admin/logs/retention \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq

# 更新策略
curl -X PUT http://127.0.0.1:18001/api/admin/logs/retention \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "max_age_hours": 168,
    "max_disk_mb": 500,
    "max_trace_files": 20,
    "max_capture_body_files": 5000,
    "pg_retention_days": 7,
    "compress_before_delete": true,
    "compressed_retention_days": 30
  }'
```

### 6.5 PG 手动修剪

```bash
# 删除 7 天前的 trace_logs
psql "$CRADMIN_PG_URL" -c "
  DELETE FROM trace_logs
  WHERE timestamp_ms < extract(epoch from now() - interval '7 days') * 1000;
"

# 删除 30 天前的 audit_log
psql "$CRADMIN_PG_URL" -c "
  DELETE FROM audit_log
  WHERE timestamp < now() - interval '30 days';
"
```

### 6.6 查看请求日志

```bash
# 最近 50 条
curl -s "http://127.0.0.1:18001/api/admin/logs?limit=50" \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq '.items[:3]'

# 按 consumer 过滤
curl -s "http://127.0.0.1:18001/api/admin/logs?consumer=alice&limit=20" \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq

# 游标分页
curl -s "http://127.0.0.1:18001/api/admin/logs?cursor=TIMESTAMP:HASH&limit=50" \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq
```

### 6.7 查看 Raw Capture

```bash
# 最近 capture 列表
curl -s "http://127.0.0.1:18001/api/admin/capture/list?hours=24&limit=20" \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq

# 单条 capture 详情（含 body）
curl -s "http://127.0.0.1:18001/api/admin/capture/REQUEST_ID" \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq

# capture 统计
curl -s "http://127.0.0.1:18001/api/admin/capture/stats?hours=24" \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq
```

---

## 7. Prometheus 指标

| 指标 | 类型 | 标签 | 含义 |
|------|------|------|------|
| `admin_log_writes_total` | Counter | `{log_type}` | trace/debug/capture 写入计数 |
| `admin_log_pg_write_errors_total` | Counter | — | PG 写入失败计数 |
| `admin_log_disk_bytes` | Gauge | `{log_type}` | 各类日志磁盘字节数 |
| `admin_log_pg_rows` | Gauge | `{table}` | PG 日志表行数 |

`log_type` 取值：`trace`、`debug`、`capture_index`、`capture_body`
`table` 取值：`trace_logs`、`request_logs`、`audit_log`

---

## 8. 前端页面

### 8.1 请求日志页（Logs）

- 路径：Dashboard → Logs
- 功能：列表、详情、按 consumer/model/cache_status 过滤、游标分页
- 数据源：PG 优先 → JSONL 回退（`load_trace_with_opts_auto`）
- 列表 API 响应预览截断 200 字符（`CRABCACHE_ADMIN_LOG_LIST_PREVIEW_CHARS`）

### 8.2 日志管理页（Logs Manage）

- 路径：Dashboard → Logs → Manage
- 功能：
  - 磁盘用量可视化（堆叠进度条）
  - 保留策略配置（表单，实时生效）
  - 手动清理（带二次确认）

### 8.3 审计日志页（Audit）

- 路径：Dashboard → Audit
- 功能：按 action 过滤、分页加载（每页 50 条）
- Action 类型：`create_key`、`revoke_key`、`put_domain_policies`、`cache_invalidate`、`update_cache_config`、`clear_logs`

### 8.4 Capture 页

- 路径：Dashboard → Capture
- 功能：列表（多维过滤）、详情（client/upstream body）、统计（推理注入率等）

---

## 9. 故障排查

### 9.1 Trace 日志不生成

**症状**：Dashboard Logs 页无数据，`trace.jsonl` 不存在。

**排查**：
```bash
# 1. 检查配置
grep -A5 '\[trace_logging\]' config/gateway.toml

# 2. 检查目录权限
ls -la /var/log/crabcache/

# 3. 检查网关日志
grep -i "trace.*init\|trace.*fail" ./logs/gateway.log.* | tail -5
```

**原因**：`enabled = false`（默认）或目录无写权限。

### 9.2 PG 同步滞后

**症状**：Dashboard 数据延迟 > 30 秒。

**排查**：
```bash
# 1. 检查同步任务是否启用
grep "PG sync" ./logs/gateway.log.* | tail -3

# 2. 检查 PG 连接
psql "$CRADMIN_PG_URL" -c "SELECT 1"

# 3. 检查是否有冲突
grep "duplicate key\|ON CONFLICT" ./logs/gateway.log.* | tail -5
```

**原因**：PG 不可达、`CRABCACHE_ADMIN_TRACE_PG_SYNC=false`、或 Gateway 已直写 PG 导致 Admin 侧同步被禁用。

### 9.3 磁盘占用过高

**症状**：`admin_log_disk_bytes` 持续增长。

**排查**：
```bash
# 查看各类日志占用
curl -s http://127.0.0.1:18001/api/admin/logs/disk-usage \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq

# 查看保留策略
curl -s http://127.0.0.1:18001/api/admin/logs/retention \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" | jq
```

**处理**：
1. 降低 `max_age_hours` 或 `max_disk_mb`
2. 降低 `max_trace_files`、`max_capture_body_files`
3. 启用 `compress_before_delete` 压缩旧文件
4. 手动执行 `POST /api/admin/logs/clear`

### 9.4 Trace 截断

**症状**：日志中出现 `trace log truncated from tail`，分析丢失早期数据。

**原因**：`trace.jsonl` 超过 `MAX_TRACE_READ_BYTES`（32 MiB）。

**处理**：
- 增加 `max_lines` 加速轮转（单文件更小）
- 增加 `max_files` 保留更多历史
- 使用 PG 作为主要查询源（无截断问题）

### 9.5 Capture body 文件堆积

**症状**：`capture_body_bytes` 过大。

**处理**：
- 降低 `max_body_files`（默认 5000）
- 降低 `sample_rate`（如 0.1 = 10% 采样）
- 启用 `sample_always_on_large_body = false`
- 手动清理：`POST /api/admin/logs/clear` target=`capture`

---

## 10. 保留策略默认值

```rust
RetentionPolicy {
    max_age_hours: 168,           // 7 天
    max_disk_mb: 500,             // 500 MB
    max_trace_files: 20,          // 20 个轮转文件
    max_capture_body_files: 5000, // 5000 个 body 文件
    pg_retention_days: 7,         // PG 保留 7 天
    compress_before_delete: false,
    compressed_retention_days: 30,
}
```

**轮转逻辑**：
- Gateway 侧：`RotatingJsonlWriter` 达到 `max_lines` 时 rename 当前文件为 `trace.jsonl.YYYYMMDD_HHMMSS`，创建新文件，删除超过 `max_files` 的旧文件
- Admin 侧：`log_retention_loop` 每 10 分钟执行 `enforce_retention()`，按策略删除过期/超量/超大文件

---

## 11. 安全与脱敏

| 机制 | 位置 | 说明 |
|------|------|------|
| API Key 脱敏 | `masking.rs` | 正则 `sk-[a-zA-Z0-9_-]{5,}` → 前 4 + 后 4（如 `sk-c...k1l2`） |
| 请求体截断 | `max_payload_bytes` | UTF-8 安全截断（`floor_char_boundary`），附加 `...<truncated>` |
| 响应预览截断 | `max_response_preview_bytes` | 同上 |
| 列表 API 截断 | `CRABCACHE_ADMIN_LOG_LIST_PREVIEW_CHARS` | 默认 200 字符 |
| user_id 审计 | `user_id_audit.rs` | 5 种状态自动检测 DeepSeek 隔离泄漏 |
| SecretString | 配置加载 | 密钥自动遮盖，不进日志 |

---

## 12. 架构决策

### 为什么同时写 JSONL 和 PG？

- **JSONL**：零依赖、低延迟、Gateway 直接写入、适合实时 tail 读取
- **PG**：结构化查询、索引过滤、分页、聚合分析、长期保留
- **双写**：JSONL 作为热路径保证写入性能，PG 作为冷路径支持复杂查询

### 为什么用增量尾读而非全量扫描？

`LiveTraceCache` 跟踪 file_len + inode，每次 poll 仅读取新增字节。2 秒轮询间隔下，增量读取开销 < 10ms，全量扫描可能 > 100ms（大文件）。

### 为什么 Trace 和 Capture 分开存储？

- **Trace**：轻量元数据（~1KB/条），适合高频查询和聚合
- **Capture**：完整 body（可能 > 1MB/条），仅在需要排查时按需加载
- 分离避免 Trace 查询被大 body 文件拖慢
