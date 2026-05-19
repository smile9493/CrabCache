use anyhow::Result;
use ort::session::Session;
use ort::value::Tensor;
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;
use tokio::sync::Semaphore;
use tracing::debug;

pub struct EmbedderPool {
    model_path: String,
    tokenizer: Tokenizer,
    session: Arc<Mutex<Session>>,
    semaphore: Arc<Semaphore>,
    hidden_size: usize,
}

impl EmbedderPool {
    pub fn load(model_path: &str, tokenizer_path: &str, max_concurrent: usize) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {e}"))?;

        let session = Session::builder()?.commit_from_file(model_path)?;

        debug!(
            model_path = model_path,
            max_concurrent = max_concurrent,
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

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let _permit = self.semaphore.acquire().await?;

        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {e}"))?;

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
        let result = tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
            let mut session = session
                .lock()
                .map_err(|e| anyhow::anyhow!("Session lock poisoned: {e}"))?;

            let shape = vec![1i64, seq_len as i64];

            let inputs = ort::inputs![
                Tensor::from_array((shape.clone(), input_ids.clone()))?,
                Tensor::from_array((shape.clone(), attention_mask.clone()))?,
                Tensor::from_array((shape.clone(), token_type_ids.clone()))?,
            ];

            let outputs = session.run(inputs)?;

            let output_view = outputs[0].try_extract_tensor::<f32>()?;
            let data = output_view.1;

            let mut mean_pool = vec![0.0f32; hidden_size];
            let mut count = 0.0f32;

            for s in 0..seq_len {
                if attention_mask[s] > 0 {
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
        .await??;

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
            EmbedderPool::load("/nonexistent/model.onnx", "/nonexistent/tokenizer.json", 4);
        assert!(result.is_err());
    }
}
