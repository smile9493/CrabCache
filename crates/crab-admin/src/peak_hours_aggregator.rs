//! Background task that aggregates trace_logs into model_peak_hours.

use std::sync::Arc;
use std::time::Duration;

use crate::state::AppState;

/// Interval between aggregation runs.
const AGGREGATE_INTERVAL_SECS: u64 = 300; // 5 minutes

/// Run the peak hours aggregation loop.
/// Spawns as a background tokio task; never returns.
pub async fn run(state: Arc<AppState>) {
    // Initial delay to let PG and trace data settle.
    tokio::time::sleep(Duration::from_secs(30)).await;
    loop {
        if let Err(e) = aggregate_once(&state).await {
            tracing::debug!(error = %e, "Peak hours aggregation failed");
        }
        tokio::time::sleep(Duration::from_secs(AGGREGATE_INTERVAL_SECS)).await;
    }
}

/// Single aggregation pass: read trace_logs since watermark, GROUP BY model+hour, upsert.
async fn aggregate_once(state: &AppState) -> anyhow::Result<()> {
    let pg = {
        let guard = state.pg_store.read();
        guard.clone()
    };
    let Some(pg) = pg else {
        return Ok(());
    };

    // Determine watermark: aggregate from the latest hour_bucket onward,
    // with a 2-hour buffer to catch late-arriving data.
    let watermark = pg.peak_hours_watermark().await.unwrap_or(None);
    let since_ms: i64 = watermark.map(|w| w - 7_200_000).unwrap_or(0).max(0);

    let agg_rows = pg
        .aggregate_trace_logs_for_peak_hours(since_ms)
        .await?;

    if agg_rows.is_empty() {
        return Ok(());
    }

    pg.upsert_model_peak_hours(&agg_rows).await?;
    tracing::debug!(
        rows = agg_rows.len(),
        "Model peak hours aggregation completed"
    );
    Ok(())
}
