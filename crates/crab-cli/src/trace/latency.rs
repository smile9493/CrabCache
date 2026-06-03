//! Trace latency breakdown (prefill / upstream / e2e / ttft / gap).

use crate::stats::{LatencyStats, median_field};
use crate::trace_entry::TraceEntry;
use std::collections::HashMap;

pub fn print_analysis(rows: &[TraceEntry], label: &str) {
    if rows.is_empty() {
        println!("\n=== {label}: no records ===");
        return;
    }

    let stream_miss: Vec<&TraceEntry> = rows.iter().filter(|r| r.has_upstream_timing()).collect();

    println!(
        "\n=== {label}: latency ({} cache-miss with timing / {} total) ===",
        stream_miss.len(),
        rows.len()
    );

    let mut prefill = LatencyStats::new("prefill (start->hdr)");
    let mut upstream = LatencyStats::new("upstream (hdr->EOS)");
    let mut e2e = LatencyStats::new("e2e (latency_ms)");
    let mut ttft = LatencyStats::new("sse_ttft (hdr->1st chunk)");
    let mut gap = LatencyStats::new("gap (e2e - upstream)");

    let mut by_pipe: HashMap<String, Vec<&TraceEntry>> = HashMap::new();
    let mut by_bucket: HashMap<&str, Vec<&TraceEntry>> = HashMap::new();
    let mut by_client: HashMap<String, Vec<&TraceEntry>> = HashMap::new();

    for r in &stream_miss {
        let dur = r.latency_ms;
        let up = r.upstream_latency_ms;
        let pf = r.effective_prefill_ms();
        e2e.add(Some(dur));
        prefill.add(pf);
        upstream.add(up);
        ttft.add(r.ttft_ms);
        if let Some(up) = up {
            gap.add(Some(dur - up));
        }
        let pipe = r.pipeline.clone().unwrap_or_else(|| "unknown".into());
        by_pipe.entry(pipe).or_default().push(r);
        by_bucket.entry(r.body_bucket()).or_default().push(r);
        let ck = r.client_kind.clone().unwrap_or_else(|| "unknown".into());
        by_client.entry(ck).or_default().push(r);
    }

    for stat in [&e2e, &prefill, &upstream, &gap, &ttft] {
        println!("{}", stat.summary_line());
    }

    print_runtime_signals(&stream_miss);
    print_by_flag(&stream_miss, "defer", |r| r.streaming_defer);
    print_by_flag(&stream_miss, "passthrough", |r| r.request_passthrough);
    print_by_pipeline(&by_pipe);
    print_by_body_size(&by_bucket);
    print_by_client_kind(&by_client);
    print_top_slow_prefill(&stream_miss);
}

fn print_runtime_signals(rows: &[&TraceEntry]) {
    println!("\n--- runtime signals ---");
    let n = rows.len();
    let defer_n = rows.iter().filter(|r| r.streaming_defer).count();
    let pct = if n > 0 {
        defer_n as f64 / n as f64 * 100.0
    } else {
        0.0
    };
    println!("  streaming_defer: {defer_n}/{n} ({pct:.1}%)");
    let passthrough_n = rows.iter().filter(|r| r.request_passthrough).count();
    let pct = if n > 0 {
        passthrough_n as f64 / n as f64 * 100.0
    } else {
        0.0
    };
    println!("  request_passthrough: {passthrough_n}/{n} ({pct:.1}%)");

    let mut session_counts: HashMap<String, usize> = HashMap::new();
    for r in rows {
        *session_counts
            .entry(r.session_store.clone().unwrap_or_else(|| "none".into()))
            .or_default() += 1;
    }
    println!("  session_store: {session_counts:?}");

    let mut pchr: Vec<f64> = rows
        .iter()
        .filter_map(|r| r.prompt_cache_hit_ratio)
        .collect();
    if !pchr.is_empty() {
        pchr.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = pchr[pchr.len() / 2];
        let mean: f64 = pchr.iter().sum::<f64>() / pchr.len() as f64;
        println!("  prompt_cache_hit_ratio: p50={mid:.2} mean={mean:.2}");
    }

    let mut affinity: HashMap<String, usize> = HashMap::new();
    for r in rows {
        *affinity
            .entry(r.affinity_kind.clone().unwrap_or_else(|| "unknown".into()))
            .or_default() += 1;
    }
    println!("  affinity_kind: {affinity:?}");

    let mut stable: HashMap<String, usize> = HashMap::new();
    for r in rows {
        *stable
            .entry(
                r.stable_session_kind
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            )
            .or_default() += 1;
    }
    println!("  stable_session_kind: {stable:?}");
}

fn print_by_flag<F>(rows: &[&TraceEntry], section: &str, pred: F)
where
    F: Copy + Fn(&TraceEntry) -> bool,
{
    println!("\n--- by {section} ---");
    for flag in [true, false] {
        let items: Vec<&TraceEntry> = rows.iter().copied().filter(|r| pred(r) == flag).collect();
        if items.is_empty() {
            continue;
        }
        let label = if flag {
            section
        } else {
            match section {
                "defer" => "non_defer",
                "passthrough" => "non_passthrough",
                _ => "other",
            }
        };
        let pf = median_field(&items, |r| r.effective_prefill_ms());
        let up = median_field(&items, |r| r.upstream_latency_ms);
        let mut out_bytes: Vec<usize> = items
            .iter()
            .map(|r| r.upstream_outbound_bytes.unwrap_or(r.content_length))
            .collect();
        out_bytes.sort_unstable();
        let out_med = out_bytes.get(out_bytes.len() / 2).copied().unwrap_or(0);
        print!(
            "  {label}: n={} prefill_p50={:.0} upstream_p50={:.0} upstream_body_p50={}KB",
            items.len(),
            pf.unwrap_or(0.0),
            up.unwrap_or(0.0),
            out_med / 1024
        );
        if section == "passthrough" && flag {
            let mut prefix: Vec<usize> = items
                .iter()
                .filter_map(|r| r.request_passthrough_prefix_len)
                .collect();
            if !prefix.is_empty() {
                prefix.sort_unstable();
                let med = prefix[prefix.len() / 2];
                print!(" prefix_p50={:.1}KB", med as f64 / 1024.0);
            }
        }
        println!();
    }
}

fn print_by_pipeline(by_pipe: &HashMap<String, Vec<&TraceEntry>>) {
    println!("\n--- by pipeline (median prefill / upstream / e2e) ---");
    let mut pipes: Vec<_> = by_pipe.iter().collect();
    pipes.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (pipe, items) in pipes {
        let pf = median_field(items, |r| r.effective_prefill_ms());
        let up = median_field(items, |r| r.upstream_latency_ms);
        let du = median_field(items, |r| Some(r.latency_ms));
        println!(
            "  {pipe}: n={}  prefill_p50={:.0}  upstream_p50={:.0}  e2e_p50={:.0}",
            items.len(),
            pf.unwrap_or(0.0),
            up.unwrap_or(0.0),
            du.unwrap_or(0.0)
        );
    }
}

fn print_by_body_size(by_bucket: &HashMap<&str, Vec<&TraceEntry>>) {
    println!("\n--- by body size ---");
    for bucket in ["lt_200KB", "200KB_1MB", "ge_1MB"] {
        let Some(items) = by_bucket.get(bucket) else {
            continue;
        };
        let pf = median_field(items, |r| r.effective_prefill_ms());
        let up = median_field(items, |r| r.upstream_latency_ms);
        println!(
            "  {bucket}: n={}  prefill_p50={:.0}  upstream_p50={:.0}",
            items.len(),
            pf.unwrap_or(0.0),
            up.unwrap_or(0.0)
        );
    }
}

fn print_by_client_kind(by_client: &HashMap<String, Vec<&TraceEntry>>) {
    if by_client.len() <= 1 {
        return;
    }
    println!("\n--- by client_kind (median prefill / upstream) ---");
    let mut kinds: Vec<_> = by_client.iter().collect();
    kinds.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (kind, items) in kinds {
        let pf = median_field(items, |r| r.effective_prefill_ms());
        let up = median_field(items, |r| r.upstream_latency_ms);
        println!(
            "  {kind}: n={}  prefill_p50={:.0}  upstream_p50={:.0}",
            items.len(),
            pf.unwrap_or(0.0),
            up.unwrap_or(0.0)
        );
    }
}

fn print_top_slow_prefill(rows: &[&TraceEntry]) {
    println!("\n--- top 5 slowest prefill ---");
    let mut ranked: Vec<&TraceEntry> = rows.to_vec();
    ranked.sort_by(|a, b| {
        let pa = a.effective_prefill_ms().unwrap_or(0.0);
        let pb = b.effective_prefill_ms().unwrap_or(0.0);
        pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
    });
    for r in ranked.into_iter().take(5) {
        let pf = r.effective_prefill_ms().unwrap_or(0.0);
        let hash = if r.request_hash.len() > 12 {
            &r.request_hash[..12]
        } else {
            &r.request_hash
        };
        println!(
            "  prefill={pf:.0} upstream={:?} e2e={} body_kb={} defer={} session_store={} model={} pipeline={:?} hash={hash}",
            r.upstream_latency_ms,
            r.latency_ms,
            r.content_length / 1024,
            r.streaming_defer,
            r.session_store.as_deref().unwrap_or("none"),
            r.model,
            r.pipeline,
        );
    }
}
