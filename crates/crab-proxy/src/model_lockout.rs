//! Model-level lockout registry.
//!
//! Tracks per-(profile, backend, model) cooldowns for providers with per-model quotas.
//! When a model is locked out, upstream_peer skips it and tries the next backend.

use dashmap::DashMap;
use serde::Serialize;
use std::time::{Duration, Instant};
use tracing::warn;

/// Exponential backoff steps (from OmniRoute).
const LOCKOUT_BACKOFF_STEPS: [u64; 5] = [60, 120, 300, 600, 1200]; // seconds

#[derive(Debug, Clone)]
pub struct ModelLockoutEntry {
    pub reason: String,
    pub until: Instant,
    pub locked_at: Instant,
    pub failure_count: u32,
    pub reset_after: Duration,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelLockoutSnapshot {
    pub key: String,
    pub reason: String,
    pub remaining_ms: u64,
    pub failure_count: u32,
}

pub struct ModelLockoutRegistry {
    locks: DashMap<String, ModelLockoutEntry>,
    failure_states: DashMap<String, FailureState>,
}

struct FailureState {
    failure_count: u32,
    last_failure_at: Instant,
    reset_after: Duration,
}

impl ModelLockoutRegistry {
    pub fn new() -> Self {
        Self {
            locks: DashMap::new(),
            failure_states: DashMap::new(),
        }
    }

    /// Check if a model on a specific backend is locked out.
    pub fn is_locked(&self, profile: &str, backend: &str, model: &str) -> bool {
        let key = lockout_key(profile, backend, model);
        if let Some(entry) = self.locks.get(&key) {
            if entry.until > Instant::now() {
                return true;
            }
            drop(entry);
            self.locks.remove(&key);
        }
        false
    }

    /// Record a failure for a model and lock it out if threshold exceeded.
    pub fn record_failure(
        &self,
        profile: &str,
        backend: &str,
        model: &str,
        reason: &str,
        base_cooldown: Duration,
        max_backoff_level: usize,
    ) {
        let key = lockout_key(profile, backend, model);
        let now = Instant::now();

        let failure_count = {
            let mut state =
                self.failure_states
                    .entry(key.clone())
                    .or_insert_with(|| FailureState {
                        failure_count: 0,
                        last_failure_at: now,
                        reset_after: base_cooldown,
                    });

            // Reset count if window expired
            if now.duration_since(state.last_failure_at) > state.reset_after {
                state.failure_count = 0;
            }
            state.failure_count += 1;
            state.last_failure_at = now;
            state.failure_count
        };

        // Compute cooldown with exponential backoff
        let backoff_idx = (failure_count - 1).min(max_backoff_level as u32);
        let step_idx = (backoff_idx as usize).min(LOCKOUT_BACKOFF_STEPS.len() - 1);
        let cooldown_secs = LOCKOUT_BACKOFF_STEPS[step_idx];
        let cooldown = Duration::from_secs(cooldown_secs);

        let until = now + cooldown;

        // Don't override a longer lockout
        if let Some(entry) = self.locks.get_mut(&key) {
            if entry.until > until {
                return;
            }
        }

        self.locks.insert(
            key.clone(),
            ModelLockoutEntry {
                reason: reason.to_string(),
                until,
                locked_at: now,
                failure_count,
                reset_after: base_cooldown,
            },
        );

        warn!(
            profile,
            backend, model, reason, cooldown_secs, failure_count, "Model lockout: locked"
        );
    }

    /// Record a quota-exhausted lockout (locks until midnight UTC).
    pub fn lock_until_midnight(&self, profile: &str, backend: &str, model: &str, reason: &str) {
        let key = lockout_key(profile, backend, model);
        let now = Instant::now();
        let until = next_midnight_utc();

        self.locks.insert(
            key,
            ModelLockoutEntry {
                reason: reason.to_string(),
                until,
                locked_at: now,
                failure_count: 1,
                reset_after: Duration::from_secs(1800),
            },
        );

        warn!(
            profile,
            backend, model, reason, "Model lockout: locked until midnight"
        );
    }

    /// Clear lockout for a specific model.
    pub fn clear(&self, profile: &str, backend: &str, model: &str) {
        let key = lockout_key(profile, backend, model);
        self.locks.remove(&key);
        self.failure_states.remove(&key);
    }

    /// Get all currently locked models (for management API).
    pub fn snapshots(&self) -> Vec<ModelLockoutSnapshot> {
        let now = Instant::now();
        self.locks
            .iter()
            .filter(|entry| entry.value().until > now)
            .map(|entry| ModelLockoutSnapshot {
                key: entry.key().clone(),
                reason: entry.value().reason.clone(),
                remaining_ms: entry.value().until.duration_since(now).as_millis() as u64,
                failure_count: entry.value().failure_count,
            })
            .collect()
    }

    /// Cleanup expired entries (call periodically).
    pub fn cleanup(&self) {
        let now = Instant::now();
        self.locks.retain(|_, entry| entry.until > now);
        self.failure_states
            .retain(|_, state| now.duration_since(state.last_failure_at) <= state.reset_after * 2);
    }
}

impl Default for ModelLockoutRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn lockout_key(profile: &str, backend: &str, model: &str) -> String {
    format!("{profile}:{backend}:{model}")
}

/// Compute next midnight UTC as an Instant.
fn next_midnight_utc() -> Instant {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let seconds_in_day = 86400;
    let next_midnight = ((now / seconds_in_day) + 1) * seconds_in_day;
    let remaining = next_midnight - now;
    Instant::now() + Duration::from_secs(remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_not_locked_by_default() {
        let registry = ModelLockoutRegistry::new();
        assert!(!registry.is_locked("p", "b", "gpt-4o"));
    }

    #[test]
    fn model_locked_after_failure() {
        let registry = ModelLockoutRegistry::new();
        registry.record_failure("p", "b", "gpt-4o", "rate_limit", Duration::from_secs(60), 5);
        assert!(registry.is_locked("p", "b", "gpt-4o"));
    }

    #[test]
    fn lockout_key_format() {
        assert_eq!(lockout_key("p", "b", "m"), "p:b:m");
    }
}
