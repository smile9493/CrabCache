# DeepSeek 上游前缀缓存（L3）

CrabCache 维护两套独立的缓存指标，请勿混谈「缓存命中率」：

| 指标 | 定义 | Prometheus / Admin |
|------|------|-------------------|
| **L3 前缀命中率** | `prompt_cache_hit_tokens / (hit + miss)` | `gateway_upstream_prompt_cache_tokens_total{status="hit\|miss"}` |
| **网关响应命中率** | L0/L1/L2 hit / 总请求 | `gateway_cache_requests_total` |

L3 由 DeepSeek API 在服务端维护 KV/前缀缓存；CrabCache 通过稳定 `messages` 前缀与粘滞路由提高命中率。L0/L1/L2 为网关响应缓存，与 L3 正交。

另有一套 **L0 prefix-aware（网关侧前缀索引）**，与 L3 无关：见下文 [L0 prefix-aware（网关侧）](#l0-prefix-aware-网关侧)。完整对照见 [DATA_PLANE.md](./DATA_PLANE.md)。

## L0 prefix-aware（网关侧）

在 `[features].prefix_aware_cache = true` 或 **MiMo 中继管道**（`mimo_token_plan_relay`，含 `mimo_relay` / `mimo_payg_relay` 别名）下，网关对「共享消息前缀、仅最后一条 user 不同」的请求维护 **prefix → full cache key** 索引（[`tiered.rs`](../crates/crab-cache/src/tiered.rs) `prefix_index`）。

**当前行为（与早期设计稿不同）**：

| 动作 | 是否发生 |
|------|----------|
| 发现 prefix 在 L0 有历史 entry | 是（`prefix_l0_lookup`） |
| 更新 `prefix_index`，便于后续 exact 命中 | 是 |
| 递增 `gateway_prefix_index_warmup_total` | 是（仅索引预热、仍走上游） |
| 直接 `send_cached_response` 返回旧轮完整响应 | **否** |

因此 prefix-aware **不替代** 当前请求的推理；它加速的是**后续**与相同前缀、不同尾消息组合的 **exact L0/L1** 命中。运维上勿将 `gateway_prefix_index_warmup_total` 误读为「网关响应命中」。

## 应用层五大实践

1. **固定 system 前缀**：Agent 指令、工具定义放在对话前部且保持不变。
2. **禁止动态污染前缀**：不要在 `messages` 头部插入时间戳、随机 ID、每轮变化的 system。
3. **只追加**：新轮次仅 `append` user/assistant，不修改或删除历史。
4. **会话粘滞**：发送 `x-conversation-id` 或 `x-prompt-cache-key` / body `prompt_cache_key`。
5. **监控 usage**：检查响应 `usage.prompt_cache_hit_tokens` 与 `prompt_cache_miss_tokens`。

## CrabCache 配置

### Reasoning 策略（影响 L3）

与 **deepseek-cursor-proxy** 一致，仅 `recover` / `reject`：

| `missing_reasoning_strategy` | 行为 | L3 |
|------------------------------|------|-----|
| `recover`（默认） | 无稳定 scope 时可能截断；有 `client_key`/`x-conversation-id` 时**就地补 reasoning、不截断** | 配稳定会话时友好 |
| `reject` | 缺 reasoning 时 409 | 不修改消息（调试） |

```toml
[reasoning]
missing_reasoning_strategy = "recover"
# 超长对话：在尾部追加摘要 user 消息（不删除历史），0=关闭
# context_summary_message_threshold = 200
# prefix_validate = true  # 记录 gateway_prefix_break_total，不阻断
```

### 粘滞路由

优先级：`x-conversation-id` > `x-prompt-cache-key` / `prompt_cache_key` > **body `user_id`（网关注入的 `project_id`）** > `x-user-id` > 客户端 IP。多租户见 [MULTI_TENANT.md](./MULTI_TENANT.md)。

Ketama 将亲和键映射到 `[upstream].deepseek_endpoints` 中的固定 peer；peer 切换会导致 L3 暂时下跌。

### 可选：affinity 反馈（`affinity_prompt_cache_feedback`）

启用 `[features].affinity_prompt_cache_feedback` 时，网关根据上游 `usage.prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` 在**请求结束**更新 `affinity_key → backend_name` 提示（非每个 SSE chunk 立即 invalidate）。连续 **3** 次 pure miss（`AFFINITY_MISS_STREAK_THRESHOLD`）后清除提示，避免瞬时 miss 导致错误换路。与 Ketama `select_with_hint` 配合使用。

## 指标与告警

- Prometheus：`gateway_upstream_prompt_cache_tokens_total`（L3）
- Prometheus：`gateway_prefix_index_warmup_total`（L0 索引预热次数，非响应命中）
- Admin：`GET /api/admin/metrics`（含 `prefix_cache_hit_ratio`）
- Admin：`GET /api/admin/metrics/prefix-cache`（按 model 分桶）
- 示例告警：[`deploy/prometheus/alerts.example.yml`](../deploy/prometheus/alerts.example.yml) 中 `CrabCacheL3PrefixCacheHitRateLow`

## 96% 场景说明

长固定 system + 多轮只追加的 Agent 场景，在单 peer、稳定 `x-conversation-id` 下 L3 可达 **90%+**。全站平均取决于客户端是否遵守前缀规范。

## 自建推理（可选）

SGLang HiCache 等自建 KV 缓存不在本仓库实现范围内；当前计划仅覆盖官方/中转 DeepSeek API。
