//! Webhook management API handlers.
//!
//! Provides REST endpoints for registering, listing, and deleting webhooks.
//! Also includes a test delivery endpoint for verifying webhook configurations.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
};
use crab_control::{ErrorResponse, GATEWAY_ADMIN_KEY_HEADER, constant_time_eq_str};

use crate::management::ManagementState;
use crate::management::is_private_or_reserved_url;
use crate::webhook::{RegisterWebhookRequest, WebhookConfig, WebhookDelivery};

/// Build the webhook admin API router.
pub fn build_webhook_routes() -> Router<ManagementState> {
    Router::new()
        .route("/v1/webhooks", post(register_webhook))
        .route("/v1/webhooks", get(list_webhooks))
        .route("/v1/webhooks/{id}", delete(delete_webhook))
        .route("/v1/webhooks/{id}/test", post(test_webhook))
}

/// Register a new webhook endpoint.
///
/// POST /v1/webhooks
///
/// Request body:
/// ```json
/// {
///     "url": "https://example.com/webhook",
///     "secret": "optional-secret",
///     "events": ["RequestCompleted"],
///     "enabled": true
/// }
/// ```
pub async fn register_webhook(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterWebhookRequest>,
) -> Result<Json<WebhookConfig>, (StatusCode, Json<ErrorResponse>)> {
    // Authenticate
    authorize(&headers, &state)?;

    // Validate URL
    if payload.url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "URL is required".to_string(),
            }),
        ));
    }

    if !payload.url.starts_with("http://") && !payload.url.starts_with("https://") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "URL must start with http:// or https://".to_string(),
            }),
        ));
    }

    // SSRF protection: reject private/reserved network addresses
    if is_private_or_reserved_url(&payload.url) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Webhook URL must not point to a private or reserved network address".to_string(),
            }),
        ));
    }

    // Generate webhook ID and secret
    let webhook_id = uuid::Uuid::new_v4().to_string().replace('-', "");
    let secret = payload
        .secret
        .unwrap_or_else(|| crate::webhook::generate_webhook_secret());
    let events = payload.events.unwrap_or_default();
    let enabled = payload.enabled.unwrap_or(true);

    let config = WebhookConfig {
        id: webhook_id,
        url: payload.url,
        secret: secret.clone(),
        events,
        enabled,
        created_at: now_secs(),
        last_triggered: None,
        failure_count: 0,
    };

    state.webhook_store.register(config.clone());

    // Return config with secret (only time secret is visible)
    Ok(Json(config))
}

/// List all registered webhooks.
///
/// GET /v1/webhooks
pub async fn list_webhooks(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<Vec<WebhookConfig>>, (StatusCode, Json<ErrorResponse>)> {
    authorize(&headers, &state)?;

    let webhooks = state.webhook_store.list_all();
    let masked: Vec<WebhookConfig> = webhooks
        .into_iter()
        .map(|mut w| {
            w.secret = format!("{}****", &w.secret[..w.secret.len().min(8)]);
            w
        })
        .collect();
    Ok(Json(masked))
}

/// Delete a webhook by ID.
///
/// DELETE /v1/webhooks/{id}
pub async fn delete_webhook(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    authorize(&headers, &state)?;

    let removed = state.webhook_store.unregister(&id);
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Webhook '{}' not found", id),
            }),
        ))
    }
}

/// Test delivery to a webhook endpoint.
///
/// POST /v1/webhooks/{id}/test
///
/// Sends a test event to the webhook URL and returns the result.
pub async fn test_webhook(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::webhook::WebhookTestResult>, (StatusCode, Json<ErrorResponse>)> {
    authorize(&headers, &state)?;

    let webhook = state.webhook_store.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Webhook '{}' not found", id),
            }),
        )
    })?;

    let delivery = WebhookDelivery::new(state.webhook_store.clone(), 0, 1000);
    let result = delivery.test_delivery(&webhook.url, &webhook.secret).await;

    Ok(Json(result))
}

/// Verify admin key from request headers.
fn authorize(
    headers: &HeaderMap,
    state: &ManagementState,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let provided = headers
        .get(GATEWAY_ADMIN_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if constant_time_eq_str(provided, &state.admin_key) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid admin key".to_string(),
            }),
        ))
    }
}

/// Get current timestamp in seconds since Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
