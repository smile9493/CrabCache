//! Zipf fit, repeat ratio, cache hit rate (shadow log analysis).

use crate::trace_entry::TraceEntry;
use std::collections::HashMap;

#[derive(Debug)]
pub struct FittedParams {
    pub total_requests: usize,
    pub unique_requests: usize,
    pub repeat_ratio: f64,
    pub semantic_cluster_ratio: f64,
    pub estimated_zipf_alpha: f64,
    pub conversation_ratio: f64,
    pub avg_latency_ms: f64,
    pub cache_hit_rate: f64,
    pub total_tokens: u64,
    pub avg_prompt_tokens: f64,
}

pub fn fit_params(rows: &[TraceEntry]) -> FittedParams {
    if rows.is_empty() {
        return FittedParams {
            total_requests: 0,
            unique_requests: 0,
            repeat_ratio: 0.0,
            semantic_cluster_ratio: 0.0,
            estimated_zipf_alpha: 1.0,
            conversation_ratio: 0.0,
            avg_latency_ms: 0.0,
            cache_hit_rate: 0.0,
            total_tokens: 0,
            avg_prompt_tokens: 0.0,
        };
    }

    let total = rows.len();
    let mut hash_counts: HashMap<&str, usize> = HashMap::new();
    for r in rows {
        *hash_counts.entry(r.request_hash.as_str()).or_default() += 1;
    }
    let unique = hash_counts.len();
    let repeat_ratio = if total > 0 {
        1.0 - unique as f64 / total as f64
    } else {
        0.0
    };

    let mut counts: Vec<usize> = hash_counts.values().copied().collect();
    counts.sort_by(|a, b| b.cmp(a));
    let alpha = zipf_alpha(&counts);

    let mut cluster_member: HashMap<u32, usize> = HashMap::new();
    for r in rows {
        *cluster_member.entry(r.semantic_cluster).or_default() += 1;
    }
    let multi: usize = cluster_member.values().filter(|&&c| c > 1).sum();
    let semantic_ratio = if total > 0 {
        multi as f64 / total as f64
    } else {
        0.0
    };

    let with_conv = rows.iter().filter(|r| r.conversation_id.is_some()).count();
    let conversation_ratio = with_conv as f64 / total as f64;

    let latencies: Vec<f64> = rows.iter().map(|r| r.latency_ms).collect();
    let avg_latency = latencies.iter().sum::<f64>() / latencies.len() as f64;

    let hits = rows.iter().filter(|r| r.cache_hit).count();
    let cache_hit_rate = hits as f64 / total as f64;

    let total_tokens: u64 = rows.iter().map(|r| r.prompt_tokens as u64).sum();
    let avg_prompt = total_tokens as f64 / total as f64;

    FittedParams {
        total_requests: total,
        unique_requests: unique,
        repeat_ratio,
        semantic_cluster_ratio: semantic_ratio,
        estimated_zipf_alpha: alpha,
        conversation_ratio,
        avg_latency_ms: avg_latency,
        cache_hit_rate,
        total_tokens,
        avg_prompt_tokens: avg_prompt,
    }
}

fn zipf_alpha(counts: &[usize]) -> f64 {
    if counts.len() < 2 {
        return 0.0;
    }
    let n = counts.len() as f64;
    let log_ranks: Vec<f64> = (1..=counts.len()).map(|i| (i as f64).ln()).collect();
    let log_freqs: Vec<f64> = counts.iter().map(|&c| (c as f64).ln()).collect();
    let sum_x: f64 = log_ranks.iter().sum();
    let sum_y: f64 = log_freqs.iter().sum();
    let sum_xy: f64 = log_ranks.iter().zip(&log_freqs).map(|(x, y)| x * y).sum();
    let sum_xx: f64 = log_ranks.iter().map(|x| x * x).sum();
    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < f64::EPSILON {
        return 0.0;
    }
    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    -slope
}

pub fn estimate_achievable(params: &FittedParams) -> f64 {
    let base = params.repeat_ratio;
    let semantic_bonus = params.semantic_cluster_ratio * (1.0 - params.repeat_ratio) * 0.5;
    let concentration_bonus = (params.estimated_zipf_alpha - 1.0).max(0.0) * 0.1;
    (base + semantic_bonus + concentration_bonus).min(0.98)
}

pub fn print_analysis(rows: &[TraceEntry], label: &str) {
    let p = fit_params(rows);
    println!("\n=== {label}: cache / Zipf fit ===");
    println!("  total_requests: {}", p.total_requests);
    println!("  unique_requests: {}", p.unique_requests);
    println!("  repeat_ratio: {:.1}%", p.repeat_ratio * 100.0);
    println!(
        "  semantic_cluster_ratio: {:.1}%",
        p.semantic_cluster_ratio * 100.0
    );
    println!("  estimated_zipf_alpha: {:.2}", p.estimated_zipf_alpha);
    println!("  conversation_ratio: {:.1}%", p.conversation_ratio * 100.0);
    println!("  cache_hit_rate: {:.1}%", p.cache_hit_rate * 100.0);
    println!("  avg_latency_ms: {:.0}", p.avg_latency_ms);
    println!("  avg_prompt_tokens: {:.1}", p.avg_prompt_tokens);
    let achievable = estimate_achievable(&p);
    println!(
        "  estimated achievable hit rate: {:.1}%",
        achievable * 100.0
    );
}
