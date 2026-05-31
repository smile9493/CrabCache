use chrono::{DateTime, FixedOffset};
use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;
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
    /// Request start → upstream response headers (MiMo prefill).
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
    /// Truncated request body snapshot (only present when `max_payload_bytes > 0`).
    #[serde(default)]
    pub request_messages_snapshot: Option<String>,
    /// Truncated response body preview (only present when `max_response_preview_bytes > 0`).
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
    /// Ketama affinity key used for backend selection.
    #[serde(default)]
    pub affinity_key: Option<String>,
    /// Categorized affinity source: `conv` | `pck` | `user` | `ip` | `unknown`.
    #[serde(default)]
    pub affinity_kind: Option<String>,
    /// Selected upstream backend node name.
    #[serde(default)]
    pub backend_name: Option<String>,
    /// Session fingerprint derived from the first user message (SHA-256 prefix).
    #[serde(default)]
    pub session_fingerprint: Option<String>,
    /// Whether this request was a coalesced follower.
    #[serde(default)]
    pub is_coalesced: bool,
    /// Client API key ID (not the consumer name).
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
    /// Upstream HTTP response status code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    /// Structured error code for diagnosis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Source of a rate-limit rejection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_source: Option<String>,
    /// Cache decision reason: "hit", "miss", "skip_stream_cache", etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_decision: Option<String>,
    /// Upstream call result classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_result: Option<String>,
    /// Per-phase durations (ms) keyed by phase name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_durations_ms: Option<serde_json::Value>,
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

/// Map a trace log entry to the admin API list DTO (shared by PG and JSONL paths).
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

/// Map trace entry to log detail DTO (shared by PG and JSONL paths).
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

pub fn trace_log_path() -> String {
    std::env::var("CRABCACHE_TRACE_LOG_PATH")
        .unwrap_or_else(|_| "/app/logs/trace.jsonl".to_string())
}

const MAX_TRACE_READ_BYTES: usize = 32 * 1024 * 1024;
/// Tail read for live-metrics polling (2s interval).
pub const LIVE_TRACE_TAIL_BYTES: usize = 2 * 1024 * 1024;

pub fn load_trace_entries(path: &str) -> Vec<TraceLogEntry> {
    let (bytes, truncated) = load_trace_bytes(path, MAX_TRACE_READ_BYTES);
    parse_trace_lines(&bytes, truncated)
}

pub fn trace_log_available(path: &str) -> bool {
    Path::new(path).is_file()
}

/// Load recent trace lines from the file tail, filtered to the time window.
pub fn load_trace_tail_for_window(
    path: &str,
    window_secs: u32,
    max_bytes: usize,
) -> Vec<TraceLogEntry> {
    if !trace_log_available(path) {
        return Vec::new();
    }
    let (bytes, truncated) = load_trace_bytes(path, max_bytes);
    let entries = parse_trace_lines(&bytes, truncated);
    filter_trace_by_window(entries, window_secs)
}

fn filter_trace_by_window(entries: Vec<TraceLogEntry>, window_secs: u32) -> Vec<TraceLogEntry> {
    if window_secs == 0 {
        return entries;
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff = now_ms.saturating_sub(u64::from(window_secs) * 1000);
    entries
        .into_iter()
        .filter(|e| e.timestamp_ms >= cutoff)
        .collect()
}

/// Load trace entries on a blocking thread (for async handlers).
pub async fn load_trace_entries_async(path: &str, hours: u32) -> Vec<TraceLogEntry> {
    let path = path.to_string();
    match tokio::task::spawn_blocking(move || load_trace_entries_for_hours(&path, hours)).await {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, "trace log spawn_blocking join failed");
            Vec::new()
        }
    }
}

pub fn load_trace_bytes(path: &str, max_bytes: usize) -> (Vec<u8>, bool) {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (Vec::new(), false),
    };
    let len = match file.metadata() {
        Ok(m) => m.len() as usize,
        Err(_) => return (Vec::new(), false),
    };
    if len == 0 {
        return (Vec::new(), false);
    }
    let read_len = len.min(max_bytes);
    let truncated = len > max_bytes;
    if truncated {
        tracing::warn!(
            path,
            bytes = len,
            max = max_bytes,
            "trace log truncated from tail"
        );
        let start = len.saturating_sub(max_bytes);
        if file.seek(SeekFrom::Start(start as u64)).is_err() {
            return (Vec::new(), false);
        }
    }
    let mut buf = vec![0u8; read_len];
    if file.read_exact(&mut buf).is_err() {
        return (Vec::new(), false);
    }
    (buf, truncated)
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

/// Parsed tail of trace.jsonl shared across live-metrics requests.
///
/// Supports incremental reads: instead of re-reading the full tail on every poll,
/// it tracks file size and only reads new bytes since the last polled offset.
/// Rotation is detected when file size shrinks or when metadata shows a new inode.
pub struct LiveTraceCache {
    /// When the cache was last refreshed.
    pub parsed_at: Option<Instant>,
    /// Tracked file size for incremental reads.
    pub file_len: u64,
    /// File mtime for rotate detection.
    pub file_mtime: Option<SystemTime>,
    #[cfg(unix)]
    /// Inode for robust rotate detection (Linux only).
    pub file_inode: Option<u64>,
    /// The live time window (seconds) that entries are filtered to.
    pub window_secs: u32,
    /// Buffered partial line bytes from the previous read (no trailing \n).
    pub partial_line: Vec<u8>,
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
            file_len: 0,
            file_mtime: None,
            #[cfg(unix)]
            file_inode: None,
            window_secs: 300,
            partial_line: Vec::new(),
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
            .field("file_len", &self.file_len)
            .field("file_mtime", &self.file_mtime)
            .field("window_secs", &self.window_secs)
            .field("partial_line_len", &self.partial_line.len())
            .field("entries", &self.entries.len())
            .field("consumers", &self.consumers.len())
            .field("key_ids", &self.key_ids.len())
            .field("session_fingerprints", &self.session_fingerprints.len())
            .finish()
    }
}

/// Load entries for the window with incremental tail support.
///
/// - **Cache hit**: file unchanged within TTL → returns cached `Arc`.
/// - **Full rebuild**: rotation detected, window changed, or cache empty → reads
///   the tail via `load_trace_bytes` and parses all lines.
/// - **Incremental**: file grew → reads only new bytes since `file_len`, appends
///   parsed entries to the in-memory buffer, prunes entries older than window.
pub fn load_live_trace_entries_cached(
    cache: &RwLock<LiveTraceCache>,
    path: &str,
    window_secs: u32,
    max_bytes: usize,
) -> Arc<Vec<TraceLogEntry>> {
    let now_ms = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let meta = std::fs::metadata(path).ok();
    let file_len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta.as_ref().and_then(|m| m.modified().ok());
    #[cfg(unix)]
    let inode = meta.as_ref().map(|m| m.ino());

    // Fast path: return cached entries if nothing changed and TTL is fresh.
    {
        let guard = cache.read();
        if let Some(at) = guard.parsed_at {
            let fresh = at.elapsed() < live_trace_cache_ttl();
            let same_window = guard.window_secs == window_secs;
            #[cfg(unix)]
            let same_file = guard.file_mtime == mtime
                && guard.file_len == file_len
                && guard.file_inode == inode;
            #[cfg(not(unix))]
            let same_file = guard.file_mtime == mtime && guard.file_len == file_len;
            if fresh && same_window && same_file {
                return Arc::clone(&guard.cached_arc);
            }
        }
    }

    let mut guard = cache.write();

    // Detect rotation: file shrunk, disappeared, inode changed, or mtime changed unexpectedly.
    let rotated = file_len < guard.file_len
        || (guard.file_len > 0 && file_len == 0)
        || (guard.file_len == 0 && file_len > 0 && guard.parsed_at.is_some());
    #[cfg(unix)]
    let rotated = rotated
        || (guard.parsed_at.is_some()
            && guard.file_inode.is_some()
            && guard.file_inode != inode
            && file_len > 0);

    if rotated || guard.window_secs != window_secs || (guard.entries.is_empty() && file_len > 0) {
        // ── Full rebuild ──────────────────────────────────────────────
        let (bytes, truncated) = load_trace_bytes(path, max_bytes);
        let mut entries = parse_trace_lines(&bytes, truncated);
        let cutoff = now_ms.saturating_sub(u64::from(window_secs) * 1000);
        entries.retain(|e| e.timestamp_ms >= cutoff);
        entries.sort_by_key(|e| e.timestamp_ms);

        let consumers: HashSet<String> = entries
            .iter()
            .filter_map(|e| e.consumer.as_ref().filter(|s| !s.is_empty()).cloned())
            .collect();
        let key_ids: HashSet<String> = entries
            .iter()
            .filter_map(|e| {
                e.upstream_key_id
                    .as_ref()
                    .or(e.client_key_id.as_ref())
                    .filter(|s| !s.is_empty())
                    .cloned()
            })
            .collect();
        let session_fingerprints: HashSet<String> = entries
            .iter()
            .filter_map(|e| {
                e.session_fingerprint
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .cloned()
            })
            .collect();

        let arc = Arc::new(entries.clone());
        *guard = LiveTraceCache {
            parsed_at: Some(Instant::now()),
            file_len,
            file_mtime: mtime,
            #[cfg(unix)]
            file_inode: inode,
            window_secs,
            partial_line: Vec::new(),
            entries,
            consumers,
            key_ids,
            session_fingerprints,
            cached_arc: arc.clone(),
        };
        return arc;
    }

    // ── Incremental: read new bytes since last poll ───────────────────
    let mut entries_changed = false;
    if file_len > guard.file_len {
        entries_changed = true;
        let read_size = (file_len - guard.file_len) as usize;
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return Arc::clone(&guard.cached_arc),
        };
        if file.seek(SeekFrom::Start(guard.file_len)).is_err() {
            return Arc::clone(&guard.cached_arc);
        }
        let mut buf = vec![0u8; read_size];
        if file.read_exact(&mut buf).is_err() {
            return Arc::clone(&guard.cached_arc);
        }

        // Combine leftover partial line with new bytes, then split on \n.
        let mut combined = Vec::new();
        std::mem::swap(&mut combined, &mut guard.partial_line);
        combined.extend_from_slice(&buf);

        // Process the text. We use the fact that JSONL is valid UTF-8.
        let text = match std::str::from_utf8(&combined) {
            Ok(t) => t,
            Err(_) => return Arc::clone(&guard.cached_arc),
        };
        let has_trailing_newline = text.ends_with('\n');

        // Split into lines; the last "line" without \n is partial.
        let mut lines: Vec<&str> = text.lines().collect();
        if !has_trailing_newline
            && !lines.is_empty()
            && let Some(partial) = lines.pop()
        {
            guard.partial_line = partial.as_bytes().to_vec();
        }

        let cutoff = now_ms.saturating_sub(u64::from(window_secs) * 1000);
        for line in lines {
            if !line.is_empty()
                && let Ok(entry) = serde_json::from_str::<TraceLogEntry>(line)
                && entry.timestamp_ms >= cutoff
            {
                if let Some(c) = entry.consumer.as_ref().filter(|s| !s.is_empty()) {
                    guard.consumers.insert(c.clone());
                }
                if let Some(k) = entry
                    .upstream_key_id
                    .as_ref()
                    .or(entry.client_key_id.as_ref())
                    .filter(|s| !s.is_empty())
                {
                    guard.key_ids.insert(k.clone());
                }
                if let Some(fp) = entry.session_fingerprint.as_ref().filter(|s| !s.is_empty()) {
                    guard.session_fingerprints.insert(fp.clone());
                }
                guard.entries.push(entry);
            }
        }

        guard.file_len = file_len;
    }

    guard.file_mtime = mtime;
    guard.parsed_at = Some(Instant::now());

    // Prune entries older than the window.
    let cutoff = now_ms.saturating_sub(u64::from(window_secs) * 1000);
    let before_retain = guard.entries.len();
    guard.entries.retain(|e| e.timestamp_ms >= cutoff);
    if guard.entries.len() != before_retain {
        entries_changed = true;
    }

    // Cap memory: keep at most 50_000 entries.
    const MAX_LIVE_ENTRIES: usize = 50_000;
    let len = guard.entries.len();
    if len > MAX_LIVE_ENTRIES {
        guard.entries.drain(0..len - MAX_LIVE_ENTRIES);
        entries_changed = true;
    }

    // Rebuild consumer set after pruning.
    if entries_changed {
        let new_consumers: HashSet<String> = guard
            .entries
            .iter()
            .filter_map(|e| e.consumer.as_ref().filter(|s| !s.is_empty()).cloned())
            .collect();
        guard.consumers = new_consumers;
        let new_key_ids: HashSet<String> = guard
            .entries
            .iter()
            .filter_map(|e| {
                e.upstream_key_id
                    .as_ref()
                    .or(e.client_key_id.as_ref())
                    .filter(|s| !s.is_empty())
                    .cloned()
            })
            .collect();
        guard.key_ids = new_key_ids;
        let new_fps: HashSet<String> = guard
            .entries
            .iter()
            .filter_map(|e| {
                e.session_fingerprint
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .cloned()
            })
            .collect();
        guard.session_fingerprints = new_fps;
    }

    if entries_changed {
        let arc = Arc::new(guard.entries.clone());
        guard.cached_arc = Arc::clone(&arc);
        arc
    } else {
        Arc::clone(&guard.cached_arc)
    }
}

/// True when live metrics should read from PG instead of JSONL tail.
pub fn live_trace_prefers_pg(path: &str, pg: Option<&crate::pg::PgStore>) -> bool {
    pg.is_some()
        && (std::env::var("CRABCACHE_LIVE_TRACE_SOURCE")
            .map(|v| v.eq_ignore_ascii_case("pg"))
            .unwrap_or(false)
            || !trace_log_available(path))
}

/// Whether trace data can be served (JSONL on disk or PG configured for fallback).
pub fn trace_source_available(path: &str, pg: Option<&crate::pg::PgStore>) -> bool {
    trace_log_available(path) || live_trace_prefers_pg(path, pg)
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
        file_len: 0,
        file_mtime: None,
        #[cfg(unix)]
        file_inode: None,
        window_secs,
        partial_line: Vec::new(),
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

/// Live trace entries: JSONL incremental tail, or PG window query when configured.
pub async fn load_live_trace_entries_auto(
    state: &std::sync::Arc<crate::state::AppState>,
    window_secs: u32,
) -> Arc<Vec<TraceLogEntry>> {
    let path = trace_log_path();
    let pg = state.pg_store.read().clone();
    if live_trace_prefers_pg(&path, pg.as_ref()) {
        if let Some(ref pg) = pg {
            return load_live_trace_entries_from_pg(&state.live_trace_cache, pg, window_secs).await;
        }
    }
    let state_clone = std::sync::Arc::clone(state);
    let path = path.clone();
    tokio::task::spawn_blocking(move || {
        load_live_trace_entries_cached(
            &state_clone.live_trace_cache,
            &path,
            window_secs,
            LIVE_TRACE_TAIL_BYTES,
        )
    })
    .await
    .unwrap_or_else(|_| Arc::new(Vec::new()))
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

pub fn parse_trace_lines(slice: &[u8], truncated: bool) -> Vec<TraceLogEntry> {
    let text = std::str::from_utf8(slice).unwrap_or("");
    let mut lines = text.lines();
    if truncated {
        lines.next(); // drop likely partial first line after tail truncation
    }
    lines
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn load_trace_entries_for_hours(path: &str, hours: u32) -> Vec<TraceLogEntry> {
    let entries = load_trace_entries(path);
    filter_trace_by_hours(entries, hours)
}

pub fn load_recent_trace_entries(path: &str, limit: usize) -> Vec<TraceLogEntry> {
    let mut entries = load_trace_entries(path);
    entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp_ms));
    entries.truncate(limit);
    entries
}

// ---------------------------------------------------------------------------
// Archive scanning: discover rotated trace files, merge + paginate.
// ---------------------------------------------------------------------------

/// A discovered trace source file.
#[derive(Debug, Clone)]
pub struct TraceSource {
    pub path: String,
    /// File modification time in ms; used for ordering merge.
    pub mtime_ms: u64,
}

/// List all trace source files in the directory of `base_path`.
///
/// The default glob is `{dir}/trace.jsonl*` to catch the active file plus
/// all rotated archives (e.g. `trace.jsonl.20250522_120000`).
pub fn list_trace_sources(base_path: &str) -> Vec<TraceSource> {
    let dir = std::path::Path::new(base_path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let base_name = std::path::Path::new(base_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("trace.jsonl");

    let mut sources = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = match name.to_str() {
                Some(s) => s,
                None => continue,
            };
            // Match base name or base name with a suffix (rotated).
            // Reject common non-rotation extensions to avoid picking up temp files.
            let is_rotation = name_str == base_name
                || (name_str.starts_with(&format!("{}.", base_name))
                    && !name_str.ends_with(".tmp")
                    && !name_str.ends_with(".bak")
                    && !name_str.ends_with('.'));
            if is_rotation
                && let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                sources.push(TraceSource {
                    path: entry.path().to_string_lossy().to_string(),
                    mtime_ms: mtime,
                });
            }
        }
    }
    // Sort newest-first so the active file is scanned first.
    sources.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));
    sources
}

/// Options for loading trace entries from multiple sources.
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

/// Parse a cursor string `timestamp_ms:request_hash` into `(ts, request_hash)`.
fn parse_cursor(cursor: &str) -> (u64, String) {
    if let Some((ts_str, hash)) = cursor.split_once(':') {
        let ts = ts_str.parse::<u64>().unwrap_or(u64::MAX);
        (ts, hash.to_string())
    } else {
        (u64::MAX, String::new())
    }
}

/// Load trace entries from multiple source files, respecting options.
///
/// Scans all sources from `list_trace_sources` (active + rotated archives).
fn entry_matches_opts(
    e: &TraceLogEntry,
    opts: &TraceLoadOpts,
    cursor_ts: u64,
    cursor_hash: &str,
) -> bool {
    if let Some(from) = opts.from_ms
        && e.timestamp_ms < from
    {
        return false;
    }
    if let Some(to) = opts.to_ms
        && e.timestamp_ms > to
    {
        return false;
    }
    if let Some(ref consumer) = opts.consumer
        && !consumer.is_empty()
    {
        let entry_consumer = e.consumer.as_deref().unwrap_or("");
        if entry_consumer != consumer.as_str() {
            return false;
        }
    }
    if let Some(ref model) = opts.model
        && !model.is_empty()
        && e.model != model.as_str()
    {
        return false;
    }
    if let Some(ref cache_tier) = opts.cache_tier
        && !cache_tier.is_empty()
    {
        let tier = e.cache_tier.as_deref().unwrap_or("");
        if tier != cache_tier.as_str() {
            return false;
        }
    }
    if let Some(ref request_hash) = opts.request_hash
        && !request_hash.is_empty()
        && e.request_hash != request_hash.as_str()
    {
        return false;
    }
    if let Some(lat_min) = opts.latency_min
        && e.latency_ms < lat_min
    {
        return false;
    }
    if let Some(lat_max) = opts.latency_max
        && e.latency_ms > lat_max
    {
        return false;
    }
    let total_tokens = e.resolved_input_tokens() + e.resolved_output_tokens();
    if let Some(tok_min) = opts.token_min
        && total_tokens < tok_min
    {
        return false;
    }
    if let Some(tok_max) = opts.token_max
        && total_tokens > tok_max
    {
        return false;
    }
    if cursor_ts < u64::MAX {
        // Keyset pagination for ORDER BY timestamp_ms DESC, request_hash DESC:
        // skip entries already returned (newer than cursor in sort order).
        if e.timestamp_ms > cursor_ts {
            return false;
        }
        if e.timestamp_ms == cursor_ts && e.request_hash.as_str() >= cursor_hash {
            return false;
        }
    }
    true
}

pub fn load_trace_with_opts(base_path: &str, opts: &TraceLoadOpts) -> Vec<TraceLogEntry> {
    let (cursor_ts, cursor_hash) = match &opts.cursor {
        Some(c) if !c.is_empty() => parse_cursor(c),
        _ => (u64::MAX, String::new()),
    };

    let sources = list_trace_sources(base_path);

    let mut all = Vec::new();
    for source in &sources {
        let raw = load_trace_bytes(&source.path, MAX_TRACE_READ_BYTES);
        let entries = parse_trace_lines(&raw.0, raw.1);

        // Apply filters per-source to avoid buffering non-matching entries.
        for e in entries {
            if entry_matches_opts(&e, opts, cursor_ts, &cursor_hash) {
                all.push(e);
            }
        }
    }

    // Sort newest-first; tie-break by request_hash DESC for stable cursor pagination.
    all.sort_by(|a, b| {
        b.timestamp_ms
            .cmp(&a.timestamp_ms)
            .then_with(|| b.request_hash.cmp(&a.request_hash))
    });

    if opts.limit > 0 && all.len() > opts.limit {
        all.truncate(opts.limit);
    }
    all
}

/// Keep entries with `timestamp_ms` within the last `hours` (0 = no filter).
pub fn filter_trace_by_hours(entries: Vec<TraceLogEntry>, hours: u32) -> Vec<TraceLogEntry> {
    if hours == 0 {
        return entries;
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff = now_ms.saturating_sub(u64::from(hours) * 3_600_000);
    entries
        .into_iter()
        .filter(|e| e.timestamp_ms >= cutoff)
        .collect()
}

pub fn find_trace_entry(path: &str, id: &str) -> Option<TraceLogEntry> {
    // First check the active file.
    if Path::new(path).exists()
        && let Some(e) = load_trace_entries(path).into_iter().find(|e| e.id() == id)
    {
        return Some(e);
    }
    // Fall back to archive sources.
    for source in list_trace_sources(path) {
        if source.path == path {
            continue; // already scanned above
        }
        let raw = load_trace_bytes(&source.path, MAX_TRACE_READ_BYTES);
        let entries = parse_trace_lines(&raw.0, raw.1);
        if let Some(e) = entries.into_iter().find(|e| e.id() == id) {
            return Some(e);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// PG-backed query functions (used when pg_store is available)
// ---------------------------------------------------------------------------

/// Load trace entries from PG with the same options as `load_trace_with_opts`.
pub async fn load_trace_with_opts_pg(
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
            tracing::warn!(error = %e, "PG trace query failed, falling back to empty");
            Vec::new()
        }
    }
}

/// Find a single trace entry from PG by composite key or legacy bare hash.
pub async fn find_trace_entry_pg(pg: &crate::pg::PgStore, id: &str) -> Option<TraceLogEntry> {
    if let Some((hash, ts)) = parse_trace_entry_id(id) {
        if let Ok(Some(entry)) = pg.find_trace_log(&hash, ts).await {
            return Some(entry);
        }
    }
    pg.find_trace_log_by_hash(id).await.ok().flatten()
}

/// Load trace entries from PG for a time window (hours).
pub async fn load_trace_entries_async_pg(
    pg: &crate::pg::PgStore,
    hours: u32,
) -> Vec<TraceLogEntry> {
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

// ---------------------------------------------------------------------------
// Auto-detect helpers (PG-first, JSONL fallback)
// These wrap the PG/JSONL dispatch so route handler bodies don't reference
// PgStore directly, avoiding axum version-conflict type leakage.
// ---------------------------------------------------------------------------

pub async fn load_trace_with_opts_auto(
    pg: Option<crate::pg::PgStore>,
    trace_path: &str,
    opts: &TraceLoadOpts,
) -> Vec<TraceLogEntry> {
    if let Some(ref pg) = pg {
        load_trace_with_opts_pg(pg, opts).await
    } else {
        load_trace_with_opts(trace_path, opts)
    }
}

pub async fn find_trace_entry_auto(
    pg: Option<crate::pg::PgStore>,
    id: &str,
    trace_path: &str,
) -> Option<TraceLogEntry> {
    if let Some(ref pg) = pg {
        if let Some(entry) = find_trace_entry_pg(pg, id).await {
            return Some(entry);
        }
    }
    find_trace_entry(trace_path, id)
}

pub async fn load_trace_entries_auto(
    pg: Option<crate::pg::PgStore>,
    trace_path: &str,
    hours: u32,
) -> Vec<TraceLogEntry> {
    if let Some(ref pg) = pg {
        load_trace_entries_async_pg(pg, hours).await
    } else {
        load_trace_entries_async(trace_path, hours).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_trace_by_hours_keeps_recent() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let old = TraceLogEntry {
            timestamp_ms: now_ms.saturating_sub(48 * 3_600_000),
            request_hash: "a".into(),
            content_length: 1,
            semantic_cluster: 0,
            conversation_id: None,
            consumer: None,
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
        };
        let new = TraceLogEntry {
            timestamp_ms: now_ms.saturating_sub(3_600_000),
            request_hash: "b".into(),
            content_length: 1,
            semantic_cluster: 0,
            conversation_id: None,
            consumer: Some("c".into()),
            model: "m".into(),
            prompt_tokens: 1,
            latency_ms: 1.0,
            upstream_latency_ms: None,
            prefill_ms: None,
            pre_header_ms: None,
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_hit: true,
            cache_tier: Some("L0_moka".into()),
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
        };
        let filtered = filter_trace_by_hours(vec![old, new], 24);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].request_hash, "b");
    }

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
    fn tail_read_only_loads_suffix() {
        let dir = std::env::temp_dir().join(format!("crab_trace_tail_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("trace.jsonl");
        let prefix = "{\"ignored\":true}\n".repeat(2000);
        let suffix = "{\"timestamp_ms\":1,\"request_hash\":\"z\",\"content_length\":1,\"semantic_cluster\":0,\"model\":\"m\",\"prompt_tokens\":1,\"latency_ms\":1.0,\"cache_hit\":false}\n";
        std::fs::write(&path, format!("{prefix}{suffix}")).unwrap();
        let (bytes, truncated) = load_trace_bytes(path.to_str().unwrap(), suffix.len() + 4);
        assert!(truncated);
        let entries = parse_trace_lines(&bytes, truncated);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].request_hash, "z");
        let _ = std::fs::remove_dir_all(&dir);
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
            },
        ];
        assert_eq!(distinct_consumers(&entries, 10), vec!["b", "a"]);
    }

    // ── Incremental tail tests ─────────────────────────────────────

    fn make_jsonl_line(ts: u64, consumer: &str) -> String {
        format!(
            r#"{{"timestamp_ms":{},"request_hash":"h{}","content_length":1,"semantic_cluster":0,"consumer":"{}","model":"m","prompt_tokens":1,"latency_ms":100.0,"upstream_latency_ms":null,"ttft_ms":null,"input_tokens":10,"output_tokens":5,"cache_hit":false,"cache_tier":null}}{}"#,
            ts, ts, consumer, "\n"
        )
    }

    #[test]
    fn incremental_tail_appends_new_lines() {
        let dir = std::env::temp_dir().join(format!("crab_inc_tail_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("trace.jsonl");

        let cache: RwLock<LiveTraceCache> = RwLock::new(LiveTraceCache::default());
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write initial 3 lines.
        let initial = format!(
            "{}{}{}",
            make_jsonl_line(now_ms - 4000, "alpha"),
            make_jsonl_line(now_ms - 3000, "beta"),
            make_jsonl_line(now_ms - 2000, "alpha"),
        );
        std::fs::write(&path, &initial).unwrap();

        // First call: full rebuild.
        let entries1 = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);
        assert_eq!(entries1.len(), 3, "all three initial entries loaded");

        let consumers1 = live_distinct_consumers(&cache);
        assert!(consumers1.contains(&"alpha".to_string()));
        assert!(consumers1.contains(&"beta".to_string()));

        // Append 2 more lines.
        let append = format!(
            "{}{}",
            make_jsonl_line(now_ms - 1000, "gamma"),
            make_jsonl_line(now_ms - 500, "alpha"),
        );
        std::fs::write(&path, format!("{initial}{append}")).unwrap();

        // Second call: should be incremental, only parse new lines.
        let entries2 = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);
        assert_eq!(
            entries2.len(),
            5,
            "all five entries after incremental append"
        );

        let consumers2 = live_distinct_consumers(&cache);
        assert!(consumers2.contains(&"alpha".to_string()));
        assert!(consumers2.contains(&"beta".to_string()));
        assert!(consumers2.contains(&"gamma".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incremental_tail_detects_rotation() {
        let dir = std::env::temp_dir().join(format!("crab_inc_rot_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("trace.jsonl");

        let cache: RwLock<LiveTraceCache> = RwLock::new(LiveTraceCache::default());
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write 2 lines.
        let content = format!(
            "{}{}",
            make_jsonl_line(now_ms - 3000, "alpha"),
            make_jsonl_line(now_ms - 2000, "beta"),
        );
        std::fs::write(&path, &content).unwrap();
        let _ = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);

        // Rotate: write a shorter file (simulates rotation/truncation).
        let rotated = make_jsonl_line(now_ms - 1000, "gamma");
        std::fs::write(&path, &rotated).unwrap();

        let entries = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);
        assert_eq!(
            entries.len(),
            1,
            "rotation should cause full rebuild (shorter file)"
        );
        assert_eq!(entries[0].timestamp_ms, now_ms - 1000, "gamma's timestamp");
        assert_eq!(
            entries[0].consumer.as_deref(),
            Some("gamma"),
            "consumer is gamma"
        );

        let consumers = live_distinct_consumers(&cache);
        assert!(consumers.contains(&"gamma".to_string()));
        assert_eq!(consumers.len(), 1, "only gamma in rotated file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incremental_tail_handles_partial_line() {
        let dir = std::env::temp_dir().join(format!("crab_inc_part_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("trace.jsonl");

        let cache: RwLock<LiveTraceCache> = RwLock::new(LiveTraceCache::default());
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Stage 1: one complete line.
        let line_a = make_jsonl_line(now_ms - 3000, "alpha");
        std::fs::write(&path, &line_a).unwrap();

        // First read: parses 1 entry, no partial line.
        let entries1 = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);
        assert_eq!(entries1.len(), 1, "one complete line parsed on first read");
        let f1 = cache.read().file_len;

        // Stage 2: append a partial line whose last field has no closing brace.
        // The partial ends in the middle: after `"input_tokens":10` (no trailing comma or field).
        let partial = format!(
            r#"{{"timestamp_ms":{},"request_hash":"h_part","content_length":1,"semantic_cluster":0,"consumer":"beta","model":"m","prompt_tokens":1,"latency_ms":200.0,"upstream_latency_ms":null,"ttft_ms":null,"input_tokens":10"#,
            now_ms - 2000,
        );
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            f.write_all(partial.as_bytes()).unwrap();
        }
        assert!(
            path.metadata().unwrap().len() > f1,
            "file grew with partial line"
        );

        // Second read: incremental, partial line stored but not parsed.
        let entries2 = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);
        assert_eq!(
            entries2.len(),
            1,
            "only original entry — partial line buffered"
        );
        assert!(
            !cache.read().partial_line.is_empty(),
            "partial_line should be non-empty"
        );

        // Stage 3: complete the partial line + append another complete line.
        // The completion continues from after `"input_tokens":10` and closes the JSON object,
        // then puts gamma_line on its own line (must be separate from the completed beta JSON).
        let completion = r#","output_tokens":5,"cache_hit":false,"cache_tier":null}"#.to_string();
        let gamma_line = make_jsonl_line(now_ms - 1000, "gamma");
        let stage3_bytes = [completion.as_bytes(), b"\n", gamma_line.as_bytes()].concat();
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            f.write_all(&stage3_bytes).unwrap();
        }

        // Third read: combines partial_line + new bytes → parses beta and gamma.
        let entries3 = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);
        assert_eq!(
            entries3.len(),
            3,
            "all three entries after partial resolved"
        );

        let consumers = live_distinct_consumers(&cache);
        assert!(consumers.contains(&"alpha".to_string()));
        assert!(consumers.contains(&"beta".to_string()));
        assert!(consumers.contains(&"gamma".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incremental_cache_hit_returns_fast_path() {
        let dir = std::env::temp_dir().join(format!("crab_inc_hit_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("trace.jsonl");

        let cache: RwLock<LiveTraceCache> = RwLock::new(LiveTraceCache::default());
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let line = make_jsonl_line(now_ms - 2000, "alpha");
        std::fs::write(&path, &line).unwrap();

        // First call populates cache.
        let entries1 = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);

        // Second call without changes: should return the same Arc.
        let entries2 = load_live_trace_entries_cached(&cache, path.to_str().unwrap(), 300, 65536);

        // Both should contain the same data and Arc should be shared.
        assert_eq!(entries1.len(), entries2.len());
        assert_eq!(
            entries1.as_ptr(),
            entries2.as_ptr(),
            "cache hit should return same Arc"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn make_trace_line(ts: u64, hash: &str) -> String {
        format!(
            r#"{{"timestamp_ms":{},"request_hash":"{}","content_length":1,"semantic_cluster":0,"model":"m","prompt_tokens":1,"latency_ms":1.0,"cache_hit":false}}"#,
            ts, hash
        )
    }

    #[test]
    fn cursor_pagination_same_timestamp_different_hashes() {
        let dir = std::env::temp_dir().join(format!("crab_cursor_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trace.jsonl");
        let ts = 1_700_000_000_000u64;
        let content = format!(
            "{}\n{}\n{}\n",
            make_trace_line(ts, "cccccccccccccccc"),
            make_trace_line(ts, "bbbbbbbbbbbbbbbb"),
            make_trace_line(ts, "aaaaaaaaaaaaaaaa"),
        );
        std::fs::write(&path, content).unwrap();

        let base = path.to_string_lossy().into_owned();
        let page1 = load_trace_with_opts(
            &base,
            &TraceLoadOpts {
                limit: 1,
                ..Default::default()
            },
        );
        assert_eq!(page1.len(), 1);
        assert_eq!(page1[0].request_hash, "cccccccccccccccc");

        let cursor = format!("{}:{}", page1[0].timestamp_ms, page1[0].request_hash);
        let page2 = load_trace_with_opts(
            &base,
            &TraceLoadOpts {
                limit: 1,
                cursor: Some(cursor),
                ..Default::default()
            },
        );
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].request_hash, "bbbbbbbbbbbbbbbb");

        let cursor2 = format!("{}:{}", page2[0].timestamp_ms, page2[0].request_hash);
        let page3 = load_trace_with_opts(
            &base,
            &TraceLoadOpts {
                limit: 1,
                cursor: Some(cursor2),
                ..Default::default()
            },
        );
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].request_hash, "aaaaaaaaaaaaaaaa");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cursor_pagination_cross_timestamp() {
        let dir = std::env::temp_dir().join(format!("crab_cursor_xts_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trace.jsonl");
        let content = format!(
            "{}\n{}\n",
            make_trace_line(100, "aaaaaaaaaaaaaaaa"),
            make_trace_line(99, "bbbbbbbbbbbbbbbb"),
        );
        std::fs::write(&path, content).unwrap();

        let base = path.to_string_lossy().into_owned();
        let page1 = load_trace_with_opts(
            &base,
            &TraceLoadOpts {
                limit: 1,
                ..Default::default()
            },
        );
        assert_eq!(page1.len(), 1);
        assert_eq!(page1[0].timestamp_ms, 100);
        assert_eq!(page1[0].request_hash, "aaaaaaaaaaaaaaaa");

        let cursor = format!("{}:{}", page1[0].timestamp_ms, page1[0].request_hash);
        let page2 = load_trace_with_opts(
            &base,
            &TraceLoadOpts {
                limit: 1,
                cursor: Some(cursor),
                ..Default::default()
            },
        );
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].timestamp_ms, 99);
        assert_eq!(page2[0].request_hash, "bbbbbbbbbbbbbbbb");

        let _ = std::fs::remove_dir_all(&dir);
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
        };
        let log = trace_entry_to_request_log(&entry);
        assert_eq!(log.id, "abc123-1700000000000");
        assert_eq!(log.cache_status, "L0");
        assert_eq!(log.upstream_user_id.as_deref(), Some("u1"));
    }

    #[test]
    fn load_trace_without_cursor_scans_archive() {
        let dir = std::env::temp_dir().join(format!("crab_archive_scan_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let base = dir.join("trace.jsonl");
        let archive = dir.join("trace.jsonl.1700000000");

        let old_line = serde_json::json!({
            "request_hash": "oldhasholdhasholdhasholdhasholdhashold",
            "timestamp_ms": 100,
            "content_length": 1,
            "semantic_cluster": 0,
            "model": "m",
            "prompt_tokens": 1,
            "latency_ms": 1.0,
            "cache_hit": false,
        });
        let new_line = serde_json::json!({
            "request_hash": "newhashnewhashnewhashnewhashnewhashnew",
            "timestamp_ms": 200,
            "content_length": 1,
            "semantic_cluster": 0,
            "model": "m",
            "prompt_tokens": 1,
            "latency_ms": 1.0,
            "cache_hit": false,
        });
        std::fs::write(&archive, format!("{old_line}\n")).unwrap();
        std::fs::write(&base, format!("{new_line}\n")).unwrap();

        let entries = load_trace_with_opts(
            base.to_str().unwrap(),
            &TraceLoadOpts {
                limit: 10,
                ..Default::default()
            },
        );
        let hashes: Vec<_> = entries.iter().map(|e| e.request_hash.as_str()).collect();
        assert!(hashes.contains(&"oldhasholdhasholdhasholdhasholdhashold"));
        assert!(hashes.contains(&"newhashnewhashnewhashnewhashnewhashnew"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_trace_entry_id_splits_hash_and_timestamp() {
        assert_eq!(
            parse_trace_entry_id("deadbeef-1700000000000"),
            Some(("deadbeef".to_string(), 1_700_000_000_000))
        );
        assert_eq!(parse_trace_entry_id("nohyphen"), None);
    }

    #[test]
    fn trace_source_available_without_jsonl_or_pg() {
        let dir = std::env::temp_dir().join(format!("crab_no_trace_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let missing = dir.join("missing.jsonl");
        assert!(!trace_log_available(missing.to_str().unwrap()));
        assert!(!live_trace_prefers_pg(missing.to_str().unwrap(), None));
        assert!(!trace_source_available(missing.to_str().unwrap(), None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
