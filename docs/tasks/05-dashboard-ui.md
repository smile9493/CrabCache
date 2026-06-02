# 任务 05: Dashboard UI 更新

> **Phase**: 3 | **优先级**: P2 | **状态**: ✅ 已完成
> **预计工作量**: 1-2 小时 | **风险**: 低

## 目标

更新 Admin Dashboard 的上游 Profile 管理页面，添加所有新供应商选项。

## 修改文件

- `crates/crab-dashboard/src/pages/upstream.rs`

## 详细步骤

### Step 1: 更新供应商预设模板

实际实现采用了 `PresetTemplate` 结构体 + `PRESETS` 常量数组的方式（而非文档原计划的 `provider_select_options()` 函数），包含 ~58 个供应商预设：

```rust
struct PresetTemplate {
    id: &'static str,
    label_zh: &'static str,
    label_en: &'static str,
    provider: &'static str,
    base_url: &'static str,
    models: &'static [&'static str],
    default_model: &'static str,
    tls_sni: &'static str,
}

const PRESETS: &[PresetTemplate] = &[
    // ── Original ── (DeepSeek, MiMo, OpenAI, Codex, Anthropic)
    // ── International ── (Groq, xAI, Mistral, Gemini, Perplexity, Together, Fireworks, Cerebras, Cohere, NVIDIA, Nebius, SiliconFlow, Hyperbolic, OpenRouter, Reka)
    // ── Cloud Platforms ── (Azure OpenAI, Azure AI, Bedrock, Vertex, watsonx, OCI, SAP)
    // ── China Providers ── (Alibaba, Qianfan, GLM, Kimi, Minimax, Moonshot, Volcengine, Doubao, Tencent, iFlytek, Baichuan, Yi, StepFun, 360AI, SenseNova, SparkDesk, Coze)
    // ── Inference Platforms ── (DeepInfra, SambaNova, Together, Fireworks, etc.)
    // ...
];
```

见 `crates/crab-dashboard/src/pages/upstream.rs` 第 41-609 行。

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
