use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::RequestLog;

#[component]
pub fn LogsPage() -> impl IntoView {
    let t = use_translations();
    let logs: RwSignal<Option<Result<Vec<RequestLog>, String>>> = RwSignal::new(None);
    let selected_log: RwSignal<Option<RequestLog>> = RwSignal::new(None);

    let load_logs = {
        let logs = logs.clone();
        move || {
            leptos::task::spawn_local({
                let logs = logs.clone();
                async move {
                    match api::fetch_logs().await {
                        Ok(l) => logs.set(Some(Ok(l))),
                        Err(e) => logs.set(Some(Err(e))),
                    }
                }
            });
        }
    };

    load_logs();

    view! {
        <div class="p-6 space-y-6">
            <div class="flex items-center justify-between">
                <SectionHeader
                    title=t.logs_title()
                    description=t.logs_desc()
                />
                <button
                    on:click=move |_| load_logs()
                    class="btn btn-secondary text-sm"
                >
                    {t.logs_refresh()}
                </button>
            </div>

            {move || match logs.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().logs_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(log_list)) => {
                    if log_list.is_empty() {
                        view! { <EmptyState message=use_translations().logs_empty() /> }.into_any()
                    } else {
                        view! {
                            <div class="glass-card-flat overflow-hidden p-0">
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th>{use_translations().logs_col_time()}</th>
                                            <th>{use_translations().logs_col_model()}</th>
                                            <th>{use_translations().logs_col_consumer()}</th>
                                            <th>{use_translations().logs_col_latency()}</th>
                                            <th>{use_translations().logs_col_tokens()}</th>
                                            <th>{use_translations().logs_col_cache()}</th>
                                            <th class="text-right">""</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {log_list.into_iter().map(|log| {
                                            let log_clone = log.clone();
                                            let cache_color = match log.cache_status.as_str() {
                                                "L0" => "teal",
                                                "L1" => "amber",
                                                "L2" => "violet",
                                                _ => "rose",
                                            };
                                            view! {
                                                <tr class="cursor-pointer"
                                                    on:click=move |_| selected_log.set(Some(log_clone.clone()))>
                                                    <td class="text-xs font-mono text-theme-secondary">{log.timestamp.clone()}</td>
                                                    <td class="text-sm text-theme">{log.model.clone()}</td>
                                                    <td class="text-sm text-theme">{log.consumer.clone()}</td>
                                                    <td class="text-sm font-mono tabular-nums text-theme">
                                                        {format!("{}ms", log.latency_ms)}
                                                    </td>
                                                    <td class="text-sm font-mono tabular-nums text-theme">
                                                        {format!("{}", log.total_tokens)}
                                                    </td>
                                                    <td><Badge text=log.cache_status.clone() color=cache_color /></td>
                                                    <td class="text-right">
                                                        <span class="text-xs text-accent">{use_translations().logs_details()}</span>
                                                    </td>
                                                </tr>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            </div>
                        }.into_any()
                    }
                }
            }}

            {move || {
                if let Some(log) = selected_log.get() {
                    let t = use_translations();
                    view! {
                        <div class="fixed inset-0 z-50 flex justify-end">
                            <div class="absolute inset-0 bg-black/50" on:click=move |_| selected_log.set(None)></div>
                            <div class="relative w-full max-w-lg detail-panel overflow-y-auto p-6 space-y-5 theme-scrollbar">
                                <div class="flex items-center justify-between">
                                    <h3 class="text-sm font-semibold text-theme">{t.logs_detail_title()}</h3>
                                    <button
                                        on:click=move |_| selected_log.set(None)
                                        class="text-theme-muted hover:text-theme text-lg leading-none"
                                    >"x"</button>
                                </div>
                                <div class="space-y-4">
                                    <Field label=t.logs_detail_timestamp() value=log.timestamp.clone() />
                                    <Field label=t.logs_detail_model() value=log.model.clone() />
                                    <Field label=t.logs_detail_consumer() value=log.consumer.clone() />
                                    <Field label=t.logs_detail_latency() value=format!("{}ms", log.latency_ms) />
                                    <Field label=t.logs_detail_tokens() value=format!("{}", log.total_tokens) />
                                    <div>
                                        <div class="text-xs text-theme-muted mb-1">{t.logs_detail_cache_status()}</div>
                                        <Badge text=log.cache_status.clone() color="teal" />
                                    </div>
                                    <div>
                                        <div class="text-xs text-theme-muted mb-1">{t.logs_detail_payload()}</div>
                                        <pre>{log.request_payload.clone()}</pre>
                                    </div>
                                    <div>
                                        <div class="text-xs text-theme-muted mb-1">{t.logs_detail_response()}</div>
                                        <pre>{log.response_preview.clone()}</pre>
                                    </div>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <div></div> }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn Field(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div>
            <div class="text-xs text-theme-muted mb-1">{label}</div>
            <div class="text-sm font-mono text-theme">{value}</div>
        </div>
    }
}