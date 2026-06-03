//! Content density analysis from trace JSONL.

use crate::trace_entry::TraceEntry;
use std::collections::HashMap;

#[derive(Debug, Default)]
struct PipelineDensity {
    count: usize,
    client_body: u64,
    upstream_outbound: u64,
    client_outbound: u64,
    reasoning_stripped: u64,
    reasoning_mirrored: u64,
    thinking_stripped: u64,
    cache_hits: usize,
}

pub fn print_analysis(rows: &[TraceEntry], label: &str) {
    if rows.is_empty() {
        println!("\n=== {label}: content density (no records) ===");
        return;
    }

    println!("\n=== {label}: content density ===");
    let mut by_pipe: HashMap<String, PipelineDensity> = HashMap::new();
    let mut total_upstream: u64 = 0;
    let mut total_stripped: u64 = 0;
    let mut total_mirrored: u64 = 0;
    let mut total_client_out: u64 = 0;

    for r in rows {
        let pipe = r.pipeline.clone().unwrap_or_else(|| "unknown".into());
        let st = by_pipe.entry(pipe).or_default();
        st.count += 1;
        if r.cache_hit {
            st.cache_hits += 1;
        }
        let body = r.content_length as u64;
        st.client_body += body;
        let up = r.upstream_outbound_bytes.unwrap_or(r.content_length) as u64;
        st.upstream_outbound += up;
        total_upstream += up;
        if let Some(b) = r.client_outbound_bytes {
            st.client_outbound += b as u64;
            total_client_out += b as u64;
        }
        if let Some(b) = r.reasoning_stripped_bytes {
            st.reasoning_stripped += b as u64;
            total_stripped += b as u64;
        }
        if let Some(b) = r.reasoning_mirrored_bytes {
            st.reasoning_mirrored += b as u64;
            total_mirrored += b as u64;
        }
        if let Some(b) = r.thinking_block_stripped_bytes {
            st.thinking_stripped += b as u64;
        }
    }

    println!("  records: {}  pipelines: {}", rows.len(), by_pipe.len());
    if total_upstream > 0 {
        println!(
            "  global reasoning_strip / upstream_outbound: {:.1}%",
            total_stripped as f64 / total_upstream as f64 * 100.0
        );
    }
    if total_client_out > 0 {
        println!(
            "  global reasoning_mirror / client_outbound: {:.1}%",
            total_mirrored as f64 / total_client_out as f64 * 100.0
        );
    }

    println!("\n--- by pipeline ---");
    let mut pipes: Vec<_> = by_pipe.iter().collect();
    pipes.sort_by(|a, b| b.1.count.cmp(&a.1.count));
    println!(
        "  {:<28} {:>6} {:>8} {:>12} {:>10}",
        "pipeline", "n", "hit%", "strip%", "up→cli"
    );
    for (pipe, st) in pipes {
        let hit_pct = if st.count > 0 {
            st.cache_hits as f64 / st.count as f64 * 100.0
        } else {
            0.0
        };
        let strip_pct = if st.upstream_outbound > 0 {
            st.reasoning_stripped as f64 / st.upstream_outbound as f64 * 100.0
        } else {
            0.0
        };
        let ratio = if st.upstream_outbound > 0 {
            st.client_outbound as f64 / st.upstream_outbound as f64
        } else {
            0.0
        };
        println!(
            "  {:<28} {:>6} {:>7.1}% {:>11.1}% {:>9.3}x",
            pipe, st.count, hit_pct, strip_pct, ratio
        );
    }
}
