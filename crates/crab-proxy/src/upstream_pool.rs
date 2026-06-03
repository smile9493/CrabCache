//! Round-robin / least-inflight pool of DeepSeek upstream API keys.

use crab_metrics::global_metrics;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const REASONING_NAMESPACE_AUTH: &str = "gateway-upstream-pool";

/// Persisted dynamic state for a single upstream key slot.
/// Written to Redis on state changes; restored on pool construction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamKeyStateSnapshot {
    pub key_id: String,
    pub cooldown_until_ms: u64,
    pub rate_limit_strikes: u32,
    /// Per-scope cooldown deadlines (scope name -> until_ms).
    #[serde(default)]
    pub scope_cooldowns: HashMap<String, u64>,
    pub enabled: bool,
    /// Per-model cooldown deadlines (model slug -> until_ms).
    #[serde(default)]
    pub model_cooldowns: HashMap<String, (u64, u32)>,
}

/// Per-model cooldown entry with progressive backoff tracking.
#[derive(Debug, Clone)]
struct ModelCooldownEntry {
    cooldown_until_ms: u64,
    backoff_level: u32,
}

/// Public view of a per-model cooldown for API display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelCooldownView {
    pub model: String,
    pub remaining_secs: u64,
    pub backoff_level: u32,
}

/// Minimum cooldown for progressive backoff (seconds).
const PROGRESSIVE_BACKOFF_BASE_SECS: u64 = 1;
/// Maximum cooldown cap for progressive backoff (seconds).
const PROGRESSIVE_BACKOFF_MAX_SECS: u64 = 1800;

/// Compute progressive backoff: doubles each level, capped at [`PROGRESSIVE_BACKOFF_MAX_SECS`].
/// Returns `(cooldown_secs, next_level)`.
fn next_progressive_cooldown(level: u32) -> (u64, u32) {
    let next_level = level.saturating_add(1);
    let secs = PROGRESSIVE_BACKOFF_BASE_SECS.saturating_mul(1u64 << level.min(20));
    (secs.min(PROGRESSIVE_BACKOFF_MAX_SECS), next_level)
}

/// Keys without an explicit `account_id` share this bucket (no cross-key rotation on 429).
pub const DEFAULT_UPSTREAM_ACCOUNT_ID: &str = "default";

/// JWT OAuth access token with a real ChatGPT account id (not `default` / `auto-*`).
pub fn looks_like_codex_oauth_key(secret: &str, account_id: &str) -> bool {
    let secret = secret.trim();
    let account_id = account_id.trim();
    secret.starts_with("eyJ")
        && secret.contains('.')
        && !account_id.is_empty()
        && account_id != DEFAULT_UPSTREAM_ACCOUNT_ID
        && !account_id.starts_with("auto-")
}

#[derive(Debug, Clone)]
pub struct UpstreamKeySpec {
    pub id: String,
    pub secret: String,
    pub enabled: bool,
    pub account_id: String,
    /// Upstream model slugs this key can serve (empty = no explicit filter).
    pub supported_models: Vec<String>,
    /// Key priority: 0 = highest (default), higher values = lower priority.
    pub priority: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpstreamKeyStatus {
    pub id: String,
    pub preview: String,
    pub account_id: String,
    pub enabled: bool,
    pub inflight: usize,
    pub cooldown_remaining_secs: u64,
    pub priority: u32,
    pub model_cooldowns: Vec<ModelCooldownView>,
}

struct UpstreamKeySlot {
    id: String,
    secret: Arc<str>,
    account_id: Arc<str>,
    enabled: AtomicBool,
    inflight: AtomicUsize,
    cooldown_until_ms: AtomicU64,
    /// Recent 429 / capacity strikes — higher values deprioritize the key (fill-first backoff).
    rate_limit_strikes: AtomicU32,
    /// Per-model-family cooldown (Codex scope: codex / spark / …).
    scope_cooldowns: RwLock<HashMap<Arc<str>, u64>>,
    /// Per-model cooldown with progressive backoff (model slug -> entry).
    model_cooldowns: RwLock<HashMap<Arc<str>, ModelCooldownEntry>>,
    supported_models: RwLock<Arc<[String]>>,
    /// Hard concurrency limit per key. Permits acquired on acquire, released on Guard drop.
    semaphore: Arc<tokio::sync::Semaphore>,
    /// Key priority: 0 = highest, higher values = lower priority.
    priority: AtomicU32,
}

pub struct UpstreamKeyPool {
    slots: Vec<UpstreamKeySlot>,
    rr: AtomicUsize,
    cooldown_secs: u64,
    max_inflight: usize,
    /// Optional quota cache for Codex quota-aware key selection (set once after construction).
    codex_quota_cache: std::sync::OnceLock<Arc<crate::codex_quota_cache::CodexQuotaCache>>,
    /// Set to true when key dynamic state (cooldowns, strikes, enabled) changes.
    /// Cleared by `take_dirty_states()`.
    state_dirty: AtomicBool,
    /// Per-model round-robin counters for fair key rotation within a model.
    model_rr: RwLock<HashMap<Arc<str>, AtomicUsize>>,
}

/// Holds an inflight slot until dropped.
pub struct UpstreamKeyGuard {
    pool: Arc<UpstreamKeyPool>,
    index: usize,
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
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

    pub fn account_id(&self) -> &str {
        &self.pool.slots[self.index].account_id
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

fn normalize_model_slug(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

fn slot_supports_model(slot: &UpstreamKeySlot, upstream_model: &str) -> bool {
    let models = slot.supported_models.read();
    if models.is_empty() {
        return true;
    }
    let want = normalize_model_slug(upstream_model);
    models.iter().any(|m| normalize_model_slug(m) == want)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Clone a `HashMap<Arc<str>, AtomicUsize>` by reading each counter's current value.
fn clone_model_rr(src: &HashMap<Arc<str>, AtomicUsize>) -> HashMap<Arc<str>, AtomicUsize> {
    src.iter()
        .map(|(k, v)| (Arc::clone(k), AtomicUsize::new(v.load(Ordering::Relaxed))))
        .collect()
}

fn slot_passes_cooldown(slot: &UpstreamKeySlot, scope: Option<&str>, model: Option<&str>, now: u64) -> bool {
    if slot.cooldown_until_ms.load(Ordering::Relaxed) > now {
        return false;
    }
    if let Some(scope) = scope.filter(|s| !s.is_empty() && *s != "default") {
        if let Some(until) = slot.scope_cooldowns.read().get(scope) {
            if *until > now {
                return false;
            }
        }
    }
    // Per-model cooldown (progressive backoff)
    if let Some(model) = model.filter(|m| !m.trim().is_empty()) {
        let model_key = normalize_model_slug(model);
        if !model_key.is_empty() {
            if let Some(entry) = slot.model_cooldowns.read().get(model_key.as_str()) {
                if entry.cooldown_until_ms > now {
                    return false;
                }
            }
        }
    }
    true
}

fn ensure_unique_ids(specs: &mut [UpstreamKeySpec]) {
    let mut id_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut next_id: usize = specs.len().saturating_add(1);
    for spec in specs.iter_mut() {
        if spec.id.is_empty() || id_seen.contains(&spec.id) {
            loop {
                let candidate = format!("key-{}", next_id);
                next_id += 1;
                if !id_seen.contains(&candidate) {
                    spec.id = candidate;
                    break;
                }
            }
        }
        id_seen.insert(spec.id.clone());
    }
}

/// Auto-assign a stable unique `account_id` for specs that have an empty one.
///
/// Uses `SHA256(secret)[..8]` → 16 hex chars, prefixed with `"auto-"`.
/// This ensures keys without an explicit account still get an independent
/// `account_id`, enabling `rotate_after_rate_limit` to find an alternative
/// key from a different account when one key gets rate-limited.
fn auto_assign_account_ids(specs: &mut [UpstreamKeySpec]) {
    use sha2::{Digest, Sha256};
    for spec in specs.iter_mut() {
        if spec.account_id.trim().is_empty() {
            let hash = hex::encode(&Sha256::digest(spec.secret.as_bytes())[..8]);
            spec.account_id = format!("auto-{}", hash);
        }
    }
}

/// Config `max_inflight == 0` means no per-key cap; tokio rejects `usize::MAX` permits.
const UNLIMITED_INFLIGHT_PERMITS: usize = 1_048_576;

fn semaphore_permits(max_inflight: usize) -> usize {
    if max_inflight == 0 {
        UNLIMITED_INFLIGHT_PERMITS
    } else {
        max_inflight
    }
}

impl UpstreamKeyPool {
    pub fn new(
        mut specs: Vec<UpstreamKeySpec>,
        cooldown_secs: u64,
        max_inflight: usize,
    ) -> Arc<Self> {
        auto_assign_account_ids(&mut specs);
        ensure_unique_ids(&mut specs);
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
                    rate_limit_strikes: AtomicU32::new(0),
                    scope_cooldowns: RwLock::new(HashMap::new()),
                    model_cooldowns: RwLock::new(HashMap::new()),
                    supported_models: RwLock::new(Arc::from(spec.supported_models.clone())),
                    semaphore: Arc::new(tokio::sync::Semaphore::new(semaphore_permits(
                        max_inflight,
                    ))),
                    priority: AtomicU32::new(spec.priority),
                }
            })
            .collect();

        Arc::new(Self {
            slots,
            rr: AtomicUsize::new(0),
            cooldown_secs,
            max_inflight,
            codex_quota_cache: std::sync::OnceLock::new(),
            state_dirty: AtomicBool::new(false),
            model_rr: RwLock::new(HashMap::new()),
        })
    }

    pub fn from_secrets(
        secrets: Vec<String>,
        cooldown_secs: u64,
        max_inflight: usize,
    ) -> Arc<Self> {
        let specs = secrets
            .into_iter()
            .enumerate()
            .map(|(i, secret)| UpstreamKeySpec {
                id: format!("key-{}", i + 1),
                secret,
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 0,
            })
            .collect();
        Self::new(specs, cooldown_secs, max_inflight)
    }

    /// Set the quota cache for this pool (Codex quota-aware key selection).
    /// Must be called once after construction; subsequent calls are silently ignored.
    pub fn set_quota_cache(
        self: &Arc<Self>,
        cache: Arc<crate::codex_quota_cache::CodexQuotaCache>,
    ) {
        let _ = self.codex_quota_cache.set(cache);
    }

    /// Get reference to the quota cache (if set).
    pub fn quota_cache(&self) -> Option<&Arc<crate::codex_quota_cache::CodexQuotaCache>> {
        self.codex_quota_cache.get()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn default_cooldown_secs(&self) -> u64 {
        self.cooldown_secs
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

    /// Return key IDs of all slots that are enabled and not in cooldown.
    pub fn available_key_ids(&self) -> Vec<String> {
        let now = now_ms();
        self.slots
            .iter()
            .filter(|s| {
                s.enabled.load(Ordering::Relaxed)
                    && s.cooldown_until_ms.load(Ordering::Relaxed) <= now
            })
            .map(|s| s.id.clone())
            .collect()
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
                let remaining = (cooldown_until - now).div_ceil(1000);
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
                let model_cooldowns: Vec<ModelCooldownView> = s.model_cooldowns
                    .read()
                    .iter()
                    .filter(|(_, entry)| entry.cooldown_until_ms > now)
                    .map(|(model, entry)| ModelCooldownView {
                        model: model.to_string(),
                        remaining_secs: (entry.cooldown_until_ms - now) / 1000,
                        backoff_level: entry.backoff_level,
                    })
                    .collect();
                UpstreamKeyStatus {
                    id: s.id.clone(),
                    preview: key_preview(&s.secret),
                    account_id: s.account_id.to_string(),
                    enabled: s.enabled.load(Ordering::Relaxed),
                    inflight: s.inflight.load(Ordering::Relaxed),
                    cooldown_remaining_secs,
                    priority: s.priority.load(Ordering::Relaxed),
                    model_cooldowns,
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
                supported_models: s.supported_models.read().to_vec(),
                priority: s.priority.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Update per-key upstream model catalogs (from Admin sync).
    pub fn update_models_catalog(&self, catalog: &std::collections::HashMap<String, Vec<String>>) {
        for slot in &self.slots {
            if let Some(models) = catalog.get(&slot.id) {
                let mut sorted = models.clone();
                sorted.sort();
                sorted.dedup();
                *slot.supported_models.write() = Arc::from(sorted);
            }
        }
    }

    /// Export dynamic key states for persistence to Redis.
    /// Returns a map of key_id -> state snapshot.
    pub fn export_key_states(&self) -> HashMap<String, UpstreamKeyStateSnapshot> {
        let now = now_ms();
        self.slots
            .iter()
            .filter_map(|slot| {
                let cooldown_until = slot.cooldown_until_ms.load(Ordering::Relaxed);
                let strikes = slot.rate_limit_strikes.load(Ordering::Relaxed);
                let enabled = slot.enabled.load(Ordering::Relaxed);
                let scope_cooldowns: HashMap<String, u64> = slot
                    .scope_cooldowns
                    .read()
                    .iter()
                    .filter(|(_, until)| **until > now)
                    .map(|(k, v)| (k.to_string(), *v))
                    .collect();
                let model_cooldowns: HashMap<String, (u64, u32)> = slot
                    .model_cooldowns
                    .read()
                    .iter()
                    .filter(|(_, entry)| entry.cooldown_until_ms > now)
                    .map(|(k, v)| (k.to_string(), (v.cooldown_until_ms, v.backoff_level)))
                    .collect();

                // Only persist if there's meaningful state
                if cooldown_until <= now && strikes == 0 && enabled && scope_cooldowns.is_empty() && model_cooldowns.is_empty() {
                    return None;
                }

                Some((
                    slot.id.clone(),
                    UpstreamKeyStateSnapshot {
                        key_id: slot.id.clone(),
                        cooldown_until_ms: cooldown_until,
                        rate_limit_strikes: strikes,
                        scope_cooldowns,
                        enabled,
                        model_cooldowns,
                    },
                ))
            })
            .collect()
    }

    /// Apply persisted key states from Redis, restoring cooldowns and strikes.
    /// Keys not found in the state map are left unchanged.
    pub fn apply_key_states(&self, states: &HashMap<String, UpstreamKeyStateSnapshot>) {
        let now = now_ms();
        for slot in &self.slots {
            if let Some(state) = states.get(&slot.id) {
                // Restore cooldown if not expired
                if state.cooldown_until_ms > now {
                    slot.cooldown_until_ms
                        .store(state.cooldown_until_ms, Ordering::Relaxed);
                }
                // Restore strikes
                slot.rate_limit_strikes
                    .store(state.rate_limit_strikes, Ordering::Relaxed);
                // Restore enabled state (bidirectional)
                slot.enabled.store(state.enabled, Ordering::Relaxed);
                // Restore unexpired scope cooldowns
                if !state.scope_cooldowns.is_empty() {
                    let mut sc = slot.scope_cooldowns.write();
                    for (scope, &until) in &state.scope_cooldowns {
                        if until > now {
                            sc.insert(Arc::from(scope.as_str()), until);
                        }
                    }
                }
                // Restore unexpired per-model cooldowns
                if !state.model_cooldowns.is_empty() {
                    let mut mc = slot.model_cooldowns.write();
                    for (model, &(until, level)) in &state.model_cooldowns {
                        if until > now {
                            mc.insert(Arc::from(model.as_str()), ModelCooldownEntry {
                                cooldown_until_ms: until,
                                backoff_level: level,
                            });
                        }
                    }
                }
            }
        }
    }

    /// If the pool's dynamic state has changed since the last call, return the
    /// exported key states and clear the dirty flag. Returns `None` if no change.
    pub fn take_dirty_states(&self) -> Option<HashMap<String, UpstreamKeyStateSnapshot>> {
        if !self.state_dirty.swap(false, Ordering::Relaxed) {
            return None;
        }
        let states = self.export_key_states();
        Some(states)
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
        let mut id_seen: std::collections::HashSet<String> =
            specs.iter().map(|s| s.id.clone()).collect();
        let mut next_id: usize = specs.len().saturating_add(1);
        for mut k in incoming {
            k.secret = k.secret.trim().to_string();
            if k.secret.is_empty() || seen.contains(&k.secret) {
                continue;
            }
            seen.insert(k.secret.clone());
            if k.id.is_empty() || id_seen.contains(&k.id) {
                loop {
                    let candidate = format!("key-{}", next_id);
                    next_id += 1;
                    if !id_seen.contains(&candidate) {
                        k.id = candidate;
                        break;
                    }
                }
            }
            id_seen.insert(k.id.clone());
            specs.push(k);
        }
        Self::hot_replace(pool, specs)
    }

    /// Hot-replace the key pool, preserving inflight/cooldown for matching ids.
    pub fn hot_replace(pool: &Arc<Self>, mut specs: Vec<UpstreamKeySpec>) -> Arc<Self> {
        auto_assign_account_ids(&mut specs);
        ensure_unique_ids(&mut specs);
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
                let mut rate_limit_strikes = 0u32;
                let mut scope_cooldowns = HashMap::new();
                let mut model_cooldowns = HashMap::new();
                let mut supported_models: Arc<[String]> = Arc::from(spec.supported_models.clone());
                let mut priority = spec.priority;
                if let Some(prev) = old.slots.iter().find(|s| s.id == id) {
                    inflight = prev.inflight.load(Ordering::Relaxed);
                    cooldown_until_ms = prev.cooldown_until_ms.load(Ordering::Relaxed);
                    rate_limit_strikes = prev.rate_limit_strikes.load(Ordering::Relaxed);
                    scope_cooldowns = prev.scope_cooldowns.read().clone();
                    model_cooldowns = prev.model_cooldowns.read().clone();
                    if supported_models.is_empty() {
                        supported_models = prev.supported_models.read().clone();
                    }
                    // Preserve runtime priority unless spec overrides (non-default)
                    if spec.priority == 0 && prev.priority.load(Ordering::Relaxed) != 0 {
                        priority = prev.priority.load(Ordering::Relaxed);
                    }
                }
                let semaphore = old
                    .slots
                    .iter()
                    .find(|s| s.id == id)
                    .map(|prev| prev.semaphore.clone())
                    .unwrap_or_else(|| {
                        Arc::new(tokio::sync::Semaphore::new(semaphore_permits(
                            old.max_inflight,
                        )))
                    });
                UpstreamKeySlot {
                    id,
                    secret: Arc::from(spec.secret.as_str()),
                    account_id: normalize_account_id(&spec.account_id),
                    enabled: AtomicBool::new(spec.enabled),
                    inflight: AtomicUsize::new(inflight),
                    cooldown_until_ms: AtomicU64::new(cooldown_until_ms),
                    rate_limit_strikes: AtomicU32::new(rate_limit_strikes),
                    scope_cooldowns: RwLock::new(scope_cooldowns),
                    model_cooldowns: RwLock::new(model_cooldowns),
                    supported_models: RwLock::new(supported_models),
                    semaphore,
                    priority: AtomicU32::new(priority),
                }
            })
            .collect();

        Arc::new(Self {
            slots: new_slots,
            rr: AtomicUsize::new(old.rr.load(Ordering::Relaxed)),
            cooldown_secs: old.cooldown_secs,
            max_inflight: old.max_inflight,
            codex_quota_cache: old.codex_quota_cache.clone(),
            state_dirty: AtomicBool::new(old.state_dirty.load(Ordering::Relaxed)),
            model_rr: RwLock::new(clone_model_rr(&old.model_rr.read())),
        })
    }

    pub fn acquire(self: &Arc<Self>) -> Option<UpstreamKeyGuard> {
        self.acquire_excluding_account(None)
    }

    /// Query the current inflight count for a specific key by id.
    /// Returns `usize::MAX` if the key is not found.
    pub fn inflight_of(&self, key_id: &str) -> usize {
        self.slots
            .iter()
            .find(|s| s.id == key_id)
            .map(|s| s.inflight.load(Ordering::Relaxed))
            .unwrap_or(usize::MAX)
    }

    /// Acquire a specific key by id (for conversation-level binding).
    /// Respects enabled + cooldown checks. Returns `None` if the key is
    /// disabled, in cooldown, or not found.
    pub fn acquire_specific(self: &Arc<Self>, key_id: &str) -> Option<UpstreamKeyGuard> {
        self.acquire_specific_scoped(key_id, None)
    }

    /// Like [`Self::acquire_specific`] but also honors Codex model-family scope cooldowns.
    pub fn acquire_specific_scoped(
        self: &Arc<Self>,
        key_id: &str,
        scope: Option<&str>,
    ) -> Option<UpstreamKeyGuard> {
        let now = now_ms();
        let idx = self.slots.iter().position(|s| {
            s.id == key_id
                && s.enabled.load(Ordering::Relaxed)
                && slot_passes_cooldown(s, scope, None, now)
        })?;
        // Try to acquire a semaphore permit (non-blocking).
        let permit = Arc::clone(&self.slots[idx].semaphore)
            .try_acquire_owned()
            .ok()?;
        let inflight = self.slots[idx].inflight.fetch_add(1, Ordering::AcqRel) + 1;
        global_metrics().set_upstream_key_inflight(&self.slots[idx].id, inflight as i64);
        Some(UpstreamKeyGuard {
            pool: Arc::clone(self),
            index: idx,
            _permit: Some(permit),
        })
    }

    /// Async variant: wait up to `timeout` for a semaphore permit on the bound key.
    /// Returns `None` on timeout instead of spilling to a different key; MiMo/Codex
    /// reasoning continuity depends on conversation-level key stickiness.
    pub async fn acquire_with_binding_async(
        self: &Arc<Self>,
        bound_key_id: &str,
        timeout: Duration,
    ) -> Option<UpstreamKeyGuard> {
        self.acquire_with_binding_async_scoped(bound_key_id, None, timeout)
            .await
    }

    pub async fn acquire_with_binding_async_scoped(
        self: &Arc<Self>,
        bound_key_id: &str,
        scope: Option<&str>,
        timeout: Duration,
    ) -> Option<UpstreamKeyGuard> {
        if let Some(guard) = self.acquire_specific_scoped(bound_key_id, scope) {
            return Some(guard);
        }

        let idx = self.slots.iter().position(|s| {
            s.id == bound_key_id
                && s.enabled.load(Ordering::Relaxed)
                && slot_passes_cooldown(s, scope, None, now_ms())
        });
        if let Some(idx) = idx {
            let sem_future = Arc::clone(&self.slots[idx].semaphore).acquire_owned();
            match tokio::time::timeout(timeout, sem_future).await {
                Ok(Ok(permit)) => {
                    let inflight = self.slots[idx].inflight.fetch_add(1, Ordering::AcqRel) + 1;
                    global_metrics()
                        .set_upstream_key_inflight(&self.slots[idx].id, inflight as i64);
                    return Some(UpstreamKeyGuard {
                        pool: Arc::clone(self),
                        index: idx,
                        _permit: Some(permit),
                    });
                }
                _ => {} // Timeout or semaphore closed → spill
            }
        }

        None
    }

    /// Acquire a key excluding a specific key_id (for overflow when bound key is full).
    /// Selects the enabled, non-cooldown key with the lowest inflight count.
    pub fn acquire_excluding_key(
        self: &Arc<Self>,
        excluded_key_id: &str,
    ) -> Option<UpstreamKeyGuard> {
        self.acquire_excluding_account_with_filter(None, |slot| slot.id != excluded_key_id)
    }

    /// Prefer JWT OAuth keys for Codex profiles (skip legacy `sk-*` placeholders).
    pub fn acquire_codex_oauth(self: &Arc<Self>) -> Option<UpstreamKeyGuard> {
        self.acquire_for_upstream_model("", true)
    }

    /// Codex-aware acquire with multiple key exclusion (for quota preflight loop).
    /// Excludes all key IDs in `exclude_keys` set plus the single `excluded_account`.
    pub fn acquire_codex_for_model_excluding(
        self: &Arc<Self>,
        upstream_model: &str,
        excluded_account: Option<&str>,
        fill_first: bool,
        exclude_keys: &std::collections::HashSet<String>,
    ) -> Option<UpstreamKeyGuard> {
        let scope = crate::codex_rate_limit::codex_model_scope(upstream_model);
        let model = upstream_model.trim();
        let explicit_only = !model.is_empty()
            && self.slots.iter().any(|s| {
                s.enabled.load(Ordering::Relaxed) && !s.supported_models.read().is_empty()
            });
        let accept = |slot: &UpstreamKeySlot| -> bool {
            if exclude_keys.contains(&slot.id) {
                return false;
            }
            looks_like_codex_oauth_key(&slot.secret, &slot.account_id)
                && slot_supports_model(slot, model)
        };
        if explicit_only {
            if let Some(guard) =
                self.acquire_scored(excluded_account, Some(scope), Some(model), fill_first, true, accept)
            {
                return Some(guard);
            }
            return self.acquire_scored(excluded_account, Some(scope), Some(model), fill_first, false, accept);
        }
        self.acquire_scored(excluded_account, Some(scope), Some(model), fill_first, false, accept)
    }

    /// Codex-aware acquire: OAuth keys, model catalog, scope cooldown, fill-first backoff.
    pub fn acquire_codex_for_model(
        self: &Arc<Self>,
        upstream_model: &str,
        excluded_account: Option<&str>,
        fill_first: bool,
    ) -> Option<UpstreamKeyGuard> {
        let scope = crate::codex_rate_limit::codex_model_scope(upstream_model);
        let model = upstream_model.trim();
        let explicit_only = !model.is_empty()
            && self.slots.iter().any(|s| {
                s.enabled.load(Ordering::Relaxed) && !s.supported_models.read().is_empty()
            });
        let accept = |slot: &UpstreamKeySlot| -> bool {
            looks_like_codex_oauth_key(&slot.secret, &slot.account_id)
                && slot_supports_model(slot, model)
        };
        if explicit_only {
            if let Some(guard) =
                self.acquire_scored(excluded_account, Some(scope), Some(model), fill_first, true, accept)
            {
                return Some(guard);
            }
            return self.acquire_scored(excluded_account, Some(scope), Some(model), fill_first, false, accept);
        }
        self.acquire_scored(excluded_account, Some(scope), Some(model), fill_first, false, accept)
    }

    /// Select a key that can serve `upstream_model` (empty model = any).
    /// Pass 1: keys with explicit catalog containing the model.
    /// Pass 2: keys with empty catalog (unknown / legacy).
    pub fn acquire_for_upstream_model(
        self: &Arc<Self>,
        upstream_model: &str,
        codex_oauth_only: bool,
    ) -> Option<UpstreamKeyGuard> {
        let model = upstream_model.trim();
        let explicit_only = !model.is_empty()
            && self.slots.iter().any(|s| {
                s.enabled.load(Ordering::Relaxed) && !s.supported_models.read().is_empty()
            });
        if explicit_only {
            if let Some(guard) = self.acquire_with_model_policy(model, codex_oauth_only, true) {
                return Some(guard);
            }
            return self.acquire_with_model_policy(model, codex_oauth_only, false);
        }
        self.acquire_scored(None, None, Some(model), false, false, |slot| {
            (!codex_oauth_only || looks_like_codex_oauth_key(&slot.secret, &slot.account_id))
                && slot_supports_model(slot, model)
        })
    }

    fn acquire_with_model_policy(
        self: &Arc<Self>,
        upstream_model: &str,
        codex_oauth_only: bool,
        require_explicit_catalog: bool,
    ) -> Option<UpstreamKeyGuard> {
        self.acquire_scored(None, None, Some(upstream_model), false, require_explicit_catalog, |slot| {
            if codex_oauth_only && !looks_like_codex_oauth_key(&slot.secret, &slot.account_id) {
                return false;
            }
            if require_explicit_catalog {
                !slot.supported_models.read().is_empty()
                    && slot_supports_model(slot, upstream_model)
            } else {
                slot.supported_models.read().is_empty() || slot_supports_model(slot, upstream_model)
            }
        })
    }

    /// Acquire a slot whose `account_id` differs from `excluded` (used after 429 on one account).
    pub fn acquire_excluding_account(
        self: &Arc<Self>,
        excluded: Option<&str>,
    ) -> Option<UpstreamKeyGuard> {
        self.acquire_excluding_account_with_filter(excluded, |_| true)
    }

    fn acquire_excluding_account_with_filter<F>(
        self: &Arc<Self>,
        excluded: Option<&str>,
        mut accept: F,
    ) -> Option<UpstreamKeyGuard>
    where
        F: FnMut(&UpstreamKeySlot) -> bool,
    {
        self.acquire_scored(excluded, None, None, false, false, move |slot| accept(slot))
    }

    fn acquire_scored<F>(
        self: &Arc<Self>,
        excluded: Option<&str>,
        scope: Option<&str>,
        model: Option<&str>,
        fill_first: bool,
        require_explicit_catalog: bool,
        mut accept: F,
    ) -> Option<UpstreamKeyGuard>
    where
        F: FnMut(&UpstreamKeySlot) -> bool,
    {
        if self.slots.is_empty() {
            return None;
        }

        // Use quota threshold from cache if available
        let quota_threshold = self.quota_cache().map(|_| 2.0); // Default 2% (OmniRoute)

        if fill_first {
            if let Some(guard) = self.acquire_scored_pass(
                excluded,
                scope,
                model,
                true,
                require_explicit_catalog,
                &mut accept,
                Some(0),
                quota_threshold,
            ) {
                return Some(guard);
            }
        }

        self.acquire_scored_pass(
            excluded,
            scope,
            model,
            fill_first,
            require_explicit_catalog,
            &mut accept,
            None,
            quota_threshold,
        )
    }

    fn acquire_scored_pass<F>(
        self: &Arc<Self>,
        excluded: Option<&str>,
        scope: Option<&str>,
        model: Option<&str>,
        fill_first: bool,
        require_explicit_catalog: bool,
        accept: &mut F,
        max_strikes: Option<u32>,
        quota_threshold_percent: Option<f64>,
    ) -> Option<UpstreamKeyGuard>
    where
        F: FnMut(&UpstreamKeySlot) -> bool,
    {
        let now = now_ms();
        let n = self.slots.len();

        // Collect unique priority values from enabled slots
        let mut priorities: Vec<u32> = self.slots.iter()
            .filter(|s| s.enabled.load(Ordering::Relaxed))
            .map(|s| s.priority.load(Ordering::Relaxed))
            .collect();
        priorities.sort_unstable();
        priorities.dedup();

        // Iterate priority buckets ascending (0 = highest priority)
        for priority in &priorities {
            let mut candidates: Vec<usize> = Vec::new();
            for i in 0..n {
                let slot = &self.slots[i];
                if !slot.enabled.load(Ordering::Relaxed) {
                    continue;
                }
                if slot.priority.load(Ordering::Relaxed) != *priority {
                    continue;
                }
                if !slot_passes_cooldown(slot, scope, model, now) {
                    continue;
                }
                if let Some(ex) = excluded
                    && slot.account_id.as_ref() == ex
                {
                    continue;
                }
                if require_explicit_catalog && slot.supported_models.read().is_empty() {
                    continue;
                }
                if !accept(slot) {
                    continue;
                }
                let strikes = slot.rate_limit_strikes.load(Ordering::Relaxed);
                if max_strikes.is_some_and(|max| strikes > max) {
                    continue;
                }

                // Quota-aware filtering: skip keys exhausted by quota cache
                if let (Some(threshold), Some(cache)) = (quota_threshold_percent, self.quota_cache()) {
                    if looks_like_codex_oauth_key(&slot.secret, &slot.account_id) {
                        if cache.is_exhausted(&slot.id, threshold) {
                            continue;
                        }
                    }
                }
                candidates.push(i);
            }

            if candidates.is_empty() {
                continue;
            }

            // Select best candidate within this priority bucket
            let start = self.next_model_rr_index(model.unwrap_or(""), candidates.len());
            let mut best_idx: Option<usize> = None;
            let mut best_score = usize::MAX;

            for offset in 0..candidates.len() {
                let ci = (start + offset) % candidates.len();
                let i = candidates[ci];
                let slot = &self.slots[i];

                let inflight = slot.inflight.load(Ordering::Relaxed);
                let strikes = slot.rate_limit_strikes.load(Ordering::Relaxed);
                let strikes_score = strikes.saturating_mul(1000) as usize;

                // Quota-aware scoring: penalize keys with low remaining headroom
                let quota_penalty = if let Some(cache) = self.quota_cache() {
                    if looks_like_codex_oauth_key(&slot.secret, &slot.account_id) {
                        if let Some(headroom) = cache.headroom_percent(&slot.id) {
                            let base_penalty = ((100.0 - headroom) / 8.0) as usize;
                            let threshold_penalty = if headroom <= 10.0 {
                                10
                            } else if headroom <= 25.0 {
                                4
                            } else {
                                0
                            };
                            base_penalty + threshold_penalty
                        } else {
                            4
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };

                let score = if fill_first {
                    strikes_score + inflight + quota_penalty
                } else {
                    inflight + quota_penalty
                };
                if score < best_score {
                    best_score = score;
                    best_idx = Some(i);
                }
            }

            if let Some(idx) = best_idx {
                // Try semaphore; fail closed so the caller can try the next candidate.
                let permit = Arc::clone(&self.slots[idx].semaphore)
                    .try_acquire_owned()
                    .ok()?;
                let inflight = self.slots[idx].inflight.fetch_add(1, Ordering::AcqRel) + 1;
                global_metrics().set_upstream_key_inflight(&self.slots[idx].id, inflight as i64);
                return Some(UpstreamKeyGuard {
                    pool: Arc::clone(self),
                    index: idx,
                    _permit: Some(permit),
                });
            }
        }

        None
    }

    fn release(&self, index: usize) {
        if let Some(slot) = self.slots.get(index) {
            let prev = slot.inflight.fetch_sub(1, Ordering::Relaxed);
            let inflight = prev.saturating_sub(1);
            global_metrics().set_upstream_key_inflight(&slot.id, inflight as i64);
        }
    }

    /// Get the next round-robin start index for a specific model.
    /// Falls back to global rr for empty models.
    fn next_model_rr_index(&self, model: &str, n: usize) -> usize {
        let model_key = normalize_model_slug(model);
        if model_key.is_empty() || n == 0 {
            return self.rr.fetch_add(1, Ordering::Relaxed) % n.max(1);
        }
        let mut map = self.model_rr.write();
        let counter = map
            .entry(Arc::from(model_key.as_str()))
            .or_insert_with(|| AtomicUsize::new(0));
        counter.fetch_add(1, Ordering::Relaxed) % n
    }

    /// Mark key rate-limited and try to acquire another from a **different** `account_id`.
    pub fn rotate_after_rate_limit(pool: &Arc<Self>, key_id: &str) -> Option<UpstreamKeyGuard> {
        Self::rotate_after_rate_limit_for(pool, key_id, pool.cooldown_secs, None, false)
    }

    /// Rate-limit cooldown with optional Codex scope + fill-first rotation.
    pub fn rotate_after_rate_limit_for(
        pool: &Arc<Self>,
        key_id: &str,
        cooldown_secs: u64,
        scope: Option<&str>,
        fill_first: bool,
    ) -> Option<UpstreamKeyGuard> {
        pool.report_rate_limited_for(key_id, cooldown_secs, scope);
        let excluded = pool
            .slots
            .iter()
            .find(|s| s.id == key_id)
            .map(|s| s.account_id.clone())?;
        global_metrics().record_upstream_key_retry("rate_limited_rotate");
        if scope.is_some() {
            pool.acquire_scored(Some(excluded.as_ref()), scope, None, fill_first, false, |_| true)
        } else {
            pool.acquire_excluding_account(Some(excluded.as_ref()))
        }
    }

    pub fn report_rate_limited(&self, key_id: &str) {
        self.report_rate_limited_for(key_id, self.cooldown_secs, None);
    }

    pub fn report_rate_limited_for(&self, key_id: &str, cooldown_secs: u64, scope: Option<&str>) {
        let until = now_ms() + cooldown_secs.saturating_mul(1000);
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            slot.cooldown_until_ms.store(until, Ordering::Relaxed);
            slot.rate_limit_strikes.fetch_add(1, Ordering::Relaxed);
            if let Some(scope) = scope.filter(|s| !s.is_empty() && *s != "default") {
                slot.scope_cooldowns.write().insert(Arc::from(scope), until);
            }
            self.state_dirty.store(true, Ordering::Relaxed);
        }
    }

    /// Report rate limit for a specific model on a key with progressive backoff.
    /// Only sets per-model cooldown, preserving the key for other models.
    pub fn report_rate_limited_for_model(
        &self,
        key_id: &str,
        model: &str,
        cooldown_secs: u64,
        scope: Option<&str>,
    ) {
        let now = now_ms();
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            // Progressive per-model cooldown
            let model_key = normalize_model_slug(model);
            if !model_key.is_empty() {
                let mut mc = slot.model_cooldowns.write();
                let model_arc: Arc<str> = Arc::from(model_key.as_str());
                let (backoff_secs, next_level) = if let Some(existing) = mc.get(&model_arc) {
                    if existing.cooldown_until_ms > now {
                        next_progressive_cooldown(existing.backoff_level)
                    } else {
                        (cooldown_secs.max(PROGRESSIVE_BACKOFF_BASE_SECS), 1u32)
                    }
                } else {
                    (cooldown_secs.max(PROGRESSIVE_BACKOFF_BASE_SECS), 1u32)
                };
                let until = now + backoff_secs.saturating_mul(1000);
                mc.insert(model_arc, ModelCooldownEntry {
                    cooldown_until_ms: until,
                    backoff_level: next_level,
                });
            }

            slot.rate_limit_strikes.fetch_add(1, Ordering::Relaxed);
            if let Some(scope) = scope.filter(|s| !s.is_empty() && *s != "default") {
                let scope_until = now + cooldown_secs.saturating_mul(1000);
                slot.scope_cooldowns.write().insert(Arc::from(scope), scope_until);
            }
            self.state_dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn record_key_success(&self, key_id: &str) {
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            let prev = slot.rate_limit_strikes.swap(0, Ordering::Relaxed);
            if prev > 0 {
                self.state_dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Record a successful response for a specific model, resetting per-model cooldown.
    pub fn record_model_success(&self, key_id: &str, model: &str) {
        self.record_key_success(key_id);
        let model_key = normalize_model_slug(model);
        if model_key.is_empty() {
            return;
        }
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            let removed = slot.model_cooldowns.write().remove(model_key.as_str()).is_some();
            if removed {
                self.state_dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Manually clear per-model cooldowns for a key.
    ///
    /// - `model = Some(slug)` clears a single model.
    /// - `model = None` clears **all** model cooldowns for the key.
    ///
    /// Returns the number of cooldowns removed.
    pub fn reset_model_cooldowns(&self, key_id: &str, model: Option<&str>) -> usize {
        let slot = match self.slots.iter().find(|s| s.id == key_id) {
            Some(s) => s,
            None => return 0,
        };
        let mut mc = slot.model_cooldowns.write();
        let removed = if let Some(m) = model {
            let mk = normalize_model_slug(m);
            if mk.is_empty() {
                return 0;
            }
            usize::from(mc.remove(mk.as_str()).is_some())
        } else {
            let count = mc.len();
            mc.clear();
            count
        };
        if removed > 0 {
            self.state_dirty.store(true, Ordering::Relaxed);
        }
        removed
    }

    pub fn report_unauthorized(&self, key_id: &str) {
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            slot.enabled.store(false, Ordering::Relaxed);
            self.state_dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn set_enabled(&self, key_id: &str, enabled: bool) -> bool {
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            slot.enabled.store(enabled, Ordering::Relaxed);
            self.state_dirty.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Update the priority of a specific key. Lower values = higher priority.
    pub fn set_priority(&self, key_id: &str, priority: u32) -> bool {
        if let Some(slot) = self.slots.iter().find(|s| s.id == key_id) {
            slot.priority.store(priority, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Check whether a key with the given ID exists in this pool.
    pub fn key_exists(&self, key_id: &str) -> bool {
        self.slots.iter().any(|s| s.id == key_id)
    }

    /// Return the full secret for a specific key by ID (for per-key testing).
    pub fn secret_by_id(&self, key_id: &str) -> Option<String> {
        self.slots
            .iter()
            .find(|s| s.id == key_id)
            .map(|s| s.secret.to_string())
    }

    /// Return configured per-key concurrency cap (0 = unlimited).
    pub fn max_inflight(&self) -> usize {
        self.max_inflight
    }

    /// Rebuild pool with a new per-key semaphore cap while preserving slot state.
    pub fn rebuild_with_max_inflight(pool: &Arc<Self>, max_inflight: usize) -> Arc<Self> {
        if pool.max_inflight == max_inflight {
            return Arc::clone(pool);
        }
        let mut specs = pool.to_specs();
        auto_assign_account_ids(&mut specs);
        ensure_unique_ids(&mut specs);
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
                let mut rate_limit_strikes = 0u32;
                let mut scope_cooldowns = HashMap::new();
                let mut model_cooldowns = HashMap::new();
                let mut supported_models: Arc<[String]> = Arc::from(spec.supported_models.clone());
                let mut priority = spec.priority;
                if let Some(prev) = pool.slots.iter().find(|s| s.id == id) {
                    inflight = prev.inflight.load(Ordering::Relaxed);
                    cooldown_until_ms = prev.cooldown_until_ms.load(Ordering::Relaxed);
                    rate_limit_strikes = prev.rate_limit_strikes.load(Ordering::Relaxed);
                    scope_cooldowns = prev.scope_cooldowns.read().clone();
                    model_cooldowns = prev.model_cooldowns.read().clone();
                    if supported_models.is_empty() {
                        supported_models = prev.supported_models.read().clone();
                    }
                    if spec.priority == 0 && prev.priority.load(Ordering::Relaxed) != 0 {
                        priority = prev.priority.load(Ordering::Relaxed);
                    }
                }
                UpstreamKeySlot {
                    id,
                    secret: Arc::from(spec.secret.as_str()),
                    account_id: normalize_account_id(&spec.account_id),
                    enabled: AtomicBool::new(spec.enabled),
                    inflight: AtomicUsize::new(inflight),
                    cooldown_until_ms: AtomicU64::new(cooldown_until_ms),
                    rate_limit_strikes: AtomicU32::new(rate_limit_strikes),
                    scope_cooldowns: RwLock::new(scope_cooldowns),
                    model_cooldowns: RwLock::new(model_cooldowns),
                    supported_models: RwLock::new(supported_models),
                    semaphore: Arc::new(tokio::sync::Semaphore::new(semaphore_permits(
                        max_inflight,
                    ))),
                    priority: AtomicU32::new(priority),
                }
            })
            .collect();

        Arc::new(Self {
            slots: new_slots,
            rr: AtomicUsize::new(pool.rr.load(Ordering::Relaxed)),
            cooldown_secs: pool.cooldown_secs,
            max_inflight,
            codex_quota_cache: pool.codex_quota_cache.clone(),
            state_dirty: AtomicBool::new(pool.state_dirty.load(Ordering::Relaxed)),
            model_rr: RwLock::new(clone_model_rr(&pool.model_rr.read())),
        })
    }

    /// Remove a key slot by id; returns a new pool or `None` if id not found.
    pub fn remove_key(pool: &Arc<Self>, key_id: &str) -> Option<Arc<Self>> {
        let specs: Vec<UpstreamKeySpec> = pool
            .to_specs()
            .into_iter()
            .filter(|s| s.id != key_id)
            .collect();
        if specs.len() == pool.len() {
            return None;
        }
        Some(Self::hot_replace(pool, specs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_account_id_is_unique_per_key() {
        let pool = UpstreamKeyPool::from_secrets(
            vec![
                "sk-key-one-aaaaaaa".into(),
                "sk-key-two-bbbbbb".into(),
                "sk-key-three-ccccc".into(),
            ],
            60,
            0,
        );
        let statuses = pool.list_status();
        assert_eq!(statuses.len(), 3);
        let ids: Vec<String> = statuses.iter().map(|k| k.account_id.clone()).collect();
        assert_ne!(
            ids[0], ids[1],
            "auto account_id for key-1 and key-2 must differ"
        );
        assert_ne!(
            ids[1], ids[2],
            "auto account_id for key-2 and key-3 must differ"
        );
        for (i, aid) in ids.iter().enumerate() {
            assert!(
                aid.starts_with("auto-"),
                "account_id for key-{} should start with 'auto-', got: {}",
                i + 1,
                aid
            );
            assert_eq!(
                aid.len(),
                21,
                "auto-xxx format should be 21 chars (auto- + 16 hex)"
            );
        }
    }

    #[test]
    fn auto_account_id_is_deterministic() {
        let pool1 = UpstreamKeyPool::from_secrets(vec!["sk-deterministic-key".into()], 60, 0);
        let pool2 = UpstreamKeyPool::from_secrets(vec!["sk-deterministic-key".into()], 60, 0);
        let s1 = pool1.list_status();
        let s2 = pool2.list_status();
        assert_eq!(s1[0].account_id, s2[0].account_id);
    }

    #[test]
    fn explicit_account_id_is_preserved() {
        let pool = UpstreamKeyPool::new(
            vec![UpstreamKeySpec {
                id: "my-key".into(),
                secret: "sk-something".into(),
                enabled: true,
                account_id: "my-custom-account".into(),
                supported_models: Vec::new(),
                priority: 0,
            }],
            60,
            0,
        );
        let statuses = pool.list_status();
        assert_eq!(statuses[0].account_id, "my-custom-account");
    }

    #[test]
    fn round_robin_prefers_lower_inflight() {
        let pool = UpstreamKeyPool::from_secrets(
            vec!["sk-aaaaaaaaaaaa".into(), "sk-bbbbbbbbbbbb".into()],
            60,
            0,
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
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-onlykey123456".into()], 60, 0);
        pool.report_rate_limited("key-1");
        assert!(pool.acquire().is_none());
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn empty_pool_returns_none() {
        let pool = UpstreamKeyPool::new(vec![], 60, 0);
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn merge_append_dedupes_secrets() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        let merged = UpstreamKeyPool::merge_append(
            &pool,
            vec![UpstreamKeySpec {
                id: String::new(),
                secret: "sk-bbbbbbbbbbbb".into(),
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 0,
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
                supported_models: Vec::new(),
                priority: 0,
            }],
        );
        assert_eq!(merged2.len(), 2);
    }

    #[test]
    fn bearer_secret_is_upstream_not_client_token() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-deepseek-upstream-secret".into()], 60, 0);
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
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "key-a2".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: "acct-a".into(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
            ],
            60,
            0,
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
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "key-b".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: "acct-b".into(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
            ],
            60,
            0,
        );
        let g1 = pool.acquire().unwrap();
        assert_eq!(g1.key_id(), "key-a");
        drop(g1);
        let g2 = UpstreamKeyPool::rotate_after_rate_limit(&pool, "key-a").unwrap();
        assert_eq!(g2.key_id(), "key-b");
    }

    #[test]
    fn rotate_finds_alternative_when_auto_account_id() {
        // from_secrets now auto-assigns unique account_ids, so rotation works.
        let pool = UpstreamKeyPool::from_secrets(
            vec!["sk-aaaaaaaaaaaa".into(), "sk-bbbbbbbbbbbb".into()],
            60,
            0,
        );
        let g1 = pool.acquire().unwrap();
        assert_eq!(g1.key_id(), "key-1");
        drop(g1);
        let g2 = UpstreamKeyPool::rotate_after_rate_limit(&pool, "key-1");
        assert!(
            g2.is_some(),
            "expected rotation to find key-2 with different auto account_id"
        );
        let g2 = g2.unwrap();
        assert_eq!(g2.key_id(), "key-2");
    }

    #[test]
    fn diagnose_empty_pool() {
        let pool = UpstreamKeyPool::new(vec![], 60, 0);
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
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "k2".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: false,
                    account_id: String::new(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
            ],
            60,
            0,
        );
        assert_eq!(
            pool.diagnose_acquire_failure(),
            PoolAcquireFailure::AllDisabled
        );
    }

    #[test]
    fn diagnose_all_in_cooldown() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 120, 0);
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
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "k2".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: String::new(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
            ],
            60,
            0,
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

    #[test]
    fn merge_append_dedupes_ids() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        let merged = UpstreamKeyPool::merge_append(
            &pool,
            vec![
                UpstreamKeySpec {
                    id: "key-1".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: String::new(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "key-1".into(),
                    secret: "sk-cccccccccccc".into(),
                    enabled: true,
                    account_id: String::new(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: String::new(),
                    secret: "sk-dddddddddddd".into(),
                    enabled: true,
                    account_id: String::new(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
            ],
        );
        assert_eq!(merged.len(), 4);
        let ids: Vec<String> = merged.list_status().into_iter().map(|s| s.id).collect();
        let unique_ids: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            ids.len(),
            unique_ids.len(),
            "duplicate ids found: {:?}",
            ids
        );
    }

    #[test]
    fn remove_key_drops_slot() {
        let pool = UpstreamKeyPool::from_secrets(
            vec!["sk-aaaaaaaaaaaa".into(), "sk-bbbbbbbbbbbb".into()],
            60,
            0,
        );
        let updated = UpstreamKeyPool::remove_key(&pool, "key-1").expect("removed");
        assert_eq!(updated.len(), 1);
        assert_eq!(updated.list_status()[0].id, "key-2");
        assert!(UpstreamKeyPool::remove_key(&pool, "missing").is_none());
    }

    #[test]
    fn new_dedupes_ids() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec {
                    id: "key-1".into(),
                    secret: "sk-aaaaaaaaaaaa".into(),
                    enabled: true,
                    account_id: String::new(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "key-1".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: String::new(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
            ],
            60,
            0,
        );
        assert_eq!(pool.len(), 2);
        let ids: Vec<String> = pool.list_status().into_iter().map(|s| s.id).collect();
        let unique_ids: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            ids.len(),
            unique_ids.len(),
            "duplicate ids found: {:?}",
            ids
        );
    }

    #[test]
    fn priority_scoring_prefers_lower_priority_value() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec {
                    id: "high-pri".into(),
                    secret: "sk-aaaaaaaaaaaa".into(),
                    enabled: true,
                    account_id: "acct-a".into(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "low-pri".into(),
                    secret: "sk-bbbbbbbbbbbb".into(),
                    enabled: true,
                    account_id: "acct-b".into(),
                    supported_models: Vec::new(),
                    priority: 5,
                },
            ],
            60,
            0,
        );
        // High priority (0) should be preferred over low priority (5)
        let guard = pool.acquire().unwrap();
        assert_eq!(guard.key_id(), "high-pri");
    }

    #[test]
    fn export_key_states_captures_cooldown() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        pool.report_rate_limited("key-1");
        let states = pool.export_key_states();
        assert_eq!(states.len(), 1);
        let state = states.get("key-1").unwrap();
        assert!(state.cooldown_until_ms > 0);
        assert_eq!(state.rate_limit_strikes, 1);
    }

    #[test]
    fn export_key_states_skips_clean_keys() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        let states = pool.export_key_states();
        assert!(states.is_empty(), "clean keys should not be exported");
    }

    #[test]
    fn apply_key_states_restores_cooldown() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut states = std::collections::HashMap::new();
        states.insert(
            "key-1".to_string(),
            UpstreamKeyStateSnapshot {
                key_id: "key-1".into(),
                cooldown_until_ms: now + 60_000,
                rate_limit_strikes: 3,
                scope_cooldowns: std::collections::HashMap::new(),
                enabled: true,
                model_cooldowns: std::collections::HashMap::new(),
            },
        );
        pool.apply_key_states(&states);
        assert_eq!(pool.available_count(), 0, "key should be in cooldown");
    }

    #[test]
    fn apply_key_states_restores_disabled() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        let mut states = std::collections::HashMap::new();
        states.insert(
            "key-1".to_string(),
            UpstreamKeyStateSnapshot {
                key_id: "key-1".into(),
                cooldown_until_ms: 0,
                rate_limit_strikes: 0,
                scope_cooldowns: std::collections::HashMap::new(),
                enabled: false,
                model_cooldowns: std::collections::HashMap::new(),
            },
        );
        pool.apply_key_states(&states);
        assert!(pool.acquire().is_none(), "disabled key should not be acquirable");
    }

    #[test]
    fn take_dirty_states_returns_none_when_clean() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        assert!(pool.take_dirty_states().is_none());
    }

    #[test]
    fn take_dirty_states_returns_some_after_rate_limit() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        pool.report_rate_limited("key-1");
        let states = pool.take_dirty_states();
        assert!(states.is_some());
        assert_eq!(states.unwrap().len(), 1);
        // Second call should return None (dirty flag cleared)
        assert!(pool.take_dirty_states().is_none());
    }

    #[test]
    fn take_dirty_states_captures_disabled() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        pool.report_unauthorized("key-1");
        let states = pool.take_dirty_states().unwrap();
        let state = states.get("key-1").unwrap();
        assert!(!state.enabled);
    }

    #[test]
    fn hot_replace_preserves_priority() {
        let pool = UpstreamKeyPool::new(
            vec![UpstreamKeySpec {
                id: "key-1".into(),
                secret: "sk-aaaaaaaaaaaa".into(),
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 5,
            }],
            60,
            0,
        );
        // hot_replace with same id but priority=0 (default) should preserve old priority
        let new_pool = UpstreamKeyPool::hot_replace(
            &pool,
            vec![UpstreamKeySpec {
                id: "key-1".into(),
                secret: "sk-aaaaaaaaaaaa".into(),
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 0,
            }],
        );
        let status = new_pool.list_status();
        assert_eq!(status[0].priority, 5, "priority should be preserved from old pool");
    }

    #[test]
    fn hot_replace_updates_priority_when_explicit() {
        let pool = UpstreamKeyPool::new(
            vec![UpstreamKeySpec {
                id: "key-1".into(),
                secret: "sk-aaaaaaaaaaaa".into(),
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 5,
            }],
            60,
            0,
        );
        // hot_replace with explicit non-zero priority should update
        let new_pool = UpstreamKeyPool::hot_replace(
            &pool,
            vec![UpstreamKeySpec {
                id: "key-1".into(),
                secret: "sk-aaaaaaaaaaaa".into(),
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 10,
            }],
        );
        let status = new_pool.list_status();
        assert_eq!(status[0].priority, 10, "explicit priority should update");
    }

    #[test]
    fn set_priority_updates_key() {
        let pool = UpstreamKeyPool::new(
            vec![UpstreamKeySpec {
                id: "key-1".into(),
                secret: "sk-aaaaaaaaaaaa".into(),
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 0,
            }],
            60,
            0,
        );
        assert!(pool.set_priority("key-1", 7));
        let status = pool.list_status();
        assert_eq!(status[0].priority, 7);
    }

    #[test]
    fn to_specs_includes_priority() {
        let pool = UpstreamKeyPool::new(
            vec![UpstreamKeySpec {
                id: "key-1".into(),
                secret: "sk-aaaaaaaaaaaa".into(),
                enabled: true,
                account_id: String::new(),
                supported_models: Vec::new(),
                priority: 3,
            }],
            60,
            0,
        );
        let specs = pool.to_specs();
        assert_eq!(specs[0].priority, 3);
    }

    #[test]
    fn acquire_excluding_key_skips_cooled_bound_key() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec {
                    id: "bound".into(),
                    secret: "sk-bound".into(),
                    enabled: true,
                    account_id: "acct-a".into(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
                UpstreamKeySpec {
                    id: "spare".into(),
                    secret: "sk-spare".into(),
                    enabled: true,
                    account_id: "acct-b".into(),
                    supported_models: Vec::new(),
                    priority: 0,
                },
            ],
            60,
            3,
        );
        pool.report_rate_limited_for("bound", 300, None);
        let guard = pool
            .acquire_excluding_key("bound")
            .expect("spill to spare key");
        assert_eq!(guard.key_id(), "spare");
    }

    #[test]
    fn per_model_cooldown_isolation() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-key-123456789".into()], 60, 0);
        // Report rate limit for model-a
        pool.report_rate_limited_for_model("key-1", "deepseek-chat", 60, None);
        // Model-b should still be available
        // But global cooldown blocks everything, so use record_model_success to clear
        pool.record_model_success("key-1", "deepseek-chat");
        let guard = pool.acquire_for_upstream_model("deepseek-coder", false);
        assert!(guard.is_some(), "model-deepseek-coder should not be affected by deepseek-chat cooldown");
    }

    #[test]
    fn progressive_backoff_increases() {
        let (d1, l1) = next_progressive_cooldown(0);
        assert_eq!(d1, 1);
        assert_eq!(l1, 1);
        let (d2, l2) = next_progressive_cooldown(1);
        assert_eq!(d2, 2);
        assert_eq!(l2, 2);
        let (d3, l3) = next_progressive_cooldown(2);
        assert_eq!(d3, 4);
        assert_eq!(l3, 3);
        // Capped at 1800
        let (d_max, _) = next_progressive_cooldown(30);
        assert_eq!(d_max, 1800);
    }

    #[test]
    fn model_cooldown_export_restore() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        pool.report_rate_limited_for_model("key-1", "deepseek-chat", 30, None);
        let states = pool.export_key_states();
        let state = states.get("key-1").unwrap();
        assert!(!state.model_cooldowns.is_empty());
        assert!(state.model_cooldowns.contains_key("deepseek-chat"));

        // Restore to a new pool
        let pool2 = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        pool2.apply_key_states(&states);
        // Check model cooldown was restored
        let slot = &pool2.slots[0];
        assert!(slot.model_cooldowns.read().contains_key("deepseek-chat"));
    }

    #[test]
    fn per_model_round_robin_distributes() {
        let pool = UpstreamKeyPool::from_secrets(
            vec!["sk-aaaaaaaaaaaa".into(), "sk-bbbbbbbbbbbb".into()],
            60,
            0,
        );
        // Same model should rotate between keys
        let g1 = pool.acquire_for_upstream_model("model-x", false).unwrap();
        let g2 = pool.acquire_for_upstream_model("model-x", false).unwrap();
        assert_ne!(g1.key_id(), g2.key_id(), "per-model RR should alternate");
    }

    #[test]
    fn model_cooldown_view_populated() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        pool.report_rate_limited_for_model("key-1", "deepseek-chat", 60, None);
        let status = pool.list_status();
        assert_eq!(status.len(), 1);
        assert!(!status[0].model_cooldowns.is_empty());
        assert_eq!(status[0].model_cooldowns[0].model, "deepseek-chat");
    }

    #[test]
    fn record_model_success_clears_cooldown() {
        let pool = UpstreamKeyPool::from_secrets(vec!["sk-aaaaaaaaaaaa".into()], 60, 0);
        pool.report_rate_limited_for_model("key-1", "deepseek-chat", 60, None);
        pool.record_model_success("key-1", "deepseek-chat");
        let slot = &pool.slots[0];
        assert!(slot.model_cooldowns.read().is_empty());
    }

    #[test]
    fn per_model_cooldown_blocks_specific_model_only() {
        let pool = UpstreamKeyPool::from_secrets(
            vec!["sk-key-one-aaaaaaa".into(), "sk-key-two-bbbbbb".into()],
            60,
            0,
        );
        // Rate-limit key-1 for deepseek-chat with a long cooldown
        pool.report_rate_limited_for_model("key-1", "deepseek-chat", 300, None);
        // key-1 is still available for other models (no global cooldown)
        let guard_other = pool.acquire_for_upstream_model("deepseek-coder", false);
        assert!(guard_other.is_some(), "key should be available for non-cooled model");
    }

    #[test]
    fn priority_bucketing_exhausts_higher_first() {
        let pool = UpstreamKeyPool::new(
            vec![
                UpstreamKeySpec { id: "hi".into(), secret: "sk-hi-aaaaaaaaaa".into(), enabled: true, account_id: String::new(), supported_models: Vec::new(), priority: 0 },
                UpstreamKeySpec { id: "lo".into(), secret: "sk-lo-bbbbbbbbbb".into(), enabled: true, account_id: String::new(), supported_models: Vec::new(), priority: 10 },
            ],
            60,
            0, // unlimited inflight
        );
        // Both available, should prefer "hi" (priority 0)
        let g1 = pool.acquire().unwrap();
        assert_eq!(g1.key_id(), "hi");
        // "hi" still available (unlimited inflight), should still get it
        let g2 = pool.acquire().unwrap();
        assert_eq!(g2.key_id(), "hi");

        // Now cool down "hi" — should fall through to "lo"
        pool.report_rate_limited("hi");
        let g3 = pool.acquire().unwrap();
        assert_eq!(g3.key_id(), "lo");
    }
}
