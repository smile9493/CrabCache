//! Periodic sync of Keys monthly usage from trace log entries.
//!
//! This background task periodically reads recent trace entries from
//! `trace.jsonl` and accumulates `resolved_input_tokens()` +
//! `resolved_output_tokens()` against `keys_meta` entries whose `name`
//! (== trace `consumer`) matches.
//!
//! The accumulated counts are written back to `admin-state.json` so they
//! survive `crab-admin` restarts.

use crate::state::AppState;
use crate::trace_log;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

/// Interval between sync ticks (environment variable, default 60).
const SYNC_INTERVAL_ENV: &str = "CRABCACHE_KEY_USAGE_SYNC_INTERVAL_SECS";

/// How far back (in seconds) we scan trace log each tick to find new entries.
const SCAN_WINDOW_SECS: u64 = 120;

pub fn sync_interval_secs() -> u64 {
    std::env::var(SYNC_INTERVAL_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
}

/// Spawn the background usage sync loop.  Called once at startup.
pub fn spawn(state: Arc<AppState>) {
    let interval_secs = sync_interval_secs();
    if interval_secs == 0 {
        tracing::info!("Key usage sync disabled (interval = 0)");
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            if let Err(e) = sync_once(&state).await {
                debug!(error = %e, "Key usage sync failed");
            }
        }
    });
    tracing::info!(interval_secs, "Key usage sync started");
}

async fn sync_once(state: &Arc<AppState>) -> Result<(), String> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let last_synced = *state.key_usage_last_synced.lock();

    // Determine cutoff: if never synced, scan last SCAN_WINDOW_SECS.
    let cutoff = if last_synced > 0 {
        last_synced
    } else {
        now_ms.saturating_sub(SCAN_WINDOW_SECS * 1000)
    };

    let pg_opt = { state.pg_store.read().clone() };
    let entries: Vec<trace_log::TraceLogEntry> = if let Some(ref pg) = pg_opt {
        trace_log::load_trace_entries(pg, 24).await
    } else {
        return Ok(());
    };

    // Build consumer→id lookup once to avoid O(keys) scan per entry.
    let consumer_to_ids: std::collections::HashMap<String, Vec<String>> = {
        use std::collections::HashMap;
        let mut m: HashMap<String, Vec<String>> = HashMap::new();
        for kv in state.keys_meta.iter() {
            let id = kv.key().clone();
            if !kv.value().name.is_empty() {
                m.entry(kv.value().name.clone())
                    .or_default()
                    .push(id.clone());
            }
            if !kv.value().token.is_empty() && kv.value().token != kv.value().name {
                m.entry(kv.value().token.clone()).or_default().push(id);
            }
        }
        m
    };
    // Determine current month key.
    let usage_month = usage_month_key(now_ms);

    let mut latest_ts = last_synced;
    let mut changed = false;

    // Match consumer -> keys_meta name and accumulate.
    for entry in entries.iter() {
        if entry.timestamp_ms <= cutoff {
            continue;
        }
        if entry.timestamp_ms > latest_ts {
            latest_ts = entry.timestamp_ms;
        }

        let consumer = match entry.consumer.as_deref() {
            Some(c) if !c.is_empty() => c,
            _ => continue,
        };

        let tokens = entry.resolved_input_tokens() + entry.resolved_output_tokens();
        if tokens == 0 {
            continue;
        }

        // O(1) lookup instead of scanning all keys.
        if let Some(ids) = consumer_to_ids.get(consumer) {
            for id in ids {
                if let Some(mut meta) = state.keys_meta.get_mut(id) {
                    if meta.usage_month != usage_month {
                        meta.tokens_this_month = 0;
                        meta.input_tokens = 0;
                        meta.output_tokens = 0;
                        meta.usage_month = usage_month.clone();
                    }
                    meta.tokens_this_month += tokens;
                    meta.input_tokens += entry.resolved_input_tokens();
                    meta.output_tokens += entry.resolved_output_tokens();
                    changed = true;
                }
            }
        }
    }

    if latest_ts > last_synced {
        *state.key_usage_last_synced.lock() = latest_ts;
    }

    if changed {
        state.flush_persist();
    }

    Ok(())
}

/// Returns a stable key for the month containing `ts_ms` (e.g. `"2025-05"`).
fn usage_month_key(ts_ms: u64) -> String {
    let secs = ts_ms / 1000;
    if let Some(dt) = chrono::DateTime::from_timestamp(secs as i64, 0) {
        dt.format("%Y-%m").to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_month_key() {
        // 2025-05-01 00:00:00 UTC
        let ts = 1746057600_000;
        let month = usage_month_key(ts);
        assert_eq!(month, "2025-05");
    }
}
