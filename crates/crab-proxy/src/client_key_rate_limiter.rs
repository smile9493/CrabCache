//! Per-client API key RPM rate limiting via token bucket.
//!
//! Uses two-tier locking to minimize contention:
//! - Outer `Mutex<HashMap<>>` for key lookup/insertion (held briefly)
//! - Per-key `Mutex<TokenBucket>` for token consumption (different keys never contend)

use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Token bucket state for a single client key.
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(rpm_limit: u32) -> Self {
        Self {
            tokens: rpm_limit as f64,
            last_refill: Instant::now(),
        }
    }

    /// Attempt to consume one token. Returns `true` if allowed, `false` if rate limited.
    fn try_consume(&mut self, rpm_limit: u32) -> bool {
        let rate = rpm_limit as f64 / 60.0;
        let max_tokens = rpm_limit as f64;

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        self.tokens = (self.tokens + elapsed * rate).min(max_tokens);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct ClientKeyRateLimiter {
    buckets: Mutex<std::collections::HashMap<String, Arc<Mutex<TokenBucket>>>>,
}

impl ClientKeyRateLimiter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buckets: Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Check and consume one request token for the given key.
    /// `rpm_limit`: max requests per minute (0 = unlimited).
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check_and_consume(&self, token: &str, rpm_limit: u32) -> bool {
        if rpm_limit == 0 {
            return true;
        }

        let mut buckets = match self.buckets.lock() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "ClientKeyRateLimiter outer lock poisoned; allowing request");
                return true;
            }
        };

        let bucket = buckets
            .entry(token.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(TokenBucket::new(rpm_limit))))
            .clone();
        drop(buckets);

        match bucket.lock() {
            Ok(mut b) => b.try_consume(rpm_limit),
            Err(e) => {
                tracing::warn!(error = %e, "ClientKeyRateLimiter inner lock poisoned; allowing request");
                true
            }
        }
    }

    /// Remove a key's bucket (called when a key is revoked).
    pub fn remove_key(&self, token: &str) {
        if let Ok(mut buckets) = self.buckets.lock() {
            buckets.remove(token);
        }
    }

    /// Periodically prune buckets that haven't been used recently.
    pub fn prune_stale(&self, max_age: Duration) {
        if let Ok(mut buckets) = self.buckets.lock() {
            let now = Instant::now();
            buckets.retain(|_, inner| {
                inner
                    .lock()
                    .map(|b| now.duration_since(b.last_refill) < max_age)
                    .unwrap_or(true)
            });
        }
    }
}
