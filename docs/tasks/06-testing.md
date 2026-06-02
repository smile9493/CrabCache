# 任务 06: 测试验证

> **Phase**: 4 | **优先级**: P1 | **状态**: 待开始
> **预计工作量**: 1-2 小时 | **风险**: 低

## 目标

验证所有新增供应商的枚举解析、路由映射、pipeline 选择和配置加载均正常工作。

## 详细步骤

### Step 1: 单元测试

在 `crates/crab-pipeline/src/types.rs` 中添加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_from_str_roundtrip() {
        let providers = vec![
            "deepseek", "mimo", "openai", "codex", "anthropic",
            "groq", "xai", "mistral", "gemini", "perplexity",
            "together", "fireworks", "cerebras", "cohere", "nvidia",
            "openrouter", "alibaba", "qianfan", "glm", "kimi",
            "minimax", "tencent", "iflytek", "baichuan", "yi",
            // ... 所有供应商
        ];
        for name in providers {
            let provider = UpstreamProvider::from_str(name);
            assert_ne!(provider, UpstreamProvider::Other, "Failed for: {}", name);
            assert_eq!(provider.as_str(), name, "Roundtrip failed for: {}", name);
        }
    }

    #[test]
    fn provider_unknown_falls_back_to_other() {
        assert_eq!(UpstreamProvider::from_str("nonexistent"), UpstreamProvider::Other);
    }

    #[test]
    fn provider_case_insensitive() {
        assert_eq!(UpstreamProvider::from_str("GROQ"), UpstreamProvider::Groq);
        assert_eq!(UpstreamProvider::from_str("Groq"), UpstreamProvider::Groq);
        assert_eq!(UpstreamProvider::from_str("groq"), UpstreamProvider::Groq);
    }
}
```

### Step 2: Profile 路由测试

在 `crates/crab-pipeline/src/profile.rs` 中添加测试：

```rust
#[test]
fn model_prefix_all_new_providers() {
    let cases = vec![
        ("grok-3", "xai"),
        ("mistral-large-latest", "mistral"),
        ("gemini-2.5-pro", "gemini"),
        ("qwen-max", "alibaba"),
        ("glm-4-plus", "glm"),
        ("kimi-k2.6", "kimi"),
        ("ernie-4.0-8k", "baidu"),
        ("hunyuan-turbos-latest", "tencent"),
        ("yi-large", "yi"),
        ("step-2-16k", "stepfun"),
        ("doubao-1.5-pro", "doubao"),
        ("MiniMax-M2.5", "minimax"),
    ];
    for (model, expected_profile) in cases {
        assert_eq!(
            model_prefix_to_profile(model),
            expected_profile,
            "Failed for model: {}",
            model
        );
    }
}
```

### Step 3: 配置加载测试

验证 `gateway.example.toml` 可被正确解析：

```bash
# 在本机执行
cargo test -p crab-gateway -- test_config_parse
```

### Step 4: Clippy 检查

```bash
cargo clippy --workspace -- -D warnings
```

### Step 5: 端到端验证（可选）

选择 3-5 个真实供应商进行端到端测试：

1. **OpenRouter**（聚合网关，无需注册即可测试部分免费模型）
2. **Groq**（免费额度，超低延迟）
3. **硅基流动**（国内供应商，免费额度）

```bash
# 本地启动网关
cargo run --bin crab-gateway -- config/gateway.toml

# 测试请求
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer your-api-key" \
  -H "Content-Type: application/json" \
  -d '{"model": "grok-3", "messages": [{"role": "user", "content": "Hello"}]}'
```

## 验收标准

- [ ] `cargo clippy --workspace` 无新增警告
- [ ] `cargo test --workspace` 全部通过
- [ ] `gateway.example.toml` 可被正确解析
- [ ] 至少 3 个新供应商端到端测试通过

## 依赖

- 依赖任务 01、02、03、04 全部完成

## 注意事项

- 端到端测试需要有效的 API Key，可在 `.env` 中配置
- 某些供应商可能有地区限制，需使用代理
- 测试时注意 API 调用费用
