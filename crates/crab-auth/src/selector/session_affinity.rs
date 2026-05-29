use super::availability::is_blocked_for_model;
use super::session_cache::SessionCache;
use super::{AuthEntry, SelectionContext, Selector};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::time::Duration;

/// Session affinity selector: decorates another selector with session-sticky behavior.
/// Same session always gets the same auth (if available).
pub struct SessionAffinitySelector {
    cache: RwLock<SessionCache>,
    fallback: Box<dyn Selector>,
}

impl SessionAffinitySelector {
    pub fn new(fallback: Box<dyn Selector>, ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(SessionCache::new(ttl)),
            fallback,
        }
    }

    /// Extract session ID from request context.
    pub fn extract_session_id(ctx: &SelectionContext) -> Option<String> {
        ctx.session_id.clone()
    }

    /// Build cache key: `"provider::sessionID::model"`
    fn cache_key(provider: &str, session_id: &str, model: &str) -> String {
        format!("{}::{}::{}", provider, session_id, model)
    }
}

#[async_trait]
impl Selector for SessionAffinitySelector {
    async fn pick(&self, ctx: &SelectionContext, auths: &[AuthEntry]) -> Option<usize> {
        let session_id = match Self::extract_session_id(ctx) {
            Some(id) => id,
            None => return self.fallback.pick(ctx, auths).await,
        };

        let key = Self::cache_key(&ctx.provider, &session_id, &ctx.model);

        {
            let cache = self.cache.read();
            if let Some(auth_id) = cache.get(&key) {
                if let Some(idx) = auths.iter().position(|a| {
                    a.record.id == auth_id && is_blocked_for_model(a, &ctx.model).is_none()
                }) {
                    return Some(idx);
                }
            }
        }

        let selected = self.fallback.pick(ctx, auths).await;

        if let Some(idx) = selected {
            let auth_id = auths[idx].record.id.clone();
            self.cache.write().insert(key, auth_id);
        }

        selected
    }

    fn mark_result(
        &self,
        auth_idx: usize,
        auth: &AuthEntry,
        success: bool,
        status_code: Option<u16>,
    ) {
        if !success && status_code == Some(429) {
            self.cache.write().invalidate_auth(&auth.record.id);
        }
        self.fallback.mark_result(auth_idx, auth, success, status_code);
    }
}
