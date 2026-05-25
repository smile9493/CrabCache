//! Per-DeepSeek-`user_id` (project_id) in-flight concurrency soft limits.

use crab_metrics::global_metrics;
use crab_reasoning::parse_deepseek_v4_thinking_suffix;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeepSeekConcurrencyTier {
    Pro,
    Flash,
}

impl DeepSeekConcurrencyTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pro => "pro",
            Self::Flash => "flash",
        }
    }

    fn max_for(self, cfg: &DeepSeekUserConcurrencyConfig) -> u32 {
        match self {
            Self::Pro => cfg.v4_pro_per_user_id,
            Self::Flash => cfg.v4_flash_per_user_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekUserConcurrencyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_v4_pro_per_user_id")]
    pub v4_pro_per_user_id: u32,
    #[serde(default = "default_v4_flash_per_user_id")]
    pub v4_flash_per_user_id: u32,
}

fn default_v4_pro_per_user_id() -> u32 {
    500
}

fn default_v4_flash_per_user_id() -> u32 {
    2500
}

impl Default for DeepSeekUserConcurrencyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            v4_pro_per_user_id: default_v4_pro_per_user_id(),
            v4_flash_per_user_id: default_v4_flash_per_user_id(),
        }
    }
}

/// Classify upstream model into DeepSeek v4 concurrency tier (after `-max`/`-none` strip).
pub fn classify_deepseek_v4_tier(model: &str) -> Option<DeepSeekConcurrencyTier> {
    let base = parse_deepseek_v4_thinking_suffix(model).base_model;
    if !base.starts_with("deepseek-v4-") {
        return None;
    }
    if base.contains("flash") {
        Some(DeepSeekConcurrencyTier::Flash)
    } else {
        Some(DeepSeekConcurrencyTier::Pro)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepSeekUserIdLimitError {
    Exceeded,
}

struct TierSlot {
    inflight: AtomicUsize,
    max: AtomicU32,
}

pub struct UpstreamUserIdLimiter {
    enabled: AtomicBool,
    pro_max: AtomicU32,
    flash_max: AtomicU32,
    pro_total_inflight: AtomicUsize,
    flash_total_inflight: AtomicUsize,
    slots: DashMap<String, Arc<TierSlot>>,
}

pub struct UpstreamUserIdGuard {
    limiter: Arc<UpstreamUserIdLimiter>,
    slot_key: String,
    tier: DeepSeekConcurrencyTier,
    slot: Arc<TierSlot>,
}

impl Drop for UpstreamUserIdGuard {
    fn drop(&mut self) {
        self.limiter.release(&self.slot_key, self.tier, &self.slot);
    }
}

impl UpstreamUserIdLimiter {
    pub fn new(cfg: DeepSeekUserConcurrencyConfig) -> Arc<Self> {
        Arc::new(Self {
            enabled: AtomicBool::new(cfg.enabled),
            pro_max: AtomicU32::new(cfg.v4_pro_per_user_id),
            flash_max: AtomicU32::new(cfg.v4_flash_per_user_id),
            pro_total_inflight: AtomicUsize::new(0),
            flash_total_inflight: AtomicUsize::new(0),
            slots: DashMap::new(),
        })
    }

    pub fn configure(&self, cfg: &DeepSeekUserConcurrencyConfig) {
        self.enabled.store(cfg.enabled, Ordering::Relaxed);
        self.pro_max
            .store(cfg.v4_pro_per_user_id, Ordering::Relaxed);
        self.flash_max
            .store(cfg.v4_flash_per_user_id, Ordering::Relaxed);
    }

    fn slot_key(user_id: &str, tier: DeepSeekConcurrencyTier) -> String {
        format!("{}:{}", tier.as_str(), user_id)
    }

    fn max_for_tier(&self, tier: DeepSeekConcurrencyTier) -> u32 {
        match tier {
            DeepSeekConcurrencyTier::Pro => self.pro_max.load(Ordering::Relaxed),
            DeepSeekConcurrencyTier::Flash => self.flash_max.load(Ordering::Relaxed),
        }
    }

    fn sync_slot(&self, user_id: &str, tier: DeepSeekConcurrencyTier) -> Arc<TierSlot> {
        let key = Self::slot_key(user_id, tier);
        let max = self.max_for_tier(tier);
        match self.slots.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(e) => {
                e.get().max.store(max, Ordering::Relaxed);
                e.get().clone()
            }
            dashmap::mapref::entry::Entry::Vacant(e) => e
                .insert(Arc::new(TierSlot {
                    inflight: AtomicUsize::new(0),
                    max: AtomicU32::new(max),
                }))
                .clone(),
        }
    }

    pub fn try_acquire(
        self: &Arc<Self>,
        user_id: &str,
        tier: DeepSeekConcurrencyTier,
    ) -> Result<UpstreamUserIdGuard, DeepSeekUserIdLimitError> {
        if !self.enabled.load(Ordering::Relaxed) {
            return Ok(UpstreamUserIdGuard {
                limiter: Arc::clone(self),
                slot_key: String::new(),
                tier,
                slot: Arc::new(TierSlot {
                    inflight: AtomicUsize::new(0),
                    max: AtomicU32::new(0),
                }),
            });
        }

        let slot = self.sync_slot(user_id, tier);
        loop {
            let current = slot.inflight.load(Ordering::Relaxed);
            let max = slot.max.load(Ordering::Relaxed);
            if max > 0 && current >= max as usize {
                return Err(DeepSeekUserIdLimitError::Exceeded);
            }
            if slot
                .inflight
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                self.bump_tier_total(tier, 1);
                return Ok(UpstreamUserIdGuard {
                    limiter: Arc::clone(self),
                    slot_key: Self::slot_key(user_id, tier),
                    tier,
                    slot,
                });
            }
        }
    }

    fn bump_tier_total(&self, tier: DeepSeekConcurrencyTier, delta: i64) {
        let counter = match tier {
            DeepSeekConcurrencyTier::Pro => &self.pro_total_inflight,
            DeepSeekConcurrencyTier::Flash => &self.flash_total_inflight,
        };
        let new = if delta > 0 {
            counter.fetch_add(delta as usize, Ordering::Relaxed) + delta as usize
        } else {
            let prev = counter.fetch_sub((-delta) as usize, Ordering::Relaxed);
            prev.saturating_sub((-delta) as usize)
        };
        global_metrics().set_deepseek_user_id_inflight(tier.as_str(), new as i64);
    }

    fn release(&self, slot_key: &str, tier: DeepSeekConcurrencyTier, slot: &Arc<TierSlot>) {
        // Empty slot_key is a sentinel for disabled-mode no-op guards (dummy slot).
        if slot_key.is_empty() {
            return;
        }
        // Always decrement inflight regardless of `enabled` state to prevent leaks
        // when `configure()` disables the limiter while guards are still alive.
        let prev = slot.inflight.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(prev > 0, "double release for slot_key={slot_key}");
        if prev > 0 {
            self.bump_tier_total(tier, -1);
        }
        // Only clean up empty slots when enabled, and only if the slot in the map
        // is still the same Arc we're releasing (avoids TOCTOU clobber of fresh slots).
        if self.enabled.load(Ordering::Relaxed) && slot.inflight.load(Ordering::Relaxed) == 0 {
            if let Some(entry) = self.slots.get(slot_key) {
                if Arc::ptr_eq(entry.value(), slot) {
                    drop(entry); // release read guard before remove
                    self.slots.remove(slot_key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_acquire_fails_when_max_two() {
        let limiter = UpstreamUserIdLimiter::new(DeepSeekUserConcurrencyConfig {
            enabled: true,
            v4_pro_per_user_id: 2,
            v4_flash_per_user_id: 2,
        });
        let _g1 = limiter
            .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
            .unwrap();
        let _g2 = limiter
            .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
            .unwrap();
        assert!(matches!(
            limiter.try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro),
            Err(DeepSeekUserIdLimitError::Exceeded)
        ));
    }

    #[test]
    fn disabled_allows_unlimited() {
        let limiter = UpstreamUserIdLimiter::new(DeepSeekUserConcurrencyConfig {
            enabled: false,
            v4_pro_per_user_id: 1,
            v4_flash_per_user_id: 1,
        });
        for _ in 0..5 {
            let _g = limiter
                .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
                .unwrap();
        }
    }

    #[test]
    fn classify_flash_and_pro() {
        assert_eq!(
            classify_deepseek_v4_tier("deepseek-v4-flash"),
            Some(DeepSeekConcurrencyTier::Flash)
        );
        assert_eq!(
            classify_deepseek_v4_tier("deepseek-v4-pro"),
            Some(DeepSeekConcurrencyTier::Pro)
        );
        assert!(classify_deepseek_v4_tier("deepseek-chat").is_none());
    }

    #[test]
    fn classify_with_thinking_suffixes() {
        assert_eq!(
            classify_deepseek_v4_tier("deepseek-v4-pro-max"),
            Some(DeepSeekConcurrencyTier::Pro)
        );
        assert_eq!(
            classify_deepseek_v4_tier("deepseek-v4-flash-none"),
            Some(DeepSeekConcurrencyTier::Flash)
        );
    }

    #[test]
    fn release_allows_reacquire() {
        let limiter = UpstreamUserIdLimiter::new(DeepSeekUserConcurrencyConfig {
            enabled: true,
            v4_pro_per_user_id: 1,
            v4_flash_per_user_id: 1,
        });
        let g = limiter
            .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
            .unwrap();
        assert!(matches!(
            limiter.try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro),
            Err(DeepSeekUserIdLimitError::Exceeded)
        ));
        drop(g);
        // After release, slot is cleaned up and re-acquire succeeds.
        let _g2 = limiter
            .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
            .unwrap();
    }

    #[test]
    fn configure_hot_reloads_max() {
        let limiter = UpstreamUserIdLimiter::new(DeepSeekUserConcurrencyConfig {
            enabled: true,
            v4_pro_per_user_id: 1,
            v4_flash_per_user_id: 1,
        });
        let _g1 = limiter
            .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
            .unwrap();
        assert!(matches!(
            limiter.try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro),
            Err(DeepSeekUserIdLimitError::Exceeded)
        ));
        // Hot-reload to increase max.
        limiter.configure(&DeepSeekUserConcurrencyConfig {
            enabled: true,
            v4_pro_per_user_id: 3,
            v4_flash_per_user_id: 2500,
        });
        let _g2 = limiter
            .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
            .unwrap();
        let _g3 = limiter
            .try_acquire("tenant-a", DeepSeekConcurrencyTier::Pro)
            .unwrap();
    }
}
