use crate::types::TokenRecord;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("record not found: {0}")]
    NotFound(String),
    #[error("duplicate id: {0}")]
    Duplicate(String),
}

#[async_trait]
pub trait TokenStore: Send + Sync {
    /// List all stored credentials.
    async fn list(&self) -> Result<Vec<TokenRecord>, StoreError>;
    /// Save (create or update) a credential. Returns the record ID.
    async fn save(&self, record: &TokenRecord) -> Result<String, StoreError>;
    /// Delete a credential by ID.
    async fn delete(&self, id: &str) -> Result<(), StoreError>;
    /// Get a single credential by ID.
    async fn get(&self, id: &str) -> Result<TokenRecord, StoreError>;
}
