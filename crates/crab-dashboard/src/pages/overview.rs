use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::{use_translations, Translations};
use crate::types::MetricsSnapshot;

#[component]
pub fn OverviewPage() -> impl IntoView {
    let t = use_translations();
    let metrics: RwSignal<Option<Result<MetricsSnapshot, String>>> = RwSignal::new(None);

    leptos::task::spawn_local(async move {
        match api::fetch_metrics().await {
            Ok(m) => metrics.set(Some(Ok(m))),
            Err(e) => metrics.set(Some(Err(e))),
        }
    });

    view! {
        <div class="p-6 space-y-6">
            <SectionHeader
                title=t.overview_title()
                description=t.overview_desc()
            />

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
    let qps = RwSignal::new(format!("{}", metrics.qps));
    let tps = RwSignal::new(format!("{}", metrics.tps));
    let active_keys = RwSignal::new(format!("{}", metrics.active_keys));
    let uptime = RwSignal::new(format!("{}h", metrics.uptime_hours));

    view! {
        <div class="bento-grid">
            <div class="bento-cell-hero">
                <MetricCard title=Translations::overview_qps() value=qps.into() subtitle=t.overview_qps_sub() />
            </div>
            <div class="bento-cell">
                <MetricCard title=Translations::overview_tps() value=tps.into() subtitle=t.overview_tps_sub() />
            </div>
            <div class="bento-cell">
                <MetricCard title=t.overview_active_keys() value=active_keys.into() subtitle=t.overview_active_keys_sub() />
            </div>
            <div class="bento-cell">
                <MetricCard title=t.overview_uptime() value=uptime.into() subtitle=t.overview_uptime_sub() />
            </div>
        </div>
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