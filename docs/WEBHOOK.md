# Webhook System

CrabCache includes a built-in webhook system for real-time event notifications. When enabled, the gateway publishes events to registered webhook endpoints via HTTP POST requests with HMAC-SHA256 signature verification.

## Event Types

The following event types are available for webhook subscriptions:

| Event Type | Description | Payload |
|------------|-------------|---------|
| `RequestCompleted` | A chat completion request finished processing | Request ID, model, latency, cache status, tokens, etc. |
| `CacheInvalidated` | Cache invalidation was performed | Scope (all/prefix/key), success status |
| `KeyCreated` | A new API key was created | Key ID |
| `KeyRevoked` | An API key was revoked | Key ID |
| `UpstreamError` | An upstream API error occurred | Request ID, status code, model |
| `RateLimitHit` | A rate limit was triggered | Request ID, key ID, retry-after |

## Registration

Register a webhook via the Management API:

```bash
curl -X POST http://127.0.0.1:9080/v1/webhooks \
  -H "x-gateway-admin-key: your-admin-key" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://your-server.com/webhook",
    "secret": "optional-secret-key",
    "events": ["RequestCompleted"],
    "enabled": true
  }'
```

### Request Body

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `url` | string | Yes | Target URL (must start with `http://` or `https://`) |
| `secret` | string | No | Secret for HMAC signing (auto-generated if omitted) |
| `events` | string[] | No | Event types to subscribe to (empty = all events) |
| `enabled` | boolean | No | Whether webhook is enabled (default: `true`) |

### Response

```json
{
  "id": "webhook-id",
  "url": "https://your-server.com/webhook",
  "secret": "whsec_...",
  "events": ["RequestCompleted"],
  "enabled": true,
  "created_at": 1748679840,
  "last_triggered": null,
  "failure_count": 0
}
```

**Important**: The `secret` is only returned in the registration response. Store it securely for signature verification.

## Webhook Delivery Format

### HTTP Request

```
POST https://your-server.com/webhook
Content-Type: application/json
X-CrabCache-Signature: <hmac-sha256-signature>
```

### Payload Example (RequestCompleted)

```json
{
  "event_type": "RequestCompleted",
  "request_id": "abc123",
  "model": "deepseek-v4",
  "consumer": "team-alpha",
  "project_id": "proj-123",
  "latency_ms": 245,
  "cache_hit": true,
  "cache_tier": "L0",
  "is_streaming": false,
  "input_tokens": 1500,
  "output_tokens": 320,
  "upstream_status": 200,
  "pipeline": "CursorDeepSeekV4",
  "backend_name": "deepseek-primary",
  "client_ip": "192.168.1.100",
  "timestamp": 1748679840
}
```

## Signature Verification

Verify the HMAC-SHA256 signature to ensure the payload is authentic:

```python
import hmac
import hashlib

def verify_webhook_signature(secret: str, payload: str, signature: str) -> bool:
    expected = hmac.new(
        secret.encode(),
        payload.encode(),
        hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(expected, signature)
```

```rust
use crab_gateway::webhook::verify_hmac_signature;

let is_valid = verify_hmac_signature(secret, payload, signature);
```

## Retry Policy

Failed deliveries are retried with exponential backoff:

- **Max retries**: 3 (configurable)
- **Base delay**: 1000ms
- **Backoff formula**: `base_delay * 2^(attempt-1)`
  - Attempt 1: 1000ms
  - Attempt 2: 2000ms
  - Attempt 3: 4000ms

## Management API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/webhooks` | Register a new webhook |
| `GET` | `/v1/webhooks` | List all registered webhooks |
| `DELETE` | `/v1/webhooks/{id}` | Delete a webhook |
| `POST` | `/v1/webhooks/{id}/test` | Test delivery to a webhook |

All endpoints require the `x-gateway-admin-key` header for authentication.

## Monitoring

Webhook delivery metrics are available via Prometheus:

- `gateway_webhook_delivery_total` - Total webhook delivery attempts
- `gateway_webhook_delivery_success_total` - Successful deliveries
- `gateway_webhook_delivery_failure_total` - Failed deliveries

## Configuration

The event bus and webhook system can be configured in `config/gateway.toml`:

```toml
[event_bus]
capacity = 1024              # Event bus channel capacity

[webhook]
enabled = true
delivery_timeout_secs = 10   # HTTP delivery timeout
max_retries = 3              # Max retry attempts
retry_base_delay_ms = 1000   # Base delay for exponential backoff
```

## Security Considerations

1. **HMAC Signing**: All webhook payloads are signed with HMAC-SHA256. Always verify signatures in your webhook handler.
2. **Secret Rotation**: If a webhook secret is compromised, delete and re-register the webhook.
3. **HTTPS**: Use HTTPS URLs for webhook endpoints in production.
4. **Rate Limiting**: Webhook delivery is rate-limited to prevent abuse. If you need higher limits, contact support.
5. **Idempotency**: Your webhook handler should be idempotent, as events may be delivered more than once (during retries).

## Architecture

```
Proxy Request → Logging Phase → EventBus.publish()
                                      │
                                      ▼
                              WebhookDelivery.start()
                                      │
                                      ▼
                              HTTP POST + HMAC Signature
                                      │
                                      ▼
                              External Webhook Server
```

The webhook delivery runs in a background thread and is completely asynchronous. It does not block the proxy request path.
