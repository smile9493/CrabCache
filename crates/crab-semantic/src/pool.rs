use ort::session::Session;
use ort::value::Tensor;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// Error type for embedder operations.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("tokenization failed: {0}")]
    Tokenization(String),
    #[error("ONNX session error: {0}")]
    Session(String),
    #[error("tensor creation failed: {0}")]
    Tensor(String),
    #[error("session lock poisoned")]
    LockPoisoned,
    #[error("embedding cancelled")]
    Cancelled,
}

impl From<ort::Error> for EmbedError {
    fn from(e: ort::Error) -> Self {
        EmbedError::Session(e.to_string())
    }
}

pub struct EmbedderPool {
    model_path: String,
    tokenizer: Tokenizer,
    session: Arc<Mutex<Session>>,
    semaphore: Arc<Semaphore>,
    hidden_size: usize,
}

impl EmbedderPool {
    /// Load the ONNX model and tokenizer, optionally verifying the model checksum.
    ///
    /// If `expected_sha256` is provided, the model file's SHA-256 hash is verified
    /// before loading to detect tampering or corruption (required for production).
    pub fn load(
        model_path: &str,
        tokenizer_path: &str,
        max_concurrent: usize,
        expected_sha256: Option<&str>,
    ) -> Result<Self, EmbedError> {
        // Verify model file checksum if expected hash is provided (production safety).
        if let Some(expected) = expected_sha256 {
            let model_bytes = std::fs::read(model_path)
                .map_err(|e| EmbedError::Session(
                    format!("Failed to read model for checksum: {e}")
                ))?;
            let actual = {
                let mut hasher = Sha256::new();
                hasher.update(&model_bytes);
                hex::encode(hasher.finalize())
            };
            if !actual.eq_ignore_ascii_case(expected) {
                warn!(
                    model_path = model_path,
                    expected_sha256 = %expected,
                    actual_sha256 = %actual,
                    "ONNX model checksum mismatch — model may be tampered or corrupted"
                );
                return Err(EmbedError::Session(
                    format!("Model checksum mismatch: expected {expected}, got {actual}")
                ));
            }
            info!(
                model_path = model_path,
                "ONNX model checksum verified"
            );
        } else {
            warn!(
                model_path = model_path,
                "Loading ONNX model without checksum verification — not recommended for production"
            );
        }

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| EmbedError::Tokenization(format!("Failed to load tokenizer: {e}")))?;

        let session = Session::builder()?
            .with_intra_threads(4)
            .map_err(|e| EmbedError::Session(e.to_string()))?
            .with_inter_threads(1)
            .map_err(|e| EmbedError::Session(e.to_string()))?
            .commit_from_file(model_path)?;

        debug!(
            model_path = model_path,
            max_concurrent = max_concurrent,
            intra_threads = 4,
            "EmbedderPool model loaded"
        );

        Ok(Self {
            model_path: model_path.to_string(),
            tokenizer,
            session: Arc::new(Mutex::new(session)),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            hidden_size: 384,
        })
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| EmbedError::Cancelled)?;

        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| EmbedError::Tokenization(format!("Tokenization failed: {e}")))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let token_type_ids: Vec<i64> = encoding
            .get_type_ids()
            .iter()
            .map(|&id| id as i64)
            .collect();

        let seq_len = input_ids.len();
        let hidden_size = self.hidden_size;

        let session = self.session.clone();
        // Clone attention_mask for the pooling loop (tensor creation consumes the input)
        let mask_for_pool = attention_mask.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<Vec<f32>, EmbedError> {
            let mut session = session
                .lock()
                .map_err(|_| EmbedError::LockPoisoned)?;

            let shape = vec![1i64, seq_len as i64];

            let inputs = ort::inputs![
                Tensor::from_array((shape.clone(), input_ids))
                    .map_err(|e| EmbedError::Tensor(e.to_string()))?,
                Tensor::from_array((shape.clone(), attention_mask))
                    .map_err(|e| EmbedError::Tensor(e.to_string()))?,
                Tensor::from_array((shape.clone(), token_type_ids))
                    .map_err(|e| EmbedError::Tensor(e.to_string()))?,
            ];

            let outputs = session.run(inputs)?;

            let output_view = outputs[0].try_extract_tensor::<f32>()?;
            let data = output_view.1;

            let mut mean_pool = vec![0.0f32; hidden_size];
            let mut count = 0.0f32;

            for s in 0..seq_len {
                if mask_for_pool[s] > 0 {
                    for h in 0..hidden_size {
                        mean_pool[h] += data[s * hidden_size + h];
                    }
                    count += 1.0;
                }
            }

            for h in 0..hidden_size {
                mean_pool[h] /= count;
            }

            let norm: f32 = mean_pool.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for h in 0..hidden_size {
                    mean_pool[h] /= norm;
                }
            }

            Ok(mean_pool)
        })
        .await
        .map_err(|_| EmbedError::Cancelled)??;

        debug!(
            text_len = text.len(),
            vector_dim = result.len(),
            "Text embedded via pool"
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedder_pool_creation_requires_files() {
        let result =
            EmbedderPool::load("/nonexistent/model.onnx", "/nonexistent/tokenizer.json", 4, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_embedder_pool_rejects_wrong_checksum() {
        let result = EmbedderPool::load(
            "/nonexistent/model.onnx",
            "/nonexistent/tokenizer.json",
            4,
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
        );
        assert!(result.is_err());
    }
}
