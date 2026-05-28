use crate::cursor_models::CursorModelsConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestPipeline {
    CursorDeepSeekV4,
    DeepSeekLight,
    MimoRelay,
    MimoTokenPlanRelay,
    MimoPaygRelay,
    GenericRelay,
}

impl RequestPipeline {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestPipeline::CursorDeepSeekV4 => "cursor_deepseek_v4",
            RequestPipeline::DeepSeekLight => "deepseek_light",
            RequestPipeline::MimoRelay => "mimo_relay",
            RequestPipeline::MimoTokenPlanRelay => "mimo_token_plan_relay",
            RequestPipeline::MimoPaygRelay => "mimo_payg_relay",
            RequestPipeline::GenericRelay => "generic_relay",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProvider {
    Deepseek,
    Mimo,
    Openai,
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
            "anthropic" => Self::Anthropic,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            UpstreamProvider::Deepseek => "deepseek",
            UpstreamProvider::Mimo => "mimo",
            UpstreamProvider::Openai => "openai",
            UpstreamProvider::Anthropic => "anthropic",
            UpstreamProvider::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineOverride {
    #[default]
    Auto,
    CursorDeepSeekV4,
    DeepSeekLight,
    MimoRelay,
    MimoTokenPlanRelay,
    MimoPaygRelay,
    GenericRelay,
}

impl PipelineOverride {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cursor_deepseek_v4" => Self::CursorDeepSeekV4,
            "deepseek_light" => Self::DeepSeekLight,
            "mimo_relay" => Self::MimoRelay,
            "mimo_token_plan_relay" => Self::MimoTokenPlanRelay,
            "mimo_payg_relay" => Self::MimoPaygRelay,
            "generic_relay" => Self::GenericRelay,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PipelineOverride::Auto => "auto",
            PipelineOverride::CursorDeepSeekV4 => "cursor_deepseek_v4",
            PipelineOverride::DeepSeekLight => "deepseek_light",
            PipelineOverride::MimoRelay => "mimo_relay",
            PipelineOverride::MimoTokenPlanRelay => "mimo_token_plan_relay",
            PipelineOverride::MimoPaygRelay => "mimo_payg_relay",
            PipelineOverride::GenericRelay => "generic_relay",
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
            PipelineSelectionReason::ModelAlias => "model_alias",
        }
    }
}

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
