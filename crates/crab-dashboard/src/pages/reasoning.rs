use leptos::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::ReasoningConfig;

#[component]
pub fn ReasoningPage() -> impl IntoView {
    let t = use_translations();
    let config: RwSignal<Option<Result<ReasoningConfig, String>>> = RwSignal::new(None);
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let saving = RwSignal::new(false);

    let reload = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_reasoning_config().await {
                Ok(c) => config.set(Some(Ok(c))),
                Err(e) => config.set(Some(Err(e))),
            }
        });
    };

    reload();

    view! {
        <div class="page-content space-y-6">
            <PageHeader
                title=move || t.reasoning_title()
                description=move || t.reasoning_desc()
            >
                <button
                    on:click=move |_| reload()
                    class="btn btn-secondary text-xs"
                >
                    {t.overview_refresh()}
                </button>
            </PageHeader>

            <Alert variant="info" message=feedback.into() />

            {move || match config.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="config-card glass-card text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(cfg)) => {
                    let thinking_mode = RwSignal::new(cfg.thinking_mode.clone());
                    let reasoning_effort = RwSignal::new(cfg.reasoning_effort.clone());
                    let reasoning_recovery =
                        RwSignal::new(cfg.reasoning_recovery.unwrap_or(false));
                    let sqlite_cache_enabled = RwSignal::new(cfg.sqlite_cache_enabled);
                    let sqlite_cache_path = RwSignal::new(cfg.sqlite_cache_path.clone().unwrap_or_default());

                    let sqlite_enabled = RwSignal::new(cfg.sqlite_cache_enabled);

                    let on_save = {
                        let save_ok = t.routing_saved().to_string();
                        move |_| {
                            saving.set(true);
                            let mut req = cfg.clone();
                            req.thinking_mode = thinking_mode.get();
                            req.reasoning_effort = reasoning_effort.get();
                            req.reasoning_recovery = Some(reasoning_recovery.get());
                            req.sqlite_cache_enabled = sqlite_cache_enabled.get();
                            req.sqlite_cache_path = if sqlite_cache_enabled.get() {
                                let p = sqlite_cache_path.get();
                                if p.is_empty() { None } else { Some(p) }
                            } else {
                                None
                            };
                            let save_ok = save_ok.clone();
                            leptos::task::spawn_local(async move {
                                match api::update_reasoning_config(&req).await {
                                    Ok(_) => feedback.set(save_ok),
                                    Err(e) => feedback.set(e),
                                }
                                saving.set(false);
                                reload();
                            });
                        }
                    };

                    view! {
                        <section class="config-section space-y-4">
                            <div class="config-card glass-card space-y-4 max-w-xl">
                                <div>
                                    <label class="config-label">{t.reasoning_thinking_mode()}</label>
                                    <select
                                        class="config-input w-full"
                                        prop:value=move || thinking_mode.get()
                                        on:change=move |ev| {
                                            thinking_mode.set(event_target_value(&ev));
                                        }
                                    >
                                        <option value="auto">"Auto"</option>
                                        <option value="enabled">"Enabled"</option>
                                        <option value="disabled">"Disabled"</option>
                                    </select>
                                    <p class="text-xs text-theme-muted mt-1">
                                        "auto: model decides, enabled: force thinking, disabled: no thinking"
                                    </p>
                                </div>

                                <div>
                                    <label class="config-label">{t.reasoning_effort()}</label>
                                    <select
                                        class="config-input w-full"
                                        prop:value=move || reasoning_effort.get()
                                        on:change=move |ev| {
                                            reasoning_effort.set(event_target_value(&ev));
                                        }
                                    >
                                        <option value="low">"Low"</option>
                                        <option value="medium">"Medium"</option>
                                        <option value="high">"High"</option>
                                    </select>
                                </div>

                                <div>
                                    <label class="flex items-center gap-2 cursor-pointer">
                                        <input
                                            type="checkbox"
                                            prop:checked=move || reasoning_recovery.get()
                                            on:change=move |ev| {
                                                reasoning_recovery.set(event_target_checked(&ev));
                                            }
                                        />
                                        <span class="config-label">{t.reasoning_recovery()}</span>
                                    </label>
                                    <p class="text-xs text-theme-muted mt-1 ml-6">
                                        "Restore reasoning content from Cursor if DeepSeek omits it"
                                    </p>
                                </div>

                                <div>
                                    <label class="flex items-center gap-2 cursor-pointer">
                                        <input
                                            type="checkbox"
                                            prop:checked=move || sqlite_cache_enabled.get()
                                            on:change=move |ev| {
                                                let checked = event_target_checked(&ev);
                                                sqlite_cache_enabled.set(checked);
                                                sqlite_enabled.set(checked);
                                            }
                                        />
                                        <span class="config-label">{t.reasoning_sqlite_cache()}</span>
                                    </label>

                                    {move || if sqlite_enabled.get() {
                                        view! {
                                            <div class="mt-2 ml-6">
                                                <label class="config-label">{t.reasoning_sqlite_path()}</label>
                                                <input
                                                    type="text"
                                                    class="config-input w-full"
                                                    prop:value=move || sqlite_cache_path.get()
                                                    on:input=move |ev| {
                                                        sqlite_cache_path.set(event_target_value(&ev));
                                                    }
                                                    placeholder="/var/lib/crabcache/reasoning.db"
                                                />
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }}
                                </div>

                                <button
                                    class="btn btn-primary text-sm"
                                    disabled=move || saving.get()
                                    on:click=on_save
                                >
                                    {move || if saving.get() { "Saving..." } else { t.routing_save() }}
                                </button>
                            </div>
                        </section>
                    }.into_any()
                }
            }}
        </div>
    }
}
