use leptos::prelude::*;

use crate::api;
use crate::components::sync_result::SyncResultCard;
use crate::components::ui::*;
use crate::locale::{Translations, use_translations};
use crate::types::{ModelApplyBody, ModelDetectResponse, ModelListResponse, SyncResult};

#[component]
pub fn ModelsPage() -> impl IntoView {
    let t = use_translations();
    let models: RwSignal<Option<Result<ModelListResponse, String>>> = RwSignal::new(None);
    let sync_result: RwSignal<Option<SyncResult>> = RwSignal::new(None);
    let detect_result: RwSignal<Option<Result<ModelDetectResponse, String>>> = RwSignal::new(None);
    let syncing: RwSignal<bool> = RwSignal::new(false);
    let detecting: RwSignal<bool> = RwSignal::new(false);
    let applying: RwSignal<bool> = RwSignal::new(false);

    leptos::task::spawn_local(async move {
        match api::fetch_models().await {
            Ok(m) => models.set(Some(Ok(m))),
            Err(e) => models.set(Some(Err(e))),
        }
    });

    let on_detect = move |_| {
        detecting.set(true);
        detect_result.set(None);
        leptos::task::spawn_local(async move {
            match api::detect_models().await {
                Ok(d) => detect_result.set(Some(Ok(d))),
                Err(e) => detect_result.set(Some(Err(e))),
            }
            detecting.set(false);
        });
    };

    let on_apply = move |_| {
        let Some(Ok(diff)) = detect_result.get() else {
            return;
        };
        applying.set(true);
        let body = ModelApplyBody {
            add: diff.to_add.clone(),
            remove: diff.to_remove.clone(),
        };
        leptos::task::spawn_local(async move {
            match api::apply_models(&body).await {
                Ok(result) => {
                    sync_result.set(Some(result));
                    detect_result.set(None);
                    if let Ok(m) = api::fetch_models().await {
                        models.set(Some(Ok(m)));
                    }
                }
                Err(e) => detect_result.set(Some(Err(e))),
            }
            applying.set(false);
        });
    };

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
                        on:click=on_detect
                        disabled=move || detecting.get()
                        class="btn btn-secondary text-sm"
                    >
                        {move || if detecting.get() { "..." } else { t.models_detect_btn() }}
                    </button>
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

            {move || sync_result.get().map(|r| view! { <SyncResultCard result=r /> })}

            {move || match detect_result.get() {
                Some(Ok(diff)) => view! {
                    <div class="glass-card space-y-3">
                        <h4 class="text-sm font-semibold text-theme">{t.models_detect_result()}</h4>
                        <p class="text-xs text-theme-muted">
                            "+ " {diff.to_add.len()} " / - " {diff.to_remove.len()} " / = " {diff.unchanged}
                        </p>
                        {(!diff.to_add.is_empty() || !diff.to_remove.is_empty()).then(|| view! {
                            <button
                                on:click=on_apply
                                disabled=move || applying.get()
                                class="btn btn-primary text-sm"
                            >
                                {t.models_apply_btn()}
                            </button>
                        })}
                    </div>
                }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">{e}</div>
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
