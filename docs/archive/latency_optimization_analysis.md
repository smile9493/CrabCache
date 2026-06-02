# MiMo v2.5-pro 全链路延迟优化方案

> 基于 79 个请求的实测数据（body 450~560KB，E2E ~16s）和代码审查。

---

## 延迟瀑布：现状一览

```
客户端请求  ──────────────────────────────────────────────────────────── 响应完成
│                                                                        │
├─ 0.00s  请求到达网关                                                    │
├─ 0.30s  pipeline_select_done (JSON 快解析 + 管线选择)                    │
├─ 0.30s  upstream_connect_done (连接池已有，~0ms)                         │
├─ 0.49s  upstream_headers_sent (发送请求头)                               │
│         ╔═══════════════════════════════════╗                           │
│         ║  6.10s  读客户端 body (450~560KB) ║ ← 最大瓶颈                 │
│         ╚═══════════════════════════════════╝                           │
├─ 6.59s  body_read_done                                                  │
├─ 6.59s  upstream_body_sent (~0.01s, 即发即完)                            │
│         ╔════════════════════════════╗                                  │
│         ║  3.96s  MiMo Prefill TTFT ║                                  │
│         ╚════════════════════════════╝                                  │
├─10.55s  upstream_response_headers (首 token)                            │
│         ╔═══════════════════════════════╗                               │
│         ║  5.33s  MiMo 流式生成        ║                                │
│         ╚═══════════════════════════════╝                               │
├─15.88s  upstream_body_done                                              │
├─15.95s  logging_done (0.07s)                                            │
└─~16.0s  E2E                                                            │
```

| 环节 | 耗时 | 占比 | 可优化性 |
|------|------|------|----------|
| **客户端→网关 body 传输** | **6.1s** | **38%** | ⭐⭐⭐ 网关侧可大幅改善 |
| MiMo 生成 (streaming) | 5.3s | 33% | ⭐ 服务端控制 |
| MiMo Prefill (TTFT) | 4.0s | 25% | ⭐⭐ 可提前触发 |
| 网关处理开销 | 0.6s | 4% | ⭐ 已很低 |

---

## 核心洞察

> [!IMPORTANT]
> **最大杠杆点不是优化网关 CPU 开销（已经只有 0.6s / 4%），而是消除 body 传输 (6.1s) 与 upstream prefill (4.0s) 之间的串行等待。**

当前流程是：**先读完全部 body → 再发给 MiMo → MiMo 开始 prefill**。如果能让 body 边读边发（streaming body forward / request passthrough），prefill 可以提前 ~6s 开始。

---

## 优化策略总览

| 编号 | 优化项 | 目标环节 | 预估节省 | 风险 | 优先级 |
|------|--------|----------|----------|------|--------|
| **S1** | Request Passthrough 扩大覆盖 | body 传输 + prefill 重叠 | **~4–6s** | 中 | **P0** |
| **S2** | 客户端 body 压缩 (gzip) | body 传输 | **~3–5s** | 低 | **P0** |
| **S3** | MiMo context 精简 (retire_prefix) | body 传输 + prefill | **~2–4s** | 中 | **P1** |
| **S4** | Chunked Transfer-Encoding 优化 | body 传输 | **~0.5–1s** | 低 | **P1** |
| S5 | SSE 流禁用 upstream gzip | streaming 阶段 CPU | ~50–100ms | 低 | P2 |
| S6 | 网关 CPU 热路径优化 (已有方案) | 网关开销 | ~15–20ms | 低 | P2 |
| S7 | HTTP/2 多路复用 (客户端侧) | body 传输 | ~0.5–1s | 低 | P2 |
| S8 | Prefill 批量/投机 (服务端) | prefill TTFT | ~1–2s | 高 | P3 |

---

## S1: Request Passthrough 全量覆盖 MiMo 请求 ⭐⭐⭐

### 现状

代码已实现 **request passthrough** 路径（[request_filter.rs:1014-1061](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L1014-L1061)）：

- 在读到 ≥1024 字节、识别出 `"model"` 后，立即进入 passthrough 模式
- 网关向 MiMo 发送 chunked body：partial prefix 先发，后续 chunk 边读边转发
- `request_body_filter` 中逐 chunk 透传（[upstream_request.rs:221-275](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/upstream_request.rs#L221-L275)）

但触发条件限制了覆盖率：

```rust
// request_filter.rs:794-795
const MIN_PASSTHROUGH_PREFIX_BYTES: usize = 1024;
// ...
if session.is_body_done() || partial_body.len() < MIN_PASSTHROUGH_PREFIX_BYTES {
    return false;
}
```

**关键约束**：只在 `mimo_direct_passthrough(pipeline)` 时触发（[request_filter.rs:839](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L839)），且跳过了 cache / composition / reasoning 等阶段。

### 优化方向

1. **确认 passthrough 在 MiMo 管线已正常运行**：检查 metrics `request_passthrough_total` 是否与 MiMo 请求量匹配
2. **降低 `MIN_PASSTHROUGH_PREFIX_BYTES`**：450KB+ 的 body 在首次 `read_request_body` 就应该返回多个 chunk，1024 字节已很保守
3. **如果 passthrough 命中率低**，排查原因：
   - `session.is_body_done()` 在第一次 read 就为 true → 客户端使用 Content-Length 且一次性发送
   - `streaming_body_forward` feature flag 未开启

### 预估收益

如果 passthrough 正常工作，body 传输与 upstream 连接/发送**完全重叠**：

```
优化前 (串行):
├─ 6.1s body read → 0.01s body sent → 3.96s prefill → 5.33s gen
│  E2E = 15.4s

优化后 (passthrough 重叠):
├─ body read 开始
├─ ~0.1s 后 prefix 发出 → upstream 开始接收
├─ 6.1s body 边读边发 (upstream 同步接收)
├─ body done → upstream 立即开始 prefill (已收完)
├─ 3.96s prefill → 5.33s gen
│  E2E = 15.4s (看起来没变？)
```

> [!IMPORTANT]
> **真正的收益在于 prefill 的提前触发**：当 passthrough 运行时，upstream 在 body 到达后 **0.01s** 就能开始 prefill（`upstream_body_sent` 与 `body_read_done` 几乎同步）。但如果 MiMo 支持 **部分 body 即可开始 prefill**（incremental prefill / streaming input），则首 token 可以提前 **~4–6s**。

对于 MiMo 模型，如果上游 API 在收到完整 JSON body 后才开始 prefill，passthrough 主要消除的是：
- `body_read_done` 到 `upstream_body_sent` 之间的网关处理延迟（0.01s，已很小）
- 但如果没有 passthrough，这个间隔是 `body_read_done` → JSON parse → pipeline → prepare → cache → key acquire → connect → send，可能需要 **50-200ms**

### 风险

- Passthrough 跳过 cache/coalesce，cache 命中率不受影响（MiMo body 通常 unique）
- 需确认 MiMo 上游支持 chunked Transfer-Encoding

---

## S2: 客户端 body 压缩 (gzip) ⭐⭐⭐

### 问题

450–560KB 的请求 body 需要 **6.1s** 才能传到网关，平均传输速率仅 **~75–90 KB/s**。这说明瓶颈在客户端上行带宽或中间网络。

### 优化方向

1. **客户端发送 gzip 压缩的 body**：
   - LLM chat 请求的 JSON body 压缩率通常 **5:1 ~ 10:1**
   - 560KB body 压缩后约 **56–112KB**
   - 按 90 KB/s 传输速率计算，传输时间从 6.1s 降至 **~0.6–1.2s**

2. **网关侧接收解压**：
   - 在 `request_filter` 的 body read 循环中检测 `Content-Encoding: gzip`
   - 使用 `flate2::read::GzDecoder` 流式解压
   - 解压 CPU 开销极低（~1–5ms for 560KB）

3. **网关已支持 upstream gzip 发送**：
   - [upstream_request.rs:66-73](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/upstream_request.rs#L66-L73) 已有 `maybe_gzip_request_body`
   - 双向压缩链路：客户端→网关 gzip + 网关→MiMo gzip

### 代码参考

当前网关 body read 循环（[request_filter.rs:1608-1663](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L1608-L1663)）：

```rust
loop {
    match session.downstream_session.read_request_body().await? {
        Some(data) => {
            // 这里需要增加 gzip 解压逻辑
            hasher.update(&data);
            full_body.extend_from_slice(&data);
        }
        None => break,
    }
    // ... passthrough / streaming defer arming
}
```

### 实现要点

```rust
// 在 loop 开始前检测
let client_gzip = session.req_header().headers
    .get(http::header::CONTENT_ENCODING)
    .and_then(|v| v.to_str().ok())
    .map(|s| s.contains("gzip"))
    .unwrap_or(false);

// 如果是 gzip，使用流式解压器
if client_gzip {
    let mut decoder = flate2::read::GzDecoder::new(data.as_ref());
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    full_body.extend_from_slice(&decompressed);
}
```

### 预估收益

| 压缩率 | 传输后大小 | 传输时间 | 节省 |
|--------|-----------|----------|------|
| 5:1 | ~110KB | ~1.2s | **~4.9s** |
| 8:1 | ~70KB | ~0.8s | **~5.3s** |
| 10:1 | ~56KB | ~0.6s | **~5.5s** |

> [!CAUTION]
> 需要客户端配合发送 `Content-Encoding: gzip`。如果客户端是 Cursor 等第三方客户端，需要在客户端配置中启用。

---

## S3: MiMo Context 精简 (retire_prefix) ⭐⭐

### 现状

`prepare_mimo_request` 支持 `retire_prefix_messages` 功能（[request_filter.rs:428-439](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L428-L439)），可以裁剪历史消息，减少 body 体积。

```rust
let mimo = prepare_mimo_request(
    payload,
    &profile_fallback,
    features.mimo_retire_prefix_messages,  // ← 开关
    features.mimo_keep_recent_turns,       // ← 保留最近 N 轮
);
```

### 优化方向

1. **启用 `mimo_retire_prefix_messages`**：保留最近 5–10 轮对话，截断更早的历史
2. **评估 body 体积降幅**：
   - 典型 Cursor 编程会话的 messages 数组包含大量历史代码 context
   - 裁剪后 body 从 560KB 可能降至 100–200KB
3. **联合 S2 的效果**：200KB body + gzip 10:1 = ~20KB → 传输 **~0.2s**

### 预估收益

| 场景 | Body 大小 | 传输时间 | 总节省 |
|------|-----------|----------|--------|
| 裁剪 50% | ~280KB | ~3.1s | ~3.0s |
| 裁剪 70% + gzip | ~17KB | ~0.2s | ~5.9s |
| 无裁剪 + gzip | ~56KB | ~0.6s | ~5.5s |

> [!WARNING]
> 裁剪历史消息可能影响 MiMo 的推理质量。需要 A/B 测试评估质量损失。

---

## S4: 客户端 Chunked Transfer-Encoding 优化

### 问题

如果客户端使用 `Content-Length` 头发送请求，某些 HTTP 栈会**先序列化完整 body 再发送**，增加延迟。而 `Transfer-Encoding: chunked` 允许边构造边发送。

### 现状

网关已处理了两种模式：
- `inbound_content_length`（[streaming_body_forward.rs:176-183](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/streaming_body_forward.rs#L176-L183)）用于 defer 判断
- Passthrough 模式下使用 chunked 转发（[upstream_headers.rs](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/upstream_headers.rs) `prepare_passthrough_upstream_headers`）

### 优化方向

确认客户端 SDK 配置：
- Python `requests` 库默认用 Content-Length → 一次性发送
- `httpx` 支持 streaming upload → chunked 发送
- Cursor 客户端的 HTTP 库是否支持 chunked

---

## S5: SSE 流禁用 Upstream gzip

已在 [latency_optimizations.md](file:///home/smile/github_project/CrabCache/latency_optimizations.md#L127-L163) 中详细描述（优化 C）。

**要点**：streaming 请求对 MiMo 设置 `Accept-Encoding: identity`，避免无谓的 gzip 解压开销。

**预估收益**：释放 ~50–100ms 总 CPU（8K token 输出）。

---

## S6: 网关 CPU 热路径优化

已在 [latency_optimizations.md](file:///home/smile/github_project/CrabCache/latency_optimizations.md) 中详细描述（优化 A–I）。

**关键项**：
- 消除第二次 JSON parse（~5–8ms）
- `prepare_mimo_request` 短路（~3–5ms）
- `extract_composition` 惰性化（~3–5ms）
- Cache key 复用 req_hash（~1–2ms）

**总计**：对网关 0.6s 开销中的 ~15–20ms（约 3%），在 16s E2E 中占比极小。

> [!NOTE]
> 网关 CPU 优化的收益在单请求延迟上微不足道（<< 1%），但在高并发下对吞吐量有帮助。

---

## S7: HTTP/2 多路复用 (客户端→网关)

### 问题

如果客户端→网关链路使用 HTTP/1.1，每个请求独占一条 TCP 连接。HTTP/2 的多路复用和头压缩可以减少连接建立开销。

### 现状

网关 Pingora 支持 HTTP/2 下行。需确认客户端是否使用 HTTP/2。

### 预估收益

- 连接复用：~0.5–1s（首次请求的 TLS 握手）
- 后续请求：几乎为 0（连接已建立）

---

## S8: MiMo Prefill 优化 (服务端)

### 问题

MiMo prefill 3.96s（25%）是纯服务端开销，取决于：
- 输入 token 数量（450–560KB JSON body → ~100K–150K tokens）
- GPU 算力（prefill 是 compute-bound）
- KV cache 命中率

### 可能优化

1. **KV cache 复用**：如果 MiMo 支持 prompt caching，相同 prefix 的请求可以复用 KV cache，将 prefill 从 4s 降至 ~0.5s
2. **Speculative decoding**：用小模型预测 token，大模型验证，加速 generation 阶段
3. **Context 压缩**：在网关侧用摘要替代历史消息（与 S3 互补）

> 这些需要 MiMo 服务端支持，网关侧无法单方面实施。

---

## 综合收益预估

### 场景 A：仅网关侧优化（无需客户端改动）

| 优化 | 当前 | 优化后 | 节省 |
|------|------|--------|------|
| S1 passthrough 覆盖 | body 6.1s → send 0.01s → prefill | body+send 重叠 | ~0.05s |
| S5 SSE identity | N/A | 无解压 | ~0.1s CPU |
| S6 CPU 热路径 | 0.6s gateway | ~0.58s | ~0.02s |
| **合计** | **~16.0s** | **~15.8s** | **~0.2s** |

> 仅网关侧优化效果有限，因为 6.1s body 传输是网络瓶颈。

### 场景 B：客户端配合 gzip 压缩

| 优化 | 当前 | 优化后 | 节省 |
|------|------|--------|------|
| S2 client gzip (8:1) | body 6.1s | ~0.8s | **~5.3s** |
| S1 passthrough | send delay ~0.05s | 0s | ~0.05s |
| **合计** | **~16.0s** | **~10.6s** | **~5.4s (-34%)** |

### 场景 C：全量优化（gzip + context 精简）

| 优化 | 当前 | 优化后 | 节省 |
|------|------|--------|------|
| S2 + S3 (gzip + retire 50%) | body 6.1s | ~0.4s | **~5.7s** |
| S3 prefill 减少 | prefill 4.0s | ~2.5s | **~1.5s** |
| S1 + S5 + S6 | misc | misc | ~0.2s |
| **合计** | **~16.0s** | **~8.7s** | **~7.3s (-46%)** |

---

## 实施路线图

### Phase 1 — 确认现状（1 天）

- [ ] 检查 `request_passthrough_total` 指标，确认 passthrough 在 MiMo 请求上的命中率
- [ ] 检查 `streaming_body_forward` feature flag 是否已开启
- [ ] 测量客户端上行带宽（`body_read_done - body_read_start` 已有 timeline）
- [ ] 确认客户端是否使用 HTTP/1.1 还是 HTTP/2

### Phase 2 — 客户端 gzip（2–3 天，最高 ROI）

- [ ] 网关侧实现 `Content-Encoding: gzip` 解压（request body 解压）
- [ ] 测试 passthrough 模式下的 gzip 解压兼容性
- [ ] 通知客户端团队启用 gzip 请求体压缩
- [ ] A/B 测试验证延迟降幅

### Phase 3 — Context 精简（3–5 天）

- [ ] 评估 `mimo_retire_prefix_messages` 对推理质量的影响
- [ ] 设置合理的 `mimo_keep_recent_turns` 值
- [ ] 监控 body 体积和 prefill 时间的变化

### Phase 4 — 微优化（持续）

- [ ] SSE identity encoding
- [ ] 网关 CPU 热路径优化（参考 [latency_optimizations.md](file:///home/smile/github_project/CrabCache/latency_optimizations.md)）

---

## 关键结论

> [!IMPORTANT]
> 1. **6.1s 的 body 传输是最大瓶颈**（38%），但这是**网络带宽问题**，不是网关 CPU 问题
> 2. **最高 ROI 的优化是客户端 gzip**（S2）：可节省 ~5s，需客户端配合
> 3. **网关侧 passthrough 已实现**但需确认覆盖率（S1）
> 4. **网关 CPU 开销仅 0.6s（4%）**，继续微优化收益递减
> 5. **MiMo prefill 4s + generation 5.3s 共 59%**，需要服务端（MiMo）侧的改进才能进一步突破
