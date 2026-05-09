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
                    class="px-4 py-2 bg-stone-800 hover:bg-stone-700 text-stone-300 text-sm font-medium rounded-lg transition-colors"
                >
                    {t.logs_refresh()}
                </button>
            </div>

            {move || match logs.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="bg-rose-500/10 border border-rose-500/20 rounded-lg p-4 text-rose-400 text-sm">
                        {format!("{}: {}", use_translations().logs_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(log_list)) => {
                    if log_list.is_empty() {
                        view! { <EmptyState message=use_translations().logs_empty() /> }.into_any()
                    } else {
                        let t = use_translations();
                        view! {
                            <div class="bg-stone-900 border border-stone-800 rounded-lg overflow-hidden">
                                <table class="w-full">
                                    <thead>
                                        <tr class="border-b border-stone-800">
                                            <th class="text-left px-5 py-3 text-xs font-medium text-stone-500 uppercase">{t.logs_col_time()}</th>
                                            <th class="text-left px-5 py-3 text-xs font-medium text-stone-500 uppercase">{t.logs_col_model()}</th>
                                            <th class="text-left px-5 py-3 text-xs font-medium text-stone-500 uppercase">{t.logs_col_consumer()}</th>
                                            <th class="text-left px-5 py-3 text-xs font-medium text-stone-500 uppercase">{t.logs_col_latency()}</th>
                                            <th class="text-left px-5 py-3 text-xs font-medium text-stone-500 uppercase">{t.logs_col_tokens()}</th>
                                            <th class="text-left px-5 py-3 text-xs font-medium text-stone-500 uppercase">{t.logs_col_cache()}</th>
                                            <th class="text-right px-5 py-3 text-xs font-medium text-stone-500 uppercase">""</th>
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
                                            let t = use_translations();
                                            view! {
                                                <tr class="border-b border-stone-800/50 hover:bg-stone-800/30 transition-colors cursor-pointer"
                                                    on:click=move |_| selected_log.set(Some(log_clone.clone()))>
                                                    <td class="px-5 py-3 text-xs font-mono text-stone-400">{log.timestamp.clone()}</td>
                                                    <td class="px-5 py-3 text-sm text-stone-300">{log.model.clone()}</td>
                                                    <td class="px-5 py-3 text-sm text-stone-300">{log.consumer.clone()}</td>
                                                    <td class="px-5 py-3 text-sm font-mono tabular-nums text-stone-300">
                                                        {format!("{}ms", log.latency_ms)}
                                                    </td>
                                                    <td class="px-5 py-3 text-sm font-mono tabular-nums text-stone-300">
                                                        {format!("{}", log.total_tokens)}
                                                    </td>
                                                    <td class="px-5 py-3">
                                                        <Badge text=log.cache_status.clone() color=cache_color />
                                                    </td>
                                                    <td class="px-5 py-3 text-right">
                                                        <span class="text-xs text-teal-500 hover:text-teal-400">{t.logs_details()}</span>
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
                            <div class="relative w-full max-w-lg bg-stone-950 border-l border-stone-800 overflow-y-auto p-6 space-y-4">
                                <div class="flex items-center justify-between">
                                    <h3 class="text-sm font-semibold text-stone-200">{t.logs_detail_title()}</h3>
                                    <button
                                        on:click=move |_| selected_log.set(None)
                                        class="text-stone-500 hover:text-stone-300 text-lg leading-none"
                                    >
                                        "x"
                                    </button>
                                </div>

                                <div class="space-y-3">
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_timestamp()}</div>
                                        <div class="text-sm font-mono text-stone-300">{log.timestamp.clone()}</div>
                                    </div>
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_model()}</div>
                                        <div class="text-sm text-stone-300">{log.model.clone()}</div>
                                    </div>
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_consumer()}</div>
                                        <div class="text-sm text-stone-300">{log.consumer.clone()}</div>
                                    </div>
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_latency()}</div>
                                        <div class="text-sm font-mono text-stone-300">{format!("{}ms", log.latency_ms)}</div>
                                    </div>
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_tokens()}</div>
                                        <div class="text-sm font-mono text-stone-300">{format!("{}", log.total_tokens)}</div>
                                    </div>
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_cache_status()}</div>
                                        <Badge text=log.cache_status.clone() color="teal" />
                                    </div>
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_payload()}</div>
                                        <pre class="bg-stone-900 border border-stone-800 rounded-md p-3 text-xs font-mono text-stone-400 overflow-x-auto max-h-48 overflow-y-auto">
                                            {log.request_payload.clone()}
                                        </pre>
                                    </div>
                                    <div>
                                        <div class="text-xs text-stone-500 mb-1">{t.logs_detail_response()}</div>
                                        <pre class="bg-stone-900 border border-stone-800 rounded-md p-3 text-xs font-mono text-stone-400 overflow-x-auto max-h-48 overflow-y-auto">
                                            {log.response_preview.clone()}
                                        </pre>
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