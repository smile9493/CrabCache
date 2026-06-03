//! Gateway Docker log health scan.

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").unwrap());
static LOG_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<ts>\d{4}-\d{2}-\d{2}T[\d:.]+Z)\s+(?P<level>\w+)\s+(?P<module>\S+):\s+(?P<msg>.*?)(?:\s+(?P<fields>.*))?$",
    )
    .unwrap()
});
static KV_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\w+)=([^\s]+)").unwrap());

#[derive(Debug, Default)]
pub struct LogHealth {
    pub parse_fail: Vec<HashMap<String, String>>,
    pub defer_parse_fail: Vec<HashMap<String, String>>,
    pub panic: Vec<String>,
    pub redis_timeout: Vec<String>,
    pub connection_closed: Vec<HashMap<String, String>>,
    pub upstream_400: Vec<HashMap<String, String>>,
    pub other_errors: HashMap<String, usize>,
}

#[derive(Debug)]
struct LogEntry {
    ts: String,
    level: String,
    module: String,
    msg: String,
    fields: HashMap<String, String>,
    raw: String,
}

fn parse_line(line: &str) -> Option<LogEntry> {
    let line = ANSI_RE.replace_all(line.trim(), "");
    if line.is_empty() {
        return None;
    }
    let caps = LOG_LINE_RE.captures(&line)?;
    let mut fields = HashMap::new();
    if let Some(f) = caps.name("fields") {
        for c in KV_RE.captures_iter(f.as_str()) {
            if let (Some(key), Some(val)) = (c.get(1), c.get(2)) {
                fields.insert(key.as_str().to_string(), val.as_str().to_string());
            }
        }
    }
    Some(LogEntry {
        ts: caps.name("ts")?.as_str().to_string(),
        level: caps.name("level")?.as_str().to_string(),
        module: caps.name("module")?.as_str().to_string(),
        msg: caps.name("msg")?.as_str().to_string(),
        fields,
        raw: line.to_string(),
    })
}

pub fn analyze(text: &str, label: &str) -> LogHealth {
    let mut health = LogHealth::default();
    for line in text.lines() {
        let Some(entry) = parse_line(line) else {
            continue;
        };
        let msg = &entry.msg;
        if msg.contains("Rejecting request: client JSON parse failed") {
            let mut rec = entry.fields.clone();
            rec.insert("ts".into(), entry.ts.clone());
            rec.insert("msg".into(), msg.clone());
            if entry.fields.get("streaming_defer").map(|s| s.as_str()) == Some("true") {
                health.defer_parse_fail.push(rec.clone());
            }
            health.parse_fail.push(rec);
        } else if msg.contains("Panic occurred")
            || (entry.module == "crab_gateway" && msg.contains("Panic"))
        {
            health.panic.push(entry.raw.clone());
        } else if msg.contains("Timed out in bb8")
            || msg.contains("Failed to persist control plane state to Redis")
        {
            health.redis_timeout.push(entry.raw.clone());
        } else if msg.contains("Fail to proxy: Downstream ConnectionClosed") {
            let mut rec = HashMap::new();
            rec.insert("ts".into(), entry.ts.clone());
            rec.insert("msg".into(), msg.clone());
            if let Some(rid) = entry.fields.get("request_id") {
                rec.insert("request_id".into(), rid.clone());
            }
            health.connection_closed.push(rec);
        } else if msg.contains("Upstream returned error status")
            && entry.fields.get("status").map(|s| s.as_str()) == Some("400")
        {
            let mut rec = entry.fields.clone();
            rec.insert("ts".into(), entry.ts.clone());
            health.upstream_400.push(rec);
        } else if entry.level == "ERROR" {
            let key = format!("{}: {}", entry.module, truncate(msg, 80));
            *health.other_errors.entry(key).or_default() += 1;
        }
    }
    print_report(&health, label);
    health
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn print_report(health: &LogHealth, label: &str) {
    println!("\n=== {label}: health scan ===");
    println!(
        "  JSON parse_fail total:        {}",
        health.parse_fail.len()
    );
    println!(
        "    └ streaming_defer=true:     {}",
        health.defer_parse_fail.len()
    );
    println!("  Tokio panic / crash:          {}", health.panic.len());
    println!(
        "  Redis bb8 persist timeout:    {}",
        health.redis_timeout.len()
    );
    println!(
        "  Downstream ConnectionClosed:   {}",
        health.connection_closed.len()
    );
    println!(
        "  Upstream HTTP 400:            {}",
        health.upstream_400.len()
    );

    if !health.defer_parse_fail.is_empty() {
        println!("\n  defer parse_fail samples:");
        for r in health.defer_parse_fail.iter().rev().take(3) {
            println!(
                "    {} request_id={} body_len={} parse_error={}",
                r.get("ts").map(|s| s.as_str()).unwrap_or("?"),
                r.get("request_id").map(|s| s.as_str()).unwrap_or("?"),
                r.get("body_len").map(|s| s.as_str()).unwrap_or("?"),
                r.get("parse_error").map(|s| s.as_str()).unwrap_or("?"),
            );
        }
    } else if !health.parse_fail.is_empty() {
        println!("\n  parse_fail samples (non-defer):");
        for r in health.parse_fail.iter().rev().take(3) {
            println!(
                "    {} request_id={} body_len={} defer={}",
                r.get("ts").map(|s| s.as_str()).unwrap_or("?"),
                r.get("request_id").map(|s| s.as_str()).unwrap_or("?"),
                r.get("body_len").map(|s| s.as_str()).unwrap_or("?"),
                r.get("streaming_defer")
                    .map(|s| s.as_str())
                    .unwrap_or("n/a"),
            );
        }
    }

    if !health.panic.is_empty() {
        println!("\n  panic samples:");
        for line in health.panic.iter().rev().take(2) {
            let s = if line.len() > 160 {
                &line[..160]
            } else {
                line.as_str()
            };
            println!("    {s}");
        }
    }

    if !health.other_errors.is_empty() {
        println!("\n  other ERROR (top 5):");
        let mut errs: Vec<_> = health.other_errors.iter().collect();
        errs.sort_by(|a, b| b.1.cmp(a.1));
        for (key, cnt) in errs.into_iter().take(5) {
            println!("    [{cnt}x] {key}");
        }
    }
}

pub fn filter_since(text: &str, since: &str) -> String {
    text.lines()
        .filter(|line| {
            let stripped = ANSI_RE.replace_all(line.trim(), "");
            parse_line(&stripped)
                .map(|e| e.ts.as_str() >= since)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
