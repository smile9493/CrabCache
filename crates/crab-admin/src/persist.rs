//! Admin auxiliary state persisted to JSON (models metadata, notes, test snapshots).

use crate::state::{KeyMetadata, StoredModel, StoredModelList, StoredUpstreamConfig};
use crab_control::UpstreamTestResult;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STATE_VERSION: u32 = 1;

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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedModels {
    pub models: Vec<PersistedModel>,
    pub synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedModel {
    pub id: String,
    pub owned_by: String,
    pub context_length: Option<u64>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub available: bool,
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
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => AdminStateFile::default(),
        }
    }

    pub fn save_now(&self, file: &AdminStateFile) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(file) {
            if let Err(e) = std::fs::write(&self.path, json) {
                tracing::warn!(path = %self.path.display(), error = %e, "Failed to write admin state");
            }
        }
    }

    /// Debounced save (300ms) to avoid hammering disk on rapid toggles.
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
        }
    }
}

impl From<&KeyMetadata> for PersistedKeyMetadata {
    fn from(m: &KeyMetadata) -> Self {
        Self {
            id: m.id.clone(),
            token: m.token.clone(),
            rpm_limit: m.rpm_limit,
            monthly_token_limit: m.monthly_token_limit,
            expired_at: m.expired_at,
            model_limits: m.model_limits.clone(),
            remain_quota: m.remain_quota,
            unlimited_quota: m.unlimited_quota,
        }
    }
}

impl From<PersistedKeyMetadata> for KeyMetadata {
    fn from(p: PersistedKeyMetadata) -> Self {
        Self {
            id: p.id,
            token: p.token,
            rpm_limit: p.rpm_limit,
            monthly_token_limit: p.monthly_token_limit,
            current_rpm: 0,
            tokens_this_month: 0,
            input_tokens: 0,
            output_tokens: 0,
            expired_at: p.expired_at,
            model_limits: p.model_limits,
            remain_quota: p.remain_quota,
            unlimited_quota: p.unlimited_quota,
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
                    id: m.id.clone(),
                    owned_by: m.owned_by.clone(),
                    context_length: m.context_length,
                    input_price_per_mtok: m.input_price_per_mtok,
                    output_price_per_mtok: m.output_price_per_mtok,
                    available: m.available,
                })
                .collect(),
            synced_at: list.synced_at.clone(),
        }
    }
}

impl From<PersistedModels> for StoredModelList {
    fn from(p: PersistedModels) -> Self {
        StoredModelList {
            models: p
                .models
                .into_iter()
                .map(|m| StoredModel {
                    id: m.id,
                    owned_by: m.owned_by,
                    context_length: m.context_length,
                    input_price_per_mtok: m.input_price_per_mtok,
                    output_price_per_mtok: m.output_price_per_mtok,
                    available: m.available,
                })
                .collect(),
            synced_at: p.synced_at,
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
    }
}
