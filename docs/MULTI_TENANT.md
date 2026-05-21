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

## 范围外

网关侧按 `user_id` 的速率限制**未实现**；DeepSeek 在上游执行账户级和按 `user_id` 的限制。请使用 `UpstreamKeyPool` 管理多个上游 API Key 和 429 冷却。

## 相关文档

- [DEEPSEEK_PREFIX_CACHE.md](./DEEPSEEK_PREFIX_CACHE.md) — L3 粘滞与前缀缓存实践
- [CURSOR_SETUP.md](./CURSOR_SETUP.md) — Cursor 客户端配置
