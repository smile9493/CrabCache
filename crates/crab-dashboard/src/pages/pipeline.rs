use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::PipelineRuntimeConfig;

#[component]
pub fn PipelinePage() -> impl IntoView {
    let t = use_translations();
    let config: RwSignal<Option<Result<PipelineRuntimeConfig, String>>> = RwSignal::new(None);
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let saving = RwSignal::new(false);

    let reload = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_pipeline_runtime().await {
                Ok(c) => config.set(Some(Ok(c))),
                Err(e) => config.set(Some(Err(e))),
            }
        });
    };

    reload();

    view! {
        <div class="space-y-4">
            <div class="flex items-center justify-end">
                <button
                    on:click=move |_| reload()
                    class="btn btn-secondary text-xs"
                >
                    {t.overview_refresh()}
                </button>
            </div>

            <Alert variant="info" message=feedback.into() />

            {move || match config.get() {
                None => view! { <crate::components::skeleton::SkeletonTable rows=5 cols=4 /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="config-card glass-card text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(cfg)) => {
                    let mode = RwSignal::new(cfg.pipeline_mode.clone());
                    let default_profile = RwSignal::new(cfg.default_upstream_profile.clone());
                    let profiles = cfg.profiles.clone();
                    let profiles_for_save = profiles.clone();
                    let on_save = {
                        let save_ok = t.pipeline_save_ok().to_string();
                        move |_| {
                            saving.set(true);
                            let req = PipelineRuntimeConfig {
                                pipeline_mode: mode.get(),
                                default_upstream_profile: default_profile.get(),
                                profiles: profiles_for_save.clone(),
                            };
                            let save_ok = save_ok.clone();
                            leptos::task::spawn_local(async move {
                                match api::update_pipeline_runtime(&req).await {
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
                                    <label class="config-label">{t.pipeline_mode_label()}</label>
                                    <select
                                        class="config-input w-full"
                                        prop:value=move || mode.get()
                                        on:change=move |ev| {
                                            mode.set(event_target_value(&ev));
                                        }
                                    >
                                        <option value="auto">{t.pipeline_mode_auto()}</option>
                                        <option value="force_cursor_v4">{t.pipeline_mode_force()}</option>
                                    </select>
                                    <p class="text-xs text-theme-muted mt-1">{t.pipeline_mode_hint()}</p>
                                </div>
                                <div>
                                    <label class="config-label">{t.pipeline_default_profile()}</label>
                                    <select
                                        class="config-input w-full"
                                        prop:value=move || default_profile.get()
                                        on:change=move |ev| {
                                            default_profile.set(event_target_value(&ev));
                                        }
                                    >
                                        {profiles.iter().map(|p| {
                                            let id = p.id.clone();
                                            let label = format!("{} ({})", id, p.provider);
                                            view! {
                                                <option value=id.clone()>{label}</option>
                                            }
                                        }).collect_view()}
                                    </select>
                                </div>
                                <button
                                    class="btn btn-primary text-sm"
                                    disabled=move || saving.get()
                                    on:click=on_save
                                >
                                    {move || if saving.get() { t.pipeline_saving() } else { t.pipeline_save() }}
                                </button>
                            </div>
                            <div class="config-card glass-card">
                                <h3 class="text-sm font-semibold text-theme mb-3">{t.pipeline_profiles_title()}</h3>
                                <table class="data-table text-sm w-full">
                                    <thead>
                                        <tr>
                                            <th>"ID"</th>
                                            <th>{t.pipeline_provider_col()}</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {profiles.iter().map(|p| {
                                            let id = p.id.clone();
                                            let provider = p.provider.clone();
                                            view! {
                                                <tr>
                                                    <td class="font-mono">{id}</td>
                                                    <td>{provider}</td>
                                                </tr>
                                            }
                                        }).collect_view()}
                                    </tbody>
                                </table>
                                <p class="text-xs text-theme-muted mt-3">{t.pipeline_profiles_hint()}</p>
                            </div>
                        </section>
                    }.into_any()
                }
            }}
        </div>
    }
}
