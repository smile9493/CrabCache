//! Background task that aggregates trace logs into model_peak_hours.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::state::AppState;
use crate::trace_log::TraceLogEntry;

/// Interval between aggregation runs.
const AGGREGATE_INTERVAL_SECS: u64 = 300; // 5 minutes

/// Run the peak hours aggregation loop.
pub async fn run(state: Arc<AppState>) {
    // Wait until PG is available (may initialize shortly after startup).
    for attempt in 0..30 {
        if state.pg_store.read().is_some() {
            break;
        }
        if attempt == 0 {
            tracing::info!("Model peak hours aggregator waiting for PostgreSQL");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    loop {
        if let Err(e) = aggregate_once(&state).await {
            tracing::warn!(error = %e, "Peak hours aggregation failed");
        }
        tokio::time::sleep(Duration::from_secs(AGGREGATE_INTERVAL_SECS)).await;
    }
}

fn aggregate_entries(
    entries: &[TraceLogEntry],
    since_ms: i64,
) -> Vec<(String, i64, i64, i64, i64)> {
    let mut buckets: HashMap<(String, i64), (i64, i64, i64)> = HashMap::new();
    for e in entries {
        let ts = e.timestamp_ms as i64;
        if ts < since_ms {
            continue;
        }
        let hour_bucket = (ts / 3_600_000) * 3_600_000;
        let inp = e.resolved_input_tokens() as i64;
        let out = e.resolved_output_tokens() as i64;
        let slot = buckets
            .entry((e.model.clone(), hour_bucket))
            .or_insert((0, 0, 0));
        slot.0 += 1;
        slot.1 += inp;
        slot.2 += out;
    }
    buckets
        .into_iter()
        .map(|((model, hour_bucket), (count, inp, out))| (model, hour_bucket, count, inp, out))
        .collect()
}

/// Single aggregation pass: read trace JSONL, GROUP BY model+hour, upsert to PG.
async fn aggregate_once(state: &AppState) -> anyhow::Result<()> {
    let pg = {
        let guard = state.pg_store.read();
        guard.clone()
    };
    let Some(pg) = pg else {
        tracing::debug!("Model peak hours aggregation skipped: PostgreSQL not ready");
        return Ok(());
    };

    let watermark = pg.peak_hours_watermark().await.unwrap_or(None);
    let since_ms: i64 = watermark.map(|w| w - 7_200_000).unwrap_or(0).max(0);

    // Read from PG trace_logs (authoritative source).
    let entries = crate::trace_log::load_trace_entries(&pg, 0).await;
    let agg_rows = aggregate_entries(&entries, since_ms);

    if agg_rows.is_empty() {
        tracing::info!(
            entries = entries.len(),
            since_ms,
            "Model peak hours aggregation: no rows to upsert"
        );
        return Ok(());
    }

    pg.upsert_model_peak_hours(&agg_rows).await?;
    tracing::info!(
        rows = agg_rows.len(),
        entries = entries.len(),
        since_ms,
        "Model peak hours aggregation completed"
    );
    Ok(())
}
