//! Analytics tab content for the Overview page.
//!
//! Extracted from `overview.rs` to keep the main page component lean and
//! allow Leptos to defer DOM/JS initialization until the tab is activated.

use crate::api;
use crate::types::{
    ContainerStats, HostDisk, InfraSnapshot, MetricsSnapshot, OverviewOpsMetrics,
    OverviewSuggestion, PrefixCacheMetricsSnapshot, SemanticConfig, TimeSeriesPoint, VolumeDisk,
};
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

#[component]
pub fn OverviewAnalytics(
    suggestions_memo: Memo<Option<Vec<OverviewSuggestion>>>,
    ts_points: RwSignal<Vec<TimeSeriesPoint>>,
    ts_window: RwSignal<String>,
    prefix_memo: Memo<Option<PrefixCacheMetricsSnapshot>>,
    metrics_memo: Memo<Option<MetricsSnapshot>>,
    ops_memo: Memo<Option<OverviewOpsMetrics>>,
    semantic_memo: Memo<Option<SemanticConfig>>,
    selected_domain: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="space-y-6">
            <InfraOverviewSection />
            {move || suggestions_memo.get().map(|s| {
                view! {
                    <super::overview::TimeSeriesChart
                        points=ts_points
                        selected_view=ts_window
                        suggestions=s
                    />
                }
            })}
            {move || prefix_memo.get().zip(metrics_memo.get()).map(|(pref, _m)| view! {
                <super::overview::PrefixCacheCard prefix=pref.clone() />
            })}
            {move || metrics_memo.get().zip(prefix_memo.get()).map(|(m, pref)| view! {
                <super::overview::TokenStats metrics=m.clone() prefix=pref.clone() />
            })}
            {move || metrics_memo.get().zip(ops_memo.get()).map(|(m, ops)| view! {
                <div class="bento-grid-2">
                    <super::overview::CoalescingCard metrics=m.clone() ops=ops.clone() />
                    <super::overview::SemanticCacheCard metrics=m.clone() semantic=semantic_memo.get().unwrap_or(SemanticConfig { enabled: false, similarity_threshold: 0.9 }) />
                </div>
            })}
            {move || metrics_memo.get().map(|m| view! {
                <super::overview::ConsumerHitTable metrics=m.clone() />
            })}
            {move || metrics_memo.get().map(|m| {
                let cb = Callback::new(move |domain: String| {
                    selected_domain.set(Some(domain));
                });
                view! {
                    <super::domains::DomainOverviewTableInline metrics=m.clone() on_domain_click=cb />
                }
            })}
            <super::domains::DomainDetailDrawer domain=selected_domain />
            {move || metrics_memo.get().zip(ops_memo.get()).map(|(m, ops)| view! {
                <div class="bento-grid-3">
                    <div class="bento-cell">
                        <super::overview::CacheHitSection metrics=m.clone() />
                    </div>
                    <div class="bento-cell">
                        <super::overview::CostSavingsSection ops=ops.clone() />
                    </div>
                    <div class="bento-cell">
                        <super::overview::LatencySection metrics=m.clone() />
                    </div>
                </div>
            })}
            {move || ops_memo.get().map(|ops| view! {
                <div class="bento-grid-2">
                    <super::overview::UpstreamKeyStrip ops=ops.clone() />
                    <super::overview::PrefixHealthCard ops=ops.clone() />
                </div>
            })}
        </div>
    }
}

#[component]
pub fn InfraOverviewSection() -> impl IntoView {
    let snapshot: RwSignal<Option<Result<InfraSnapshot, String>>> = RwSignal::new(None);

    let load_snapshot = move || {
        leptos::task::spawn_local(async move {
            let result = api::fetch_infra_snapshot().await;
            match result {
                Ok(s) => snapshot.set(Some(Ok(s))),
                Err(e) => snapshot.set(Some(Err(e))),
            }
        });
    };

    load_snapshot();
    leptos::task::spawn_local(async move {
        loop {
            TimeoutFuture::new(15_000).await;
            load_snapshot();
        }
    });

    view! {
        <div class="glass-card space-y-4">
            <div class="flex items-center justify-between">
                <h3 class="text-sm font-semibold text-theme">"Infrastructure"</h3>
                <a href="/infra" class="text-xs text-accent hover:underline">"Open full infra page"</a>
            </div>
            {move || match snapshot.get() {
                None => view! { <div class="text-xs text-theme-muted">"Loading infrastructure snapshot..."</div> }.into_any(),
                Some(Err(e)) => view! { <div class="text-xs text-warning">{format!("Infra snapshot unavailable: {e}")}</div> }.into_any(),
                Some(Ok(s)) => {
                    let container_count = s.containers.len();
                    let mut top_containers = s.containers.clone();
                    top_containers.sort_by(|a, b| {
                        let a_cpu = a.cpu_percent.unwrap_or(0.0);
                        let b_cpu = b.cpu_percent.unwrap_or(0.0);
                        b_cpu.partial_cmp(&a_cpu).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    top_containers.truncate(3);
                    let top_disk = top_host_disk(&s.host_disks);
                    let collection_error = s.collection_error.clone();
                    let all_containers = s.containers.clone();
                    let host_disks = s.host_disks.clone();
                    let volumes = s.volumes.clone();
                    let collected_at = s.collected_at;

                    view! {
                        <div class="space-y-3">
                            <div class="grid grid-cols-2 md:grid-cols-5 gap-3 text-sm">
                                <div>
                                    <div class="text-xs text-theme-muted">"Docker"</div>
                                    <div class=if s.docker_connected { "text-accent font-semibold" } else { "text-warning font-semibold" }>
                                        {if s.docker_connected { "Connected" } else { "Unavailable" }}
                                    </div>
                                </div>
                                <div>
                                    <div class="text-xs text-theme-muted">"Containers"</div>
                                    <div class="font-mono tabular-nums">{container_count}</div>
                                </div>
                                <div>
                                    <div class="text-xs text-theme-muted">"Compose Project"</div>
                                    <div class="font-mono truncate">{s.compose_project.clone()}</div>
                                </div>
                                <div>
                                    <div class="text-xs text-theme-muted">"Host Disk"</div>
                                    <div class="font-mono tabular-nums">
                                        {top_disk
                                            .map(|d| format!("{:.1}% used", d.usage_percent))
                                            .unwrap_or_else(|| "—".to_string())}
                                    </div>
                                </div>
                                <div>
                                    <div class="text-xs text-theme-muted">"Collected"</div>
                                    <div class="font-mono truncate">{fmt_unix_secs(collected_at)}</div>
                                </div>
                            </div>
                            {if top_containers.is_empty() {
                                view! { <div class="text-xs text-theme-muted">"No container metrics yet."</div> }.into_any()
                            } else {
                                view! {
                                    <div class="overflow-x-auto">
                                        <table class="w-full text-xs">
                                            <thead>
                                                <tr class="text-left text-theme-muted border-b border-theme">
                                                    <th class="py-1 pr-2">"Container"</th>
                                                    <th class="py-1 pr-2">"CPU"</th>
                                                    <th class="py-1 pr-2">"Mem"</th>
                                                    <th class="py-1">"RX/TX"</th>
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
                                }.into_any()
                            }}
                            {collection_error.map(|msg| {
                                view! {
                                    <div class="text-xs text-warning bg-warning/10 border border-warning/20 rounded px-2 py-1">
                                        {msg}
                                    </div>
                                }
                            })}
                            <details class="rounded border border-theme/40">
                                <summary class="cursor-pointer list-none px-3 py-2 text-xs text-theme-muted flex items-center justify-between">
                                    <span>"Show infra details"</span>
                                    <span class="font-mono">{format!("{} containers · {} disks · {} volumes", all_containers.len(), host_disks.len(), volumes.len())}</span>
                                </summary>
                                <div class="p-3 pt-2 space-y-4 border-t border-theme/30">
                                    <div class="space-y-2">
                                        <h4 class="text-xs font-semibold text-theme">"All containers"</h4>
                                        <InfraContainersDenseTable containers=all_containers />
                                    </div>
                                    <div class="space-y-2">
                                        <h4 class="text-xs font-semibold text-theme">"Host disks"</h4>
                                        <InfraHostDiskBars disks=host_disks />
                                    </div>
                                    {(!volumes.is_empty()).then(|| view! {
                                        <div class="space-y-2">
                                            <h4 class="text-xs font-semibold text-theme">"Volumes"</h4>
                                            <InfraVolumesDenseTable volumes=volumes />
                                        </div>
                                    })}
                                </div>
                            </details>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

fn fmt_opt_pct(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}%"))
        .unwrap_or_else(|| "—".to_string())
}

fn fmt_rate(v: Option<f64>) -> String {
    match v {
        Some(x) if x >= 1_000_000.0 => format!("{:.1}MB/s", x / 1_000_000.0),
        Some(x) if x >= 1_000.0 => format!("{:.1}KB/s", x / 1_000.0),
        Some(x) => format!("{:.0}B/s", x),
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

fn top_host_disk(disks: &[HostDisk]) -> Option<&HostDisk> {
    disks.iter().max_by(|a, b| {
        a.usage_percent
            .partial_cmp(&b.usage_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

#[component]
fn InfraContainersDenseTable(containers: Vec<ContainerStats>) -> impl IntoView {
    view! {
        <div class="overflow-x-auto">
            <table class="w-full text-xs">
                <thead>
                    <tr class="text-left text-theme-muted border-b border-theme">
                        <th class="py-1 pr-2">"Name"</th>
                        <th class="py-1 pr-2">"CPU"</th>
                        <th class="py-1 pr-2">"Mem"</th>
                        <th class="py-1 pr-2">"RX"</th>
                        <th class="py-1 pr-2">"TX"</th>
                        <th class="py-1">"Status"</th>
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
