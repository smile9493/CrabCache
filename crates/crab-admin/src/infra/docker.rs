use crate::infra::rates::{compute_bps, compute_cpu_percent};
use crate::infra::types::*;
use bollard::Docker;
use bollard::container::ListContainersOptions;
use bollard::container::StatsOptions;
use bollard::models::ContainerSummary;
use bollard::volume::ListVolumesOptions;
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

const BOLLARD_TIMEOUT: u64 = 120;
const STATS_CONCURRENCY: usize = 4;

fn socket_path(docker_host: &str) -> &str {
    docker_host.strip_prefix("unix://").unwrap_or(docker_host)
}

fn docker_tcp_allowed() -> bool {
    std::env::var("CRABCACHE_DOCKER_ALLOW_TCP")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// Try to connect to the Docker daemon (unix socket; optional TCP when explicitly enabled).
pub fn try_connect(docker_host: &str) -> Option<Docker> {
    let path = socket_path(docker_host);
    match Docker::connect_with_unix(path, BOLLARD_TIMEOUT, bollard::API_DEFAULT_VERSION) {
        Ok(d) => Some(d),
        Err(e) => {
            tracing::warn!(
                error = %e,
                docker_host = %docker_host,
                "Docker unix connect failed — infra monitoring disabled"
            );
            if !docker_tcp_allowed() {
                tracing::info!(
                    "Set CRABCACHE_DOCKER_ALLOW_TCP=1 to allow HTTP fallback to 127.0.0.1:2375"
                );
                return None;
            }
            Docker::connect_with_http(
                "http://127.0.0.1:2375",
                BOLLARD_TIMEOUT,
                bollard::API_DEFAULT_VERSION,
            )
            .map_err(|e2| {
                tracing::debug!(error = %e2, "Docker TCP connect failed");
                e2
            })
            .ok()
        }
    }
}

struct ContainerMeta {
    id: String,
    name: String,
    status: String,
}

fn container_meta(c: &ContainerSummary) -> Option<ContainerMeta> {
    let id = c.id.as_deref()?.to_string();
    let names = c.names.as_deref().unwrap_or(&[]);
    let raw = names.first().cloned().unwrap_or_else(|| id.clone());
    let name = raw.strip_prefix('/').unwrap_or(&raw).to_string();
    let status = c.status.as_deref().unwrap_or("unknown").to_string();
    Some(ContainerMeta { id, name, status })
}

async fn fetch_container_stats(
    docker: Docker,
    meta: ContainerMeta,
    prev: Option<ContainerRawSample>,
    now: Instant,
    permit: Arc<Semaphore>,
) -> Option<(ContainerStats, ContainerRawSample)> {
    let _permit = permit.acquire().await.ok()?;

    let stats_opts = StatsOptions {
        stream: false,
        one_shot: true,
    };
    let stats = docker
        .stats(&meta.id, Some(stats_opts))
        .next()
        .await?
        .map_err(|e| {
            tracing::warn!(container = %meta.name, error = %e, "stats failed");
            e
        })
        .ok()?;

    let cpu_total = stats.cpu_stats.cpu_usage.total_usage;
    let system_cpu = stats.cpu_stats.system_cpu_usage.unwrap_or(0);
    let online_cpus = stats.cpu_stats.online_cpus.unwrap_or(1) as u32;

    let mem_usage = stats.memory_stats.usage.unwrap_or(0);
    let mem_limit = stats.memory_stats.limit.unwrap_or(1);
    let mem_percent = if mem_limit > 0 {
        (mem_usage as f64 / mem_limit as f64) * 100.0
    } else {
        0.0
    };

    let (net_rx, net_tx) = stats
        .networks
        .as_ref()
        .map(|nets| {
            nets.values().fold((0u64, 0u64), |(rx, tx), n| {
                (rx + n.rx_bytes, tx + n.tx_bytes)
            })
        })
        .unwrap_or((0, 0));

    let container_id = meta.id.clone();
    let (cpu_percent, net_rx_bps, net_tx_bps) = if let Some(p) = prev {
        let cpu_pct = compute_cpu_percent(
            &ContainerRawSample {
                cpu_total_usage: cpu_total,
                system_cpu_usage: system_cpu,
                net_rx_bytes: net_rx,
                net_tx_bytes: net_tx,
                online_cpus,
                sampled_at: now,
            },
            &p,
        );
        let elapsed = now.duration_since(p.sampled_at).as_secs_f64();
        let rx_bps = compute_bps(net_rx, p.net_rx_bytes, elapsed);
        let tx_bps = compute_bps(net_tx, p.net_tx_bytes, elapsed);
        (Some(cpu_pct), rx_bps, tx_bps)
    } else {
        (None, None, None)
    };

    let raw = ContainerRawSample {
        cpu_total_usage: cpu_total,
        system_cpu_usage: system_cpu,
        net_rx_bytes: net_rx,
        net_tx_bytes: net_tx,
        online_cpus,
        sampled_at: now,
    };

    Some((
        ContainerStats {
            name: meta.name,
            container_id,
            cpu_percent,
            mem_usage_bytes: mem_usage,
            mem_limit_bytes: mem_limit,
            mem_percent,
            net_rx_bps,
            net_tx_bps,
            status: meta.status,
        },
        raw,
    ))
}

/// Collect a snapshot of all containers matching the compose project label.
pub async fn collect_snapshot(
    docker: &Docker,
    compose_project: &str,
    prev: &HashMap<String, ContainerRawSample>,
) -> Result<(Vec<ContainerStats>, HashMap<String, ContainerRawSample>), String> {
    let now = Instant::now();
    let filter_key = "com.docker.compose.project".to_string();
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![format!("{filter_key}={compose_project}")],
    );

    let options = ListContainersOptions::<String> {
        all: false,
        filters,
        ..Default::default()
    };

    let containers = docker
        .list_containers(Some(options))
        .await
        .map_err(|e| format!("list_containers failed: {e}"))?;

    let metas: Vec<ContainerMeta> = containers.iter().filter_map(container_meta).collect();
    let docker = docker.clone();
    let permit = Arc::new(Semaphore::new(STATS_CONCURRENCY));

    let results: Vec<_> = stream::iter(metas)
        .map(|meta| {
            let docker = docker.clone();
            let permit = Arc::clone(&permit);
            let prev_sample = prev.get(&meta.id).cloned();
            async move { fetch_container_stats(docker, meta, prev_sample, now, permit).await }
        })
        .buffer_unordered(STATS_CONCURRENCY)
        .collect()
        .await;

    let mut stats_list = Vec::new();
    let mut new_prev = HashMap::new();
    for item in results.into_iter().flatten() {
        let (stats, raw) = item;
        new_prev.insert(stats.container_id.clone(), raw);
        stats_list.push(stats);
    }

    stats_list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((stats_list, new_prev))
}

/// Collect Docker named volumes for this compose project (`{project}_{volume}`).
pub async fn collect_volumes(
    docker: &Docker,
    compose_project: &str,
) -> Result<Vec<VolumeDisk>, String> {
    let prefix = format!("{compose_project}_");

    let options = ListVolumesOptions::<String>::default();
    let list = docker
        .list_volumes(Some(options))
        .await
        .map_err(|e| format!("list_volumes failed: {e}"))?;

    let vols = list.volumes.unwrap_or_default();
    let mut results = Vec::new();

    for vol in &vols {
        let name = &vol.name;
        if !name.starts_with(&prefix) {
            continue;
        }
        if vol.mountpoint.is_empty() {
            continue;
        }
        let mp = vol.mountpoint.as_str();
        let disk = crate::infra::host::get_disk_usage(mp).unwrap_or_else(|_| HostDisk {
            mount_point: mp.to_string(),
            total_bytes: 0,
            used_bytes: 0,
            available_bytes: 0,
            usage_percent: 0.0,
        });
        let short_name = name
            .strip_prefix(&prefix)
            .unwrap_or(name.as_str())
            .to_string();
        results.push(VolumeDisk {
            volume_name: short_name,
            total_bytes: disk.total_bytes,
            used_bytes: disk.used_bytes,
            available_bytes: disk.available_bytes,
            usage_percent: disk.usage_percent,
        });
    }

    results.sort_by(|a, b| a.volume_name.cmp(&b.volume_name));
    Ok(results)
}
