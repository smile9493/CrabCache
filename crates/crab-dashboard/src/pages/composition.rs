use crate::api::{fetch_composition_debug, fetch_composition_summary, fetch_composition_trends};
use crate::components::canvas_bar_chart::CanvasBarChart;
use crate::components::canvas_line_chart::CanvasLineChart;
use crate::components::line_chart::ChartSeries;
use crate::locale::{Locale, use_locale, use_translations};
use crate::types::{CompositionDebugEntry, CompositionSummary, CompositionTrendsResponse};
use leptos::prelude::*;
use leptos::task::spawn_local;

/// Reactive state for the composition page.
#[derive(Clone, Default)]
struct CompositionState {
    summary: Option<CompositionSummary>,
    trends: Option<CompositionTrendsResponse>,
    loading: bool,
    error: Option<String>,
}

/// Debug state for the composition debug drawer.
#[derive(Clone, Default)]
struct DebugState {
    entries: Vec<CompositionDebugEntry>,
    total: usize,
    loading: bool,
    error: Option<String>,
    search_hash: String,
    search_consumer: String,
    selected_entry: Option<CompositionDebugEntry>,
}

/// Format a percentage (0.0–1.0) as "XX.X%".
fn pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}

/// Truncate a string to max_len chars with ellipsis.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    } else {
        s.to_string()
    }
}

/// A simple horizontal bar with a label, value, and percentage bar.
#[component]
fn StatBar(label: String, value: String, pct_val: f64, max_pct: f64) -> impl IntoView {
    let bar_width = if max_pct > 0.0 {
        format!("{:.1}%", (pct_val / max_pct) * 100.0)
    } else {
        "0%".to_string()
    };
    view! {
        <div class="flex items-center gap-2 mb-1">
            <span class="text-xs text-[var(--text-secondary)] w-32 truncate shrink-0" title={label.clone()}>{label.clone()}</span>
            <span class="text-xs font-mono w-16 text-right shrink-0">{value}</span>
            <div class="flex-1 h-3 rounded-full bg-[var(--bg-tertiary)] overflow-hidden">
                <div
                    class="h-full rounded-full bg-[var(--accent)] transition-all duration-300"
                    style=format!("width: {}", bar_width)
                ></div>
            </div>
            <span class="text-xs text-[var(--text-secondary)] w-12 text-right">{pct(pct_val)}</span>
        </div>
    }
}

/// Histogram panel using the shared Plotters bar chart (consistent sizing with Overview/Live).
#[component]
fn VerticalBarChart(title: String, bars: Vec<(String, usize)>) -> impl IntoView {
    let title_h = title.clone();
    let bars_x = bars.clone();
    let bars_s = bars;
    let x_labels = Signal::derive(move || {
        bars_x
            .iter()
            .map(|(l, _)| truncate(l, 10))
            .collect::<Vec<_>>()
    });
    let series = Signal::derive(move || {
        vec![ChartSeries {
            label: String::new(),
            color: "var(--cc-accent)".to_string(),
            values: bars_s.iter().map(|(_, c)| Some(*c as f64)).collect(),
            dashed: false,
            fill: false,
        }]
    });
    view! {
        <div class="glass-card panel-chart">
            <h3 class="text-sm font-semibold text-[var(--text-primary)]">{title_h}</h3>
            <CanvasBarChart
                x_labels=x_labels
                series=series
                height_px=220
                y_unit=""
                empty_message=""
            />
        </div>
    }
}

/// A component rate bar (component name, present count, rate).
#[component]
fn ComponentRateBar(component: String, present_count: usize, rate: f64) -> impl IntoView {
    let bar_width = format!("{:.1}%", rate * 100.0);
    view! {
        <div class="flex items-center gap-2 mb-2">
            <span class="text-xs text-[var(--text-secondary)] w-20 truncate shrink-0">{component}</span>
            <span class="text-xs font-mono w-8 text-right shrink-0">{present_count}</span>
            <div class="flex-1 h-3 rounded-full bg-[var(--bg-tertiary)] overflow-hidden">
                <div
                    class="h-full rounded-full bg-[var(--accent)] transition-all duration-300"
                    style=format!("width: {}", bar_width)
                ></div>
            </div>
            <span class="text-xs text-[var(--text-secondary)] w-12 text-right">{pct(rate)}</span>
        </div>
    }
}

/// Hourly request trend (line area chart).
#[component]
fn TrendLineChart(title: String, points: Vec<(String, u32)>) -> impl IntoView {
    let title_h = title.clone();
    let points_x = points.clone();
    let points_s = points;
    let x_labels = Signal::derive(move || {
        points_x
            .iter()
            .map(|(l, _)| truncate(l, 8))
            .collect::<Vec<_>>()
    });
    let series = Signal::derive(move || {
        vec![ChartSeries {
            label: String::new(),
            color: "var(--cc-accent)".to_string(),
            values: points_s.iter().map(|(_, v)| Some(*v as f64)).collect(),
            dashed: false,
            fill: true,
        }]
    });
    view! {
        <div class="glass-card panel-chart">
            <h3 class="text-sm font-semibold text-[var(--text-primary)]">{title_h}</h3>
            <CanvasLineChart
                x_labels=x_labels
                series=series
                height_px=220
                y_unit="req"
                empty_message=""
                y_min=Some(0.0)
            />
        </div>
    }
}

/// Summary card component.
#[component]
fn SummaryCard(label: String, value: String, subtitle: String) -> impl IntoView {
    view! {
        <div class="glass-card text-center p-4 min-w-28 min-h-[5.5rem] flex flex-col justify-center">
            <div class="text-lg font-bold text-[var(--text-primary)]">{value}</div>
            <div class="text-xs text-[var(--text-secondary)] mt-0.5">{label}</div>
            <div class="text-[10px] text-[var(--text-tertiary)]">{subtitle}</div>
        </div>
    }
}

// ── Composition Debug Drawer ─────────────────────────────────────

/// Drawer panel showing full composition debug text (system/tools).
#[component]
fn CompositionDebugDrawer(debug_state: RwSignal<DebugState>) -> impl IntoView {
    let entry = Signal::derive(move || debug_state.get().selected_entry);
    let visible = move || entry.get().is_some();
    let locale = use_locale();
    let close = move |_| debug_state.update(|s| s.selected_entry = None);

    view! {
        <Show when=visible>
            <div class="fixed inset-0 z-50 flex justify-end">
                {/* Backdrop */}
                <div class="absolute inset-0 bg-black/30" on:click=close></div>
                {/* Drawer */}
                <div class="relative w-full max-w-2xl bg-[var(--bg-primary)] shadow-xl overflow-y-auto">
                    <div class="sticky top-0 flex items-center justify-between p-4 border-b border-[var(--border-color)] bg-[var(--bg-primary)] z-10">
                        <h2 class="text-sm font-semibold text-[var(--text-primary)]">{move || match locale.get() {
                            Locale::ZhCN => "调试详情",
                            Locale::EnUS => "Debug Details",
                        }}</h2>
                        <button class="text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] px-2 py-1" on:click=close>
                            {move || match locale.get() {
                                Locale::ZhCN => "关闭",
                                Locale::EnUS => "Close",
                            }}
                        </button>
                    </div>

                    <div class="p-4 space-y-4">
                        {move || entry.get().map(|e| {
                            let mut sections = Vec::new();

                            // Metadata section
                            let meta = vec![
                                ("request_hash", e.request_hash.clone()),
                                ("consumer", e.consumer.clone()),
                                ("domain", e.domain.clone()),
                                ("project_id", e.project_id.clone().unwrap_or_default()),
                                ("model", e.model.clone()),
                            ];
                            sections.push(view! {
                                <div class="debug-section">
                                    <h3 class="text-xs font-semibold text-[var(--accent)] mb-2 uppercase tracking-wider">{move || match locale.get() {
                                        Locale::ZhCN => "元数据",
                                        Locale::EnUS => "Metadata",
                                    }}</h3>
                                    <table class="w-full text-xs">
                                        <tbody>
                                            {meta.into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| {
                                                view! {
                                                    <tr>
                                                        <td class="text-[var(--text-secondary)] pr-3 py-0.5 align-top whitespace-nowrap font-medium">{k.to_string()}</td>
                                                        <td class="text-[var(--text-primary)] py-0.5 break-all font-mono">{v}</td>
                                                    </tr>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </tbody>
                                    </table>
                                </div>
                            }.into_any());

                            // System text section
                            if let Some(ref sys_text) = e.system_text {
                                sections.push(view! {
                                    <div class="debug-section">
                                        <h3 class="text-xs font-semibold text-[var(--accent)] mb-2 uppercase tracking-wider">{move || match locale.get() {
                                            Locale::ZhCN => "系统提示",
                                            Locale::EnUS => "System Prompt",
                                        }}</h3>
                                        <pre class="text-xs text-[var(--text-primary)] bg-[var(--bg-tertiary)] p-3 rounded-md overflow-x-auto max-h-96 overflow-y-auto whitespace-pre-wrap break-all">{sys_text.clone()}</pre>
                                    </div>
                                }.into_any());
                            }

                            // Tools section
                            if let Some(ref tools) = e.tools_json {
                                sections.push(view! {
                                    <div class="debug-section">
                                        <h3 class="text-xs font-semibold text-[var(--accent)] mb-2 uppercase tracking-wider">{move || match locale.get() {
                                            Locale::ZhCN => "工具定义",
                                            Locale::EnUS => "Tools",
                                        }}</h3>
                                        <pre class="text-xs text-[var(--text-primary)] bg-[var(--bg-tertiary)] p-3 rounded-md overflow-x-auto max-h-96 overflow-y-auto whitespace-pre-wrap break-all">{tools.clone()}</pre>
                                    </div>
                                }.into_any());
                            }

                            sections.into_view()
                        })}
                    </div>

                    <div class="sticky bottom-0 p-4 border-t border-[var(--border-color)] bg-[var(--bg-primary)]">
                        <button class="w-full text-xs py-2 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors" on:click=close>
                            {move || match locale.get() {
                                Locale::ZhCN => "关闭",
                                Locale::EnUS => "Close",
                            }}
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// Main composition page component.
#[component]
pub fn CompositionPage() -> impl IntoView {
    let t = use_translations();
    let locale = use_locale();

    let state = RwSignal::new(CompositionState::default());
    let hours = RwSignal::new(24u32);
    let debug_state = RwSignal::new(DebugState::default());

    // Fetch composition data on mount and when hours changes.
    Effect::new(move |_| {
        let h = hours.get();
        spawn_local({
            let state = state;
            async move {
                state.try_update(|s| {
                    s.loading = true;
                    s.error = None;
                });

                // Fetch summary and trends in parallel.
                let (summary_res, trends_res) = futures::join!(
                    fetch_composition_summary(h, None, None),
                    fetch_composition_trends(),
                );

                state.try_update(|s| {
                    s.loading = false;
                    match summary_res {
                        Ok(resp) => s.summary = Some(resp.summary),
                        Err(e) => s.error = Some(e),
                    }
                    match trends_res {
                        Ok(resp) => s.trends = Some(resp),
                        Err(e) => {
                            // Don't overwrite a more important error.
                            if s.error.is_none() {
                                s.error = Some(e);
                            }
                        }
                    }
                });
            }
        });
    });

    // Fetch debug entries.
    let fetch_debug = move || {
        let h = hours.get();
        let hash = debug_state.get_untracked().search_hash.clone();
        let consumer = debug_state.get_untracked().search_consumer.clone();
        spawn_local({
            let debug_state = debug_state;
            async move {
                debug_state.try_update(|s| {
                    s.loading = true;
                    s.error = None;
                });
                match fetch_composition_debug(
                    h,
                    Some(20),
                    if hash.is_empty() { None } else { Some(&hash) },
                    if consumer.is_empty() {
                        None
                    } else {
                        Some(&consumer)
                    },
                )
                .await
                {
                    Ok(resp) => {
                        debug_state.try_update(|s| {
                            s.loading = false;
                            s.entries = resp.entries;
                            s.total = resp.total;
                        });
                    }
                    Err(e) => {
                        debug_state.try_update(|s| {
                            s.loading = false;
                            s.error = Some(e);
                        });
                    }
                }
            }
        });
    };

    fetch_debug();

    view! {
        <div class="composition-page space-y-4">
            // ── Debug drawer ──────────────────────────────────────────
            <CompositionDebugDrawer debug_state=debug_state />

            // ── Page header ──────────────────────────────────────────
            <div class="flex flex-wrap items-center justify-between gap-3 mb-4">
                <div>
                    <h1 class="page-title text-lg font-bold text-[var(--text-primary)]">{move || t.composition_title()}</h1>
                    <p class="page-desc text-sm text-[var(--text-secondary)]">{move || t.composition_desc()}</p>
                </div>
                <div class="flex items-center gap-2">
                    <label class="text-xs text-[var(--text-secondary)]">{move || t.composition_hours()}</label>
                    <select
                        prop:value=move || hours.get().to_string()
                        on:change=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<u32>() {
                                hours.set(v.clamp(1, 168));
                                fetch_debug();
                            }
                        }
                        class="input text-xs py-1 px-2"
                    >
                        <option value="1">1</option>
                        <option value="6">6</option>
                        <option value="24" selected>24</option>
                        <option value="72">72</option>
                        <option value="168">168</option>
                    </select>
                    <button
                        on:click=move |_| {
                            let h = hours.get();
                            let state = state;
                            spawn_local(async move {
                                state.try_update(|s| { s.loading = true; s.error = None; });
                                let (sr, tr) = futures::join!(
                                    fetch_composition_summary(h, None, None),
                                    fetch_composition_trends(),
                                );
                                state.try_update(|s| {
                                    s.loading = false;
                                    match sr {
                                        Ok(resp) => s.summary = Some(resp.summary),
                                        Err(e) => s.error = Some(e),
                                    }
                                    match tr {
                                        Ok(resp) => s.trends = Some(resp),
                                        Err(e) => if s.error.is_none() { s.error = Some(e); }
                                    }
                                });
                            });
                        }
                        class="btn btn-secondary text-xs"
                    >
                        {move || t.composition_refresh()}
                    </button>
                </div>
            </div>

            // ── Loading state ────────────────────────────────────────
            {move || if state.get().loading && state.get().summary.is_none() {
                view! {
                    <div class="flex items-center justify-center py-16">
                        <div class="text-sm text-[var(--text-secondary)] animate-pulse">
                            {move || match locale.get() {
                                Locale::ZhCN => "加载中…",
                                Locale::EnUS => "Loading…",
                            }}
                        </div>
                    </div>
                }.into_any()
            } else if let Some(ref err) = state.get().error {
                view! {
                    <div class="error-card glass-card p-4 text-center">
                        <p class="text-sm text-red-500 mb-2">{move || t.composition_load_error()}</p>
                        <p class="text-xs text-[var(--text-secondary)]">{err.clone()}</p>
                    </div>
                }.into_any()
            } else if state.get().summary.is_none() {
                view! {
                    <div class="empty-card glass-card p-8 text-center">
                        <p class="text-sm text-[var(--text-secondary)]">{move || t.composition_no_data()}</p>
                    </div>
                }.into_any()
            } else {
                let s = state.get().summary.unwrap_or_default();
                let tr = state.get().trends.clone();
                view! {
                    <div class="composition-body space-y-4">
                    // ── Summary cards ────────────────────────────────────
                    <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-3">
                        <SummaryCard
                            label=t.composition_total_entries().to_string()
                            value=s.total_entries.to_string()
                            subtitle=String::new()
                        />
                        <SummaryCard
                            label=t.composition_avg_latency().to_string()
                            value=format!("{:.0}ms", s.avg_latency_ms)
                            subtitle=String::new()
                        />
                        <SummaryCard
                            label=t.composition_avg_tokens().to_string()
                            value=s.avg_total_tokens.to_string()
                            subtitle=String::new()
                        />
                        <SummaryCard
                            label={
                                match locale.get() {
                                    Locale::ZhCN => "延迟".to_string(),
                                    Locale::EnUS => "Latency".to_string(),
                                }
                            }
                            value=format!("{:.1}", s.avg_latency_ms)
                            subtitle="ms".to_string()
                        />
                        <SummaryCard
                            label=t.composition_tenant_count().to_string()
                            value=s.tenant_count.to_string()
                            subtitle=String::new()
                        />
                        <SummaryCard
                            label=t.composition_consumer_count().to_string()
                            value=s.consumer_count.to_string()
                            subtitle=String::new()
                        />
                    </div>

                    // ── Model distribution ───────────────────────────────
                    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 items-start">
                        <VerticalBarChart
                            title=t.composition_model_distribution().to_string()
                            bars=s.model_distribution.iter().map(|n| (n.name.clone(), n.count)).collect()
                        />

                        // ── Tool count histogram ─────────────────────────
                        <VerticalBarChart
                            title=t.composition_tool_histogram().to_string()
                            bars=s.tool_count_histogram.iter().map(|b| (b.bucket_label.clone(), b.count)).collect()
                        />
                    </div>

                    // ── Component rates + message histogram ──────────────
                    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 items-start">
                        <div class="glass-card panel-chart">
                            <h3 class="text-sm font-semibold mb-3 text-[var(--text-primary)]">
                                {move || t.composition_component_rates()}
                            </h3>
                            {s.component_rates.iter().map(|cr| {
                                view! {
                                    <ComponentRateBar
                                        component=cr.component.clone()
                                        present_count=cr.present_count
                                        rate=cr.rate
                                    />
                                }
                            }).collect::<Vec<_>>()}
                        </div>

                        <VerticalBarChart
                            title=t.composition_msg_histogram().to_string()
                            bars=s.message_count_histogram.iter().map(|b| (b.bucket_label.clone(), b.count)).collect()
                        />
                    </div>

                    // ── Project / Consumer distribution ─────────────────
                    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 items-start">
                        <div class="glass-card panel-chart">
                            <h3 class="text-sm font-semibold mb-3 text-[var(--text-primary)]">
                                {move || t.composition_project_distribution()}
                            </h3>
                            {if s.project_distribution.is_empty() {
                                view! {
                                    <p class="text-xs text-[var(--text-tertiary)] italic">
                                        {move || match locale.get() {
                                            Locale::ZhCN => "无项目数据",
                                            Locale::EnUS => "No project data",
                                        }}
                                    </p>
                                }.into_any()
                            } else {
                                s.project_distribution.iter().map(|n| {
                                    view! {
                                        <StatBar
                                            label=n.name.clone()
                                            value=n.count.to_string()
                                            pct_val=n.count as f64 / (s.total_entries.max(1)) as f64
                                            max_pct=1.0f64.max(s.project_distribution.first().map(|n| n.count as f64 / (s.total_entries.max(1)) as f64).unwrap_or(0.0))
                                        />
                                    }
                                }).collect::<Vec<_>>().into_any()
                            }}
                        </div>

                        <div class="glass-card panel-chart">
                            <h3 class="text-sm font-semibold mb-3 text-[var(--text-primary)]">
                                {move || t.composition_consumer_distribution()}
                            </h3>
                            {if s.consumer_distribution.is_empty() {
                                view! {
                                    <p class="text-xs text-[var(--text-tertiary)] italic">
                                        {move || match locale.get() {
                                            Locale::ZhCN => "无消费者数据",
                                            Locale::EnUS => "No consumer data",
                                        }}
                                    </p>
                                }.into_any()
                            } else {
                                s.consumer_distribution.iter().map(|n| {
                                    view! {
                                        <StatBar
                                            label=n.name.clone()
                                            value=n.count.to_string()
                                            pct_val=n.count as f64 / (s.total_entries.max(1)) as f64
                                            max_pct=1.0f64.max(s.consumer_distribution.first().map(|n| n.count as f64 / (s.total_entries.max(1)) as f64).unwrap_or(0.0))
                                        />
                                    }
                                }).collect::<Vec<_>>().into_any()
                            }}
                        </div>
                    </div>

                    // ── Trends chart ────────────────────────────────────
                    {tr.as_ref().map(|trends| {
                        let labels: Vec<(String, u32)> = trends.points.iter().map(|p| {
                            // Convert timestamp_ms to hour label.
                            let ts = crate::datetime::format_ms_china_hour_label(p.timestamp_ms);
                            (ts, p.request_count)
                        }).collect();
                        view! {
                            <TrendLineChart
                                title=format!("{} ({})", t.composition_trends(), trends.hours)
                                points=labels
                            />
                        }
                    })}
                    </div>
                }.into_any()
            }}

            // ── Debug section ─────────────────────────────────────────
            <div class="glass-card p-4">
                <h3 class="text-sm font-semibold text-[var(--text-primary)] mb-3">
                    {move || match locale.get() {
                        Locale::ZhCN => "调试详情",
                        Locale::EnUS => "Debug Details",
                    }}
                </h3>
                <p class="text-xs text-[var(--text-tertiary)] mb-3">
                    {move || match locale.get() {
                        Locale::ZhCN => "查看原始系统提示和工具定义。需启用 [trace_logging.composition_debug] enabled = true。",
                        Locale::EnUS => "View raw system prompts and tool definitions. Requires [trace_logging.composition_debug] enabled = true.",
                    }}
                </p>

                {/* Search controls */}
                <div class="flex items-center gap-2 mb-3">
                    <input
                        type="text"
                        placeholder=move || match locale.get() {
                            Locale::ZhCN => "按 request_hash 搜索",
                            Locale::EnUS => "Search by request_hash",
                        }.to_string()
                        prop:value=move || debug_state.get().search_hash
                        on:input=move |ev| {
                            debug_state.update(|s| s.search_hash = event_target_value(&ev));
                        }
                        class="input text-xs py-1 px-2 flex-1"
                    />
                    <input
                        type="text"
                        placeholder=move || match locale.get() {
                            Locale::ZhCN => "按 consumer 筛选",
                            Locale::EnUS => "Filter by consumer",
                        }.to_string()
                        prop:value=move || debug_state.get().search_consumer
                        on:input=move |ev| {
                            debug_state.update(|s| s.search_consumer = event_target_value(&ev));
                        }
                        class="input text-xs py-1 px-2 flex-1"
                    />
                    <button
                        class="btn btn-primary text-xs"
                        on:click=move |_| fetch_debug()
                    >
                        {move || match locale.get() {
                            Locale::ZhCN => "搜索",
                            Locale::EnUS => "Search",
                        }}
                    </button>
                </div>

                {/* Debug entries */}
                {move || {
                    let ds = debug_state.get();
                    if ds.loading && ds.entries.is_empty() {
                        view! {
                            <div class="text-xs text-[var(--text-secondary)] animate-pulse py-4">
                                {move || match locale.get() {
                                    Locale::ZhCN => "加载中…",
                                    Locale::EnUS => "Loading…",
                                }}
                            </div>
                        }.into_any()
                    } else if let Some(ref err) = ds.error {
                        view! {
                            <div class="text-xs text-red-500 py-2">{err.clone()}</div>
                        }.into_any()
                    } else if ds.entries.is_empty() {
                        view! {
                            <div class="text-xs text-[var(--text-tertiary)] italic py-4">
                                {move || match locale.get() {
                                    Locale::ZhCN => "无调试条目（需启用 composition_debug）",
                                    Locale::EnUS => "No debug entries (composition_debug must be enabled)",
                                }}
                            </div>
                        }.into_any()
                    } else {
                        let entries = ds.entries.clone();
                        let total = ds.total;
                        let entries_count = entries.len();
                        view! {
                            <div class="text-xs text-[var(--text-secondary)] mb-2">
                                {move || format!("{} / {} {}",
                                    total,
                                    entries_count,
                                    match locale.get() {
                                        Locale::ZhCN => "条条目",
                                        Locale::EnUS => "entries",
                                    }
                                )}
                            </div>
                            <div class="space-y-2">
                                {entries.into_iter().map(move |entry| {
                                    let hash = entry.request_hash.clone();
                                    let consumer = entry.consumer.clone();
                                    let model = entry.model.clone();
                                    let has_system = entry.system_text.is_some();
                                    let has_tools = entry.tools_json.is_some();
                                    let entry_for_click = entry.clone();
                                    view! {
                                        <div
                                            class="glass-card p-2 cursor-pointer hover:bg-[var(--bg-tertiary)] transition-colors"
                                            on:click=move |_| {
                                                debug_state.update(|s| s.selected_entry = Some(entry_for_click.clone()));
                                            }
                                        >
                                            <div class="flex items-center justify-between gap-2">
                                                <span class="font-mono text-xs text-[var(--text-primary)]">{truncate(&hash, 16)}</span>
                                                <span class="text-[10px] text-[var(--text-secondary)]">{consumer}</span>
                                            </div>
                                            <div class="flex items-center gap-2 mt-1">
                                                <span class="text-[10px] text-[var(--accent)]">{model}</span>
                                                {if has_system {
                                                    view! { <span class="text-[9px] px-1 py-0.5 rounded bg-blue-500/10 text-blue-400">system</span> }.into_any()
                                                } else {
                                                    let _: () = view! {};
                                                    ().into_any()
                                                }}
                                                {if has_tools {
                                                    view! { <span class="text-[9px] px-1 py-0.5 rounded bg-green-500/10 text-green-400">tools</span> }.into_any()
                                                } else {
                                                    let _: () = view! {};
                                                    ().into_any()
                                                }}
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
