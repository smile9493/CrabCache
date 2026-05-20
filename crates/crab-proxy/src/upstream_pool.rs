//! Round-robin / least-inflight pool of DeepSeek upstream API keys.

use crab_metrics::global_metrics;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const REASONING_NAMESPACE_AUTH: &str = "gateway-upstream-pool";

#[derive(Debug, Clone)]
pub struct UpstreamKeySpec {
    pub id: String,
    pub secret: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpstreamKeyStatus {
    pub id: String,
    pub preview: String,
    pub enabled: bool,
    pub inflight: usize,
    pub cooldown_remaining_secs: u64,
}

struct UpstreamKeySlot {
    id: String,
    secret: Arc<str>,
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
            })
            .collect()
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

    /// Mark key rate-limited and try to acquire another (next request; same-request retry not supported by Pingora).
    pub fn rotate_after_rate_limit(pool: &Arc<Self>, key_id: &str) -> Option<UpstreamKeyGuard> {
        pool.report_rate_limited(key_id);
        global_metrics().record_upstream_key_retry("rate_limited_rotate");
        pool.acquire()
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
            }],
        );
        assert_eq!(merged.len(), 2);
        let merged2 = UpstreamKeyPool::merge_append(
            &merged,
            vec![UpstreamKeySpec {
                id: String::new(),
                secret: "sk-bbbbbbbbbbbb".into(),
                enabled: true,
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
}
