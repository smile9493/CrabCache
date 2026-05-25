use crate::api::{fetch_capture_detail, fetch_capture_list, fetch_capture_stats};
use crate::types::{CaptureStatsResponse, RawCaptureEntry, StructureDiff};
use leptos::prelude::*;
use leptos::task::spawn_local;

#[derive(Clone, Default)]
struct CaptureListState {
    entries: Vec<RawCaptureEntry>,
    total: usize,
    loading: bool,
    error: Option<String>,
}

#[derive(Clone, Default)]
struct DetailState {
    request_id: String,
    client_body: Option<String>,
    upstream_body: Option<String>,
    entry: Option<RawCaptureEntry>,
    loading: bool,
    error: Option<String>,
}

#[derive(Clone, Default)]
struct StatsState {
    stats: Option<CaptureStatsResponse>,
    loading: bool,
    error: Option<String>,
}

fn fmt_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{} B", b)
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{:.2} MB", b as f64 / (1024.0 * 1024.0))
    }
}

fn fmt_delta(b: i64) -> String {
    if b >= 0 {
        format!("+{}", fmt_bytes(b as u64))
    } else {
        format!("-{}", fmt_bytes((-b) as u64))
    }
}

fn fmt_ts(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let naive = chrono::NaiveDateTime::from_timestamp_opt(secs, 0);
    match naive {
        Some(dt) => format!("{} UTC", dt.format("%Y-%m-%d %H:%M:%S")),
        None => {
            let h = ((ms / 1000) / 3600) % 24;
            let m = ((ms / 1000) % 3600) / 60;
            let s = (ms / 1000) % 60;
            format!("{:02}:{:02}:{:02}", h, m, s)
        }
    }
}

fn bool_icon(v: bool) -> &'static str {
    if v { "Yes" } else { "No" }
}

#[component]
fn StatsPanel(stats: Option<CaptureStatsResponse>, loading: bool) -> impl IntoView {
    if loading {
        return view! { <div class="text-sm text-[var(--text-muted)]">"Loading stats..."</div> }.into_any();
    }
    let Some(s) = stats else {
        return view! { <div></div> }.into_any();
    };
    view! {
        <div class="grid grid-cols-2 md:grid-cols-4 gap-3 mb-4">
            <StatCard label="Total Captures" value=s.total_captures.to_string()/>
            <StatCard label="Avg Delta" value=fmt_delta(s.avg_delta_bytes as i64)/>
            <StatCard label="Reasoning Injected" value=format!("{:.0}%", s.reasoning_injection_rate * 100.0)/>
            <StatCard label="Msg Count P99" value=s.message_count_p99.to_string()/>
            <StatCard label="Thinking Markup" value=format!("{:.0}%", s.thinking_markup_rate * 100.0)/>
            <StatCard label="Avg Client Body" value=fmt_bytes(s.avg_client_body_bytes as u64)/>
            <StatCard label="Avg Upstream Body" value=fmt_bytes(s.avg_upstream_body_bytes as u64)/>
        </div>
    }.into_any()
}

#[component]
fn StatCard(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="bg-[var(--bg-secondary)] rounded-lg p-3 border border-[var(--border-color)]">
            <div class="text-xs text-[var(--text-muted)] mb-1">{label}</div>
            <div class="text-lg font-semibold text-[var(--text-primary)]">{value}</div>
        </div>
    }
}

fn diff_row_class(highlight: bool) -> &'static str {
    if highlight {
        "border-b border-[var(--border-color)] bg-[var(--warning-bg)]"
    } else {
        "border-b border-[var(--border-color)]"
    }
}

fn diff_delta_class(highlight: bool) -> &'static str {
    if highlight {
        "py-1.5 px-2 text-right font-mono font-semibold text-[var(--warning-text)]"
    } else {
        "py-1.5 px-2 text-right font-mono text-[var(--text-muted)]"
    }
}

#[component]
fn StructureTable(diff: StructureDiff) -> impl IntoView {
    let client = diff.client;
    let upstream = diff.upstream;
    let rows: Vec<(&str, String, String, String, bool)> = vec![
        ("Messages", client.message_count.to_string(), upstream.message_count.to_string(), diff.delta_message_count.to_string(), diff.delta_message_count > 2),
        ("Content Chars", format!("{}", client.total_content_chars), format!("{}", upstream.total_content_chars), format!("{}", diff.delta_content_chars), diff.delta_content_chars > 10000),
        ("Reasoning Chars", format!("{}", client.total_reasoning_content_chars), format!("{}", upstream.total_reasoning_content_chars), format!("{}", diff.delta_reasoning_chars), diff.reasoning_was_injected),
        ("System Chars", format!("{}", client.system_chars), format!("{}", upstream.system_chars), format!("{}", diff.delta_system_chars), diff.delta_system_chars > 0),
        ("Tool Count", client.tool_count.to_string(), upstream.tool_count.to_string(), diff.delta_tool_count.to_string(), diff.delta_tool_count > 0),
        ("Thinking Markup", bool_icon(client.has_thinking_markup).to_string(), bool_icon(upstream.has_thinking_markup).to_string(), String::new(), upstream.has_thinking_markup && !client.has_thinking_markup),
    ];

    view! {
        <div class="overflow-x-auto">
            <table class="w-full text-xs">
                <thead>
                    <tr class="border-b border-[var(--border-color)]">
                        <th class="text-left py-2 px-2 text-[var(--text-muted)]">"Metric"</th>
                        <th class="text-right py-2 px-2 text-[var(--text-muted)]">"Client"</th>
                        <th class="text-right py-2 px-2 text-[var(--text-muted)]">"Upstream"</th>
                        <th class="text-right py-2 px-2 text-[var(--text-muted)]">"Delta"</th>
                    </tr>
                </thead>
                <tbody>
                    {rows.into_iter().map(|(label, cv, uv, delta, hl)| {
                        view! {
                            <tr class=diff_row_class(hl)>
                                <td class="py-1.5 px-2 text-[var(--text-secondary)]">{label}</td>
                                <td class="py-1.5 px-2 text-right font-mono">{cv}</td>
                                <td class="py-1.5 px-2 text-right font-mono">{uv}</td>
                                <td class=diff_delta_class(hl)>{delta}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

#[component]
fn JsonViewer(title: &'static str, content: Option<String>) -> impl IntoView {
    let Some(body) = content else {
        return view! {
            <div class="flex-1 min-w-0">
                <div class="text-xs font-semibold text-[var(--text-muted)] mb-1">{title}</div>
                <div class="text-sm text-[var(--text-muted)] italic">"No body captured"</div>
            </div>
        }.into_any();
    };
    let pretty = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or(body),
        Err(_) => body,
    };
    let size_str = fmt_bytes(pretty.len() as u64);
    view! {
        <div class="flex-1 min-w-0">
            <div class="text-xs font-semibold text-[var(--text-muted)] mb-1 flex items-center justify-between">
                <span>{title}</span>
                <span class="text-[var(--text-muted)] font-normal">{format!("({})", size_str)}</span>
            </div>
            <pre class="text-xs font-mono bg-[var(--bg-primary)] border border-[var(--border-color)] rounded p-3 overflow-auto max-h-[60vh] whitespace-pre-wrap break-all">{pretty}</pre>
        </div>
    }.into_any()
}

#[component]
pub fn CapturePage() -> impl IntoView {
    let list_state = RwSignal::new(CaptureListState::default());
    let detail_state = RwSignal::new(DetailState::default());
    let stats_state = RwSignal::new(StatsState::default());

    // Load list on mount.
    {
        let list_state = list_state;
        let stats_state = stats_state;
        spawn_local(async move {
            list_state.update(|s| s.loading = true);
            stats_state.update(|s| s.loading = true);

            match fetch_capture_list(24, Some(200), None, None, None).await {
                Ok(resp) => list_state.update(|s| {
                    s.entries = resp.entries;
                    s.total = resp.total;
                    s.loading = false;
                }),
                Err(e) => list_state.update(|s| {
                    s.error = Some(e);
                    s.loading = false;
                }),
            }
            match fetch_capture_stats(24).await {
                Ok(stats) => stats_state.update(|s| {
                    s.stats = Some(stats);
                    s.loading = false;
                }),
                Err(e) => stats_state.update(|s| {
                    s.error = Some(e);
                    s.loading = false;
                }),
            }
        });
    }

    let open_detail = move |request_id: String| {
        let detail_state = detail_state;
        spawn_local(async move {
            detail_state.update(|s| {
                s.loading = true;
                s.request_id = request_id.clone();
            });
            match fetch_capture_detail(&request_id).await {
                Ok(resp) => detail_state.update(|s| {
                    s.entry = Some(resp.entry);
                    s.client_body = resp.client_body;
                    s.upstream_body = resp.upstream_body;
                    s.loading = false;
                }),
                Err(e) => detail_state.update(|s| {
                    s.error = Some(e);
                    s.loading = false;
                }),
            }
        });
    };

    let close_detail = move |_: leptos::ev::MouseEvent| {
        detail_state.update(|s| *s = DetailState::default());
    };

    view! {
        <div class="space-y-4">
            // Stats
            {move || {
                let ss = stats_state.get();
                view! { <StatsPanel stats=ss.stats loading=ss.loading /> }
            }}

            // Capture list
            {move || {
                let ls = list_state.get();
                if ls.loading {
                    return view! { <div class="text-sm text-[var(--text-muted)]">"Loading captures..."</div> }.into_any();
                }
                if let Some(err) = &ls.error {
                    let err_msg = format!("Error: {}", err);
                    return view! { <div class="text-sm text-[var(--error-text)]">{err_msg}</div> }.into_any();
                }
                if ls.entries.is_empty() {
                    return view! {
                        <div class="text-sm text-[var(--text-muted)] text-center py-8">
                            "No captures found. Enable [raw_capture] in gateway config."
                        </div>
                    }.into_any();
                }
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-xs">
                            <thead>
                                <tr class="border-b border-[var(--border-color)] text-[var(--text-muted)]">
                                    <th class="text-left py-2 px-2">"Time"</th>
                                    <th class="text-left py-2 px-2">"Model"</th>
                                    <th class="text-left py-2 px-2">"Consumer"</th>
                                    <th class="text-right py-2 px-2">"Client"</th>
                                    <th class="text-right py-2 px-2">"Upstream"</th>
                                    <th class="text-right py-2 px-2">"Delta"</th>
                                    <th class="text-center py-2 px-2">"Reasoning"</th>
                                    <th class="text-center py-2 px-2">"Thinking"</th>
                                    <th class="text-center py-2 px-2">"Msgs"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {ls.entries.iter().map(|e| {
                                    let rid = e.request_id.clone();
                                    let rid2 = rid.clone();
                                    let has_reasoning = e.structure.upstream.has_reasoning_content;
                                    let has_thinking = e.structure.upstream.has_thinking_markup;
                                    let delta_class = if e.delta_bytes > 10000 {
                                        "py-1.5 px-2 text-right font-mono text-[var(--warning-text)] font-semibold"
                                    } else {
                                        "py-1.5 px-2 text-right font-mono text-[var(--text-muted)]"
                                    };
                                    let msg_count = e.structure.upstream.message_count;
                                    view! {
                                        <tr
                                            class="border-b border-[var(--border-color)] hover:bg-[var(--bg-hover)] cursor-pointer"
                                            on:click=move |_| open_detail(rid2.clone())
                                        >
                                            <td class="py-1.5 px-2 font-mono text-[var(--text-muted)]">{fmt_ts(e.timestamp_ms)}</td>
                                            <td class="py-1.5 px-2">{e.model.clone()}</td>
                                            <td class="py-1.5 px-2 text-[var(--text-muted)]">{e.consumer.clone().unwrap_or_default()}</td>
                                            <td class="py-1.5 px-2 text-right font-mono">{fmt_bytes(e.client_body_bytes)}</td>
                                            <td class="py-1.5 px-2 text-right font-mono">{fmt_bytes(e.upstream_body_bytes)}</td>
                                            <td class=delta_class>{fmt_delta(e.delta_bytes)}</td>
                                            <td class="py-1.5 px-2 text-center">{bool_icon(has_reasoning)}</td>
                                            <td class="py-1.5 px-2 text-center">{bool_icon(has_thinking)}</td>
                                            <td class="py-1.5 px-2 text-center font-mono">{msg_count}</td>
                                        </tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}

            // Detail drawer
            {move || {
                let ds = detail_state.get();
                if ds.request_id.is_empty() {
                    return view! { <div></div> }.into_any();
                }
                let req_id = ds.request_id.clone();
                view! {
                    <div class="fixed inset-0 z-50 flex">
                        <div class="absolute inset-0 bg-black/40" on:click=close_detail></div>
                        <div class="ml-auto w-full max-w-[90vw] bg-[var(--bg-secondary)] border-l border-[var(--border-color)] overflow-y-auto relative">
                            <div class="sticky top-0 bg-[var(--bg-secondary)] border-b border-[var(--border-color)] p-4 flex items-center justify-between z-10">
                                <div>
                                    <h3 class="text-sm font-semibold">"Capture Detail"</h3>
                                    <span class="text-xs font-mono text-[var(--text-muted)]">{req_id}</span>
                                </div>
                                <button class="btn btn-ghost text-xs" on:click=close_detail>"Close"</button>
                            </div>
                            <div class="p-4 space-y-4">
                                // Meta info
                                {if let Some(ref entry) = ds.entry {
                                    let retired = entry.retired_prefix_messages.map(|r| r.to_string()).unwrap_or_default();
                                    view! {
                                        <div class="grid grid-cols-2 md:grid-cols-4 gap-2 text-xs">
                                            <MetaItem label="Model" value=entry.model.clone()/>
                                            <MetaItem label="Consumer" value=entry.consumer.clone().unwrap_or_default()/>
                                            <MetaItem label="Client" value=fmt_bytes(entry.client_body_bytes)/>
                                            <MetaItem label="Upstream" value=fmt_bytes(entry.upstream_body_bytes)/>
                                            <MetaItem label="Delta" value=fmt_delta(entry.delta_bytes)/>
                                            <MetaItem label="Stream" value=bool_icon(entry.stream).to_string()/>
                                            <MetaItem label="Reasoning Strategy" value=entry.reasoning_strategy.clone().unwrap_or_default()/>
                                            <MetaItem label="Retired Prefix Msgs" value=retired/>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <div></div> }.into_any()
                                }}

                                // Structure diff table
                                {if let Some(ref entry) = ds.entry {
                                    view! { <StructureTable diff=entry.structure.clone()/> }.into_any()
                                } else {
                                    view! { <div></div> }.into_any()
                                }}

                                // JSON viewers
                                <div class="flex gap-4 flex-col md:flex-row">
                                    <JsonViewer title="Client Body" content=ds.client_body.clone()/>
                                    <JsonViewer title="Upstream Body" content=ds.upstream_body.clone()/>
                                </div>

                                // Error
                                {if let Some(ref err) = ds.error {
                                    let err_msg = format!("Error: {}", err);
                                    view! { <div class="text-sm text-[var(--error-text)]">{err_msg}</div> }.into_any()
                                } else {
                                    view! { <div></div> }.into_any()
                                }}
                            </div>
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn MetaItem(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div>
            <div class="text-[var(--text-muted)]">{label}</div>
            <div class="font-medium">{value}</div>
        </div>
    }
}
