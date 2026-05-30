//! Client-level lockout (brute-force login protection).
//!
//! Ported from OmniRoute's lockoutPolicy.ts. Tracks failed auth attempts
//! per client fingerprint and locks out after threshold exceeded.

use dashmap::DashMap;
use serde::Serialize;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tracing::warn;

pub struct ClientLockoutConfig {
    pub max_attempts: u32,
    pub lockout_duration: Duration,
    pub attempt_window: Duration,
}

impl Default for ClientLockoutConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            lockout_duration: Duration::from_secs(900),  // 15 minutes
            attempt_window: Duration::from_secs(300),     // 5 minutes
        }
    }
}

struct ClientLockoutState {
    attempts: VecDeque<Instant>,
    locked_until: Option<Instant>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientLockoutSnapshot {
    pub identifier: String,
    #[serde(skip)]
    pub locked_until: Option<Instant>,
    pub remaining_ms: u64,
    pub attempts: usize,
}

pub struct ClientLockoutRegistry {
    state: DashMap<String, ClientLockoutState>,
    config: ClientLockoutConfig,
}

impl ClientLockoutRegistry {
    pub fn new(config: ClientLockoutConfig) -> Self {
        Self {
            state: DashMap::new(),
            config,
        }
    }

    /// Check if a client is currently locked out.
    pub fn check_lockout(&self, identifier: &str) -> LockoutStatus {
        if let Some(mut entry) = self.state.get_mut(identifier) {
            let now = Instant::now();

            // Check if lockout has expired
            if let Some(until) = entry.locked_until {
                if now < until {
                    return LockoutStatus {
                        locked: true,
                        remaining_ms: until.duration_since(now).as_millis() as u64,
                        attempts: entry.attempts.len(),
                    };
                }
                // Clear expired lockout
                entry.locked_until = None;
                entry.attempts.clear();
            }

            // Count recent attempts within window
            let window_start = now - self.config.attempt_window;
            while entry.attempts.front().is_some_and(|&t| t < window_start) {
                entry.attempts.pop_front();
            }

            LockoutStatus {
                locked: false,
                remaining_ms: 0,
                attempts: entry.attempts.len(),
            }
        } else {
            LockoutStatus {
                locked: false,
                remaining_ms: 0,
                attempts: 0,
            }
        }
    }

    /// Record a failed auth attempt. Returns whether the client is now locked out.
    pub fn record_failed_attempt(&self, identifier: &str) -> LockoutStatus {
        let now = Instant::now();
        let mut entry = self
            .state
            .entry(identifier.to_string())
            .or_insert_with(|| ClientLockoutState {
                attempts: VecDeque::new(),
                locked_until: None,
            });

        // Clean old attempts
        let window_start = now - self.config.attempt_window;
        while entry.attempts.front().is_some_and(|&t| t < window_start) {
            entry.attempts.pop_front();
        }

        entry.attempts.push_back(now);

        // Check threshold
        if entry.attempts.len() as u32 >= self.config.max_attempts {
            let locked_until = now + self.config.lockout_duration;
            entry.locked_until = Some(locked_until);
            warn!(
                identifier,
                attempts = entry.attempts.len(),
                lockout_secs = self.config.lockout_duration.as_secs(),
                "Client lockout: too many failed attempts"
            );
            return LockoutStatus {
                locked: true,
                remaining_ms: self.config.lockout_duration.as_millis() as u64,
                attempts: entry.attempts.len(),
            };
        }

        LockoutStatus {
            locked: false,
            remaining_ms: 0,
            attempts: entry.attempts.len(),
        }
    }

    /// Record a successful auth — clears history.
    pub fn record_success(&self, identifier: &str) {
        self.state.remove(identifier);
    }

    /// Get all currently locked identifiers.
    pub fn snapshots(&self) -> Vec<ClientLockoutSnapshot> {
        let now = Instant::now();
        self.state
            .iter()
            .filter_map(|entry| {
                let locked_until = entry.value().locked_until?;
                if locked_until > now {
                    Some(ClientLockoutSnapshot {
                        identifier: entry.key().clone(),
                        remaining_ms: locked_until.duration_since(now).as_millis() as u64,
                        locked_until: Some(locked_until),
                        attempts: entry.value().attempts.len(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for ClientLockoutRegistry {
    fn default() -> Self {
        Self::new(ClientLockoutConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct LockoutStatus {
    pub locked: bool,
    pub remaining_ms: u64,
    pub attempts: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_locked_by_default() {
        let registry = ClientLockoutRegistry::default();
        let status = registry.check_lockout("test-client");
        assert!(!status.locked);
    }

    #[test]
    fn locked_after_max_attempts() {
        let config = ClientLockoutConfig {
            max_attempts: 3,
            lockout_duration: Duration::from_secs(60),
            attempt_window: Duration::from_secs(300),
        };
        let registry = ClientLockoutRegistry::new(config);
        registry.record_failed_attempt("test-client");
        registry.record_failed_attempt("test-client");
        let status = registry.record_failed_attempt("test-client");
        assert!(status.locked);
    }

    #[test]
    fn success_clears_state() {
        let config = ClientLockoutConfig {
            max_attempts: 3,
            lockout_duration: Duration::from_secs(60),
            attempt_window: Duration::from_secs(300),
        };
        let registry = ClientLockoutRegistry::new(config);
        registry.record_failed_attempt("test-client");
        registry.record_failed_attempt("test-client");
        registry.record_success("test-client");
        let status = registry.check_lockout("test-client");
        assert!(!status.locked);
    }
}
