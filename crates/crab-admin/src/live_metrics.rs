use crate::trace_log::TraceLogEntry;
use crate::types::{LiveMetricsBucket, LiveMetricsResponse, LiveRequestPoint};
use std::collections::HashMap;

const MAX_WINDOW_SECS: u32 = 900;
const MAX_BUCKET_SECS: u32 = 60;
const MIN_BUCKET_SECS: u32 = 1;

pub fn clamp_live_params(window_secs: u32, bucket_secs: u32) -> (u32, u32) {
    let window = window_secs.clamp(60, MAX_WINDOW_SECS);
    let bucket = bucket_secs.clamp(MIN_BUCKET_SECS, MAX_BUCKET_SECS.min(window));
    (window, bucket)
}

pub fn aggregate_live_metrics(
    entries: &[TraceLogEntry],
    consumer: &str,
    window_secs: u32,
    bucket_secs: u32,
    trace_available: bool,
    available_consumers: Vec<String>,
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
    let mut latest: Option<&TraceLogEntry> = None;

    for entry in entries {
        if !consumer_matches(entry, consumer) {
            continue;
        }
        if entry.timestamp_ms < start_ms {
            continue;
        }
        if latest.map(|l| l.timestamp_ms < entry.timestamp_ms).unwrap_or(true) {
            latest = Some(entry);
        }

        let bucket_id = entry.timestamp_ms / bucket_ms;
        let slot = acc.entry(bucket_id).or_insert_with(|| BucketAcc {
            timestamp_ms: bucket_id * bucket_ms,
            ..Default::default()
        });
        slot.request_count += 1;
        slot.e2e_sum += entry.latency_ms;
        slot.input_tokens += entry.resolved_input_tokens();
        slot.output_tokens += entry.resolved_output_tokens();
        if let Some(up) = entry.upstream_latency_ms {
            slot.upstream_sum += up;
            slot.upstream_count += 1;
        }
        if let Some(ttft) = entry.ttft_ms {
            slot.ttft_sum += ttft;
            slot.ttft_count += 1;
        }
    }

    let mut buckets = Vec::new();
    for bucket_id in start_bucket..=end_bucket {
        let timestamp_ms = bucket_id * bucket_ms;
        if let Some(slot) = acc.remove(&bucket_id) {
            buckets.push(slot.into_bucket());
        } else {
            buckets.push(LiveMetricsBucket {
                timestamp_ms,
                request_count: 0,
                e2e_latency_ms: 0.0,
                upstream_latency_ms: None,
                ttft_ms: None,
                upstream_sample_count: 0,
                ttft_sample_count: 0,
                input_tokens: 0,
                output_tokens: 0,
            });
        }
    }

    let summary = summarize_window(&buckets);

    LiveMetricsResponse {
        consumer: consumer.to_string(),
        window_secs,
        bucket_secs,
        trace_available,
        buckets,
        available_consumers,
        latest: latest.map(live_point_from_entry),
        summary,
    }
}

fn consumer_matches(entry: &TraceLogEntry, consumer: &str) -> bool {
    entry
        .consumer
        .as_deref()
        .map(|c| c == consumer)
        .unwrap_or(false)
}

#[derive(Default)]
struct BucketAcc {
    timestamp_ms: u64,
    request_count: u32,
    e2e_sum: f64,
    upstream_sum: f64,
    upstream_count: u32,
    ttft_sum: f64,
    ttft_count: u32,
    input_tokens: u64,
    output_tokens: u64,
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
            ttft_ms: if self.ttft_count > 0 {
                Some(self.ttft_sum / f64::from(self.ttft_count))
            } else {
                None
            },
            upstream_sample_count: self.upstream_count,
            ttft_sample_count: self.ttft_count,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
        }
    }
}

fn live_point_from_entry(entry: &TraceLogEntry) -> LiveRequestPoint {
    LiveRequestPoint {
        timestamp_ms: entry.timestamp_ms,
        model: entry.model.clone(),
        e2e_latency_ms: entry.latency_ms,
        upstream_latency_ms: entry.upstream_latency_ms,
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
    let mut ttft_weighted_sum = 0.0f64;
    let mut ttft_total_count = 0u32;
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;

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
        if let Some(ttft) = b.ttft_ms {
            ttft_weighted_sum += ttft * f64::from(b.ttft_sample_count);
            ttft_total_count += b.ttft_sample_count;
        }
        input_tokens += b.input_tokens;
        output_tokens += b.output_tokens;
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
        avg_ttft_ms: if ttft_total_count > 0 {
            ttft_weighted_sum / f64::from(ttft_total_count)
        } else {
            0.0
        },
        input_tokens,
        output_tokens,
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
            300,
            5,
            true,
            vec!["client-a".into(), "client-b".into()],
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
        let b = LiveMetricsBucket {
            timestamp_ms: 0,
            request_count: 0,
            e2e_latency_ms: 0.0,
            upstream_latency_ms: None,
            ttft_ms: None,
            upstream_sample_count: 0,
            ttft_sample_count: 0,
            input_tokens: 0,
            output_tokens: 0,
        };
        assert!(b.upstream_latency_ms.is_none());
        assert!(b.ttft_ms.is_none());
    }

    #[test]
    fn weighted_summary_uneven_buckets() {
        // Two buckets with very different request counts.
        // Bucket A: high upstream (100ms) with 1000 requests.
        // Bucket B: low upstream (10ms) with 1 request.
        // The weighted avg should be ~99.9ms, not 55ms (unweighted average).
        let buckets = vec![
            LiveMetricsBucket {
                timestamp_ms: 1000,
                request_count: 1000,
                e2e_latency_ms: 500.0,
                upstream_latency_ms: Some(100.0),
                ttft_ms: Some(50.0),
                upstream_sample_count: 1000,
                ttft_sample_count: 1000,
                input_tokens: 10000,
                output_tokens: 5000,
            },
            LiveMetricsBucket {
                timestamp_ms: 2000,
                request_count: 1,
                e2e_latency_ms: 200.0,
                upstream_latency_ms: Some(10.0),
                ttft_ms: Some(5.0),
                upstream_sample_count: 1,
                ttft_sample_count: 1,
                input_tokens: 10,
                output_tokens: 5,
            },
        ];
        let s = summarize_window(&buckets);
        // E2E: (500*1000 + 200*1) / 1001 ≈ 499.7
        assert!((s.avg_e2e_latency_ms - 499.7).abs() < 0.1);
        // Upstream: (100*1000 + 10*1) / 1001 ≈ 99.9 (NOT (100+10)/2 = 55)
        assert!((s.avg_upstream_latency_ms - 99.9).abs() < 0.1, "upstream={}", s.avg_upstream_latency_ms);
        // TTFT: (50*1000 + 5*1) / 1001 ≈ 49.95
        assert!((s.avg_ttft_ms - 49.95).abs() < 0.1, "ttft={}", s.avg_ttft_ms);
        assert_eq!(s.request_count, 1001);
        assert_eq!(s.input_tokens, 10010);
        assert_eq!(s.output_tokens, 5005);
    }
}
