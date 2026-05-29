# 运行时日志结论（压缩版）

一页纸汇总：内测 Trace / MiMo 专项 / 延迟分解 + 代码对照。**详述**见 [OPS_RUNBOOK.md](OPS_RUNBOOK.md)、[DATA_PLANE.md](DATA_PLANE.md#运行时分析对照2026-05-内测)。

**样本**：~12k 全量 Trace；~5.2k MiMo；~2k miss 有完整延迟字段（2026-05-25～29）。

---

## 结论（7 条）

1. **网关代理正常**：MiMo 全量转发成功；压测 2000+/min 无崩；Cache HIT 亚毫秒。数据面 P0–P2 在「网关自身」已验证。
2. **低命中率是流量形态**：Cursor 每轮全量 history → 精确键必变；压测 99.7% vs 真实 ~3%。**勿用 98% 目标评判内测**。Prefix L0 只预热索引、不短路。
3. **429 是 MiMo 上游 Key，不是 sk-cc**：`rpm_limit=0`；单 Key + 高峰并行 + 大 body。Cursor「User API Key…」多指**厂商 Key**。
4. **backend 分散 ≠ Ketama  vnode 不够**：Trace 有 `session_fingerprint` ≠ 路由用 sfp——`affinity_key` 在 full parse **之前**固定，**未用 sfp 重算**；实际多为 `pck:` / `user:` / `ip:`。e5fc69ee 会话 8 backend → 上游 prefix cache ~3.5%。
5. **「网关 overhead 5.5s」是度量口径问题**：`upstream_latency_ms` = **响应头 → SSE 结束**（偏输出流）；`total − upstream` = **响应头之前全部**（含读满 body、上传 MiMo、**上游 prefill**）。`ttft_ms≈0` 不表示用户首字快。
6. **架构放大延迟**：必须先 **读满 body** 再发上游（`streaming_body_forward` 未实现）；上游默认 **HTTP/1**；**无** gzip 上传 body。
7. **Coalescing 对内测 Cursor 价值小**；Leader 失败 Follower 不重试上游（防雪崩）→ 批量 502 属设计。

---

## Trace 字段口径（排障必看）

| 字段 | 含义 |
|------|------|
| `latency_ms` | 请求开始 → logging（端到端） |
| `upstream_latency_ms` | 上游**响应头**到达 → 上游 body 结束 |
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
| Affinity bug | parse 后按 **sfp 重算** `affinity_key`；评估 Cursor 下 **pck 优先级** |
| 指标 | Phase histogram 拆 pre_header；Dashboard/告警对齐 |

### P1

| 项 | 动作 |
|----|------|
| `streaming_body_forward` | MiMo 边读边发，降 wall-clock |
| L2 语义缓存 | 配置启用 `[semantic]`，略涨相似问句命中 |
| MiMo 近似键 + Coalesce | 高峰减重复上游（未实现） |
| 可观测 | `limit_source` 结构化 429/503；Trace 轮转/Admin 读上限 |

### P2+

就近 MiMo 节点 · 上游 body gzip（需 API 支持）· `mimo_context_compression` 落地 · H2/keepalive 微调 · per-key in-flight / 429 熔断 · 差分缓存/io_uring（非当前主药）

---

## 代码锚点（验证用）

| 主题 | 位置 |
|------|------|
| 读满 body 再上游 | `phases/request_filter.rs` `read_request_body` 循环 |
| affinity 早于 sfp | 同文件 `extract_affinity_key` → 后文 `session_fingerprint_from_payload` |
| `upstream.start` = 响应头时刻 | `phases/response_filter.rs` |
| `upstream_latency_ms` 结算 | `phases/response_body.rs` EOS |
| 去上游 Content-Encoding | `upstream_headers.rs` `normalize_replaced_body_headers` |
| 强制上游 H1 | `connection_helpers.rs` `upstream_force_http1` |

---

## 一句话

**网关没问题；慢/429/低命中/多 backend 来自 Cursor 大 context、MiMo 配额、affinity 未绑 sfp、以及把 prefill 算进「网关 overhead」。先 Key + affinity + phase 指标，再流式 body 与 L2。**
