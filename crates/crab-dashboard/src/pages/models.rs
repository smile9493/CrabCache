use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::{Translations, use_translations};
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
        <div class="page-content space-y-6">
            <div class="flex items-center justify-between">
                <SectionHeader
                    title=t.models_title()
                    description=t.models_desc()
                />
                <div class="flex items-center gap-3">
                    {move || match models.get() {
                        Some(Ok(m)) => view! {
                            <span class="text-xs text-theme-muted">
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
                                "btn btn-secondary text-sm opacity-50 cursor-not-allowed"
                            } else {
                                "btn btn-primary text-sm"
                            }
                        }
                    >
                        {move || if syncing.get() { t.models_syncing() } else { t.models_sync_btn() }}
                    </button>
                </div>
            </div>

            {move || match sync_result.get() {
                Some(result) => view! {
                    <div class="glass-card">
                        <h4 class="text-sm font-semibold text-accent mb-4">{t.models_sync_result()}</h4>
                        <div class="sync-grid">
                            <div>
                                <div class="sync-stat-label">{t.models_added()}</div>
                                <div class="sync-stat-value text-accent">{result.added.len()}</div>
                                {if !result.added.is_empty() {
                                    view! {
                                        <div class="mt-2 text-xs text-theme-secondary space-y-0.5">
                                            {result.added.iter().map(|id| {
                                                let id = id.clone();
                                                view! { <div class="truncate">{id}</div> }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </div>
                            <div>
                                <div class="sync-stat-label">{t.models_removed()}</div>
                                <div class="sync-stat-value text-error">{result.removed.len()}</div>
                                {if !result.removed.is_empty() {
                                    view! {
                                        <div class="mt-2 text-xs text-theme-secondary space-y-0.5">
                                            {result.removed.iter().map(|id| {
                                                let id = id.clone();
                                                view! { <div class="truncate">{id}</div> }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </div>
                            <div>
                                <div class="sync-stat-label">{t.models_unchanged()}</div>
                                <div class="sync-stat-value text-warning">{result.unchanged}</div>
                            </div>
                            <div>
                                <div class="sync-stat-label">{t.models_col_status()}</div>
                                <div class="sync-stat-value text-theme">{result.total}</div>
                            </div>
                        </div>
                    </div>
                }.into_any(),
                None => view! { <span></span> }.into_any(),
            }}

            {move || match models.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", t.models_sync_result(), e)}
                    </div>
                }.into_any(),
                Some(Ok(resp)) => {
                    if resp.models.is_empty() {
                        view! {
                            <div class="glass-card p-12 text-center">
                                <div class="text-3xl mb-3">{Translations::empty_state_icon()}</div>
                                <p class="text-theme-secondary text-sm">{t.models_empty()}</p>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="glass-card-flat overflow-hidden p-0">
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th>{t.models_col_id()}</th>
                                            <th>{t.models_col_owner()}</th>
                                            <th class="text-right">{t.models_col_context()}</th>
                                            <th class="text-right">{t.models_col_input_price()}</th>
                                            <th class="text-right">{t.models_col_output_price()}</th>
                                            <th class="text-center">{t.models_col_status()}</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {resp.models.into_iter().map(|model| {
                                            let status_text = if model.available {
                                                t.models_status_available()
                                            } else {
                                                t.models_status_unavailable()
                                            };
                                            let context_str = model.context_length
                                                .map(|c| format!("{}K", c / 1024))
                                                .unwrap_or_else(|| "\u{2014}".to_string());
                                            let input_price = model.input_price_per_mtok
                                                .map(|p| format!("${:.2}", p))
                                                .unwrap_or_else(|| "\u{2014}".to_string());
                                            let output_price = model.output_price_per_mtok
                                                .map(|p| format!("${:.2}", p))
                                                .unwrap_or_else(|| "\u{2014}".to_string());
                                            view! {
                                                <tr>
                                                    <td>
                                                        <span class="font-mono text-sm text-theme">{model.id}</span>
                                                    </td>
                                                    <td class="text-theme-secondary">{model.owned_by}</td>
                                                    <td class="text-right font-mono tabular-nums text-theme">
                                                        {context_str}
                                                    </td>
                                                    <td class="text-right font-mono tabular-nums text-warning">
                                                        {input_price}
                                                    </td>
                                                    <td class="text-right font-mono tabular-nums text-error">
                                                        {output_price}
                                                    </td>
                                                    <td class="text-center">
                                                        <Badge
                                                            text=status_text.to_string()
                                                            color=if model.available { "teal" } else { "rose" }
                                                        />
                                                    </td>
                                                </tr>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                                <div class="px-5 py-3 border-t border-theme text-xs text-theme-muted">
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
