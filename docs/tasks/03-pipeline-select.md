# 任务 03: Pipeline 选择更新

> **Phase**: 1c | **优先级**: P0 | **状态**: 待开始
> **预计工作量**: 30 分钟 | **风险**: 低

## 目标

更新 `auto_pipeline_legacy()` 函数，确保新增供应商变体都被正确匹配到 `GenericRelay` pipeline。

## 修改文件

- `crates/crab-pipeline/src/select.rs`

## 详细步骤

### Step 1: 更新 auto_pipeline_legacy() match 分支

```rust
fn auto_pipeline_legacy(
    provider: UpstreamProvider,
    model: &str,
    ctx: &PipelineRequestContext<'_>,
) -> RequestPipeline {
    match provider {
        // ── DeepSeek 特殊处理 ──
        UpstreamProvider::Deepseek => {
            if is_deepseek_v4_model(model) && ctx.has_cursor_signals() {
                RequestPipeline::CursorDeepSeekV4
            } else {
                RequestPipeline::DeepSeekLight
            }
        }

        // ── MiMo 特殊处理 ──
        UpstreamProvider::Mimo => RequestPipeline::MimoTokenPlanRelay,

        // ── Codex 特殊处理 ──
        UpstreamProvider::Codex => RequestPipeline::CodexRelay,

        // ── 所有其他供应商（OpenAI 兼容格式）统一走 GenericRelay ──
        UpstreamProvider::Openai
        | UpstreamProvider::Anthropic
        | UpstreamProvider::Groq
        | UpstreamProvider::Xai
        | UpstreamProvider::Mistral
        | UpstreamProvider::Gemini
        | UpstreamProvider::Perplexity
        | UpstreamProvider::Together
        | UpstreamProvider::Fireworks
        | UpstreamProvider::Cerebras
        | UpstreamProvider::Cohere
        | UpstreamProvider::Nvidia
        | UpstreamProvider::Nebius
        | UpstreamProvider::Siliconflow
        | UpstreamProvider::Hyperbolic
        | UpstreamProvider::OpenRouter
        | UpstreamProvider::Reka
        | UpstreamProvider::AzureOpenai
        | UpstreamProvider::AzureAi
        | UpstreamProvider::Bedrock
        | UpstreamProvider::VertexAi
        | UpstreamProvider::Watsonx
        | UpstreamProvider::Oci
        | UpstreamProvider::Sap
        | UpstreamProvider::Alibaba
        | UpstreamProvider::Qianfan
        | UpstreamProvider::Glm
        | UpstreamProvider::Kimi
        | UpstreamProvider::Minimax
        | UpstreamProvider::Moonshot
        | UpstreamProvider::Volcengine
        | UpstreamProvider::Doubao
        | UpstreamProvider::Tencent
        | UpstreamProvider::Iflytek
        | UpstreamProvider::Baichuan
        | UpstreamProvider::Yi
        | UpstreamProvider::Stepfun
        | UpstreamProvider::Ai360
        | UpstreamProvider::Sensenova
        | UpstreamProvider::Sparkdesk
        | UpstreamProvider::Coze
        | UpstreamProvider::Baidu
        | UpstreamProvider::DeepInfra
        | UpstreamProvider::LambdaAi
        | UpstreamProvider::Sambanova
        | UpstreamProvider::Nscale
        | UpstreamProvider::Ovhcloud
        | UpstreamProvider::Baseten
        | UpstreamProvider::Databricks
        | UpstreamProvider::Snowflake
        | UpstreamProvider::Wandb
        | UpstreamProvider::Ai21
        | UpstreamProvider::Gigachat
        | UpstreamProvider::Venice
        | UpstreamProvider::Codestral
        | UpstreamProvider::Upstage
        | UpstreamProvider::Maritalk
        | UpstreamProvider::Modal
        | UpstreamProvider::Huggingface
        | UpstreamProvider::GitHubModels
        | UpstreamProvider::VercelAiGateway
        | UpstreamProvider::MetaLlama
        | UpstreamProvider::V0Vercel
        | UpstreamProvider::Morph
        | UpstreamProvider::FeatherlessAi
        | UpstreamProvider::Llm7
        | UpstreamProvider::Lepton
        | UpstreamProvider::Kluster
        | UpstreamProvider::Friendliai
        | UpstreamProvider::Llamagate
        | UpstreamProvider::Heroku
        | UpstreamProvider::Galadriel
        | UpstreamProvider::Datarobot
        | UpstreamProvider::Clarifai
        | UpstreamProvider::Gitlawb
        | UpstreamProvider::InferenceNet
        | UpstreamProvider::Nanogpt
        | UpstreamProvider::Predibase
        | UpstreamProvider::Bytez
        | UpstreamProvider::Aimlapi
        | UpstreamProvider::Novita
        | UpstreamProvider::Piapi
        | UpstreamProvider::Getgoapi
        | UpstreamProvider::Laozhang
        | UpstreamProvider::Glhf
        | UpstreamProvider::Cablyai
        | UpstreamProvider::Thebai
        | UpstreamProvider::Fenayai
        | UpstreamProvider::Empower
        | UpstreamProvider::NousResearch
        | UpstreamProvider::Petals
        | UpstreamProvider::Poe
        | UpstreamProvider::Gitlab
        | UpstreamProvider::Chutes
        | UpstreamProvider::VoyageAi
        | UpstreamProvider::JinaAi
        | UpstreamProvider::FalAi
        | UpstreamProvider::StabilityAi
        | UpstreamProvider::BlackForestLabs
        | UpstreamProvider::Recraft
        | UpstreamProvider::Poolside
        | UpstreamProvider::ArceeAi
        | UpstreamProvider::Inclusionai
        | UpstreamProvider::Liquid
        | UpstreamProvider::Nomic
        | UpstreamProvider::Krutrim
        | UpstreamProvider::Monsterapi
        | UpstreamProvider::Byteplus
        | UpstreamProvider::Bluesminds
        | UpstreamProvider::FreemodelDev
        | UpstreamProvider::Blackbox
        | UpstreamProvider::Bazaarlink
        | UpstreamProvider::Completions
        | UpstreamProvider::Enally
        | UpstreamProvider::Freetheai
        | UpstreamProvider::Crof
        | UpstreamProvider::Longcat
        | UpstreamProvider::Pollinations
        | UpstreamProvider::Puter
        | UpstreamProvider::Uncloseai
        | UpstreamProvider::Replicate
        | UpstreamProvider::OllamaCloud
        | UpstreamProvider::Agentrouter
        | UpstreamProvider::CommandCode
        | UpstreamProvider::Astraflow
        | UpstreamProvider::OpencodeZen
        | UpstreamProvider::OpencodeGo
        | UpstreamProvider::Zai
        | UpstreamProvider::Phind
        | UpstreamProvider::Huggingchat
        | UpstreamProvider::Dify
        | UpstreamProvider::Publicai
        | UpstreamProvider::Sapio
        | UpstreamProvider::Freeaiapikey
        | UpstreamProvider::Other => RequestPipeline::GenericRelay,
    }
}
```

## 验收标准

- [ ] `cargo check -p crab-pipeline` 编译通过（match 穷尽性检查）
- [ ] `cargo test -p crab-pipeline` 全部通过
- [ ] DeepSeek/MiMo/Codex 仍走原有特殊 pipeline
- [ ] 所有新增供应商统一走 `GenericRelay`

## 依赖

- 依赖任务 01（`UpstreamProvider` 枚举扩展）

## 注意事项

- `GenericRelay` 是纯透传 pipeline，适用于所有 OpenAI 兼容格式
- 未来如果某些供应商需要特殊处理（如 Gemini 的非 OpenAI 格式），可拆分为独立分支
- 使用 `|` 合并所有 GenericRelay 分支，避免 Clippy 警告
