use leptos::prelude::*;

use crate::api;
use crate::auth::{complete_login, use_admin_key};
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::pages::pipeline::PipelinePage;
use crate::pages::reasoning::ReasoningPage;
use crate::types::{SystemUpdateResult, SystemVersion, UpdateCheckResult};

#[component]
pub fn SystemPage() -> impl IntoView {
    let t = use_translations();
    let active_tab: RwSignal<usize> = RwSignal::new(0);

    let tab_labels = vec![
        t.tab_general().to_string(),
        t.tab_pipeline().to_string(),
        t.tab_reasoning().to_string(),
    ];

    view! {
        <div class="page-content space-y-4">
            <SectionHeader title=t.system_title() description=t.system_desc() />
            <TabBar tabs=tab_labels active=active_tab />
            {move || match active_tab.get() {
                0 => view! { <GeneralTab /> }.into_any(),
                1 => view! { <PipelinePage /> }.into_any(),
                _ => view! { <ReasoningPage /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn GeneralTab() -> impl IntoView {
    let t = use_translations();
    let admin_key_signal = use_admin_key();

    // Version & Update state
    let version: RwSignal<Option<Result<SystemVersion, String>>> = RwSignal::new(None);
    let update_check: RwSignal<Option<Result<UpdateCheckResult, String>>> = RwSignal::new(None);
    let update_result: RwSignal<Option<Result<SystemUpdateResult, String>>> = RwSignal::new(None);
    let checking = RwSignal::new(false);
    let updating = RwSignal::new(false);
    let show_update_confirm = RwSignal::new(false);

    // Admin key state
    let key_message: RwSignal<Option<String>> = RwSignal::new(None);
    let key_error = RwSignal::new(false);
    let current_key: RwSignal<String> = RwSignal::new(String::new());
    let new_key: RwSignal<String> = RwSignal::new(String::new());
    let confirm_key: RwSignal<String> = RwSignal::new(String::new());

    let load_version = move || {
        leptos::task::spawn_local(async move {
            version.set(None);
            match api::fetch_system_version().await {
                Ok(v) => version.set(Some(Ok(v))),
                Err(e) => version.set(Some(Err(e))),
            }
        });
    };

    let do_check_updates = move || {
        checking.set(true);
        update_check.set(None);
        leptos::task::spawn_local(async move {
            match api::check_for_updates().await {
                Ok(r) => update_check.set(Some(Ok(r))),
                Err(e) => update_check.set(Some(Err(e))),
            }
            checking.set(false);
        });
    };

    let do_update = move || {
        show_update_confirm.set(false);
        updating.set(true);
        update_result.set(None);
        leptos::task::spawn_local(async move {
            match api::trigger_system_update().await {
                Ok(r) => update_result.set(Some(Ok(r))),
                Err(e) => update_result.set(Some(Err(e))),
            }
            updating.set(false);
        });
    };

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
                    let success = val
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if success {
                        key_message.set(Some(t.system_key_changed().to_string()));
                        key_error.set(false);
                        new_key.set(String::new());
                        confirm_key.set(String::new());
                        current_key.set(String::new());
                        let _ = complete_login(&new_clone);
                        admin_key_signal.set(new_clone);
                    } else {
                        let err = val
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or(t.system_key_change_failed())
                            .to_string();
                        key_message.set(Some(err));
                        key_error.set(true);
                    }
                }
                Err(e) => {
                    key_message.set(Some(e));
                    key_error.set(true);
                }
            }
        });
    };

    load_version();

    view! {
        <div class="space-y-6">
            // Version & Update
            <div class="card">
                <div class="card-header">{t.system_version_title()}</div>
                <div class="card-body space-y-3">
                    <div class="flex flex-col gap-2">
                        <div class="flex items-center gap-2">
                            <span class="text-sm font-medium">{t.system_current_version()}:</span>
                            <code class="text-sm px-2 py-0.5 rounded bg-muted">
                                {move || {
                                    version.get()
                                        .and_then(|r| r.ok())
                                        .map(|v| v.current_version)
                                        .unwrap_or_else(|| "—".to_string())
                                }}
                            </code>
                        </div>

                        <div class="flex items-center gap-2">
                            <span class="text-sm font-medium">{t.system_latest_version()}:</span>
                            <code class="text-sm px-2 py-0.5 rounded bg-muted">
                                {move || {
                                    update_check.get()
                                        .and_then(|r| r.ok())
                                        .map(|uc| uc.latest_version)
                                        .or_else(|| {
                                            version.get()
                                                .and_then(|r| r.ok())
                                                .and_then(|v| v.latest)
                                                .map(|rel| rel.tag_name.trim_start_matches('v').to_string())
                                        })
                                        .unwrap_or_else(|| "—".to_string())
                                }}
                            </code>
                            {move || {
                                update_check.get()
                                    .and_then(|r| r.ok())
                                    .map(|uc| {
                                        if uc.update_available {
                                            view! {
                                                <span class="text-xs px-2 py-0.5 rounded-full bg-yellow-500/20 text-yellow-600">
                                                    {t.system_update_available()}
                                                </span>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <span class="text-xs px-2 py-0.5 rounded-full bg-green-500/20 text-green-600">
                                                    {t.system_up_to_date()}
                                                </span>
                                            }.into_any()
                                        }
                                    })
                                    .unwrap_or_else(|| ().into_any())
                            }}
                        </div>

                        {move || {
                            version.get()
                                .and_then(|r| r.ok())
                                .and_then(|v| v.latest)
                                .map(|rel| {
                                    view! {
                                        <div class="flex items-center gap-2 text-xs text-muted-foreground">
                                            <span>{t.system_published()}:</span>
                                            <span>{rel.published_at.unwrap_or_else(|| "—".to_string())}</span>
                                        </div>
                                    }.into_any()
                                })
                                .unwrap_or_else(|| ().into_any())
                        }}
                    </div>

                    {move || checking.get().then(|| {
                        view! {
                            <div class="flex items-center gap-2 text-sm text-muted-foreground">
                                <Spinner />
                                <span>{t.system_checking()}</span>
                            </div>
                        }.into_any()
                    })}

                    <div class="flex gap-2">
                        <button class="btn btn-secondary" on:click=move |_| do_check_updates() disabled=move || checking.get()>
                            {t.system_check_updates()}
                        </button>
                        {move || {
                            let available = update_check.get()
                                .and_then(|r| r.ok())
                                .map(|uc| uc.update_available)
                                .unwrap_or(false);
                            available.then(|| {
                                view! {
                                    <button class="btn btn-primary" on:click=move |_| show_update_confirm.set(true) disabled=move || updating.get()>
                                        {move || if updating.get() { format!("{}...", t.system_update_now()) } else { t.system_update_now().to_string() }}
                                    </button>
                                }.into_any()
                            })
                        }}
                    </div>

                    {move || update_result.get().map(|res| {
                        match res {
                            Ok(r) => {
                                let cls = if r.success { "alert alert-success" } else { "alert alert-danger" };
                                let msg = r.message.as_deref().unwrap_or(r.error.as_deref().unwrap_or("Unknown result"));
                                view! { <div class=cls>{msg.to_string()}</div> }.into_any()
                            }
                            Err(e) => view! { <div class="alert alert-danger">{e}</div> }.into_any(),
                        }
                    })}

                    {move || version.get().and_then(|r| r.err()).map(|e| {
                        view! { <div class="alert alert-info">{t.system_no_release()}: {e}</div> }.into_any()
                    })}
                </div>
            </div>

            // Admin Key Management
            <div class="card">
                <div class="card-header">{t.system_admin_key_title()}</div>
                <div class="card-body space-y-3">
                    <p class="text-sm text-muted-foreground">{t.system_admin_key_desc()}</p>
                    <div class="flex flex-col gap-2 max-w-sm">
                        <label class="text-sm font-medium">{t.system_current_key()}</label>
                        <input type="password" class="input" placeholder="••••••••"
                            on:input=move |ev| current_key.set(event_target_value(&ev))
                            prop:value=move || current_key.get()
                        />
                    </div>
                    <div class="flex flex-col gap-2 max-w-sm">
                        <label class="text-sm font-medium">{t.system_new_key()}</label>
                        <input type="password" class="input" placeholder={t.system_new_key().to_string()}
                            on:input=move |ev| new_key.set(event_target_value(&ev))
                            prop:value=move || new_key.get()
                        />
                    </div>
                    <div class="flex flex-col gap-2 max-w-sm">
                        <label class="text-sm font-medium">{t.system_confirm_key()}</label>
                        <input type="password" class="input" placeholder={t.system_confirm_key().to_string()}
                            on:input=move |ev| confirm_key.set(event_target_value(&ev))
                            prop:value=move || confirm_key.get()
                        />
                    </div>
                    <button class="btn btn-primary" on:click=move |_| do_change_key()>
                        {t.system_change_key()}
                    </button>
                    {move || {
                        key_message
                            .get()
                            .map(|msg| {
                                let cls = if key_error.get() {
                                    "alert alert-danger"
                                } else {
                                    "alert alert-success"
                                };
                                view! { <div class=cls>{msg}</div> }.into_any()
                            })
                            .unwrap_or_else(|| ().into_any())
                    }}
                </div>
            </div>

            // Confirm Update Dialog
            {move || {
                if show_update_confirm.get() {
                    view! {
                        <div class="modal-overlay" on:click=move |_| show_update_confirm.set(false)>
                            <div class="modal-box" on:click=move |ev| { ev.stop_propagation(); }>
                                <div class="modal-title">{t.system_update_now()}</div>
                                <p class="text-sm text-muted mb-4">{t.system_update_confirm()}</p>
                                <div class="flex gap-2 justify-end">
                                    <button class="btn btn-secondary" on:click=move |_| show_update_confirm.set(false)>Cancel</button>
                                    <button class="btn btn-primary" on:click=move |_| do_update()>{t.system_update_now()}</button>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}

            {move || {
                if updating.get() {
                    view! {
                        <div class="modal-overlay">
                            <div class="modal-box text-center">
                                <Spinner />
                                <p class="mt-3 text-sm text-muted">Updating, please wait...</p>
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
