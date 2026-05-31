//! Floating session drill-down panel (merged from former Session Monitor page).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use leptos::prelude::*;

use crate::api;
use crate::components::canvas_line_chart::CanvasLineChart;
use crate::components::histogram_chart::HistogramChart;
use crate::components::horizontal_bar_chart::HorizontalBarChart;
use crate::components::icons::{Icon, IconName};
use crate::components::line_chart::ChartSeries;
use crate::components::ui::{Badge, EmptyState};
use crate::locale::use_translations;
use crate::types::{KeyRoutingResponse, SessionTimelineResponse};

fn minute_bucket_label(ts_ms: u64) -> String {
    let minute = (ts_ms / 1000) / 60;
    format!("t+{minute}m")
}

fn build_timeline_buckets(tl: &SessionTimelineResponse) -> (Vec<String>, Vec<Option<f64>>) {
    let mut map: BTreeMap<String, u64> = BTreeMap::new();
    for event in &tl.events {
        let label = minute_bucket_label(event.timestamp_ms);
        *map.entry(label).or_insert(0) += 1;
    }
    let labels: Vec<String> = map.keys().cloned().collect();
    let values: Vec<Option<f64>> = map.values().map(|v| Some(*v as f64)).collect();
    (labels, values)
}

fn build_backend_distribution(r: &KeyRoutingResponse) -> (Vec<String>, Vec<f64>) {
    let mut rows: Vec<_> = r.backends.iter().collect();
    rows.sort_by_key(|b| b.request_count);
    let labels: Vec<String> = rows.iter().map(|b| b.backend_name.clone()).collect();
    let values: Vec<f64> = rows.iter().map(|b| b.request_count as f64).collect();
    (labels, values)
}

fn build_session_latencies(tl: &SessionTimelineResponse) -> Vec<f64> {
    tl.events
        .iter()
        .map(|e| e.latency_ms)
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect()
}

#[component]
pub fn SessionDrilldownPanel(open: RwSignal<bool>) -> impl IntoView {
    let t = use_translations();
    let fingerprint = RwSignal::new(String::new());
    let selected_key = RwSignal::new(String::new());
    let timeline: RwSignal<Option<Result<SessionTimelineResponse, String>>> = RwSignal::new(None);
    let key_routing: RwSignal<Option<Result<KeyRoutingResponse, String>>> = RwSignal::new(None);
    let alive = Arc::new(AtomicBool::new(true));

    on_cleanup({
        let alive = Arc::clone(&alive);
        move || alive.store(false, Ordering::Relaxed)
    });

    let load_timeline = {
        let alive = Arc::clone(&alive);
        Callback::new(move |_: ()| {
            let fp = fingerprint.get().trim().to_string();
            if fp.is_empty() {
                return;
            }
            let _ = timeline.try_set(None);
            let alive = Arc::clone(&alive);
            leptos::task::spawn_local(async move {
                let result = api::fetch_session_timeline(&fp).await;
                if !alive.load(Ordering::Relaxed) {
                    return;
                }
                let _ = timeline.try_set(Some(result));
            });
        })
    };

    let load_routing = {
        let alive = Arc::clone(&alive);
        Callback::new(move |_: ()| {
            let key_id = selected_key.get().trim().to_string();
            if key_id.is_empty() {
                return;
            }
            let _ = key_routing.try_set(None);
            let alive = Arc::clone(&alive);
            leptos::task::spawn_local(async move {
                let result = api::fetch_key_routing(&key_id, 300).await;
                if !alive.load(Ordering::Relaxed) {
                    return;
                }
                let _ = key_routing.try_set(Some(result));
            });
        })
    };

    let close_label = t.chart_detail_close();

    view! {
        <div
            class="session-drilldown-host"
            class:session-drilldown-host--hidden=move || !open.get()
        >
            <div
                class="session-drilldown-backdrop"
                role="presentation"
                on:click=move |_| open.set(false)
            ></div>
            <aside
                class="session-drilldown-panel glass-card"
                role="dialog"
                aria-modal="true"
                aria-label=t.session_drilldown_title()
                on:click=move |ev| { ev.stop_propagation(); }
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        open.set(false);
                        ev.prevent_default();
                    }
                }
                tabindex="-1"
            >
                <div class="session-drilldown-header">
                    <div class="min-w-0">
                        <div class="flex items-center gap-2">
                            <Icon name=IconName::Radar class="w-4 h-4 text-accent" />
                            <h2 class="session-drilldown-title">{t.session_drilldown_title()}</h2>
                        </div>
                        <p class="session-drilldown-subtitle">{t.session_drilldown_hint()}</p>
                    </div>
                    <button
                        type="button"
                        class="btn btn-secondary text-xs shrink-0"
                        on:click=move |_| open.set(false)
                    >
                        {close_label}
                    </button>
                </div>
                <div class="session-drilldown-body space-y-4">
                    <div class="flex items-center gap-2">
                        <input
                            type="text"
                            class="input flex-1 font-mono text-sm"
                            placeholder="sfp:xxxx"
                            prop:value=move || fingerprint.get()
                            on:input=move |ev| fingerprint.set(event_target_value(&ev))
                        />
                        <button class="btn btn-primary text-sm shrink-0" on:click=move |_| load_timeline.run(())>
                            {t.session_load_timeline()}
                        </button>
                    </div>

                    {move || match timeline.get() {
                        None => view! {
                            <p class="text-xs text-theme-muted">{t.session_enter_fingerprint()}</p>
                        }.into_any(),
                        Some(Err(e)) => view! {
                            <p class="text-sm text-error">{e}</p>
                        }.into_any(),
                        Some(Ok(tl)) => {
                            let (bucket_labels, bucket_values) = build_timeline_buckets(&tl);
                            let latencies = std::sync::Arc::new(build_session_latencies(&tl));
                            let latency_sig = {
                                let latencies = std::sync::Arc::clone(&latencies);
                                Signal::derive(move || latencies.as_ref().clone())
                            };
                            let bucket_labels_sig = Signal::derive(move || bucket_labels.clone());
                            let bucket_series_sig = Signal::derive(move || {
                                vec![ChartSeries {
                                    label: "events/min".to_string(),
                                    color: "var(--cc-accent)".to_string(),
                                    values: bucket_values.clone(),
                                    dashed: false,
                                    fill: true,
                                }]
                            });
                            view! {
                                <div class="space-y-3">
                                    <div class="flex items-center justify-between gap-2">
                                        <h3 class="text-sm font-semibold text-theme">{t.session_timeline_title()}</h3>
                                        <span class="text-xs text-theme-muted font-mono">
                                            {format!("{} / {}s", tl.total_events, tl.window_secs)}
                                        </span>
                                    </div>
                                    <div class="flex items-center gap-2">
                                        <select
                                            class="input text-sm flex-1"
                                            prop:value=move || selected_key.get()
                                            on:change=move |ev| selected_key.set(event_target_value(&ev))
                                        >
                                            <option value="">{t.session_select_key_routing()}</option>
                                            {tl.unique_keys.iter().map(|k| view! {
                                                <option value={k.clone()}>{k.clone()}</option>
                                            }).collect_view()}
                                        </select>
                                        <button class="btn btn-secondary text-sm shrink-0" on:click={
                                            let cb = load_routing.clone();
                                            move |_| cb.run(())
                                        }>
                                            {t.session_load_routing()}
                                        </button>
                                    </div>
                                    <div>
                                        <p class="text-xs text-theme-muted mb-2">{t.session_timeline_desc()}</p>
                                        <CanvasLineChart
                                            x_labels=bucket_labels_sig
                                            series=bucket_series_sig
                                            height_px=140
                                            y_unit="req"
                                            empty_message=t.live_no_data()
                                        />
                                    </div>
                                    <div>
                                        <p class="text-xs text-theme-muted mb-2">{t.session_latency_dist()}</p>
                                        <HistogramChart
                                            values=latency_sig
                                            bin_count=12
                                            height_px=120
                                            y_unit="req"
                                            empty_message=t.live_no_data()
                                        />
                                    </div>
                                    <div class="overflow-auto max-h-48">
                                        <table class="table text-xs">
                                            <thead>
                                                <tr>
                                                    <th>"Time"</th>
                                                    <th>"Model"</th>
                                                    <th>"Backend"</th>
                                                    <th>"Cache"</th>
                                                    <th>"Latency"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {if tl.events.is_empty() {
                                                    view! {
                                                        <tr>
                                                            <td colspan="5">
                                                                <EmptyState message=t.live_no_data() />
                                                            </td>
                                                        </tr>
                                                    }.into_any()
                                                } else {
                                                    tl.events.iter().map(|e| {
                                                        view! {
                                                            <tr>
                                                                <td class="font-mono">{format!("{}", e.timestamp_ms)}</td>
                                                                <td>{e.model.clone()}</td>
                                                                <td>{e.backend_name.clone().unwrap_or_else(|| "—".to_string())}</td>
                                                                <td>
                                                                    {if e.cache_hit {
                                                                        view! { <Badge text="hit".to_string() color="teal" /> }
                                                                    } else {
                                                                        view! { <Badge text="miss".to_string() color="amber" /> }
                                                                    }}
                                                                </td>
                                                                <td class="font-mono">{format!("{:.1} ms", e.latency_ms)}</td>
                                                            </tr>
                                                        }
                                                    }).collect_view().into_any()
                                                }}
                                            </tbody>
                                        </table>
                                    </div>
                                </div>
                            }.into_any()
                        }
                    }}

                    {move || match key_routing.get() {
                        None => ().into_any(),
                        Some(Err(e)) => view! {
                            <p class="text-sm text-error">{e}</p>
                        }.into_any(),
                        Some(Ok(r)) => {
                            let (backend_labels, backend_values) = build_backend_distribution(&r);
                            let backend_labels_sig = Signal::derive(move || backend_labels.clone());
                            let backend_values_sig = Signal::derive(move || backend_values.clone());
                            view! {
                                <div class="space-y-3 border-t border-theme pt-3">
                                    <h3 class="text-sm font-semibold text-theme">{t.session_routing_title()}</h3>
                                    <p class="text-xs text-theme-muted font-mono">
                                        {format!("prefix_breaks={} migrations={}", r.prefix_break_count, r.migrations.len())}
                                    </p>
                                    <HorizontalBarChart
                                        labels=backend_labels_sig
                                        values=backend_values_sig
                                        width=400
                                        height_px=120
                                        empty_message=t.live_no_data()
                                    />
                                    <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                        {r.backends.iter().map(|b| {
                                            let hit_pct = b.cache_hit_rate * 100.0;
                                            let name = b.backend_name.clone();
                                            let affinity = b.affinity_kind.clone();
                                            let summary = t.session_backend_requests_fmt(
                                                b.request_count,
                                                hit_pct,
                                                b.avg_latency_ms,
                                            );
                                            view! {
                                                <div class="glass-card p-3 text-xs">
                                                    <p class="font-semibold text-theme truncate">{name}</p>
                                                    <p class="text-theme-muted font-mono mt-1">{summary}</p>
                                                    {affinity.map(|k| view! {
                                                        <p class="text-[10px] text-theme-muted mt-1">{k}</p>
                                                    })}
                                                </div>
                                            }
                                        }).collect_view()}
                                    </div>
                                    <div class="space-y-2">
                                        <p class="text-xs font-semibold text-warning">{t.session_migration_alerts()}</p>
                                        {if r.migrations.is_empty() {
                                            view! {
                                                <p class="text-xs text-theme-muted">{t.session_no_migrations()}</p>
                                            }.into_any()
                                        } else {
                                            r.migrations.iter().map(|m| {
                                                view! {
                                                    <div class="rounded border border-warning/40 bg-warning/10 px-2 py-1.5 text-xs">
                                                        <p class="text-warning font-medium">
                                                            {format!("{} → {}", m.from_backend, m.to_backend)}
                                                        </p>
                                                        <p class="font-mono text-theme-muted text-[10px]">{m.session_fingerprint.clone()}</p>
                                                    </div>
                                                }
                                            }).collect_view().into_any()
                                        }}
                                    </div>
                                </div>
                            }.into_any()
                        }
                    }}
                </div>
            </aside>
        </div>
    }
}
