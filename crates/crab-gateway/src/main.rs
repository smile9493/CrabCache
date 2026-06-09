use anyhow::Result;
use async_trait::async_trait;
use crab_cache::{FingerprintConfig, RequestCoalescer, TieredCache, TtlConfig};
use crab_client_endpoint::{DiscoveryConfig, discover};
use crab_gateway::config::GatewayConfig;
use crab_gateway::management::{InvalidateRateState, ManagementState, serve_with_shutdown as serve_management};
use crab_metrics::global_metrics;
use crab_proxy::{
    ClientKeyLimiter, ClientKeyRateLimiter, DeepSeekUserConcurrencyConfig, GatewayProxy,
    GatewayState, RawCaptureLogger, RuntimeConfig, SemanticRuntimeState, SharedSemanticRuntime,
    UpstreamUserIdLimiter,
};
use crab_reasoning::ReasoningBackend;
use crab_route::LbRouter;
use crab_semantic::{EmbedderPool, SemanticCache, SemanticGateConfig, VectorStore};
use crab_state::{
    RedisStateConfig, RedisStateStore, apply_snapshot_to_runtime, build_snapshot_from_runtime,
    spawn_key_state_persist_task, spawn_state_refresh_task,
};
use parking_lot::RwLock;
use pingora_core::server::Server;
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::background_service;
use pingora_core::services::listening::Service;
use pingora_proxy::http_proxy;
use prometheus::Registry;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

// ---------------------------------------------------------------------------
// PG Trace Writer (gateway-side, independent of Admin Dashboard pool)
// Uses a single tokio-postgres connection (no pool dependency).
// ---------------------------------------------------------------------------

struct PgTraceStore {
    client: tokio::sync::Mutex<tokio_postgres::Client>,
}

impl PgTraceStore {
    async fn connect(pg_url: &str) -> anyhow::Result<Self> {
        use anyhow::Context;

        let (client, connection) = tokio_postgres::connect(pg_url, tokio_postgres::NoTls)
            .await
            .context("pg trace connect")?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::warn!("PG trace connection error: {}", e);
            }
        });
        Ok(Self {
            client: tokio::sync::Mutex::new(client),
        })
    }

    async fn insert_batch(&self, entries: &[crab_proxy::SanitizedLogEntry]) -> anyhow::Result<()> {
        use anyhow::Context;

        if entries.is_empty() {
            return Ok(());
        }
        let mut client = self.client.lock().await;
        let tx = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client.transaction(),
        )
        .await
        .context("pg trace begin tx timeout (10s)")?
        .context("pg trace begin tx")?;
        let stmt = tx
            .prepare(
                "INSERT INTO trace_logs
                    (request_hash, timestamp_ms, content_length, semantic_cluster,
                     model, prompt_tokens, latency_ms, cache_hit,
                     conversation_id, consumer, domain, project_id,
                     upstream_latency_ms, ttft_ms, input_tokens, output_tokens,
                     cache_tier, composition,
                     request_messages_snapshot, response_preview,
                     retired_prefix_messages, reasoning_strategy,
                     prompt_cache_hit_ratio, upstream_profile_id, pipeline,
                     upstream_model, client_body_user_id, upstream_user_id,
                     user_id_audit, upstream_key_id,
                     session_store, stable_session_kind, upstream_outbound_bytes,
                     prefill_ms, pre_header_ms,
                     affinity_key, affinity_kind, backend_name,
                     session_fingerprint, is_coalesced, client_key_id,
                     request_passthrough, request_passthrough_prefix_len,
                     status_code, error_code, limit_source, cache_decision,
                     upstream_result, phase_durations_ms, client_ip, client_kind)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,
                         $15,$16,$17,$18::jsonb,$19,$20,$21,$22,$23,$24,$25,
                         $26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,
                         $39,$40,$41,$42,$43,$44,$45,$46,$47,$48,$49::jsonb,$50,$51)
                 ON CONFLICT (request_hash, timestamp_ms) DO NOTHING",
            )
            .await
            .context("pg prepare insert_trace_logs")?;

        for e in entries {
            let composition_json = e
                .composition
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|err| anyhow::anyhow!("serialize composition: {err}"))?;
            let composition_pg = composition_json
                .as_ref()
                .map(tokio_postgres::types::Json);
            let phase_durations_pg: Option<tokio_postgres::types::Json<serde_json::Value>> = match &e.phase_durations_ms {
                Some(v) => Some(tokio_postgres::types::Json(v.clone())),
                None => None,
            };
            tx.execute(
                &stmt,
                &[
                    &e.request_hash,
                    &(e.timestamp_ms as i64),
                    &(e.content_length.try_into().unwrap_or(i32::MAX)),
                    &(e.semantic_cluster.try_into().unwrap_or(i32::MAX)),
                    &e.model,
                    &(e.prompt_tokens.try_into().unwrap_or(i32::MAX)),
                    &e.latency_ms,
                    &e.cache_hit,
                    &e.conversation_id,
                    &e.consumer,
                    &e.domain,
                    &e.project_id,
                    &e.upstream_latency_ms,
                    &e.ttft_ms,
                    &e.input_tokens.map(|v| v as i64),
                    &e.output_tokens.map(|v| v as i64),
                    &e.cache_tier,
                    &composition_pg,
                    &e.request_messages_snapshot,
                    &e.response_preview,
                    &e.retired_prefix_messages.map(|v| v.try_into().unwrap_or(i32::MAX)),
                    &e.reasoning_strategy,
                    &e.prompt_cache_hit_ratio,
                    &e.upstream_profile_id,
                    &e.pipeline,
                    &e.upstream_model,
                    &e.client_body_user_id,
                    &e.upstream_user_id,
                    &e.user_id_audit,
                    &e.upstream_key_id,
                    &e.session_store,
                    &e.stable_session_kind,
                    &e.upstream_outbound_bytes.map(|v| v.try_into().unwrap_or(i32::MAX)),
                    &e.prefill_ms,
                    &e.pre_header_ms,
                    &e.affinity_key,
                    &e.affinity_kind,
                    &e.backend_name,
                    &e.session_fingerprint,
                    &e.is_coalesced,
                    &e.client_key_id,
                    &e.request_passthrough,
                    &e.request_passthrough_prefix_len.map(|v| v.try_into().unwrap_or(i32::MAX)),
                    &e.status_code.map(|v| v as i32),
                    &e.error_code,
                    &e.limit_source,
                    &e.cache_decision,
                    &e.upstream_result,
                    &phase_durations_pg,
                    &e.client_ip,
                    &e.client_kind,
                ],
            )
            .await
            .map_err(|err| {
                tracing::warn!(
                    request_hash = %e.request_hash,
                    timestamp_ms = e.timestamp_ms,
                    model = %e.model,
                    "PG trace insert error detail: {err}"
                );
                anyhow::anyhow!("pg execute insert_trace_logs: {err}")
            })?;
        }

        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tx.commit(),
        )
        .await
        .context("pg commit insert_trace_logs timeout (30s)")?
        .context("pg commit insert_trace_logs")?;
        global_metrics().inc_admin_log_write("trace");
        Ok(())
    }
}

/// Redact password from PG URL for logging.
fn redact_pg_url(url: &str) -> String {
    if let Some(at) = url.find('@') {
        if let Some(slash) = url[..at].rfind('/') {
            let end = (slash + 2).min(url.len());
            let prefix = &url[..end];
            let suffix = &url[at..];
            return format!("{}****:****{}", prefix, suffix);
        }
    }
    url.to_string()
}

/// Try to recover control-plane state from PG `gateway_state_snapshots`.
/// Returns `Ok(true)` if recovery succeeded, `Ok(false)` if no snapshot was found.
async fn recover_from_pg_snapshot(
    pg_url: &str,
    runtime: &Arc<RuntimeConfig>,
    store: &Arc<RedisStateStore>,
    key_cooldown_secs: u64,
) -> anyhow::Result<bool> {
    use anyhow::Context;

    let (client, connection) = tokio_postgres::connect(pg_url, tokio_postgres::NoTls)
        .await
        .context("pg recovery connect")?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::warn!("PG recovery connection error: {}", e);
        }
    });

    let row = client
        .query_opt(
            "SELECT keys_json, runtime_json, profiles_json,
                    key_states_json, domain_policies_json
             FROM gateway_state_snapshots
             ORDER BY snapshot_at DESC LIMIT 1",
            &[],
        )
        .await
        .context("query latest snapshot")?;

    let row = match row {
        Some(r) => r,
        None => return Ok(false),
    };

    // Build a ControlPlaneSnapshot from the PG columns.
    let keys_json: Option<tokio_postgres::types::Json<serde_json::Value>> = row.get(0);
    let runtime_json: Option<tokio_postgres::types::Json<serde_json::Value>> = row.get(1);
    let profiles_json: Option<tokio_postgres::types::Json<serde_json::Value>> = row.get(2);
    let key_states_json: Option<tokio_postgres::types::Json<serde_json::Value>> = row.get(3);
    let domain_policies_json: Option<tokio_postgres::types::Json<serde_json::Value>> = row.get(4);

    let mut snap = crab_state::ControlPlaneSnapshot::default();

    if let Some(keys_json) = keys_json {
        if let Ok(keys) = serde_json::from_value::<std::collections::HashMap<String, crab_state::StoredKeySnapshot>>(keys_json.0.clone()) {
            snap.keys = keys;
        }
    }
    if let Some(runtime_json) = runtime_json {
        if let Ok(rt) = serde_json::from_value::<Option<crab_state::RuntimeSnapshot>>(runtime_json.0.clone()) {
            snap.runtime = rt;
        }
    }
    if let Some(profiles_json) = profiles_json {
        if let Ok(profs) = serde_json::from_value::<Option<Vec<crab_state::UpstreamProfileSnapshot>>>(profiles_json.0.clone()) {
            snap.upstream_profiles = profs;
        }
    }
    if let Some(key_states_json) = key_states_json {
        if let Ok(states) = serde_json::from_value::<std::collections::HashMap<String, crab_proxy::UpstreamKeyStateSnapshot>>(key_states_json.0.clone()) {
            snap.key_states = states;
        }
    }
    if let Some(domain_policies_json) = domain_policies_json {
        if let Ok(policies) = serde_json::from_value::<indexmap::IndexMap<String, crab_proxy::DomainPolicy>>(domain_policies_json.0.clone()) {
            snap.domain_policies = policies;
        }
    }

    // Apply the snapshot to runtime.
    crab_state::apply_snapshot_to_runtime(runtime, &snap, key_cooldown_secs)
        .context("apply PG snapshot to runtime")?;

    // Also save to Redis so it's in sync.
    store.save_all(&snap).await.context("save PG snapshot to Redis")?;

    let profile_ids: Vec<String> = runtime.upstream_profiles.read().keys().cloned().collect();
    info!(
        keys = snap.keys.len(),
        profile_count = profile_ids.len(),
        profiles = ?profile_ids,
        "Recovered control-plane state from PG snapshot"
    );

    Ok(true)
}

struct MetricsServer {
    addr: String,
    registry: Registry,
    global_rate: Arc<pingora_limits::rate::Rate>,
}

#[async_trait]
impl pingora_core::services::background::BackgroundService for MetricsServer {
    async fn start(&self, mut shutdown: pingora_core::server::ShutdownWatch) {
        let listener = match tokio::net::TcpListener::bind(&self.addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(addr = %self.addr, error = %e, "Failed to bind metrics addr");
                global_metrics().set_background_task_healthy("metrics_server", false);
                return;
            }
        };
        global_metrics().set_background_task_healthy("metrics_server", true);
        global_metrics().set_background_task_last_success_now("metrics_server");

        let mut rate_tick = tokio::time::interval(std::time::Duration::from_secs(1));
        rate_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let accept = tokio::select! {
                _ = shutdown.changed() => break,
                _ = rate_tick.tick() => {
                    let rps = self.global_rate.rate(&crab_gateway::GLOBAL_RATE_KEY);
                    global_metrics().record_global_rps(rps);
                    continue;
                }
                result = listener.accept() => result,
            };
            let Ok((mut stream, _)) = accept else {
                continue;
            };
            let registry = self.registry.clone();
            tokio::spawn(async move {
                let output = match tokio::task::spawn_blocking(move || {
                    let encoder = prometheus::TextEncoder::new();
                    let metric_families = registry.gather();
                    encoder.encode_to_string(&metric_families)
                })
                .await
                {
                    Ok(Ok(body)) => body,
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, "Failed to encode prometheus metrics");
                        String::new()
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Metrics gather task join failed");
                        String::new()
                    }
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
                    output.len(),
                    output
                );
                use tokio::io::AsyncWriteExt;
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
        global_metrics().record_background_task_shutdown_drained("metrics_server");
    }
}

// ---------------------------------------------------------------------------
// Management API BackgroundService
// ---------------------------------------------------------------------------

struct ManagementService {
    listen_addr: String,
    state: ManagementState,
}

#[async_trait]
impl pingora_core::services::background::BackgroundService for ManagementService {
    async fn start(&self, shutdown: ShutdownWatch) {
        global_metrics().set_background_task_healthy("management_api", true);
        global_metrics().set_background_task_last_success_now("management_api");
        if let Err(e) = serve_management(&self.listen_addr, self.state.clone(), shutdown).await {
            tracing::error!(error = %e, "Management API server failed");
            global_metrics().set_background_task_healthy("management_api", false);
        }
        global_metrics().record_background_task_shutdown_drained("management_api");
    }
}

// ---------------------------------------------------------------------------
// Webhook Delivery BackgroundService
// ---------------------------------------------------------------------------

struct WebhookService {
    delivery: crab_gateway::webhook::WebhookDelivery,
    event_bus: Arc<crab_proxy::EventBus>,
}

#[async_trait]
impl pingora_core::services::background::BackgroundService for WebhookService {
    async fn start(&self, shutdown: ShutdownWatch) {
        let rx = self.event_bus.subscribe();
        self.delivery.start_with_shutdown(rx, shutdown).await;
        global_metrics().record_background_task_shutdown_drained("webhook_delivery");
    }
}

// ---------------------------------------------------------------------------
// Codex Quota Refresh BackgroundService
// ---------------------------------------------------------------------------

struct CodexQuotaRefreshService {
    runtime: Arc<RuntimeConfig>,
    cache: Arc<crab_proxy::codex_quota_cache::CodexQuotaCache>,
}

#[async_trait]
impl pingora_core::services::background::BackgroundService for CodexQuotaRefreshService {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        global_metrics().set_background_task_healthy("codex_quota_refresh", true);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    refresh_codex_quotas(&self.runtime, &self.cache).await;
                    global_metrics().set_background_task_last_success_now("codex_quota_refresh");
                }
                _ = shutdown.changed() => {
                    tracing::info!("Codex quota refresh shutting down");
                    break;
                }
            }
        }
        global_metrics().record_background_task_shutdown_drained("codex_quota_refresh");
    }
}

// ---------------------------------------------------------------------------
// Prune Service (rate limiter + idempotency cleanup) BackgroundService
// ---------------------------------------------------------------------------

struct PruneService {
    rate_limiter: Arc<crab_proxy::ClientKeyRateLimiter>,
    idempotency: Arc<crab_cache::IdempotencyStore>,
    model_lockouts: Arc<crab_proxy::model_lockout::ModelLockoutRegistry>,
    client_lockouts: Arc<crab_proxy::client_lockout::ClientLockoutRegistry>,
    backend_load: Arc<crab_proxy::backend_state::BackendLoadRegistry>,
    key_binding_store: Option<Arc<crab_proxy::key_binding::KeyBindingStore>>,
    runtime: Arc<crab_proxy::RuntimeConfig>,
}

#[async_trait]
impl pingora_core::services::background::BackgroundService for PruneService {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        let mut prune_interval = tokio::time::interval(std::time::Duration::from_secs(300));
        let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = prune_interval.tick() => {
                    // Rate limiter stale bucket cleanup.
                    self.rate_limiter.prune_stale(std::time::Duration::from_secs(600));
                    // Model lockout expired entry cleanup.
                    self.model_lockouts.cleanup();
                    // Client lockout expired entry cleanup.
                    self.client_lockouts.cleanup();
                    // Backend load registry: remove stale (profile, backend) slots.
                    let active_keys = self.runtime.active_backend_keys();
                    self.backend_load.prune_inactive(&active_keys);
                    // Key binding index reconciliation (fixes drift from Moka capacity eviction).
                    if let Some(ref store) = self.key_binding_store {
                        let (entries, sum, consistent) = store.diagnostic_snapshot();
                        crab_metrics::global_metrics().set_key_binding_diagnostics(
                            entries,
                            sum,
                            consistent,
                            store.effective_ttl_secs(),
                        );
                        store.reconcile_key_sessions();
                    }
                }
                _ = cleanup_interval.tick() => {
                    self.idempotency.cleanup_expired();
                }
                _ = shutdown.changed() => {
                    tracing::info!("Prune service shutting down");
                    break;
                }
            }
        }
    }
}

/// Wire `CodexQuotaCache` to all Codex profile key pools.
fn wire_codex_quota_caches(
    runtime: &Arc<RuntimeConfig>,
    cache: &Arc<crab_proxy::codex_quota_cache::CodexQuotaCache>,
) {
    let map = runtime.upstream_profiles.read();
    for profile in map.values() {
        if profile.provider == crab_pipeline::UpstreamProvider::Codex {
            let pool_arc = profile.upstream_pool.read().clone();
            pool_arc.set_quota_cache(cache.clone());
        }
    }
}

/// Refresh quotas for all enabled Codex keys (background task).
async fn refresh_codex_quotas(
    runtime: &Arc<RuntimeConfig>,
    cache: &Arc<crab_proxy::codex_quota_cache::CodexQuotaCache>,
) {
    let profiles = {
        let map = runtime.upstream_profiles.read();
        map.values()
            .filter(|p| p.provider == crab_pipeline::UpstreamProvider::Codex)
            .map(|p| {
                let pool = p.upstream_pool.read().clone();
                let base_url = p.base_url.clone();
                (pool, base_url)
            })
            .collect::<Vec<_>>()
    };
    for (pool, base_url) in profiles {
        for spec in pool.to_specs() {
            if !spec.enabled {
                continue;
            }
            if spec.account_id.is_empty()
                || spec.account_id == crab_proxy::DEFAULT_UPSTREAM_ACCOUNT_ID
            {
                continue;
            }
            cache
                .refresh_key(&spec.id, &base_url, &spec.secret, &spec.account_id)
                .await;
        }
    }
}

/// Detect whether a PG error is retryable (connection/infra issue vs data issue).
fn is_retryable_pg_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}").to_lowercase();
    // Connection-level or transient infra errors
    if msg.contains("connection")
        || msg.contains("closed")
        || msg.contains("timeout")
        || msg.contains("refused")
        || msg.contains("broken pipe")
        || msg.contains("network")
    {
        return true;
    }
    // Undefined table — Admin migration hasn't run yet; worth waiting for.
    if msg.contains("42p01") || msg.contains("does not exist") {
        return true;
    }
    false
}

/// Quick connectivity check: execute `SELECT 1`.
async fn is_store_alive(store: &PgTraceStore) -> bool {
    let client = store.client.lock().await;
    match tokio::time::timeout(std::time::Duration::from_secs(3), client.simple_query("SELECT 1")).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

/// Reconnect to PG with exponential backoff up to `max_backoff`.
async fn reconnect_with_backoff(
    pg_url: &str,
    max_backoff: std::time::Duration,
) -> anyhow::Result<PgTraceStore> {
    let mut backoff = std::time::Duration::from_secs(1);
    for attempt in 0..5 {
        match PgTraceStore::connect(pg_url).await {
            Ok(s) => {
                global_metrics().record_trace_pg_reconnect("success");
                global_metrics().set_background_task_healthy("pg_trace_writer", true);
                return Ok(s);
            }
            Err(e) => {
                tracing::warn!(
                    attempt,
                    backoff_secs = backoff.as_secs(),
                    "PG trace reconnect attempt failed: {:#}",
                    e
                );
                global_metrics().record_trace_pg_reconnect("failure");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
    anyhow::bail!("PG trace reconnect exhausted (5 attempts)")
}

/// Final flush of remaining `buf` entries, with retries (called on channel close).
async fn drain_with_retry(
    store: &PgTraceStore,
    buf: &mut Vec<crab_proxy::SanitizedLogEntry>,
    max_backoff: std::time::Duration,
) {
    if buf.is_empty() {
        return;
    }
    let mut backoff = std::time::Duration::from_secs(1);
    for attempt in 0..5 {
        let start = std::time::Instant::now();
        match store.insert_batch(buf).await {
            Ok(()) => {
                global_metrics().record_trace_write("pg", "success");
                global_metrics().record_trace_pg_flush_latency(start.elapsed());
                buf.clear();
                return;
            }
            Err(e) => {
                tracing::warn!(
                    attempt,
                    entries = buf.len(),
                    "PG trace final flush failed: {:#}",
                    e
                );
                global_metrics().inc_admin_log_pg_write_error();
                global_metrics().record_trace_write("pg", "failure");
                global_metrics().record_trace_pg_flush_latency(start.elapsed());
                if !is_retryable_pg_error(&e) {
                    buf.clear();
                    return;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
    tracing::warn!(
        entries = buf.len(),
        "PG trace final flush exhausted retries, dropping remaining entries"
    );
    buf.clear();
}

/// Main loop for the PG trace writer thread with connection retry and
/// failure-resilient batch handling.
async fn pg_trace_writer_loop(
    pg_url: String,
    pg_rx: std::sync::mpsc::Receiver<crab_proxy::SanitizedLogEntry>,
) {
    const INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);
    const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);
    const MAX_PENDING_DRAIN: usize = 50_000;

    let mut backoff = INITIAL_BACKOFF;
    let mut buf = Vec::with_capacity(100);

    // Phase 1: initial connection with retry (blocks until connected).
    let mut store: PgTraceStore = loop {
        match PgTraceStore::connect(&pg_url).await {
            Ok(s) => {
                info!("PG trace store connected (initial)");
                global_metrics().set_background_task_healthy("pg_trace_writer", true);
                global_metrics().set_background_task_last_success_now("pg_trace_writer");
                break s;
            }
            Err(e) => {
                tracing::warn!(
                    backoff_secs = backoff.as_secs(),
                    "PG trace connect failed, retrying: {:#}",
                    e
                );
                global_metrics().set_background_task_healthy("pg_trace_writer", false);
                global_metrics().inc_admin_log_pg_write_error();
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    };

    // Phase 2: drain + flush loop.
    loop {
        // Block on first entry (or detect channel close).
        let first = match pg_rx.recv() {
            Ok(e) => e,
            Err(_) => {
                // Channel closed — final flush with retries.
                drain_with_retry(&mut store, &mut buf, MAX_BACKOFF).await;
                global_metrics().record_background_task_shutdown_drained("pg_trace_writer");
                info!("PG trace writer exiting (channel closed)");
                return;
            }
        };
        buf.push(first);

        // Drain more entries up to batch size.
        while buf.len() < 100 {
            match pg_rx.try_recv() {
                Ok(e) => buf.push(e),
                Err(_) => break,
            }
        }

        global_metrics().set_trace_pg_queue_depth(buf.len() as f64);
        let start = std::time::Instant::now();

        // Try to insert; on failure keep buf for retry.
        match store.insert_batch(&buf).await {
            Ok(()) => {
                global_metrics().record_trace_write("pg", "success");
                global_metrics().set_background_task_last_success_now("pg_trace_writer");
                global_metrics().set_background_task_healthy("pg_trace_writer", true);
                global_metrics().record_trace_pg_flush_latency(start.elapsed());
                buf.clear();
                backoff = INITIAL_BACKOFF; // reset on success
            }
            Err(e) => {
                global_metrics().inc_admin_log_pg_write_error();
                global_metrics().record_trace_write("pg", "failure");
                global_metrics().record_trace_pg_flush_latency(start.elapsed());

                if is_retryable_pg_error(&e) {
                    tracing::warn!(
                        entries = buf.len(),
                        "PG trace insert failed (retryable), backing off: {:#}",
                        e
                    );
                    global_metrics().set_background_task_healthy("pg_trace_writer", false);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);

                    // Reconnect if connection likely dropped.
                    if !is_store_alive(&store).await {
                        tracing::warn!("PG trace connection lost, reconnecting...");
                        match reconnect_with_backoff(&pg_url, MAX_BACKOFF).await {
                            Ok(new_store) => {
                                store = new_store;
                                // Retry the preserved buf immediately next iteration.
                            }
                            Err(e) => {
                                tracing::warn!("PG trace reconnect failed: {:#}", e);
                            }
                        }
                    }
                    // Backpressure: if too many entries queued, drop oldest batch.
                    if buf.len() > MAX_PENDING_DRAIN {
                        tracing::warn!(
                            pending = buf.len(),
                            "PG trace writer backpressure: dropping oldest batch"
                        );
                        buf.clear();
                    }
                } else {
                    // Non-retryable (data/schema mismatch) — log and drop.
                    tracing::warn!(
                        entries = buf.len(),
                        "PG trace insert failed (non-retryable), dropping batch: {:#}",
                        e
                    );
                    buf.clear();
                }
            }
        }
    }
}

fn main() -> Result<()> {
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        tracing::error!(
            location = %location,
            message = %message,
            "Panic occurred — aborting process to prevent inconsistent state"
        );
        // Abort the process to prevent panicked threads from leaving
        // shared state (DashMap, RwLock) in an inconsistent state.
        std::process::abort();
    }));

    std::fs::create_dir_all("./logs").ok();
    crab_proxy::init_debug_log(std::env::var("CRABCACHE_DEBUG_LOG_PATH").ok().as_deref());

    // Live log broadcast for SSE streaming (created before tracing so the layer can use it).
    let log_broadcast = crab_gateway::live_logs::create_log_broadcast(4096);

    let file_writer = tracing_subscriber::fmt::writer::BoxMakeWriter::new(|| {
        let writer: Box<dyn Write + Send> = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("./logs/gateway.log")
        {
            Ok(file) => Box::new(file),
            Err(_) => Box::new(std::io::sink()),
        };
        writer
    });

    // File layer: JSON format, INFO+ (or RUST_LOG override)
    let file_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Stdout layer: only WARN+ by default (configurable via RUST_LOG_STDOUT)
    let stdout_filter = tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG_STDOUT").unwrap_or_else(|_| "warn".to_string()),
    );

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_filter(file_filter);

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_filter(stdout_filter);

    // Optional: OpenTelemetry tracing layer (feature-gated + env-gated).
    #[cfg(feature = "otel")]
    let otel_layer = {
        let otel_config = crab_gateway::otel::OtelConfig::from_env();
        if otel_config.should_activate() {
            info!(
                endpoint = ?otel_config.endpoint,
                service_name = %otel_config.service_name,
                sample_ratio = otel_config.sample_ratio,
                "OpenTelemetry tracing enabled"
            );
        }
        crab_gateway::otel::build_otel_layer(&otel_config)
    };

    let live_layer = crab_gateway::live_logs::LiveLogLayer::new(log_broadcast.clone());

    let registry = tracing_subscriber::registry()
        .with(file_layer)
        .with(stdout_layer)
        .with(live_layer);

    #[cfg(feature = "otel")]
    let registry = registry.with(otel_layer);

    registry.init();

    let (config_path, clear_reasoning_cache) = parse_cli_args();

    let config = GatewayConfig::load(&config_path)?;
    info!(config_path = %config_path, "Configuration loaded");

    if clear_reasoning_cache {
        let reasoning_config = config.reasoning.clone().unwrap_or_default();
        let backend_env = std::env::var("CRABCACHE_REASONING_BACKEND")
            .ok()
            .unwrap_or_else(|| reasoning_config.backend.clone());
        let store = if backend_env == "pg" {
            let pg_url_env = std::env::var("CRABCACHE_REASONING_PG_URL").ok();
            let pg_url = reasoning_config
                .pg_url
                .as_deref()
                .or(pg_url_env.as_deref())
                .ok_or_else(|| anyhow::anyhow!("reasoning.backend = \"pg\" requires pg_url"))?;
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(ReasoningBackend::open_pg(
                pg_url,
                reasoning_config.cache_max_age_secs,
                reasoning_config.cache_max_rows,
            ))?
        } else {
            ReasoningBackend::from_config(
                &reasoning_config.backend,
                &reasoning_config.cache_db_path,
                reasoning_config.redis_url.as_deref(),
                &config.cache.l1_redis_url,
                reasoning_config.cache_max_age_secs,
                reasoning_config.cache_max_rows,
                reasoning_config.max_reasoning_entry_bytes,
            )?
        };
        let deleted = store.clear()?;
        info!(
            deleted,
            path = %reasoning_config.cache_db_path,
            "Reasoning cache cleared"
        );
        return Ok(());
    }

    if let Err(errors) = config.validate() {
        for err in &errors {
            tracing::error!("Configuration validation error: {}", err);
        }
        anyhow::bail!(
            "Configuration validation failed with {} error(s)",
            errors.len()
        );
    }
    for warning in config.security_warnings() {
        tracing::warn!("Security warning: {}", warning);
    }
    info!("Configuration validated successfully");

    let mut server = Server::new(None)?;
    server.bootstrap();
    if config.worker_threads > 0 {
        server.configuration.threads = config.worker_threads;
    }

    let registry = Registry::new();
    global_metrics().register(&registry)?;

    let global_rate = Arc::new(pingora_limits::rate::Rate::new(
        std::time::Duration::from_secs(1),
    ));

    let metrics_service = MetricsServer {
        addr: config.metrics_addr.clone(),
        registry,
        global_rate: global_rate.clone(),
    };
    server.add_service(background_service("metrics", metrics_service));

    // Single-threaded runtime for startup block_on — avoids worker-pool deadlock before run_forever.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let upstream_profiles = config.build_upstream_profile_runtimes(&rt)?;
    let default_profile_id = config.gateway.default_upstream_profile.clone();
    let pipeline_globals = config.pipeline_globals();
    let default_profile = upstream_profiles
        .get(&default_profile_id)
        .cloned()
        .or_else(|| upstream_profiles.values().next().cloned())
        .expect("at least one upstream profile");
    let default_backends: Vec<crab_route::Backend> = default_profile
        .router
        .meta()
        .iter()
        .map(|(addr, m)| crab_route::Backend::new(m.name.clone(), *addr, 1, m.tls_sni.clone()))
        .collect();
    let router = LbRouter::new(&default_backends)?;
    let backend_count = router.meta().len();
    info!(
        backend_count = backend_count,
        profile_count = upstream_profiles.len(),
        default_profile = %default_profile_id,
        "Upstream profiles initialized"
    );

    let l1_pool = rt.block_on(async {
        bb8::Pool::builder()
            .max_size(config.cache.l1_pool_size.unwrap_or(32))
            .connection_timeout(Duration::from_secs(
                config.cache.l1_connection_timeout_secs.unwrap_or(10),
            ))
            .idle_timeout(Some(Duration::from_secs(60)))
            .build(bb8_redis::RedisConnectionManager::new(
                config.cache.l1_redis_url.clone(),
            )?)
            .await
    })?;

    let ttl_config = Arc::new(RwLock::new(TtlConfig {
        default_ttl_secs: config.cache.default_ttl_secs.unwrap_or(3600),
        model_overrides: config.cache.model_ttl_overrides.clone().unwrap_or_default(),
        consumer_overrides: config
            .cache
            .consumer_ttl_overrides
            .clone()
            .unwrap_or_default(),
        consumer_model_overrides: HashMap::new(),
    }));

    let l0_config = crab_cache::L0Config {
        max_capacity: config.cache.l0_max_capacity.unwrap_or(10_000),
        ttl_secs: config.cache.l0_ttl_secs.unwrap_or(3600),
        max_entry_bytes: config.cache.l0_max_entry_bytes.unwrap_or(0),
    };

    let tiered_cache = Arc::new(
        rt.block_on(async { TieredCache::new(l1_pool, l0_config, ttl_config.clone()).await })?,
    );

    let session_store = if config.features.mimo_session_store {
        match rt.block_on(async { crab_proxy::SessionStore::new(&config.cache.l1_redis_url).await })
        {
            Ok(store) => {
                tracing::info!("MiMo session store enabled (Redis crab:session:*)");
                Some(Arc::new(store))
            }
            Err(e) => {
                tracing::warn!(error = %e, "MiMo session store disabled: Redis connect failed");
                None
            }
        }
    } else {
        None
    };

    // Background async work (Responses chain Redis persist, etc.) must not use the
    // startup current_thread runtime handle: after block_on returns, Pingora worker
    // threads cannot drive that runtime and Handle::current() on the main thread panics.
    let background_handle = {
        let bg_rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("crab-bg")
            .enable_all()
            .build()?;
        let handle = bg_rt.handle().clone();
        Box::leak(Box::new(bg_rt));
        handle
    };

    let responses_chain_store = if config.features.responses_chain_redis {
        match rt.block_on(async {
            crab_proxy::ResponsesChainStore::new_tiered(
                config.features.responses_chain_max_capacity,
                config.features.responses_chain_ttl_secs,
                &config.cache.l1_redis_url,
                config.features.responses_chain_max_value_bytes,
                config.features.responses_chain_max_output_items,
                background_handle.clone(),
            )
            .await
        }) {
            Ok(store) => {
                tracing::info!(
                    "Responses chain store enabled (Moka L0 + Redis L1 crab:responses_chain:*)"
                );
                store
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Responses chain Redis unavailable; using Moka L0 only"
                );
                crab_proxy::ResponsesChainStore::new_l0_only(
                    config.features.responses_chain_max_capacity,
                    config.features.responses_chain_ttl_secs,
                    background_handle.clone(),
                )
            }
        }
    } else {
        crab_proxy::ResponsesChainStore::new_l0_only(
            config.features.responses_chain_max_capacity,
            config.features.responses_chain_ttl_secs,
            background_handle,
        )
    };

    let semantic_cache = if config.semantic.enabled {
        let pool = EmbedderPool::load(
            &config.semantic.model_path,
            &config.semantic.tokenizer_path,
            config.semantic.max_concurrent_embeds,
            config.semantic.model_sha256.as_deref(),
        )?;

        let store = VectorStore::new(
            &config.semantic.qdrant_url,
            &config.semantic.collection_name,
            config.semantic.vector_size.unwrap_or(384),
        );

        let store = rt.block_on(store)?;

        let cache = SemanticCache::new(
            Arc::new(pool),
            store,
            config.semantic.similarity_threshold.unwrap_or(0.95),
            config.semantic.ttl_secs.unwrap_or(7200),
        );

        Some(Arc::new(rt.block_on(cache)?))
    } else {
        None
    };

    let conn_config = config.connection.clone().unwrap_or_default();

    let reasoning_config = config.reasoning.clone().unwrap_or_default();
    let reasoning_backend_env = std::env::var("CRABCACHE_REASONING_BACKEND")
        .ok()
        .unwrap_or_else(|| reasoning_config.backend.clone());
    let reasoning_store = if reasoning_backend_env == "pg" {
        let pg_url_env = std::env::var("CRABCACHE_REASONING_PG_URL").ok();
        let pg_url = reasoning_config
            .pg_url
            .as_deref()
            .or(pg_url_env.as_deref())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "reasoning.backend = \"pg\" requires pg_url or CRABCACHE_REASONING_PG_URL"
                )
            })?;
        let rt = tokio::runtime::Runtime::new()?;
        Arc::new(rt.block_on(ReasoningBackend::open_pg(
            pg_url,
            reasoning_config.cache_max_age_secs,
            reasoning_config.cache_max_rows,
        ))?)
    } else {
        Arc::new(ReasoningBackend::from_config(
            &reasoning_config.backend,
            &reasoning_config.cache_db_path,
            reasoning_config.redis_url.as_deref(),
            &config.cache.l1_redis_url,
            reasoning_config.cache_max_age_secs,
            reasoning_config.cache_max_rows,
            reasoning_config.max_reasoning_entry_bytes,
        )?)
    };

    let upstream_base_url = default_profile.base_url.clone();
    let fallback_model = default_profile.fallback_model.clone();
    let mgmt_cfg = config.management_config();
    let mgmt_listen = mgmt_cfg.listen_addr.clone();
    let mgmt_admin_key = mgmt_cfg.admin_key.into_inner();

    let upstream_pool = default_profile.upstream_pool.clone();
    info!(
        upstream_key_count = upstream_pool.read().len(),
        default_profile = %default_profile_id,
        "Upstream key pool initialized (default profile)"
    );
    let mut legacy_client_tokens = std::collections::HashSet::new();
    let api_key = config.api_key.inner();
    if !api_key.is_empty() {
        legacy_client_tokens.insert(api_key.to_string());
    }
    let legacy_api_key_as_client_auth = config.gateway.legacy_api_key_as_client_auth;
    let auto_project_id_from_client_key = config.gateway.auto_project_id_from_client_key;

    info!(
        thinking_mode = %reasoning_config.thinking_mode,
        reasoning_effort = %reasoning_config.reasoning_effort,
        display_reasoning = reasoning_config.display_reasoning,
        collapsible_reasoning = reasoning_config.collapsible_reasoning,
        missing_reasoning_strategy = %reasoning_config.missing_reasoning_strategy,
        "Reasoning configuration"
    );

    let trace_logger = if let Some(trace_config) = &config.trace_logging {
        if trace_config.enabled {
            // Spawn PG trace writer if configured.
            let pg_sink = trace_config.pg_url.as_ref().and_then(|pg_url_str| {
                let (pg_tx, pg_rx) =
                    // Buffer sized to 100_000 to make blocking extremely unlikely.
                    // SyncSender::send() is blocking; the real fix requires changing
                    // crab-proxy's TraceLogger API to use tokio::sync::mpsc::Sender.
                    std::sync::mpsc::sync_channel::<crab_proxy::SanitizedLogEntry>(100_000);
                let pg_url_owned = pg_url_str.clone();
                match std::thread::Builder::new()
                    .name("crab-pg-trace-writer".into())
                    .spawn(move || {
                        let pg_url = pg_url_owned;
                        let rt = match tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                        {
                            Ok(rt) => rt,
                            Err(e) => {
                                tracing::warn!("Failed to create PG trace writer runtime: {}", e);
                                return;
                            }
                        };
                        rt.block_on(pg_trace_writer_loop(pg_url, pg_rx));
                    }) {
                    Ok(_) => {
                        info!(pg_url = %redact_pg_url(pg_url_str), "PG trace writer started");
                        Some(pg_tx)
                    }
                    Err(e) => {
                        tracing::warn!("Failed to spawn PG trace writer thread: {}", e);
                        None
                    }
                }
            });

            let logger = crab_proxy::TraceLogger::init(trace_config.clone(), pg_sink);

            info!(
                path = %trace_config.path,
                max_lines = trace_config.max_lines,
                max_files = trace_config.max_files,
                "Trace logging enabled"
            );
            if let Some(ref debug_cfg) = trace_config.composition_debug
                && debug_cfg.enabled
            {
                info!(
                    debug_path = %debug_cfg.path,
                    debug_max_lines = debug_cfg.max_lines,
                    debug_max_files = debug_cfg.max_files,
                    "Composition debug logging enabled"
                );
            }
            Some(Arc::new(logger))
        } else {
            None
        }
    } else {
        None
    };

    let raw_capture_logger = if let Some(rc_config) = &config.raw_capture {
        if rc_config.enabled {
            let logger = RawCaptureLogger::init(rc_config.clone());
            info!(
                dir = %rc_config.dir,
                max_index_lines = rc_config.max_index_lines,
                max_body_files = rc_config.max_body_files,
                "Raw capture logging enabled"
            );
            Some(Arc::new(logger))
        } else {
            None
        }
    } else {
        None
    };

    let runtime = RuntimeConfig::new(
        router,
        ttl_config,
        conn_config,
        config.cache.stream_cache_enabled,
        FingerprintConfig {
            version: config.cache.fingerprint_version,
            normalize_content: config.cache.fingerprint_normalize_content,
        },
        upstream_base_url,
        fallback_model,
        upstream_pool,
        upstream_profiles,
        default_profile_id,
        pipeline_globals,
        legacy_api_key_as_client_auth,
        legacy_client_tokens,
        auto_project_id_from_client_key,
    );

    let state_redis_url = config
        .state
        .redis_url
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| config.cache.l1_redis_url.clone());

    let state_store: Option<Arc<RedisStateStore>> = if config.state.is_redis() {
        let store = rt.block_on(async {
            RedisStateStore::connect(&RedisStateConfig {
                redis_url: state_redis_url.clone(),
                key_prefix: config.state.key_prefix.clone(),
                pool_size: config.state.pool_size,
                connection_timeout_secs: config.state.connection_timeout_secs,
            })
            .await
        })?;
        let store = Arc::new(store);
        let pg_url = crab_gateway::config::resolve_control_pg_url(&config);

        let redis_missing = rt.block_on(store.is_empty())?;
        let (version, snap) = if redis_missing {
            (0, crab_state::ControlPlaneSnapshot::default())
        } else {
            rt.block_on(store.load_all())?
        };

        let keys_empty = snap.keys.is_empty();
        if redis_missing || keys_empty {
            let mut recovered_from_pg = false;
            if let Some(ref pg_url) = pg_url {
                match rt.block_on(recover_from_pg_snapshot(
                    pg_url,
                    &runtime,
                    &store,
                    config.upstream.key_cooldown_secs,
                )) {
                    Ok(true) => {
                        info!("Recovered control-plane state from PG snapshot");
                        recovered_from_pg = true;
                    }
                    Ok(false) => {
                        info!("No PG snapshot available for control-plane recovery");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "PG snapshot recovery failed");
                    }
                }
            }

            if !recovered_from_pg && redis_missing {
                if let Ok(raw) = std::env::var("CRABCACHE_BOOTSTRAP_CLIENT_KEYS") {
                    for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        if runtime.keys.contains_key(token) {
                            continue;
                        }
                        let id = uuid::Uuid::new_v4().to_string();
                        runtime.keys.insert(
                            token.to_string(),
                            crab_proxy::StoredKey {
                                id,
                                name: "bootstrap".to_string(),
                                key_hash: token.to_string(),
                                enabled: true,
                                domain: None,
                                project_id: None,
                                pipeline: None,
                                upstream_profile: None,
                                max_concurrent: 0,
                                rpm_limit: 0,
                            },
                        );
                        info!(
                            key_preview = %crab_gateway::config::mask_api_key(token),
                            "Bootstrap client API key registered"
                        );
                    }
                }
                if !runtime.keys.is_empty() {
                    let snap = build_snapshot_from_runtime(&runtime);
                    rt.block_on(store.save_all(&snap))?;
                    info!(
                        keys = snap.keys.len(),
                        "Initialized Redis control plane state from bootstrap keys"
                    );
                } else {
                    tracing::warn!(
                        "Redis control plane empty; not writing empty snapshot — \
                         Admin will reconcile client keys from PostgreSQL"
                    );
                }
            } else if !recovered_from_pg && keys_empty {
                tracing::warn!(
                    version,
                    "Redis client keys empty and PG recovery unavailable; \
                     create keys via Admin or Management API"
                );
            }
        } else {
            apply_snapshot_to_runtime(&runtime, &snap, config.upstream.key_cooldown_secs)?;
            let profile_ids: Vec<String> =
                runtime.upstream_profiles.read().keys().cloned().collect();
            info!(
                version,
                keys = snap.keys.len(),
                profile_count = profile_ids.len(),
                profiles = ?profile_ids,
                snap_has_upstream_profiles = snap.upstream_profiles.is_some(),
                "Loaded control plane state from Redis"
            );
        }
        spawn_state_refresh_task(
            store.clone(),
            runtime.clone(),
            config.upstream.key_cooldown_secs,
            config.state.refresh_interval_secs,
        );
        spawn_key_state_persist_task(store.clone(), runtime.clone());
        Some(store)
    } else {
        if let Ok(raw) = std::env::var("CRABCACHE_BOOTSTRAP_CLIENT_KEYS") {
            for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if runtime.keys.contains_key(token) {
                    continue;
                }
                let id = uuid::Uuid::new_v4().to_string();
                runtime.keys.insert(
                    token.to_string(),
                    crab_proxy::StoredKey {
                        id,
                        name: "bootstrap".to_string(),
                        key_hash: token.to_string(),
                        enabled: true,
                        domain: None,
                        project_id: None,
                        pipeline: None,
                        upstream_profile: None,
                        max_concurrent: 0,
                        rpm_limit: 0,
                    },
                );
                info!(
                    key_preview = %crab_gateway::config::mask_api_key(token),
                    "Bootstrap client API key registered"
                );
            }
        }
        None
    };

    // PG control-plane snapshot writer (P1: disaster recovery).
    if let Some(pg_url_owned) = crab_gateway::config::resolve_control_pg_url(&config) {
        let rt_ref = runtime.clone();
        std::thread::Builder::new()
            .name("crab-pg-control-writer".into())
            .spawn(move || {
                let pg_url = pg_url_owned;
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::warn!("Failed to create PG control writer runtime: {}", e);
                        return;
                    }
                };
                rt.block_on(async {
                    let store = match crab_gateway::pg_control_store::PgControlStore::connect(&pg_url)
                        .await
                    {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("Failed to connect PG control store: {}", e);
                            return;
                        }
                    };
                    let interval_secs = crab_gateway::pg_control_store::snapshot_interval_secs();
                    info!(interval_secs, "PG control-plane snapshot writer started");
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
                        let snap = crab_gateway::pg_control_store::build_snapshot(&rt_ref);
                        let ver = 0i64;
                        if snap.keys.is_empty() {
                            tracing::debug!(
                                "Skipping PG control snapshot write: no client keys in runtime"
                            );
                            continue;
                        }
                        if let Err(e) = store.upsert_snapshot(&snap, ver).await {
                            tracing::warn!("PG control snapshot write failed: {:#}", e);
                        }
                    }
                });
            })
            .map_err(|e| {
                tracing::error!("Failed to spawn PG control writer thread: {}", e);
            })
            .ok();
    }

    // Health checking is now handled by Pingora's LoadBalancer<Consistent> with
    // TcpHealthCheck. The LoadBalancer runs as a BackgroundService (registered below).
    //
    // The LB's built-in consecutive-threshold mechanism replaces our custom
    // BackendHealth + CircuitBreakerConfig + TCP health check thread.

    let reasoning_config_shared = Arc::new(RwLock::new(Arc::new(reasoning_config)));

    let client_key_limiter = ClientKeyLimiter::new();
    client_key_limiter.sync_all_keys(&runtime.keys);
    let client_key_rate_limiter = ClientKeyRateLimiter::new();

    let semantic_threshold = config.semantic.similarity_threshold.unwrap_or(0.95);
    let semantic_runtime: SharedSemanticRuntime =
        Arc::new(parking_lot::RwLock::new(SemanticRuntimeState::new(
            config.semantic.enabled && semantic_cache.is_some(),
            semantic_threshold,
            SemanticGateConfig {
                min_query_chars: config.semantic.min_query_chars,
                max_query_chars: config.semantic.max_query_chars,
                embed_only_on_exact_miss: config.semantic.embed_only_on_exact_miss,
            },
        )));

    let client_endpoint = Arc::new(RwLock::new({
        let snap = discover(&DiscoveryConfig::from_env());
        info!(
            gateway_url_public = ?snap.gateway_url_public,
            public_source = ?snap.public_source,
            "Client endpoint discovery at gateway startup"
        );
        snap
    }));

    let pricing_shared = Arc::new(parking_lot::RwLock::new(
        config.cache.pricing.clone().unwrap_or_default(),
    ));
    let features_shared = Arc::new(parking_lot::RwLock::new(config.features.clone()));
    let cors_enabled_state = Arc::new(AtomicBool::new(config.gateway.cors_enabled));
    let max_request_body_bytes_state =
        Arc::new(AtomicUsize::new(config.limits.max_request_body_bytes));

    // MiMo / Codex conversation-level key binding store (created if either feature enabled).
    let key_binding_store = if config.features.mimo_key_binding || config.features.codex_key_binding
    {
        let ttl = if config.features.mimo_key_binding && config.features.codex_key_binding {
            config
                .features
                .mimo_key_binding_ttl_secs
                .max(config.features.codex_key_binding_ttl_secs)
        } else if config.features.codex_key_binding {
            config.features.codex_key_binding_ttl_secs
        } else {
            config.features.mimo_key_binding_ttl_secs
        };
        Some(crab_proxy::key_binding::KeyBindingStore::new(ttl))
    } else {
        None
    };

    // Shared lockout registries (created once, shared between GatewayState and ManagementState).
    let client_lockouts = Arc::new(crab_proxy::client_lockout::ClientLockoutRegistry::new(
        crab_proxy::client_lockout::ClientLockoutConfig {
            max_attempts: config.features.client_lockout_max_attempts,
            lockout_duration: std::time::Duration::from_secs(
                config.features.client_lockout_duration_secs,
            ),
            attempt_window: std::time::Duration::from_secs(
                config.features.client_lockout_attempt_window_secs,
            ),
        },
    ));
    let model_lockouts = Arc::new(crab_proxy::model_lockout::ModelLockoutRegistry::default());

    // Create Codex quota cache (shared between gateway runtime and management)
    let codex_quota_cache = Arc::new(crab_proxy::codex_quota_cache::CodexQuotaCache::new());

    // Initialize event bus and webhook delivery
    let event_bus = Arc::new(crab_proxy::event_bus::EventBus::new(1024));
    let webhook_store = crab_gateway::webhook::WebhookStore::new();
    let webhook_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to create webhook HTTP client");

    let fault_injection = Arc::new(crab_proxy::fault_injection::FaultInjection::default());

    let test_http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("Failed to create test HTTP client");

    let mgmt_state = ManagementState {
        runtime: runtime.clone(),
        tiered_cache: tiered_cache.clone(),
        reasoning_store: reasoning_store.clone(),
        reasoning_config: reasoning_config_shared.clone(),
        admin_key: mgmt_admin_key,
        global_rate: global_rate.clone(),
        state_store: state_store.clone(),
        invalidate_all_in_progress: Arc::new(AtomicBool::new(false)),
        invalidate_job: Arc::new(Mutex::new(None)),
        invalidate_rate: Arc::new(Mutex::new(InvalidateRateState::default())),
        invalidate_scan_timeout_secs: mgmt_cfg.invalidate_scan_timeout_secs,
        client_key_limiter: client_key_limiter.clone(),
        upstream_key_cooldown_secs: config.upstream_key_cooldown_secs(),
        semantic_runtime: semantic_runtime.clone(),
        semantic_cache: semantic_cache.clone(),
        cors_enabled: cors_enabled_state.clone(),
        max_request_body_bytes: max_request_body_bytes_state.clone(),
        max_concurrent_requests: config.limits.max_concurrent_requests,
        pricing: pricing_shared.clone(),
        features: features_shared.clone(),
        client_endpoint: client_endpoint.clone(),
        client_lockouts: client_lockouts.clone(),
        model_lockouts: model_lockouts.clone(),
        webhook_store: webhook_store.clone(),
        webhook_client: webhook_client.clone(),
        codex_quota_cache: Some(codex_quota_cache.clone()),
        test_http_client,
        fault_injection: fault_injection.clone(),
        log_broadcast: Some(log_broadcast.clone()),
        log_file_path: Some(std::path::PathBuf::from("./logs/gateway.log")),
    };

    let mgmt_listen_bg = mgmt_listen.clone();
    server.add_service(background_service(
        "management-api",
        ManagementService {
            listen_addr: mgmt_listen_bg,
            state: mgmt_state,
        },
    ));

    let request_semaphore = Arc::new(tokio::sync::Semaphore::new(
        config.limits.max_concurrent_requests,
    ));

    let deepseek_user_concurrency: DeepSeekUserConcurrencyConfig =
        config.upstream.deepseek_user_concurrency.clone();
    let deepseek_user_id_limiter = UpstreamUserIdLimiter::new(deepseek_user_concurrency.clone());

    let runtime_warmup = runtime.clone();
    // Extract LB health service handle before runtime moves into GatewayState.
    let lb_health_svc = {
        let lb_router = runtime.router.read();
        lb_router.health_service()
    };
    let coalescer = {
        let max_inflight = config.upstream.max_coalesce_inflight.unwrap_or(1000);
        let timeout = config.upstream.coalesce_timeout_secs.unwrap_or(60);
        Arc::new(RequestCoalescer::with_config(max_inflight, timeout))
    };
    let prewarm_semaphore = Arc::new(tokio::sync::Semaphore::new(4));

    // Register webhook delivery as a BackgroundService
    server.add_service(background_service(
        "webhook-delivery",
        WebhookService {
            delivery: crab_gateway::webhook::WebhookDelivery::new(
                webhook_store.clone(),
                3,    // max_retries
                1000, // retry_base_delay_ms
            ),
            event_bus: event_bus.clone(),
        },
    ));

    let state = Arc::new(GatewayState {
        runtime,
        tiered_cache,
        semantic_cache,
        semantic_runtime: semantic_runtime.clone(),
        coalescer,
        idempotency: Arc::new(crab_cache::IdempotencyStore::new(
            std::time::Duration::from_secs(5),
            10_000,
        )),
        reasoning_store,
        reasoning_config: reasoning_config_shared,
        cors_enabled: cors_enabled_state.clone(),
        trace_logger,
        raw_capture_logger,
        cache_key_namespace: config.cache.cache_key_namespace.clone(),
        pricing: pricing_shared.clone(),
        max_sse_cache_bytes: config.cache.max_sse_cache_bytes,
        max_request_body_bytes: max_request_body_bytes_state.clone(),
        request_semaphore,
        client_key_limiter,
        client_key_rate_limiter,
        deepseek_user_id_limiter,
        features: features_shared.clone(),
        seen_session_fingerprints: moka::sync::Cache::builder()
            .max_capacity(10_000)
            .time_to_live(std::time::Duration::from_secs(3600))
            .build(),
        upstream_connector: parking_lot::RwLock::new(None),
        affinity_backend_hints: moka::sync::Cache::builder()
            .max_capacity(10_000)
            .time_to_live(std::time::Duration::from_secs(3600))
            .build(),
        backend_load: Arc::new(crab_proxy::backend_state::BackendLoadRegistry::default()),
        prewarm_semaphore,
        global_rate: global_rate.clone(),
        client_endpoint: client_endpoint.clone(),
        session_store,
        key_binding_store,
        responses_chain_store,
        circuit_breakers: Arc::new(crab_proxy::circuit_breaker::CircuitBreakerRegistry::default()),
        model_lockouts,
        client_lockouts,
        event_bus: event_bus.clone(),
        codex_quota_cache: codex_quota_cache.clone(),
        fault_injection: fault_injection.clone(),
    });

    // Register prune service (rate limiter + idempotency cleanup) as a BackgroundService
    server.add_service(background_service(
        "prune",
        PruneService {
            rate_limiter: state.client_key_rate_limiter.clone(),
            idempotency: state.idempotency.clone(),
            model_lockouts: state.model_lockouts.clone(),
            client_lockouts: state.client_lockouts.clone(),
            backend_load: state.backend_load.clone(),
            key_binding_store: state.key_binding_store.clone(),
            runtime: state.runtime.clone(),
        },
    ));

    // Codex quota: wire cache to all Codex profile key pools + register background refresh
    {
        let quota_cache = state.codex_quota_cache.clone();
        wire_codex_quota_caches(&state.runtime, &quota_cache);

        // Register Codex quota refresh as a BackgroundService
        server.add_service(background_service(
            "codex-quota-refresh",
            CodexQuotaRefreshService {
                runtime: state.runtime.clone(),
                cache: quota_cache,
            },
        ));
    }

    let proxy = GatewayProxy::new(state.clone());
    let proxy_obj = http_proxy(&server.configuration, proxy);
    // Inject shared connector for direct pool pre-warm before Service takes ownership.
    *state.upstream_connector.write() = Some(proxy_obj.connector_arc());

    let mut proxy_service = Service::new("crab-gateway".to_string(), proxy_obj);
    proxy_service.add_tcp(&config.listen_addr);

    server.add_service(proxy_service);

    // Register Pingora's LoadBalancer health check as a BackgroundService.
    // This replaces the custom TCP health check thread that was previously
    // spawned in a std::thread. The LB's TcpHealthCheck uses consecutive
    // failure/success thresholds to manage backend health state.
    server.add_service(background_service("lb-health-check", lb_health_svc));

    info!(
        listen_addr = %config.listen_addr,
        metrics_addr = %config.metrics_addr,
        management_addr = %mgmt_listen,
        "CrabCache gateway starting"
    );

    // ── Backend connection pre-warm ───────────────────────────────
    // Directly establish TCP+TLS connections to all backends through Pingora's
    // connection pool. Each peer uses a different affinity key so that Ketama
    // distributes across all backends. The connections are released back to the
    // pool (not used for HTTP traffic), so subsequent real requests find a warm
    // connection ready.
    if config.features.connection_prewarm {
        let profiles = runtime_warmup.upstream_profiles.read().clone();
        if let Some(connector) = state.upstream_connector.read().clone() {
            let backends: Vec<crab_proxy::connection_prewarm::PrewarmBackend> = profiles
                .iter()
                .flat_map(|(pid, profile)| {
                    profile.router.meta().iter().map(move |(addr, meta)| {
                        (pid.clone(), *addr, meta.name.clone(), meta.tls_sni.clone())
                    })
                })
                .collect();
            server.add_service(background_service(
                "connection-prewarm",
                crab_proxy::connection_prewarm::StartupPrewarmService {
                    connector,
                    backends,
                },
            ));
        }
    }

    server.run_forever();
}

fn parse_cli_args() -> (String, bool) {
    let mut config_path = "config/gateway.toml".to_string();
    let mut clear_reasoning_cache = false;
    for arg in std::env::args().skip(1) {
        if arg == "--clear-reasoning-cache" {
            clear_reasoning_cache = true;
        } else if !arg.starts_with('-') {
            config_path = arg;
        }
    }
    (config_path, clear_reasoning_cache)
}
