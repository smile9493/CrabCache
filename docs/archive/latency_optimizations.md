# CrabCache MiMo 请求延迟优化：基于实际代码的分析

> 纯分析，不修改任何文件。每项建议引用具体文件与行号。

---

## 当前 `request_filter` 热路径的 CPU 时间线（700KB body）

```mermaid
gantt
    title request_filter 各阶段耗时示意 (700KB body)
    dateFormat X
    axisFormat %L ms

    section Body Read
    read_request_body + SHA-256   :0, 3

    section JSON Parse
    serde_json::from_slice (1st)  :3, 8

    section Pipeline + Prepare
    select_pipeline               :8, 9
    prepare_mimo_request          :9, 12
    serde_json::to_vec            :12, 15

    section Composition
    extract_composition           :15, 18
    SHA-256 outbound_fp           :18, 19
    serde_json::from_slice (2nd)  :19, 23

    section Cache
    cache key gen                 :23, 24
    L0/L1 lookup                  :24, 27

    section Key Acquire
    upstream_key acquire          :27, 28
```

**核心问题**：`serde_json::from_slice` 被调用了**两次**（L217 和 L614–617），加上 `extract_composition` 遍历 messages，700KB body 在 request_filter 内产生约 **15–20ms** 的纯 CPU 开销。

---

## 优化 A（高收益 · 低风险）：消除第二次 JSON 解析

### 问题

[request_filter.rs:601–617](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L601-L617)：

```rust
let new_body = ctx.new_request_body.clone().unwrap_or(full_body);
// ...
let parsed_upstream_payload =
    serde_json::from_slice::<serde_json::Value>(new_body.as_ref())  // ← 第二次完整 parse
        .ok()
        .map(Arc::new);
```

而 `prepare_mimo_request`（L397–408）内部已经有一个完整的 `Value::Object(payload)` — 它做完 `filter_fields` / `retire_prefix` 后，调用 `serde_json::to_vec(&mimo.payload)` 序列化回 bytes，然后这里又立刻 parse 回来。

**700KB body 的代价**：`to_vec` ~3ms + `from_slice` ~5ms = **~8ms 纯浪费**。

### 修复方向

让 `prepare_mimo_request` 同时返回 `Arc<Value>`（prepared payload 的解析树）：

```rust
pub struct MimoPreparedRequest {
    pub payload_bytes: Bytes,           // 序列化后的 bytes (upstream 发送用)
    pub payload_parsed: Arc<Value>,     // 同一棵解析树 (composition / debug 用)
    pub model: String,
    pub retired_prefix_messages: u32,
}
```

调用方直接用 `mimo.payload_parsed` 赋给 `ctx.parsed_upstream_payload`，**省掉 L614–617 的第二次 parse**。

### 预估收益

| body 大小 | 节省 |
|-----------|------|
| 200 KB | ~2 ms |
| 700 KB | ~5–8 ms |
| 2 MB | ~15–20 ms |

---

## 优化 B（高收益 · 低风险）：`extract_composition` 惰性化

### 问题

[request_filter.rs:542–599](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L542-L599)：

```rust
if let Some(payload) = ctx.parsed_request_payload.as_ref() {
    // 每个请求都跑：遍历 system_text (100KB) + tools (100KB) + messages
    ctx.request_composition = Some(extract_composition(payload, &hints));
    // ...
    if let Some(debug_tx) = composition_debug_tx() {
        // 额外的 extract_system_text + extract_tools_json (各 100KB 上限)
    }
}
```

`extract_composition` 对每个请求都执行完整 JSON 遍历：
- 扫描 `messages` 数组计算 `message_count`、`last_user_message_length`
- 提取 `system_text` 的前 100KB
- 提取 `tools` JSON 的前 100KB

对 700KB body 这是 **~3–5ms** 的纯 CPU 开销。

### 修复方向

**方案 1**（推荐）：将 `extract_composition` 移到 `logging` 阶段（[logging.rs](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/logging.rs)），此时请求已完成，不在关键路径上。`record_composition_metrics` 也可以延迟。

**方案 2**：只在 `debug_tx` 存在 OR `RUST_LOG=debug` 时才执行 `extract_system_text` / `extract_tools_json`。composition 基础指标（`message_count` 等）可从 `parsed_request_payload` 中 O(1) 提取（`messages.as_array().len()`）。

### 预估收益

| body 大小 | 节省 |
|-----------|------|
| 200 KB | ~1 ms |
| 700 KB | ~3–5 ms |
| 2 MB | ~8–12 ms |

---

## 优化 C（中收益 · 低风险）：SSE 流禁用 upstream gzip

### 问题

[upstream_headers.rs:10](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/upstream_headers.rs#L10)：

```rust
pub const UPSTREAM_ACCEPT_ENCODING: &str = "gzip, deflate, br";
```

[upstream_headers.rs:41](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/upstream_headers.rs#L41) — 无论 streaming 还是 non-streaming，都发送 `Accept-Encoding: gzip, deflate, br`。

当 MiMo 上游返回 gzip 压缩的 SSE 流时：
1. Gateway 的 `upstream_response_decompress` 需要逐 chunk **解压**
2. 解压后的 SSE 数据再转给 Cursor 客户端

SSE 流的压缩率极低（每个 `data: {...}\n\n` 块只有几十到几百字节，gzip 字典来不及建立），但解压的 CPU 开销是实打实的。对于长推理输出（MiMo 可能产出 8K+ token），这是连续几十秒的额外 CPU 消耗。

### 修复方向

在 [smooth_upstream_client_headers](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/upstream_headers.rs#L30-L42) 中，当 `is_streaming = true` 时：

```rust
let encoding = if is_streaming {
    "identity"  // SSE 流不压缩
} else {
    UPSTREAM_ACCEPT_ENCODING  // 非流式保持 gzip
};
let _ = req.insert_header(header::ACCEPT_ENCODING, encoding);
```

### 预估收益

- **per-chunk 解压延迟**：每个 SSE chunk 节省 ~0.05–0.1ms
- **累积**：8K token 输出（~1000 SSE chunks）节省 **~50–100ms 总 CPU**
- **TTFT 无直接改善**，但释放 CPU 给其他并发请求

---

## 优化 D（中收益 · 低风险）：惰性 SHA-256 outbound fingerprint

### 问题

[request_filter.rs:607–611](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L607-L611)：

```rust
let outbound_fp: String = {
    let mut hasher = Sha256::new();
    hasher.update(new_body.as_ref());    // ← 700KB SHA-256
    let h = hex::encode(hasher.finalize());
    h[..h.len().min(8)].to_string()
};
```

这个 `outbound_fp` **仅用于 debug_agent_log**（L644），每个请求都对 700KB body 做完整 SHA-256 计算，耗时约 **~1–2ms**。

### 修复方向

只在 `debug_agent_log` 实际会输出时才计算：

```rust
let outbound_fp = if crate::is_debug_agent_log_enabled() {
    let mut hasher = Sha256::new();
    hasher.update(new_body.as_ref());
    let h = hex::encode(hasher.finalize());
    h[..h.len().min(8)].to_string()
} else {
    String::new()
};
```

或者更彻底：直接复用 `ctx.req_hash`（已经在 body read 阶段增量计算过）。`outbound_fp` 的目的是对比 inbound vs outbound body 是否变化，可以用 body length + `retired_prefix` 数量来替代。

### 预估收益：~1–2ms per request

---

## 优化 E（高收益 · 中风险）：`prepare_mimo_request` 短路

### 问题

[request_filter.rs:396–408](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L396-L408)：

```rust
let mimo = prepare_mimo_request(
    payload,
    &profile_fallback,
    features.mimo_retire_prefix_messages,
    features.mimo_keep_recent_turns,
);
// 即使什么都没改，也要 serde_json::to_vec
ctx.new_request_body = Some(Bytes::from(
    serde_json::to_vec(&mimo.payload).unwrap_or_default(),
));
```

当 `mimo_retire_prefix_messages = false` 且 model 无变化（大多数场景）时，`prepare_mimo_request` 只做了 `filter_fields`（移除 `max_tokens` 等几个字段），然后对整个 700KB payload 做 `serde_json::to_vec` 序列化。

但 `filter_fields` 只移除了顶层几个小字段，payload 的主体（`messages` 数组，占 99% 体积）完全没变。

### 修复方向

在 `prepare_mimo_request` 内部判断：如果仅移除了少量顶层字段且 model 没变，返回一个 `None`（表示"用原始 body 做 patch"）：

```rust
// 在 normalize.rs 中
pub fn prepare_mimo_request(...) -> MimoPreparedRequest {
    let removed = filter_fields(&mut payload);
    if retired == 0 && !model_changed && removed.is_empty() {
        return MimoPreparedRequest {
            payload_bytes: None,  // 调用方直接用 full_body
            ...
        };
    }
    // 只在有变化时才序列化
}
```

调用方：

```rust
if let Some(prepared_bytes) = mimo.payload_bytes {
    ctx.new_request_body = Some(prepared_bytes);
} else {
    // 直接用 full_body (Bytes clone = O(1))
}
```

> [!WARNING]
> 需要确保 `filter_fields` 移除的字段在 MiMo 上游不会引起 400 错误。如果 MiMo 对 `max_tokens` 等字段容错（忽略未知字段），可以考虑连 `filter_fields` 都跳过。

### 预估收益

- 当 retire_prefix 未触发时：省 ~3–5ms（跳过序列化）
- 加上优化 A 的联合效果：省 ~8–13ms

---

## 优化 F（中收益 · 低风险）：cache key 生成并行化

### 问题

[cache_coalesce.rs:38–54](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/cache_coalesce.rs#L38-L54)：

```rust
let cache_key_body = ctx.original_request_body.as_deref().expect("...");
// ...
let cache_key = crab_cache::generate_namespaced_cache_key_with_fingerprint(
    cache_key_body,       // ← 原始 body (700KB)
    cache_namespace,
    &fingerprint,
);
```

`generate_namespaced_cache_key_with_fingerprint` 内部对 700KB body 做 **SHA-256 哈希**。但 `ctx.req_hash` 在 body read 阶段（[request_filter.rs:1319–1329](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L1319-L1329)）**已经**增量计算过同一个 body 的 SHA-256。

这意味着同一个 body 被 SHA-256 了**两次**。

### 修复方向

让 `generate_namespaced_cache_key_with_fingerprint` 接受预计算的 hash：

```rust
pub fn generate_cache_key_from_prehash(
    body_hash: &str,       // 已有的 req_hash
    namespace: Option<&str>,
    fingerprint: &Fingerprint,
) -> String {
    // 直接用 body_hash 而不重新计算
}
```

### 预估收益：~1–2ms（700KB body 的 SHA-256 开销）

---

## 优化 G（中收益 · 低风险）：`response_body.rs` cache write 用 `mem::take`

### 问题

[response_body.rs:418](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/response_body.rs#L418)：

```rust
let sse_body = std::mem::take(&mut ctx.stream.client_sse_body);
```

> ✅ **已优化**：当前代码已用 `std::mem::take` 而非 `clone()`。无需改动。

---

## 优化 H（高收益 · 中风险）：streaming defer 更激进的阈值

### 现状（2026-05）

- 阈值已降为 **32 KiB**（[`request_filter.rs`](../crates/crab-proxy/src/phases/request_filter.rs) `MIN_STREAMING_DEFER_BYTES`）。
- **H2 已落地**：`try_defer_finalize_early_exact_cache` 在 EOS 对完整 body 做 L0/L1 exact lookup；命中则 `suppress_upstream` 返回缓存（upstream 可能已 idle 连接）。
- 安全门禁：`defer_body_incomplete`（Content-Length / truncated JSON）、circuit breaker、`skip_upstream_trailing_empty_eos` PATCH。

### 历史问题（已缓解）

defer 路径在 partial read 时无法生成 exact cache key，因此 **early handoff 阶段**仍跳过 Phase 5；完整 body 到达后在 `finalize_streaming_body` 再查缓存。

### 预估收益

- **32 KiB 阈值**：对 200–700KB body，upstream connect 更早与上传并行（典型 **~1–2s** wall clock，取决于上行带宽）。
- **EOS early exact**：defer 路径与正常路径共享 cache hit；idle upstream 连接为可接受浪费。

---

## 优化 I（低收益 · 极低风险）：减少 `debug_agent_log` 的 JSON 构建

### 问题

整个 `request_filter.rs` 有 **17 处** `debug_agent_log` 调用（通过 grep 确认），每处都构建一个 `serde_json::json!({...})`。即使 debug 输出关闭，`json!()` 宏仍然会执行完整的 JSON 构建。

### 修复方向

用 `if is_debug_agent_log_enabled()` 守护，或将 `debug_agent_log` 改为宏，惰性构建参数：

```rust
macro_rules! debug_agent_log {
    ($tag:expr, $source:expr, $msg:expr, $payload:expr) => {
        if $crate::is_debug_agent_log_enabled() {
            $crate::debug_agent_log_inner($tag, $source, $msg, $payload);
        }
    };
}
```

### 预估收益：~0.5–1ms（17 次 JSON Value 构建的累积开销）

---

## 优先级排序（按延迟影响从大到小）

| 优先级 | 优化项 | 节省 (700KB) | 风险 | 复杂度 |
|--------|--------|-------------|------|--------|
| **P0** | A: 消除第二次 JSON parse | ~5–8 ms | 低 | 低 |
| **P0** | E: prepare 短路 (无变化跳过序列化) | ~3–5 ms | 中 | 中 |
| **P1** | B: extract_composition 移到 logging | ~3–5 ms | 低 | 低 |
| **P1** | C: SSE 流禁用 upstream gzip | ~50–100 ms 总 CPU | 低 | 低 |
| **P1** | F: cache key 复用 req_hash | ~1–2 ms | 低 | 低 |
| **P2** | D: 惰性 outbound_fp SHA-256 | ~1–2 ms | 低 | 极低 |
| **P2** | I: debug_agent_log 惰性化 | ~0.5–1 ms | 低 | 低 |
| **P2** | H: streaming defer 阈值调优 | ~5–10 ms（仅 defer 路径） | 中 | 中 |

**P0 + P1 合计**：对 700KB MiMo body 的 request_filter 阶段，从 ~25ms 降至 ~10ms（**省 ~15ms gap 延迟**）。

---

## 全链路延迟瀑布（优化前 vs 优化后）

```
优化前 (700KB MiMo, MISS path):
├─ body read + SHA-256 ............. 3ms
├─ JSON parse (1st) ................ 5ms
├─ pipeline select ................. 1ms
├─ prepare_mimo_request ............ 3ms
│  └─ serde_json::to_vec ........... 3ms   ← 优化E可省
├─ extract_composition ............. 4ms   ← 优化B可省
├─ SHA-256 outbound_fp ............. 1.5ms ← 优化D可省
├─ JSON parse (2nd) ................ 5ms   ← 优化A可省
├─ cache key gen (SHA-256 again) ... 1.5ms ← 优化F可省
├─ L0/L1 cache lookup .............. 3ms
├─ upstream key acquire ............ 0.5ms
├─ TCP+TLS connect ................. 30-80ms (预热后 <5ms)
├─ body upload ..................... 5-15ms
├─ ─── 以上为 "gap" (首包前) ──── ~65-125ms
├─ upstream prefill (MiMo) ......... 3-15s
├─ TTFT (首 token) ................. 3-15s
├─ SSE decompress per chunk ........ 0.05ms × N ← 优化C可省
└─ total decode .................... 10-60s

优化后:
├─ body read + SHA-256 ............. 3ms
├─ JSON parse (1st) ................ 5ms
├─ pipeline select ................. 1ms
├─ prepare (short-circuit, no ser) . 1ms   ✂️ -5ms
├─ cache key gen (reuse hash) ...... 0.5ms ✂️ -1ms
├─ L0/L1 cache lookup .............. 3ms
├─ upstream key acquire ............ 0.5ms
├─ TCP+TLS connect ................. <5ms (预热)
├─ body upload ..................... 5-15ms
├─ ─── gap ──── ~24-34ms (vs 65-125ms)
├─ [composition → logging 阶段] .... 0ms 热路径 ✂️ -4ms
├─ upstream prefill (MiMo) ......... 3-15s (不变)
└─ SSE identity (无解压) ........... 0ms per chunk ✂️

总 gap 节省: ~15-20ms
总 CPU 节省: ~50-100ms (SSE 解压) + ~15ms (parse/hash)
```

---

## 与三线计划的关系

| 三线计划项 | 本文优化项 | 关系 |
|-----------|-----------|------|
| ① 上下文体积 | E (prepare 短路) | 互补：retire_prefix 开启时 E 不适用 |
| ② TTFT 指标 | A, B, D, F | 直接降低 gap 指标 |
| ③ streaming_body_forward | H (defer 阈值) | H 是 ③ 的参数调优 |
| 新增 | C (SSE identity) | 独立项，可立即实施 |
| 新增 | I (debug log 惰性) | 独立项，低优先级 |
