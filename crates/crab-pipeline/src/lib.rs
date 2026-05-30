mod codex_model_aliases;
mod cursor_models;
mod profile;
mod select;
mod signals;
mod types;

pub use codex_model_aliases::{
    canonicalize_client_model, is_openai_or_codex_display_model, normalize_client_model_key,
    resolve_codex_display_alias,
};
pub use cursor_models::{
    CursorModelEntry, CursorModelsConfig, synthetic_models_list_json, validate_cursor_models,
};
pub use profile::{model_prefix_to_profile, resolve_upstream_profile_id};
pub use select::{select_request_pipeline, validate_pipeline_override};
pub use signals::cursor_agent_signals;
pub use types::{
    PipelineGlobals, PipelineMode, PipelineOverride, PipelineRequestContext, PipelineSelection,
    PipelineSelectionReason, ProfileDescriptor, RequestPipeline, UpstreamProvider,
    CODEX_STATIC_MODELS,
};

pub fn is_deepseek_v4_model(model: &str) -> bool {
    let base = crab_reasoning::parse_deepseek_v4_thinking_suffix(model).base_model;
    base.starts_with("deepseek-v4-")
}
