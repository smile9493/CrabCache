//! 4-state circuit breaker (CLOSED/DEGRADED/OPEN/HALF_OPEN) ported from OmniRoute.
//!
//! Each backend has its own circuit breaker. The breaker tracks consecutive failures
//! and transitions through states to protect upstream backends from cascading failures.
//!
//! Key design decisions (from OmniRoute):
//! - 429 errors are NOT counted toward provider-level breakers (they are connection-scoped).
//! - DEGRADED state is a warning zone at 60% of failure_threshold.
//! - Adaptive backoff escalates timeout after repeated OPEN cycles.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Failure classification (from OmniRoute's classify429).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailureKind {
    RateLimit,
    QuotaExhausted,
    Transient,
}

/// Circuit breaker state (4-state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CircuitState {
    Closed,
    Degraded,
    Open,
    HalfOpen,
}

/// Configuration for a circuit breaker instance.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub degradation_threshold: u32,
    pub reset_timeout: Duration,
    pub half_open_requests: u32,
    pub max_backoff_multiplier: u32,
    pub backoff_escalation_count: u32,
    pub cooldown_by_kind: HashMap<FailureKind, Duration>,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 12,
            degradation_threshold: 7,
            reset_timeout: Duration::from_secs(30),
            half_open_requests: 1,
            max_backoff_multiplier: 4,
            backoff_escalation_count: 3,
            cooldown_by_kind: HashMap::new(),
        }
    }
}

/// The circuit breaker instance for a single backend.
pub struct CircuitBreaker {
    name: String,
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    open_cycle_count: u32,
    last_failure_time: Option<std::time::Instant>,
    last_failure_kind: Option<FailureKind>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(name: String, config: CircuitBreakerConfig) -> Self {
        Self {
            name,
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            open_cycle_count: 0,
            last_failure_time: None,
            last_failure_kind: None,
            config,
        }
    }

    /// Check if the breaker allows a request through.
    pub fn can_execute(&self) -> bool {
        match self.state {
            CircuitState::Closed | CircuitState::Degraded => true,
            CircuitState::Open => self.should_transition_to_half_open(),
            CircuitState::HalfOpen => true,
        }
    }

    /// Get the current state (reads last_failure_time for Open->HalfOpen check).
    pub fn state(&self) -> CircuitState {
        if self.state == CircuitState::Open && self.should_transition_to_half_open() {
            CircuitState::HalfOpen
        } else {
            self.state
        }
    }

    /// Record a success. Decrements failure count (gradual recovery for DEGRADED).
    pub fn on_success(&mut self) {
        self.success_count += 1;
        match self.state {
            CircuitState::Open => {
                info!(
                    breaker = %self.name,
                    "Circuit breaker: OPEN -> CLOSED (probe success)"
                );
                self.reset();
            }
            CircuitState::HalfOpen => {
                info!(
                    breaker = %self.name,
                    "Circuit breaker: HALF_OPEN -> CLOSED (probe success)"
                );
                self.reset();
            }
            CircuitState::Degraded => {
                self.failure_count = self.failure_count.saturating_sub(1);
                if self.failure_count <= self.config.degradation_threshold {
                    debug!(
                        breaker = %self.name,
                        failure_count = self.failure_count,
                        "Circuit breaker: DEGRADED -> CLOSED (gradual recovery)"
                    );
                    self.state = CircuitState::Closed;
                }
            }
            CircuitState::Closed => {
                self.failure_count = self.failure_count.saturating_sub(1);
            }
        }
    }

    /// Record a failure. Returns the new state for metrics.
    pub fn on_failure(&mut self, kind: FailureKind) -> CircuitState {
        self.failure_count += 1;
        self.last_failure_time = Some(std::time::Instant::now());
        self.last_failure_kind = Some(kind);

        // QuotaExhausted forces immediate open
        if kind == FailureKind::QuotaExhausted && self.failure_count >= 1 {
            self.transition_to_open();
            return self.state;
        }

        match self.state {
            CircuitState::HalfOpen => {
                self.transition_to_open();
            }
            CircuitState::Degraded => {
                if self.failure_count >= self.config.failure_threshold {
                    self.transition_to_open();
                }
            }
            CircuitState::Closed => {
                if self.failure_count >= self.config.failure_threshold {
                    self.transition_to_open();
                } else if self.failure_count >= self.config.degradation_threshold {
                    warn!(
                        breaker = %self.name,
                        failure_count = self.failure_count,
                        threshold = self.config.failure_threshold,
                        "Circuit breaker: CLOSED -> DEGRADED"
                    );
                    self.state = CircuitState::Degraded;
                }
            }
            CircuitState::Open => {}
        }

        self.state
    }

    /// Get effective cooldown duration (with adaptive backoff).
    pub fn effective_cooldown(&self) -> Duration {
        if let Some(kind) = self.last_failure_kind {
            if let Some(&kind_cooldown) = self.config.cooldown_by_kind.get(&kind) {
                return kind_cooldown;
            }
        }

        let base = self.config.reset_timeout;
        if self.open_cycle_count <= self.config.backoff_escalation_count {
            return base;
        }
        let escalation = self.open_cycle_count - self.config.backoff_escalation_count;
        let multiplier = 1u64 << escalation.min(6);
        let timeout = base * multiplier as u32;
        let max = base * self.config.max_backoff_multiplier;
        timeout.min(max)
    }

    /// Get remaining cooldown in ms (0 if can execute).
    pub fn retry_after_ms(&self) -> u64 {
        if self.can_execute() {
            return 0;
        }
        if let Some(last) = self.last_failure_time {
            let elapsed = last.elapsed();
            let cooldown = self.effective_cooldown();
            if elapsed >= cooldown {
                0
            } else {
                (cooldown - elapsed).as_millis() as u64
            }
        } else {
            self.effective_cooldown().as_millis() as u64
        }
    }

    /// Get a snapshot for metrics/reporting.
    pub fn snapshot(&self) -> CircuitBreakerSnapshot {
        CircuitBreakerSnapshot {
            name: self.name.clone(),
            state: self.state(),
            failure_count: self.failure_count,
            open_cycle_count: self.open_cycle_count,
            retry_after_ms: self.retry_after_ms(),
        }
    }

    fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.open_cycle_count = 0;
        self.last_failure_time = None;
        self.last_failure_kind = None;
    }

    fn transition_to_open(&mut self) {
        self.state = CircuitState::Open;
        self.open_cycle_count += 1;
        let cooldown = self.effective_cooldown();
        warn!(
            breaker = %self.name,
            failure_count = self.failure_count,
            open_cycle = self.open_cycle_count,
            cooldown_secs = cooldown.as_secs(),
            "Circuit breaker: -> OPEN"
        );
    }

    fn should_transition_to_half_open(&self) -> bool {
        if self.state != CircuitState::Open {
            return false;
        }
        if let Some(last) = self.last_failure_time {
            last.elapsed() >= self.effective_cooldown()
        } else {
            true
        }
    }
}

/// Read-only snapshot for metrics and management API.
#[derive(Debug, Clone, Serialize)]
pub struct CircuitBreakerSnapshot {
    pub name: String,
    pub state: CircuitState,
    pub failure_count: u32,
    pub open_cycle_count: u32,
    pub retry_after_ms: u64,
}

/// Thread-safe registry of circuit breakers keyed by backend name.
pub struct CircuitBreakerRegistry {
    breakers: DashMap<String, std::sync::Arc<tokio::sync::RwLock<CircuitBreaker>>>,
    default_config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    pub fn new(default_config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: DashMap::new(),
            default_config,
        }
    }

    /// Get or create a circuit breaker for a backend.
    pub fn get(&self, backend: &str) -> std::sync::Arc<tokio::sync::RwLock<CircuitBreaker>> {
        self.breakers
            .entry(backend.to_string())
            .or_insert_with(|| {
                std::sync::Arc::new(tokio::sync::RwLock::new(CircuitBreaker::new(
                    backend.to_string(),
                    self.default_config.clone(),
                )))
            })
            .clone()
    }

    /// Check if a backend can execute (fast path: read lock).
    pub async fn can_execute(&self, backend: &str) -> bool {
        if let Some(breaker) = self.breakers.get(backend) {
            breaker.read().await.can_execute()
        } else {
            true
        }
    }

    /// Record a failure for a backend.
    pub async fn on_failure(&self, backend: &str, kind: FailureKind) {
        let breaker = self.get(backend);
        let mut b = breaker.write().await;
        b.on_failure(kind);
    }

    /// Record a success for a backend.
    pub async fn on_success(&self, backend: &str) {
        let breaker = self.get(backend);
        let mut b = breaker.write().await;
        b.on_success();
    }

    /// Get snapshots of all breakers (for management API).
    pub async fn snapshots(&self) -> Vec<CircuitBreakerSnapshot> {
        let mut result = Vec::new();
        for entry in self.breakers.iter() {
            let snap = entry.value().read().await.snapshot();
            result.push(snap);
        }
        result
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

/// Classify an upstream error into a FailureKind.
pub fn classify_failure(status: u16, body: Option<&str>) -> FailureKind {
    match status {
        408 | 500 | 502 | 503 | 504 => FailureKind::Transient,
        429 => classify_429(body),
        _ => FailureKind::Transient,
    }
}

/// Classify a 429 error (from OmniRoute's classify429.ts).
///
/// Quota-exhausted patterns are checked first; generic "limit" and "exceed"
/// come last to avoid false positives on plain "rate limit exceeded" messages.
fn classify_429(body: Option<&str>) -> FailureKind {
    let body_lower = body.unwrap_or("").to_lowercase();
    // Check specific quota patterns first (order matters!)
    let specific_quota = [
        "daily",
        "monthly",
        "quota",
        "billing",
        "credit",
        "hard-limit",
        "insufficient",
        "payment",
        "usage limit",
        "exceeded daily",
        "exceeded monthly",
    ];
    for pattern in &specific_quota {
        if body_lower.contains(pattern) {
            return FailureKind::QuotaExhausted;
        }
    }
    FailureKind::RateLimit
}

/// Parse Retry-After header value into Duration.
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    if let Ok(secs) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new("test".into(), CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.can_execute());
    }

    #[test]
    fn circuit_breaker_transitions_to_degraded() {
        let config = CircuitBreakerConfig {
            failure_threshold: 10,
            degradation_threshold: 6,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".into(), config);
        for _ in 0..6 {
            cb.on_failure(FailureKind::Transient);
        }
        assert_eq!(cb.state(), CircuitState::Degraded);
        assert!(cb.can_execute());
    }

    #[test]
    fn circuit_breaker_transitions_to_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 5,
            degradation_threshold: 3,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".into(), config);
        for _ in 0..5 {
            cb.on_failure(FailureKind::Transient);
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.can_execute());
    }

    #[test]
    fn circuit_breaker_gradual_recovery() {
        let config = CircuitBreakerConfig {
            failure_threshold: 10,
            degradation_threshold: 6,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new("test".into(), config);
        for _ in 0..7 {
            cb.on_failure(FailureKind::Transient);
        }
        assert_eq!(cb.state(), CircuitState::Degraded);
        // Gradual recovery: each success decrements failure_count by 1.
        // When failure_count drops to <= degradation_threshold (6), state → Closed.
        cb.on_success();
        assert_eq!(cb.failure_count, 6);
        assert_eq!(cb.state(), CircuitState::Closed);
        // Further success keeps it Closed
        cb.on_success();
        assert_eq!(cb.failure_count, 5);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn quota_exhausted_immediate_open() {
        let mut cb = CircuitBreaker::new("test".into(), CircuitBreakerConfig::default());
        cb.on_failure(FailureKind::QuotaExhausted);
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn classify_429_quota_patterns() {
        assert_eq!(
            classify_429(Some("daily quota exceeded")),
            FailureKind::QuotaExhausted
        );
        assert_eq!(
            classify_429(Some("rate limit exceeded")),
            FailureKind::RateLimit
        );
        assert_eq!(classify_429(None), FailureKind::RateLimit);
    }
}
