//! NDJSON debug instrumentation (enabled via `CRABCACHE_DEBUG_LOG_PATH`).

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

const DEBUG_SESSION_ID: &str = "5c7cd5";
const DEBUG_DEFAULT_PATH: &str = "/opt/projct/CrabCache/.cursor/debug-5c7cd5.log";

static DEBUG_PATH: OnceLock<Option<String>> = OnceLock::new();

fn debug_path() -> Option<&'static String> {
    DEBUG_PATH
        .get_or_init(|| {
            std::env::var("CRABCACHE_DEBUG_LOG_PATH")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| Some(DEBUG_DEFAULT_PATH.to_string()))
        })
        .as_ref()
}

/// Append one NDJSON line for debug-mode hypothesis testing. Never log secrets.
pub fn debug_agent_log(
    hypothesis_id: &str,
    location: &str,
    message: &str,
    data: serde_json::Value,
) {
    let Some(path) = debug_path() else {
        return;
    };
    let run_id =
        std::env::var("CRABCACHE_DEBUG_RUN_ID").unwrap_or_else(|_| "pre-fix".to_string());
    let line = serde_json::json!({
        "sessionId": DEBUG_SESSION_ID,
        "runId": run_id,
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": chrono::Utc::now().timestamp_millis(),
    });
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}
