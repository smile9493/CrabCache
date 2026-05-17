# Agent 客户端 Key 迁移指南

网关启用 **DeepSeek 上游 Key 池** 后，客户端与上游密钥已解耦。

## 变更摘要

| 角色 | 以前 | 现在 |
|------|------|------|
| Agent / IDE | `Authorization: Bearer <CRABCACHE_API_KEY>` | `Authorization: Bearer <sk-cc-*>` |
| 服务端上游 | 与客户端相同 Key | `CRABCACHE_UPSTREAM_KEYS` 或 `[upstream].keys` |

DeepSeek 账号 Key **不得**下发给 Agent。

## 迁移步骤

1. 启动网关并确认 `/ready` 为 200。
2. 通过 Management API 创建客户端 Key：

```bash
curl -s -X POST "http://127.0.0.1:9080/v1/keys" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"name":"my-agent","enabled":true}'
```

3. 将返回的 `key_full`（`sk-cc-...`）配置到 Agent 的 API Key。
4. Base URL 仍为 `https://你的网关/v1`（或 `http://127.0.0.1:8080/v1`）。
5. 运行验收：`CLIENT_API_KEY=sk-cc-... ./scripts/verify_deployment.sh`

## 过渡期（不推荐生产）

若短期内无法更换所有 Agent，可在 [`config/gateway.toml`](../config/gateway.example.toml) 中设置：

```toml
[gateway]
legacy_api_key_as_client_auth = true
```

此时顶层 `api_key` / `CRABCACHE_API_KEY` 仍可作为客户端 Bearer。请在完成迁移后关闭。

## 多上游 Key

在 `.env` 或配置中设置：

```bash
CRABCACHE_UPSTREAM_KEYS=sk-ds-1,sk-ds-2,sk-ds-3
```

网关会在缓存未命中时轮换使用池内 Key，客户端仍只需一个 `sk-cc-*`。
