use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },

    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("invalid response: {0}")]
    InvalidResponse(String),
}
