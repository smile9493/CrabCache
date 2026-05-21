# Multi-tenant isolation (`project_id` / DeepSeek `user_id`)

CrabCache runs as a **single gateway process** and isolates tenants logically—no per-project gateway process required.

## Architecture

```text
Client (sk-cc-* + optional X-Project-Id)
  → resolve project_id (key-bound, header validated)
  → inject body.user_id for DeepSeek upstream
  → L0/L1 cache key prefix: {global_namespace}:{project_id}:{hash}
  → Reasoning SQLite namespace includes project_id
  → L2 Qdrant filter: tenant_id == project_id
```

## Tenant identification

| Source | Role |
|--------|------|
| `StoredKey.project_id` | Authoritative tenant id (Management API / Admin) |
| `X-Project-Id` header | Must match key when key has `project_id`; otherwise sanitized and used if key has none |
| Mismatch | HTTP **403** `project_mismatch` |
| Invalid id | HTTP **400** `invalid_project_id` (must match `[a-zA-Z0-9\-_]+`, max 512) |

## DeepSeek `user_id`

When `project_id` is resolved, the gateway **overwrites** `user_id` on the upstream OpenAI-compatible body. This enables official DeepSeek isolation:

- KV / prefix cache boundaries per tenant
- Content-safety scoping per tenant
- Upstream scheduling / per-`user_id` concurrency (account-level limits still apply)

**Not** the same as OpenAI’s legacy `"user"` field (abuse tracking only).

## Gateway cache layers

| Layer | Isolation |
|-------|-----------|
| L0/L1 | `effective_cache_namespace(global, project)` → `{global}:{project}:{fingerprint_hash}` |
| Reasoning store | `reasoning_cache_namespace(..., project_id)` |
| L2 semantic | Qdrant payload `tenant_id` + search filter; point id `hash(tenant:query)` |
| Ketama L3 stickiness | `x-conversation-id` > `prompt_cache_key` > **body `user_id`** > `x-user-id` > IP |

Configure optional global prefix in `gateway.toml`:

```toml
[cache]
# Optional; combined with per-request project_id as "{global}:{project}:{hash}"
# cache_key_namespace = "org"
```

## Management API

Create or patch keys with `project_id`:

```json
POST /v1/keys
{
  "name": "project-a",
  "enabled": true,
  "project_id": "project_a"
}
```

## Metrics and trace

- Prometheus `consumer` label falls back to `project_id` when the key has no `name`-based consumer.
- Trace JSONL includes `project_id` when set.

## Security

Do not trust client-supplied `user_id` in the JSON body; the gateway replaces it when `project_id` is resolved. Bind tenants to API keys in production.

## Out of scope

Gateway-side per-`user_id` rate limiting is **not** implemented; DeepSeek enforces account and per-`user_id` limits upstream. Use `UpstreamKeyPool` for multiple upstream API keys and 429 cooldown.

## Related docs

- [DEEPSEEK_PREFIX_CACHE.md](./DEEPSEEK_PREFIX_CACHE.md) — L3 stickiness and prefix practices
- [CURSOR_SETUP.md](./CURSOR_SETUP.md) — Cursor client configuration
