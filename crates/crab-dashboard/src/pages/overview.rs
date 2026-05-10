use leptos::prelude::*;
use gloo_timers::future::TimeoutFuture;

use crate::api;
use crate::components::ui::*;
use crate::locale::{use_translations, Translations};
use crate::types::MetricsSnapshot;

#[component]
pub fn OverviewPage() -> impl IntoView {
    let t = use_translations();
    let metrics: RwSignal<Option<Result<MetricsSnapshot, String>>> = RwSignal::new(None);
    let auto_refresh = RwSignal::new(true);
    let last_update = RwSignal::new(String::new());

    let load_metrics = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_metrics().await {
                Ok(m) => {
                    metrics.set(Some(Ok(m)));
                    last_update.set(chrono::Local::now().format("%H:%M:%S").to_string());
                }
                Err(e) => metrics.set(Some(Err(e))),
            }
        });
    };

    load_metrics();

    leptos::task::spawn_local(async move {
        loop {
            TimeoutFuture::new(5000).await;
            if auto_refresh.get() {
                load_metrics();
            }
        }
    });

    view! {
        <div class="p-6 space-y-6">
            <div class="flex items-center justify-between">
                <SectionHeader
                    title=t.overview_title()
                    description=t.overview_desc()
                />
                <div class="flex items-center gap-3">
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
                        {t.overview_auto_refresh()}
                    </label>
                    <button
                        on:click=move |_| load_metrics()
                        class="btn btn-secondary text-xs"
                    >
                        {t.overview_refresh()}
                    </button>
                </div>
            </div>

            {move || match metrics.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().overview_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(m)) => view! {
                    <div class="space-y-6">
                        <MetricsBento metrics=m.clone() />
                        <TokenStats metrics=m.clone() />
                        <TimeSeriesChart metrics=m.clone() />
                        <div class="bento-grid-3">
                            <div class="bento-cell">
                                <CacheHitSection metrics=m.clone() />
                            </div>
                            <div class="bento-cell">
                                <CostSavingsSection metrics=m.clone() />
                            </div>
                            <div class="bento-cell">
                                <LatencySection metrics=m />
                            </div>
                        </div>
                    </div>
                }.into_any(),
            }}
        </div>
    }
}

#[component]
fn MetricsBento(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let total_hits = metrics.l0_hits + metrics.l1_hits + metrics.l2_hits;
    let total_requests = total_hits + metrics.cache_misses;
    let hit_rate = if total_requests > 0 {
        total_hits as f64 / total_requests as f64 * 100.0
    } else {
        0.0
    };

    view! {
        <div class="bento-grid">
            <div class="bento-cell-hero">
                <div class="metric-card h-full">
                    <div class="flex items-start justify-between mb-4">
                        <div>
                            <div class="metric-card-label">{Translations::overview_qps()}</div>
                            <div class="metric-card-value">
                                {format!("{:.2}", metrics.qps)}
                            </div>
                        </div>
                        <div class="text-3xl opacity-30">"⚡"</div>
                    </div>
                    <div class="grid grid-cols-2 gap-4 mt-auto">
                        <div>
                            <div class="text-xs text-theme-muted mb-1">{Translations::overview_tps()}</div>
                            <div class="text-lg font-mono tabular-nums text-theme">
                                {format!("{:.2}", metrics.tps)}
                            </div>
                        </div>
                        <div>
                            <div class="text-xs text-theme-muted mb-1">{t.overview_hit_rate()}</div>
                            <div class="text-lg font-mono tabular-nums text-accent font-semibold">
                                {format!("{:.1}%", hit_rate)}
                            </div>
                        </div>
                    </div>
                </div>
            </div>
            <div class="bento-cell">
                <div class="metric-card h-full">
                    <div class="flex items-start justify-between mb-3">
                        <div class="metric-card-label">{t.overview_active_keys()}</div>
                        <div class="text-2xl opacity-30">"🔑"</div>
                    </div>
                    <div class="metric-card-value">
                        {format!("{}", metrics.active_keys)}
                    </div>
                    <div class="metric-card-sub">{t.overview_active_keys_sub()}</div>
                    <div class="mt-3 flex items-center gap-2">
                        <div class="online-dot"></div>
                        <span class="online-label">"Active"</span>
                    </div>
                </div>
            </div>
            <div class="bento-cell">
                <div class="metric-card h-full">
                    <div class="flex items-start justify-between mb-3">
                        <div class="metric-card-label">{t.overview_uptime()}</div>
                        <div class="text-2xl opacity-30">"⏱"</div>
                    </div>
                    <div class="metric-card-value">
                        {format!("{}h", metrics.uptime_hours)}
                    </div>
                    <div class="metric-card-sub">{t.overview_uptime_sub()}</div>
                    <div class="mt-3 pt-3 border-t border-theme">
                        <div class="flex justify-between text-xs">
                            <span class="text-theme-muted">"Cache Hits"</span>
                            <span class="font-mono tabular-nums text-accent">
                                {format!("{}", total_hits)}
                            </span>
                        </div>
                    </div>
                </div>
            </div>
            <div class="bento-cell">
                <div class="metric-card h-full">
                    <div class="flex items-start justify-between mb-3">
                        <div class="metric-card-label">"Cache Tokens"</div>
                        <div class="text-2xl opacity-30">"💾"</div>
                    </div>
                    <div class="space-y-2">
                        <div class="flex justify-between items-baseline">
                            <span class="text-xs text-theme-muted">"Hit"</span>
                            <span class="text-lg font-mono tabular-nums text-accent">
                                {format!("{}", metrics.cache_hit_tokens)}
                            </span>
                        </div>
                        <div class="flex justify-between items-baseline">
                            <span class="text-xs text-theme-muted">"Miss"</span>
                            <span class="text-lg font-mono tabular-nums text-theme">
                                {format!("{}", metrics.cache_miss_tokens)}
                            </span>
                        </div>
                    </div>
                    <div class="mt-3 pt-3 border-t border-theme">
                        <div class="progress-bar h-2">
                            <div 
                                class="progress-bar-fill"
                                style=format!("width: {}%", if metrics.cache_hit_tokens + metrics.cache_miss_tokens > 0 {
                                    metrics.cache_hit_tokens as f64 / (metrics.cache_hit_tokens + metrics.cache_miss_tokens) as f64 * 100.0
                                } else { 0.0 })
                            ></div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn TokenStats(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-sm font-semibold text-theme">{t.overview_token_stats()}</h3>
                <div class="text-2xl opacity-30">"📊"</div>
            </div>
            <div class="grid grid-cols-3 gap-6">
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
            </div>
        </div>
    }
}

#[component]
fn TimeSeriesChart(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let selected_view = RwSignal::new("hourly".to_string());
    
    let current_data = move || {
        match selected_view.get().as_str() {
            "hourly" => metrics.hourly_stats.clone(),
            "daily" => metrics.daily_stats.clone(),
            "weekly" => metrics.weekly_stats.clone(),
            "monthly" => metrics.monthly_stats.clone(),
            _ => metrics.hourly_stats.clone(),
        }
    };

    view! {
        <div class="glass-card">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-sm font-semibold text-theme">{t.overview_usage_trends()}</h3>
                <div class="flex gap-2">
                    <button
                        on:click=move |_| selected_view.set("hourly".to_string())
                        class=move || {
                            if selected_view.get() == "hourly" {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_hourly()}
                    </button>
                    <button
                        on:click=move |_| selected_view.set("daily".to_string())
                        class=move || {
                            if selected_view.get() == "daily" {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_daily()}
                    </button>
                    <button
                        on:click=move |_| selected_view.set("weekly".to_string())
                        class=move || {
                            if selected_view.get() == "weekly" {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_weekly()}
                    </button>
                    <button
                        on:click=move |_| selected_view.set("monthly".to_string())
                        class=move || {
                            if selected_view.get() == "monthly" {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.overview_monthly()}
                    </button>
                </div>
            </div>
            
            <div class="space-y-4">
                {move || {
                    let data = current_data();
                    let t = use_translations();
                    if data.is_empty() {
                        view! {
                            <div class="text-center py-8 text-theme-muted text-sm">
                                {t.overview_no_data()}
                            </div>
                        }.into_any()
                    } else {
                        let max_tokens = data.iter().map(|d| d.tokens).max().unwrap_or(1);
                        view! {
                            <div class="space-y-3">
                                {data.into_iter().map(|point| {
                                    let pct = point.tokens as f64 / max_tokens as f64 * 100.0;
                                    let t = use_translations();
                                    view! {
                                        <div class="flex items-center gap-3">
                                            <span class="w-20 text-xs text-theme-secondary font-mono">
                                                {point.timestamp}
                                            </span>
                                            <div class="flex-1">
                                                <div class="progress-bar h-6">
                                                    <div
                                                        class="progress-bar-fill flex items-center justify-end pr-2"
                                                        style=format!("width: {}%", pct.min(100.0))
                                                    >
                                                        <span class="text-xs font-mono tabular-nums text-theme">
                                                            {format_number(point.tokens)}
                                                        </span>
                                                    </div>
                                                </div>
                                            </div>
                                            <div class="w-24 text-right">
                                                <div class="text-xs text-theme-muted">
                                                    {format!("{} {}", point.requests, t.overview_requests())}
                                                </div>
                                                <div class="text-xs text-accent">
                                                    {format!("{} {}", point.cache_hits, t.overview_hits())}
                                                </div>
                                            </div>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

#[component]
fn CacheHitSection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let total = (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits + metrics.cache_misses).max(1);

    let l0 = RwSignal::new(metrics.l0_hits as f64);
    let l1 = RwSignal::new(metrics.l1_hits as f64);
    let l2 = RwSignal::new(metrics.l2_hits as f64);
    let miss = RwSignal::new(metrics.cache_misses as f64);

    let hit_rate = RwSignal::new(
        (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) as f64 / total as f64 * 100.0
    );

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_cache_hit_title()}</h3>
            <div class="space-y-3">
                <ProgressBar label=crate::locale::Translations::overview_l0_label() value=l0.into() max=total as f64 />
                <ProgressBar label=crate::locale::Translations::overview_l1_label() value=l1.into() max=total as f64 />
                <ProgressBar label=crate::locale::Translations::overview_l2_label() value=l2.into() max=total as f64 />
                <ProgressBar label=t.overview_miss_label() value=miss.into() max=total as f64 />
            </div>
            <div class="mt-4 pt-3 border-t border-theme flex justify-between text-sm">
                <span class="text-theme-secondary">{t.overview_hit_rate()}</span>
                <span class="font-mono tabular-nums text-accent font-semibold">
                    {move || format!("{:.1}%", hit_rate.get())}
                </span>
            </div>
        </div>
    }
}

#[component]
fn CostSavingsSection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let direct_cost = metrics.cache_miss_tokens as f64 * 0.14 / 1_000_000.0
        + metrics.cache_hit_tokens as f64 * 0.14 / 1_000_000.0;
    let actual_cost = metrics.cache_miss_tokens as f64 * 0.14 / 1_000_000.0
        + metrics.cache_hit_tokens as f64 * 0.014 / 1_000_000.0;
    let saved = direct_cost - actual_cost;
    let saved_pct = if direct_cost > 0.0 { saved / direct_cost * 100.0 } else { 0.0 };

    let direct = RwSignal::new(format!("${:.2}", direct_cost));
    let actual = RwSignal::new(format!("${:.2}", actual_cost));
    let saved_str = RwSignal::new(format!("${:.2}", saved));
    let pct = RwSignal::new(saved_pct);

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_cost_title()}</h3>
            <div class="grid grid-cols-2 gap-4 mb-4">
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_cost_standard()}</div>
                    <div class="text-xl font-mono tabular-nums text-theme">
                        {move || direct.get()}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.overview_cost_with_cache()}</div>
                    <div class="text-xl font-mono tabular-nums text-accent">
                        {move || actual.get()}
                    </div>
                </div>
            </div>
            <div class="pt-3 border-t border-theme flex justify-between items-center">
                <span class="text-sm text-theme-secondary">{t.overview_cost_saved()}</span>
                <div class="text-right">
                    <span class="text-lg font-mono tabular-nums text-warning font-semibold">
                        {move || saved_str.get()}
                    </span>
                    <span class="ml-2 text-xs text-warning">
                        {move || format!("({:.1}%)", pct.get())}
                    </span>
                </div>
            </div>
        </div>
    }
}

#[component]
fn LatencySection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let stages: Vec<(&str, f64)> = vec![
        (crate::locale::Translations::overview_latency_l0(), metrics.latency_l0_ms),
        (crate::locale::Translations::overview_latency_l1(), metrics.latency_l1_ms),
        (crate::locale::Translations::overview_latency_l2(), metrics.latency_l2_ms),
        (t.overview_latency_upstream(), metrics.latency_upstream_ms),
    ];

    let max_latency = stages.iter().map(|&(_, v)| v).fold(0.0f64, f64::max).max(1.0);

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_latency_title()}</h3>
            <div class="space-y-3">
                {stages.into_iter().map(|(label, value)| {
                    let pct = value / max_latency * 100.0;
                    view! {
                        <div class="flex items-center gap-3">
                            <span class="w-20 text-xs text-theme-secondary shrink-0">{label}</span>
                            <div class="flex-1 progress-bar h-2">
                                <div
                                    class="progress-bar-fill"
                                    style=format!("width: {}%", pct.min(100.0))
                                ></div>
                            </div>
                            <span class="w-16 text-xs font-mono tabular-nums text-theme text-right">
                                {format!("{:.1}ms", value)}
                            </span>
                        </div>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}