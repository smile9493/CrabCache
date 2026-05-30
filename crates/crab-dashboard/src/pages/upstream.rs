use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;

use crate::api;
use crate::components::codex_oauth_panel::CodexOAuthPanel;
use crate::components::routing_tab::RoutingTab;
use crate::components::skeleton::SkeletonUpstreamProfileCard;
use crate::components::sync_result::SyncResultCard;
use crate::components::upstream_key_pool_cards::UpstreamKeyPoolCards;
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
        base_url: "https://chatgpt.com",
        models: &[
            "gpt-5",
            "gpt-5-codex",
            "gpt-5.1-codex",
            "gpt-5.2-codex",
            "gpt-5.3-codex",
        ],
        default_model: "gpt-5-codex",
        tls_sni: "chatgpt.com",
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

/// Synced upstream models for the active profile; falls back to static presets.
fn effective_model_list(synced: &[String], provider: &str) -> Vec<String> {
    if !synced.is_empty() {
        return synced.to_vec();
    }
    models_for_provider(provider)
        .iter()
        .map(|s| (*s).to_string())
        .collect()
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
    let profile_model_options: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let profile_models_syncing = RwSignal::new(false);

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
    let quota_auto_refreshing = RwSignal::new(false);
    let delete_confirm_id: RwSignal<Option<String>> = RwSignal::new(None);
    let key_deleting: RwSignal<HashMap<String, bool>> = RwSignal::new(HashMap::new());
    let pool_delete_error = RwSignal::new(String::new());

    // Profile deletion state
    let show_delete_confirm = RwSignal::new(false);
    let deleting = RwSignal::new(false);

    let default_profile_id = RwSignal::new(String::new());
    let profiles_loaded = RwSignal::new(false);

    let run_key_quota_probe = {
        let key_testing = key_testing;
        let key_test_results = key_test_results;
        let quota_auto_refreshing = quota_auto_refreshing;
        let testing_all = testing_all;
        std::sync::Arc::new(
            move |pid: String, keys: Vec<crate::types::UpstreamKeyView>, only_missing: bool, finish_all: bool| {
                let targets: Vec<_> = keys
                    .into_iter()
                    .filter(|k| k.enabled && (!only_missing || k.quota.is_none()))
                    .collect();
                if targets.is_empty() {
                    if finish_all {
                        testing_all.set(false);
                    }
                    return;
                }
                quota_auto_refreshing.set(true);
                leptos::task::spawn_local(async move {
                    for k in targets {
                        key_testing.try_update(|m| m.insert(k.id.clone(), true));
                        let result = api::test_upstream_profile_key(&pid, &k.id).await;
                        key_testing.try_update(|m| m.insert(k.id.clone(), false));
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
                        key_test_results.try_update(|m| m.insert(k.id.clone(), tr));
                    }
                    quota_auto_refreshing.set(false);
                    if finish_all {
                        testing_all.set(false);
                    }
                });
            },
        )
    };

    // API loaders
    let load_key_pool = {
        let key_pool = key_pool;
        let delete_confirm_id = delete_confirm_id;
        let pool_delete_error = pool_delete_error;
        let default_profile_id = default_profile_id;
        std::sync::Arc::new(move |pid: String| {
            key_pool.set(None);
            delete_confirm_id.set(None);
            pool_delete_error.set(String::new());
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
                    Ok(v) => {
                        key_pool.try_set(Some(Ok(v)));
                    }
                    Err(e) => {
                        key_pool.try_set(Some(Err(e)));
                    }
                }
            });
        })
    };

    {
        let probe = run_key_quota_probe.clone();
        Effect::new(move |_| {
            if let Some(Ok(pool)) = key_pool.get() {
                let pid = active_profile.get_untracked();
                let prov = provider.get_untracked();
                let b_url = base_url.get_untracked();
                if prov == "codex" || prov == "openai" || b_url.contains("chatgpt.com") {
                    probe.clone()(pid, pool.keys.clone(), true, false);
                }
            }
        });
    }

    let load_profile_models = move |pid: String| {
        profile_model_options.set(Vec::new());
        leptos::task::spawn_local(async move {
            if let Ok(resp) = api::fetch_models(Some(&pid)).await {
                let ids: Vec<String> = resp.models.into_iter().map(|m| m.id).collect();
                profile_model_options.set(ids);
            }
        });
    };

    let load_profile_data = {
        let load_key_pool = load_key_pool.clone();
        std::sync::Arc::new(move |pid: String| {
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
            load_profile_models(pid.clone());
            load_key_pool.clone()(pid);
        })
    };

    let load_profiles_and_select = {
        let load_profile_data = load_profile_data.clone();
        std::sync::Arc::new(move |pid: Option<String>| {
            let load = load_profile_data.clone();
            leptos::task::spawn_local(async move {
                if let Ok(resp) = api::fetch_upstream_profiles().await {
                    let def = resp.default_profile_id.clone();
                    default_profile_id.try_set(def.clone());
                    profiles.try_set(resp.profiles);
                    let select = pid.unwrap_or(def);
                    active_profile.try_set(select.clone());
                    load(select);
                }
                profiles_loaded.try_set(true);
            });
        })
    };

    // Initial load: gateway default profile (not hardcoded id)
    load_profiles_and_select.clone()(None);

    let probe_all_keys = run_key_quota_probe.clone();
    let reload_key_pool = load_key_pool.clone();
    let refresh_profiles = load_profiles_and_select.clone();
    let open_profile = load_profile_data.clone();

    let on_sync_profile_models = Callback::new(move |_| {
        let pid = active_profile.get();
        profile_models_syncing.set(true);
        save_error.set(String::new());
        leptos::task::spawn_local(async move {
            match api::sync_models(&pid).await {
                Ok(r) => {
                    sync_result.set(Some(r));
                    if let Ok(resp) = api::fetch_models(Some(&pid)).await {
                        profile_model_options.set(
                            resp.models.into_iter().map(|m| m.id).collect(),
                        );
                    }
                }
                Err(e) => save_error.set(e),
            }
            profile_models_syncing.set(false);
        });
    });

    // Actions
    let on_test = Callback::new(move |_| {
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
    });

    let on_save = Callback::new({
        let refresh = refresh_profiles.clone();
        move |_| {
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
        let r = refresh.clone();

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
                    fallback_profile_id: None,
                    fallback_max_retries: None,
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
                    r.clone()(Some(pid));
                }
                Err(e) => { save_error.try_set(e); },
            }
            saving.try_set(false);
        });
        }
    });

    let on_save_pool = Callback::new(move |_| {
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
    });

    let on_delete_profile = Callback::new({
        let refresh = refresh_profiles.clone();
        move |_| {
        let pid = active_profile.get();
        if pid == default_profile_id.get_untracked() {
            return;
        }
        deleting.set(true);
        let r = refresh.clone();
        leptos::task::spawn_local(async move {
            match api::delete_upstream_profile(&pid).await {
                Ok(_) => {
                    show_delete_confirm.try_set(false);
                    drawer_profile.try_set(None);
                    drawer_creating.try_set(false);
                    r.clone()(None);
                }
                Err(e) => {
                    save_error.try_set(e);
                }
            }
            deleting.try_set(false);
        });
        }
    });

    let on_create_profile = Callback::new({
        let refresh = refresh_profiles.clone();
        move |_| {
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
        let r = refresh.clone();
        leptos::task::spawn_local(async move {
            let req = PutUpstreamProfileAdminRequest {
                provider: prov,
                base_url: url,
                fallback_model: model_val,
                endpoints: Vec::new(),
                tls_sni: sni,
                proxy_url: proxy_opt,
                fallback_profile_id: None,
                fallback_max_retries: None,
            };
            match api::put_upstream_profile(&id, &req).await {
                Ok(_) => {
                    drawer_creating.try_set(false);
                    new_profile_id.try_set(String::new());
                    active_profile.try_set(id.clone());
                    drawer_profile.try_set(Some(id.clone()));
                    r.clone()(Some(id));
                }
                Err(e) => {
                    save_error.try_set(e);
                }
            }
            saving.try_set(false);
        });
        }
    });

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
                                        on:click={
                                            let open = open_profile.clone();
                                            move |_| {
                                            drawer_creating.set(false);
                                            drawer_tab.set(0);
                                            active_profile.set(pid.clone());
                                            drawer_profile.set(Some(pid.clone()));
                                            open.clone()(pid.clone());
                                        }}
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
                if !show {
                    ().into_any()
                } else {
                let probe_fn = probe_all_keys.clone();
                let reload_fn = reload_key_pool.clone();
                view! {
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
                                {move || {
                                    let probe = probe_fn.clone();
                                    let reload = reload_fn.clone();
                                    if drawer_creating.get() {
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
                                                        on:click=move |_| on_create_profile.run(())
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
                                                            <div class="flex items-center justify-between mb-1">
                                                                <label class="block text-xs font-semibold text-theme-muted">{t.upstream_model_label()}</label>
                                                                <button type="button" class="text-xs text-accent disabled:opacity-50"
                                                                    disabled=move || profile_models_syncing.get()
                                                                    on:click=move |_| on_sync_profile_models.run(())
                                                                >
                                                                    {move || if profile_models_syncing.get() { t.models_syncing() } else { t.models_sync_btn() }}
                                                                </button>
                                                            </div>
                                                            {move || {
                                                                let options = effective_model_list(&profile_model_options.get(), &provider.get());
                                                                if options.is_empty() {
                                                                    view! { <input type="text" prop:value=move || model.get()
                                                                        on:input=move |ev| model.set(event_target_value(&ev))
                                                                        class="input font-mono text-sm" placeholder="model-name"
                                                                    /> }.into_any()
                                                                } else {
                                                                    view! {
                                                                        <select class="input font-mono text-sm"
                                                                            prop:value=move || {
                                                                                let m = model.get();
                                                                                let opts = effective_model_list(&profile_model_options.get(), &provider.get());
                                                                                if m == CUSTOM_MODEL_SENTINEL { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                                else if opts.iter().any(|o| o == &m) { m }
                                                                                else { CUSTOM_MODEL_SENTINEL.to_string() }
                                                                            }
                                                                            on:change=move |ev| {
                                                                                let v = event_target_value(&ev);
                                                                                if v != CUSTOM_MODEL_SENTINEL { model.set(v); } else { model.set(String::new()); }
                                                                            }
                                                                        >
                                                                            {options.iter().map(|m| {
                                                                                let value = m.clone();
                                                                                let text = m.clone();
                                                                                view! { <option value=value>{text}</option> }
                                                                            }).collect_view()}
                                                                            <option value=CUSTOM_MODEL_SENTINEL>{t.upstream_model_custom()}</option>
                                                                        </select>
                                                                    }.into_any()
                                                                }
                                                            }}
                                                            {move || {
                                                                let m = model.get();
                                                                let options = effective_model_list(&profile_model_options.get(), &provider.get());
                                                                let is_custom = m == CUSTOM_MODEL_SENTINEL || (!m.is_empty() && !options.is_empty() && !options.contains(&m));
                                                                is_custom.then(|| view! {
                                                                    <input type="text" class="input font-mono text-sm mt-2" placeholder="model-name"
                                                                        prop:value=move || { let m = model.get(); if m == CUSTOM_MODEL_SENTINEL { String::new() } else { m } }
                                                                        on:input=move |ev| model.set(event_target_value(&ev))
                                                                    />
                                                                })
                                                            }}
                                                            {move || {
                                                                let n = profile_model_options.get().len();
                                                                (n > 0).then(|| view! {
                                                                    <p class="text-xs text-theme-muted mt-1">{n} {t.upstream_template_models_count()}</p>
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
                                                                on:click=move |_| on_test.run(())
                                                                disabled=move || testing.get()
                                                                class="btn btn-secondary text-xs"
                                                            >
                                                                {move || if testing.get() { t.upstream_testing() } else { t.upstream_test_btn() }}
                                                            </button>
                                                            <button type="button" on:click=move |_| on_save.run(()) disabled=move || saving.get() class="btn btn-primary text-xs">
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
                                                            let pid_for_toggle = pid_for_all.clone();
                                                            let pid_for_test = pid_for_all.clone();
                                                            let pid_for_delete = pid_for_all.clone();
                                                            let probe_all = probe.clone();
                                                            let reload_pool = reload.clone();
                                                            view! {
                                                                <div>
                                                                    <div class="flex items-center gap-2 mb-3 flex-wrap">
                                                                        <button type="button" class="btn btn-secondary text-xs"
                                                                            disabled=move || testing_all.get() || quota_auto_refreshing.get()
                                                                            on:click={{
                                                                                let keys = keys_for_all.clone();
                                                                                let pid = pid_for_all.clone();
                                                                                let probe = probe_all.clone();
                                                                                move |_| {
                                                                                    testing_all.set(true);
                                                                                    probe.clone()(pid.clone(), keys.clone(), false, true);
                                                                                }
                                                                            }}
                                                                        >
                                                                            {move || if testing_all.get() || quota_auto_refreshing.get() {
                                                                                t.upstream_testing_all()
                                                                            } else {
                                                                                t.upstream_test_all_quotas()
                                                                            }}
                                                                        </button>
                                                                        <button type="button" class="text-xs text-accent cursor-pointer"
                                                                            on:click=move |_| { key_test_results.set(HashMap::new()); }
                                                                        >{t.upstream_clear_results()}</button>
                                                                        {move || quota_auto_refreshing.get().then(|| view! {
                                                                            <span class="text-xs text-theme-muted">{t.upstream_pool_auto_quota()}</span>
                                                                        })}
                                                                        {move || (!pool_delete_error.get().is_empty()).then(|| view! {
                                                                            <span class="text-xs text-error">{pool_delete_error.get()}</span>
                                                                        })}
                                                                    </div>
                                                                    <UpstreamKeyPoolCards
                                                                        keys=pool.keys.clone()
                                                                        on_toggle=Callback::new({
                                                                            let reload = reload_pool.clone();
                                                                            move |(id, next): (String, bool)| {
                                                                                let pid = pid_for_toggle.clone();
                                                                                let reload = reload.clone();
                                                                                leptos::task::spawn_local(async move {
                                                                                    let req = PatchUpstreamKeyRequest { enabled: Some(next), secret: None };
                                                                                    let default_id = default_profile_id.get_untracked();
                                                                                    let _ = if pid == default_id {
                                                                                        api::patch_upstream_key(&id, &req).await
                                                                                    } else {
                                                                                        api::patch_upstream_profile_key(&pid, &id, &req).await
                                                                                    };
                                                                                    reload.clone()(pid);
                                                                                });
                                                                            }
                                                                        })
                                                                        on_test=Callback::new({
                                                                            move |kid: String| {
                                                                                let pid = pid_for_test.clone();
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
                                                                        })
                                                                        on_delete_confirm=Callback::new({
                                                                            let reload = reload_pool.clone();
                                                                            move |kid: String| {
                                                                                let pid = pid_for_delete.clone();
                                                                                let reload = reload.clone();
                                                                                let err_prefix = t.upstream_pool_delete_error().to_string();
                                                                                leptos::task::spawn_local(async move {
                                                                                    key_deleting.try_update(|m| { m.insert(kid.clone(), true); });
                                                                                    pool_delete_error.set(String::new());
                                                                                    let default_id = default_profile_id.get_untracked();
                                                                                    let result = if pid == default_id {
                                                                                        api::delete_upstream_key(&kid).await
                                                                                    } else {
                                                                                        api::delete_upstream_profile_key(&pid, &kid).await
                                                                                    };
                                                                                    key_deleting.try_update(|m| { m.insert(kid.clone(), false); });
                                                                                    delete_confirm_id.set(None);
                                                                                    match result {
                                                                                        Ok(()) => reload.clone()(pid),
                                                                                        Err(e) => pool_delete_error.set(format!("{err_prefix}: {e}")),
                                                                                    }
                                                                                });
                                                                            }
                                                                        })
                                                                        key_testing=key_testing.read_only()
                                                                        key_test_results=key_test_results.read_only()
                                                                        delete_confirm_id=delete_confirm_id
                                                                        key_deleting=key_deleting.read_only()
                                                                    />
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
                                                            <p class="text-xs text-theme-muted mt-1">
                                                                {move || t.upstream_pool_hint()}
                                                            </p>
                                                        </div>
                                                        <div class="flex items-center gap-3">
                                                            <button on:click=move |_| on_save_pool.run(()) disabled=move || pool_saving.get() class="btn btn-primary text-xs">
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
                                                </div>
                                            }.into_any()
                                        },
                                        _ => {
                                            // Routing tab
                                            let pid = drawer_profile.get().unwrap_or_default();
                                            view! { <RoutingTab profile_id=pid /> }.into_any()
                                        },
                                    }
                                }
                                }}
                            </div>
                        </div>
                    </div>
                }.into_any()
                }
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
                                on:click=move |_| on_delete_profile.run(())
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
