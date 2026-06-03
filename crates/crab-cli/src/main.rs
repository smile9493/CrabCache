//! CrabCache ops CLI — trace analysis, health checks, density reports.

mod density;
mod fetch;
mod health;
mod stats;
mod trace;
mod trace_entry;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{self, Read};
#[derive(Parser)]
#[command(name = "crab-cli", version, about = "CrabCache gateway ops CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// One-shot report: trace + docker logs + metrics
    Report {
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: String,
        #[arg(long, default_value_t = 500)]
        tail: usize,
        #[arg(long, default_value_t = 800)]
        log_tail: usize,
        #[arg(long)]
        compare_tail: Option<usize>,
        #[arg(long)]
        since: Option<String>,
    },
    Trace {
        #[command(subcommand)]
        cmd: TraceCommands,
    },
    Health {
        #[command(subcommand)]
        cmd: HealthCommands,
    },
    /// Content density from trace JSONL
    Density {
        file: Option<String>,
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: Option<String>,
        #[arg(long, default_value_t = 500)]
        tail: usize,
    },
}

#[derive(Subcommand)]
enum TraceCommands {
    /// Latency breakdown (prefill / upstream / e2e / ttft)
    Analyze {
        file: Option<String>,
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: Option<String>,
        #[arg(long, default_value_t = 500)]
        tail: usize,
    },
    /// Upstream key distribution + 429
    Keys {
        file: Option<String>,
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: Option<String>,
        #[arg(long, default_value_t = 500)]
        tail: usize,
    },
    /// Before/after window comparison
    Compare {
        file: Option<String>,
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: Option<String>,
        /// Compare last N lines vs previous N (tail windows)
        #[arg(long)]
        compare_tail: Option<usize>,
        /// Split at line index: rows [0..N) vs [N..]
        #[arg(long)]
        split_at: Option<usize>,
        #[arg(long, default_value_t = 1000)]
        tail: usize,
    },
    /// Zipf / cache hit rate fit
    Cache {
        file: Option<String>,
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: Option<String>,
        #[arg(long, default_value_t = 5000)]
        tail: usize,
    },
}

#[derive(Subcommand)]
enum HealthCommands {
    /// Docker gateway log health scan
    Logs {
        file: Option<String>,
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: Option<String>,
        #[arg(long, default_value_t = 800)]
        tail: usize,
        #[arg(long)]
        since: Option<String>,
    },
    /// Prometheus metrics text summary
    Metrics {
        file: Option<String>,
        #[arg(long, value_parser = ["wuming", "crabcache-deploy"])]
        target: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Report {
            target,
            tail,
            log_tail,
            compare_tail,
            since,
        } => run_report(&target, tail, log_tail, compare_tail, since),
        Commands::Trace { cmd } => run_trace(cmd),
        Commands::Health { cmd } => run_health(cmd),
        Commands::Density { file, target, tail } => {
            let rows = load_trace(file.as_deref(), target.as_deref(), tail)?;
            density::print_analysis(&rows, "density");
            Ok(())
        }
    }
}

fn run_report(
    target: &str,
    tail: usize,
    log_tail: usize,
    compare_tail: Option<usize>,
    since: Option<String>,
) -> Result<()> {
    print_banner(Some(target));
    let rows = trace_entry::load_jsonl_str(&fetch::fetch_trace_tail(target, tail)?, Some(tail))?;
    if let Some(n) = compare_tail {
        if rows.len() >= n * 2 {
            trace::print_compare(&rows, n);
        }
    }
    trace::print_latency(&rows, "trace");
    trace::print_keys(&rows, "trace");
    trace::print_cache(&rows, "trace");
    density::print_analysis(&rows, "trace");

    let mut log_text = fetch::fetch_docker_logs(target, log_tail)?;
    if let Some(ref s) = since {
        log_text = health::docker_logs::filter_since(&log_text, s);
    }
    health::docker_logs::analyze(&log_text, "docker logs");

    let metrics = fetch::fetch_metrics(target)?;
    if !metrics.trim().is_empty() {
        health::prometheus::print_analysis(&metrics);
    }

    print_hints();
    Ok(())
}

fn run_trace(cmd: TraceCommands) -> Result<()> {
    match cmd {
        TraceCommands::Analyze { file, target, tail } => {
            print_banner(target.as_deref());
            let rows = load_trace(file.as_deref(), target.as_deref(), tail)?;
            trace::print_latency(&rows, "trace");
            print_hints();
        }
        TraceCommands::Keys { file, target, tail } => {
            print_banner(target.as_deref());
            let rows = load_trace(file.as_deref(), target.as_deref(), tail)?;
            trace::print_keys(&rows, "trace");
        }
        TraceCommands::Compare {
            file,
            target,
            compare_tail,
            split_at,
            tail,
        } => {
            print_banner(target.as_deref());
            let rows = load_trace(file.as_deref(), target.as_deref(), tail)?;
            if let Some(n) = compare_tail {
                trace::print_compare(&rows, n);
            } else if let Some(at) = split_at {
                trace::print_compare_at(&rows, at);
            } else {
                anyhow::bail!("specify --compare-tail N or --split-at N");
            }
            trace::print_latency(&rows, "trace (full)");
        }
        TraceCommands::Cache { file, target, tail } => {
            print_banner(target.as_deref());
            let rows = load_trace(file.as_deref(), target.as_deref(), tail)?;
            trace::print_cache(&rows, "trace");
        }
    }
    Ok(())
}

fn run_health(cmd: HealthCommands) -> Result<()> {
    match cmd {
        HealthCommands::Logs {
            file,
            target,
            tail,
            since,
        } => {
            print_banner(target.as_deref());
            let mut text = load_text(
                file.as_deref(),
                target.as_deref(),
                |t, n| fetch::fetch_docker_logs(t, n),
                tail,
            )?;
            if let Some(ref s) = since {
                text = health::docker_logs::filter_since(&text, s);
            }
            health::docker_logs::analyze(&text, "docker logs");
        }
        HealthCommands::Metrics { file, target } => {
            print_banner(target.as_deref());
            let text = if let Some(path) = file {
                read_file_or_stdin(&path)?
            } else if let Some(t) = target {
                fetch::fetch_metrics(&t)?
            } else {
                anyhow::bail!("pass metrics file path or --target");
            };
            health::prometheus::print_analysis(&text);
        }
    }
    Ok(())
}

fn load_trace(
    file: Option<&str>,
    target: Option<&str>,
    tail: usize,
) -> Result<Vec<trace_entry::TraceEntry>> {
    if let Some(path) = file {
        return trace_entry::load_jsonl(path, Some(tail));
    }
    if let Some(t) = target {
        let text = fetch::fetch_trace_tail(t, tail)?;
        return trace_entry::load_jsonl_str(&text, Some(tail));
    }
    anyhow::bail!("pass trace file path or --target")
}

fn load_text(
    file: Option<&str>,
    target: Option<&str>,
    fetch_fn: impl FnOnce(&str, usize) -> Result<String>,
    tail: usize,
) -> Result<String> {
    if let Some(path) = file {
        return read_file_or_stdin(path);
    }
    if let Some(t) = target {
        return fetch_fn(t, tail);
    }
    anyhow::bail!("pass file path or --target")
}

fn read_file_or_stdin(path: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        return Ok(buf);
    }
    Ok(std::fs::read_to_string(path)?)
}

fn print_banner(target: Option<&str>) {
    let now = chrono_now_utc();
    println!("CrabCache gateway log analysis  {now}");
    if let Some(t) = target {
        println!("  target: {t}");
    }
}

fn chrono_now_utc() -> String {
    // Avoid chrono dependency: format from SystemTime
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs} UTC (unix)")
}

fn print_hints() {
    println!("\n--- hints ---");
    println!("  prefill_ms = request start -> upstream response headers (MiMo SLO)");
    println!("  gap ≈ body read + upload + prefill; prefer prefill_ms over gap");
    println!("  defer parse_fail with streaming_defer=true → docs/STREAMING_BODY_FORWARD.md");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
