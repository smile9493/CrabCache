# dsv4-cc-proxy 参考对照

上游项目：[HosheaLi/dsv4-cc-proxy](https://github.com/HosheaLi/dsv4-cc-proxy)（Anthropic API / Claude Code 代理）。

本地可选浅克隆（已 `.gitignore`）：

```bash
git clone --depth 1 https://github.com/HosheaLi/dsv4-cc-proxy.git dsv4-cc-proxy
```

## 与 CrabCache 的映射

| dsv4-cc-proxy | CrabCache（OpenAI/Cursor） |
|---------------|---------------------------|
| 请求：tool_use 前注入空 `thinking` 块 | `prepare_upstream_request` + ReasoningStore 补全 `reasoning_content` |
| 请求：`adaptive` → `disabled`，剥历史 thinking 块 | `thinking_mode` / 入站 `strip_cursor_thinking_blocks` |
| 响应：SSE 过滤 `content_block` thinking 事件 | `display_reasoning = false` → `strip_reasoning_delta_for_client` |
| thinking **enabled** 时不剥响应 | `display_reasoning = true` → mirror/fold 到 `content` |

## CrabCache 配置（长会话 + 多轮 tool）

```toml
[reasoning]
display_reasoning = false   # 静默：思考链仅存 ReasoningStore，不占 Cursor 上下文
missing_reasoning_strategy = "recover"
backend = "redis"
```

相关文档：[DEEPSEEK_CURSOR_PROXY_PARITY.md](./DEEPSEEK_CURSOR_PROXY_PARITY.md)、[CURSOR_SETUP.md](./CURSOR_SETUP.md)。

## 运维：切换 `display_reasoning`

修改 `[reasoning].display_reasoning` 或 `PUT /v1/runtime/reasoning` 后：

1. **推荐**：`POST /v1/cache/invalidate`（`scope: all`，带 `x-cache-invalidate-confirm: all`），或 `PUT /v1/cache/fingerprint` 升版本使旧精确键自然 miss。
2. **自动兜底**：缓存条目含 `client_display_reasoning`；静默模式下若 `response_body` 或 `sse_body` 仍含 Thinking 标记（`<details>Thinking`、`<think>`、`reasoning_content` 字段等），流式命中会 **force regen**（`thinking_markup_regen`）。非流式命中会对 JSON 做 `sanitize_client_completion` 后再下发。切换开关后仍建议 `invalidate` 一次最干净。

`PUT /v1/runtime/reasoning` 在 `display_reasoning` 变更时返回 `cache_invalidate_recommended: true` 并写 warn 日志。
