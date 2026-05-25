use leptos::prelude::*;

use crate::api;
use crate::components::sync_result::SyncResultCard;
use crate::components::ui::*;
use crate::locale::{Translations, use_translations};
use crate::types::{CursorModelAlias, CursorModelsConfig, ModelApplyBody, ModelDetectResponse, ModelListResponse, SyncResult};

#[component]
pub fn ModelsPage() -> impl IntoView {
    let t = use_translations();
    let active_tab: RwSignal<usize> = RwSignal::new(0);

    view! {
        <div class="page-content space-y-6">
            <SectionHeader title=t.models_title() description=t.models_desc() />
            <TabBar tabs=vec!["Catalog", "Aliases"] active=active_tab />
            {move || match active_tab.get() {
                0 => view! { <CatalogPanel /> }.into_any(),
                _ => view! { <AliasesPanel /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn CatalogPanel() -> impl IntoView {
    let t = use_translations();
    let profile_id = RwSignal::new("deepseek".to_string());
    let models: RwSignal<Option<Result<ModelListResponse, String>>> = RwSignal::new(None);
    let profiles: RwSignal<Vec<String>> = RwSignal::new(vec!["deepseek".to_string()]);
    let sync_result: RwSignal<Option<SyncResult>> = RwSignal::new(None);
    let detect_result: RwSignal<Option<Result<ModelDetectResponse, String>>> = RwSignal::new(None);
    let syncing: RwSignal<bool> = RwSignal::new(false);
    let detecting: RwSignal<bool> = RwSignal::new(false);
    let applying: RwSignal<bool> = RwSignal::new(false);

    let reload_models = move |pid: String| {
        leptos::task::spawn_local(async move {
            match api::fetch_models(Some(&pid)).await {
                Ok(m) => models.set(Some(Ok(m))),
                Err(e) => models.set(Some(Err(e))),
            }
        });
    };

    leptos::task::spawn_local(async move {
        if let Ok(list) = api::fetch_upstream_profiles().await {
            let ids: Vec<String> = list.profiles.into_iter().map(|p| p.id).collect();
            if !ids.is_empty() {
                profiles.set(ids.clone());
                if !ids.contains(&profile_id.get_untracked()) {
                    profile_id.set(ids[0].clone());
                }
            }
        }
        reload_models(profile_id.get_untracked());
    });

    let on_profile_change = move |ev| {
        let pid = event_target_value(&ev);
        profile_id.set(pid.clone());
        reload_models(pid);
    };

    let on_detect = move |_| {
        let pid = profile_id.get();
        detecting.set(true);
        detect_result.set(None);
        leptos::task::spawn_local(async move {
            match api::detect_models(&pid).await {
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
        let pid = profile_id.get();
        applying.set(true);
        let body = ModelApplyBody {
            profile_id: pid.clone(),
            add: diff.to_add.clone(),
            remove: diff.to_remove.clone(),
        };
        leptos::task::spawn_local(async move {
            match api::apply_models(&body).await {
                Ok(result) => {
                    sync_result.set(Some(result));
                    detect_result.set(None);
                    reload_models(pid);
                }
                Err(e) => detect_result.set(Some(Err(e))),
            }
            applying.set(false);
        });
    };

    let on_sync = move |_| {
        let pid = profile_id.get();
        syncing.set(true);
        sync_result.set(None);
        leptos::task::spawn_local(async move {
            match api::sync_models(&pid).await {
                Ok(result) => {
                    sync_result.set(Some(result));
                    reload_models(pid);
                }
                Err(e) => models.set(Some(Err(e))),
            }
            syncing.set(false);
        });
    };

    view! {
        <div class="space-y-4">
            <div class="flex items-center justify-between flex-wrap gap-3">
                <div class="flex items-center gap-3 flex-wrap">
                    <label class="text-xs text-theme-muted">{t.upstream_profile_label()}</label>
                    <select
                        class="config-input text-sm"
                        prop:value=move || profile_id.get()
                        on:change=on_profile_change
                    >
                        {move || profiles.get().into_iter().map(|id| {
                            let opt_val = id.clone();
                            let label = id;
                            view! { <option value=opt_val>{label}</option> }
                        }).collect_view()}
                    </select>
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

#[component]
fn AliasesPanel() -> impl IntoView {
    let t = use_translations();
    let data: RwSignal<Option<Result<CursorModelsConfig, String>>> = RwSignal::new(None);
    let saving: RwSignal<bool> = RwSignal::new(false);
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let aliases: RwSignal<Vec<(RwSignal<String>, RwSignal<String>)>> = RwSignal::new(Vec::new());

    let load = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_cursor_models().await {
                Ok(config) => {
                    let pairs: Vec<(RwSignal<String>, RwSignal<String>)> = config
                        .aliases
                        .clone()
                        .into_iter()
                        .map(|a| (RwSignal::new(a.model), RwSignal::new(a.alias)))
                        .collect();
                    aliases.set(pairs);
                    data.set(Some(Ok(config)));
                }
                Err(e) => data.set(Some(Err(e))),
            }
        });
    };

    load();

    let on_add = move |_| {
        aliases.update(|list| {
            list.push((RwSignal::new(String::new()), RwSignal::new(String::new())));
        });
    };

    let on_remove = move |idx: usize| {
        aliases.update(|list| {
            if idx < list.len() {
                list.remove(idx);
            }
        });
    };

    let on_save = move |_| {
        saving.set(true);
        feedback.set(String::new());
        let pairs = aliases.get();
        let config = CursorModelsConfig {
            aliases: pairs
                .into_iter()
                .map(|(model, alias)| CursorModelAlias {
                    model: model.get(),
                    alias: alias.get(),
                })
                .filter(|a| !a.model.is_empty() && !a.alias.is_empty())
                .collect(),
        };
        leptos::task::spawn_local(async move {
            match api::update_cursor_models(&config).await {
                Ok(_) => {
                    feedback.set(t.routing_saved().to_string());
                }
                Err(e) => feedback.set(e),
            }
            saving.set(false);
        });
    };

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h4 class="config-card-title">{t.cursor_models_title()}</h4>
                <p class="config-card-desc">{t.cursor_models_desc()}</p>
            </div>
            <div class="config-card-body">
                {move || match data.get() {
                    None => view! { <div class="text-sm text-theme-secondary py-4">"Loading..."</div> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="text-error text-sm py-4">{e}</div>
                    }.into_any(),
                    Some(Ok(_)) => {
                        let rows = aliases.get();
                        view! {
                            <table class="w-full text-sm">
                                <thead>
                                    <tr class="text-left text-theme-secondary border-b border-theme-border">
                                        <th class="pb-2 pr-3 font-medium">{t.cursor_models_col_model()}</th>
                                        <th class="pb-2 pr-3 font-medium">{t.cursor_models_col_alias()}</th>
                                        <th class="pb-2 w-20"></th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {rows.into_iter().enumerate().map(|(idx, (model_sig, alias_sig))| {
                                        let remove_idx = idx;
                                        view! {
                                            <tr class="border-b border-theme-border/50">
                                                <td class="py-2 pr-3">
                                                    <input
                                                        type="text"
                                                        class="input w-full text-sm"
                                                        prop:value=move || model_sig.get()
                                                        on:input=move |e| model_sig.set(event_target_value(&e))
                                                        placeholder="gpt-4o"
                                                    />
                                                </td>
                                                <td class="py-2 pr-3">
                                                    <input
                                                        type="text"
                                                        class="input w-full text-sm"
                                                        prop:value=move || alias_sig.get()
                                                        on:input=move |e| alias_sig.set(event_target_value(&e))
                                                        placeholder="deepseek-chat"
                                                    />
                                                </td>
                                                <td class="py-2 text-center">
                                                    <button
                                                        on:click=move |_| on_remove(remove_idx)
                                                        class="btn btn-ghost text-xs text-error"
                                                    >
                                                        "\u{2715}"
                                                    </button>
                                                </td>
                                            </tr>
                                        }
                                    }).collect::<Vec<_>>()}
                                </tbody>
                            </table>

                            <div class="flex items-center justify-between mt-4 pt-3 border-t border-theme-border/50">
                                <button
                                    on:click=on_add
                                    class="btn btn-secondary text-xs"
                                >
                                    {t.cursor_models_alias_add()}
                                </button>
                                <div class="flex items-center gap-3">
                                    {move || if !feedback.get().is_empty() {
                                        view! {
                                            <span class="text-xs text-theme-secondary">{feedback.get()}</span>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }}
                                    <button
                                        on:click=on_save
                                        disabled=move || saving.get()
                                        class="btn btn-primary text-sm"
                                    >
                                        {t.routing_save()}
                                    </button>
                                </div>
                            </div>
                        }.into_any()
                    },
                }}
            </div>
        </div>
    }
}
