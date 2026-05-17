use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{
    PatchUpstreamKeyRequest, PutUpstreamKeysRequest, UpdateUpstreamConfigRequest, UpstreamConfig,
    UpstreamKeyInput, UpstreamKeysView,
};

#[component]
pub fn UpstreamPage() -> impl IntoView {
    let t = use_translations();
    let config: RwSignal<Option<Result<UpstreamConfig, String>>> = RwSignal::new(None);

    let base_url = RwSignal::new(String::new());
    let api_key = RwSignal::new(String::new());
    let api_key_dirty = RwSignal::new(false);
    let endpoints_text = RwSignal::new(String::new());

    let saving = RwSignal::new(false);
    let saved = RwSignal::new(false);
    let save_error = RwSignal::new(String::new());

    let key_pool: RwSignal<Option<Result<UpstreamKeysView, String>>> = RwSignal::new(None);
    let pool_secrets_text = RwSignal::new(String::new());
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
                    endpoints_text.set(c.endpoints.join("\n"));
                    api_key_dirty.set(false);
                    config.set(Some(Ok(c)));
                }
                Err(e) => config.set(Some(Err(e))),
            }
        });
    };

    load_data();
    load_key_pool();

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
        let req = PutUpstreamKeysRequest { keys };
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

        let endpoints: Vec<String> = endpoints_text
            .get()
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let api_key_value = if api_key_dirty.get() {
            Some(api_key.get())
        } else {
            None
        };

        let req = UpdateUpstreamConfigRequest {
            base_url: base_url.get().trim().to_string(),
            api_key: api_key_value,
            endpoints,
        };

        leptos::task::spawn_local(async move {
            match api::update_upstream_config(&req).await {
                Ok(c) => {
                    base_url.set(c.base_url.clone());
                    endpoints_text.set(c.endpoints.join("\n"));
                    api_key.set(String::new());
                    api_key_dirty.set(false);
                    saved.set(true);
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

            {move || match config.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().upstream_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(cfg)) => {
                    let masked = cfg.api_key_masked.clone();
                    let has_key = !cfg.api_key.is_empty();
                    view! {
                        <div class="glass-card space-y-5">
                            <div class="grid grid-cols-1 gap-5">
                                <div>
                                    <label class="block text-sm font-medium text-theme mb-1.5">
                                        {use_translations().upstream_base_url_label()}
                                    </label>
                                    <input
                                        type="text"
                                        prop:value=move || base_url.get()
                                        on:input=move |ev| {
                                            base_url.set(event_target_value(&ev));
                                        }
                                        class="input font-mono text-sm"
                                        placeholder="https://api.deepseek.com"
                                    />
                                    <p class="text-xs text-theme-muted mt-1">
                                        {use_translations().upstream_base_url_hint()}
                                    </p>
                                </div>

                                <div>
                                    <label class="block text-sm font-medium text-theme mb-1.5">
                                        {use_translations().upstream_api_key_label()}
                                    </label>
                                    <input
                                        type="password"
                                        prop:value=move || api_key.get()
                                        on:input=move |ev| {
                                            api_key.set(event_target_value(&ev));
                                            api_key_dirty.set(true);
                                        }
                                        class="input font-mono text-sm"
                                        placeholder={
                                            if has_key {
                                                masked.clone()
                                            } else {
                                                "sk-...".to_string()
                                            }
                                        }
                                    />
                                    <p class="text-xs text-theme-muted mt-1">
                                        {if has_key {
                                            use_translations().upstream_api_key_masked_hint()
                                        } else {
                                            use_translations().upstream_api_key_hint()
                                        }}
                                    </p>
                                </div>

                                <div>
                                    <label class="block text-sm font-medium text-theme mb-1.5">
                                        {use_translations().upstream_endpoints_label()}
                                    </label>
                                    <textarea
                                        prop:value=move || endpoints_text.get()
                                        on:input=move |ev| {
                                            endpoints_text.set(event_target_value(&ev));
                                        }
                                        class="input font-mono text-sm h-28 resize-y"
                                        placeholder="113.240.66.218:443"
                                    ></textarea>
                                    <p class="text-xs text-theme-muted mt-1">
                                        {use_translations().upstream_endpoints_hint()}
                                    </p>
                                </div>
                            </div>

                            <div class="flex items-center gap-3 pt-2">
                                <button
                                    on:click=on_save
                                    disabled=move || saving.get()
                                    class="btn btn-primary text-sm"
                                >
                                    {move || if saving.get() {
                                        use_translations().upstream_saving()
                                    } else {
                                        use_translations().upstream_save_btn()
                                    }}
                                </button>

                                {move || {
                                    if saved.get() {
                                        view! {
                                            <span class="text-xs text-accent font-medium">
                                                {use_translations().upstream_saved()}
                                            </span>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }
                                }}

                                {move || {
                                    if !save_error.get().is_empty() {
                                        view! {
                                            <span class="text-xs text-error">
                                                {save_error.get()}
                                            </span>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }
                                }}
                            </div>
                        </div>
                    }.into_any()
                }
            }}

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
                                {move || t.upstream_pool_replace_label()}
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
