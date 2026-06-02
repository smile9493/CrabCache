# 任务 01: 核心枚举扩展

> **Phase**: 1a | **优先级**: P0 | **状态**: 待开始
> **预计工作量**: 2-3 小时 | **风险**: 低

## 目标

扩展 `UpstreamProvider` 枚举，添加所有 OmniRoute API Key 供应商变体。

## 修改文件

- `crates/crab-pipeline/src/types.rs`

## 详细步骤

### Step 1: 扩展 UpstreamProvider 枚举

在现有 6 个变体基础上，按分类添加新变体：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProvider {
    // ── 原有 ──
    Deepseek,
    Mimo,
    Openai,
    Codex,
    Anthropic,

    // ── 国际主流 ──
    Groq,
    Xai,
    Mistral,
    Gemini,
    Perplexity,
    Together,
    Fireworks,
    Cerebras,
    Cohere,
    Nvidia,
    Nebius,
    Siliconflow,
    Hyperbolic,
    OpenRouter,
    Reka,

    // ── 云平台 ──
    AzureOpenai,
    AzureAi,
    Bedrock,
    VertexAi,
    Watsonx,
    Oci,
    Sap,

    // ── 中国供应商 ──
    Alibaba,
    Qianfan,
    Glm,
    Kimi,
    Minimax,
    Moonshot,
    Volcengine,
    Doubao,
    Tencent,
    Iflytek,
    Baichuan,
    Yi,
    Stepfun,
    Ai360,
    Sensenova,
    Sparkdesk,
    Coze,
    Baidu,

    // ── 推理平台 ──
    DeepInfra,
    LambdaAi,
    Sambanova,
    Nscale,
    Ovhcloud,
    Baseten,
    Databricks,
    Snowflake,
    Wandb,
    Ai21,
    Gigachat,
    Venice,
    Codestral,
    Upstage,
    Maritalk,
    Modal,
    Huggingface,
    GitHubModels,
    VercelAiGateway,
    MetaLlama,
    V0Vercel,
    Morph,
    FeatherlessAi,
    Llm7,
    Lepton,
    Kluster,
    Friendliai,
    Llamagate,
    Heroku,
    Galadriel,
    Datarobot,
    Clarifai,
    Gitlawb,
    InferenceNet,
    Nanogpt,
    Predibase,
    Bytez,
    Aimlapi,
    Novita,
    Piapi,
    Getgoapi,
    Laozhang,
    Glhf,
    Cablyai,
    Thebai,
    Fenayai,
    Empower,
    NousResearch,
    Petals,
    Poe,
    Gitlab,
    Chutes,
    VoyageAi,
    JinaAi,
    FalAi,
    StabilityAi,
    BlackForestLabs,
    Recraft,
    Poolside,
    ArceeAi,
    Inclusionai,
    Liquid,
    Nomic,
    Krutrim,
    Monsterapi,
    Byteplus,
    Bluesminds,
    FreemodelDev,
    Blackbox,
    Bazaarlink,
    Completions,
    Enally,
    Freetheai,
    Crof,
    Longcat,
    Pollinations,
    Puter,
    Uncloseai,
    Replicate,
    OllamaCloud,
    Agentrouter,
    CommandCode,
    Astraflow,
    OpencodeZen,
    OpencodeGo,
    Zai,
    Phind,
    Huggingchat,
    Dify,
    Publicai,
    Sapio,
    Freeaiapikey,

    // ── 保留兜底 ──
    Other,
}
```

### Step 2: 更新 from_str() 方法

为每个新变体添加字符串解析分支：

```rust
pub fn from_str(s: &str) -> Self {
    match s.to_lowercase().as_str() {
        // 原有
        "deepseek" => Self::Deepseek,
        "mimo" | "xiaomi" => Self::Mimo,
        "openai" => Self::Openai,
        "codex" => Self::Codex,
        "anthropic" => Self::Anthropic,

        // 国际主流
        "groq" => Self::Groq,
        "xai" | "grok" => Self::Xai,
        "mistral" => Self::Mistral,
        "gemini" | "google" => Self::Gemini,
        "perplexity" => Self::Perplexity,
        "together" => Self::Together,
        "fireworks" => Self::Fireworks,
        "cerebras" => Self::Cerebras,
        "cohere" => Self::Cohere,
        "nvidia" | "nim" => Self::Nvidia,
        "nebius" => Self::Nebius,
        "siliconflow" => Self::Siliconflow,
        "hyperbolic" => Self::Hyperbolic,
        "openrouter" => Self::OpenRouter,
        "reka" => Self::Reka,

        // 云平台
        "azure-openai" => Self::AzureOpenai,
        "azure-ai" | "azure-ai-foundry" => Self::AzureAi,
        "bedrock" | "aws" => Self::Bedrock,
        "vertex" | "vertex-ai" => Self::VertexAi,
        "watsonx" | "ibm" => Self::Watsonx,
        "oci" | "oracle" => Self::Oci,
        "sap" => Self::Sap,

        // 中国供应商
        "alibaba" | "qwen" | "dashscope" => Self::Alibaba,
        "qianfan" | "baidu-cloud" => Self::Qianfan,
        "glm" | "zhipu" | "bigmodel" => Self::Glm,
        "kimi" | "moonshot-ai" => Self::Kimi,
        "minimax" => Self::Minimax,
        "moonshot" => Self::Moonshot,
        "volcengine" | "volc" => Self::Volcengine,
        "doubao" => Self::Doubao,
        "tencent" | "hunyuan" => Self::Tencent,
        "iflytek" | "spark" | "xinghuo" => Self::Iflytek,
        "baichuan" => Self::Baichuan,
        "yi" | "01-ai" | "lingyiwanwu" => Self::Yi,
        "stepfun" | "step" => Self::Stepfun,
        "360ai" | "360" => Self::Ai360,
        "sensenova" | "sensetime" => Self::Sensenova,
        "sparkdesk" => Self::Sparkdesk,
        "coze" => Self::Coze,
        "baidu" | "ernie" | "yiyan" => Self::Baidu,

        // 推理平台
        "deepinfra" => Self::DeepInfra,
        "lambda" | "lambda-ai" => Self::LambdaAi,
        "sambanova" => Self::Sambanova,
        "nscale" => Self::Nscale,
        "ovhcloud" | "ovh" => Self::Ovhcloud,
        "baseten" => Self::Baseten,
        "databricks" => Self::Databricks,
        "snowflake" => Self::Snowflake,
        "wandb" | "weights-biases" => Self::Wandb,
        "ai21" => Self::Ai21,
        "gigachat" | "sber" => Self::Gigachat,
        "venice" => Self::Venice,
        "codestral" => Self::Codestral,
        "upstage" => Self::Upstage,
        "maritalk" | "maritaca" => Self::Maritalk,
        "modal" => Self::Modal,
        "huggingface" | "hf" => Self::Huggingface,
        "github-models" => Self::GitHubModels,
        "vercel-ai-gateway" | "vercel" => Self::VercelAiGateway,
        "meta-llama" | "meta" => Self::MetaLlama,
        "v0-vercel" | "v0" => Self::V0Vercel,
        "morph" => Self::Morph,
        "featherless" | "featherless-ai" => Self::FeatherlessAi,
        "llm7" => Self::Llm7,
        "lepton" => Self::Lepton,
        "kluster" => Self::Kluster,
        "friendli" | "friendliai" => Self::Friendliai,
        "llamagate" => Self::Llamagate,
        "heroku" => Self::Heroku,
        "galadriel" => Self::Galadriel,
        "datarobot" => Self::Datarobot,
        "clarifai" => Self::Clarifai,
        "gitlawb" => Self::Gitlawb,
        "inference-net" => Self::InferenceNet,
        "nanogpt" => Self::Nanogpt,
        "predibase" => Self::Predibase,
        "bytez" => Self::Bytez,
        "aimlapi" | "aiml" => Self::Aimlapi,
        "novita" => Self::Novita,
        "piapi" => Self::Piapi,
        "getgoapi" | "goapi" => Self::Getgoapi,
        "laozhang" => Self::Laozhang,
        "glhf" => Self::Glhf,
        "cablyai" => Self::Cablyai,
        "thebai" => Self::Thebai,
        "fenayai" => Self::Fenayai,
        "empower" => Self::Empower,
        "nous-research" | "nous" => Self::NousResearch,
        "petals" => Self::Petals,
        "poe" => Self::Poe,
        "gitlab" | "gitlab-duo-pat" => Self::Gitlab,
        "chutes" => Self::Chutes,
        "voyage-ai" | "voyage" => Self::VoyageAi,
        "jina-ai" | "jina" => Self::JinaAi,
        "fal-ai" | "fal" => Self::FalAi,
        "stability-ai" | "stability" => Self::StabilityAi,
        "black-forest-labs" | "bfl" => Self::BlackForestLabs,
        "recraft" => Self::Recraft,
        "poolside" => Self::Poolside,
        "arcee" | "arcee-ai" => Self::ArceeAi,
        "inclusionai" | "inclusion" => Self::Inclusionai,
        "liquid" | "liquid-ai" => Self::Liquid,
        "nomic" => Self::Nomic,
        "krutrim" => Self::Krutrim,
        "monsterapi" | "monster" => Self::Monsterapi,
        "byteplus" => Self::Byteplus,
        "bluesminds" | "blues" => Self::Bluesminds,
        "freemodel-dev" | "freemodel" => Self::FreemodelDev,
        "blackbox" => Self::Blackbox,
        "bazaarlink" => Self::Bazaarlink,
        "completions" | "completions-me" => Self::Completions,
        "enally" => Self::Enally,
        "freetheai" => Self::Freetheai,
        "crof" | "crofai" => Self::Crof,
        "longcat" => Self::Longcat,
        "pollinations" => Self::Pollinations,
        "puter" => Self::Puter,
        "uncloseai" => Self::Uncloseai,
        "replicate" => Self::Replicate,
        "ollama-cloud" | "ollama" => Self::OllamaCloud,
        "agentrouter" => Self::Agentrouter,
        "command-code" | "cmd" => Self::CommandCode,
        "astraflow" => Self::Astraflow,
        "opencode-zen" => Self::OpencodeZen,
        "opencode-go" => Self::OpencodeGo,
        "zai" | "z-ai" => Self::Zai,
        "phind" => Self::Phind,
        "huggingchat" => Self::Huggingchat,
        "dify" => Self::Dify,
        "publicai" => Self::Publicai,
        "sapio" => Self::Sapio,
        "freeaiapikey" => Self::Freeaiapikey,

        _ => Self::Other,
    }
}
```

### Step 3: 更新 as_str() 方法

为每个新变体添加字符串输出：

```rust
pub fn as_str(self) -> &'static str {
    match self {
        Self::Deepseek => "deepseek",
        Self::Mimo => "mimo",
        // ... 所有变体
        Self::Other => "other",
    }
}
```

### Step 4: 更新 uses_oauth() 方法

当前仅 `Codex` 使用 OAuth，保持不变：

```rust
pub fn uses_oauth(self) -> bool {
    matches!(self, Self::Codex)
}
```

## 验收标准

- [ ] `cargo check -p crab-pipeline` 编译通过
- [ ] `cargo clippy -p crab-pipeline` 无新增警告
- [ ] `from_str("groq")` 返回 `UpstreamProvider::Groq`
- `from_str("unknown")` 返回 `UpstreamProvider::Other`
- [ ] 所有变体的 `as_str()` 返回值与 `from_str()` 互逆

## 依赖

无前置依赖。

## 注意事项

- 使用 `#[allow(clippy::match_same_arms)]` 抑制 match 分支合并警告（如果需要保留独立分支用于未来扩展）
- 或者使用 `|` 合并格式相同的分支（如所有走 GenericRelay 的供应商）
