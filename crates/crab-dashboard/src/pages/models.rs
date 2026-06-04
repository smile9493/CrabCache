use leptos::prelude::*;

use crate::api;
use crate::components::sync_result::SyncResultCard;
use crate::components::ui::*;
use crate::locale::{Translations, use_translations};
use crate::types::{ModelApplyBody, ModelDetectResponse, ModelListResponse, SyncResult};

#[component]
pub fn ModelsPage() -> impl IntoView {
    let t = use_translations();
    let profile_id = RwSignal::new("deepseek".to_string());
    let models: RwSignal<Option<Result<ModelListResponse, String>>> = RwSignal::new(None);
    let profiles: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let sync_result: RwSignal<Option<SyncResult>> = RwSignal::new(None);
    let detect_result: RwSignal<Option<Result<ModelDetectResponse, String>>> = RwSignal::new(None);
    let syncing: RwSignal<bool> = RwSignal::new(false);
    let detecting: RwSignal<bool> = RwSignal::new(false);
    let applying: RwSignal<bool> = RwSignal::new(false);
    let last_error: RwSignal<Option<String>> = RwSignal::new(None);

    let reload_models = move |pid: String| {
        leptos::task::spawn_local(async move {
            match api::fetch_models(Some(&pid)).await {
                Ok(m) => {
                    models.try_set(Some(Ok(m)));
                }
                Err(e) => {
                    models.try_set(Some(Err(e)));
                }
            }
        });
    };

    leptos::task::spawn_local(async move {
        if let Ok(resp) = api::fetch_upstream_profiles().await {
            let ids: Vec<String> = resp.profiles.iter().map(|p| p.id.clone()).collect();
            if !ids.is_empty() {
                profiles.try_set(ids.clone());
                if !ids.contains(&profile_id.try_get_untracked().unwrap_or_default()) {
                    profile_id.try_set(ids[0].clone());
                }
            }
        }
        reload_models(profile_id.try_get_untracked().unwrap_or_default());
    });

    let on_detect = move |_| {
        let pid = profile_id.get();
        detecting.set(true);
        last_error.set(None);
        detect_result.set(None);
        leptos::task::spawn_local(async move {
            match api::detect_models(&pid).await {
                Ok(d) => {
                    detect_result.try_set(Some(Ok(d)));
                }
                Err(e) => {
                    detect_result.try_set(Some(Err(e)));
                }
            }
            detecting.try_set(false);
        });
    };

    let on_apply = move |_| {
        let Some(Ok(diff)) = detect_result.get() else {
            return;
        };
        let pid = profile_id.get();
        applying.set(true);
        last_error.set(None);
        let body = ModelApplyBody {
            profile_id: pid.clone(),
            add: diff.to_add.clone(),
            remove: diff.to_remove.clone(),
        };
        leptos::task::spawn_local(async move {
            match api::apply_models(&body).await {
                Ok(result) => {
                    sync_result.try_set(Some(result));
                    detect_result.try_set(None);
                    reload_models(pid);
                }
                Err(e) => {
                    last_error.try_set(Some(e));
                }
            }
            applying.try_set(false);
        });
    };

    let on_sync = move |_| {
        let pid = profile_id.get();
        syncing.set(true);
        last_error.set(None);
        sync_result.set(None);
        leptos::task::spawn_local(async move {
            match api::sync_models(&pid).await {
                Ok(result) => {
                    sync_result.try_set(Some(result));
                    reload_models(pid);
                }
                Err(e) => {
                    last_error.try_set(Some(e));
                }
            }
            syncing.try_set(false);
        });
    };

    view! {
        <div class="page-content space-y-6">
            <SectionHeader title=t.models_title() description=t.models_desc() />

            // Profile chip selector
            <div class="profile-chip-group">
                {move || {
                    let selected = profile_id.get();
                    profiles.get().into_iter().map(|id| {
                        let pid = id.clone();
                        let pid2 = id.clone();
                        let icon_char = id.chars().next()
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_default();
                        let is_active = selected == pid;
                        let class = if is_active {
                            "profile-chip profile-chip-active"
                        } else {
                            "profile-chip"
                        };
                        view! {
                            <button
                                type="button"
                                class=class
                                on:click=move |_| {
                                    profile_id.set(pid2.clone());
                                    sync_result.set(None);
                                    detect_result.set(None);
                                    reload_models(pid2.clone());
                                }
                            >
                                <span class="profile-chip-icon">{icon_char}</span>
                                <span class="font-mono">{id}</span>
                            </button>
                        }
                    }).collect_view()
                }}
            </div>

            // Action bar
            <div class="flex items-center justify-between flex-wrap gap-3">
                <div class="flex items-center gap-3 flex-wrap">
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

            // Sync result
            {move || sync_result.get().map(|r| view! { <SyncResultCard result=r /> })}

            // Error alert
            {move || last_error.get().map(|e| view! {
                <div class="glass-card text-error text-sm">{e}</div>
            })}

            // Detect diff preview
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

            // Model cards
            {move || match models.get() {
                None => view! {
                    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                        {(0..6).map(|_| view! { <crate::components::skeleton::SkeletonModelCard /> }).collect_view()}
                    </div>
                }.into_any(),
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
                        let total = resp.total;
                        view! {
                            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                                {resp.models.into_iter().map(|model| {
                                    let status_text = if model.available {
                                        t.models_status_available()
                                    } else {
                                        t.models_status_unavailable()
                                    };
                                    let badge_color = if model.available { "green" } else { "stone" };
                                    let card_class = if model.available {
                                        "model-card"
                                    } else {
                                        "model-card model-card-unavailable"
                                    };
                                    let name_class = if model.available {
                                        "model-card-name truncate"
                                    } else {
                                        "model-card-name model-card-name-unavailable truncate"
                                    };
                                    let price_unit = t.models_price_unit();
                                    let context_str = model.context_length
                                        .map(|c| format!("{}K", c / 1024))
                                        .unwrap_or_else(|| "\u{2014}".to_string());
                                    let input_price = model.input_price_per_mtok
                                        .map(|p| format!("${:.2}{price_unit}", p))
                                        .unwrap_or_else(|| "\u{2014}".to_string());
                                    let output_price = model.output_price_per_mtok
                                        .map(|p| format!("${:.2}{price_unit}", p))
                                        .unwrap_or_else(|| "\u{2014}".to_string());
                                    let icon_char = model.id.chars().next()
                                        .map(|c| c.to_uppercase().to_string())
                                        .unwrap_or_default();
                                    view! {
                                        <div class=card_class>
                                            <div class="flex items-start justify-between gap-2 mb-1">
                                                <div class="flex items-start gap-2 min-w-0 flex-1">
                                                    <span class="model-card-icon">{icon_char}</span>
                                                    <div class="min-w-0">
                                                        <div class=name_class title=model.id.clone()>
                                                            {model.id.clone()}
                                                        </div>
                                                        <div class="model-card-owner truncate" title=model.owned_by.clone()>
                                                            {model.owned_by.clone()}
                                                        </div>
                                                    </div>
                                                </div>
                                                <Badge
                                                    text=status_text.to_string()
                                                    color=badge_color
                                                />
                                            </div>
                                            <div class="model-card-stats">
                                                <div class="model-card-stat">
                                                    <span class="model-card-stat-value">{context_str}</span>
                                                    <span class="model-card-stat-label">{t.models_card_ctx_label()}</span>
                                                </div>
                                                <div class="model-card-stat model-card-pricing">
                                                    <div class="model-card-pricing-row">
                                                        <span class="model-card-stat-label">{t.models_card_input_label()}</span>
                                                        <span class="model-card-stat-value model-card-stat-value-price-in">{input_price}</span>
                                                    </div>
                                                    <div class="model-card-pricing-row">
                                                        <span class="model-card-stat-label">{t.models_card_output_label()}</span>
                                                        <span class="model-card-stat-value model-card-stat-value-price-out">{output_price}</span>
                                                    </div>
                                                </div>
                                            </div>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                                <div class="models-catalog-footer">
                                    {t.models_catalog_footer(total)}
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}
