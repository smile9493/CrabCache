# CrabCache 持久化指南

本文说明各功能数据的存储位置、多实例部署要求，以及备份与恢复流程。

## 数据分类

| 类别 | 数据 | 存储 | 多实例 |
|------|------|------|--------|
| 响应缓存 L0 | Moka 热缓存 | 进程内存 | 不共享（设计如此） |
| 响应缓存 L1 | 精确命中条目 | Redis `cache:{key}` | 共享 |
| 响应缓存 L2 | 语义向量 | Qdrant | 共享 |
| 控制面 | 客户端 `sk-cc-*`、TTL、指纹、路由、连接参数、上游 relay、**域策略** | Redis `crab:state:*` | 共享（必选） |
| 上游密钥池 | DeepSeek `sk-ds-*` | Redis `crab:state:upstream_keys` **+** `admin-state.json` v4 | 共享（`Some([])` 可清空池） |
| 域用量计数 | `domain_usage`（当月 token/成本累计） | PostgreSQL `domain_usage` 表 + 进程内存 | PG 持久化，Admin 代理同步 |
| 消费者月度用量 | `consumer_usage_monthly`（聚合 token/请求数） | PostgreSQL `consumer_usage_monthly` 表 | PG 持久化，从 trace_logs 聚合 |
| 审计日志 | 管理操作记录（密钥创建/吊销/策略变更等） | PostgreSQL `audit_log` 表 | PG 持久化 |
| Reasoning | 思考链恢复 | SQLite 或 Redis `crab:reasoning:*` | 多实例需 `redis` |
| 影子日志 | 脱敏 Trace | JSONL 文件 / 卷 | 每实例或集中采集 |
| Admin UI | 模型元数据、Key 配额、Keys 月度用量 | `data/admin-state.json` | Admin 单实例卷 |
| Admin 指标采样 | 历史采样点（Prometheus 快照） | `data/metrics.sqlite` | Admin 单实例卷 |
| 配置基线 | 启动默认值 | `gateway.toml` + 环境变量 | 各实例相同文件 |

## Redis Key 命名

与 L1 缓存 `cache:{sha}` 隔离，控制面使用前缀 `crab:state`（可配置 `[state].key_prefix`）：

| Key | 说明 |
|-----|------|
| `{prefix}:version` | 单调 revision（INCR） |
| `{prefix}:keys` | JSON：`token -> StoredKey` |
| `{prefix}:runtime` | JSON：TTL、指纹、stream_cache、relay、backends、`connection` |
| `{prefix}:upstream_keys` | JSON：`UpstreamKeySpec[]`（含 secret，敏感）；缺 key = 保留内存池，`[]` = 清空 |
| `{prefix}:domain_policies` | JSON：`domain -> DomainPolicy`（预算/命中率阈值，**不含** `domain_usage`） |
| Pub/Sub `{prefix}:rev` | 发布新 revision；各实例 **订阅 + 轮询** `refresh_interval_secs` 双路径刷新 |

Reasoning（`[reasoning].backend = "redis"`）：

| Key | 说明 |
|-----|------|
| `crab:reasoning:{logical_key}` | JSON：`{reasoning, message_json, created_at}`，带 TTL |

稳定会话 scope、`x-conversation-id` / `req:hash` 与运维清单见 **[REASONING_STORE.md](./REASONING_STORE.md)**。

## 配置

### 控制面（多实例必选）

```toml
[state]
backend = "redis"   # memory | redis
redis_url = ""      # 空则复用 cache.l1_redis_url
key_prefix = "crab:state"
refresh_interval_secs = 5
```

环境变量：`CRABCACHE_STATE_BACKEND=redis|memory`

**单实例 vs 多副本**（`config/gateway.example.toml` 顶部注释对照）：

| 场景 | `[state].backend` | `[reasoning].backend` |
|------|-------------------|------------------------|
| 本机开发 / 单容器 | `memory`（默认） | `sqlite` 或 `redis` |
| `docker compose --scale gateway=N` | **`redis`**（`gateway.docker.toml`） | **`redis`** |

Management 写穿 Redis 失败时重试 3 次并记录 `gateway_state_persist_total` / `gateway_state_persist_errors_total`。

### Reasoning（多实例必选）

```toml
[reasoning]
backend = "redis"   # sqlite | redis
redis_url = ""      # 空则复用 L1 Redis
max_reasoning_entry_bytes = 524288
```

## Docker Compose

### 单实例（默认）

```bash
docker compose up -d
```

卷：`gateway_data`（SQLite reasoning）、`gateway_logs`（trace）、`redis_data`、`admin_data`（Admin profile）。

### 多实例 Gateway

```bash
# gateway.docker.toml 中设置 state.backend=redis, reasoning.backend=redis
docker compose up -d --scale gateway=2
```

注意：

- 去掉 `container_name: crabcache-gateway` 或改为不固定名称（scale 时 Compose 会自动处理）。
- 宿主机端口映射仅适用于单副本；多副本时通过 OpenResty/LB 访问 8080。
- 共享 `redis_data`；`gateway_data` 在多实例 + Redis reasoning 时主要用于 trace/logs。

### Admin 持久化

```bash
docker compose --profile admin up -d
```

`admin_data` 卷挂载 `/app/data`，保存 `admin-state.json`（含 `keys_meta` 配额字段和 `upstream_pool_secrets`）。

**上游 Key 双副本**：Admin 的 `upstream_pool_secrets`（default deepseek profile）同时写入 `admin-state.json` v4 和 Gateway Redis `crab:state:upstream_keys`。**运行时权威**为 Redis；Admin 重启时若 Redis 池为空，会从 `admin-state.json` 回推到 Gateway。上游 Key 通过 Dashboard → Upstream → Save Key Pool 配置，**无需**在 `.env` 中设置真实 Key。

**双源说明**：Admin 的 `domain_policies` 也写入 `admin-state.json`，并在启动时 `sync_domain_policies_to_gateway` 推到 Gateway。**运行时权威**为 Gateway Redis `crab:state:domain_policies`；多 Gateway 副本以 Redis 为准，Admin 重启后应从 Management `GET /v1/domains/policies` 对齐（勿只在 Admin 本地改策略而不同步网关）。

## 备份

| 资产 | 方法 |
|------|------|
| Redis | RDB/AOF、`redis-cli --rdb`；含 `cache:*`、`crab:state:*`、`crab:reasoning:*` |
| Qdrant | 备份 `qdrant_data` 卷 |
| Reasoning SQLite | 复制 `data/reasoning_content.sqlite3` |
| Trace | 复制 `gateway_logs` 或 `[trace_logging].path` |
| Admin state | 复制 `data/admin-state.json` |
| 配置 | 版本化管理 `gateway.toml`、`.env`（勿提交明文密钥） |

## 恢复检查清单

1. 启动 Redis / Qdrant，确认健康。
2. 恢复 Redis 数据或确认 `crab:state:keys` 非空。
3. 各 Gateway 实例 `[state].backend = "redis"`，日志出现 “Loaded control plane state from Redis”。
4. `[reasoning].backend = "redis"` 时无需挂载共享 SQLite。
5. `GET /v1/keys`（Management）与 Agent 请求验证 `sk-cc-*`。
6. Admin 挂载 `admin_data` 后 Keys 页配额字段仍在。

## 明确不持久化

- L0 Moka、Request Coalescing inflight
- Prometheus 进程计数器（靠外部 TSDB）
- Admin `metrics_history` 时序环（现由 SQLite `data/metrics.sqlite` 持久化，重启不丢）

## 迁移

首次启用 Redis 控制面且 key 为空时，网关用 `gateway.toml` + `CRABCACHE_BOOTSTRAP_CLIENT_KEYS` 初始化并写入 Redis。已有 Key 见 [AGENT_CLIENT_KEY_MIGRATION.md](./AGENT_CLIENT_KEY_MIGRATION.md)。

生产切换顺序：文档与卷 → `state.backend=redis` → `reasoning.backend=redis` → 扩缩 Gateway 副本。
