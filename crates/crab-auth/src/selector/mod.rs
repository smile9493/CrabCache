pub mod availability;
pub mod fill_first;
pub mod round_robin;
pub mod session_affinity;
pub mod session_cache;

pub use availability::{
    BlockReason, canonical_model_key, extract_claude_session_id, fnv64a_hash, is_blocked_for_model,
};
pub use fill_first::FillFirstSelector;
pub use round_robin::RoundRobinSelector;
pub use session_affinity::SessionAffinitySelector;
pub use session_cache::SessionCache;

use crate::types::TokenRecord;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Runtime auth state (extends TokenRecord with runtime tracking)
#[derive(Debug, Clone)]
pub struct AuthEntry {
    /// The persisted token record
    pub record: TokenRecord,
    /// Per-model availability states
    pub model_states: HashMap<String, ModelState>,
    /// Global cooldown (applies when no model-specific state exists)
    pub next_retry_after: Option<DateTime<Utc>>,
    /// Whether the auth is transiently unavailable (vs permanently disabled)
    pub unavailable: bool,
    /// Priority (higher = preferred, default 0)
    pub priority: i32,
}

impl AuthEntry {
    pub fn from_record(record: TokenRecord) -> Self {
        Self {
            record,
            model_states: HashMap::new(),
            next_retry_after: None,
            unavailable: false,
            priority: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelState {
    pub disabled: bool,
    pub unavailable: bool,
    pub next_retry_after: Option<DateTime<Utc>>,
    pub quota_exceeded: bool,
    pub next_recover_at: Option<DateTime<Utc>>,
}

/// Context for a single request, used by selectors to make decisions
#[derive(Debug, Clone, Default)]
pub struct SelectionContext {
    /// Provider being requested
    pub provider: String,
    /// Model being requested
    pub model: String,
    /// Session ID for affinity (extracted from headers/body)
    pub session_id: Option<String>,
    /// Whether this is a WebSocket connection
    pub is_websocket: bool,
}

/// The core selector trait
#[async_trait]
pub trait Selector: Send + Sync {
    /// Pick the best available auth for the given request context.
    /// Returns None if all auths are blocked (caller should return 429).
    async fn pick(&self, ctx: &SelectionContext, auths: &[AuthEntry]) -> Option<usize>;

    /// Notify the selector that an auth produced a specific result.
    /// Used for cooldown tracking.
    fn mark_result(
        &self,
        auth_idx: usize,
        auth: &AuthEntry,
        success: bool,
        status_code: Option<u16>,
    );
}
