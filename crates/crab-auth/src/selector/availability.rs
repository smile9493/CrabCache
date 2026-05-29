use super::AuthEntry;
use chrono::{DateTime, Utc};

/// Reasons why an auth is blocked
#[derive(Debug, Clone)]
pub enum BlockReason {
    /// Auth is permanently disabled
    Disabled,
    /// Auth is temporarily unavailable
    Unavailable,
    /// Per-model disabled
    ModelDisabled,
    /// Per-model unavailable with cooldown
    ModelCooldown { until: DateTime<Utc> },
    /// Quota exceeded with recovery time
    QuotaExceeded { until: DateTime<Utc> },
    /// Global cooldown
    GlobalCooldown { until: DateTime<Utc> },
}

/// Check if an auth is blocked for a specific model.
/// Returns `Some(BlockReason)` if blocked, `None` if available.
pub fn is_blocked_for_model(auth: &AuthEntry, model: &str) -> Option<BlockReason> {
    if auth.record.disabled {
        return Some(BlockReason::Disabled);
    }

    let model_state = auth
        .model_states
        .get(model)
        .or_else(|| auth.model_states.get(&canonical_model_key(model)));

    if let Some(state) = model_state {
        if state.disabled {
            return Some(BlockReason::ModelDisabled);
        }
        if state.unavailable
            && let Some(until) = state.next_retry_after
            && until > Utc::now()
        {
            return Some(BlockReason::ModelCooldown { until });
        }
        if state.quota_exceeded
            && let Some(until) = state.next_recover_at
            && until > Utc::now()
        {
            return Some(BlockReason::QuotaExceeded { until });
        }
    }

    if auth.unavailable
        && let Some(until) = auth.next_retry_after
        && until > Utc::now()
    {
        return Some(BlockReason::GlobalCooldown { until });
    }

    None
}

/// Strip thinking/reasoning suffixes from model names for canonical matching.
/// e.g. "claude-sonnet-4-thinking" -> "claude-sonnet-4"
pub fn canonical_model_key(model: &str) -> String {
    let lower = model.to_lowercase();
    for suffix in &["-thinking", "-max", "-none", "-extended"] {
        if let Some(base) = lower.strip_suffix(suffix) {
            return base.to_string();
        }
    }
    lower
}

/// Extract session ID from Claude Code `user_id` format.
/// e.g. `"user_abc123_account__session_550e8400-e29b-41d4-a716-446655440000"`
/// -> `"550e8400-e29b-41d4-a716-446655440000"`
pub fn extract_claude_session_id(user_id: &str) -> Option<String> {
    let re = regex::Regex::new(r"_session_([a-f0-9-]+)$").ok()?;
    re.captures(user_id)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Compute FNV-64a hash of message content for session fingerprinting.
pub fn fnv64a_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    let prime: u64 = 0x100000001b3; // FNV prime
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(prime);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::ModelState;
    use std::collections::HashMap;

    fn make_auth(id: &str, disabled: bool) -> AuthEntry {
        AuthEntry {
            record: crate::types::TokenRecord {
                id: id.to_string(),
                provider: crate::types::Provider::Claude,
                access_token: "tok".into(),
                refresh_token: None,
                id_token: None,
                expired_at: None,
                last_refresh: None,
                email: None,
                disabled,
                metadata: HashMap::new(),
                file_path: None,
            },
            model_states: HashMap::new(),
            next_retry_after: None,
            unavailable: false,
            priority: 0,
        }
    }

    #[test]
    fn test_disabled_auth_blocked() {
        let auth = make_auth("a1", true);
        assert!(matches!(
            is_blocked_for_model(&auth, "claude-sonnet-4"),
            Some(BlockReason::Disabled)
        ));
    }

    #[test]
    fn test_available_auth_not_blocked() {
        let auth = make_auth("a1", false);
        assert!(is_blocked_for_model(&auth, "claude-sonnet-4").is_none());
    }

    #[test]
    fn test_cooldown_blocks_until_expiry() {
        let mut auth = make_auth("a1", false);
        auth.unavailable = true;
        auth.next_retry_after = Some(Utc::now() + chrono::Duration::seconds(60));
        assert!(matches!(
            is_blocked_for_model(&auth, "claude-sonnet-4"),
            Some(BlockReason::GlobalCooldown { .. })
        ));
    }

    #[test]
    fn test_expired_cooldown_allows() {
        let mut auth = make_auth("a1", false);
        auth.unavailable = true;
        auth.next_retry_after = Some(Utc::now() - chrono::Duration::seconds(10));
        assert!(is_blocked_for_model(&auth, "claude-sonnet-4").is_none());
    }

    #[test]
    fn test_model_state_disabled() {
        let mut auth = make_auth("a1", false);
        auth.model_states.insert(
            "claude-sonnet-4".into(),
            ModelState {
                disabled: true,
                ..Default::default()
            },
        );
        assert!(matches!(
            is_blocked_for_model(&auth, "claude-sonnet-4"),
            Some(BlockReason::ModelDisabled)
        ));
    }

    #[test]
    fn test_model_state_cooldown() {
        let mut auth = make_auth("a1", false);
        auth.model_states.insert(
            "claude-sonnet-4".into(),
            ModelState {
                unavailable: true,
                next_retry_after: Some(Utc::now() + chrono::Duration::seconds(30)),
                ..Default::default()
            },
        );
        assert!(matches!(
            is_blocked_for_model(&auth, "claude-sonnet-4"),
            Some(BlockReason::ModelCooldown { .. })
        ));
    }

    #[test]
    fn test_quota_exceeded() {
        let mut auth = make_auth("a1", false);
        auth.model_states.insert(
            "claude-sonnet-4".into(),
            ModelState {
                quota_exceeded: true,
                next_recover_at: Some(Utc::now() + chrono::Duration::seconds(30)),
                ..Default::default()
            },
        );
        assert!(matches!(
            is_blocked_for_model(&auth, "claude-sonnet-4"),
            Some(BlockReason::QuotaExceeded { .. })
        ));
    }

    #[test]
    fn test_canonical_model_key_strips_suffix() {
        assert_eq!(canonical_model_key("claude-sonnet-4-thinking"), "claude-sonnet-4");
        assert_eq!(canonical_model_key("claude-sonnet-4-max"), "claude-sonnet-4");
        assert_eq!(canonical_model_key("gpt-4-none"), "gpt-4");
        assert_eq!(canonical_model_key("claude-sonnet-4"), "claude-sonnet-4");
        assert_eq!(canonical_model_key("CLAUDE-SONNET-4-THINKING"), "claude-sonnet-4");
    }

    #[test]
    fn test_canonical_model_key_in_model_state_lookup() {
        let mut auth = make_auth("a1", false);
        auth.model_states.insert(
            canonical_model_key("claude-sonnet-4-thinking"),
            ModelState {
                disabled: true,
                ..Default::default()
            },
        );
        assert!(is_blocked_for_model(&auth, "claude-sonnet-4-thinking").is_some());
    }

    #[test]
    fn test_extract_claude_session_id() {
        assert_eq!(
            extract_claude_session_id(
                "user_abc123_account__session_550e8400-e29b-41d4-a716-446655440000"
            ),
            Some("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
        assert_eq!(extract_claude_session_id("no_session_here"), None);
    }

    #[test]
    fn test_fnv64a_hash_deterministic() {
        let data = b"hello world";
        let h1 = fnv64a_hash(data);
        let h2 = fnv64a_hash(data);
        assert_eq!(h1, h2);
        assert_ne!(h1, 0);
    }
}
