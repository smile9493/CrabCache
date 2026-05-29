use crate::trace_log::TraceLogEntry;
use crate::types::{
    LiveMetricsBucket, LiveMetricsResponse, LiveMetricsSeries, LiveRequestPoint, Ohlc,
};
use std::collections::HashMap;

const MAX_WINDOW_SECS: u32 = 30 * 24 * 3600; // 30 days
const MAX_BUCKET_SECS: u32 = 3600; // 1 hour buckets for long windows
const MIN_BUCKET_SECS: u32 = 5;
/// Maximum number of series returned by group-by to avoid unbounded growth.
const MAX_SERIES: usize = 20;

pub fn clamp_live_params(window_secs: u32, bucket_secs: u32) -> (u32, u32) {
    let window = window_secs.clamp(60, MAX_WINDOW_SECS);
    let bucket = bucket_secs.clamp(MIN_BUCKET_SECS, MAX_BUCKET_SECS.min(window));
    (window, bucket)
}

/// Build a composite grouping key from the chosen dimensions.
fn group_key_for_entry(entry: &TraceLogEntry, group_by: &[String]) -> Option<String> {
    if group_by.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(group_by.len());
    for dim in group_by {
        match dim.as_str() {
            "model" => parts.push(format!("model={}", entry.model)),
            "key_id" => {
                let kid = entry
                    .upstream_key_id
                    .as_deref()
                    .or(entry.client_key_id.as_deref())
                    .unwrap_or("unknown");
                parts.push(format!("key_id={kid}"));
            }
            "cache_hit" | "cache_status" => {
                parts.push(format!(
                    "cache={}",
                    if entry.cache_hit { "HIT" } else { "MISS" }
                ));
            }
            "backend_name" => {
                let bn = entry.backend_name.as_deref().unwrap_or("unknown");
                parts.push(format!("backend={bn}"));
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("|"))
    }
}

/// Human-readable label for a group key (first dimension value).
fn label_for_group_key(group_key: &str) -> String {
    group_key
        .split('|')
        .next()
        .and_then(|part| part.split_once('='))
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| group_key.to_string())
}

pub fn aggregate_live_metrics(
    entries: &[TraceLogEntry],
    consumer: &str,
    key_id: &str,
    session_fingerprint: &str,
    window_secs: u32,
    bucket_secs: u32,
    trace_available: bool,
    available_consumers: Vec<String>,
    group_by: &[String],
) -> LiveMetricsResponse {
    let (window_secs, bucket_secs) = clamp_live_params(window_secs, bucket_secs);
    let bucket_ms = u64::from(bucket_secs) * 1000;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let start_ms = now_ms.saturating_sub(u64::from(window_secs) * 1000);
    let start_bucket = start_ms / bucket_ms;
    let end_bucket = now_ms / bucket_ms;

    let mut acc: HashMap<u64, BucketAcc> = HashMap::new();
    let mut series_acc: HashMap<String, HashMap<u64, BucketAcc>> = HashMap::new();
    let mut latest: Option<&TraceLogEntry> = None;

    for entry in entries {
        if !consumer_matches(entry, consumer) {
            continue;
        }
        if !key_id_matches(entry, key_id) {
            continue;
        }
        if !session_fingerprint_matches(entry, session_fingerprint) {
            continue;
        }
        if entry.timestamp_ms < start_ms {
            continue;
        }
        if latest
            .map(|l| l.timestamp_ms < entry.timestamp_ms)
            .unwrap_or(true)
        {
            latest = Some(entry);
        }

        let bucket_id = entry.timestamp_ms / bucket_ms;

        // Global aggregation (always).
        let slot = acc.entry(bucket_id).or_insert_with(|| BucketAcc {
            timestamp_ms: bucket_id * bucket_ms,
            ..Default::default()
        });
        accumulate_entry(slot, entry);

        // Series aggregation (only when group_by is active).
        if let Some(gk) = group_key_for_entry(entry, group_by) {
            let series_map = series_acc.entry(gk).or_insert_with(HashMap::new);
            let s_slot = series_map.entry(bucket_id).or_insert_with(|| BucketAcc {
                timestamp_ms: bucket_id * bucket_ms,
                ..Default::default()
            });
            accumulate_entry(s_slot, entry);
        }
    }

    let mut buckets = Vec::new();
    for bucket_id in start_bucket..=end_bucket {
        let timestamp_ms = bucket_id * bucket_ms;
        if let Some(slot) = acc.remove(&bucket_id) {
            buckets.push(slot.into_bucket());
        } else {
            buckets.push(empty_bucket(timestamp_ms));
        }
    }

    let summary = summarize_window(&buckets);

    // Build series (sorted by request_count descending, capped at MAX_SERIES).
    let mut series: Vec<LiveMetricsSeries> = Vec::new();
    if !series_acc.is_empty() {
        let _first_dim = group_by.first().cloned().unwrap_or_default();
        let mut series_vec: Vec<(String, HashMap<u64, BucketAcc>)> =
            series_acc.into_iter().collect();
        series_vec.sort_by(|a, b| {
            let count_a: u32 = a.1.values().map(|s| s.request_count).sum();
            let count_b: u32 = b.1.values().map(|s| s.request_count).sum();
            count_b.cmp(&count_a)
        });
        series_vec.truncate(MAX_SERIES);
        for (idx, (gk, mut series_map)) in series_vec.into_iter().enumerate() {
            let mut s_buckets = Vec::new();
            for bucket_id in start_bucket..=end_bucket {
                let timestamp_ms = bucket_id * bucket_ms;
                if let Some(slot) = series_map.remove(&bucket_id) {
                    s_buckets.push(slot.into_bucket());
                } else {
                    s_buckets.push(empty_bucket(timestamp_ms));
                }
            }
            let s_summary = summarize_window(&s_buckets);
            let color_hint = crate::types::SERIES_COLORS
                .get(idx % crate::types::SERIES_COLORS.len())
                .unwrap_or(&"")
                .to_string();
            series.push(LiveMetricsSeries {
                label: label_for_group_key(&gk),
                group_key: gk,
                color_hint,
                buckets: s_buckets,
                summary: s_summary,
            });
        }
    }

    LiveMetricsResponse {
        consumer: consumer.to_string(),
        window_secs,
        bucket_secs,
        trace_available,
        buckets,
        available_consumers,
        latest: latest.map(live_point_from_entry),
        summary,
        series,
    }
}

fn accumulate_entry(slot: &mut BucketAcc, entry: &TraceLogEntry) {
    slot.request_count += 1;
    slot.e2e_sum += entry.latency_ms;
    let inp = entry.resolved_input_tokens();
    let out = entry.resolved_output_tokens();
    slot.input_tokens += inp;
    slot.output_tokens += out;

    // Track model, upstream key, downstream key frequency.
    if !entry.model.is_empty() {
        *slot.model_counts.entry(entry.model.clone()).or_insert(0) += 1;
    }
    if let Some(kid) = entry.upstream_key_id.as_deref().or(entry.client_key_id.as_deref()) {
        if !kid.is_empty() {
            *slot.upstream_key_counts.entry(kid.to_string()).or_insert(0) += 1;
        }
    }
    if let Some(cons) = entry.consumer.as_deref() {
        if !cons.is_empty() {
            *slot.downstream_key_counts.entry(cons.to_string()).or_insert(0) += 1;
        }
    }

    // OHLC tracking for input tokens.
    if inp > 0 {
        slot.input_token_entries.push((entry.timestamp_ms, inp));
    }
    if out > 0 {
        slot.output_token_entries.push((entry.timestamp_ms, out));
    }

    if let Some(up) = entry.upstream_latency_ms {
        slot.upstream_sum += up;
        slot.upstream_count += 1;
    }
    if let Some(pre) = entry.prefill_ms.or(entry.pre_header_ms) {
        slot.pre_header_sum += pre;
        slot.pre_header_count += 1;
    }
    if let Some(ttft) = entry.ttft_ms {
        slot.ttft_sum += ttft;
        slot.ttft_count += 1;
    }
    if entry.cache_hit {
        slot.cache_hit_count += 1;
    }
}

fn consumer_matches(entry: &TraceLogEntry, consumer: &str) -> bool {
    entry
        .consumer
        .as_deref()
        .map(|c| c == consumer)
        .unwrap_or(false)
}

fn key_id_matches(entry: &TraceLogEntry, key_id: &str) -> bool {
    if key_id.is_empty() {
        return true;
    }
    entry
        .upstream_key_id
        .as_deref()
        .or(entry.client_key_id.as_deref())
        .map(|k| k == key_id)
        .unwrap_or(false)
}

fn session_fingerprint_matches(entry: &TraceLogEntry, fp: &str) -> bool {
    if fp.is_empty() {
        return true;
    }
    entry
        .session_fingerprint
        .as_deref()
        .map(|s| s == fp)
        .unwrap_or(false)
}

#[derive(Default)]
struct BucketAcc {
    timestamp_ms: u64,
    request_count: u32,
    e2e_sum: f64,
    upstream_sum: f64,
    upstream_count: u32,
    pre_header_sum: f64,
    pre_header_count: u32,
    ttft_sum: f64,
    ttft_count: u32,
    input_tokens: u64,
    output_tokens: u64,
    cache_hit_count: u32,
    /// Raw (timestamp_ms, value) pairs for OHLC computation.
    input_token_entries: Vec<(u64, u64)>,
    output_token_entries: Vec<(u64, u64)>,
    /// Frequency maps for model, upstream key, and downstream key (consumer).
    model_counts: HashMap<String, u32>,
    upstream_key_counts: HashMap<String, u32>,
    downstream_key_counts: HashMap<String, u32>,
}

impl BucketAcc {
    fn into_bucket(self) -> LiveMetricsBucket {
        let n = f64::from(self.request_count.max(1));
        LiveMetricsBucket {
            timestamp_ms: self.timestamp_ms,
            request_count: self.request_count,
            e2e_latency_ms: if self.request_count > 0 {
                self.e2e_sum / n
            } else {
                0.0
            },
            upstream_latency_ms: if self.upstream_count > 0 {
                Some(self.upstream_sum / f64::from(self.upstream_count))
            } else {
                None
            },
            pre_header_ms: if self.pre_header_count > 0 {
                Some(self.pre_header_sum / f64::from(self.pre_header_count))
            } else {
                None
            },
            ttft_ms: if self.ttft_count > 0 {
                Some(self.ttft_sum / f64::from(self.ttft_count))
            } else {
                None
            },
            upstream_sample_count: self.upstream_count,
            pre_header_sample_count: self.pre_header_count,
            ttft_sample_count: self.ttft_count,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_hit_count: self.cache_hit_count,
            input_tokens_ohlc: compute_ohlc(&self.input_token_entries),
            output_tokens_ohlc: compute_ohlc(&self.output_token_entries),
            max_inflight: None,
            top_model: top_value(&self.model_counts),
            top_upstream_key: top_value(&self.upstream_key_counts),
            top_downstream_key: top_value(&self.downstream_key_counts),
        }
    }
}

/// Return the most frequently occurring value from a frequency map, or empty string.
fn top_value(counts: &HashMap<String, u32>) -> String {
    counts
        .iter()
        .max_by_key(|(_, cnt)| *cnt)
        .map(|(k, _)| k.clone())
        .unwrap_or_default()
}

fn compute_ohlc(entries: &[(u64, u64)]) -> Option<Ohlc> {
    if entries.is_empty() {
        return None;
    }
    // entries are already in chronological order within a bucket.
    let open = entries[0].1;
    let close = entries[entries.len() - 1].1;
    let high = entries.iter().map(|(_, v)| *v).max().unwrap_or(0);
    let low = entries.iter().map(|(_, v)| *v).min().unwrap_or(0);
    Some(Ohlc {
        open,
        high,
        low,
        close,
    })
}

fn empty_bucket(timestamp_ms: u64) -> LiveMetricsBucket {
    LiveMetricsBucket {
        timestamp_ms,
        request_count: 0,
        e2e_latency_ms: 0.0,
        upstream_latency_ms: None,
        pre_header_ms: None,
        ttft_ms: None,
        upstream_sample_count: 0,
        pre_header_sample_count: 0,
        ttft_sample_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_hit_count: 0,
        input_tokens_ohlc: None,
        output_tokens_ohlc: None,
        max_inflight: None,
        top_model: String::new(),
        top_upstream_key: String::new(),
        top_downstream_key: String::new(),
    }
}

fn live_point_from_entry(entry: &TraceLogEntry) -> LiveRequestPoint {
    LiveRequestPoint {
        timestamp_ms: entry.timestamp_ms,
        model: entry.model.clone(),
        e2e_latency_ms: entry.latency_ms,
        upstream_latency_ms: entry.upstream_latency_ms,
        pre_header_ms: entry.pre_header_ms,
        ttft_ms: entry.ttft_ms,
        input_tokens: entry.resolved_input_tokens(),
        output_tokens: entry.resolved_output_tokens(),
        cache_status: entry.cache_status_label(),
    }
}

fn summarize_window(buckets: &[LiveMetricsBucket]) -> crate::types::LiveMetricsSummary {
    let mut request_count = 0u32;
    let mut e2e_sum = 0.0f64;
    let mut upstream_weighted_sum = 0.0f64;
    let mut upstream_total_count = 0u32;
    let mut pre_header_weighted_sum = 0.0f64;
    let mut pre_header_total_count = 0u32;
    let mut ttft_weighted_sum = 0.0f64;
    let mut ttft_total_count = 0u32;
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut cache_hit_total = 0u32;

    for b in buckets {
        request_count += b.request_count;
        if b.request_count > 0 {
            e2e_sum += b.e2e_latency_ms * f64::from(b.request_count);
        }
        if let Some(up) = b.upstream_latency_ms {
            // Weight by the number of samples in this bucket
            upstream_weighted_sum += up * f64::from(b.upstream_sample_count);
            upstream_total_count += b.upstream_sample_count;
        }
        if let Some(pre) = b.pre_header_ms {
            pre_header_weighted_sum += pre * f64::from(b.pre_header_sample_count);
            pre_header_total_count += b.pre_header_sample_count;
        }
        if let Some(ttft) = b.ttft_ms {
            ttft_weighted_sum += ttft * f64::from(b.ttft_sample_count);
            ttft_total_count += b.ttft_sample_count;
        }
        input_tokens += b.input_tokens;
        output_tokens += b.output_tokens;
        cache_hit_total += b.cache_hit_count;
    }

    crate::types::LiveMetricsSummary {
        request_count,
        avg_e2e_latency_ms: if request_count > 0 {
            e2e_sum / f64::from(request_count)
        } else {
            0.0
        },
        avg_upstream_latency_ms: if upstream_total_count > 0 {
            upstream_weighted_sum / f64::from(upstream_total_count)
        } else {
            0.0
        },
        avg_pre_header_ms: if pre_header_total_count > 0 {
            pre_header_weighted_sum / f64::from(pre_header_total_count)
        } else {
            0.0
        },
        avg_ttft_ms: if ttft_total_count > 0 {
            ttft_weighted_sum / f64::from(ttft_total_count)
        } else {
            0.0
        },
        input_tokens,
        output_tokens,
        cache_hit_ratio: if request_count > 0 {
            f64::from(cache_hit_total) / f64::from(request_count)
        } else {
            0.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace_log::TraceLogEntry;

    fn entry(
        ts: u64,
        consumer: &str,
        e2e: f64,
        upstream: Option<f64>,
        input: u64,
        output: u64,
    ) -> TraceLogEntry {
        TraceLogEntry {
            timestamp_ms: ts,
            request_hash: "abc".into(),
            content_length: 10,
            semantic_cluster: 0,
            conversation_id: None,
            consumer: Some(consumer.into()),
            model: "deepseek-chat".into(),
            prompt_tokens: (input + output) as usize,
            latency_ms: e2e,
            upstream_latency_ms: upstream,
            prefill_ms: None,
            pre_header_ms: None,
            ttft_ms: None,
            input_tokens: Some(input),
            output_tokens: Some(output),
            cache_hit: upstream.is_none(),
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
        }
    }

    #[test]
    fn aggregate_filters_consumer_and_buckets() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let bucket_ms = 5_000;
        let t0 = (now_ms / bucket_ms) * bucket_ms;
        let entries = vec![
            entry(t0 + 1000, "client-a", 100.0, Some(80.0), 10, 5),
            entry(t0 + 2000, "client-b", 200.0, None, 1, 1),
            entry(t0 + 3000, "client-a", 150.0, Some(90.0), 20, 10),
        ];
        let resp = aggregate_live_metrics(
            &entries,
            "client-a",
            "",
            "",
            300,
            5,
            true,
            vec!["client-a".into(), "client-b".into()],
            &[],
        );
        assert!(resp.trace_available);
        let with_reqs = resp
            .buckets
            .iter()
            .find(|b| b.request_count > 0)
            .expect("bucket with requests");
        assert_eq!(with_reqs.upstream_latency_ms, Some(85.0));
        assert_eq!(resp.summary.request_count, 2);
        assert_eq!(resp.summary.input_tokens, 30);
        assert_eq!(resp.summary.output_tokens, 15);
        assert!(resp.summary.avg_e2e_latency_ms > 0.0);
        assert!(resp.series.is_empty(), "no series when group_by is empty");
        let active: Vec<_> = resp
            .buckets
            .iter()
            .filter(|b| b.request_count > 0)
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].request_count, 2);
    }

    #[test]
    fn empty_bucket_has_no_upstream_or_ttft() {
        let b = empty_bucket(0);
        assert!(b.upstream_latency_ms.is_none());
        assert!(b.ttft_ms.is_none());
        assert!(b.input_tokens_ohlc.is_none());
    }

    #[test]
    fn weighted_summary_uneven_buckets() {
        let buckets = vec![
            LiveMetricsBucket {
                timestamp_ms: 1000,
                request_count: 1000,
                e2e_latency_ms: 500.0,
                upstream_latency_ms: Some(100.0),
                pre_header_ms: None,
                ttft_ms: Some(50.0),
                upstream_sample_count: 1000,
                pre_header_sample_count: 0,
                ttft_sample_count: 1000,
                input_tokens: 10000,
                output_tokens: 5000,
                cache_hit_count: 200,
                input_tokens_ohlc: None,
                output_tokens_ohlc: None,
                max_inflight: None,
                top_model: String::new(),
                top_upstream_key: String::new(),
                top_downstream_key: String::new(),
            },
            LiveMetricsBucket {
                timestamp_ms: 2000,
                request_count: 1,
                e2e_latency_ms: 200.0,
                upstream_latency_ms: Some(10.0),
                pre_header_ms: None,
                ttft_ms: Some(5.0),
                upstream_sample_count: 1,
                pre_header_sample_count: 0,
                ttft_sample_count: 1,
                input_tokens: 10,
                output_tokens: 5,
                cache_hit_count: 0,
                input_tokens_ohlc: None,
                output_tokens_ohlc: None,
                max_inflight: None,
                top_model: String::new(),
                top_upstream_key: String::new(),
                top_downstream_key: String::new(),
            },
        ];
        let s = summarize_window(&buckets);
        assert!((s.avg_e2e_latency_ms - 499.7).abs() < 0.1);
        assert!(
            (s.avg_upstream_latency_ms - 99.9).abs() < 0.1,
            "upstream={}",
            s.avg_upstream_latency_ms
        );
        assert!(
            (s.avg_ttft_ms - 49.95).abs() < 0.1,
            "ttft={}",
            s.avg_ttft_ms
        );
        assert_eq!(s.request_count, 1001);
        assert_eq!(s.input_tokens, 10010);
        assert_eq!(s.output_tokens, 5005);
    }

    #[test]
    fn ohlc_computation() {
        let entries = vec![(100, 10), (200, 30), (300, 5), (400, 20)];
        let ohlc = compute_ohlc(&entries).unwrap();
        assert_eq!(ohlc.open, 10);
        assert_eq!(ohlc.high, 30);
        assert_eq!(ohlc.low, 5);
        assert_eq!(ohlc.close, 20);
    }

    #[test]
    fn ohlc_empty_returns_none() {
        assert!(compute_ohlc(&[]).is_none());
    }

    #[test]
    fn group_by_model_produces_series() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let bucket_ms = 5_000;
        let t0 = (now_ms / bucket_ms) * bucket_ms;

        let mut e1 = entry(t0 + 1000, "client-a", 100.0, Some(80.0), 10, 5);
        e1.model = "deepseek-chat".into();
        let mut e2 = entry(t0 + 2000, "client-a", 200.0, Some(90.0), 20, 10);
        e2.model = "gpt-4o".into();
        let mut e3 = entry(t0 + 3000, "client-a", 150.0, Some(85.0), 15, 8);
        e3.model = "deepseek-chat".into();

        let entries = vec![e1, e2, e3];
        let resp = aggregate_live_metrics(
            &entries,
            "client-a",
            "",
            "",
            300,
            5,
            true,
            vec!["client-a".into()],
            &["model".into()],
        );

        assert_eq!(resp.series.len(), 2, "should have 2 series by model");
        assert_eq!(resp.summary.request_count, 3);
        // deepseek-chat has 2 requests, gpt-4o has 1.
        let ds = resp
            .series
            .iter()
            .find(|s| s.label == "deepseek-chat")
            .unwrap();
        assert_eq!(ds.summary.request_count, 2);
        let gpt = resp.series.iter().find(|s| s.label == "gpt-4o").unwrap();
        assert_eq!(gpt.summary.request_count, 1);
    }
}
