use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;

use crate::api;
use crate::components::codex_oauth_panel::CodexOAuthPanel;
use crate::components::routing_tab::RoutingTab;
use crate::components::skeleton::SkeletonUpstreamProfileCard;
use crate::components::sync_result::SyncResultCard;
use crate::components::upstream_key_pool_cards::UpstreamKeyPoolCards;
use crate::components::ui::*;

/// Incremented by [`CodexOAuthPanel`] when a credential is imported,
/// so the upstream page effect can re-load the key pool.
pub static KEY_POOL_REFRESH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Signal the upstream page to refresh the key pool for the current profile.
pub fn signal_refresh_key_pool() {
    KEY_POOL_REFRESH_COUNTER.fetch_add(1, Ordering::Relaxed);
}
use crate::locale::use_translations;
use crate::types::{
    KeyQuotaInfo, PatchUpstreamKeyRequest, PutUpstreamKeysRequest, PutUpstreamProfileAdminRequest,
    SyncResult, UpstreamKeyInput, UpstreamKeysPutMode,
    UpstreamKeysView, UpstreamProfileAdminView, UpstreamTestBody, UpstreamTestResult,
};

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
    // ── Original ──
    PresetTemplate {
        id: "deepseek",
        label_zh: "DeepSeek 官方",
        label_en: "DeepSeek Official",
        provider: "deepseek",
        base_url: "https://api.deepseek.com",
        models: &[
            "deepseek-v4-pro",
            "deepseek-v4-flash",
            "deepseek-v4-flash-max",
            "deepseek-chat",
        ],
        default_model: "deepseek-v4-pro",
        tls_sni: "api.deepseek.com",
    },
    PresetTemplate {
        id: "mimo",
        label_zh: "MiMo",
        label_en: "MiMo",
        provider: "mimo",
        base_url: "https://api.xiaomimimo.com",
        models: &["mimo-v2.5-pro", "mimo-v2-flash"],
        default_model: "mimo-v2.5-pro",
        tls_sni: "api.xiaomimimo.com",
    },
    PresetTemplate {
        id: "mimo-tp-cn",
        label_zh: "MiMo TP CN",
        label_en: "MiMo TP CN",
        provider: "mimo",
        base_url: "https://token-plan-cn.xiaomimimo.com",
        models: &["mimo-v2.5-pro", "mimo-v2-flash"],
        default_model: "mimo-v2.5-pro",
        tls_sni: "token-plan-cn.xiaomimimo.com",
    },
    PresetTemplate {
        id: "mimo-tp-sgp",
        label_zh: "MiMo TP SGP",
        label_en: "MiMo TP SGP",
        provider: "mimo",
        base_url: "https://token-plan-sgp.xiaomimimo.com",
        models: &["mimo-v2.5-pro", "mimo-v2-flash"],
        default_model: "mimo-v2.5-pro",
        tls_sni: "token-plan-sgp.xiaomimimo.com",
    },
    PresetTemplate {
        id: "openai",
        label_zh: "OpenAI",
        label_en: "OpenAI",
        provider: "openai",
        base_url: "https://api.openai.com",
        models: &["gpt-5", "gpt-5-mini", "gpt-4o", "gpt-4-turbo"],
        default_model: "gpt-5",
        tls_sni: "api.openai.com",
    },
    PresetTemplate {
        id: "anthropic",
        label_zh: "Anthropic (Claude)",
        label_en: "Anthropic (Claude)",
        provider: "anthropic",
        base_url: "https://api.anthropic.com",
        models: &["claude-sonnet-4", "claude-opus-4", "claude-haiku-3.5"],
        default_model: "claude-sonnet-4",
        tls_sni: "api.anthropic.com",
    },
    PresetTemplate {
        id: "codex",
        label_zh: "OpenAI Codex",
        label_en: "OpenAI Codex",
        provider: "codex",
        base_url: "https://chatgpt.com",
        models: &[
            "gpt-5",
            "gpt-5-codex",
            "gpt-5.1-codex",
            "gpt-5.2-codex",
            "gpt-5.3-codex",
        ],
        default_model: "gpt-5-codex",
        tls_sni: "chatgpt.com",
    },
    // ── International Mainstream ──
    PresetTemplate {
        id: "groq",
        label_zh: "Groq",
        label_en: "Groq",
        provider: "groq",
        base_url: "https://api.groq.com/openai/v1",
        models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768"],
        default_model: "llama-3.3-70b-versatile",
        tls_sni: "api.groq.com",
    },
    PresetTemplate {
        id: "xai",
        label_zh: "xAI (Grok)",
        label_en: "xAI (Grok)",
        provider: "xai",
        base_url: "https://api.x.ai/v1",
        models: &["grok-3", "grok-3-mini", "grok-2"],
        default_model: "grok-3",
        tls_sni: "api.x.ai",
    },
    PresetTemplate {
        id: "mistral",
        label_zh: "Mistral",
        label_en: "Mistral",
        provider: "mistral",
        base_url: "https://api.mistral.ai/v1",
        models: &["mistral-large-latest", "mistral-medium-latest", "codestral-latest"],
        default_model: "mistral-large-latest",
        tls_sni: "api.mistral.ai",
    },
    PresetTemplate {
        id: "gemini",
        label_zh: "Google Gemini",
        label_en: "Google Gemini",
        provider: "gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        models: &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash"],
        default_model: "gemini-2.5-pro",
        tls_sni: "generativelanguage.googleapis.com",
    },
    PresetTemplate {
        id: "perplexity",
        label_zh: "Perplexity",
        label_en: "Perplexity",
        provider: "perplexity",
        base_url: "https://api.perplexity.ai",
        models: &["sonar-pro", "sonar", "sonar-small-online"],
        default_model: "sonar-pro",
        tls_sni: "api.perplexity.ai",
    },
    PresetTemplate {
        id: "together",
        label_zh: "Together AI",
        label_en: "Together AI",
        provider: "together",
        base_url: "https://api.together.xyz/v1",
        models: &["meta-llama/Llama-3.3-70B-Instruct-Turbo", "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo"],
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        tls_sni: "api.together.xyz",
    },
    PresetTemplate {
        id: "fireworks",
        label_zh: "Fireworks AI",
        label_en: "Fireworks AI",
        provider: "fireworks",
        base_url: "https://api.fireworks.ai/inference/v1",
        models: &["accounts/fireworks/models/llama-v3p3-70b-instruct", "accounts/fireworks/models/llama-v3p1-8b-instruct"],
        default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct",
        tls_sni: "api.fireworks.ai",
    },
    PresetTemplate {
        id: "cerebras",
        label_zh: "Cerebras",
        label_en: "Cerebras",
        provider: "cerebras",
        base_url: "https://api.cerebras.ai/v1",
        models: &["llama-3.3-70b", "llama-3.1-8b"],
        default_model: "llama-3.3-70b",
        tls_sni: "api.cerebras.ai",
    },
    PresetTemplate {
        id: "cohere",
        label_zh: "Cohere",
        label_en: "Cohere",
        provider: "cohere",
        base_url: "https://api.cohere.com/v2",
        models: &["command-a", "command-r-plus", "command-r"],
        default_model: "command-a",
        tls_sni: "api.cohere.com",
    },
    PresetTemplate {
        id: "nvidia",
        label_zh: "NVIDIA NIM",
        label_en: "NVIDIA NIM",
        provider: "nvidia",
        base_url: "https://integrate.api.nvidia.com/v1",
        models: &["meta/llama-3.3-70b-instruct", "meta/llama-3.1-8b-instruct"],
        default_model: "meta/llama-3.3-70b-instruct",
        tls_sni: "integrate.api.nvidia.com",
    },
    PresetTemplate {
        id: "nebius",
        label_zh: "Nebius AI",
        label_en: "Nebius AI",
        provider: "nebius",
        base_url: "https://api.studio.nebius.ai/v1",
        models: &["meta-llama/Meta-Llama-3.3-70B-Instruct", "meta-llama/Meta-Llama-3.1-8B-Instruct"],
        default_model: "meta-llama/Meta-Llama-3.3-70B-Instruct",
        tls_sni: "api.studio.nebius.ai",
    },
    PresetTemplate {
        id: "siliconflow",
        label_zh: "硅基流动",
        label_en: "SiliconFlow",
        provider: "siliconflow",
        base_url: "https://api.siliconflow.cn/v1",
        models: &["Qwen/Qwen2.5-72B-Instruct", "Qwen/Qwen2.5-14B-Instruct", "deepseek-ai/DeepSeek-V3"],
        default_model: "Qwen/Qwen2.5-72B-Instruct",
        tls_sni: "api.siliconflow.cn",
    },
    PresetTemplate {
        id: "hyperbolic",
        label_zh: "Hyperbolic",
        label_en: "Hyperbolic",
        provider: "hyperbolic",
        base_url: "https://api.hyperbolic.xyz/v1",
        models: &["meta-llama/Meta-Llama-3.1-70B-Instruct", "meta-llama/Meta-Llama-3.1-8B-Instruct"],
        default_model: "meta-llama/Meta-Llama-3.1-70B-Instruct",
        tls_sni: "api.hyperbolic.xyz",
    },
    PresetTemplate {
        id: "openrouter",
        label_zh: "OpenRouter",
        label_en: "OpenRouter",
        provider: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        models: &["anthropic/claude-sonnet-4", "google/gemini-2.5-pro", "meta-llama/llama-3.3-70b-instruct"],
        default_model: "anthropic/claude-sonnet-4",
        tls_sni: "openrouter.ai",
    },
    PresetTemplate {
        id: "reka",
        label_zh: "Reka",
        label_en: "Reka",
        provider: "reka",
        base_url: "https://api.reka.ai/v1",
        models: &["reka-core", "reka-flash", "reka-edge"],
        default_model: "reka-core",
        tls_sni: "api.reka.ai",
    },
    // ── Cloud Platforms ──
    PresetTemplate {
        id: "azure-openai",
        label_zh: "Azure OpenAI",
        label_en: "Azure OpenAI",
        provider: "azure-openai",
        base_url: "https://YOUR_RESOURCE.openai.azure.com",
        models: &["gpt-4o", "gpt-4-turbo", "gpt-35-turbo"],
        default_model: "gpt-4o",
        tls_sni: "YOUR_RESOURCE.openai.azure.com",
    },
    PresetTemplate {
        id: "azure-ai",
        label_zh: "Azure AI Foundry",
        label_en: "Azure AI Foundry",
        provider: "azure-ai",
        base_url: "https://YOUR_RESOURCE.services.ai.azure.com/openai/v1",
        models: &["gpt-4o", "gpt-4-turbo"],
        default_model: "gpt-4o",
        tls_sni: "YOUR_RESOURCE.services.ai.azure.com",
    },
    PresetTemplate {
        id: "bedrock",
        label_zh: "Amazon Bedrock",
        label_en: "Amazon Bedrock",
        provider: "bedrock",
        base_url: "https://bedrock-runtime.us-east-1.amazonaws.com",
        models: &["anthropic.claude-sonnet-4-20250514-v1:0", "anthropic.claude-haiku-4-20250414-v1:0"],
        default_model: "anthropic.claude-sonnet-4-20250514-v1:0",
        tls_sni: "bedrock-runtime.us-east-1.amazonaws.com",
    },
    PresetTemplate {
        id: "vertex",
        label_zh: "Google Vertex AI",
        label_en: "Google Vertex AI",
        provider: "vertex-ai",
        base_url: "https://us-central1-aiplatform.googleapis.com/v1",
        models: &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash"],
        default_model: "gemini-2.5-pro",
        tls_sni: "us-central1-aiplatform.googleapis.com",
    },
    PresetTemplate {
        id: "watsonx",
        label_zh: "IBM watsonx",
        label_en: "IBM watsonx",
        provider: "watsonx",
        base_url: "https://us-south.ml.cloud.ibm.com/ml/gateway/v1",
        models: &["meta-llama/llama-3-3-70b-instruct", "meta-llama/llama-3-1-8b-instruct"],
        default_model: "meta-llama/llama-3-3-70b-instruct",
        tls_sni: "us-south.ml.cloud.ibm.com",
    },
    PresetTemplate {
        id: "oci",
        label_zh: "OCI 生成式 AI",
        label_en: "OCI Generative AI",
        provider: "oci",
        base_url: "https://inference.generativeai.us-chicago-1.oci.oraclecloud.com/openai/v1",
        models: &["cohere.command-r-plus", "cohere.command-r"],
        default_model: "cohere.command-r-plus",
        tls_sni: "inference.generativeai.us-chicago-1.oci.oraclecloud.com",
    },
    PresetTemplate {
        id: "sap",
        label_zh: "SAP AI Hub",
        label_en: "SAP AI Hub",
        provider: "sap",
        base_url: "https://YOUR_DEPLOYMENT_URL",
        models: &["gpt-4o", "gpt-4-turbo"],
        default_model: "gpt-4o",
        tls_sni: "YOUR_DEPLOYMENT_URL",
    },
    // ── China Providers ──
    PresetTemplate {
        id: "alibaba",
        label_zh: "阿里通义千问",
        label_en: "Alibaba Qwen",
        provider: "alibaba",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        models: &["qwen-max", "qwen-plus", "qwen-turbo", "qwen-long"],
        default_model: "qwen-max",
        tls_sni: "dashscope.aliyuncs.com",
    },
    PresetTemplate {
        id: "qianfan",
        label_zh: "百度千帆",
        label_en: "Baidu Qianfan",
        provider: "qianfan",
        base_url: "https://qianfan.baidubce.com/v2",
        models: &["ernie-4.0-8k", "ernie-3.5-8k", "ernie-speed-8k"],
        default_model: "ernie-4.0-8k",
        tls_sni: "qianfan.baidubce.com",
    },
    PresetTemplate {
        id: "glm",
        label_zh: "智谱 GLM",
        label_en: "Zhipu GLM",
        provider: "glm",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        models: &["glm-4-plus", "glm-4", "glm-4-flash"],
        default_model: "glm-4-plus",
        tls_sni: "open.bigmodel.cn",
    },
    PresetTemplate {
        id: "kimi",
        label_zh: "Kimi (月之暗面)",
        label_en: "Kimi (Moonshot)",
        provider: "kimi",
        base_url: "https://api.moonshot.cn/v1",
        models: &["kimi-k2.6", "moonshot-v1-128k", "moonshot-v1-32k"],
        default_model: "kimi-k2.6",
        tls_sni: "api.moonshot.cn",
    },
    PresetTemplate {
        id: "minimax",
        label_zh: "Minimax",
        label_en: "Minimax",
        provider: "minimax",
        base_url: "https://api.minimax.chat/v1",
        models: &["MiniMax-M2.5", "MiniMax-M1", "abab6.5s-chat"],
        default_model: "MiniMax-M2.5",
        tls_sni: "api.minimax.chat",
    },
    PresetTemplate {
        id: "volcengine",
        label_zh: "火山引擎",
        label_en: "Volcengine",
        provider: "volcengine",
        base_url: "https://ark.cn-beijing.volces.com/api/v3",
        models: &["doubao-seed-2-0-code-preview", "doubao-1.5-pro-256k", "doubao-1.5-lite-32k"],
        default_model: "doubao-seed-2-0-code-preview",
        tls_sni: "ark.cn-beijing.volces.com",
    },
    PresetTemplate {
        id: "tencent",
        label_zh: "腾讯混元",
        label_en: "Tencent Hunyuan",
        provider: "tencent",
        base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        models: &["hunyuan-turbos-latest", "hunyuan-turbos", "hunyuan-lite"],
        default_model: "hunyuan-turbos-latest",
        tls_sni: "api.hunyuan.cloud.tencent.com",
    },
    PresetTemplate {
        id: "iflytek",
        label_zh: "科大讯飞星火",
        label_en: "iFlytek Spark",
        provider: "iflytek",
        base_url: "https://spark-api-open.xf-yun.com/v1",
        models: &["generalv3.5", "generalv3", "4.0Ultra"],
        default_model: "generalv3.5",
        tls_sni: "spark-api-open.xf-yun.com",
    },
    PresetTemplate {
        id: "baichuan",
        label_zh: "百川",
        label_en: "Baichuan",
        provider: "baichuan",
        base_url: "https://api.baichuan-ai.com/v1",
        models: &["Baichuan4", "Baichuan3-Turbo", "Baichuan2-Turbo"],
        default_model: "Baichuan4",
        tls_sni: "api.baichuan-ai.com",
    },
    PresetTemplate {
        id: "yi",
        label_zh: "零一万物",
        label_en: "01.AI Yi",
        provider: "yi",
        base_url: "https://api.lingyiwanwu.com/v1",
        models: &["yi-large", "yi-medium", "yi-light"],
        default_model: "yi-large",
        tls_sni: "api.lingyiwanwu.com",
    },
    PresetTemplate {
        id: "stepfun",
        label_zh: "阶跃星辰",
        label_en: "StepFun",
        provider: "stepfun",
        base_url: "https://api.stepfun.com/v1",
        models: &["step-2-16k", "step-1-128k", "step-1-32k"],
        default_model: "step-2-16k",
        tls_sni: "api.stepfun.com",
    },
    PresetTemplate {
        id: "360ai",
        label_zh: "360 AI",
        label_en: "360 AI",
        provider: "360ai",
        base_url: "https://api.360.cn/v1",
        models: &["360-gpt2-pro", "360-gpt2"],
        default_model: "360-gpt2-pro",
        tls_sni: "api.360.cn",
    },
    PresetTemplate {
        id: "sensenova",
        label_zh: "商汤 SenseNova",
        label_en: "SenseTime SenseNova",
        provider: "sensenova",
        base_url: "https://api.sensenova.cn/v1/llm",
        models: &["SenseChat-5", "SenseChat-4"],
        default_model: "SenseChat-5",
        tls_sni: "api.sensenova.cn",
    },
    PresetTemplate {
        id: "coze",
        label_zh: "Coze (字节)",
        label_en: "Coze (ByteDance)",
        provider: "coze",
        base_url: "https://api.coze.com/v1",
        models: &["coze-bot"],
        default_model: "coze-bot",
        tls_sni: "api.coze.com",
    },
    PresetTemplate {
        id: "baidu",
        label_zh: "百度 ERNIE",
        label_en: "Baidu ERNIE",
        provider: "baidu",
        base_url: "https://yiyan.baidu.com",
        models: &["ernie-speed", "ernie-lite"],
        default_model: "ernie-speed",
        tls_sni: "yiyan.baidu.com",
    },
    // ── Inference Platforms ──
    PresetTemplate {
        id: "deepinfra",
        label_zh: "DeepInfra",
        label_en: "DeepInfra",
        provider: "deepinfra",
        base_url: "https://api.deepinfra.com/v1/openai",
        models: &["meta-llama/Meta-Llama-3.1-70B-Instruct", "meta-llama/Meta-Llama-3.1-8B-Instruct"],
        default_model: "meta-llama/Meta-Llama-3.1-70B-Instruct",
        tls_sni: "api.deepinfra.com",
    },
    PresetTemplate {
        id: "sambanova",
        label_zh: "SambaNova",
        label_en: "SambaNova",
        provider: "sambanova",
        base_url: "https://api.sambanova.ai/v1",
        models: &["Meta-Llama-3.3-70B-Instruct", "Meta-Llama-3.1-8B-Instruct"],
        default_model: "Meta-Llama-3.3-70B-Instruct",
        tls_sni: "api.sambanova.ai",
    },
    PresetTemplate {
        id: "github-models",
        label_zh: "GitHub Models",
        label_en: "GitHub Models",
        provider: "github-models",
        base_url: "https://models.inference.ai.azure.com",
        models: &["gpt-4o", "gpt-4-turbo", "Meta-Llama-3.1-405B-Instruct"],
        default_model: "gpt-4o",
        tls_sni: "models.inference.ai.azure.com",
    },
    PresetTemplate {
        id: "huggingface",
        label_zh: "HuggingFace",
        label_en: "HuggingFace",
        provider: "huggingface",
        base_url: "https://api-inference.huggingface.co/v1",
        models: &["meta-llama/Meta-Llama-3.1-70B-Instruct", "meta-llama/Meta-Llama-3.1-8B-Instruct"],
        default_model: "meta-llama/Meta-Llama-3.1-70B-Instruct",
        tls_sni: "api-inference.huggingface.co",
    },
    PresetTemplate {
        id: "replicate",
        label_zh: "Replicate",
        label_en: "Replicate",
        provider: "replicate",
        base_url: "https://openai-proxy.replicate.com/v1",
        models: &["meta/llama-3.3-70b-instruct", "meta/llama-3.1-8b-instruct"],
        default_model: "meta/llama-3.3-70b-instruct",
        tls_sni: "openai-proxy.replicate.com",
    },
    PresetTemplate {
        id: "ollama-cloud",
        label_zh: "Ollama Cloud",
        label_en: "Ollama Cloud",
        provider: "ollama-cloud",
        base_url: "https://api.ollama.com/v1",
        models: &["llama3.3:70b", "llama3.1:8b"],
        default_model: "llama3.3:70b",
        tls_sni: "api.ollama.com",
    },
    PresetTemplate {
        id: "aimlapi",
        label_zh: "AI/ML API",
        label_en: "AI/ML API",
        provider: "aimlapi",
        base_url: "https://api.aimlapi.com/v1",
        models: &["gpt-4o", "gpt-4-turbo", "claude-sonnet-4"],
        default_model: "gpt-4o",
        tls_sni: "api.aimlapi.com",
    },
    PresetTemplate {
        id: "novita",
        label_zh: "Novita AI",
        label_en: "Novita AI",
        provider: "novita",
        base_url: "https://api.novita.ai/v3/openai",
        models: &["meta-llama/llama-3.3-70b-instruct", "meta-llama/llama-3.1-8b-instruct"],
        default_model: "meta-llama/llama-3.3-70b-instruct",
        tls_sni: "api.novita.ai",
    },
    PresetTemplate {
        id: "chutes",
        label_zh: "Chutes.ai",
        label_en: "Chutes.ai",
        provider: "chutes",
        base_url: "https://llm.chutes.ai/v1",
        models: &["meta-llama/llama-3.3-70b-instruct", "meta-llama/llama-3.1-8b-instruct"],
        default_model: "meta-llama/llama-3.3-70b-instruct",
        tls_sni: "llm.chutes.ai",
    },
    PresetTemplate {
        id: "poe",
        label_zh: "Poe",
        label_en: "Poe",
        provider: "poe",
        base_url: "https://api.poe.com/v1",
        models: &["Claude-Sonnet-4", "GPT-4o", "Gemini-2.5-Pro"],
        default_model: "Claude-Sonnet-4",
        tls_sni: "api.poe.com",
    },
    PresetTemplate {
        id: "phind",
        label_zh: "Phind",
        label_en: "Phind",
        provider: "phind",
        base_url: "https://api.phind.com/v1",
        models: &["Phind-70B", "Phind-34B"],
        default_model: "Phind-70B",
        tls_sni: "https.api.phind.com",
    },
    // ── Custom (must be last) ──
    PresetTemplate {
        id: "custom",
        label_zh: "自定义",
        label_en: "Custom",
        provider: "custom",
        base_url: "",
        models: &[],
        default_model: "",
        tls_sni: "",
    },
];

fn parse_upstream_pool_line(line: &str) -> (String, String) {
    let t = line.trim();
    if let Some((account_id, secret)) = t.split_once(':') {
        let account_id = account_id.trim();
        let secret = secret.trim();
        if !account_id.is_empty() && !secret.is_empty() {
            return (account_id.to_string(), secret.to_string());
        }
    }
    (String::new(), t.to_string())
}

fn pool_lines_to_key_inputs(lines: Vec<String>) -> Vec<UpstreamKeyInput> {
    lines
        .into_iter()
        .map(|line| {
            let (account_id, secret) = parse_upstream_pool_line(&line);
            UpstreamKeyInput {
                id: String::new(),
                secret,
                enabled: true,
                account_id,
                priority: 0,
            }
        })
        .collect()
}

fn validate_base_url(url: &str) -> Option<String> {
    let t = url.trim();
    if t.is_empty() {
        return Some("Base URL is required".into());
    }
    if t.ends_with("/v1") || t.contains("/v1/") {
        return Some("Do not include /v1 in Base URL".into());
    }
    if !t.starts_with("http://") && !t.starts_with("https://") {
        return Some("Base URL must start with http:// or https://".into());
    }
    None
}

fn first_key_from_text(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

fn preset_for_id(id: &str) -> Option<&'static PresetTemplate> {
    PRESETS.iter().find(|p| p.id == id)
}

fn models_for_provider(provider: &str) -> &'static [&'static str] {
    PRESETS
        .iter()
        .find(|p| p.provider == provider && !p.models.is_empty())
        .map(|p| p.models)
        .unwrap_or(&[])
}

/// Synced upstream models for the active profile; falls back to static presets.
fn effective_model_list(synced: &[String], provider: &str) -> Vec<String> {
    if !synced.is_empty() {
        return synced.to_vec();
    }
    models_for_provider(provider)
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

const CUSTOM_MODEL_SENTINEL: &str = "__custom_model__";

/// Drawer title is authoritative while editing; `active_profile` can lag after list refresh.
fn effective_editing_profile_id(
    drawer_profile: Option<String>,
    active_profile: &str,
) -> String {
    drawer_profile
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| active_profile.to_string())
}

fn apply_profile_to_form(
    p: &UpstreamProfileAdminView,
    provider: &RwSignal<String>,
    base_url: &RwSignal<String>,
    model: &RwSignal<String>,
    endpoints_text: &RwSignal<String>,
    tls_sni: &RwSignal<String>,
    proxy_url: &RwSignal<String>,
    fallback_profile_id: &RwSignal<String>,
    fallback_max_retries: &RwSignal<u64>,
) {
    provider.set(p.provider.clone());
    base_url.set(p.base_url.clone());
    model.set(p.fallback_model.clone());
    endpoints_text.set(p.endpoints.join("\n"));
    tls_sni.set(p.tls_sni.clone());
    proxy_url.set(p.proxy_url.clone().unwrap_or_default());
    fallback_profile_id.set(p.fallback_profile_id.clone().unwrap_or_default());
    fallback_max_retries.set(p.fallback_max_retries as u64);
}

fn upsert_profile_in_list(
    profiles: &RwSignal<Vec<UpstreamProfileAdminView>>,
    updated: UpstreamProfileAdminView,
) {
    profiles.update(|list| {
        if let Some(entry) = list.iter_mut().find(|p| p.id == updated.id) {
            *entry = updated;
        } else {
            list.push(updated);
        }
        list.sort_by(|a, b| a.id.cmp(&b.id));
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreationStep {
    PickTemplate,
    FillForm,
}

#[component]
pub fn UpstreamPage() -> impl IntoView {
    let t = use_translations();

    // Page state — filled from GET /api/admin/upstream/profiles (no hardcoded vendor defaults)
    let profiles: RwSignal<Vec<UpstreamProfileAdminView>> = RwSignal::new(Vec::new());
    let active_profile = RwSignal::new(String::new());
    let gateway_reachable = RwSignal::new(true);

    // Drawer-based navigation: Some(profile_id) opens drawer, None = list view
    let drawer_profile: RwSignal<Option<String>> = RwSignal::new(None);
    let drawer_creating: RwSignal<bool> = RwSignal::new(false);
    // Internal drawer tabs: 0 = Config, 1 = Keys, 2 = Routing
    let drawer_tab: RwSignal<usize> = RwSignal::new(0);

    // Form inputs — populated when a profile is selected or a preset is picked
    let provider = RwSignal::new(String::new());
    let base_url = RwSignal::new(String::new());
    let model = RwSignal::new(String::new());
    let endpoints_text = RwSignal::new(String::new());
    let tls_sni = RwSignal::new(String::new());
    let proxy_url = RwSignal::new(String::new());
    let show_advanced = RwSignal::new(false);
    let fallback_profile_id = RwSignal::new(String::new());
    let fallback_max_retries = RwSignal::new(2u64);
    let form_load_generation = RwSignal::new(0u32);

    // Inline creation state
    let new_profile_id = RwSignal::new(String::new());
    let creation_step = RwSignal::new(CreationStep::PickTemplate);

    // Auto-initialize model to first predefined model on mount / provider change.
    // This ensures the <select> DOM value and the model signal stay in sync.
    // Only depends on `provider` — does NOT re-run when model changes (so user
    // selections like "Custom Model" are not overridden).
    {
        // RwSignal implements Copy; no .clone() needed for closure capture.
        Effect::new(move |_| {
            let prov = provider.get();
            let models = models_for_provider(&prov);
            if !models.is_empty() && model.get().is_empty() {
                model.set(models[0].to_string());
            }
        });
    }

    // Connection testing state
    let testing = RwSignal::new(false);
    let test_result: RwSignal<Option<UpstreamTestResult>> = RwSignal::new(None);
    let test_error = RwSignal::new(String::new());

    // Saving configuration state
    let saving = RwSignal::new(false);
    let saved = RwSignal::new(false);
    let save_error = RwSignal::new(String::new());
    let sync_result: RwSignal<Option<SyncResult>> = RwSignal::new(None);
    let profile_model_options: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let profile_models_syncing = RwSignal::new(false);

    // Key pool state
    let key_pool: RwSignal<Option<Result<UpstreamKeysView, String>>> = RwSignal::new(None);
    let pool_secrets_text = RwSignal::new(String::new());
    let pool_replace_mode = RwSignal::new(false);
    let pool_saving = RwSignal::new(false);
    let pool_saved = RwSignal::new(false);
    let pool_error = RwSignal::new(String::new());

    // Per-key quota test state
    let key_test_results: RwSignal<HashMap<String, UpstreamTestResult>> =
        RwSignal::new(HashMap::new());
    let key_testing: RwSignal<HashMap<String, bool>> = RwSignal::new(HashMap::new());
    let testing_all = RwSignal::new(false);
    let quota_auto_refreshing = RwSignal::new(false);
    let delete_confirm_id: RwSignal<Option<String>> = RwSignal::new(None);
    let key_deleting: RwSignal<HashMap<String, bool>> = RwSignal::new(HashMap::new());
    let pool_delete_error = RwSignal::new(String::new());

    // Per-profile connectivity test status: None = untested, Some(true) = connected, Some(false) = unreachable
    let profile_test_status: RwSignal<HashMap<String, Option<bool>>> = RwSignal::new(HashMap::new());

    // Profile deletion state
    let show_delete_confirm = RwSignal::new(false);
    let deleting = RwSignal::new(false);

    let default_profile_id = RwSignal::new(String::new());
    let profiles_loaded = RwSignal::new(false);

    let run_key_quota_probe = {
        let key_testing = key_testing;
        let key_test_results = key_test_results;
        let quota_auto_refreshing = quota_auto_refreshing;
        let testing_all = testing_all;
        std::sync::Arc::new(
            move |pid: String, keys: Vec<crate::types::UpstreamKeyView>, only_missing: bool, finish_all: bool| {
                let targets: Vec<_> = keys
                    .into_iter()
                    .filter(|k| k.enabled && (!only_missing || k.quota.is_none()))
                    .collect();
                if targets.is_empty() {
                    if finish_all {
                        testing_all.set(false);
                    }
                    return;
                }
                quota_auto_refreshing.set(true);
                leptos::task::spawn_local(async move {
                    for k in targets {
                        key_testing.try_update(|m| m.insert(k.id.clone(), true));
                        let result = api::test_upstream_profile_key(&pid, &k.id).await;
                        key_testing.try_update(|m| m.insert(k.id.clone(), false));
                        let tr = match result {
                            Ok(r) => r,
                            Err(e) => UpstreamTestResult {
                                ok: false,
                                status_code: 0,
                                latency_ms: 0,
                                model_count: None,
                                error: Some(e),
                                quota: None,
                            },
                        };
                        key_test_results.try_update(|m| m.insert(k.id.clone(), tr));
                    }
                    quota_auto_refreshing.set(false);
                    if finish_all {
                        testing_all.set(false);
                    }
                });
            },
        )
    };

    // API loaders
    let load_key_pool = {
        let key_pool = key_pool;
        let delete_confirm_id = delete_confirm_id;
        let pool_delete_error = pool_delete_error;
        let default_profile_id = default_profile_id;
        std::sync::Arc::new(move |pid: String| {
            key_pool.set(None);
            delete_confirm_id.set(None);
            pool_delete_error.set(String::new());
            let default_id = default_profile_id.get_untracked();
            leptos::task::spawn_local(async move {
                let result = if pid == default_id {
                    api::fetch_upstream_keys().await
                } else {
                    api::fetch_upstream_profile_keys(&pid)
                        .await
                        .map(|v| UpstreamKeysView { keys: v.keys })
                };
                match result {
                    Ok(v) => {
                        key_pool.try_set(Some(Ok(v)));
                    }
                    Err(e) => {
                        key_pool.try_set(Some(Err(e)));
                    }
                }
            });
        })
    };

    {
        let probe = run_key_quota_probe.clone();
        Effect::new(move |_| {
            if let Some(Ok(pool)) = key_pool.get() {
                let pid = active_profile.get_untracked();
                let prov = provider.get_untracked();
                let b_url = base_url.get_untracked();
                if prov == "codex" || prov == "openai" || b_url.contains("chatgpt.com") {
                    probe.clone()(pid, pool.keys.clone(), true, false);
                }
            }
        });
    }

    // 120s auto-refresh for Codex profile quota data (aligned with cliproxyapi-dashboard)
    {
        let probe = run_key_quota_probe.clone();
        let interval_handle: std::cell::Cell<Option<gloo_timers::callback::Interval>> =
            std::cell::Cell::new(None);
        Effect::new(move |_| {
            // Cancel any existing interval when profile/provider changes
            interval_handle.take().map(|h| h.cancel());

            let pid = active_profile.get();
            let prov = provider.get();
            let b_url = base_url.get();
            let is_codex = prov == "codex" || prov == "openai" || b_url.contains("chatgpt.com");

            if !is_codex || pid.is_empty() {
                return;
            }

            // Clone values for the interval closure
            let probe_tick = probe.clone();
            let pid_tick = pid.clone();
            let key_pool_ref = key_pool;

            let handle = gloo_timers::callback::Interval::new(120_000, move || {
                if let Some(Some(Ok(pool))) = key_pool_ref.try_get_untracked() {
                    probe_tick.clone()(pid_tick.clone(), pool.keys.clone(), false, false);
                }
            });
            interval_handle.set(Some(handle));
        });
    }

    let model_req_id = Arc::new(AtomicU64::new(0));
    let load_profile_models = std::sync::Arc::new({
        let model_req_id = model_req_id.clone();
        move |pid: String| {
            profile_model_options.set(Vec::new());
            let req_id = model_req_id.fetch_add(1, Ordering::Relaxed) + 1;
            let model_req_id = model_req_id.clone();
            leptos::task::spawn_local(async move {
                if let Ok(resp) = api::fetch_models(Some(&pid)).await {
                    if model_req_id.load(Ordering::Relaxed) != req_id { return; }
                    let ids: Vec<String> = resp.models.into_iter().map(|m| m.id).collect();
                    profile_model_options.set(ids);
                }
            });
        }
    });

    let load_profile_data = {
        let load_key_pool = load_key_pool.clone();
        let load_profile_models = load_profile_models.clone();
        std::sync::Arc::new(move |pid: String| {
            // Reset states
            test_result.set(None);
            test_error.set(String::new());
            save_error.set(String::new());
            saved.set(false);
            sync_result.set(None);
            pool_secrets_text.set(String::new());
            pool_saved.set(false);
            pool_error.set(String::new());
            key_test_results.set(HashMap::new());
            key_testing.set(HashMap::new());
            testing_all.set(false);
            quota_auto_refreshing.set(false);

            let load_key_pool = load_key_pool.clone();
            let pid_for_models = pid.clone();
            load_profile_models.clone()(pid_for_models);
            leptos::task::spawn_local(async move {
                let loaded = if let Ok(resp) = api::fetch_upstream_profiles().await {
                    gateway_reachable.try_set(true);
                    default_profile_id.try_set(resp.default_profile_id.clone());
                    profiles.try_set(resp.profiles.clone());
                    resp.profiles.into_iter().find(|p| p.id == pid)
                } else {
                    gateway_reachable.try_set(false);
                    profiles
                        .get_untracked()
                        .into_iter()
                        .find(|p| p.id == pid)
                };

                if let Some(p) = loaded {
                    apply_profile_to_form(
                        &p,
                        &provider,
                        &base_url,
                        &model,
                        &endpoints_text,
                        &tls_sni,
                        &proxy_url,
                        &fallback_profile_id,
                        &fallback_max_retries,
                    );
                    form_load_generation.update(|g| *g += 1);
                }
                load_key_pool(pid);
            });
        })
    };

    let load_profiles_and_select = {
        let load_profile_data = load_profile_data.clone();
        std::sync::Arc::new(move |pid: Option<String>| {
            let load = load_profile_data.clone();
            leptos::task::spawn_local(async move {
                if let Ok(resp) = api::fetch_upstream_profiles().await {
                    gateway_reachable.try_set(true);
                    let def = resp.default_profile_id.clone();
                    default_profile_id.try_set(def.clone());
                    profiles.try_set(resp.profiles);
                    let select = pid.unwrap_or(def);
                    active_profile.try_set(select.clone());
                    load(select);
                } else {
                    gateway_reachable.try_set(false);
                }
                profiles_loaded.try_set(true);
            });
        })
    };

    // Initial load: gateway default profile (not hardcoded id)
    load_profiles_and_select.clone()(None);

    let probe_all_keys = run_key_quota_probe.clone();
    let reload_key_pool = load_key_pool.clone();
    let refresh_profiles = load_profiles_and_select.clone();
    let open_profile = load_profile_data.clone();

    let on_sync_profile_models = Callback::new(move |_| {
        let pid = active_profile.get();
        profile_models_syncing.set(true);
        save_error.set(String::new());
        leptos::task::spawn_local(async move {
            match api::sync_models(&pid).await {
                Ok(r) => {
                    sync_result.set(Some(r));
                    if let Ok(resp) = api::fetch_models(Some(&pid)).await {
                        profile_model_options.set(
                            resp.models.into_iter().map(|m| m.id).collect(),
                        );
                    }
                }
                Err(e) => save_error.set(e),
            }
            profile_models_syncing.set(false);
        });
    });

    // Actions
    let on_test = Callback::new(move |_| {
        testing.set(true);
        test_result.set(None);
        test_error.set(String::new());

        let url = base_url.get();
        if let Some(err) = validate_base_url(&url) {
            test_error.set(err);
            testing.set(false);
            return;
        }

        let pid = active_profile.get();
        let bulk_key = first_key_from_text(&pool_secrets_text.get());

        leptos::task::spawn_local(async move {
            if let Some(key) = bulk_key {
                // If a new bulk key is entered, test using that key
                match api::test_upstream_connection(&UpstreamTestBody {
                    base_url: url,
                    api_key: key,
                })
                .await
                {
                    Ok(r) => {
                        let r_ok = r.ok;
                        test_result.try_set(Some(r));
                        profile_test_status.try_update(|m| {
                            m.insert(pid.clone(), Some(r_ok));
                        });
                    },
                    Err(e) => {
                        test_error.try_set(e);
                        profile_test_status.try_update(|m| {
                            m.insert(pid.clone(), Some(false));
                        });
                    },
                }
            } else if pid == default_profile_id.try_get_untracked().unwrap_or_default()
                || profiles.try_get().unwrap_or_default().iter().any(|p| p.id == pid)
            {
                // Otherwise test saved profile keys
                match api::test_upstream_profile(&pid).await {
                    Ok(r) => {
                        let r_ok = r.ok;
                        test_result.try_set(Some(r));
                        profile_test_status.try_update(|m| {
                            m.insert(pid.clone(), Some(r_ok));
                        });
                    },
                    Err(e) => {
                        test_error.try_set(e);
                        profile_test_status.try_update(|m| {
                            m.insert(pid.clone(), Some(false));
                        });
                    },
                }
            } else {
                test_error.try_set(
                    "Please enter a key in bulk input to test this unsaved profile.".to_string(),
                );
            }
            testing.try_set(false);
        });
    });

    let on_save = Callback::new({
        move |_| {
        saving.set(true);
        saved.set(false);
        save_error.set(String::new());
        sync_result.set(None);

        let url = base_url.get().trim().to_string();
        if let Some(err) = validate_base_url(&url) {
            save_error.set(err);
            saving.set(false);
            return;
        }
        let model_val = model.get().trim().to_string();
        if model_val.is_empty() {
            save_error.set("Model is required".into());
            saving.set(false);
            return;
        }

        let endpoints: Vec<String> = endpoints_text
            .get()
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let keys_to_append: Vec<String> = pool_secrets_text
            .get()
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        let pid = effective_editing_profile_id(drawer_profile.get(), &active_profile.get());
        if pid.is_empty() {
            save_error.set("No profile selected".into());
            saving.set(false);
            return;
        }
        active_profile.set(pid.clone());
        let prov = provider.get().trim().to_string();
        if prov.is_empty() {
            save_error.set("Provider is required".into());
            saving.set(false);
            return;
        }
        let sni_val = tls_sni.get().trim().to_string();
        let sni = if sni_val.is_empty() {
            None
        } else {
            Some(sni_val)
        };
        let proxy = proxy_url.get().trim().to_string();
        let proxy_opt = if proxy.is_empty() {
            None
        } else {
            Some(proxy)
        };
        let keys_to_append_clone = keys_to_append.clone();
        let default_id = default_profile_id.get_untracked();
        let fpid = fallback_profile_id.get().trim().to_string();
        let fpid_opt = if fpid.is_empty() { None } else { Some(fpid) };
        let fmr = fallback_max_retries.get() as u32;

        leptos::task::spawn_local(async move {
            let req = PutUpstreamProfileAdminRequest {
                provider: prov,
                base_url: url.clone(),
                fallback_model: model_val.clone(),
                endpoints: endpoints.clone(),
                tls_sni: sni,
                proxy_url: proxy_opt,
                fallback_profile_id: fpid_opt,
                fallback_max_retries: Some(fmr),
            };
            match api::put_upstream_profile(&pid, &req).await {
                Ok(updated) => {
                    upsert_profile_in_list(&profiles, updated.clone());
                    apply_profile_to_form(
                        &updated,
                        &provider,
                        &base_url,
                        &model,
                        &endpoints_text,
                        &tls_sni,
                        &proxy_url,
                        &fallback_profile_id,
                        &fallback_max_retries,
                    );
                    form_load_generation.update(|g| *g += 1);
                    active_profile.try_set(pid.clone());
                    drawer_profile.try_set(Some(pid.clone()));

                    let mut keys_ok = true;
                    if !keys_to_append_clone.is_empty() {
                        let keys = pool_lines_to_key_inputs(keys_to_append_clone);
                        let key_req = PutUpstreamKeysRequest {
                            keys,
                            mode: UpstreamKeysPutMode::Append,
                        };
                        let key_err = if pid == default_id {
                            api::put_upstream_keys(&key_req).await.err()
                        } else {
                            api::put_upstream_profile_keys(&pid, &key_req).await.err()
                        };
                        if let Some(e) = key_err {
                            save_error.try_set(format!("Profile saved but keys failed: {e}"));
                            keys_ok = false;
                        } else {
                            pool_secrets_text.try_set(String::new());
                        }
                    } else {
                        pool_secrets_text.try_set(String::new());
                    }
                    if keys_ok {
                        saved.try_set(true);
                    }
                    if let Ok(resp) = api::fetch_upstream_profiles().await {
                        default_profile_id.try_set(resp.default_profile_id.clone());
                        profiles.try_set(resp.profiles);
                        if let Some(p) = profiles.get().iter().find(|p| p.id == pid) {
                            apply_profile_to_form(
                                p,
                                &provider,
                                &base_url,
                                &model,
                                &endpoints_text,
                                &tls_sni,
                                &proxy_url,
                                &fallback_profile_id,
                                &fallback_max_retries,
                            );
                        }
                    }
                }
                Err(e) => { save_error.try_set(e); },
            }
            saving.try_set(false);
        });
        }
    });

    let on_save_pool = Callback::new(move |_| {
        pool_saving.set(true);
        pool_saved.set(false);
        pool_error.set(String::new());
        let secrets: Vec<String> = pool_secrets_text
            .get()
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if secrets.is_empty() {
            pool_error.set(
                use_translations()
                    .upstream_pool_empty_keys_error()
                    .to_string(),
            );
            pool_saving.set(false);
            return;
        }
        let pid = active_profile.get();
        let default_id = default_profile_id.get_untracked();
        let keys = pool_lines_to_key_inputs(secrets);
        let mode = if pool_replace_mode.get() {
            UpstreamKeysPutMode::Replace
        } else {
            UpstreamKeysPutMode::Append
        };
        let req = PutUpstreamKeysRequest { keys, mode };
        leptos::task::spawn_local(async move {
            let result = if pid == default_id {
                api::put_upstream_keys(&req).await.map(|v| UpstreamKeysView { keys: v.keys })
            } else {
                api::put_upstream_profile_keys(&pid, &req).await.map(|v| UpstreamKeysView { keys: v.keys })
            };
            match result {
                Ok(v) => {
                    key_pool.try_set(Some(Ok(v)));
                    pool_secrets_text.try_set(String::new());
                    pool_saved.try_set(true);
                    // Refresh profiles list (since key count changed)
                    if let Ok(resp) = api::fetch_upstream_profiles().await {
                        default_profile_id.try_set(resp.default_profile_id.clone());
                        profiles.try_set(resp.profiles);
                    }
                }
                Err(e) => { pool_error.try_set(e); },
            }
            pool_saving.try_set(false);
        });
    });

    let on_delete_profile = Callback::new({
        let refresh = refresh_profiles.clone();
        move |_| {
        let pid = effective_editing_profile_id(drawer_profile.get(), &active_profile.get());
        if pid.is_empty() {
            return;
        }
        if pid == default_profile_id.get_untracked() {
            return;
        }
        deleting.set(true);
        let r = refresh.clone();
        leptos::task::spawn_local(async move {
            match api::delete_upstream_profile(&pid).await {
                Ok(_) => {
                    show_delete_confirm.try_set(false);
                    drawer_profile.try_set(None);
                    drawer_creating.try_set(false);
                    r.clone()(None);
                }
                Err(e) => {
                    show_delete_confirm.try_set(false);
                    save_error.try_set(e);
                }
            }
            deleting.try_set(false);
        });
        }
    });

    let on_create_profile = Callback::new({
        move |_| {
        saved.set(false);
        save_error.set(String::new());
        let id = new_profile_id.get().trim().to_string();
        if id.is_empty() {
            save_error.set("Profile ID is required".to_string());
            return;
        }
        let prov = provider.get().trim().to_string();
        if prov.is_empty() {
            save_error.set("Provider is required".to_string());
            return;
        }
        let url = base_url.get().trim().to_string();
        if let Some(err) = validate_base_url(&url) {
            save_error.set(err);
            return;
        }
        let model_val = model.get().trim().to_string();
        if model_val.is_empty() {
            save_error.set("Fallback model is required".to_string());
            return;
        }
        let sni_val = tls_sni.get().trim().to_string();
        let sni = if sni_val.is_empty() {
            None
        } else {
            Some(sni_val)
        };
        let proxy = proxy_url.get().trim().to_string();
        let proxy_opt = if proxy.is_empty() {
            None
        } else {
            Some(proxy)
        };

        saving.set(true);
        let fpid = fallback_profile_id.get().trim().to_string();
        let fpid_opt = if fpid.is_empty() { None } else { Some(fpid) };
        let fmr = fallback_max_retries.get() as u32;
        leptos::task::spawn_local(async move {
            let req = PutUpstreamProfileAdminRequest {
                provider: prov,
                base_url: url,
                fallback_model: model_val,
                endpoints: Vec::new(),
                tls_sni: sni,
                proxy_url: proxy_opt,
                fallback_profile_id: fpid_opt,
                fallback_max_retries: Some(fmr),
            };
            match api::put_upstream_profile(&id, &req).await {
                Ok(updated) => {
                    upsert_profile_in_list(&profiles, updated.clone());
                    apply_profile_to_form(
                        &updated,
                        &provider,
                        &base_url,
                        &model,
                        &endpoints_text,
                        &tls_sni,
                        &proxy_url,
                        &fallback_profile_id,
                        &fallback_max_retries,
                    );
                    form_load_generation.update(|g| *g += 1);
                    saved.try_set(true);
                    drawer_creating.try_set(false);
                    new_profile_id.try_set(String::new());
                    active_profile.try_set(id.clone());
                    drawer_profile.try_set(Some(id.clone()));
                    if let Ok(resp) = api::fetch_upstream_profiles().await {
                        default_profile_id.try_set(resp.default_profile_id.clone());
                        profiles.try_set(resp.profiles);
                    }
                }
                Err(e) => {
                    save_error.try_set(e);
                }
            }
            saving.try_set(false);
        });
        }
    });

    let on_set_default_profile = Callback::new(move |_| {
        let pid = effective_editing_profile_id(drawer_profile.get(), &active_profile.get());
        if pid.is_empty() {
            save_error.set("No profile selected".into());
            return;
        }
        if pid == default_profile_id.get_untracked() {
            return;
        }
        save_error.set(String::new());
        leptos::task::spawn_local(async move {
            match api::fetch_pipeline_runtime().await {
                Ok(cfg) => {
                    let req = crate::types::PipelineRuntimeConfig {
                        pipeline_mode: cfg.pipeline_mode,
                        default_upstream_profile: pid.clone(),
                        profiles: cfg.profiles,
                    };
                    match api::update_pipeline_runtime(&req).await {
                        Ok(updated) => {
                            default_profile_id.try_set(updated.default_upstream_profile);
                            saved.try_set(true);
                        }
                        Err(e) => {
                            save_error.try_set(e);
                        }
                    }
                }
                Err(e) => {
                    save_error.try_set(e);
                }
            }
        });
    });

    view! {
        <div class="page-content space-y-6">
            <SectionHeader
                title=t.upstream_title()
                description=t.upstream_desc()
            />

            {move || (!gateway_reachable.get()).then(|| view! {
                <div class="glass-card text-warning text-sm">
                    {t.upstream_gateway_unreachable()}
                </div>
            })}

            // Profile card grid
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                {move || {
                    if !profiles_loaded.get() {
                        (0..3)
                            .map(|_| view! { <SkeletonUpstreamProfileCard /> })
                            .collect_view()
                            .into_any()
                    } else {
                        let list = profiles.get();
                        let mut views: Vec<leptos::prelude::AnyView> = Vec::new();
                        if list.is_empty() {
                            views.push(view! {
                                <div class="upstream-profiles-empty empty-state py-10">
                                    <div class="empty-state-icon">"◇"</div>
                                    <p class="empty-state-title">{t.upstream_profiles_empty_title()}</p>
                                    <p class="text-xs text-theme-muted mt-1 max-w-sm mx-auto">
                                        {t.upstream_profiles_empty_desc()}
                                    </p>
                                </div>
                            }.into_any());
                        } else {
                            let def_id = default_profile_id.get();
                            for p in list {
                                let pid = p.id.clone();
                                let pid_badge = p.id.clone();
                                let pid_edit = p.id.clone();
                                let pid3 = p.id.clone();
                                let pid2 = p.id.clone();
                                let badge_count = p.key_pool_count;
                                let keys_available = p.keys_available;
                                let is_default = p.id == def_id;
                                let card_class = if is_default {
                                    "upstream-card upstream-card-default"
                                } else {
                                    "upstream-card"
                                };
                                let status_dot = if p.key_pool_count == 0 {
                                    None
                                } else if p.keys_available == p.key_pool_count {
                                    Some("upstream-card-status-ok")
                                } else {
                                    Some("upstream-card-status-warn")
                                };
                                views.push(view! {
                                    <div
                                        class=card_class
                                        on:click={
                                            let open = open_profile.clone();
                                            move |_| {
                                            drawer_creating.set(false);
                                            drawer_tab.set(0);
                                            active_profile.set(pid.clone());
                                            drawer_profile.set(Some(pid.clone()));
                                            open.clone()(pid.clone());
                                        }}
                                    >
                                        <div class="flex items-start justify-between mb-3">
                                            <div class="flex items-center gap-2 min-w-0">
                                                <span class="upstream-card-icon">
                                                    {p.id.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()}
                                                </span>
                                                <div class="min-w-0">
                                                    <div class="flex items-center gap-2">
                                                        <div class="upstream-card-title truncate">{p.id.clone()}</div>
                                                        {status_dot.map(|cls| view! {
                                                            <span class=format!("upstream-card-status-dot {cls}") title="Key pool status"></span>
                                                        })}
                                                        {move || {
                                                            let ts = profile_test_status.get();
                                                            let pid_check = pid_badge.clone();
                                                            match ts.get(&pid_check) {
                                                                Some(Some(true)) => view! {
                                                                    <span class="badge badge-success text-[10px]">{t.upstream_status_connected()}</span>
                                                                }.into_any(),
                                                                Some(Some(false)) => view! {
                                                                    <span class="badge badge-error text-[10px]">{t.upstream_status_unreachable()}</span>
                                                                }.into_any(),
                                                                _ => view! { <span class="badge text-[10px]">{t.upstream_status_untested()}</span> }.into_any(),
                                                            }
                                                        }}
                                                    </div>
                                                    <div class="upstream-card-subtitle font-mono text-xs truncate max-w-[180px]">
                                                        {p.base_url.clone()}
                                                    </div>
                                                    {(!is_default).then(|| view! {
                                                        <div class="text-xs text-theme-muted mt-0.5 truncate">{p.provider.clone()}</div>
                                                    })}
                                                </div>
                                            </div>
                                            {if is_default {
                                                view! { <span class="badge badge-accent text-xs shrink-0">{t.upstream_default_badge()}</span> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                        </div>
                                        <div class="flex items-center gap-3 text-xs text-theme-muted">
                                            <span class="flex items-center gap-1">
                                                <span class="upstream-card-stat">{keys_available}</span>
                                                <span>"/"</span>
                                                <span class="upstream-card-stat">{badge_count}</span>
                                                " keys"
                                            </span>
                                            <span class="flex items-center gap-1">
                                                <span class="upstream-card-stat">{p.endpoints.len()}</span>
                                                " endpoints"
                                            </span>
                                        </div>
                                        <div class="flex items-center gap-2 mt-2 pt-2 border-t border-theme/10">
                                            <button type="button" class="btn btn-secondary text-[10px] px-2 py-0.5"
                                                on:click={
                                                    let open = open_profile.clone();
                                                    let pid_test = pid2.clone();
                                                    move |ev| {
                                                        ev.stop_propagation();
                                                        active_profile.set(pid_test.clone());
                                                        drawer_profile.set(Some(pid_test.clone()));
                                                        drawer_tab.set(0);
                                                        drawer_creating.set(false);
                                                        open.clone()(pid_test.clone());
                                                    }
                                                }
                                            >"Edit"</button>
                                        </div>
                                        {if !is_default {
                                            view! {
                                                <button
                                                    type="button"
                                                    class="upstream-card-delete"
                                                    on:click=move |ev| {
                                                        ev.stop_propagation();
                                                        active_profile.set(pid3.clone());
                                                        show_delete_confirm.set(true);
                                                    }
                                                    title="Delete profile"
                                                >
                                                    "✕"
                                                </button>
                                            }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                    </div>
                                }.into_any());
                            }
                        }
                        views.into_any()
                    }
                }}

                {move || profiles_loaded.get().then(|| view! {
                    <div
                        class="upstream-card upstream-card-new"
                        on:click=move |_| {
                            drawer_creating.set(true);
                            drawer_profile.set(None);
                            creation_step.set(CreationStep::PickTemplate);
                            new_profile_id.set(String::new());
                            provider.set("custom".to_string());
                            base_url.set(String::new());
                            model.set(String::new());
                            endpoints_text.set(String::new());
                            tls_sni.set(String::new());
                            proxy_url.set(String::new());
                            save_error.set(String::new());
                        }
                    >
                        <div class="flex flex-col items-center justify-center h-full gap-2 text-theme-muted">
                            <span class="text-3xl leading-none">"+"</span>
                            <span class="text-sm font-medium">{t.upstream_new_profile_label()}</span>
                        </div>
                    </div>
                })}
            </div>

            // Right-side drawer overlay
            {move || {
                let show = drawer_profile.get().is_some() || drawer_creating.get();
                if !show {
                    ().into_any()
                } else {
                let probe_fn = probe_all_keys.clone();
                let reload_fn = reload_key_pool.clone();
                view! {
                    <div class="upstream-drawer-backdrop" on:click=move |_| {
                        drawer_profile.set(None);
                        drawer_creating.set(false);
                    }>
                        <div class="upstream-drawer" on:click=|ev| ev.stop_propagation()>
                            <div class="upstream-drawer-header">
                                <div class="min-w-0">
                                    <h3 class="text-base font-semibold text-theme truncate">
                                        {move || if drawer_creating.get() {
                                            t.upstream_tab_new_profile().to_string()
                                        } else {
                                            drawer_profile.get().unwrap_or_default()
                                        }}
                                    </h3>
                                    {move || {
                                        if drawer_creating.get() {
                                            ().into_any()
                                        } else {
                                            let pid = drawer_profile.get().unwrap_or_default();
                                            let prov = provider.get();
                                            let is_def = pid == default_profile_id.get();
                                            view! {
                                                <div class="flex flex-wrap items-center gap-2 mt-1">
                                                    <span class="badge text-[10px] font-mono">{prov}</span>
                                                    {is_def.then(|| view! {
                                                        <span class="badge badge-accent text-[10px]">{t.upstream_default_badge()}</span>
                                                    })}
                                                    {(!is_def && !pid.is_empty()).then(|| {
                                                        // Callback implements Copy; no .clone() needed.
                                                        view! {
                                                            <button type="button" class="btn btn-secondary text-[10px] px-2 py-0.5"
                                                                on:click=move |_| on_set_default_profile.run(())
                                                            >
                                                                {t.pipeline_default_profile()}
                                                            </button>
                                                        }
                                                    })}
                                                </div>
                                            }.into_any()
                                        }
                                    }}
                                </div>
                                <button
                                    type="button"
                                    class="text-theme-muted hover:text-theme text-lg"
                                    on:click=move |_| {
                                        drawer_profile.set(None);
                                        drawer_creating.set(false);
                                    }
                                >
                                    "✕"
                                </button>
                            </div>

                            // Internal drawer tabs (only when editing, not creating)
                            {move || (!drawer_creating.get()).then(|| view! {
                                <div class="upstream-drawer-tabs">
                                    <button
                                        type="button"
                                        class=move || if drawer_tab.get() == 0 { "tab-item tab-item-active" } else { "tab-item" }
                                        on:click=move |_| drawer_tab.set(0)
                                    >
                                        {t.upstream_subtab_profiles()}
                                    </button>
                                    <button
                                        type="button"
                                        class=move || if drawer_tab.get() == 1 { "tab-item tab-item-active" } else { "tab-item" }
                                        on:click=move |_| drawer_tab.set(1)
                                    >
                                        {t.upstream_subtab_keys()}
                                    </button>
                                    <button
                                        type="button"
                                        class=move || if drawer_tab.get() == 2 { "tab-item tab-item-active" } else { "tab-item" }
                                        on:click=move |_| drawer_tab.set(2)
                                    >
                                        {t.upstream_subtab_routing()}
                                    </button>
                                </div>
                            })}

                            <div class="upstream-drawer-body">
                                {move || {
                                    let probe = probe_fn.clone();
                                    let reload = reload_fn.clone();
                                    if drawer_creating.get() {
                                    // ---- Creation flow ----
                                    if creation_step.get() == CreationStep::PickTemplate {
                                        view! {
                                            <div class="space-y-4">
                                                <h4 class="text-sm font-semibold text-theme">
                                                    {t.upstream_pick_template()}
                                                </h4>
                                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                                    {PRESETS.iter().map(|preset| {
                                                        let pid = preset.id;
                                                        let label = match t.locale {
                                                            crate::locale::Locale::ZhCN => preset.label_zh,
                                                            _ => preset.label_en,
                                                        };
                                                        let models_count = preset.models.len();
                                                        let is_custom = preset.id == "custom";
                                                        view! {
                                                            <button
                                                                type="button"
                                                                class="glass-card text-left p-4 hover:border-accent/50 transition-colors cursor-pointer space-y-2"
                                                                on:click=move |_| {
                                                                    if let Some(p) = preset_for_id(pid) {
                                                                        new_profile_id.set(p.id.to_string());
                                                                        provider.set(p.provider.to_string());
                                                                        base_url.set(p.base_url.to_string());
                                                                        model.set(p.default_model.to_string());
                                                                        tls_sni.set(p.tls_sni.to_string());
                                                                        endpoints_text.set(String::new());
                                                                        save_error.set(String::new());
                                                                        creation_step.set(CreationStep::FillForm);
                                                                    }
                                                                }
                                                            >
                                                                <div class="font-semibold text-sm text-theme">{label}</div>
                                                                {if !is_custom {
                                                                    view! {
                                                                        <div class="text-xs text-theme-muted font-mono truncate">{preset.base_url}</div>
                                                                        <div class="text-xs text-accent">
                                                                            {format!("{} {}", models_count, t.upstream_template_models_count())}
                                                                        </div>
                                                                    }.into_any()
                                                                } else {
                                                                    view! { <div class="text-xs text-theme-muted">{t.upstream_preset_custom()}</div> }.into_any()
                                                                }}
                                                            </button>
                                                        }
                                                    }).collect_view()}
                                                </div>
                                            </div>
                                        }.into_any()
                                    } else {
                                        // Step 2: Fill form
                                        view! {
                                            <div class="space-y-4">
                                                <div class="flex items-center justify-between">
                                                    <h4 class="text-sm font-semibold text-theme">{t.upstream_tab_new_profile()}</h4>
                                                    <button type="button" class="text-xs text-accent cursor-pointer"
                                                        on:click=move |_| creation_step.set(CreationStep::PickTemplate)
                                                    >{t.upstream_back_to_templates()}</button>
                                                </div>
                                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_new_profile_id()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="e.g. mimo"
                                                            prop:value=move || new_profile_id.get()
                                                            on:input=move |ev| new_profile_id.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_provider_label()}</label>
                                                        {move || {
                                                            form_load_generation.get();
                                                            let current = provider.get();
                                                            view! {
                                                                <select class="input text-sm"
                                                                    prop:value=current.clone()
                                                                    on:change=move |ev| {
                                                                        let new_prov = event_target_value(&ev);
                                                                        provider.set(new_prov.clone());
                                                                        // Reset model when provider changes so the select auto-initializes.
                                                                        let new_models = models_for_provider(&new_prov);
                                                                        if new_models.is_empty() {
                                                                            model.set(String::new());
                                                                        } else {
                                                                            model.set(new_models[0].to_string());
                                                                        }
                                                                    }
                                                                >
                                                            <option value="deepseek">DeepSeek</option>
                                                            <option value="mimo">MiMo</option>
                                                            <option value="openai">OpenAI</option>
                                                            <option value="codex">Codex</option>
                                                            <option value="anthropic">Anthropic</option>
                                                            <optgroup label="International">
                                                                <option value="groq">Groq</option>
                                                                <option value="xai">xAI</option>
                                                                <option value="mistral">Mistral</option>
                                                                <option value="gemini">Gemini</option>
                                                                <option value="perplexity">Perplexity</option>
                                                                <option value="together">Together</option>
                                                                <option value="fireworks">Fireworks</option>
                                                                <option value="cerebras">Cerebras</option>
                                                                <option value="cohere">Cohere</option>
                                                                <option value="nvidia">NVIDIA NIM</option>
                                                                <option value="nebius">Nebius</option>
                                                                <option value="siliconflow">SiliconFlow</option>
                                                                <option value="hyperbolic">Hyperbolic</option>
                                                                <option value="openrouter">OpenRouter</option>
                                                                <option value="reka">Reka</option>
                                                            </optgroup>
                                                            <optgroup label="Cloud">
                                                                <option value="azure-openai">Azure OpenAI</option>
                                                                <option value="azure-ai">Azure AI</option>
                                                                <option value="bedrock">Bedrock</option>
                                                                <option value="vertex-ai">Vertex AI</option>
                                                                <option value="watsonx">watsonx</option>
                                                                <option value="oci">OCI</option>
                                                                <option value="sap">SAP</option>
                                                            </optgroup>
                                                            <optgroup label="中国">
                                                                <option value="alibaba">通义千问</option>
                                                                <option value="qianfan">百度千帆</option>
                                                                <option value="glm">智谱 GLM</option>
                                                                <option value="kimi">Kimi</option>
                                                                <option value="minimax">Minimax</option>
                                                                <option value="volcengine">火山引擎</option>
                                                                <option value="tencent">混元</option>
                                                                <option value="iflytek">讯飞星火</option>
                                                                <option value="baichuan">百川</option>
                                                                <option value="yi">零一万物</option>
                                                                <option value="stepfun">阶跃星辰</option>
                                                                <option value="360ai">360 AI</option>
                                                                <option value="sensenova">商汤</option>
                                                                <option value="coze">Coze</option>
                                                                <option value="baidu">百度 ERNIE</option>
                                                            </optgroup>
                                                            <optgroup label="Inference">
                                                                <option value="deepinfra">DeepInfra</option>
                                                                <option value="sambanova">SambaNova</option>
                                                                <option value="github-models">GitHub Models</option>
                                                                <option value="huggingface">HuggingFace</option>
                                                                <option value="replicate">Replicate</option>
                                                                <option value="ollama-cloud">Ollama Cloud</option>
                                                                <option value="aimlapi">AI/ML API</option>
                                                                <option value="novita">Novita AI</option>
                                                                <option value="chutes">Chutes.ai</option>
                                                                <option value="poe">Poe</option>
                                                                <option value="phind">Phind</option>
                                                            </optgroup>
                                                            <option value="custom">Custom</option>
                                                                </select>
                                                            }
                                                        }}
                                                    </div>
                                                    <div class="md:col-span-2">
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_base_url_label()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="https://..."
                                                            prop:value=move || base_url.get()
                                                            on:input=move |ev| base_url.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_model_label()}</label>
                                                        {move || {
                                                            let models = models_for_provider(&provider.get());
                                                            if models.is_empty() {
                                                                view! { <input type="text" class="input font-mono text-sm" placeholder="model-name"
                                                                    prop:value=move || model.get() on:input=move |ev| model.set(event_target_value(&ev))
                                                                /> }.into_any()
                                                            } else {
                                                                view! {
                                                                    <select class="input font-mono text-sm"
                                                                        prop:value=move || {
                                                                            let m = model.get();
                                                                            if m == CUSTOM_MODEL_SENTINEL { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                            else if models_for_provider(&provider.get()).contains(&m.as_str()) { m }
                                                                            else { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                        }
                                                                        on:change=move |ev| {
                                                                            let v = event_target_value(&ev);
                                                                            if v != CUSTOM_MODEL_SENTINEL { model.set(v); } else { model.set(String::new()); }
                                                                        }
                                                                    >
                                                                        {models.iter().map(|m| view! { <option value=*m>{*m}</option> }).collect_view()}
                                                                        <option value=CUSTOM_MODEL_SENTINEL>{t.upstream_model_custom()}</option>
                                                                    </select>
                                                                }.into_any()
                                                            }
                                                        }}
                                                        {move || {
                                                            let m = model.get();
                                                            let models = models_for_provider(&provider.get());
                                                            let is_custom = m == CUSTOM_MODEL_SENTINEL || (!m.is_empty() && !models.is_empty() && !models.contains(&m.as_str()));
                                                            is_custom.then(|| view! {
                                                                <input type="text" class="input font-mono text-sm mt-2" placeholder="model-name"
                                                                    prop:value=move || { let m = model.get(); if m == CUSTOM_MODEL_SENTINEL { String::new() } else { m } }
                                                                    on:input=move |ev| model.set(event_target_value(&ev))
                                                                />
                                                            })
                                                        }}
                                                    </div>
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_tls_sni_label()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="e.g. api.deepseek.com"
                                                            prop:value=move || tls_sni.get()
                                                            on:input=move |ev| tls_sni.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                    <div class="md:col-span-2">
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_proxy_label()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="socks5://127.0.0.1:1080"
                                                            prop:value=move || proxy_url.get()
                                                            on:input=move |ev| proxy_url.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                </div>
                                                {move || if !save_error.get().is_empty() {
                                                    view! { <div class="text-xs text-error mt-2">{save_error.get()}</div> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                <div class="flex justify-end gap-2 pt-4 border-t border-theme/10">
                                                    <button type="button" class="btn btn-secondary text-xs"
                                                        on:click=move |_| { drawer_creating.set(false); drawer_profile.set(None); }
                                                    >"Cancel"</button>
                                                    <button type="button" class="btn btn-primary text-xs"
                                                        on:click=move |_| on_create_profile.run(())
                                                        disabled=move || saving.get()
                                                    >"Create Profile"</button>
                                                </div>
                                            </div>
                                        }.into_any()
                                    }
                                } else {
                                    // ---- Edit mode: drawer_tab controls content ----
                                    match drawer_tab.get() {
                                        0 => {
                                            // Config tab — edit form
                                            view! {
                                                <div class="space-y-4">
                                                    <div class="flex flex-wrap gap-2">
                                                        {PRESETS.iter().map(|preset| {
                                                            let pid = preset.id;
                                                            let label = match t.locale {
                                                                crate::locale::Locale::ZhCN => preset.label_zh,
                                                                _ => preset.label_en,
                                                            };
                                                            view! {
                                                                <button type="button" class="btn btn-secondary text-xs"
                                                                    on:click=move |_| {
                                                                        if let Some(p) = preset_for_id(pid) {
                                                                            base_url.set(p.base_url.to_string());
                                                                            model.set(p.default_model.to_string());
                                                                            provider.set(p.provider.to_string());
                                                                            if !p.tls_sni.is_empty() { tls_sni.set(p.tls_sni.to_string()); }
                                                                        }
                                                                    }
                                                                >{label}</button>
                                                            }
                                                        }).collect_view()}
                                                    </div>
                                                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                                        <div>
                                                            <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_provider_label()}</label>
                                                            {move || {
                                                                form_load_generation.get();
                                                                let current = provider.get();
                                                                view! {
                                                                    <select
                                                                        prop:value=current.clone()
                                                                        on:change=move |ev| {
                                                                            let new_prov = event_target_value(&ev);
                                                                            provider.set(new_prov.clone());
                                                                            // Reset model when provider changes so the select auto-initializes.
                                                                            let new_models = models_for_provider(&new_prov);
                                                                            if new_models.is_empty() {
                                                                                model.set(String::new());
                                                                            } else {
                                                                                model.set(new_models[0].to_string());
                                                                            }
                                                                        }
                                                                        class="input text-sm"
                                                                    >
                                                                <option value="deepseek">DeepSeek</option>
                                                                <option value="mimo">MiMo</option>
                                                                <option value="openai">OpenAI</option>
                                                                <option value="codex">Codex</option>
                                                                <option value="anthropic">Anthropic</option>
                                                                <optgroup label="International">
                                                                    <option value="groq">Groq</option>
                                                                    <option value="xai">xAI</option>
                                                                    <option value="mistral">Mistral</option>
                                                                    <option value="gemini">Gemini</option>
                                                                    <option value="perplexity">Perplexity</option>
                                                                    <option value="together">Together</option>
                                                                    <option value="fireworks">Fireworks</option>
                                                                    <option value="cerebras">Cerebras</option>
                                                                    <option value="cohere">Cohere</option>
                                                                    <option value="nvidia">NVIDIA NIM</option>
                                                                    <option value="nebius">Nebius</option>
                                                                    <option value="siliconflow">SiliconFlow</option>
                                                                    <option value="hyperbolic">Hyperbolic</option>
                                                                    <option value="openrouter">OpenRouter</option>
                                                                    <option value="reka">Reka</option>
                                                                </optgroup>
                                                                <optgroup label="Cloud">
                                                                    <option value="azure-openai">Azure OpenAI</option>
                                                                    <option value="azure-ai">Azure AI</option>
                                                                    <option value="bedrock">Bedrock</option>
                                                                    <option value="vertex-ai">Vertex AI</option>
                                                                    <option value="watsonx">watsonx</option>
                                                                    <option value="oci">OCI</option>
                                                                    <option value="sap">SAP</option>
                                                                </optgroup>
                                                                <optgroup label="中国">
                                                                    <option value="alibaba">通义千问</option>
                                                                    <option value="qianfan">百度千帆</option>
                                                                    <option value="glm">智谱 GLM</option>
                                                                    <option value="kimi">Kimi</option>
                                                                    <option value="minimax">Minimax</option>
                                                                    <option value="volcengine">火山引擎</option>
                                                                    <option value="tencent">混元</option>
                                                                    <option value="iflytek">讯飞星火</option>
                                                                    <option value="baichuan">百川</option>
                                                                    <option value="yi">零一万物</option>
                                                                    <option value="stepfun">阶跃星辰</option>
                                                                    <option value="360ai">360 AI</option>
                                                                    <option value="sensenova">商汤</option>
                                                                    <option value="coze">Coze</option>
                                                                    <option value="baidu">百度 ERNIE</option>
                                                                    <option value="doubao">豆包</option>
                                                                    <option value="moonshot">Moonshot</option>
                                                                    <option value="sparkdesk">星火 SparkDesk</option>
                                                                </optgroup>
                                                                <optgroup label="Inference">
                                                                    <option value="deepinfra">DeepInfra</option>
                                                                    <option value="sambanova">SambaNova</option>
                                                                    <option value="github-models">GitHub Models</option>
                                                                    <option value="huggingface">HuggingFace</option>
                                                                    <option value="replicate">Replicate</option>
                                                                    <option value="ollama-cloud">Ollama Cloud</option>
                                                                    <option value="aimlapi">AI/ML API</option>
                                                                    <option value="novita">Novita AI</option>
                                                                    <option value="chutes">Chutes.ai</option>
                                                                    <option value="poe">Poe</option>
                                                                    <option value="phind">Phind</option>
                                                                    <option value="lambda-ai">Lambda AI</option>
                                                                    <option value="nscale">nScale</option>
                                                                    <option value="ovhcloud">OVHcloud</option>
                                                                    <option value="baseten">Baseten</option>
                                                                    <option value="databricks">Databricks</option>
                                                                    <option value="snowflake">Snowflake</option>
                                                                    <option value="wandb">W&B</option>
                                                                    <option value="ai21">AI21</option>
                                                                    <option value="gigachat">GigaChat</option>
                                                                    <option value="venice">Venice</option>
                                                                    <option value="codestral">Codestral</option>
                                                                    <option value="upstage">Upstage</option>
                                                                    <option value="maritalk">Maritalk</option>
                                                                    <option value="modal">Modal</option>
                                                                    <option value="vercel-ai-gateway">Vercel AI</option>
                                                                    <option value="meta-llama">Meta Llama</option>
                                                                    <option value="v0-vercel">v0</option>
                                                                    <option value="morph">Morph</option>
                                                                    <option value="featherless-ai">Featherless</option>
                                                                    <option value="llm7">LLM7</option>
                                                                    <option value="lepton">Lepton</option>
                                                                    <option value="kluster">Kluster</option>
                                                                    <option value="friendliai">FriendliAI</option>
                                                                    <option value="llamagate">LlamaGate</option>
                                                                    <option value="heroku">Heroku</option>
                                                                    <option value="galadriel">Galadriel</option>
                                                                    <option value="datarobot">DataRobot</option>
                                                                    <option value="clarifai">Clarifai</option>
                                                                    <option value="gitlawb">Gitlawb</option>
                                                                    <option value="inference-net">Inference.net</option>
                                                                    <option value="nanogpt">NanoGPT</option>
                                                                    <option value="predibase">Predibase</option>
                                                                    <option value="bytez">Bytez</option>
                                                                    <option value="piapi">PiAPI</option>
                                                                    <option value="getgoapi">GoAPI</option>
                                                                    <option value="laozhang">LaoZhang</option>
                                                                    <option value="glhf">GLHF</option>
                                                                    <option value="cablyai">CablyAI</option>
                                                                    <option value="thebai">TheB.AI</option>
                                                                    <option value="fenayai">FenayAI</option>
                                                                    <option value="empower">Empower</option>
                                                                    <option value="nous-research">Nous Research</option>
                                                                    <option value="petals">Petals</option>
                                                                    <option value="gitlab">GitLab</option>
                                                                    <option value="voyage-ai">Voyage AI</option>
                                                                    <option value="jina-ai">Jina AI</option>
                                                                    <option value="fal-ai">Fal.ai</option>
                                                                    <option value="stability-ai">Stability AI</option>
                                                                    <option value="black-forest-labs">Black Forest Labs</option>
                                                                    <option value="recraft">Recraft</option>
                                                                    <option value="poolside">Poolside</option>
                                                                    <option value="arcee-ai">Arcee AI</option>
                                                                    <option value="inclusionai">InclusionAI</option>
                                                                    <option value="liquid">Liquid AI</option>
                                                                    <option value="nomic">Nomic</option>
                                                                    <option value="krutrim">Krutrim</option>
                                                                    <option value="monsterapi">MonsterAPI</option>
                                                                    <option value="byteplus">BytePlus</option>
                                                                    <option value="bluesminds">BluesMinds</option>
                                                                    <option value="freemodel-dev">FreeModel</option>
                                                                    <option value="blackbox">Blackbox</option>
                                                                    <option value="bazaarlink">BazaarLink</option>
                                                                    <option value="completions">Completions.me</option>
                                                                    <option value="enally">Enally</option>
                                                                    <option value="freetheai">FreeTheAI</option>
                                                                    <option value="crof">CrofAI</option>
                                                                    <option value="longcat">LongCat</option>
                                                                    <option value="pollinations">Pollinations</option>
                                                                    <option value="puter">Puter</option>
                                                                    <option value="uncloseai">UncloseAI</option>
                                                                    <option value="agentrouter">AgentRouter</option>
                                                                    <option value="command-code">Command Code</option>
                                                                    <option value="astraflow">Astraflow</option>
                                                                    <option value="opencode-zen">OpenCode Zen</option>
                                                                    <option value="opencode-go">OpenCode Go</option>
                                                                    <option value="zai">Z.AI</option>
                                                                    <option value="huggingchat">HuggingChat</option>
                                                                    <option value="dify">Dify</option>
                                                                    <option value="publicai">PublicAI</option>
                                                                    <option value="sapio">Sapio</option>
                                                                    <option value="freeaiapikey">FreeAIAPIKey</option>
                                                                    <option value="cloudflare-ai">Cloudflare AI</option>
                                                                    <option value="scaleway">Scaleway</option>
                                                                    <option value="api-airforce">Api.airforce</option>
                                                                    <option value="qoder">Qoder</option>
                                                                    <option value="hackclub">Hackclub</option>
                                                                </optgroup>
                                                                <option value="custom">Custom</option>
                                                                    </select>
                                                                }
                                                            }}
                                                        </div>
                                                        <div>
                                                            <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_base_url_label()}</label>
                                                            <input type="text" prop:value=move || base_url.get()
                                                                on:input=move |ev| base_url.set(event_target_value(&ev))
                                                                class="input font-mono text-sm" placeholder="https://api.deepseek.com"
                                                            />
                                                        </div>
                                                        <div>
                                                            <div class="flex items-center justify-between mb-1">
                                                                <label class="block text-xs font-semibold text-theme-muted">{t.upstream_model_label()}</label>
                                                                <button type="button" class="text-xs text-accent disabled:opacity-50"
                                                                    disabled=move || profile_models_syncing.get()
                                                                    on:click=move |_| on_sync_profile_models.run(())
                                                                >
                                                                    {move || if profile_models_syncing.get() { t.models_syncing() } else { t.models_sync_btn() }}
                                                                </button>
                                                            </div>
                                                            {move || {
                                                                let options = effective_model_list(&profile_model_options.get(), &provider.get());
                                                                if options.is_empty() {
                                                                    view! { <input type="text" prop:value=move || model.get()
                                                                        on:input=move |ev| model.set(event_target_value(&ev))
                                                                        class="input font-mono text-sm" placeholder="model-name"
                                                                    /> }.into_any()
                                                                } else {
                                                                    view! {
                                                                        <select class="input font-mono text-sm"
                                                                            prop:value=move || {
                                                                                let m = model.get();
                                                                                let opts = effective_model_list(&profile_model_options.get(), &provider.get());
                                                                                if m == CUSTOM_MODEL_SENTINEL { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                                else if opts.iter().any(|o| o == &m) { m }
                                                                                else { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                            }
                                                                            on:change=move |ev| {
                                                                                let v = event_target_value(&ev);
                                                                                if v != CUSTOM_MODEL_SENTINEL { model.set(v); } else { model.set(String::new()); }
                                                                            }
                                                                        >
                                                                            {options.iter().map(|m| {
                                                                                let value = m.clone();
                                                                                let text = m.clone();
                                                                                view! { <option value=value>{text}</option> }
                                                                            }).collect_view()}
                                                                            <option value=CUSTOM_MODEL_SENTINEL>{t.upstream_model_custom()}</option>
                                                                        </select>
                                                                    }.into_any()
                                                                }
                                                            }}
                                                            {move || {
                                                                let m = model.get();
                                                                let options = effective_model_list(&profile_model_options.get(), &provider.get());
                                                                let is_custom = m == CUSTOM_MODEL_SENTINEL || (!m.is_empty() && !options.is_empty() && !options.contains(&m));
                                                                is_custom.then(|| view! {
                                                                    <input type="text" class="input font-mono text-sm mt-2" placeholder="model-name"
                                                                        prop:value=move || { let m = model.get(); if m == CUSTOM_MODEL_SENTINEL { String::new() } else { m } }
                                                                        on:input=move |ev| model.set(event_target_value(&ev))
                                                                    />
                                                                })
                                                            }}
                                                            {move || {
                                                                let n = profile_model_options.get().len();
                                                                (n > 0).then(|| view! {
                                                                    <p class="text-xs text-theme-muted mt-1">{n} {t.upstream_template_models_count()}</p>
                                                                })
                                                            }}
                                                        </div>
                                                        {move || (drawer_profile.get().unwrap_or_default() != default_profile_id.get_untracked()).then(|| view! {
                                                            <div>
                                                                <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_tls_sni_label()}</label>
                                                                <input type="text" prop:value=move || tls_sni.get()
                                                                    on:input=move |ev| tls_sni.set(event_target_value(&ev))
                                                                    class="input font-mono text-sm" placeholder="e.g. api.deepseek.com"
                                                                />
                                                            </div>
                                                        })}
                                                    </div>
                                                    <div class="space-y-2">
                                                        <button type="button" class="text-xs text-accent"
                                                            on:click=move |_| show_advanced.update(|v| *v = !*v)
                                                        >
                                                            {move || if show_advanced.get() { t.upstream_hide_advanced() } else { t.upstream_show_advanced() }}
                                                        </button>
                                                        {move || show_advanced.get().then(|| view! {
                                                            <div class="mt-2">
                                                                <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_endpoints_label()}</label>
                                                                <textarea prop:value=move || endpoints_text.get()
                                                                    on:input=move |ev| endpoints_text.set(event_target_value(&ev))
                                                                    class="input font-mono text-sm h-24 resize-y"
                                                                    placeholder="api.deepseek.com:443"
                                                                ></textarea>
                                                                <p class="text-xs text-theme-muted mt-1">{t.upstream_endpoints_hint()}</p>
                                                            </div>
                                                            <div class="mt-3">
                                                                <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_proxy_label()}</label>
                                                                <input type="text" prop:value=move || proxy_url.get()
                                                                    on:input=move |ev| proxy_url.set(event_target_value(&ev))
                                                                    class="input font-mono text-sm" placeholder="socks5://127.0.0.1:1080"
                                                                />
                                                                <p class="text-xs text-theme-muted mt-1">{t.upstream_proxy_hint()}</p>
                                                            </div>
                                                            <div class="mt-3">
                                                                <label class="block text-xs font-semibold text-theme-muted mb-1">"Fallback Profile"</label>
                                                                <select
                                                                    class="input font-mono text-sm w-full"
                                                                    prop:value=move || fallback_profile_id.get()
                                                                    on:change=move |ev| fallback_profile_id.set(event_target_value(&ev))
                                                                >
                                                                    <option value="">"None"</option>
                                                                    {move || {
                                                                        let current = active_profile.get();
                                                                        profiles.get().into_iter()
                                                                            .filter(|p| p.id != current)
                                                                            .map(|p| view! { <option value=p.id.clone()>{p.id.clone()}</option> })
                                                                            .collect_view()
                                                                    }}
                                                                </select>
                                                                <p class="text-xs text-theme-muted mt-1">"Auto-failover to this profile on errors."</p>
                                                            </div>
                                                            <div class="mt-3">
                                                                <ConfigRangeU64
                                                                    label=move || format!("Fallback Max Retries: {}", fallback_max_retries.get())
                                                                    value=fallback_max_retries min=1 max=5 min_hint="1" max_hint="5" accent="blue"
                                                                />
                                                            </div>
                                                        })}
                                                    </div>
                                                    <div class="upstream-drawer-test-section space-y-4">
                                                        <h4 class="text-sm font-semibold text-theme">
                                                            {t.upstream_connection_test_title()}
                                                        </h4>
                                                        <div class="test-result-panel">
                                                            {move || if let Some(res) = test_result.get() {
                                                                view! {
                                                                    <div class="test-result-row">
                                                                        <span class="test-result-label">{t.upstream_test_status_code()}</span>
                                                                        <span class=move || if res.ok { "test-result-value test-result-success font-semibold" } else { "test-result-value test-result-error font-semibold" }>
                                                                            {if res.ok { "✓ Success" } else { "✗ Failed" }}
                                                                        </span>
                                                                    </div>
                                                                    <div class="test-result-row">
                                                                        <span class="test-result-label">{t.upstream_test_latency()}</span>
                                                                        <span class="test-result-value test-result-value-mono">{format!("{} ms", res.latency_ms)}</span>
                                                                    </div>
                                                                    <div class="test-result-row">
                                                                        <span class="test-result-label">{t.upstream_test_models_found()}</span>
                                                                        <span class="test-result-value test-result-value-mono">{res.model_count.unwrap_or(0)}</span>
                                                                    </div>
                                                                    {res.error.map(|err| view! {
                                                                        <div class="text-xs text-error mt-2 p-2 bg-error/5 rounded border border-error/10">
                                                                            {err}
                                                                        </div>
                                                                    })}
                                                                }.into_any()
                                                            } else if !test_error.get().is_empty() {
                                                                view! {
                                                                    <div class="text-xs text-error p-2 bg-error/5 rounded border border-error/10 font-mono">
                                                                        {test_error.get()}
                                                                    </div>
                                                                }.into_any()
                                                            } else {
                                                                view! {
                                                                    <p class="text-xs text-theme-muted text-center py-3">
                                                                        {t.upstream_test_not_run()}
                                                                    </p>
                                                                }.into_any()
                                                            }}
                                                        </div>
                                                        <div class="flex flex-wrap items-center gap-3">
                                                            <button
                                                                type="button"
                                                                on:click=move |_| on_test.run(())
                                                                disabled=move || testing.get()
                                                                class="btn btn-secondary text-xs"
                                                            >
                                                                {move || if testing.get() { t.upstream_testing() } else { t.upstream_test_btn() }}
                                                            </button>
                                                            <button type="button" on:click=move |_| on_save.run(()) disabled=move || saving.get() class="btn btn-primary text-xs">
                                                                {move || if saving.get() { t.upstream_saving() } else { t.upstream_save_btn() }}
                                                            </button>
                                                            {move || if saved.get() {
                                                                view! { <span class="text-xs text-accent font-medium">{t.upstream_saved()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                            {move || if !save_error.get().is_empty() {
                                                                view! { <span class="text-xs text-error">{save_error.get()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                        </div>
                                                    </div>
                                                </div>
                                            }.into_any()
                                        },
                                        1 => {
                                            // Keys tab — key pool
                                            view! {
                                                <div class="space-y-4">
                                                    <div>
                                                        <h3 class="text-base font-semibold text-theme">
                                                            {move || t.upstream_pool_title_for(&drawer_profile.get().unwrap_or_default())}
                                                        </h3>
                                                        <p class="text-xs text-theme-muted mt-1">{move || t.upstream_pool_desc()}</p>
                                                        {move || (drawer_profile.get().unwrap_or_default() == default_profile_id.get_untracked()).then(|| view! {
                                                            <p class="text-xs text-theme-muted mt-1">{t.upstream_pool_deepseek_hint()}</p>
                                                        })}
                                                        {move || (drawer_profile.get().unwrap_or_default() != default_profile_id.get_untracked()).then(|| view! {
                                                            <p class="text-xs text-theme-muted mt-1">{t.upstream_pool_patch_deepseek_only()}</p>
                                                        })}
                                                    </div>
                                                    {move || match key_pool.get() {
                                                        None => view! { <Spinner /> }.into_any(),
                                                        Some(Err(e)) => view! { <p class="text-xs text-error">{e}</p> }.into_any(),
                                                        Some(Ok(pool)) => {
                                                            let keys_for_all = pool.keys.clone();
                                                            let pid_for_all = drawer_profile.get().unwrap_or_default();
                                                            let pid_for_toggle = pid_for_all.clone();
                                                            let pid_for_test = pid_for_all.clone();
                                                            let pid_for_delete = pid_for_all.clone();
                                                            let probe_all = probe.clone();
                                                            let reload_pool = reload.clone();
                                                            view! {
                                                                <div>
                                                                    <div class="flex items-center gap-2 mb-3 flex-wrap">
                                                                        <button type="button" class="btn btn-secondary text-xs"
                                                                            disabled=move || testing_all.get() || quota_auto_refreshing.get()
                                                                            on:click={{
                                                                                let keys = keys_for_all.clone();
                                                                                let pid = pid_for_all.clone();
                                                                                let probe = probe_all.clone();
                                                                                move |_| {
                                                                                    testing_all.set(true);
                                                                                    probe.clone()(pid.clone(), keys.clone(), false, true);
                                                                                }
                                                                            }}
                                                                        >
                                                                            {move || if testing_all.get() || quota_auto_refreshing.get() {
                                                                                t.upstream_testing_all()
                                                                            } else {
                                                                                t.upstream_test_all_quotas()
                                                                            }}
                                                                        </button>
                                                                        <button type="button" class="text-xs text-accent cursor-pointer"
                                                                            on:click=move |_| { key_test_results.set(HashMap::new()); }
                                                                        >{t.upstream_clear_results()}</button>
                                                                        {move || quota_auto_refreshing.get().then(|| view! {
                                                                            <span class="text-xs text-theme-muted">{t.upstream_pool_auto_quota()}</span>
                                                                        })}
                                                                        {move || (!pool_delete_error.get().is_empty()).then(|| view! {
                                                                            <span class="text-xs text-error">{pool_delete_error.get()}</span>
                                                                        })}
                                                                    </div>
                                                                    <UpstreamKeyPoolCards
                                                                        keys=pool.keys.clone()
                                                                        on_toggle=Callback::new({
                                                                            let reload = reload_pool.clone();
                                                                            move |(id, next): (String, bool)| {
                                                                                let pid = pid_for_toggle.clone();
                                                                                let reload = reload.clone();
                                                                                leptos::task::spawn_local(async move {
                                                                                    let req = PatchUpstreamKeyRequest { enabled: Some(next), secret: None, priority: None };
                                                                                    let default_id = default_profile_id.get_untracked();
                                                                                    let result = if pid == default_id {
                                                                                        api::patch_upstream_key(&id, &req).await
                                                                                    } else {
                                                                                        api::patch_upstream_profile_key(&pid, &id, &req).await
                                                                                    };
                                                                                    if let Err(e) = result {
                                                                                        pool_delete_error.try_set(format!("Toggle failed: {e}"));
                                                                                    }
                                                                                    reload.clone()(pid);
                                                                                });
                                                                            }
                                                                        })
                                                                        on_test=Callback::new({
                                                                            move |kid: String| {
                                                                                let pid = pid_for_test.clone();
                                                                                leptos::task::spawn_local(async move {
                                                                                    key_testing.try_update(|m| { m.insert(kid.clone(), true); });
                                                                                    let result = api::test_upstream_profile_key(&pid, &kid).await;
                                                                                    key_testing.try_update(|m| { m.insert(kid.clone(), false); });
                                                                                    let tr = match result {
                                                                                        Ok(r) => r,
                                                                                        Err(e) => UpstreamTestResult {
                                                                                            ok: false, status_code: 0, latency_ms: 0,
                                                                                            model_count: None, error: Some(e), quota: None,
                                                                                        },
                                                                                    };
                                                                                    key_test_results.try_update(|m| { m.insert(kid, tr); });
                                                                                });
                                                                            }
                                                                        })
                                                                        on_delete_confirm=Callback::new({
                                                                            let reload = reload_pool.clone();
                                                                            move |kid: String| {
                                                                                let pid = pid_for_delete.clone();
                                                                                let reload = reload.clone();
                                                                                let err_prefix = t.upstream_pool_delete_error().to_string();
                                                                                leptos::task::spawn_local(async move {
                                                                                    key_deleting.try_update(|m| { m.insert(kid.clone(), true); });
                                                                                    pool_delete_error.set(String::new());
                                                                                    let default_id = default_profile_id.get_untracked();
                                                                                    let result = if pid == default_id {
                                                                                        api::delete_upstream_key(&kid).await
                                                                                    } else {
                                                                                        api::delete_upstream_profile_key(&pid, &kid).await
                                                                                    };
                                                                                    key_deleting.try_update(|m| { m.insert(kid.clone(), false); });
                                                                                    delete_confirm_id.set(None);
                                                                                    match result {
                                                                                        Ok(()) => reload.clone()(pid),
                                                                                        Err(e) => pool_delete_error.set(format!("{err_prefix}: {e}")),
                                                                                    }
                                                                                });
                                                                            }
                                                                        })
                                                                        key_testing=key_testing.read_only()
                                                                        key_test_results=key_test_results.read_only()
                                                                        delete_confirm_id=delete_confirm_id
                                                                        key_deleting=key_deleting.read_only()
                                                                    />
                                                                </div>
                                                            }.into_any()
                                                        },
                                                    }}
                                                    {move || {
                                                        let prov = provider.get();
                                                        let b_url = base_url.get();
                                                        let pid = drawer_profile.get().unwrap_or_default();
                                                        let is_codex = prov == "codex" || prov == "openai" || b_url.contains("chatgpt.com");
                                                        is_codex.then(|| view! {
                                                            <CodexOAuthPanel
                                                                profile_id=pid
                                                                on_pool_changed=Callback::new(move |_: ()| { signal_refresh_key_pool(); })
                                                            />
                                                        })
                                                    }}
                                                    <div class="space-y-4 pt-4 border-t border-theme/10">
                                                        <div>
                                                            <label class="block text-xs font-semibold text-theme-muted mb-1">
                                                                {move || if pool_replace_mode.get() { t.upstream_pool_replace_label() } else { t.upstream_pool_append_label() }}
                                                            </label>
                                                            <label class="flex items-center gap-2 text-xs text-theme-muted mb-2">
                                                                <input type="checkbox" prop:checked=move || pool_replace_mode.get()
                                                                    on:change=move |ev| pool_replace_mode.set(event_target_checked(&ev))
                                                                />
                                                                {t.upstream_pool_replace_confirm()}
                                                            </label>
                                                            <textarea prop:value=move || pool_secrets_text.get()
                                                                on:input=move |ev| pool_secrets_text.set(event_target_value(&ev))
                                                                class="input font-mono text-sm h-24 resize-y"
                                                                placeholder="sk-...\nacct-b:sk-...\n"
                                                            ></textarea>
                                                            <p class="text-xs text-theme-muted mt-1">
                                                                {move || t.upstream_pool_hint()}
                                                            </p>
                                                        </div>
                                                        <div class="flex items-center gap-3">
                                                            <button on:click=move |_| on_save_pool.run(()) disabled=move || pool_saving.get() class="btn btn-primary text-xs">
                                                                {move || if pool_saving.get() { t.upstream_pool_saving() } else { t.upstream_pool_save_btn() }}
                                                            </button>
                                                            {move || if pool_saved.get() {
                                                                view! { <span class="text-xs text-accent font-medium">{t.upstream_pool_saved()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                            {move || if !pool_error.get().is_empty() {
                                                                view! { <span class="text-xs text-error">{pool_error.get()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                        </div>
                                                    </div>
                                                </div>
                                            }.into_any()
                                        },
                                        _ => {
                                            // Routing tab
                                            let pid = drawer_profile.get().unwrap_or_default();
                                            view! { <RoutingTab profile_id=pid /> }.into_any()
                                        },
                                    }
                                }
                                }}
                            </div>
                        </div>
                    </div>
                }.into_any()
                }
            }}

            {move || sync_result.get().map(|r| view! { <SyncResultCard result=r /> })}

            {move || show_delete_confirm.get().then(|| view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 animate-fade-in">
                    <div class="glass-card max-w-sm w-full space-y-4">
                        <h4 class="text-sm font-semibold text-theme">{t.upstream_delete_profile_btn()}</h4>
                        <p class="text-xs text-theme-muted">{t.upstream_profile_delete_confirm()}</p>
                        <div class="flex gap-2 justify-end">
                            <button
                                class="btn btn-secondary text-xs"
                                on:click=move |_| show_delete_confirm.set(false)
                            >
                                "Cancel"
                            </button>
                            <button
                                class="btn btn-primary text-xs"
                                on:click=move |_| on_delete_profile.run(())
                                disabled=move || deleting.get()
                            >
                                {move || if deleting.get() { "Deleting..." } else { "Delete" }}
                            </button>
                        </div>
                    </div>
                </div>
            })}
        </div>
    }
}

/// Displays a quota progress bar for an upstream key (balance in CNY).
#[component]
fn QuotaProgressBar(quota: KeyQuotaInfo) -> impl IntoView {
    let t = use_translations();
    let percentage = match (quota.balance, quota.total_granted) {
        (Some(b), Some(g)) if g > 0.0 => Some((b / g * 100.0).clamp(0.0, 100.0)),
        _ => None,
    };

    let bar_color = match percentage {
        Some(p) if p > 50.0 => "bg-success",
        Some(p) if p > 20.0 => "bg-warning",
        Some(_) => "bg-error",
        None => "bg-theme-muted",
    };

    let available_icon = match quota.is_available {
        Some(true) => {
            view! { <span class="text-success text-xs mr-1">{t.upstream_quota_available()}</span> }
                .into_any()
        }
        Some(false) => {
            view! { <span class="text-error text-xs mr-1">{t.upstream_quota_exhausted()}</span> }
                .into_any()
        }
        None => view! { <span></span> }.into_any(),
    };

    view! {
        <div class="min-w-[120px]">
            <div class="flex items-center gap-1 mb-0.5">
                {available_icon}
                {match (quota.balance, quota.total_granted) {
                    (Some(b), Some(g)) => view! {
                        <span class="text-xs font-mono">
                            {format!("CNY {:.2} / {:.2}", b, g)}
                        </span>
                    }.into_any(),
                    (Some(b), _) => view! {
                        <span class="text-xs font-mono">
                            {format!("CNY {:.2}", b)}
                        </span>
                    }.into_any(),
                    _ => view! {
                        <span class="text-xs text-theme-muted">{t.upstream_quota_na()}</span>
                    }.into_any(),
                }}
            </div>
            {percentage.map(|p| view! {
                <div class="w-full h-1.5 rounded-full bg-theme/10 overflow-hidden">
                    <div
                        class={format!("h-full rounded-full transition-all {}", bar_color)}
                        style={format!("width: {:.1}%", p)}
                    ></div>
                </div>
            })}
        </div>
    }
}
