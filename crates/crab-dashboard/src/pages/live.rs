use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

use crate::api;
use crate::components::line_chart::{ChartSeries, LineChart, TokenLineChart};
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::page_visible::page_visible;
use crate::pages::overview::format_number;
use crate::types::{LiveMetricsBucket, LiveMetricsResponse};

const MAX_CHART_POINTS: usize = 36;

fn format_bucket_time(ts_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ts_ms as i64)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn poll_interval_ms(window_secs: u32) -> u32 {
    if window_secs >= 900 { 3000 } else { 2000 }
}

fn compress_chart_buckets(buckets: &[LiveMetricsBucket]) -> Vec<LiveMetricsBucket> {
    let active: Vec<_> = buckets
        .iter()
        .filter(|b| b.request_count > 0)
        .cloned()
        .collect();
    if active.len() <= MAX_CHART_POINTS {
        return active;
    }
    active[active.len() - MAX_CHART_POINTS..].to_vec()
}

#[component]
pub fn LivePage() -> impl IntoView {
    let t = use_translations();
    let consumers: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let selected_consumer: RwSignal<Option<String>> = RwSignal::new(None);
    let window_secs: RwSignal<u32> = RwSignal::new(300);
    let live_data: RwSignal<Option<Result<LiveMetricsResponse, String>>> = RwSignal::new(None);
    let consumers_loaded = RwSignal::new(false);
    let consumers_error = RwSignal::new(None::<String>);
    let auto_refresh = RwSignal::new(true);
    let last_update = RwSignal::new(String::new());
    let load_generation = RwSignal::new(0u64);

    // Fetch consumers from live-metrics/consumers endpoint (lightweight).
    // Falls back to fetch_keys if consumers endpoint is not available.
    let load_consumers = move || {
        leptos::task::spawn_local(async move {
            // Try the lightweight consumers endpoint first.
            match api::fetch_live_consumers(300).await {
                Ok(val) => {
                    let names: Vec<String> = val
                        .get("available_consumers")
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !names.is_empty() {
                        if selected_consumer.get_untracked().is_none() {
                            if let Some(first) = names.first() {
                                selected_consumer.set(Some(first.clone()));
                            }
                        }
                        consumers.set(names);
                        consumers_loaded.set(true);
                        return;
                    }
                }
                Err(_) => { /* fall through to keys fallback */ }
            }

            // Fallback: fetch keys and extract consumer names.
            match api::fetch_keys().await {
                Ok(keys) => {
                    let names: Vec<String> = keys
                        .into_iter()
                        .map(|k| k.name)
                        .filter(|n| !n.is_empty())
                        .collect();
                    if !names.is_empty() {
                        if selected_consumer.get_untracked().is_none() {
                            if let Some(first) = names.first() {
                                selected_consumer.set(Some(first.clone()));
                            }
                        }
                        consumers.set(names);
                    }
                }
                Err(e) => consumers_error.set(Some(e)),
            }
            consumers_loaded.set(true);
        });
    };

    let load_live = {
        let consumers = consumers;
        move || {
            let Some(consumer) = selected_consumer.get() else {
                live_data.set(None);
                return;
            };
            load_generation.update(|g| *g += 1);
            let request_id = load_generation.get();
            let window = window_secs.get();
            leptos::task::spawn_local(async move {
                match api::fetch_live_metrics(&consumer, window).await {
                    Ok(data) => {
                        if load_generation.get() != request_id {
                            return;
                        }
                        if !data.available_consumers.is_empty() {
                            consumers.set(data.available_consumers.clone());
                            if selected_consumer.get_untracked().is_none() {
                                if let Some(first) = data.available_consumers.first() {
                                    selected_consumer.set(Some(first.clone()));
                                }
                            }
                        }
                        live_data.set(Some(Ok(data)));
                        last_update.set(chrono::Local::now().format("%H:%M:%S").to_string());
                    }
                    Err(e) => {
                        if load_generation.get() == request_id {
                            live_data.set(Some(Err(e)));
                        }
                    }
                }
            });
        }
    };

    load_consumers();

    Effect::new({
        let load_live = load_live;
        move |_| {
            let _ = selected_consumer.get();
            let _ = window_secs.get();
            load_live();
        }
    });

    leptos::task::spawn_local({
        let load_live = load_live;
        async move {
            loop {
                let interval = poll_interval_ms(window_secs.get_untracked());
                TimeoutFuture::new(interval).await;
                if auto_refresh.get()
                    && selected_consumer.get_untracked().is_some()
                    && page_visible()
                {
                    load_live();
                }
            }
        }
    });

    view! {
        <div class="page-content space-y-6">
            <PageHeader
                title=move || t.live_title()
                description=move || t.live_desc()
            >
                <div class="flex items-center gap-3 flex-wrap justify-end">
                    <span class="text-xs text-theme-muted">
                        {move || format!("{}: {}", t.overview_last_update(), last_update.get())}
                    </span>
                    <label class="flex items-center gap-2 text-xs text-theme-secondary">
                        <input
                            type="checkbox"
                            prop:checked=move || auto_refresh.get()
                            on:change=move |ev| auto_refresh.set(event_target_checked(&ev))
                            class="rounded"
                        />
                        {move || {
                            let ms = poll_interval_ms(window_secs.get());
                            format!("{} ({}s)", t.live_auto_refresh(), ms / 1000)
                        }}
                    </label>
                    <button on:click=move |_| load_live() class="btn btn-secondary text-xs">
                        {t.overview_refresh()}
                    </button>
                </div>
            </PageHeader>

            <div class="glass-card p-4 flex flex-wrap items-end gap-4">
                <div class="flex flex-col gap-1 min-w-[12rem]">
                    <label class="text-xs text-theme-secondary">{t.live_consumer_label()}</label>
                    <select
                        class="input text-sm"
                        prop:value=move || selected_consumer.get().unwrap_or_default()
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            if v.is_empty() {
                                selected_consumer.set(None);
                            } else {
                                selected_consumer.set(Some(v));
                            }
                        }
                    >
                        <option value="">{t.live_select_consumer()}</option>
                        {move || consumers.get().into_iter().map(|name| {
                            let label = name.clone();
                            view! { <option value=label.clone()>{label.clone()}</option> }
                        }).collect_view()}
                    </select>
                </div>
                <div class="flex gap-2">
                    <button
                        class=move || if window_secs.get() == 300 { "btn btn-primary text-xs" } else { "btn btn-secondary text-xs" }
                        on:click=move |_| window_secs.set(300)
                    >
                        {t.live_window_5m()}
                    </button>
                    <button
                        class=move || if window_secs.get() == 900 { "btn btn-primary text-xs" } else { "btn btn-secondary text-xs" }
                        on:click=move |_| window_secs.set(900)
                    >
                        {t.live_window_15m()}
                    </button>
                </div>
            </div>

            {move || {
                if !consumers_loaded.get() {
                    return view! {
                        <div class="glass-card p-8 flex justify-center">
                            <Spinner />
                        </div>
                    }.into_any();
                }
                if let Some(err) = consumers_error.get() {
                    return view! {
                        <div class="glass-card p-6 text-error text-sm space-y-1">
                            <p>{t.live_keys_load_error()}</p>
                            <p class="text-theme-muted text-xs">{err}</p>
                        </div>
                    }.into_any();
                }
                if selected_consumer.get().is_none() {
                    return view! {
                        <div class="glass-card p-6 text-sm text-theme-muted">
                            {t.live_pick_consumer_hint()}
                        </div>
                    }.into_any();
                }
                match live_data.get() {
                None => view! {
                    <div class="glass-card p-8 flex justify-center">
                        <Spinner />
                    </div>
                }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card p-6 text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(data)) => {
                    if !data.trace_available {
                        return view! {
                            <div class="glass-card p-6 text-sm text-theme-muted">
                                {t.live_trace_unavailable()}
                            </div>
                        }.into_any();
                    }
                    if data.summary.request_count == 0 {
                        return view! {
                            <div class="glass-card p-6 text-sm text-theme-muted">
                                {t.live_no_data()}
                            </div>
                        }.into_any();
                    }
                    let chart_buckets = compress_chart_buckets(&data.buckets);
                    view! {
                        <LiveSummaryCards data=data.clone() />
                        <LiveLatencyCharts buckets=chart_buckets.clone() />
                        <LiveTokenChart buckets=chart_buckets />
                        <LiveLatestCard data=data />
                    }.into_any()
                }
                }
            }}
        </div>
    }
}

#[component]
fn LiveSummaryCards(data: LiveMetricsResponse) -> impl IntoView {
    let t = use_translations();
    let s = data.summary;
    let qps = if data.window_secs > 0 {
        s.request_count as f64 / f64::from(data.window_secs)
    } else {
        0.0
    };
    let upstream_display = if s.avg_upstream_latency_ms > 0.0 {
        format!("{:.0} ms", s.avg_upstream_latency_ms)
    } else {
        t.live_upstream_na().to_string()
    };
    let ttft_display = if s.avg_ttft_ms > 0.0 {
        format!("{:.0} ms", s.avg_ttft_ms)
    } else {
        t.live_upstream_na().to_string()
    };
    view! {
        <div class="grid grid-cols-2 md:grid-cols-6 gap-4">
            <div class="metric-card">
                <div class="metric-card-label">{t.live_requests()}</div>
                <div class="metric-card-value">{s.request_count.to_string()}</div>
            </div>
            <div class="metric-card">
                <div class="metric-card-label">{t.live_qps()}</div>
                <div class="metric-card-value">{format!("{:.2}", qps)}</div>
            </div>
            <div class="metric-card">
                <div class="metric-card-label">{t.live_avg_e2e()}</div>
                <div class="metric-card-value">{format!("{:.0} ms", s.avg_e2e_latency_ms)}</div>
            </div>
            <div class="metric-card">
                <div class="metric-card-label">{t.live_avg_upstream()}</div>
                <div class="metric-card-value">{upstream_display}</div>
            </div>
            <div class="metric-card">
                <div class="metric-card-label">{t.live_avg_ttft()}</div>
                <div class="metric-card-value">{ttft_display}</div>
            </div>
            <div class="metric-card">
                <div class="metric-card-label">{t.live_tokens_total()}</div>
                <div class="metric-card-value">
                    {format!("{} / {}", format_number(s.input_tokens), format_number(s.output_tokens))}
                </div>
                <div class="metric-card-sub">{t.live_tokens_in_out()}</div>
            </div>
        </div>
    }
}

#[component]
fn LiveLatencyCharts(buckets: Vec<LiveMetricsBucket>) -> impl IntoView {
    let t = use_translations();
    let stored = StoredValue::new(buckets);
    let e2e_label = t.live_series_e2e().to_string();
    let upstream_label = t.live_series_upstream().to_string();
    let ttft_label = t.live_series_ttft().to_string();

    let x_labels = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|b| format_bucket_time(b.timestamp_ms))
            .collect()
    });
    let series = Signal::derive(move || {
        let b = stored.get_value();
        vec![
            ChartSeries {
                label: e2e_label.clone(),
                color: "var(--accent-primary)",
                values: b
                    .iter()
                    .map(|x| {
                        if x.request_count > 0 {
                            Some(x.e2e_latency_ms)
                        } else {
                            None
                        }
                    })
                    .collect(),
                dashed: false,
            },
            ChartSeries {
                label: upstream_label.clone(),
                color: "var(--warning)",
                values: b.iter().map(|x| x.upstream_latency_ms).collect(),
                dashed: true,
            },
            ChartSeries {
                label: ttft_label.clone(),
                color: "var(--info)",
                values: b.iter().map(|x| x.ttft_ms).collect(),
                dashed: false,
            },
        ]
    });
    view! {
        <div class="glass-card p-4 space-y-3">
            <h3 class="text-sm font-semibold text-theme">{t.live_latency_chart()}</h3>
            <p class="text-xs text-theme-muted">{t.live_upstream_hint()}</p>
            <LineChart
                x_labels=x_labels
                series=series
                y_unit="ms"
                empty_message=t.live_no_data()
            />
        </div>
    }
}

#[component]
fn LiveTokenChart(buckets: Vec<LiveMetricsBucket>) -> impl IntoView {
    let t = use_translations();
    let stored = StoredValue::new(buckets);
    let x_labels = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|b| format_bucket_time(b.timestamp_ms))
            .collect()
    });
    let input_values = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|b| {
                if b.input_tokens > 0 {
                    Some(b.input_tokens as f64)
                } else {
                    None
                }
            })
            .collect()
    });
    let output_values = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|b| {
                if b.output_tokens > 0 {
                    Some(b.output_tokens as f64)
                } else {
                    None
                }
            })
            .collect()
    });
    view! {
        <div class="glass-card p-4 space-y-3">
            <h3 class="text-sm font-semibold text-theme">{t.live_token_chart()}</h3>
            <TokenLineChart
                x_labels=x_labels
                input_values=input_values
                output_values=output_values
                input_label=t.live_tokens_input().to_string()
                output_label=t.live_tokens_output().to_string()
                empty_message=t.live_no_data()
            />
        </div>
    }
}

#[component]
fn LiveLatestCard(data: LiveMetricsResponse) -> impl IntoView {
    let t = use_translations();
    match data.latest {
        None => view! { <></> }.into_any(),
        Some(latest) => view! {
            <div class="glass-card p-4">
                <h3 class="text-sm font-semibold text-theme mb-3">{t.live_latest_request()}</h3>
                <dl class="grid grid-cols-2 md:grid-cols-4 gap-3 text-xs">
                    <div>
                        <dt class="text-theme-muted">{t.live_latest_model()}</dt>
                        <dd class="font-mono text-theme">{latest.model.clone()}</dd>
                    </div>
                    <div>
                        <dt class="text-theme-muted">{t.live_latest_cache()}</dt>
                        <dd class="font-mono text-theme">{latest.cache_status.clone()}</dd>
                    </div>
                    <div>
                        <dt class="text-theme-muted">{t.live_series_e2e()}</dt>
                        <dd class="font-mono text-theme">{format!("{:.0} ms", latest.e2e_latency_ms)}</dd>
                    </div>
                    <div>
                        <dt class="text-theme-muted">{t.live_series_upstream()}</dt>
                        <dd class="font-mono text-theme">
                            {latest.upstream_latency_ms
                                .map(|v| format!("{v:.0} ms"))
                                .unwrap_or_else(|| t.live_upstream_na().to_string())}
                        </dd>
                    </div>
                    <div>
                        <dt class="text-theme-muted">{t.live_series_ttft()}</dt>
                        <dd class="font-mono text-theme">
                            {latest.ttft_ms
                                .map(|v| format!("{v:.0} ms"))
                                .unwrap_or_else(|| t.live_upstream_na().to_string())}
                        </dd>
                    </div>
                    <div>
                        <dt class="text-theme-muted">{t.live_tokens_input()}</dt>
                        <dd class="font-mono text-theme">{format_number(latest.input_tokens)}</dd>
                    </div>
                    <div>
                        <dt class="text-theme-muted">{t.live_tokens_output()}</dt>
                        <dd class="font-mono text-theme">{format_number(latest.output_tokens)}</dd>
                    </div>
                </dl>
            </div>
        }.into_any(),
    }
}
