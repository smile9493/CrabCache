use pingora_core::Error as PingoraError;

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("Cache lookup failed: {0}")]
    CacheLookup(#[source] anyhow::Error),

    #[error("Downstream write failed: {0}")]
    DownstreamWrite(#[source] PingoraError),

    #[error("Upstream connection failed: {0}")]
    UpstreamConnect(#[source] PingoraError),

    #[error("JSON serialization/deserialization failed: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Request body parse failed")]
    InvalidBody,

    #[error("Authentication failed")]
    Unauthorized,

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<ProxyError> for PingoraError {
    fn from(err: ProxyError) -> Self {
        *PingoraError::explain(pingora_core::ErrorType::InternalError, format!("{:#}", err))
    }
}
