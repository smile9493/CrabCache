use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{ModelListResponse, UpstreamConfig, UpdateUpstreamConfigRequest};

#[component]
pub fn UpstreamPage() -> impl IntoView {
    let t = use_translations();
    let config: RwSignal<Option<Result<UpstreamConfig, String>>> = RwSignal::new(None);
    let models: RwSignal<Option<Result<ModelListResponse, String>>> = RwSignal::new(None);

    let base_url = RwSignal::new(String::new());
    let api_key = RwSignal::new(String::new());
    let api_key_dirty = RwSignal::new(false);
    let model = RwSignal::new(String::new());
    let endpoints_text = RwSignal::new(String::new());

    let saving = RwSignal::new(false);
    let saved = RwSignal::new(false);
    let save_error = RwSignal::new(String::new());

    let load_data = {
        let config = config.clone();
        let models = models.clone();
        let base_url = base_url.clone();
        let _api_key = api_key.clone();
        let api_key_dirty = api_key_dirty.clone();
        let model = model.clone();
        let endpoints_text = endpoints_text.clone();
        move || {
            leptos::task::spawn_local({
                let config = config.clone();
                async move {
                    match api::fetch_upstream_config().await {
                        Ok(c) => {
                            base_url.set(c.base_url.clone());
                            model.set(c.model.clone());
                            endpoints_text.set(c.endpoints.join("\n"));
                            api_key_dirty.set(false);
                            config.set(Some(Ok(c)));
                        }
                        Err(e) => config.set(Some(Err(e))),
                    }
                }
            });
            leptos::task::spawn_local({
                let models = models.clone();
                async move {
                    match api::fetch_models().await {
                        Ok(m) => models.set(Some(Ok(m))),
                        Err(e) => models.set(Some(Err(e))),
                    }
                }
            });
        }
    };

    load_data();

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
            model: model.get().trim().to_string(),
            endpoints,
        };

        leptos::task::spawn_local(async move {
            match api::update_upstream_config(&req).await {
                Ok(c) => {
                    base_url.set(c.base_url.clone());
                    model.set(c.model.clone());
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
        <div class="p-6 space-y-6">
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
                    let current_model = model.clone();
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
                                        {use_translations().upstream_model_label()}
                                    </label>
                                    {move || match models.get() {
                                        None => view! {
                                            <input
                                                type="text"
                                                prop:value=move || current_model.get()
                                                class="input font-mono text-sm"
                                                disabled=true
                                            />
                                            <p class="text-xs text-theme-muted mt-1">"Loading models..."</p>
                                        }.into_any(),
                                        Some(Err(_)) => view! {
                                            <input
                                                type="text"
                                                prop:value=move || current_model.get()
                                                on:input=move |ev| {
                                                    model.set(event_target_value(&ev));
                                                }
                                                class="input font-mono text-sm"
                                                placeholder="deepseek-v4-pro"
                                            />
                                            <p class="text-xs text-theme-muted mt-1">
                                                {use_translations().upstream_model_hint()}
                                            </p>
                                        }.into_any(),
                                        Some(Ok(model_list)) => {
                                            let model_ids: Vec<String> = model_list.models.iter().map(|m| m.id.clone()).collect();
                                            if model_ids.is_empty() {
                                                view! {
                                                    <input
                                                        type="text"
                                                        prop:value=move || current_model.get()
                                                        on:input=move |ev| {
                                                            model.set(event_target_value(&ev));
                                                        }
                                                        class="input font-mono text-sm"
                                                        placeholder="deepseek-v4-pro"
                                                    />
                                                    <p class="text-xs text-theme-muted mt-1">
                                                        "No models synced. Visit Models page to sync upstream models."
                                                    </p>
                                                }.into_any()
                                            } else {
                                                let has_current = model_ids.contains(&current_model.get());
                                                view! {
                                                    <select
                                                        prop:value=move || current_model.get()
                                                        on:change=move |ev| {
                                                            model.set(event_target_value(&ev));
                                                        }
                                                        class="input font-mono text-sm"
                                                    >
                                                        {if !has_current && !current_model.get().is_empty() {
                                                            view! {
                                                                <option value={current_model.get()} selected=true>
                                                                    {current_model.get()}
                                                                </option>
                                                            }.into_any()
                                                        } else {
                                                            view! { <option></option> }.into_any()
                                                        }}
                                                        {model_ids.into_iter().map(|id| {
                                                            let id_str = id.clone();
                                                            let selected = id_str == current_model.get();
                                                            view! {
                                                                <option value={id.clone()} selected=selected>
                                                                    {id_str}
                                                                </option>
                                                            }
                                                        }).collect::<Vec<_>>()}
                                                    </select>
                                                    <p class="text-xs text-theme-muted mt-1">
                                                        {use_translations().upstream_model_hint()}
                                                    </p>
                                                }.into_any()
                                            }
                                        }
                                    }}
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
                                        placeholder="api.deepseek.com:443"
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
        </div>
    }
}