mod backends;
mod client;
pub mod codex_wham;
mod error;
mod types;
mod upstream_url;
mod validate;

pub use backends::parse_backend_endpoints;
pub use client::GatewayAdminClient;
pub use error::ControlError;
pub use types::*;
pub use upstream_url::{UpstreamBaseUrl, parse_upstream_base_url};
pub use codex_wham::CodexQuotaWindowItem;
pub use validate::{
    KeyQuotaInfo, ModelApplyRequest, ModelDetectResult, UpstreamTestRequest, UpstreamTestResult,
    validate_deepseek_key, validate_upstream_key,
};

/// Constant-time string comparison to prevent timing side-channel attacks on secret keys.
///
/// Returns `true` only if both strings have the same length and identical byte content.
/// The comparison time depends only on the length of `b`, not on the content.
#[inline]
pub fn constant_time_eq_str(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && a.as_bytes().ct_eq(b.as_bytes()).into()
}
