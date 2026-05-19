use leptos::prelude::*;

use crate::api;
use crate::components::sync_result::SyncResultCard;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{
    PatchUpstreamKeyRequest, PutUpstreamKeysRequest, SyncResult, UpdateUpstreamConfigRequest,
    UpstreamConfig, UpstreamKeyInput, UpstreamKeysPutMode, UpstreamKeysView, UpstreamTestBody,
};

const OFFICIAL_BASE: &str = "https://api.deepseek.com";
const DEFAULT_MODEL: &str = "deepseek-v4-pro";

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
    text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[component]
pub fn UpstreamPage() -> impl IntoView {
    let t = use_translations();
    let config: RwSignal<Option<Result<UpstreamConfig, String>>> = RwSignal::new(None);

    let base_url = RwSignal::new(OFFICIAL_BASE.to_string());
    let model = RwSignal::new(DEFAULT_MODEL.to_string());
    let endpoints_text = RwSignal::new(String::new());
    let show_advanced = RwSignal::new(false);

    let saving = RwSignal::new(false);
    let saved = RwSignal::new(false);
    let save_error = RwSignal::new(String::new());
    let sync_result: RwSignal<Option<SyncResult>> = RwSignal::new(None);

    let testing = RwSignal::new(false);
    let test_ok = RwSignal::new(false);
    let test_message = RwSignal::new(String::new());

    let key_pool: RwSignal<Option<Result<UpstreamKeysView, String>>> = RwSignal::new(None);
    let pool_secrets_text = RwSignal::new(String::new());
    let pool_replace_mode = RwSignal::new(false);
    let pool_saving = RwSignal::new(false);
    let pool_saved = RwSignal::new(false);
    let pool_error = RwSignal::new(String::new());

    let load_key_pool = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_upstream_keys().await {
                Ok(v) => key_pool.set(Some(Ok(v))),
                Err(e) => key_pool.set(Some(Err(e))),
            }
        });
    };

    let load_data = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_upstream_config().await {
                Ok(c) => {
                    base_url.set(c.base_url.clone());
                    model.set(c.model.clone());
                    endpoints_text.set(c.endpoints.join("\n"));
                    if let Some(ref lt) = c.last_test {
                        test_ok.set(lt.ok);
                        test_message.set(
                            lt.error
                                .clone()
                                .unwrap_or_else(|| format!("OK ({} ms)", lt.latency_ms)),
                        );
                    }
                    config.set(Some(Ok(c)));
                }
                Err(e) => config.set(Some(Err(e))),
            }
        });
    };

    load_data();
    load_key_pool();

    let on_test = move |_| {
        testing.set(true);
        test_ok.set(false);
        test_message.set(String::new());
        let url = base_url.get();
        if let Some(err) = validate_base_url(&url) {
            test_message.set(err);
            testing.set(false);
            return;
        }
        let key = first_key_from_text(&pool_secrets_text.get()).or_else(|| {
            config.get().and_then(|c| match c {
                Ok(cfg) if cfg.key_pool_count > 0 => None,
                _ => None,
            })
        });
        let key = match key {
            Some(k) => k,
            None => {
                test_message.set("Add at least one API key in the pool textarea to test".into());
                testing.set(false);
                return;
            }
        };
        leptos::task::spawn_local(async move {
            match api::test_upstream_connection(&UpstreamTestBody {
                base_url: url,
                api_key: key,
            })
            .await
            {
                Ok(r) => {
                    test_ok.set(r.ok);
                    test_message.set(
                        r.error.unwrap_or_else(|| {
                            format!(
                                "OK — {} models, {} ms",
                                r.model_count.unwrap_or(0),
                                r.latency_ms
                            )
                        }),
                    );
                }
                Err(e) => test_message.set(e),
            }
            testing.set(false);
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
            pool_error.set("Enter at least one DeepSeek API key (one per line)".to_string());
            pool_saving.set(false);
            return;
        }
        let keys: Vec<UpstreamKeyInput> = secrets
            .into_iter()
            .enumerate()
            .map(|(i, secret)| UpstreamKeyInput {
                id: format!("key-{}", i + 1),
                secret,
                enabled: true,
            })
            .collect();
        let mode = if pool_replace_mode.get() {
            UpstreamKeysPutMode::Replace
        } else {
            UpstreamKeysPutMode::Append
        };
        let req = PutUpstreamKeysRequest { keys, mode };
        leptos::task::spawn_local(async move {
            match api::put_upstream_keys(&req).await {
                Ok(v) => {
                    key_pool.set(Some(Ok(v)));
                    pool_secrets_text.set(String::new());
                    pool_saved.set(true);
                }
                Err(e) => pool_error.set(e),
            }
            pool_saving.set(false);
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

        let req = UpdateUpstreamConfigRequest {
            base_url: url,
            model: model_val,
            api_key: None,
            endpoints,
            keys_to_append,
        };

        leptos::task::spawn_local(async move {
            match api::update_upstream_config(&req).await {
                Ok(resp) => {
                    base_url.set(resp.config.base_url.clone());
                    model.set(resp.config.model.clone());
                    endpoints_text.set(resp.config.endpoints.join("\n"));
                    pool_secrets_text.set(String::new());
                    if let Some(s) = resp.sync {
                        sync_result.set(Some(s));
                    }
                    saved.set(true);
                    match api::fetch_upstream_keys().await {
                        Ok(v) => key_pool.set(Some(Ok(v))),
                        Err(e) => key_pool.set(Some(Err(e))),
                    }
                }
                Err(e) => save_error.set(e),
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

            {move || match config.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().upstream_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(cfg)) => {
                    let gw_ok = cfg.gateway_reachable;
                    view! {
                        {(!gw_ok).then(|| view! {
                            <div class="glass-card text-warning text-sm">
                                {use_translations().upstream_gateway_unreachable()}
                            </div>
                        })}

                        <div class="glass-card space-y-4">
                            <div class="flex flex-wrap gap-2">
                                <button
                                    type="button"
                                    class="btn btn-secondary text-xs"
                                    on:click=move |_| {
                                        base_url.set(OFFICIAL_BASE.into());
                                        model.set(DEFAULT_MODEL.into());
                                    }
                                >
                                    {use_translations().upstream_preset_official()}
                                </button>
                                <button
                                    type="button"
                                    class="btn btn-secondary text-xs"
                                    on:click=move |_| {
                                        base_url.set(String::new());
                                    }
                                >
                                    {use_translations().upstream_preset_custom()}
                                </button>
                            </div>

                            <div class="grid grid-cols-1 gap-5">
                                <div>
                                    <label class="block text-sm font-medium text-theme mb-1.5">
                                        {use_translations().upstream_base_url_label()}
                                    </label>
                                    <input
                                        type="text"
                                        prop:value=move || base_url.get()
                                        on:input=move |ev| base_url.set(event_target_value(&ev))
                                        class="input font-mono text-sm"
                                        placeholder=OFFICIAL_BASE
                                    />
                                    <p class="text-xs text-theme-muted mt-1">
                                        {use_translations().upstream_base_url_hint()}
                                    </p>
                                </div>

                                <div>
                                    <label class="block text-sm font-medium text-theme mb-1.5">
                                        {use_translations().upstream_model_label()}
                                    </label>
                                    <input
                                        type="text"
                                        prop:value=move || model.get()
                                        on:input=move |ev| model.set(event_target_value(&ev))
                                        class="input font-mono text-sm"
                                        placeholder=DEFAULT_MODEL
                                    />
                                    <p class="text-xs text-theme-muted mt-1">
                                        {use_translations().upstream_model_hint()}
                                    </p>
                                </div>

                                <div>
                                    <button
                                        type="button"
                                        class="text-xs text-accent"
                                        on:click=move |_| show_advanced.update(|v| *v = !*v)
                                    >
                                        {move || if show_advanced.get() {
                                            use_translations().upstream_hide_advanced()
                                        } else {
                                            use_translations().upstream_show_advanced()
                                        }}
                                    </button>
                                    {move || show_advanced.get().then(|| view! {
                                        <div class="mt-2">
                                            <label class="block text-sm font-medium text-theme mb-1.5">
                                                {use_translations().upstream_endpoints_label()}
                                            </label>
                                            <textarea
                                                prop:value=move || endpoints_text.get()
                                                on:input=move |ev| endpoints_text.set(event_target_value(&ev))
                                                class="input font-mono text-sm h-24 resize-y"
                                                placeholder="api.deepseek.com:443"
                                            ></textarea>
                                            <p class="text-xs text-theme-muted mt-1">
                                                {use_translations().upstream_endpoints_hint()}
                                            </p>
                                        </div>
                                    })}
                                </div>
                            </div>

                            <div class="flex flex-wrap items-center gap-3 pt-2 border-t border-theme/10">
                                <button
                                    type="button"
                                    on:click=on_test
                                    disabled=move || testing.get()
                                    class="btn btn-secondary text-sm"
                                >
                                    {move || if testing.get() {
                                        use_translations().upstream_testing()
                                    } else {
                                        use_translations().upstream_test_btn()
                                    }}
                                </button>
                                <button
                                    type="button"
                                    on:click=on_save
                                    disabled=move || saving.get()
                                    class=move || {
                                        if test_ok.get() {
                                            "btn btn-primary text-sm"
                                        } else {
                                            "btn btn-primary text-sm opacity-90"
                                        }
                                    }
                                >
                                    {move || if saving.get() {
                                        use_translations().upstream_saving()
                                    } else {
                                        use_translations().upstream_save_btn()
                                    }}
                                </button>
                                {move || if saved.get() {
                                    view! {
                                        <span class="text-xs text-accent font-medium">
                                            {use_translations().upstream_saved()}
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
                            {move || if !test_message.get().is_empty() {
                                let ok = test_ok.get();
                                view! {
                                    <p class=move || if ok { "text-xs text-accent" } else { "text-xs text-error" }>
                                        {test_message.get()}
                                    </p>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                        </div>
                    }.into_any()
                }
            }}

            {move || sync_result.get().map(|r| view! { <SyncResultCard result=r /> })}

            <div class="glass-card space-y-4 mt-6">
                <div>
                    <h2 class="text-lg font-semibold text-theme">{move || t.upstream_pool_title()}</h2>
                    <p class="text-sm text-theme-muted mt-1">{move || t.upstream_pool_desc()}</p>
                </div>

                {move || match key_pool.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! {
                        <p class="text-sm text-error">{e}</p>
                    }.into_any(),
                    Some(Ok(pool)) => view! {
                        <div class="overflow-x-auto">
                            <table class="w-full text-sm">
                                <thead>
                                    <tr class="text-left text-theme-muted border-b border-theme/10">
                                        <th class="py-2 pr-3">{move || t.upstream_pool_col_id()}</th>
                                        <th class="py-2 pr-3">{move || t.upstream_pool_col_preview()}</th>
                                        <th class="py-2 pr-3">{move || t.upstream_pool_col_enabled()}</th>
                                        <th class="py-2 pr-3">{move || t.upstream_pool_col_inflight()}</th>
                                        <th class="py-2">{move || t.upstream_pool_col_cooldown()}</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {pool.keys.iter().map(|k| {
                                        let id = k.id.clone();
                                        let enabled = k.enabled;
                                        view! {
                                            <tr class="border-b border-theme/5">
                                                <td class="py-2 pr-3 font-mono">{k.id.clone()}</td>
                                                <td class="py-2 pr-3 font-mono">{k.preview.clone()}</td>
                                                <td class="py-2 pr-3">
                                                    <input
                                                        type="checkbox"
                                                        prop:checked=enabled
                                                        on:change=move |_| {
                                                            let id = id.clone();
                                                            let next = !enabled;
                                                            leptos::task::spawn_local(async move {
                                                                let _ = api::patch_upstream_key(
                                                                    &id,
                                                                    &PatchUpstreamKeyRequest {
                                                                        enabled: Some(next),
                                                                        secret: None,
                                                                    },
                                                                ).await;
                                                                load_key_pool();
                                                            });
                                                        }
                                                    />
                                                </td>
                                                <td class="py-2 pr-3">{k.inflight}</td>
                                                <td class="py-2">{k.cooldown_remaining_secs}</td>
                                            </tr>
                                        }
                                    }).collect_view()}
                                </tbody>
                            </table>
                        </div>

                        <div>
                            <label class="block text-sm font-medium text-theme mb-1.5">
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
                                placeholder="sk-...\nsk-..."
                            ></textarea>
                        </div>

                        <div class="flex items-center gap-3">
                            <button
                                on:click=on_save_pool
                                disabled=move || pool_saving.get()
                                class="btn btn-primary text-sm"
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
                    }.into_any(),
                }}
            </div>
        </div>
    }
}
