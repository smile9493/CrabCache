//! Stale-while-revalidate: serve stale cache entries immediately and spawn a
//! background Pingora `Subrequest` to refresh the cache from upstream.
//!
//! The subrequest re-enters the full `ProxyHttp` pipeline. A marker type
//! (`RevalidationMarker`) is embedded in the `SubrequestCtx.user_ctx` so that
//! `request_filter` can detect the revalidation subrequest and skip cache
//! lookup, forcing an upstream fetch that writes fresh data back to cache.

use pingora_proxy::subrequest::{BodyMode, Ctx as SubrequestCtx};
use std::any::Any;
use tracing::{debug, info, warn};

use bytes::Bytes;

/// Marker type embedded in `SubrequestCtx.user_ctx` to identify revalidation
/// subrequests. When `request_filter` detects this marker, it skips cache
/// lookup and proceeds directly to upstream.
#[derive(Debug)]
pub struct RevalidationMarker {
    /// The cache key that should be refreshed.
    pub cache_key: String,
}

impl RevalidationMarker {
    /// Attempt to downcast a `UserCtx` to a `RevalidationMarker` reference.
    pub fn from_user_ctx(ctx: &dyn Any) -> Option<&Self> {
        ctx.downcast_ref::<RevalidationMarker>()
    }
}

/// Check if the current session is a revalidation subrequest.
///
/// Returns `Some(&RevalidationMarker)` if this is a revalidation subrequest
/// with our marker in `user_ctx`, `None` otherwise.
pub fn is_revalidation_subrequest(session: &pingora_proxy::Session) -> Option<&RevalidationMarker> {
    let sub_ctx = session.subrequest_ctx.as_ref()?;
    let user_ctx = sub_ctx.user_ctx()?;
    RevalidationMarker::from_user_ctx(user_ctx.as_ref())
}

/// Spawn a background subrequest to revalidate a stale cache entry.
///
/// Uses `create_subrequest` so we can send the original request body through
/// the subrequest handle before running it. LLM APIs require a JSON body —
/// without it, the upstream would return 400/422.
///
/// The subrequest re-enters the full `ProxyHttp` pipeline, inheriting the
/// original session's headers (including `Authorization`) and path. The
/// `RevalidationMarker` in `user_ctx` tells `request_filter` to skip cache
/// lookup and force an upstream fetch.
pub fn spawn_cache_revalidation(
    session: &pingora_proxy::Session,
    cache_key: &str,
    body_bytes: &[u8],
) {
    let spawner = match &session.subrequest_spawner {
        Some(s) => s,
        None => {
            debug!(
                cache_key = %cache_key,
                "Cannot spawn revalidation: subrequest_spawner not available"
            );
            return;
        }
    };

    let marker = RevalidationMarker {
        cache_key: cache_key.to_string(),
    };

    let ctx = SubrequestCtx::builder()
        .body_mode(BodyMode::ExpectBody)
        .user_ctx(Box::new(marker) as Box<dyn Any + Sync + Send>)
        .build();

    let (prepared, handle) = spawner.create_subrequest(session.as_downstream(), ctx);

    let body = Bytes::from(body_bytes.to_vec());
    let cache_key = cache_key.to_string();
    let cache_key_for_task = cache_key.clone();

    tokio::spawn(async move {
        // Send the original request body through the subrequest handle so that
        // the subrequest session can read it as its downstream body.
        if let Err(e) = handle
            .tx
            .send(pingora_core::protocols::http::HttpTask::Body(
                Some(body),
                true,
            ))
            .await
        {
            warn!(
                cache_key = %cache_key_for_task,
                error = %e,
                "Failed to send body to revalidation subrequest"
            );
            return;
        }

        // Run the subrequest through the full proxy pipeline.
        prepared.run().await;

        info!(
            cache_key = %cache_key_for_task,
            "Background cache revalidation subrequest completed"
        );
    });

    info!(
        cache_key = %cache_key,
        body_len = body_bytes.len(),
        "Spawned background cache revalidation subrequest"
    );
}

/// Convenience helper: if the cache entry is stale, spawn a background
/// revalidation subrequest. Call this after every successful cache hit.
pub fn maybe_spawn_swr(
    session: &pingora_proxy::Session,
    cache_key: &str,
    entry: &crab_cache::CacheEntry,
    body_bytes: &[u8],
) {
    // Only revalidate if the entry has actually exceeded its TTL.
    if !entry.is_stale() {
        return;
    }

    // Cap revalidation: don't bother if the entry is extremely old (e.g. > 24h).
    // At that point the cache key is likely from a different conversation.
    if entry.stale_age_secs() > 86400 {
        return;
    }

    spawn_cache_revalidation(session, cache_key, body_bytes);
}
