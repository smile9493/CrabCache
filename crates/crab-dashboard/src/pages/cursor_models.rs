use leptos::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::locale::use_translations;
use crate::types::{CursorModelAlias, CursorModelsConfig};

#[component]
pub fn CursorModelsPage() -> impl IntoView {
    let t = use_translations();
    let data: RwSignal<Option<Result<CursorModelsConfig, String>>> = RwSignal::new(None);
    let saving: RwSignal<bool> = RwSignal::new(false);
    let feedback: RwSignal<String> = RwSignal::new(String::new());

    // Editable aliases
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
        <div class="page-content space-y-6">
            <PageHeader
                title=move || t.cursor_models_title()
                description=move || t.cursor_models_desc()
            >
                <button
                    on:click=move |_| {
                        load();
                        feedback.set(String::new());
                    }
                    class="btn btn-secondary text-xs"
                >
                    {t.overview_refresh()}
                </button>
            </PageHeader>

            <div class="config-card glass-card">
                <div class="config-card-head">
                    <h4 class="config-card-title">{move || t.cursor_models_title()}</h4>
                    <p class="config-card-desc">{move || t.cursor_models_desc()}</p>
                </div>
                <div class="config-card-body">
                    {move || match data.get() {
                        None => view! { <div class="text-sm text-theme-secondary py-4">"Loading..."</div> }.into_any(),
                        Some(Err(e)) => view! {
                            <div class="text-error text-sm py-4">{e}</div>
                        }.into_any(),
                        Some(Ok(_)) => {
                            let rows = aliases.get();
                            let _row_count = rows.len();
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
                                                            "✕"
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
                                            {move || if saving.get() {
                                                t.routing_save()
                                            } else {
                                                t.routing_save()
                                            }}
                                        </button>
                                    </div>
                                </div>
                            }.into_any()
                        },
                    }}
                </div>
            </div>
        </div>
    }
}
