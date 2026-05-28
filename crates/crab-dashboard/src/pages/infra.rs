use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

use crate::api;
use crate::components::canvas_line_chart::CanvasLineChart;
use crate::components::line_chart::ChartSeries;
use crate::components::page_header::PageHeader;
use crate::locale::use_translations;
use crate::page_visible::page_visible;
use crate::types::{
    ContainerStats, HostDisk, InfraSnapshot, InfraTimeseriesResponse, SpeedTestJobView, VolumeDisk,
};

fn format_bytes(bytes: u64) -> String {
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

fn format_rate(bps: Option<f64>) -> String {
    match bps {
        Some(v) if v >= 1_000_000_000.0 => format!("{:.1} GB/s", v / 1_000_000_000.0),
        Some(v) if v >= 1_000_000.0 => format!("{:.1} MB/s", v / 1_000_000.0),
        Some(v) if v >= 1_000.0 => format!("{:.1} KB/s", v / 1_000.0),
        Some(v) => format!("{:.0} B/s", v),
        None => "—".to_string(),
    }
}

fn format_cpu(pct: Option<f64>) -> String {
    pct.map(|p| format!("{:.1}%", p))
        .unwrap_or_else(|| "—".to_string())
}

fn format_unix_secs(ts: u64) -> String {
    if ts == 0 {
        return "—".to_string();
    }
    let dt = chrono::DateTime::from_timestamp(ts as i64, 0);
    dt.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}

async fn run_upload_probe(job_id: &str, token: &str, nbytes: u64) -> Result<(), String> {
    let size = nbytes.min(10 * 1024 * 1024) as usize;
    let body = vec![0u8; size];
    api::post_infra_speed_test_upload(job_id, token, body).await
}

async fn poll_speed_job(
    job_id: &str,
    direction: &str,
    timeout_secs: u32,
) -> Result<SpeedTestJobView, String> {
    let mut upload_sent = false;
    for _ in 0..timeout_secs {
        TimeoutFuture::new(1_000).await;
        let job = api::fetch_infra_speed_test_job(job_id).await?;
        if job.status == "failed" {
            return Ok(job);
        }
        if direction == "upload"
            && !upload_sent
            && let (Some(token), bytes) = (
                job.upload_token.clone(),
                job.upload_bytes.unwrap_or(10 * 1024 * 1024),
            )
        {
            run_upload_probe(job_id, &token, bytes).await?;
            upload_sent = true;
            continue;
        }
        if direction == "both"
            && !upload_sent
            && job.download_mbps.is_some()
            && job.upload_mbps.is_none()
            && let (Some(token), bytes) = (
                job.upload_token.clone(),
                job.upload_bytes.unwrap_or(10 * 1024 * 1024),
            )
        {
            run_upload_probe(job_id, &token, bytes).await?;
            upload_sent = true;
            continue;
        }
        if job.status == "done" {
            return Ok(job);
        }
    }
    Err("timeout".into())
}

#[component]
fn ContainerTable(containers: ReadSignal<Vec<ContainerStats>>) -> impl IntoView {
    let t = use_translations();
    let cpu_hint = t.infra_cpu_hint();
    view! {
        <div class="overflow-x-auto">
            <table class="data-table">
                <thead>
                    <tr>
                        <th>{t.infra_container_name()}</th>
                        <th class="text-right" title=cpu_hint>{t.infra_cpu()}</th>
                        <th class="text-right">{t.infra_memory()}</th>
                        <th class="text-right">{t.infra_net_rx()}</th>
                        <th class="text-right">{t.infra_net_tx()}</th>
                        <th>{t.infra_status()}</th>
                    </tr>
                </thead>
                <tbody>
                    {move || {
                        containers.get().into_iter().map(|c| {
                            view! {
                                <tr>
                                    <td class="font-medium">{c.name.clone()}</td>
                                    <td class="text-right tabular-nums" title=cpu_hint>
                                        {format_cpu(c.cpu_percent)}
                                    </td>
                                    <td class="text-right tabular-nums">
                                        <div class="flex flex-col items-end gap-0.5">
                                            <span>{format_bytes(c.mem_usage_bytes)}
                                                / {format_bytes(c.mem_limit_bytes)}</span>
                                            <div class="progress-bar w-32 h-1.5 bg-base-200 rounded-full overflow-hidden">
                                                <div class="h-full rounded-full transition-all duration-500"
                                                    style:width=format!("{:.0}%", c.mem_percent.min(100.0))
                                                    style:background="var(--accent)"
                                                ></div>
                                            </div>
                                        </div>
                                    </td>
                                    <td class="text-right tabular-nums text-muted">
                                        {format_rate(c.net_rx_bps)}
                                    </td>
                                    <td class="text-right tabular-nums text-muted">
                                        {format_rate(c.net_tx_bps)}
                                    </td>
                                    <td>
                                        <span class=if c.status.contains("Up") {
                                            "badge badge-success"
                                        } else {
                                            "badge badge-warning"
                                        }>
                                            {c.status.clone()}
                                        </span>
                                    </td>
                                </tr>
                            }
                        }).collect::<Vec<_>>()
                    }}
                </tbody>
            </table>
        </div>
    }
}

#[component]
fn HostDiskCard(disks: ReadSignal<Vec<HostDisk>>) -> impl IntoView {
    let t = use_translations();
    move || {
        disks.get().into_iter().map(|d| {
            let used_pct = d.usage_percent;
            let color = if used_pct > 90.0 {
                "var(--danger)"
            } else if used_pct > 75.0 {
                "var(--warning)"
            } else {
                "var(--accent)"
            };
            view! {
                <div class="stat-card glass-card">
                    <div class="stat-label">{t.infra_disk_usage()} - {d.mount_point}</div>
                    <div class="stat-value">{format_bytes(d.used_bytes)}
                        / {format_bytes(d.total_bytes)}
                    </div>
                    <div class="progress-bar w-full h-2 bg-base-200 rounded-full overflow-hidden mt-2">
                        <div class="h-full rounded-full transition-all duration-500"
                            style:width=format!("{:.0}%", used_pct.min(100.0))
                            style:background=color
                        ></div>
                    </div>
                    <div class="flex justify-between text-xs text-muted mt-1">
                        <span>{format!("{:.1}%", used_pct)} {t.infra_disk_usage()}</span>
                        <span>{format_bytes(d.available_bytes)} {t.infra_available()}</span>
                    </div>
                </div>
            }
        }).collect::<Vec<_>>()
    }
}

#[component]
fn VolumeTable(volumes: ReadSignal<Vec<VolumeDisk>>) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="overflow-x-auto">
            <table class="data-table">
                <thead>
                    <tr>
                        <th>{t.infra_volume_name()}</th>
                        <th class="text-right">{t.infra_disk_usage()}</th>
                    </tr>
                </thead>
                <tbody>
                    {move || {
                        volumes.get().into_iter().map(|v| {
                            view! {
                                <tr>
                                    <td class="font-medium">{v.volume_name}</td>
                                    <td class="text-right tabular-nums">
                                        {format_bytes(v.used_bytes)}
                                        " / "
                                        {format_bytes(v.total_bytes)}
                                        " ("
                                        {format!("{:.1}%", v.usage_percent)}
                                        ")"
                                    </td>
                                </tr>
                            }
                        }).collect::<Vec<_>>()
                    }}
                </tbody>
            </table>
        </div>
    }
}

#[component]
fn InfraHistoryChart(
    timeseries: ReadSignal<Option<InfraTimeseriesResponse>>,
    sample_count: ReadSignal<usize>,
) -> impl IntoView {
    let t = use_translations();

    let x_labels = Signal::derive(move || {
        timeseries
            .get()
            .map(|ts| ts.cpu.iter().map(|p| p.timestamp.clone()).collect())
            .unwrap_or_default()
    });

    let series = Signal::derive(move || {
        let Some(ts) = timeseries.get() else {
            return Vec::new();
        };
        vec![
            ChartSeries {
                label: "CPU %".to_string(),
                color: "var(--accent-primary)".to_string(),
                values: ts.cpu.iter().map(|p| Some(p.value)).collect(),
                dashed: false,
                fill: true,
            },
            ChartSeries {
                label: "Mem %".to_string(),
                color: "var(--info)".to_string(),
                values: ts.memory.iter().map(|p| Some(p.value)).collect(),
                dashed: false,
                fill: true,
            },
        ]
    });

    view! {
        <>
            <p class="text-xs text-theme-muted mb-3">
                {t.infra_history_samples()}: {sample_count}
            </p>
            <CanvasLineChart
                x_labels=x_labels
                series=series
                height_px=200
                y_unit="%"
                empty_message=t.infra_history_collecting()
            />
        </>
    }
}

#[component]
pub fn InfraPage() -> impl IntoView {
    let t = use_translations();

    let snapshot: RwSignal<Option<Result<InfraSnapshot, String>>> = RwSignal::new(None);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let collection_warning: RwSignal<Option<String>> = RwSignal::new(None);
    let docker_connected = RwSignal::new(false);
    let load_gen = RwSignal::new(0u64);

    let containers = RwSignal::new(Vec::new());
    let host_disks = RwSignal::new(Vec::new());
    let volumes = RwSignal::new(Vec::new());
    let compose_project = RwSignal::new(String::new());
    let last_collected = RwSignal::new(Option::<u64>::None);
    let history_samples = RwSignal::new(0usize);

    let ts_window = RwSignal::new("1h".to_string());
    let selected_container = RwSignal::new(String::new());
    let timeseries: RwSignal<Option<InfraTimeseriesResponse>> = RwSignal::new(None);
    let speed_job: RwSignal<Option<SpeedTestJobView>> = RwSignal::new(None);
    let speed_testing = RwSignal::new(false);
    let speed_message = RwSignal::new(String::new());

    let load_timeseries = move |window: String, container_id: String| {
        if container_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::fetch_infra_timeseries(&window, Some(&container_id)).await {
                Ok(ts) => timeseries.set(Some(ts)),
                Err(_) => timeseries.set(None),
            }
        });
    };

    let reload_timeseries = Callback::new(move |(window, cid): (String, String)| {
        load_timeseries(window, cid);
    });

    let run_speed_test = move |direction: &'static str| {
        move |_| {
            speed_testing.set(true);
            speed_message.set(t.infra_speed_test_running().to_string());
            speed_job.set(None);
            let dir = direction.to_string();
            leptos::task::spawn_local(async move {
                match api::post_infra_speed_test(&dir).await {
                    Ok(accepted) => match poll_speed_job(&accepted.job_id, &dir, 120).await {
                        Ok(job) => {
                            speed_job.set(Some(job));
                            speed_message.set(String::new());
                        }
                        Err(e) => {
                            speed_message.set(if e == "timeout" {
                                t.infra_speed_test_timeout().to_string()
                            } else {
                                e
                            });
                        }
                    },
                    Err(e) => speed_message.set(e),
                }
                speed_testing.set(false);
            });
        }
    };

    let do_fetch = move || {
        load_gen.update(|g| *g += 1);
        let req_id = load_gen.get();
        let window = ts_window.get_untracked();
        let cid = selected_container.get_untracked();
        leptos::task::spawn_local(async move {
            match api::fetch_infra_status().await {
                Ok(status) => {
                    docker_connected.set(status.docker_connected);
                    compose_project.set(status.compose_project);
                    last_collected.set(status.last_collected_at);
                    history_samples.set(status.history_sample_count);
                }
                Err(_) => docker_connected.set(false),
            }
            if load_gen.get() != req_id {
                return;
            }
            match api::fetch_infra_snapshot().await {
                Ok(snap) => {
                    let connected = snap.docker_connected;
                    let first_id = snap.containers.first().map(|c| c.container_id.clone());
                    containers.set(snap.containers.clone());
                    host_disks.set(snap.host_disks.clone());
                    volumes.set(snap.volumes.clone());
                    compose_project.set(snap.compose_project.clone());
                    last_collected.set(Some(snap.collected_at));
                    collection_warning.set(snap.collection_error.clone());
                    docker_connected.set(connected);
                    snapshot.set(Some(Ok(snap)));
                    error.set(None);

                    if selected_container.get_untracked().is_empty()
                        && let Some(id) = first_id.clone()
                    {
                        selected_container.set(id);
                    }
                    let chart_cid = if cid.is_empty() {
                        first_id.unwrap_or_default()
                    } else {
                        cid
                    };
                    if connected && !chart_cid.is_empty() {
                        load_timeseries(window, chart_cid);
                    }
                }
                Err(e) => {
                    error.set(Some(e));
                    snapshot.set(None);
                }
            }
        });
    };

    do_fetch();
    leptos::task::spawn_local(async move {
        loop {
            TimeoutFuture::new(10_000).await;
            if page_visible() {
                do_fetch();
            }
        }
    });

    view! {
        <PageHeader
            title={move || t.infra_title()}
            description={move || t.infra_desc()}
        >
            <span></span>
        </PageHeader>

        <div class="page-content space-y-6">
            {move || {
                if !docker_connected.get() {
                    view! {
                        <div class="alert alert-warning">
                            <span class="alert-icon">!</span>
                            <span>{t.infra_docker_unavailable()}</span>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            {move || {
                collection_warning.get().map(|w| view! {
                    <div class="alert alert-warning">
                        <span class="alert-icon">!</span>
                        <span>{w}</span>
                    </div>
                })
            }}

            {move || {
                error.get().map(|e| view! {
                    <div class="alert alert-error">
                        <span class="alert-icon">!</span>
                        <span>{e}</span>
                    </div>
                })
            }}

            <div class="dash-card">
                <div class="dash-card-header">
                    <span class="dash-card-title">{t.infra_containers()}</span>
                    <span class="panel-header-meta">
                        {move || {
                            let count = containers.get().len();
                            let project = compose_project.get();
                            if count > 0 && !project.is_empty() {
                                format!("{} containers \u{00b7} {} \u{00b7} {}", count, project, format_unix_secs(last_collected.get().unwrap_or(0)))
                            } else if count > 0 {
                                format!("{} containers \u{00b7} {}", count, format_unix_secs(last_collected.get().unwrap_or(0)))
                            } else {
                                String::new()
                            }
                        }}
                    </span>
                </div>
                <div class="dash-card-body-flush">
                    {move || {
                        if docker_connected.get() {
                            view! {
                                <ContainerTable containers=containers.read_only() />
                            }.into_any()
                        } else {
                            view! {
                                <div class="p-8 text-center text-theme-muted">
                                    <p class="text-lg">{t.infra_docker_unavailable()}</p>
                                </div>
                            }.into_any()
                        }
                    }}
                </div>
            </div>

            <div class="dash-card">
                <div class="dash-card-header">
                    <span class="dash-card-title">{t.infra_host_disk()}</span>
                </div>
                <div class="dash-card-body">
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                        <HostDiskCard disks=host_disks.read_only() />
                    </div>
                </div>
            </div>

            {move || (!volumes.get().is_empty()).then(|| view! {
                <div class="dash-card">
                    <div class="dash-card-header">
                        <span class="dash-card-title">{t.infra_volumes()}</span>
                    </div>
                    <div class="dash-card-body-flush">
                        <VolumeTable volumes=volumes.read_only() />
                    </div>
                </div>
            })}

            {move || docker_connected.get().then(|| {
                let container_options = containers.get();
                view! {
                    <div class="dash-card">
                        <div class="dash-card-header">
                            <span class="dash-card-title">{t.infra_history()}</span>
                            <div class="flex items-center gap-2">
                                <select
                                    id="infra-chart-container"
                                    class="input input-sm"
                                    style="width: auto; min-width: 8rem;"
                                    on:change=move |ev| {
                                        let value = event_target_value(&ev);
                                        selected_container.set(value.clone());
                                        load_timeseries(ts_window.get_untracked(), value);
                                    }
                                >
                                    {container_options.into_iter().map(|c| {
                                        let id = c.container_id.clone();
                                        let name = c.name.clone();
                                        view! { <option value=id>{name}</option> }
                                    }).collect::<Vec<_>>()}
                                </select>
                                <div class="flex gap-1">
                                    <button
                                        type="button"
                                        class=move || if ts_window.get() == "1h" { "btn btn-primary btn-sm" } else { "btn btn-secondary btn-sm" }
                                        on:click={
                                            let cid = selected_container;
                                            move |_| {
                                                ts_window.set("1h".to_string());
                                                reload_timeseries.run(("1h".to_string(), cid.get()));
                                            }
                                        }
                                    >"1h"</button>
                                    <button
                                        type="button"
                                        class=move || if ts_window.get() == "24h" { "btn btn-primary btn-sm" } else { "btn btn-secondary btn-sm" }
                                        on:click={
                                            let cid = selected_container;
                                            move |_| {
                                                ts_window.set("24h".to_string());
                                                reload_timeseries.run(("24h".to_string(), cid.get()));
                                            }
                                        }
                                    >"24h"</button>
                                </div>
                            </div>
                        </div>
                        <div class="dash-card-body">
                            <InfraHistoryChart
                                timeseries=timeseries.read_only()
                                sample_count=history_samples.read_only()
                            />
                        </div>
                    </div>
                }
            })}

            <div class="dash-card">
                <div class="dash-card-header">
                    <span class="dash-card-title">{t.infra_speed_test()}</span>
                </div>
                <div class="dash-card-body space-y-3">
                    <div class="flex flex-wrap gap-2">
                        <button
                            type="button"
                            class="btn btn-secondary text-sm"
                            disabled=move || speed_testing.get()
                            on:click=run_speed_test("download")
                        >
                            {t.infra_speed_test_run()}
                        </button>
                        <button
                            type="button"
                            class="btn btn-secondary text-sm"
                            disabled=move || speed_testing.get()
                            on:click=run_speed_test("upload")
                        >
                            {t.infra_speed_test_upload()}
                        </button>
                        <button
                            type="button"
                            class="btn btn-secondary text-sm"
                            disabled=move || speed_testing.get()
                            on:click=run_speed_test("both")
                        >
                            {t.infra_speed_test_both()}
                        </button>
                    </div>
                    {move || {
                        if speed_testing.get() {
                            view! { <p class="text-sm text-theme-muted">{speed_message.get()}</p> }.into_any()
                        } else if let Some(job) = speed_job.get() {
                            let mut result_rows = Vec::new();
                            if let Some(mbps) = job.download_mbps {
                                result_rows.push(view! {
                                    <div class="test-result-row">
                                        <span class="test-result-label">{t.infra_speed_test_result()}</span>
                                        <span class="test-result-value test-result-value-mono">{format!("{:.1} Mbps", mbps)}</span>
                                    </div>
                                });
                            }
                            if let Some(mbps) = job.upload_mbps {
                                result_rows.push(view! {
                                    <div class="test-result-row">
                                        <span class="test-result-label">{t.infra_upload_result()}</span>
                                        <span class="test-result-value test-result-value-mono">{format!("{:.1} Mbps", mbps)}</span>
                                    </div>
                                });
                            }
                            if let Some(err) = &job.error {
                                view! { <p class="text-sm" style="color: var(--error)">{err.clone()}</p> }.into_any()
                            } else if result_rows.is_empty() {
                                ().into_any()
                            } else {
                                view! { <div class="test-result-panel mt-2">{result_rows}</div> }.into_any()
                            }
                        } else if !speed_message.get().is_empty() {
                            view! { <p class="text-sm" style="color: var(--error)">{speed_message.get()}</p> }.into_any()
                        } else {
                            ().into_any()
                        }
                    }}
                </div>
            </div>
        </div>
    }
}
