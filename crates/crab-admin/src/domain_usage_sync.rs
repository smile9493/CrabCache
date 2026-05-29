//! Background sync for domain_usage counters.
//!
//! Periodically fetches domain usage from the Gateway Management API,
//! persists it to PostgreSQL, and restores it after a Gateway restart
//! (detected via uptime regression in `gateway_probe_cache`).

use crate::state::AppState;
use crab_control::PutDomainUsageRequest;
use std::sync::Arc;
use tracing::{debug, info};

/// Interval between sync ticks (env var, default 60s).
const SYNC_INTERVAL_ENV: &str = "CRABCACHE_DOMAIN_USAGE_SYNC_INTERVAL_SECS";

pub fn sync_interval_secs() -> u64 {
    std::env::var(SYNC_INTERVAL_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
}

/// Spawn the background domain_usage sync loop. Called once at startup.
pub fn spawn(state: Arc<AppState>) {
    let interval_secs = sync_interval_secs();
    if interval_secs == 0 {
        info!("Domain usage sync disabled (interval = 0)");
        return;
    }
    tokio::spawn(async move {
        let mut prev_uptime: Option<u64> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            if let Err(e) = sync_once(&state, &mut prev_uptime).await {
                debug!(error = %e, "Domain usage sync failed");
            }
        }
    });
    info!(interval_secs, "Domain usage sync started");
}

async fn sync_once(state: &Arc<AppState>, prev_uptime: &mut Option<u64>) -> Result<(), String> {
    let pg = state.pg_store.read().clone().ok_or("PG not available")?;

    // 1. Fetch current domain usage from Gateway.
    let resp = state
        .gateway
        .get_domain_usage()
        .await
        .map_err(|e| format!("get_domain_usage: {e}"))?;

    // 2. Detect Gateway restart (uptime went down).
    let current_uptime = state
        .gateway_probe_cache
        .read()
        .as_ref()
        .and_then(|(_, p)| p.status.as_ref().map(|s| s.uptime_secs));

    if let (Some(prev), Some(curr)) = (*prev_uptime, current_uptime) {
        if curr < prev {
            info!(
                prev_uptime = prev,
                curr_uptime = curr,
                "Gateway restart detected — restoring domain_usage from PG"
            );
            restore_to_gateway(state, &pg, &resp.month).await?;
        }
    }
    *prev_uptime = current_uptime;

    // 3. Persist fetched usage to PG.
    let mut upserted = 0u64;
    for entry in &resp.usage {
        if entry.tokens == 0 && entry.spend_usd <= 0.0 {
            continue;
        }
        pg.upsert_domain_usage(&entry.domain, &resp.month, entry.tokens, entry.spend_usd)
            .await
            .map_err(|e| format!("upsert_domain_usage: {e}"))?;
        upserted += 1;
    }
    if upserted > 0 {
        debug!(upserted, month = %resp.month, "Domain usage persisted to PG");
    }

    // 4. Aggregate consumer usage from trace_logs for the current month.
    match pg.aggregate_consumer_usage(&resp.month).await {
        Ok(n) => {
            if n > 0 {
                debug!(month = %resp.month, consumers = n, "Consumer usage aggregated from trace_logs");
            }
        }
        Err(e) => {
            debug!(error = %e, "Consumer usage aggregation failed");
        }
    }

    // 5. Prune old months (keep last 3 months).
    if let Ok(current_date) = chrono::Utc::now()
        .format("%Y-%m")
        .to_string()
        .parse::<chrono::NaiveDate>()
    {
        let cutoff = (current_date - chrono::Duration::days(90))
            .format("%Y-%m")
            .to_string();
        if let Ok(pruned) = pg.prune_domain_usage(&cutoff).await {
            if pruned > 0 {
                debug!(pruned, "Old domain_usage records pruned");
            }
        }
    }

    Ok(())
}

/// Restore domain_usage from PG into the Gateway (only the month not yet present on Gateway).
async fn restore_to_gateway(
    state: &Arc<AppState>,
    pg: &crate::pg::PgStore,
    current_month: &str,
) -> Result<(), String> {
    let db_usage = pg
        .load_domain_usage(current_month)
        .await
        .map_err(|e| format!("load_domain_usage: {e}"))?;

    if db_usage.is_empty() {
        debug!("No persisted domain_usage to restore for month {current_month}");
        return Ok(());
    }

    // Merge: start from Gateway's current snapshot, then overlay PG values
    // (PG is the source of truth after a restart).
    let gw_usage = state
        .gateway
        .get_domain_usage()
        .await
        .map_err(|e| format!("get_domain_usage for merge: {e}"))?;

    let mut merged: std::collections::HashMap<String, crab_control::DomainUsageEntry> =
        std::collections::HashMap::new();

    // Start with Gateway's current counters (may be empty after restart).
    for e in gw_usage.usage {
        merged.insert(e.domain.clone(), e);
    }

    // Overlay PG counters (authoritative for what was persisted).
    for (domain, tokens, spend_usd) in db_usage {
        let entry = merged
            .entry(domain.clone())
            .or_insert(crab_control::DomainUsageEntry {
                domain,
                tokens: 0,
                spend_usd: 0.0,
            });
        entry.tokens = entry.tokens.max(tokens);
        entry.spend_usd = entry.spend_usd.max(spend_usd);
    }

    let entries: Vec<crab_control::DomainUsageEntry> = merged.into_values().collect();

    state
        .gateway
        .put_domain_usage(&PutDomainUsageRequest {
            usage: entries.clone(),
            month: current_month.to_string(),
        })
        .await
        .map_err(|e| format!("put_domain_usage: {e}"))?;

    info!(
        domains = entries.len(),
        month = current_month,
        "Domain usage restored to Gateway from PG"
    );
    Ok(())
}
