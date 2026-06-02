use crate::client_kind::ClientKind;
use crate::cursor_models::CursorModelsConfig;
use crate::rule_engine::PipelineRuleEngine;
use crab_translator::WireFormat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestPipeline {
    CursorDeepSeekV4,
    DeepSeekLight,
    /// MiMo Chat-Completions direct passthrough relay (token-plan & payg unified).
    MimoTokenPlanRelay,
    GenericRelay,
    /// Codex (ChatGPT) Responses API relay — translates Chat Completions ↔ Responses API.
    CodexRelay,
    /// Codex client → DeepSeek upstream: Responses API ↔ Chat Completions with DeepSeek normalization.
    CodexDeepSeek,
    /// Codex client → MiMo upstream: Responses API ↔ Chat Completions with MiMo normalization.
    CodexMimo,
}

impl RequestPipeline {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestPipeline::CursorDeepSeekV4 => "cursor_deepseek_v4",
            RequestPipeline::DeepSeekLight => "deepseek_light",
            RequestPipeline::MimoTokenPlanRelay => "mimo_token_plan_relay",
            RequestPipeline::GenericRelay => "generic_relay",
            RequestPipeline::CodexRelay => "codex_relay",
            RequestPipeline::CodexDeepSeek => "codex_deepseek",
            RequestPipeline::CodexMimo => "codex_mimo",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cursor_deepseek_v4" => Self::CursorDeepSeekV4,
            "deepseek_light" => Self::DeepSeekLight,
            "mimo_token_plan_relay" | "mimo_payg_relay" => Self::MimoTokenPlanRelay,
            "generic_relay" => Self::GenericRelay,
            "codex_relay" => Self::CodexRelay,
            "codex_deepseek" => Self::CodexDeepSeek,
            "codex_mimo" => Self::CodexMimo,
            _ => Self::GenericRelay,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProvider {
    // ── Original ──
    Deepseek,
    Mimo,
    Openai,
    Codex,
    Anthropic,

    // ── International mainstream ──
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

    // ── Cloud platforms ──
    AzureOpenai,
    AzureAi,
    Bedrock,
    VertexAi,
    Watsonx,
    Oci,
    Sap,

    // ── China providers ──
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

    // ── Inference platforms ──
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

    // ── Fallback ──
    Other,
}

impl UpstreamProvider {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            // Original
            "deepseek" => Self::Deepseek,
            "mimo" => Self::Mimo,
            "openai" => Self::Openai,
            "codex" => Self::Codex,
            "anthropic" => Self::Anthropic,

            // International mainstream
            "groq" => Self::Groq,
            "xai" | "grok" => Self::Xai,
            "mistral" => Self::Mistral,
            "gemini" | "google" => Self::Gemini,
            "perplexity" | "pplx" => Self::Perplexity,
            "together" => Self::Together,
            "fireworks" => Self::Fireworks,
            "cerebras" => Self::Cerebras,
            "cohere" => Self::Cohere,
            "nvidia" | "nim" => Self::Nvidia,
            "nebius" => Self::Nebius,
            "siliconflow" => Self::Siliconflow,
            "hyperbolic" | "hyp" => Self::Hyperbolic,
            "openrouter" => Self::OpenRouter,
            "reka" => Self::Reka,

            // Cloud platforms
            "azure-openai" => Self::AzureOpenai,
            "azure-ai" | "azure-ai-foundry" => Self::AzureAi,
            "bedrock" | "aws" => Self::Bedrock,
            "vertex" | "vertex-ai" => Self::VertexAi,
            "watsonx" | "ibm" => Self::Watsonx,
            "oci" | "oracle" => Self::Oci,
            "sap" => Self::Sap,

            // China providers
            "alibaba" | "ali" | "qwen" | "dashscope" | "alibaba-cn" | "ali-cn" => Self::Alibaba,
            "qianfan" | "baidu-cloud" => Self::Qianfan,
            "glm" | "zhipu" | "bigmodel" | "glm-cn" | "glmcn" | "glmt" => Self::Glm,
            "kimi" | "moonshot-ai" | "kimi-coding-apikey" | "kmca" => Self::Kimi,
            "minimax" | "minimax-cn" => Self::Minimax,
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

            // Inference platforms
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
            "aimlapi" | "aiml" | "ai/ml-api" => Self::Aimlapi,
            "novita" | "novita-ai" => Self::Novita,
            "piapi" => Self::Piapi,
            "getgoapi" | "goapi" => Self::Getgoapi,
            "laozhang" | "laozhang-ai" => Self::Laozhang,
            "glhf" => Self::Glhf,
            "cablyai" => Self::Cablyai,
            "thebai" => Self::Thebai,
            "fenayai" => Self::Fenayai,
            "empower" => Self::Empower,
            "nous-research" | "nous" => Self::NousResearch,
            "petals" => Self::Petals,
            "poe" => Self::Poe,
            "gitlab" | "gitlab-duo-pat" => Self::Gitlab,
            "chutes" | "chutes-ai" => Self::Chutes,
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
            "blackbox" | "blackbox-ai" => Self::Blackbox,
            "bazaarlink" => Self::Bazaarlink,
            "completions" | "completions-me" => Self::Completions,
            "enally" | "enally-ai" => Self::Enally,
            "freetheai" | "free-the-ai" => Self::Freetheai,
            "crof" | "crofai" => Self::Crof,
            "longcat" | "longcat-ai" => Self::Longcat,
            "pollinations" => Self::Pollinations,
            "puter" | "puter-ai" => Self::Puter,
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
            "publicai" | "public-ai" => Self::Publicai,
            "sapio" => Self::Sapio,
            "freeaiapikey" | "free-ai-api-key" => Self::Freeaiapikey,

            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            // Original
            Self::Deepseek => "deepseek",
            Self::Mimo => "mimo",
            Self::Openai => "openai",
            Self::Codex => "codex",
            Self::Anthropic => "anthropic",

            // International mainstream
            Self::Groq => "groq",
            Self::Xai => "xai",
            Self::Mistral => "mistral",
            Self::Gemini => "gemini",
            Self::Perplexity => "perplexity",
            Self::Together => "together",
            Self::Fireworks => "fireworks",
            Self::Cerebras => "cerebras",
            Self::Cohere => "cohere",
            Self::Nvidia => "nvidia",
            Self::Nebius => "nebius",
            Self::Siliconflow => "siliconflow",
            Self::Hyperbolic => "hyperbolic",
            Self::OpenRouter => "openrouter",
            Self::Reka => "reka",

            // Cloud platforms
            Self::AzureOpenai => "azure-openai",
            Self::AzureAi => "azure-ai",
            Self::Bedrock => "bedrock",
            Self::VertexAi => "vertex-ai",
            Self::Watsonx => "watsonx",
            Self::Oci => "oci",
            Self::Sap => "sap",

            // China providers
            Self::Alibaba => "alibaba",
            Self::Qianfan => "qianfan",
            Self::Glm => "glm",
            Self::Kimi => "kimi",
            Self::Minimax => "minimax",
            Self::Moonshot => "moonshot",
            Self::Volcengine => "volcengine",
            Self::Doubao => "doubao",
            Self::Tencent => "tencent",
            Self::Iflytek => "iflytek",
            Self::Baichuan => "baichuan",
            Self::Yi => "yi",
            Self::Stepfun => "stepfun",
            Self::Ai360 => "360ai",
            Self::Sensenova => "sensenova",
            Self::Sparkdesk => "sparkdesk",
            Self::Coze => "coze",
            Self::Baidu => "baidu",

            // Inference platforms
            Self::DeepInfra => "deepinfra",
            Self::LambdaAi => "lambda-ai",
            Self::Sambanova => "sambanova",
            Self::Nscale => "nscale",
            Self::Ovhcloud => "ovhcloud",
            Self::Baseten => "baseten",
            Self::Databricks => "databricks",
            Self::Snowflake => "snowflake",
            Self::Wandb => "wandb",
            Self::Ai21 => "ai21",
            Self::Gigachat => "gigachat",
            Self::Venice => "venice",
            Self::Codestral => "codestral",
            Self::Upstage => "upstage",
            Self::Maritalk => "maritalk",
            Self::Modal => "modal",
            Self::Huggingface => "huggingface",
            Self::GitHubModels => "github-models",
            Self::VercelAiGateway => "vercel-ai-gateway",
            Self::MetaLlama => "meta-llama",
            Self::V0Vercel => "v0-vercel",
            Self::Morph => "morph",
            Self::FeatherlessAi => "featherless-ai",
            Self::Llm7 => "llm7",
            Self::Lepton => "lepton",
            Self::Kluster => "kluster",
            Self::Friendliai => "friendliai",
            Self::Llamagate => "llamagate",
            Self::Heroku => "heroku",
            Self::Galadriel => "galadriel",
            Self::Datarobot => "datarobot",
            Self::Clarifai => "clarifai",
            Self::Gitlawb => "gitlawb",
            Self::InferenceNet => "inference-net",
            Self::Nanogpt => "nanogpt",
            Self::Predibase => "predibase",
            Self::Bytez => "bytez",
            Self::Aimlapi => "aimlapi",
            Self::Novita => "novita",
            Self::Piapi => "piapi",
            Self::Getgoapi => "getgoapi",
            Self::Laozhang => "laozhang",
            Self::Glhf => "glhf",
            Self::Cablyai => "cablyai",
            Self::Thebai => "thebai",
            Self::Fenayai => "fenayai",
            Self::Empower => "empower",
            Self::NousResearch => "nous-research",
            Self::Petals => "petals",
            Self::Poe => "poe",
            Self::Gitlab => "gitlab",
            Self::Chutes => "chutes",
            Self::VoyageAi => "voyage-ai",
            Self::JinaAi => "jina-ai",
            Self::FalAi => "fal-ai",
            Self::StabilityAi => "stability-ai",
            Self::BlackForestLabs => "black-forest-labs",
            Self::Recraft => "recraft",
            Self::Poolside => "poolside",
            Self::ArceeAi => "arcee-ai",
            Self::Inclusionai => "inclusionai",
            Self::Liquid => "liquid",
            Self::Nomic => "nomic",
            Self::Krutrim => "krutrim",
            Self::Monsterapi => "monsterapi",
            Self::Byteplus => "byteplus",
            Self::Bluesminds => "bluesminds",
            Self::FreemodelDev => "freemodel-dev",
            Self::Blackbox => "blackbox",
            Self::Bazaarlink => "bazaarlink",
            Self::Completions => "completions",
            Self::Enally => "enally",
            Self::Freetheai => "freetheai",
            Self::Crof => "crof",
            Self::Longcat => "longcat",
            Self::Pollinations => "pollinations",
            Self::Puter => "puter",
            Self::Uncloseai => "uncloseai",
            Self::Replicate => "replicate",
            Self::OllamaCloud => "ollama-cloud",
            Self::Agentrouter => "agentrouter",
            Self::CommandCode => "command-code",
            Self::Astraflow => "astraflow",
            Self::OpencodeZen => "opencode-zen",
            Self::OpencodeGo => "opencode-go",
            Self::Zai => "zai",
            Self::Phind => "phind",
            Self::Huggingchat => "huggingchat",
            Self::Dify => "dify",
            Self::Publicai => "publicai",
            Self::Sapio => "sapio",
            Self::Freeaiapikey => "freeaiapikey",

            Self::Other => "other",
        }
    }

    /// Returns `true` for providers that use OAuth tokens instead of API keys
    /// (e.g. Codex uses OpenAI OAuth tokens validated via `chatgpt-account-id`).
    pub fn uses_oauth(self) -> bool {
        matches!(self, Self::Codex)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineOverride {
    #[default]
    Auto,
    CursorDeepSeekV4,
    DeepSeekLight,
    /// MiMo Chat-Completions direct passthrough relay (token-plan & payg unified).
    MimoTokenPlanRelay,
    GenericRelay,
    CodexRelay,
    CodexDeepSeek,
    CodexMimo,
}

impl PipelineOverride {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cursor_deepseek_v4" => Self::CursorDeepSeekV4,
            "deepseek_light" => Self::DeepSeekLight,
            "mimo_token_plan_relay" | "mimo_payg_relay" => Self::MimoTokenPlanRelay,
            "generic_relay" => Self::GenericRelay,
            "codex_relay" => Self::CodexRelay,
            "codex_deepseek" => Self::CodexDeepSeek,
            "codex_mimo" => Self::CodexMimo,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PipelineOverride::Auto => "auto",
            PipelineOverride::CursorDeepSeekV4 => "cursor_deepseek_v4",
            PipelineOverride::DeepSeekLight => "deepseek_light",
            PipelineOverride::MimoTokenPlanRelay => "mimo_token_plan_relay",
            PipelineOverride::GenericRelay => "generic_relay",
            PipelineOverride::CodexRelay => "codex_relay",
            PipelineOverride::CodexDeepSeek => "codex_deepseek",
            PipelineOverride::CodexMimo => "codex_mimo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMode {
    Auto,
    ForceCursorV4,
}

impl PipelineMode {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "force_cursor_v4" => Self::ForceCursorV4,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PipelineMode::Auto => "auto",
            PipelineMode::ForceCursorV4 => "force_cursor_v4",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineSelectionReason {
    GlobalForceCursorV4,
    KeyOverride,
    DomainOverride,
    ProviderDefault,
    ModelPrefixProfile,
    CursorSignals,
    DeepSeekNonV4,
    MimoProvider,
    CodexProvider,
    ModelAlias,
    CodexDeepSeekProvider,
    CodexMimoProvider,
    /// Selected by the declarative rule engine (rule name recorded).
    RuleEngine(String),
}

impl PipelineSelectionReason {
    pub fn as_str(&self) -> &str {
        match self {
            PipelineSelectionReason::GlobalForceCursorV4 => "global_force_cursor_v4",
            PipelineSelectionReason::KeyOverride => "key_override",
            PipelineSelectionReason::DomainOverride => "domain_override",
            PipelineSelectionReason::ProviderDefault => "provider_default",
            PipelineSelectionReason::ModelPrefixProfile => "model_prefix_profile",
            PipelineSelectionReason::CursorSignals => "cursor_signals",
            PipelineSelectionReason::DeepSeekNonV4 => "deepseek_non_v4",
            PipelineSelectionReason::MimoProvider => "mimo_provider",
            PipelineSelectionReason::CodexProvider => "codex_provider",
            PipelineSelectionReason::ModelAlias => "model_alias",
            PipelineSelectionReason::CodexDeepSeekProvider => "codex_deepseek_provider",
            PipelineSelectionReason::CodexMimoProvider => "codex_mimo_provider",
            PipelineSelectionReason::RuleEngine(name) => name.as_str(),
        }
    }
}

/// Static Codex model catalog (aligned with new-api `relay/channel/codex/constants.go`).
pub const CODEX_STATIC_MODELS: &[&str] = &[
    "gpt-5",
    "gpt-5.5",
    "gpt-5-codex",
    "gpt-5-codex-mini",
    "gpt-5.1",
    "gpt-5.1-codex",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5.2",
    "gpt-5.2-codex",
    "gpt-5.3-codex",
    "gpt-5.3-codex-spark",
    "gpt-5.4",
    "gpt-5/compact",
    "gpt-5-codex/compact",
    "gpt-5-codex-mini/compact",
    "gpt-5.1/compact",
    "gpt-5.1-codex/compact",
    "gpt-5.1-codex-max/compact",
    "gpt-5.1-codex-mini/compact",
    "gpt-5.2/compact",
    "gpt-5.2-codex/compact",
    "gpt-5.3-codex/compact",
    "gpt-5.3-codex-spark/compact",
    "gpt-5.4/compact",
];

#[derive(Debug, Clone)]
pub struct PipelineGlobals {
    pub default_upstream_profile: String,
    pub pipeline_mode: PipelineMode,
    pub known_profile_ids: Vec<String>,
    pub cursor_models: CursorModelsConfig,
    /// Declarative rule engine (None = use legacy if/match logic).
    pub rule_engine: Option<PipelineRuleEngine>,
}

impl Default for PipelineGlobals {
    fn default() -> Self {
        Self {
            default_upstream_profile: "deepseek".to_string(),
            pipeline_mode: PipelineMode::Auto,
            known_profile_ids: vec!["deepseek".to_string()],
            cursor_models: CursorModelsConfig::default(),
            rule_engine: None,
        }
    }
}

impl PipelineGlobals {
    pub fn with_profiles_and_mode(
        default_id: impl Into<String>,
        profile_ids: impl IntoIterator<Item = String>,
        pipeline_mode: PipelineMode,
    ) -> Self {
        let default_upstream_profile = default_id.into();
        let mut known_profile_ids: Vec<String> = profile_ids.into_iter().collect();
        if !known_profile_ids
            .iter()
            .any(|id| id == &default_upstream_profile)
        {
            known_profile_ids.push(default_upstream_profile.clone());
        }
        Self {
            default_upstream_profile,
            pipeline_mode,
            known_profile_ids,
            cursor_models: CursorModelsConfig::default(),
            rule_engine: None,
        }
    }

    pub fn with_profiles_mode_and_cursor_models(
        default_id: impl Into<String>,
        profile_ids: impl IntoIterator<Item = String>,
        pipeline_mode: PipelineMode,
        cursor_models: CursorModelsConfig,
    ) -> Self {
        let mut g = Self::with_profiles_and_mode(default_id, profile_ids, pipeline_mode);
        g.cursor_models = cursor_models;
        g
    }

    pub fn with_profiles_mode_cursor_models_and_rule_engine(
        default_id: impl Into<String>,
        profile_ids: impl IntoIterator<Item = String>,
        pipeline_mode: PipelineMode,
        cursor_models: CursorModelsConfig,
        rule_engine: Option<PipelineRuleEngine>,
    ) -> Self {
        let mut g = Self::with_profiles_mode_and_cursor_models(
            default_id,
            profile_ids,
            pipeline_mode,
            cursor_models,
        );
        g.rule_engine = rule_engine;
        g
    }
}

/// Per-request hints for pipeline / profile resolution.
#[derive(Debug, Clone, Default)]
pub struct PipelineRequestContext<'a> {
    pub model: &'a str,
    pub payload: Option<&'a serde_json::Value>,
    pub key_pipeline: Option<PipelineOverride>,
    pub key_upstream_profile: Option<&'a str>,
    pub domain_pipeline: Option<PipelineOverride>,
    pub domain_upstream_profile: Option<&'a str>,
    pub conversation_id_header: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    /// Resolved from `[gateway.cursor_models]` for outbound `model` field.
    pub alias_upstream_model: Option<&'a str>,
    /// Pipeline hint from alias entry (`auto` uses legacy rules).
    pub model_alias_pipeline: Option<PipelineOverride>,
    /// Detected client kind (from `ClientDetector`). `None` = not yet detected.
    pub client_kind: Option<ClientKind>,
    /// Detected wire format (from path + method). `None` = ChatCompletions (default).
    pub wire_format: Option<WireFormat>,
}

#[derive(Debug, Clone)]
pub struct PipelineSelection {
    pub pipeline: RequestPipeline,
    pub upstream_profile_id: String,
    pub provider: UpstreamProvider,
    pub reason: PipelineSelectionReason,
    pub client_kind: ClientKind,
}

#[derive(Debug, Clone)]
pub struct ProfileDescriptor {
    pub id: String,
    pub provider: UpstreamProvider,
}

impl PipelineGlobals {
    pub fn with_profiles(
        default_id: impl Into<String>,
        profile_ids: impl IntoIterator<Item = String>,
    ) -> Self {
        Self::with_profiles_and_mode(default_id, profile_ids, PipelineMode::Auto)
    }
}

#[allow(dead_code)]
pub type ModelPrefixProfileMap = HashMap<String, String>;

/// Routing strategy for selecting among upstream backends.
///
/// This enum is used in pipeline configuration to specify the load balancing
/// algorithm. It maps to `BackendRouteStrategy` in the proxy layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    /// Ketama consistent hashing — session affinity via hash ring.
    Ketama,
    /// Power of Two Choices — randomly pick two backends, choose the better one.
    P2c,
    /// Cost-optimized — balance load and cost weight.
    CostOptimized,
}

impl Default for RoutingStrategy {
    fn default() -> Self {
        Self::Ketama
    }
}

impl RoutingStrategy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "p2c" | "power_of_two" | "power-of-two" => Self::P2c,
            "cost_optimized" | "cost-optimized" | "eco" => Self::CostOptimized,
            _ => Self::Ketama,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ketama => "ketama",
            Self::P2c => "p2c",
            Self::CostOptimized => "cost_optimized",
        }
    }
}
