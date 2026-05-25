//! Reasoning admin API types (Dashboard ↔ Admin ↔ Gateway runtime).

use serde::{Deserialize, Serialize};

/// Unified reasoning settings for Admin API (`/api/admin/reasoning/config`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningConfig {
    pub thinking_mode: String,
    pub reasoning_effort: String,
    pub missing_reasoning_strategy: String,
    pub display_reasoning: bool,
    pub collapsible_reasoning: bool,
    /// Populated on GET when gateway recommends L0/L1 invalidation after display_reasoning change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_invalidate_recommended: Option<bool>,
    /// Read-only: from gateway startup config.
    #[serde(default)]
    pub storage_backend: String,
    #[serde(default)]
    pub cache_db_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_url_masked: Option<String>,
    /// Legacy dashboard field: derived from `storage_backend == "sqlite"` on GET.
    #[serde(default)]
    pub sqlite_cache_enabled: bool,
    /// Legacy dashboard field: mirrors `cache_db_path` when sqlite backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sqlite_cache_path: Option<String>,
    /// Legacy: maps to `missing_reasoning_strategy == "recover"` on GET; accepted on PUT for compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_recovery: Option<bool>,
}

impl ReasoningConfig {
    pub fn missing_reasoning_strategy_from_recovery(recovery: bool) -> String {
        if recovery {
            "recover".to_string()
        } else {
            "reject".to_string()
        }
    }

    pub fn thinking_mode_for_gateway(mode: &str) -> String {
        if mode == "auto" {
            "auto".to_string()
        } else {
            mode.to_string()
        }
    }

    /// Map gateway stored `enabled` back to dashboard `auto` when appropriate.
    pub fn thinking_mode_for_display(stored: &str, requested_auto: bool) -> String {
        if stored == "enabled" && requested_auto {
            "auto".to_string()
        } else {
            stored.to_string()
        }
    }
}
