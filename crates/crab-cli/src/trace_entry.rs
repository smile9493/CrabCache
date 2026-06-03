//! Lightweight trace JSONL entry (subset of `SanitizedLogEntry` in crab-proxy).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
/// Fields needed by CLI analyzers; extra JSON keys are ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TraceEntry {
    pub timestamp_ms: u64,
    #[serde(default)]
    pub request_hash: String,
    #[serde(default)]
    pub content_length: usize,
    #[serde(default)]
    pub semantic_cluster: u32,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt_tokens: usize,
    #[serde(default)]
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
    pub cache_hit: bool,
    #[serde(default)]
    pub cache_tier: Option<String>,
    #[serde(default)]
    pub upstream_profile_id: Option<String>,
    #[serde(default)]
    pub upstream_key_id: Option<String>,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub client_kind: Option<String>,
    #[serde(default)]
    pub session_store: Option<String>,
    #[serde(default)]
    pub stable_session_kind: Option<String>,
    #[serde(default)]
    pub affinity_kind: Option<String>,
    #[serde(default)]
    pub request_passthrough: bool,
    #[serde(default)]
    pub request_passthrough_prefix_len: Option<usize>,
    #[serde(default)]
    pub upstream_outbound_bytes: Option<usize>,
    #[serde(default)]
    pub client_outbound_bytes: Option<usize>,
    #[serde(default)]
    pub reasoning_stripped_bytes: Option<usize>,
    #[serde(default)]
    pub reasoning_mirrored_bytes: Option<usize>,
    #[serde(default)]
    pub thinking_block_stripped_bytes: Option<usize>,
    #[serde(default)]
    pub message_retire_est_tokens: Option<usize>,
    #[serde(default)]
    pub prompt_cache_hit_ratio: Option<f64>,
    #[serde(default)]
    pub retired_prefix_messages: Option<usize>,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub limit_source: Option<String>,
    #[serde(default)]
    pub upstream_result: Option<String>,
    #[serde(default)]
    pub is_coalesced: bool,
    /// Legacy / optional trace fields (may appear in older captures).
    #[serde(default)]
    pub streaming_defer: bool,
    #[serde(default)]
    pub streaming_defer_reject_reason: Option<String>,
}

impl TraceEntry {
    pub fn effective_prefill_ms(&self) -> Option<f64> {
        self.prefill_ms.or(self.pre_header_ms)
    }

    pub fn body_bucket(&self) -> &'static str {
        body_bucket(self.content_length)
    }

    pub fn has_upstream_timing(&self) -> bool {
        !self.cache_hit && (self.upstream_latency_ms.is_some() || self.prefill_ms.is_some())
    }
}

pub fn body_bucket(content_length: usize) -> &'static str {
    if content_length < 200_000 {
        "lt_200KB"
    } else if content_length < 1_000_000 {
        "200KB_1MB"
    } else {
        "ge_1MB"
    }
}

/// Load JSONL from path (`-` = stdin). Optional `tail` keeps last N lines.
pub fn load_jsonl(path: &str, tail: Option<usize>) -> Result<Vec<TraceEntry>> {
    let mut rows = Vec::new();
    if path == "-" {
        let stdin = io::stdin();
        let mut lock = stdin.lock();
        load_reader(&mut lock, &mut rows)?;
    } else {
        let file = File::open(path).with_context(|| format!("open trace file {path}"))?;
        let mut reader = BufReader::new(file);
        load_reader(&mut reader, &mut rows)?;
    }
    if let Some(n) = tail {
        if n > 0 && rows.len() > n {
            rows = rows.split_off(rows.len() - n);
        }
    }
    Ok(rows)
}

fn load_reader<R: Read>(reader: &mut R, rows: &mut Vec<TraceEntry>) -> Result<()> {
    let mut buf = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        let n = buf.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<TraceEntry>(trimmed) {
            Ok(entry) => rows.push(entry),
            Err(_) => continue,
        }
    }
    Ok(())
}

/// Load from in-memory text (e.g. SSH fetch).
pub fn load_jsonl_str(text: &str, tail: Option<usize>) -> Result<Vec<TraceEntry>> {
    let mut rows = Vec::new();
    load_reader(&mut text.as_bytes(), &mut rows)?;
    if let Some(n) = tail {
        if n > 0 && rows.len() > n {
            rows = rows.split_off(rows.len() - n);
        }
    }
    Ok(rows)
}

/// Resolve trace path: file, `-`, or fetch via target.
pub fn resolve_trace(
    path: Option<&str>,
    target: Option<&str>,
    tail: usize,
) -> Result<Vec<TraceEntry>> {
    if let Some(p) = path {
        return load_jsonl(p, Some(tail));
    }
    if let Some(t) = target {
        let text = crate::fetch::fetch_trace_tail(t, tail)?;
        return load_jsonl_str(&text, Some(tail));
    }
    anyhow::bail!("no trace input: pass a file path or --target")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_bucket_labels() {
        assert_eq!(body_bucket(100), "lt_200KB");
        assert_eq!(body_bucket(500_000), "200KB_1MB");
        assert_eq!(body_bucket(2_000_000), "ge_1MB");
    }

    #[test]
    fn load_minimal_jsonl() {
        let data = r#"{"timestamp_ms":1,"latency_ms":10.0,"cache_hit":false,"prefill_ms":5.0}
{"timestamp_ms":2,"latency_ms":20.0,"cache_hit":true}"#;
        let rows = load_jsonl_str(data, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].cache_hit);
        assert!(rows[1].cache_hit);
    }
}
