//! NDJSON debug instrumentation (enabled via `CRABCACHE_DEBUG_LOG_PATH`).

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;
use std::sync::mpsc;

static DEBUG_WRITER: OnceLock<Option<mpsc::SyncSender<String>>> = OnceLock::new();

/// Initialize the debug log writer. Must be called once at startup.
/// Spawns a dedicated writer thread with a persistent file handle.
pub fn init_debug_log(path: Option<&str>) {
    let tx = path.filter(|p| !p.is_empty()).map(|p| {
        let (tx, rx) = mpsc::sync_channel::<String>(4096);
        let path = p.to_string();
        std::thread::Builder::new()
            .name("crab-debug-log".into())
            .spawn(move || {
                let mut f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .expect("debug log open");
                for line in rx {
                    let _ = writeln!(f, "{line}");
                }
            })
            .expect("debug log thread");
        tx
    });
    let _ = DEBUG_WRITER.set(tx);
}

/// True when `CRABCACHE_DEBUG_LOG_PATH` initialized the debug log writer.
#[inline]
pub fn is_debug_agent_log_enabled() -> bool {
    DEBUG_WRITER.get().is_some_and(Option::is_some)
}

/// Append one NDJSON line for debug-mode hypothesis testing. Never log secrets.
pub fn debug_agent_log(
    hypothesis_id: &str,
    location: &str,
    message: &str,
    data: serde_json::Value,
) {
    let Some(Some(tx)) = DEBUG_WRITER.get() else {
        return;
    };
    let run_id = std::env::var("CRABCACHE_DEBUG_RUN_ID").unwrap_or_else(|_| "pre-fix".to_string());
    let mut line = serde_json::json!({
        "runId": run_id,
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": chrono::Utc::now().timestamp_millis(),
    });
    if let Ok(session_id) = std::env::var("CRABCACHE_DEBUG_SESSION_ID")
        && !session_id.is_empty()
    {
        line["sessionId"] = serde_json::Value::String(session_id);
    }
    let _ = tx.try_send(line.to_string()); // non-blocking, drop on full channel
}
