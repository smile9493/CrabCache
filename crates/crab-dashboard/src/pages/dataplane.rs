use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::components::ui::MetricCard;
use crate::locale::use_translations;

/// Global refresh interval (seconds).
const REFRESH_SECS: u64 = 10;

/// Data Plane diagnostics dashboard — SLO summary, phase latency, error attribution.
#[component]
pub fn DataPlanePage() -> impl IntoView {
    let t = use_translations();
    let summary = RwSignal::new(serde_json::Value::Null);
    let phases = RwSignal::new(serde_json::Value::Null);
    let errors = RwSignal::new(serde_json::Value::Null);
    let slo = RwSignal::new(serde_json::Value::Null);
    let fetch_error = RwSignal::new(None::<String>);

    let fetch = {
        let summary = summary;
        let phases = phases;
        let errors = errors;
        let slo = slo;
        let fetch_error = fetch_error;
        move || {
            let summary = summary;
            let phases = phases;
            let errors = errors;
            let slo = slo;
            let fetch_error = fetch_error;
            leptos::task::spawn_local(async move {
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/summary").await {
                    Ok(data) => summary.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/phases").await {
                    Ok(data) => phases.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/errors").await {
                    Ok(data) => errors.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/slo").await {
                    Ok(data) => slo.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
            });
        }
    };

    fetch();

    let fetch = fetch;
    leptos::task::spawn_local(async move {
        loop {
            TimeoutFuture::new((REFRESH_SECS * 1000) as u32).await;
            fetch();
        }
    });

    let is_loaded = Signal::derive(move || !summary.get().is_null());

    view! {
        <div class="page-content space-y-6">
            <PageHeader title=move || t.dataplane_page_title() description=move || t.dataplane_page_desc()>
                <div />
            </PageHeader>

            {move || {
                if let Some(ref err) = fetch_error.get() {
                    return view! { <div class="alert alert-error">{err.clone()}</div> }.into_any();
                }
                view! {}.into_any()
            }}

            {move || {
                if !is_loaded.get() {
                    return view! { <crate::components::ui::Spinner /> }.into_any();
                }
                view! {}.into_any()
            }}

            // ── SLO Summary Cards ──────────────────────────────────────
            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                <MetricCard
                    title=t.dataplane_cache_hit_rate()
                    value=Signal::derive(move || summary.get().get("hit_rate_5m").and_then(|v| v.as_f64()).map(|v| format!("{:.1}%", v * 100.0)).unwrap_or_else(|| "—".to_string()))
                    subtitle=t.dataplane_last_5min()
                />
                <MetricCard
                    title=t.dataplane_p95_e2e_latency()
                    value=Signal::derive(move || summary.get().get("e2e_p95_ms").and_then(|v| v.as_f64()).map(|v| format!("{:.1}ms", v)).unwrap_or_else(|| "—".to_string()))
                    subtitle=t.dataplane_end_to_end()
                />
                <MetricCard
                    title=t.dataplane_error_rate()
                    value=Signal::derive(move || summary.get().get("error_rate").and_then(|v| v.as_f64()).map(|v| format!("{:.4}%", v * 100.0)).unwrap_or_else(|| "—".to_string()))
                    subtitle=t.dataplane_5xx_of_total()
                />
                <MetricCard
                    title=t.dataplane_cost_saved()
                    value=Signal::derive(move || summary.get().get("cost_saved_usd").and_then(|v| v.as_f64()).map(|v| format!("${:.2}", v)).unwrap_or_else(|| "—".to_string()))
                    subtitle=t.dataplane_cumulative_usd()
                />
            </div>

            // ── SLO Compliance Cards ──────────────────────────────────
            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                <MetricCard
                    title=t.dataplane_sample_count_ring()
                    value=Signal::derive(move || slo.get().get("sample_count").and_then(|v| v.as_u64()).map(|v| format!("{}", v)).unwrap_or_else(|| "—".to_string()))
                    subtitle=t.dataplane_metrics_history_ring()
                />
                <MetricCard
                    title=t.dataplane_qps_5m()
                    value=Signal::derive(move || slo.get().get("qps_5m").and_then(|v| v.as_f64()).map(|v| format!("{:.1}", v)).unwrap_or_else(|| "—".to_string()))
                    subtitle=t.dataplane_avg_qps()
                />
                <div />
                <MetricCard
                    title=t.dataplane_backend_count()
                    value=Signal::derive(move || summary.get().get("backend_count").and_then(|v| v.as_u64()).map(|v| format!("{}", v)).unwrap_or_else(|| "—".to_string()))
                    subtitle=t.dataplane_active_backends()
                />
            </div>

            // ── Phase Latency Table ────────────────────────────────────
            <div class="dash-card">
                <div class="dash-card-header">
                    <span class="dash-card-title">{t.dataplane_phase_latency_ms()}</span>
                </div>
                <div class="dash-card-body">
                    {move || {
                        let p = phases.get();
                        let phase_map = p.get("phases").and_then(|v| v.as_object()).cloned().unwrap_or_default();
                        let mut phase_names: Vec<String> = phase_map.keys().map(|k| k.clone()).collect();
                        phase_names.sort();

                        if phase_names.is_empty() {
                            return view! { <p class="text-theme-muted text-sm italic">{t.dataplane_no_phase_data()}</p> }.into_any();
                        }

                        view! {
                            <div class="overflow-x-auto">
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th>{t.dataplane_phase()}</th>
                                            <th style="text-align: right">{t.dataplane_p50_ms()}</th>
                                            <th style="text-align: right">{t.dataplane_p95_ms()}</th>
                                            <th style="text-align: right">{t.dataplane_p99_ms()}</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {phase_names.iter().map(|name| {
                                            let phase_data = phase_map.get(name).and_then(|v| v.as_object());
                                            let p50 = phase_data.and_then(|m| m.get("p50_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                            let p95 = phase_data.and_then(|m| m.get("p95_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                            let p99 = phase_data.and_then(|m| m.get("p99_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                            let bar_width_pct = p95.parse::<f64>().ok().map(|v| (v / 1000.0).min(1.0) * 100.0).unwrap_or(0.0);

                                            view! {
                                                <tr>
                                                    <td style="font-family: var(--font-mono); font-size: 0.75rem">{name.to_string()}</td>
                                                    <td style="text-align: right; font-family: var(--font-mono); font-size: 0.75rem">{p50}</td>
                                                    <td style="text-align: right; font-family: var(--font-mono); font-size: 0.75rem; color: var(--cc-info); font-weight: 500">
                                                        <div style="display: flex; align-items: center; justify-content: flex-end; gap: 0.5rem">
                                                            <div style="width: 4rem; height: 0.375rem; background: var(--cc-bg-elevated); border-radius: 9999px; overflow: hidden">
                                                                <div style={format!("height: 100%; background: var(--cc-info); border-radius: 9999px; width: {}%", bar_width_pct)}></div>
                                                            </div>
                                                            {p95}
                                                        </div>
                                                    </td>
                                                    <td style="text-align: right; font-family: var(--font-mono); font-size: 0.75rem">{p99}</td>
                                                </tr>
                                            }.into_any()
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            </div>
                        }.into_any()
                    }}
                </div>
            </div>

            // ── Error Attribution ──────────────────────────────────────
            <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                // Rejection Reasons
                <div class="dash-card">
                    <div class="dash-card-header">
                        <span class="dash-card-title">{t.dataplane_rejection_reasons_total()}</span>
                    </div>
                    <div class="dash-card-body">
                        {move || {
                            let e = errors.get();
                            let reasons = e.get("rejection_reasons").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                            if reasons.is_empty() {
                                return view! { <p class="text-theme-muted text-sm italic">{t.dataplane_no_rejections()}</p> }.into_any();
                            }
                            let max_count = reasons.iter().filter_map(|r| r.get("count").and_then(|c| c.as_u64())).max().unwrap_or(1);
                            view! {
                                <div style="display: flex; flex-direction: column; gap: 0.5rem">
                                    {reasons.into_iter().map(|r| {
                                        let reason = r.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                        let count = r.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                                        let pct = max_count as f64;

                                        view! {
                                            <div style="display: flex; align-items: center; gap: 0.5rem">
                                                <span style="font-size: 0.75rem; font-family: var(--font-mono); color: var(--cc-text-muted); width: 9rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap" title=reason.clone()>{reason.clone()}</span>
                                                <div style="flex: 1; height: 1rem; background: var(--cc-bg-elevated); border-radius: 9999px; overflow: hidden">
                                                    <div style={format!("height: 100%; background: var(--cc-error); border-radius: 9999px; width: {}%", (count as f64 / pct) * 100.0)}></div>
                                                </div>
                                                <span style="font-size: 0.75rem; font-family: var(--font-mono); color: var(--cc-text); width: 4rem; text-align: right">{count}</span>
                                            </div>
                                        }.into_any()
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>

                // Error Sources
                <div class="dash-card">
                    <div class="dash-card-header">
                        <span class="dash-card-title">{t.dataplane_error_sources_total()}</span>
                    </div>
                    <div class="dash-card-body">
                        {move || {
                            let e = errors.get();
                            let sources = e.get("error_sources").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                            if sources.is_empty() {
                                return view! { <p class="text-theme-muted text-sm italic">{t.dataplane_no_error_sources()}</p> }.into_any();
                            }
                            let max_count = sources.iter().filter_map(|s| s.get("count").and_then(|c| c.as_u64())).max().unwrap_or(1);
                            view! {
                                <div style="display: flex; flex-direction: column; gap: 0.5rem">
                                    {sources.into_iter().map(|s| {
                                        let source = s.get("source").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                        let count = s.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                                        view! {
                                            <div style="display: flex; align-items: center; gap: 0.5rem">
                                                <span style="font-size: 0.75rem; font-family: var(--font-mono); color: var(--cc-text-muted); width: 9rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap" title=source.clone()>{source.clone()}</span>
                                                <div style="flex: 1; height: 1rem; background: var(--cc-bg-elevated); border-radius: 9999px; overflow: hidden">
                                                    <div style={format!("height: 100%; background: var(--cc-warning); border-radius: 9999px; width: {}%", (count as f64 / max_count as f64) * 100.0)}></div>
                                                </div>
                                                <span style="font-size: 0.75rem; font-family: var(--font-mono); color: var(--cc-text); width: 4rem; text-align: right">{count}</span>
                                            </div>
                                        }.into_any()
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>

            // ── Trace Error Top 10 ─────────────────────────────────────
            {move || {
                let e = errors.get();
                let trace_errors = e.get("trace_errors_top10").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                if trace_errors.is_empty() {
                    return view! {}.into_any();
                }
                view! {
                    <div class="dash-card">
                        <div class="dash-card-header">
                            <span class="dash-card-title">{t.dataplane_pg_trace_errors_1h()}</span>
                        </div>
                        <div class="dash-card-body-flush">
                            <div class="overflow-x-auto">
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th>{t.dataplane_error_code()}</th>
                                            <th>{t.dataplane_status()}</th>
                                            <th>{t.dataplane_upstream_result()}</th>
                                            <th style="text-align: right">{t.dataplane_count()}</th>
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
                                                    <td style="color: var(--cc-error); font-weight: 500">{error_code}</td>
                                                    <td>{status_code}</td>
                                                    <td>{upstream_result}</td>
                                                    <td style="text-align: right">{count}</td>
                                                </tr>
                                            }.into_any()
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </div>
                }.into_any()
            }}

        </div>
    }
}
