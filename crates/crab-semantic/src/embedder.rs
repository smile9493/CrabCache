use crate::EmbedError;
use ort::value::Tensor;
use tokenizers::Tokenizer;
use tracing::debug;

pub struct Embedder {
    model_path: String,
    tokenizer: Tokenizer,
}

impl Embedder {
    pub fn load(model_path: &str, tokenizer_path: &str) -> Result<Self, EmbedError> {
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| EmbedError::Tokenization(format!("Failed to load tokenizer: {e}")))?;

        debug!(model_path = model_path, "Embedder model path configured");

        Ok(Self {
            model_path: model_path.to_string(),
            tokenizer,
        })
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
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
        let hidden_size = 384usize;

        let model_path = self.model_path.clone();
        let mask_for_pool = attention_mask.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<Vec<f32>, EmbedError> {
            let mut session = ort::session::Session::builder()?.commit_from_file(&model_path)?;

            let shape = vec![1i64, seq_len as i64];

            let inputs = ort::inputs![
                Tensor::from_array((shape.clone(), input_ids))
                    .map_err(|e| EmbedError::Tensor(e.to_string()))?,
                Tensor::from_array((shape.clone(), attention_mask))
                    .map_err(|e| EmbedError::Tensor(e.to_string()))?,
                Tensor::from_array((shape, token_type_ids))
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

            for val in mean_pool.iter_mut() {
                *val /= count;
            }

            let norm: f32 = mean_pool.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for val in mean_pool.iter_mut() {
                    *val /= norm;
                }
            }

            Ok(mean_pool)
        })
        .await
        .map_err(|_| EmbedError::Cancelled)??;

        debug!(
            text_len = text.len(),
            vector_dim = result.len(),
            "Text embedded"
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedder_creation_requires_files() {
        let result = Embedder::load("/nonexistent/model.onnx", "/nonexistent/tokenizer.json");
        assert!(result.is_err());
    }
}
