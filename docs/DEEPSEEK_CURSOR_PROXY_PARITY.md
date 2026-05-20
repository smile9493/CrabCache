# deepseek-cursor-proxy 能力对照（CrabCache 内置）

本仓库 [`deepseek-cursor-proxy/`](../deepseek-cursor-proxy/) 为 [yxlao/deepseek-cursor-proxy](https://github.com/yxlao/deepseek-cursor-proxy) 参考实现（Python）。**生产请使用 CrabCache 网关**（`crab-gateway` + `crab-reasoning`），不要与 Python 代理共用同一 Cursor Base URL。

## 要解决的问题

| 问题 | 表现 | 处理 |
|------|------|------|
| 多轮 Tool Call + Thinking | 400：`reasoning_content must be passed back` | SQLite 缓存 + 出站补全 |
| 流式兼容 | Cursor “trouble connecting to the model provider” | 流式仅 `delta.content`，删除 `reasoning_content`（含 `null`） |
| 协议 | `functions` / 多段 `content` 等 | `prepare_upstream_request` 规范化 |

## 能力对照

| 能力 | deepseek-cursor-proxy | CrabCache |
|------|----------------------|-----------|
| reasoning SQLite | `~/.deepseek-cursor-proxy/...` | `[reasoning].cache_db_path` |
| 命名空间隔离 | `authorization_hash` | 客户端 `Authorization` 哈希（`reasoning_cache_namespace`） |
| 缺失策略 recover / fill_only / reject | 支持 | 支持，默认 **recover** |
| 流式 partial 落库 | server finally | `flush_streaming_reasoning` |
| L0/L1/L2 缓存、合并、路由 | 无 | 有（`stream` 参与精确缓存键，避免流式/非流式混用） |
| 内置 ngrok | 有 | 无（OpenResty / Cloudflare Tunnel） |

## 刻意差异（流式）

参考项目在 `display_reasoning=true` 时保留 `delta.reasoning_content` 并用 per-chunk `<details>` HTML。

CrabCache 流式路径：

- 增量 **`delta.content`**
- **始终移除** `reasoning_content`
- 缓存命中若仅有 JSON，经 `json_to_sse_stream` / `message_to_cursor_safe_delta` 同样不含该字段

非流式仍可用 `fold_reasoning_into_content` + `<details>`（`display_reasoning=true`）。

## 代码映射

| Python | Rust |
|--------|------|
| `transform.prepare_upstream_request` | `crates/crab-reasoning/src/normalize.rs` |
| `reasoning_store.py` | `crates/crab-reasoning/src/store.rs` |
| `streaming.py` | `crates/crab-reasoning/src/streaming.rs` |
| `server._rewrite_sse_line` | `crates/crab-proxy/src/proxy.rs` |

## 部署

```text
Cursor → https://<domain>:18000/v1 → gateway:8080 → DeepSeek
         sk-cc-*                         CRABCACHE_API_KEY
```

- 配置：[`docs/CURSOR_SETUP.md`](CURSOR_SETUP.md)
- 验收：`CLIENT_API_KEY=sk-... bash scripts/verify_deployment.sh`（含流式 SSE 检查）

## 参考目录

见 [`deepseek-cursor-proxy/REFERENCE.md`](../deepseek-cursor-proxy/REFERENCE.md)。
