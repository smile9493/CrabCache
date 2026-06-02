//! Analytics panels for the unified Overview page (below gateway status + infra).

use crate::api;
use crate::locale::use_translations;
use crate::types::{
    ContainerStats, HostDisk, InfraSnapshot, VolumeDisk,
};
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy)]
enum InfraTileVariant {
    Teal,
    Orange,
    Green,
    Accent,
    Muted,
    Warn,
}

fn infra_tile_class(v: InfraTileVariant) -> &'static str {
    match v {
        InfraTileVariant::Teal => "live-stat-tile live-stat-tile-teal",
        InfraTileVariant::Orange => "live-stat-tile live-stat-tile-orange",
        InfraTileVariant::Green => "live-stat-tile live-stat-tile-green",
        InfraTileVariant::Accent => "live-stat-tile live-stat-tile-accent",
        InfraTileVariant::Muted => "live-stat-tile live-stat-tile-muted",
        InfraTileVariant::Warn => "live-stat-tile live-stat-tile-warn",
    }
}

struct InfraSnapshotSummary {
    rx_bps: f64,
    tx_bps: f64,
    total_containers: usize,
    active_containers: usize,
    mem_used_bytes: u64,
    mem_limit_bytes: u64,
    disk_used_bytes: u64,
    disk_total_bytes: u64,
    max_disk_pct: f64,
}

fn container_is_active(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.contains("up") || s.contains("running")
}

/// Active/total container count for overview Infra metric card headline.
pub fn infra_container_headline(snapshot: &InfraSnapshot) -> String {
    let summary = summarize_infra(snapshot);
    format!("{}/{}", summary.active_containers, summary.total_containers)
}

fn summarize_infra(s: &InfraSnapshot) -> InfraSnapshotSummary {
    let mut rx_bps = 0.0;
    let mut tx_bps = 0.0;
    let mut mem_used_bytes = 0u64;
    let mut mem_limit_bytes = 0u64;
    let mut active_containers = 0usize;

    for c in &s.containers {
        rx_bps += c.net_rx_bps.unwrap_or(0.0);
        tx_bps += c.net_tx_bps.unwrap_or(0.0);
        mem_used_bytes = mem_used_bytes.saturating_add(c.mem_usage_bytes);
        mem_limit_bytes = mem_limit_bytes.saturating_add(c.mem_limit_bytes);
        if container_is_active(&c.status) {
            active_containers += 1;
        }
    }

    let mut disk_used_bytes = 0u64;
    let mut disk_total_bytes = 0u64;
    let mut max_disk_pct = 0.0f64;
    for d in &s.host_disks {
        disk_used_bytes = disk_used_bytes.saturating_add(d.used_bytes);
        disk_total_bytes = disk_total_bytes.saturating_add(d.total_bytes);
        if d.usage_percent > max_disk_pct {
            max_disk_pct = d.usage_percent;
        }
    }

    InfraSnapshotSummary {
        rx_bps,
        tx_bps,
        total_containers: s.containers.len(),
        active_containers,
        mem_used_bytes,
        mem_limit_bytes,
        disk_used_bytes,
        disk_total_bytes,
        max_disk_pct,
    }
}

#[component]
fn InfraStatTile(label: &'static str, value: String, variant: InfraTileVariant) -> impl IntoView {
    view! {
        <div class=infra_tile_class(variant)>
            <span class="live-stat-tile-label">{label}</span>
            <span class="live-stat-tile-value">{value}</span>
        </div>
    }
}

#[derive(Clone, Copy)]
enum InfraModuleIcon {
    Containers,
    Disk,
    Volume,
}

#[component]
fn InfraModuleIconView(kind: InfraModuleIcon) -> impl IntoView {
    match kind {
        InfraModuleIcon::Containers => view! {
            <svg class="infra-detail-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M4 7h16v10H4z"></path>
                <path d="M8 7V5h8v2"></path>
            </svg>
        }.into_any(),
        InfraModuleIcon::Disk => view! {
            <svg class="infra-detail-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <ellipse cx="12" cy="6" rx="7" ry="2"></ellipse>
                <path d="M5 6v12c0 1.1 3.1 2 7 2s7-.9 7-2V6"></path>
            </svg>
        }.into_any(),
        InfraModuleIcon::Volume => view! {
            <svg class="infra-detail-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M12 3L4 7v10l8 4 8-4V7z"></path>
                <path d="M12 11v10"></path>
            </svg>
        }.into_any(),
    }
}

#[component]
fn InfraDetailModule(
    icon: InfraModuleIcon,
    title: &'static str,
    meta: String,
    children: Children,
) -> impl IntoView {
    view! {
        <section class="infra-detail-module">
            <div class="infra-detail-module-head">
                <span class="infra-detail-module-title">
                    <InfraModuleIconView kind=icon />
                    <span>{title}</span>
                </span>
                <span class="infra-detail-module-meta">{meta}</span>
            </div>
            <div class="infra-detail-module-body">
                {children()}
            </div>
        </section>
    }
}

#[component]
pub fn InfraOverviewModule(
    /// When true, loads on mount and renders without collapsible wrapper (for modals).
    #[prop(default = false)]
    embedded: bool,
) -> impl IntoView {
    let t = use_translations();
    let snapshot: RwSignal<Option<Result<InfraSnapshot, String>>> = RwSignal::new(None);
    let loaded = RwSignal::new(false);
    let alive = Arc::new(AtomicBool::new(true));

    let load_snapshot = move || {
        if !loaded.get_untracked() {
            loaded.set(true);
        }
        leptos::task::spawn_local(async move {
            let result = api::fetch_infra_snapshot().await;
            match result {
                Ok(s) => {
                    snapshot.try_set(Some(Ok(s)));
                }
                Err(e) => {
                    snapshot.try_set(Some(Err(e)));
                }
            }
        });
    };

    {
        let alive_poll = Arc::clone(&alive);
        leptos::task::spawn_local(async move {
            loop {
                TimeoutFuture::new(15_000).await;
                if !alive_poll.load(Ordering::Relaxed) {
                    break;
                }
                if loaded.try_get_untracked() == Some(true) {
                    load_snapshot();
                }
            }
        });
    }

    on_cleanup(move || {
        alive.store(false, Ordering::Relaxed);
    });

    if embedded {
        Effect::new(move |_| {
            if !loaded.get_untracked() {
                load_snapshot();
            }
        });
    }

    let body = move || match snapshot.get() {
        None => view! {
            <div class="text-xs text-theme-muted">"Loading infrastructure snapshot..."</div>
        }
        .into_any(),
        Some(Err(e)) => view! {
            <div class="text-xs text-warning">{format!("Infra snapshot unavailable: {e}")}</div>
        }
        .into_any(),
        Some(Ok(s)) => {
            let summary = summarize_infra(&s);
            let mut top_containers = s.containers.clone();
            top_containers.sort_by(|a, b| {
                let a_cpu = a.cpu_percent.unwrap_or(0.0);
                let b_cpu = b.cpu_percent.unwrap_or(0.0);
                b_cpu
                    .partial_cmp(&a_cpu)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            top_containers.truncate(3);
            let collection_error = s.collection_error.clone();
            let all_containers = s.containers.clone();
            let host_disks = s.host_disks.clone();
            let volumes = s.volumes.clone();
            let compose_project = s.compose_project.clone();
            let docker_connected = s.docker_connected;
            let collected_at = s.collected_at;
            let mem_pct = if summary.mem_limit_bytes > 0 {
                summary.mem_used_bytes as f64 / summary.mem_limit_bytes as f64 * 100.0
            } else {
                0.0
            };
            let disk_pct = if summary.disk_total_bytes > 0 {
                summary.disk_used_bytes as f64 / summary.disk_total_bytes as f64 * 100.0
            } else {
                summary.max_disk_pct
            };
            let rx_display = if summary.rx_bps > 0.0 {
                fmt_rate(Some(summary.rx_bps))
            } else {
                "—".to_string()
            };
            let tx_display = if summary.tx_bps > 0.0 {
                fmt_rate(Some(summary.tx_bps))
            } else {
                "—".to_string()
            };
            let active_display =
                format!("{}/{}", summary.active_containers, summary.total_containers);
            let mem_display = fmt_bytes(summary.mem_used_bytes);
            let disk_display = if summary.disk_total_bytes > 0 {
                fmt_bytes(summary.disk_used_bytes)
            } else {
                format!("{:.1}%", summary.max_disk_pct)
            };
            let docker_label = t.sidebar_infra();
            let docker_value = if docker_connected {
                "OK".to_string()
            } else {
                "—".to_string()
            };
            let details_meta = format!(
                "{} · {} · {}",
                summary.total_containers,
                host_disks.len(),
                volumes.len()
            );

            view! {
                        <div class="space-y-3">
                            <div class="infra-overview-meta">
                                {compose_project}
                                " · "
                                {t.infra_last_collected()}
                                " "
                                {fmt_unix_secs(collected_at)}
                            </div>
                            <div class="infra-stat-grid">
                                <InfraStatTile
                                    label=t.infra_download_speed()
                                    value=rx_display
                                    variant=InfraTileVariant::Teal
                                />
                                <InfraStatTile
                                    label=t.infra_upload_speed()
                                    value=tx_display
                                    variant=InfraTileVariant::Orange
                                />
                                <InfraStatTile
                                    label=t.infra_active_containers()
                                    value=active_display
                                    variant=InfraTileVariant::Green
                                />
                                <InfraStatTile
                                    label=t.infra_mem_cumulative()
                                    value=mem_display
                                    variant=InfraTileVariant::Accent
                                />
                                <InfraStatTile
                                    label=t.infra_disk_cumulative()
                                    value=disk_display
                                    variant=InfraTileVariant::Muted
                                />
                                <InfraStatTile
                                    label=docker_label
                                    value=docker_value
                                    variant=if docker_connected { InfraTileVariant::Accent } else { InfraTileVariant::Warn }
                                />
                            </div>
                            <div class="text-[11px] text-theme-muted font-mono tabular-nums flex flex-wrap gap-x-3 gap-y-0.5">
                                <span>{format!("{:.1}% {}", mem_pct, t.infra_memory())}</span>
                                <span>{format!("{:.1}% {}", disk_pct, t.infra_disk_usage())}</span>
                            </div>
                            {if top_containers.is_empty() {
                                view! {
                                    <div class="text-xs text-theme-muted">"No container metrics yet."</div>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="space-y-1.5">
                                        <div class="text-xs font-semibold text-theme-muted">{t.infra_top_containers()}</div>
                                        <div class="overflow-x-auto">
                                            <table class="w-full text-xs">
                                                <thead>
                                                    <tr class="text-left text-theme-muted border-b border-theme">
                                                        <th class="py-1 pr-2">{t.infra_container_name()}</th>
                                                        <th class="py-1 pr-2">{t.infra_cpu()}</th>
                                                        <th class="py-1 pr-2">{t.infra_memory()}</th>
                                                        <th class="py-1">{format!("{}/{}", t.infra_net_rx(), t.infra_net_tx())}</th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {top_containers.into_iter().map(|c| {
                                                        view! {
                                                            <tr class="border-b border-theme/40">
                                                                <td class="py-1 pr-2">{c.name}</td>
                                                                <td class="py-1 pr-2 font-mono">{fmt_opt_pct(c.cpu_percent)}</td>
                                                                <td class="py-1 pr-2 font-mono">{format!("{:.1}%", c.mem_percent)}</td>
                                                                <td class="py-1 font-mono">
                                                                    {format!("{}/{}", fmt_rate(c.net_rx_bps), fmt_rate(c.net_tx_bps))}
                                                                </td>
                                                            </tr>
                                                        }
                                                    }).collect_view()}
                                                </tbody>
                                            </table>
                                        </div>
                                    </div>
                                }.into_any()
                            }}
                            {collection_error.map(|msg| {
                                view! {
                                    <div class="text-xs text-warning bg-warning/10 border border-warning/20 rounded px-2 py-1">
                                        {msg}
                                    </div>
                                }
                            })}
                            <details class="infra-details">
                                <summary>
                                    <span>{t.infra_expand_details()}</span>
                                    <span class="font-mono">{details_meta}</span>
                                </summary>
                                <div class="infra-details-body">
                                    <InfraDetailModule
                                        icon=InfraModuleIcon::Containers
                                        title=t.infra_containers()
                                        meta=format!("{} {}", summary.total_containers, t.infra_status())
                                    >
                                        <InfraContainersDenseTable containers=all_containers />
                                    </InfraDetailModule>
                                    <InfraDetailModule
                                        icon=InfraModuleIcon::Disk
                                        title=t.infra_host_disk()
                                        meta=format!("{:.1}% peak", summary.max_disk_pct)
                                    >
                                        <InfraHostDiskBars disks=host_disks />
                                    </InfraDetailModule>
                                    {(!volumes.is_empty()).then(|| view! {
                                        <InfraDetailModule
                                            icon=InfraModuleIcon::Volume
                                            title=t.infra_volumes()
                                            meta=format!("{}", volumes.len())
                                        >
                                            <InfraVolumesDenseTable volumes=volumes />
                                        </InfraDetailModule>
                                    })}
                                </div>
                            </details>
                        </div>
                    }.into_any()
        }
    };

    view! {
        {if embedded {
            view! { <div class="space-y-3">{body}</div> }.into_any()
        } else {
            view! {
                <details class="overview-collapsible"
                    on:toggle=move |ev| {
                        let el = ev.target().unwrap().unchecked_into::<web_sys::HtmlDetailsElement>();
                        if el.open() && !loaded.get_untracked() {
                            load_snapshot();
                        }
                    }
                >
                    <summary class="overview-collapsible-head">
                        <span class="overview-collapsible-title">
                            <InfraModuleIconContainers />
                            <span>{t.infra_title()}</span>
                        </span>
                    </summary>
                    <div class="overview-collapsible-body">
                        {body}
                    </div>
                </details>
            }.into_any()
        }}
    }
}

#[component]
fn InfraModuleIconContainers() -> impl IntoView {
    view! {
        <svg class="overview-module-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="2" y="7" width="20" height="14" rx="2" ry="2"></rect>
            <path d="M16 7V5a2 2 0 00-2-2h-4a2 2 0 00-2 2v2"></path>
        </svg>
    }
}

fn fmt_opt_pct(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}%"))
        .unwrap_or_else(|| "—".to_string())
}

fn fmt_rate(v: Option<f64>) -> String {
    match v {
        Some(x) if x >= 1_000_000_000.0 => format!("{:.1} GB/s", x / 1_000_000_000.0),
        Some(x) if x >= 1_000_000.0 => format!("{:.1} MB/s", x / 1_000_000.0),
        Some(x) if x >= 1_000.0 => format!("{:.1} KB/s", x / 1_000.0),
        Some(x) => format!("{:.0} B/s", x),
        None => "—".to_string(),
    }
}

fn fmt_bytes(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GiB", bytes as f64 / (1 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / (1 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KiB", bytes as f64 / (1 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
}

fn fmt_unix_secs(ts: u64) -> String {
    if ts == 0 {
        return "—".to_string();
    }
    let dt = chrono::DateTime::from_timestamp(ts as i64, 0);
    dt.map(|d| d.format("%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}

#[component]
fn InfraContainersDenseTable(containers: Vec<ContainerStats>) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="overflow-x-auto">
            <table class="w-full text-xs">
                <thead>
                    <tr class="text-left text-theme-muted border-b border-theme">
                        <th class="py-1 pr-2">{t.infra_container_name()}</th>
                        <th class="py-1 pr-2">{t.infra_cpu()}</th>
                        <th class="py-1 pr-2">{t.infra_memory()}</th>
                        <th class="py-1 pr-2">{t.infra_net_rx()}</th>
                        <th class="py-1 pr-2">{t.infra_net_tx()}</th>
                        <th class="py-1">{t.infra_status()}</th>
                    </tr>
                </thead>
                <tbody>
                    {containers.into_iter().map(|c| {
                        view! {
                            <tr class="border-b border-theme/40">
                                <td class="py-1 pr-2">{c.name}</td>
                                <td class="py-1 pr-2 font-mono">{fmt_opt_pct(c.cpu_percent)}</td>
                                <td class="py-1 pr-2 font-mono">{format!("{:.1}%", c.mem_percent)}</td>
                                <td class="py-1 pr-2 font-mono">{fmt_rate(c.net_rx_bps)}</td>
                                <td class="py-1 pr-2 font-mono">{fmt_rate(c.net_tx_bps)}</td>
                                <td class="py-1">{c.status}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

#[component]
fn InfraHostDiskBars(disks: Vec<HostDisk>) -> impl IntoView {
    view! {
        <div class="space-y-2">
            {disks.into_iter().map(|d| {
                view! {
                    <div class="space-y-1">
                        <div class="flex items-center justify-between text-xs">
                            <span class="font-mono">{d.mount_point}</span>
                            <span class="font-mono">{format!("{:.1}%", d.usage_percent)}</span>
                        </div>
                        <div class="h-1.5 rounded bg-theme/20 overflow-hidden">
                            <div
                                class="h-full rounded bg-accent"
                                style=format!("width: {:.1}%", d.usage_percent.min(100.0))
                            ></div>
                        </div>
                        <div class="text-[11px] text-theme-muted font-mono">
                            {format!("{} / {} · avail {}", fmt_bytes(d.used_bytes), fmt_bytes(d.total_bytes), fmt_bytes(d.available_bytes))}
                        </div>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

#[component]
fn InfraVolumesDenseTable(volumes: Vec<VolumeDisk>) -> impl IntoView {
    view! {
        <div class="overflow-x-auto">
            <table class="w-full text-xs">
                <thead>
                    <tr class="text-left text-theme-muted border-b border-theme">
                        <th class="py-1 pr-2">"Volume"</th>
                        <th class="py-1">"Usage"</th>
                    </tr>
                </thead>
                <tbody>
                    {volumes.into_iter().map(|v| {
                        view! {
                            <tr class="border-b border-theme/40">
                                <td class="py-1 pr-2">{v.volume_name}</td>
                                <td class="py-1 font-mono">
                                    {format!("{} / {} ({:.1}%)", fmt_bytes(v.used_bytes), fmt_bytes(v.total_bytes), v.usage_percent)}
                                </td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}
