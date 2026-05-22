use chrono::{DateTime, FixedOffset};
use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
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
    #[serde(default)]
    pub ttft_ms: Option<f64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
}

impl TraceLogEntry {
    pub fn resolved_input_tokens(&self) -> u64 {
        self.input_tokens
            .unwrap_or(self.prompt_tokens as u64)
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

/// TTL for shared live trace parse cache (mtime-invalidated).
pub const LIVE_TRACE_CACHE_TTL: Duration = Duration::from_secs(1);

/// Parsed tail of trace.jsonl shared across live-metrics requests.
#[derive(Debug, Clone, Default)]
pub struct LiveTraceCache {
    parsed_at: Option<Instant>,
    file_mtime: Option<SystemTime>,
    window_secs: u32,
    max_bytes: usize,
    entries: Arc<Vec<TraceLogEntry>>,
}

/// Load entries for the window, reusing parse when mtime and params are unchanged within TTL.
pub fn load_live_trace_entries_cached(
    cache: &RwLock<LiveTraceCache>,
    path: &str,
    window_secs: u32,
    max_bytes: usize,
) -> Arc<Vec<TraceLogEntry>> {
    let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    {
        let guard = cache.read();
        if let Some(at) = guard.parsed_at {
            if at.elapsed() < LIVE_TRACE_CACHE_TTL
                && guard.window_secs == window_secs
                && guard.max_bytes == max_bytes
                && guard.file_mtime == mtime
            {
                return Arc::clone(&guard.entries);
            }
        }
    }

    let entries = if trace_log_available(path) {
        load_trace_tail_for_window(path, window_secs, max_bytes)
    } else {
        Vec::new()
    };
    let arc = Arc::new(entries);
    *cache.write() = LiveTraceCache {
        parsed_at: Some(Instant::now()),
        file_mtime: mtime,
        window_secs,
        max_bytes,
        entries: Arc::clone(&arc),
    };
    arc
}

/// Recent distinct consumer names from trace entries (newest first).
pub fn distinct_consumers(entries: &[TraceLogEntry], limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut by_time: Vec<_> = entries.iter().collect();
    by_time.sort_by_key(|e| std::cmp::Reverse(e.timestamp_ms));
    for entry in by_time {
        if let Some(c) = entry.consumer.as_ref().filter(|s| !s.is_empty()) {
            if seen.insert(c.clone()) {
                out.push(c.clone());
                if out.len() >= limit {
                    break;
                }
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
    let dir = std::path::Path::new(base_path).parent().unwrap_or(std::path::Path::new("."));
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
            if is_rotation {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
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
    pub limit: usize,
    pub cursor: Option<String>, // "timestamp_ms:request_hash"
}

/// Parse a cursor string `timestamp_ms:request_hash` into `(ts, hash)`.
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
/// The current implementation:
/// 1. Lists all sources via `list_trace_sources`.
/// 2. Reads the newest one (active) with `load_trace_bytes`.
/// 3. (Future) could walk older sources for cursor pagination.
fn entry_matches_opts(e: &TraceLogEntry, opts: &TraceLoadOpts, cursor_ts: u64, cursor_hash: &str) -> bool {
    if let Some(from) = opts.from_ms {
        if e.timestamp_ms < from {
            return false;
        }
    }
    if let Some(to) = opts.to_ms {
        if e.timestamp_ms > to {
            return false;
        }
    }
    if let Some(ref consumer) = opts.consumer {
        if !consumer.is_empty() {
            let entry_consumer = e.consumer.as_deref().unwrap_or("");
            if entry_consumer != consumer.as_str() {
                return false;
            }
        }
    }
    if cursor_ts < u64::MAX {
        if e.timestamp_ms < cursor_ts {
            return false;
        }
        if e.timestamp_ms == cursor_ts && e.id().as_str() <= cursor_hash {
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
    // For MVP, only scan the first (newest) source for common queries.
    // Cursor pagination scans all.
    let scan_all = opts.cursor.is_some();
    // Cap the entries we buffer from older sources when paginating.
    // Since sources are sorted newest-first, once we have enough entries
    // from newer sources, older sources cannot affect the top-N after sorting.
    let early_stop_cap = if scan_all && opts.limit > 0 {
        opts.limit.saturating_mul(2).max(1024)
    } else {
        usize::MAX
    };

    let mut all = Vec::new();
    for source in &sources {
        if !scan_all {
            // Common short-queries: only read newest source.
            if !all.is_empty() {
                break;
            }
        } else if all.len() >= early_stop_cap {
            // Pagination: once we've collected enough from newer sources,
            // remaining older sources cannot add entries that sort ahead.
            break;
        }

        let raw = load_trace_bytes(&source.path, MAX_TRACE_READ_BYTES);
        let entries = parse_trace_lines(&raw.0, raw.1);

        // Apply filters per-source to avoid buffering non-matching entries.
        for e in entries {
            if entry_matches_opts(&e, opts, cursor_ts, &cursor_hash) {
                all.push(e);
            }
        }
    }

    // Sort newest-first.
    all.sort_by_key(|e| std::cmp::Reverse(e.timestamp_ms));

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
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_hit: false,
            cache_tier: None,
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
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_hit: true,
            cache_tier: Some("L0_moka".into()),
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
                ttft_ms: None,
                input_tokens: None,
                output_tokens: None,
                cache_hit: false,
                cache_tier: None,
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
                ttft_ms: None,
                input_tokens: None,
                output_tokens: None,
                cache_hit: false,
                cache_tier: None,
            },
        ];
        assert_eq!(distinct_consumers(&entries, 10), vec!["b", "a"]);
    }
}

pub fn find_trace_entry(path: &str, id: &str) -> Option<TraceLogEntry> {
    // First check the active file.
    if Path::new(path).exists() {
        if let Some(e) = load_trace_entries(path).into_iter().find(|e| e.id() == id)
        {
            return Some(e);
        }
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
