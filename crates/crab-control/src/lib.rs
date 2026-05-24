mod backends;
mod client;
mod error;
mod types;
mod upstream_url;
mod validate;

pub use backends::parse_backend_endpoints;
pub use client::GatewayAdminClient;
pub use error::ControlError;
pub use types::*;
pub use upstream_url::{UpstreamBaseUrl, parse_upstream_base_url};
pub use validate::{
    ModelApplyRequest, ModelDetectResult, UpstreamTestRequest, UpstreamTestResult,
    validate_deepseek_key, validate_upstream_key,
};
