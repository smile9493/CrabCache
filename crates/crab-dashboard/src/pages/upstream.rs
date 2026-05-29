use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;

use crate::api;
use crate::components::codex_oauth_panel::CodexOAuthPanel;
use crate::components::routing_tab::RoutingTab;
use crate::components::skeleton::SkeletonUpstreamProfileCard;
use crate::components::sync_result::SyncResultCard;
use crate::components::ui::*;

/// Incremented by [`CodexOAuthPanel`] when a credential is imported,
/// so the upstream page effect can re-load the key pool.
pub static KEY_POOL_REFRESH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Signal the upstream page to refresh the key pool for the current profile.
pub fn signal_refresh_key_pool() {
    KEY_POOL_REFRESH_COUNTER.fetch_add(1, Ordering::Relaxed);
}
use crate::locale::use_translations;
use crate::types::{
    KeyQuotaInfo, PatchUpstreamKeyRequest, PutUpstreamKeysRequest, PutUpstreamProfileAdminRequest,
    SyncResult, UpdateUpstreamConfigRequest, UpstreamKeyInput, UpstreamKeysPutMode,
    UpstreamKeysView, UpstreamProfileAdminView, UpstreamTestBody, UpstreamTestResult,
};

struct PresetTemplate {
    id: &'static str,
    label_zh: &'static str,
    label_en: &'static str,
    provider: &'static str,
    base_url: &'static str,
    models: &'static [&'static str],
    default_model: &'static str,
    tls_sni: &'static str,
}

const PRESETS: &[PresetTemplate] = &[
    PresetTemplate {
        id: "deepseek",
        label_zh: "DeepSeek 官方",
        label_en: "DeepSeek Official",
        provider: "deepseek",
        base_url: "https://api.deepseek.com",
        models: &[
            "deepseek-v4-pro",
            "deepseek-v4-flash",
            "deepseek-v4-flash-max",
            "deepseek-chat",
        ],
        default_model: "deepseek-v4-pro",
        tls_sni: "api.deepseek.com",
    },
    PresetTemplate {
        id: "mimo",
        label_zh: "MiMo",
        label_en: "MiMo",
        provider: "mimo",
        base_url: "https://api.xiaomimimo.com",
        models: &["xiaomi/mimo-v2.5-pro", "xiaomi/mimo-v2-flash"],
        default_model: "xiaomi/mimo-v2.5-pro",
        tls_sni: "api.xiaomimimo.com",
    },
    PresetTemplate {
        id: "mimo-tp-cn",
        label_zh: "MiMo TP CN",
        label_en: "MiMo TP CN",
        provider: "mimo",
        base_url: "https://token-plan-cn.xiaomimimo.com",
        models: &["xiaomi/mimo-v2.5-pro", "xiaomi/mimo-v2-flash"],
        default_model: "xiaomi/mimo-v2.5-pro",
        tls_sni: "token-plan-cn.xiaomimimo.com",
    },
    PresetTemplate {
        id: "mimo-tp-sgp",
        label_zh: "MiMo TP SGP",
        label_en: "MiMo TP SGP",
        provider: "mimo",
        base_url: "https://token-plan-sgp.xiaomimimo.com",
        models: &["xiaomi/mimo-v2.5-pro", "xiaomi/mimo-v2-flash"],
        default_model: "xiaomi/mimo-v2.5-pro",
        tls_sni: "token-plan-sgp.xiaomimimo.com",
    },
    PresetTemplate {
        id: "custom",
        label_zh: "自定义",
        label_en: "Custom",
        provider: "custom",
        base_url: "",
        models: &[],
        default_model: "",
        tls_sni: "",
    },
    PresetTemplate {
        id: "codex",
        label_zh: "OpenAI Codex",
        label_en: "OpenAI Codex",
        provider: "codex",
        base_url: "https://api.openai.com",
        models: &["gpt-4o", "gpt-4o-mini", "o3-mini"],
        default_model: "gpt-4o",
        tls_sni: "api.openai.com",
    },
];

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
        .map(|line| {
            let (account_id, secret) = parse_upstream_pool_line(&line);
            UpstreamKeyInput {
                id: String::new(),
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

fn preset_for_id(id: &str) -> Option<&'static PresetTemplate> {
    PRESETS.iter().find(|p| p.id == id)
}

fn models_for_provider(provider: &str) -> &'static [&'static str] {
    PRESETS
        .iter()
        .find(|p| p.provider == provider && !p.models.is_empty())
        .map(|p| p.models)
        .unwrap_or(&[])
}

const CUSTOM_MODEL_SENTINEL: &str = "__custom_model__";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreationStep {
    PickTemplate,
    FillForm,
}

#[component]
pub fn UpstreamPage() -> impl IntoView {
    let t = use_translations();

    // Page state
    let profiles: RwSignal<Vec<UpstreamProfileAdminView>> = RwSignal::new(Vec::new());
    let active_profile = RwSignal::new("deepseek".to_string());
    let gateway_reachable = RwSignal::new(true);

    // Drawer-based navigation: Some(profile_id) opens drawer, None = list view
    let drawer_profile: RwSignal<Option<String>> = RwSignal::new(None);
    let drawer_creating: RwSignal<bool> = RwSignal::new(false);
    // Internal drawer tabs: 0 = Config, 1 = Keys, 2 = Routing
    let drawer_tab: RwSignal<usize> = RwSignal::new(0);

    // Form inputs
    let provider = RwSignal::new("deepseek".to_string());
    let base_url = RwSignal::new("https://api.deepseek.com".to_string());
    let model = RwSignal::new("deepseek-v4-pro".to_string());
    let endpoints_text = RwSignal::new(String::new());
    let tls_sni = RwSignal::new(String::new());
    let proxy_url = RwSignal::new(String::new());
    let show_advanced = RwSignal::new(false);

    // Inline creation state
    let new_profile_id = RwSignal::new(String::new());
    let creation_step = RwSignal::new(CreationStep::PickTemplate);

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

    let default_profile_id = RwSignal::new(String::new());
    let profiles_loaded = RwSignal::new(false);

    // API loaders
    let load_key_pool = move |pid: String| {
        key_pool.set(None);
        key_test_results.set(HashMap::new());
        key_testing.set(HashMap::new());
        let default_id = default_profile_id.get_untracked();
        leptos::task::spawn_local(async move {
            let result = if pid == default_id {
                api::fetch_upstream_keys().await
            } else {
                api::fetch_upstream_profile_keys(&pid)
                    .await
                    .map(|v| UpstreamKeysView { keys: v.keys })
            };
            match result {
                Ok(v) => { key_pool.try_set(Some(Ok(v))); },
                Err(e) => { key_pool.try_set(Some(Err(e))); },
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
        let default_id = default_profile_id.get_untracked();
        if pid == default_id {
            leptos::task::spawn_local(async move {
                match api::fetch_upstream_config().await {
                    Ok(c) => {
                        base_url.try_set(c.base_url.clone());
                        model.try_set(c.model.clone());
                        endpoints_text.try_set(c.endpoints.join("\n"));
                        tls_sni.try_set(String::new());
                        proxy_url.try_set(String::new());
                        provider.try_set("deepseek".to_string());
                        gateway_reachable.try_set(c.gateway_reachable);
                        if let Some(ref lt) = c.last_test {
                            test_result.try_set(Some(lt.clone()));
                        }
                    }
                    Err(e) => {
                        save_error.try_set(e);
                    }
                }
            });
        } else if let Some(p) = p_list.iter().find(|p| p.id == pid) {
            provider.set(p.provider.clone());
            base_url.set(p.base_url.clone());
            model.set(p.fallback_model.clone());
            endpoints_text.set(p.endpoints.join("\n"));
            tls_sni.set(p.tls_sni.clone());
            proxy_url.set(p.proxy_url.clone().unwrap_or_default());
        }
        load_key_pool(pid);
    };

    let load_profiles_and_select = move |pid: Option<String>| {
        leptos::task::spawn_local(async move {
            if let Ok(resp) = api::fetch_upstream_profiles().await {
                let def = resp.default_profile_id.clone();
                default_profile_id.try_set(def.clone());
                profiles.try_set(resp.profiles);
                let select = pid.unwrap_or(def);
                active_profile.try_set(select.clone());
                load_profile_data(select);
            }
            profiles_loaded.try_set(true);
        });
    };

    // Initial load: gateway default profile (not hardcoded id)
    load_profiles_and_select(None);

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
                    Ok(r) => { test_result.try_set(Some(r)); },
                    Err(e) => { test_error.try_set(e); },
                }
            } else if pid == default_profile_id.try_get_untracked().unwrap_or_default()
                || profiles.try_get().unwrap_or_default().iter().any(|p| p.id == pid)
            {
                // Otherwise test saved profile keys
                match api::test_upstream_profile(&pid).await {
                    Ok(r) => { test_result.try_set(Some(r)); },
                    Err(e) => { test_error.try_set(e); },
                }
            } else {
                test_error.try_set(
                    "Please enter a key in bulk input to test this unsaved profile.".to_string(),
                );
            }
            testing.try_set(false);
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
        let sni = if sni_val.is_empty() {
            None
        } else {
            Some(sni_val)
        };
        let proxy = proxy_url.get().trim().to_string();
        let proxy_opt = if proxy.is_empty() {
            None
        } else {
            Some(proxy)
        };
        let keys_to_append_clone = keys_to_append.clone();

        leptos::task::spawn_local(async move {
            let result = if pid == default_profile_id.try_get_untracked().unwrap_or_default() {
                let req = UpdateUpstreamConfigRequest {
                    base_url: url.clone(),
                    model: model_val.clone(),
                    api_key: None,
                    endpoints: endpoints.clone(),
                    keys_to_append: keys_to_append_clone.clone(),
                };
                api::update_upstream_config(&req).await.map(|resp| {
                    if let Some(s) = resp.sync {
                        sync_result.try_set(Some(s));
                    }
                })
            } else {
                let req = PutUpstreamProfileAdminRequest {
                    provider: prov,
                    base_url: url.clone(),
                    fallback_model: model_val.clone(),
                    endpoints: endpoints.clone(),
                    tls_sni: sni,
                    proxy_url: proxy_opt,
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
                        let key_err = if pid == default_profile_id.try_get_untracked().unwrap_or_default() {
                            api::put_upstream_keys(&key_req).await.err()
                        } else {
                            api::put_upstream_profile_keys(&pid, &key_req).await.err()
                        };
                        if let Some(e) = key_err {
                            save_error.try_set(format!("Profile saved but keys failed: {e}"));
                        } else {
                            pool_secrets_text.try_set(String::new());
                        }
                    } else {
                        pool_secrets_text.try_set(String::new());
                    }
                    saved.try_set(true);
                    load_profiles_and_select(Some(pid));
                }
                Err(e) => { save_error.try_set(e); },
            }
            saving.try_set(false);
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
            pool_error.set(
                use_translations()
                    .upstream_pool_empty_keys_error()
                    .to_string(),
            );
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
            let result = if pid == default_profile_id.try_get_untracked().unwrap_or_default() {
                api::put_upstream_keys(&req).await
            } else {
                api::put_upstream_profile_keys(&pid, &req)
                    .await
                    .map(|v| UpstreamKeysView { keys: v.keys })
            };
            match result {
                Ok(v) => {
                    key_pool.try_set(Some(Ok(v)));
                    pool_secrets_text.try_set(String::new());
                    pool_saved.try_set(true);
                    // Refresh profiles list (since key count changed)
                    if let Ok(resp) = api::fetch_upstream_profiles().await {
                        default_profile_id.try_set(resp.default_profile_id.clone());
                        profiles.try_set(resp.profiles);
                    }
                }
                Err(e) => { pool_error.try_set(e); },
            }
            pool_saving.try_set(false);
        });
    };

    let on_delete_profile = move |_| {
        let pid = active_profile.get();
        if pid == default_profile_id.get_untracked() {
            return;
        }
        deleting.set(true);
        leptos::task::spawn_local(async move {
            match api::delete_upstream_profile(&pid).await {
                Ok(_) => {
                    show_delete_confirm.try_set(false);
                    drawer_profile.try_set(None);
                    drawer_creating.try_set(false);
                    load_profiles_and_select(None);
                }
                Err(e) => {
                    save_error.try_set(e);
                }
            }
            deleting.try_set(false);
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
        let sni = if sni_val.is_empty() {
            None
        } else {
            Some(sni_val)
        };
        let proxy = proxy_url.get().trim().to_string();
        let proxy_opt = if proxy.is_empty() {
            None
        } else {
            Some(proxy)
        };

        saving.set(true);
        leptos::task::spawn_local(async move {
            let req = PutUpstreamProfileAdminRequest {
                provider: prov,
                base_url: url,
                fallback_model: model_val,
                endpoints: Vec::new(),
                tls_sni: sni,
                proxy_url: proxy_opt,
            };
            match api::put_upstream_profile(&id, &req).await {
                Ok(_) => {
                    drawer_creating.try_set(false);
                    new_profile_id.try_set(String::new());
                    active_profile.try_set(id.clone());
                    drawer_profile.try_set(Some(id.clone()));
                    load_profiles_and_select(Some(id));
                }
                Err(e) => {
                    save_error.try_set(e);
                }
            }
            saving.try_set(false);
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

            // Profile card grid
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                {move || {
                    if !profiles_loaded.get() {
                        (0..3)
                            .map(|_| view! { <SkeletonUpstreamProfileCard /> })
                            .collect_view()
                            .into_any()
                    } else {
                        let list = profiles.get();
                        let mut views: Vec<leptos::prelude::AnyView> = Vec::new();
                        if list.is_empty() {
                            views.push(view! {
                                <div class="upstream-profiles-empty empty-state py-10">
                                    <div class="empty-state-icon">"◇"</div>
                                    <p class="empty-state-title">{t.upstream_profiles_empty_title()}</p>
                                    <p class="text-xs text-theme-muted mt-1 max-w-sm mx-auto">
                                        {t.upstream_profiles_empty_desc()}
                                    </p>
                                </div>
                            }.into_any());
                        } else {
                            let def_id = default_profile_id.get();
                            for p in list {
                                let pid = p.id.clone();
                                let pid3 = p.id.clone();
                                let badge_count = p.key_pool_count;
                                let keys_available = p.keys_available;
                                let is_default = p.id == def_id;
                                let card_class = if is_default {
                                    "upstream-card upstream-card-default"
                                } else {
                                    "upstream-card"
                                };
                                let status_dot = if p.key_pool_count == 0 {
                                    None
                                } else if p.keys_available == p.key_pool_count {
                                    Some("upstream-card-status-ok")
                                } else {
                                    Some("upstream-card-status-warn")
                                };
                                views.push(view! {
                                    <div
                                        class=card_class
                                        on:click=move |_| {
                                            drawer_creating.set(false);
                                            drawer_tab.set(0);
                                            active_profile.set(pid.clone());
                                            drawer_profile.set(Some(pid.clone()));
                                            load_profile_data(pid.clone());
                                        }
                                    >
                                        <div class="flex items-start justify-between mb-3">
                                            <div class="flex items-center gap-2 min-w-0">
                                                <span class="upstream-card-icon">
                                                    {p.id.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()}
                                                </span>
                                                <div class="min-w-0">
                                                    <div class="flex items-center gap-2">
                                                        <div class="upstream-card-title truncate">{p.id.clone()}</div>
                                                        {status_dot.map(|cls| view! {
                                                            <span class=format!("upstream-card-status-dot {cls}") title="Key pool status"></span>
                                                        })}
                                                    </div>
                                                    <div class="upstream-card-subtitle font-mono text-xs truncate max-w-[180px]">
                                                        {p.base_url.clone()}
                                                    </div>
                                                    {(!is_default).then(|| view! {
                                                        <div class="text-xs text-theme-muted mt-0.5 truncate">{p.provider.clone()}</div>
                                                    })}
                                                </div>
                                            </div>
                                            {if is_default {
                                                view! { <span class="badge badge-accent text-xs shrink-0">{t.upstream_default_badge()}</span> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                        </div>
                                        <div class="flex items-center gap-3 text-xs text-theme-muted">
                                            <span class="flex items-center gap-1">
                                                <span class="upstream-card-stat">{keys_available}</span>
                                                <span>"/"</span>
                                                <span class="upstream-card-stat">{badge_count}</span>
                                                " keys"
                                            </span>
                                            <span class="flex items-center gap-1">
                                                <span class="upstream-card-stat">{p.endpoints.len()}</span>
                                                " endpoints"
                                            </span>
                                        </div>
                                        {if !is_default {
                                            view! {
                                                <button
                                                    type="button"
                                                    class="upstream-card-delete"
                                                    on:click=move |ev| {
                                                        ev.stop_propagation();
                                                        active_profile.set(pid3.clone());
                                                        show_delete_confirm.set(true);
                                                    }
                                                    title="Delete profile"
                                                >
                                                    "✕"
                                                </button>
                                            }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                    </div>
                                }.into_any());
                            }
                        }
                        views.into_any()
                    }
                }}

                {move || profiles_loaded.get().then(|| view! {
                    <div
                        class="upstream-card upstream-card-new"
                        on:click=move |_| {
                            drawer_creating.set(true);
                            drawer_profile.set(None);
                            creation_step.set(CreationStep::PickTemplate);
                            new_profile_id.set(String::new());
                            provider.set("custom".to_string());
                            base_url.set(String::new());
                            model.set(String::new());
                            endpoints_text.set(String::new());
                            tls_sni.set(String::new());
                            proxy_url.set(String::new());
                            save_error.set(String::new());
                        }
                    >
                        <div class="flex flex-col items-center justify-center h-full gap-2 text-theme-muted">
                            <span class="text-3xl leading-none">"+"</span>
                            <span class="text-sm font-medium">{t.upstream_tab_new_profile()}</span>
                        </div>
                    </div>
                })}
            </div>

            // Right-side drawer overlay
            {move || {
                let show = drawer_profile.get().is_some() || drawer_creating.get();
                show.then(|| view! {
                    <div class="upstream-drawer-backdrop" on:click=move |_| {
                        drawer_profile.set(None);
                        drawer_creating.set(false);
                    }>
                        <div class="upstream-drawer" on:click=|ev| ev.stop_propagation()>
                            <div class="upstream-drawer-header">
                                <h3 class="text-base font-semibold text-theme">
                                    {move || if drawer_creating.get() {
                                        t.upstream_tab_new_profile().to_string()
                                    } else {
                                        drawer_profile.get().unwrap_or_default()
                                    }}
                                </h3>
                                <button
                                    type="button"
                                    class="text-theme-muted hover:text-theme text-lg"
                                    on:click=move |_| {
                                        drawer_profile.set(None);
                                        drawer_creating.set(false);
                                    }
                                >
                                    "✕"
                                </button>
                            </div>

                            // Internal drawer tabs (only when editing, not creating)
                            {move || (!drawer_creating.get()).then(|| view! {
                                <div class="upstream-drawer-tabs">
                                    <button
                                        type="button"
                                        class=move || if drawer_tab.get() == 0 { "tab-item tab-item-active" } else { "tab-item" }
                                        on:click=move |_| drawer_tab.set(0)
                                    >
                                        {t.upstream_subtab_profiles()}
                                    </button>
                                    <button
                                        type="button"
                                        class=move || if drawer_tab.get() == 1 { "tab-item tab-item-active" } else { "tab-item" }
                                        on:click=move |_| drawer_tab.set(1)
                                    >
                                        {t.upstream_subtab_keys()}
                                    </button>
                                    <button
                                        type="button"
                                        class=move || if drawer_tab.get() == 2 { "tab-item tab-item-active" } else { "tab-item" }
                                        on:click=move |_| drawer_tab.set(2)
                                    >
                                        {t.upstream_subtab_routing()}
                                    </button>
                                </div>
                            })}

                            <div class="upstream-drawer-body">
                                {move || if drawer_creating.get() {
                                    // ---- Creation flow ----
                                    if creation_step.get() == CreationStep::PickTemplate {
                                        view! {
                                            <div class="space-y-4">
                                                <h4 class="text-sm font-semibold text-theme">
                                                    {t.upstream_pick_template()}
                                                </h4>
                                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                                    {PRESETS.iter().map(|preset| {
                                                        let pid = preset.id;
                                                        let label = match t.locale {
                                                            crate::locale::Locale::ZhCN => preset.label_zh,
                                                            _ => preset.label_en,
                                                        };
                                                        let models_count = preset.models.len();
                                                        let is_custom = preset.id == "custom";
                                                        view! {
                                                            <button
                                                                type="button"
                                                                class="glass-card text-left p-4 hover:border-accent/50 transition-colors cursor-pointer space-y-2"
                                                                on:click=move |_| {
                                                                    let p = preset_for_id(pid).unwrap();
                                                                    new_profile_id.set(p.id.to_string());
                                                                    provider.set(p.provider.to_string());
                                                                    base_url.set(p.base_url.to_string());
                                                                    model.set(p.default_model.to_string());
                                                                    tls_sni.set(p.tls_sni.to_string());
                                                                    endpoints_text.set(String::new());
                                                                    save_error.set(String::new());
                                                                    creation_step.set(CreationStep::FillForm);
                                                                }
                                                            >
                                                                <div class="font-semibold text-sm text-theme">{label}</div>
                                                                {if !is_custom {
                                                                    view! {
                                                                        <div class="text-xs text-theme-muted font-mono truncate">{preset.base_url}</div>
                                                                        <div class="text-xs text-accent">
                                                                            {format!("{} {}", models_count, t.upstream_template_models_count())}
                                                                        </div>
                                                                    }.into_any()
                                                                } else {
                                                                    view! { <div class="text-xs text-theme-muted">{t.upstream_preset_custom()}</div> }.into_any()
                                                                }}
                                                            </button>
                                                        }
                                                    }).collect_view()}
                                                </div>
                                            </div>
                                        }.into_any()
                                    } else {
                                        // Step 2: Fill form
                                        view! {
                                            <div class="space-y-4">
                                                <div class="flex items-center justify-between">
                                                    <h4 class="text-sm font-semibold text-theme">{t.upstream_tab_new_profile()}</h4>
                                                    <button type="button" class="text-xs text-accent cursor-pointer"
                                                        on:click=move |_| creation_step.set(CreationStep::PickTemplate)
                                                    >{t.upstream_back_to_templates()}</button>
                                                </div>
                                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_new_profile_id()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="e.g. mimo"
                                                            prop:value=move || new_profile_id.get()
                                                            on:input=move |ev| new_profile_id.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_provider_label()}</label>
                                                        <input type="text" class="input text-sm" placeholder="e.g. mimo"
                                                            prop:value=move || provider.get()
                                                            on:input=move |ev| provider.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                    <div class="md:col-span-2">
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_base_url_label()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="https://..."
                                                            prop:value=move || base_url.get()
                                                            on:input=move |ev| base_url.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_model_label()}</label>
                                                        {move || {
                                                            let models = models_for_provider(&provider.get());
                                                            if models.is_empty() {
                                                                view! { <input type="text" class="input font-mono text-sm" placeholder="model-name"
                                                                    prop:value=move || model.get() on:input=move |ev| model.set(event_target_value(&ev))
                                                                /> }.into_any()
                                                            } else {
                                                                view! {
                                                                    <select class="input font-mono text-sm"
                                                                        prop:value=move || {
                                                                            let m = model.get();
                                                                            if m == CUSTOM_MODEL_SENTINEL { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                            else if models_for_provider(&provider.get()).contains(&m.as_str()) { m }
                                                                            else { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                        }
                                                                        on:change=move |ev| {
                                                                            let v = event_target_value(&ev);
                                                                            if v != CUSTOM_MODEL_SENTINEL { model.set(v); } else { model.set(String::new()); }
                                                                        }
                                                                    >
                                                                        {models.iter().map(|m| view! { <option value=*m>{*m}</option> }).collect_view()}
                                                                        <option value=CUSTOM_MODEL_SENTINEL>{t.upstream_model_custom()}</option>
                                                                    </select>
                                                                }.into_any()
                                                            }
                                                        }}
                                                        {move || {
                                                            let m = model.get();
                                                            let models = models_for_provider(&provider.get());
                                                            let is_custom = m == CUSTOM_MODEL_SENTINEL || (!m.is_empty() && !models.is_empty() && !models.contains(&m.as_str()));
                                                            is_custom.then(|| view! {
                                                                <input type="text" class="input font-mono text-sm mt-2" placeholder="model-name"
                                                                    prop:value=move || { let m = model.get(); if m == CUSTOM_MODEL_SENTINEL { String::new() } else { m } }
                                                                    on:input=move |ev| model.set(event_target_value(&ev))
                                                                />
                                                            })
                                                        }}
                                                    </div>
                                                    <div>
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_tls_sni_label()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="e.g. api.deepseek.com"
                                                            prop:value=move || tls_sni.get()
                                                            on:input=move |ev| tls_sni.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                    <div class="md:col-span-2">
                                                        <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_proxy_label()}</label>
                                                        <input type="text" class="input font-mono text-sm" placeholder="socks5://127.0.0.1:1080"
                                                            prop:value=move || proxy_url.get()
                                                            on:input=move |ev| proxy_url.set(event_target_value(&ev))
                                                        />
                                                    </div>
                                                </div>
                                                {move || if !save_error.get().is_empty() {
                                                    view! { <div class="text-xs text-error mt-2">{save_error.get()}</div> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                <div class="flex justify-end gap-2 pt-4 border-t border-theme/10">
                                                    <button type="button" class="btn btn-secondary text-xs"
                                                        on:click=move |_| { drawer_creating.set(false); drawer_profile.set(None); }
                                                    >"Cancel"</button>
                                                    <button type="button" class="btn btn-primary text-xs"
                                                        on:click=on_create_profile
                                                        disabled=move || saving.get()
                                                    >"Create Profile"</button>
                                                </div>
                                            </div>
                                        }.into_any()
                                    }
                                } else {
                                    // ---- Edit mode: drawer_tab controls content ----
                                    match drawer_tab.get() {
                                        0 => {
                                            // Config tab — edit form
                                            view! {
                                                <div class="space-y-4">
                                                    <div class="flex flex-wrap gap-2">
                                                        {PRESETS.iter().map(|preset| {
                                                            let pid = preset.id;
                                                            let label = match t.locale {
                                                                crate::locale::Locale::ZhCN => preset.label_zh,
                                                                _ => preset.label_en,
                                                            };
                                                            view! {
                                                                <button type="button" class="btn btn-secondary text-xs"
                                                                    on:click=move |_| {
                                                                        let p = preset_for_id(pid).unwrap();
                                                                        base_url.set(p.base_url.to_string());
                                                                        model.set(p.default_model.to_string());
                                                                        provider.set(p.provider.to_string());
                                                                        if !p.tls_sni.is_empty() { tls_sni.set(p.tls_sni.to_string()); }
                                                                    }
                                                                >{label}</button>
                                                            }
                                                        }).collect_view()}
                                                    </div>
                                                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                                        <div>
                                                            <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_provider_label()}</label>
                                                            <input type="text" prop:value=move || provider.get()
                                                                on:input=move |ev| provider.set(event_target_value(&ev))
                                                                class="input text-sm" placeholder="deepseek"
                                                            />
                                                        </div>
                                                        <div>
                                                            <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_base_url_label()}</label>
                                                            <input type="text" prop:value=move || base_url.get()
                                                                on:input=move |ev| base_url.set(event_target_value(&ev))
                                                                class="input font-mono text-sm" placeholder="https://api.deepseek.com"
                                                            />
                                                        </div>
                                                        <div>
                                                            <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_model_label()}</label>
                                                            {move || {
                                                                let models = models_for_provider(&provider.get());
                                                                if models.is_empty() {
                                                                    view! { <input type="text" prop:value=move || model.get()
                                                                        on:input=move |ev| model.set(event_target_value(&ev))
                                                                        class="input font-mono text-sm" placeholder="model-name"
                                                                    /> }.into_any()
                                                                } else {
                                                                    view! {
                                                                        <select class="input font-mono text-sm"
                                                                            prop:value=move || {
                                                                                let m = model.get();
                                                                                if m == CUSTOM_MODEL_SENTINEL { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                                else if models_for_provider(&provider.get()).contains(&m.as_str()) { m }
                                                                                else { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                            }
                                                                            on:change=move |ev| {
                                                                                let v = event_target_value(&ev);
                                                                                if v != CUSTOM_MODEL_SENTINEL { model.set(v); } else { model.set(String::new()); }
                                                                            }
                                                                        >
                                                                            {models.iter().map(|m| view! { <option value=*m>{*m}</option> }).collect_view()}
                                                                            <option value=CUSTOM_MODEL_SENTINEL>{t.upstream_model_custom()}</option>
                                                                        </select>
                                                                    }.into_any()
                                                                }
                                                            }}
                                                            {move || {
                                                                let m = model.get();
                                                                let models = models_for_provider(&provider.get());
                                                                let is_custom = m == CUSTOM_MODEL_SENTINEL || (!m.is_empty() && !models.is_empty() && !models.contains(&m.as_str()));
                                                                is_custom.then(|| view! {
                                                                    <input type="text" class="input font-mono text-sm mt-2" placeholder="model-name"
                                                                        prop:value=move || { let m = model.get(); if m == CUSTOM_MODEL_SENTINEL { String::new() } else { m } }
                                                                        on:input=move |ev| model.set(event_target_value(&ev))
                                                                    />
                                                                })
                                                            }}
                                                        </div>
                                                        {move || (drawer_profile.get().unwrap_or_default() != "deepseek").then(|| view! {
                                                            <div>
                                                                <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_tls_sni_label()}</label>
                                                                <input type="text" prop:value=move || tls_sni.get()
                                                                    on:input=move |ev| tls_sni.set(event_target_value(&ev))
                                                                    class="input font-mono text-sm" placeholder="e.g. api.deepseek.com"
                                                                />
                                                            </div>
                                                        })}
                                                    </div>
                                                    <div class="space-y-2">
                                                        <button type="button" class="text-xs text-accent"
                                                            on:click=move |_| show_advanced.update(|v| *v = !*v)
                                                        >
                                                            {move || if show_advanced.get() { t.upstream_hide_advanced() } else { t.upstream_show_advanced() }}
                                                        </button>
                                                        {move || show_advanced.get().then(|| view! {
                                                            <div class="mt-2">
                                                                <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_endpoints_label()}</label>
                                                                <textarea prop:value=move || endpoints_text.get()
                                                                    on:input=move |ev| endpoints_text.set(event_target_value(&ev))
                                                                    class="input font-mono text-sm h-24 resize-y"
                                                                    placeholder="api.deepseek.com:443"
                                                                ></textarea>
                                                                <p class="text-xs text-theme-muted mt-1">{t.upstream_endpoints_hint()}</p>
                                                            </div>
                                                            <div class="mt-3">
                                                                <label class="block text-xs font-semibold text-theme-muted mb-1">{t.upstream_proxy_label()}</label>
                                                                <input type="text" prop:value=move || proxy_url.get()
                                                                    on:input=move |ev| proxy_url.set(event_target_value(&ev))
                                                                    class="input font-mono text-sm" placeholder="socks5://127.0.0.1:1080"
                                                                />
                                                                <p class="text-xs text-theme-muted mt-1">{t.upstream_proxy_hint()}</p>
                                                            </div>
                                                        })}
                                                    </div>
                                                    <div class="upstream-drawer-test-section space-y-4">
                                                        <h4 class="text-sm font-semibold text-theme">
                                                            {t.upstream_connection_test_title()}
                                                        </h4>
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
                                                                    <p class="text-xs text-theme-muted text-center py-3">
                                                                        {t.upstream_test_not_run()}
                                                                    </p>
                                                                }.into_any()
                                                            }}
                                                        </div>
                                                        <div class="flex flex-wrap items-center gap-3">
                                                            <button
                                                                type="button"
                                                                on:click=on_test
                                                                disabled=move || testing.get()
                                                                class="btn btn-secondary text-xs"
                                                            >
                                                                {move || if testing.get() { t.upstream_testing() } else { t.upstream_test_btn() }}
                                                            </button>
                                                            <button type="button" on:click=on_save disabled=move || saving.get() class="btn btn-primary text-xs">
                                                                {move || if saving.get() { t.upstream_saving() } else { t.upstream_save_btn() }}
                                                            </button>
                                                            {move || if saved.get() {
                                                                view! { <span class="text-xs text-accent font-medium">{t.upstream_saved()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                            {move || if !save_error.get().is_empty() {
                                                                view! { <span class="text-xs text-error">{save_error.get()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                        </div>
                                                    </div>
                                                </div>
                                            }.into_any()
                                        },
                                        1 => {
                                            // Keys tab — key pool
                                            view! {
                                                <div class="space-y-4">
                                                    <div>
                                                        <h3 class="text-base font-semibold text-theme">
                                                            {move || t.upstream_pool_title_for(&drawer_profile.get().unwrap_or_default())}
                                                        </h3>
                                                        <p class="text-xs text-theme-muted mt-1">{move || t.upstream_pool_desc()}</p>
                                                        {move || (drawer_profile.get().unwrap_or_default() == "deepseek").then(|| view! {
                                                            <p class="text-xs text-theme-muted mt-1">{t.upstream_pool_deepseek_hint()}</p>
                                                        })}
                                                        {move || (drawer_profile.get().unwrap_or_default() != "deepseek").then(|| view! {
                                                            <p class="text-xs text-theme-muted mt-1">{t.upstream_pool_patch_deepseek_only()}</p>
                                                        })}
                                                    </div>
                                                    {move || match key_pool.get() {
                                                        None => view! { <Spinner /> }.into_any(),
                                                        Some(Err(e)) => view! { <p class="text-xs text-error">{e}</p> }.into_any(),
                                                        Some(Ok(pool)) => {
                                                            let keys_for_all = pool.keys.clone();
                                                            let pid_for_all = drawer_profile.get().unwrap_or_default();
                                                            view! {
                                                                <div>
                                                                    <div class="flex items-center gap-2 mb-3">
                                                                        <button type="button" class="btn btn-secondary text-xs"
                                                                            disabled=move || testing_all.get()
                                                                            on:click={{
                                                                                let keys = keys_for_all.clone();
                                                                                let pid = pid_for_all.clone();
                                                                                move |_| {
                                                                                    let keys = keys.clone();
                                                                                    let pid = pid.clone();
                                                                                    testing_all.set(true);
                                                                                    leptos::task::spawn_local(async move {
                                                                                        for k in &keys {
                                                                                            key_testing.try_update(|m| { m.insert(k.id.clone(), true); });
                                                                                            let result = api::test_upstream_profile_key(&pid, &k.id).await;
                                                                                            key_testing.try_update(|m| { m.insert(k.id.clone(), false); });
                                                                                            let tr = match result {
                                                                                                Ok(r) => r,
                                                                                                Err(e) => UpstreamTestResult {
                                                                                                    ok: false, status_code: 0, latency_ms: 0,
                                                                                                    model_count: None, error: Some(e), quota: None,
                                                                                                },
                                                                                            };
                                                                                            key_test_results.try_update(|m| { m.insert(k.id.clone(), tr); });
                                                                                        }
                                                                                        testing_all.try_set(false);
                                                                                    });
                                                                                }
                                                                            }}
                                                                        >
                                                                            {move || if testing_all.get() { t.upstream_testing_all() } else { t.upstream_test_all_quotas() }}
                                                                        </button>
                                                                        <button type="button" class="text-xs text-accent cursor-pointer"
                                                                            on:click=move |_| { key_test_results.set(HashMap::new()); }
                                                                        >{t.upstream_clear_results()}</button>
                                                                    </div>
                                                                    <div class="overflow-x-auto">
                                                                        <table class="table text-sm">
                                                                            <thead><tr>
                                                                                <th>{t.upstream_pool_col_id()}</th>
                                                                                <th>{t.upstream_pool_col_preview()}</th>
                                                                                <th>{t.upstream_pool_col_account()}</th>
                                                                                <th>{t.upstream_pool_col_enabled()}</th>
                                                                                <th>{t.upstream_pool_col_quota()}</th>
                                                                                <th>{t.upstream_pool_col_inflight()}</th>
                                                                                <th>{t.upstream_pool_col_cooldown()}</th>
                                                                                <th>{t.upstream_pool_col_test()}</th>
                                                                            </tr></thead>
                                                                            <tbody>
                                                                                {pool.keys.iter().map(|k| {
                                                                                    let kid = k.id.clone();
                                                                                    let kid2 = k.id.clone();
                                                                                    let kid3 = k.id.clone();
                                                                                    let kid_for_test = k.id.clone();
                                                                                    let enabled = k.enabled;
                                                                                    let pid = drawer_profile.get().unwrap_or_default();
                                                                                    let pid2 = pid.clone();
                                                                                    view! {
                                                                                        <tr>
                                                                                            <td class="font-mono">{k.id.clone()}</td>
                                                                                            <td class="font-mono">{k.preview.clone()}</td>
                                                                                            <td class="font-mono text-xs">
                                                                                                {if k.account_id.is_empty() { "default".to_string() } else { k.account_id.clone() }}
                                                                                            </td>
                                                                                            <td>
                                                                                                <div class="flex items-center gap-2">
                                                                                                    <input type="checkbox" prop:checked=enabled
                                                                                                        on:change=move |_| {
                                                                                                            let id = kid.clone();
                                                                                                            let next = !enabled;
                                                                                                            let pid = pid.clone();
                                                                                                            leptos::task::spawn_local(async move {
                                                                                                                let req = PatchUpstreamKeyRequest { enabled: Some(next), secret: None };
                                                                                                                let _ = if pid == "deepseek" {
                                                                                                                    api::patch_upstream_key(&id, &req).await
                                                                                                                } else {
                                                                                                                    api::patch_upstream_profile_key(&pid, &id, &req).await
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
                                                                                            <td>{move || {
                                                                                                let results = key_test_results.get();
                                                                                                match results.get(&kid2) {
                                                                                                    Some(result) => match &result.quota {
                                                                                                        Some(quota) => view! { <QuotaProgressBar quota=quota.clone() /> }.into_any(),
                                                                                                        None => {
                                                                                                            if result.ok {
                                                                                                                view! { <span class="text-xs text-accent">{t.upstream_quota_available()}</span> }.into_any()
                                                                                                            } else {
                                                                                                                let err_msg = result.error.clone().unwrap_or_else(|| "Unknown error".to_string());
                                                                                                                let (label, cls) = match result.status_code {
                                                                                                                    401 | 403 => (t.upstream_quota_auth_failed(), "text-xs text-error"),
                                                                                                                    402 => (t.upstream_quota_exhausted(), "text-xs text-error"),
                                                                                                                    429 => (t.upstream_quota_rate_limited(), "text-xs text-warning"),
                                                                                                                    _ => (t.upstream_quota_test_failed(), "text-xs text-error"),
                                                                                                                };
                                                                                                                view! { <span class=cls title={err_msg}>{label}</span> }.into_any()
                                                                                                            }
                                                                                                        }
                                                                                                    },
                                                                                                    None => view! { <span class="text-xs text-theme-muted">"-"</span> }.into_any(),
                                                                                                }
                                                                                            }}</td>
                                                                                            <td class="font-mono">{k.inflight}</td>
                                                                                            <td class="font-mono text-xs">
                                                                                                {if k.cooldown_remaining_secs > 0 {
                                                                                                    view! { <span class="text-warning">{format!("{}s", k.cooldown_remaining_secs)}</span> }.into_any()
                                                                                                } else {
                                                                                                    view! { <span class="text-theme-muted">"-"</span> }.into_any()
                                                                                                }}
                                                                                            </td>
                                                                                            <td>{move || {
                                                                                                let is_testing = key_testing.get().get(&kid3).copied().unwrap_or(false);
                                                                                                view! {
                                                                                                    <button type="button" class="btn btn-secondary text-xs" disabled=is_testing
                                                                                                        on:click={{
                                                                                                            let kid = kid_for_test.clone();
                                                                                                            let pid = pid2.clone();
                                                                                                            move |_| {
                                                                                                                let kid = kid.clone();
                                                                                                                let pid = pid.clone();
                                                                                                                leptos::task::spawn_local(async move {
                                                                                                                    key_testing.try_update(|m| { m.insert(kid.clone(), true); });
                                                                                                                    let result = api::test_upstream_profile_key(&pid, &kid).await;
                                                                                                                    key_testing.try_update(|m| { m.insert(kid.clone(), false); });
                                                                                                                    let tr = match result {
                                                                                                                        Ok(r) => r,
                                                                                                                        Err(e) => UpstreamTestResult {
                                                                                                                            ok: false, status_code: 0, latency_ms: 0,
                                                                                                                            model_count: None, error: Some(e), quota: None,
                                                                                                                        },
                                                                                                                    };
                                                                                                                    key_test_results.try_update(|m| { m.insert(kid, tr); });
                                                                                                                });
                                                                                                            }
                                                                                                        }}
                                                                                                    >{if is_testing { "..." } else { t.upstream_pool_col_test() }}</button>
                                                                                                }
                                                                                            }}</td>
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
                                                    {move || {
                                                        let prov = provider.get();
                                                        let b_url = base_url.get();
                                                        let pid = drawer_profile.get().unwrap_or_default();
                                                        let is_codex = prov == "codex" || prov == "openai" || b_url.contains("openai.com");
                                                        is_codex.then(|| view! {
                                                            <CodexOAuthPanel profile_id=pid />
                                                        })
                                                    }}
                                                    <div class="space-y-4 pt-4 border-t border-theme/10">
                                                        <div>
                                                            <label class="block text-xs font-semibold text-theme-muted mb-1">
                                                                {move || if pool_replace_mode.get() { t.upstream_pool_replace_label() } else { t.upstream_pool_append_label() }}
                                                            </label>
                                                            <label class="flex items-center gap-2 text-xs text-theme-muted mb-2">
                                                                <input type="checkbox" prop:checked=move || pool_replace_mode.get()
                                                                    on:change=move |ev| pool_replace_mode.set(event_target_checked(&ev))
                                                                />
                                                                {t.upstream_pool_replace_confirm()}
                                                            </label>
                                                            <textarea prop:value=move || pool_secrets_text.get()
                                                                on:input=move |ev| pool_secrets_text.set(event_target_value(&ev))
                                                                class="input font-mono text-sm h-24 resize-y"
                                                                placeholder="sk-...\nacct-b:sk-...\n"
                                                            ></textarea>
                                                        </div>
                                                        <div class="flex items-center gap-3">
                                                            <button on:click=on_save_pool disabled=move || pool_saving.get() class="btn btn-primary text-xs">
                                                                {move || if pool_saving.get() { t.upstream_pool_saving() } else { t.upstream_pool_save_btn() }}
                                                            </button>
                                                            {move || if pool_saved.get() {
                                                                view! { <span class="text-xs text-accent font-medium">{t.upstream_pool_saved()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                            {move || if !pool_error.get().is_empty() {
                                                                view! { <span class="text-xs text-error">{pool_error.get()}</span> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                        </div>
                                                    </div>
                                                    // Codex OAuth panel (device code + PKCE)
                                                    {move || {
                                                        let pid = drawer_profile.get().unwrap_or_default();
                                                        view! { <CodexOAuthPanel profile_id=pid /> }
                                                    }}
                                                </div>
                                            }.into_any()
                                        },
                                        _ => {
                                            // Routing tab
                                            let pid = drawer_profile.get().unwrap_or_default();
                                            view! { <RoutingTab profile_id=pid /> }.into_any()
                                        },
                                    }
                                }}
                            </div>
                        </div>
                    </div>
                })
            }}

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

/// Displays a quota progress bar for an upstream key (balance in CNY).
#[component]
fn QuotaProgressBar(quota: KeyQuotaInfo) -> impl IntoView {
    let t = use_translations();
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
        Some(true) => {
            view! { <span class="text-success text-xs mr-1">{t.upstream_quota_available()}</span> }
                .into_any()
        }
        Some(false) => {
            view! { <span class="text-error text-xs mr-1">{t.upstream_quota_exhausted()}</span> }
                .into_any()
        }
        None => view! { <span></span> }.into_any(),
    };

    view! {
        <div class="min-w-[120px]">
            <div class="flex items-center gap-1 mb-0.5">
                {available_icon}
                {match (quota.balance, quota.total_granted) {
                    (Some(b), Some(g)) => view! {
                        <span class="text-xs font-mono">
                            {format!("CNY {:.2} / {:.2}", b, g)}
                        </span>
                    }.into_any(),
                    (Some(b), _) => view! {
                        <span class="text-xs font-mono">
                            {format!("CNY {:.2}", b)}
                        </span>
                    }.into_any(),
                    _ => view! {
                        <span class="text-xs text-theme-muted">{t.upstream_quota_na()}</span>
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
