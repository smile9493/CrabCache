use leptos::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{RequestDetail, RequestLog};

#[component]
pub fn LogsPage() -> impl IntoView {
    let t = use_translations();
    let logs: RwSignal<Option<Result<Vec<RequestLog>, String>>> = RwSignal::new(None);
    let selected_id: RwSignal<Option<String>> = RwSignal::new(None);
    let selected_summary: RwSignal<Option<RequestLog>> = RwSignal::new(None);
    let detail: RwSignal<Option<Result<RequestDetail, String>>> = RwSignal::new(None);
    let detail_loading = RwSignal::new(false);

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

    let load_logs = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_logs().await {
                Ok(list) => {
                    if let Some(first) = list.first().cloned() {
                        if selected_id.get().is_none() {
                            let id = first.id.clone();
                            selected_id.set(Some(id.clone()));
                            selected_summary.set(Some(first));
                            load_detail(id);
                        }
                    }
                    logs.set(Some(Ok(list)));
                }
                Err(e) => logs.set(Some(Err(e))),
            }
        });
    };

    load_logs();

    view! {
        <div class="page-content logs-page space-y-6">
            <PageHeader
                title=move || t.logs_title()
                description=move || t.logs_desc()
            >
                <button
                    on:click=move |_| load_logs()
                    class="btn btn-secondary text-sm"
                >
                    {t.logs_refresh()}
                </button>
            </PageHeader>

            {move || match logs.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().logs_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(log_list)) => {
                    if log_list.is_empty() {
                        view! { <EmptyState message=t.logs_empty() /> }.into_any()
                    } else {
                        view! {
                            <div class="logs-split">
                                <div class="logs-split-list glass-card-flat">
                                    <table class="table table-compact">
                                        <thead>
                                            <tr>
                                                <th>{t.logs_col_time()}</th>
                                                <th>{t.logs_col_model()}</th>
                                                <th>{t.logs_col_cache()}</th>
                                                <th class="text-right">{t.logs_col_latency()}</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {log_list.into_iter().map(|log| {
                                                let log_for_click = log.clone();
                                                let id = log.id.clone();
                                                let is_selected = move || {
                                                    selected_id.get().as_deref() == Some(id.as_str())
                                                };
                                                let cache_color = match log.cache_status.as_str() {
                                                    "L0" => "teal",
                                                    "L1" => "amber",
                                                    "L2" => "violet",
                                                    _ => "rose",
                                                };
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
                                                        <td>
                                                            <Badge text=log.cache_status.clone() color=cache_color />
                                                        </td>
                                                        <td class="text-right text-xs font-mono tabular-nums text-theme">
                                                            {format!("{}ms", log.latency_ms)}
                                                        </td>
                                                    </tr>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </tbody>
                                    </table>
                                </div>

                                <div class="logs-split-detail glass-card">
                                    {move || {
                                        if let Some(summary) = selected_summary.get() {
                                            view! {
                                                <LogDetailPane
                                                    summary=summary
                                                    detail=detail
                                                    loading=detail_loading
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
) -> impl IntoView {
    let t = use_translations();
    let cache_color = match summary.cache_status.as_str() {
        "L0" => "teal",
        "L1" => "amber",
        "L2" => "violet",
        _ => "rose",
    };

    view! {
        <div class="logs-detail-inner">
            <div class="logs-detail-header">
                <h3 class="text-sm font-semibold text-theme">{t.logs_detail_title()}</h3>
                <Badge text=summary.cache_status.clone() color=cache_color />
            </div>

            <div class="logs-detail-meta">
                <DetailField label=t.logs_detail_timestamp() value=summary.timestamp.clone() />
                <DetailField label=t.logs_detail_model() value=summary.model.clone() />
                <DetailField label=t.logs_detail_consumer() value=summary.consumer.clone() />
                <DetailField label=t.logs_detail_latency() value=format!("{}ms", summary.latency_ms) />
                <DetailField label=t.logs_detail_tokens() value=format!("{}", summary.total_tokens) />
            </div>

            {move || {
                if loading.get() {
                    view! { <Spinner /> }.into_any()
                } else {
                    match detail.get() {
                        None => ().into_any(),
                        Some(Err(e)) => view! {
                            <div class="text-sm text-error">{e}</div>
                        }.into_any(),
                        Some(Ok(d)) => view! {
                            <div class="logs-detail-sections space-y-4">
                                <DetailField label=t.logs_detail_route() value=d.route_backend.clone() />
                                <DetailField label=t.logs_detail_cache_path() value=d.cache_path.clone() />
                                <div>
                                    <div class="text-xs text-theme-muted mb-1">{t.logs_detail_payload()}</div>
                                    <pre class="logs-pre">{d.request_payload.clone()}</pre>
                                </div>
                                <div>
                                    <div class="text-xs text-theme-muted mb-1">{t.logs_detail_response()}</div>
                                    <pre class="logs-pre">{d.response_body.clone()}</pre>
                                </div>
                            </div>
                        }.into_any(),
                    }
                }
            }}
        </div>
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
