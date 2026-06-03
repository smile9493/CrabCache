//! Upstream API key distribution, 429 cooldown, uniformity.

use crate::trace_entry::TraceEntry;
use std::collections::HashMap;

#[derive(Debug, Default)]
struct KeyStats {
    total: usize,
    success: usize,
    rate_limited: usize,
    other_fail: usize,
    cooldown: usize,
}

pub fn print_analysis(rows: &[TraceEntry], label: &str) {
    if rows.is_empty() {
        println!("\n=== {label}: key distribution (no records) ===");
        return;
    }

    let upstream_miss: Vec<&TraceEntry> = rows
        .iter()
        .filter(|r| !r.cache_hit && r.upstream_key_id.is_some())
        .collect();

    println!("\n=== {label}: upstream key distribution ===");
    println!(
        "  requests with upstream_key_id: {}/{}",
        upstream_miss.len(),
        rows.len()
    );

    if upstream_miss.is_empty() {
        let no_key = rows
            .iter()
            .filter(|r| !r.cache_hit && r.upstream_key_id.is_none())
            .count();
        println!("  cache-miss without key_id: {no_key}");
        return;
    }

    let mut by_key: HashMap<String, KeyStats> = HashMap::new();
    for r in &upstream_miss {
        let key = r.upstream_key_id.clone().unwrap_or_default();
        let st = by_key.entry(key).or_default();
        st.total += 1;
        match r.status_code {
            Some(200) => st.success += 1,
            Some(429) => {
                st.rate_limited += 1;
                if r.upstream_result.as_deref() == Some("429_rate_limited")
                    || r.error_code.as_deref() == Some("upstream_rate_limited")
                    || r.limit_source.is_some()
                {
                    st.cooldown += 1;
                }
            }
            Some(code) if (500..600).contains(&code) => st.other_fail += 1,
            _ => {
                if r.upstream_result.as_deref() == Some("success") {
                    st.success += 1;
                } else {
                    st.other_fail += 1;
                }
            }
        }
    }

    let n_keys = by_key.len();
    let total: usize = by_key.values().map(|s| s.total).sum();
    let expected = if n_keys > 0 {
        total as f64 / n_keys as f64
    } else {
        0.0
    };

    println!("  distinct keys: {n_keys}");
    println!("  total routed requests: {total}");
    if n_keys > 0 {
        println!("  expected uniform share: {expected:.1} req/key");
    }

    let mut keys: Vec<_> = by_key.iter().collect();
    keys.sort_by(|a, b| b.1.total.cmp(&a.1.total));

    println!("\n--- per key (sorted by volume) ---");
    for (key_id, st) in &keys {
        let short = if key_id.len() > 8 {
            format!("{}…", &key_id[..8])
        } else {
            (*key_id).to_string()
        };
        let pct = st.total as f64 / total as f64 * 100.0;
        let deviation = if expected > 0.0 {
            (st.total as f64 - expected).abs() / expected * 100.0
        } else {
            0.0
        };
        println!(
            "  {short}: n={} ({pct:.1}%) dev={deviation:.0}% ok={} 429={} fail={} cooldown_hint={}",
            st.total, st.success, st.rate_limited, st.other_fail, st.cooldown
        );
    }

    // Uniformity: coefficient of variation of per-key counts
    if n_keys > 1 {
        let counts: Vec<f64> = by_key.values().map(|s| s.total as f64).collect();
        let mean: f64 = counts.iter().sum::<f64>() / counts.len() as f64;
        let variance: f64 =
            counts.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / counts.len() as f64;
        let std = variance.sqrt();
        let cv = if mean > 0.0 { std / mean * 100.0 } else { 0.0 };
        println!("\n  uniformity CV (lower is more even): {cv:.1}%");
        let max_share = keys.first().map(|(_, s)| s.total).unwrap_or(0) as f64 / total as f64;
        println!("  max key share: {:.1}%", max_share * 100.0);
    }

    let rate_429: usize = upstream_miss
        .iter()
        .filter(|r| r.status_code == Some(429))
        .count();
    let exhausted = rows
        .iter()
        .filter(|r| {
            r.error_code.as_deref() == Some("upstream_key_exhausted")
                || r.limit_source.as_deref() == Some("upstream_key_exhausted")
        })
        .count();
    println!("\n--- rate limit signals ---");
    println!("  HTTP 429 in trace: {rate_429}");
    println!("  upstream_key_exhausted errors: {exhausted}");

    if rate_429 > 0 {
        println!("\n  recent 429 samples:");
        for r in upstream_miss
            .iter()
            .filter(|r| r.status_code == Some(429))
            .take(5)
        {
            println!(
                "    key={:?} pipeline={:?} model={} result={:?}",
                r.upstream_key_id, r.pipeline, r.model, r.upstream_result
            );
        }
    }
}
