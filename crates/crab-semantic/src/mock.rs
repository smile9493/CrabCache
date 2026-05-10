use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Deterministic embedder for testing without ONNX model files.
///
/// This mock generates stable 384-dimensional unit vectors based on text content hash.
/// Same text always produces the same vector, and semantically similar texts
/// (sharing hash prefixes) will have higher cosine similarity.
pub struct DeterministicEmbedder;

impl DeterministicEmbedder {
    pub fn new() -> Self {
        Self
    }

    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();

        let mut vec = Self::seeded_vector(seed, 384);

        // L2 normalize
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            vec.iter_mut().for_each(|x| *x /= norm);
        }

        Ok(vec)
    }

    /// Generate a deterministic vector from a u64 seed using xorshift.
    fn seeded_vector(seed: u64, dim: usize) -> Vec<f32> {
        let mut state = seed;
        (0..dim)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state as f32) / (u64::MAX as f32)) * 2.0 - 1.0
            })
            .collect()
    }
}

impl Default for DeterministicEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deterministic_embed_same_text() {
        let embedder = DeterministicEmbedder::new();
        let v1 = embedder.embed("hello world").await.unwrap();
        let v2 = embedder.embed("hello world").await.unwrap();
        assert_eq!(v1, v2);
    }

    #[tokio::test]
    async fn test_different_text_different_vector() {
        let embedder = DeterministicEmbedder::new();
        let v1 = embedder.embed("hello world").await.unwrap();
        let v2 = embedder.embed("goodbye world").await.unwrap();
        assert_ne!(v1, v2);
    }

    #[tokio::test]
    async fn test_vector_is_unit_length() {
        let embedder = DeterministicEmbedder::new();
        let v = embedder.embed("test").await.unwrap();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[tokio::test]
    async fn test_dimension_is_384() {
        let embedder = DeterministicEmbedder::new();
        let v = embedder.embed("test").await.unwrap();
        assert_eq!(v.len(), 384);
    }
}
