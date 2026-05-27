use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

use crate::api;
use crate::components::bar_chart::BarChart;
use crate::components::line_chart::ChartSeries;
use crate::components::page_header::PageHeader;
use crate::components::skeleton::SkeletonLive;
use crate::locale::{Translations, use_translations};
use crate::page_visible::page_visible;
use crate::pages::overview::format_number;
use crate::types::{
    KeyRoutingResponse, LiveMetricsBucket, LiveMetricsResponse, ProfileRoutingView,
};
use crate::view_state;

const MAX_CHART_POINTS: usize = 240;

const WINDOW_OPTIONS: &[(u32, fn(Translations) -> &'static str)] = &[
    (60, |t| t.live_window_1m()),
    (300, |t| t.live_window_5m()),
    (600, |t| t.live_window_10m()),
    (900, |t| t.live_window_15m()),
    (1800, |t| t.live_window_30m()),
    (12 * 3600, |t| t.live_window_12h()),
    (24 * 3600, |t| t.live_window_1d()),
    (3 * 24 * 3600, |t| t.live_window_3d()),
    (7 * 24 * 3600, |t| t.live_window_7d()),
    (15 * 24 * 3600, |t| t.live_window_15d()),
    (30 * 24 * 3600, |t| t.live_window_30d()),
];

fn format_bucket_time(ts_ms: u64) -> String {
    crate::datetime::format_ms_china_time(ts_ms, true)
}

fn poll_interval_ms(window_secs: u32) -> u32 {
    if window_secs >= 7 * 24 * 3600 {
        60000
    } else if window_secs >= 24 * 3600 {
        30000
    } else if window_secs >= 3600 {
        15000
    } else if window_secs >= 1800 {
        5000
    } else if window_secs >= 900 {
        3000
    } else {
        2000
    }
}

fn compress_chart_buckets(buckets: &[LiveMetricsBucket]) -> Vec<LiveMetricsBucket> {
    if buckets.len() <= MAX_CHART_POINTS {
        return buckets.to_vec();
    }
    let step = (buckets.len() as f64 / MAX_CHART_POINTS as f64).ceil() as usize;
    let mut sampled: Vec<LiveMetricsBucket> =
        buckets.iter().step_by(step.max(1)).cloned().collect();
    if let Some(last) = buckets.last().cloned()
        && sampled
            .last()
            .map(|x| x.timestamp_ms != last.timestamp_ms)
            .unwrap_or(true)
    {
        sampled.push(last);
    }
    sampled
}

fn window_label(window_secs: u32, t: Translations) -> &'static str {
    WINDOW_OPTIONS
        .iter()
        .find(|(s, _)| *s == window_secs)
        .map(|(_, label)| label(t))
        .unwrap_or("—")
}

fn cache_hit_pct(data: &LiveMetricsResponse) -> f64 {
    if data.summary.cache_hit_ratio > 0.0 {
        return data.summary.cache_hit_ratio * 100.0;
    }
    let hits: u32 = data.buckets.iter().map(|b| b.cache_hit_count).sum();
    let total: u32 = data.buckets.iter().map(|b| b.request_count).sum();
    if total > 0 {
        hits as f64 / total as f64 * 100.0
    } else {
        0.0
    }
}

fn build_backend_distribution(r: &KeyRoutingResponse) -> (Vec<String>, Vec<Option<f64>>) {
    let labels: Vec<String> = r
        .backends
        .iter()
        .map(|b| {
            let kind = b.affinity_kind.as_deref().unwrap_or("n/a");
            format!("{} · {}", b.backend_name, kind)
        })
        .collect();
    let values: Vec<Option<f64>> = r
        .backends
        .iter()
        .map(|b| Some(b.request_count as f64))
        .collect();
    (labels, values)
}

fn build_affinity_distribution(r: &KeyRoutingResponse) -> (Vec<String>, Vec<Option<f64>>) {
    let mut map: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for b in &r.backends {
        let kind = b.affinity_kind.clone().unwrap_or_else(|| "n/a".to_string());
        *map.entry(kind).or_insert(0) += b.request_count;
    }
    let labels: Vec<String> = map.keys().cloned().collect();
    let values: Vec<Option<f64>> = map.values().map(|v| Some(*v as f64)).collect();
    (labels, values)
}

#[derive(Clone, Copy)]
enum LiveTileVariant {
    Accent,
    Teal,
    Orange,
    Green,
    Muted,
    Warn,
}

fn tile_class(v: LiveTileVariant) -> &'static str {
    match v {
        LiveTileVariant::Accent => "live-stat-tile live-stat-tile-accent",
        LiveTileVariant::Teal => "live-stat-tile live-stat-tile-teal",
        LiveTileVariant::Orange => "live-stat-tile live-stat-tile-orange",
        LiveTileVariant::Green => "live-stat-tile live-stat-tile-green",
        LiveTileVariant::Muted => "live-stat-tile live-stat-tile-muted",
        LiveTileVariant::Warn => "live-stat-tile live-stat-tile-warn",
    }
}

#[component]
pub fn LivePage() -> impl IntoView {
    let t = use_translations();
    let vs = view_state::load_view_state();
    let consumers: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let selected_consumer: RwSignal<Option<String>> = RwSignal::new(vs.live_consumer.clone());
    let window_secs: RwSignal<u32> = RwSignal::new(vs.live_window_secs.unwrap_or(12 * 3600));
    let live_data: RwSignal<Option<Result<LiveMetricsResponse, String>>> = RwSignal::new(None);
    let consumers_loaded = RwSignal::new(false);
    let consumers_error = RwSignal::new(None::<String>);
    let auto_refresh = RwSignal::new(true);
    let last_update = RwSignal::new(String::new());
    let load_generation = RwSignal::new(0u64);
    let routing_profiles: RwSignal<Option<Result<Vec<ProfileRoutingView>, String>>> =
        RwSignal::new(None);
    let routing_key_ids: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let selected_routing_key: RwSignal<Option<String>> = RwSignal::new(None);
    let routing_key_data: RwSignal<Option<Result<KeyRoutingResponse, String>>> =
        RwSignal::new(None);

    Effect::new(move |_| {
        let consumer = selected_consumer.get();
        let window = window_secs.get();
        let mut state = view_state::load_view_state();
        state.live_consumer = consumer;
        state.live_window_secs = Some(window);
        view_state::save_view_state(&state);
    });

    let load_consumers = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_live_consumers(window_secs.get_untracked()).await {
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
                        if selected_consumer.get_untracked().is_none()
                            && let Some(first) = names.first()
                        {
                            selected_consumer.set(Some(first.clone()));
                        }
                        consumers.set(names);
                        consumers_loaded.set(true);
                        return;
                    }
                }
                Err(_) => {}
            }
            match api::fetch_keys().await {
                Ok(keys) => {
                    let names: Vec<String> = keys
                        .into_iter()
                        .map(|k| k.name)
                        .filter(|n| !n.is_empty())
                        .collect();
                    if !names.is_empty() {
                        if selected_consumer.get_untracked().is_none()
                            && let Some(first) = names.first()
                        {
                            selected_consumer.set(Some(first.clone()));
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
                            if selected_consumer.get_untracked().is_none()
                                && let Some(first) = data.available_consumers.first()
                            {
                                selected_consumer.set(Some(first.clone()));
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

    let load_routing = move || {
        leptos::task::spawn_local(async move {
            routing_profiles.set(Some(api::fetch_routing_profiles().await));
            match api::fetch_keys().await {
                Ok(keys) => {
                    let ids: Vec<String> = keys
                        .into_iter()
                        .map(|k| k.id)
                        .filter(|id| !id.is_empty())
                        .collect();
                    if !ids.is_empty() {
                        let chosen = selected_routing_key
                            .get_untracked()
                            .filter(|id| ids.iter().any(|x| x == id))
                            .unwrap_or_else(|| ids[0].clone());
                        selected_routing_key.set(Some(chosen.clone()));
                        routing_key_ids.set(ids);
                        routing_key_data.set(Some(api::fetch_key_routing(&chosen).await));
                    } else {
                        routing_key_ids.set(Vec::new());
                        selected_routing_key.set(None);
                        routing_key_data.set(None);
                    }
                }
                Err(e) => {
                    routing_key_ids.set(Vec::new());
                    selected_routing_key.set(None);
                    routing_key_data.set(Some(Err(e)));
                }
            }
        });
    };

    load_consumers();
    load_routing();

    Effect::new({
        let load_live = load_live;
        move |_| {
            let _ = selected_consumer.get();
            let _ = window_secs.get();
            load_live();
        }
    });

    Effect::new(move |_| {
        let Some(key_id) = selected_routing_key.get() else {
            return;
        };
        leptos::task::spawn_local(async move {
            routing_key_data.set(Some(api::fetch_key_routing(&key_id).await));
        });
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
                    load_routing();
                }
            }
        }
    });

    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::*;
        let load_live = load_live;
        let vis_cb = Closure::wrap(Box::new(move || {
            if !web_sys::window().unwrap().document().unwrap().hidden()
                && auto_refresh.get_untracked()
                && selected_consumer.get_untracked().is_some()
            {
                load_live();
            }
        }) as Box<dyn FnMut()>);
        web_sys::window()
            .unwrap()
            .add_event_listener_with_callback("visibilitychange", vis_cb.as_ref().unchecked_ref())
            .ok();
        vis_cb.forget();
    }

    view! {
        <div class="page-content space-y-5">
            <PageHeader
                title=move || t.live_title()
                description=move || t.live_desc()
            >
                <button on:click=move |_| load_live() class="btn btn-secondary text-xs">
                    {move || t.overview_refresh()}
                </button>
            </PageHeader>

            <LiveTopConfigRow
                consumers=consumers
                selected_consumer=selected_consumer
                window_secs=window_secs
                auto_refresh=auto_refresh
                last_update=last_update
            />

            {move || {
                if !consumers_loaded.get() {
                    return view! { <SkeletonLive /> }.into_any();
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
                    None => view! { <SkeletonLive /> }.into_any(),
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
                        let chart_buckets = compress_chart_buckets(&data.buckets);
                        let consumer_name = data.consumer.clone();
                        let window = data.window_secs;
                        view! {
                            <LiveTrafficPanel
                                data=data.clone()
                                buckets=chart_buckets.clone()
                                consumer=consumer_name
                                window_secs=window
                            />
                            <LiveBottomRow
                                data=data
                                buckets=chart_buckets
                                routing_profiles=routing_profiles
                                routing_key_ids=routing_key_ids
                                selected_routing_key=selected_routing_key
                                routing_key_data=routing_key_data
                            />
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}

#[component]
fn LiveTopConfigRow(
    consumers: RwSignal<Vec<String>>,
    selected_consumer: RwSignal<Option<String>>,
    window_secs: RwSignal<u32>,
    auto_refresh: RwSignal<bool>,
    last_update: RwSignal<String>,
) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
            <div class="live-config-card">
                <div class="live-config-card-title">{t.live_config_consumer()}</div>
                <select
                    class="input text-sm font-mono"
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

            <div class="live-config-card md:col-span-1">
                <div class="live-config-card-title">{t.live_config_window()}</div>
                <LiveWindowPills window_secs=window_secs />
            </div>

            <div class="live-config-card">
                <div class="live-config-card-title">{t.live_config_refresh()}</div>
                <div class="flex flex-col gap-2">
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
                    <span class="text-xs text-theme-muted font-mono">
                        {move || format!("{} {}", t.overview_last_update(), last_update.get())}
                    </span>
                </div>
            </div>
        </div>
    }
}

#[component]
fn LiveWindowPills(window_secs: RwSignal<u32>) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="live-window-pills">
            {WINDOW_OPTIONS.iter().map(|(secs, label_fn)| {
                let secs = *secs;
                let label = label_fn(t);
                view! {
                    <button
                        type="button"
                        class=move || {
                            if window_secs.get() == secs {
                                "live-pill live-pill-active"
                            } else {
                                "live-pill"
                            }
                        }
                        on:click=move |_| window_secs.set(secs)
                    >
                        {label}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}

#[component]
fn LiveStatTile(label: &'static str, value: String, variant: LiveTileVariant) -> impl IntoView {
    view! {
        <div class=tile_class(variant)>
            <span class="live-stat-tile-label">{label}</span>
            <span class="live-stat-tile-value">{value}</span>
        </div>
    }
}

#[component]
fn LiveTrafficPanel(
    data: LiveMetricsResponse,
    buckets: Vec<LiveMetricsBucket>,
    consumer: String,
    window_secs: u32,
) -> impl IntoView {
    let t = use_translations();
    let s = data.summary.clone();
    let qps = if window_secs > 0 {
        s.request_count as f64 / f64::from(window_secs)
    } else {
        0.0
    };
    let hit_pct = cache_hit_pct(&data);
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
    let window_lbl = window_label(window_secs, t).to_string();
    let stored = StoredValue::new(buckets);
    let throughput_label = t.live_chart_throughput().to_string();
    let hit_label = t.live_cache_hit_trend().to_string();

    let x_labels = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|b| format_bucket_time(b.timestamp_ms))
            .collect()
    });
    let throughput_series = Signal::derive(move || {
        let b = stored.get_value();
        vec![ChartSeries {
            label: throughput_label.clone(),
            color: "var(--cc-accent)",
            values: b.iter().map(|x| Some(x.request_count as f64)).collect(),
            dashed: false,
            fill: false,
        }]
    });
    let hit_series = Signal::derive(move || {
        let b = stored.get_value();
        vec![ChartSeries {
            label: hit_label.clone(),
            color: "var(--cc-success)",
            values: b
                .iter()
                .map(|x| {
                    if x.request_count > 0 {
                        Some(x.cache_hit_count as f64 / x.request_count as f64 * 100.0)
                    } else {
                        None
                    }
                })
                .collect(),
            dashed: false,
            fill: false,
        }]
    });

    view! {
        <div class="glass-card-flush live-traffic-panel">
            <div class="panel-header">
                <span class="flex items-center">
                    <span class="live-pulse-dot"></span>
                    {t.live_traffic_stats()}
                </span>
                <span class="panel-header-meta">
                    {consumer.clone()}
                    " · "
                    {window_lbl}
                </span>
            </div>
            <div class="p-4">
                <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_11.5rem] gap-4">
                    <div class="space-y-2 min-w-0">
                        <h4 class="text-xs font-semibold text-theme-muted uppercase tracking-wide">
                            {t.live_chart_throughput()}
                        </h4>
                        <BarChart
                            x_labels=x_labels
                            series=throughput_series
                            height_px=160
                            y_unit="req"
                            empty_message=t.live_no_data()
                        />
                    </div>
                    <div class="space-y-2 min-w-0">
                        <h4 class="text-xs font-semibold text-theme-muted uppercase tracking-wide">
                            {t.live_cache_hit_trend()}
                        </h4>
                        <BarChart
                            x_labels=x_labels
                            series=hit_series
                            height_px=160
                            y_unit="%"
                            empty_message=t.live_no_data()
                        />
                    </div>
                    <div class="grid grid-cols-2 gap-2">
                        <LiveStatTile
                            label=t.live_qps()
                            value=format!("{:.2}", qps)
                            variant=LiveTileVariant::Accent
                        />
                        <LiveStatTile
                            label=t.live_requests()
                            value=s.request_count.to_string()
                            variant=LiveTileVariant::Teal
                        />
                        <LiveStatTile
                            label=t.live_cache_hit_pct()
                            value=format!("{hit_pct:.1}%")
                            variant=LiveTileVariant::Green
                        />
                        <LiveStatTile
                            label=t.live_avg_e2e()
                            value=format!("{:.0} ms", s.avg_e2e_latency_ms)
                            variant=LiveTileVariant::Orange
                        />
                        <LiveStatTile
                            label=t.live_avg_upstream()
                            value=upstream_display
                            variant=LiveTileVariant::Muted
                        />
                        <LiveStatTile
                            label=t.live_avg_ttft()
                            value=ttft_display
                            variant=LiveTileVariant::Warn
                        />
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn LiveBottomRow(
    data: LiveMetricsResponse,
    buckets: Vec<LiveMetricsBucket>,
    routing_profiles: RwSignal<Option<Result<Vec<ProfileRoutingView>, String>>>,
    routing_key_ids: RwSignal<Vec<String>>,
    selected_routing_key: RwSignal<Option<String>>,
    routing_key_data: RwSignal<Option<Result<KeyRoutingResponse, String>>>,
) -> impl IntoView {
    view! {
        <div class="grid grid-cols-1 lg:grid-cols-3 gap-4">
            <LiveRoutingPanel
                profiles=routing_profiles
                routing_key_ids=routing_key_ids
                selected_routing_key=selected_routing_key
                routing_key_data=routing_key_data
            />
            <LiveLatencyPanel buckets=buckets.clone() />
            <LiveLatestPanel data=data />
        </div>
        <LiveTokenPanel buckets=buckets />
    }
}

#[component]
fn LiveRoutingPanel(
    profiles: RwSignal<Option<Result<Vec<ProfileRoutingView>, String>>>,
    routing_key_ids: RwSignal<Vec<String>>,
    selected_routing_key: RwSignal<Option<String>>,
    routing_key_data: RwSignal<Option<Result<KeyRoutingResponse, String>>>,
) -> impl IntoView {
    let t = use_translations();
    let selected_profile = RwSignal::new(String::new());
    view! {
        <div class="glass-card p-4 h-full flex flex-col">
            <h3 class="text-sm font-semibold text-theme mb-3">{t.live_routing_title()}</h3>
            {move || match profiles.get() {
                None => view! {
                    <p class="text-xs text-theme-muted">{t.live_routing_loading()}</p>
                }.into_any(),
                Some(Err(e)) => view! {
                    <p class="text-xs text-error">{format!("{}: {e}", t.live_routing_error())}</p>
                }.into_any(),
                Some(Ok(items)) => {
                    if items.is_empty() {
                        return view! { <p class="text-xs text-theme-muted">"No routing profiles."</p> }.into_any();
                    }
                    if selected_profile.get().is_empty() {
                        selected_profile.set(items[0].profile_id.clone());
                    }
                    let selected_id = selected_profile.get();
                    let active = items
                        .iter()
                        .find(|p| p.profile_id == selected_id)
                        .cloned()
                        .unwrap_or_else(|| items[0].clone());
                    let backend_total = active.backends.len();
                    let backend_healthy = active.backends.iter().filter(|b| b.healthy).count();
                    let circuit_open = active
                        .backends
                        .iter()
                        .filter(|b| b.circuit_state.eq_ignore_ascii_case("open"))
                        .count();
                    let backend_pct = if backend_total > 0 {
                        backend_healthy as f64 / backend_total as f64 * 100.0
                    } else {
                        0.0
                    };
                    let key_pct = if active.key_pool.total > 0 {
                        active.key_pool.available as f64 / active.key_pool.total as f64 * 100.0
                    } else {
                        0.0
                    };
                    let circuit_pct = if backend_total > 0 {
                        (backend_total.saturating_sub(circuit_open)) as f64
                            / backend_total as f64
                            * 100.0
                    } else {
                        0.0
                    };
                    let status_ok = backend_healthy == backend_total && circuit_open == 0;
                    view! {
                        <div class="space-y-3 flex-1">
                            <div class="flex flex-wrap gap-1">
                                {items.into_iter().map(|p| {
                                    let id = p.profile_id.clone();
                                    let id_for_class = id.clone();
                                    let id_for_click = id.clone();
                                    let id_for_label = id.clone();
                                    view! {
                                        <button
                                            class=move || {
                                                if selected_profile.get() == id_for_class {
                                                    "live-pill live-pill-active"
                                                } else {
                                                    "live-pill"
                                                }
                                            }
                                            on:click=move |_| selected_profile.set(id_for_click.clone())
                                        >
                                            {id_for_label}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                            <RoutingMetricRow
                                label="Backends"
                                value=format!("{}/{}", backend_healthy, backend_total)
                                pct=backend_pct
                            />
                            <RoutingMetricRow
                                label="Upstream keys"
                                value=format!("{}/{}", active.key_pool.available, active.key_pool.total)
                                pct=key_pct
                            />
                            <RoutingMetricRow
                                label="Circuit"
                                value=if circuit_open == 0 {
                                    "closed".to_string()
                                } else {
                                    format!("{} open", circuit_open)
                                }
                                pct=circuit_pct
                            />
                            <div class="space-y-2 pt-2 border-t border-theme">
                                <div class="text-[11px] font-medium text-theme-muted uppercase tracking-wide">
                                    "Key / Affinity Distribution"
                                </div>
                                <select
                                    class="input text-xs font-mono"
                                    prop:value=move || selected_routing_key.get().unwrap_or_default()
                                    on:change=move |ev| {
                                        let v = event_target_value(&ev);
                                        if v.is_empty() {
                                            selected_routing_key.set(None);
                                        } else {
                                            selected_routing_key.set(Some(v));
                                        }
                                    }
                                >
                                    <option value="">"Select key_id"</option>
                                    {move || routing_key_ids.get().into_iter().map(|id| {
                                        let id_val = id.clone();
                                        view! { <option value=id_val.clone()>{id_val.clone()}</option> }
                                    }).collect_view()}
                                </select>
                                {move || match routing_key_data.get() {
                                    None => view! { <p class="text-[11px] text-theme-muted">"Loading key routing…"</p> }.into_any(),
                                    Some(Err(e)) => view! { <p class="text-[11px] text-error">{e}</p> }.into_any(),
                                    Some(Ok(resp)) => {
                                        let (backend_labels, backend_values) = build_backend_distribution(&resp);
                                        let (aff_labels, aff_values) = build_affinity_distribution(&resp);
                                        let backend_labels_sv = StoredValue::new(backend_labels);
                                        let backend_values_sv = StoredValue::new(backend_values);
                                        let aff_labels_sv = StoredValue::new(aff_labels);
                                        let aff_values_sv = StoredValue::new(aff_values);
                                        let backend_x = Signal::derive(move || backend_labels_sv.get_value());
                                        let backend_series = Signal::derive(move || {
                                            vec![ChartSeries {
                                                label: "backend".to_string(),
                                                color: "var(--cc-accent)",
                                                values: backend_values_sv.get_value(),
                                                dashed: false,
                                                fill: false,
                                            }]
                                        });
                                        let aff_x = Signal::derive(move || aff_labels_sv.get_value());
                                        let aff_series = Signal::derive(move || {
                                            vec![ChartSeries {
                                                label: "affinity_kind".to_string(),
                                                color: "var(--cc-info)",
                                                values: aff_values_sv.get_value(),
                                                dashed: false,
                                                fill: false,
                                            }]
                                        });
                                        view! {
                                            <div class="space-y-2">
                                                <div class="text-[11px] text-theme-muted font-mono">
                                                    {format!("key_id={} · prefix_breaks={}", resp.key_id, resp.prefix_break_count)}
                                                </div>
                                                <div class="grid grid-cols-1 gap-2">
                                                    <div class="border border-theme rounded-md p-2">
                                                        <div class="text-[11px] text-theme-muted mb-1">"Backend distribution"</div>
                                                        <BarChart
                                                            x_labels=backend_x
                                                            series=backend_series
                                                            height_px=95
                                                            y_unit="req"
                                                            empty_message=t.live_no_data()
                                                        />
                                                    </div>
                                                    <div class="border border-theme rounded-md p-2">
                                                        <div class="text-[11px] text-theme-muted mb-1">"Affinity kind distribution"</div>
                                                        <BarChart
                                                            x_labels=aff_x
                                                            series=aff_series
                                                            height_px=85
                                                            y_unit="req"
                                                            empty_message=t.live_no_data()
                                                        />
                                                    </div>
                                                </div>
                                            </div>
                                        }.into_any()
                                    }
                                }}
                            </div>
                            <div class="space-y-1.5 pt-1 border-t border-theme">
                                {active.backends.iter().take(4).map(|b| {
                                    let pct = if b.healthy { 100.0 } else { 25.0 };
                                    let state = if b.healthy { "healthy" } else { "unhealthy" };
                                    view! {
                                        <RoutingMetricRow
                                            label="Node"
                                            value=format!("{} {} {}ms", b.name, state, b.latency_ms)
                                            pct=pct
                                        />
                                    }
                                }).collect_view()}
                            </div>
                            <div class="mt-auto pt-2">
                                <span class=if status_ok {
                                    "inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium bg-[color-mix(in_srgb,var(--cc-success)_20%,transparent)] text-[var(--cc-success)]"
                                } else {
                                    "inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium bg-[color-mix(in_srgb,var(--cc-warning)_25%,transparent)] text-[var(--cc-warning)]"
                                }>
                                    {if status_ok { "OK" } else { "DEGRADED" }}
                                </span>
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn RoutingMetricRow(label: &'static str, value: String, pct: f64) -> impl IntoView {
    let width = format!("{:.0}%", pct.clamp(0.0, 100.0));
    view! {
        <div class="live-metric-row">
            <span class="live-metric-row-label">{label}</span>
            <div class="live-metric-row-bar">
                <div class="live-metric-row-fill" style=format!("width: {width}")></div>
            </div>
            <span class="live-metric-row-value">{value}</span>
        </div>
    }
}

#[component]
fn LiveLatencyPanel(buckets: Vec<LiveMetricsBucket>) -> impl IntoView {
    let t = use_translations();
    let stored = StoredValue::new(buckets);
    let e2e_label = t.live_series_e2e().to_string();
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
                color: "var(--cc-accent)",
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
                fill: false,
            },
            ChartSeries {
                label: ttft_label.clone(),
                color: "var(--cc-info)",
                values: b.iter().map(|x| x.ttft_ms).collect(),
                dashed: false,
                fill: false,
            },
        ]
    });
    view! {
        <div class="glass-card p-4 h-full flex flex-col">
            <h3 class="text-sm font-semibold text-theme mb-2">{t.live_nodes_title()}</h3>
            <p class="text-[10px] text-theme-muted mb-2 leading-snug">{t.live_upstream_hint()}</p>
            <div class="flex-1 min-h-0">
                <BarChart
                    x_labels=x_labels
                    series=series
                    height_px=140
                    y_unit="ms"
                    empty_message=t.live_no_data()
                />
            </div>
        </div>
    }
}

#[component]
fn LiveTokenPanel(buckets: Vec<LiveMetricsBucket>) -> impl IntoView {
    let t = use_translations();
    let stored = StoredValue::new(buckets);
    let x_labels = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|b| format_bucket_time(b.timestamp_ms))
            .collect()
    });
    let input_label = t.live_tokens_input().to_string();
    let output_label = t.live_tokens_output().to_string();
    let series = Signal::derive(move || {
        let b = stored.get_value();
        vec![
            ChartSeries {
                label: input_label.clone(),
                color: "var(--cc-accent)",
                values: b.iter().map(|x| Some(x.input_tokens as f64)).collect(),
                dashed: false,
                fill: false,
            },
            ChartSeries {
                label: output_label.clone(),
                color: "var(--cc-info)",
                values: b.iter().map(|x| Some(x.output_tokens as f64)).collect(),
                dashed: false,
                fill: false,
            },
        ]
    });
    view! {
        <div class="glass-card p-4">
            <h3 class="text-sm font-semibold text-theme mb-3">{t.live_token_chart()}</h3>
            <BarChart
                x_labels=x_labels
                series=series
                height_px=180
                y_unit="tok"
                empty_message=t.live_no_data()
            />
        </div>
    }
}

#[component]
fn LiveLatestPanel(data: LiveMetricsResponse) -> impl IntoView {
    let t = use_translations();
    let s = data.summary;
    view! {
        <div class="glass-card p-4 h-full flex flex-col">
            <h3 class="text-sm font-semibold text-theme mb-3">{t.live_latest_request()}</h3>
            {match data.latest {
                None => view! {
                    <p class="text-xs text-theme-muted flex-1">{t.live_no_data()}</p>
                    <div class="grid grid-cols-2 gap-2 mt-2 pt-3 border-t border-theme">
                        <LiveStatTile
                            label=t.live_tokens_input()
                            value=format_number(s.input_tokens)
                            variant=LiveTileVariant::Muted
                        />
                        <LiveStatTile
                            label=t.live_tokens_output()
                            value=format_number(s.output_tokens)
                            variant=LiveTileVariant::Muted
                        />
                    </div>
                }.into_any(),
                Some(latest) => view! {
                    <dl class="space-y-2 text-xs flex-1">
                        <div class="flex justify-between gap-2">
                            <dt class="text-theme-muted">{t.live_latest_model()}</dt>
                            <dd class="font-mono text-theme truncate">{latest.model}</dd>
                        </div>
                        <div class="flex justify-between gap-2">
                            <dt class="text-theme-muted">{t.live_latest_cache()}</dt>
                            <dd class="font-mono text-theme">{latest.cache_status}</dd>
                        </div>
                        <div class="flex justify-between gap-2">
                            <dt class="text-theme-muted">{t.live_series_e2e()}</dt>
                            <dd class="font-mono text-theme">{format!("{:.0} ms", latest.e2e_latency_ms)}</dd>
                        </div>
                        <div class="flex justify-between gap-2">
                            <dt class="text-theme-muted">{t.live_series_ttft()}</dt>
                            <dd class="font-mono text-theme">
                                {latest.ttft_ms
                                    .map(|v| format!("{v:.0} ms"))
                                    .unwrap_or_else(|| t.live_upstream_na().to_string())}
                            </dd>
                        </div>
                    </dl>
                    <div class="grid grid-cols-2 gap-2 mt-2 pt-3 border-t border-theme">
                        <LiveStatTile
                            label=t.live_tokens_input()
                            value=format_number(latest.input_tokens)
                            variant=LiveTileVariant::Accent
                        />
                        <LiveStatTile
                            label=t.live_tokens_output()
                            value=format_number(latest.output_tokens)
                            variant=LiveTileVariant::Teal
                        />
                    </div>
                }.into_any(),
            }}
        </div>
    }
}
