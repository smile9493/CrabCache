//! Before/after trace window comparison.

use crate::stats::median_field;
use crate::trace::key_distribution;
use crate::trace_entry::TraceEntry;
use std::collections::HashMap;

pub fn print_compare(rows: &[TraceEntry], split_at: usize) {
    if rows.len() < split_at.saturating_mul(2) {
        println!(
            "\n=== compare: need at least {} rows (have {}) ===",
            split_at * 2,
            rows.len()
        );
        return;
    }
    let split = rows.len() - split_at;
    let older = &rows[..split];
    let recent = &rows[split..];

    println!("\n=== Before / after comparison (older vs recent tail) ===");
    println!("  samples:  older={}  recent={}", older.len(), recent.len());

    let o_pf = median_field(older, |r| r.effective_prefill_ms());
    let r_pf = median_field(recent, |r| r.effective_prefill_ms());
    if let (Some(o), Some(r)) = (o_pf, r_pf) {
        println!("  prefill_p50:  {o:.0} -> {r:.0} ms  ({:+.0})", r - o);
    }

    let o_up = median_field(older, |r| r.upstream_latency_ms);
    let r_up = median_field(recent, |r| r.upstream_latency_ms);
    if let (Some(o), Some(r)) = (o_up, r_up) {
        println!("  upstream_p50: {o:.0} -> {r:.0} ms  ({:+.0})", r - o);
    }

    let o_hit = hit_rate(older);
    let r_hit = hit_rate(recent);
    println!(
        "  cache_hit_rate: {:.1}% -> {:.1}%",
        o_hit * 100.0,
        r_hit * 100.0
    );

    print_key_shift(older, recent);
}

fn hit_rate(rows: &[TraceEntry]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let hits = rows.iter().filter(|r| r.cache_hit).count();
    hits as f64 / rows.len() as f64
}

fn print_key_shift(older: &[TraceEntry], recent: &[TraceEntry]) {
    fn key_counts(rows: &[TraceEntry]) -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for r in rows {
            if let Some(ref k) = r.upstream_key_id {
                *m.entry(k.clone()).or_default() += 1;
            }
        }
        m
    }
    let o = key_counts(older);
    let r = key_counts(recent);
    if o.is_empty() && r.is_empty() {
        return;
    }
    println!("\n  upstream_key_id distinct: {} -> {}", o.len(), r.len());
    let o_max = o.iter().max_by_key(|(_, c)| *c);
    let r_max = r.iter().max_by_key(|(_, c)| *c);
    if let Some((k, c)) = o_max {
        let share = *c as f64 / o.values().sum::<usize>() as f64 * 100.0;
        println!(
            "  older top key: {}… ({c} reqs, {share:.0}%)",
            &k[..k.len().min(8)]
        );
    }
    if let Some((k, c)) = r_max {
        let share = *c as f64 / r.values().sum::<usize>() as f64 * 100.0;
        println!(
            "  recent top key: {}… ({c} reqs, {share:.0}%)",
            &k[..k.len().min(8)]
        );
    }
}

/// Split by line index (0-based): rows before `split_at` vs from `split_at`.
pub fn print_compare_at(rows: &[TraceEntry], split_at: usize) {
    if split_at == 0 || split_at >= rows.len() {
        println!(
            "\n=== compare: invalid split-at {split_at} (len={}) ===",
            rows.len()
        );
        return;
    }
    let (older, recent) = rows.split_at(split_at);
    println!(
        "\n=== Before / after comparison (first {} vs rest) ===",
        split_at
    );
    println!("  samples:  older={}  recent={}", older.len(), recent.len());
    let o_pf = median_field(older, |r| r.effective_prefill_ms());
    let r_pf = median_field(recent, |r| r.effective_prefill_ms());
    if let (Some(o), Some(r)) = (o_pf, r_pf) {
        println!("  prefill_p50:  {o:.0} -> {r:.0} ms  ({:+.0})", r - o);
    }
    let o_up = median_field(older, |r| r.upstream_latency_ms);
    let r_up = median_field(recent, |r| r.upstream_latency_ms);
    if let (Some(o), Some(r)) = (o_up, r_up) {
        println!("  upstream_p50: {o:.0} -> {r:.0} ms  ({:+.0})", r - o);
    }
    key_distribution::print_analysis(recent, "recent window keys");
}
