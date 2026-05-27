use leptos::prelude::*;

use crate::api;
use crate::components::chart::waterfall_stages_from_log;
use crate::components::histogram_chart::HistogramChart;
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::components::waterfall::WaterfallChart;
use crate::locale::use_translations;
use crate::types::{LogsFilterQuery, RequestDetail, RequestLog};

fn cache_tier_color(status: &str) -> &'static str {
    match status {
        "L0" | "L0_MOKA" | "L0_moka" => "teal",
        "L1" | "L1_REDIS" | "L1_redis" => "amber",
        "L2" | "L2_SEMANTIC" | "L2_semantic" => "violet",
        _ => "rose",
    }
}

/// Draft/applied filter fields for the logs list (string inputs → parsed on fetch).
#[derive(Clone, Default, PartialEq)]
struct LogsFilterForm {
    model: String,
    consumer: String,
    cache_tier: String,
    request_hash: String,
    latency_min: String,
    latency_max: String,
    token_min: String,
    token_max: String,
    from_time: String,
    to_time: String,
}

impl LogsFilterForm {
    fn is_active(&self) -> bool {
        !self.model.is_empty()
            || !self.consumer.is_empty()
            || !self.cache_tier.is_empty()
            || !self.request_hash.is_empty()
            || !self.latency_min.is_empty()
            || !self.latency_max.is_empty()
            || !self.token_min.is_empty()
            || !self.token_max.is_empty()
            || !self.from_time.is_empty()
            || !self.to_time.is_empty()
    }

    fn to_query(&self, limit: usize, cursor: Option<String>) -> LogsFilterQuery {
        fn opt_str(s: &str) -> Option<String> {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        fn parse_f64(s: &str) -> Option<f64> {
            let t = s.trim();
            if t.is_empty() { None } else { t.parse().ok() }
        }
        fn parse_u64(s: &str) -> Option<u64> {
            let t = s.trim();
            if t.is_empty() { None } else { t.parse().ok() }
        }
        fn parse_datetime_to_ms(s: &str) -> Option<u64> {
            let t = s.trim();
            if t.is_empty() {
                return None;
            }
            // datetime-local format: "YYYY-MM-DDTHH:MM" → parse as Beijing time (UTC+8)
            let dt_str = if t.len() == 16 {
                format!("{}:00", t)
            } else {
                t.to_string()
            };
            chrono::NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|ndt| {
                    use chrono::TimeZone;
                    let beijing = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
                    beijing
                        .from_local_datetime(&ndt)
                        .unwrap()
                        .timestamp_millis() as u64
                })
        }
        LogsFilterQuery {
            limit: Some(limit),
            cursor,
            model: opt_str(&self.model),
            consumer: opt_str(&self.consumer),
            cache_tier: opt_str(&self.cache_tier),
            request_hash: opt_str(&self.request_hash),
            latency_min: parse_f64(&self.latency_min),
            latency_max: parse_f64(&self.latency_max),
            token_min: parse_u64(&self.token_min),
            token_max: parse_u64(&self.token_max),
            from_ms: parse_datetime_to_ms(&self.from_time),
            to_ms: parse_datetime_to_ms(&self.to_time),
        }
    }
}

#[component]
pub fn LogsPage() -> impl IntoView {
    let t = use_translations();
    let logs: RwSignal<Option<Result<Vec<RequestLog>, String>>> = RwSignal::new(None);
    let selected_id: RwSignal<Option<String>> = RwSignal::new(None);
    let selected_summary: RwSignal<Option<RequestLog>> = RwSignal::new(None);
    let detail: RwSignal<Option<Result<RequestDetail, String>>> = RwSignal::new(None);
    let detail_loading = RwSignal::new(false);
    let next_cursor: RwSignal<Option<String>> = RwSignal::new(None);
    let prev_cursors: RwSignal<Vec<Option<String>>> = RwSignal::new(Vec::new());
    let page_cursor: RwSignal<Option<String>> = RwSignal::new(None);
    let loading_more: RwSignal<bool> = RwSignal::new(false);
    let total_in_window: RwSignal<u64> = RwSignal::new(0);
    let has_next: RwSignal<bool> = RwSignal::new(false);
    let has_prev: RwSignal<bool> = RwSignal::new(false);
    let filter_draft = RwSignal::new(LogsFilterForm::default());
    let active_filter = RwSignal::new(LogsFilterForm::default());
    let page_generation = RwSignal::new(0u64);
    let show_advanced = RwSignal::new(false);

    let load_detail = move |id: String| {
        detail_loading.set(true);
        detail.set(None);
        leptos::task::spawn_local(async move {
            let result = api::fetch_log_detail(&id).await;
            detail.set(Some(result));
            detail_loading.set(false);
        });
    };

    let select_log = move |log: RequestLog| {
        let id = log.id.clone();
        selected_id.set(Some(id.clone()));
        selected_summary.set(Some(log));
        load_detail(id);
    };

    let fetch_page = move |cursor: Option<String>, reset_selection: bool| {
        page_generation.update(|g| *g += 1);
        let request_id = page_generation.get();
        let query = active_filter.get().to_query(100, cursor);
        leptos::task::spawn_local(async move {
            match api::fetch_logs(&query).await {
                Ok(resp) => {
                    if page_generation.get_untracked() != request_id {
                        return;
                    }
                    logs.set(Some(Ok(resp.items.clone())));
                    next_cursor.set(resp.next_cursor);
                    total_in_window.set(resp.total_in_window);
                    has_next.set(resp.has_more);
                    if reset_selection {
                        if let Some(first) = resp.items.into_iter().next() {
                            let id = first.id.clone();
                            selected_id.set(Some(id.clone()));
                            selected_summary.set(Some(first));
                            load_detail(id);
                        } else {
                            selected_id.set(None);
                            selected_summary.set(None);
                            detail.set(None);
                        }
                    }
                }
                Err(e) => {
                    if page_generation.get_untracked() == request_id {
                        logs.set(Some(Err(e)));
                    }
                }
            }
            loading_more.set(false);
        });
    };

    let reload_first_page = move || {
        next_cursor.set(None);
        prev_cursors.set(Vec::new());
        page_cursor.set(None);
        has_next.set(false);
        has_prev.set(false);
        selected_id.set(None);
        selected_summary.set(None);
        detail.set(None);
        fetch_page(None, true);
    };

    let apply_filters = move || {
        active_filter.set(filter_draft.get());
        reload_first_page();
    };

    let clear_filters = move || {
        filter_draft.set(LogsFilterForm::default());
        active_filter.set(LogsFilterForm::default());
        reload_first_page();
    };

    let filter_by_hash = move |hash: String| {
        let mut form = filter_draft.get();
        form.request_hash = hash;
        filter_draft.set(form.clone());
        active_filter.set(form);
        reload_first_page();
    };

    reload_first_page();

    let load_next = move || {
        if loading_more.get() {
            return;
        }
        let current_cursor = next_cursor.get();
        if current_cursor.is_none() {
            return;
        }
        loading_more.set(true);
        let cursor_for_fetch = current_cursor.clone();
        let prev_page = page_cursor.get();
        prev_cursors.update(|cursors| cursors.push(prev_page));
        page_cursor.set(cursor_for_fetch.clone());
        has_prev.set(true);
        fetch_page(cursor_for_fetch, false);
    };

    let load_prev = move || {
        if loading_more.get() {
            return;
        }
        let mut cursors = prev_cursors.get();
        let prev = cursors.pop();
        if prev.is_none() {
            return;
        }
        prev_cursors.set(cursors);
        loading_more.set(true);
        let cursor_for_fetch = prev.flatten();
        page_cursor.set(cursor_for_fetch.clone());
        has_prev.set(!prev_cursors.get().is_empty());
        fetch_page(cursor_for_fetch, false);
    };

    let page_info = Signal::derive(move || {
        let total = total_in_window.get();
        if total > 0 {
            format!("{} total", total)
        } else {
            String::new()
        }
    });

    view! {
        <div class="page-content logs-page space-y-6">
            <PageHeader
                title=move || t.logs_title()
                description=move || t.logs_desc()
            >
                <button
                    on:click=move |_| reload_first_page()
                    class="btn btn-secondary text-sm"
                >
                    {t.logs_refresh()}
                </button>
            </PageHeader>

            <div class="glass-card-flat logs-filter-bar">
                <input
                    type="text"
                    class="input"
                    placeholder=t.logs_filter_model().to_string()
                    prop:value=move || filter_draft.get().model
                    on:input=move |ev| filter_draft.update(|f| f.model = event_target_value(&ev))
                />
                <input
                    type="text"
                    class="input"
                    placeholder=t.logs_filter_consumer().to_string()
                    prop:value=move || filter_draft.get().consumer
                    on:input=move |ev| filter_draft.update(|f| f.consumer = event_target_value(&ev))
                />
                <select
                    class="input"
                    prop:value=move || filter_draft.get().cache_tier
                    on:change=move |ev| filter_draft.update(|f| f.cache_tier = event_target_value(&ev))
                >
                    <option value="">{t.logs_filter_cache_all()}</option>
                    <option value="L0_moka">"L0_moka"</option>
                    <option value="L1_redis">"L1_redis"</option>
                    <option value="L2_semantic">"L2_semantic"</option>
                </select>
                <div class="logs-filter-actions">
                    <button on:click=move |_| apply_filters() class="btn btn-primary text-xs">
                        {t.logs_filter_apply()}
                    </button>
                    <button
                        on:click=move |_| clear_filters()
                        class="btn btn-secondary text-xs"
                        disabled=move || !active_filter.get().is_active()
                    >
                        {t.logs_filter_clear()}
                    </button>
                    <button
                        on:click=move |_| show_advanced.update(|v| *v = !*v)
                        class="btn btn-secondary text-xs"
                    >
                        {t.logs_filter_advanced()}
                    </button>
                </div>
            </div>

            {move || if show_advanced.get() {
                view! {
                    <div class="glass-card-flat logs-filter-bar logs-filter-advanced">
                        <input
                            type="text"
                            class="input"
                            placeholder=t.logs_filter_hash().to_string()
                            prop:value=move || filter_draft.get().request_hash
                            on:input=move |ev| filter_draft.update(|f| f.request_hash = event_target_value(&ev))
                        />
                        <input
                            type="text"
                            class="input"
                            placeholder=t.logs_filter_latency_min().to_string()
                            prop:value=move || filter_draft.get().latency_min
                            on:input=move |ev| filter_draft.update(|f| f.latency_min = event_target_value(&ev))
                        />
                        <input
                            type="text"
                            class="input"
                            placeholder=t.logs_filter_latency_max().to_string()
                            prop:value=move || filter_draft.get().latency_max
                            on:input=move |ev| filter_draft.update(|f| f.latency_max = event_target_value(&ev))
                        />
                        <input
                            type="text"
                            class="input"
                            placeholder=t.logs_filter_token_min().to_string()
                            prop:value=move || filter_draft.get().token_min
                            on:input=move |ev| filter_draft.update(|f| f.token_min = event_target_value(&ev))
                        />
                        <input
                            type="text"
                            class="input"
                            placeholder=t.logs_filter_token_max().to_string()
                            prop:value=move || filter_draft.get().token_max
                            on:input=move |ev| filter_draft.update(|f| f.token_max = event_target_value(&ev))
                        />
                        <div class="flex flex-col gap-1">
                            <label class="text-xs text-theme-muted">{t.logs_filter_from_time()}</label>
                            <input
                                type="datetime-local"
                                class="input"
                                prop:value=move || filter_draft.get().from_time
                                on:input=move |ev| filter_draft.update(|f| f.from_time = event_target_value(&ev))
                            />
                        </div>
                        <div class="flex flex-col gap-1">
                            <label class="text-xs text-theme-muted">{t.logs_filter_to_time()}</label>
                            <input
                                type="datetime-local"
                                class="input"
                                prop:value=move || filter_draft.get().to_time
                                on:input=move |ev| filter_draft.update(|f| f.to_time = event_target_value(&ev))
                            />
                        </div>
                    </div>
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}

            // Latency distribution histogram
            {move || {
                if let Some(Ok(ref log_list)) = logs.get() {
                    let latencies: Vec<f64> = log_list.iter().map(|l| l.latency_ms as f64).filter(|v| v.is_finite() && *v > 0.0).collect();
                    if !latencies.is_empty() {
                        let stored = StoredValue::new(latencies);
                        let latency_sig = Signal::derive(move || stored.get_value());
                        view! {
                            <div class="glass-card p-4 space-y-3">
                                <h3 class="text-sm font-semibold text-theme">{t.logs_latency_distribution()}</h3>
                                <HistogramChart
                                    values=latency_sig
                                    bin_count=15
                                    height_px=150
                                    y_unit="req"
                                    empty_message=""
                                />
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            {move || match logs.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().logs_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(log_list)) => {
                    if log_list.is_empty() {
                        let empty_msg = if active_filter.get().is_active() {
                            t.logs_no_results()
                        } else {
                            t.logs_empty()
                        };
                        view! { <EmptyState message=empty_msg /> }.into_any()
                    } else {
                        let has_next_val = has_next.get();
                        let has_prev_val = has_prev.get();
                        let loading = loading_more.get();
                        let page_str = page_info.get();
                        view! {
                            <div class="logs-split">
                                <div class="logs-split-list glass-card-flat">
                                    <table class="table table-compact">
                                        <thead>
                                            <tr>
                                                <th>{t.logs_col_time()}</th>
                                                <th>{t.logs_col_model()}</th>
                                                <th class="hidden md:table-cell">{t.logs_col_input_tokens()}</th>
                                                <th class="hidden md:table-cell">{t.logs_col_output_tokens()}</th>
                                                <th>{t.logs_col_cache()}</th>
                                                <th class="text-right">{t.logs_col_latency()}</th>
                                                <th class="hidden md:table-cell text-right">{t.logs_col_ttft()}</th>
                                                <th class="hidden md:table-cell text-right">{t.logs_col_len()}</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {log_list.into_iter().map(|log| {
                                                let log_for_click = log.clone();
                                                let id = log.id.clone();
                                                let is_selected = move || {
                                                    selected_id.get().as_deref() == Some(id.as_str())
                                                };
                                                let cache_color = cache_tier_color(&log.cache_status);
                                                view! {
                                                    <tr
                                                        class=move || {
                                                            if is_selected() {
                                                                "logs-row-selected"
                                                            } else {
                                                                "logs-row"
                                                            }
                                                        }
                                                        on:click=move |_| select_log(log_for_click.clone())
                                                    >
                                                        <td class="text-xs font-mono text-theme-secondary">
                                                            {log.timestamp.clone()}
                                                        </td>
                                                        <td class="text-sm text-theme truncate max-w-[8rem]">
                                                            {log.model.clone()}
                                                        </td>
                                                        <td class="hidden md:table-cell text-xs font-mono tabular-nums text-theme-secondary">
                                                            {log.input_tokens.map(|t| format!("{}", t)).unwrap_or_default()}
                                                        </td>
                                                        <td class="hidden md:table-cell text-xs font-mono tabular-nums text-theme-secondary">
                                                            {log.output_tokens.map(|t| format!("{}", t)).unwrap_or_default()}
                                                        </td>
                                                        <td>
                                                            <Badge text=log.cache_status.clone() color=cache_color />
                                                        </td>
                                                        <td class="text-right text-xs font-mono tabular-nums text-theme">
                                                            {format!("{}ms", log.latency_ms)}
                                                        </td>
                                                        <td class="hidden md:table-cell text-right text-xs font-mono tabular-nums text-theme-secondary">
                                                            {log.ttft_ms.map(|ms| format!("{:.0}ms", ms)).unwrap_or_default()}
                                                        </td>
                                                        <td class="hidden md:table-cell text-right text-xs font-mono tabular-nums text-theme-secondary">
                                                            {log.content_length.map(|l| format!("{}B", l)).unwrap_or_default()}
                                                        </td>
                                                    </tr>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </tbody>
                                    </table>
                                    <div class="flex items-center justify-between px-4 py-3 border-t border-theme">
                                        <span class="text-xs text-theme-muted">{page_str}</span>
                                        <div class="flex items-center gap-2">
                                            <button
                                                on:click=move |_| load_prev()
                                                disabled=!has_prev_val || loading
                                                class="btn btn-secondary text-sm"
                                            >
                                                "←"
                                            </button>
                                            <button
                                                on:click=move |_| load_next()
                                                disabled=!has_next_val || loading
                                                class="btn btn-secondary text-sm"
                                            >
                                                {if loading {
                                                    t.logs_loading()
                                                } else {
                                                    "→"
                                                }}
                                            </button>
                                        </div>
                                    </div>
                                </div>

                                <div class="logs-split-detail glass-card">
                                    {move || {
                                        if let Some(summary) = selected_summary.get() {
                                            view! {
                                                <LogDetailPane
                                                    summary=summary
                                                    detail=detail
                                                    loading=detail_loading
                                                    on_filter_hash=Callback::new(filter_by_hash)
                                                />
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div class="logs-detail-empty">
                                                    <p class="text-sm text-theme-muted">{t.logs_select_hint()}</p>
                                                </div>
                                            }.into_any()
                                        }
                                    }}
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}

#[component]
fn LogDetailPane(
    summary: RequestLog,
    detail: RwSignal<Option<Result<RequestDetail, String>>>,
    loading: RwSignal<bool>,
    on_filter_hash: Callback<String>,
) -> impl IntoView {
    let t = use_translations();
    let cache_color = cache_tier_color(&summary.cache_status);

    view! {
        <div class="logs-detail-inner">
            <div class="logs-detail-header">
                <h3 class="text-sm font-semibold text-theme">{t.logs_detail_title()}</h3>
                <div class="flex items-center gap-2">
                    <Badge text=summary.cache_status.clone() color=cache_color />
                    <ExportButton summary=summary.clone() detail=detail />
                </div>
            </div>

            <div class="logs-detail-meta">
                <DetailField label=t.logs_detail_timestamp() value=summary.timestamp.clone() />
                <DetailField label=t.logs_detail_model() value=summary.model.clone() />
                <DetailField label=t.logs_detail_consumer() value=summary.consumer.clone() />
                <DetailField label=t.logs_detail_latency() value=format!("{}ms", summary.latency_ms) />
                <DetailField label=t.logs_detail_tokens() value=format!("{}", summary.total_tokens) />
                {summary.project_id.clone().map(|v| view! {
                    <DetailField label=t.logs_detail_project_id() value=v />
                })}
                {summary.upstream_user_id.clone().map(|v| view! {
                    <DetailField label=t.logs_detail_upstream_user_id() value=v />
                })}
                {summary.user_id_audit.clone().map(|v| view! {
                    <DetailField label=t.logs_detail_user_id_audit() value=v />
                })}
                {summary.upstream_key_id.clone().map(|v| view! {
                    <DetailField label="Upstream Key" value=v />
                })}
            </div>

            {move || {
                if loading.get() {
                    view! {
                        <div class="logs-detail-skeleton space-y-4 p-2">
                            <div class="skeleton-line w-1/3 h-4 rounded"></div>
                            <div class="grid grid-cols-2 gap-4">
                                <div><div class="skeleton-line w-full h-3 rounded mb-1"></div><div class="skeleton-line w-3/4 h-4 rounded"></div></div>
                                <div><div class="skeleton-line w-full h-3 rounded mb-1"></div><div class="skeleton-line w-1/2 h-4 rounded"></div></div>
                            </div>
                            <div class="grid grid-cols-2 gap-4">
                                <div><div class="skeleton-line w-full h-3 rounded mb-1"></div><div class="skeleton-line w-2/3 h-4 rounded"></div></div>
                                <div><div class="skeleton-line w-full h-3 rounded mb-1"></div><div class="skeleton-line w-3/5 h-4 rounded"></div></div>
                            </div>
                            <div class="space-y-2">
                                <div class="skeleton-line w-1/4 h-4 rounded"></div>
                                <div class="skeleton-line w-full h-20 rounded"></div>
                            </div>
                            <div class="space-y-2">
                                <div class="skeleton-line w-1/5 h-4 rounded"></div>
                                <div class="skeleton-line w-full h-24 rounded"></div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    match detail.get() {
                        None => view! {
                            <div class="text-sm text-theme-muted">"…"</div>
                        }.into_any(),
                        Some(Err(e)) => view! {
                            <div class="text-sm text-error">{e}</div>
                        }.into_any(),
                        Some(Ok(d)) => {
                            let upstream_str = d.upstream_latency_ms
                                .map(|ms| format!("{:.1}ms", ms))
                                .unwrap_or_default();
                            let ttft_str = d.ttft_ms
                                .map(|ms| format!("{:.1}ms", ms))
                                .unwrap_or_default();
                            let input_str = d.input_tokens
                                .map(|t| format!("{}", t))
                                .unwrap_or_default();
                            let output_str = d.output_tokens
                                .map(|t| format!("{}", t))
                                .unwrap_or_default();
                            let waterfall_stages = waterfall_stages_from_log(&summary, &d);
                            let has_waterfall = waterfall_stages.len() >= 2;
                            view! {
                                <div class="logs-detail-sections space-y-4">
                                    <div class="grid grid-cols-2 gap-4">
                                        <DetailField label=t.logs_detail_route() value=d.route_backend.clone() />
                                        <DetailField label=t.logs_detail_cache_path() value=d.cache_path.clone() />
                                    </div>

                                    // Latency breakdown: WaterfallChart when >= 2 stages, else DetailField rows
                                    {if has_waterfall {
                                        view! {
                                            <div>
                                                <h4 class="text-xs font-semibold text-theme mb-1">
                                                    {t.logs_detail_latency_waterfall()}
                                                </h4>
                                                <p class="text-[10px] text-theme-muted mb-2">
                                                    {t.logs_detail_latency_waterfall_note()}
                                                </p>
                                                <WaterfallChart stages=waterfall_stages />
                                            </div>
                                        }.into_any()
                                    } else if !upstream_str.is_empty() || !ttft_str.is_empty() {
                                        view! {
                                            <div class="grid grid-cols-2 gap-4">
                                                {if !upstream_str.is_empty() {
                                                    view! { <DetailField label=t.logs_detail_upstream_latency() value=upstream_str.clone() /> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                                {if !ttft_str.is_empty() {
                                                    view! { <DetailField label=t.logs_detail_ttft() value=ttft_str.clone() /> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                            </div>
                                        }.into_any()
                                    } else { view! { <span></span> }.into_any() }}

                                    // Input/Output token breakdown
                                    {if !input_str.is_empty() || !output_str.is_empty() {
                                        view! {
                                            <div class="grid grid-cols-2 gap-4">
                                                {if !input_str.is_empty() {
                                                    view! { <DetailField label=t.logs_detail_input_tokens() value=input_str.clone() /> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                                {if !output_str.is_empty() {
                                                    view! { <DetailField label=t.logs_detail_output_tokens() value=output_str.clone() /> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                            </div>
                                        }.into_any()
                                    } else { view! { <span></span> }.into_any() }}

                                    // Diagnostics section
                                    <div class="diagnostics-section">
                                        <h4 class="text-xs font-semibold text-theme mb-1">{t.logs_detail_diagnostics()}</h4>
                                        <div class="grid grid-cols-2 gap-4">
                                            {if let Some(ref hash) = d.request_hash {
                                                let hash_short = hash[..16.min(hash.len())].to_string();
                                                let hash_full = hash.clone();
                                                view! {
                                                    <div class="detail-field">
                                                        <div class="detail-field-label">{t.logs_detail_request_hash()}</div>
                                                        <button
                                                            type="button"
                                                            class="logs-hash-link"
                                                            title=t.logs_filter_hash_click()
                                                            on:click=move |ev| {
                                                                ev.stop_propagation();
                                                                on_filter_hash.run(hash_full.clone());
                                                            }
                                                        >
                                                            {hash_short}
                                                        </button>
                                                    </div>
                                                }.into_any()
                                            } else { view! { <span></span> }.into_any() }}
                                            {if let Some(cluster) = d.semantic_cluster {
                                                view! {
                                                    <DetailField label=t.logs_detail_semantic_cluster() value=format!("#{}", cluster) />
                                                }.into_any()
                                            } else { view! { <span></span> }.into_any() }}
                                        </div>
                                    </div>

                                    <div>
                                        <h4 class="text-xs font-semibold text-theme mb-1">{t.logs_detail_payload()}</h4>
                                        <JsonBlock json_str=d.request_payload.clone() />
                                    </div>

                                    <div>
                                        <h4 class="text-xs font-semibold text-theme mb-1">{t.logs_detail_response()}</h4>
                                        <div class="text-xs text-theme-muted mb-1">
                                            {if d.response_body == "(未启用 body 采集)" || d.response_body.is_empty() {
                                                "(未启用 body 采集)"
                                            } else {
                                                "（预览内容，可能因截断而不完整）"
                                            }}
                                        </div>
                                        <JsonBlock json_str=d.response_body.clone() />
                                    </div>
                                </div>
                            }.into_any()
                        },
                    }
                }
            }}
        </div>
    }
}

/// A foldable section for a chat message (system/user/assistant).
#[component]
fn CollapsibleMessage(msg_role: String, content: String) -> impl IntoView {
    let expanded = RwSignal::new(false);

    let role_color = match msg_role.as_str() {
        "system" => "text-amber-400",
        "user" => "text-sky-400",
        "assistant" => "text-emerald-400",
        _ => "text-theme",
    };

    let char_count = content.chars().count();
    let preview = if char_count > 120 {
        let truncated: String = content.chars().take(120).collect();
        format!("{}...", truncated)
    } else {
        content.clone()
    };

    view! {
        <div class="collapsible-message border border-theme-border rounded mb-2 overflow-hidden">
            <button
                class="collapsible-message-header flex items-center w-full px-3 py-2 text-xs font-mono gap-2 hover:bg-theme-hover transition-colors"
                on:click=move |_| expanded.update(|v| *v = !*v)
            >
                <span class={move || if expanded.get() { "rotate-90" } else { "" } }>
                    "▶"
                </span>
                <span class=role_color>{msg_role.clone()}</span>
                <span class="text-theme-muted truncate">{preview}</span>
            </button>
            {move || if expanded.get() {
                view! {
                    <div class="px-3 py-2 border-t border-theme-border">
                        <pre class="text-xs text-theme leading-relaxed whitespace-pre-wrap break-words">{content.clone()}</pre>
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }}
        </div>
    }
}

/// Renders a JSON string with syntax highlighting and messages folding.
/// When parsing fails or the content is a metadata summary (not real request body),
/// falls back to raw `<pre>` display.
#[component]
fn JsonBlock(json_str: String) -> impl IntoView {
    let is_body_not_enabled = json_str == "(未启用 body 采集)" || json_str.is_empty();
    if is_body_not_enabled {
        return view! {
            <div class="text-xs text-theme-muted italic px-2 py-4">"(未启用 body 采集)"</div>
        }
        .into_any();
    }

    // Try to parse as JSON
    match serde_json::from_str::<serde_json::Value>(&json_str) {
        Ok(val) => {
            // Check for messages array
            let has_messages = val.get("messages").and_then(|m| m.as_array()).is_some();
            let is_metadata_summary = val.get("request_hash").is_some()
                && val.get("content_length").is_some()
                && val.get("cache_hit").is_some();
            let is_chat_response = val.get("choices").and_then(|c| c.as_array()).is_some()
                || val.get("object").and_then(|o| o.as_str()) == Some("chat.completion");

            if is_metadata_summary {
                render_json_highlighted(val, 0).into_any()
            } else if has_messages {
                render_chat_request(val).into_any()
            } else if is_chat_response {
                render_chat_response(val).into_any()
            } else {
                render_json_highlighted(val, 0).into_any()
            }
        }
        Err(_) => {
            // Not JSON, render as plain text
            view! {
                <pre class="logs-pre text-xs">{json_str.clone()}</pre>
            }
            .into_any()
        }
    }
}

/// Render a parsed JSON value with CSS-based syntax highlighting (no WASM deps).
/// Uses colored spans for keys, strings, numbers, booleans, null.
fn render_json_highlighted(value: serde_json::Value, depth: usize) -> impl IntoView {
    if depth > 8 {
        // Prevent stack overflow on deeply nested objects
        return view! { <span class="text-theme-muted">"..."</span> }.into_any();
    }

    match value {
        serde_json::Value::Null => view! { <span class="json-null">"null"</span> }.into_any(),
        serde_json::Value::Bool(b) => {
            if b {
                view! { <span class="json-boolean-true">"true"</span> }.into_any()
            } else {
                view! { <span class="json-boolean-false">"false"</span> }.into_any()
            }
        }
        serde_json::Value::Number(n) => {
            view! { <span class="json-number">{n.to_string()}</span> }.into_any()
        }
        serde_json::Value::String(s) => {
            view! { <span class="json-string">"\"" {s} "\""</span> }.into_any()
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                return view! { <span class="json-bracket">"[]"</span> }.into_any();
            }
            let len = arr.len();
            let items: Vec<_> = arr.into_iter()
                .enumerate()
                .map(|(i, v)| {
                    let val_view = render_json_highlighted(v, depth + 1);
                    view! {
                        <div class="json-line">
                            {val_view}
                            {if i < len - 1 { view! { <span class="json-comma">","</span> }.into_any() } else { view! { <span></span> }.into_any() }}
                        </div>
                    }
                })
                .collect();
            view! {
                <div class="json-block">
                    <span class="json-bracket">"["</span>
                    <div class="json-indent">{items}</div>
                    <span class="json-bracket">"]"</span>
                </div>
            }
            .into_any()
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                return view! { <span class="json-bracket">"{}"</span> }.into_any();
            }
            let len = obj.len();
            let entries: Vec<_> = obj.into_iter()
                .enumerate()
                .map(|(i, (k, v))| {
                    let val_view = render_json_highlighted(v, depth + 1);
                    view! {
                        <div class="json-line">
                            <span class="json-key">"\"" {k} "\""</span>
                            <span class="json-colon">": "</span>
                            {val_view}
                            {if i < len - 1 { view! { <span class="json-comma">","</span> }.into_any() } else { view! { <span></span> }.into_any() }}
                        </div>
                    }
                })
                .collect();
            view! {
                <div class="json-block">
                    <span class="json-bracket">"{"</span>
                    <div class="json-indent">{entries}</div>
                    <span class="json-bracket">"}"</span>
                </div>
            }
            .into_any()
        }
    }
}

/// Render a chat completion request with foldable messages by role.
fn render_chat_request(val: serde_json::Value) -> impl IntoView {
    let model = val
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    let stream = val.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let has_tools = val.get("tools").is_some() || val.get("functions").is_some();
    let messages: Vec<(String, String)> = val
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|msg| {
                    let role = msg
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown");
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    if role.is_empty() && content.is_empty() {
                        None
                    } else {
                        Some((role.to_string(), content.to_string()))
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    view! {
        <div class="json-chat-request space-y-2">
            {if let Some(ref model_name) = model {
                view! {
                    <div class="flex gap-4 text-xs text-theme-muted mb-2">
                        <span>"model: "<span class="text-theme">{model_name.clone()}</span></span>
                        <span>"stream: "<span class="text-theme">{if stream { "true" } else { "false" }}</span></span>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="flex gap-4 text-xs text-theme-muted mb-2">
                        <span>"stream: "<span class="text-theme">{if stream { "true" } else { "false" }}</span></span>
                    </div>
                }.into_any()
            }}
            <div class="text-xs text-theme-muted mb-1">{format!("messages [{}]", messages.len())}</div>
            {messages.into_iter().map(|(msg_role, content)| {
                view! {
                    <CollapsibleMessage msg_role content />
                }
            }).collect::<Vec<_>>()}
            {if has_tools {
                view! {
                    <div class="text-xs text-theme-muted mt-2 italic">"(tools/functions present)"</div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}
        </div>
    }
}

/// Render a chat completion response with choices.
fn render_chat_response(val: serde_json::Value) -> impl IntoView {
    let model = val
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    let usage_str = val.get("usage").map(|u| {
        format!(
            "{} in / {} out / {} total",
            u.get("prompt_tokens").and_then(|t| t.as_u64()).unwrap_or(0),
            u.get("completion_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0),
            u.get("total_tokens").and_then(|t| t.as_u64()).unwrap_or(0),
        )
    });
    // Pre-extract choice data into owned strings to avoid lifetime issues.
    let choices_data: Vec<(i64, String, String, String)> = val
        .get("choices")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .map(|choice| {
                    let idx = choice.get("index").and_then(|i| i.as_i64()).unwrap_or(0);
                    let finish = choice
                        .get("finish_reason")
                        .and_then(|f| f.as_str())
                        .unwrap_or("")
                        .to_string();
                    let message = choice.get("message");
                    let delta = choice.get("delta");
                    let content = message
                        .or(delta)
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let reasoning = message
                        .or(delta)
                        .and_then(|m| m.get("reasoning_content"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    (idx, finish, content, reasoning)
                })
                .collect()
        })
        .unwrap_or_default();

    view! {
        <div class="json-chat-response space-y-2">
            {if let Some(ref model_name) = model {
                view! { <div class="text-xs text-theme-muted">"model: "<span class="text-theme">{model_name.clone()}</span></div> }.into_any()
            } else { view! { <span></span> }.into_any() }}
            {choices_data.into_iter().map(|(idx, finish, content, reasoning)| {
                view! {
                    <div class="border border-theme-border rounded px-3 py-2">
                        <div class="flex gap-3 text-xs text-theme-muted mb-1">
                            <span>{format!("#[{}]", idx)}</span>
                            {if !finish.is_empty() { view! { <span>"finish: "{finish}</span> }.into_any() } else { view! { <span></span> }.into_any() }}
                        </div>
                        {if !content.is_empty() {
                            view! { <pre class="text-xs text-theme whitespace-pre-wrap break-words">{content}</pre> }.into_any()
                        } else { view! { <span></span> }.into_any() }}
                        {if !reasoning.is_empty() {
                            view! {
                                <details class="mt-1">
                                    <summary class="text-xs text-theme-muted cursor-pointer">"reasoning_content"</summary>
                                    <pre class="text-xs text-amber-400/70 whitespace-pre-wrap break-words mt-1">{reasoning}</pre>
                                </details>
                            }.into_any()
                        } else { view! { <span></span> }.into_any() }}
                    </div>
                }
            }).collect::<Vec<_>>()}
            {if let Some(ref usage_str) = usage_str {
                view! {
                    <div class="text-xs text-theme-muted mt-2">
                        "usage: "<span class="text-theme">{usage_str.clone()}</span>
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}
        </div>
    }
}

/// Export button that downloads the full detail as a JSON file.
#[component]
fn ExportButton(
    summary: RequestLog,
    detail: RwSignal<Option<Result<RequestDetail, String>>>,
) -> impl IntoView {
    let handle_export = move |_| {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::JsValue;
        let detail_val = detail.get();
        if let Some(Ok(ref d)) = detail_val {
            let export = serde_json::json!({
                "summary": {
                    "id": summary.id,
                    "timestamp": summary.timestamp,
                    "model": summary.model,
                    "consumer": summary.consumer,
                    "latency_ms": summary.latency_ms,
                    "total_tokens": summary.total_tokens,
                    "cache_status": summary.cache_status,
                },
                "detail": {
                    "cache_path": d.cache_path,
                    "route_backend": d.route_backend,
                    "request_payload": d.request_payload,
                    "response_body": d.response_body,
                },
            });
            let json_str = serde_json::to_string_pretty(&export).unwrap_or_default();
            let blob = web_sys::Blob::new_with_str_sequence(&JsValue::from_str(&json_str));
            if let Ok(blob) = blob {
                let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap_or_default();
                let window = web_sys::window().expect("window");
                let doc = window.document().expect("document");
                let a = doc.create_element("a").expect("a");
                let html_a: web_sys::HtmlElement = a.unchecked_into();
                html_a.set_attribute("href", &url).ok();
                html_a
                    .set_attribute("download", &format!("request-{}.json", summary.id))
                    .ok();
                html_a.set_attribute("style", "display:none").ok();
                doc.body().unwrap().append_child(&html_a).ok();
                html_a.click();
                doc.body().unwrap().remove_child(&html_a).ok();
                web_sys::Url::revoke_object_url(&url).ok();
            }
        }
    };

    let disabled = move || detail.get().map(|r| r.is_err()).unwrap_or(true);

    view! {
        <button
            class="text-xs px-2 py-1 rounded border border-theme-border text-theme-muted hover:text-theme hover:border-theme transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
            on:click=handle_export
            disabled=disabled
        >
            "⬇ "
        </button>
    }
}

#[component]
fn DetailField(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="detail-field">
            <div class="detail-field-label">{label}</div>
            <div class="detail-field-value">{value}</div>
        </div>
    }
}
