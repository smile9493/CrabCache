# 任务 02: Profile 路由扩展

> **Phase**: 1b | **优先级**: P0 | **状态**: 待开始
> **预计工作量**: 1 小时 | **风险**: 低

## 目标

扩展 `model_prefix_to_profile()` 函数，让新供应商的模型名能自动路由到对应的 profile。

## 修改文件

- `crates/crab-pipeline/src/profile.rs`

## 详细步骤

### Step 1: 扩展 model_prefix_to_profile()

在现有映射基础上添加新供应商的模型前缀：

```rust
pub fn model_prefix_to_profile(model: &str) -> &'static str {
    let lower = model.to_lowercase();

    // ── 原有映射 ──
    if lower.starts_with("deepseek-") { return "deepseek"; }
    if lower.starts_with("mimo") { return "mimo"; }
    if lower.starts_with("gpt-")
        || lower.starts_with("gpt ")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("chatgpt-")
        || lower.starts_with("codex ")
        || lower.starts_with("codex-")
        || lower == "codex"
    { return "openai"; }
    if lower.starts_with("claude-") { return "anthropic"; }

    // ── 国际主流 ──
    if lower.starts_with("grok-") { return "xai"; }
    if lower.starts_with("mistral-")
        || lower.starts_with("codestral-")
        || lower.starts_with("pixtral-")
        || lower.starts_with("open-mistral-")
    { return "mistral"; }
    if lower.starts_with("gemini-") { return "gemini"; }
    if lower.starts_with("llama-") { return "together"; }
    if lower.starts_with("mixtral-") { return "fireworks"; }
    if lower.starts_with("command-") { return "cohere"; }
    if lower.starts_with("nemotron-") { return "nvidia"; }
    if lower.starts_with("reka-") { return "reka"; }

    // ── 中国供应商 ──
    if lower.starts_with("qwen-") || lower.starts_with("qwq-") { return "alibaba"; }
    if lower.starts_with("glm-") || lower.starts_with("chatglm-") { return "glm"; }
    if lower.starts_with("kimi-") { return "kimi"; }
    if lower.starts_with("ernie-") || lower.starts_with("yiyan") { return "baidu"; }
    if lower.starts_with("hunyuan-") { return "tencent"; }
    if lower.starts_with("spark-") || lower.starts_with("xinghuo") { return "iflytek"; }
    if lower.starts_with("baichuan-") { return "baichuan"; }
    if lower.starts_with("yi-") { return "yi"; }
    if lower.starts_with("step-") { return "stepfun"; }
    if lower.starts_with("doubao-") { return "doubao"; }
    if lower.starts_with("minimax-") { return "minimax"; }
    if lower.starts_with("abab-") { return "minimax"; }
    if lower.starts_with("sensenova-") { return "sensenova"; }
    if lower.starts_with("360-") { return "360ai"; }

    // ── 推理平台特有模型 ──
    if lower.starts_with("jamba-") { return "ai21"; }
    if lower.starts_with("gigachat-") { return "gigachat"; }
    if lower.starts_with("solar-") { return "upstage"; }
    if lower.starts_with("sabia-") || lower.starts_with("sabiazinho-") { return "maritalk"; }
    if lower.starts_with("dbrx-") { return "databricks"; }
    if lower.starts_with("snowflake-") { return "snowflake"; }
    if lower.starts_with("lfm-") { return "liquid"; }
    if lower.starts_with("palmyra-") { return "writer"; }

    // 默认兜底
    "deepseek"
}
```

### Step 2: 添加单元测试

```rust
#[test]
fn model_prefix_grok() {
    assert_eq!(model_prefix_to_profile("grok-3"), "xai");
}

#[test]
fn model_prefix_mistral() {
    assert_eq!(model_prefix_to_profile("mistral-large-latest"), "mistral");
}

#[test]
fn model_prefix_gemini() {
    assert_eq!(model_prefix_to_profile("gemini-2.5-pro"), "gemini");
}

#[test]
fn model_prefix_qwen() {
    assert_eq!(model_prefix_to_profile("qwen-max"), "alibaba");
}

#[test]
fn model_prefix_glm() {
    assert_eq!(model_prefix_to_profile("glm-4-plus"), "glm");
}

#[test]
fn model_prefix_kimi() {
    assert_eq!(model_prefix_to_profile("kimi-k2.6"), "kimi");
}

#[test]
fn model_prefix_ernie() {
    assert_eq!(model_prefix_to_profile("ernie-4.0-8k"), "baidu");
}

#[test]
fn model_prefix_hunyuan() {
    assert_eq!(model_prefix_to_profile("hunyuan-turbos-latest"), "tencent");
}

#[test]
fn model_prefix_unknown_falls_back() {
    assert_eq!(model_prefix_to_profile("unknown-model-v1"), "deepseek");
}
```

## 验收标准

- [ ] `cargo test -p crab-pipeline` 全部通过
- [ ] 新增测试用例覆盖所有主要供应商的模型前缀
- [ ] 未知模型名默认路由到 `"deepseek"`（保持向后兼容）

## 依赖

- 依赖任务 01（`UpstreamProvider` 枚举扩展）

## 注意事项

- 模型前缀匹配按**优先级排序**，更具体的前缀应放在前面
- 保留 `"deepseek"` 作为默认兜底，确保向后兼容
- `openai` 的前缀列表较长（gpt-、o1、o3、chatgpt-、codex），保持不变
