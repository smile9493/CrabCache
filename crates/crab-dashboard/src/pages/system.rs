use leptos::prelude::*;

use crate::api;
use crate::auth::{complete_login, use_admin_key};
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::pages::design_system::DesignSystemPage;
use crate::pages::features::FeaturesGrid;
use crate::pages::pipeline::PipelinePage;
use crate::pages::reasoning::ReasoningPage;
use crate::types::{LimitsConfig, SystemUpdateResult, SystemVersion, UpdateCheckResult};

#[component]
pub fn SystemPage() -> impl IntoView {
    let t = use_translations();

    view! {
        <div class="page-content space-y-8">
            <SectionHeader title=t.system_title() description=t.system_desc() />

            // ── Section 1: Core ──
            <section class="config-section">
                <h4 class="config-section-title">{t.section_core()}</h4>
                <div class="cache-card-grid">
                    <VersionCard />
                    <AdminKeyCard />
                </div>
            </section>

            // ── Section 2: Data Plane ──
            <section class="config-section">
                <h4 class="config-section-title">{t.section_data()}</h4>
                <div class="cache-card-grid">
                    <PipelinePage />
                    <ReasoningPage />
                </div>
            </section>

            // ── Section 3: Experimental Features ──
            <section class="config-section">
                <h4 class="config-section-title">{t.section_features()}</h4>
                <div class="cache-card-grid">
                    <FeaturesGrid />
                </div>
            </section>

            // ── Section 4: Limits ──
            <section class="config-section">
                <h4 class="config-section-title">{t.section_limits()}</h4>
                <div class="cache-card-grid">
                    <LimitsCard />
                </div>
            </section>

            // ── Section 5: Design System ──
            <section class="config-section">
                <h4 class="config-section-title">{t.section_design()}</h4>
                <div class="cache-card-grid">
                    <DesignSystemPage />
                </div>
            </section>
        </div>
    }
}

#[component]
fn VersionCard() -> impl IntoView {
    let t = use_translations();

    let version: RwSignal<Option<Result<SystemVersion, String>>> = RwSignal::new(None);
    let update_check: RwSignal<Option<Result<UpdateCheckResult, String>>> = RwSignal::new(None);
    let update_result: RwSignal<Option<Result<SystemUpdateResult, String>>> = RwSignal::new(None);
    let checking = RwSignal::new(false);
    let updating = RwSignal::new(false);
    let show_update_confirm = RwSignal::new(false);

    let load_version = move || {
        leptos::task::spawn_local(async move {
            version.try_set(None);
            match api::fetch_system_version().await {
                Ok(v) => { version.try_set(Some(Ok(v))); }
                Err(e) => { version.try_set(Some(Err(e))); }
            }
        });
    };

    let do_check_updates = move || {
        checking.set(true);
        update_check.set(None);
        leptos::task::spawn_local(async move {
            match api::check_for_updates().await {
                Ok(r) => { update_check.try_set(Some(Ok(r))); }
                Err(e) => { update_check.try_set(Some(Err(e))); }
            }
            checking.try_set(false);
        });
    };

    let do_update = move || {
        show_update_confirm.set(false);
        updating.set(true);
        update_result.set(None);
        leptos::task::spawn_local(async move {
            match api::trigger_system_update().await {
                Ok(r) => { update_result.try_set(Some(Ok(r))); }
                Err(e) => { update_result.try_set(Some(Err(e))); }
            }
            updating.try_set(false);
        });
    };

    load_version();

    let current_version_str = Signal::derive(move || {
        version.get().and_then(|r| r.ok()).map(|v| v.current_version).unwrap_or_else(|| "—".to_string())
    });
    let latest_version_str = Signal::derive(move || {
        update_check.get().and_then(|r| r.ok()).map(|uc| uc.latest_version)
            .or_else(|| version.get().and_then(|r| r.ok()).and_then(|v| v.latest).map(|rel| rel.tag_name.trim_start_matches('v').to_string()))
            .unwrap_or_else(|| "—".to_string())
    });
    let update_available = Signal::derive(move || {
        update_check.get().and_then(|r| r.ok()).map(|uc| uc.update_available).unwrap_or(false)
    });
    let checking_label = Signal::derive(move || {
        if checking.get() { format!("{}...", t.system_checking()) } else { t.system_check_updates().to_string() }
    });
    let updating_label = Signal::derive(move || {
        if updating.get() { format!("{}...", t.system_update_now()) } else { t.system_update_now().to_string() }
    });
    let inline_confirm_style = "background: color-mix(in srgb, var(--cc-warning) 10%, var(--cc-bg-card)); border: 1px solid color-mix(in srgb, var(--cc-warning) 30%, var(--cc-border));";

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h3 class="config-card-title">{t.system_version_title()}</h3>
            </div>
            <div class="config-card-body space-y-3">
                <div class="flex items-center gap-2">
                    <span class="text-sm font-medium">{t.system_current_version()}:</span>
                    <code class="text-sm px-2 py-0.5 rounded" style="background: var(--cc-bg-elevated);">{current_version_str}</code>
                </div>
                <div class="flex items-center gap-2 flex-wrap">
                    <span class="text-sm font-medium">{t.system_latest_version()}:</span>
                    <code class="text-sm px-2 py-0.5 rounded" style="background: var(--cc-bg-elevated);">{latest_version_str}</code>
                    {move || update_check.get().and_then(|r| r.ok()).map(|uc| {
                        if uc.update_available {
                            view! { <Badge text=t.system_update_available().to_string() color="warning" /> }.into_any()
                        } else {
                            view! { <Badge text=t.system_up_to_date().to_string() color="success" /> }.into_any()
                        }
                    })}
                </div>
                {move || version.get().and_then(|r| r.ok()).and_then(|v| v.latest).map(|rel| {
                    let pub_at = rel.published_at.unwrap_or_else(|| "—".to_string());
                    view! { <p class="text-xs text-theme-muted">{t.system_published()} ": " {pub_at}</p> }.into_any()
                })}
                <div class="flex gap-2 flex-wrap">
                    <button class="btn btn-secondary btn-sm" on:click=move |_| do_check_updates() disabled=move || checking.get()>
                        {checking_label}
                    </button>
                    {move || {
                        if update_available.get() && !show_update_confirm.get() {
                            view! {
                                <button class="btn btn-primary btn-sm" on:click=move |_| show_update_confirm.set(true) disabled=move || updating.get()>
                                    {updating_label}
                                </button>
                            }.into_any()
                        } else { ().into_any() }
                    }}
                </div>
                {move || {
                    if show_update_confirm.get() {
                        view! {
                            <div class="flex items-center gap-3 p-3 rounded-md" style=inline_confirm_style>
                                <span class="text-sm">{t.system_update_confirm()}</span>
                                <button class="btn btn-ghost btn-sm" on:click=move |_| show_update_confirm.set(false)>{t.system_cancel()}</button>
                                <button class="btn btn-primary btn-sm" on:click=move |_| do_update()>{t.system_update_now()}</button>
                            </div>
                        }.into_any()
                    } else { ().into_any() }
                }}
                {move || {
                    if updating.get() {
                        view! {
                            <div class="flex items-center gap-2 text-sm text-theme-muted">
                                <div class="spinner" style="width: 1rem; height: 1rem;"></div>
                                <span>{t.system_updating()}</span>
                            </div>
                        }.into_any()
                    } else { ().into_any() }
                }}
                {move || update_result.get().map(|res| match res {
                    Ok(r) => {
                        let variant = if r.success { "success" } else { "error" };
                        let msg = r.message.as_deref().unwrap_or(r.error.as_deref().unwrap_or("Unknown result")).to_string();
                        view! { <Alert variant=variant message=Signal::derive(move || msg.clone()) /> }.into_any()
                    }
                    Err(e) => view! { <Alert variant="error" message=Signal::derive(move || e.clone()) /> }.into_any(),
                })}
                {move || version.get().and_then(|r| r.err()).map(|e| {
                    let msg = format!("{}: {}", t.system_no_release(), e);
                    view! { <Alert variant="info" message=Signal::derive(move || msg.clone()) /> }.into_any()
                })}
            </div>
        </div>
    }
}

#[component]
fn AdminKeyCard() -> impl IntoView {
    let t = use_translations();
    let admin_key_signal = use_admin_key();

    let key_message: RwSignal<Option<String>> = RwSignal::new(None);
    let key_error = RwSignal::new(false);
    let current_key: RwSignal<String> = RwSignal::new(String::new());
    let new_key: RwSignal<String> = RwSignal::new(String::new());
    let confirm_key: RwSignal<String> = RwSignal::new(String::new());

    let do_change_key = move || {
        let old = current_key.get();
        let new = new_key.get();
        let confirm = confirm_key.get();

        if new.len() < 4 {
            key_message.set(Some(t.system_key_too_short().to_string()));
            key_error.set(true);
            return;
        }
        if new != confirm {
            key_message.set(Some(t.system_key_mismatch().to_string()));
            key_error.set(true);
            return;
        }

        let new_clone = new.clone();
        key_message.set(None);
        leptos::task::spawn_local(async move {
            match api::change_admin_key(&old, &new_clone).await {
                Ok(val) => {
                    let success = val.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    if success {
                        key_message.try_set(Some(t.system_key_changed().to_string()));
                        key_error.try_set(false);
                        new_key.try_set(String::new());
                        confirm_key.try_set(String::new());
                        current_key.try_set(String::new());
                        let _ = complete_login(&new_clone);
                        admin_key_signal.try_set(new_clone);
                    } else {
                        let err = val.get("error").and_then(|v| v.as_str()).unwrap_or(t.system_key_change_failed()).to_string();
                        key_message.try_set(Some(err));
                        key_error.try_set(true);
                    }
                }
                Err(e) => {
                    key_message.try_set(Some(e));
                    key_error.try_set(true);
                }
            }
        });
    };

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h3 class="config-card-title">{t.system_admin_key_title()}</h3>
                <p class="config-card-desc">{t.system_admin_key_desc()}</p>
            </div>
            <div class="config-card-body space-y-3">
                <div class="flex flex-col gap-1.5">
                    <label class="text-xs font-medium text-theme-muted">{t.system_current_key()}</label>
                    <input type="password" class="input" placeholder="••••••••"
                        on:input=move |ev| current_key.set(event_target_value(&ev))
                        prop:value=move || current_key.get()
                    />
                </div>
                <div class="flex flex-col gap-1.5">
                    <label class="text-xs font-medium text-theme-muted">{t.system_new_key()}</label>
                    <input type="password" class="input" placeholder=t.system_new_key()
                        on:input=move |ev| new_key.set(event_target_value(&ev))
                        prop:value=move || new_key.get()
                    />
                </div>
                <div class="flex flex-col gap-1.5">
                    <label class="text-xs font-medium text-theme-muted">{t.system_confirm_key()}</label>
                    <input type="password" class="input" placeholder=t.system_confirm_key()
                        on:input=move |ev| confirm_key.set(event_target_value(&ev))
                        prop:value=move || confirm_key.get()
                    />
                </div>
                <button class="btn btn-primary btn-sm" on:click=move |_| do_change_key()>
                    {t.system_change_key()}
                </button>
                {move || {
                    key_message.get().map(|msg| {
                        let variant = if key_error.get() { "error" } else { "success" };
                        view! { <Alert variant=variant message=Signal::derive(move || msg.clone()) /> }.into_any()
                    })
                }}
            </div>
        </div>
    }
}

#[component]
fn LimitsCard() -> impl IntoView {
    let t = use_translations();
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let loaded = RwSignal::new(false);
    let cfg: RwSignal<Option<LimitsConfig>> = RwSignal::new(None);

    let max_body = RwSignal::new(1_048_576u64);
    let max_conc = RwSignal::new(512u64);
    let legacy_auth = RwSignal::new(false);
    let cors = RwSignal::new(false);

    let load = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_limits_config().await {
                Ok(c) => {
                    max_body.set(c.max_request_body_bytes as u64);
                    max_conc.set(c.max_concurrent_requests as u64);
                    legacy_auth.set(c.legacy_api_key_as_client_auth);
                    cors.set(c.cors_enabled);
                    cfg.set(Some(c));
                    loaded.set(true);
                }
                Err(e) => feedback.set(e),
            }
        });
    };

    let save = move |_| {
        leptos::task::spawn_local(async move {
            let req = LimitsConfig {
                max_request_body_bytes: max_body.get() as usize,
                max_concurrent_requests: max_conc.get() as usize,
                legacy_api_key_as_client_auth: legacy_auth.get(),
                cors_enabled: cors.get(),
            };
            match api::update_limits_config(&req).await {
                Ok(c) => {
                    cfg.set(Some(c));
                    feedback.set(t.routing_saved().to_string());
                }
                Err(e) => feedback.set(e),
            }
        });
    };

    load();

    let body_label = Signal::derive(move || format!("{}: {}", "Max Request Body", max_body.get()));
    let feedback_msg = Signal::derive(move || feedback.get());

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h3 class="config-card-title">{t.system_limits_title()}</h3>
                <p class="config-card-desc">{t.system_limits_desc()}</p>
            </div>
            <div class="config-card-body">
                {move || {
                    if !loaded.get() {
                        return view! { <p class="text-xs text-theme-muted">Loading...</p> }.into_any();
                    }
                    view! {
                        <div class="space-y-3">
                            <ConfigRangeU64 label=move || body_label.get() value=max_body min=65536 max=10_485_760 min_hint="64KB" max_hint="10MB" accent="accent" />
                            <div class="flex items-center justify-between py-1">
                                <span class="text-xs text-theme-muted">"Max Concurrent Requests"</span>
                                <span class="text-sm font-mono">{move || max_conc.get()}</span>
                            </div>
                            <label class="flex items-center gap-2 text-xs text-theme-muted cursor-pointer select-none">
                                <input type="checkbox" prop:checked=move || legacy_auth.get() on:change=move |ev| legacy_auth.set(event_target_checked(&ev)) />
                                "Legacy API Key as Client Auth"
                            </label>
                            <label class="flex items-center gap-2 text-xs text-theme-muted cursor-pointer select-none">
                                <input type="checkbox" prop:checked=move || cors.get() on:change=move |ev| cors.set(event_target_checked(&ev)) />
                                "CORS Enabled"
                            </label>
                            {move || {
                                let msg = feedback_msg.get();
                                if !msg.is_empty() {
                                    view! { <p class="text-xs" style="color: var(--cc-accent);">{msg}</p> }.into_any()
                                } else { ().into_any() }
                            }}
                            <button on:click=save class="btn btn-primary btn-sm">{t.routing_save()}</button>
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}
