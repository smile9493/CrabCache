use super::{AuthEntry, SelectionContext, Selector, availability::is_blocked_for_model};
use async_trait::async_trait;

/// Fill-first selector: always picks the first available auth (sorted by ID).
/// Burns through one account before moving to the next.
/// Useful for staggering rolling-window subscription caps.
pub struct FillFirstSelector;

impl FillFirstSelector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FillFirstSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Selector for FillFirstSelector {
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

        Some(available[0].0)
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
