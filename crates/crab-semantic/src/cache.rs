use crate::embedder::Embedder;
use crate::store::VectorStore;
use anyhow::Result;
use crab_cache::CacheEntry;
use crab_metrics::global_metrics;
use std::sync::Arc;
use tracing::debug;

pub struct SemanticCache {
    embedder: Arc<Embedder>,
    store: VectorStore,
    threshold: f32,
    ttl_secs: u64,
}

impl SemanticCache {
    pub async fn new(
        embedder: Arc<Embedder>,
        store: VectorStore,
        threshold: f32,
        ttl_secs: u64,
    ) -> Result<Self> {
        store.ensure_collection().await?;

        Ok(Self {
            embedder,
            store,
            threshold,
            ttl_secs,
        })
    }

    pub async fn search(&self, query_text: &str) -> Option<CacheEntry> {
        let vector = self.embedder.embed(query_text).await.ok()?;

        let result = self.store.search(&vector, self.threshold).await;

        if result.is_some() {
            global_metrics().record_semantic_cache_hit(true);
            debug!(
                query_len = query_text.len(),
                threshold = self.threshold,
                "Semantic cache hit"
            );
        } else {
            global_metrics().record_semantic_cache_hit(false);
            debug!(
                query_len = query_text.len(),
                threshold = self.threshold,
                "Semantic cache miss"
            );
        }

        result
    }

    pub async fn insert(&self, query_text: &str, entry: &CacheEntry) -> Result<()> {
        let vector = self.embedder.embed(query_text).await?;

        let id = simple_hash(query_text);

        self.store.upsert(&id, &vector, entry, self.ttl_secs).await?;

        debug!(
            query_len = query_text.len(),
            id = %id,
            "Entry inserted into semantic cache"
        );

        Ok(())
    }
}

fn simple_hash(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
