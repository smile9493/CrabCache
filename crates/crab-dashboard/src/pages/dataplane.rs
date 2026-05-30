use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::locale::use_translations;

/// Global refresh interval (seconds).
const REFRESH_SECS: u64 = 10;

/// Data Plane diagnostics dashboard — SLO summary, phase latency, error attribution.
#[component]
pub fn DataPlanePage() -> impl IntoView {
    let tt = use_translations();

    let summary = RwSignal::new(serde_json::Value::Null);
    let phases = RwSignal::new(serde_json::Value::Null);
    let errors = RwSignal::new(serde_json::Value::Null);
    let slo = RwSignal::new(serde_json::Value::Null);
    let fetch_error = RwSignal::new(None::<String>);

    // Fetch all data plane endpoints
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
            spawn_local(async move {
                // Fetch summary
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/summary").await {
                    Ok(data) => summary.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                // Fetch phases
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/phases").await {
                    Ok(data) => phases.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                // Fetch errors
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/errors").await {
                    Ok(data) => errors.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
                // Fetch SLO
                match api::fetch_json::<serde_json::Value>("/api/admin/dataplane/slo").await {
                    Ok(data) => slo.set(data),
                    Err(e) => fetch_error.set(Some(e)),
                }
            });
        }
    };

    // Initial fetch
    fetch();

    // Refresh every REFRESH_SECS
    #[allow(unused_braces)]
    {
        let fetch = fetch;
        spawn_local(async move {
            loop {
                TimeoutFuture::new(REFRESH_SECS * 1000).await;
                fetch();
            }
        });
    }

    view! {
        <div class="space-y-6 p-4 md:p-6">
            <PageHeader title="Data Plane" subtitle="Real-time data plane observability: SLO, phase latency, error attribution" />

            {move || {
                if let Some(ref err) = fetch_error.get() {
                    return view! { <div class="bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 p-3 rounded-lg text-sm">{ err }</div> }.into_any();
                }
                view! {}.into_any()
            }}

            // ── SLO Summary Cards ──────────────────────────────────────
            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                {move || slo_card("Cache Hit Rate", summary.get().get("hit_rate_5m").and_then(|v| v.as_f64()).map(|v| format!("{:.1}%", v * 100.0)).unwrap_or_else(|| "—".to_string()), "Over last 5 minutes")}
                {move || slo_card("P95 E2E Latency", summary.get().get("e2e_p95_ms").and_then(|v| v.as_f64()).map(|v| format!("{:.1}ms", v)).unwrap_or_else(|| "—".to_string()), "End-to-end")}
                {move || slo_card("Error Rate", summary.get().get("error_rate").and_then(|v| v.as_f64()).map(|v| format!("{:.4}%", v * 100.0)).unwrap_or_else(|| "—".to_string()), "5xx / total requests")}
                {move || slo_card("Cost Saved", summary.get().get("cost_saved_usd").and_then(|v| v.as_f64()).map(|v| format!("${:.2}", v)).unwrap_or_else(|| "—".to_string()), "Cumulative USD")}
            </div>

            // ── SLO Compliance Cards ──────────────────────────────────
            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
                {move || slo_card("Sample Count (Ring)", slo.get().get("sample_count").and_then(|v| v.as_u64()).map(|v| format!("{}", v)).unwrap_or_else(|| "—".to_string()), "Metrics history ring")}
                {move || slo_card("QPS (5m)", slo.get().get("qps_5m").and_then(|v| v.as_f64()).map(|v| format!("{:.1}", v)).unwrap_or_else(|| "—".to_string()), "Avg queries/sec")}
                {move || slo_card("", "".to_string(), "")}
                {move || slo_card("Backend Count", summary.get().get("backend_count").and_then(|v| v.as_u64()).map(|v| format!("{}", v)).unwrap_or_else(|| "—".to_string()), "Active upstream backends")}
            </div>

            // ── Phase Latency Table ────────────────────────────────────
            <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-4">
                <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3">Phase Latency (ms)</h3>
                {move || {
                    let p = phases.get();
                    let phase_map = p.get("phases").and_then(|v| v.as_object()).cloned().unwrap_or_default();
                    let mut phase_names: Vec<&str> = phase_map.keys().map(|k| k.as_str()).collect();
                    phase_names.sort();

                    if phase_names.is_empty() {
                        return view! { <p class="text-gray-500 dark:text-gray-400 text-sm italic">No phase data available.</p> }.into_any();
                    }

                    view! {
                        <div class="overflow-x-auto">
                            <table class="min-w-full text-sm">
                                <thead>
                                    <tr class="border-b border-gray-200 dark:border-gray-700">
                                        <th class="text-left py-2 px-3 font-medium text-gray-600 dark:text-gray-300">Phase</th>
                                        <th class="text-right py-2 px-3 font-medium text-gray-600 dark:text-gray-300">P50 (ms)</th>
                                        <th class="text-right py-2 px-3 font-medium text-gray-600 dark:text-gray-300">P95 (ms)</th>
                                        <th class="text-right py-2 px-3 font-medium text-gray-600 dark:text-gray-300">P99 (ms)</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {phase_names.iter().map(|name| {
                                        let phase_data = phase_map.get(*name).and_then(|v| v.as_object());
                                        let p50 = phase_data.and_then(|m| m.get("p50_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                        let p95 = phase_data.and_then(|m| m.get("p95_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                        let p99 = phase_data.and_then(|m| m.get("p99_ms")).and_then(|v| v.as_f64()).map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".to_string());
                                        let bar_width_pct = p95.parse::<f64>().ok().map(|v| (v / 1000.0).min(1.0) * 100.0).unwrap_or(0.0);

                                        view! {
                                            <tr class="border-b border-gray-100 dark:border-gray-700/50 hover:bg-gray-50 dark:hover:bg-gray-700/30">
                                                <td class="py-2 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">{*name}</td>
                                                <td class="text-right py-2 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">{p50}</td>
                                                <td class="text-right py-2 px-3 font-mono text-xs text-blue-600 dark:text-blue-400 font-medium">
                                                    <div class="flex items-center justify-end gap-2">
                                                        <div class="w-16 h-1.5 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                                                            <div class="h-full bg-blue-500 rounded-full" style={ format!("width: {}%", bar_width_pct) }></div>
                                                        </div>
                                                        {p95}
                                                    </div>
                                                </td>
                                                <td class="text-right py-2 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">{p99}</td>
                                            </tr>
                                        }.into_any()
                                    }).collect::<Vec<_>>()}
                                </tbody>
                            </table>
                        </div>
                    }.into_any()
                }}
            </div>

            // ── Error Attribution ──────────────────────────────────────
            <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                // Rejection Reasons
                <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-4">
                    <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3">Rejection Reasons (total)</h3>
                    {move || {
                        let e = errors.get();
                        let reasons = e.get("rejection_reasons").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                        if reasons.is_empty() {
                            return view! { <p class="text-gray-500 dark:text-gray-400 text-sm italic">No rejections recorded.</p> }.into_any();
                        }
                        let max_count = reasons.iter().filter_map(|r| r.get("count").and_then(|c| c.as_u64())).max().unwrap_or(1);
                        view! {
                            <div class="space-y-2">
                                {reasons.into_iter().map(|r| {
                                    let reason = r.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                    let count = r.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                                    let pct = max_count as f64;

                                    view! {
                                        <div class="flex items-center gap-2">
                                            <span class="text-xs font-mono text-gray-600 dark:text-gray-400 w-36 truncate" title=&reason>{&reason}</span>
                                            <div class="flex-1 h-4 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                                                <div class="h-full bg-red-500 rounded-full" style={ format!("width: {}%", (count as f64 / pct) * 100.0) }></div>
                                            </div>
                                            <span class="text-xs font-mono text-gray-700 dark:text-gray-300 w-16 text-right">{count}</span>
                                        </div>
                                    }.into_any()
                                }).collect::<Vec<_>>()}
                            </div>
                        }.into_any()
                    }}
                </div>

                // Error Sources
                <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-4">
                    <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3">Error Sources (total)</h3>
                    {move || {
                        let e = errors.get();
                        let sources = e.get("error_sources").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                        if sources.is_empty() {
                            return view! { <p class="text-gray-500 dark:text-gray-400 text-sm italic">No error sources recorded.</p> }.into_any();
                        }
                        let max_count = sources.iter().filter_map(|s| s.get("count").and_then(|c| c.as_u64())).max().unwrap_or(1);
                        view! {
                            <div class="space-y-2">
                                {sources.into_iter().map(|s| {
                                    let source = s.get("source").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                    let count = s.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                                    view! {
                                        <div class="flex items-center gap-2">
                                            <span class="text-xs font-mono text-gray-600 dark:text-gray-400 w-36 truncate" title=&source>{&source}</span>
                                            <div class="flex-1 h-4 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                                                <div class="h-full bg-yellow-500 rounded-full" style={ format!("width: {}%", (count as f64 / max_count as f64) * 100.0) }></div>
                                            </div>
                                            <span class="text-xs font-mono text-gray-700 dark:text-gray-300 w-16 text-right">{count}</span>
                                        </div>
                                    }.into_any()
                                }).collect::<Vec<_>>()}
                            </div>
                        }.into_any()
                    }}
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
                    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-4">
                        <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3">PG Trace Errors (last 1h)</h3>
                        <div class="overflow-x-auto">
                            <table class="min-w-full text-sm">
                                <thead>
                                    <tr class="border-b border-gray-200 dark:border-gray-700">
                                        <th class="text-left py-2 px-3 font-medium text-gray-600 dark:text-gray-300">Error Code</th>
                                        <th class="text-left py-2 px-3 font-medium text-gray-600 dark:text-gray-300">Status</th>
                                        <th class="text-left py-2 px-3 font-medium text-gray-600 dark:text-gray-300">Upstream Result</th>
                                        <th class="text-right py-2 px-3 font-medium text-gray-600 dark:text-gray-300">Count</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {trace_errors.into_iter().map(|row| {
                                        let error_code = row.get("error_code").and_then(|v| v.as_str()).unwrap_or("—").to_string();
                                        let status_code = row.get("status_code").and_then(|v| v.as_i64()).map(|v| format!("{}", v)).unwrap_or_else(|| "—".to_string());
                                        let upstream_result = row.get("upstream_result").and_then(|v| v.as_str()).unwrap_or("—").to_string();
                                        let count = row.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                                        view! {
                                            <tr class="border-b border-gray-100 dark:border-gray-700/50">
                                                <td class="py-2 px-3 font-mono text-xs text-red-600 dark:text-red-400">{error_code}</td>
                                                <td class="py-2 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">{status_code}</td>
                                                <td class="py-2 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">{upstream_result}</td>
                                                <td class="text-right py-2 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">{count}</td>
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

/// Render a single SLO card.
fn slo_card(title: &str, value: String, subtitle: &str) -> impl IntoView {
    view! {
        <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-4">
            <p class="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">{title}</p>
            <p class="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">{value}</p>
            <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">{subtitle}</p>
        </div>
    }
}
