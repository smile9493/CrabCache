use crab_metrics::global_metrics;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn key(profile: &str, backend: &str) -> String {
    format!("{profile}\n{backend}")
}

/// Default scoring weights for multi-factor weighted routing.
pub const DEFAULT_SCORE_WEIGHTS: ScoreWeights = ScoreWeights {
    health: 0.30,
    latency_inv: 0.25,
    load_inv: 0.20,
    affinity_hit: 0.15,
    rate_429_inv: 0.10,
};

/// Configurable weights for the multi-factor backend scoring model.
#[derive(Debug, Clone, Copy)]
pub struct ScoreWeights {
    pub health: f64,
    pub latency_inv: f64,
    pub load_inv: f64,
    pub affinity_hit: f64,
    pub rate_429_inv: f64,
}

impl ScoreWeights {
    pub fn from_config(
        health: f64,
        latency_inv: f64,
        load_inv: f64,
        affinity_hit: f64,
        rate_429_inv: f64,
    ) -> Self {
        let total = health + latency_inv + load_inv + affinity_hit + rate_429_inv;
        if total <= 0.0 {
            return DEFAULT_SCORE_WEIGHTS;
        }
        Self {
            health: health / total,
            latency_inv: latency_inv / total,
            load_inv: load_inv / total,
            affinity_hit: affinity_hit / total,
            rate_429_inv: rate_429_inv / total,
        }
    }
}

impl Default for ScoreWeights {
    fn default() -> Self {
        DEFAULT_SCORE_WEIGHTS
    }
}

/// Breakdown of scoring factors for a single backend.
#[derive(Debug, Clone)]
pub struct ScoreFactors {
    pub health: f64,
    pub latency_inv: f64,
    pub load_inv: f64,
    pub affinity_hit: f64,
    pub rate_429_inv: f64,
}

/// Scored backend result returned by multi-factor selection.
#[derive(Debug, Clone)]
pub struct BackendHealthScore {
    pub name: String,
    pub score: f64,
    pub factors: ScoreFactors,
}

struct BackendSlot {
    profile: String,
    backend: String,
    max_inflight: usize,
    semaphore: Arc<Semaphore>,
    inflight: AtomicUsize,
    ewma_prefill_ms: AtomicU64,
    ewma_upstream_ms: AtomicU64,
    overloaded_until_ms: AtomicU64,
    failure_streak: AtomicU64,
    locked_until_ms: AtomicU64,
    /// 429-specific tracking (quota preflight).
    total_429s: AtomicU64,
    total_requests: AtomicU64,
    last_429_at_ms: AtomicU64,
    consecutive_429s: AtomicU64,
}

impl BackendSlot {
    fn new(profile: &str, backend: &str, max_inflight: usize) -> Self {
        Self {
            profile: profile.to_string(),
            backend: backend.to_string(),
            max_inflight,
            semaphore: Arc::new(Semaphore::new(max_inflight)),
            inflight: AtomicUsize::new(0),
            ewma_prefill_ms: AtomicU64::new(0),
            ewma_upstream_ms: AtomicU64::new(0),
            overloaded_until_ms: AtomicU64::new(0),
            failure_streak: AtomicU64::new(0),
            locked_until_ms: AtomicU64::new(0),
            total_429s: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            last_429_at_ms: AtomicU64::new(0),
            consecutive_429s: AtomicU64::new(0),
        }
    }

    fn observe_ewma(target: &AtomicU64, sample_ms: u64) {
        let mut current = target.load(Ordering::Relaxed);
        loop {
            let next = if current == 0 {
                sample_ms
            } else {
                // 20% new sample, 80% historical average.
                ((current.saturating_mul(4)).saturating_add(sample_ms)) / 5
            };
            match target.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }

    fn overload_state(&self, prefill_threshold_ms: u64) -> &'static str {
        if self.locked_until_ms.load(Ordering::Relaxed) > now_ms() {
            return "locked";
        }
        if self.overloaded_until_ms.load(Ordering::Relaxed) > now_ms() {
            return "cooldown";
        }
        if self.inflight.load(Ordering::Relaxed) >= self.max_inflight {
            return "inflight";
        }
        if prefill_threshold_ms > 0
            && self.ewma_prefill_ms.load(Ordering::Relaxed) >= prefill_threshold_ms
        {
            return "latency";
        }
        "ready"
    }
}

#[derive(Default)]
pub struct BackendLoadRegistry {
    slots: DashMap<String, Arc<BackendSlot>>,
}

impl BackendLoadRegistry {
    fn slot(&self, profile: &str, backend: &str, max_inflight: usize) -> Arc<BackendSlot> {
        let max_inflight = max_inflight.max(1);
        self.slots
            .entry(key(profile, backend))
            .or_insert_with(|| Arc::new(BackendSlot::new(profile, backend, max_inflight)))
            .clone()
    }

    pub fn try_acquire(
        &self,
        profile: &str,
        backend: &str,
        max_inflight: usize,
    ) -> Option<BackendPermit> {
        let slot = self.slot(profile, backend, max_inflight);
        let permit = slot.semaphore.clone().try_acquire_owned().ok()?;
        let inflight = slot.inflight.fetch_add(1, Ordering::Relaxed) + 1;
        global_metrics().set_backend_inflight(profile, backend, inflight as i64);
        Some(BackendPermit {
            _permit: permit,
            slot,
        })
    }

    pub fn inflight(&self, profile: &str, backend: &str) -> usize {
        self.slots
            .get(&key(profile, backend))
            .map(|s| s.inflight.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn overload_state(
        &self,
        profile: &str,
        backend: &str,
        max_inflight: usize,
        prefill_threshold_ms: u64,
    ) -> &'static str {
        let slot = self.slot(profile, backend, max_inflight);
        slot.overload_state(prefill_threshold_ms)
    }

    pub fn is_available(
        &self,
        profile: &str,
        backend: &str,
        max_inflight: usize,
        prefill_threshold_ms: u64,
    ) -> bool {
        self.overload_state(profile, backend, max_inflight, prefill_threshold_ms) == "ready"
    }

    pub fn observe_latency(
        &self,
        profile: &str,
        backend: &str,
        max_inflight: usize,
        prefill_ms: Option<f64>,
        upstream_ms: Option<f64>,
        status: Option<u16>,
        prefill_threshold_ms: u64,
        overload_cooldown_ms: u64,
    ) -> &'static str {
        let slot = self.slot(profile, backend, max_inflight);
        let status = status.unwrap_or(0);
        if let Some(ms) = prefill_ms.filter(|v| v.is_finite() && *v >= 0.0) {
            let sample = ms.round() as u64;
            BackendSlot::observe_ewma(&slot.ewma_prefill_ms, sample);
            if prefill_threshold_ms > 0 && sample >= prefill_threshold_ms {
                slot.overloaded_until_ms.store(
                    now_ms().saturating_add(overload_cooldown_ms),
                    Ordering::Relaxed,
                );
            }
        }
        if let Some(ms) = upstream_ms.filter(|v| v.is_finite() && *v >= 0.0) {
            BackendSlot::observe_ewma(&slot.ewma_upstream_ms, ms.round() as u64);
        }
        slot.total_requests.fetch_add(1, Ordering::Relaxed);
        if status == 429 {
            slot.total_429s.fetch_add(1, Ordering::Relaxed);
            slot.last_429_at_ms.store(now_ms(), Ordering::Relaxed);
            slot.consecutive_429s.fetch_add(1, Ordering::Relaxed);
        } else if status >= 200 && status < 300 {
            slot.consecutive_429s.store(0, Ordering::Relaxed);
        }
        if status == 0 || status >= 500 || status == 429 || status == 408 {
            let streak = slot.failure_streak.fetch_add(1, Ordering::Relaxed) + 1;
            if streak >= 3 {
                slot.locked_until_ms.store(
                    now_ms().saturating_add(overload_cooldown_ms.saturating_mul(2)),
                    Ordering::Relaxed,
                );
            }
        } else if (200..500).contains(&status) {
            slot.failure_streak.store(0, Ordering::Relaxed);
            slot.locked_until_ms.store(0, Ordering::Relaxed);
        }
        slot.overload_state(prefill_threshold_ms)
    }

    // ── Quota Preflight (P1-2) ───────────────────────────────────────

    /// Check whether a backend should be skipped due to recent 429 history.
    pub fn should_skip_backend(
        &self,
        profile: &str,
        backend: &str,
        max_inflight: usize,
        cooldown_ms: u64,
        max_consecutive_429: u32,
        skip_threshold: f64,
    ) -> bool {
        let slot = self.slot(profile, backend, max_inflight);
        let now = now_ms();
        let last_429 = slot.last_429_at_ms.load(Ordering::Relaxed);
        if last_429 > 0 && now.saturating_sub(last_429) < cooldown_ms {
            return true;
        }
        let consecutive = slot.consecutive_429s.load(Ordering::Relaxed);
        if consecutive >= max_consecutive_429 as u64 {
            return true;
        }
        let total = slot.total_requests.load(Ordering::Relaxed);
        let total_429 = slot.total_429s.load(Ordering::Relaxed);
        if total >= 5 && total_429 as f64 / total as f64 >= skip_threshold {
            return true;
        }
        false
    }

    /// Record a 429 response for quota preflight tracking.
    pub fn record_429(&self, profile: &str, backend: &str, max_inflight: usize) {
        let slot = self.slot(profile, backend, max_inflight);
        slot.total_requests.fetch_add(1, Ordering::Relaxed);
        slot.total_429s.fetch_add(1, Ordering::Relaxed);
        slot.last_429_at_ms.store(now_ms(), Ordering::Relaxed);
        slot.consecutive_429s.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a successful response (resets consecutive 429 counter).
    pub fn record_success(&self, profile: &str, backend: &str, max_inflight: usize) {
        let slot = self.slot(profile, backend, max_inflight);
        slot.total_requests.fetch_add(1, Ordering::Relaxed);
        slot.consecutive_429s.store(0, Ordering::Relaxed);
    }

    /// Compute a multi-factor score for a backend (higher = better).
    pub fn score_backend(
        &self,
        profile: &str,
        backend: &str,
        max_inflight: usize,
        prefill_threshold_ms: u64,
        weights: &ScoreWeights,
        is_healthy: bool,
        affinity_hit: f64,
    ) -> BackendHealthScore {
        let slot = self.slot(profile, backend, max_inflight);
        let health_score = if is_healthy { 1.0 } else { 0.0 };
        let ewma_upstream = slot.ewma_upstream_ms.load(Ordering::Relaxed);
        let latency_score = if ewma_upstream == 0 {
            0.5
        } else if prefill_threshold_ms > 0 {
            (1.0 - (ewma_upstream as f64 / (prefill_threshold_ms as f64 * 2.0))).max(0.0)
        } else {
            (1.0 - (ewma_upstream as f64 / 30_000.0)).max(0.0)
        };
        let inflight = slot.inflight.load(Ordering::Relaxed) as f64;
        let load_score = if slot.max_inflight == 0 {
            0.5
        } else {
            (1.0 - (inflight / slot.max_inflight as f64)).max(0.0)
        };
        let total = slot.total_requests.load(Ordering::Relaxed);
        let total_429 = slot.total_429s.load(Ordering::Relaxed);
        let rate_429_score = if total == 0 {
            1.0
        } else {
            (1.0 - (total_429 as f64 / total as f64)).max(0.0)
        };
        let factors = ScoreFactors {
            health: health_score,
            latency_inv: latency_score,
            load_inv: load_score,
            affinity_hit: affinity_hit.clamp(0.0, 1.0),
            rate_429_inv: rate_429_score,
        };
        let score = weights.health * health_score
            + weights.latency_inv * latency_score
            + weights.load_inv * load_score
            + weights.affinity_hit * affinity_hit.clamp(0.0, 1.0)
            + weights.rate_429_inv * rate_429_score;
        BackendHealthScore {
            name: slot.backend.clone(),
            score,
            factors,
        }
    }
}

pub struct BackendPermit {
    _permit: OwnedSemaphorePermit,
    slot: Arc<BackendSlot>,
}

impl Drop for BackendPermit {
    fn drop(&mut self) {
        let inflight = self
            .slot
            .inflight
            .fetch_sub(1, Ordering::Relaxed)
            .saturating_sub(1);
        global_metrics().set_backend_inflight(&self.slot.profile, &self.slot.backend, inflight as i64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semaphore_limits_backend_inflight() {
        let registry = BackendLoadRegistry::default();
        let first = registry.try_acquire("p", "b", 1);
        assert!(first.is_some());
        assert!(registry.try_acquire("p", "b", 1).is_none());
        drop(first);
        assert!(registry.try_acquire("p", "b", 1).is_some());
    }

    #[test]
    fn latency_observation_marks_backend_overloaded() {
        let registry = BackendLoadRegistry::default();
        let state = registry.observe_latency(
            "p",
            "b",
            10,
            Some(60_000.0),
            None,
            Some(200),
            30_000,
            1_000,
        );
        assert_eq!(state, "cooldown");
        assert!(!registry.is_available("p", "b", 10, 30_000));
    }

    #[test]
    fn repeated_failures_lock_backend() {
        let registry = BackendLoadRegistry::default();
        for _ in 0..3 {
            let _ = registry.observe_latency(
                "p",
                "b",
                10,
                None,
                None,
                Some(500),
                30_000,
                1_000,
            );
        }
        assert_eq!(
            registry.overload_state("p", "b", 10, 30_000),
            "locked"
        );
    }
}
