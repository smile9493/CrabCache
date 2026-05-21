# 从 new-api + deepseek-cursor-proxy 迁移到 CrabCache

本文说明如何将原先 **new-api（Go）+ deepseek-cursor-proxy（Python）** 双栈能力，收敛到 **单栈 CrabCache**（`crab-gateway` + `crab-reasoning`）。

## 架构对照

```text
# 原双栈
Cursor → new-api (/v1, 用户令牌) → [可选] deepseek-cursor-proxy :9000 → api.deepseek.com

# CrabCache 单栈
Cursor → OpenResty :18000 → crab-gateway :8080 → api.deepseek.com
         sk-cc-*                          CRABCACHE_UPSTREAM_KEYS
```

## 配置映射

| new-api / Python 代理 | CrabCache |
|----------------------|-----------|
| 渠道 `ApiKey` 多行 / MultiKey 轮询 | `CRABCACHE_UPSTREAM_KEYS=sk-1,sk-2` 或 `PUT /v1/upstream/keys` |
| 用户令牌（计费） | Management `POST /v1/keys` → `sk-cc-*` |
| `deepseek-v4-flash-max` 后缀 | 内置 `parse_deepseek_v4_thinking_suffix`（同 new-api `-max`/`-none`） |
| Python `missing_reasoning_strategy=recover` | 默认 **`recover`**（与 proxy 一致）；`client_key` scope 下就地补 reasoning、不截断 tool 历史 |
| Python 流式保留 `reasoning_content` | CrabCache 流式**仅** `delta.content`（防 Cursor 断连） |
| Python 无答案缓存 | CrabCache **L0/L1** + Coalescing（需部署后 invalidate 一次） |
| ngrok 公网 URL | OpenResty + Let's Encrypt（`docs/deploy-1panel-openresty.md`） |
| new-api `thinking_to_content` | CrabCache `display_reasoning` + 流式 `rewrite_sse_chunk` |

## 迁移步骤

1. **复制上游 Key 池**  
   将 new-api 渠道中启用的 DeepSeek Key 写入 `.env`：
   ```env
   CRABCACHE_UPSTREAM_KEYS=sk-from-channel-1,sk-from-channel-2
   ```

2. **创建客户端 Key**  
   ```bash
   curl -s -X POST "http://127.0.0.1:9080/v1/keys" \
     -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
     -H "Content-Type: application/json" \
     -d '{"name":"cursor","enabled":true}'
   ```

3. **Cursor 设置**  
   - Base URL: `https://<domain>:18000/v1`  
   - API Key: 上一步的 `sk-cc-*`  
   - Model: `deepseek-v4-flash` 或 `deepseek-v4-flash-max`

4. **启动与验收**  
   ```bash
   docker compose up -d --build
   bash scripts/verify_cursor_e2e.sh
   ```

5. **停用旧栈**  
   勿让 Cursor 同时指向 new-api 与 CrabCache；下线 Python 代理与重复反代。

## 行为差异（预期）

| 场景 | 双栈 | CrabCache |
|------|------|-----------|
| 重复相同请求 | 每次打上游 | L0/L1 命中可不打上游 |
| 上游 429 | new-api 换 Key |  Key 冷却 60s + 多 Key 轮询；Follower 不重复打上游 |
| 限流错误文案 | 上游 JSON | 同上；池耗尽时 `503` + `upstream_key_exhausted` |

## 参考

- [`CURSOR_SETUP.md`](CURSOR_SETUP.md)
- [`DEEPSEEK_CURSOR_PROXY_PARITY.md`](DEEPSEEK_CURSOR_PROXY_PARITY.md)
- new-api 源码：`new-api/setting/reasoning/suffix.go`、`new-api/model/channel.go`（`GetNextEnabledKey`）
