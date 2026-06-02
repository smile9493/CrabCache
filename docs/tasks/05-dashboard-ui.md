# 任务 05: Dashboard UI 更新

> **Phase**: 3 | **优先级**: P2 | **状态**: 待开始
> **预计工作量**: 1-2 小时 | **风险**: 低

## 目标

更新 Admin Dashboard 的上游 Profile 管理页面，添加所有新供应商选项。

## 修改文件

- `crates/crab-dashboard/src/pages/upstream.rs`

## 详细步骤

### Step 1: 更新供应商下拉列表

在上游 Profile 创建/编辑表单中的供应商选择下拉框中添加所有新供应商：

```rust
// 供应商分组显示
fn provider_select_options() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        ("原供应商", vec![
            ("deepseek", "DeepSeek"),
            ("mimo", "MiMo (小米)"),
            ("openai", "OpenAI"),
            ("codex", "Codex"),
            ("anthropic", "Anthropic"),
        ]),
        ("国际主流", vec![
            ("groq", "Groq"),
            ("xai", "xAI (Grok)"),
            ("mistral", "Mistral"),
            ("gemini", "Google Gemini"),
            ("perplexity", "Perplexity"),
            ("together", "Together AI"),
            ("fireworks", "Fireworks AI"),
            ("cerebras", "Cerebras"),
            ("cohere", "Cohere"),
            ("nvidia", "NVIDIA NIM"),
            ("openrouter", "OpenRouter"),
        ]),
        ("云平台", vec![
            ("azure-openai", "Azure OpenAI"),
            ("bedrock", "Amazon Bedrock"),
            ("vertex", "Google Vertex AI"),
        ]),
        ("中国供应商", vec![
            ("alibaba", "阿里通义千问"),
            ("qianfan", "百度千帆"),
            ("glm", "智谱 GLM"),
            ("kimi", "Kimi (月之暗面)"),
            ("minimax", "Minimax"),
            ("tencent", "腾讯混元"),
            ("iflytek", "科大讯飞星火"),
            ("baichuan", "百川"),
            ("yi", "零一万物"),
            ("stepfun", "阶跃星辰"),
            ("doubao", "豆包"),
        ]),
        ("推理平台", vec![
            ("deepinfra", "DeepInfra"),
            ("sambanova", "SambaNova"),
            ("huggingface", "HuggingFace"),
            ("replicate", "Replicate"),
            ("github-models", "GitHub Models"),
        ]),
    ]
}
```

### Step 2: 更新模型同步逻辑

如果 Dashboard 有从供应商 `/v1/models` 端点同步模型列表的功能，需要确保新供应商的 models 端点格式兼容。

### Step 3: 更新 Profile 测试功能

`POST /v1/upstream/profiles/{id}/test` 端点会探测 `GET {base_url}/v1/models`，需确保新供应商的 base_url 正确。

## 验收标准

- [ ] Dashboard 上游页面供应商下拉列表显示所有新供应商
- [ ] 供应商按分组显示（国际主流/云平台/中国供应商/推理平台）
- [ ] 选择新供应商后可正常创建 Profile
- [ ] Profile 测试功能对新供应商正常工作

## 依赖

- 依赖任务 01（`UpstreamProvider` 枚举扩展）

## 注意事项

- 前端使用 Leptos WASM，修改后需重新构建 `trunk build`
- 下拉列表过长时考虑添加搜索过滤功能
- 分组标签使用 `<optgroup>` 或等效的 Leptos 组件
