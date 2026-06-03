//! Real-time log streaming for the Management API.
//!
//! Provides two endpoints modeled after CLIProxyAPI's dual-mode log architecture:
//! - `GET /v1/logs/stream` — SSE push of live tracing log lines
//! - `GET /v1/logs` — incremental polling of historical log lines (file-based)
//!
//! A custom `tracing::Subscriber::Layer` captures formatted log lines and
//! broadcasts them via `tokio::sync::broadcast` to zero or more SSE consumers.

use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::management::{ManagementState, authorize};

// ── Data types ───────────────────────────────────────────────────────

/// A single formatted log line pushed to SSE consumers.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    /// Epoch millis when the log was captured.
    pub ts: u64,
    /// The formatted log text (single line, no trailing newline).
    pub line: String,
}

/// Broadcast sender for live log lines. Cloned cheaply.
pub type LogBroadcast = broadcast::Sender<LogLine>;

/// Create a new broadcast channel for live log lines.
pub fn create_log_broadcast(capacity: usize) -> LogBroadcast {
    let (tx, _) = broadcast::channel(capacity);
    tx
}

// ── tracing Layer ────────────────────────────────────────────────────

/// A `tracing::Layer` that formats log events and broadcasts them.
///
/// Runs in addition to the existing file and stdout layers.  Captures only a
/// human-readable single-line representation for real-time streaming.
pub struct LiveLogLayer {
    sender: LogBroadcast,
}

impl LiveLogLayer {
    pub fn new(sender: LogBroadcast) -> Self {
        Self { sender }
    }
}

impl<S> Layer<S> for LiveLogLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = *meta.level();

        let mut visitor = LogLineVisitor {
            message: String::new(),
            fields: Vec::new(),
        };
        event.record(&mut visitor);

        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d %H:%M:%S%.3f");
        let target = meta.target();
        let line = if visitor.fields.is_empty() {
            format!("[{timestamp}] [{level}] {target}: {}", visitor.message)
        } else {
            format!(
                "[{timestamp}] [{level}] {target}: {} {}",
                visitor.message,
                visitor.fields.join(" ")
            )
        };

        let ts = now.timestamp_millis() as u64;

        let _ = self.sender.send(LogLine { ts, line });
    }
}

struct LogLineVisitor {
    message: String,
    fields: Vec<String>,
}

impl tracing::field::Visit for LogLineVisitor {
    // tracing macros may call both `record_str` and `record_debug` for the "message" field
    // (e.g. `info!("hello")` emits via `record_debug`; structured macros may use `record_str`).
    // The `is_empty()` guard ensures only the first visitor callback captures the message,
    // preventing duplicates regardless of call order.
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            if self.message.is_empty() {
                let mut s = format!("{value:?}");
                // Strip surrounding quotes that Debug adds for &str.
                if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                    s = s[1..s.len() - 1].to_string();
                }
                self.message = s;
            }
        } else {
            self.fields
                .push(format!("{}={value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            if self.message.is_empty() {
                self.message = value.to_string();
            }
        } else {
            self.fields
                .push(format!("{}={value}", field.name()));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .push(format!("{}={value}", field.name()));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .push(format!("{}={value}", field.name()));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .push(format!("{}={value}", field.name()));
    }
}

// ── SSE endpoint ─────────────────────────────────────────────────────

/// `GET /v1/logs/stream` — Server-Sent Events stream of live log lines.
///
/// Auth: `x-gateway-admin-key` header.
pub async fn stream_logs(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    authorize(&headers, &state.admin_key)?;

    let Some(ref sender) = state.log_broadcast else {
        return Err((
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "live log streaming not enabled"})),
        )
            .into_response());
    };

    let mut rx = sender.subscribe();

    let stream = async_stream::stream! {
        yield Ok::<_, Infallible>(Event::default().comment("connected"));

        loop {
            match rx.recv().await {
                Ok(log_line) => {
                    if let Ok(json) = serde_json::to_string(&log_line) {
                        yield Ok(Event::default().event("log").data(json));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    yield Ok(Event::default().comment(format!("lagged {n} events")));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}

// ── Historical log polling ───────────────────────────────────────────

const DEFAULT_LOG_FILE: &str = "./logs/gateway.log";
const MAX_LINES: usize = 2000;

/// Query parameters for `GET /v1/logs`.
#[derive(serde::Deserialize)]
pub struct LogsQuery {
    limit: Option<usize>,
}

/// `GET /v1/logs` — Retrieve the tail of the gateway log file.
///
/// Query parameters:
/// - `limit` (optional): max lines to return (default 200, max 2000).
///
/// Returns:
/// ```json
/// { "lines": ["..."], "line_count": N, "latest_ts": epoch_ms }
/// ```
pub async fn get_logs(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Query(params): Query<LogsQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    authorize(&headers, &state.admin_key)?;

    let log_path = state
        .log_file_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_LOG_FILE));

    let limit = params.limit.unwrap_or(200).min(MAX_LINES);

    let lines = read_log_tail(&log_path, limit).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to read log file: {e}")})),
        )
            .into_response()
    })?;

    let latest_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    Ok(Json(serde_json::json!({
        "lines": lines,
        "line_count": lines.len(),
        "latest_ts": latest_ts,
    })))
}

/// Read the last `limit` lines from a log file (tail behavior).
fn read_log_tail(path: &Path, limit: usize) -> std::io::Result<Vec<String>> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut tail: VecDeque<String> = VecDeque::with_capacity(limit);
    for line in reader.lines() {
        let line = line?;
        if tail.len() >= limit {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    Ok(tail.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_read_log_tail() {
        let dir = std::env::temp_dir().join(format!("crab_logs_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.log");
        let mut f = fs::File::create(&path).unwrap();
        for i in 0..20 {
            writeln!(f, "line {i}").unwrap();
        }

        let lines = read_log_tail(&path, 5).unwrap();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "line 15");
        assert_eq!(lines[4], "line 19");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_log_tail_fewer_than_limit() {
        let dir = std::env::temp_dir().join(format!("crab_logs_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.log");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "only line").unwrap();

        let lines = read_log_tail(&path, 100).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "only line");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_log_tail_missing_file() {
        let result = read_log_tail(Path::new("/nonexistent/path.log"), 10);
        assert!(result.is_err());
    }
}
