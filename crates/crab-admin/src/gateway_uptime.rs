//! Helpers to detect Gateway process restarts via Management `/v1/status` uptime.

use crate::state::AppState;

/// Latest gateway uptime from the probe cache (`None` if not yet probed).
pub fn current_gateway_uptime_secs(state: &AppState) -> Option<u64> {
    state
        .gateway_probe_cache
        .read()
        .as_ref()
        .and_then(|(_, p)| p.status.as_ref().map(|s| s.uptime_secs))
}

/// `true` when uptime decreased (new Gateway process / container restart).
pub fn detect_gateway_restart(prev: Option<u64>, curr: Option<u64>) -> bool {
    matches!((prev, curr), (Some(prev), Some(curr)) if curr < prev)
}
