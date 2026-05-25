use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UserIdAuditBreakdown {
    pub injected: usize,
    pub absent: usize,
    pub stripped_client: usize,
    pub mismatch: usize,
    pub not_applicable: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserIdModelCount {
    pub model: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserIdProjectCount {
    pub project_id: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekUserIdAudit {
    pub deepseek_requests: usize,
    pub with_upstream_user_id: usize,
    pub without_upstream_user_id: usize,
    pub upstream_user_id_ratio: f64,
    pub missing_project_id: usize,
    pub client_user_id_leaks: usize,
    pub audit_breakdown: UserIdAuditBreakdown,
    pub by_upstream_model: Vec<UserIdModelCount>,
    pub top_project_ids: Vec<UserIdProjectCount>,
    pub isolation_ok: bool,
    pub conclusion: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceAnalysis {
    pub total_requests: usize,
    pub unique_requests: usize,
    pub repeat_ratio: f64,
    pub semantic_cluster_ratio: f64,
    pub estimated_zipf_alpha: f64,
    pub estimated_hit_rate: f64,
    pub avg_latency_ms: f64,
    pub avg_prompt_tokens: f64,
    pub cache_hit_ratio: f64,
    pub top_models: Vec<ModelUsage>,
    pub cluster_distribution: Vec<ClusterInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deepseek_user_id: Option<DeepSeekUserIdAudit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusterInfo {
    pub cluster_id: usize,
    pub count: usize,
    pub percentage: f64,
}
