# 多租户隔离（`project_id` / DeepSeek `user_id`）

CrabCache 以**单一网关进程**运行，通过逻辑方式隔离租户——无需为每个项目部署独立网关。

## 架构

```text
客户端（sk-cc-* + 可选 X-Project-Id）
  → 解析 project_id（密钥绑定的域名、请求头校验）
  → 为 DeepSeek 上游注入 body.user_id
  → L0/L1 缓存键前缀：{global_namespace}:{project_id}:{hash}
  → Reasoning SQLite 命名空间包含 project_id
  → L2 Qdrant 过滤：tenant_id == project_id
```

## 租户标识

| 来源 | 作用 |
|------|------|
| `StoredKey.project_id` | 权威租户 ID（Management API / Admin 设置） |
| `X-Project-Id` 请求头 | 当密钥已绑定 `project_id` 时必须匹配；密钥未绑定时可清理后使用 |
| 不匹配 | HTTP **403** `project_mismatch` |
| 无效 ID | HTTP **400** `invalid_project_id`（必须匹配 `[a-zA-Z0-9\-_]+`，最长 512） |

## DeepSeek `user_id`

当 `project_id` 解析成功后，网关会**覆写**上游 OpenAI 兼容请求体中的 `user_id`。这实现了 DeepSeek 官方的隔离能力：

- 按租户隔离的 KV / 前缀缓存边界
- 按租户的内容安全范围
- 上游调度 / 按 `user_id` 的并发控制（账户级别限制仍然适用）

**注意**：这与 OpenAI 遗留的 `"user"` 字段（仅用于滥用追踪）不同。

## 网关缓存层级

| 层级 | 隔离方式 |
|------|----------|
| L0/L1 | `effective_cache_namespace(global, project)` → `{global}:{project}:{fingerprint_hash}` |
| Reasoning 存储 | `reasoning_cache_namespace(..., project_id)` |
| L2 语义 | Qdrant payload `tenant_id` + 搜索过滤；point id `hash(tenant:query)` |
| Ketama L3 粘滞 | `x-conversation-id` > `prompt_cache_key` > **body `user_id`** > `x-user-id` > IP |

在 `gateway.toml` 中配置可选全局前缀：

```toml
[cache]
# 可选；与请求级 project_id 组合为 "{global}:{project}:{hash}"
# cache_key_namespace = "org"
```

## Management API

创建或更新密钥时指定 `project_id`：

```json
POST /v1/keys
{
  "name": "project-a",
  "enabled": true,
  "project_id": "project_a"
}
```

## 指标与追踪

- Prometheus `consumer` 标签在密钥没有基于 `name` 的 consumer 时，回退到 `project_id`。
- Trace JSONL 在设置 `project_id` 后包含该字段。

## 安全

不要信任客户端提供的 JSON body 中的 `user_id`；当 `project_id` 解析后网关会替换它。生产环境中应将租户绑定到 API 密钥上。

## per-`user_id` 并发软限（可选）

在 `gateway.toml` 中启用 `[upstream.deepseek_user_concurrency]`（默认 `enabled = false`）后，网关对带 `project_id` 的 DeepSeek v4 请求做进程内 in-flight 计数：`deepseek-v4-pro` 与 `deepseek-v4-flash` 分别使用 `v4_pro_per_user_id` / `v4_flash_per_user_id`（默认 500 / 2500）。超限时返回网关 **429**（`deepseek_user_concurrency_exceeded`），区别于上游 429 与 Key 池轮换。无 `project_id` 时不施加该限制。

## DeepSeek 上游 Key 池（勿与并发混淆）

官方文档：**并发按 DeepSeek 账号计，与 API Key 数量无关**。同一账号下配置多个 Key **不会**提高并发额度。

| 场景 | 建议 |
|------|------|
| 提高总并发 | 多个 **DeepSeek 账号**（或向官方申请账号扩容） |
| 租户隔离 / per-user 并发槽 | 为每个 `sk-cc-*` 配置 **`project_id`**（映射为上游 `user_id`） |
| 单账号 Key 轮换 | 仅用于密钥吊销、401、运维切换；**不能**靠同账号多 Key 缓解并发 429 |
| 多账号 429 轮换 | 为每个 Key 设置 **`account_id`**（Management PUT 或 Dashboard 行格式 `account_id:sk-...`）；HTTP 429 时仅在**不同** `account_id` 间切换 |

未设置 `account_id` 的 Key 归入内部桶 `default`（视为同一账号，429 后不互相轮换）。

`UpstreamKeyPool` 按 **upstream profile** 管理凭证（Dashboard「上游配置」页可分别维护 `deepseek` / `mimo` 等池）。

## 验收：影子日志 user_id 审计

启用 `[trace_logging]` 后，Admin **Trace 分析**页展示 **DeepSeek user_id 隔离审计**（`user_id_audit=injected` 表示 `project_id` 已写入上游 body）。详见 [OBSERVABILITY.md](./OBSERVABILITY.md)。

## 相关文档

- [DEEPSEEK_PREFIX_CACHE.md](./DEEPSEEK_PREFIX_CACHE.md) — L3 粘滞与前缀缓存实践
- [CURSOR_SETUP.md](./CURSOR_SETUP.md) — Cursor 客户端配置
- [OBSERVABILITY.md](./OBSERVABILITY.md) — 影子日志与 user_id 审计字段
