use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use std::path::Path;

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
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
}

impl TraceLogEntry {
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

pub fn load_trace_entries(path: &str) -> Vec<TraceLogEntry> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub fn load_recent_trace_entries(path: &str, limit: usize) -> Vec<TraceLogEntry> {
    let mut entries = load_trace_entries(path);
    entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp_ms));
    entries.truncate(limit);
    entries
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
}

pub fn find_trace_entry(path: &str, id: &str) -> Option<TraceLogEntry> {
    if Path::new(path).exists() {
        load_trace_entries(path)
            .into_iter()
            .find(|e| e.id() == id)
    } else {
        None
    }
}
