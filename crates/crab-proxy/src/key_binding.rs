//! Conversation-level upstream key binding for MiMo pipeline.
//!
//! Once a conversation binds to a key, all subsequent requests from the same
//! conversation use that key unless the key is permanently rejected or transient
//! failures cross the configured failure threshold.
//!
//! Uses manual idle TTL checks in `get()` (moka `time_to_idle` requires a Tokio runtime at build).

use crate::context::GatewayContext;
use dashmap::DashMap;
use moka::notification::RemovalCause;
use moka::sync::Cache;
use std::sync::Arc;

/// Determine the stable session id for MiMo key binding.
///
/// Priority: `conversation_id` > `prompt_cache_key` > `session_fingerprint`.
/// Returns `None` when none are available, causing the caller to skip sticky
/// binding and use pool-wide round-robin instead.
///
/// **Does NOT use `client_key_fingerprint`** — that would collapse all
/// conversations sharing the same `sk-cc-*` into a single upstream key.
pub fn resolve_mimo_binding_key(ctx: &GatewayContext) -> Option<String> {
    if let Some(s) = ctx
        .conversation_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        return Some(format!("conv:{s}"));
    }
    if let Some(s) = ctx
        .prompt_cache_key
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        return Some(format!("pck:{s}"));
    }
    if let Some(s) = ctx
        .session_fingerprint
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        return Some(format!("sfp:{s}"));
    }
    None
}

/// Binding key prefix for structured logs / metrics.
pub fn mimo_binding_key_kind(bind_key: &str) -> &'static str {
    if bind_key.starts_with("conv:") {
        "conv"
    } else if bind_key.starts_with("pck:") {
        "pck"
    } else if bind_key.starts_with("sfp:") {
        "sfp"
    } else {
        "unknown"
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A binding from a stable session id to a specific upstream key.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub key_id: String,
    pub bound_at_ms: u64,
    pub last_used_ms: u64,
    pub transient_failures: u32,
}

/// Thread-safe store mapping stable session id -> upstream key id.
///
/// Maintains a reverse index (`key_sessions`) tracking how many sessions are
/// bound to each key, enabling multi-session-per-key binding (1:N).
pub struct KeyBindingStore {
    cache: Cache<String, KeyBinding>,
    ttl_ms: u64,
    /// Reverse index: key_id -> number of sessions currently bound to it.
    key_sessions: DashMap<String, u32>,
}

impl KeyBindingStore {
    pub fn new(ttl_secs: u64) -> Arc<Self> {
        let key_sessions: DashMap<String, u32> = DashMap::new();
        let key_sessions_for_listener = key_sessions.clone();
        let cache = Cache::builder()
            .max_capacity(50_000)
            .eviction_listener(move |_key: Arc<String>, value: KeyBinding, cause| {
                // Only handle `Size` eviction here — the one case NOT covered by
                // explicit `get()`/`remove()`/`put()` code paths which already call
                // `decrement_key_count` manually.
                // `Explicit`/`Expired` are handled in `get()` and `remove()`.
                // `Replaced` must be skipped (same-key TTL refresh, counts unchanged).
                if let RemovalCause::Size = cause {
                    if let Some(mut count) =
                        key_sessions_for_listener.get_mut(&value.key_id)
                    {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            drop(count);
                            key_sessions_for_listener.remove(&value.key_id);
                        }
                    }
                }
            })
            .build();
        Arc::new(Self {
            cache,
            ttl_ms: ttl_secs.saturating_mul(1000),
            key_sessions,
        })
    }

    /// Look up the bound key for a session. Returns `None` if no binding
    /// exists or if the binding has expired (idle TTL exceeded).
    pub fn get(&self, session_id: &str) -> Option<KeyBinding> {
        let binding = self.cache.get(session_id)?;
        if self.ttl_ms > 0 && now_ms().saturating_sub(binding.last_used_ms) > self.ttl_ms {
            self.cache.remove(session_id);
            self.decrement_key_count(&binding.key_id);
            return None;
        }
        Some(binding)
    }

    /// Create or update a binding: session_id -> key_id.
    ///
    /// If the session was previously bound to a different key, the old key's
    /// count is decremented before incrementing the new key's count.
    /// Re-putting the same key only refreshes the TTL without changing counts.
    pub fn put(&self, session_id: String, key_id: String) {
        let old = self.cache.get(&session_id);
        let is_new_binding = match old {
            Some(ref o) if o.key_id != key_id => {
                self.decrement_key_count(&o.key_id);
                true
            }
            Some(_) => false, // same key, TTL refresh only
            None => true,
        };
        let now = now_ms();
        self.cache.insert(
            session_id,
            KeyBinding {
                key_id: key_id.clone(),
                bound_at_ms: now,
                last_used_ms: now,
                transient_failures: 0,
            },
        );
        if is_new_binding {
            self.increment_key_count(&key_id);
        }
    }

    /// Refresh the last_used timestamp for an existing binding (resets idle timer).
    pub fn touch(&self, session_id: &str) {
        if let Some(mut binding) = self.cache.get(session_id) {
            binding.last_used_ms = now_ms();
            self.cache.insert(session_id.to_string(), binding);
        }
    }

    /// Count a transient upstream failure for the currently-bound key.
    /// Returns the updated failure count when the binding still points to `key_id`.
    pub fn record_failure(&self, session_id: &str, key_id: &str) -> Option<u32> {
        let mut binding = self.cache.get(session_id)?;
        if binding.key_id != key_id {
            return None;
        }
        binding.last_used_ms = now_ms();
        binding.transient_failures = binding.transient_failures.saturating_add(1);
        let count = binding.transient_failures;
        self.cache.insert(session_id.to_string(), binding);
        Some(count)
    }

    /// Clear transient failure strikes after a successful upstream response.
    pub fn reset_failures(&self, session_id: &str, key_id: &str) {
        if let Some(mut binding) = self.cache.get(session_id) {
            if binding.key_id == key_id {
                binding.last_used_ms = now_ms();
                binding.transient_failures = 0;
                self.cache.insert(session_id.to_string(), binding);
            }
        }
    }

    /// Remove a binding explicitly (e.g., after key 401 / permanent failure).
    /// Decrements the key session count for the removed binding's key.
    pub fn remove(&self, session_id: &str) {
        if let Some(old) = self.cache.get(session_id) {
            self.cache.remove(session_id);
            self.decrement_key_count(&old.key_id);
        }
    }

    /// Namespace prefix so Codex and MiMo bindings on the same conversation id do not collide.
    pub fn codex_session_key(session_id: &str) -> String {
        format!("codex:{session_id}")
    }

    /// Number of active bindings (for diagnostics).
    pub fn len(&self) -> u64 {
        self.cache.iter().count() as u64
    }

    /// Rebuild `key_sessions` from the live cache entries, correcting any drift
    /// caused by Moka capacity eviction or other edge cases.
    ///
    /// Call periodically (e.g., every 5 minutes via PruneService).
    pub fn reconcile_key_sessions(&self) {
        let mut actual: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for (_session, binding) in self.cache.iter() {
            *actual.entry(binding.key_id.clone()).or_insert(0) += 1;
        }
        // Replace all entries with ground truth.
        self.key_sessions.clear();
        for (kid, count) in actual {
            if count > 0 {
                self.key_sessions.insert(kid, count);
            }
        }
    }

    /// Return the effective idle TTL in seconds (0 means disabled).
    pub fn effective_ttl_secs(&self) -> u64 {
        self.ttl_ms / 1000
    }

    /// Return a diagnostic snapshot: (cache_entries, index_sum, is_consistent).
    ///
    /// `is_consistent` is true when the `key_sessions` reverse index sum matches
    /// the number of live cache entries (no drift from capacity eviction).
    pub fn diagnostic_snapshot(&self) -> (u64, u32, bool) {
        let cache_entries = self.cache.iter().count() as u64;
        let index_sum: u32 = self.key_sessions.iter().map(|r| *r.value()).sum();
        let consistent = index_sum as u64 == cache_entries;
        (cache_entries, index_sum, consistent)
    }

    /// Create a store with a custom max capacity (for testing eviction listener).
    #[cfg(test)]
    pub(crate) fn new_with_capacity(ttl_secs: u64, max_capacity: u64) -> Arc<Self> {
        let key_sessions: DashMap<String, u32> = DashMap::new();
        let key_sessions_for_listener = key_sessions.clone();
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .eviction_listener(move |_key: Arc<String>, value: KeyBinding, cause| {
                match cause {
                    RemovalCause::Size | RemovalCause::Explicit | RemovalCause::Expired => {
                        if let Some(mut count) =
                            key_sessions_for_listener.get_mut(&value.key_id)
                        {
                            *count = count.saturating_sub(1);
                            if *count == 0 {
                                drop(count);
                                key_sessions_for_listener.remove(&value.key_id);
                            }
                        }
                    }
                    RemovalCause::Replaced => {}
                }
            })
            .build();
        Arc::new(Self {
            cache,
            ttl_ms: ttl_secs.saturating_mul(1000),
            key_sessions,
        })
    }

    /// Return the number of sessions currently bound to `key_id`.
    pub fn count_sessions_for_key(&self, key_id: &str) -> u32 {
        self.key_sessions
            .get(key_id)
            .map(|r| *r)
            .unwrap_or(0)
    }

    /// Find the key in `available_keys` with the fewest bound sessions that is
    /// still under `max_sessions`. Returns `None` if all keys are at capacity.
    pub fn least_loaded_key(&self, available_keys: &[&str], max_sessions: u32) -> Option<String> {
        if max_sessions == 0 {
            return available_keys.first().map(|k| k.to_string());
        }
        available_keys
            .iter()
            .filter_map(|&kid| {
                let count = self.count_sessions_for_key(kid);
                if count < max_sessions {
                    Some((kid.to_string(), count))
                } else {
                    None
                }
            })
            .min_by_key(|(_, count)| *count)
            .map(|(kid, _)| kid)
    }

    fn increment_key_count(&self, key_id: &str) {
        self.key_sessions
            .entry(key_id.to_string())
            .and_modify(|c| *c = c.saturating_add(1))
            .or_insert(1);
    }

    fn decrement_key_count(&self, key_id: &str) {
        if let Some(mut count) = self.key_sessions.get_mut(key_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                drop(count);
                self.key_sessions.remove(key_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::GatewayContext;

    #[test]
    fn resolve_mimo_binding_key_prefers_conversation_id() {
        let mut ctx = GatewayContext::new("req".into());
        ctx.conversation_id = Some("c1".into());
        ctx.prompt_cache_key = Some("pck".into());
        ctx.session_fingerprint = Some("sfp".into());
        assert_eq!(
            resolve_mimo_binding_key(&ctx).as_deref(),
            Some("conv:c1")
        );
    }

    #[test]
    fn resolve_mimo_binding_key_uses_session_fingerprint() {
        let mut ctx = GatewayContext::new("req".into());
        ctx.session_fingerprint = Some("abc123".into());
        assert_eq!(
            resolve_mimo_binding_key(&ctx).as_deref(),
            Some("sfp:abc123")
        );
    }

    #[test]
    fn resolve_mimo_binding_key_none_without_conversation_signals() {
        let mut ctx = GatewayContext::new("req".into());
        ctx.client_key_fingerprint = Some("fe1fc33d2272a93f".into());
        assert!(resolve_mimo_binding_key(&ctx).is_none());
    }

    #[test]
    fn put_and_get() {
        let store = KeyBindingStore::new(300);
        store.put("session-1".into(), "key-a".into());
        let b = store.get("session-1").unwrap();
        assert_eq!(b.key_id, "key-a");
    }

    #[test]
    fn missing_session_returns_none() {
        let store = KeyBindingStore::new(300);
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn touch_refreshes_last_used() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        let before = store.get("s1").unwrap().last_used_ms;
        store.touch("s1");
        let after = store.get("s1").unwrap().last_used_ms;
        assert!(after >= before);
    }

    #[test]
    fn remove_clears_binding() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        store.remove("s1");
        assert!(store.get("s1").is_none());
    }

    #[test]
    fn key_session_count_tracks_bindings() {
        let store = KeyBindingStore::new(300);
        assert_eq!(store.count_sessions_for_key("k1"), 0);

        store.put("s1".into(), "k1".into());
        assert_eq!(store.count_sessions_for_key("k1"), 1);

        store.put("s2".into(), "k1".into());
        assert_eq!(store.count_sessions_for_key("k1"), 2);

        store.put("s3".into(), "k2".into());
        assert_eq!(store.count_sessions_for_key("k1"), 2);
        assert_eq!(store.count_sessions_for_key("k2"), 1);

        store.remove("s1");
        assert_eq!(store.count_sessions_for_key("k1"), 1);

        store.remove("s2");
        assert_eq!(store.count_sessions_for_key("k1"), 0);
    }

    #[test]
    fn rebind_decrements_old_key() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        assert_eq!(store.count_sessions_for_key("k1"), 1);
        assert_eq!(store.count_sessions_for_key("k2"), 0);

        // Re-bind s1 from k1 to k2
        store.put("s1".into(), "k2".into());
        assert_eq!(store.count_sessions_for_key("k1"), 0);
        assert_eq!(store.count_sessions_for_key("k2"), 1);
    }

    #[test]
    fn repeated_put_same_key_no_double_count() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        assert_eq!(store.count_sessions_for_key("k1"), 1);

        // Same key again: count should stay at 1
        store.put("s1".into(), "k1".into());
        assert_eq!(store.count_sessions_for_key("k1"), 1);

        // And a third time
        store.put("s1".into(), "k1".into());
        assert_eq!(store.count_sessions_for_key("k1"), 1);
    }

    #[test]
    fn least_loaded_key_picks_fewest() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        store.put("s2".into(), "k1".into());
        store.put("s3".into(), "k2".into());

        let keys = vec!["k1", "k2", "k3"];
        // k1=2, k2=1, k3=0 → should pick k3
        assert_eq!(store.least_loaded_key(&keys, 2), Some("k3".to_string()));
    }

    #[test]
    fn least_loaded_key_respects_max_sessions() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        store.put("s2".into(), "k1".into());
        store.put("s3".into(), "k2".into());

        let keys = vec!["k1", "k2"];
        // max_sessions=2: k1=2 (full), k2=1 → should pick k2
        assert_eq!(store.least_loaded_key(&keys, 2), Some("k2".to_string()));

        // max_sessions=1: k1=2 (full), k2=1 (full) → None
        assert_eq!(store.least_loaded_key(&keys, 1), None);
    }

    #[test]
    fn ttl_respects_config_value() {
        // Previously, non-zero TTL was forced to at least 86400 (24h).
        // After the fix, the configured value should be used directly.
        let store_300 = KeyBindingStore::new(300);
        assert_eq!(store_300.effective_ttl_secs(), 300);

        let store_60 = KeyBindingStore::new(60);
        assert_eq!(store_60.effective_ttl_secs(), 60);

        // ttl=0 means disabled (mapped to ttl_ms=0).
        let store_disabled = KeyBindingStore::new(0);
        assert_eq!(store_disabled.effective_ttl_secs(), 0);

        // ttl=86400 should stay at 86400 (not get doubled).
        let store_24h = KeyBindingStore::new(86400);
        assert_eq!(store_24h.effective_ttl_secs(), 86400);
    }

    #[test]
    fn reconcile_key_sessions_rebuilds_index() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        store.put("s2".into(), "k1".into());
        store.put("s3".into(), "k2".into());
        assert_eq!(store.count_sessions_for_key("k1"), 2);
        assert_eq!(store.count_sessions_for_key("k2"), 1);

        // Simulate drift: manually corrupt key_sessions to have wrong counts.
        store.key_sessions.insert("k1".into(), 999);
        store.key_sessions.insert("ghost_key".into(), 50);
        assert_eq!(store.count_sessions_for_key("k1"), 999);
        assert_eq!(store.count_sessions_for_key("ghost_key"), 50);

        // Reconcile should rebuild from cache ground truth.
        store.reconcile_key_sessions();
        assert_eq!(store.count_sessions_for_key("k1"), 2);
        assert_eq!(store.count_sessions_for_key("k2"), 1);
        assert_eq!(store.count_sessions_for_key("ghost_key"), 0);
    }

    #[test]
    fn reconcile_fixes_capacity_eviction_drift() {
        // Simulate the exact failure scenario: Moka evicts entries due to capacity,
        // but the eviction listener fires asynchronously (not inline during insert).
        // After capacity eviction, reconcile_key_sessions() must correct the drift.
        let store = KeyBindingStore::new_with_capacity(300, 5);

        // Insert 5 entries all bound to "k_full".
        for i in 0..5 {
            store.put(format!("s{i}"), "k_full".into());
        }
        assert_eq!(store.count_sessions_for_key("k_full"), 5);

        // Insert more entries with different keys. Moka's housekeeper may evict
        // old entries asynchronously, so key_sessions may or may not be updated
        // by the listener — but reconcile must always fix drift.
        for i in 5..20 {
            store.put(format!("s{i}"), "k_new".into());
        }

        // Simulate drift: corrupt key_sessions to pretend all 5 k_full entries
        // still exist (the exact scenario we're fixing).
        store.key_sessions.insert("k_full".into(), 5);

        // Reconcile: rebuild key_sessions from cache ground truth.
        store.reconcile_key_sessions();

        let total_in_index: u32 = store
            .key_sessions
            .iter()
            .map(|r| *r.value())
            .sum();
        let live_count = store.cache.iter().count() as u32;
        assert_eq!(
            total_in_index, live_count,
            "after reconcile, key_sessions index ({total_in_index}) must match live cache entries ({live_count})"
        );
    }
}
