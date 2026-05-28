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
| `streaming_body_forward` | ⬜ | 配置预留，[`gateway.example.toml`](../config/gateway.example.toml) 标注未实现 |
| MiMo 近似缓存键（sfp + msg count） | ⬜ | 精确键仍为 body SHA-256 |

### 二、SSE 流式

| 项 | 状态 | 说明 / 代码 |
|----|------|-------------|
| `SseEvent<'a>` + memchr 行切分 | ✅ | [`sse.rs`](../crates/crab-proxy/src/sse.rs) |
| `SsePipeline` 抽象 | ✅ | [`sse_pipeline/`](../crates/crab-proxy/src/sse_pipeline/) |
| `upstream_response_body_filter` 瘦身 | 🟡 | 流式逻辑已委托 pipeline；[`proxy.rs`](../crates/crab-proxy/src/proxy.rs) 仍 ~3k 行 |
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
| HTTP/2 上游多路复用 | ⬜ | 默认 `upstream_force_http1` |

### 五、可观测性

| 项 | 状态 | 说明 / 代码 |
|----|------|-------------|
| `gateway_request_phase_latency_seconds` | ✅ | `body_read_done` … `logging_done`（无 `body_read_start`） |
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
| `proxy.rs` → `phases/` 拆分 | ⬜ | 见数据面优化附录 |

---

## Feature 开关（`[features]`）

| 开关 | 默认 | 行为摘要 |
|------|------|----------|
| `prefix_aware_cache` | off | L0 前缀索引；MiMo 三管道**默认等效开启** |
| `connection_prewarm` | off | 共享 `Connector` 直预热；需 fork 注入，见 PATCH.md |
| `affinity_prompt_cache_feedback` | off | 请求末根据 `prompt_cache_*` 更新 Ketama hint |
| `streaming_body_forward` | off | **未实现** |
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

## 维护约定

- 数据面行为变更：更新 **本文件** + `CLAUDE.md` / `gateway.example.toml` / 相关指标段（`OBSERVABILITY.md`）。
- P3 实现启动时：在 `DATA_PLANE_P3.md` 增加「实现状态」小节，并将上表对应行改为 🟡/✅。
- 《数据面优化.md》保留为**愿景与论证**；勿单独改其优先级表而不改本文件。
