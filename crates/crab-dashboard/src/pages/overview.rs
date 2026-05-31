use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::anomaly::detect_and_toast_with;
use crate::api;
use crate::components::canvas_line_chart::CanvasLineChart;
use crate::components::horizontal_bar_chart::HorizontalBarChart;
use crate::components::line_chart::ChartSeries;
use crate::components::page_header::PageHeader;
use crate::components::skeleton::SkeletonOverview;
use crate::components::sparkline::Sparkline;
use crate::locale::use_translations;
use crate::page_visible::page_visible;
use crate::time_utils::{format_number, now_hms_string};
use crate::types::TimeSeriesPoint;
use crate::types::{
    GatewayHealth, MetricsSnapshot, MetricsSnapshotCore, OverviewCore, OverviewOpsMetrics,
    OverviewSuggestion, PrefixCacheMetricsSnapshot, SemanticConfig, TraceSummary,
};
use crate::view_state;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn metrics_from_core(
    core: &MetricsSnapshotCore,
    points: &[TimeSeriesPoint],
    window: &str,
) -> MetricsSnapshot {
    let (hourly_stats, daily_stats) = if window == "7d" {
        (vec![], points.to_vec())
    } else {
        (points.to_vec(), vec![])
    };
    MetricsSnapshot {
        qps: core.qps,
        tps: core.tps,
        l0_hits: core.l0_hits,
        l1_hits: core.l1_hits,
        l2_hits: core.l2_hits,
        cache_misses: core.cache_misses,
        cache_hit_tokens: core.cache_hit_tokens,
        cache_miss_tokens: core.cache_miss_tokens,
        total_input_tokens: core.total_input_tokens,
        total_output_tokens: core.total_output_tokens,
        total_tokens: core.total_tokens,
        latency_l0_ms: core.latency_l0_ms,
        latency_l1_ms: core.latency_l1_ms,
        latency_l2_ms: core.latency_l2_ms,
        latency_upstream_ms: core.latency_upstream_ms,
        active_keys: core.active_keys,
        uptime_hours: core.uptime_hours,
        uptime_secs: core.uptime_secs,
        hourly_stats,
        daily_stats,
        weekly_stats: vec![],
        monthly_stats: vec![],
        semantic_hits: core.semantic_hits,
        semantic_rejected: core.semantic_rejected,
        semantic_skipped: core.semantic_skipped,
        prefix_cache_hit_tokens: core.prefix_cache_hit_tokens,
        prefix_cache_miss_tokens: core.prefix_cache_miss_tokens,
        prefix_cache_hit_ratio: core.prefix_cache_hit_ratio,
        hit_rate_cumulative: core.hit_rate_cumulative,
        hit_rate_5m: core.hit_rate_5m,
        token_hit_rate_5m: core.token_hit_rate_5m,
        qps_5m: core.qps_5m,
        coalesced_total: core.coalesced_total,
        consumer_buckets: core.consumer_buckets.clone(),
        domain_buckets: core.domain_buckets.clone(),
        metrics_sample_insufficient: core.metrics_sample_insufficient,
        history_meta: core.history_meta.clone(),
        tier_deltas_5m: core.tier_deltas_5m,
        latency_upstream_p99_ms: core.latency_upstream_p99_ms,
        latency_ttft_p99_ms: core.latency_ttft_p99_ms,
        latency_prefill_p99_ms: core.latency_prefill_p99_ms,
        latency_cache_fetch_p99_ms: core.latency_cache_fetch_p99_ms,
        error_rate_5m: core.error_rate_5m,
        http_4xx_5m: core.http_4xx_5m,
        http_5xx_5m: core.http_5xx_5m,
        qps_prev_1h: core.qps_prev_1h,
        hit_rate_prev_1h: core.hit_rate_prev_1h,
    }
}

#[component]
pub fn OverviewPage() -> impl IntoView {
    let t = use_translations();
    let vs = view_state::load_view_state();
    let overview_core: RwSignal<Option<Result<OverviewCore, String>>> = RwSignal::new(None);
    let trace_summary: RwSignal<Option<TraceSummary>> = RwSignal::new(None);
    let peak_hours_data: RwSignal<crate::types::ModelPeakHoursResponse> =
        RwSignal::new(crate::types::ModelPeakHoursResponse {
            models: vec![],
            data: vec![],
            last_aggregated_at: None,
        });
    let peak_hours_error: RwSignal<Option<String>> = RwSignal::new(None);
    let ts_points: RwSignal<Vec<TimeSeriesPoint>> = RwSignal::new(Vec::new());
    let ts_window = RwSignal::new(vs.ts_window.clone().unwrap_or_else(|| "1h".to_string()));
    let auto_refresh = RwSignal::new(vs.auto_refresh.unwrap_or(true));
    let last_update = RwSignal::new(String::new());
    let is_loading = RwSignal::new(false);
    let last_update_ts = RwSignal::new(0u64);
    let load_generation = RwSignal::new(0u64);
    let ts_generation = RwSignal::new(0u64);
    let etag = RwSignal::new(String::new());
    let ts_etag = RwSignal::new(String::new());
    let alive = Arc::new(AtomicBool::new(true));
    let toasts = crate::components::toast::use_toast();

    // SSE connection — receives pushed metrics, reducing polling overhead.
    // Data flows through a non-reactive buffer + rAF flush to decouple SSE
    // message frequency from Leptos signal propagation frequency.
    let sse_active = RwSignal::new(false);
    {
        let overview_core = overview_core;
        let last_update = last_update;
        let last_update_ts = last_update_ts;
        let sse_active = sse_active;
        let alive = Arc::clone(&alive);
        let toasts = toasts;
        leptos::task::spawn_local(async move {
            use futures::StreamExt;
            use std::cell::RefCell;
            use std::rc::Rc;

            // rAF flush state: buffer holds the latest OverviewCore, dirty
            // marks whether a new value arrived since the last flush, and
            // active keeps the rAF loop running while the SSE connection is up.
            let buffer: Rc<RefCell<Option<OverviewCore>>> = Rc::new(RefCell::new(None));
            let dirty: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
            let active: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

            // Start a self-rescheduling requestAnimationFrame loop that flushes
            // the buffer to Leptos signals at most once per frame.
            let raf_state: Rc<RefCell<Option<js_sys::Function>>> = Rc::new(RefCell::new(None));
            {
                let buffer = buffer.clone();
                let dirty = dirty.clone();
                let active = active.clone();
                let raf_state_inner = raf_state.clone();
                let alive_for_raf = Arc::clone(&alive);

                let flush = move || {
                    if !alive_for_raf.load(Ordering::Relaxed) {
                        *active.borrow_mut() = false;
                        return;
                    }
                    if *dirty.borrow() {
                        if let Some(core) = buffer.borrow_mut().take() {
                            detect_and_toast_with(toasts, &core);
                            overview_core.try_set(Some(Ok(core)));
                            last_update.try_set(now_hms_string());
                            last_update_ts.try_set(js_sys::Date::now() as u64);
                        }
                        *dirty.borrow_mut() = false;
                    }
                    if *active.borrow() {
                        // Self-reschedule: re-register the same persistent closure
                        // instead of allocating a new Closure::once every frame.
                        if let Some(func) = raf_state_inner.borrow().as_ref() {
                            let _ = web_sys::window().unwrap().request_animation_frame(func);
                        }
                    }
                };

                let closure =
                    wasm_bindgen::closure::Closure::wrap(Box::new(flush) as Box<dyn FnMut()>);
                let func: js_sys::Function = closure.into_js_value().into();
                *raf_state.borrow_mut() = Some(func);
            }

            // Exponential backoff for SSE reconnection attempts.
            let mut retry_delay_ms = 1_000;

            loop {
                if !alive.load(Ordering::Relaxed) {
                    break;
                }
                let es = match api::connect_sse().await {
                    Ok(es) => es,
                    Err(_) => {
                        if !alive.load(Ordering::Relaxed) {
                            break;
                        }
                        sse_active.try_set(false);
                        *active.borrow_mut() = false;
                        gloo_timers::future::TimeoutFuture::new(retry_delay_ms).await;
                        retry_delay_ms = (retry_delay_ms * 2).min(30_000);
                        continue;
                    }
                };
                sse_active.try_set(true);
                *active.borrow_mut() = true;
                retry_delay_ms = 1_000;

                // Kick off the rAF loop if not already running.
                if let Some(func) = raf_state.borrow().as_ref() {
                    let _ = web_sys::window().unwrap().request_animation_frame(func);
                }

                // Bridge EventSource callbacks to an async channel.
                // Wrap sender in Rc<RefCell<Option>> so both closures can drop it
                // to signal the receiver that the connection is closed.
                let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();
                let tx_shared: Rc<
                    RefCell<Option<futures::channel::mpsc::UnboundedSender<String>>>,
                > = Rc::new(RefCell::new(Some(tx)));

                let tx_msg = Rc::clone(&tx_shared);
                let message_closure = wasm_bindgen::closure::Closure::wrap(Box::new(
                    move |ev: web_sys::MessageEvent| {
                        if let Some(data) = ev.data().as_string() {
                            if let Some(ref sender) = *tx_msg.borrow() {
                                let _ = sender.unbounded_send(data);
                            }
                        }
                    },
                )
                    as Box<dyn FnMut(web_sys::MessageEvent)>);
                es.set_onmessage(Some(message_closure.as_ref().unchecked_ref()));

                // On error, take the sender to close the channel and exit the rx loop
                let tx_err = Rc::clone(&tx_shared);
                let error_closure =
                    wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                        tx_err.borrow_mut().take();
                    })
                        as Box<dyn FnMut(web_sys::Event)>);
                es.set_onerror(Some(error_closure.as_ref().unchecked_ref()));

                // Write incoming data to non-reactive buffer; rAF loop flushes
                // at most once per animation frame.
                while let Some(data) = rx.next().await {
                    if !alive.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                        if parsed.get("type").and_then(|t| t.as_str()) == Some("metrics") {
                            if let Some(metrics_data) = parsed.get("data") {
                                if let Ok(core) =
                                    serde_json::from_value::<OverviewCore>(metrics_data.clone())
                                {
                                    *buffer.borrow_mut() = Some(core);
                                    *dirty.borrow_mut() = true;
                                }
                            }
                        }
                    }
                }

                // Connection lost — stop rAF loop and retry
                es.set_onmessage(None);
                es.set_onerror(None);
                drop(message_closure);
                drop(error_closure);
                if !alive.load(Ordering::Relaxed) {
                    es.close();
                    break;
                }
                sse_active.try_set(false);
                *active.borrow_mut() = false;
                es.close();
                gloo_timers::future::TimeoutFuture::new(retry_delay_ms).await;
                retry_delay_ms = (retry_delay_ms * 2).min(30_000);
            }
        });
    }

    let alive_for_core = Arc::clone(&alive);
    let load_core: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if !alive_for_core.load(Ordering::Relaxed) {
            return;
        }
        load_generation.try_update(|g| *g += 1);
        let request_id = load_generation.try_get().unwrap_or(0);
        let current_etag = etag.try_get().unwrap_or_default();
        is_loading.try_set(true);
        let alive = Arc::clone(&alive_for_core);
        leptos::task::spawn_local(async move {
            TimeoutFuture::new(12_000).await;
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            if load_generation.try_get() == Some(request_id) {
                if overview_core.try_get().map_or(true, |o| o.is_none()) {
                    overview_core.try_set(Some(Err(
                        "Overview request timed out. Please refresh or re-login admin key."
                            .to_string(),
                    )));
                }
                is_loading.try_set(false);
            }
        });
        let alive = Arc::clone(&alive_for_core);
        leptos::task::spawn_local(async move {
            match api::fetch_overview_core(&current_etag).await {
                Ok(result) => {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    etag.try_set(result.etag);
                    if load_generation.try_get() == Some(request_id)
                        && let Some(core) = result.core
                    {
                        detect_and_toast_with(toasts, &core);
                        overview_core.try_set(Some(Ok(core)));
                        last_update.try_set(now_hms_string());
                        last_update_ts.try_set(js_sys::Date::now() as u64);
                    }
                }
                Err(e) => {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    if load_generation.try_get() == Some(request_id) {
                        overview_core.try_set(Some(Err(e)));
                    }
                }
            }
            if load_generation.try_get() == Some(request_id) {
                is_loading.try_set(false);
            }
        });
    });

    let alive_for_trace = Arc::clone(&alive);
    let load_trace: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let alive = Arc::clone(&alive_for_trace);
        leptos::task::spawn_local(async move {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            if let Ok(summary) = api::fetch_overview_trace().await {
                if !alive.load(Ordering::Relaxed) {
                    return;
                }
                trace_summary.try_set(Some(summary));
            }
        });
    });

    {
        let ph = peak_hours_data;
        let ph_err = peak_hours_error;
        let alive_ph = Arc::clone(&alive);
        leptos::task::spawn_local(async move {
            loop {
                if !alive_ph.load(Ordering::Relaxed) {
                    break;
                }
                match api::fetch_model_peak_hours(7).await {
                    Ok(resp) => {
                        ph.set(resp);
                        ph_err.set(None);
                    }
                    Err(e) => ph_err.set(Some(e)),
                }
                TimeoutFuture::new(300_000).await;
            }
        });
    }

    let alive_for_ts = Arc::clone(&alive);
    let load_timeseries: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if !alive_for_ts.load(Ordering::Relaxed) {
            return;
        }
        ts_generation.try_update(|g| *g += 1);
        let request_id = ts_generation.try_get().unwrap_or(0);
        let window = ts_window.try_get_untracked().unwrap_or_default();
        let current_ts_etag = ts_etag.try_get().unwrap_or_default();
        let alive = Arc::clone(&alive_for_ts);
        leptos::task::spawn_local(async move {
            match api::fetch_overview_timeseries_etag(&window, &current_ts_etag).await {
                Ok(result) => {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    ts_etag.try_set(result.etag);
                    if ts_generation.try_get() == Some(request_id) {
                        if let Some(points) = result.points {
                            ts_points.try_set(points);
                        }
                    }
                }
                Err(e) => {
                    web_sys::console::warn_1(
                        &format!("[overview] timeseries fetch failed: {e}").into(),
                    );
                }
            }
        });
    });

    let deferred_loaded = RwSignal::new(false);

    load_core();

    Effect::new({
        let load_trace = Arc::clone(&load_trace);
        let load_timeseries = Arc::clone(&load_timeseries);
        move |_| {
            if deferred_loaded.get() {
                return;
            }
            if matches!(overview_core.get(), Some(Ok(_))) {
                deferred_loaded.set(true);
                load_trace();
                load_timeseries();
            }
        }
    });

    Effect::new({
        let load_timeseries = Arc::clone(&load_timeseries);
        move |_| {
            if !deferred_loaded.get() {
                return;
            }
            let _ = ts_window.get();
            load_timeseries();
        }
    });

    // Persist view state on change
    Effect::new(move |_| {
        let w = ts_window.get();
        let ar = auto_refresh.get();
        let mut state = view_state::load_view_state();
        state.ts_window = Some(w);
        state.auto_refresh = Some(ar);
        view_state::save_view_state(&state);
    });

    let alive_poll = Arc::clone(&alive);
    let load_core_poll = Arc::clone(&load_core);
    let load_ts_poll = Arc::clone(&load_timeseries);
    let load_trace_poll = Arc::clone(&load_trace);
    leptos::task::spawn_local(async move {
        let mut tick: u64 = 0;
        loop {
            TimeoutFuture::new(10_000).await;
            if !alive_poll.load(Ordering::Relaxed) {
                break;
            }
            tick += 1;
            if auto_refresh.try_get() == Some(true) && page_visible() {
                // Core metrics: skip polling when SSE is actively pushing updates.
                if sse_active.try_get() != Some(true) {
                    load_core_poll();
                }
                // Timeseries & trace: always poll regardless of SSE (SSE does not push these).
                if tick.is_multiple_of(6) && deferred_loaded.try_get_untracked() == Some(true) {
                    load_ts_poll();
                    load_trace_poll();
                }
            }
        }
    });

    // Relative time updater
    let relative_time = RwSignal::new(String::new());
    let alive_clock = Arc::clone(&alive);
    leptos::task::spawn_local(async move {
        loop {
            TimeoutFuture::new(1_000).await;
            if !alive_clock.load(Ordering::Relaxed) {
                break;
            }
            let ts = last_update_ts.try_get().unwrap_or(0);
            if ts > 0 {
                let now = js_sys::Date::now() as u64;
                let elapsed_ms = now.saturating_sub(ts);
                let text = if elapsed_ms < 1000 {
                    "just now".to_string()
                } else if elapsed_ms < 60_000 {
                    format!("{}s ago", elapsed_ms / 1000)
                } else if elapsed_ms < 3_600_000 {
                    format!("{}m ago", elapsed_ms / 60_000)
                } else {
                    format!("{}h ago", elapsed_ms / 3_600_000)
                };
                relative_time.try_set(text);
            }
        }
    });

    on_cleanup(move || {
        alive.store(false, Ordering::Relaxed);
    });

    view! {
        // Progress bar at top
        <div class="fixed top-0 left-0 right-0 z-50 h-0.5 bg-transparent"
            style:opacity=move || if is_loading.get() { "1" } else { "0" }
            style:transition="opacity 0.3s"
        >
            <div class="h-full bg-accent animate-pulse"
                style:width="100%"
                style:animation="loading-bar 1.5s ease-in-out infinite"
            ></div>
        </div>

        <div class="page-content space-y-6">
            <PageHeader
                title=move || t.overview_title()
                description=move || t.overview_desc()
            >
                <div class="flex items-center gap-3 flex-wrap justify-end">
                    <span class="text-xs text-theme-muted">
                        {move || {
                            let rt = relative_time.get();
                            if rt.is_empty() {
                                format!("{}: {}", t.overview_last_update(), last_update.get())
                            } else {
                                format!("{}: {}", t.overview_last_update(), rt)
                            }
                        }}
                    </span>
                    <label class="flex items-center gap-2 text-xs text-theme-secondary">
                        <input
                            type="checkbox"
                            prop:checked=move || auto_refresh.get()
                            on:change=move |ev| auto_refresh.set(event_target_checked(&ev))
                            class="rounded"
                        />
                        {t.overview_auto_refresh()}
                    </label>
                    <button
                        on:click=move |_| {
                            load_core();
                            load_trace();
                            load_timeseries();
                        }
                        class=move || {
                            if is_loading.get() {
                                "btn btn-secondary text-xs animate-spin-slow"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_refresh()}
                    </button>
                </div>
            </PageHeader>

            {move || match overview_core.get() {
                None => view! { <SkeletonOverview /> }.into_any(),
                Some(Err(e)) => {
                    let t = use_translations();
                    let hint = overview_error_hint(&e, &t);
                    view! {
                        <div class="glass-card text-error text-sm space-y-2">
                            <p>{format!("{}: {}", t.overview_load_error(), e)}</p>
                            {hint.map(|h| view! { <p class="text-theme-muted text-xs">{h.clone()}</p> })}
                        </div>
                    }.into_any()
                }
                Some(Ok(_b)) => view! {
                    <OverviewContent
                        overview_core
                        trace_summary
                        peak_hours_data
                        peak_hours_error
                        ts_points
                        ts_window
                    />
                }.into_any(),
            }}
        </div>
    }
}

/// Content section rendered when core data is available.
/// Uses Memo internally so each subsection only re-renders when its
/// specific data has changed (by PartialEq).
#[component]
fn OverviewContent(
    overview_core: RwSignal<Option<Result<OverviewCore, String>>>,
    trace_summary: RwSignal<Option<TraceSummary>>,
    peak_hours_data: RwSignal<crate::types::ModelPeakHoursResponse>,
    peak_hours_error: RwSignal<Option<String>>,
    ts_points: RwSignal<Vec<TimeSeriesPoint>>,
    ts_window: RwSignal<String>,
) -> impl IntoView {
    let t = use_translations();
    let selected_domain: RwSignal<Option<String>> = RwSignal::new(None);

    // Memo for metrics snapshot — only changes when derived value differs.
    let metrics_memo = Memo::new(move |_| {
        let core_opt = overview_core.get();
        let core = match core_opt {
            Some(Ok(ref c)) => c,
            _ => return None,
        };
        let window = ts_window.get();
        let points = ts_points.get();
        Some(metrics_from_core(&core.metrics, &points, &window))
    });

    // Memo for health.
    let health_memo =
        Memo::new(move |_| overview_core.get().and_then(|r| r.ok()).map(|c| c.health));

    // Memo for prefix cache.
    let prefix_memo = Memo::new(move |_| {
        overview_core
            .get()
            .and_then(|r| r.ok())
            .map(|c| c.prefix_cache)
    });

    // Memo for semantic config.
    let semantic_memo =
        Memo::new(move |_| overview_core.get().and_then(|r| r.ok()).map(|c| c.semantic));

    // Memo for ops.
    let ops_memo = Memo::new(move |_| overview_core.get().and_then(|r| r.ok()).map(|c| c.ops));

    // Memo for suggestions.
    let suggestions_memo = Memo::new(move |_| {
        overview_core
            .get()
            .and_then(|r| r.ok())
            .map(|c| c.suggestions)
    });

    // Memo for trace summary.
    let trace_memo = Memo::new(move |_| trace_summary.get());

    // Derive the upstream CTA signal — boolean-only, very cheap.
    let show_cta = Memo::new(move |_| {
        let core_opt = overview_core.get();
        let core = match core_opt {
            Some(Ok(ref c)) => c,
            _ => return false,
        };
        core.health.upstream_key_count == 0 && core.ops.upstream_key_count == 0
    });

    view! {
        <div class="space-y-2">
            {move || show_cta.get().then(|| view! {
                <div class="glass-card flex flex-wrap items-center justify-between gap-3 border border-warning/30">
                    <p class="text-sm text-warning">{t.overview_setup_upstream_cta()}</p>
                    <a href="/upstream" class="btn btn-primary text-sm">
                        {t.overview_setup_upstream_link()}
                    </a>
                </div>
            })}

            <super::overview_cards::OverviewCardGrid
                health_memo=health_memo
                metrics_memo=metrics_memo
                ops_memo=ops_memo
                prefix_memo=prefix_memo
                semantic_memo=semantic_memo
                trace_memo=trace_memo
                suggestions_memo=suggestions_memo
                ts_points=ts_points
                ts_window=ts_window
                selected_domain=selected_domain
                peak_hours_data=peak_hours_data
                peak_hours_error=peak_hours_error
            />

            <DataPlaneDiagnostics />
            <ObservabilityFooter />
        </div>
    }
}

fn overview_error_hint(err: &str, t: &crate::locale::Translations) -> Option<String> {
    if err.contains("HTTP 502") {
        Some(t.overview_error_hint_502().to_string())
    } else if err.contains("HTTP 503") {
        Some(t.overview_error_hint_503().to_string())
    } else {
        None
    }
}

#[component]
fn ChartSuggestions(suggestions: Vec<OverviewSuggestion>, target: &'static str) -> impl IntoView {
    let filtered: Vec<_> = suggestions
        .into_iter()
        .filter(|s| s.target == target)
        .collect();
    let t = use_translations();
    view! {
        {(!filtered.is_empty()).then(|| view! {
            <div class="impact-hint space-y-2">
                <div class="text-xs font-medium text-theme-secondary">{t.overview_suggestions_title()}</div>
                {filtered.into_iter().map(|s| {
                    let class = match s.severity.as_str() {
                        "warn" => "text-warning text-xs",
                        "action" => "text-accent text-xs",
                        _ => "text-theme-muted text-xs",
                    };
                    view! { <p class=class>{s.message}</p> }
                }).collect::<Vec<_>>()}
            </div>
        })}
    }
}

#[component]
pub fn OverviewHealthStrip(health: GatewayHealth, error_rate: f64) -> impl IntoView {
    let t = use_translations();
    let healthy = health.healthy;
    let stream_on = health.stream_cache_enabled;
    let err_msg = health.error.clone();
    let keys = format!(
        "{}/{}",
        health.upstream_keys_available, health.upstream_key_count
    );

    view! {
        <div class="glass-card flex flex-wrap items-center gap-4 text-sm">
            <span class="font-medium text-theme-secondary">{t.overview_health_title()}</span>
            <span class=if healthy { "flex items-center gap-2 text-accent" } else { "flex items-center gap-2 text-error" }>
                <span class=if healthy { "online-dot" } else { "w-2 h-2 rounded-full bg-error" }></span>
                {if healthy { t.overview_status_active() } else { t.overview_health_unhealthy() }}
            </span>
            <span class="text-theme-muted">
                {t.overview_health_upstream_keys()}: <span class="font-mono text-theme">{keys}</span>
            </span>
            <span class="text-theme-muted">
                {t.overview_health_stream_cache()}: <span class="font-mono text-theme">
                    {if stream_on { "on" } else { "off" }}
                </span>
            </span>
            {move || {
                if error_rate > 0.001 {
                    view! {
                        <span class="flex items-center gap-1.5 text-error">
                            <span class="w-2 h-2 rounded-full bg-error"></span>
                            <span class="font-mono text-xs">{format!("err: {:.1}%", error_rate * 100.0)}</span>
                        </span>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}
            {err_msg.map(|e| {
                let tip = e.clone();
                view! {
                    <span class="text-xs text-error truncate max-w-md" title=tip>{e}</span>
                }
            })}
        </div>
    }
}

#[component]
pub fn TraceCompareBanner(
    trace: TraceSummary,
    metrics: MetricsSnapshot,
    #[prop(default = false)] compact: bool,
) -> impl IntoView {
    let t = use_translations();
    let trace_pct = trace.cache_hit_ratio * 100.0;
    let gw_pct = if metrics.metrics_sample_insufficient {
        None
    } else {
        Some(metrics.hit_rate_5m * 100.0)
    };

    let delta_html = gw_pct.map(|gw| {
        let delta = trace_pct - gw;
        let (color_class, prefix) = if delta > 0.0 {
            ("text-green-500", "+")
        } else if delta < -15.0 {
            ("text-red-500", "")
        } else if delta < -5.0 {
            ("text-amber-400", "")
        } else {
            ("text-theme-muted", if delta >= 0.0 { "+" } else { "" })
        };
        (color_class, format!("{}{:.1}%", prefix, delta))
    });

    view! {
        {if compact {
            view! {
                <div class="trace-compare-strip">
                    <span class="trace-compare-strip-label">{t.overview_trace_compare_title()}</span>
                    <span class="trace-compare-strip-stat">
                        "24h " <span class="text-accent">{format!("{trace_pct:.1}%")}</span>
                        " · " {trace.total_requests} " req"
                    </span>
                    <span class="trace-compare-strip-stat">
                        "5m "
                        {match gw_pct {
                            Some(p) => view! { <span class="text-accent">{format!("{p:.1}%")}</span> }.into_any(),
                            None => view! { <span class="text-theme-muted">"—"</span> }.into_any(),
                        }}
                    </span>
                    {delta_html.map(|(color, text)| {
                        view! { <span class=format!("trace-compare-strip-stat {color}")>{text}</span> }.into_any()
                    }).unwrap_or_else(|| ().into_any())}
                    <span class="trace-compare-strip-actions">
                        <a href="/cache?tab=trace" class="trace-compare-strip-link">{t.overview_trace_compare_link()}</a>
                        <a href="/live" class="trace-compare-strip-link">{t.sidebar_live()}</a>
                    </span>
                </div>
            }.into_any()
        } else {
            view! {
                <div class="glass-card flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h3 class="text-sm font-semibold text-theme mb-1">{t.overview_trace_compare_title()}</h3>
                        <p class="text-xs text-theme-muted mb-2">{t.trace_hours_note()}</p>
                        <div class="flex flex-wrap gap-6 text-sm font-mono tabular-nums">
                            <span>
                                "24h trace: " <span class="text-accent">{format!("{trace_pct:.1}%")}</span>
                                " (" {trace.total_requests} " req)"
                            </span>
                            <span>
                                "5m gateway: "
                                {match gw_pct {
                                    Some(p) => view! { <span class="text-accent">{format!("{p:.1}%")}</span> }.into_any(),
                                    None => view! { <span class="text-theme-muted">"—"</span> }.into_any(),
                                }}
                            </span>
                            {delta_html.map(|(color, text)| {
                                view! { <span class=color>{text}</span> }.into_any()
                            }).unwrap_or_else(|| view! { <span></span> }.into_any())}
                        </div>
                    </div>
                    <div class="flex gap-2 shrink-0">
                        <a href="/cache?tab=trace" class="btn btn-secondary text-xs">
                            {t.overview_trace_compare_link()}
                        </a>
                        <a href="/live" class="btn btn-secondary text-xs">
                            {t.sidebar_live()}
                        </a>
                    </div>
                </div>
            }.into_any()
        }}
    }
}

#[component]
pub fn OpsMetricsRow(ops: OverviewOpsMetrics) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_ops_title()}</h3>
            <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
                <div>
                    <div class="text-xs text-theme-muted">{t.overview_ops_ttft()}</div>
                    <div class="text-xl font-mono tabular-nums text-accent">
                        {format!("{:.0}ms", ops.ttft_ms)}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted">{t.overview_ops_coalesced_5m()}</div>
                    <div class="text-xl font-mono tabular-nums text-accent">
                        {format!("{:.0}", ops.coalesced_5m)}
                    </div>
                    <div class="text-xs text-theme-muted">{format!("Σ {}", ops.coalesced_total)}</div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted">{t.overview_ops_rejected_5m()}</div>
                    <div class="text-xl font-mono tabular-nums text-warning">
                        {format!("{:.0}", ops.rejected_5m)}
                    </div>
                    <div class="text-xs text-theme-muted">{format!("Σ {}", ops.rejected_total)}</div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted">{t.overview_cost_saved_5m()}</div>
                    <div class="text-xl font-mono tabular-nums text-warning">
                        {format!("${:.4}", ops.cost_saved_usd_5m)}
                    </div>
                    <div class="text-xs text-theme-muted">
                        {format!("${:.2}", ops.cost_saved_usd_total)}
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn PrefixCacheCard(prefix: PrefixCacheMetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let ratio_pct = prefix.hit_ratio * 100.0;
    let total = prefix.hit_tokens + prefix.miss_tokens;
    let by_model = prefix.by_model.clone();

    view! {
        <div class="glass-card glass-card-flush">
            <crate::components::ui::PanelHeader
                title=move || t.overview_prefix_cache_title().to_string()
                meta=move || t.overview_prefix_cache_desc().to_string()
            />
            <div class="p-5 pt-0">
            <div class="flex flex-wrap items-end gap-6 mb-4">
                <div>
                    <div class="text-3xl font-mono tabular-nums text-accent font-semibold">
                        {format!("{:.1}%", ratio_pct)}
                    </div>
                    <div class="text-xs text-theme-muted mt-1">"L3 hit ratio"</div>
                </div>
                <div class="text-sm font-mono tabular-nums text-theme-secondary space-y-1">
                    <div>{format!("hit: {}", prefix.hit_tokens)}</div>
                    <div>{format!("miss: {}", prefix.miss_tokens)}</div>
                    <div class="text-theme-muted">{format!("total tokens: {}", total)}</div>
                </div>
            </div>
            {(!by_model.is_empty()).then(|| view! {
                <div>
                    <h4 class="text-xs font-medium text-theme-muted mb-2">{t.overview_prefix_by_model()}</h4>
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-theme-muted border-b border-theme">
                                    <th class="pb-2 pr-4">"model"</th>
                                    <th class="pb-2 pr-4">"hit"</th>
                                    <th class="pb-2 pr-4">"miss"</th>
                                    <th class="pb-2">"ratio"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {by_model.into_iter().map(|row| {
                                    view! {
                                        <tr class="border-b border-theme/50">
                                            <td class="py-2 pr-4 font-mono text-theme">{row.model}</td>
                                            <td class="py-2 pr-4 font-mono tabular-nums">{format_number(row.hit_tokens)}</td>
                                            <td class="py-2 pr-4 font-mono tabular-nums">{format_number(row.miss_tokens)}</td>
                                            <td class="py-2 font-mono tabular-nums text-accent">
                                                {format!("{:.1}%", row.hit_ratio * 100.0)}
                                            </td>
                                        </tr>
                                    }
                                }).collect::<Vec<_>>()}
                            </tbody>
                        </table>
                    </div>
                </div>
            })}
            </div>
        </div>
    }
}

#[component]
pub fn TokenStats(metrics: MetricsSnapshot, prefix: PrefixCacheMetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let l3_total = prefix.hit_tokens + prefix.miss_tokens;
    let l3_ratio = if l3_total > 0 {
        prefix.hit_tokens as f64 / l3_total as f64 * 100.0
    } else {
        0.0
    };

    view! {
        <div class="glass-card">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-sm font-semibold text-theme">{t.overview_token_stats()}</h3>
                <div class="text-2xl opacity-30">"📊"</div>
            </div>
            <div class="grid grid-cols-2 md:grid-cols-4 gap-6">
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_input_tokens()}</div>
                    <div class="text-2xl font-mono tabular-nums text-theme font-semibold">
                        {format_number(metrics.total_input_tokens)}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_output_tokens()}</div>
                    <div class="text-2xl font-mono tabular-nums text-accent font-semibold">
                        {format_number(metrics.total_output_tokens)}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_total_tokens()}</div>
                    <div class="text-2xl font-mono tabular-nums text-warning font-semibold">
                        {format_number(metrics.total_tokens)}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_l3_input_ratio()}</div>
                    <div class="text-2xl font-mono tabular-nums text-accent font-semibold">
                        {format!("{l3_ratio:.1}%")}
                    </div>
                    <div class="text-xs text-theme-muted mt-1">
                        {format!("{} / {} L3 tokens", format_number(prefix.hit_tokens), format_number(l3_total))}
                    </div>
                </div>
            </div>
        </div>
    }
}

const MAX_TIMESERIES_CHART_POINTS: usize = 36;

fn compress_timeseries_points(data: Vec<TimeSeriesPoint>) -> Vec<TimeSeriesPoint> {
    if data.len() <= MAX_TIMESERIES_CHART_POINTS {
        return data;
    }
    data[data.len() - MAX_TIMESERIES_CHART_POINTS..].to_vec()
}

#[component]
pub fn TimeSeriesChart(
    points: RwSignal<Vec<TimeSeriesPoint>>,
    selected_view: RwSignal<String>,
    suggestions: Vec<OverviewSuggestion>,
    #[prop(default = false)] compact: bool,
) -> impl IntoView {
    let t = use_translations();

    let chart_points = Memo::new(move |_| compress_timeseries_points(points.get()));

    let has_chart_data = Memo::new(move |_| {
        chart_points
            .get()
            .iter()
            .any(|p| p.tokens > 0 || p.requests > 0)
    });

    let x_labels = Signal::derive(move || {
        chart_points
            .get()
            .iter()
            .map(|p| p.timestamp.clone())
            .collect::<Vec<_>>()
    });

    let token_series = Signal::derive(move || {
        let points = chart_points.get();
        vec![ChartSeries {
            label: t.overview_input_tokens().to_string(),
            color: "var(--accent-primary)".to_string(),
            values: points.iter().map(|p| Some(p.tokens as f64)).collect(),
            dashed: false,
            fill: true,
        }]
    });

    let request_series = Signal::derive(move || {
        let points = chart_points.get();
        vec![ChartSeries {
            label: t.overview_requests().to_string(),
            color: "var(--info)".to_string(),
            values: points.iter().map(|p| Some(p.requests as f64)).collect(),
            dashed: false,
            fill: false,
        }]
    });

    view! {
        <div class=if compact { "glass-card glass-card-compact" } else { "glass-card" }>
            <div class="flex items-center justify-between mb-2">
                <h3 class="text-sm font-semibold text-theme">{t.overview_usage_trends()}</h3>
                <div class="flex gap-1">
                    <button
                        on:click=move |_| selected_view.set("1h".to_string())
                        class=move || {
                            if selected_view.get() == "1h" {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_timeseries_1h()}
                    </button>
                    <button
                        on:click=move |_| selected_view.set("24h".to_string())
                        class=move || {
                            if selected_view.get() == "24h" {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_timeseries_24h()}
                    </button>
                    <button
                        on:click=move |_| selected_view.set("7d".to_string())
                        class=move || {
                            if selected_view.get() == "7d" {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_timeseries_7d()}
                    </button>
                </div>
            </div>

            <ChartSuggestions suggestions=suggestions target="timeseries" />

            {move || {
                if !has_chart_data.get() {
                    view! {
                        <div class="overview-ts-empty">
                            <p class="text-xs text-theme-muted">{t.overview_collecting_timeseries()}</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="space-y-1">
                            <CanvasLineChart
                                x_labels=x_labels
                                series=token_series
                                height_px=if compact { 132 } else { 160 }
                                y_unit="tokens"
                                empty_message=t.overview_collecting_timeseries()
                            />
                            <CanvasLineChart
                                x_labels=x_labels
                                series=request_series
                                height_px=if compact { 132 } else { 160 }
                                y_unit="req"
                                empty_message=t.overview_collecting_timeseries()
                            />
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

fn format_uptime_display(uptime_secs: u64, uptime_hours: u64) -> String {
    if uptime_secs > 0 {
        if uptime_secs < 3600 {
            let mins = uptime_secs / 60;
            let secs = uptime_secs % 60;
            return format!("{mins}m {secs}s");
        }
        let hours = uptime_secs / 3600;
        let mins = (uptime_secs % 3600) / 60;
        if mins > 0 {
            return format!("{hours}h {mins}m");
        }
        return format!("{hours}h");
    }
    format!("{uptime_hours}h")
}

#[component]
pub fn CoalescingCard(metrics: MetricsSnapshot, ops: OverviewOpsMetrics) -> impl IntoView {
    let t = use_translations();
    let d = metrics.tier_deltas_5m;
    let unique_requests = d.l0 + d.l1 + d.l2 + d.miss;
    let coalesced_5m = ops.coalesced_5m;
    let total_activity = unique_requests + coalesced_5m;
    let efficiency_pct = if total_activity > 0 {
        coalesced_5m as f64 / total_activity as f64 * 100.0
    } else {
        0.0
    };
    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-1">{t.overview_coalescing_title()}</h3>
            <p class="text-xs text-theme-muted mb-3">{t.overview_coalescing_desc()}</p>
            <div class="text-3xl font-mono tabular-nums text-accent font-semibold">
                {format!("{:.0}", ops.coalesced_5m)}
            </div>
            <div class="text-xs text-theme-muted mt-1">
                {format!("5m · Σ {} (metrics {})", ops.coalesced_total, metrics.coalesced_total)}
            </div>
            <div class="mt-4 space-y-1.5">
                <div class="flex items-center justify-between text-xs text-theme-muted">
                    <span>"5m coalesce share"</span>
                    <span class="font-mono tabular-nums text-accent">{format!("{:.1}%", efficiency_pct)}</span>
                </div>
                <div class="h-2 w-full bg-theme-tertiary rounded overflow-hidden">
                    <div
                        class="h-full bg-accent transition-all duration-300"
                        style:width=format!("{:.1}%", efficiency_pct.min(100.0))
                    ></div>
                </div>
                <div class="flex items-center justify-between text-[11px] text-theme-muted font-mono tabular-nums">
                    <span>{format!("saved {:.0}", coalesced_5m)}</span>
                    <span>{format!("unique {:.0}", unique_requests)}</span>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn SemanticCacheCard(metrics: MetricsSnapshot, semantic: SemanticConfig) -> impl IntoView {
    let t = use_translations();
    let disabled = !semantic.enabled;

    view! {
        <div class="glass-card h-full">
            <div class="flex items-center gap-2 mb-1">
                <h3 class="text-sm font-semibold text-theme">{t.overview_semantic_card_title()}</h3>
                {disabled.then(|| view! {
                    <span class="text-xs px-2 py-0.5 rounded bg-warning/20 text-warning">
                        {t.overview_semantic_disabled()}
                    </span>
                })}
            </div>
            <p class="text-xs text-theme-muted mb-3">
                {if disabled {
                    t.overview_semantic_disabled().to_string()
                } else {
                    t.overview_semantic_hint().to_string()
                }}
            </p>
            <div class="grid grid-cols-3 gap-3 text-center">
                <div>
                    <div class="text-xs text-theme-muted">{t.overview_hits()}</div>
                    <div class="text-xl font-mono text-accent">{metrics.semantic_hits}</div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted">"rejected"</div>
                    <div class="text-xl font-mono text-warning">{metrics.semantic_rejected}</div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted">"skipped"</div>
                    <div class="text-xl font-mono text-theme-secondary">{metrics.semantic_skipped}</div>
                </div>
            </div>
            {(!disabled).then(|| view! {
                <p class="text-xs text-theme-muted mt-3">
                    {format!("threshold {:.2}", semantic.similarity_threshold)}
                </p>
            })}
        </div>
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConsumerSortField {
    Consumer,
    HitTokens,
    MissTokens,
    Ratio,
}

#[component]
pub fn ConsumerHitTable(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let sort_by = RwSignal::new(ConsumerSortField::HitTokens);
    let sort_desc = RwSignal::new(true);
    let consumer_buckets = std::sync::Arc::new(metrics.consumer_buckets);

    let sorted_buckets = {
        let data = std::sync::Arc::clone(&consumer_buckets);
        Memo::new(move |_| {
            let mut buckets = (*data).clone();
            let desc = sort_desc.get();
            match sort_by.get() {
                ConsumerSortField::Consumer => {
                    buckets.sort_by(|a, b| {
                        let ord = a.consumer.cmp(&b.consumer);
                        if desc { ord.reverse() } else { ord }
                    });
                }
                ConsumerSortField::HitTokens => {
                    buckets.sort_by(|a, b| {
                        let ord = a.hit_tokens.cmp(&b.hit_tokens);
                        if desc { ord.reverse() } else { ord }
                    });
                }
                ConsumerSortField::MissTokens => {
                    buckets.sort_by(|a, b| {
                        let ord = a.miss_tokens.cmp(&b.miss_tokens);
                        if desc { ord.reverse() } else { ord }
                    });
                }
                ConsumerSortField::Ratio => {
                    buckets.sort_by(|a, b| {
                        let ord = a
                            .hit_ratio
                            .partial_cmp(&b.hit_ratio)
                            .unwrap_or(std::cmp::Ordering::Equal);
                        if desc { ord.reverse() } else { ord }
                    });
                }
            }
            buckets
        })
    };

    let sort_icon = move |field: ConsumerSortField| {
        move || {
            if sort_by.get() == field {
                if sort_desc.get() { " ▼" } else { " ▲" }
            } else {
                ""
            }
        }
    };

    let toggle_sort = move |field: ConsumerSortField| {
        move |_| {
            if sort_by.get() == field {
                sort_desc.update(|d| *d = !*d);
            } else {
                sort_by.set(field);
                sort_desc.set(true);
            }
        }
    };

    // Top 10 consumers by total tokens for the horizontal bar chart.
    let top10_labels: Signal<Vec<String>> = {
        let buckets = std::sync::Arc::clone(&consumer_buckets);
        Signal::derive(move || {
            let mut sorted = (*buckets).clone();
            sorted.sort_by(|a, b| {
                (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens))
            });
            sorted.iter().take(10).map(|b| b.consumer.clone()).collect()
        })
    };
    let top10_values: Signal<Vec<f64>> = {
        let buckets = std::sync::Arc::clone(&consumer_buckets);
        Signal::derive(move || {
            let mut sorted = (*buckets).clone();
            sorted.sort_by(|a, b| {
                (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens))
            });
            sorted
                .iter()
                .take(10)
                .map(|b| (b.hit_tokens + b.miss_tokens) as f64)
                .collect()
        })
    };

    view! {
        <div class="glass-card">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_consumer_table_title()}</h3>
            <div class="mb-4">
                <HorizontalBarChart
                    labels=top10_labels
                    values=top10_values
                    width=520
                    height_px=160
                    empty_message="No consumer data."
                />
            </div>
            {move || {
                let buckets = sorted_buckets.get();
                if buckets.is_empty() {
                    view! {
                        <p class="text-sm text-theme-muted">{t.overview_no_data()}</p>
                    }.into_any()
                } else {
                    view! {
                        <div class="overflow-x-auto">
                            <table class="w-full text-sm">
                                <thead>
                                    <tr class="text-left text-xs text-theme-muted border-b border-theme">
                                        <th class="pb-2 pr-4 cursor-pointer select-none hover:text-theme" on:click=toggle_sort(ConsumerSortField::Consumer)>
                                            {t.overview_consumer_col()}<span class="text-accent">{sort_icon(ConsumerSortField::Consumer)}</span>
                                        </th>
                                        <th class="pb-2 pr-4 cursor-pointer select-none hover:text-theme" on:click=toggle_sort(ConsumerSortField::HitTokens)>
                                            "hit tokens"<span class="text-accent">{sort_icon(ConsumerSortField::HitTokens)}</span>
                                        </th>
                                        <th class="pb-2 pr-4 cursor-pointer select-none hover:text-theme" on:click=toggle_sort(ConsumerSortField::MissTokens)>
                                            "miss tokens"<span class="text-accent">{sort_icon(ConsumerSortField::MissTokens)}</span>
                                        </th>
                                        <th class="pb-2 pr-4 cursor-pointer select-none hover:text-theme" on:click=toggle_sort(ConsumerSortField::Ratio)>
                                            "ratio"<span class="text-accent">{sort_icon(ConsumerSortField::Ratio)}</span>
                                        </th>
                                        <th class="pb-2">"trend"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {buckets.into_iter().map(|b| {
                                        let spark_values = vec![b.miss_tokens as f64, b.hit_tokens as f64];
                                        view! {
                                            <tr class="border-b border-theme/50">
                                                <td class="py-2 pr-4 font-mono text-theme">{b.consumer}</td>
                                                <td class="py-2 pr-4 font-mono tabular-nums">{format_number(b.hit_tokens)}</td>
                                                <td class="py-2 pr-4 font-mono tabular-nums">{format_number(b.miss_tokens)}</td>
                                                <td class="py-2 font-mono tabular-nums text-accent">
                                                    {format!("{:.1}%", b.hit_ratio * 100.0)}
                                                </td>
                                                <td class="py-2">
                                                    <Sparkline
                                                        values=spark_values
                                                        color="var(--cc-accent)"
                                                        width=56
                                                        height=20
                                                    />
                                                </td>
                                            </tr>
                                        }
                                    }).collect::<Vec<_>>()}
                                </tbody>
                            </table>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
pub fn CacheHitSection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let d = metrics.tier_deltas_5m;
    let total = (d.l0 + d.l1 + d.l2 + d.miss).max(1) as f64;
    let hit_rate = (d.l0 + d.l1 + d.l2) as f64 / total * 100.0;

    let segments = vec![
        crate::components::donut_chart::DonutSegment {
            label: crate::locale::Translations::overview_l0_label().to_string(),
            value: d.l0 as f64,
            color: "var(--cc-tier-l0)",
        },
        crate::components::donut_chart::DonutSegment {
            label: crate::locale::Translations::overview_l1_label().to_string(),
            value: d.l1 as f64,
            color: "var(--cc-tier-l1)",
        },
        crate::components::donut_chart::DonutSegment {
            label: crate::locale::Translations::overview_l2_label().to_string(),
            value: d.l2 as f64,
            color: "var(--cc-tier-l2)",
        },
        crate::components::donut_chart::DonutSegment {
            label: t.overview_miss_label().to_string(),
            value: d.miss as f64,
            color: "var(--cc-tier-miss)",
        },
    ];

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-1">{t.overview_gateway_cache_title()}</h3>
            <p class="text-xs text-theme-muted mb-4">{t.overview_tier_5m_hint()}</p>
            <div class="flex justify-center">
                <crate::components::donut_chart::DonutChart
                    segments=segments
                    center_label=format!("{:.1}%", hit_rate)
                    size=160
                />
            </div>
        </div>
    }
}

#[component]
pub fn CostSavingsSection(ops: OverviewOpsMetrics) -> impl IntoView {
    let t = use_translations();

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-1">{t.overview_cost_title()}</h3>
            <p class="text-xs text-theme-muted mb-4">{t.overview_cost_pricing_hint()}</p>
            <div class="space-y-4">
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_cost_saved_total()}</div>
                    <div class="text-2xl font-mono tabular-nums text-warning font-semibold">
                        {format!("${:.4}", ops.cost_saved_usd_total)}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_cost_saved_5m()}</div>
                    <div class="text-xl font-mono tabular-nums text-accent">
                        {format!("${:.4}", ops.cost_saved_usd_5m)}
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn UpstreamKeyStrip(ops: OverviewOpsMetrics) -> impl IntoView {
    let t = use_translations();
    let locale = crate::locale::use_locale();
    let hint = ops
        .upstream_default_profile_id
        .as_deref()
        .map(|id| t.overview_upstream_keys_hint(id))
        .unwrap_or_else(|| match locale.get() {
            crate::locale::Locale::ZhCN => "与上游配置页默认 Profile 的 Key 池一致".to_string(),
            crate::locale::Locale::EnUS => {
                "Matches the default profile key pool on Upstream page".to_string()
            }
        });
    view! {
        <div class="glass-card h-full flex flex-col justify-between">
            <div>
                <h3 class="text-sm font-semibold text-theme mb-2">{t.overview_upstream_keys_strip()}</h3>
                <div class="text-3xl font-mono tabular-nums text-accent">
                    {format!("{}/{}", ops.upstream_keys_available, ops.upstream_key_count)}
                </div>
                <p class="text-xs text-theme-muted mt-2">{hint}</p>
            </div>
            <a href="/upstream" class="btn btn-secondary text-xs mt-4 w-fit">
                {t.overview_upstream_keys_link()}
            </a>
        </div>
    }
}

#[component]
pub fn PrefixHealthCard(ops: OverviewOpsMetrics) -> impl IntoView {
    let t = use_translations();
    let reasoning_total = ops.reasoning_store_hits + ops.reasoning_store_misses;
    let reasoning_hit_pct = if reasoning_total > 0 {
        ops.reasoning_store_hits as f64 / reasoning_total as f64 * 100.0
    } else {
        0.0
    };

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-1">{t.overview_prefix_health_title()}</h3>
            <p class="text-xs text-theme-muted mb-4">{t.overview_prefix_health_desc()}</p>
            <div class="grid grid-cols-2 gap-3 text-sm font-mono tabular-nums">
                <div>
                    <span class="text-xs text-theme-muted block">"prefix_break"</span>
                    <span class="text-warning">{ops.prefix_break_total}</span>
                </div>
                <div>
                    <span class="text-xs text-theme-muted block">"sse_omitted"</span>
                    <span>{ops.stream_cache_sse_omitted}</span>
                </div>
                <div>
                    <span class="text-xs text-theme-muted block">"reasoning hit"</span>
                    <span class="text-accent">{ops.reasoning_store_hits}</span>
                </div>
                <div>
                    <span class="text-xs text-theme-muted block">"reasoning miss"</span>
                    <span>{ops.reasoning_store_misses}</span>
                </div>
            </div>
            <p class="text-xs text-theme-muted mt-3">
                {format!("reasoning store hit {:.1}%", reasoning_hit_pct)}
            </p>
            <a href="/cache" class="text-xs text-accent hover:underline mt-2 inline-block">
                "Cache / reasoning →"
            </a>
        </div>
    }
}

#[component]
fn ObservabilityFooter() -> impl IntoView {
    let t = use_translations();
    let metrics_host =
        option_env!("CRABCACHE_GATEWAY_METRICS_URL").unwrap_or("http://127.0.0.1:9090/metrics");

    view! {
        <div class="flex flex-wrap items-center justify-between gap-3 text-xs text-theme-muted pt-2 border-t border-theme">
            <a
                href="/docs/OBSERVABILITY.md"
                target="_blank"
                rel="noopener noreferrer"
                class="text-accent hover:underline"
            >
                {t.overview_observability_doc()}
            </a>
            <span class="font-mono truncate" title=metrics_host>
                "Prometheus: " {metrics_host}
            </span>
        </div>
    }
}

/// Data Plane diagnostics section embedded in the overview page.
/// Shows phase-level latency breakdown, error attribution, and trace errors.
#[component]
fn DataPlaneDiagnostics() -> impl IntoView {
    let t = use_translations();
    let phases = RwSignal::new(serde_json::Value::Null);
    let errors = RwSignal::new(serde_json::Value::Null);
    let fetch_error = RwSignal::new(None::<String>);
    let is_loaded = RwSignal::new(false);

    let fetch = {
        let phases = phases;
        let errors = errors;
        let fetch_error = fetch_error;
        let is_loaded = is_loaded;
        move || {
            let phases = phases;
            let errors = errors;
            let fetch_error = fetch_error;
            let is_loaded = is_loaded;
            leptos::task::spawn_local(async move {
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/phases").await {
                    Ok(data) => phases.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/errors").await {
                    Ok(data) => errors.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                is_loaded.set(true);
            });
        }
    };

    fetch();

    let fetch = fetch;
    leptos::task::spawn_local(async move {
        loop {
            TimeoutFuture::new(30_000).await;
            fetch();
        }
    });

    view! {
        <div id="ov-diag" class="overview-section-anchor overview-diag-compact mt-3 border-t border-theme pt-3">
            <div class="flex items-baseline gap-2 mb-2">
                <h2 class="text-sm font-semibold text-theme">{t.dataplane_page_title()}</h2>
                <span class="text-[10px] text-theme-muted">{t.dataplane_page_desc()}</span>
            </div>

            {move || {
                if let Some(ref err) = fetch_error.get() {
                    return view! { <div class="alert alert-error text-sm">{err.clone()}</div> }.into_any();
                }
                view! {}.into_any()
            }}

            <div class="diag-card-grid grid grid-cols-1 xl:grid-cols-3 gap-2 mb-2 items-stretch">
                // Phase Latency card
                <div class="dash-card">
                    <div class="dash-card-header">
                        <span class="dash-card-title">{t.dataplane_phase_latency_ms()}</span>
                    </div>
                    <div class="dash-card-body diag-card-body overflow-x-auto">
                        {move || {
                            if !is_loaded.get() {
                                return view! {
                                    <div class="p-4 space-y-2">
                                        {(0..5).map(|_| view! {
                                            <div class="dp-skeleton-row">
                                                <div class="skeleton-block" style="width:6rem;height:0.75rem"></div>
                                                <div class="dp-skeleton-bar" style="height:0.625rem"></div>
                                                <div class="skeleton-block" style="width:3rem;height:0.75rem"></div>
                                            </div>
                                        }).collect_view()}
                                    </div>
                                }.into_any();
                            }
                            let p = phases.get();
                            let phase_map = p.get("phases").and_then(|v| v.as_object()).cloned().unwrap_or_default();
                            let mut phase_names: Vec<String> = phase_map.keys().map(|k| k.clone()).collect();
                            phase_names.sort();
                            if phase_names.is_empty() {
                                return view! { <p class="text-theme-muted text-sm italic p-4">{t.dataplane_no_phase_data()}</p> }.into_any();
                            }
                            let phase_summaries: Vec<_> = phase_names.iter().map(|name| {
                                let pd = phase_map.get(name).and_then(|v| v.as_object());
                                (name.clone(), pd)
                            }).filter(|(_, pd)| {
                                pd.as_ref().is_some_and(|m| {
                                    ["p50_ms", "p95_ms", "p99_ms"].iter().any(|k| {
                                        m.get(*k).and_then(|v| v.as_f64()).is_some_and(|v| v > 0.0)
                                    })
                                })
                            }).collect();
                            view! {
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th>{t.dataplane_phase()}</th>
                                            <th style="text-align:right">{t.dataplane_p50_ms()}</th>
                                            <th style="text-align:right">{t.dataplane_p95_ms()}</th>
                                            <th style="text-align:right">{t.dataplane_p99_ms()}</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {phase_summaries.into_iter().map(|(name, pd)| {
                                            let p50 = pd.and_then(|m| m.get("p50_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                            let p95 = pd.and_then(|m| m.get("p95_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                            let p99 = pd.and_then(|m| m.get("p99_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                            let bar_w = p95.parse::<f64>().ok().map(|v| (v / 1000.0).min(1.0) * 100.0).unwrap_or(0.0);
                                            view! {
                                                <tr>
                                                    <td style="font-family:var(--font-mono);font-size:0.75rem">{name}</td>
                                                    <td style="text-align:right;font-family:var(--font-mono);font-size:0.75rem">{p50}</td>
                                                    <td style="text-align:right;font-family:var(--font-mono);font-size:0.75rem;color:var(--cc-info);font-weight:500">
                                                        <div style="display:flex;align-items:center;justify-content:flex-end;gap:0.5rem">
                                                            <div style="width:3rem;height:0.375rem;background:var(--cc-bg-elevated);border-radius:9999px;overflow:hidden">
                                                                <div style={format!("height:100%;background:var(--cc-info);border-radius:9999px;width:{}%",bar_w)}></div>
                                                            </div>
                                                            {p95}
                                                        </div>
                                                    </td>
                                                    <td style="text-align:right;font-family:var(--font-mono);font-size:0.75rem">{p99}</td>
                                                </tr>
                                            }.into_any()
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            }.into_any()
                        }}
                    </div>
                </div>

                <div class="dash-card">
                    <div class="dash-card-header">
                        <span class="dash-card-title">{t.dataplane_rejection_reasons_total()}</span>
                    </div>
                    <div class="dash-card-body diag-card-body">
                            {move || {
                                if !is_loaded.get() {
                                    return view! {
                                        <div class="p-3 space-y-2">
                                            {(0..4).map(|_| view! {
                                                <div class="dp-skeleton-row">
                                                    <div class="skeleton-block" style="width:8rem;height:0.75rem"></div>
                                                    <div class="dp-skeleton-bar" style="height:0.875rem"></div>
                                                    <div class="skeleton-block" style="width:2.5rem;height:0.75rem"></div>
                                                </div>
                                            }).collect_view()}
                                        </div>
                                    }.into_any();
                                }
                                let e = errors.get();
                                let reasons = e.get("rejection_reasons").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                                if reasons.is_empty() {
                                    return view! { <p class="text-theme-muted text-xs italic py-1">{t.dataplane_no_rejections()}</p> }.into_any();
                                }
                                let max_count = reasons.iter().filter_map(|r| r.get("count").and_then(|c| c.as_u64())).max().unwrap_or(1);
                                view! {
                                    <div style="display:flex;flex-direction:column;gap:0.5rem">
                                        {reasons.into_iter().map(|r| {
                                            let reason = r.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                            let count = r.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                                            view! {
                                                <div style="display:flex;align-items:center;gap:0.5rem">
                                                    <span style="font-size:0.75rem;font-family:var(--font-mono);color:var(--cc-text-muted);width:8rem;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title=reason.clone()>{reason.clone()}</span>
                                                    <div style="flex:1;height:0.875rem;background:var(--cc-bg-elevated);border-radius:9999px;overflow:hidden">
                                                        <div style={format!("height:100%;background:var(--cc-error);border-radius:9999px;width:{}%",(count as f64/max_count as f64)*100.0)}></div>
                                                    </div>
                                                    <span style="font-size:0.75rem;font-family:var(--font-mono);color:var(--cc-text);width:3.5rem;text-align:right">{count}</span>
                                                </div>
                                            }.into_any()
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }}
                        </div>
                    </div>

                <div class="dash-card">
                    <div class="dash-card-header">
                        <span class="dash-card-title">{t.dataplane_error_sources_total()}</span>
                    </div>
                    <div class="dash-card-body diag-card-body">
                            {move || {
                                if !is_loaded.get() {
                                    return view! {
                                        <div class="p-3 space-y-2">
                                            {(0..4).map(|_| view! {
                                                <div class="dp-skeleton-row">
                                                    <div class="skeleton-block" style="width:8rem;height:0.75rem"></div>
                                                    <div class="dp-skeleton-bar" style="height:0.875rem"></div>
                                                    <div class="skeleton-block" style="width:2.5rem;height:0.75rem"></div>
                                                </div>
                                            }).collect_view()}
                                        </div>
                                    }.into_any();
                                }
                                let e = errors.get();
                                let sources = e.get("error_sources").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                                if sources.is_empty() {
                                    return view! { <p class="text-theme-muted text-xs italic py-1">{t.dataplane_no_error_sources()}</p> }.into_any();
                                }
                                let max_count = sources.iter().filter_map(|s| s.get("count").and_then(|c| c.as_u64())).max().unwrap_or(1);
                                view! {
                                    <div style="display:flex;flex-direction:column;gap:0.5rem">
                                        {sources.into_iter().map(|s| {
                                            let source = s.get("source").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                            let count = s.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                                            view! {
                                                <div style="display:flex;align-items:center;gap:0.5rem">
                                                    <span style="font-size:0.75rem;font-family:var(--font-mono);color:var(--cc-text-muted);width:8rem;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title=source.clone()>{source.clone()}</span>
                                                    <div style="flex:1;height:0.875rem;background:var(--cc-bg-elevated);border-radius:9999px;overflow:hidden">
                                                        <div style={format!("height:100%;background:var(--cc-warning);border-radius:9999px;width:{}%",(count as f64/max_count as f64)*100.0)}></div>
                                                    </div>
                                                    <span style="font-size:0.75rem;font-family:var(--font-mono);color:var(--cc-text);width:3.5rem;text-align:right">{count}</span>
                                                </div>
                                            }.into_any()
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }}
                        </div>
                    </div>
            </div>

            // Trace Error Top 10 — full-width
            {move || {
                let e = errors.get();
                let trace_errors = e.get("trace_errors_top10").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                if trace_errors.is_empty() {
                    return view! {}.into_any();
                }
                view! {
                    <div class="dash-card dash-card-flush">
                        <div class="dash-card-header">
                            <span class="dash-card-title">{t.dataplane_pg_trace_errors_1h()}</span>
                        </div>
                        <div class="dash-card-body-flush overflow-x-auto">
                            <table class="table">
                                <thead>
                                    <tr>
                                        <th>{t.dataplane_error_code()}</th>
                                        <th>{t.dataplane_status()}</th>
                                        <th>{t.dataplane_upstream_result()}</th>
                                        <th style="text-align:right">{t.dataplane_count()}</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {trace_errors.into_iter().map(|row| {
                                        let error_code = row.get("error_code").and_then(|v| v.as_str()).unwrap_or("—").to_string();
                                        let status_code = row.get("status_code").and_then(|v| v.as_i64()).map(|v| format!("{}", v)).unwrap_or_else(|| "—".to_string());
                                        let upstream_result = row.get("upstream_result").and_then(|v| v.as_str()).unwrap_or("—").to_string();
                                        let count = row.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                                        view! {
                                            <tr>
                                                <td style="color:var(--cc-error);font-weight:500">{error_code}</td>
                                                <td>{status_code}</td>
                                                <td>{upstream_result}</td>
                                                <td style="text-align:right">{count}</td>
                                            </tr>
                                        }.into_any()
                                    }).collect::<Vec<_>>()}
                                </tbody>
                            </table>
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
pub fn LatencySection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    // (label, avg_ms, p99_ms)
    let stages: Vec<(&str, f64, f64)> = vec![
        (
            crate::locale::Translations::overview_latency_l0(),
            metrics.latency_l0_ms,
            0.0,
        ),
        (
            crate::locale::Translations::overview_latency_l1(),
            metrics.latency_l1_ms,
            0.0,
        ),
        (
            crate::locale::Translations::overview_latency_l2(),
            metrics.latency_l2_ms,
            0.0,
        ),
        (
            t.overview_latency_upstream(),
            metrics.latency_upstream_ms,
            metrics.latency_upstream_p99_ms,
        ),
        ("MiMo prefill (hdr)", 0.0, metrics.latency_prefill_p99_ms),
    ];

    let max_latency = stages
        .iter()
        .map(|&(_, avg, p99)| avg.max(p99))
        .fold(0.0f64, f64::max)
        .max(1.0);

    // SLO thresholds (ms)
    let upstream_slo = 2000.0_f64;

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_latency_title()}</h3>
            <div class="space-y-3">
                {stages.into_iter().map(|(label, avg, p99)| {
                    let pct = avg / max_latency * 100.0;
                    let over_slo = label == t.overview_latency_upstream() && p99 > upstream_slo;
                    let bar_class = if over_slo { "progress-bar-fill bg-warning" } else { "progress-bar-fill" };
                    view! {
                        <div>
                            <div class="flex items-center gap-3">
                                <span class="w-20 text-xs text-theme-secondary shrink-0">{label}</span>
                                <div class="flex-1 progress-bar h-2">
                                    <div
                                        class=bar_class
                                        style=format!("width: {}%", pct.min(100.0))
                                    ></div>
                                </div>
                                <span class="w-20 text-xs font-mono tabular-nums text-theme text-right">
                                    {format!("{:.1}ms", avg)}
                                </span>
                            </div>
                            {if p99 > 0.0 {
                                view! {
                                    <div class="flex items-center gap-3 mt-1">
                                        <span class="w-20 text-xs text-theme-muted shrink-0 text-right">"P99"</span>
                                        <div class="flex-1 text-xs font-mono tabular-nums text-theme-muted">
                                            {format!("{:.1}ms", p99)}
                                            {if over_slo {
                                                view! { <span class="text-warning ml-2">"over SLO"</span> }.into_any()
                                            } else {
                                                ().into_any()
                                            }}
                                        </div>
                                    </div>
                                }.into_any()
                            } else {
                                ().into_any()
                            }}
                        </div>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}
