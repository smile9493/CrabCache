use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{ApiKey, CreateKeyRequest};

#[component]
pub fn KeysPage() -> impl IntoView {
    let t = use_translations();
    let keys: RwSignal<Option<Result<Vec<ApiKey>, String>>> = RwSignal::new(None);

    let load_keys = {
        let keys = keys.clone();
        move || {
            leptos::task::spawn_local({
                let keys = keys.clone();
                async move {
                    match api::fetch_keys().await {
                        Ok(k) => keys.set(Some(Ok(k))),
                        Err(e) => keys.set(Some(Err(e))),
                    }
                }
            });
        }
    };

    load_keys();

    let show_create = RwSignal::new(false);
    let new_key_name = RwSignal::new(String::new());
    let new_key_rpm = RwSignal::new(60u32);
    let new_key_budget = RwSignal::new(1_000_000u64);
    let creating = RwSignal::new(false);
    let create_error = RwSignal::new(String::new());

    let on_create = move |_| {
        creating.set(true);
        create_error.set(String::new());
        let req = CreateKeyRequest {
            name: new_key_name.get(),
            rpm_limit: new_key_rpm.get(),
            monthly_token_budget: new_key_budget.get(),
        };
        leptos::task::spawn_local(async move {
            match api::create_key(&req).await {
                Ok(_) => {
                    show_create.set(false);
                    new_key_name.set(String::new());
                    load_keys();
                }
                Err(e) => {
                    create_error.set(e);
                }
            }
            creating.set(false);
        });
    };

    let on_revoke = move |id: String| {
        let id = id.clone();
        leptos::task::spawn_local(async move {
            let _ = api::revoke_key(&id).await;
            load_keys();
        });
    };

    view! {
        <div class="p-6 space-y-6">
            <div class="flex items-center justify-between">
                <SectionHeader
                    title=t.keys_title()
                    description=t.keys_desc()
                />
                <button
                    on:click=move |_| show_create.set(true)
                    class="btn btn-primary text-sm"
                >
                    {t.keys_new_btn()}
                </button>
            </div>

            {move || {
                if show_create.get() {
                    let t = use_translations();
                    view! {
                        <div class="glass-card space-y-4">
                            <h3 class="text-sm font-semibold text-theme">{t.keys_create_title()}</h3>
                            <div class="grid grid-cols-3 gap-4">
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_name_label()}</label>
                                    <input
                                        type="text"
                                        prop:value=move || new_key_name.get()
                                        on:input=move |ev| {
                                            let val = event_target_value(&ev);
                                            new_key_name.set(val);
                                        }
                                        class="input"
                                        placeholder="e.g. production-app"
                                    />
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_rpm_label()}</label>
                                    <input
                                        type="number"
                                        prop:value=move || new_key_rpm.get()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse() {
                                                new_key_rpm.set(v);
                                            }
                                        }
                                        class="input"
                                    />
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_budget_label()}</label>
                                    <input
                                        type="number"
                                        prop:value=move || new_key_budget.get()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse() {
                                                new_key_budget.set(v);
                                            }
                                        }
                                        class="input"
                                    />
                                </div>
                            </div>
                            {move || {
                                if !create_error.get().is_empty() {
                                    view! { <div class="text-xs text-error">{create_error.get()}</div> }.into_any()
                                } else {
                                    view! { <div></div> }.into_any()
                                }
                            }}
                            <div class="flex gap-2">
                                <button
                                    on:click=on_create
                                    disabled=move || creating.get()
                                    class="btn btn-primary text-sm"
                                >
                                    {move || if creating.get() { t.keys_creating() } else { t.keys_create_btn() }}
                                </button>
                                <button
                                    on:click=move |_| show_create.set(false)
                                    class="btn btn-secondary text-sm"
                                >
                                    {t.keys_cancel()}
                                </button>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <div></div> }.into_any()
                }
            }}

            {move || match keys.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().keys_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(key_list)) => {
                    if key_list.is_empty() {
                        view! { <EmptyState message=use_translations().keys_empty() /> }.into_any()
                    } else {
                        let t = use_translations();
                        view! {
                            <div class="glass-card-flat overflow-hidden p-0">
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th>{t.keys_col_name()}</th>
                                            <th>{t.keys_col_key()}</th>
                                            <th>{crate::locale::Translations::keys_col_rpm()}</th>
                                            <th>{t.keys_col_tokens()}</th>
                                            <th>{t.keys_col_cost()}</th>
                                            <th>{t.keys_col_status()}</th>
                                            <th class="text-right">""</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {key_list.into_iter().map(|key| {
                                            let id = key.id.clone();
                                            let tokens_pct = if key.monthly_token_budget > 0 {
                                                (key.tokens_used_this_month as f64 / key.monthly_token_budget as f64 * 100.0).min(100.0)
                                            } else { 0.0 };
                                            let cost = key.tokens_used_this_month as f64 * 0.14 / 1_000_000.0;
                                            let t = use_translations();
                                            view! {
                                                <tr>
                                                    <td class="text-theme font-medium">{key.name.clone()}</td>
                                                    <td>
                                                        <code class="text-xs font-mono text-theme-secondary bg-theme-tertiary px-2 py-0.5 rounded">
                                                            {key.key_preview.clone()}
                                                        </code>
                                                    </td>
                                                    <td class="font-mono tabular-nums text-theme">
                                                        {format!("{}/min", key.rpm_limit)}
                                                    </td>
                                                    <td>
                                                        <div class="space-y-1">
                                                            <span class="text-sm font-mono tabular-nums text-theme">
                                                                {format!("{}", key.tokens_used_this_month)}
                                                            </span>
                                                            <div class="progress-bar w-24">
                                                                <div
                                                                    class="progress-bar-fill"
                                                                    style=format!("width: {}%", tokens_pct)
                                                                ></div>
                                                            </div>
                                                        </div>
                                                    </td>
                                                    <td class="font-mono tabular-nums text-theme">
                                                        {format!("${:.2}", cost)}
                                                    </td>
                                                    <td>
                                                        {if key.active {
                                                            view! { <Badge text=t.keys_status_active().to_string() color="teal" /> }
                                                        } else {
                                                            view! { <Badge text=t.keys_status_revoked().to_string() color="rose" /> }
                                                        }}
                                                    </td>
                                                    <td class="text-right">
                                                        {if key.active {
                                                            view! {
                                                                <button
                                                                    on:click={
                                                                        let id = id.clone();
                                                                        move |_| on_revoke(id.clone())
                                                                    }
                                                                    class="text-xs text-error hover:text-error transition-colors"
                                                                >
                                                                    {t.keys_revoke_btn()}
                                                                </button>
                                                            }.into_any()
                                                        } else {
                                                            view! { <span></span> }.into_any()
                                                        }}
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
        </div>
    }
}