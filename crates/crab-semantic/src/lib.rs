mod cache;
mod embedder;
mod gate;
mod pool;
mod store;

#[cfg(test)]
pub mod mock;

pub use cache::SemanticCache;
pub use embedder::Embedder;
pub use gate::{GateDecision, SemanticGateConfig, evaluate_semantic_gate};
pub use pool::EmbedderPool;
pub use store::VectorStore;
