# 数据面 P2 验收手册

本文档提供 **MiMo 中继** 和 **DeepSeek/Cursor** 两条流量路径的操作验收清单，对应 [DATA_PLANE.md](./DATA_PLANE.md) 建议验收表。

> [!NOTE]
> 本文档用于**手工 + 自动化**验证已落地的数据面 feature。自动化回归测试见 `crates/crab-gateway/tests/data_plane.rs`。  
> **生产事故 / MiMo 429 / 低命中率解读** 不在本验收范围内，见 [OPS_RUNBOOK.md](./OPS_RUNBOOK.md)。

---

## 前置条件

| 项 | 要求 |
|----|------|
| OS | Linux（macOS Pingora SSE 有 flush bug，不推荐验收） |
| Redis | `docker compose up -d redis`，或已有 Redis 7 实例 |
| 网关配置 | 复制 `config/gateway.example.toml` → `config/gateway.toml`，修改 `api_key` |

### Feature 开关（`[features]`）

```toml
[features]
connection_prewarm = true          # 直池 TCP+TLS 预热
affinity_prompt_cache_feedback = true  # 请求末 affinity hint
prefix_aware_cache = true          # L0 前缀索引（MiMo 默认等效开启）
```

### 启动

```bash
cargo run --bin crab-gateway -- config/gateway.toml
```

---

## MiMo 中继路径

适用管道：`mimo_relay` / `mimo_token_plan_relay` / `mimo_payg_relay`。

### 1. Exact cache（early cache，full parse 前）

**目的**：验证 [`body_quick_parse.rs`](../crates/crab-proxy/src/body_quick_parse.rs) + `try_early_exact_cache` 在第二次请求时命中 exact cache，跳过 full JSON parse。

```bash
# 写入一份缓存
curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"mimo-v2","messages":[{"role":"user","content":"hello"}],"stream":false}' | jq .

# 再发一次（应命中 exact cache，x-cache-status: hit）
curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"mimo-v2","messages":[{"role":"user","content":"hello"}],"stream":false}' | jq .
```

**预期**：
- 第二次响应头含 `x-cache-status: hit`
- Prometheus 无 `json_parse_client` 阶段 latency（或值极小）
- 网关日志无 `Prepared upstream request`（未进入 prepare 流程）

### 2. Prefix 索引预热

**目的**：验证 [`tiered.rs`](../crates/crab-cache/src/tiered.rs) prefix index 在共享前缀请求中被更新，但**不短路返回**。

```bash
# 请求 A：前缀 + 尾消息 1
curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"mimo-v2","messages":[{"role":"system","content":"You are helpful."},{"role":"user","content":"msg1"}],"stream":false}' | jq .

# 请求 B：相同前缀 + 不同尾消息
curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"mimo-v2","messages":[{"role":"system","content":"You are helpful."},{"role":"user","content":"msg2"}],"stream":false}' | jq .
```

**预期**：
- 两个请求均返回 200（B 不因 prefix 索引短路返回 A 的响应）
- Prometheus `gateway_prefix_index_warmup_total` 值 +1（B 触发索引预热）

### 3. MiMo pipeline 默认开启 prefix-aware

**目的**：MiMo 管道不需要显式设 `prefix_aware_cache = true`，即等效启用。

- 将 `prefix_aware_cache` 注释掉（默认 false）
- 执行上方 Prefix 索引测试
- **预期**：`gateway_prefix_index_warmup_total` 仍递增（因 `is_mimo_pipeline` 覆盖）

---

## DeepSeek / Cursor 路径

适用管道：`cursor_deepseek_v4`、`deepseek_light`。

### 4. Connection pre-warm

**目的**：验证 [`connection_prewarm.rs`](../crates/crab-proxy/src/connection_prewarm.rs) 对新 `session_fingerprint` 触发直池预热。

```bash
# 首次请求（新会话指纹，应触发 prewarm）
curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"hello"}],"stream":false}' | jq .
```

**预期**：
- 网关日志含 `"New session fingerprint detected"` 或 `"Direct pre-warm: TLS connection established"`
- Prometheus **无** 到 `127.0.0.1:8080` 的额外 HTTP 请求（非 loopback 模式）
- 第二次相同 conversation 的 TTFT 可选对比（不强制）

### 5. Affinity prompt-cache feedback

**目的**：验证上游 `prompt_cache_hit_tokens` 反馈到路由 hint。

```bash
# 固定 conversation_id，连续发两次
for i in 1 2; do
  curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
    -H "Authorization: Bearer $API_KEY" \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"deepseek-v4-pro\",\"conversation_id\":\"test-affinity\",\"messages\":[{\"role\":\"user\",\"content\":\"msg $i\"}],\"stream\":false}" | jq '.usage'
done
```

**预期**：
- 若上游返回 `prompt_cache_hit_tokens > 0`，第二次请求应路由到同一 backend
- Prometheus `gateway_upstream_prompt_cache_tokens_total{status="hit"}` 递增

### 6. Prefix L0 索引（DeepSeek 长会话）

**目的**：DeepSeek 长会话共享 system 前缀时触发 prefix 索引预热。

```bash
# 请求 1：system + user
curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-v4-pro","messages":[{"role":"system","content":"You are a coding assistant."},{"role":"user","content":"explain caching"}],"stream":false}' | jq .

# 请求 2：相同 system，不同 user
curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-v4-pro","messages":[{"role":"system","content":"You are a coding assistant."},{"role":"user","content":"explain routing"}],"stream":false}' | jq .
```

**预期**（需 `prefix_aware_cache = true`）：
- `gateway_prefix_index_warmup_total` +1
- 两个请求均返回 200（第二个不短路）

---

## Prometheus 核对表

验收后在 `http://127.0.0.1:9090/metrics`（默认端口）检查：

| 指标 | 预期 | 勿混淆 |
|------|------|--------|
| `gateway_prefix_index_warmup_total` | 随 prefix 测试递增 | 勿等同于 `gateway_cache_requests_total`（后者是网关完整响应命中） |
| `gateway_request_phase_latency_seconds` | 各 phase 有值 | 观察 `pipeline_select_done`、`upstream_body_sent` 是否合理 |
| `gateway_upstream_prompt_cache_tokens_total` | DeepSeek 上游 hit/miss | 与 L0/L1/L2 完全正交 |
| `gateway_upstream_latency_seconds` | 首次 vs 后续 TTFT 差异（可选） | 非必须通过 |

---

## 集成测试（自动化）

以下测试在 CI 中执行（Redis 可用时）：

- `crates/crab-gateway/tests/data_plane.rs` — prefix L0 + exact key roundtrip
- `crates/crab-proxy/src/metrics_helpers.rs` — affinity streak / finalize 单测
- `crates/crab-proxy/src/body_quick_parse.rs` — 字段提取边界
- `crates/crab-route/src/ring.rs` — `select_with_hint` 加权 backend

```bash
cargo test -p crab-gateway -p crab-proxy -p crab-route -p crab-cache
```

---

## 已知限制

- `streaming_body_forward`（MiMo 流式 body）— 默认 off，见 [STREAMING_BODY_FORWARD.md](STREAMING_BODY_FORWARD.md)
- `proxy.rs` 已拆至 `phases/`（~566 行 + `phases/request_filter.rs` 等）；Phase 1–4 子模块（`routing_gate` 等）仍为可选后续
- Raw capture 仍为同步通道 — logging 阶段阻塞 ~5-15ms，后续优化
