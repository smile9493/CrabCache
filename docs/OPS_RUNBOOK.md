# 运维与事故处置手册（对照运行时分析）

本文档将 **Trace / 网关日志 / Prometheus** 中常见现象，映射到 **数据面（proxy 热路径）**、**控制面 / Admin**、**运维配置** 三条责任轨。避免把「内测低命中率」或「MiMo 429」误判为「数据面 P2 未做完」。

**相关文档**：[运行时日志结论（压缩版）](RUNTIME_LOG_FINDINGS.md) · [数据面实现状态](DATA_PLANE.md) · [数据面优化展望](../数据面优化.md) · [P2 验收手册](DATA_PLANE_ACCEPTANCE.md) · [可观测性](OBSERVABILITY.md) · [Cursor 接入](CURSOR_SETUP.md) · [持久化](PERSISTENCE.md)

---

## 1. 背景：内测 Trace 报告（2026-05 样本）

以下结论来自约 **10 天、12k+ 行** `trace.jsonl` 与容器日志交叉验证（高峰日 2026-05-28 晚间）。用于排障口径，**不是** SLA 承诺。

| 指标 | 观测值 | 解读 |
|------|--------|------|
| 综合缓存命中率 | ~25% | Cursor 长线程 + 大 body（约 1/3 请求 >500KB）→ 精确键几乎唯一 |
| HIT 延迟 | ~0ms | 网关 L0/L1 路径正常 |
| MISS P99 | 数秒～数十秒 | 以上游推理 + 大 context 为主，非 parse 瓶颈 |
| Pipeline | 多为 `mimo_relay` | 模型名仍可能显示 `deepseek-v4-*`（别名/中继），≠ 未走 MiMo |
| MiMo 429 / 池耗尽 | 单上游 Key + 高峰并行 | `gateway_upstream_key_retries_total{cooldown_only}`、`upstream_key_exhausted` |
| Coalescing Follower 失败 | Leader 上游失败后批量 502 | 设计如此（避免双倍上游），非 Follower bug |
| Trace 截断 | 单行 >4MB Admin 读上限 | 事后分析困难，与 proxy 性能无关 |

> **命中率目标**：产品文档中的 **>98%** 适用于 **高重复、非 Cursor 长对话** 流量。内测 Cursor 场景应单独看 `hit_rate_5m`、consumer、body 分位数，勿与 98% 直接对比。

---

## 2. 责任轨总览

| 现象 | 数据面（P0–P3） | 运维 / 配置 | 控制面 / Admin / 产品 |
|------|-----------------|-------------|------------------------|
| MiMo / 上游 **429**、Key 冷却 | △ 近似键+Coalescing（未做） | **加 Profile 上游 Key**、降 Cursor 并行 | cooldown API、Dashboard 展示 |
| **upstream_key_exhausted** (503) | ✗ | 同上 + 确认仅 1 把 Key 时无轮换 | `GET /v1/upstream/profiles/{id}/keys` |
| 客户端 `sk-cc-*` 限流 | ✗（`rpm_limit=0` 即不限） | 按需设 `rpm_limit` / `max_concurrent` | Management API |
| 命中率低 | △ L2 语义、差分缓存（P3） | 启用 `[semantic]`、调 TTL | Cursor 减 context |
| MISS P99 高 | △ 已落地：预热、early parse | 上游节点健康、减 body | 模型/用量 |
| Follower 雪崩 | ○ 策略未文档化 | 降并发 | 可选：可重试 503 + `Retry-After` |
| **ConnectionClosed** ~15s | △ 连接预热 ✅ | 上游 WAF/限流、TE/CL | 见 [CURSOR_SETUP](CURSOR_SETUP.md) |
| Trace **截断** | ✗ | `trace_logging.max_lines`、轮转 | Admin 读取上限 |
| Redis **启动 persist 超时** | ✗ | `depends_on` + 健康检查 | [PERSISTENCE](PERSISTENCE.md) |
| 错误文案「盲盒」 | △ 阶段指标 ✅ | — | `limit_source` 结构化（规划） |

图例：**✗** 无关 · **○** 未覆盖 · **△** 部分缓解 · 粗体为常见主因

---

## 3. P0 / P1 处置清单

### P0：MiMo（或任意 Profile）上游配额被打穿

**症状**：日志 `upstream rate limited, attempting key rotation`、Prometheus `gateway_upstream_key_retries_total{outcome="cooldown_only"}`、`gateway_rejected_requests_total{reason="upstream_key_exhausted"}`；Cursor 可能显示 **`User API Key Rate limit exceeded`**（指**用户配置的上游 Key**，不一定是 `sk-cc-*`）。

1. `GET /v1/upstream/profiles/mimo/keys`（或对应 `profile_id`）— 确认 **≥2 把** 启用 Key，且 `account_id` 分流策略符合预期（429 仅跨账号轮换）。
2. 高峰临时 **减少 Cursor 并行**（多 Agent / Task 同时跑）。
3. 核对 Prometheus：`gateway_upstream_key_retries_total`、`gateway_rejected_requests_total`。
4. **不要**指望仅调大 `connection_prewarm` 或完成 Bytes 优化消除 429。

详见 [CURSOR_SETUP — 限流排查](CURSOR_SETUP.md#限流排查user-api-key-rate-limit-exceeded)。

### P1b：MiMo 端到端偏慢（大 context）

**症状**：`duration_ms` 18–26s（`client_body_bytes` ~700KB），而 ~137KB 时约 6–11s；`gap` 与 `prefill_ms` 均在 **5–8s** 量级。

| 指标 | 含义 | 典型值（wuming 样本） |
|------|------|------------------------|
| `prefill_ms` | 请求进入 → 上游**响应头**（MiMo prefill，主 SLO） | p50 ~6s |
| `ttft_ms` | 响应头 → 首个上游 body chunk | 通常 &lt;500ms |
| `upstream_latency_ms` | 响应头 → SSE 结束（生成） | 随输出 token 增长 |
| `gap` | `duration - upstream`（含读 body + 上传 + prefill） | 大 body 上传更明显 |

**处置（优先运维，再开网关特性）**：

1. Cursor：限制对话历史、避免整文件进 `messages`、长会话开新 thread（目标 body **&lt;200KB**）。
2. 分析：`python3 scripts/analyze_downstream_latency.py` + `raw_capture/index.jsonl`（看 `prefill_ms` 与 body 分桶）。
3. 可选网关：`[features] mimo_retire_prefix_messages = true`、`mimo_keep_recent_turns = 6`（**只缩小上游 body，不改 L0/L1 缓存键**）。
4. **不要**在未通过门禁前开启 `streaming_body_forward = true`（见 [STREAMING_BODY_FORWARD.md](STREAMING_BODY_FORWARD.md)）。

### P1：可观测与启动

| 项 | 动作 |
|----|------|
| Trace 事后分析 | 提高 `max_lines`、控制 `raw_capture` 体积；Admin 大文件读取见 OBSERVABILITY |
| Redis 控制面 | Compose `depends_on: service_healthy`；persist 失败重试见 PERSISTENCE |
| 命中率解读 | Overview `hit_rate_5m` + Trace 24h；区分 L0–L2 与 L3（上游 prompt cache） |

### P2：数据面继续演进（边际收益）

在 P0 稳定后，按 ROI 排序见 [DATA_PLANE.md — 运行时对照](DATA_PLANE.md#运行时分析对照2026-05-内测)：

1. 启用 **L2 语义缓存**（`[semantic].enabled`）— 相似问句，非 429 主药  
2. **MiMo 近似缓存键** + Coalescing — 高峰减重复上游  
3. `streaming_body_forward`（P3）— 大 body 内存与读 body 时间  

---

## 4. 数据面已落地 vs 报告问题（避免重复投入）

以下 **P0–P2 已交付**（见 [DATA_PLANE.md](DATA_PLANE.md)），对报告中的 **网关自身延迟** 已验证有效（HIT ≈0ms、阶段水印可用）：

- `Bytes`、增量 SHA-256、Raw 预解析复用  
- MiMo `body_quick_parse` + early exact cache  
- Prefix L0 **索引预热**（不短路返回旧响应）  
- 连接直预热、SSE pipeline、`gateway_request_phase_latency_seconds`  

**不能**单靠继续做完上表解决：MiMo 429、25% 综合命中率、Trace 截断、Redis 启动竞态。

---

## 5. 规划项（非 proxy 热路径，单独排期）

| 项 | 轨道 | 说明 |
|----|------|------|
| 每上游 Key 最大 in-flight | 产品/网关 | 防止单 Key 被 Cursor 打穿；矩阵见 [数据面优化 §运行时](../数据面优化.md#运行时分析对照与遗漏项) |
| 429/503 `limit_source` JSON | 控制面 | `gateway_client_rpm` / `mimo_upstream_429` / `upstream_pool_cooldown` / `coalesce_leader_failed` |
| Coalescing Leader 失败策略 | 产品 | 快速 503+Retry-After vs 排队（现：Follower 不重试上游） |
| 上游连续 429 熔断 | 网关 | 短时拒新请求，保护 Key 冷却 |
| Admin：Profile Key 冷却 / 配额面板 | Admin | 指标已有，UI 与文档待对齐 |
| 命中率 98% 前提说明 | 文档 | 已写入 DATA_PLANE + 数据面优化 |

---

## 6. 推荐 Prometheus / 日志核对

```text
# 上游 Key 池
gateway_upstream_key_retries_total
gateway_rejected_requests_total{reason="upstream_key_exhausted"}

# 缓存（勿与 L3 混淆）
gateway_cache_requests_total{tier,result}
gateway_prefix_index_warmup_total

# 合并
gateway_coalesced_requests_total

# 阶段耗时（区分 parse vs upstream）
gateway_request_phase_latency_seconds
```

网关日志关键词：`upstream rate limited`、`cooldown_only`、`Coalesce follower`、`trace line truncated`。

---

## 7. 维护约定

- 新事故复盘：在本文件 **§1** 增补一行样本表，并在 [DATA_PLANE.md](DATA_PLANE.md) 对照表同步。  
- 数据面代码变更：仍只改 [DATA_PLANE.md](DATA_PLANE.md) 状态表，不单独改优先级矩阵。  
- Cursor 限流文案：以 [CURSOR_SETUP.md](CURSOR_SETUP.md) 为准（DeepSeek vs MiMo Profile 分节）。
