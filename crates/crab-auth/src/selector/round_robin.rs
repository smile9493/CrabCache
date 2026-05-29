use super::{AuthEntry, SelectionContext, Selector, availability::is_blocked_for_model};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;

/// Round-robin selector with priority awareness and per-provider-model cursors.
///
/// Algorithm:
/// 1. Filter auths by availability (`is_blocked_for_model`)
/// 2. Group by priority, select highest priority bucket
/// 3. Within bucket: round-robin using per-provider-model cursor
pub struct RoundRobinSelector {
    cursors: RwLock<HashMap<String, usize>>,
    max_keys: usize,
}

impl RoundRobinSelector {
    pub fn new() -> Self {
        Self {
            cursors: RwLock::new(HashMap::new()),
            max_keys: 1024,
        }
    }

    pub fn with_max_keys(max_keys: usize) -> Self {
        Self {
            cursors: RwLock::new(HashMap::new()),
            max_keys,
        }
    }
}

impl Default for RoundRobinSelector {
    fn default() -> Self {
        Self::new()
    }
}

const CURSOR_WRAP: usize = usize::MAX / 2;

#[async_trait]
impl Selector for RoundRobinSelector {
    async fn pick(&self, ctx: &SelectionContext, auths: &[AuthEntry]) -> Option<usize> {
        if auths.is_empty() {
            return None;
        }

        let mut available: Vec<(usize, &AuthEntry)> = auths
            .iter()
            .enumerate()
            .filter(|(_, a)| is_blocked_for_model(a, &ctx.model).is_none())
            .collect();

        if available.is_empty() {
            return None;
        }

        let max_priority = available.iter().map(|(_, a)| a.priority).max().unwrap_or(0);
        available.retain(|(_, a)| a.priority == max_priority);

        available.sort_by(|a, b| a.1.record.id.cmp(&b.1.record.id));

        let cursor_key = format!(
            "{}:{}",
            ctx.provider,
            super::availability::canonical_model_key(&ctx.model)
        );
        let len = available.len();

        let mut cursors = self.cursors.write();

        if cursors.len() >= self.max_keys {
            cursors.clear();
        }

        let cursor = cursors.entry(cursor_key.clone()).or_insert(0);
        let idx = *cursor % len;
        *cursor = (*cursor + 1) % CURSOR_WRAP;

        let (original_idx, _) = available[idx];
        Some(original_idx)
    }

    fn mark_result(
        &self,
        _auth_idx: usize,
        _auth: &AuthEntry,
        _success: bool,
        _status_code: Option<u16>,
    ) {
    }
}
