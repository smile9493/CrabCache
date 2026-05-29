# `streaming_body_forward` 安全改进方案

基于对完整代码链路的深度分析，识别出 **3 个根因 bug** 和 **2 个架构缺陷**。

---

## 问题根因分析

### 故障现象回顾

wuming 上出现过：
- `outbound_bytes=0`，MiMo 返回 `400 Param Incorrect`
- `Invalid JSON in request body` 日志（`streaming_defer=true`）
- 上游收到空 body 或截断 JSON

### Bug 1：finalize Handled 后，Pingora 仍发空 DATA 帧

**位置**：[upstream_request.rs:157–176](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/upstream_request.rs#L157-L176) + [proxy.rs:409–412](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/proxy.rs#L409-L412)

**链路**：

```
request_filter → streaming_deferred_handoff
  → ctx.streaming_body.active = true
  → ctx.upstream.retry_buffer_truncated = true
  → return Ok(false)  ← 告诉 Pingora "继续上游"

Pingora → upstream_peer → upstream_request_filter
  → set_send_end_stream(false)  ← 正确，不要发 END_STREAM
  → 无 Content-Length（deferred 模式）

Pingora → request_body_filter (中间 chunk)
  → body.take() → append_chunk → *body = None → return
  → Pingora 不发这些 chunk

Pingora → request_body_filter (EOS)
  → finalize_streaming_body
    → run_post_body_phases
      → JSON parse 失败 ← 或 cache hit
      → suppress_upstream = true
      → return Handled

  → request_body_filter 回到 L166:
    ctx.streaming_body.suppress_upstream = true;
    *body = None;
    return Ok(());  ← 问题在这里
```

**问题**：当 `finalize` 返回 `Handled` 时，`*body = None` 且 `return Ok(())`。Pingora 的 duplex loop 收到 `Ok(())` + `body=None` + `end_of_stream=true` 后，会向上游发送一个 **空的 END_STREAM DATA 帧**（H2）或空 chunked 尾（H1）。

对于 H1 路径（[proxy_h1.rs:318](file:///home/smile/github_project/CrabCache/third_party/pingora-proxy/src/proxy_h1.rs#L318)），`defer_upstream_request_body` 返回 true 时 Pingora 已经进入了 body pipe 模式。EOS 时 `body=None` 会被当作 "empty final chunk" 发送。

但此时 upstream 已连接，且 `upstream_request_filter` 里发的 header 是 **无 Content-Length**（[upstream_request.rs:36](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/upstream_request.rs#L36)：`prepare_streaming_deferred_upstream_headers`）。上游 MiMo 收到 headers + 空 body = `{}` parse error → 400。

> [!CAUTION]
> 这是最核心的 bug：**suppress_upstream 只抑制了后续 chunk，但 EOS 时的空 body 仍会到达上游**。Pingora 无法区分 "我不想发任何 body" 和 "发一个空的最终 chunk"。

### Bug 2：JSON parse 失败时 `new_request_body` 为 None

**位置**：[request_filter.rs:256–272](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L256-L272)

当 `finalize_streaming_body` → `run_post_body_phases` 中 JSON parse 失败时：

```rust
Err(parse_err) => {
    // ...
    if ctx.streaming_body.active {
        ctx.streaming_body.suppress_upstream = true;  // L269
    }
    return Ok(true);  // Handled
}
```

此时 `ctx.new_request_body` **仍然是 None**。回到 `request_body_filter` → `suppress_upstream = true` → `*body = None`。

但如果 Pingora 的 body pipe 已经启动（在 `send_body_to_pipe` 中），suppress 仅阻止新 chunk，无法撤回已建立的 upstream 连接。上游等待 body 超时或收到 RST。

### Bug 3：`streaming_deferred_handoff` 中的 `retry_buffer_truncated = true` 副作用

**位置**：[request_filter.rs:798](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L798)

```rust
ctx.upstream.retry_buffer_truncated = true;
```

设置 `retry_buffer_truncated = true` 是为了让 `request_body_filter` 在 EOS 时 emit body（而不是等 retry buffer）。但这个 flag 同时影响了 `should_emit_prepared_upstream_body`（[upstream_body.rs:10](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/upstream_body.rs#L10)）：

```rust
pub fn should_emit_prepared_upstream_body(end_of_stream: bool, retry_buffer_truncated: bool) -> bool {
    end_of_stream || retry_buffer_truncated
}
```

如果 `retry_buffer_truncated = true` 且 `request_body_filter` 被调用时 `end_of_stream = false`（中间 chunk），但 `new_request_body` 此时碰巧不为 None（比如前一次 finalize 部分执行后遗留），body 会**在 EOS 之前**就被发出去 —— 此时 body 不完整。

> [!WARNING]
> 这个场景在正常流程中不会触发（因为 streaming_body 模式下 `new_request_body` 直到 finalize 才设置），但如果代码有微小变更或异常退出，就是一个定时炸弹。

---

## 架构缺陷

### 缺陷 A：defer 路径无法中止上游连接

当 `finalize` 返回 `Handled`（cache hit 或 error）时，上游 TCP/TLS 连接**已建立**但不需要了。当前没有机制主动关闭这个连接。Pingora 会等到 upstream timeout 或 RST。

更严重的是：即使 `suppress_upstream = true`，Pingora 的 H1/H2 duplex loop 仍然会尝试读 upstream response。如果上游返回 400（因为空 body），这个 400 会和 `finalize` 发给客户端的 cached response / error response **竞争**。

### 缺陷 B：`finalize_streaming_body` 在 `request_body_filter` 内调用

`finalize` 做了大量工作（full JSON parse + prepare + cache lookup + coalesce），但它在 `request_body_filter` 回调内执行。这个回调在 Pingora 的 body pipe 中被调用，pipe 的 channel 有背压，但没有 "abort upstream" 的语义。

---

## 改进方案

### Phase 1：修复 3 个 Bug（安全门禁前置条件）

#### 1.1 Body 不为空断言 + 强制 suppress

**文件**：[upstream_request.rs](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/upstream_request.rs)

在 `run_request_body_filter` 的 `emit_now` 分支中，加入**最终防线**：

```rust
// L183-223 emit_now 分支末尾：
if emit_now && body.as_ref().is_some_and(|b| !b.is_empty()) {
    ctx.upstream.prepared_upstream_body_emitted = true;
} else if emit_now && ctx.streaming_body.active {
    // ★ 新增：defer 模式下 finalize 失败，body 为空
    // 绝不能让空 body 到达 upstream
    warn!(
        request_id = %ctx.request_id,
        "streaming defer: empty body at EOS, forcing suppress"
    );
    ctx.streaming_body.suppress_upstream = true;
    *body = None;
    global_metrics().record_defer_empty_body_suppressed();
    return Ok(());
}
```

同时，在 `suppress_upstream` 检查（L178–181）后面加一个 **EOS 空 body 守护**：

```rust
if ctx.streaming_body.suppress_upstream {
    *body = None;
    return Ok(());
}

// ★ 新增：即使 suppress 未设置，defer 模式下如果 finalize 没产出 new_request_body 且已 EOS，也不发
if ctx.streaming_body.active
    && ctx.streaming_body.finalized
    && ctx.new_request_body.is_none()
    && end_of_stream
{
    warn!(
        request_id = %ctx.request_id,
        "streaming defer finalized but no prepared body; suppressing upstream"
    );
    ctx.streaming_body.suppress_upstream = true;
    *body = None;
    return Ok(());
}
```

#### 1.2 `upstream_request_filter` 中为 defer 设置 Content-Length: 0 占位

**文件**：[upstream_request.rs:35–37](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/upstream_request.rs#L35-L37)

当前：

```rust
if ctx.streaming_body.active && ctx.new_request_body.is_none() {
    prepare_streaming_deferred_upstream_headers(upstream_request, ctx.is_streaming);
    upstream_request.set_send_end_stream(false);
}
```

**不改**。但在 `prepare_streaming_deferred_upstream_headers` 中加注释说明：**不设 Content-Length 是有意为之**（chunked transfer），MiMo 支持 chunked。真正的防线在 `request_body_filter` 的 1.1 守护。

#### 1.3 `streaming_deferred_handoff` 不再滥用 `retry_buffer_truncated`

**文件**：[request_filter.rs:798](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L798)

引入独立的标志位代替 piggyback：

```rust
// 新增到 StreamingBodyState:
pub streaming_defer_emit_at_eos: bool,

// streaming_deferred_handoff:
- ctx.upstream.retry_buffer_truncated = true;
+ ctx.streaming_body.streaming_defer_emit_at_eos = true;

// request_body_filter emit_now 判断:
let emit_now = end_of_stream
    || ctx.upstream.retry_buffer_truncated
+   || ctx.streaming_body.streaming_defer_emit_at_eos
    || (ctx.streaming_body.active && ctx.streaming_body.finalized);
```

这样 `retry_buffer_truncated` 只反映 Pingora 真实的 buffer 状态，不被 streaming defer 滥用。

> [!IMPORTANT]
> 需要同步修改 `defer_upstream_request_body`（[proxy.rs:409–412](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/proxy.rs#L409-L412)）：
> ```rust
> fn defer_upstream_request_body(&self, _session: &Session, ctx: &Self::CTX) -> bool {
> -   ctx.upstream.retry_buffer_truncated
> +   ctx.streaming_body.streaming_defer_emit_at_eos
>         || (ctx.streaming_body.active && !ctx.streaming_body.finalized)
> }
> ```

---

### Phase 2：门禁指标 + 自动启停

#### 2.1 新增 Prometheus 指标

```rust
// crab-metrics 新增：
gateway_streaming_defer_total              // defer 触发总数
gateway_streaming_defer_finalize_ok_total  // finalize 成功（ContinueUpstream）
gateway_streaming_defer_cache_hit_total    // finalize 返回 Handled（cache hit）
gateway_streaming_defer_parse_fail_total   // finalize 返回 Handled（JSON parse error）
gateway_streaming_defer_empty_body_total   // 1.1 守护触发（空 body 被抑制）
gateway_streaming_defer_suppress_total     // suppress_upstream 生效次数
```

#### 2.2 灰度门禁条件

在 `FeaturesConfig` 中新增：

```toml
[features]
streaming_body_forward = true
streaming_body_forward_auto_disable_threshold = 3  # 连续 N 次 parse_fail 或 empty_body 后自动关闭
```

运行时检查：

```rust
// 在 try_arm_streaming_defer_on_partial_body 入口：
if proxy.state.streaming_defer_circuit_breaker.is_open() {
    return false;  // 熔断，回退到同步模式
}
```

Circuit breaker 逻辑：
- 连续 `N` 次 `parse_fail` 或 `empty_body` → 打开（禁用 defer）
- 打开后 60s 尝试 half-open（放过 1 个请求）
- half-open 成功 → 关闭（恢复 defer）

#### 2.3 灰度验证命令

```bash
# 开启后，观察 5 分钟内这些指标是否为 0：
ssh wuming 'curl -s http://127.0.0.1:9090/metrics' | grep streaming_defer
# 期望：
# gateway_streaming_defer_parse_fail_total 0
# gateway_streaming_defer_empty_body_total 0
# gateway_streaming_defer_finalize_ok_total > 0
```

---

### Phase 3：架构改进（可选，收益递减）

#### 3.1 defer 路径加入 early exact cache

当前 [request_filter.rs:204](file:///home/smile/github_project/CrabCache/crates/crab-proxy/src/phases/request_filter.rs#L204)：

```rust
let skip_early_exact = skip_early_pipeline_select || ctx.streaming_body.active;
```

`streaming_body.active` 时跳过 early exact cache 是因为此时 body 不完整，无法生成 cache key。

**改进**：在 `finalize_streaming_body` 中（body 完整后），先做一次 early exact cache 查询，**再**做完整的 `run_post_body_phases`：

```rust
pub(crate) async fn finalize_streaming_body(...) {
    // ... body assembly ...
    let full_body = Bytes::from(std::mem::take(&mut ctx.streaming_body.buffer));
    
    // ★ 新增：early exact cache（在 full JSON parse 之前）
    let mut hasher = Sha256::new();
    hasher.update(full_body.as_ref());
    let hash = hex::encode(hasher.finalize());
    if let Ok(key) = generate_cache_key_from_hash(&hash, namespace, fingerprint) {
        if proxy.try_early_exact_cache(session, ctx, &key, display_reasoning).await? {
            ctx.streaming_body.suppress_upstream = true;
            return Ok(StreamingFinalizeOutcome::Handled);
        }
    }
    
    // ... run_post_body_phases ...
}
```

**收益**：defer 路径的 cache hit 不需要做 full JSON parse（省 ~5ms），同时避免了浪费已建立的上游连接。

#### 3.2 `finalize` 返回 abort 语义

将 `StreamingFinalizeOutcome` 从 2 态扩展为 3 态：

```rust
pub enum StreamingFinalizeOutcome {
    /// Response already sent, suppress upstream. Close upstream connection if idle.
    Handled,
    /// Prepared body ready, caller should emit via request_body_filter.
    ContinueUpstream,
    /// Fatal error, abort the entire request (don't even try to send to upstream).
    Abort,
}
```

`Abort` 时 `request_body_filter` 返回一个 Pingora Error，触发 `fail_to_proxy` → 干净地断开上下游连接。比 `suppress_upstream + body=None` 更安全。

---

## 实施优先级

| Phase | 内容 | 收益 | 复杂度 | 何时做 |
|-------|------|------|--------|--------|
| **1** | 3 个 bug fix + 空 body 守护 | **消除 400 根因** | 低 | 立即 |
| **2** | 门禁指标 + 熔断器 | 安全灰度 | 中 | Phase 1 后 |
| **3** | early cache + abort 语义 | defer 路径也享受 cache | 中 | Phase 2 验证通过后 |

---

## Open Questions

> [!IMPORTANT]
> **Q1**：Phase 1.3 中把 `retry_buffer_truncated` 换成独立 flag，是否需要同步修改 Pingora 的 `proxy_h1.rs:318` 和 `proxy_h2.rs:320` 中的判断？目前 `defer_upstream_request_body` 覆盖了这个语义，但 Pingora 内部还有直接检查 `retry_buffer_truncated()` 的地方。

> [!IMPORTANT]
> **Q2**：Phase 3.2 的 `Abort` 态 —— Pingora 的 `request_body_filter` 返回 `Err(...)` 时的行为是否符合预期？需要确认 Pingora 是否会干净地关闭 upstream 连接而不是 panic 或 hang。

> [!WARNING]
> **Q3**：Phase 1.1 中的空 body 守护在 H2 场景下是否有效？H2 的 DATA 帧可能在 `request_body_filter` 返回 `Ok(())` 之前就已经被 Pingora 的 writer task 发出。如果是这样，需要在 `upstream_request_filter` 阶段就设置一个 "defer body pending" 的 flag，让 Pingora 的 writer 等待直到 finalize 完成。

## Verification Plan

### Automated Tests

```bash
# 现有单元测试
cargo test -p crab-proxy streaming_body_forward

# 新增：finalize 失败后 suppress 防空 body 测试
# 新增：circuit breaker 熔断/恢复测试
# 新增：retry_buffer_truncated 独立 flag 回归测试
```

### 灰度步骤

1. Phase 1 修复后：`make hot-update`，在 crabcache-deploy 上跑 10 分钟，观察 `streaming_defer_empty_body_total = 0`
2. 确认后推 wuming：`python3 scripts/hot_update.py --target wuming`
3. 开启 `streaming_body_forward = true`
4. 观察 `gap` p50 是否 ≤3.5s，`parse_fail_total = 0`，`empty_body_total = 0`
5. 连续 24h 无告警后，宣布 defer 路径稳定
