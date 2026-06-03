use chrono::{DateTime, FixedOffset};
use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// China Standard Time (UTC+8), no DST.
fn beijing_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("UTC+8")
}

/// Format Unix seconds as Beijing local time for the logs UI.
pub fn format_beijing_from_unix_secs(secs: u64) -> String {
    DateTime::from_timestamp(secs as i64, 0)
        .map(|utc| utc.with_timezone(&beijing_offset()))
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "—".to_string())
}

/// Format Unix milliseconds as Beijing local time for the logs UI.
pub fn format_beijing_from_millis(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|utc| utc.with_timezone(&beijing_offset()))
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "—".to_string())
}

#[derive(Debug, Clone, Deserialize)]
pub struct TraceLogEntry {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub content_length: usize,
    pub semantic_cluster: u32,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub consumer: Option<String>,
    pub model: String,
    pub prompt_tokens: usize,
    pub latency_ms: f64,
    #[serde(default)]
    pub upstream_latency_ms: Option<f64>,
    #[serde(default)]
    pub prefill_ms: Option<f64>,
    #[serde(default)]
    pub pre_header_ms: Option<f64>,
    #[serde(default)]
    pub ttft_ms: Option<f64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub composition: Option<crab_composition::RequestComposition>,
    #[serde(default)]
    pub request_messages_snapshot: Option<String>,
    #[serde(default)]
    pub response_preview: Option<String>,
    #[serde(default)]
    pub retired_prefix_messages: Option<usize>,
    #[serde(default)]
    pub reasoning_strategy: Option<String>,
    #[serde(default)]
    pub prompt_cache_hit_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_body_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id_audit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_key_id: Option<String>,
    #[serde(default)]
    pub affinity_key: Option<String>,
    #[serde(default)]
    pub affinity_kind: Option<String>,
    #[serde(default)]
    pub backend_name: Option<String>,
    #[serde(default)]
    pub session_fingerprint: Option<String>,
    #[serde(default)]
    pub is_coalesced: bool,
    #[serde(default)]
    pub client_key_id: Option<String>,
    #[serde(default)]
    pub session_store: Option<String>,
    #[serde(default)]
    pub stable_session_kind: Option<String>,
    #[serde(default)]
    pub upstream_outbound_bytes: Option<usize>,
    #[serde(default)]
    pub request_passthrough: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_passthrough_prefix_len: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_decision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_durations_ms: Option<serde_json::Value>,
    /// Resolved downstream client IP (X-Forwarded-For / X-Real-IP / peer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    /// Detected client kind (e.g. `cursor`, `codex`, `generic`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_kind: Option<String>,
}

impl TraceLogEntry {
    pub fn resolved_input_tokens(&self) -> u64 {
        self.input_tokens.unwrap_or(self.prompt_tokens as u64)
    }

    pub fn resolved_output_tokens(&self) -> u64 {
        self.output_tokens.unwrap_or(0)
    }

    pub fn id(&self) -> String {
        format!("{}-{}", self.request_hash, self.timestamp_ms)
    }

    pub fn cache_status_label(&self) -> String {
        if let Some(tier) = self.cache_tier.as_ref().filter(|s| !s.is_empty()) {
            return tier.to_uppercase();
        }
        if self.cache_hit {
            "HIT".to_string()
        } else {
            "MISS".to_string()
        }
    }
}

/// Max chars for `response_preview` in list API (`CRABCACHE_ADMIN_LOG_LIST_PREVIEW_CHARS`).
/// Default 200; set to `0` to disable truncation.
fn list_preview_max_chars() -> Option<usize> {
    std::env::var("CRABCACHE_ADMIN_LOG_LIST_PREVIEW_CHARS")
        .ok()
        .and_then(|v| v.parse().ok())
        .or(Some(200))
}

/// Map a trace log entry to the admin API list DTO.
pub fn trace_entry_to_request_log(e: &TraceLogEntry) -> crab_admin_types::RequestLog {
    let consumer = e
        .consumer
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| e.conversation_id.clone().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "—".to_string());
    let request_payload = e.request_messages_snapshot.clone().unwrap_or_else(|| {
        let summary = serde_json::json!({
            "request_hash": e.request_hash,
            "content_length": e.content_length,
            "semantic_cluster": e.semantic_cluster,
            "input_tokens": e.resolved_input_tokens(),
            "output_tokens": e.resolved_output_tokens(),
            "cache_hit": e.cache_hit,
            "cache_tier": e.cache_tier,
        });
        serde_json::to_string_pretty(&summary).unwrap_or_default()
    });
    let response_preview = match list_preview_max_chars() {
        Some(max) if max > 0 => e
            .response_preview
            .clone()
            .unwrap_or_default()
            .chars()
            .take(max)
            .collect(),
        _ => e.response_preview.clone().unwrap_or_default(),
    };
    crab_admin_types::RequestLog {
        id: e.id(),
        timestamp: format_beijing_from_millis(e.timestamp_ms as i64),
        model: e.model.clone(),
        consumer,
        latency_ms: e.latency_ms.round() as u64,
        total_tokens: e.resolved_input_tokens() + e.resolved_output_tokens(),
        cache_status: e.cache_status_label(),
        request_payload,
        response_preview,
        input_tokens: e.input_tokens.or(Some(e.resolved_input_tokens())),
        output_tokens: e.output_tokens.or(Some(e.resolved_output_tokens())),
        ttft_ms: e.ttft_ms,
        content_length: Some(e.content_length),
        request_hash: Some(e.request_hash.clone()),
        project_id: e.project_id.clone(),
        upstream_user_id: e.upstream_user_id.clone(),
        user_id_audit: e.user_id_audit.clone(),
        upstream_key_id: e.upstream_key_id.clone(),
    }
}

/// Map trace entry to log detail DTO.
pub fn trace_entry_to_request_detail(e: &TraceLogEntry) -> crab_admin_types::RequestDetail {
    let cache_path = if e.cache_hit {
        e.cache_tier
            .clone()
            .unwrap_or_else(|| "gateway-cache".to_string())
    } else {
        "upstream".to_string()
    };
    let request_payload = e.request_messages_snapshot.clone().unwrap_or_else(|| {
        let payload = serde_json::json!({
            "request_hash": e.request_hash,
            "content_length": e.content_length,
            "semantic_cluster": e.semantic_cluster,
            "conversation_id": e.conversation_id,
            "model": e.model,
            "prompt_tokens": e.prompt_tokens,
            "latency_ms": e.latency_ms,
            "cache_hit": e.cache_hit,
            "cache_tier": e.cache_tier,
        });
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    });
    let response_body = e
        .response_preview
        .clone()
        .unwrap_or_else(|| "(未启用 body 采集)".to_string());
    crab_admin_types::RequestDetail {
        cache_path,
        request_payload,
        response_body,
        route_backend: e.backend_name.clone().unwrap_or_else(|| "—".to_string()),
        upstream_latency_ms: e.upstream_latency_ms,
        ttft_ms: e.ttft_ms,
        input_tokens: e.input_tokens,
        output_tokens: e.output_tokens,
        request_hash: Some(e.request_hash.clone()),
        semantic_cluster: Some(e.semantic_cluster),
        upstream_key_id: e.upstream_key_id.clone(),
        affinity_kind: e.affinity_kind.clone(),
        backend_name: e.backend_name.clone(),
        session_fingerprint: e.session_fingerprint.clone(),
        is_coalesced: e.is_coalesced,
        client_key_id: e.client_key_id.clone(),
        pipeline: e.pipeline.clone(),
        upstream_model: e.upstream_model.clone(),
        request_passthrough: e.request_passthrough,
        request_passthrough_prefix_len: e.request_passthrough_prefix_len,
        status_code: e.status_code,
        error_code: e.error_code.clone(),
        cache_decision: e.cache_decision.clone(),
        upstream_result: e.upstream_result.clone(),
        phase_durations_ms: e.phase_durations_ms.clone(),
    }
}

/// Trace log file path (used only for JSONL fallback buffer, not queries).
pub fn trace_log_path() -> String {
    std::env::var("CRABCACHE_TRACE_LOG_PATH")
        .unwrap_or_else(|_| "/app/logs/trace.jsonl".to_string())
}

/// Default TTL for the live trace parse cache.
pub const LIVE_TRACE_CACHE_TTL: Duration = Duration::from_secs(3);

/// Configurable TTL via environment variable `CRABCACHE_LIVE_TRACE_CACHE_TTL_SECS`.
pub fn live_trace_cache_ttl() -> Duration {
    std::env::var("CRABCACHE_LIVE_TRACE_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&s| s > 0)
        .map(Duration::from_secs)
        .unwrap_or(LIVE_TRACE_CACHE_TTL)
}

/// Live trace cache backed by PG queries.
///
/// Caches the result of a time-windowed PG query with TTL-based invalidation.
pub struct LiveTraceCache {
    /// When the cache was last refreshed.
    pub parsed_at: Option<Instant>,
    /// The live time window (seconds) that entries are filtered to.
    pub window_secs: u32,
    /// Cached entries in timestamp order (ascending).
    pub entries: Vec<TraceLogEntry>,
    /// Inline consumer HashSet for fast `live_distinct_consumers`.
    pub consumers: HashSet<String>,
    /// Inline key_id set for per-key filtering.
    pub key_ids: HashSet<String>,
    /// Inline session_fingerprint set for per-session filtering.
    pub session_fingerprints: HashSet<String>,
    /// Immutable Arc snapshot returned on cache hit.
    pub cached_arc: Arc<Vec<TraceLogEntry>>,
}

impl Default for LiveTraceCache {
    fn default() -> Self {
        Self {
            parsed_at: None,
            window_secs: 300,
            entries: Vec::new(),
            consumers: HashSet::new(),
            key_ids: HashSet::new(),
            session_fingerprints: HashSet::new(),
            cached_arc: Arc::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for LiveTraceCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveTraceCache")
            .field("parsed_at", &self.parsed_at)
            .field("window_secs", &self.window_secs)
            .field("entries", &self.entries.len())
            .field("consumers", &self.consumers.len())
            .field("key_ids", &self.key_ids.len())
            .field("session_fingerprints", &self.session_fingerprints.len())
            .finish()
    }
}

/// Parse `{request_hash}-{timestamp_ms}` detail/list IDs. Returns `(hash, ts)`.
pub fn parse_trace_entry_id(id: &str) -> Option<(String, u64)> {
    let (hash, ts_str) = id.rsplit_once('-')?;
    let ts = ts_str.parse().ok()?;
    Some((hash.to_string(), ts))
}

fn rebuild_live_cache(
    cache: &RwLock<LiveTraceCache>,
    window_secs: u32,
    entries: Vec<TraceLogEntry>,
) -> Arc<Vec<TraceLogEntry>> {
    let now_ms = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff = now_ms.saturating_sub(u64::from(window_secs) * 1000);
    let mut filtered: Vec<TraceLogEntry> = entries
        .into_iter()
        .filter(|e| e.timestamp_ms >= cutoff)
        .collect();
    filtered.sort_by_key(|e| e.timestamp_ms);

    let consumers: HashSet<String> = filtered
        .iter()
        .filter_map(|e| e.consumer.as_ref().filter(|s| !s.is_empty()).cloned())
        .collect();
    let key_ids: HashSet<String> = filtered
        .iter()
        .filter_map(|e| {
            e.upstream_key_id
                .as_ref()
                .or(e.client_key_id.as_ref())
                .filter(|s| !s.is_empty())
                .cloned()
        })
        .collect();
    let session_fingerprints: HashSet<String> = filtered
        .iter()
        .filter_map(|e| {
            e.session_fingerprint
                .as_ref()
                .filter(|s| !s.is_empty())
                .cloned()
        })
        .collect();
    let arc = Arc::new(filtered);
    *cache.write() = LiveTraceCache {
        parsed_at: Some(Instant::now()),
        window_secs,
        entries: arc.as_ref().clone(),
        consumers,
        key_ids,
        session_fingerprints,
        cached_arc: Arc::clone(&arc),
    };
    arc
}

async fn load_live_trace_entries_from_pg(
    cache: &RwLock<LiveTraceCache>,
    pg: &crate::pg::PgStore,
    window_secs: u32,
) -> Arc<Vec<TraceLogEntry>> {
    {
        let guard = cache.read();
        if guard
            .parsed_at
            .is_some_and(|t| t.elapsed() < live_trace_cache_ttl())
            && guard.window_secs == window_secs
        {
            return Arc::clone(&guard.cached_arc);
        }
    }
    let now_ms = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let from_ms = now_ms.saturating_sub(u64::from(window_secs) * 1000);
    let entries = pg
        .load_trace_logs(Some(from_ms), None, None, None, None, None, 50_000)
        .await
        .unwrap_or_default();
    rebuild_live_cache(cache, window_secs, entries)
}

/// Live trace entries from PG. Falls back to empty if PG is unavailable.
pub async fn load_live_trace_entries(
    cache: &RwLock<LiveTraceCache>,
    pg: &crate::pg::PgStore,
    window_secs: u32,
) -> Arc<Vec<TraceLogEntry>> {
    load_live_trace_entries_from_pg(cache, pg, window_secs).await
}

/// Whether trace data source is available (PG must be configured).
pub fn trace_source_available(pg: Option<&crate::pg::PgStore>) -> bool {
    pg.is_some()
}

/// Fast consumer list from the live trace cache (avoids sorting).
pub fn live_distinct_consumers(cache: &RwLock<LiveTraceCache>) -> Vec<String> {
    let guard = cache.read();
    let mut out: Vec<String> = guard.consumers.iter().cloned().collect();
    out.sort_by(|a, b| b.cmp(a));
    out
}

/// Fast key_id list from the live trace cache.
pub fn live_distinct_key_ids(cache: &RwLock<LiveTraceCache>) -> Vec<String> {
    let guard = cache.read();
    let mut out: Vec<String> = guard.key_ids.iter().cloned().collect();
    out.sort();
    out
}

/// Fast session_fingerprint list from the live trace cache.
pub fn live_distinct_session_fingerprints(cache: &RwLock<LiveTraceCache>) -> Vec<String> {
    let guard = cache.read();
    let mut out: Vec<String> = guard.session_fingerprints.iter().cloned().collect();
    out.sort();
    out
}

/// Recent distinct consumer names from trace entries (newest first).
pub fn distinct_consumers(entries: &[TraceLogEntry], limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut by_time: Vec<_> = entries.iter().collect();
    by_time.sort_by_key(|e| std::cmp::Reverse(e.timestamp_ms));
    for entry in by_time {
        if let Some(c) = entry.consumer.as_ref().filter(|s| !s.is_empty())
            && seen.insert(c.clone())
        {
            out.push(c.clone());
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// PG-backed query functions (sole query path)
// ---------------------------------------------------------------------------

/// Options for loading trace entries.
#[derive(Debug, Clone, Default)]
pub struct TraceLoadOpts {
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub consumer: Option<String>,
    pub model: Option<String>,
    pub cache_tier: Option<String>,
    pub request_hash: Option<String>,
    pub latency_min: Option<f64>,
    pub latency_max: Option<f64>,
    pub token_min: Option<u64>,
    pub token_max: Option<u64>,
    pub limit: usize,
    pub cursor: Option<String>, // "timestamp_ms:request_hash"
}

/// Load trace entries from PG with the given options.
pub async fn load_trace_with_opts(
    pg: &crate::pg::PgStore,
    opts: &TraceLoadOpts,
) -> Vec<TraceLogEntry> {
    let fetch_limit = if opts.limit > 0 {
        opts.limit.saturating_add(1).min(5000)
    } else {
        5000
    };

    match pg
        .query_trace_logs_paginated(
            opts.cursor.as_deref(),
            opts.from_ms,
            opts.to_ms,
            opts.consumer.as_deref(),
            opts.model.as_deref(),
            opts.cache_tier.as_deref(),
            opts.request_hash.as_deref(),
            opts.latency_min,
            opts.latency_max,
            opts.token_min,
            opts.token_max,
            fetch_limit,
        )
        .await
    {
        Ok((mut entries, _next_cursor)) => {
            if opts.limit > 0 && entries.len() > opts.limit {
                entries.truncate(opts.limit);
            }
            entries
        }
        Err(e) => {
            tracing::warn!(error = %e, "PG trace query failed");
            Vec::new()
        }
    }
}

/// Find a single trace entry from PG by composite key or legacy bare hash.
pub async fn find_trace_entry(pg: &crate::pg::PgStore, id: &str) -> Option<TraceLogEntry> {
    if let Some((hash, ts)) = parse_trace_entry_id(id) {
        if let Ok(Some(entry)) = pg.find_trace_log(&hash, ts).await {
            return Some(entry);
        }
    }
    pg.find_trace_log_by_hash(id).await.ok().flatten()
}

/// Load trace entries from PG for a time window (hours).
pub async fn load_trace_entries(pg: &crate::pg::PgStore, hours: u32) -> Vec<TraceLogEntry> {
    let from_ms = if hours > 0 {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Some(now_ms.saturating_sub(u64::from(hours) * 3_600_000))
    } else {
        None
    };

    match pg
        .load_trace_logs(from_ms, None, None, None, None, None, 100_000)
        .await
    {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, "PG trace analysis query failed");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beijing_offset_is_eight_hours_ahead_of_utc() {
        assert_eq!(
            format_beijing_from_unix_secs(1_704_067_200),
            "2024-01-01 08:00:00"
        );
    }

    #[test]
    fn beijing_from_millis_matches_secs() {
        assert_eq!(
            format_beijing_from_millis(1_704_067_200_000),
            "2024-01-01 08:00:00"
        );
    }

    #[test]
    fn distinct_consumers_returns_recent_unique() {
        let entries = vec![
            TraceLogEntry {
                timestamp_ms: 2,
                request_hash: "b".into(),
                content_length: 1,
                semantic_cluster: 0,
                conversation_id: None,
                consumer: Some("b".into()),
                model: "m".into(),
                prompt_tokens: 1,
                latency_ms: 1.0,
                upstream_latency_ms: None,
                prefill_ms: None,
                pre_header_ms: None,
                ttft_ms: None,
                input_tokens: None,
                output_tokens: None,
                cache_hit: false,
                cache_tier: None,
                domain: None,
                project_id: None,
                composition: None,
                request_messages_snapshot: None,
                response_preview: None,
                retired_prefix_messages: None,
                reasoning_strategy: None,
                prompt_cache_hit_ratio: None,
                upstream_profile_id: None,
                pipeline: None,
                upstream_model: None,
                client_body_user_id: None,
                upstream_user_id: None,
                user_id_audit: None,
                upstream_key_id: None,
                affinity_key: None,
                affinity_kind: None,
                backend_name: None,
                session_fingerprint: None,
                is_coalesced: false,
                client_key_id: None,
                session_store: None,
                stable_session_kind: None,
                upstream_outbound_bytes: None,
                request_passthrough: false,
                request_passthrough_prefix_len: None,
                status_code: None,
                error_code: None,
                limit_source: None,
                cache_decision: None,
                upstream_result: None,
                phase_durations_ms: None,
                client_ip: None,
                client_kind: None,
            },
            TraceLogEntry {
                timestamp_ms: 1,
                request_hash: "a".into(),
                content_length: 1,
                semantic_cluster: 0,
                conversation_id: None,
                consumer: Some("a".into()),
                model: "m".into(),
                prompt_tokens: 1,
                latency_ms: 1.0,
                upstream_latency_ms: None,
                prefill_ms: None,
                pre_header_ms: None,
                ttft_ms: None,
                input_tokens: None,
                output_tokens: None,
                cache_hit: false,
                cache_tier: None,
                domain: None,
                project_id: None,
                composition: None,
                request_messages_snapshot: None,
                response_preview: None,
                retired_prefix_messages: None,
                reasoning_strategy: None,
                prompt_cache_hit_ratio: None,
                upstream_profile_id: None,
                pipeline: None,
                upstream_model: None,
                client_body_user_id: None,
                upstream_user_id: None,
                user_id_audit: None,
                upstream_key_id: None,
                affinity_key: None,
                affinity_kind: None,
                backend_name: None,
                session_fingerprint: None,
                is_coalesced: false,
                client_key_id: None,
                session_store: None,
                stable_session_kind: None,
                upstream_outbound_bytes: None,
                request_passthrough: false,
                request_passthrough_prefix_len: None,
                status_code: None,
                error_code: None,
                limit_source: None,
                cache_decision: None,
                upstream_result: None,
                phase_durations_ms: None,
                client_ip: None,
                client_kind: None,
            },
        ];
        assert_eq!(distinct_consumers(&entries, 10), vec!["b", "a"]);
    }

    #[test]
    fn trace_entry_to_request_log_truncates_response_preview_by_default() {
        let entry = TraceLogEntry {
            request_hash: "abc123".into(),
            timestamp_ms: 1,
            content_length: 10,
            semantic_cluster: 0,
            conversation_id: None,
            consumer: None,
            model: "m".into(),
            prompt_tokens: 5,
            latency_ms: 42.0,
            upstream_latency_ms: None,
            prefill_ms: None,
            pre_header_ms: None,
            ttft_ms: None,
            cache_hit: false,
            cache_tier: None,
            domain: None,
            project_id: None,
            composition: None,
            request_messages_snapshot: None,
            response_preview: Some("x".repeat(300)),
            retired_prefix_messages: None,
            reasoning_strategy: None,
            prompt_cache_hit_ratio: None,
            upstream_profile_id: None,
            pipeline: None,
            upstream_model: None,
            client_body_user_id: None,
            input_tokens: None,
            output_tokens: None,
            upstream_user_id: None,
            user_id_audit: None,
            upstream_key_id: None,
            affinity_key: None,
            affinity_kind: None,
            backend_name: None,
            session_fingerprint: None,
            is_coalesced: false,
            client_key_id: None,
            session_store: None,
            stable_session_kind: None,
            upstream_outbound_bytes: None,
            request_passthrough: false,
            request_passthrough_prefix_len: None,
            status_code: None,
            error_code: None,
            limit_source: None,
            cache_decision: None,
            upstream_result: None,
            phase_durations_ms: None,
            client_ip: None,
            client_kind: None,
        };
        let log = trace_entry_to_request_log(&entry);
        assert_eq!(log.response_preview.chars().count(), 200);
    }

    #[test]
    fn trace_entry_to_request_log_uses_composite_id() {
        let entry = TraceLogEntry {
            request_hash: "abc123".into(),
            timestamp_ms: 1_700_000_000_000,
            content_length: 10,
            semantic_cluster: 0,
            conversation_id: None,
            consumer: None,
            model: "m".into(),
            prompt_tokens: 5,
            latency_ms: 42.0,
            upstream_latency_ms: None,
            prefill_ms: None,
            pre_header_ms: None,
            ttft_ms: None,
            cache_hit: true,
            cache_tier: Some("l0".into()),
            domain: None,
            project_id: None,
            composition: None,
            request_messages_snapshot: None,
            response_preview: None,
            retired_prefix_messages: None,
            reasoning_strategy: None,
            prompt_cache_hit_ratio: None,
            upstream_profile_id: None,
            pipeline: None,
            upstream_model: None,
            client_body_user_id: None,
            input_tokens: Some(5),
            output_tokens: Some(2),
            upstream_user_id: Some("u1".into()),
            user_id_audit: Some("injected".into()),
            upstream_key_id: Some("key-1".into()),
            affinity_key: None,
            affinity_kind: None,
            backend_name: None,
            session_fingerprint: None,
            is_coalesced: false,
            client_key_id: None,
            session_store: None,
            stable_session_kind: None,
            upstream_outbound_bytes: None,
            request_passthrough: false,
            request_passthrough_prefix_len: None,
            status_code: None,
            error_code: None,
            limit_source: None,
            cache_decision: None,
            upstream_result: None,
            phase_durations_ms: None,
            client_ip: None,
            client_kind: None,
        };
        let log = trace_entry_to_request_log(&entry);
        assert_eq!(log.id, "abc123-1700000000000");
        assert_eq!(log.cache_status, "L0");
        assert_eq!(log.upstream_user_id.as_deref(), Some("u1"));
    }

    #[test]
    fn parse_trace_entry_id_splits_hash_and_timestamp() {
        assert_eq!(
            parse_trace_entry_id("deadbeef-1700000000000"),
            Some(("deadbeef".to_string(), 1_700_000_000_000))
        );
        assert_eq!(parse_trace_entry_id("nohyphen"), None);
    }
}
