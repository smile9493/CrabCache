//! Prometheus exposition text parsing for ops summaries.

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static MODEL_PHASE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"model="([^"]+)".*phase="([^"]+)""#).unwrap());
static MODEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"model="([^"]+)""#).unwrap());
static KEY_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"key_id="([^"]+)""#).unwrap());
static EVENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"event="([^"]+)""#).unwrap());
static REASON_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"reason="([^"]+)""#).unwrap());
static RESULT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"result="([^"]+)""#).unwrap());

pub fn parse_counters(text: &str, prefix: &str) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for line in text.lines() {
        if line.starts_with('#') || !line.starts_with(prefix) {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(v) = parts[1].parse::<f64>() {
                out.insert(parts[0].to_string(), v);
            }
        }
    }
    out
}

pub fn print_analysis(text: &str) {
    print_defer_counters(text);
    print_passthrough(text);
    print_key_binding(text);
    print_rejected(text);
    print_upstream_keys(text);
    print_phase_latency(text);
    print_upstream_latency(text);
}

fn print_defer_counters(text: &str) {
    println!("\n=== Prometheus: streaming defer counters ===");
    let defer = parse_counters(text, "gateway_streaming_defer_");
    if defer.is_empty() {
        println!("  (no gateway_streaming_defer_* counters)");
    } else {
        let mut names: Vec<_> = defer.keys().collect();
        names.sort();
        for name in names {
            let short = name.replace("gateway_streaming_defer_", "");
            println!("  {short}: {:.0}", defer[name]);
        }
    }
}

fn print_passthrough(text: &str) {
    println!("\n=== Prometheus: request passthrough ===");
    let pt = parse_counters(text, "gateway_request_passthrough");
    if pt.is_empty() {
        println!("  (no gateway_request_passthrough_total)");
    } else {
        let mut names: Vec<_> = pt.keys().collect();
        names.sort();
        for name in names {
            println!("  {name}: {:.0}", pt[name]);
        }
    }
}

fn print_key_binding(text: &str) {
    println!("\n=== Prometheus: key binding (MiMo) ===");
    let binding = parse_counters(text, "gateway_key_binding_total");
    if binding.is_empty() {
        println!("  (no gateway_key_binding_total)");
        return;
    }
    for (name, val) in &binding {
        let event = EVENT_RE
            .captures(name)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("?");
        println!("  {event}: {val:.0}");
    }
}

fn print_rejected(text: &str) {
    println!("\n=== Prometheus: rejected requests ===");
    let rejected = parse_counters(text, "gateway_rejected_requests_total");
    if rejected.is_empty() {
        println!("  (no gateway_rejected_requests_total)");
        return;
    }
    let mut items: Vec<_> = rejected.iter().collect();
    items.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (name, val) in items.iter().take(10) {
        let reason = REASON_RE
            .captures(name)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or(name);
        println!("  {reason}: {val:.0}");
    }
}

fn print_upstream_keys(text: &str) {
    println!("\n=== Prometheus: upstream key pool ===");
    let requests = parse_counters(text, "gateway_upstream_key_requests_total");
    if !requests.is_empty() {
        let mut by_key: HashMap<String, HashMap<String, f64>> = HashMap::new();
        for (name, val) in &requests {
            let key = KEY_ID_RE
                .captures(name)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .unwrap_or("?");
            let result = RESULT_RE
                .captures(name)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .unwrap_or("?");
            *by_key
                .entry(key.to_string())
                .or_default()
                .entry(result.to_string())
                .or_insert(0.0) += *val;
        }
        let mut keys: Vec<_> = by_key.iter().collect();
        keys.sort_by(|a, b| {
            let sa: f64 = a.1.values().sum();
            let sb: f64 = b.1.values().sum();
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        for (key, results) in keys.iter().take(12) {
            let short = if key.len() > 8 {
                format!("{}…", &key[..8])
            } else {
                (*key).to_string()
            };
            let total: f64 = results.values().sum();
            println!("  {short}: total={total:.0} {results:?}");
        }
    }

    let inflight = parse_counters(text, "gateway_upstream_key_inflight");
    if !inflight.is_empty() {
        println!("\n  upstream_key_inflight (non-zero):");
        for (name, val) in &inflight {
            if *val > 0.0 {
                let key = KEY_ID_RE
                    .captures(name)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str())
                    .unwrap_or("?");
                println!("    {key}: {val:.0}");
            }
        }
    }
}

fn print_phase_latency(text: &str) {
    println!("\n=== Prometheus: phase latency (sum/count → avg ms) ===");
    let sums = parse_counters(text, "gateway_request_phase_latency_seconds_sum");
    let counts = parse_counters(text, "gateway_request_phase_latency_seconds_count");
    let mut by_model_phase: HashMap<(String, String), (f64, f64)> = HashMap::new();
    for (key, total) in &sums {
        let Some(caps) = MODEL_PHASE_RE.captures(key) else {
            continue;
        };
        let model = caps.get(1).unwrap().as_str().to_string();
        let phase = caps.get(2).unwrap().as_str().to_string();
        let cnt_key = key.replace("_sum", "_count");
        let cnt = counts.get(&cnt_key).copied().unwrap_or(0.0);
        if cnt > 0.0 {
            by_model_phase.insert((model, phase), (total / cnt * 1000.0, cnt));
        }
    }
    let mut models: Vec<String> = by_model_phase.keys().map(|(m, _)| m.clone()).collect();
    models.sort();
    models.dedup();
    for model in models {
        println!("  [{model}]");
        for phase in [
            "body_read_done",
            "json_parse_done",
            "upstream_connect_done",
            "prefill_done",
            "ttft",
        ] {
            if let Some((avg_ms, n)) = by_model_phase.get(&(model.clone(), phase.to_string())) {
                println!("    {phase}: avg={avg_ms:.1}ms (n={n:.0})");
            }
        }
    }
}

fn print_upstream_latency(text: &str) {
    println!("\n=== Prometheus: upstream latency avg ===");
    let up_sum = parse_counters(text, "gateway_upstream_latency_seconds_sum");
    let up_cnt = parse_counters(text, "gateway_upstream_latency_seconds_count");
    for (key, total) in &up_sum {
        let Some(caps) = MODEL_RE.captures(key) else {
            continue;
        };
        let model = caps.get(1).unwrap().as_str();
        let cnt = up_cnt
            .get(&key.replace("_sum", "_count"))
            .copied()
            .unwrap_or(0.0);
        if cnt > 0.0 {
            println!("  {model}: avg={:.0}ms (n={cnt:.0})", total / cnt * 1000.0);
        }
    }
}
