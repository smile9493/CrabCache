use std::collections::BTreeMap;

use leptos::prelude::*;

use crate::api;
use crate::components::canvas_line_chart::CanvasLineChart;
use crate::components::horizontal_bar_chart::HorizontalBarChart;
use crate::components::line_chart::ChartSeries;
use crate::components::ui::*;
use crate::types::{KeyRoutingResponse, SessionTimelineResponse};

fn minute_bucket_label(ts_ms: u64) -> String {
    let minute = (ts_ms / 1000) / 60;
    format!("t+{}m", minute)
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

fn build_backend_distribution(r: &KeyRoutingResponse) -> (Vec<String>, Vec<Option<f64>>) {
    let labels: Vec<String> = r.backends.iter().map(|b| b.backend_name.clone()).collect();
    let values: Vec<Option<f64>> = r
        .backends
        .iter()
        .map(|b| Some(b.request_count as f64))
        .collect();
    (labels, values)
}

#[component]
pub fn SessionMonitorPage() -> impl IntoView {
    let fingerprint = RwSignal::new(String::new());
    let selected_key = RwSignal::new(String::new());
    let timeline: RwSignal<Option<Result<SessionTimelineResponse, String>>> = RwSignal::new(None);
    let key_routing: RwSignal<Option<Result<KeyRoutingResponse, String>>> = RwSignal::new(None);

    let load_timeline = move || {
        let fp = fingerprint.get().trim().to_string();
        if fp.is_empty() {
            return;
        }
        timeline.set(None);
        leptos::task::spawn_local(async move {
            timeline.set(Some(api::fetch_session_timeline(&fp).await));
        });
    };

    let load_routing = move || {
        let key_id = selected_key.get().trim().to_string();
        if key_id.is_empty() {
            return;
        }
        key_routing.set(None);
        leptos::task::spawn_local(async move {
            key_routing.set(Some(api::fetch_key_routing(&key_id).await));
        });
    };

    view! {
        <div class="page-content space-y-6">
            <SectionHeader
                title="Session Monitor"
                description="Inspect per-session timeline and per-key routing distribution."
            />

            <div class="glass-card p-4 space-y-3">
                <div class="flex items-center gap-2">
                    <input
                        type="text"
                        class="input flex-1 font-mono text-sm"
                        placeholder="session fingerprint (e.g. sfp:xxxx)"
                        prop:value=move || fingerprint.get()
                        on:input=move |ev| fingerprint.set(event_target_value(&ev))
                    />
                    <button class="btn btn-primary text-sm" on:click=move |_| load_timeline()>
                        "Load Timeline"
                    </button>
                </div>
            </div>

            {move || match timeline.get() {
                None => view! { <div class="glass-card p-4 text-xs text-theme-muted">"Enter session fingerprint to load timeline."</div> }.into_any(),
                Some(Err(e)) => view! { <div class="glass-card text-error text-sm">{e}</div> }.into_any(),
                Some(Ok(tl)) => {
                    let (bucket_labels, bucket_values) = build_timeline_buckets(&tl);
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
                        <div class="glass-card p-4 space-y-3">
                            <div class="flex items-center justify-between">
                                <h3 class="text-sm font-semibold text-theme">"Session Timeline"</h3>
                                <span class="text-xs text-theme-muted">
                                    {format!("events: {}  window: {}s", tl.total_events, tl.window_secs)}
                                </span>
                            </div>
                            <div class="flex items-center gap-2">
                                <select
                                    class="input text-sm"
                                    prop:value=move || selected_key.get()
                                    on:change=move |ev| selected_key.set(event_target_value(&ev))
                                >
                                    <option value="">"Select key for routing view"</option>
                                    {tl.unique_keys.iter().map(|k| view! { <option value={k.clone()}>{k.clone()}</option> }).collect_view()}
                                </select>
                                <button class="btn btn-secondary text-sm" on:click=move |_| load_routing()>
                                    "Load Routing"
                                </button>
                            </div>
                            <div>
                                <p class="text-xs text-theme-muted mb-2">"Session events timeline (minute buckets)"</p>
                                <CanvasLineChart
                                    x_labels=bucket_labels_sig
                                    series=bucket_series_sig
                                    height_px=180
                                    y_unit="req"
                                    empty_message="No events in current window."
                                />
                            </div>
                            <div class="overflow-auto max-h-[26rem]">
                                <table class="table">
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
                                            view! { <tr><td colspan="5"><EmptyState message="No events in current window." /></td></tr> }.into_any()
                                        } else {
                                            tl.events.iter().map(|e| {
                                                view! {
                                                    <tr>
                                                        <td class="font-mono text-xs">{format!("{}", e.timestamp_ms)}</td>
                                                        <td>{e.model.clone()}</td>
                                                        <td>{e.backend_name.clone().unwrap_or_else(|| "unknown".to_string())}</td>
                                                        <td>
                                                            {if e.cache_hit {
                                                                view! { <Badge text="hit".to_string() color="teal" /> }
                                                            } else {
                                                                view! { <Badge text="miss".to_string() color="amber" /> }
                                                            }}
                                                        </td>
                                                        <td class="font-mono text-xs">{format!("{:.1} ms", e.latency_ms)}</td>
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
                Some(Err(e)) => view! { <div class="glass-card text-error text-sm">{e}</div> }.into_any(),
                Some(Ok(r)) => {
                    let backend_labels: Vec<String> = r.backends.iter().map(|b| b.backend_name.clone()).collect();
                    let backend_values: Vec<f64> = r.backends.iter().map(|b| b.request_count as f64).collect();
                    let backend_labels_sig = Signal::derive(move || backend_labels.clone());
                    let backend_values_sig = Signal::derive(move || backend_values.clone());
                    view! {
                        <div class="glass-card p-4 space-y-3">
                            <h3 class="text-sm font-semibold text-theme">"Routing Distribution"</h3>
                            <p class="text-xs text-theme-muted">
                                {format!("prefix breaks: {}  migrations: {}", r.prefix_break_count, r.migrations.len())}
                            </p>
                            <div>
                                <p class="text-xs text-theme-muted mb-2">"Backend distribution"</p>
                                <HorizontalBarChart
                                    labels=backend_labels_sig
                                    values=backend_values_sig
                                    width=520
                                    height_px=160
                                    empty_message="No backend routing data."
                                />
                            </div>
                            <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
                                {r.backends.iter().map(|b| view! {
                                    <div class="rounded border border-theme-border p-2">
                                        <p class="text-xs font-medium">{b.backend_name.clone()}</p>
                                        <p class="text-xs text-theme-muted">
                                            {format!("requests: {}  hit: {:.1}%  latency: {:.1}ms", b.request_count, b.cache_hit_rate * 100.0, b.avg_latency_ms)}
                                        </p>
                                    </div>
                                }).collect_view()}
                            </div>
                            <div class="space-y-2">
                                <p class="text-xs font-semibold text-amber-300">"Affinity migration alerts"</p>
                                {if r.migrations.is_empty() {
                                    view! { <p class="text-xs text-theme-muted">"No migration detected in current window."</p> }.into_any()
                                } else {
                                    view! {
                                        <div class="space-y-2">
                                            {r.migrations.iter().map(|m| {
                                                view! {
                                                    <div class="rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2">
                                                        <p class="text-xs text-amber-200 font-medium">
                                                            {format!("session {} moved {} -> {}", m.session_fingerprint, m.from_backend, m.to_backend)}
                                                        </p>
                                                        <p class="text-[11px] text-theme-muted font-mono">{format!("ts: {}", m.timestamp_ms)}</p>
                                                    </div>
                                                }
                                            }).collect_view()}
                                        </div>
                                    }.into_any()
                                }}
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
