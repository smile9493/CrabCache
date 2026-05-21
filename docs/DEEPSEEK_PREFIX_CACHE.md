# DeepSeek 上游前缀缓存（L3）

CrabCache 维护两套独立的缓存指标，请勿混谈「缓存命中率」：

| 指标 | 定义 | Prometheus / Admin |
|------|------|-------------------|
| **L3 前缀命中率** | `prompt_cache_hit_tokens / (hit + miss)` | `gateway_upstream_prompt_cache_tokens_total{status="hit\|miss"}` |
| **网关响应命中率** | L0/L1/L2 hit / 总请求 | `gateway_cache_requests_total` |

L3 由 DeepSeek API 在服务端维护 KV/前缀缓存；CrabCache 通过稳定 `messages` 前缀与粘滞路由提高命中率。L0/L1/L2 为网关响应缓存，与 L3 正交。

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

## 指标与告警

- Prometheus：`gateway_upstream_prompt_cache_tokens_total`
- Admin：`GET /api/admin/metrics`（含 `prefix_cache_hit_ratio`）
- Admin：`GET /api/admin/metrics/prefix-cache`（按 model 分桶）
- 示例告警：[`deploy/prometheus/alerts.example.yml`](../deploy/prometheus/alerts.example.yml) 中 `CrabCacheL3PrefixCacheHitRateLow`

## 96% 场景说明

长固定 system + 多轮只追加的 Agent 场景，在单 peer、稳定 `x-conversation-id` 下 L3 可达 **90%+**。全站平均取决于客户端是否遵守前缀规范。

## 自建推理（可选）

SGLang HiCache 等自建 KV 缓存不在本仓库实现范围内；当前计划仅覆盖官方/中转 DeepSeek API。
