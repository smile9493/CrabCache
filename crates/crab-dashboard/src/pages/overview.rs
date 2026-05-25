use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

use crate::api;
use crate::components::line_chart::{ChartSeries, LineChart};
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::locale::{Translations, use_translations};
use crate::page_visible::page_visible;
use crate::types::TimeSeriesPoint;
use crate::types::{
    GatewayHealth, MetricsSnapshot, MetricsSnapshotCore, OverviewCore, OverviewOpsMetrics,
    OverviewSuggestion, PrefixCacheMetricsSnapshot, SemanticConfig, TraceSummary,
};

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
    }
}

#[component]
pub fn OverviewPage() -> impl IntoView {
    let t = use_translations();
    let overview_core: RwSignal<Option<Result<OverviewCore, String>>> = RwSignal::new(None);
    let trace_summary: RwSignal<Option<TraceSummary>> = RwSignal::new(None);
    let ts_points: RwSignal<Vec<TimeSeriesPoint>> = RwSignal::new(Vec::new());
    let ts_window = RwSignal::new("1h".to_string());
    let auto_refresh = RwSignal::new(true);
    let last_update = RwSignal::new(String::new());
    let load_generation = RwSignal::new(0u64);
    let ts_generation = RwSignal::new(0u64);
    let etag = RwSignal::new(String::new());

    let load_core = move || {
        load_generation.update(|g| *g += 1);
        let request_id = load_generation.get();
        let current_etag = etag.get();
        leptos::task::spawn_local(async move {
            match api::fetch_overview_core(&current_etag).await {
                Ok(result) => {
                    etag.set(result.etag);
                    if load_generation.get() == request_id {
                        if let Some(core) = result.core {
                            overview_core.set(Some(Ok(core)));
                            last_update.set(chrono::Local::now().format("%H:%M:%S").to_string());
                        }
                    }
                }
                Err(e) => {
                    if load_generation.get() == request_id {
                        overview_core.set(Some(Err(e)));
                    }
                }
            }
        });
    };

    let load_trace = move || {
        leptos::task::spawn_local(async move {
            if let Ok(summary) = api::fetch_overview_trace().await {
                trace_summary.set(Some(summary));
            }
        });
    };

    let load_timeseries = move || {
        ts_generation.update(|g| *g += 1);
        let request_id = ts_generation.get();
        let window = ts_window.get_untracked();
        leptos::task::spawn_local(async move {
            match api::fetch_overview_timeseries(&window).await {
                Ok(resp) => {
                    if ts_generation.get() == request_id {
                        ts_points.set(resp.points);
                    }
                }
                Err(_) => {}
            }
        });
    };

    let deferred_loaded = RwSignal::new(false);

    load_core();

    Effect::new({
        let load_trace = load_trace;
        let load_timeseries = load_timeseries;
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
        let load_timeseries = load_timeseries;
        move |_| {
            if !deferred_loaded.get() {
                return;
            }
            let _ = ts_window.get();
            load_timeseries();
        }
    });

    leptos::task::spawn_local(async move {
        loop {
            TimeoutFuture::new(10_000).await;
            if auto_refresh.get() && page_visible() {
                load_core();
            }
        }
    });

    view! {
        <div class="page-content space-y-6">
            <PageHeader
                title=move || t.overview_title()
                description=move || t.overview_desc()
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
                        {t.overview_auto_refresh()}
                    </label>
                    <button
                        on:click=move |_| {
                            load_core();
                            load_trace();
                            load_timeseries();
                        }
                        class="btn btn-secondary text-xs"
                    >
                        {t.overview_refresh()}
                    </button>
                </div>
            </PageHeader>

            {move || match overview_core.get() {
                None => view! { <Spinner /> }.into_any(),
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

    // Derive the upstream CTA signal — boolean-only, very cheap.
    let show_cta = Memo::new(move |_| {
        let core_opt = overview_core.get();
        let core = match core_opt {
            Some(Ok(ref c)) => c,
            _ => return false,
        };
        core.health.upstream_key_count == 0 && core.ops.upstream_key_count == 0
    });

    // Trace from its own signal.
    let trace = Memo::new(move |_| {
        trace_summary.get().unwrap_or(TraceSummary {
            hours: 24,
            total_requests: 0,
            cache_hit_ratio: 0.0,
        })
    });

    view! {
        <div class="space-y-6">
            {move || show_cta.get().then(|| view! {
                <div class="glass-card flex flex-wrap items-center justify-between gap-3 border border-warning/30">
                    <p class="text-sm text-warning">{t.overview_setup_upstream_cta()}</p>
                    <a href="/upstream" class="btn btn-primary text-sm">
                        {t.overview_setup_upstream_link()}
                    </a>
                </div>
            })}
            <MetricsLegend />
            {move || health_memo.get().map(|h| view! { <OverviewHealthStrip health=h /> })}
            {move || metrics_memo.get().map(|m| view! {
                <HistoryMetaHint metrics=m.clone() />
            })}
            {move || metrics_memo.get().zip(suggestions_memo.get()).map(|(m, s)| view! {
                <MetricsBento metrics=m.clone() suggestions=s.clone() />
            })}
            {move || metrics_memo.get().zip(Some(trace.get())).map(|(m, tr)| view! {
                <TraceCompareBanner trace=tr metrics=m.clone() />
            })}
            {move || ops_memo.get().map(|ops| view! {
                <OpsMetricsRow ops=ops.clone() />
            })}
            {move || prefix_memo.get().zip(metrics_memo.get()).map(|(pref, _m)| view! {
                <PrefixCacheCard prefix=pref.clone() />
            })}
            {move || metrics_memo.get().zip(prefix_memo.get()).map(|(m, pref)| view! {
                <TokenStats metrics=m.clone() prefix=pref.clone() />
            })}
            {move || suggestions_memo.get().map(|s| {
                view! {
                    <TimeSeriesChart
                        points=ts_points
                        selected_view=ts_window
                        suggestions=s
                    />
                }
            })}
            {move || metrics_memo.get().zip(ops_memo.get()).map(|(m, ops)| view! {
                <div class="bento-grid-2">
                    <CoalescingCard metrics=m.clone() ops=ops.clone() />
                    <SemanticCacheCard metrics=m.clone() semantic=semantic_memo.get().unwrap_or(SemanticConfig { enabled: false, similarity_threshold: 0.9 }) />
                </div>
            })}
            {move || metrics_memo.get().map(|m| view! {
                <ConsumerHitTable metrics=m.clone() />
            })}
            {move || metrics_memo.get().map(|m| {
                let cb = Callback::new(move |domain: String| {
                    selected_domain.set(Some(domain));
                });
                view! {
                    <crate::pages::domains::DomainOverviewTableInline metrics=m.clone() on_domain_click=cb />
                }
            })}
            <crate::pages::domains::DomainDetailDrawer domain=selected_domain />
            {move || metrics_memo.get().zip(ops_memo.get()).map(|(m, ops)| view! {
                <div class="bento-grid-3">
                    <div class="bento-cell">
                        <CacheHitSection metrics=m.clone() />
                    </div>
                    <div class="bento-cell">
                        <CostSavingsSection ops=ops.clone() />
                    </div>
                    <div class="bento-cell">
                        <LatencySection metrics=m.clone() />
                    </div>
                </div>
            })}
            {move || ops_memo.get().map(|ops| view! {
                <div class="bento-grid-2">
                    <UpstreamKeyStrip ops=ops.clone() />
                    <PrefixHealthCard ops=ops.clone() />
                </div>
            })}
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
fn MetricsLegend() -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card text-xs text-theme-muted space-y-1">
            <p>{t.overview_legend_l0_l2()}</p>
            <p>{t.overview_legend_l3()}</p>
            <p>{t.overview_legend_5m()}</p>
            <p>{t.overview_legend_cumulative()}</p>
        </div>
    }
}

#[component]
fn OverviewHealthStrip(health: GatewayHealth) -> impl IntoView {
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
fn HistoryMetaHint(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let meta = metrics.history_meta.clone();
    let insufficient = metrics.metrics_sample_insufficient;
    let show = insufficient || meta.sample_count < 3;

    view! {
        {show.then(|| view! {
            <p class="text-xs text-warning">
                {if insufficient {
                    t.overview_sample_insufficient().to_string()
                } else {
                    t.overview_history_meta(meta.sample_count, meta.oldest_sample_at_secs)
                }}
            </p>
        })}
        {meta.gateway_counter_reset.then(|| view! {
            <p class="text-xs text-warning mt-1">{t.overview_gateway_reset()}</p>
        })}
    }
}

#[component]
fn TraceCompareBanner(trace: TraceSummary, metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let trace_pct = trace.cache_hit_ratio * 100.0;
    let gw_pct = if metrics.metrics_sample_insufficient {
        None
    } else {
        Some(metrics.hit_rate_5m * 100.0)
    };

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
                </div>
            </div>
            <a href="/cache?tab=trace" class="btn btn-secondary text-xs shrink-0">
                {t.overview_trace_compare_link()}
            </a>
        </div>
    }
}

#[component]
fn OpsMetricsRow(ops: OverviewOpsMetrics) -> impl IntoView {
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
fn PrefixCacheCard(prefix: PrefixCacheMetricsSnapshot) -> impl IntoView {
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
fn MetricsBento(metrics: MetricsSnapshot, suggestions: Vec<OverviewSuggestion>) -> impl IntoView {
    let t = use_translations();
    let total_hits = metrics.l0_hits + metrics.l1_hits + metrics.l2_hits;
    let total_requests = total_hits + metrics.cache_misses;
    let hit_rate_cumulative = if metrics.hit_rate_cumulative > 0.0 {
        metrics.hit_rate_cumulative * 100.0
    } else if total_requests > 0 {
        total_hits as f64 / total_requests as f64 * 100.0
    } else {
        0.0
    };
    let hit_rate_5m = metrics.hit_rate_5m * 100.0;
    let token_hit_5m = metrics.token_hit_rate_5m * 100.0;
    let insufficient = metrics.metrics_sample_insufficient;

    view! {
        <div class="bento-grid">
            <div class="bento-cell-hero">
                <div class="metric-card h-full">
                    <div class="flex items-start justify-between mb-4">
                        <div>
                            <div class="metric-card-label">{t.overview_qps_5m()}</div>
                            <div class="metric-card-value">
                                {format!("{:.2}", metrics.qps_5m)}
                            </div>
                            <div class="text-xs text-theme-muted mt-1">
                                {format!("{} {:.2}", t.overview_hit_rate_cumulative_hint(), metrics.qps)}
                            </div>
                        </div>
                        <div class="text-3xl opacity-30">"⚡"</div>
                    </div>
                    <div class="grid grid-cols-2 gap-4 mt-auto">
                        <div>
                            <div class="text-xs text-theme-muted mb-1">{t.overview_hit_rate_5m()}</div>
                            <div class="text-lg font-mono tabular-nums text-accent font-semibold">
                                {if insufficient {
                                    "—".to_string()
                                } else {
                                    format!("{hit_rate_5m:.1}%")
                                }}
                            </div>
                            <div class="text-xs text-theme-muted mt-0.5">
                                {format!("{:.1}% {}", hit_rate_cumulative, t.overview_hit_rate_cumulative_hint())}
                            </div>
                        </div>
                        <div>
                            <div class="text-xs text-theme-muted mb-1">{t.overview_token_hit_rate_5m()}</div>
                            <div class="text-lg font-mono tabular-nums text-theme">
                                {if insufficient {
                                    "—".to_string()
                                } else {
                                    format!("{token_hit_5m:.1}%")
                                }}
                            </div>
                            <div class="text-xs text-theme-muted mt-0.5">
                                {Translations::overview_tps()} " " {format!("{:.2}", metrics.tps)}
                            </div>
                        </div>
                    </div>
                    {insufficient.then(|| view! {
                        <p class="text-xs text-warning mt-3">{t.overview_sample_insufficient()}</p>
                    })}
                    <ChartSuggestions suggestions=suggestions.clone() target="hit_rate" />
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
                        <span class="online-label">{t.overview_status_active()}</span>
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
                        {format_uptime_display(metrics.uptime_secs, metrics.uptime_hours)}
                    </div>
                    <div class="metric-card-sub">{t.overview_uptime_sub()}</div>
                    <div class="mt-3 pt-3 border-t border-theme space-y-1">
                        <div class="flex justify-between text-xs">
                            <span class="text-theme-muted">{t.overview_cache_hits()}</span>
                            <span class="font-mono tabular-nums text-accent">
                                {format!("{}", total_hits)}
                            </span>
                        </div>
                        <div class="flex justify-between text-xs text-theme-muted" title=t.overview_semantic_hint()>
                            <span>{t.overview_semantic_guard()}</span>
                            <span class="font-mono tabular-nums">
                                {format!(
                                    "{} / {} / {}",
                                    metrics.semantic_hits,
                                    metrics.semantic_rejected,
                                    metrics.semantic_skipped
                                )}
                            </span>
                        </div>
                    </div>
                </div>
            </div>
            <div class="bento-cell">
                <div class="metric-card h-full">
                    <div class="flex items-start justify-between mb-3">
                        <div class="metric-card-label">{t.overview_cache_tokens()}</div>
                        <div class="text-2xl opacity-30">"💾"</div>
                    </div>
                    <div class="space-y-2">
                        <div class="flex justify-between items-baseline">
                            <span class="text-xs text-theme-muted">{t.overview_token_hit()}</span>
                            <span class="text-lg font-mono tabular-nums text-accent">
                                {format!("{}", metrics.cache_hit_tokens)}
                            </span>
                        </div>
                        <div class="flex justify-between items-baseline">
                            <span class="text-xs text-theme-muted">{t.overview_token_miss()}</span>
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
fn TokenStats(metrics: MetricsSnapshot, prefix: PrefixCacheMetricsSnapshot) -> impl IntoView {
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
fn TimeSeriesChart(
    points: RwSignal<Vec<TimeSeriesPoint>>,
    selected_view: RwSignal<String>,
    suggestions: Vec<OverviewSuggestion>,
) -> impl IntoView {
    let t = use_translations();

    let chart_points = Memo::new(move |_| compress_timeseries_points(points.get()));

    let x_labels = Signal::derive(move || {
        chart_points
            .get()
            .iter()
            .map(|p| p.timestamp.clone())
            .collect::<Vec<_>>()
    });

    let series = Signal::derive(move || {
        let points = chart_points.get();
        vec![
            ChartSeries {
                label: t.overview_input_tokens().to_string(),
                color: "var(--accent-primary)",
                values: points.iter().map(|p| Some(p.tokens as f64)).collect(),
                dashed: false,
            },
            ChartSeries {
                label: t.overview_requests().to_string(),
                color: "var(--info)",
                values: points.iter().map(|p| Some(p.requests as f64)).collect(),
                dashed: false,
            },
        ]
    });

    view! {
        <div class="glass-card">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-sm font-semibold text-theme">{t.overview_usage_trends()}</h3>
                <div class="flex gap-2">
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

            <LineChart
                x_labels=x_labels
                series=series
                height_px=220
                y_unit="tokens"
                empty_message=t.overview_collecting_timeseries()
            />
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

pub fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

#[component]
fn CoalescingCard(metrics: MetricsSnapshot, ops: OverviewOpsMetrics) -> impl IntoView {
    let t = use_translations();
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
        </div>
    }
}

#[component]
fn SemanticCacheCard(metrics: MetricsSnapshot, semantic: SemanticConfig) -> impl IntoView {
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

#[component]
fn ConsumerHitTable(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let buckets = metrics.consumer_buckets.clone();
    view! {
        <div class="glass-card">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_consumer_table_title()}</h3>
            {if buckets.is_empty() {
                view! {
                    <p class="text-sm text-theme-muted">{t.overview_no_data()}</p>
                }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-theme-muted border-b border-theme">
                                    <th class="pb-2 pr-4">{t.overview_consumer_col()}</th>
                                    <th class="pb-2 pr-4">"hit tokens"</th>
                                    <th class="pb-2 pr-4">"miss tokens"</th>
                                    <th class="pb-2">"ratio"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {buckets.into_iter().map(|b| {
                                    view! {
                                        <tr class="border-b border-theme/50">
                                            <td class="py-2 pr-4 font-mono text-theme">{b.consumer}</td>
                                            <td class="py-2 pr-4 font-mono tabular-nums">{format_number(b.hit_tokens)}</td>
                                            <td class="py-2 pr-4 font-mono tabular-nums">{format_number(b.miss_tokens)}</td>
                                            <td class="py-2 font-mono tabular-nums text-accent">
                                                {format!("{:.1}%", b.hit_ratio * 100.0)}
                                            </td>
                                        </tr>
                                    }
                                }).collect::<Vec<_>>()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn CacheHitSection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let d = metrics.tier_deltas_5m;
    let total = (d.l0 + d.l1 + d.l2 + d.miss).max(1) as f64;

    let l0 = RwSignal::new(d.l0 as f64);
    let l1 = RwSignal::new(d.l1 as f64);
    let l2 = RwSignal::new(d.l2 as f64);
    let miss = RwSignal::new(d.miss as f64);

    let hit_rate = RwSignal::new((d.l0 + d.l1 + d.l2) as f64 / total * 100.0);

    view! {
        <div class="glass-card h-full">
            <h3 class="text-sm font-semibold text-theme mb-1">{t.overview_gateway_cache_title()}</h3>
            <p class="text-xs text-theme-muted mb-4">{t.overview_tier_5m_hint()}</p>
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
fn CostSavingsSection(ops: OverviewOpsMetrics) -> impl IntoView {
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
fn UpstreamKeyStrip(ops: OverviewOpsMetrics) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card h-full flex flex-col justify-between">
            <div>
                <h3 class="text-sm font-semibold text-theme mb-2">{t.overview_upstream_keys_strip()}</h3>
                <div class="text-3xl font-mono tabular-nums text-accent">
                    {format!("{}/{}", ops.upstream_keys_available, ops.upstream_key_count)}
                </div>
                <p class="text-xs text-theme-muted mt-2">"available / configured"</p>
            </div>
            <a href="/upstream" class="btn btn-secondary text-xs mt-4 w-fit">
                {t.overview_upstream_keys_link()}
            </a>
        </div>
    }
}

#[component]
fn PrefixHealthCard(ops: OverviewOpsMetrics) -> impl IntoView {
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

#[component]
fn LatencySection(metrics: MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let stages: Vec<(&str, f64)> = vec![
        (
            crate::locale::Translations::overview_latency_l0(),
            metrics.latency_l0_ms,
        ),
        (
            crate::locale::Translations::overview_latency_l1(),
            metrics.latency_l1_ms,
        ),
        (
            crate::locale::Translations::overview_latency_l2(),
            metrics.latency_l2_ms,
        ),
        (t.overview_latency_upstream(), metrics.latency_upstream_ms),
    ];

    let max_latency = stages
        .iter()
        .map(|&(_, v)| v)
        .fold(0.0f64, f64::max)
        .max(1.0);

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
