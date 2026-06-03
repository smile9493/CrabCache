//! SSH remote fetch for trace, docker logs, and Prometheus metrics.

use anyhow::{Context, Result};
use std::process::Command;

pub const GATEWAY_CONTAINER: &str = "crabcache-gateway-1";
pub const TRACE_PATH: &str = "/app/logs/trace.jsonl";
pub const METRICS_URL: &str = "http://127.0.0.1:9090/metrics";

/// Deploy target → SSH host alias.
pub fn ssh_host(target: &str) -> Result<&'static str> {
    match target {
        "wuming" => Ok("wuming"),
        "crabcache-deploy" => Ok("crabcache-deploy"),
        _ => anyhow::bail!("unknown target {target:?}; use wuming or crabcache-deploy"),
    }
}

pub fn ssh_run(host: &str, remote_cmd: &str) -> Result<String> {
    let proc = Command::new("ssh")
        .arg(host)
        .arg(remote_cmd)
        .output()
        .context("spawn ssh")?;
    if !proc.status.success() {
        let stderr = String::from_utf8_lossy(&proc.stderr);
        let stdout = String::from_utf8_lossy(&proc.stdout);
        anyhow::bail!("ssh {host} failed: {stderr}{stdout}");
    }
    Ok(String::from_utf8_lossy(&proc.stdout).into_owned())
}

pub fn fetch_trace_tail(target: &str, tail: usize) -> Result<String> {
    let host = ssh_host(target)?;
    let cmd = format!("docker exec {GATEWAY_CONTAINER} tail -n {tail} {TRACE_PATH} 2>/dev/null");
    ssh_run(host, &cmd)
}

pub fn fetch_docker_logs(target: &str, tail: usize) -> Result<String> {
    let host = ssh_host(target)?;
    let cmd = format!("docker logs {GATEWAY_CONTAINER} --tail {tail} 2>&1");
    ssh_run(host, &cmd)
}

pub fn fetch_metrics(target: &str) -> Result<String> {
    let host = ssh_host(target)?;
    let cmd = format!("curl -sf {METRICS_URL} 2>/dev/null || true");
    ssh_run(host, &cmd)
}
