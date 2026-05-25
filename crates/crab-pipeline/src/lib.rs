mod cursor_models;
mod profile;
mod select;
mod signals;
mod types;

pub use cursor_models::{
    CursorModelEntry, CursorModelsConfig, synthetic_models_list_json, validate_cursor_models,
};
pub use profile::{model_prefix_to_profile, resolve_upstream_profile_id};
pub use select::{select_request_pipeline, validate_pipeline_override};
pub use signals::cursor_agent_signals;
pub use types::{
    PipelineGlobals, PipelineMode, PipelineOverride, PipelineRequestContext, PipelineSelection,
    PipelineSelectionReason, ProfileDescriptor, RequestPipeline, UpstreamProvider,
};

pub fn is_deepseek_v4_model(model: &str) -> bool {
    let base = crab_reasoning::parse_deepseek_v4_thinking_suffix(model).base_model;
    base.starts_with("deepseek-v4-")
}
