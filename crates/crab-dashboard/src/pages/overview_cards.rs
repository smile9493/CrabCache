//! Overview dashboard card grid with center-modal drill-down.

use leptos::prelude::*;

use crate::api;
use crate::components::donut_chart::{DonutChart, DonutSegment};
use crate::components::horizontal_bar_chart::HorizontalBarChart;
use crate::components::overview_card::OverviewMetricCard;
use crate::components::peak_hours_heatmap::PeakHoursHeatmap;
use crate::components::sparkline::Sparkline;
use crate::components::ui::ProgressBar;
use crate::locale::{Translations, use_translations};
use crate::pages::domains::{DomainDetailDrawer, DomainOverviewTableInline};
use crate::pages::overview::{
    CacheHitSection, CoalescingCard, ConsumerHitTable, LatencySection,
    OpsMetricsRow, OverviewHealthStrip, PrefixCacheCard, PrefixHealthCard, SemanticCacheCard,
    TimeSeriesChart, TokenStats, TraceCompareBanner,
};
use crate::pages::overview_analytics::{InfraOverviewModule, infra_container_headline};
use crate::time_utils::format_number;
use crate::types::{
    GatewayHealth, MetricsSnapshot, OverviewOpsMetrics, OverviewSuggestion,
    PrefixCacheMetricsSnapshot, SemanticConfig, TimeSeriesPoint, TraceSummary,
};
use wasm_bindgen::JsCast;

fn top_consumer_label(metrics: &MetricsSnapshot) -> (String, Vec<f64>) {
    let mut buckets = metrics.consumer_buckets.clone();
    buckets.sort_by(|a, b| (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens)));
    if let Some(top) = buckets.first() {
        let values: Vec<f64> = buckets
            .iter()
            .take(5)
            .map(|b| (b.hit_tokens + b.miss_tokens) as f64)
            .collect();
        (top.consumer.clone(), values)
    } else {
        ("—".to_string(), vec![])
    }
}

fn top_domain_label(metrics: &MetricsSnapshot) -> (String, f64) {
    let mut buckets = metrics.domain_buckets.clone();
    buckets.sort_by(|a, b| (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens)));
    buckets
        .first()
        .map(|d| (d.domain.clone(), d.hit_ratio * 100.0))
        .unwrap_or_else(|| ("—".to_string(), 0.0))
}

#[component]
fn MiniTierDonut(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let d = metrics.tier_deltas_5m;
    let total = (d.l0 + d.l1 + d.l2 + d.miss).max(1) as f64;
    let hit_rate = (d.l0 + d.l1 + d.l2) as f64 / total * 100.0;
    let segments = vec![
        DonutSegment {
            label: Translations::overview_l0_label().to_string(),
            value: d.l0 as f64,
            color: "var(--cc-tier-l0)",
        },
        DonutSegment {
            label: Translations::overview_l1_label().to_string(),
            value: d.l1 as f64,
            color: "var(--cc-tier-l1)",
        },
        DonutSegment {
            label: Translations::overview_l2_label().to_string(),
            value: d.l2 as f64,
            color: "var(--cc-tier-l2)",
        },
        DonutSegment {
            label: t.overview_miss_label(d.miss).to_string(),
            value: d.miss as f64,
            color: "var(--cc-tier-miss)",
        },
    ];
    view! {
        <DonutChart
            segments=segments
            center_label=format!("{hit_rate:.0}%")
            size=52
        />
    }
}

#[component]
pub fn OverviewCardGrid(
    health_memo: Memo<Option<GatewayHealth>>,
    metrics_memo: Memo<Option<MetricsSnapshot>>,
    ops_memo: Memo<Option<OverviewOpsMetrics>>,
    prefix_memo: Memo<Option<PrefixCacheMetricsSnapshot>>,
    semantic_memo: Memo<Option<SemanticConfig>>,
    trace_memo: Memo<Option<TraceSummary>>,
    suggestions_memo: Memo<Option<Vec<OverviewSuggestion>>>,
    ts_points: RwSignal<Vec<TimeSeriesPoint>>,
    ts_window: RwSignal<String>,
    selected_domain: RwSignal<Option<String>>,
    #[prop(optional)] peak_hours_data: Option<RwSignal<crate::types::ModelPeakHoursResponse>>,
    #[prop(optional)] peak_hours_error: Option<RwSignal<Option<String>>>,
) -> impl IntoView {
    let t = use_translations();

    // Keyboard shortcuts for segment navigation (1-4)
    {
        let handler = move |ev: web_sys::KeyboardEvent| {
            if let Some(target) = ev.target() {
                if target.dyn_ref::<web_sys::HtmlInputElement>().is_some() {
                    return;
                }
            }
            let section_id = match ev.key().as_str() {
                "1" => Some("ov-hero"),
                "2" => Some("ov-ts"),
                "3" => Some("ov-detail"),
                "4" => Some("ov-diag"),
                _ => None,
            };
            if let Some(id) = section_id {
                if let Some(el) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id(id))
                {
                    el.scroll_into_view_with_bool(true);
                }
            }
        };
        let handler = std::sync::Arc::new(std::cell::RefCell::new(handler));
        let handler_clone = handler.clone();
        Effect::new(move |_| {
            let Some(window) = web_sys::window() else {
                return;
            };
            let h = handler_clone.clone();
            let closure =
                wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::KeyboardEvent| {
                    let h = h.borrow();
                    (h)(ev);
                }) as Box<dyn FnMut(_)>);
            let _ = window
                .add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
            // Store closure so it can be removed on cleanup.
            let closure_js: js_sys::Function = closure.into_js_value().into();
            let window_clone = window.clone();
            on_cleanup(move || {
                let _ = window_clone.remove_event_listener_with_callback(
                    "keydown",
                    closure_js.as_ref(),
                );
            });
        });
    }

    let open_health = RwSignal::new(false);
    let open_hit = RwSignal::new(false);
    let open_qps = RwSignal::new(false);
    let open_error = RwSignal::new(false);
    let open_token = RwSignal::new(false);
    let open_coalesce = RwSignal::new(false);
    let open_semantic = RwSignal::new(false);
    let open_latency = RwSignal::new(false);
    let open_ops = RwSignal::new(false);
    let open_consumer = RwSignal::new(false);
    let open_domain = RwSignal::new(false);
    let open_prefix = RwSignal::new(false);
    let open_prefix_health = RwSignal::new(false);
    let open_infra = RwSignal::new(false);

    // Mutual exclusion: only one modal can be open at a time.
    let all_open_signals = [
        open_health,
        open_hit,
        open_qps,
        open_error,
        open_token,
        open_coalesce,
        open_semantic,
        open_latency,
        open_ops,
        open_consumer,
        open_domain,
        open_prefix,
        open_prefix_health,
        open_infra,
    ];
    let close_others = Callback::new(move |except_idx: usize| {
        for (i, s) in all_open_signals.iter().enumerate() {
            if i != except_idx {
                s.set(false);
            }
        }
    });

    // Create per-card open callbacks for mutual exclusion.
    let c0 = close_others.clone();
    let on_open_health = Callback::new(move |_: ()| c0.run(0));
    let c1 = close_others.clone();
    let on_open_hit = Callback::new(move |_: ()| c1.run(1));
    let c2 = close_others.clone();
    let on_open_qps = Callback::new(move |_: ()| c2.run(2));
    let c3 = close_others.clone();
    let on_open_error = Callback::new(move |_: ()| c3.run(3));
    let c4 = close_others.clone();
    let on_open_token = Callback::new(move |_: ()| c4.run(4));
    let c5 = close_others.clone();
    let on_open_coalesce = Callback::new(move |_: ()| c5.run(5));
    let c6 = close_others.clone();
    let on_open_semantic = Callback::new(move |_: ()| c6.run(6));
    let c7 = close_others.clone();
    let on_open_latency = Callback::new(move |_: ()| c7.run(7));
    let c8 = close_others.clone();
    let on_open_ops = Callback::new(move |_: ()| c8.run(8));
    let c9 = close_others.clone();
    let on_open_consumer = Callback::new(move |_: ()| c9.run(9));
    let c10 = close_others.clone();
    let on_open_domain = Callback::new(move |_: ()| c10.run(10));
    let c11 = close_others.clone();
    let on_open_prefix = Callback::new(move |_: ()| c11.run(11));
    let c12 = close_others.clone();
    let on_open_prefix_health = Callback::new(move |_: ()| c12.run(12));
    let c13 = close_others;
    let on_open_infra = Callback::new(move |_: ()| c13.run(13));

    // Infra headline fetched async once.
    let infra_headline: RwSignal<String> = RwSignal::new("—".to_string());
    let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let alive = std::sync::Arc::clone(&alive);
        leptos::task::spawn_local(async move {
            if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            match api::fetch_infra_snapshot().await {
                Ok(s) => infra_headline.try_set(infra_container_headline(&s)),
                Err(_) => infra_headline.try_set("—".to_string()),
            };
        });
    }
    on_cleanup(move || {
        alive.store(false, std::sync::atomic::Ordering::Relaxed);
    });

    // Suggestions for the always-visible TimeSeriesChart (no dependency on core memos).
    let ts_suggestions: Vec<OverviewSuggestion> =
        suggestions_memo.get().unwrap_or_default();

    view! {
        <div class="space-y-4">
            // --- Always-visible: segment navigation ---
            <nav class="overview-segment-nav">
                <a class="overview-segment-link segment-active"
                    href="#ov-hero"
                    on:click=move |ev| {
                        ev.prevent_default();
                        if let Some(el) = web_sys::window()
                            .and_then(|w| w.document())
                            .and_then(|d| d.get_element_by_id("ov-hero"))
                        {
                            el.scroll_into_view_with_bool(true);
                        }
                    }
                >{t.overview_section_hero()}</a>
                <a class="overview-segment-link"
                    href="#ov-ts"
                    on:click=move |ev| {
                        ev.prevent_default();
                        if let Some(el) = web_sys::window()
                            .and_then(|w| w.document())
                            .and_then(|d| d.get_element_by_id("ov-ts"))
                        {
                            el.scroll_into_view_with_bool(true);
                        }
                    }
                >{t.overview_section_timeseries()}</a>
                <a class="overview-segment-link"
                    href="#ov-detail"
                    on:click=move |ev| {
                        ev.prevent_default();
                        if let Some(el) = web_sys::window()
                            .and_then(|w| w.document())
                            .and_then(|d| d.get_element_by_id("ov-detail"))
                        {
                            el.scroll_into_view_with_bool(true);
                        }
                    }
                >{t.overview_section_detail()}</a>
                <a class="overview-segment-link"
                    href="#ov-diag"
                    on:click=move |ev| {
                        ev.prevent_default();
                        if let Some(el) = web_sys::window()
                            .and_then(|w| w.document())
                            .and_then(|d| d.get_element_by_id("ov-diag"))
                        {
                            el.scroll_into_view_with_bool(true);
                        }
                    }
                >{t.overview_section_diagnostics()}</a>
            </nav>

            // --- Always-visible: trend chart + peak hours ---
            <div id="ov-ts" class="overview-section-anchor overview-analytics-row">
                <TimeSeriesChart
                    points=ts_points
                    selected_view=ts_window
                    suggestions=ts_suggestions
                    compact=true
                />
                {move || {
                    if let (Some(ph), Some(ph_err)) = (peak_hours_data, peak_hours_error) {
                        let resp = ph.get();
                        if let Some(err) = ph_err.get() {
                            view! {
                                <div class="peak-hours-container peak-hours-side">
                                    <div class="peak-hours-header">
                                    <h3 class="peak-hours-title">{t.overview_peak_hours_title()}</h3>
                                </div>
                                <p class="text-xs text-theme-muted">
                                    {if err.contains("PG not available") || err.contains("503") {
                                        t.overview_peak_hours_pg_unavailable().to_string()
                                    } else {
                                        t.overview_peak_hours_load_failed(&err)
                                    }}
                                </p>
                            </div>
                        }.into_any()
                        } else if resp.models.is_empty() {
                            view! {
                                <div class="peak-hours-container peak-hours-side">
                                    <div class="peak-hours-header">
                                        <h3 class="peak-hours-title">{t.overview_peak_hours_title()}</h3>
                                    </div>
                                    <p class="text-xs text-theme-muted">{t.overview_peak_hours_no_data()}</p>
                                </div>
                            }.into_any()
                        } else {
                            let data_sig = Signal::derive(move || ph.get().data);
                            let models_sig = Signal::derive(move || ph.get().models);
                            let on_del = Callback::new(move |(model, bucket): (String, i64)| {
                                leptos::task::spawn_local(async move {
                                    if api::delete_model_peak_hour(&model, bucket).await.is_ok() {
                                        if let Ok(resp) = api::fetch_model_peak_hours(7).await {
                                            ph.set(resp);
                                        }
                                    }
                                });
                            });
                            view! {
                                <PeakHoursHeatmap data=data_sig models=models_sig on_delete=on_del />
                            }.into_any()
                        }
                    } else {
                        ().into_any()
                    }
                }}
            </div>

            // --- Conditional: core metric cards (require all 6 memos) ---
            // The 14 open/close signals live outside this block and survive re-renders.
            {move || {
                let h = health_memo.get();
                let m = metrics_memo.get();
                let o = ops_memo.get();
                let p = prefix_memo.get();
                let sem = semantic_memo.get();
                let sugg = suggestions_memo.get().unwrap_or_default();
                let tr = trace_memo.get();
                match (h, m, o, p, sem, tr) {
                    (Some(health), Some(metrics), Some(ops), Some(prefix), Some(semantic), Some(trace)) => {
                        let healthy = health.healthy;
                        let health_headline = if healthy {
                            t.overview_status_active().to_string()
                        } else {
                            t.overview_health_unhealthy().to_string()
                        };
                        let hit_headline = if metrics.metrics_sample_insufficient {
                            "—".to_string()
                        } else {
                            format!("{:.1}%", metrics.hit_rate_5m * 100.0)
                        };
                        let qps_headline = format!("{:.2}", metrics.qps_5m);
                        let err_headline = if metrics.error_rate_5m > 0.001 {
                            format!("{:.2}%", metrics.error_rate_5m * 100.0)
                        } else {
                            "0%".to_string()
                        };
                        let token_headline = format_number(metrics.pg_total_tokens.max(metrics.total_tokens));
                        let coalesce_headline = format!("{:.0}", ops.coalesced_5m);
                        let semantic_headline = metrics.semantic_hits.to_string();
                        let latency_headline = format!("{:.0}ms", metrics.latency_upstream_p99_ms);
                        let ops_headline = format!("{:.0}ms", ops.ttft_ms);
                        let prefix_headline = format!("{:.1}%", prefix.hit_ratio * 100.0);
                        let prefix_break_headline = ops.prefix_break_total.to_string();

                        let hit_rate_trend = if metrics.hit_rate_prev_1h > 0.001 && !metrics.metrics_sample_insufficient {
                            let delta = (metrics.hit_rate_5m - metrics.hit_rate_prev_1h) / metrics.hit_rate_prev_1h * 100.0;
                            Some((format!("{:+.1}%", delta), delta >= 0.0))
                        } else {
                            None
                        };
                        let qps_trend = if metrics.qps_prev_1h > 0.01 {
                            let delta = (metrics.qps_5m - metrics.qps_prev_1h) / metrics.qps_prev_1h * 100.0;
                            Some((format!("{:+.1}%", delta), delta >= 0.0))
                        } else {
                            None
                        };

                        let (top_consumer, _) = top_consumer_label(&metrics);
                        let (top_domain, top_domain_hit) = top_domain_label(&metrics);
                        let top_domain_hit_preview = top_domain_hit;

                        let metrics_hit_preview = metrics.clone();
                        let metrics_hit_detail = metrics.clone();
                        let metrics_consumer = metrics.clone();
                        let metrics_domain = metrics.clone();
                        let metrics_latency = metrics.clone();
                        let metrics_coalesce = metrics.clone();
                        let metrics_semantic = metrics.clone();
                        let metrics_token = metrics.clone();
                        let trace_hit = trace.clone();
                        let suggestions_qps = sugg.clone();
                        let ops_coalesce = ops.clone();
                        let ops_prefix = ops.clone();
                        let prefix_token = prefix.clone();
                        let prefix_card = prefix.clone();

                        let consumer_buckets = metrics.consumer_buckets.clone();
                        let consumer_labels: Signal<Vec<String>> = {
                            let buckets = consumer_buckets.clone();
                            Signal::derive(move || {
                                let mut sorted = buckets.clone();
                                sorted.sort_by(|a, b| {
                                    (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens))
                                });
                                sorted.iter().take(5).map(|b| b.consumer.clone()).collect()
                            })
                        };
                        let consumer_values: Signal<Vec<f64>> = {
                            let buckets = consumer_buckets;
                            Signal::derive(move || {
                                let mut sorted = buckets.clone();
                                sorted.sort_by(|a, b| {
                                    (b.hit_tokens + b.miss_tokens).cmp(&(a.hit_tokens + a.miss_tokens))
                                });
                                sorted
                                    .iter()
                                    .take(5)
                                    .map(|b| (b.hit_tokens + b.miss_tokens) as f64)
                                    .collect()
                            })
                        };

                        view! {
                            <div class="overview-compact space-y-2">
                                <TraceCompareBanner trace=trace.clone() metrics=metrics.clone() compact=true />

                                // Section: Key Metrics (Core indicators - always visible)
                                <div id="ov-hero" class="overview-section-anchor">
                                    <div class="overview-cards-hero">
                                    <OverviewMetricCard
                                        label=t.overview_hit_rate_5m().to_string()
                                        headline=hit_headline
                                        open=open_hit
                                        on_open=on_open_hit
                                        preview=move || {
                                            let trend_view = hit_rate_trend.as_ref().map(|(text, up)| {
                                                let cls = if *up {
                                                    "overview-metric-card-trend trend-up"
                                                } else {
                                                    "overview-metric-card-trend trend-down"
                                                };
                                                view! { <span class=cls>{text.clone()}</span> }.into_any()
                                            });
                                            view! {
                                                <div class="flex flex-col items-end gap-0.5">
                                                    {trend_view.unwrap_or_else(|| ().into_any())}
                                                    <MiniTierDonut metrics=metrics_hit_preview.clone() />
                                                </div>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! {
                                                <div class="space-y-4">
                                                    <TraceCompareBanner trace=trace_hit.clone() metrics=metrics_hit_detail.clone() />
                                                    <CacheHitSection metrics=metrics_hit_detail.clone() />
                                                </div>
                                            }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.overview_health_title().to_string()
                                        headline=health_headline
                                        open=open_health
                                        on_open=on_open_health
                                        preview=move || {
                                            view! {
                                                <span class=if healthy { "online-dot" } else { "w-2 h-2 rounded-full bg-error" }></span>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! {
                                                <OverviewHealthStrip health=health.clone() error_rate=metrics.error_rate_5m />
                                            }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.overview_qps_5m().to_string()
                                        headline=qps_headline
                                        open=open_qps
                                        on_open=on_open_qps
                                        preview=move || {
                                            let spark: Vec<f64> = ts_points
                                                .get_untracked()
                                                .iter()
                                                .map(|p| p.requests as f64)
                                                .collect();
                                            let trend_view = qps_trend.as_ref().map(|(text, up)| {
                                                let cls = if *up {
                                                    "overview-metric-card-trend trend-up"
                                                } else {
                                                    "overview-metric-card-trend trend-down"
                                                };
                                                view! { <span class=cls>{text.clone()}</span> }.into_any()
                                            });
                                            view! {
                                                <div class="flex flex-col items-end gap-0.5">
                                                    {trend_view.unwrap_or_else(|| ().into_any())}
                                                    <Sparkline
                                                        values=spark
                                                        color="var(--cc-accent)"
                                                        width=88
                                                        height=32
                                                    />
                                                </div>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! {
                                                <TimeSeriesChart
                                                    points=ts_points
                                                    selected_view=ts_window
                                                    suggestions=suggestions_qps.clone()
                                                />
                                            }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.overview_error_rate_title().to_string()
                                        headline=err_headline
                                        open=open_error
                                        on_open=on_open_error
                                        preview=move || {
                                            let warn = metrics.error_rate_5m > 0.01;
                                            view! {
                                                <span class=if warn { "text-warning text-[10px]" } else { "text-accent text-[10px]" }>
                                                    {if warn {
                                                        t.overview_error_elevated()
                                                    } else {
                                                        t.overview_error_normal()
                                                    }}
                                                </span>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! {
                                                <div class="grid grid-cols-2 gap-4 text-sm font-mono">
                                                    <div>
                                                        <div class="text-xs text-theme-muted">{t.overview_http_4xx_5m()}</div>
                                                        <div class="text-xl text-warning">{format!("{:.0}", metrics.http_4xx_5m)}</div>
                                                    </div>
                                                    <div>
                                                        <div class="text-xs text-theme-muted">{t.overview_http_5xx_5m()}</div>
                                                        <div class="text-xl text-error">{format!("{:.0}", metrics.http_5xx_5m)}</div>
                                                    </div>
                                                </div>
                                            }.into_any()
                                        }
                                    />
                                    </div>
                                </div> // close ov-hero

                                // Section: Details (single dense mosaic)
                                <div id="ov-detail" class="overview-section-anchor">
                                    <div class="overview-cards-mosaic">
                                        <OverviewMetricCard
                                            label=t.overview_latency_title().to_string()
                                            headline=latency_headline
                                            subtitle=t.overview_latency_upstream().to_string()
                                            open=open_latency
                                            on_open=on_open_latency
                                            preview=move || {
                                                let stages = [
                                                    metrics_latency.latency_l0_ms,
                                                    metrics_latency.latency_l1_ms,
                                                    metrics_latency.latency_l2_ms,
                                                    metrics_latency.latency_upstream_ms,
                                                ];
                                                let max = stages.iter().copied().fold(1.0_f64, f64::max);
                                                view! {
                                                    <div class="flex flex-col gap-0.5 w-full min-w-[4.5rem]">
                                                        {stages.into_iter().enumerate().map(|(i, v)| {
                                                            let pct = v / max * 100.0;
                                                            let color = match i {
                                                                0 => "var(--cc-tier-l0)",
                                                                1 => "var(--cc-tier-l1)",
                                                                2 => "var(--cc-tier-l2)",
                                                                _ => "var(--cc-warning)",
                                                            };
                                                            view! {
                                                                <div class="h-1 rounded-full bg-theme-tertiary overflow-hidden">
                                                                    <div class="h-full" style=format!("width:{pct}%;background:{color}")></div>
                                                                </div>
                                                            }
                                                        }).collect_view()}
                                                    </div>
                                                }.into_any()
                                            }
                                            detail=move || {
                                                view! { <LatencySection metrics=metrics_latency.clone() /> }.into_any()
                                            }
                                        />
                                        <OverviewMetricCard
                                            label=t.overview_token_stats().to_string()
                                            headline=token_headline
                                            open=open_token
                                            on_open=on_open_token
                                            preview=move || {
                                                let total = metrics_token.cache_hit_tokens + metrics_token.cache_miss_tokens;
                                                let pct = if total > 0 {
                                                    metrics_token.cache_hit_tokens as f64 / total as f64 * 100.0
                                                } else {
                                                    0.0
                                                };
                                                let v = Signal::derive(move || pct);
                                                view! {
                                                    <ProgressBar label="" value=v max=100.0 />
                                                }.into_any()
                                            }
                                            detail=move || {
                                                view! {
                                                    <div class="space-y-4">
                                                        <TokenStats metrics=metrics_token.clone() prefix=prefix_token.clone() />
                                                        <PrefixCacheCard prefix=prefix_card.clone() />
                                                    </div>
                                                }.into_any()
                                            }
                                        />
                                        <OverviewMetricCard
                                            label=t.overview_consumer_table_title().to_string()
                                            headline=top_consumer
                                            open=open_consumer
                                            on_open=on_open_consumer
                                            preview=move || {
                                                view! {
                                                    <HorizontalBarChart
                                                        labels=consumer_labels
                                                        values=consumer_values
                                                        width=120
                                                        height_px=36
                                                        empty_message=t.overview_no_data()
                                                    />
                                                }.into_any()
                                            }
                                            detail=move || {
                                                view! { <ConsumerHitTable metrics=metrics_consumer.clone() /> }.into_any()
                                            }
                                        />
                                        <OverviewMetricCard
                                            label=t.overview_domain_card_title().to_string()
                                            headline=top_domain
                                            open=open_domain
                                            on_open=on_open_domain
                                            preview=move || {
                                                view! {
                                                    <span class="text-[10px] font-mono text-accent whitespace-nowrap">
                                                        {t.overview_domain_hit_fmt(top_domain_hit_preview)}
                                                    </span>
                                                }.into_any()
                                            }
                                            detail=move || {
                                                let cb = Callback::new(move |domain: String| {
                                                    selected_domain.set(Some(domain));
                                                });
                                                view! {
                                                    <DomainOverviewTableInline
                                                        metrics=metrics_domain.clone()
                                                        on_domain_click=cb
                                                    />
                                                }.into_any()
                                            }
                                        />
                                    <OverviewMetricCard
                                        label=t.overview_coalescing_title().to_string()
                                        headline=coalesce_headline
                                        open=open_coalesce
                                        on_open=on_open_coalesce
                                        preview=move || {
                                            let d = metrics_coalesce.tier_deltas_5m;
                                            let unique = d.l0 + d.l1 + d.l2 + d.miss;
                                            let coalesced = ops_coalesce.coalesced_5m;
                                            let eff = if unique + coalesced > 0 {
                                                coalesced as f64 / (unique as f64 + coalesced as f64) * 100.0
                                            } else {
                                                0.0
                                            };
                                            let v = Signal::derive(move || eff);
                                            view! {
                                                <ProgressBar label="" value=v max=100.0 />
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! {
                                                <CoalescingCard metrics=metrics_coalesce.clone() ops=ops_coalesce.clone() />
                                            }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.overview_semantic_card_title().to_string()
                                        headline=semantic_headline
                                        open=open_semantic
                                        on_open=on_open_semantic
                                        preview=move || {
                                            view! {
                                                <div class="grid grid-cols-3 gap-1 text-center text-[10px] font-mono">
                                                    <span class="text-accent">{metrics_semantic.semantic_hits}</span>
                                                    <span class="text-warning">{metrics_semantic.semantic_rejected}</span>
                                                    <span class="text-theme-muted">{metrics_semantic.semantic_skipped}</span>
                                                </div>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! {
                                                <SemanticCacheCard metrics=metrics_semantic.clone() semantic=semantic.clone() />
                                            }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.overview_ops_title().to_string()
                                        headline=ops_headline
                                        subtitle=t.overview_ops_ttft().to_string()
                                        open=open_ops
                                        on_open=on_open_ops
                                        preview=move || {
                                            view! {
                                                <div class="grid grid-cols-1 gap-0.5 text-[10px] font-mono text-theme-muted whitespace-nowrap">
                                                    <span>{format!("TTFT {:.0}ms", ops.ttft_ms)}</span>
                                                    <span>{format!("rej {:.0}", ops.rejected_5m)}</span>
                                                </div>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! { <OpsMetricsRow ops=ops.clone() /> }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.overview_prefix_cache_title().to_string()
                                        headline=prefix_headline
                                        open=open_prefix
                                        on_open=on_open_prefix
                                        preview=move || {
                                            let total = prefix.hit_tokens + prefix.miss_tokens;
                                            let pct = if total > 0 {
                                                prefix.hit_tokens as f64 / total as f64 * 100.0
                                            } else {
                                                0.0
                                            };
                                            let v = Signal::derive(move || pct);
                                            view! {
                                                <ProgressBar label="" value=v max=100.0 />
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! { <PrefixCacheCard prefix=prefix.clone() /> }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.overview_prefix_health_title().to_string()
                                        headline=prefix_break_headline
                                        subtitle=t.overview_prefix_breaks_subtitle().to_string()
                                        open=open_prefix_health
                                        on_open=on_open_prefix_health
                                        preview=move || {
                                            let warn = ops_prefix.prefix_break_total > 0;
                                            view! {
                                                <span class=if warn { "text-warning text-[10px]" } else { "text-theme-muted text-[10px]" }>
                                                    {if warn {
                                                        t.overview_prefix_breaks_detected()
                                                    } else {
                                                        t.overview_prefix_breaks_stable()
                                                    }}
                                                </span>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! { <PrefixHealthCard ops=ops_prefix.clone() /> }.into_any()
                                        }
                                    />
                                    <OverviewMetricCard
                                        label=t.infra_title().to_string()
                                        headline=infra_headline
                                        subtitle=t.sidebar_infra().to_string()
                                        open=open_infra
                                        on_open=on_open_infra
                                        preview=move || {
                                            view! {
                                                <span class="text-[10px] text-theme-muted">{t.overview_card_infra_hint()}</span>
                                            }.into_any()
                                        }
                                        detail=move || {
                                            view! { <InfraOverviewModule embedded=true /> }.into_any()
                                        }
                                    />
                                    </div>
                                </div> // close ov-detail
                                <DomainDetailDrawer domain=selected_domain />
                            </div>
                        }.into_any()
                    }
                    _ => ().into_any(),
                }
            }}
        </div>
    }
}
