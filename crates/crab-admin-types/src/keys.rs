use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_preview: String,
    pub key_full: Option<String>,
    pub active: bool,
    pub rpm_limit: u32,
    pub monthly_token_budget: u64,
    pub tokens_used_this_month: u64,
    pub expired_at: Option<u64>,
    pub model_limits: Vec<String>,
    pub remain_quota: i64,
    pub unlimited_quota: bool,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub inflight: usize,
    /// True when created via Dashboard (audit log or non-zero monthly budget).
    #[serde(default)]
    pub manually_created: bool,
    /// How many active keys share this display name (duplicate-name detector).
    #[serde(default)]
    pub duplicate_name_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_token_budget: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remain_quota: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlimited_quota: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub rpm_limit: u32,
    pub monthly_token_budget: u64,
    pub expired_at: Option<u64>,
    pub model_limits: Option<Vec<String>>,
    pub remain_quota: Option<i64>,
    pub unlimited_quota: Option<bool>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
    #[serde(default)]
    pub max_concurrent: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneDuplicateKeysRequest {
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneDuplicateKeysResponse {
    pub dry_run: bool,
    pub kept: Vec<PruneKeyDecision>,
    pub revoked: Vec<PruneKeyDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneKeyDecision {
    pub id: String,
    pub name: String,
    pub key_preview: String,
    pub reason: String,
}
