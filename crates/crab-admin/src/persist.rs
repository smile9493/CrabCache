//! Admin auxiliary state persisted to JSON (models metadata, notes, test snapshots).

use crate::state::{KeyMetadata, StoredModel, StoredModelList, StoredUpstreamConfig};
use crate::types::UpstreamTestResult;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STATE_VERSION: u32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminStateFile {
    pub version: u32,
    #[serde(default)]
    pub models: PersistedModels,
    #[serde(default)]
    pub upstream_notes: Option<String>,
    #[serde(default)]
    pub last_upstream_test: Option<UpstreamTestResult>,
    #[serde(default)]
    pub upstream_snapshot: Option<PersistedUpstreamSnapshot>,
    #[serde(default)]
    pub keys_meta: Vec<PersistedKeyMetadata>,
    #[serde(default)]
    pub domain_policies: Vec<PersistedDomainPolicy>,
    /// Per-profile upstream API keys for model sync (v3).
    #[serde(default)]
    pub upstream_profile_secrets: PersistedProfileSecrets,
    /// Default (deepseek) upstream key pool secrets (v4).
    #[serde(default)]
    pub upstream_pool_secrets: Vec<PersistedUpstreamPoolSecret>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedProfileSecrets {
    #[serde(default)]
    pub by_profile: std::collections::HashMap<String, Vec<PersistedUpstreamPoolSecret>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUpstreamPoolSecret {
    pub id: String,
    pub secret: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub account_id: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedDomainPolicy {
    pub domain: String,
    pub monthly_token_budget: u64,
    pub monthly_cost_budget_usd: f64,
    pub min_hit_rate: f64,
    pub enabled: bool,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedKeyMetadata {
    pub id: String,
    pub token: String,
    pub rpm_limit: u64,
    pub monthly_token_limit: u64,
    pub expired_at: Option<u64>,
    pub model_limits: Vec<String>,
    pub remain_quota: i64,
    pub unlimited_quota: bool,
    /// Admin-side key name (consumer label). Added in v2.
    #[serde(default)]
    pub name: String,
    /// Month key (YYYY-MM) for accumulated usage counters. Added in v2.
    #[serde(default)]
    pub usage_month: String,
    #[serde(default)]
    pub tokens_this_month: u64,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedModels {
    pub models: Vec<PersistedModel>,
    #[serde(default)]
    pub synced_at_by_profile: std::collections::HashMap<String, String>,
    /// Legacy single sync timestamp (migrated into map).
    #[serde(default)]
    pub synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedModel {
    #[serde(default = "default_profile_id_persist")]
    pub profile_id: String,
    pub id: String,
    pub owned_by: String,
    pub context_length: Option<u64>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub available: bool,
    #[serde(default)]
    pub account_ids: Vec<String>,
    #[serde(default)]
    pub key_ids: Vec<String>,
}

fn default_profile_id_persist() -> String {
    "deepseek".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUpstreamSnapshot {
    pub base_url: String,
    pub model: String,
    pub endpoints: Vec<String>,
}

#[derive(Clone)]
pub struct PersistHandle {
    path: PathBuf,
    debounce: Arc<Mutex<Option<Instant>>>,
}

impl PersistHandle {
    pub fn path() -> PathBuf {
        std::env::var("CRABCACHE_ADMIN_STATE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/admin-state.json"))
    }

    pub fn new() -> Self {
        Self {
            path: Self::path(),
            debounce: Arc::new(Mutex::new(None)),
        }
    }

    pub fn load(&self) -> AdminStateFile {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => {
                let mut file: AdminStateFile = serde_json::from_str(&content).unwrap_or_default();
                file.migrate_v4();
                file
            }
            Err(_) => AdminStateFile::default(),
        }
    }

    pub fn save_now(&self, file: &AdminStateFile) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let write_ok = if let Ok(json) = serde_json::to_string_pretty(file) {
            // Atomic write: write to tmp file then rename to avoid corruption
            // if the process is killed mid-write (e.g. docker restart).
            let tmp_path = self.path.with_extension("json.tmp");
            let ok = std::fs::write(&tmp_path, &json).is_ok()
                && std::fs::rename(&tmp_path, &self.path).is_ok();
            // Best-effort cleanup of tmp file if rename failed.
            if !ok {
                let _ = std::fs::remove_file(&tmp_path);
            }
            ok
        } else {
            false
        };
        if !write_ok {
            tracing::warn!(path = %self.path.display(), "Failed to write admin state");
        }
    }

    /// Debounced save (300ms) to avoid hammering disk on rapid toggles.
    #[allow(dead_code)]
    pub fn save_debounced(self: &Arc<Self>, file: AdminStateFile) {
        *self.debounce.lock() = Some(Instant::now());
        let handle = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let should_write = handle
                .debounce
                .lock()
                .as_ref()
                .map(|instant| instant.elapsed() >= Duration::from_millis(280))
                .unwrap_or(false);
            if should_write {
                handle.save_now(&file);
            }
        });
    }
}

impl AdminStateFile {
    fn migrate_v2(&mut self) {
        if self.version >= 2 {
            return;
        }
        if let Some(ts) = self.models.synced_at.take() {
            self.models
                .synced_at_by_profile
                .entry("deepseek".to_string())
                .or_insert(ts);
        }
        for m in &mut self.models.models {
            if m.profile_id.is_empty() {
                m.profile_id = "deepseek".to_string();
            }
        }
        self.version = 2;
    }

    fn migrate_v3(&mut self) {
        self.migrate_v2();
        if self.version >= 3 {
            return;
        }
        self.version = 3;
    }

    fn migrate_v4(&mut self) {
        self.migrate_v3();
        if self.version >= 4 {
            return;
        }
        // v4 adds upstream_pool_secrets; serde(default) handles existing files.
        self.version = 4;
    }
}

impl Default for AdminStateFile {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            models: PersistedModels::default(),
            upstream_notes: None,
            last_upstream_test: None,
            upstream_snapshot: None,
            keys_meta: Vec::new(),
            domain_policies: Vec::new(),
            upstream_profile_secrets: PersistedProfileSecrets::default(),
            upstream_pool_secrets: Vec::new(),
        }
    }
}

impl From<&KeyMetadata> for PersistedKeyMetadata {
    fn from(m: &KeyMetadata) -> Self {
        Self {
            id: m.id.clone(),
            token: m.token.clone(),
            name: m.name.clone(),
            rpm_limit: m.rpm_limit,
            monthly_token_limit: m.monthly_token_limit,
            expired_at: m.expired_at,
            model_limits: m.model_limits.clone(),
            remain_quota: m.remain_quota,
            unlimited_quota: m.unlimited_quota,
            max_concurrent: m.max_concurrent,
            usage_month: m.usage_month.clone(),
            tokens_this_month: m.tokens_this_month,
            input_tokens: m.input_tokens,
            output_tokens: m.output_tokens,
        }
    }
}

impl From<PersistedKeyMetadata> for KeyMetadata {
    fn from(p: PersistedKeyMetadata) -> Self {
        Self {
            id: p.id,
            token: p.token,
            name: p.name,
            rpm_limit: p.rpm_limit,
            monthly_token_limit: p.monthly_token_limit,
            current_rpm: 0,
            tokens_this_month: p.tokens_this_month,
            input_tokens: p.input_tokens,
            output_tokens: p.output_tokens,
            expired_at: p.expired_at,
            model_limits: p.model_limits,
            remain_quota: p.remain_quota,
            unlimited_quota: p.unlimited_quota,
            max_concurrent: p.max_concurrent,
            usage_month: p.usage_month,
        }
    }
}

impl From<&StoredModelList> for PersistedModels {
    fn from(list: &StoredModelList) -> Self {
        PersistedModels {
            models: list
                .models
                .iter()
                .map(|m| PersistedModel {
                    profile_id: m.profile_id.clone(),
                    id: m.id.clone(),
                    owned_by: m.owned_by.clone(),
                    context_length: m.context_length,
                    input_price_per_mtok: m.input_price_per_mtok,
                    output_price_per_mtok: m.output_price_per_mtok,
                    available: m.available,
                    account_ids: m.account_ids.clone(),
                    key_ids: m.key_ids.clone(),
                })
                .collect(),
            synced_at_by_profile: list.synced_at_by_profile.clone(),
            synced_at: None,
        }
    }
}

impl From<PersistedModels> for StoredModelList {
    fn from(mut p: PersistedModels) -> Self {
        if let Some(ts) = p.synced_at.take() {
            p.synced_at_by_profile
                .entry("deepseek".to_string())
                .or_insert(ts);
        }
        StoredModelList {
            models: p
                .models
                .into_iter()
                .map(|m| StoredModel {
                    profile_id: if m.profile_id.is_empty() {
                        "deepseek".to_string()
                    } else {
                        m.profile_id
                    },
                    id: m.id,
                    owned_by: m.owned_by,
                    context_length: m.context_length,
                    input_price_per_mtok: m.input_price_per_mtok,
                    output_price_per_mtok: m.output_price_per_mtok,
                    available: m.available,
                    account_ids: m.account_ids,
                    key_ids: m.key_ids,
                })
                .collect(),
            synced_at_by_profile: p.synced_at_by_profile,
        }
    }
}

pub fn build_state_file(
    models: &StoredModelList,
    upstream: &StoredUpstreamConfig,
    last_test: Option<UpstreamTestResult>,
    notes: Option<String>,
    keys_meta: &[PersistedKeyMetadata],
    domain_policies: &[PersistedDomainPolicy],
    profile_secrets: &PersistedProfileSecrets,
    pool_secrets: &[PersistedUpstreamPoolSecret],
) -> AdminStateFile {
    AdminStateFile {
        version: STATE_VERSION,
        models: models.into(),
        upstream_notes: notes,
        last_upstream_test: last_test,
        upstream_snapshot: Some(PersistedUpstreamSnapshot {
            base_url: upstream.base_url.clone(),
            model: upstream.model.clone(),
            endpoints: upstream.endpoints.clone(),
        }),
        keys_meta: keys_meta.to_vec(),
        domain_policies: domain_policies.to_vec(),
        upstream_profile_secrets: profile_secrets.clone(),
        upstream_pool_secrets: pool_secrets.to_vec(),
    }
}

impl From<&std::collections::HashMap<String, Vec<crate::state::UpstreamPoolSecret>>>
    for PersistedProfileSecrets
{
    fn from(
        map: &std::collections::HashMap<String, Vec<crate::state::UpstreamPoolSecret>>,
    ) -> Self {
        PersistedProfileSecrets {
            by_profile: map
                .iter()
                .map(|(profile_id, secrets)| {
                    (
                        profile_id.clone(),
                        secrets
                            .iter()
                            .map(|s| PersistedUpstreamPoolSecret {
                                id: s.id.clone(),
                                secret: s.secret.clone(),
                                enabled: s.enabled,
                                account_id: s.account_id.clone(),
                            })
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

impl From<PersistedProfileSecrets>
    for std::collections::HashMap<String, Vec<crate::state::UpstreamPoolSecret>>
{
    fn from(p: PersistedProfileSecrets) -> Self {
        p.by_profile
            .into_iter()
            .map(|(profile_id, secrets)| {
                (
                    profile_id,
                    secrets
                        .into_iter()
                        .map(|s| crate::state::UpstreamPoolSecret {
                            id: s.id,
                            secret: s.secret,
                            enabled: s.enabled,
                            account_id: s.account_id,
                        })
                        .collect(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_v4_roundtrip_pool_secrets() {
        let file = AdminStateFile {
            version: 4,
            upstream_pool_secrets: vec![
                PersistedUpstreamPoolSecret {
                    id: "key-1".to_string(),
                    secret: "sk-ds-test123456".to_string(),
                    enabled: true,
                    account_id: String::new(),
                },
                PersistedUpstreamPoolSecret {
                    id: "key-2".to_string(),
                    secret: "sk-ds-test789012".to_string(),
                    enabled: false,
                    account_id: String::new(),
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        let loaded: AdminStateFile = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, 4);
        assert_eq!(loaded.upstream_pool_secrets.len(), 2);
        assert_eq!(loaded.upstream_pool_secrets[0].id, "key-1");
        assert_eq!(loaded.upstream_pool_secrets[0].secret, "sk-ds-test123456");
        assert!(loaded.upstream_pool_secrets[0].enabled);
        assert!(!loaded.upstream_pool_secrets[1].enabled);
    }

    #[test]
    fn persist_v3_to_v4_migration() {
        // Simulate a v3 file (no upstream_pool_secrets field).
        let json = r#"{"version":3,"models":{"models":[]}}"#;
        let mut file: AdminStateFile = serde_json::from_str(json).unwrap();
        file.migrate_v4();
        assert_eq!(file.version, 4);
        assert!(file.upstream_pool_secrets.is_empty());
    }
}
