# MiMo API 常见错误码与解决方法

在使用 API 调用 MiMo 模型（如 `mimo-v2.5-pro`）时，网关和客户端可能遇到以下常见 HTTP 错误状态码。

| 错误码 | 错误类型 | 常见原因 | 解决方法 |
| :--- | :--- | :--- | :--- |
| **400** | 格式错误 (Bad Request) | 1. 请求体 JSON 格式不规范。<br>2. 缺少必需参数（如 `messages` 或 `model`）。<br>3. 消息格式不符合接口要求。<br>4. 使用了不存在的模型名称。<br>5. 多模态输入（如图像）格式、大小超限，或文件无法公开访问。<br>6. **多轮对话思考模式下，未完整回传 `reasoning_content`**。 | 1. 检查并校对 JSON 请求体字段与格式。<br>2. 检查多模态图片的 URL 可达性与尺寸规格。<br>3. 确保携带完整上下文的 `reasoning_content`。 |
| **401** | 认证失败 (Unauthorized) | 1. 缺少 API Key，或 `Authorization` 头部格式错误（如非 `Bearer sk-...`）。<br>2. **混用了 Token Plan（套餐包）与按量付费 API 的 API Key**。 | 1. 检查网关与客户端配置的 API Key 格式。<br>2. 如果使用 Token Plan，确保配置了专属的 Base URL 和对应的 API Key，切勿交叉使用。 |
| **402** | 余额不足 (Payment Required) | 账户余额不足，或套餐包额度已耗尽。 | 检查上游账户余额并及时充值或订购套餐。 |
| **403** | 拒绝访问 (Forbidden) | 服务暂不支持当前地区，或 API Key 触发风控被禁用。 | 检查客户端请求来源 IP 是否在支持区域内；重新生成 API Key，并注意检查输入内容合规性。 |
| **404** | 资源未找到 (Not Found) | 调用的模型或接口不支持多模态输入（例如向不支持图像的模型发送了图片数据）。 | 确认所选的模型版本（如 `mimo-v2.5-pro`）与所请求接口支持多模态输入。 |
| **421** | 内容拦截 (Misdirected) | 输入内容触发了上游的安全审核与合规拦截规则。 | 避免发送包含敏感、不安全或违反合规要求的内容。 |
| **429** | 请求超限 (Too Many Requests) | 1. 请求频率（RPM/TPM）超出了并发速率限制。<br>2. Token Plan 额度已用尽。 | 1. 在客户端实现指数退避与重试机制（Exponential Backoff）。<br>2. 降低并发调用频率，或升级 Token Plan 套餐，或切换为按量付费通道。 |
| **500** | 服务器失败 (Internal Server Error) | 上游 MiMo 服务内部故障。 | 稍后发起重试，或联系上游服务商协助解决。 |
| **503** | 服务器故障 (Service Unavailable) | 上游 MiMo 服务器负载过高。 | 触发网关熔断机制。建议稍后重试。 |

---

## 日志排查与对照指南

结合我们在网关 `/app/logs/raw_capture/index.jsonl` 中捕获的日志，如果发现部分请求的 `upstream_body_bytes` 为 `0` 且 `delta_bytes` 为负，通常说明请求在网关处已被拦截，或在上游连接时失败。

### 1. 429 / 503 场景引发的熔断 (Circuit Breaker)
当上游服务器频繁返回 `429`（超限）或 `503`（负载过高）时，网关的熔断器会记录失败率。一旦失败率超标，网关会直接返回 **503** 错误以保护系统，此时不再向外部上游发送请求，因此日志中 `upstream_body_bytes` 为 `0`。

### 2. 400 格式错误
MiMo 模型对 `reasoning_content` 的完整回传有严格要求。如果使用 DeepSeek 混用管线或者在多轮对话中裁剪不当导致格式异常，会上报 **400** 错误，需要对照网关 `client_path` 下捕获的原始 JSON 体与 `upstream_path` 差异进行定位。
