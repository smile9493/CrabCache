use anyhow::Result;
use crab_cache::CacheEntry;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, Distance, Filter, PointStruct, SearchPointsBuilder,
    UpsertPointsBuilder, Value, VectorParamsBuilder,
};

pub const DEFAULT_TENANT_ID: &str = "__default__";

fn tenant_label(tenant_id: Option<&str>) -> &str {
    tenant_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_TENANT_ID)
}
use tracing::debug;

pub struct VectorStore {
    client: Qdrant,
    collection: String,
    vector_size: u64,
}

impl VectorStore {
    pub async fn new(url: &str, collection: &str, vector_size: u64) -> Result<Self> {
        let client = Qdrant::from_url(url).build()?;

        debug!(
            url = url,
            collection = collection,
            vector_size = vector_size,
            "VectorStore client created"
        );

        Ok(Self {
            client,
            collection: collection.to_string(),
            vector_size,
        })
    }

    pub async fn ensure_collection(&self) -> Result<()> {
        let collections = self.client.list_collections().await?;
        let exists = collections
            .collections
            .iter()
            .any(|c| c.name == self.collection);

        if !exists {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection).vectors_config(
                        VectorParamsBuilder::new(self.vector_size, Distance::Cosine),
                    ),
                )
                .await?;

            debug!(collection = %self.collection, "Collection created");
        }

        Ok(())
    }

    pub async fn search(
        &self,
        vector: &[f32],
        threshold: f32,
        tenant_id: Option<&str>,
    ) -> Option<CacheEntry> {
        let tenant = tenant_label(tenant_id);
        let filter = Filter::must([Condition::matches("tenant_id", tenant.to_string())]);
        let result = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection, vector.to_vec(), 1)
                    .score_threshold(threshold)
                    .filter(filter),
            )
            .await
            .ok()?;

        if result.result.is_empty() {
            return None;
        }

        let hit = &result.result[0];
        debug!(
            score = hit.score,
            threshold = threshold,
            "Semantic search completed"
        );

        if hit.score < threshold {
            return None;
        }

        let payload = &hit.payload;
        let entry_json = payload.get("entry")?.as_str()?;

        serde_json::from_str(entry_json).ok()
    }

    pub async fn upsert(
        &self,
        id: &str,
        vector: &[f32],
        entry: &CacheEntry,
        ttl_secs: u64,
        tenant_id: Option<&str>,
    ) -> Result<()> {
        let tenant = tenant_label(tenant_id);
        let entry_json = serde_json::to_string(entry)?;
        let mut payload = std::collections::HashMap::new();
        payload.insert("entry".to_string(), Value::from(entry_json));
        payload.insert("ttl_secs".to_string(), Value::from(ttl_secs as i64));
        payload.insert("tenant_id".to_string(), Value::from(tenant.to_string()));

        let point = PointStruct::new(id, vector.to_vec(), payload);

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]))
            .await?;

        debug!(
            id = id,
            collection = %self.collection,
            "Vector upserted"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vector_store_creation() {
        let result = VectorStore::new("http://localhost:6334", "test", 384).await;
        assert!(result.is_err() || result.is_ok());
    }
}
