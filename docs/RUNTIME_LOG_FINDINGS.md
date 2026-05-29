# 运行时日志结论（压缩版）

一页纸汇总：内测 Trace / MiMo 专项 / 延迟分解 + 代码对照。**详述**见 [OPS_RUNBOOK.md](OPS_RUNBOOK.md)、[DATA_PLANE.md](DATA_PLANE.md#运行时分析对照2026-05-内测)。

**样本**：~12k 全量 Trace；~5.2k MiMo；~2k miss 有完整延迟字段（2026-05-25～29）。

---

## 结论（7 条）

1. **网关代理正常**：MiMo 全量转发成功；压测 2000+/min 无崩；Cache HIT 亚毫秒。数据面 P0–P2 在「网关自身」已验证。
2. **低命中率是流量形态**：Cursor 每轮全量 history → 精确键必变；压测 99.7% vs 真实 ~3%。**勿用 98% 目标评判内测**。Prefix L0 只预热索引、不短路。
3. **429 是 MiMo 上游 Key，不是 sk-cc**：`rpm_limit=0`；单 Key + 高峰并行 + 大 body。Cursor「User API Key…」多指**厂商 Key**。
4. **backend 分散（已修）**：`affinity_key` 在 `session_fingerprint` 与 body `user` 就绪后 **重算**（`refresh_affinity_key`）；仍优先 `conv` / `pck` / `user` 头与字段，再 `sfp:`。
5. **「pre_header / gap」口径**：`pre_header_ms ≈ latency_ms − upstream_latency_ms`（Trace / Live API）；表示 **响应头之前**（读 body、上传 MiMo、上游 prefill），**不是** SSE 转发慢。`upstream_latency_ms` 在网关内为 **响应头 → 上游 body 结束**。
6. **`streaming_body_forward`（可选）**：MiMo 在 partial body 后先 `upstream_peer`（与客户端续传重叠）；wuming 灰度 **开** 见 `gateway.docker.toml`。配合 keepalive + `connection_prewarm`。请求体 gzip：`[features] upstream_request_gzip`（默认关，需 `scripts/gate_upstream_encoding_probe.sh` 验证 MiMo）。
7. **Coalescing 对内测 Cursor 价值小**；Leader 失败 Follower 不重试上游（防雪崩）→ 批量 502 属设计。

---

## Trace 字段口径（排障必看）

| 字段 | 含义 |
|------|------|
| `latency_ms` | 请求开始 → logging（端到端） |
| `upstream_latency_ms` | 上游**响应头**到达 → 上游 body 结束 |
| `pre_header_ms` | `latency_ms − upstream_latency_ms`（响应头之前） |
| `ttft_ms` | 响应头 → 首个 SSE chunk（非用户 TTFT） |
| `session_fingerprint` | 首条 user 消息 hash（**日志分组用**） |
| `affinity_kind` | 来自 **`affinity_key` 前缀**，未必是 `sfp` |

**正确拆延迟**：用 Prometheus `gateway_request_phase_latency_seconds`（`body_read_*`、`upstream_body_sent`、`upstream_body_done`），勿单独用 `total − upstream` 当网关 KPI。

---

## 改进方向（优先级）

### P0

| 项 | 动作 |
|----|------|
| MiMo Key 池 | ≥2 Key、`account_id` 分流；高峰降 Cursor 并行 |
| Affinity | 已落地 sfp 重算；评估 Cursor 下 **pck 优先级** |
| 指标 | `pre_header_ms` Live API；`gateway_request_phase_latency_seconds` 含 `upstream_response_headers` |

### P1

| 项 | 动作 |
|----|------|
| `streaming_body_forward` | 已支持；wuming 见 [STREAMING_BODY_FORWARD.md](STREAMING_BODY_FORWARD.md) |
| 上游响应 gzip | `Accept-Encoding: gzip, deflate, br` + `upstream_response_decompress.rs`（R7） |
| HTTP/2 上游 | `upstream_force_http1 = false`（全局默认）；`h2_ping_interval_secs` |
| L2 语义缓存 | 配置启用 `[semantic]`，略涨相似问句命中 |
| MiMo 近似键 + Coalesce | 高峰减重复上游（未实现） |
| 可观测 | `limit_source` 结构化 429/503；Trace 轮转/Admin 读上限 |

### P2+

就近 MiMo 节点 · `upstream_request_gzip` 灰度（探针通过后）· `mimo_context_compression` 落地 · per-key in-flight / 429 熔断 · 差分缓存/io_uring（非当前主药）

---

## 代码锚点（验证用）

| 主题 | 位置 |
|------|------|
| 读满 body 再上游 | `phases/request_filter.rs` `read_request_body` 循环 |
| affinity 早于 sfp | 同文件 `extract_affinity_key` → 后文 `session_fingerprint_from_payload` |
| `upstream.headers_at` = 响应头时刻；`upstream.start` = 连接时刻 | `phases/response_filter.rs` / `upstream_peer.rs` |
| `prefill_ms` / `ttft_ms`（trace & raw_capture） | `helper_fns::request_timing_ms` |
| `upstream_latency_ms` 结算（headers → EOS） | `phases/response_body.rs` EOS |
| 上游请求 framing | `upstream_headers.rs` `normalize_replaced_body_headers`（清 TE；可选 `Content-Encoding: gzip`） |
| 上游 Accept-Encoding | `upstream_headers.rs` `UPSTREAM_ACCEPT_ENCODING` |
| 上游响应解压 | `upstream_response_decompress.rs` |
| HTTP/1 vs H2 | `connection_helpers.rs` `upstream_force_http1`（默认 **false**） |

---

## 一句话

**网关没问题；慢/429/低命中/多 backend 来自 Cursor 大 context、MiMo 配额、affinity 未绑 sfp、以及把 prefill 算进「网关 overhead」。先 Key + affinity + phase 指标，再流式 body 与 L2。**
