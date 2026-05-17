mod cache;
mod embedder;
mod gate;
mod pool;
mod store;

#[cfg(test)]
pub mod mock;

pub use cache::SemanticCache;
pub use embedder::Embedder;
pub use gate::{evaluate_semantic_gate, GateDecision, SemanticGateConfig};
pub use pool::EmbedderPool;
pub use store::VectorStore;
