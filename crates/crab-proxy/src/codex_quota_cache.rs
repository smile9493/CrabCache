//! Codex quota cache — per-key WHAM data for quota-aware key selection.
//!
//! Mirrors OmniRoute `src/domain/quotaCache.ts` and `open-sse/services/codexQuotaFetcher.ts`:
//! - In-memory cache keyed by key_id (UpstreamKeySlot.id)
//! - Lazy fetch on first acquire; background refresh for active keys
//! - 429 marks exhausted; reset_at auto-clears when window passes

use crate::upstream_pool::UpstreamKeyPool;
use crab_control::codex_wham::CodexQuotaSnapshot;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

const EXHAUSTED_TTL_SECS: u64 = 300; // 5 minutes (OmniRoute EXHAUSTED_TTL_MS)
const DEFAULT_CACHE_TTL_SECS: u64 = 60;

/// Per-key quota entry.
#[derive(Debug, Clone)]
struct QuotaEntry {
    snapshot: CodexQuotaSnapshot,
    fetched_at: Instant,
    /// Whether this entry was marked exhausted by a 429 response (no quota data available).
    marked_exhausted: bool,
    /// When the entry was marked exhausted (for TTL-based expiry).
    exhausted_at: Option<Instant>,
}

impl QuotaEntry {
    fn is_stale(&self, ttl: Duration) -> bool {
        self.fetched_at.elapsed() > ttl
    }

    fn is_exhausted(&self, threshold_percent: f64) -> bool {
        if self.marked_exhausted {
            // Check TTL: if EXHAUSTED_TTL_SECS passed, auto-clear
            if let Some(exhausted_at) = self.exhausted_at {
                if exhausted_at.elapsed() > Duration::from_secs(EXHAUSTED_TTL_SECS) {
                    return false;
                }
            }
            return true;
        }
        self.snapshot.is_exhausted(threshold_percent)
    }

    /// Minimum remaining percent from snapshot, or 0 if exhausted by 429.
    fn remaining_percent(&self) -> f64 {
        if self.marked_exhausted {
            return 0.0;
        }
        self.snapshot.min_remaining_percent().unwrap_or(100.0)
    }
}

/// Codex quota cache — thread-safe, async-capable.
pub struct CodexQuotaCache {
    entries: DashMap<String, QuotaEntry>,
    http_client: reqwest::Client,
    /// Semaphore to limit concurrent WHAM fetches (avoid thundering herd).
    fetch_semaphore: Semaphore,
    /// Max concurrent fetches.
    max_concurrent_fetches: usize,
}

impl CodexQuotaCache {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
            fetch_semaphore: Semaphore::new(5),
            max_concurrent_fetches: 5,
        }
    }

    /// Check if a key is exhausted (cached or marked from 429).
    /// `threshold_percent` = minimum remaining % to stay eligible (default: 2.0).
    pub fn is_exhausted(&self, key_id: &str, threshold_percent: f64) -> bool {
        if let Some(entry) = self.entries.get(key_id) {
            entry.is_exhausted(threshold_percent)
        } else {
            false // Unknown = assume available (fail-open)
        }
    }

    /// Get minimum remaining percent for a key.
    /// Returns `None` if no data is cached (caller should proceed).
    pub fn headroom_percent(&self, key_id: &str) -> Option<f64> {
        self.entries.get(key_id).map(|e| e.remaining_percent())
    }

    /// Mark a key as exhausted from a 429 response (no quota data available).
    pub fn mark_exhausted_from_429(&self, key_id: &str, provider: &str) {
        let now = Instant::now();
        self.entries
            .entry(key_id.to_string())
            .and_modify(|entry| {
                entry.marked_exhausted = true;
                entry.exhausted_at = Some(now);
            })
            .or_insert_with(|| QuotaEntry {
                snapshot: CodexQuotaSnapshot {
                    session_used_percent: None,
                    weekly_used_percent: None,
                    session_reset_at: None,
                    weekly_reset_at: None,
                    limit_reached: true,
                },
                fetched_at: now,
                marked_exhausted: true,
                exhausted_at: Some(now),
            });
        tracing::debug!(
            key_id = %key_id,
            provider = %provider,
            "codex quota: marked exhausted from 429"
        );
    }

    /// Fetch and update quota for a key (lazy, with TTL check).
    /// Returns the snapshot if available.
    pub async fn fetch_and_update(
        &self,
        key_id: &str,
        base_url: &str,
        api_key: &str,
        account_id: &str,
    ) -> Option<CodexQuotaSnapshot> {
        // Check cache freshness
        if let Some(entry) = self.entries.get(key_id) {
            if !entry.is_stale(Duration::from_secs(DEFAULT_CACHE_TTL_SECS)) {
                // Return cached data
                if entry.marked_exhausted {
                    // Still exhausted within TTL
                    return Some(entry.snapshot.clone());
                }
                return Some(entry.snapshot.clone());
            }
        }

        // Acquire permit to limit concurrent fetches
        let _permit = self.fetch_semaphore.try_acquire().ok()?;

        // Fetch from WHAM
        let url = format!("{}/backend-api/wham/usage", base_url.trim_end_matches('/'));
        let resp = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("chatgpt-account-id", account_id)
            .header("Accept", "application/json")
            .header("originator", "codex_cli_rs")
            .header(
                "User-Agent",
                "codex_cli_rs/0.118.0 (Mac OS 26.3.1; arm64) iTerm.app/3.6.9",
            )
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        let body: serde_json::Value = resp.json().await.ok()?;
        let snapshot = CodexQuotaSnapshot::parse_wham_json(&body)?;

        // Update cache
        let now = Instant::now();
        self.entries.insert(
            key_id.to_string(),
            QuotaEntry {
                snapshot: snapshot.clone(),
                fetched_at: now,
                marked_exhausted: false,
                exhausted_at: None,
            },
        );

        Some(snapshot)
    }

    /// Refresh a key's quota data (for background refresh).
    pub async fn refresh_key(&self, key_id: &str, base_url: &str, api_key: &str, account_id: &str) {
        if let Some(entry) = self.entries.get(key_id) {
            if !entry.is_stale(Duration::from_secs(DEFAULT_CACHE_TTL_SECS)) {
                return; // Still fresh
            }
        }
        self.fetch_and_update(key_id, base_url, api_key, account_id)
            .await;
    }

    /// Clear a key's cache entry (e.g., when key is revoked).
    pub fn remove(&self, key_id: &str) {
        self.entries.remove(key_id);
    }

    /// Get cache stats for diagnostics.
    pub fn stats(&self) -> CacheStats {
        let total = self.entries.len();
        let exhausted = self.entries.iter().filter(|e| e.is_exhausted(2.0)).count();
        let stale = self
            .entries
            .iter()
            .filter(|e| e.is_stale(Duration::from_secs(DEFAULT_CACHE_TTL_SECS)))
            .count();
        CacheStats {
            total,
            exhausted,
            stale,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total: usize,
    pub exhausted: usize,
    pub stale: usize,
}

impl Default for CodexQuotaCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache_not_exhausted() {
        let cache = CodexQuotaCache::new();
        assert!(!cache.is_exhausted("key-1", 2.0));
    }

    #[test]
    fn mark_exhausted_from_429() {
        let cache = CodexQuotaCache::new();
        cache.mark_exhausted_from_429("key-1", "codex");
        assert!(cache.is_exhausted("key-1", 2.0));
    }

    #[test]
    fn headroom_percent_none_when_empty() {
        let cache = CodexQuotaCache::new();
        assert_eq!(cache.headroom_percent("key-1"), None);
    }

    #[test]
    fn stats_empty() {
        let cache = CodexQuotaCache::new();
        let stats = cache.stats();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.exhausted, 0);
        assert_eq!(stats.stale, 0);
    }
}
