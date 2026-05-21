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
| `missing_reasoning_strategy = "recover"` | **默认（与 deepseek-cursor-proxy 一致）**；`client_key`/`x-conversation-id` 下就地补 reasoning，不截断 tool 历史 |
| `missing_reasoning_strategy = "reject"` | 无法恢复时 **HTTP 409**（与 proxy `--missing-reasoning-strategy reject` 一致） |
| `display_reasoning = true` | 非流式：可折叠 `<details>` Thinking；**流式**：仅增量 `delta.content`，不下发 `reasoning_content`（避免 Cursor 断连） |
| `stream_cache_enabled`（`[cache]`） | 流式响应缓存；Stop 后仍会持久化已收到的 partial reasoning |

公网域名+端口部署时 Base URL 须包含端口，例如 `https://v4.example.com:18000/v1`（不是无端口 URL）。

### 推荐配置（Cursor 默认）

```toml
[reasoning]
thinking_mode = "enabled"
reasoning_effort = "max"
missing_reasoning_strategy = "recover"
display_reasoning = true
collapsible_reasoning = true

[cache]
stream_cache_enabled = true

[reasoning]
backend = "redis"
redis_url = "redis://127.0.0.1:6379"
```

### ReasoningStore 持久化 + 稳定会话（减少 recover / notice）

完整说明见 **[REASONING_STORE.md](REASONING_STORE.md)**（scope 优先级、部署检查清单、验收脚本）。

1. **`[reasoning].backend = "redis"`**（或 `CRABCACHE_REASONING_BACKEND=redis`）：思考链写入 Redis `crab:reasoning:*`，容器重启、多副本共享；Docker 见 `config/gateway.docker.toml`（已默认 redis）。
2. **稳定会话 ID**：`x-conversation-id` / `prompt_cache_key` / **`client:<sk-cc>`** → ReasoningStore 固定 scope；多轮 tool 历史从 Redis 补全。
3. **Cursor 侧**：尽量带 `x-conversation-id`；否则自动用 `sk-cc` 哈希作 scope。
4. **任务卡住**：入站 recovery notice 会先剥离；有稳定 scope 时不做 `latest_user` 截断（见 H-G `upstream_msg_count`）。
5. **验收**：日志 `Prepared upstream request` 中 `patched > 0`、`recovered = 0` 表示 Store 补全成功；若频繁 `recovered > 0` 且出现 `[crabcache] Refreshed reasoning_content history.`，检查 Redis 与是否缺少会话头。

部署或升级网关后，**务必清理一次 L0/L1 旧缓存**（旧条目可能混用 `stream:true/false` 键）：

```bash
curl -s -X POST "http://127.0.0.1:9080/v1/cache/invalidate" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "x-cache-invalidate-confirm: all" \
  -H "Content-Type: application/json" \
  -d '{"scope":"all"}'
```

可选：将 `fingerprint_version` 加 1（`PUT /v1/cache/fingerprint`）使旧精确缓存键自然 miss。

**L3 前缀**：`recover` 会截断不可恢复历史（与 proxy 一致）；冲 L3 时固定 system + 追加消息，并配 `x-conversation-id`（见 [`DEEPSEEK_PREFIX_CACHE.md`](DEEPSEEK_PREFIX_CACHE.md)）。

热更新（无需重启）：

```bash
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

一键 Cursor 验收：

```bash
export DOMAIN=v4.example.com
export CLIENT_API_KEY=sk-cc-...
export CRABCACHE_GATEWAY_ADMIN_KEY=...
bash scripts/verify_cursor_e2e.sh
```

## 生产检查清单（跑通 Cursor）

| 步骤 | 命令 / 配置 |
|------|-------------|
| 多上游 Key | `.env` 设置 `CRABCACHE_UPSTREAM_KEYS=sk-1,sk-2,...` |
| 客户端 Key | `POST /v1/keys` 创建 `sk-cc-*`，Cursor 仅填此 Key |
| TLS | `curl -sS https://<domain>:18000/ready` 无 `-k` 返回 200 |
| 清缓存 | `POST /v1/cache/invalidate` + `x-cache-invalidate-confirm: all` |
| 上游池状态 | `GET /v1/upstream/keys` → `cooldown_remaining_secs` 应为 0 |

## 限流排查（`User API Key Rate limit exceeded`）

该错误来自 **DeepSeek 上游账号**（`CRABCACHE_UPSTREAM_KEYS`），不是 Cursor 里的 `sk-cc-*`。

1. 确认 `.env` 中上游 Key 与手动 `curl api.deepseek.com` 测试用的是**同一批新 Key**。
2. `curl -s http://127.0.0.1:9080/v1/upstream/keys -H "x-gateway-admin-key: ..." | jq` 查看 `cooldown_remaining_secs`。
3. 增加 `CRABCACHE_UPSTREAM_KEYS` 条目（对齐 new-api 渠道 MultiKey）。
4. 将 `reasoning_effort` 降为 `medium`（热更新见上文）。
5. 高峰仍不足时：临时 `missing_reasoning_strategy=recover`（省 token，牺牲 L3 前缀命中）。
6. 网关 Coalescing：Leader 上游失败时 Follower **不会**再打上游（避免雪崩）；日志中不应再出现大量 `falling through to upstream` 紧随 429。

从 new-api 迁移：见 [`NEW_API_MIGRATION.md`](NEW_API_MIGRATION.md)。

## 模型后缀（对齐 new-api）

Cursor 可使用：

- `deepseek-v4-flash` + 配置中的 `reasoning_effort`
- `deepseek-v4-flash-max` → 自动映射为 `flash` + `thinking.enabled` + `reasoning_effort=max`
- `deepseek-v4-pro-none` → `thinking.disabled`

实现：`crates/crab-reasoning/src/normalize.rs` 中 `parse_deepseek_v4_thinking_suffix`。

## 上游 ~15s `ConnectionClosed`（0 字节响应）

日志形如 `Upstream ConnectionClosed ... bytes already read: 0 ... duration_ms≈15000` 时，按优先级排查：

| 原因 | 说明 |
|------|------|
| **Chunked + Content-Length 冲突** | Cursor 经 OpenResty 多为 HTTP/2 入站；Pingora 转上游 H1 时可能带 `Transfer-Encoding: chunked`，网关已在 `upstream_request_filter` **去掉 TE、只保留 Content-Length**（`upstream_headers.rs`）。 |
| **>64KiB body 未发到上游** | `request_filter` 读光 body 后 Pingora retry buffer 仅 64KiB；超限则跳过首次 `send_body_to_pipe`，导致只发头不发 body。已 patch `third_party/pingora-proxy`（`retry_buffer_truncated` 时仍触发 body filter）+ `request_body_filter` 注入 `new_request_body`。 |
| DeepSeek 限流 / WAF | 对端在返回 HTTP 头前关连接；文案常为 `User API Key Rate limit exceeded` → 加 `CRABCACHE_UPSTREAM_KEYS` |
| Coalescing 雪崩 | 已修复：Leader 失败时 Follower **不再** `falling through to upstream` |

网关对替换后的上游请求还会：

- `User-Agent: curl/8.7.1`、`Accept-Encoding: identity`（对齐 curl / deepseek-cursor-proxy）
- 流式：`Accept: text/event-stream`；非流式：`Accept: application/json`

**JA3 / TLS 指纹**：一般无需改；若 TE/CL 与请求头顺滑后仍断连，再考虑上游 TLS 套件调优（最后手段）。

确认修复（`CRABCACHE_DEBUG_LOG_PATH`）：

```bash
docker compose build gateway && docker compose up -d gateway
cat .cursor/debug-3f9816.log | jq -c 'select(.hypothesisId=="H3")'
# 期望：had_transfer_encoding 可为 true，但 header_names 中无 transfer-encoding
cat .cursor/debug-3f9816.log | jq -c 'select(.hypothesisId=="R1")'
# header_to_body_ms 应 < 50ms
cat .cursor/debug-3f9816.log | jq -c 'select(.hypothesisId=="H1")'
# alpn 应为 "H1"
```

`[connection]` 推荐（已写入 `gateway.docker.toml`）：

```toml
upstream_force_http1 = true
upstream_request_timeout_secs = 300
upstream_write_timeout_secs = 300
upstream_connection_timeout_secs = 60
upstream_disable_keepalive = true
```
