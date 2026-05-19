//! Shared DeepSeek upstream validation helpers.

/// Returns an error message if the API key looks invalid or placeholder.
pub fn validate_deepseek_key(secret: &str) -> Result<(), String> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err("API key must not be empty".to_string());
    }
    if trimmed.starts_with("sk-your-") {
        return Err("API key appears to be a placeholder (starts with 'sk-your-')".to_string());
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamTestResult {
    pub ok: bool,
    pub status_code: u16,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamTestRequest {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelDetectResult {
    pub to_add: Vec<String>,
    pub to_remove: Vec<String>,
    pub unchanged: usize,
    pub upstream_total: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelApplyRequest {
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}
