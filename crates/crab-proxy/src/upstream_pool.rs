//! Round-robin / least-inflight pool of DeepSeek upstream API keys.

use crab_metrics::global_metrics;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const REASONING_NAMESPACE_AUTH: &str = "gateway-upstream-pool";

/// Keys without an explicit `account_id` share this bucket (no cross-key rotation on 429).
pub const DEFAULT_UPSTREAM_ACCOUNT_ID: &str = "default";

#[derive(Debug, Clone)]
pub struct UpstreamKeySpec {
    pub id: String,
    pub secret: String,
    pub enabled: bool,
    pub account_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpstreamKeyStatus {
    pub id: String,
    pub preview: String,
    pub account_id: String,
    pub enabled: bool,
    pub inflight: usize,
    pub cooldown_remaining_secs: u64,
}

struct UpstreamKeySlot {
    id: String,
    secret: Arc<str>,
    account_id: Arc<str>,
    enabled: AtomicBool,
    inflight: AtomicUsize,
    cooldown_until_ms: AtomicU64,
}

pub struct UpstreamKeyPool {
    slots: Vec<UpstreamKeySlot>,
    rr: AtomicUsize,
    cooldown_secs: u64,
}

/// Holds an inflight slot until dropped.
pub struct UpstreamKeyGuard {
    pool: Arc<UpstreamKeyPool>,
    index: usize,
}

impl Drop for UpstreamKeyGuard {
    fn drop(&mut self) {
        self.pool.release(self.index);
    }
}

impl UpstreamKeyGuard {
    pub fn key_id(&self) -> &str {
        &self.pool.slots[self.index].id
    }

    pub fn bearer_secret(&self) -> &str {
        &self.pool.slots[self.index].secret
    }
}

/// Diagnoses why `acquire()` returned `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolAcquireFailure {
    /// No key slots at all (pool was initialized empty).
    Empty,
    /// All keys are explicitly disabled.
    AllDisabled,
    /// All keys are in cooldown (rate-limited); includes the minimum seconds until one recovers.
    AllInCooldown { min_retry_secs: u64 },
    /// Mix of disabled and in-cooldown keys (none available for any other reason).
    Unavailable,
}

pub fn normalize_account_id(raw: &str) -> Arc<str> {
    let t = raw.trim();
    if t.is_empty() {
        Arc::from(DEFAULT_UPSTREAM_ACCOUNT_ID)
    } else {
        Arc::from(t)
    }
}

pub fn key_preview(secret: &str) -> String {
    if secret.len() <= 12 {
        "***".to_string()
    } else {
        format!("{}...{}", &secret[..4], &secret[secret.len() - 4..])
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl UpstreamKeyPool {
    pub fn new(specs: Vec<UpstreamKeySpec>, cooldown_secs: u64) -> Arc<Self> {
        let slots: Vec<UpstreamKeySlot> = specs
            .into_iter()
            .enumerate()
            .map(|(i, spec)| {
                let id = if spec.id.is_empty() {
                    format!("key-{}", i + 1)
                } else {
                    spec.id
                };
                UpstreamKeySlot {
                    id,
                    secret: Arc::from(spec.secret.as_str()),
                    account_id: normalize_account_id(&spec.account_id),
                    enabled: AtomicBool::new(spec.enabled),
                    inflight: AtomicUsize::new(0),
                    cooldown_until_ms: AtomicU64::new(0),
                }
            })
            .collect();

        Arc::new(Self {
            slots,
            rr: AtomicUsize::new(0),
            cooldown_secs,
        })
    }

    pub fn from_secrets(secrets: Vec<String>, cooldown_secs: u64) -> Arc<Self> {
        let specs = secrets
            .into_iter()
            .enumerate()
            .map(|(i, secret)| UpstreamKeySpec {
                id: format!("key-{}", i + 1),
                secret,
                enabled: true,
                account_id: String::new(),
            })
            .collect();
        Self::new(specs, cooldown_secs)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn available_count(&self) -> usize {
        let now = now_ms();
        self.slots
            .iter()
            .filter(|s| {
                s.enabled.load(Ordering::Relaxed)
                    && s.cooldown_until_ms.load(Ordering::Relaxed) <= now
            })
            .count()
    }

    /// Diagnose why `acquire()` returns `None` without consuming a key.
    pub fn diagnose_acquire_failure(&self) -> PoolAcquireFailure {
        if self.slots.is_empty() {
            return PoolAcquireFailure::Empty;
        }
        let now = now_ms();
        let mut has_enabled = false;
        let mut min_cooldown_remaining = u64::MAX;
        let mut all_enabled_in_cooldown = true;

        for slot in &self.slots {
            let enabled = slot.enabled.load(Ordering::Relaxed);
            let cooldown_until = slot.cooldown_until_ms.load(Ordering::Relaxed);
            let in_cooldown = cooldown_until > now;

            if enabled {
                has_enabled = true;
            }
            if enabled && in_cooldown {
                let remaining = (cooldown_until - now + 999) / 1000;
                if remaining < min_cooldown_remaining {
                    min_cooldown_remaining = remaining;
                }
            }
            if enabled && !in_cooldown {
                all_enabled_in_cooldown = false;
            }
        }

        if !has_enabled {
            return PoolAcquireFailure::AllDisabled;
        }
        if all_enabled_in_cooldown {
            return PoolAcquireFailure::AllInCooldown {
                min_retry_secs: min_cooldown_remaining.min(3600),
            };
        }
        PoolAcquireFailure::Unavailable
    }

    pub fn list_status(&self) -> Vec<UpstreamKeyStatus> {
        let now = now_ms();
        self.slots
            .iter()
            .map(|s| {
                let cooldown_until = s.cooldown_until_ms.load(Ordering::Relaxed);
                let cooldown_remaining_secs = if cooldown_until > now {
                    (cooldown_until - now) / 1000
                } else {
                    0
                };
                UpstreamKeyStatus {
                    id: s.id.clone(),
                    preview: key_preview(&s.secret),
                    account_id: s.account_id.to_string(),
                    enabled: s.enabled.load(Ordering::Relaxed),
                    inflight: s.inflight.load(Ordering::Relaxed),
                    cooldown_remaining_secs,
                }
            })
            .collect()
    }

    pub fn to_specs(&self) -> Vec<UpstreamKeySpec> {
        self.slots
            .iter()
            .map(|s| UpstreamKeySpec {
                id: s.id.clone(),
                secret: s.secret.to_string(),
                enabled: s.enabled.load(Ordering::Relaxed),
                account_id: if s.account_id.as_ref() == DEFAULT_UPSTREAM_ACCOUNT_ID {
                    String::new()
                } else {
                    s.account_id.to_string()
                },
            })
            .collect()
    }

    /// Return the first enabled key's full secret for admin / sync usage.
    /// Returns `None` if no enabled key is available.
    pub fn admin_secret(&self) -> Option<String> {
        let now = now_ms();
        self.slots
            .iter()
            .find(|s| {
                s.enabled.load(Ordering::Relaxed)
                    && s.cooldown_until_ms.load(Ordering::Relaxed) <= now
            })
            .map(|s| s.secret.to_string())
    }

    /// Append keys by secret (dedupe); preserve existing slots.
    pub fn merge_append(pool: &Arc<Self>, incoming: Vec<UpstreamKeySpec>) -> Arc<Self> {
        let mut specs = pool.to_specs();
        let mut seen: std::collections::HashSet<String> =
            specs.iter().map(|s| s.secret.clone()).collect();
        for mut k in incoming {
            k.secret = k.secret.trim().to_string();
            if k.secret.is_empty() || seen.contains(&k.secret) {
                continue;
            }
            seen.insert(k.secret.clone());
            if k.id.is_empty() {
                k.id = format!("key-{}", specs.len() + 1);
            }
            specs.push(k);
        }
        Self::hot_replace(pool, specs)
    }

    /// Hot-replace the key pool, preserving inflight/cooldown for matching ids.
    pub fn hot_replace(pool: &Arc<Self>, specs: Vec<UpstreamKeySpec>) -> Arc<Self> {
        let old = pool;
        let new_slots: Vec<UpstreamKeySlot> = specs
            .into_iter()
            .enumerate()
            .map(|(i, spec)| {
                let id = if spec.id.is_empty() {
                    format!("key-{}", i + 1)
                } else {
                    spec.id
                };
                let mut inflight = 0usize;
                let mut cooldown_until_ms = 0u64;
                if let Some(prev) = old.slots.iter().find(|s| s.id == id) {
                    inflight = prev.inflight.load(Ordering::Relaxed);
                    cooldown_until_ms = prev.cooldown_until_ms.load(Ordering::Relaxed);
                }
                UpstreamKeySlot {
                    id,
                    secret: Arc::from(spec.secret.as_str()),
                    account_id: normalize_account_id(&spec.account_id),
                    enabled: AtomicBool::new(spec.enabled),
                    inflight: AtomicUsize::new(inflight),
                    cooldown_until_ms: AtomicU64::new(cooldown_until_ms),
                }
            })
            .collect();

        Arc::new(Self {
            slots: new_slots,
            rr: AtomicUsize::new(old.rr.load(Ordering::Relaxed)),
            cooldown_secs: old.cooldown_secs,
        })
    }

    pub fn acquire(self: &Arc<Self>) -> Option<UpstreamKeyGuard> {
        self.acquire_excluding_account(None)
    }

    /// Acquire a slot whose `account_id` differs from `excluded` (used after 429 on one account).
    pub fn acquire_excluding_account(
        self: &Arc<Self>,
        excluded: Option<&str>,
    ) -> Option<UpstreamKeyGuard> {
        if self.slots.is_empty() {
            return None;
        }

        let now = now_ms();
        let n = self.slots.len();
        let start = self.rr.fetch_add(1, Ordering::Relaxed) % n;

        let mut best_idx: Option<usize> = None;
        let mut best_inflight = usize::MAX;

        for offset in 0..n {
            let i = (start + offset) % n;
            let slot = &self.slots[i];
            if !slot.enabled.load(Ordering::Relaxed) {
                continue;
            }
            if slot.cooldown_until_ms.load(Ordering::Relaxed) > now {
                continue;
            }
            if let Some(ex) = excluded {
                if slot.account_id.as_ref() == ex {
                    continue;
                }
            }
            let inflight = slot.inflight.load(Ordering::Relaxed);
            if inflight < best_inflight {
                best_inflight = inflight;
                best_idx = Some(i);
            }
        }

        let idx = best_idx?;
        let inflight = self.slots[idx].inflight.fetch_add(1, Ordering::Relaxed) + 1;
        global_metrics().set_upstream_key_inflight(&self.slots[idx].id, inflight as i64);
        Some(UpstreamKeyGuard {
            pool: Arc::clone(self),
            index: idx,
        })
    }

    fn release(&self, index: usize) {
        if let Some(slot) = self.slots.get(index) {
            let prev = slot.inflight.fetch_sub(1, Ordering::Relaxed);
            let inflight = prev.saturating_sub(1);
            global_metrics().set_upstream_key_inflight(&slot.id, inflight as i64);
        }
    }

    /// Mark key rate-limited and try to acquire another from a **different** `account_id`.
    pub fn rotate_after_rate_limit(pool: &Arc<Self>, key_id: &str) -> Option<UpstreamKeyGuard> {
        pool.report_rate_limited(key_id);
        let excluded = pool
            .slots
            .iter()
            .find(|s| s.id == key_id)
            .map(|s| s.account_id.clone())?;
        global_metrics().record_upstream_key_retry("rate_limited_rotate");
        pool.acquire_excluding_account(Some(excluded.as_ref()))
    }

    pub fn report_rate_limited(&self, key_id: &str) {
        let until = now_ms() + self.cooldown_secs * 1000;
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            slot.cooldown_until_ms.store(until, Ordering::Relaxed);
        }
    }

    pub fn report_unauthorized(&self, key_id: &str) {
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            slot.enabled.store(false, Ordering::Relaxed);
        }
    }

    pub fn set_enabled(&self, key_id: &str, enabled: bool) -> bool {
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            slot.enabled.store(enabled, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_prefers_lower_inflight() {
        let pool = UpstreamKeyPool::from_secrets(
            vec!["sk-aaaaaaaaaaaa".into(), "sk-bbbbbbbbbbbb".into()],
            60,
        );
        let g1 = pool.acquire().unwrap();
        assert_eq!(g1.key_id(), "key-1");
        let g2 = pool.acquire().unwrap();
        assert_eq!(g2.key_id(), "key-2");
        drop(g1);
        let g3 = pool.acquire().unwrap();
        assert_eq!(g3.key_id(), "key-1");
    }

    #[test]
    fn cooldown_skips_key() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-onlykey123456".into()], 60);
        pool.report_rate_limited("key-1");
        assert!(pool.acquire().is_none());
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn empty_pool_returns_none() {
        let pool = UpstreamKeyPool::new(vec![], 60);
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn merge_append_dedupes_secrets() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60);
        let merged = UpstreamKeyPool::merge_append(
            &pool,
            vec![UpstreamKeySpec {
                id: String::new(),
                secret: "sk-bbbbbbbbbbbb".into(),
                enabled: true,
                account_id: String::new(),
            }],
        );
        assert_eq!(merged.len(), 2);
        let merged2 = UpstreamKeyPool::merge_append(
            &merged,
            vec![UpstreamKeySpec {
                id: String::new(),
                secret: "sk-bbbbbbbbbbbb".into(),
                enabled: true,
                account_id: String::new(),
            }],
        );
        assert_eq!(merged2.len(), 2);
    }

    #[test]
    fn bearer_secret_is_upstream_not_client_token() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-deepseek-upstream-secret".into()], 60);
        let guard = pool.acquire().expect("key");
        assert_eq!(guard.bearer_secret(), "sk-deepseek-upstream-secret");
        assert!(!guard.bearer_secret().starts_with("sk-cc-"));
    }

    #[test]
    fn rotate_skips_same_account_id() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec {
                    id: "key-a1".into(),
                    secret: "sk-aaaaaaaaaaaa".into(),
                    enabled: true,
                    account_id: "acct-a".into(),
                },
                UpstreamKeySpec {
                    id: "key-a2".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: "acct-a".into(),
                },
            ],
            60,
        );
        let _g = pool.acquire().unwrap();
        assert!(UpstreamKeyPool::rotate_after_rate_limit(&pool, "key-a1").is_none());
    }

    #[test]
    fn rotate_picks_different_account_id() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec {
                    id: "key-a".into(),
                    secret: "sk-aaaaaaaaaaaa".into(),
                    enabled: true,
                    account_id: "acct-a".into(),
                },
                UpstreamKeySpec {
                    id: "key-b".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: "acct-b".into(),
                },
            ],
            60,
        );
        let g1 = pool.acquire().unwrap();
        assert_eq!(g1.key_id(), "key-a");
        drop(g1);
        let g2 = UpstreamKeyPool::rotate_after_rate_limit(&pool, "key-a").unwrap();
        assert_eq!(g2.key_id(), "key-b");
    }

    #[test]
    fn rotate_none_when_all_default_account() {
        let pool = UpstreamKeyPool::from_secrets(
            vec!["sk-aaaaaaaaaaaa".into(), "sk-bbbbbbbbbbbb".into()],
            60,
        );
        let g1 = pool.acquire().unwrap();
        assert_eq!(g1.key_id(), "key-1");
        drop(g1);
        assert!(UpstreamKeyPool::rotate_after_rate_limit(&pool, "key-1").is_none());
    }

    #[test]
    fn diagnose_empty_pool() {
        let pool = UpstreamKeyPool::new(vec![], 60);
        assert_eq!(pool.diagnose_acquire_failure(), PoolAcquireFailure::Empty);
    }

    #[test]
    fn diagnose_all_disabled() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec {
                    id: "k1".into(),
                    secret: "sk-aaaaaaaaaaaa".into(),
                    enabled: false,
                    account_id: String::new(),
                },
                UpstreamKeySpec {
                    id: "k2".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: false,
                    account_id: String::new(),
                },
            ],
            60,
        );
        assert_eq!(
            pool.diagnose_acquire_failure(),
            PoolAcquireFailure::AllDisabled
        );
    }

    #[test]
    fn diagnose_all_in_cooldown() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 120);
        pool.report_rate_limited("key-1");
        let failure = pool.diagnose_acquire_failure();
        match failure {
            PoolAcquireFailure::AllInCooldown { min_retry_secs } => {
                assert!(min_retry_secs > 0 && min_retry_secs <= 120);
            }
            other => panic!("expected AllInCooldown, got {:?}", other),
        }
    }

    #[test]
    fn diagnose_mixed_disabled_and_cooldown() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec {
                    id: "k1".into(),
                    secret: "sk-aaaaaaaaaaaa".into(),
                    enabled: false,
                    account_id: String::new(),
                },
                UpstreamKeySpec {
                    id: "k2".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: String::new(),
                },
            ],
            60,
        );
        pool.report_rate_limited("k2");
        // One disabled, one in cooldown → AllInCooldown (all enabled keys are cooling down).
        match pool.diagnose_acquire_failure() {
            PoolAcquireFailure::AllInCooldown { min_retry_secs } => {
                assert!(min_retry_secs > 0 && min_retry_secs <= 60);
            }
            other => panic!("expected AllInCooldown, got {:?}", other),
        }
    }
}
