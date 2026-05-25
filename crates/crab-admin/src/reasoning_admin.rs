//! Map gateway reasoning runtime API ↔ Admin/Dashboard `ReasoningConfig`.

use crab_admin_types::ReasoningConfig;
use crab_control::ReasoningRuntimeConfigView;

pub fn config_from_gateway(view: &ReasoningRuntimeConfigView) -> ReasoningConfig {
    let storage_backend = view.storage_backend.clone().unwrap_or_default();
    let cache_db_path = view.cache_db_path.clone().unwrap_or_default();
    let sqlite = storage_backend == "sqlite";
    ReasoningConfig {
        thinking_mode: view.thinking_mode.clone(),
        reasoning_effort: view.reasoning_effort.clone(),
        missing_reasoning_strategy: view.missing_reasoning_strategy.clone(),
        display_reasoning: view.display_reasoning,
        collapsible_reasoning: view.collapsible_reasoning,
        cache_invalidate_recommended: view.cache_invalidate_recommended.then_some(true),
        storage_backend: storage_backend.clone(),
        cache_db_path: cache_db_path.clone(),
        redis_url_masked: view.redis_url_masked.clone(),
        sqlite_cache_enabled: sqlite,
        sqlite_cache_path: if sqlite && !cache_db_path.is_empty() {
            Some(cache_db_path)
        } else {
            None
        },
        reasoning_recovery: Some(view.missing_reasoning_strategy == "recover"),
    }
}

pub fn gateway_put_from_config(req: &ReasoningConfig) -> ReasoningRuntimeConfigView {
    let missing_reasoning_strategy = if !req.missing_reasoning_strategy.is_empty() {
        req.missing_reasoning_strategy.clone()
    } else {
        req.reasoning_recovery
            .map(ReasoningConfig::missing_reasoning_strategy_from_recovery)
            .unwrap_or_else(|| "recover".to_string())
    };
    ReasoningRuntimeConfigView {
        thinking_mode: ReasoningConfig::thinking_mode_for_gateway(&req.thinking_mode),
        reasoning_effort: req.reasoning_effort.clone(),
        missing_reasoning_strategy,
        display_reasoning: req.display_reasoning,
        collapsible_reasoning: req.collapsible_reasoning,
        cache_invalidate_recommended: false,
        storage_backend: None,
        cache_db_path: None,
        redis_url_masked: None,
    }
}

pub fn config_after_put(view: &ReasoningRuntimeConfigView) -> ReasoningConfig {
    config_from_gateway(view)
}
