# Upstream Key Pool

> Concurrency control, key selection, and operational reference for the `UpstreamKeyPool` system.

## 1. Concurrency Semantics

The gateway has **four independent concurrency dimensions**. They are not shared and do not interact:

| Control | Scope | Config | Default | What it limits |
|---------|-------|--------|---------|----------------|
| `max_inflight_per_key` | Per-key, per-profile | `[[upstream.profiles]]` TOML | 0 (unlimited) | Concurrent requests through a **single key slot** in a profile pool |
| `features.mimo_key_max_inflight` | Per-key, runtime override | `[features]` TOML | 0 | Override for MiMo profiles when profile-level is unset |
| `features.codex_key_max_inflight` | Per-key, runtime override | `[features]` TOML | 0 | Override for Codex profiles when profile-level is unset |
| Coalesce inflight (request coalescing) | Per-cache-key, global | automatic | N/A | Followers waiting on the **same cache key**; not a semaphore on upstream |

**Key distinction**: `max_inflight_per_key` is a **tokio Semaphore per key slot** initialized once when the pool is built. To change it at runtime, the pool must be **rebuilt** (see §3). Coalesce inflight is a separate `tokio::sync::watch` mechanism that does not cap upstream connections — it only deduplicates concurrent identical requests.

### When inflight limit is hit

The pool falls through to the next candidate key (priority order). If **all** keys are at their inflight cap, `PoolAcquireFailure::AllInflightFull` is returned and the request gets a 503 with `error_code: "upstream_pool_exhausted"`.

## 2. Key Selection Strategy

When `acquire()` (or any variant) is called, the pool evaluates candidates in this order:

1. **Priority sort** — keys are sorted by `priority` descending (higher = preferred). Keys with equal priority are sorted by insertion order (stable).
2. **Disabled filter** — keys with `enabled = false` are skipped.
3. **Cooldown check** — keys in per-key cooldown (from 429 or error responses) are skipped. Per-key cooldown is independent of the global `key_cooldown_secs`.
4. **Quota/exclusion** — for Codex, keys whose `account_id` matches the excluded account are skipped (429 cross-account rotation).
5. **Model support** — if the key has `supported_models` non-empty and the request model is not in the set, the key is skipped (`NoModelSupport`).
6. **Least inflight** — among eligible keys with the same priority, the one with the lowest current inflight count is selected (tie-break: insertion order).

### Special acquire variants

| Variant | Use case |
|---------|----------|
| `acquire()` | Standard upstream pool (any key) |
| `acquire_for_upstream_model(model)` | Respects `supported_models` filtering |
| `acquire_specific(key_id)` | Pin to a specific key (MiMo conversation binding) |
| `acquire_codex_oauth()` | Codex OAuth-only keys |
| `acquire_codex_for_model(model, excluded, fill_first)` | Codex with model family scope cooldowns |
| `acquire_excluding_key(key_id)` | Exclude a single key (retry after failure) |
| `acquire_excluding_account(account_id)` | Exclude all keys for an account (429 rotation) |

## 3. `hot_replace` vs `rebuild_with_max_inflight`

| Operation | `hot_replace` | `rebuild_with_max_inflight` |
|-----------|---------------|----------------|
| **Purpose** | Replace key set while preserving inflight counts | Change per-key semaphore cap |
| **When called** | Management API `PUT /v1/upstream/profiles/{id}/keys` | Profile `max_inflight_per_key` change |
| **Preserves existing inflight** | Yes — surviving keys keep their `Arc<Semaphore>` and inflight count | No — new semaphore per key (inflight resets to 0) |
| **Key identity** | Matched by `id`; new keys get fresh semaphores | All keys rebuilt with new semaphore permits |
| **Atomic swap** | `*pool_handle.write() = new_pool` (RwLock write) | Same |
| **Dropped keys** | Keys not in new set lose their semaphore → existing guards complete naturally, no new acquires | N/A (same key set) |
| **Zero-downtime** | Yes — in-flight requests finish on old guards | Yes — but inflight counters reset |

### Typical usage pattern

```rust
// Management API updates keys
let new_pool = UpstreamKeyPool::hot_replace(&current_pool, new_specs);
*profile.upstream_pool.write() = new_pool;

// Change max_inflight_per_key
let rebuilt = UpstreamKeyPool::rebuild_with_max_inflight(&pool, new_max);
*profile.upstream_pool.write() = rebuilt;
```

## 4. Related Documentation

- Config templates: `config/gateway.example.toml` — `[[upstream.profiles]]` and `[[upstream.profiles.keys]]` sections
- Key management API: `GET/PUT/PATCH /v1/upstream/profiles/{id}/keys`
- Environment variable overrides: `CRABCACHE_API_KEY`, `CRABCACHE_GATEWAY_ADMIN_KEY`
