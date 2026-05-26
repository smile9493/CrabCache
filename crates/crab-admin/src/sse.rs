use axum::extract::Query;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::state::AppState;

/// SSE event payload types broadcast to all connected clients.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum SseEvent {
    /// Core metrics changed — the full OverviewCore JSON.
    #[serde(rename = "metrics")]
    Metrics(serde_json::Value),
    /// Gateway health state changed.
    #[serde(rename = "health")]
    Health {
        healthy: bool,
        uptime_secs: u64,
        error: Option<String>,
    },
}

/// Create the broadcast channel for SSE events. Call once at startup.
pub fn create_broadcast() -> broadcast::Sender<SseEvent> {
    broadcast::channel(64).0
}

/// SSE handler with query-parameter auth (EventSource can't set custom headers).
pub async fn sse_events(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // Authenticate via ?key= query parameter.
    let provided_key = params.get("key").map(|s| s.as_str()).unwrap_or("");
    let admin_key = state.admin_key.read().clone();
    if provided_key != admin_key {
        return axum::http::Response::builder()
            .status(401)
            .body(axum::body::Body::from("Unauthorized"))
            .unwrap();
    }

    let mut rx = state.sse_broadcast.subscribe();

    let stream = async_stream::stream! {
        yield Ok::<_, Infallible>(Event::default().comment("connected"));

        loop {
            match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
                Ok(Ok(evt)) => {
                    if let Ok(json) = serde_json::to_string(&evt) {
                        let event_name = match &evt {
                            SseEvent::Metrics(_) => "metrics",
                            SseEvent::Health { .. } => "health",
                        };
                        yield Ok(Event::default().event(event_name).data(json));
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    yield Ok(Event::default().comment(format!("lagged {} events", n)));
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => break,
                Err(_) => {
                    yield Ok(Event::default().comment("keepalive"));
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
