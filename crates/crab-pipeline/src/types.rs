use crate::cursor_models::CursorModelsConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestPipeline {
    CursorDeepSeekV4,
    DeepSeekLight,
    MimoTokenPlanRelay,
    MimoPaygRelay,
    GenericRelay,
    /// Codex (ChatGPT) Responses API relay — translates Chat Completions ↔ Responses API.
    CodexRelay,
}

impl RequestPipeline {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestPipeline::CursorDeepSeekV4 => "cursor_deepseek_v4",
            RequestPipeline::DeepSeekLight => "deepseek_light",
            RequestPipeline::MimoTokenPlanRelay => "mimo_token_plan_relay",
            RequestPipeline::MimoPaygRelay => "mimo_payg_relay",
            RequestPipeline::GenericRelay => "generic_relay",
            RequestPipeline::CodexRelay => "codex_relay",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProvider {
    Deepseek,
    Mimo,
    Openai,
    Codex,
    Anthropic,
    Other,
}

impl UpstreamProvider {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "deepseek" => Self::Deepseek,
            "mimo" | "xiaomi" => Self::Mimo,
            "openai" => Self::Openai,
            "codex" => Self::Codex,
            "anthropic" => Self::Anthropic,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            UpstreamProvider::Deepseek => "deepseek",
            UpstreamProvider::Mimo => "mimo",
            UpstreamProvider::Openai => "openai",
            UpstreamProvider::Codex => "codex",
            UpstreamProvider::Anthropic => "anthropic",
            UpstreamProvider::Other => "other",
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
    MimoTokenPlanRelay,
    MimoPaygRelay,
    GenericRelay,
    CodexRelay,
}

impl PipelineOverride {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cursor_deepseek_v4" => Self::CursorDeepSeekV4,
            "deepseek_light" => Self::DeepSeekLight,
            "mimo_token_plan_relay" => Self::MimoTokenPlanRelay,
            "mimo_payg_relay" => Self::MimoPaygRelay,
            "generic_relay" => Self::GenericRelay,
            "codex_relay" => Self::CodexRelay,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PipelineOverride::Auto => "auto",
            PipelineOverride::CursorDeepSeekV4 => "cursor_deepseek_v4",
            PipelineOverride::DeepSeekLight => "deepseek_light",
            PipelineOverride::MimoTokenPlanRelay => "mimo_token_plan_relay",
            PipelineOverride::MimoPaygRelay => "mimo_payg_relay",
            PipelineOverride::GenericRelay => "generic_relay",
            PipelineOverride::CodexRelay => "codex_relay",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl PipelineSelectionReason {
    pub fn as_str(self) -> &'static str {
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
}

impl Default for PipelineGlobals {
    fn default() -> Self {
        Self {
            default_upstream_profile: "deepseek".to_string(),
            pipeline_mode: PipelineMode::Auto,
            known_profile_ids: vec!["deepseek".to_string()],
            cursor_models: CursorModelsConfig::default(),
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
}

#[derive(Debug, Clone)]
pub struct PipelineSelection {
    pub pipeline: RequestPipeline,
    pub upstream_profile_id: String,
    pub provider: UpstreamProvider,
    pub reason: PipelineSelectionReason,
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
