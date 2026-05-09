use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::{use_translations, Translations};
use crate::types::{ModelListResponse, SyncResult};

#[component]
pub fn ModelsPage() -> impl IntoView {
    let t = use_translations();
    let models: RwSignal<Option<Result<ModelListResponse, String>>> = RwSignal::new(None);
    let sync_result: RwSignal<Option<SyncResult>> = RwSignal::new(None);
    let syncing: RwSignal<bool> = RwSignal::new(false);

    leptos::task::spawn_local(async move {
        match api::fetch_models().await {
            Ok(m) => models.set(Some(Ok(m))),
            Err(e) => models.set(Some(Err(e))),
        }
    });

    let on_sync = move |_| {
        syncing.set(true);
        sync_result.set(None);
        leptos::task::spawn_local(async move {
            match api::sync_models().await {
                Ok(result) => {
                    sync_result.set(Some(result));
                    match api::fetch_models().await {
                        Ok(m) => models.set(Some(Ok(m))),
                        Err(e) => models.set(Some(Err(e))),
                    }
                }
                Err(e) => models.set(Some(Err(e))),
            }
            syncing.set(false);
        });
    };

    view! {
        <div class="p-6 space-y-6">
            <div class="flex items-center justify-between">
                <SectionHeader
                    title=t.models_title()
                    description=t.models_desc()
                />
                <div class="flex items-center gap-3">
                    {move || match models.get() {
                        Some(Ok(m)) => view! {
                            <span class="text-xs text-stone-500">
                                {t.models_synced_at()} ": " {m.synced_at.unwrap_or_else(|| t.models_never_synced().to_string())}
                            </span>
                        }.into_any(),
                        _ => view! { <span></span> }.into_any(),
                    }}
                    <button
                        on:click=on_sync
                        disabled=move || syncing.get()
                        class=move || {
                            if syncing.get() {
                                "px-4 py-2 bg-stone-600 text-stone-300 text-sm font-medium rounded-lg cursor-not-allowed"
                            } else {
                                "px-4 py-2 bg-teal-600 hover:bg-teal-700 text-white text-sm font-medium rounded-lg transition-colors"
                            }
                        }
                    >
                        {move || if syncing.get() { t.models_syncing() } else { t.models_sync_btn() }}
                    </button>
                </div>
            </div>

            {move || match sync_result.get() {
                Some(result) => view! {
                    <div class="bg-stone-900 border border-teal-800/50 rounded-lg p-4">
                        <h4 class="text-sm font-semibold text-teal-400 mb-3">{t.models_sync_result()}</h4>
                        <div class="grid grid-cols-4 gap-4">
                            <div>
                                <div class="text-xs text-stone-500">{t.models_added()}</div>
                                <div class="text-lg font-mono tabular-nums text-teal-400">
                                    {result.added.len()}
                                </div>
                                {if result.added.is_empty() {
                                    view! { <span></span> }.into_any()
                                } else {
                                    view! {
                                        <div class="mt-1 text-xs text-stone-400 space-y-0.5">
                                            {result.added.iter().map(|id| {
                                                let id = id.clone();
                                                view! {
                                                    <div class="truncate">{id}</div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                }}
                            </div>
                            <div>
                                <div class="text-xs text-stone-500">{t.models_removed()}</div>
                                <div class="text-lg font-mono tabular-nums text-rose-400">
                                    {result.removed.len()}
                                </div>
                                {if result.removed.is_empty() {
                                    view! { <span></span> }.into_any()
                                } else {
                                    view! {
                                        <div class="mt-1 text-xs text-stone-400 space-y-0.5">
                                            {result.removed.iter().map(|id| {
                                                let id = id.clone();
                                                view! {
                                                    <div class="truncate">{id}</div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                }}
                            </div>
                            <div>
                                <div class="text-xs text-stone-500">{t.models_unchanged()}</div>
                                <div class="text-lg font-mono tabular-nums text-amber-400">
                                    {result.unchanged}
                                </div>
                            </div>
                            <div>
                                <div class="text-xs text-stone-500">{t.models_col_status()}</div>
                                <div class="text-lg font-mono tabular-nums text-stone-200">
                                    {result.total}
                                </div>
                            </div>
                        </div>
                    </div>
                }.into_any(),
                None => view! { <span></span> }.into_any(),
            }}

            {move || match models.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="bg-rose-500/10 border border-rose-500/20 rounded-lg p-4 text-rose-400 text-sm">
                        {format!("{}: {}", t.models_sync_result(), e)}
                    </div>
                }.into_any(),
                Some(Ok(resp)) => {
                    if resp.models.is_empty() {
                        view! {
                            <div class="bg-stone-900 border border-stone-800 rounded-lg p-12 text-center">
                                <div class="text-3xl mb-3">{Translations::empty_state_icon()}</div>
                                <p class="text-stone-400 text-sm">{t.models_empty()}</p>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="bg-stone-900 border border-stone-800 rounded-lg overflow-hidden">
                                <table class="w-full text-sm">
                                    <thead>
                                        <tr class="border-b border-stone-800">
                                            <th class="px-4 py-3 text-left text-xs font-medium text-stone-400 uppercase tracking-wider">
                                                {t.models_col_id()}
                                            </th>
                                            <th class="px-4 py-3 text-left text-xs font-medium text-stone-400 uppercase tracking-wider">
                                                {t.models_col_owner()}
                                            </th>
                                            <th class="px-4 py-3 text-right text-xs font-medium text-stone-400 uppercase tracking-wider">
                                                {t.models_col_context()}
                                            </th>
                                            <th class="px-4 py-3 text-right text-xs font-medium text-stone-400 uppercase tracking-wider">
                                                {t.models_col_input_price()}
                                            </th>
                                            <th class="px-4 py-3 text-right text-xs font-medium text-stone-400 uppercase tracking-wider">
                                                {t.models_col_output_price()}
                                            </th>
                                            <th class="px-4 py-3 text-center text-xs font-medium text-stone-400 uppercase tracking-wider">
                                                {t.models_col_status()}
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody class="divide-y divide-stone-800/50">
                                        {resp.models.into_iter().map(|model| {
                                            let status_class = if model.available {
                                                "bg-teal-500/10 text-teal-400 border-teal-500/20"
                                            } else {
                                                "bg-rose-500/10 text-rose-400 border-rose-500/20"
                                            };
                                            let status_text = if model.available {
                                                t.models_status_available()
                                            } else {
                                                t.models_status_unavailable()
                                            };
                                            let context_str = model.context_length
                                                .map(|c| format!("{}K", c / 1024))
                                                .unwrap_or_else(|| "—".to_string());
                                            let input_price = model.input_price_per_mtok
                                                .map(|p| format!("${:.2}", p))
                                                .unwrap_or_else(|| "—".to_string());
                                            let output_price = model.output_price_per_mtok
                                                .map(|p| format!("${:.2}", p))
                                                .unwrap_or_else(|| "—".to_string());
                                            view! {
                                                <tr class="hover:bg-stone-800/30 transition-colors">
                                                    <td class="px-4 py-3">
                                                        <span class="font-mono text-sm text-stone-200">{model.id}</span>
                                                    </td>
                                                    <td class="px-4 py-3 text-stone-400">{model.owned_by}</td>
                                                    <td class="px-4 py-3 text-right font-mono tabular-nums text-stone-300">
                                                        {context_str}
                                                    </td>
                                                    <td class="px-4 py-3 text-right font-mono tabular-nums text-amber-400">
                                                        {input_price}
                                                    </td>
                                                    <td class="px-4 py-3 text-right font-mono tabular-nums text-rose-400">
                                                        {output_price}
                                                    </td>
                                                    <td class="px-4 py-3 text-center">
                                                        <span class=format!(
                                                            "inline-flex items-center px-2 py-0.5 rounded text-xs font-medium border {}",
                                                            status_class
                                                        )>
                                                            {status_text}
                                                        </span>
                                                    </td>
                                                </tr>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                                <div class="px-4 py-3 border-t border-stone-800 text-xs text-stone-500">
                                    {format!("{} {}", resp.total, t.models_col_id())}
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}
