# Cursor + DeepSeek Thinking 接入指南

CrabCache 在网关内内置了与 [deepseek-cursor-proxy](https://github.com/yxlao/deepseek-cursor-proxy) 等价的 **reasoning_content** 注入、恢复与流式缓存逻辑（Rust：`crab-reasoning`）。仓库内 `deepseek-cursor-proxy/` 与 Go 版 [`cursor-deepseek/`](../cursor-deepseek/) 仅为对照参考，无需单独部署。Go 侧「`gpt-4o` 等模型别名」体验的吸收路线图见 [`CURSOR_DEEPSEEK_ABSORPTION_PLAN.md`](CURSOR_DEEPSEEK_ABSORPTION_PLAN.md)；协议对照见 [`DEEPSEEK_CURSOR_PROXY_PARITY.md`](DEEPSEEK_CURSOR_PROXY_PARITY.md)。

## 架构

```
Cursor  →  HTTPS  →  CrabCache (:8080)  →  DeepSeek API
              sk-cc-*              upstream key pool
```

- **客户端 Key**：Management API 颁发的 `sk-cc-*`（推荐）
- **上游 Key**：`CRABCACHE_UPSTREAM_KEYS` 或 `[upstream].keys`（DeepSeek 账号密钥，勿下发给 Cursor）

### 请求管道（自动选择）

网关按 **上游 profile** 与 **模型/客户端信号** 选择三条管道之一（`crab-pipeline`）：

| 管道 | 典型场景 | 行为 |
|------|----------|------|
| `cursor_deepseek_v4` | DeepSeek profile + `deepseek-v4-*` + Cursor/agent 信号 | Reasoning 注入/恢复、SSE 改写、L2 语义缓存 |
| `deepseek_light` | DeepSeek profile + 其他 `deepseek-*`（如 `deepseek-chat`） | 仅字段规范化，**不**注入 `thinking` |
| `generic_relay` | OpenAI/Anthropic 等 profile | 透传，不碰 reasoning |

默认 **自动**；可为 Key 或域名策略固定管道 / profile：

```bash
curl -s -X POST "http://127.0.0.1:9080/v1/keys" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"name":"cursor","enabled":true,"pipeline":"auto","upstream_profile":"deepseek"}'
```

`pipeline_mode = "force_cursor_v4"`（`[gateway]`）可全局强制 V4 管道（调试）；生产建议 `auto`。

多厂商上游见 `config/gateway.example.toml` 中 `[[upstream.profiles]]` 与 `[gateway] default_upstream_profile`。

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
| Model | `deepseek-v4-pro` / `deepseek-v4-flash-max`，或配置别名如 `gpt-4o`（见 `[gateway.cursor_models]`） |

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
| `display_reasoning = true` | 非流式：可折叠 `<details>` Thinking；**流式**：思考 mirror 到 `delta.content`，不下发 `reasoning_content` 字段 |
| `display_reasoning = false` | **静默模式**（默认，`gateway.toml` / `gateway.docker.toml`）：思考仅存 ReasoningStore；下发 Cursor 前统一 `sanitize_client_completion`（剥 `reasoning_content` 与 `<details>Thinking` 块）。缓存命中在静默模式下也会 regen 带 Thinking 标记的旧 `sse_body`。切换后见 [DSV4_CC_PROXY_REFERENCE.md](DSV4_CC_PROXY_REFERENCE.md) 清 L0/L1 |
| `stream_cache_enabled`（`[cache]`） | 流式响应缓存；Stop 后仍会持久化已收到的 partial reasoning |

公网域名+端口部署时 Base URL 须包含端口，例如 `https://v4.example.com:18000/v1`（不是无端口 URL）。

### 推荐配置（Cursor 默认）

```toml
[reasoning]
thinking_mode = "enabled"
reasoning_effort = "max"
missing_reasoning_strategy = "recover"
display_reasoning = false
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
curl -s -X PUT "http://127.0.0.1:9080/v1/runtime/pipeline" \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"pipeline_mode":"auto","default_upstream_profile":"deepseek","profiles":[{"id":"deepseek","provider":"deepseek"}]}'

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

Cursor 常把该文案显示为 **「用户提供的 API Key 限流」**，多数情况下指 **网关转发的上游厂商 Key**，而不是 Cursor 里填的 `sk-cc-*`（客户端 Key 默认 `rpm_limit=0` 即不限）。

完整对照表与 Prometheus 核对见 **[OPS_RUNBOOK.md](OPS_RUNBOOK.md)**。

### DeepSeek Profile（`default` / CursorDeepSeekV4）

上游账号来自 Profile 的 API Key 池（环境变量 `CRABCACHE_UPSTREAM_KEYS` 或 Management `PUT /v1/upstream/profiles/{id}/keys`）。

1. 确认上游 Key 与手动 `curl api.deepseek.com` 测试用的是**同一批有效 Key**。
2. `GET /v1/upstream/profiles/default/keys`（或 `GET /v1/upstream/keys` 视部署）查看 `cooldown_remaining_secs`。
3. 增加池内 Key 数量（对齐 new-api MultiKey）。
4. 将 `reasoning_effort` 降为 `medium`（热更新见上文）。
5. 高峰仍不足：临时 `missing_reasoning_strategy=recover`（省 token，牺牲 L3 前缀命中）。

### MiMo Profile

日志表现为 `upstream rate limited, attempting key rotation`、HTTP **429**；仅 **1 把** 上游 Key 时 Prometheus 可见 `gateway_upstream_key_retries_total{outcome="cooldown_only"}`，并可能出现 `gateway_rejected_requests_total{reason="upstream_key_exhausted"}`（503，未打到厂商）。

1. `GET /v1/upstream/profiles/mimo/keys` — 至少 **2 把** 启用 Key；429 轮换仅在 **`account_id` 不同** 的 Key 间进行。
2. 高峰 **降低 Cursor 并行**（多 Agent/Task）；大 body（数百 KB）会放大 QPS 压力。
3. 勿与「网关 sk-cc 限流」混淆：日志中不应出现 `Rate limit exceeded for this API key`（除非 Management 显式设置了 `rpm_limit`）。
4. Coalescing：Leader 上游 429/失败时 Follower **不会**再打上游；可能出现批量 Follower 502，属防雪崩设计。

### 通用

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
| DeepSeek / **MiMo** 限流 / WAF | 对端在返回 HTTP 头前关连接；文案常为 `User API Key Rate limit exceeded` → 加对应 Profile 上游 Key，MiMo 见上文 [MiMo Profile](#mimo-profile) |
| Coalescing 雪崩 | 已修复：Leader 失败时 Follower **不再** `falling through to upstream` |

网关对替换后的上游请求还会：

- `User-Agent: curl/8.7.1`、`Accept-Encoding: gzip, deflate, br`（网关在上游响应侧解压，见 `upstream_response_decompress.rs`）
- 流式：`Accept: text/event-stream`；非流式：`Accept: application/json`

**JA3 / TLS 指纹**：主网关上游（chat/completions）由 Pingora BoringSSL 控制，仅支持曲线顺序调优（`[connection] upstream_tls_curves`），无法模拟完整浏览器 JA3/JA4。OAuth 出站（Claude/Gemini/xAI/Codex token exchange）已统一使用 `wreq` Chrome 浏览器仿真（JA3/JA4 级），可通过 `CRABCACHE_OAUTH_TLS_EMULATION` 环境变量切换预设（默认 `chrome130`，可选 `chrome124` 对齐 OmniRoute）。若 TE/CL 与请求头顺滑后仍断连，再考虑上游 TLS 套件调优（最后手段）。

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
upstream_force_http1 = false
upstream_request_timeout_secs = 300
upstream_write_timeout_secs = 300
upstream_connection_timeout_secs = 60
upstream_disable_keepalive = true
```
