use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Duration;

struct SessionEntry {
    auth_id: String,
    expires_at: DateTime<Utc>,
}

/// Simple TTL cache for session-to-auth bindings.
pub struct SessionCache {
    entries: HashMap<String, SessionEntry>,
    ttl: Duration,
}

impl SessionCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    /// Get auth ID for session (does NOT refresh TTL).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).and_then(|e| {
            if e.expires_at > Utc::now() {
                Some(e.auth_id.as_str())
            } else {
                None
            }
        })
    }

    /// Get auth ID for session and refresh TTL.
    pub fn get_and_refresh(&mut self, key: &str) -> Option<String> {
        let now = Utc::now();
        if let Some(entry) = self.entries.get_mut(key)
            && entry.expires_at > now
        {
            entry.expires_at = now + chrono::Duration::from_std(self.ttl).unwrap_or(chrono::Duration::seconds(300));
            return Some(entry.auth_id.clone());
        }
        None
    }

    /// Bind a session to an auth ID.
    pub fn insert(&mut self, key: String, auth_id: String) {
        let expires_at = Utc::now() + chrono::Duration::from_std(self.ttl).unwrap_or(chrono::Duration::seconds(300));
        self.entries.insert(key, SessionEntry { auth_id, expires_at });
    }

    /// Remove all sessions bound to a specific auth ID.
    pub fn invalidate_auth(&mut self, auth_id: &str) {
        self.entries.retain(|_, e| e.auth_id != auth_id);
    }

    /// Remove expired entries.
    pub fn sweep(&mut self) {
        let now = Utc::now();
        self.entries.retain(|_, e| e.expires_at > now);
    }

    /// Number of active entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_cache_basic() {
        let mut cache = SessionCache::new(Duration::from_secs(60));
        cache.insert("s1".into(), "auth_a".into());
        assert_eq!(cache.get("s1"), Some("auth_a"));
        assert_eq!(cache.get("s2"), None);
    }

    #[test]
    fn test_session_cache_ttl_expiry() {
        let mut cache = SessionCache::new(Duration::from_secs(0));
        cache.insert("s1".into(), "auth_a".into());
        std::thread::sleep(Duration::from_millis(10));
        assert!(cache.get("s1").is_none());
    }

    #[test]
    fn test_session_cache_get_and_refresh() {
        let mut cache = SessionCache::new(Duration::from_secs(60));
        cache.insert("s1".into(), "auth_a".into());
        let result = cache.get_and_refresh("s1");
        assert_eq!(result, Some("auth_a".into()));
        assert!(cache.get("s1").is_some());
    }

    #[test]
    fn test_session_cache_invalidate_auth() {
        let mut cache = SessionCache::new(Duration::from_secs(60));
        cache.insert("s1".into(), "auth_a".into());
        cache.insert("s2".into(), "auth_b".into());
        cache.insert("s3".into(), "auth_a".into());

        cache.invalidate_auth("auth_a");
        assert!(cache.get("s1").is_none());
        assert_eq!(cache.get("s2"), Some("auth_b"));
        assert!(cache.get("s3").is_none());
    }

    #[test]
    fn test_session_cache_sweep() {
        let mut cache = SessionCache::new(Duration::from_secs(0));
        cache.insert("s1".into(), "auth_a".into());
        std::thread::sleep(Duration::from_millis(10));
        cache.sweep();
        assert!(cache.get("s1").is_none());
    }
}
