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
                    <div class="bg-rose-500/10 border border-rose-500/20 rounded-lg p-4 text-rose-400 text-sm">
                        {format!("{}: {}", use_translations().overview_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(m)) => view! {
                    <div class="space-y-6">
                        <MetricsGrid metrics=m.clone() />
                        <CacheHitSection metrics=m.clone() />
                        <CostSavingsSection metrics=m.clone() />
                        <LatencySection metrics=m />
                    </div>
                }.into_any(),
            }}
        </div>
    }
}

#[component]
fn MetricsGrid(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let qps = RwSignal::new(format!("{}", metrics.qps));
    let tps = RwSignal::new(format!("{}", metrics.tps));
    let active_keys = RwSignal::new(format!("{}", metrics.active_keys));
    let uptime = RwSignal::new(format!("{}h", metrics.uptime_hours));

    view! {
        <div class="grid grid-cols-4 gap-4">
            <MetricCard title=Translations::overview_qps() value=qps.into() subtitle=t.overview_qps_sub() accent="teal" />
            <MetricCard title=Translations::overview_tps() value=tps.into() subtitle=t.overview_tps_sub() accent="amber" />
            <MetricCard title=t.overview_active_keys() value=active_keys.into() subtitle=t.overview_active_keys_sub() accent="violet" />
            <MetricCard title=t.overview_uptime() value=uptime.into() subtitle=t.overview_uptime_sub() accent="rose" />
        </div>
    }
}

#[component]
fn CacheHitSection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let total = metrics.l0_hits + metrics.l1_hits + metrics.l2_hits + metrics.cache_misses;
    let total = if total == 0 { 1 } else { total };

    let l0_pct = RwSignal::new(metrics.l0_hits as f64);
    let l1_pct = RwSignal::new(metrics.l1_hits as f64);
    let l2_pct = RwSignal::new(metrics.l2_hits as f64);
    let miss_pct = RwSignal::new(metrics.cache_misses as f64);

    let hit_rate = RwSignal::new(
        if total > 0 {
            (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) as f64 / total as f64 * 100.0
        } else {
            0.0
        }
    );

    view! {
        <div class="bg-stone-900 border border-stone-800 rounded-lg p-5">
            <h3 class="text-sm font-semibold text-stone-200 mb-4">{t.overview_cache_hit_title()}</h3>
            <div class="space-y-3">
                <ProgressBar label=crate::locale::Translations::overview_l0_label() value=l0_pct.into() max=total as f64 color="teal" />
                <ProgressBar label=crate::locale::Translations::overview_l1_label() value=l1_pct.into() max=total as f64 color="amber" />
                <ProgressBar label=crate::locale::Translations::overview_l2_label() value=l2_pct.into() max=total as f64 color="violet" />
                <ProgressBar label=t.overview_miss_label() value=miss_pct.into() max=total as f64 color="rose" />
            </div>
            <div class="mt-4 pt-3 border-t border-stone-800 flex justify-between text-sm">
                <span class="text-stone-400">{t.overview_hit_rate()}</span>
                <span class="font-mono tabular-nums text-teal-400 font-semibold">
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
        <div class="bg-stone-900 border border-stone-800 rounded-lg p-5">
            <h3 class="text-sm font-semibold text-stone-200 mb-4">{t.overview_cost_title()}</h3>
            <div class="grid grid-cols-2 gap-4">
                <div>
                    <div class="text-xs text-stone-500 mb-1">{t.overview_cost_standard()}</div>
                    <div class="text-xl font-mono tabular-nums text-stone-300">
                        {move || direct.get()}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-stone-500 mb-1">{t.overview_cost_with_cache()}</div>
                    <div class="text-xl font-mono tabular-nums text-teal-400">
                        {move || actual.get()}
                    </div>
                </div>
            </div>
            <div class="mt-4 pt-3 border-t border-stone-800">
                <div class="flex justify-between items-center">
                    <span class="text-sm text-stone-400">{t.overview_cost_saved()}</span>
                    <div class="text-right">
                        <span class="text-lg font-mono tabular-nums text-amber-400 font-semibold">
                            {move || saved_str.get()}
                        </span>
                        <span class="ml-2 text-xs text-amber-500">
                            {move || format!("({:.1}%)", pct.get())}
                        </span>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn LatencySection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let stages: Vec<(&str, f64, &str)> = vec![
        (crate::locale::Translations::overview_latency_l0(), metrics.latency_l0_ms, "teal"),
        (crate::locale::Translations::overview_latency_l1(), metrics.latency_l1_ms, "amber"),
        (crate::locale::Translations::overview_latency_l2(), metrics.latency_l2_ms, "violet"),
        (t.overview_latency_upstream(), metrics.latency_upstream_ms, "rose"),
    ];

    let max_latency = stages.iter().map(|(_, v, _)| *v).fold(0.0f64, f64::max).max(1.0);

    view! {
        <div class="bg-stone-900 border border-stone-800 rounded-lg p-5">
            <h3 class="text-sm font-semibold text-stone-200 mb-4">{t.overview_latency_title()}</h3>
            <div class="space-y-3">
                {stages.into_iter().map(|(label, value, color)| {
                    let color_class = match color {
                        "teal" => "bg-teal-500",
                        "amber" => "bg-amber-500",
                        "violet" => "bg-violet-500",
                        "rose" => "bg-rose-500",
                        _ => "bg-teal-500",
                    };
                    let pct = value / max_latency * 100.0;
                    view! {
                        <div class="flex items-center gap-3">
                            <span class="w-20 text-xs text-stone-400 shrink-0">{label}</span>
                            <div class="flex-1 bg-stone-800 rounded-full h-2.5 overflow-hidden">
                                <div
                                    class=format!("h-full rounded-full {}", color_class)
                                    style=format!("width: {}%", pct.min(100.0))
                                ></div>
                            </div>
                            <span class="w-16 text-xs font-mono tabular-nums text-stone-300 text-right">
                                {format!("{:.1}ms", value)}
                            </span>
                        </div>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}