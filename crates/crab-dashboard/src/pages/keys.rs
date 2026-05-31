use leptos::prelude::*;

use crate::api;
use crate::clipboard;
use crate::components::horizontal_bar_chart::HorizontalBarChart;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{
    ApiKey, CreateKeyRequest, KeyConcurrencyResponse, KeyRoutingResponse, NetworkInfo,
    PatchKeyRequest,
};
use std::collections::{HashMap, HashSet};

fn build_key_usage_top10(keys: &[ApiKey]) -> (Vec<String>, Vec<f64>) {
    let mut rows: Vec<&ApiKey> = keys
        .iter()
        .filter(|k| k.tokens_used_this_month > 0)
        .collect();
    rows.sort_by(|a, b| b.tokens_used_this_month.cmp(&a.tokens_used_this_month));
    let labels: Vec<String> = rows.iter().take(10).map(|k| k.name.clone()).collect();
    let values: Vec<f64> = rows
        .iter()
        .take(10)
        .map(|k| k.tokens_used_this_month as f64)
        .collect();
    (labels, values)
}

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
    let key_routing: RwSignal<HashMap<String, Result<KeyRoutingResponse, String>>> =
        RwSignal::new(HashMap::new());
    let key_concurrency: RwSignal<HashMap<String, Result<KeyConcurrencyResponse, String>>> =
        RwSignal::new(HashMap::new());

    let show_copy_notice = move |kind: CopyNoticeKind| {
        copy_notice.set(Some(kind));
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(2000).await;
            copy_notice.try_set(None);
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
                Ok(k) => {
                    keys.try_set(Some(Ok(k)));
                }
                Err(e) => {
                    keys.try_set(Some(Err(e)));
                }
            }
        });
    };

    let load_network_info = move || {
        network_info.set(None);
        leptos::task::spawn_local(async move {
            match api::fetch_network_info().await {
                Ok(info) => {
                    network_info.try_set(Some(Ok(info)));
                }
                Err(e) => {
                    network_info.try_set(Some(Err(e)));
                }
            }
        });
    };

    let load_key_routing = move |key_id: String| {
        if key_routing.with(|m| m.contains_key(&key_id)) {
            return;
        }
        leptos::task::spawn_local(async move {
            let result = api::fetch_key_routing(&key_id, 300).await;
            key_routing.try_update(|m| {
                m.insert(key_id, result);
            });
        });
    };

    let load_key_concurrency = move |key_id: String| {
        if key_concurrency.with(|m| m.contains_key(&key_id)) {
            return;
        }
        leptos::task::spawn_local(async move {
            let result = api::fetch_key_concurrency(&key_id, 300).await;
            key_concurrency.try_update(|m| {
                m.insert(key_id, result);
            });
        });
    };

    load_keys();
    load_network_info();

    leptos::task::spawn_local(async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(5_000).await;
            load_keys();
        }
    });

    let show_create = RwSignal::new(false);
    let new_key_name = RwSignal::new(String::new());
    let new_key_domain = RwSignal::new(String::new());
    let new_key_project_id = RwSignal::new(String::new());
    let new_key_rpm = RwSignal::new(0u32);
    let new_key_budget = RwSignal::new(1_000_000u64);
    let new_key_unlimited = RwSignal::new(true);
    let new_key_quota = RwSignal::new(1_000_000i64);
    let new_key_max_concurrent = RwSignal::new(0u32);
    let new_key_pipeline = RwSignal::new(String::new());
    let new_key_upstream_profile = RwSignal::new(String::new());
    let pipeline_profiles: RwSignal<Option<Vec<String>>> = RwSignal::new(None);
    let creating = RwSignal::new(false);
    let create_error = RwSignal::new(String::new());
    let edit_error = RwSignal::new(String::new());
    let edit_success = RwSignal::new(false);

    let selected_keys: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    let show_confirm_revoke: RwSignal<Option<String>> = RwSignal::new(None);
    let show_confirm_batch_revoke = RwSignal::new(false);
    let revoke_message: RwSignal<String> = RwSignal::new(String::new());

    let toggle_select = move |id: &str| {
        let mut set = selected_keys.get();
        if set.contains(id) {
            set.remove(id);
        } else {
            set.insert(id.to_string());
        }
        selected_keys.set(set);
    };

    let toggle_select_all = move |ids: &[String]| {
        let current = selected_keys.get();
        let all_selected = ids.iter().all(|id| current.contains(id));
        if all_selected {
            selected_keys.set(HashSet::new());
        } else {
            selected_keys.set(ids.iter().cloned().collect());
        }
    };

    let do_batch_revoke = move || {
        let ids: Vec<String> = selected_keys.get().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::batch_revoke_keys(&ids).await {
                Ok(_) => {
                    show_confirm_batch_revoke.try_set(false);
                    selected_keys.try_set(HashSet::new());
                    load_keys();
                }
                Err(e) => {
                    revoke_message.try_set(e);
                }
            }
        });
    };

    let do_revoke = move |id: &str| {
        let id = id.to_string();
        show_confirm_revoke.set(None);
        leptos::task::spawn_local(async move {
            if let Err(e) = api::revoke_key(&id).await {
                revoke_message.try_set(e);
            } else {
                load_keys();
            }
        });
    };

    let on_create = move |_| {
        creating.set(true);
        create_error.set(String::new());
        let domain = new_key_domain.get();
        let project_raw = new_key_project_id.get();
        if !project_raw.trim().is_empty()
            && (project_raw.trim().len() > 512
                || !project_raw
                    .trim()
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        {
            create_error
                .set("project_id must match [a-zA-Z0-9\\-_]+ and be at most 512 characters".into());
            creating.set(false);
            return;
        }
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
            project_id: {
                let p = new_key_project_id.get();
                if p.trim().is_empty() {
                    None
                } else {
                    Some(p.trim().to_string())
                }
            },
            pipeline: {
                let p = new_key_pipeline.get();
                if p.is_empty() || p == "auto" {
                    None
                } else {
                    Some(p)
                }
            },
            upstream_profile: {
                let p = new_key_upstream_profile.get();
                if p.is_empty() { None } else { Some(p) }
            },
            max_concurrent: Some(new_key_max_concurrent.get()),
        };
        leptos::task::spawn_local(async move {
            match api::create_key(&req).await {
                Ok(key) => {
                    show_create.try_set(false);
                    new_key_name.try_set(String::new());
                    created_key.try_set(Some(key));
                }
                Err(e) => {
                    create_error.try_set(e);
                }
            }
            creating.try_set(false);
        });
    };

    let dismiss_created_key = move |_| {
        created_key.set(None);
        load_keys();
    };

    let editing_key_id: RwSignal<Option<String>> = RwSignal::new(None);
    let edit_name = RwSignal::new(String::new());
    let edit_domain = RwSignal::new(String::new());
    let edit_project_id = RwSignal::new(String::new());
    let edit_pipeline = RwSignal::new(String::new());
    let edit_upstream_profile = RwSignal::new(String::new());
    let edit_enabled = RwSignal::new(true);
    let edit_max_concurrent = RwSignal::new(0u32);
    let edit_rpm = RwSignal::new(0u32);
    let edit_budget = RwSignal::new(0u64);
    let edit_unlimited = RwSignal::new(true);
    let edit_quota = RwSignal::new(0i64);

    let start_edit = move |key: ApiKey| {
        editing_key_id.set(Some(key.id.clone()));
        edit_error.set(String::new());
        edit_success.set(false);
        copy_notice.set(None);
        edit_name.set(key.name.clone());
        edit_domain.set(key.domain.clone().unwrap_or_default());
        edit_project_id.set(key.project_id.clone().unwrap_or_default());
        edit_pipeline.set(key.pipeline.clone().unwrap_or_default());
        edit_upstream_profile.set(key.upstream_profile.clone().unwrap_or_default());
        edit_enabled.set(key.active);
        edit_max_concurrent.set(key.max_concurrent);
        edit_rpm.set(key.rpm_limit);
        edit_budget.set(key.monthly_token_budget);
        edit_unlimited.set(key.unlimited_quota);
        edit_quota.set(key.remain_quota);
        leptos::task::spawn_local(async move {
            if let Ok(cfg) = api::fetch_pipeline_runtime().await {
                pipeline_profiles.try_set(Some(cfg.profiles.into_iter().map(|p| p.id).collect()));
            }
        });
    };

    let cancel_edit = move || {
        editing_key_id.set(None);
    };

    let on_save_edit = move |id: &str| {
        let id = id.to_string();
        let name_val = edit_name.get();
        let domain_val = edit_domain.get();
        let project_id_val = edit_project_id.get();
        let pipeline_val = edit_pipeline.get();
        let upstream_profile_val = edit_upstream_profile.get();
        let enabled_val = edit_enabled.get();
        let max_concurrent_val = edit_max_concurrent.get();
        let rpm_val = edit_rpm.get();
        let budget_val = edit_budget.get();
        let unlimited_val = edit_unlimited.get();
        let quota_val = edit_quota.get();

        let req = PatchKeyRequest {
            name: {
                let v = name_val.trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            },
            enabled: Some(enabled_val),
            domain: {
                let v = domain_val.trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            },
            project_id: {
                let v = project_id_val.trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            },
            pipeline: {
                let v = pipeline_val.trim().to_string();
                if v.is_empty() || v == "auto" {
                    None
                } else {
                    Some(v)
                }
            },
            upstream_profile: {
                let v = upstream_profile_val.trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            },
            max_concurrent: Some(max_concurrent_val),
            rpm_limit: Some(rpm_val),
            monthly_token_budget: Some(budget_val),
            remain_quota: if unlimited_val { None } else { Some(quota_val) },
            unlimited_quota: Some(unlimited_val),
        };
        leptos::task::spawn_local(async move {
            edit_error.try_set(String::new());
            match api::patch_key(&id, &req).await {
                Ok(_) => {
                    edit_success.try_set(true);
                    editing_key_id.try_set(None);
                    load_keys();
                    leptos::task::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(3000).await;
                        edit_success.try_set(false);
                    });
                }
                Err(e) => {
                    edit_error.try_set(e);
                }
            }
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
                    on:click=move |_| {
                        show_create.set(true);
                        edit_success.set(false);
                        leptos::task::spawn_local(async move {
                            if let Ok(cfg) = api::fetch_pipeline_runtime().await {
                                pipeline_profiles.try_set(Some(
                                    cfg.profiles.into_iter().map(|p| p.id).collect(),
                                ));
                            }
                        });
                    }
                    class="btn btn-primary text-sm"
                >
                    {t.keys_new_btn()}
                </button>
            </div>

            {move || {
                if edit_success.get() {
                    let t = use_translations();
                    view! {
                        <div class="text-xs text-teal-400 font-medium" role="status">
                            {t.keys_edit_success()}
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}

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
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_project_id_label()}</label>
                                    <input
                                        type="text"
                                        prop:value=move || new_key_project_id.get()
                                        on:input=move |ev| {
                                            new_key_project_id.set(event_target_value(&ev));
                                        }
                                        class="input font-mono text-sm"
                                        placeholder="e.g. project_alpha"
                                    />
                                    <p class="text-xs text-theme-muted mt-1">{t.keys_project_id_hint()}</p>
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_rpm_label()}</label>
                                    <input
                                        type="number"
                                        min="0"
                                        prop:value=move || new_key_rpm.get()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse() {
                                                new_key_rpm.set(v);
                                            }
                                        }
                                        class="input"
                                    />
                                    <p class="text-xs text-theme-muted mt-1">{t.keys_rpm_hint()}</p>
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_budget_label()}</label>
                                    <input
                                        type="number"
                                        min="0"
                                        prop:value=move || new_key_budget.get()
                                        on:input=move |ev| {
                                            if let Ok(v) = event_target_value(&ev).parse() {
                                                new_key_budget.set(v);
                                            }
                                        }
                                        class="input"
                                    />
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_pipeline_label()}</label>
                                    <select
                                        class="input w-full"
                                        prop:value=move || new_key_pipeline.get()
                                        on:change=move |ev| new_key_pipeline.set(event_target_value(&ev))
                                    >
                                        <option value="auto">{t.keys_override_auto()}</option>
                                        <option value="cursor_deepseek_v4">"cursor_deepseek_v4"</option>
                                        <option value="deepseek_light">"deepseek_light"</option>
                                        <option value="mimo_relay">"mimo_relay"</option>
                                        <option value="mimo_token_plan_relay">"mimo_token_plan_relay"</option>
                                        <option value="mimo_payg_relay">"mimo_payg_relay"</option>
                                        <option value="generic_relay">"generic_relay"</option>
                                    </select>
                                </div>
                                <div>
                                    <label class="block text-xs text-theme-muted mb-1">{t.keys_upstream_profile_label()}</label>
                                    <select
                                        class="input w-full"
                                        prop:value=move || new_key_upstream_profile.get()
                                        on:change=move |ev| new_key_upstream_profile.set(event_target_value(&ev))
                                    >
                                        <option value="">{t.keys_override_auto()}</option>
                                        {move || pipeline_profiles.get().unwrap_or_default().into_iter().map(|id| {
                                            view! { <option value=id.clone()>{id.clone()}</option> }
                                        }).collect_view()}
                                    </select>
                                    <p class="text-xs text-theme-muted mt-1">{t.keys_upstream_profile_hint()}</p>
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
                            <div>
                                <label class="block text-xs text-theme-muted mb-1">{t.keys_max_concurrent_label()}</label>
                                <input type="number" min="0" prop:value=move || new_key_max_concurrent.get() on:input=move |ev| {
                                    if let Ok(v) = event_target_value(&ev).parse() { new_key_max_concurrent.set(v); }
                                } class="input w-full" />
                            </div>
                            {move || {
                                let err = create_error.get();
                                (!err.is_empty()).then(|| view! { <div class="text-xs text-error">{err}</div> })
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
                        let all_ids: Vec<String> = filtered_keys.iter().map(|k| k.id.clone()).collect();
                        let (usage_labels, usage_values) = build_key_usage_top10(&filtered_keys);
                        let usage_labels = std::sync::Arc::new(usage_labels);
                        let usage_values = std::sync::Arc::new(usage_values);
                        let usage_labels_sig = {
                            let usage_labels = std::sync::Arc::clone(&usage_labels);
                            Signal::derive(move || usage_labels.as_ref().clone())
                        };
                        let usage_values_sig = {
                            let usage_values = std::sync::Arc::clone(&usage_values);
                            Signal::derive(move || usage_values.as_ref().clone())
                        };
                        let select_count = move || selected_keys.get().len();
                        view! {
                            <div class="glass-card p-4 mb-4">
                                <h3 class="text-sm font-semibold text-theme mb-3">"Top 10 Key Token Usage"</h3>
                                <HorizontalBarChart
                                    labels=usage_labels_sig
                                    values=usage_values_sig
                                    width=520
                                    height_px=180
                                    empty_message="No token usage data."
                                />
                            </div>
                            {move || {
                                let count = select_count();
                                if count > 0 {
                                    view! {
                                        <div class="flex items-center gap-2 mb-2">
                                            <span class="text-xs text-theme-secondary">{format!("{} selected", count)}</span>
                                            <button
                                                on:click=move |_| show_confirm_batch_revoke.set(true)
                                                class="btn btn-secondary text-xs"
                                            >
                                                {t.keys_revoke_btn()}
                                            </button>
                                        </div>
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }
                            }}
                            <div class="glass-card-flat overflow-hidden p-0">
                                <table class="table">
                                    <thead>
                                        <tr>
                                            <th class="w-8">
                                                <input
                                                    type="checkbox"
                                                    prop:checked={
                                                        let ids = all_ids.clone();
                                                        move || { let s = selected_keys.get(); ids.iter().all(|id| s.contains(id)) && !ids.is_empty() }
                                                    }
                                                    on:change={
                                                        let ids = all_ids;
                                                        move |_| toggle_select_all(&ids)
                                                    }
                                                    class="rounded"
                                                />
                                            </th>
                                            <th>{t.keys_col_name()}</th>
                                            <th>{t.keys_col_key()}</th>
                                            <th>{t.keys_col_rpm()}</th>
                                            <th>{t.keys_col_concurrency()}</th>
                                            <th>"Routing"</th>
                                            <th>"Prefix"</th>
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
                                            let rpm_str = if key.rpm_limit == 0 {
                                                t.keys_unlimited().to_string()
                                            } else {
                                                format!("{}", key.rpm_limit)
                                            };
                                            let concurrency_str = if key.max_concurrent == 0 {
                                                format!("{} ({})", t.keys_unlimited(), key.inflight)
                                            } else {
                                                format!("{}/{}", key.inflight, key.max_concurrent)
                                            };
                                            let quota_str = if key.unlimited_quota {
                                                t.keys_unlimited().to_string()
                                            } else {
                                                format!("{}", key.remain_quota)
                                            };
                                            let id_for_check = id.clone();
                                            let id_for_peak = id.clone();
                                            let id_for_routing = id.clone();
                                            let id_for_prefix = id.clone();
                                            let is_checked = move || selected_keys.get().contains(&id_for_check);
                                            let id_for_edit = id.clone();
                                            let is_editing = move || editing_key_id.get().as_deref() == Some(&id_for_edit);
                                            let key_for_edit = key.clone();
                                            let inflight_now = key_for_edit.inflight;
                                            let max_concurrent_now = key_for_edit.max_concurrent;
                                            load_key_routing(id.clone());
                                            load_key_concurrency(id.clone());
                                            let t = use_translations();
                                            if is_editing() {
                                                view! {
                                                            <tr class="bg-theme-tertiary/40">
                                                                <td colspan="11" class="p-3">
                                                                    <h4 class="text-xs font-semibold text-theme mb-2">{t.keys_edit_title()}</h4>
                                                                    <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_name_label()}</label>
                                                                            <input type="text" prop:value=move || edit_name.get() on:input=move |ev| edit_name.set(event_target_value(&ev)) class="input text-sm" />
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_domain_label()}</label>
                                                                            <input type="text" prop:value=move || edit_domain.get() on:input=move |ev| edit_domain.set(event_target_value(&ev)) class="input text-sm" />
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_project_id_label()}</label>
                                                                            <input type="text" prop:value=move || edit_project_id.get() on:input=move |ev| edit_project_id.set(event_target_value(&ev)) class="input text-sm font-mono" />
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_rpm_label()}</label>
                                                                            <input type="number" min="0" prop:value=move || edit_rpm.get() on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { edit_rpm.set(v); } } class="input text-sm" />
                                                                            <p class="text-xs text-theme-muted mt-0.5">{t.keys_rpm_hint()}</p>
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_max_concurrent_label()}</label>
                                                                            <input type="number" min="0" prop:value=move || edit_max_concurrent.get() on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { edit_max_concurrent.set(v); } } class="input text-sm" />
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_budget_label()}</label>
                                                                            <input type="number" min="0" prop:value=move || edit_budget.get() on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { edit_budget.set(v); } } class="input text-sm" />
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_pipeline_label()}</label>
                                                                            <select prop:value=move || edit_pipeline.get() on:change=move |ev| edit_pipeline.set(event_target_value(&ev)) class="input w-full text-sm">
                                                                                <option value="auto">{t.keys_override_auto()}</option>
                                                                                <option value="cursor_deepseek_v4">"cursor_deepseek_v4"</option>
                                                                                <option value="deepseek_light">"deepseek_light"</option>
                                                                                <option value="mimo_relay">"mimo_relay"</option>
                                                                                <option value="mimo_token_plan_relay">"mimo_token_plan_relay"</option>
                                                                                <option value="mimo_payg_relay">"mimo_payg_relay"</option>
                                                                                <option value="generic_relay">"generic_relay"</option>
                                                                            </select>
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_upstream_profile_label()}</label>
                                                                            <select prop:value=move || edit_upstream_profile.get() on:change=move |ev| edit_upstream_profile.set(event_target_value(&ev)) class="input w-full text-sm">
                                                                                <option value="">{t.keys_override_auto()}</option>
                                                                                {move || pipeline_profiles.get().unwrap_or_default().into_iter().map(|pid| {
                                                                                    view! { <option value=pid.clone()>{pid.clone()}</option> }
                                                                                }).collect_view()}
                                                                            </select>
                                                                        </div>
                                                                        <div class="flex flex-col gap-2 justify-end">
                                                                            <label class="flex items-center gap-2 text-xs text-theme-muted">
                                                                                <input type="checkbox" prop:checked=move || edit_enabled.get() on:change=move |ev| edit_enabled.set(event_target_checked(&ev)) class="rounded" />
                                                                                {t.keys_status_active()}
                                                                            </label>
                                                                            <label class="flex items-center gap-2 text-xs text-theme-muted">
                                                                                <input type="checkbox" prop:checked=move || edit_unlimited.get() on:change=move |ev| edit_unlimited.set(event_target_checked(&ev)) class="rounded" />
                                                                                {t.keys_unlimited_quota()}
                                                                            </label>
                                                                        </div>
                                                                        <div>
                                                                            <label class="block text-xs text-theme-muted mb-1">{t.keys_quota_label()}</label>
                                                                            <input type="number" prop:value=move || edit_quota.get() on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { edit_quota.set(v); } } class="input text-sm" disabled=move || edit_unlimited.get() />
                                                                        </div>
                                                                    </div>
                                                                    {move || {
                                                                        let err = edit_error.get();
                                                                        (!err.is_empty()).then(|| view! { <p class="text-xs text-error mt-2">{err}</p> })
                                                                    }}
                                                                    <div class="flex gap-2 mt-3">
                                                                        <button on:click={
                                                                            let id = id.clone();
                                                                            move |_| on_save_edit(&id)
                                                                        } class="btn btn-primary text-xs">{t.keys_save_btn()}</button>
                                                                        <button on:click=move |_| cancel_edit() class="btn btn-secondary text-xs">{t.keys_cancel()}</button>
                                                                    </div>
                                                                </td>
                                                            </tr>
                                                        }.into_any()
                                                    } else {
                                                        view! {
                                                            <tr>
                                                                <td class="w-8">
                                                                    <input
                                                                        type="checkbox"
                                                                        prop:checked=is_checked
                                                                        on:change={
                                                                            let id = id.clone();
                                                                            move |_| toggle_select(&id)
                                                                        }
                                                                        class="rounded"
                                                                    />
                                                                </td>
                                                                <td class="text-theme font-medium">{key_for_edit.name.clone()}</td>
                                                                <td>
                                                                    <div class="flex items-center gap-2 max-w-md">
                                                                        <code class="text-xs font-mono text-theme-secondary bg-theme-tertiary px-2 py-0.5 rounded break-all">
                                                                            {display_key.clone()}
                                                                        </code>
                                                                        {if can_copy {
                                                                            view! {
                                                                                <button
                                                                                    on:click={
                                                                                        let token = display_key.clone();
                                                                                        move |_| copy_text(token.clone())
                                                                                    }
                                                                                    class="btn btn-secondary text-xs shrink-0"
                                                                                >
                                                                                    {t.keys_copy_btn()}
                                                                                </button>
                                                                            }.into_any()
                                                                        } else {
                                                                            view! {
                                                                                <span
                                                                                    class="text-xs text-theme-muted shrink-0"
                                                                                    title=t.keys_copy_unavailable()
                                                                                >
                                                                                    {t.keys_copy_unavailable()}
                                                                                </span>
                                                                            }.into_any()
                                                                        }}
                                                                    </div>
                                                                </td>
                                                                <td class="font-mono tabular-nums text-theme text-sm">
                                                                    {rpm_str}
                                                                </td>
                                                                <td class="font-mono tabular-nums text-theme text-sm">
                                                                    <div class="space-y-1">
                                                                        <div>{concurrency_str}</div>
                                                                        <div class="text-[11px] text-theme-muted">
                                                                            {move || {
                                                                                if let Some(Ok(c)) = key_concurrency.with(|m| m.get(&id_for_peak).cloned()) {
                                                                                    format!("peak {}", c.concurrent_peak)
                                                                                } else {
                                                                                    "peak ...".to_string()
                                                                                }
                                                                            }}
                                                                        </div>
                                                                        <div class="h-1.5 w-24 bg-theme-tertiary rounded overflow-hidden">
                                                                            <div
                                                                                class="h-full bg-accent"
                                                                                style:width=move || {
                                                                                    let max = max_concurrent_now;
                                                                                    let ratio = if max == 0 {
                                                                                        0.0
                                                                                    } else {
                                                                                        (inflight_now as f64 / max as f64).clamp(0.0, 1.0)
                                                                                    };
                                                                                    format!("{:.0}%", ratio * 100.0)
                                                                                }
                                                                            ></div>
                                                                        </div>
                                                                    </div>
                                                                </td>
                                                                <td class="text-theme text-xs">
                                                                    {move || {
                                                                        if let Some(Ok(r)) = key_routing.with(|m| m.get(&id_for_routing).cloned()) {
                                                                            let total: u64 = r.backends.iter().map(|b| b.request_count).sum();
                                                                            if let Some(top) = r.backends.iter().max_by_key(|b| b.request_count) {
                                                                                if total > 0 {
                                                                                    format!("{} ({:.0}%)", top.backend_name, top.request_count as f64 * 100.0 / total as f64)
                                                                                } else {
                                                                                    "n/a".to_string()
                                                                                }
                                                                            } else {
                                                                                "n/a".to_string()
                                                                            }
                                                                        } else {
                                                                            "loading...".to_string()
                                                                        }
                                                                    }}
                                                                </td>
                                                                <td class="text-theme text-xs">
                                                                    {move || {
                                                                        if let Some(Ok(r)) = key_routing.with(|m| m.get(&id_for_prefix).cloned()) {
                                                                            let total: u64 = r.backends.iter().map(|b| b.request_count).sum();
                                                                            if total == 0 {
                                                                                "n/a".to_string()
                                                                            } else {
                                                                                let safe = total.saturating_sub(r.prefix_break_count);
                                                                                format!("{:.1}%", safe as f64 * 100.0 / total as f64)
                                                                            }
                                                                        } else {
                                                                            "loading...".to_string()
                                                                        }
                                                                    }}
                                                                </td>
                                                                <td class="font-mono tabular-nums text-theme text-sm">
                                                                    {quota_str}
                                                                </td>
                                                                <td class="font-mono tabular-nums text-theme text-sm">
                                                                    {format!("{}", key_for_edit.tokens_used_this_month)}
                                                                </td>
                                                                <td>
                                                                    {if key_for_edit.active {
                                                                        view! { <Badge text=t.keys_status_active().to_string() color="teal" /> }
                                                                    } else {
                                                                        view! { <Badge text=t.keys_status_revoked().to_string() color="rose" /> }
                                                                    }}
                                                                </td>
                                                                <td class="text-right whitespace-nowrap">
                                                                    {if key_for_edit.active {
                                                                        view! {
                                                                            <button
                                                                                on:click={
                                                                                    let k = key_for_edit.clone();
                                                                                    move |_| start_edit(k.clone())
                                                                                }
                                                                                class="text-xs text-accent hover:text-accent transition-colors mr-3"
                                                                            >
                                                                                {t.keys_edit_btn()}
                                                                            </button>
                                                                            <button
                                                                                on:click={
                                                                                    let id = id.clone();
                                                                                    move |_| show_confirm_revoke.set(Some(id.clone()))
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
                                                        }.into_any()
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
                if let Some(id) = show_confirm_revoke.get() {
                    let t = use_translations();
                    view! {
                        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
                            <div class="glass-card max-w-md w-full space-y-4">
                                <h4 class="text-sm font-semibold text-theme">{t.keys_revoke_btn()}</h4>
                                <p class="text-xs text-theme-muted">{t.keys_confirm_revoke_body()}</p>
                                <div class="flex gap-2 justify-end">
                                    <button class="btn btn-secondary text-xs" on:click=move |_| show_confirm_revoke.set(None)>{t.keys_cancel()}</button>
                                    <button
                                        class="btn btn-primary text-xs"
                                        on:click={
                                            let id = id.clone();
                                            move |_| do_revoke(&id)
                                        }
                                    >
                                        {t.keys_revoke_btn()}
                                    </button>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}

            {move || {
                if show_confirm_batch_revoke.get() {
                    let t = use_translations();
                    view! {
                        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
                            <div class="glass-card max-w-md w-full space-y-4">
                                <h4 class="text-sm font-semibold text-theme">{t.keys_batch_revoke_title()}</h4>
                                <p class="text-xs text-theme-muted">{t.keys_batch_revoke_body()}</p>
                                <div class="flex gap-2 justify-end">
                                    <button class="btn btn-secondary text-xs" on:click=move |_| show_confirm_batch_revoke.set(false)>{t.keys_cancel()}</button>
                                    <button class="btn btn-primary text-xs" on:click=move |_| do_batch_revoke()>{t.keys_revoke_btn()}</button>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}

            {move || {
                let msg = revoke_message.get();
                if !msg.is_empty() {
                    let t = use_translations();
                    view! {
                        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
                            <div class="glass-card max-w-md w-full space-y-4">
                                <p class="text-xs text-error">{msg}</p>
                                <div class="flex gap-2 justify-end">
                                    <button class="btn btn-secondary text-xs" on:click=move |_| revoke_message.set(String::new())>{t.keys_cancel()}</button>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}
        </div>
    }
}
