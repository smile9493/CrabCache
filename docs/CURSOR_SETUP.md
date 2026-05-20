# Cursor + DeepSeek Thinking 接入指南

CrabCache 在网关内内置了与 [deepseek-cursor-proxy](https://github.com/yxlao/deepseek-cursor-proxy) 等价的 **reasoning_content** 注入、恢复与流式缓存逻辑（Rust：`crab-reasoning`）。仓库内 `deepseek-cursor-proxy/` 仅为对照参考，无需单独部署 Python 代理。详见 [`DEEPSEEK_CURSOR_PROXY_PARITY.md`](DEEPSEEK_CURSOR_PROXY_PARITY.md)。

## 架构

```
Cursor  →  HTTPS  →  CrabCache (:8080)  →  DeepSeek API
              sk-cc-*              upstream key pool
```

- **客户端 Key**：Management API 颁发的 `sk-cc-*`（推荐）
- **上游 Key**：`CRABCACHE_UPSTREAM_KEYS` 或 `[upstream].keys`（DeepSeek 账号密钥，勿下发给 Cursor）

## 公网入口

Cursor 无法使用 `localhost` 作为 API Base URL。任选其一：

1. **Nginx / Caddy** 反代本机 `127.0.0.1:8080`（见 [`deploy/nginx/crabcache-api.conf.example`](../deploy/nginx/crabcache-api.conf.example)）
2. **[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/)**
3. **手动 ngrok**：`ngrok http 8080`，将打印的 HTTPS URL 用作 Base URL 根

CrabCache **不内置** ngrok 子进程；隧道由外部工具提供。

## Cursor 配置

| 字段 | 值 |
|------|-----|
| Base URL | `https://<你的域名>/v1` |
| API Key | `sk-cc-...`（Management `POST /v1/keys` 创建） |
| Model | 请求体中的 model，如 `deepseek-v4-pro` |

创建客户端 Key：

```bash
curl -s -X POST "http://127.0.0.1:9080/v1/keys" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"name":"cursor","enabled":true}'
```

## 上游与 reasoning 配置

`config/gateway.toml` 中 `[reasoning]` 段：

| 选项 | 说明 |
|------|------|
| `missing_reasoning_strategy = "recover"` | 默认：自动截断不可恢复历史并注入缓存的 reasoning（**降低 L3 前缀命中率**） |
| `missing_reasoning_strategy = "fill_only"` | 仅从 ReasoningStore 补全，不截断历史（**推荐冲 L3**，见 [`DEEPSEEK_PREFIX_CACHE.md`](DEEPSEEK_PREFIX_CACHE.md)） |
| `missing_reasoning_on_fill_only` | `omit_reasoning`（默认）或 `reject`（fill_only 时仍缺 reasoning） |
| `missing_reasoning_strategy = "reject"` | 严格模式：无法恢复时返回 **HTTP 409**（调试用） |
| `display_reasoning = true` | 非流式：可折叠 `<details>` Thinking；**流式**：仅增量 `delta.content`，不下发 `reasoning_content`（避免 Cursor 断连） |
| `stream_cache_enabled`（`[cache]`） | 流式响应缓存；Stop 后仍会持久化已收到的 partial reasoning |

公网域名+端口部署时 Base URL 须包含端口，例如 `https://v4.example.com:18000/v1`（不是无端口 URL）。

### 推荐配置（二选一）

**高 Cursor 兼容（牺牲 L3）**

```toml
[reasoning]
missing_reasoning_strategy = "recover"
```

请求头建议：`x-conversation-id: <stable-id>`

**高 L3 前缀命中（Agent / 固定 system + 只追加）**

```toml
[reasoning]
missing_reasoning_strategy = "fill_only"
missing_reasoning_on_fill_only = "omit_reasoning"
```

请求头建议：`x-conversation-id` 或 `x-prompt-cache-key`（与 body `prompt_cache_key` 二选一即可）。

热更新（无需重启）：

```bash
# 高 L3
curl -s -X PUT "http://127.0.0.1:9080/v1/runtime/reasoning" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"thinking_mode":"enabled","reasoning_effort":"max","missing_reasoning_strategy":"fill_only","missing_reasoning_on_fill_only":"omit_reasoning","display_reasoning":true,"collapsible_reasoning":true}'

# 高 Cursor 兼容
curl -s -X PUT "http://127.0.0.1:9080/v1/runtime/reasoning" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"thinking_mode":"enabled","reasoning_effort":"max","missing_reasoning_strategy":"recover","display_reasoning":true,"collapsible_reasoning":true}'
```

## 清空 reasoning 缓存

当出现异常历史或调试 strict 模式时：

```bash
# CLI
crab-gateway --clear-reasoning-cache config/gateway.toml

# Management API
curl -s -X DELETE "http://127.0.0.1:9080/v1/reasoning/cache" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}"
```

## 过渡期（单 Key）

若短期内无法更换 Cursor 中的 Key，可启用 legacy 客户端认证（**不推荐生产**）：

```toml
[gateway]
legacy_api_key_as_client_auth = true
```

详见 [`AGENT_CLIENT_KEY_MIGRATION.md`](AGENT_CLIENT_KEY_MIGRATION.md)。

## Docker 快速启动

```bash
cp .env.example .env
# 设置 CRABCACHE_API_KEY、CRABCACHE_GATEWAY_ADMIN_KEY
docker compose up -d --build
```

验收（需先 `docker compose build gateway`）：

```bash
CLIENT_API_KEY=sk-cc-... ./scripts/verify_deployment.sh
CLIENT_API_KEY=sk-cc-... bash scripts/verify_domain_port.sh
# 可选第二模型
VERIFY_MODEL=deepseek-v4-flash CLIENT_API_KEY=sk-cc-... bash scripts/verify_domain_port.sh
```

脚本会校验流式响应中**不得**出现 `reasoning_content` 且包含 `data: [DONE]`。
