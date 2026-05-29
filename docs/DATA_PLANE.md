# 数据面实现状态（P0–P3）

本文档是 [数据面优化.md](../数据面优化.md) 的**交付对照表**：说明哪些建议已落地、哪些仍为展望、与正式文档/代码的权威行为差异。

**最后对齐提交**：`69057bf`（P2 验收 + 测试收尾）；此前 `48082e1`、`fff766f`、`1f9ed97`。

---

## 快速导航

| 文档 | 用途 |
|------|------|
| [数据面优化.md](../数据面优化.md) | 完整改进展望、优先级矩阵、附录（拆分建议） |
| [DATA_PLANE_P3.md](./DATA_PLANE_P3.md) | P3 实验项设计与回滚（差分缓存、WASM、io_uring） |
| [OBSERVABILITY.md](./OBSERVABILITY.md) | Prometheus / Admin / 阶段耗时指标 |
| [DEEPSEEK_PREFIX_CACHE.md](./DEEPSEEK_PREFIX_CACHE.md) | L3 上游前缀缓存 + **L0 prefix 索引** |
| [../third_party/pingora-proxy/PATCH.md](../third_party/pingora-proxy/PATCH.md) | Fork：`Arc<Connector>` 与直预热 |
| [../CLAUDE.md](../CLAUDE.md) | 特性开关、`[features]` 配置摘要 |
| [./DATA_PLANE_ACCEPTANCE.md](./DATA_PLANE_ACCEPTANCE.md) | P2 验收手册（MiMo/DeepSeek 双线路径 + Prometheus 核对） |
| [./OPS_RUNBOOK.md](./OPS_RUNBOOK.md) | 运行时 Trace 对照、P0 事故处置、非数据面遗漏项 |
| [../config/gateway.example.toml](../config/gateway.example.toml) | `[features]` 注释 |

---

## 实现状态总表

图例：**✅ 已落地** · **🟡 部分** · **⬜ 未做** · **📋 仅设计**

### 一、请求处理管道

| 项 | 状态 | 说明 / 代码 |
|----|------|-------------|
| `original_request_body` → `Bytes` | ✅ | [`context.rs`](../crates/crab-proxy/src/context.rs) |
| 增量 SHA-256 | ✅ | [`proxy.rs`](../crates/crab-proxy/src/proxy.rs) `read_request_body` 循环 |
| Raw capture 复用 `Arc<Value>` | ✅ | `parsed_request_payload` / `parsed_upstream_payload` |
| `body_quick_parse` + MiMo early exact cache | ✅ | [`body_quick_parse.rs`](../crates/crab-proxy/src/body_quick_parse.rs)；命中 exact 前可跳过 full parse；miss 后仍 parse + `prepare_mimo`；单测覆盖边界用例 |
| 首 chunk 选 pipeline（无全量 body） | ⬜ | 仍需读满 body（`max_request_body_bytes`） |
| `streaming_body_forward` | ✅ | MiMo partial read + EOS finalize；见 [STREAMING_BODY_FORWARD.md](STREAMING_BODY_FORWARD.md)，默认 off |
| MiMo 近似缓存键（sfp + msg count） | ⬜ | 精确键仍为 body SHA-256 |

### 二、SSE 流式

| 项 | 状态 | 说明 / 代码 |
|----|------|-------------|
| `SseEvent<'a>` + memchr 行切分 | ✅ | [`sse.rs`](../crates/crab-proxy/src/sse.rs) |
| `SsePipeline` 抽象 | ✅ | [`sse_pipeline/`](../crates/crab-proxy/src/sse_pipeline/) |
| `upstream_response_body_filter` 瘦身 | ✅ | 流式逻辑在 `sse_pipeline/`；外围状态机在 [`phases/response_body.rs`](../crates/crab-proxy/src/phases/response_body.rs) |
| `sse_rewrite` SIMD remainder | ⬜ | 仍为逐字节扫描 |
| 环形缓冲 + 异步流式写缓存 | ⬜ | EOS 后 `tokio::spawn` put，无背压环 |

### 三、缓存

| 项 | 状态 | 说明 / 代码 |
|----|------|-------------|
| MiMo 默认 prefix-aware L0 | ✅ | `prefix_aware_cache` 或 MiMo pipeline |
| Prefix 命中行为 | ✅ | **仅** `update_prefix_index` + 指标，**不**短路返回缓存体（与旧版展望文不同） |
| `gateway_prefix_index_warmup_total` | ✅ | [`registry.rs`](../crates/crab-metrics/src/registry.rs) |
| L3 affinity 反馈（hint + finalize） | ✅ | `affinity_prompt_cache_feedback`；连续 3 次 pure miss 才 invalidate |
| 差分缓存 | 📋 | [`DATA_PLANE_P3.md`](./DATA_PLANE_P3.md) |
| 自适应 TTL | ⬜ | 仍为 model/consumer 静态 TTL |

### 四、路由与连接

| 项 | 状态 | 说明 / 代码 |
|----|------|-------------|
| 连接池直预热（TCP+TLS，无 HTTP） | ✅ | [`connection_prewarm.rs`](../crates/crab-proxy/src/connection_prewarm.rs)；启动 + 新 `session_fingerprint` 在 `upstream_peer` |
| `select_with_hint` 真实 weight | ✅ | [`ring.rs`](../crates/crab-route/src/ring.rs) |
| 自适应 Ketama 权重 | ⬜ | 仅 health check 降权（Pingora LB） |
| HTTP/2 上游多路复用 | ✅ | 默认 `upstream_force_http1 = false`；`h2_ping_interval_secs` |
| 上游响应 gzip 协商 + 解压 | ✅ | `UPSTREAM_ACCEPT_ENCODING` + `upstream_response_decompress.rs` |
| 上游请求 gzip（可选） | ✅ | `[features] upstream_request_gzip` + `upstream_body_compress.rs`（默认关） |

### 五、可观测性

| 项 | 状态 | 说明 / 代码 |
|----|------|-------------|
| `gateway_request_phase_latency_seconds` | ✅ | `body_read_start` … `logging_done` |
| `record_request_body_stage` | ✅ | `json_parse_client` / `prepare_upstream_body` 等 |
| Raw capture 异步化 / logging &lt;1ms | ⬜ | 仍为 writer 线程 + 同步通道 |
| OpenTelemetry | ⬜ | 仅 `x-request-id` |

### 六、长期 / P3

| 项 | 状态 | 文档 |
|----|------|------|
| io_uring | 📋 | DATA_PLANE_P3 |
| WASM filters | 📋 | DATA_PLANE_P3 |
| 多模态 body / 嵌入 | 📋 | DATA_PLANE_P3 |
| 请求优先级队列 | ⬜ | 仅 `request_semaphore` |
| `proxy.rs` → `phases/` 拆分 | ✅ | [`phases/`](../crates/crab-proxy/src/phases/)：`request_filter`、`cache_coalesce`、`upstream_peer`、`upstream_request`、`response_filter`、`response_body`、`logging`；[`proxy.rs`](../crates/crab-proxy/src/proxy.rs) ~566 行（helpers + 瘦 `ProxyHttp` 委托） |

---

## Feature 开关（`[features]`）

| 开关 | 默认 | 行为摘要 |
|------|------|----------|
| `prefix_aware_cache` | off | L0 前缀索引；MiMo 三管道**默认等效开启** |
| `connection_prewarm` | off | 共享 `Connector` 直预热；需 fork 注入，见 PATCH.md |
| `affinity_prompt_cache_feedback` | off | 请求末根据 `prompt_cache_*` 更新 Ketama hint |
| `streaming_body_forward` | off | MiMo partial read + EOS finalize（[STREAMING_BODY_FORWARD.md](STREAMING_BODY_FORWARD.md)） |
| `delta_cache` / `wasm_filters` / `io_uring_backend` | off | P3，见 DATA_PLANE_P3 |

---

## 与《数据面优化.md》的差异（必读）

以下段落若以展望原文为准会**误解现网行为**，以本表与代码为准：

1. **§3.2 Prefix Phase 1**：不再在 prefix L0 命中时 `send_cached_response` 短路；只建 `prefix_index` 并继续上游（见 [DEEPSEEK_PREFIX_CACHE.md](./DEEPSEEK_PREFIX_CACHE.md)#l0-prefix-aware-网关侧)）。
2. **§4.2 连接预热**：已改为 Pingora **同池** `get_http_session` / `release_http_session`，非 loopback `GET /v1/models`。
3. **§1.1 内存表**：`Vec` clone 与全量双份 body 问题已用 `Bytes` + 增量哈希缓解；prepare 路径仍有 `to_vec` 序列化。

---

## 部署注意：pingora-proxy fork

`connection_prewarm` 依赖仓库内 [`third_party/pingora-proxy`](../third_party/pingora-proxy/)：

1. `HttpProxy` 持有 `Arc<Connector<C>>`。
2. `crab-gateway` 在 `Service::new` **之前**执行 `*state.upstream_connector.write() = Some(proxy.connector_arc())`。
3. 运行时预热使用与真实请求相同的 `HttpPeer`（`create_upstream_peer`：ALPN/TLS/keepalive）。

未使用 workspace fork 时，预热无法与业务流量共享连接池。

---

## 建议验收（手工 / 集成）

| 场景 | 预期 |
|------|------|
| MiMo 重复 exact key | 第二次请求在 full parse 前可 early cache 返回 |
| 新 `session_fingerprint` | `upstream_peer` 后 spawn prewarm；Prometheus 无额外上游 HTTP |
| `affinity_prompt_cache_feedback` | 有 hit token 的请求末写入 hint；连续 3 chunk pure miss 后 hint 清除 |
| Prefix 共享前缀 | `gateway_prefix_index_warmup_total` 增加，响应仍来自上游 |
| `select_with_hint` + weight≠1 |  hinted 后端可被 `ready()` 选中 |

自动化：单元测试见 `crab-proxy`（`body_quick_parse`、`metrics_helpers` affinity）、`crab-route`（`select_with_hint`）；集成测试见 [`crates/crab-gateway/tests/data_plane.rs`](../crates/crab-gateway/tests/data_plane.rs)。验收手册见 [`DATA_PLANE_ACCEPTANCE.md`](./DATA_PLANE_ACCEPTANCE.md)。

---

## 运行时分析对照（2026-05 内测）

完整处置步骤见 **[OPS_RUNBOOK.md](./OPS_RUNBOOK.md)**；一页纸结论见 **[RUNTIME_LOG_FINDINGS.md](./RUNTIME_LOG_FINDINGS.md)**。下表说明：**继续推进数据面能否明显改善该问题**。

| 报告现象 | 数据面（已做 + 文档剩余） | 明显改善？ | 更应优先 |
|----------|---------------------------|------------|----------|
| MiMo 429 / 单上游 Key | 预热、early cache △；近似键 ⬜ | **否** | 加 Key、降 Cursor 并行 |
| 命中率 ~25% | Prefix 索引 ✅（不短路）；L2 ⬜/关 | **部分** | 启用 L2、减 context；勿对标 98% |
| MISS P99 数秒+ | 阶段指标 ✅；SSE 异步写 ⬜ | **否** | 上游与 body 体量 |
| Coalescing Follower 502 | 设计行为 | **否**（除非改产品策略） | 降并发；近似键 ⬜ |
| ConnectionClosed | 预热 ✅ | **部分** | 限流/TE/CL，见 CURSOR_SETUP |
| Trace 截断 / Redis 启动 | 不在数据面 | **否** | OBSERVABILITY、PERSISTENCE |
| DeepSeek L3 prompt cache 高 | affinity 反馈 ✅ | **是**（DeepSeek 线） | 与 MiMo 429 无关 |

### 数据面 ROI 排序（在内测场景下）

1. **启用 L2 语义缓存**（配置 `[semantic]`，见 `gateway.example.toml`）  
2. **MiMo 近似缓存键**（§1.3 折中，⬜）— 利于 Coalescing，缓解高峰上游压力  
3. **`streaming_body_forward`**（P3）— 大 body 内存与读 body 延迟  
4. 差分缓存 / io_uring / WASM — 长期或大促，**非**当前 429 主药  

### 命中率目标口径

文档性能目标 **>98%** 指 **高重复、短对话、精确键可复用** 的流量。Cursor 内测（大 body、低重复）应观测 `hit_rate_5m`、`token_hit_rate_5m` 及 consumer 维度，详见 OPS_RUNBOOK §1。

### 非数据面遗漏（单独排期）

| 项 | 说明 |
|----|------|
| 上游 Key 池与 per-key in-flight | Management `profiles/{id}/keys`；避免单 Key 耗尽 |
| 429/503 `limit_source` | 区分客户端 RPM、厂商 429、池冷却、Coalesce Leader 失败 |
| Coalescing 失败体验 | Retry-After / 是否排队 |
| 上游 429 熔断 | 短时拒新请求保护冷却 |
| Trace / Admin 读取上限 | 与 `trace_logging`、Admin API 相关 |
| Cursor 并行与 context 裁剪 | 运维与客户端，非 proxy 代码 |

---

## 维护约定

- 数据面行为变更：更新 **本文件** + `CLAUDE.md` / `gateway.example.toml` / 相关指标段（`OBSERVABILITY.md`）。
- 生产事故复盘：更新 **[OPS_RUNBOOK.md](./OPS_RUNBOOK.md)** §1 样本表与本节对照表。
- P3 实现启动时：在 `DATA_PLANE_P3.md` 增加「实现状态」小节，并将上表对应行改为 🟡/✅。
- 《数据面优化.md》保留为**愿景与论证**；勿单独改其优先级表而不改本文件。
