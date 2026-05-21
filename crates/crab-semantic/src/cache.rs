use crate::pool::EmbedderPool;
use crate::store::VectorStore;
use anyhow::Result;
use crab_cache::CacheEntry;
use crab_metrics::global_metrics;
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

pub struct SemanticCache {
    pool: Arc<EmbedderPool>,
    store: VectorStore,
    threshold: f32,
    ttl_secs: u64,
}

impl SemanticCache {
    pub async fn new(
        pool: Arc<EmbedderPool>,
        store: VectorStore,
        threshold: f32,
        ttl_secs: u64,
    ) -> Result<Self> {
        store.ensure_collection().await?;

        Ok(Self {
            pool,
            store,
            threshold,
            ttl_secs,
        })
    }

    pub async fn search(&self, query_text: &str, tenant_id: Option<&str>) -> Option<CacheEntry> {
        let start = Instant::now();
        let vector = self.pool.embed(query_text).await.ok()?;
        let elapsed = start.elapsed();
        global_metrics().record_semantic_embed_latency(elapsed);

        let result = self
            .store
            .search(&vector, self.threshold, tenant_id)
            .await;

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

    pub async fn insert(
        &self,
        query_text: &str,
        entry: &CacheEntry,
        tenant_id: Option<&str>,
    ) -> Result<()> {
        let start = Instant::now();
        let vector = self.pool.embed(query_text).await?;
        let elapsed = start.elapsed();
        global_metrics().record_semantic_embed_latency(elapsed);

        let tenant = tenant_id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(crate::store::DEFAULT_TENANT_ID);
        let id = simple_hash(&format!("{tenant}:{query_text}"));

        self.store
            .upsert(&id, &vector, entry, self.ttl_secs, tenant_id)
            .await?;

        debug!(
            query_len = query_text.len(),
            id = %id,
            "Entry inserted into semantic cache"
        );

        Ok(())
    }
}

fn simple_hash(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}
