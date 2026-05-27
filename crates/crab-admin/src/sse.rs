use axum::extract::Query;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};
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

/// Token lifetime for SSE authentication (60 seconds).
const SSE_TOKEN_TTL: Duration = Duration::from_secs(60);

/// Generate a short-lived SSE token. Requires valid admin key in `x-admin-key` header.
/// The token is stored in-memory and can only be used once for SSE connection.
pub async fn sse_token(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Validate admin key from header.
    let provided = headers
        .get("x-admin-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let admin_key = state.admin_key.read().clone();
    if provided != admin_key {
        return axum::http::Response::builder()
            .status(401)
            .body(axum::body::Body::from("Unauthorized"))
            .unwrap();
    }

    // Generate random token using UUID v4.
    let token = uuid::Uuid::new_v4().to_string().replace('-', "");
    let expires = Instant::now() + SSE_TOKEN_TTL;

    // Evict expired tokens before inserting.
    state.sse_tokens.retain(|_, exp| *exp > Instant::now());

    // Limit token count to prevent abuse.
    if state.sse_tokens.len() > 100 {
        state.sse_tokens.clear();
    }

    state.sse_tokens.insert(token.clone(), expires);

    axum::http::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({ "token": token }).to_string(),
        ))
        .unwrap()
}

/// SSE handler with short-lived token auth (EventSource can't set custom headers).
/// Accepts `?token=` query parameter. The token is consumed on use (one-time).
pub async fn sse_events(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // Authenticate via ?token= query parameter (one-time use).
    let provided_token = params.get("token").map(|s| s.as_str()).unwrap_or("");

    // Atomically remove and validate — eliminates TOCTOU race where two
    // concurrent requests with the same token could both pass validation.
    let valid = match state.sse_tokens.remove(provided_token) {
        Some((_, exp)) if exp > Instant::now() => true,
        _ => false,
    };

    if !valid {
        return axum::http::Response::builder()
            .status(401)
            .body(axum::body::Body::from("Unauthorized or expired token"))
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
