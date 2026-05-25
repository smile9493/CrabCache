use std::collections::HashMap;

use leptos::prelude::*;

use crate::api;
use crate::components::sync_result::SyncResultCard;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{
    KeyQuotaInfo, PatchUpstreamKeyRequest, PutUpstreamKeysRequest, PutUpstreamProfileAdminRequest,
    SyncResult, UpdateUpstreamConfigRequest, UpstreamKeyInput, UpstreamKeysPutMode,
    UpstreamKeysView, UpstreamTestBody, UpstreamProfileAdminView, UpstreamTestResult,
};

const OFFICIAL_BASE: &str = "https://api.deepseek.com";
const DEFAULT_MODEL: &str = "deepseek-v4-pro";
const MIMO_BASE: &str = "https://api.xiaomimimo.com";
const MIMO_MODEL: &str = "xiaomi/mimo-v2.5-pro";

fn parse_upstream_pool_line(line: &str) -> (String, String) {
    let t = line.trim();
    if let Some((account_id, secret)) = t.split_once(':') {
        let account_id = account_id.trim();
        let secret = secret.trim();
        if !account_id.is_empty() && !secret.is_empty() {
            return (account_id.to_string(), secret.to_string());
        }
    }
    (String::new(), t.to_string())
}

fn pool_lines_to_key_inputs(lines: Vec<String>) -> Vec<UpstreamKeyInput> {
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let (account_id, secret) = parse_upstream_pool_line(&line);
            UpstreamKeyInput {
                id: format!("key-{}", i + 1),
                secret,
                enabled: true,
                account_id,
            }
        })
        .collect()
}

fn validate_base_url(url: &str) -> Option<String> {
    let t = url.trim();
    if t.is_empty() {
        return Some("Base URL is required".into());
    }
    if t.ends_with("/v1") || t.contains("/v1/") {
        return Some("Do not include /v1 in Base URL".into());
    }
    if !t.starts_with("http://") && !t.starts_with("https://") {
        return Some("Base URL must start with http:// or https://".into());
    }
    None
}

fn first_key_from_text(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[component]
pub fn UpstreamPage() -> impl IntoView {
    let t = use_translations();
    
    // Page state
    let profiles: RwSignal<Vec<UpstreamProfileAdminView>> = RwSignal::new(Vec::new());
    let active_profile = RwSignal::new("deepseek".to_string());
    let is_creating = RwSignal::new(false);
    let gateway_reachable = RwSignal::new(true);

    // Form inputs
    let provider = RwSignal::new("deepseek".to_string());
    let base_url = RwSignal::new(OFFICIAL_BASE.to_string());
    let model = RwSignal::new(DEFAULT_MODEL.to_string());
    let endpoints_text = RwSignal::new(String::new());
    let tls_sni = RwSignal::new(String::new());
    let show_advanced = RwSignal::new(false);

    // Inline creation state
    let new_profile_id = RwSignal::new(String::new());

    // Connection testing state
    let testing = RwSignal::new(false);
    let test_result: RwSignal<Option<UpstreamTestResult>> = RwSignal::new(None);
    let test_error = RwSignal::new(String::new());

    // Saving configuration state
    let saving = RwSignal::new(false);
    let saved = RwSignal::new(false);
    let save_error = RwSignal::new(String::new());
    let sync_result: RwSignal<Option<SyncResult>> = RwSignal::new(None);

    // Key pool state
    let key_pool: RwSignal<Option<Result<UpstreamKeysView, String>>> = RwSignal::new(None);
    let pool_secrets_text = RwSignal::new(String::new());
    let pool_replace_mode = RwSignal::new(false);
    let pool_saving = RwSignal::new(false);
    let pool_saved = RwSignal::new(false);
    let pool_error = RwSignal::new(String::new());

    // Per-key quota test state
    let key_test_results: RwSignal<HashMap<String, UpstreamTestResult>> =
        RwSignal::new(HashMap::new());
    let key_testing: RwSignal<HashMap<String, bool>> = RwSignal::new(HashMap::new());
    let testing_all = RwSignal::new(false);

    // Profile deletion state
    let show_delete_confirm = RwSignal::new(false);
    let deleting = RwSignal::new(false);

    // API loaders
    let load_key_pool = move |pid: String| {
        key_pool.set(None);
        key_test_results.set(HashMap::new());
        key_testing.set(HashMap::new());
        leptos::task::spawn_local(async move {
            let result = if pid == "deepseek" {
                api::fetch_upstream_keys().await
            } else {
                api::fetch_upstream_profile_keys(&pid)
                    .await
                    .map(|v| UpstreamKeysView { keys: v.keys })
            };
            match result {
                Ok(v) => key_pool.set(Some(Ok(v))),
                Err(e) => key_pool.set(Some(Err(e))),
            }
        });
    };

    let load_profile_data = move |pid: String| {
        // Reset states
        test_result.set(None);
        test_error.set(String::new());
        save_error.set(String::new());
        saved.set(false);
        sync_result.set(None);
        
        let p_list = profiles.get();
        if pid == "deepseek" {
            leptos::task::spawn_local(async move {
                match api::fetch_upstream_config().await {
                    Ok(c) => {
                        base_url.set(c.base_url.clone());
                        model.set(c.model.clone());
                        endpoints_text.set(c.endpoints.join("\n"));
                        tls_sni.set(String::new());
                        provider.set("deepseek".to_string());
                        gateway_reachable.set(c.gateway_reachable);
                        if let Some(ref lt) = c.last_test {
                            test_result.set(Some(lt.clone()));
                        }
                    }
                    Err(e) => {
                        save_error.set(e);
                    }
                }
            });
        } else if let Some(p) = p_list.iter().find(|p| p.id == pid) {
            provider.set(p.provider.clone());
            base_url.set(p.base_url.clone());
            model.set(p.fallback_model.clone());
            endpoints_text.set(p.endpoints.join("\n"));
            tls_sni.set(p.tls_sni.clone());
        }
        load_key_pool(pid);
    };

    let load_profiles_and_select = move |pid: String| {
        leptos::task::spawn_local(async move {
            if let Ok(resp) = api::fetch_upstream_profiles().await {
                profiles.set(resp.profiles);
            }
            load_profile_data(pid);
        });
    };

    // Initial load
    load_profiles_and_select("deepseek".to_string());

    // Actions
    let on_test = move |_| {
        testing.set(true);
        test_result.set(None);
        test_error.set(String::new());
        
        let url = base_url.get();
        if let Some(err) = validate_base_url(&url) {
            test_error.set(err);
            testing.set(false);
            return;
        }

        let pid = active_profile.get();
        let bulk_key = first_key_from_text(&pool_secrets_text.get());

        leptos::task::spawn_local(async move {
            if let Some(key) = bulk_key {
                // If a new bulk key is entered, test using that key
                match api::test_upstream_connection(&UpstreamTestBody {
                    base_url: url,
                    api_key: key,
                })
                .await
                {
                    Ok(r) => test_result.set(Some(r)),
                    Err(e) => test_error.set(e),
                }
            } else if pid == "deepseek" || profiles.get().iter().any(|p| p.id == pid) {
                // Otherwise test saved profile keys
                match api::test_upstream_profile(&pid).await {
                    Ok(r) => test_result.set(Some(r)),
                    Err(e) => test_error.set(e),
                }
            } else {
                test_error.set("Please enter a key in bulk input to test this unsaved profile.".to_string());
            }
            testing.set(false);
        });
    };

    let on_save = move |_| {
        saving.set(true);
        saved.set(false);
        save_error.set(String::new());
        sync_result.set(None);

        let url = base_url.get().trim().to_string();
        if let Some(err) = validate_base_url(&url) {
            save_error.set(err);
            saving.set(false);
            return;
        }
        let model_val = model.get().trim().to_string();
        if model_val.is_empty() {
            save_error.set("Model is required".into());
            saving.set(false);
            return;
        }

        let endpoints: Vec<String> = endpoints_text
            .get()
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let keys_to_append: Vec<String> = pool_secrets_text
            .get()
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        let pid = active_profile.get();
        let prov = provider.get();
        let sni_val = tls_sni.get().trim().to_string();
        let sni = if sni_val.is_empty() { None } else { Some(sni_val) };
        let keys_to_append_clone = keys_to_append.clone();

        leptos::task::spawn_local(async move {
            let result = if pid == "deepseek" {
                let req = UpdateUpstreamConfigRequest {
                    base_url: url.clone(),
                    model: model_val.clone(),
                    api_key: None,
                    endpoints: endpoints.clone(),
                    keys_to_append: keys_to_append_clone.clone(),
                };
                api::update_upstream_config(&req).await.map(|resp| {
                    if let Some(s) = resp.sync {
                        sync_result.set(Some(s));
                    }
                })
            } else {
                let req = PutUpstreamProfileAdminRequest {
                    provider: prov,
                    base_url: url.clone(),
                    fallback_model: model_val.clone(),
                    endpoints: endpoints.clone(),
                    tls_sni: sni,
                };
                api::put_upstream_profile(&pid, &req).await.map(|_| ())
            };

            match result {
                Ok(_) => {
                    if !keys_to_append_clone.is_empty() {
                        let keys = pool_lines_to_key_inputs(keys_to_append_clone);
                        let key_req = PutUpstreamKeysRequest {
                            keys,
                            mode: UpstreamKeysPutMode::Append,
                        };
                        let key_err = if pid == "deepseek" {
                            api::put_upstream_keys(&key_req).await.err()
                        } else {
                            api::put_upstream_profile_keys(&pid, &key_req)
                                .await
                                .err()
                        };
                        if let Some(e) = key_err {
                            save_error.set(format!("Profile saved but keys failed: {e}"));
                        } else {
                            pool_secrets_text.set(String::new());
                        }
                    } else {
                        pool_secrets_text.set(String::new());
                    }
                    saved.set(true);
                    load_profiles_and_select(pid);
                }
                Err(e) => save_error.set(e),
            }
            saving.set(false);
        });
    };

    let on_save_pool = move |_| {
        pool_saving.set(true);
        pool_saved.set(false);
        pool_error.set(String::new());
        let secrets: Vec<String> = pool_secrets_text
            .get()
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if secrets.is_empty() {
            pool_error.set(use_translations().upstream_pool_empty_keys_error().to_string());
            pool_saving.set(false);
            return;
        }
        let pid = active_profile.get();
        let keys = pool_lines_to_key_inputs(secrets);
        let mode = if pool_replace_mode.get() {
            UpstreamKeysPutMode::Replace
        } else {
            UpstreamKeysPutMode::Append
        };
        let req = PutUpstreamKeysRequest { keys, mode };
        leptos::task::spawn_local(async move {
            let result = if pid == "deepseek" {
                api::put_upstream_keys(&req).await
            } else {
                api::put_upstream_profile_keys(&pid, &req)
                    .await
                    .map(|v| UpstreamKeysView { keys: v.keys })
            };
            match result {
                Ok(v) => {
                    key_pool.set(Some(Ok(v)));
                    pool_secrets_text.set(String::new());
                    pool_saved.set(true);
                    // Refresh profiles list (since key count changed)
                    if let Ok(resp) = api::fetch_upstream_profiles().await {
                        profiles.set(resp.profiles);
                    }
                }
                Err(e) => pool_error.set(e),
            }
            pool_saving.set(false);
        });
    };

    let on_delete_profile = move |_| {
        let pid = active_profile.get();
        if pid == "deepseek" { return; }
        deleting.set(true);
        leptos::task::spawn_local(async move {
            match api::delete_upstream_profile(&pid).await {
                Ok(_) => {
                    show_delete_confirm.set(false);
                    active_profile.set("deepseek".to_string());
                    load_profiles_and_select("deepseek".to_string());
                }
                Err(e) => {
                    save_error.set(e);
                }
            }
            deleting.set(false);
        });
    };

    let on_create_profile = move |_| {
        let id = new_profile_id.get().trim().to_string();
        if id.is_empty() {
            save_error.set("Profile ID is required".to_string());
            return;
        }
        let prov = provider.get().trim().to_string();
        let url = base_url.get().trim().to_string();
        if let Some(err) = validate_base_url(&url) {
            save_error.set(err);
            return;
        }
        let model_val = model.get().trim().to_string();
        if model_val.is_empty() {
            save_error.set("Fallback model is required".to_string());
            return;
        }
        let sni_val = tls_sni.get().trim().to_string();
        let sni = if sni_val.is_empty() { None } else { Some(sni_val) };

        saving.set(true);
        leptos::task::spawn_local(async move {
            let req = PutUpstreamProfileAdminRequest {
                provider: prov,
                base_url: url,
                fallback_model: model_val,
                endpoints: Vec::new(),
                tls_sni: sni,
            };
            match api::put_upstream_profile(&id, &req).await {
                Ok(_) => {
                    is_creating.set(false);
                    new_profile_id.set(String::new());
                    active_profile.set(id.clone());
                    load_profiles_and_select(id);
                }
                Err(e) => {
                    save_error.set(e);
                }
            }
            saving.set(false);
        });
    };

    view! {
        <div class="page-content space-y-6">
            <SectionHeader
                title=t.upstream_title()
                description=t.upstream_desc()
            />

            <div class="glass-card text-sm text-theme-secondary">
                {t.upstream_l3_affinity_hint()}
            </div>

            {move || (!gateway_reachable.get()).then(|| view! {
                <div class="glass-card text-warning text-sm">
                    {t.upstream_gateway_unreachable()}
                </div>
            })}

            <div class="tab-bar">
                {move || profiles.get().into_iter().map(|p| {
                    let pid = p.id.clone();
                    let active = active_profile.get() == pid && !is_creating.get();
                    let badge_count = p.key_pool_count;
                    view! {
                        <button
                            type="button"
                            class=move || if active { "tab-item tab-item-active" } else { "tab-item" }
                            on:click=move |_| {
                                is_creating.set(false);
                                active_profile.set(pid.clone());
                                load_profile_data(pid.clone());
                            }
                        >
                            {p.id.clone()}
                            <span class="profile-tab-badge">{badge_count}</span>
                            {if p.id != "deepseek" {
                                view! {
                                    <span
                                        class="ml-2 text-theme-muted hover:text-error cursor-pointer font-bold"
                                        on:click=move |ev| {
                                            ev.stop_propagation();
                                            active_profile.set(p.id.clone());
                                            show_delete_confirm.set(true);
                                        }
                                    >
                                        "✕"
                                    </span>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                        </button>
                    }
                }).collect_view()}
                
                <button
                    type="button"
                    class=move || if is_creating.get() { "tab-item tab-item-active" } else { "tab-item" }
                    on:click=move |_| {
                        is_creating.set(true);
                        // Reset forms to fresh new profile state
                        new_profile_id.set(String::new());
                        provider.set("custom".to_string());
                        base_url.set(String::new());
                        model.set("deepseek-v4-pro".to_string());
                        endpoints_text.set(String::new());
                        tls_sni.set(String::new());
                        save_error.set(String::new());
                    }
                >
                    {t.upstream_tab_new_profile()}
                </button>
            </div>

            <div class="upstream-split">
                <div class="space-y-6">
                    {move || if is_creating.get() {
                        // Rendering New Profile Form
                        view! {
                            <div class="glass-card space-y-4">
                                <h3 class="text-base font-semibold text-theme">
                                    {t.upstream_tab_new_profile()}
                                </h3>
                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_new_profile_id()}
                                        </label>
                                        <input
                                            type="text"
                                            class="input font-mono text-sm"
                                            placeholder="e.g. mimo"
                                            prop:value=move || new_profile_id.get()
                                            on:input=move |ev| new_profile_id.set(event_target_value(&ev))
                                        />
                                    </div>
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_provider_label()}
                                        </label>
                                        <input
                                            type="text"
                                            class="input text-sm"
                                            placeholder="e.g. mimo"
                                            prop:value=move || provider.get()
                                            on:input=move |ev| provider.set(event_target_value(&ev))
                                        />
                                    </div>
                                    <div class="md:col-span-2">
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_base_url_label()}
                                        </label>
                                        <input
                                            type="text"
                                            class="input font-mono text-sm"
                                            placeholder="https://..."
                                            prop:value=move || base_url.get()
                                            on:input=move |ev| base_url.set(event_target_value(&ev))
                                        />
                                    </div>
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_model_label()}
                                        </label>
                                        <input
                                            type="text"
                                            class="input font-mono text-sm"
                                            placeholder="deepseek-v4-pro"
                                            prop:value=move || model.get()
                                            on:input=move |ev| model.set(event_target_value(&ev))
                                        />
                                    </div>
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_tls_sni_label()}
                                        </label>
                                        <input
                                            type="text"
                                            class="input font-mono text-sm"
                                            placeholder="e.g. api.deepseek.com"
                                            prop:value=move || tls_sni.get()
                                            on:input=move |ev| tls_sni.set(event_target_value(&ev))
                                        />
                                    </div>
                                </div>

                                {move || if !save_error.get().is_empty() {
                                    view! {
                                        <div class="text-xs text-error mt-2">{save_error.get()}</div>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}

                                <div class="flex justify-end gap-2 pt-4 border-t border-theme/10">
                                    <button
                                        type="button"
                                        class="btn btn-secondary text-xs"
                                        on:click=move |_| is_creating.set(false)
                                    >
                                        "Cancel"
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-primary text-xs"
                                        on:click=on_create_profile
                                        disabled=move || saving.get()
                                    >
                                        "Create Profile"
                                    </button>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        // Rendering Edit Form
                        view! {
                            <div class="glass-card space-y-4">
                                <div class="flex flex-wrap gap-2">
                                    <button
                                        type="button"
                                        class="btn btn-secondary text-xs"
                                        on:click=move |_| {
                                            base_url.set(OFFICIAL_BASE.into());
                                            model.set(DEFAULT_MODEL.into());
                                            provider.set("deepseek".into());
                                        }
                                    >
                                        {t.upstream_preset_official()}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary text-xs"
                                        on:click=move |_| {
                                            base_url.set(MIMO_BASE.into());
                                            model.set(MIMO_MODEL.into());
                                            provider.set("mimo".into());
                                        }
                                    >
                                        {t.upstream_preset_mimo()}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary text-xs"
                                        on:click=move |_| {
                                            base_url.set(String::new());
                                        }
                                    >
                                        {t.upstream_preset_custom()}
                                    </button>
                                </div>

                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_provider_label()}
                                        </label>
                                        <input
                                            type="text"
                                            prop:value=move || provider.get()
                                            on:input=move |ev| provider.set(event_target_value(&ev))
                                            class="input text-sm"
                                            placeholder="deepseek"
                                        />
                                    </div>
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_base_url_label()}
                                        </label>
                                        <input
                                            type="text"
                                            prop:value=move || base_url.get()
                                            on:input=move |ev| base_url.set(event_target_value(&ev))
                                            class="input font-mono text-sm"
                                            placeholder=OFFICIAL_BASE
                                        />
                                    </div>
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {t.upstream_model_label()}
                                        </label>
                                        <input
                                            type="text"
                                            prop:value=move || model.get()
                                            on:input=move |ev| model.set(event_target_value(&ev))
                                            class="input font-mono text-sm"
                                            placeholder=DEFAULT_MODEL
                                        />
                                    </div>
                                    {move || (active_profile.get() != "deepseek").then(|| view! {
                                        <div>
                                            <label class="block text-xs font-semibold text-theme-muted mb-1">
                                                {t.upstream_tls_sni_label()}
                                            </label>
                                            <input
                                                type="text"
                                                prop:value=move || tls_sni.get()
                                                on:input=move |ev| tls_sni.set(event_target_value(&ev))
                                                class="input font-mono text-sm"
                                                placeholder="e.g. api.deepseek.com"
                                            />
                                        </div>
                                    })}
                                </div>

                                <div class="space-y-2">
                                    <button
                                        type="button"
                                        class="text-xs text-accent"
                                        on:click=move |_| show_advanced.update(|v| *v = !*v)
                                    >
                                        {move || if show_advanced.get() {
                                            t.upstream_hide_advanced()
                                        } else {
                                            t.upstream_show_advanced()
                                        }}
                                    </button>
                                    {move || show_advanced.get().then(|| view! {
                                        <div class="mt-2">
                                            <label class="block text-xs font-semibold text-theme-muted mb-1">
                                                {t.upstream_endpoints_label()}
                                            </label>
                                            <textarea
                                                prop:value=move || endpoints_text.get()
                                                on:input=move |ev| endpoints_text.set(event_target_value(&ev))
                                                class="input font-mono text-sm h-24 resize-y"
                                                placeholder="api.deepseek.com:443"
                                            ></textarea>
                                            <p class="text-xs text-theme-muted mt-1">
                                                {t.upstream_endpoints_hint()}
                                            </p>
                                        </div>
                                    })}
                                </div>

                                <div class="flex items-center gap-3 pt-4 border-t border-theme/10">
                                    <button
                                        type="button"
                                        on:click=on_save
                                        disabled=move || saving.get()
                                        class="btn btn-primary text-xs"
                                    >
                                        {move || if saving.get() { t.upstream_saving() } else { t.upstream_save_btn() }}
                                    </button>
                                    {move || if saved.get() {
                                        view! {
                                            <span class="text-xs text-accent font-medium">
                                                {t.upstream_saved()}
                                            </span>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }}
                                    {move || if !save_error.get().is_empty() {
                                        view! {
                                            <span class="text-xs text-error">{save_error.get()}</span>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }}
                                </div>
                            </div>
                        }.into_any()
                    }}
                    
                    {move || if is_creating.get() {
                        view! { <span></span> }.into_any()
                    } else {
                        view! {
                            <div class="glass-card space-y-4">
                                <div>
                                    <h3 class="text-base font-semibold text-theme">
                                        {move || t.upstream_pool_title_for(&active_profile.get())}
                                    </h3>
                                    <p class="text-xs text-theme-muted mt-1">{move || t.upstream_pool_desc()}</p>
                                    {move || (active_profile.get() == "deepseek").then(|| view! {
                                        <p class="text-xs text-theme-muted mt-1">
                                            {t.upstream_pool_deepseek_hint()}
                                        </p>
                                    })}
                                    {move || (active_profile.get() != "deepseek").then(|| view! {
                                        <p class="text-xs text-theme-muted mt-1">
                                            {t.upstream_pool_patch_deepseek_only()}
                                        </p>
                                    })}
                                </div>

                                {move || match key_pool.get() {
                                    None => view! { <Spinner /> }.into_any(),
                                    Some(Err(e)) => view! {
                                        <p class="text-xs text-error">{e}</p>
                                    }.into_any(),
                                    Some(Ok(pool)) => {
                                        let keys_for_all = pool.keys.clone();
                                        let pid_for_all = active_profile.get();
                                        view! {
                                            <div>
                                                // "Test All" button
                                                <div class="flex items-center gap-2 mb-3">
                                                    <button
                                                        type="button"
                                                        class="btn btn-secondary text-xs"
                                                        disabled=move || testing_all.get()
                                                        on:click={
                                                            let keys = keys_for_all.clone();
                                                            let pid = pid_for_all.clone();
                                                            move |_| {
                                                                let keys = keys.clone();
                                                                let pid = pid.clone();
                                                                testing_all.set(true);
                                                                leptos::task::spawn_local(async move {
                                                                    for k in &keys {
                                                                        key_testing.update(|m| { m.insert(k.id.clone(), true); });
                                                                        let result = api::test_upstream_profile_key(&pid, &k.id).await;
                                                                        key_testing.update(|m| { m.insert(k.id.clone(), false); });
                                                                        let tr = match result {
                                                                            Ok(r) => r,
                                                                            Err(e) => UpstreamTestResult {
                                                                                ok: false,
                                                                                status_code: 0,
                                                                                latency_ms: 0,
                                                                                model_count: None,
                                                                                error: Some(e),
                                                                                quota: None,
                                                                            },
                                                                        };
                                                                        key_test_results.update(|m| { m.insert(k.id.clone(), tr); });
                                                                    }
                                                                    testing_all.set(false);
                                                                });
                                                            }
                                                        }
                                                    >
                                                        {move || if testing_all.get() { "Testing..." } else { "Test All Quotas" }}
                                                    </button>
                                                    <button
                                                        type="button"
                                                        class="text-xs text-accent cursor-pointer"
                                                        on:click=move |_| {
                                                            key_test_results.set(HashMap::new());
                                                        }
                                                    >
                                                        "Clear Results"
                                                    </button>
                                                </div>

                                                <div class="overflow-x-auto">
                                                    <table class="table text-sm">
                                                        <thead>
                                                            <tr>
                                                                <th>{t.upstream_pool_col_id()}</th>
                                                                <th>{t.upstream_pool_col_preview()}</th>
                                                                <th>{t.upstream_pool_col_account()}</th>
                                                                <th>{t.upstream_pool_col_enabled()}</th>
                                                                <th>"Quota"</th>
                                                                <th>{t.upstream_pool_col_inflight()}</th>
                                                                <th>{t.upstream_pool_col_cooldown()}</th>
                                                                <th>"Test"</th>
                                                            </tr>
                                                        </thead>
                                                        <tbody>
                                                            {pool.keys.iter().map(|k| {
                                                                let kid = k.id.clone();
                                                                let kid2 = k.id.clone();
                                                                let kid3 = k.id.clone();
                                                                let kid_for_test = k.id.clone();
                                                                let enabled = k.enabled;
                                                                let pid = active_profile.get();
                                                                let pid2 = active_profile.get();
                                                                view! {
                                                                    <tr>
                                                                        <td class="font-mono">{k.id.clone()}</td>
                                                                        <td class="font-mono">{k.preview.clone()}</td>
                                                                        <td class="font-mono text-xs">
                                                                            {if k.account_id.is_empty() {
                                                                                "default".to_string()
                                                                            } else {
                                                                                k.account_id.clone()
                                                                            }}
                                                                        </td>
                                                                        <td>
                                                                            <div class="flex items-center gap-2">
                                                                                <input
                                                                                    type="checkbox"
                                                                                    prop:checked=enabled
                                                                                    on:change=move |_| {
                                                                                        let id = kid.clone();
                                                                                        let next = !enabled;
                                                                                        let pid = pid.clone();
                                                                                        leptos::task::spawn_local(async move {
                                                                                            let req = PatchUpstreamKeyRequest {
                                                                                                enabled: Some(next),
                                                                                                secret: None,
                                                                                            };
                                                                                            let _ = if pid == "deepseek" {
                                                                                                api::patch_upstream_key(&id, &req).await
                                                                                            } else {
                                                                                                api::patch_upstream_profile_key(
                                                                                                    &pid, &id, &req,
                                                                                                )
                                                                                                .await
                                                                                            };
                                                                                            load_key_pool(pid.clone());
                                                                                        });
                                                                                    }
                                                                                />
                                                                                <span class=move || if enabled { "badge badge-success text-xs" } else { "badge text-xs" }>
                                                                                    {if enabled { t.upstream_key_status_enabled() } else { t.upstream_key_status_disabled() }}
                                                                                </span>
                                                                            </div>
                                                                        </td>
                                                                        // Quota column
                                                                        <td>
                                                                            {move || {
                                                                                let results = key_test_results.get();
                                                                                match results.get(&kid2) {
                                                                                    Some(result) => match &result.quota {
                                                                                        Some(quota) => view! {
                                                                                            <QuotaProgressBar quota=quota.clone() />
                                                                                        }.into_any(),
                                                                                        None => {
                                                                                            if result.ok {
                                                                                                view! {
                                                                                                    <span class="text-xs text-accent">"OK"</span>
                                                                                                }.into_any()
                                                                                            } else {
                                                                                                let err_msg = result.error.clone().unwrap_or_else(|| "Unknown error".to_string());
                                                                                                view! {
                                                                                                    <span class="text-xs text-error" title={err_msg}>"Fail"</span>
                                                                                                }.into_any()
                                                                                            }
                                                                                        }
                                                                                    },
                                                                                    None => view! {
                                                                                        <span class="text-xs text-theme-muted">"-"</span>
                                                                                    }.into_any(),
                                                                                }
                                                                            }}
                                                                        </td>
                                                                        <td class="font-mono">{k.inflight}</td>
                                                                        <td class="font-mono text-xs">
                                                                            {if k.cooldown_remaining_secs > 0 {
                                                                                view! {
                                                                                    <span class="text-warning">
                                                                                        {format!("{}s", k.cooldown_remaining_secs)}
                                                                                    </span>
                                                                                }.into_any()
                                                                            } else {
                                                                                view! { <span class="text-theme-muted">"-"</span> }.into_any()
                                                                            }}
                                                                        </td>
                                                                        // Test button column
                                                                        <td>
                                                                            {move || {
                                                                                let is_testing = key_testing.get().get(&kid3).copied().unwrap_or(false);
                                                                                view! {
                                                                                    <button
                                                                                        type="button"
                                                                                        class="btn btn-secondary text-xs"
                                                                                        disabled=is_testing
                                                                                        on:click={
                                                                                            let kid = kid_for_test.clone();
                                                                                            let pid = pid2.clone();
                                                                                            move |_| {
                                                                                                let kid = kid.clone();
                                                                                                let pid = pid.clone();
                                                                                            leptos::task::spawn_local(async move {
                                                                                                key_testing.update(|m| { m.insert(kid.clone(), true); });
                                                                                                let result = api::test_upstream_profile_key(&pid, &kid).await;
                                                                                                key_testing.update(|m| { m.insert(kid.clone(), false); });
                                                                                                let tr = match result {
                                                                                                    Ok(r) => r,
                                                                                                    Err(e) => UpstreamTestResult {
                                                                                                        ok: false,
                                                                                                        status_code: 0,
                                                                                                        latency_ms: 0,
                                                                                                        model_count: None,
                                                                                                        error: Some(e),
                                                                                                        quota: None,
                                                                                                    },
                                                                                                };
                                                                                                key_test_results.update(|m| { m.insert(kid, tr); });
                                                                                            });
                                                                                            }
                                                                                        }
                                                                                    >
                                                                                        {if is_testing { "..." } else { "Test" }}
                                                                                    </button>
                                                                                }
                                                                            }}
                                                                        </td>
                                                                    </tr>
                                                                }
                                                            }).collect_view()}
                                                        </tbody>
                                                    </table>
                                                </div>
                                            </div>
                                        }.into_any()
                                    },
                                }}

                                <div class="space-y-4 pt-4 border-t border-theme/10">
                                    <div>
                                        <label class="block text-xs font-semibold text-theme-muted mb-1">
                                            {move || if pool_replace_mode.get() {
                                                t.upstream_pool_replace_label()
                                            } else {
                                                t.upstream_pool_append_label()
                                            }}
                                        </label>
                                        <label class="flex items-center gap-2 text-xs text-theme-muted mb-2">
                                            <input
                                                type="checkbox"
                                                prop:checked=move || pool_replace_mode.get()
                                                on:change=move |ev| pool_replace_mode.set(event_target_checked(&ev))
                                            />
                                            {t.upstream_pool_replace_confirm()}
                                        </label>
                                        <textarea
                                            prop:value=move || pool_secrets_text.get()
                                            on:input=move |ev| pool_secrets_text.set(event_target_value(&ev))
                                            class="input font-mono text-sm h-24 resize-y"
                                            placeholder="sk-...\nacct-b:sk-...\n"
                                        ></textarea>
                                    </div>

                                    <div class="flex items-center gap-3">
                                        <button
                                            on:click=on_save_pool
                                            disabled=move || pool_saving.get()
                                            class="btn btn-primary text-xs"
                                        >
                                            {move || if pool_saving.get() {
                                                t.upstream_pool_saving()
                                            } else {
                                                t.upstream_pool_save_btn()
                                            }}
                                        </button>
                                        {move || if pool_saved.get() {
                                            view! {
                                                <span class="text-xs text-accent font-medium">
                                                    {t.upstream_pool_saved()}
                                                </span>
                                            }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                        {move || if !pool_error.get().is_empty() {
                                            view! { <span class="text-xs text-error">{pool_error.get()}</span> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    }}
                </div>

                <div class="space-y-6">
                    <div class="glass-card space-y-4">
                        <h3 class="text-sm font-semibold text-theme">
                            "Connection Test"
                        </h3>
                        
                        <div class="test-result-panel">
                            {move || if let Some(res) = test_result.get() {
                                view! {
                                    <div class="test-result-row">
                                        <span class="test-result-label">{t.upstream_test_status_code()}</span>
                                        <span class=move || if res.ok { "test-result-value test-result-success font-semibold" } else { "test-result-value test-result-error font-semibold" }>
                                            {if res.ok { "✓ Success" } else { "✗ Failed" }}
                                        </span>
                                    </div>
                                    <div class="test-result-row">
                                        <span class="test-result-label">{t.upstream_test_latency()}</span>
                                        <span class="test-result-value test-result-value-mono">{format!("{} ms", res.latency_ms)}</span>
                                    </div>
                                    <div class="test-result-row">
                                        <span class="test-result-label">{t.upstream_test_models_found()}</span>
                                        <span class="test-result-value test-result-value-mono">{res.model_count.unwrap_or(0)}</span>
                                    </div>
                                    {res.error.map(|err| view! {
                                        <div class="text-xs text-error mt-2 p-2 bg-error/5 rounded border border-error/10">
                                            {err}
                                        </div>
                                    })}
                                }.into_any()
                            } else if !test_error.get().is_empty() {
                                view! {
                                    <div class="text-xs text-error p-2 bg-error/5 rounded border border-error/10 font-mono">
                                        {test_error.get()}
                                    </div>
                                }.into_any()
                            } else {
                                view! {
                                    <p class="text-xs text-theme-muted text-center py-4">
                                        "No connection test performed yet."
                                    </p>
                                }.into_any()
                            }}
                        </div>

                        <button
                            type="button"
                            on:click=on_test
                            disabled=move || testing.get()
                            class="btn btn-secondary w-full text-xs"
                        >
                            {move || if testing.get() { t.upstream_testing() } else { t.upstream_test_btn() }}
                        </button>
                    </div>
                </div>
            </div>

            {move || sync_result.get().map(|r| view! { <SyncResultCard result=r /> })}

            {move || show_delete_confirm.get().then(|| view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 animate-fade-in">
                    <div class="glass-card max-w-sm w-full space-y-4">
                        <h4 class="text-sm font-semibold text-theme">{t.upstream_delete_profile_btn()}</h4>
                        <p class="text-xs text-theme-muted">{t.upstream_profile_delete_confirm()}</p>
                        <div class="flex gap-2 justify-end">
                            <button
                                class="btn btn-secondary text-xs"
                                on:click=move |_| show_delete_confirm.set(false)
                            >
                                "Cancel"
                            </button>
                            <button
                                class="btn btn-primary text-xs"
                                on:click=on_delete_profile
                                disabled=move || deleting.get()
                            >
                                {move || if deleting.get() { "Deleting..." } else { "Delete" }}
                            </button>
                        </div>
                    </div>
                </div>
            })}
        </div>
    }
}

/// Displays a quota progress bar for an upstream key.
#[component]
fn QuotaProgressBar(quota: KeyQuotaInfo) -> impl IntoView {
    let percentage = match (quota.balance, quota.total_granted) {
        (Some(b), Some(g)) if g > 0.0 => Some((b / g * 100.0).clamp(0.0, 100.0)),
        _ => None,
    };

    let bar_color = match percentage {
        Some(p) if p > 50.0 => "bg-success",
        Some(p) if p > 20.0 => "bg-warning",
        Some(_) => "bg-error",
        None => "bg-theme-muted",
    };

    let available_icon = match quota.is_available {
        Some(true) => view! { <span class="text-success text-xs mr-1">{"OK"}</span> }.into_any(),
        Some(false) => view! { <span class="text-error text-xs mr-1">{"X"}</span> }.into_any(),
        None => view! { <span></span> }.into_any(),
    };

    view! {
        <div class="min-w-[120px]">
            <div class="flex items-center gap-1 mb-0.5">
                {available_icon}
                {match (quota.balance, quota.total_granted) {
                    (Some(b), Some(g)) => view! {
                        <span class="text-xs font-mono">
                            {format!("${:.2} / ${:.2}", b, g)}
                        </span>
                    }.into_any(),
                    (Some(b), _) => view! {
                        <span class="text-xs font-mono">
                            {format!("${:.2}", b)}
                        </span>
                    }.into_any(),
                    _ => view! {
                        <span class="text-xs text-theme-muted">"N/A"</span>
                    }.into_any(),
                }}
            </div>
            {percentage.map(|p| view! {
                <div class="w-full h-1.5 rounded-full bg-theme/10 overflow-hidden">
                    <div
                        class={format!("h-full rounded-full transition-all {}", bar_color)}
                        style={format!("width: {:.1}%", p)}
                    ></div>
                </div>
            })}
        </div>
    }
}
