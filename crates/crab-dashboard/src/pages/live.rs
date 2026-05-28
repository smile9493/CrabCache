use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use wasm_bindgen::JsCast;

use crate::api;
use crate::components::bar_chart::BarChart;
use crate::components::chart_preview_card::ChartPreviewCard;
use crate::components::horizontal_bar_chart::HorizontalBarChart;
use crate::components::line_chart::{ChartSeries, TokenLineChart};
use crate::components::page_header::PageHeader;
use crate::components::skeleton::SkeletonLive;
use crate::locale::{Translations, use_translations};
use crate::page_visible::page_visible;
use crate::pages::overview::format_number;
use crate::types::{
    KeyConcurrencyResponse, KeyRoutingResponse, LiveMetricsBucket, LiveMetricsResponse,
    LiveMetricsSummary, ProfileRoutingView,
};
use crate::view_state;

fn now_hms_string() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{:02}:{:02}:{:02}",
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds()
    )
}

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

fn max_bucket_latencies(buckets: &[LiveMetricsBucket]) -> (f64, Option<f64>, Option<f64>) {
    let mut max_e2e = 0.0_f64;
    let mut max_upstream: Option<f64> = None;
    let mut max_ttft: Option<f64> = None;
    for b in buckets {
        if b.request_count > 0 {
            max_e2e = max_e2e.max(b.e2e_latency_ms);
        }
        if let Some(u) = b.upstream_latency_ms {
            max_upstream = Some(max_upstream.map_or(u, |m| m.max(u)));
        }
        if let Some(t) = b.ttft_ms {
            max_ttft = Some(max_ttft.map_or(t, |m| m.max(t)));
        }
    }
    (max_e2e, max_upstream, max_ttft)
}

fn format_latency_opt(v: Option<f64>, na: &str) -> String {
    v.map(|x| format!("{x:.0} ms"))
        .unwrap_or_else(|| na.to_string())
}

fn series_has_points(values: &[Option<f64>]) -> bool {
    values.iter().any(|v| matches!(v, Some(x) if *x > 0.0))
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
    let routing_key_concurrency: RwSignal<Option<Result<KeyConcurrencyResponse, String>>> =
        RwSignal::new(None);
    let alive = Arc::new(AtomicBool::new(true));

    Effect::new(move |_| {
        let consumer = selected_consumer.get();
        let window = window_secs.get();
        let mut state = view_state::load_view_state();
        state.live_consumer = consumer;
        state.live_window_secs = Some(window);
        view_state::save_view_state(&state);
    });

    let alive_for_load_consumers = Arc::clone(&alive);
    let load_consumers = || {
        let alive = Arc::clone(&alive_for_load_consumers);
        leptos::task::spawn_local(async move {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            if let Ok(val) = api::fetch_live_consumers(window_secs.get_untracked()).await {
                if !alive.load(Ordering::Relaxed) {
                    return;
                }
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
            match api::fetch_keys().await {
                Ok(keys) => {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
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

    // rAF buffer for live_data: decouples polling frequency from signal propagation.
    // With 2-60s polling this is primarily for future-proofing; the pattern ensures
    // that increasing poll frequency won't cause cascading signal writes.
    let live_buffer: Arc<Mutex<Option<Result<LiveMetricsResponse, String>>>> =
        Arc::new(Mutex::new(None));
    let live_dirty: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let live_active: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    {
        let live_buffer = Arc::clone(&live_buffer);
        let live_dirty = Arc::clone(&live_dirty);
        let live_active = Arc::clone(&live_active);
        let alive_for_raf = Arc::clone(&alive);
        let live_raf_state: Rc<RefCell<Option<js_sys::Function>>> =
            Rc::new(RefCell::new(None));
        let live_raf_state_inner = live_raf_state.clone();

        let flush = move || {
            if !alive_for_raf.load(Ordering::Relaxed) {
                live_active.store(false, Ordering::Relaxed);
                return;
            }
            if *live_dirty.lock().expect("live_dirty lock poisoned") {
                if let Some(data) = live_buffer.lock().expect("live_buffer lock poisoned").take() {
                    live_data.set(Some(data));
                    last_update.set(now_hms_string());
                }
                *live_dirty.lock().expect("live_dirty lock poisoned") = false;
            }
            if live_active.load(Ordering::Relaxed) {
                let state = live_raf_state_inner.clone();
                let next = wasm_bindgen::closure::Closure::once(move || {
                    if let Some(func) = state.borrow().as_ref() {
                        let _ = web_sys::window().unwrap().request_animation_frame(func);
                    }
                });
                let next_js = next.into_js_value();
                let _ = web_sys::window()
                    .unwrap()
                    .request_animation_frame(next_js.unchecked_ref());
            }
        };
        let closure = wasm_bindgen::closure::Closure::wrap(
            Box::new(flush) as Box<dyn FnMut()>
        );
        let func: js_sys::Function = closure.into_js_value().into();
        *live_raf_state.borrow_mut() = Some(func);

        // Kick off the rAF loop.
        if let Some(func) = live_raf_state.borrow().as_ref() {
            let _ = web_sys::window().unwrap().request_animation_frame(func);
        }
    }

    let live_buffer_for_loader = Arc::clone(&live_buffer);
    let live_dirty_for_loader = Arc::clone(&live_dirty);
    let alive_for_loader = Arc::clone(&alive);
    let load_live_fn: Rc<RefCell<dyn FnMut()>> = {
        let consumers = consumers;
        Rc::new(RefCell::new(move || {
            let alive = Arc::clone(&alive_for_loader);
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            let Some(consumer) = selected_consumer.get() else {
                live_data.set(None);
                return;
            };
            load_generation.update(|g| *g += 1);
            let request_id = load_generation.get();
            let window = window_secs.get();
            let buf = Arc::clone(&live_buffer_for_loader);
            let dirty = Arc::clone(&live_dirty_for_loader);
            let alive = Arc::clone(&alive);
            leptos::task::spawn_local(async move {
                match api::fetch_live_metrics(&consumer, window).await {
                    Ok(data) => {
                        if !alive.load(Ordering::Relaxed) {
                            return;
                        }
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
                        buf.lock().expect("live_buffer lock poisoned").replace(Ok(data));
                        *dirty.lock().expect("live_dirty lock poisoned") = true;
                    }
                    Err(e) => {
                        if !alive.load(Ordering::Relaxed) {
                            return;
                        }
                        if load_generation.get() == request_id {
                            live_data.set(Some(Err(e)));
                        }
                    }
                }
            });
        }))
    };

    let alive_for_routing = Arc::clone(&alive);
    let routing_profiles_for_routing = routing_profiles;
    let routing_key_ids_for_routing = routing_key_ids;
    let selected_routing_key_for_routing = selected_routing_key;
    let routing_key_data_for_routing = routing_key_data;
    let routing_key_concurrency_for_routing = routing_key_concurrency;
    let load_routing = move || {
        let alive = Arc::clone(&alive_for_routing);
        leptos::task::spawn_local(async move {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            routing_profiles_for_routing.set(Some(api::fetch_routing_profiles().await));
            match api::fetch_keys().await {
                Ok(keys) => {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    let ids: Vec<String> = keys
                        .into_iter()
                        .map(|k| k.id)
                        .filter(|id| !id.is_empty())
                        .collect();
                    if !ids.is_empty() {
                        let chosen = selected_routing_key_for_routing
                            .get_untracked()
                            .filter(|id| ids.iter().any(|x| x == id))
                            .unwrap_or_else(|| ids[0].clone());
                        selected_routing_key_for_routing.set(Some(chosen.clone()));
                        routing_key_ids_for_routing.set(ids);
                        let routing = api::fetch_key_routing(&chosen).await;
                        let concurrency = api::fetch_key_concurrency(&chosen).await;
                        routing_key_data_for_routing.set(Some(routing));
                        routing_key_concurrency_for_routing.set(Some(concurrency));
                    } else {
                        routing_key_ids_for_routing.set(Vec::new());
                        selected_routing_key_for_routing.set(None);
                        routing_key_data_for_routing.set(None);
                        routing_key_concurrency_for_routing.set(None);
                    }
                }
                Err(e) => {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    routing_key_ids_for_routing.set(Vec::new());
                    selected_routing_key_for_routing.set(None);
                    routing_key_data_for_routing.set(Some(Err(e)));
                    routing_key_concurrency_for_routing.set(None);
                }
            }
        });
    };

    load_consumers();
    load_routing();

    Effect::new({
        let ll = load_live_fn.clone();
        move |_| {
            let _ = selected_consumer.get();
            let _ = window_secs.get();
            ll.borrow_mut()();
        }
    });

    let alive_for_key_effect = Arc::clone(&alive);
    Effect::new(move |_| {
        if !alive_for_key_effect.load(Ordering::Relaxed) {
            return;
        }
        let Some(key_id) = selected_routing_key.get() else {
            routing_key_concurrency.set(None);
            return;
        };
        let alive = Arc::clone(&alive_for_key_effect);
        leptos::task::spawn_local(async move {
            let (routing, concurrency) = futures::join!(
                api::fetch_key_routing(&key_id),
                api::fetch_key_concurrency(&key_id),
            );
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            routing_key_data.set(Some(routing));
            routing_key_concurrency.set(Some(concurrency));
        });
    });

    let alive_poll = Arc::clone(&alive);
    leptos::task::spawn_local({
        let ll = load_live_fn.clone();
        async move {
            loop {
                let interval = poll_interval_ms(window_secs.get_untracked());
                TimeoutFuture::new(interval).await;
                if !alive_poll.load(Ordering::Relaxed) {
                    break;
                }
                if auto_refresh.get()
                    && selected_consumer.get_untracked().is_some()
                    && page_visible()
                {
                    ll.borrow_mut()();
                    load_routing();
                }
            }
        }
    });

    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::*;
        let ll = load_live_fn.clone();
        let alive_vis = Arc::clone(&alive);
        let vis_cb = Closure::wrap(Box::new(move || {
            if !alive_vis.load(Ordering::Relaxed) {
                return;
            }
            if !web_sys::window().unwrap().document().unwrap().hidden()
                && auto_refresh.get_untracked()
                && selected_consumer.get_untracked().is_some()
            {
                ll.borrow_mut()();
            }
        }) as Box<dyn FnMut()>);
        web_sys::window()
            .unwrap()
            .add_event_listener_with_callback("visibilitychange", vis_cb.as_ref().unchecked_ref())
            .ok();
        vis_cb.forget();
    }

    let alive_cleanup = Arc::clone(&alive);
    on_cleanup(move || {
        alive_cleanup.store(false, Ordering::Relaxed);
        live_active.store(false, Ordering::Relaxed);
    });

    view! {
        <div class="page-content space-y-5">
            <PageHeader
                title=move || t.live_title()
                description=move || t.live_desc()
            >
                <button on:click=move |_| {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some(consumer) = selected_consumer.get() else {
                        live_data.set(None);
                        return;
                    };
                    load_generation.update(|g| *g += 1);
                    let request_id = load_generation.get();
                    let window = window_secs.get();
                    let buf = Arc::clone(&live_buffer);
                    let dirty = Arc::clone(&live_dirty);
                    let consumers = consumers;
                    let alive = Arc::clone(&alive);
                    leptos::task::spawn_local(async move {
                        match api::fetch_live_metrics(&consumer, window).await {
                            Ok(data) => {
                                if !alive.load(Ordering::Relaxed) || load_generation.get() != request_id {
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
                                buf.lock().expect("live_buffer lock poisoned").replace(Ok(data));
                                *dirty.lock().expect("live_dirty lock poisoned") = true;
                            }
                            Err(e) => {
                                if !alive.load(Ordering::Relaxed) {
                                    return;
                                }
                                if load_generation.get() == request_id {
                                    live_data.set(Some(Err(e)));
                                }
                            }
                        }
                    });
                } class="btn btn-secondary text-xs">
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
                                routing_key_concurrency=routing_key_concurrency
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
    let buckets = std::sync::Arc::new(buckets);
    let throughput_label = t.live_chart_throughput().to_string();
    let hit_label = t.live_cache_hit_trend().to_string();

    let x_labels = {
        let buckets = std::sync::Arc::clone(&buckets);
        Signal::derive(move || {
            buckets
                .iter()
            .map(|b| format_bucket_time(b.timestamp_ms))
            .collect()
        })
    };
    let throughput_series = {
        let buckets = std::sync::Arc::clone(&buckets);
        Signal::derive(move || {
            let b = buckets.as_ref();
            vec![ChartSeries {
                label: throughput_label.clone(),
                color: "var(--cc-accent)",
                values: b.iter().map(|x| Some(x.request_count as f64)).collect(),
                dashed: false,
                fill: false,
            }]
        })
    };
    let hit_series = {
        let buckets = std::sync::Arc::clone(&buckets);
        Signal::derive(move || {
            let b = buckets.as_ref();
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
        })
    };

    let chart_subtitle = format!("{consumer} · {window_lbl}");
    let throughput_open = RwSignal::new(false);
    let hit_open = RwSignal::new(false);
    let throughput_title = t.live_chart_throughput().to_string();
    let hit_title = t.live_cache_hit_trend().to_string();
    let no_data = t.live_no_data();

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
                    <ChartPreviewCard
                        title=throughput_title.clone()
                        subtitle=chart_subtitle.clone()
                        open=throughput_open
                        preview=move || {
                            view! {
                                <BarChart
                                    x_labels=x_labels
                                    series=throughput_series
                                    height_px=140
                                    y_unit="req"
                                    interactive=true
                                    empty_message=no_data
                                />
                            }
                            .into_any()
                        }
                        detail=move || {
                            view! {
                                <BarChart
                                    x_labels=x_labels
                                    series=throughput_series
                                    height_px=380
                                    y_unit="req"
                                    interactive=true
                                    empty_message=no_data
                                />
                            }
                            .into_any()
                        }
                    />
                    <ChartPreviewCard
                        title=hit_title.clone()
                        subtitle=chart_subtitle.clone()
                        open=hit_open
                        preview=move || {
                            view! {
                                <BarChart
                                    x_labels=x_labels
                                    series=hit_series
                                    height_px=140
                                    y_unit="%"
                                    interactive=true
                                    empty_message=no_data
                                />
                            }
                            .into_any()
                        }
                        detail=move || {
                            view! {
                                <BarChart
                                    x_labels=x_labels
                                    series=hit_series
                                    height_px=380
                                    y_unit="%"
                                    interactive=true
                                    empty_message=no_data
                                />
                            }
                            .into_any()
                        }
                    />
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
    routing_key_concurrency: RwSignal<Option<Result<KeyConcurrencyResponse, String>>>,
) -> impl IntoView {
    view! {
        <div class="space-y-4">
            <div class="live-detail-row">
                <LiveRoutingSummaryPanel profiles=routing_profiles />
                <LiveLatencyPanel buckets=buckets.clone() summary=data.summary.clone() />
                <LiveLatestPanel data=data />
            </div>
            <div class="live-detail-row">
                <LiveKeyDistributionPanel
                    routing_key_ids=routing_key_ids
                    selected_routing_key=selected_routing_key
                    routing_key_data=routing_key_data
                />
                <LiveCacheLayerPanel buckets=buckets.clone() />
                <LiveKeyActivityPanel
                    routing_key_data=routing_key_data
                    routing_key_concurrency=routing_key_concurrency
                />
            </div>
            <LiveTokenPanel buckets=buckets />
        </div>
    }
}

#[component]
fn LiveRoutingSummaryPanel(
    profiles: RwSignal<Option<Result<Vec<ProfileRoutingView>, String>>>,
) -> impl IntoView {
    let t = use_translations();
    let selected_profile = RwSignal::new(String::new());
    view! {
        <div class="glass-card p-4 live-detail-card flex flex-col">
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
                            <div class="space-y-1.5 pt-2 border-t border-theme flex-1">
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
fn LiveKeyDistributionPanel(
    routing_key_ids: RwSignal<Vec<String>>,
    selected_routing_key: RwSignal<Option<String>>,
    routing_key_data: RwSignal<Option<Result<KeyRoutingResponse, String>>>,
) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card p-4 live-detail-card flex flex-col space-y-2">
            <h3 class="text-sm font-semibold text-theme">"Key / Affinity"</h3>
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
                None => view! {
                    <p class="text-[11px] text-theme-muted live-chart-compact">"Loading key routing…"</p>
                }.into_any(),
                Some(Err(e)) => view! {
                    <p class="text-[11px] text-error live-chart-compact">{e}</p>
                }.into_any(),
                Some(Ok(resp)) => {
                    let (backend_labels, backend_values) = build_backend_distribution(&resp);
                    let (aff_labels, aff_values) = build_affinity_distribution(&resp);
                    let backend_has = series_has_points(&backend_values);
                    let aff_has = series_has_points(&aff_values);
                    let backend_labels_arc = std::sync::Arc::new(backend_labels);
                    let backend_values_arc: std::sync::Arc<Vec<f64>> =
                        std::sync::Arc::new(backend_values.into_iter().flatten().collect());
                    let aff_labels_arc = std::sync::Arc::new(aff_labels);
                    let aff_values_arc: std::sync::Arc<Vec<f64>> =
                        std::sync::Arc::new(aff_values.into_iter().flatten().collect());

                    let backend_labels_sig = {
                        let backend_labels_arc = std::sync::Arc::clone(&backend_labels_arc);
                        Signal::derive(move || backend_labels_arc.as_ref().clone())
                    };
                    let backend_values_sig = {
                        let backend_values_arc = std::sync::Arc::clone(&backend_values_arc);
                        Signal::derive(move || backend_values_arc.as_ref().clone())
                    };
                    let aff_labels_sig = {
                        let aff_labels_arc = std::sync::Arc::clone(&aff_labels_arc);
                        Signal::derive(move || aff_labels_arc.as_ref().clone())
                    };
                    let aff_values_sig = {
                        let aff_values_arc = std::sync::Arc::clone(&aff_values_arc);
                        Signal::derive(move || aff_values_arc.as_ref().clone())
                    };
                    view! {
                        <div class="space-y-2 flex-1 flex flex-col">
                            <div class="text-[11px] text-theme-muted font-mono">
                                {format!("prefix_breaks={}", resp.prefix_break_count)}
                            </div>
                            <div class="grid grid-cols-1 sm:grid-cols-2 gap-2 flex-1">
                                <div class="border border-theme rounded-md p-2 flex flex-col">
                                    <div class="text-[11px] text-theme-muted mb-1">"Backend"</div>
                                    {if backend_has {
                                        view! {
                                            <HorizontalBarChart
                                                labels=backend_labels_sig
                                                values=backend_values_sig
                                                width=280
                                                height_px=72
                                                empty_message=t.live_no_data()
                                            />
                                        }.into_any()
                                    } else {
                                        view! {
                                            <p class="text-[11px] text-theme-muted live-chart-compact flex-1 flex items-center">
                                                {t.live_no_data()}
                                            </p>
                                        }.into_any()
                                    }}
                                </div>
                                <div class="border border-theme rounded-md p-2 flex flex-col">
                                    <div class="text-[11px] text-theme-muted mb-1">"Affinity kind"</div>
                                    {if aff_has {
                                        view! {
                                            <HorizontalBarChart
                                                labels=aff_labels_sig
                                                values=aff_values_sig
                                                width=280
                                                height_px=72
                                                empty_message=t.live_no_data()
                                            />
                                        }.into_any()
                                    } else {
                                        view! {
                                            <p class="text-[11px] text-theme-muted live-chart-compact flex-1 flex items-center">
                                                {t.live_no_data()}
                                            </p>
                                        }.into_any()
                                    }}
                                </div>
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
fn LiveLatencyPanel(
    buckets: Vec<LiveMetricsBucket>,
    summary: LiveMetricsSummary,
) -> impl IntoView {
    let t = use_translations();
    let buckets = std::sync::Arc::new(buckets);
    let (max_e2e, max_upstream, max_ttft) = max_bucket_latencies(buckets.as_ref());
    let na = t.live_upstream_na();
    let e2e_label = t.live_series_e2e().to_string();
    let upstream_label = t.live_series_upstream().to_string();
    let ttft_label = t.live_series_ttft().to_string();
    let x_labels = {
        let buckets = std::sync::Arc::clone(&buckets);
        Signal::derive(move || {
            buckets
                .iter()
                .map(|b| format_bucket_time(b.timestamp_ms))
                .collect()
        })
    };
    let series = {
        let buckets = std::sync::Arc::clone(&buckets);
        Signal::derive(move || {
            let b = buckets.as_ref();
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
                label: upstream_label.clone(),
                color: "var(--warning)",
                values: b.iter().map(|x| x.upstream_latency_ms).collect(),
                dashed: true,
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
        })
    };
    let latency_open = RwSignal::new(false);
    let latency_title = t.live_latency_chart().to_string();
    let no_data = t.live_no_data();
    view! {
        <div class="glass-card p-4 live-detail-card space-y-3 flex flex-col">
            <h3 class="text-sm font-semibold text-theme">{t.live_nodes_title()}</h3>
            <div class="grid grid-cols-2 gap-2">
                <LiveStatTile
                    label=t.live_avg_e2e()
                    value=format!("{:.0} ms", summary.avg_e2e_latency_ms)
                    variant=LiveTileVariant::Accent
                />
                <LiveStatTile
                    label="Max E2E"
                    value=format!("{max_e2e:.0} ms")
                    variant=LiveTileVariant::Orange
                />
                <LiveStatTile
                    label=t.live_avg_ttft()
                    value=format!("{:.0} ms", summary.avg_ttft_ms)
                    variant=LiveTileVariant::Teal
                />
                <LiveStatTile
                    label="Max TTFT"
                    value=format_latency_opt(max_ttft, na)
                    variant=LiveTileVariant::Warn
                />
            </div>
            <ChartPreviewCard
                title=latency_title
                open=latency_open
                preview=move || {
                    view! {
                        <BarChart
                            x_labels=x_labels
                            series=series
                            height_px=110
                            y_unit="ms"
                            interactive=true
                            empty_message=no_data
                        />
                    }
                    .into_any()
                }
                detail=move || {
                    view! {
                        <BarChart
                            x_labels=x_labels
                            series=series
                            height_px=360
                            y_unit="ms"
                            interactive=true
                            empty_message=no_data
                        />
                    }
                    .into_any()
                }
            />
            <p class="text-[10px] text-theme-muted leading-snug">
                {format!("Max upstream: {}", format_latency_opt(max_upstream, na))}
                " · "
                {t.live_upstream_hint()}
            </p>
        </div>
    }
}

#[component]
fn LiveCacheLayerPanel(buckets: Vec<LiveMetricsBucket>) -> impl IntoView {
    let t = use_translations();
    let stored = StoredValue::new(buckets);
    let x_labels = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|b| format_bucket_time(b.timestamp_ms))
            .collect()
    });
    let hit_series = Signal::derive(move || {
        let b = stored.get_value();
        vec![
            ChartSeries {
                label: "hits".to_string(),
                color: "var(--cc-success)",
                values: b.iter().map(|x| Some(x.cache_hit_count as f64)).collect(),
                dashed: false,
                fill: false,
            },
            ChartSeries {
                label: "miss".to_string(),
                color: "var(--cc-warning)",
                values: b
                    .iter()
                    .map(|x| {
                        Some((x.request_count.saturating_sub(x.cache_hit_count)) as f64)
                    })
                    .collect(),
                dashed: false,
                fill: false,
            },
        ]
    });
    let total_hits: u32 = stored.get_value().iter().map(|b| b.cache_hit_count).sum();
    let total_req: u32 = stored
        .get_value()
        .iter()
        .map(|b| b.request_count)
        .sum();
    let hit_pct = if total_req > 0 {
        total_hits as f64 / total_req as f64 * 100.0
    } else {
        0.0
    };
    let cache_open = RwSignal::new(false);
    let cache_title = t.live_cache_hit_trend().to_string();
    let no_data = t.live_no_data();
    view! {
        <div class="glass-card p-4 live-detail-card space-y-2 flex flex-col">
            <div class="flex items-center justify-between gap-2">
                <h3 class="text-sm font-semibold text-theme">{t.live_cache_hit_trend()}</h3>
                <span class="text-xs font-mono text-theme-muted">{format!("{hit_pct:.1}%")}</span>
            </div>
            <ChartPreviewCard
                title=cache_title
                open=cache_open
                preview=move || {
                    view! {
                        <BarChart
                            x_labels=x_labels
                            series=hit_series
                            height_px=100
                            y_unit="req"
                            interactive=true
                            empty_message=no_data
                        />
                    }
                    .into_any()
                }
                detail=move || {
                    view! {
                        <BarChart
                            x_labels=x_labels
                            series=hit_series
                            height_px=360
                            y_unit="req"
                            interactive=true
                            empty_message=no_data
                        />
                    }
                    .into_any()
                }
            />
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
    let input_values = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|x| Some(x.input_tokens as f64))
            .collect()
    });
    let output_values = Signal::derive(move || {
        stored
            .get_value()
            .iter()
            .map(|x| Some(x.output_tokens as f64))
            .collect()
    });
    let token_open = RwSignal::new(false);
    let token_title = t.live_token_chart().to_string();
    let no_data = t.live_no_data();
    let in_lbl = t.live_tokens_input().to_string();
    let out_lbl = t.live_tokens_output().to_string();
    let in_lbl_preview = in_lbl.clone();
    let out_lbl_preview = out_lbl.clone();
    let in_lbl_detail = in_lbl.clone();
    let out_lbl_detail = out_lbl.clone();
    view! {
        <div class="glass-card p-4">
            <ChartPreviewCard
                title=token_title
                open=token_open
                preview=move || {
                    view! {
                        <TokenLineChart
                            x_labels=x_labels
                            input_values=input_values
                            output_values=output_values
                            input_label=in_lbl_preview.clone()
                            output_label=out_lbl_preview.clone()
                            height_px=140
                            interactive=true
                            empty_message=no_data
                        />
                    }
                    .into_any()
                }
                detail=move || {
                    view! {
                        <TokenLineChart
                            x_labels=x_labels
                            input_values=input_values
                            output_values=output_values
                            input_label=in_lbl_detail.clone()
                            output_label=out_lbl_detail.clone()
                            height_px=380
                            interactive=true
                            empty_message=no_data
                        />
                    }
                    .into_any()
                }
            />
        </div>
    }
}

#[component]
fn LiveLatestPanel(data: LiveMetricsResponse) -> impl IntoView {
    let t = use_translations();
    let s = data.summary;
    view! {
        <div class="glass-card p-4 live-detail-card space-y-3 flex flex-col">
            <h3 class="text-sm font-semibold text-theme">{t.live_latest_request()}</h3>
            {match data.latest {
                None => view! {
                    <p class="text-xs text-theme-muted">{t.live_no_data()}</p>
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
                    <dl class="space-y-2 text-xs">
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

#[component]
fn LiveKeyActivityPanel(
    routing_key_data: RwSignal<Option<Result<KeyRoutingResponse, String>>>,
    routing_key_concurrency: RwSignal<Option<Result<KeyConcurrencyResponse, String>>>,
) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card p-4 live-detail-card space-y-3 flex flex-col">
            <h3 class="text-sm font-semibold text-theme">"Key activity (5m)"</h3>
            {move || match (routing_key_concurrency.get(), routing_key_data.get()) {
                (None, None) => view! {
                    <p class="text-xs text-theme-muted">"Select a key in load balancing card."</p>
                }.into_any(),
                (concurrency, routing) => {
                    let mut tiles: Vec<(&'static str, String, LiveTileVariant)> = Vec::new();
                    if let Some(Ok(c)) = &concurrency {
                        tiles.push((
                            "Active now",
                            c.active_now.to_string(),
                            LiveTileVariant::Accent,
                        ));
                        tiles.push((
                            "Peak concurrent",
                            c.concurrent_peak.to_string(),
                            LiveTileVariant::Orange,
                        ));
                        tiles.push((
                            "Window requests",
                            c.total_requests.to_string(),
                            LiveTileVariant::Teal,
                        ));
                    }
                    if let Some(Ok(r)) = &routing {
                        tiles.push((
                            "Prefix breaks",
                            r.prefix_break_count.to_string(),
                            LiveTileVariant::Warn,
                        ));
                        tiles.push((
                            "Migrations",
                            r.migrations.len().to_string(),
                            LiveTileVariant::Muted,
                        ));
                    }
                    view! {
                        <div class="grid grid-cols-2 gap-2">
                            {tiles.into_iter().map(|(label, value, variant)| {
                                view! {
                                    <LiveStatTile label=label value=value variant=variant />
                                }
                            }).collect_view()}
                        </div>
                        {if let Some(Ok(c)) = concurrency {
                            let recent: Vec<_> = c.entries.iter().rev().take(5).collect();
                            view! {
                                <div class="border-t border-theme pt-2 space-y-1">
                                    <div class="text-[11px] font-medium text-theme-muted uppercase tracking-wide">
                                        "Recent requests"
                                    </div>
                                    {if recent.is_empty() {
                                        view! { <p class="text-[11px] text-theme-muted">{t.live_no_data()}</p> }.into_any()
                                    } else {
                                        recent.into_iter().map(|e| {
                                            let cache = if e.cache_hit {
                                                e.cache_tier.clone().unwrap_or_else(|| "hit".into())
                                            } else {
                                                "miss".into()
                                            };
                                            let backend = e.backend_name.clone().unwrap_or_else(|| "—".into());
                                            view! {
                                                <div class="text-[11px] font-mono flex justify-between gap-2 border-b border-theme/50 py-1 last:border-0">
                                                    <span class="truncate text-theme">{e.model.clone()}</span>
                                                    <span class="text-theme-muted shrink-0">
                                                        {format!("{backend} · {cache} · {:.0}ms", e.latency_ms)}
                                                    </span>
                                                </div>
                                            }
                                        }).collect_view().into_any()
                                    }}
                                </div>
                            }.into_any()
                        } else if let Some(Err(e)) = concurrency {
                            view! { <p class="text-[11px] text-error">{e}</p> }.into_any()
                        } else {
                            ().into_any()
                        }}
                        {if let Some(Ok(r)) = routing {
                            if r.migrations.is_empty() {
                                view! { <p class="text-[11px] text-theme-muted">"No affinity migrations in window."</p> }.into_any()
                            } else {
                                view! {
                                    <div class="border-t border-theme pt-2 space-y-1">
                                        <div class="text-[11px] font-medium text-[var(--cc-warning)] uppercase tracking-wide">
                                            "Affinity migrations"
                                        </div>
                                        {r.migrations.iter().rev().take(3).map(|m| {
                                            view! {
                                                <div class="text-[11px] font-mono text-[var(--cc-warning)] py-0.5">
                                                    {format!(
                                                        "{} → {} ({})",
                                                        m.from_backend,
                                                        m.to_backend,
                                                        crate::datetime::format_ms_china_time(m.timestamp_ms, true)
                                                    )}
                                                </div>
                                            }
                                        }).collect_view()}
                                    </div>
                                }.into_any()
                            }
                        } else if let Some(Err(e)) = routing {
                            view! { <p class="text-[11px] text-error">{e}</p> }.into_any()
                        } else {
                            ().into_any()
                        }}
                    }.into_any()
                }
            }}
        </div>
    }
}
