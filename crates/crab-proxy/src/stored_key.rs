//! Client API key metadata stored in `RuntimeConfig::keys`.

#[derive(Debug, Clone)]
pub struct StoredKey {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub enabled: bool,
    pub domain: Option<String>,
    /// DeepSeek `user_id` / tenant bucket; bound to this client key when set.
    pub project_id: Option<String>,
    /// `auto` | `cursor_deepseek_v4` | `deepseek_light` | `mimo_relay`
    /// | `mimo_token_plan_relay` | `mimo_payg_relay` | `generic_relay`
    pub pipeline: Option<String>,
    pub upstream_profile: Option<String>,
    /// Max simultaneous in-flight requests (0 = unlimited).
    pub max_concurrent: u32,
    /// Max requests per minute (0 = unlimited).
    pub rpm_limit: u32,
}
