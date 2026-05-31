//! Programmable fault injection for integration testing.
//!
//! Only compiled when `cfg(test)` or `feature = "fault-injection"` is active.
//! Provides atomic flags that can be toggled at runtime (via Management API in debug builds)
//! to simulate upstream failures, cache corruption, and coalescing leader failures.
//!
//! # Safety
//! All fields use atomic types for lock-free concurrent access. The `trigger_after_count`
//! mechanism allows precise timing control (e.g. "fail on the 3rd request").

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Runtime fault injection configuration.
///
/// Each flag is independent. When `should_trigger()` returns `true` AND a specific
/// flag is set, the corresponding fault is injected at the designated code path.
pub struct FaultInjection {
    /// Skip L1 Redis lookup (simulate Redis outage).
    pub redis_down: AtomicBool,
    /// Force upstream to return 429 (simulate rate limiting).
    pub force_upstream_429: AtomicBool,
    /// Inject artificial delay (ms) before upstream request.
    pub upstream_delay_ms: AtomicU32,
    /// Return corrupted/random data from L0 cache hits.
    pub corrupt_l0_cache: AtomicBool,
    /// Force coalescing leader to fail (simulate leader crash).
    pub force_coalesce_leader_fail: AtomicBool,
    /// Force upstream connection failure.
    pub force_connection_fail: AtomicBool,
    /// Trigger faults only after N requests have passed through.
    pub trigger_after_count: AtomicU32,
    /// Internal counter (incremented on each request).
    current_count: AtomicU32,
}

impl Default for FaultInjection {
    fn default() -> Self {
        Self {
            redis_down: AtomicBool::new(false),
            force_upstream_429: AtomicBool::new(false),
            upstream_delay_ms: AtomicU32::new(0),
            corrupt_l0_cache: AtomicBool::new(false),
            force_coalesce_leader_fail: AtomicBool::new(false),
            force_connection_fail: AtomicBool::new(false),
            trigger_after_count: AtomicU32::new(0),
            current_count: AtomicU32::new(0),
        }
    }
}

impl FaultInjection {
    /// Increment the request counter and return `true` if faults should activate.
    ///
    /// Returns `true` when:
    /// - `trigger_after_count` is 0 (always trigger), OR
    /// - the internal counter has reached `trigger_after_count`.
    pub fn should_trigger(&self) -> bool {
        let count = self.current_count.fetch_add(1, Ordering::Relaxed) + 1;
        let threshold = self.trigger_after_count.load(Ordering::Relaxed);
        threshold == 0 || count >= threshold
    }

    /// Reset all flags and the counter to defaults.
    pub fn reset(&self) {
        self.redis_down.store(false, Ordering::Relaxed);
        self.force_upstream_429.store(false, Ordering::Relaxed);
        self.upstream_delay_ms.store(0, Ordering::Relaxed);
        self.corrupt_l0_cache.store(false, Ordering::Relaxed);
        self.force_coalesce_leader_fail
            .store(false, Ordering::Relaxed);
        self.force_connection_fail.store(false, Ordering::Relaxed);
        self.trigger_after_count.store(0, Ordering::Relaxed);
        self.current_count.store(0, Ordering::Relaxed);
    }

    /// Snapshot current config for Management API serialization.
    pub fn snapshot(&self) -> FaultInjectionSnapshot {
        FaultInjectionSnapshot {
            redis_down: self.redis_down.load(Ordering::Relaxed),
            force_upstream_429: self.force_upstream_429.load(Ordering::Relaxed),
            upstream_delay_ms: self.upstream_delay_ms.load(Ordering::Relaxed),
            corrupt_l0_cache: self.corrupt_l0_cache.load(Ordering::Relaxed),
            force_coalesce_leader_fail: self
                .force_coalesce_leader_fail
                .load(Ordering::Relaxed),
            force_connection_fail: self.force_connection_fail.load(Ordering::Relaxed),
            trigger_after_count: self.trigger_after_count.load(Ordering::Relaxed),
            current_count: self.current_count.load(Ordering::Relaxed),
        }
    }
}

/// Serializable snapshot of fault injection state (for Management API).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FaultInjectionSnapshot {
    pub redis_down: bool,
    pub force_upstream_429: bool,
    pub upstream_delay_ms: u32,
    pub corrupt_l0_cache: bool,
    pub force_coalesce_leader_fail: bool,
    pub force_connection_fail: bool,
    pub trigger_after_count: u32,
    pub current_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_trigger_always_when_threshold_zero() {
        let fi = FaultInjection::default();
        assert!(fi.should_trigger());
        assert!(fi.should_trigger());
    }

    #[test]
    fn should_trigger_after_count() {
        let fi = FaultInjection::default();
        fi.trigger_after_count.store(3, Ordering::Relaxed);
        assert!(!fi.should_trigger()); // count=1
        assert!(!fi.should_trigger()); // count=2
        assert!(fi.should_trigger()); // count=3
        assert!(fi.should_trigger()); // count=4
    }

    #[test]
    fn reset_clears_everything() {
        let fi = FaultInjection::default();
        fi.redis_down.store(true, Ordering::Relaxed);
        fi.trigger_after_count.store(5, Ordering::Relaxed);
        fi.reset();
        assert!(!fi.redis_down.load(Ordering::Relaxed));
        assert_eq!(fi.trigger_after_count.load(Ordering::Relaxed), 0);
        assert_eq!(fi.current_count.load(Ordering::Relaxed), 0);
    }
}
