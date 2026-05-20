use leptos::prelude::*;

use crate::api;
use crate::clipboard;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{ApiKey, CreateKeyRequest, NetworkInfo};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyNoticeKind {
    Ok,
    Failed,
}

#[component]
pub fn KeysPage() -> impl IntoView {
    let t = use_translations();
    let keys: RwSignal<Option<Result<Vec<ApiKey>, String>>> = RwSignal::new(None);
    let network_info: RwSignal<Option<Result<NetworkInfo, String>>> = RwSignal::new(None);
    let search_query: RwSignal<String> = RwSignal::new(String::new());
    let copy_notice: RwSignal<Option<CopyNoticeKind>> = RwSignal::new(None);
    let created_key: RwSignal<Option<ApiKey>> = RwSignal::new(None);

    let show_copy_notice = move |kind: CopyNoticeKind| {
        copy_notice.set(Some(kind));
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(2000).await;
            copy_notice.set(None);
        });
    };

    let copy_text = move |text: String| {
        if clipboard::copy_text(&text) {
            show_copy_notice(CopyNoticeKind::Ok);
        } else {
            show_copy_notice(CopyNoticeKind::Failed);
        }
    };

    let load_keys = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_keys().await {
                Ok(k) => keys.set(Some(Ok(k))),
                Err(e) => keys.set(Some(Err(e))),
            }
        });
    };

    let load_network_info = move || {
        network_info.set(None);
        leptos::task::spawn_local(async move {
            match api::fetch_network_info().await {
                Ok(info) => network_info.set(Some(Ok(info))),
                Err(e) => network_info.set(Some(Err(e))),
            }
        });
    };

    load_keys();
    load_network_info();

    let show_create = RwSignal::new(false);
    let new_key_name = RwSignal::new(String::new());
    let new_key_domain = RwSignal::new(String::new());
    let new_key_rpm = RwSignal::new(60u32);
    let new_key_budget = RwSignal::new(1_000_000u64);
    let new_key_unlimited = RwSignal::new(true);
    let new_key_quota = RwSignal::new(1_000_000i64);
    let creating = RwSignal::new(false);
    let create_error = RwSignal::new(String::new());

    let on_create = move |_| {
        creating.set(true);
        create_error.set(String::new());
        let domain = new_key_domain.get();
        let req = CreateKeyRequest {
            name: new_key_name.get(),
            rpm_limit: new_key_rpm.get(),
            monthly_token_budget: new_key_budget.get(),
            expired_at: None,
            model_limits: None,
            remain_quota: if new_key_unlimited.get() {
                None
            } else {
                Some(new_key_quota.get())
            },
            unlimited_quota: Some(new_key_unlimited.get()),
            domain: if domain.trim().is_empty() {
                None
            } else {
                Some(domain.trim().to_string())
            },
        };
        leptos::task::spawn_local(async move {
            match api::create_key(&req).await {
                Ok(key) => {
                    show_create.set(false);
                    new_key_name.set(String::new());
                    created_key.set(Some(key));
                }
                Err(e) => {
                    create_error.set(e);
                }
            }
            creating.set(false);
        });
    };

    let dismiss_created_key = move |_| {
        created_key.set(None);
        load_keys();
    };

    let on_revoke = move |id: String| {
        let id = id.clone();
        leptos::task::spawn_local(async move {
            let _ = api::revoke_key(&id).await;
            load_keys();
        });
    };

    view! {
        <div class="page-content space-y-6">
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
                if let Some(kind) = copy_notice.get() {
                    let msg = match kind {
                        CopyNoticeKind::Ok => use_translations().keys_copy_ok(),
                        CopyNoticeKind::Failed => use_translations().keys_copy_failed(),
                    };
                    view! {
                        <div class="text-xs text-accent font-medium">{msg}</div>
                    }.into_any()
                } else {
                    view! { <div></div> }.into_any()
                }
            }}

            <div class="glass-card p-4">
                {move || match network_info.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => {
                        let t = use_translations();
                        view! {
                            <div class="space-y-2">
                                <p class="text-xs text-error">{t.keys_network_load_failed()}</p>
                                <p class="text-xs text-theme-muted font-mono">{e}</p>
                                <button
                                    on:click=move |_| load_network_info()
                                    class="btn btn-secondary text-xs"
                                >
                                    {t.overview_refresh()}
                                </button>
                            </div>
                        }.into_any()
                    }
                    Some(Ok(info)) => {
                        let gateway_url = info.gateway_url.clone();
                        let lan_url = info.gateway_url_lan.clone();
                        let openresty_url = info.gateway_url_openresty.clone();
                        let t = use_translations();
                        view! {
                            <div class="flex items-center justify-between">
                                <div class="flex items-center gap-6 flex-wrap">
                                    <div class="flex items-center gap-3">
                                        <span class="text-xs text-theme-muted font-semibold">{t.keys_gateway_url_label()}</span>
                                        <code class="text-sm font-mono text-theme bg-theme-tertiary px-2 py-1 rounded">
                                            {gateway_url.clone()}
                                        </code>
                                        <button
                                            on:click={
                                                let url = gateway_url.clone();
                                                move |_| copy_text(url.clone())
                                            }
                                            class="text-xs text-accent hover:text-accent transition-colors"
                                        >
                                            {t.keys_copy_btn()}
                                        </button>
                                    </div>
                                    {if let Some(lan) = lan_url {
                                        view! {
                                            <div class="flex items-center gap-3">
                                                <span class="text-xs text-theme-muted font-semibold">{t.keys_lan_url_label()}</span>
                                                <code class="text-sm font-mono text-accent bg-accent/10 px-2 py-1 rounded">
                                                    {lan.clone()}
                                                </code>
                                                <button
                                                    on:click={
                                                        let url = lan.clone();
                                                        move |_| copy_text(url.clone())
                                                    }
                                                    class="text-xs text-accent hover:text-accent transition-colors"
                                                >
                                                    {t.keys_copy_btn()}
                                                </button>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }}
                                    {if let Some(proxy) = openresty_url {
                                        view! {
                                            <div class="flex items-center gap-3">
                                                <span class="text-xs text-theme-muted font-semibold">{t.keys_openresty_url_label()}</span>
                                                <code class="text-sm font-mono text-violet-400 bg-violet-500/10 px-2 py-1 rounded">
                                                    {proxy.clone()}
                                                </code>
                                                <button
                                                    on:click={
                                                        let url = proxy.clone();
                                                        move |_| copy_text(url.clone())
                                                    }
                                                    class="text-xs text-accent hover:text-accent transition-colors"
                                                >
                                                    {t.keys_copy_btn()}
                                                </button>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }}
                                </div>
                                <div class="flex items-center gap-3">
                                    <input
                                        type="text"
                                        placeholder=t.keys_search_placeholder()
                                        prop:value=move || search_query.get()
                                        on:input=move |ev| search_query.set(event_target_value(&ev))
                                        class="input w-64 text-sm"
                                    />
                                    <button
                                        on:click=move |_| load_keys()
                                        class="btn btn-secondary text-xs"
                                    >
                                        {t.overview_refresh()}
                                    </button>
                                </div>
                            </div>
                            <p class="text-xs text-theme-muted mt-2">
                                {t.keys_gateway_url_hint()}
                            </p>
                        }.into_any()
                    }
                }}
            </div>

            {move || {
                if show_create.get() {
                    let t = use_translations();
                    view! {
                        <div class="glass-card space-y-4">
                            <h3 class="text-sm font-semibold text-theme">{t.keys_create_title()}</h3>
                            <div class="grid grid-cols-2 gap-4">
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_name_label()}</label>
                                    <input
                                        type="text"
                                        prop:value=move || new_key_name.get()
                                        on:input=move |ev| {
                                            new_key_name.set(event_target_value(&ev));
                                        }
                                        class="input"
                                        placeholder="e.g. production-app"
                                    />
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_domain_label()}</label>
                                    <input
                                        type="text"
                                        prop:value=move || new_key_domain.get()
                                        on:input=move |ev| {
                                            new_key_domain.set(event_target_value(&ev));
                                        }
                                        class="input"
                                        placeholder="e.g. backend-team"
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
                            </div>
                            <div class="grid grid-cols-2 gap-4">
                                <div>
                                    <label class="flex items-center gap-2 text-xs text-theme-muted mb-1">
                                        <input
                                            type="checkbox"
                                            prop:checked=move || new_key_unlimited.get()
                                            on:change=move |ev| {
                                                new_key_unlimited.set(event_target_checked(&ev));
                                            }
                                            class="rounded"
                                        />
                                        {t.keys_unlimited_quota()}
                                    </label>
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_quota_label()}</label>
                                    <input
                                        type="number"
                                        prop:value=move || new_key_quota.get()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse() {
                                                new_key_quota.set(v);
                                            }
                                        }
                                        class="input"
                                        disabled=move || new_key_unlimited.get()
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
                                    disabled=move || creating.get() || new_key_name.get().is_empty()
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

            {move || {
                if let Some(key) = created_key.get() {
                    let t = use_translations();
                    let token = key.key_full.clone().unwrap_or_default();
                    let gateway_base = network_info
                        .get()
                        .and_then(|r| r.ok())
                        .map(|n| {
                            n.gateway_url_openresty
                                .clone()
                                .or(n.gateway_url_lan.clone())
                                .unwrap_or_else(|| n.gateway_url.clone())
                        })
                        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
                    let config_snippet = format!(
                        "OPENAI_API_BASE={}/v1\nOPENAI_API_KEY={}",
                        gateway_base.trim_end_matches('/'),
                        token
                    );
                    view! {
                        <div class="glass-card border-2 border-accent/30 space-y-4">
                            <h3 class="text-sm font-semibold text-accent">{t.keys_created_title()}</h3>
                            <p class="text-xs text-theme-muted">{t.keys_created_hint()}</p>
                            <code class="block text-sm font-mono text-theme bg-theme-tertiary px-3 py-2 rounded break-all">
                                {token.clone()}
                            </code>
                            <div class="flex flex-wrap gap-2">
                                <button
                                    on:click={
                                        let token = token.clone();
                                        move |_| copy_text(token.clone())
                                    }
                                    class="btn btn-primary text-sm"
                                >
                                    {t.keys_copy_btn()}
                                </button>
                                <button
                                    on:click={
                                        let snippet = config_snippet.clone();
                                        move |_| copy_text(snippet.clone())
                                    }
                                    class="btn btn-secondary text-sm"
                                >
                                    {t.keys_copy_config_btn()}
                                </button>
                                <button
                                    on:click=dismiss_created_key
                                    class="btn btn-secondary text-sm"
                                >
                                    {t.keys_created_done()}
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
                    let filtered_keys: Vec<ApiKey> = key_list
                        .into_iter()
                        .filter(|key| {
                            let query = search_query.get().to_lowercase();
                            if query.is_empty() {
                                true
                            } else {
                                key.name.to_lowercase().contains(&query)
                                    || key.key_preview.to_lowercase().contains(&query)
                            }
                        })
                        .collect();

                    if filtered_keys.is_empty() {
                        let t = use_translations();
                        view! { <EmptyState message=t.keys_no_results() /> }.into_any()
                    } else {
                        let t = use_translations();
                        view! {
                            <div class="glass-card-flat overflow-hidden p-0">
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th>{t.keys_col_name()}</th>
                                            <th>{t.keys_col_key()}</th>
                                            <th>{t.keys_col_quota()}</th>
                                            <th>{t.keys_col_tokens()}</th>
                                            <th>{t.keys_col_status()}</th>
                                            <th class="text-right">""</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {filtered_keys.into_iter().map(|key| {
                                            let id = key.id.clone();
                                            let display_key = key
                                                .key_full
                                                .clone()
                                                .unwrap_or_else(|| key.key_preview.clone());
                                            let can_copy = key.key_full.is_some();
                                            let quota_str = if key.unlimited_quota {
                                                t.keys_unlimited().to_string()
                                            } else {
                                                format!("{}", key.remain_quota)
                                            };
                                            let t = use_translations();
                                            view! {
                                                <tr>
                                                    <td class="text-theme font-medium">{key.name.clone()}</td>
                                                    <td>
                                                        <div class="flex items-center gap-2">
                                                            <code class="text-xs font-mono text-theme-secondary bg-theme-tertiary px-2 py-0.5 rounded">
                                                                {display_key.clone()}
                                                            </code>
                                                            {if can_copy {
                                                                view! {
                                                                    <button
                                                                        on:click={
                                                                            let token = display_key.clone();
                                                                            move |_| copy_text(token.clone())
                                                                        }
                                                                        class="text-xs text-accent hover:text-accent transition-colors"
                                                                    >
                                                                        {t.keys_copy_btn()}
                                                                    </button>
                                                                }.into_any()
                                                            } else {
                                                                view! {
                                                                    <span
                                                                        class="text-xs text-theme-muted"
                                                                        title=t.keys_copy_unavailable()
                                                                    >
                                                                        {t.keys_copy_unavailable()}
                                                                    </span>
                                                                }.into_any()
                                                            }}
                                                        </div>
                                                    </td>
                                                    <td class="font-mono tabular-nums text-theme">
                                                        {quota_str}
                                                    </td>
                                                    <td class="font-mono tabular-nums text-theme">
                                                        {format!("{}", key.tokens_used_this_month)}
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
