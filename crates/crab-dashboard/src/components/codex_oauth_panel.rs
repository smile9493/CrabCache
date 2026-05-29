//! Codex OAuth login panel for the upstream Keys tab.
//!
//! Supports two flows:
//! - **Device Code**: headless, shows user_code for manual entry
//! - **PKCE**: browser redirect with auto/manual callback modes

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[component]
pub fn CodexOAuthPanel(profile_id: String) -> impl IntoView {
    let t = use_translations();
    // Tab: 0 = Device Code, 1 = PKCE
    let tab: RwSignal<u8> = RwSignal::new(0);

    view! {
        <div class="border-t border-theme/10 pt-4 mt-4 space-y-3">
            // Tab switcher
            <div class="flex items-center gap-1 rounded-lg bg-theme/5 p-0.5 w-fit">
                <button
                    class=move || {
                        if tab.get() == 0 {
                            "px-3 py-1 text-xs rounded-md bg-theme/10 text-theme font-medium"
                        } else {
                            "px-3 py-1 text-xs rounded-md text-theme-muted hover:text-theme"
                        }
                    }
                    on:click=move |_| tab.set(0)
                >
                    {t.upstream_codex_tab_device()}
                </button>
                <button
                    class=move || {
                        if tab.get() == 1 {
                            "px-3 py-1 text-xs rounded-md bg-theme/10 text-theme font-medium"
                        } else {
                            "px-3 py-1 text-xs rounded-md text-theme-muted hover:text-theme"
                        }
                    }
                    on:click=move |_| tab.set(1)
                >
                    {t.upstream_codex_tab_pkce()}
                </button>
            </div>

            // Tab content
            {move || {
                let pid = profile_id.clone();
                if tab.get() == 0 {
                    view! { <DeviceCodePanel profile_id=pid /> }.into_any()
                } else {
                    view! { <PkcePanel profile_id=pid /> }.into_any()
                }
            }}
        </div>
    }
}

// ─── Device Code Panel ────────────────────────────────────────────────────────

#[component]
fn DeviceCodePanel(profile_id: String) -> impl IntoView {
    let t = use_translations();

    let session_id: RwSignal<Option<String>> = RwSignal::new(None);
    let user_code: RwSignal<Option<String>> = RwSignal::new(None);
    let verify_url: RwSignal<Option<String>> = RwSignal::new(None);
    let poll_interval: RwSignal<u64> = RwSignal::new(5);
    let flow_status: RwSignal<String> = RwSignal::new("idle".to_string());
    let email: RwSignal<Option<String>> = RwSignal::new(None);
    let account_id: RwSignal<Option<String>> = RwSignal::new(None);
    let credential_id: RwSignal<Option<String>> = RwSignal::new(None);
    let error_msg: RwSignal<String> = RwSignal::new(String::new());
    let copied: RwSignal<bool> = RwSignal::new(false);

    let credentials: RwSignal<Vec<CredentialEntry>> = RwSignal::new(Vec::new());
    let creds_loading: RwSignal<bool> = RwSignal::new(false);

    let alive = Arc::new(AtomicBool::new(true));

    let on_start_device = move |_| {
        let pid = profile_id.clone();
        let alive = Arc::clone(&alive);
        flow_status.set("starting".to_string());
        error_msg.set(String::new());
        leptos::task::spawn_local(async move {
            match api::start_codex_device_login(&pid).await {
                Ok(start) => {
                    if !alive.load(Ordering::Relaxed) { return; }
                    session_id.set(Some(start.session_id));
                    user_code.set(Some(start.user_code));
                    verify_url.set(Some(start.verify_url));
                    poll_interval.set(start.poll_interval_secs.max(3));
                    flow_status.set("polling".to_string());
                    copied.set(false);
                }
                Err(e) => {
                    if !alive.load(Ordering::Relaxed) { return; }
                    error_msg.set(e);
                    flow_status.set("failed".to_string());
                }
            }
        });
    };

    let on_copy_code = move |_| {
        if let Some(code) = user_code.get() {
            crate::clipboard::copy_text(&code);
            copied.set(true);
            leptos::task::spawn_local(async move {
                TimeoutFuture::new(2000).await;
                copied.set(false);
            });
        }
    };

    let on_cancel = move |_| {
        let pid = profile_id.clone();
        let sid = session_id.get_untracked();
        leptos::task::spawn_local(async move {
            if let Some(s) = sid {
                let _ = api::cancel_codex_device_login(&pid, &s).await;
            }
        });
        session_id.set(None);
        user_code.set(None);
        verify_url.set(None);
        flow_status.set("idle".to_string());
        error_msg.set(String::new());
    };

    // Poll loop
    let pid_for_poll = profile_id.clone();
    let alive_for_poll = Arc::clone(&alive);
    leptos::task::spawn_local(async move {
        loop {
            if !alive_for_poll.load(Ordering::Relaxed) { break; }
            let status = flow_status.get_untracked();
            if status != "polling" {
                TimeoutFuture::new(500).await;
                continue;
            }
            let sid = session_id.get_untracked();
            let pid = pid_for_poll.clone();
            let alive = Arc::clone(&alive_for_poll);
            if let Some(s) = sid {
                match api::poll_codex_device_login(&pid, &s).await {
                    Ok(status_resp) => {
                        if !alive.load(Ordering::Relaxed) { break; }
                        match status_resp.status.as_str() {
                            "pending" => {
                                let interval = poll_interval.get_untracked();
                                TimeoutFuture::new(interval * 1000).await;
                            }
                            "completed" => {
                                flow_status.set("completed".to_string());
                                if let Some(e) = status_resp.email { email.set(Some(e)); }
                                if let Some(a) = status_resp.account_id { account_id.set(Some(a)); }
                                if let Some(c) = status_resp.credential_id { credential_id.set(Some(c)); }
                                crate::pages::upstream::signal_refresh_key_pool();
                            }
                            "failed" | "expired" => {
                                flow_status.set(status_resp.status.clone());
                                if let Some(e) = status_resp.error { error_msg.set(e); }
                            }
                            _ => {
                                flow_status.set("failed".to_string());
                                error_msg.set(format!("Unknown status: {}", status_resp.status));
                            }
                        }
                    }
                    Err(e) => {
                        if !alive.load(Ordering::Relaxed) { break; }
                        error_msg.set(e);
                        flow_status.set("failed".to_string());
                    }
                }
            } else {
                TimeoutFuture::new(500).await;
            }
        }
    });

    let alive_for_creds = Arc::clone(&alive);
    leptos::task::spawn_local(async move {
        creds_loading.set(true);
        if let Ok(list) = api::list_codex_credentials().await {
            if alive_for_creds.load(Ordering::Relaxed) {
                credentials.set(list.credentials);
            }
        }
        creds_loading.set(false);
    });

    on_cleanup(move || { alive.store(false, Ordering::Relaxed); });

    view! {
        <div class="space-y-3">
            // Idle
            {move || (flow_status.get() == "idle").then(|| view! {
                <button class="btn btn-secondary text-xs" on:click=on_start_device>
                    {t.upstream_codex_oauth_start()}
                </button>
            })}

            // Starting
            {move || (flow_status.get() == "starting").then(|| view! {
                <div class="flex items-center gap-2">
                    <Spinner />
                    <span class="text-xs text-theme-muted">{t.upstream_codex_oauth_starting()}</span>
                </div>
            })}

            // Polling
            {move || (flow_status.get() == "polling").then(|| {
                let code = user_code.get().unwrap_or_default();
                let url = verify_url.get().unwrap_or_default();
                let copied_signal = copied.get();
                view! {
                    <div class="space-y-3">
                        <div class="flex items-center gap-3 p-3 rounded-lg bg-accent/10 border border-accent/20">
                            <span class="font-mono text-2xl font-bold text-accent tracking-widest select-all">
                                {code.clone()}
                            </span>
                            <button class="btn btn-secondary text-xs" on:click=on_copy_code>
                                {if copied_signal { t.upstream_codex_oauth_copied() } else { t.upstream_codex_oauth_copy() }}
                            </button>
                        </div>
                        <a href=url.clone() target="_blank" class="text-xs text-accent underline">
                            {t.upstream_codex_oauth_verify()}
                        </a>
                        <div class="flex items-center gap-2">
                            <Spinner />
                            <span class="text-xs text-theme-muted">{t.upstream_codex_oauth_polling()}</span>
                        </div>
                        <button class="btn btn-secondary text-xs" on:click=on_cancel>
                            {t.upstream_codex_oauth_cancel()}
                        </button>
                    </div>
                }
            })}

            // Completed
            {move || (flow_status.get() == "completed").then(|| {
                let em = email.get();
                let acc = account_id.get();
                view! {
                    <div class="space-y-2">
                        <div class="flex items-center gap-2">
                            <span class="badge badge-success text-xs">{t.upstream_codex_oauth_success()}</span>
                        </div>
                        <div class="text-xs text-theme-muted space-y-1">
                            {em.map(|e| view! { <p>{t.upstream_codex_oauth_email()}: {e}</p> })}
                            {acc.map(|a| view! { <p>{t.upstream_codex_oauth_account()}: {a}</p> })}
                        </div>
                        <p class="text-xs text-accent">{t.upstream_codex_oauth_imported()}</p>
                        <button class="btn btn-secondary text-xs" on:click=move |_| {
                            session_id.set(None); user_code.set(None); flow_status.set("idle".to_string());
                            email.set(None); account_id.set(None); credential_id.set(None); error_msg.set(String::new());
                        }>{t.upstream_codex_oauth_start()}</button>
                    </div>
                }
            })}

            // Failed / Expired
            {move || {
                let status = flow_status.get();
                (status == "failed" || status == "expired").then(|| {
                    let err = error_msg.get();
                    let label = if status == "expired" { t.upstream_codex_oauth_expired() } else { t.upstream_codex_oauth_failed() };
                    view! {
                        <div class="space-y-2">
                            <span class="badge text-xs text-error">{label}</span>
                            {(!err.is_empty()).then(|| view! { <p class="text-xs text-error">{err}</p> })}
                            <button class="btn btn-secondary text-xs" on:click=move |_| {
                                flow_status.set("idle".to_string()); error_msg.set(String::new());
                            }>{t.upstream_codex_oauth_retry()}</button>
                        </div>
                    }
                })
            }}

            // Saved credentials
            <CredentialList profile_id=profile_id.clone() credentials=credentials loading=creds_loading error_msg=error_msg />
        </div>
    }
}

// ─── PKCE Panel ───────────────────────────────────────────────────────────────

#[component]
fn PkcePanel(profile_id: String) -> impl IntoView {
    let t = use_translations();

    let session_id: RwSignal<Option<String>> = RwSignal::new(None);
    let auth_url: RwSignal<Option<String>> = RwSignal::new(None);
    let pkce_mode: RwSignal<String> = RwSignal::new(String::new()); // "auto" | "manual"
    let flow_status: RwSignal<String> = RwSignal::new("idle".to_string());
    let callback_input: RwSignal<String> = RwSignal::new(String::new());
    let email: RwSignal<Option<String>> = RwSignal::new(None);
    let account_id: RwSignal<Option<String>> = RwSignal::new(None);
    let credential_id: RwSignal<Option<String>> = RwSignal::new(None);
    let error_msg: RwSignal<String> = RwSignal::new(String::new());
    let copied: RwSignal<bool> = RwSignal::new(false);

    let credentials: RwSignal<Vec<CredentialEntry>> = RwSignal::new(Vec::new());
    let creds_loading: RwSignal<bool> = RwSignal::new(false);

    let alive = Arc::new(AtomicBool::new(true));

    // Start PKCE
    let on_start_pkce = move |_| {
        let pid = profile_id.clone();
        let alive = Arc::clone(&alive);
        flow_status.set("starting".to_string());
        error_msg.set(String::new());
        leptos::task::spawn_local(async move {
            match api::start_codex_pkce_login(&pid).await {
                Ok(resp) => {
                    if !alive.load(Ordering::Relaxed) { return; }
                    session_id.set(Some(resp.session_id));
                    auth_url.set(Some(resp.auth_url));
                    pkce_mode.set(resp.mode.clone());
                    if resp.mode == "auto" {
                        flow_status.set("polling".to_string());
                    } else {
                        flow_status.set("waiting_paste".to_string());
                    }
                    copied.set(false);
                }
                Err(e) => {
                    if !alive.load(Ordering::Relaxed) { return; }
                    error_msg.set(e);
                    flow_status.set("failed".to_string());
                }
            }
        });
    };

    let on_copy_url = move |_| {
        if let Some(url) = auth_url.get() {
            crate::clipboard::copy_text(&url);
            copied.set(true);
            leptos::task::spawn_local(async move {
                TimeoutFuture::new(2000).await;
                copied.set(false);
            });
        }
    };

    // Confirm manual exchange
    let on_confirm = move |_| {
        let pid = profile_id.clone();
        let sid = session_id.get_untracked().unwrap_or_default();
        let url = callback_input.get_untracked();
        if url.is_empty() {
            error_msg.set("Please paste the callback URL".to_string());
            return;
        }
        flow_status.set("exchanging".to_string());
        error_msg.set(String::new());
        leptos::task::spawn_local(async move {
            match api::exchange_codex_pkce(&pid, &sid, &url).await {
                Ok(resp) => {
                    if resp.status == "completed" {
                        flow_status.set("completed".to_string());
                        email.set(resp.email);
                        account_id.set(resp.account_id);
                        credential_id.set(resp.credential_id);
                        crate::pages::upstream::signal_refresh_key_pool();
                    } else {
                        flow_status.set("failed".to_string());
                        error_msg.set(resp.error.unwrap_or_else(|| "Exchange failed".into()));
                    }
                }
                Err(e) => {
                    error_msg.set(e);
                    flow_status.set("failed".to_string());
                }
            }
        });
    };

    let on_cancel_pkce = move |_| {
        let pid = profile_id.clone();
        let sid = session_id.get_untracked();
        leptos::task::spawn_local(async move {
            if let Some(s) = sid {
                let _ = api::cancel_codex_pkce_login(&pid, &s).await;
            }
        });
        session_id.set(None);
        auth_url.set(None);
        flow_status.set("idle".to_string());
        error_msg.set(String::new());
        callback_input.set(String::new());
    };

    // Auto-poll loop (for "auto" mode)
    let pid_for_poll = profile_id.clone();
    let alive_for_poll = Arc::clone(&alive);
    leptos::task::spawn_local(async move {
        loop {
            if !alive_for_poll.load(Ordering::Relaxed) { break; }
            let status = flow_status.get_untracked();
            if status != "polling" {
                TimeoutFuture::new(500).await;
                continue;
            }
            let sid = session_id.get_untracked();
            let pid = pid_for_poll.clone();
            let alive = Arc::clone(&alive_for_poll);
            if let Some(s) = sid {
                match api::poll_codex_pkce_login(&pid, &s).await {
                    Ok(resp) => {
                        if !alive.load(Ordering::Relaxed) { break; }
                        match resp.status.as_str() {
                            "pending" => { TimeoutFuture::new(3000).await; }
                            "completed" => {
                                flow_status.set("completed".to_string());
                                email.set(resp.email);
                                account_id.set(resp.account_id);
                                credential_id.set(resp.credential_id);
                                crate::pages::upstream::signal_refresh_key_pool();
                            }
                            "failed" | "expired" => {
                                flow_status.set(resp.status.clone());
                                error_msg.set(resp.error.unwrap_or_else(|| "Login failed".into()));
                            }
                            _ => { TimeoutFuture::new(3000).await; }
                        }
                    }
                    Err(e) => {
                        if !alive.load(Ordering::Relaxed) { break; }
                        error_msg.set(e);
                        flow_status.set("failed".to_string());
                    }
                }
            } else {
                TimeoutFuture::new(500).await;
            }
        }
    });

    // Load credentials
    let alive_for_creds = Arc::clone(&alive);
    leptos::task::spawn_local(async move {
        creds_loading.set(true);
        if let Ok(list) = api::list_codex_credentials().await {
            if alive_for_creds.load(Ordering::Relaxed) {
                credentials.set(list.credentials);
            }
        }
        creds_loading.set(false);
    });

    on_cleanup(move || { alive.store(false, Ordering::Relaxed); });

    view! {
        <div class="space-y-3">
            // Idle: start button
            {move || (flow_status.get() == "idle").then(|| view! {
                <div class="space-y-2">
                    <button class="btn btn-secondary text-xs" on:click=on_start_pkce>
                        {t.upstream_codex_pkce_start()}
                    </button>
                </div>
            })}

            // Starting
            {move || (flow_status.get() == "starting").then(|| view! {
                <div class="flex items-center gap-2">
                    <Spinner />
                    <span class="text-xs text-theme-muted">{t.upstream_codex_oauth_starting()}</span>
                </div>
            })}

            // Waiting for paste (manual mode)
            {move || (flow_status.get() == "waiting_paste").then(|| {
                let url = auth_url.get().unwrap_or_default();
                let c = copied.get();
                view! {
                    <div class="space-y-3">
                        // Auth URL
                        <div class="flex items-center gap-2">
                            <a href=url.clone() target="_blank"
                               class="btn btn-secondary text-xs">
                                {t.upstream_codex_pkce_open_browser()}
                            </a>
                            <button class="btn btn-secondary text-xs" on:click=on_copy_url>
                                {if c { t.upstream_codex_oauth_copied() } else { t.upstream_codex_oauth_copy() }}
                            </button>
                        </div>
                        <p class="text-xs text-theme-muted">{t.upstream_codex_pkce_manual_hint()}</p>
                        // Callback URL input
                        <div class="flex items-center gap-2">
                            <input
                                type="text"
                                class="input input-bordered input-xs flex-1 text-xs font-mono"
                                placeholder="http://localhost:1455/auth/callback?code=...&state=..."
                                prop:value=callback_input
                                on:input=move |ev| callback_input.set(event_target_value(&ev))
                            />
                            <button class="btn btn-primary text-xs" on:click=on_confirm>
                                {t.upstream_codex_pkce_confirm()}
                            </button>
                        </div>
                        <button class="btn btn-secondary text-xs" on:click=on_cancel_pkce>
                            {t.upstream_codex_oauth_cancel()}
                        </button>
                    </div>
                }
            })}

            // Exchanging (manual mode confirmed)
            {move || (flow_status.get() == "exchanging").then(|| view! {
                <div class="flex items-center gap-2">
                    <Spinner />
                    <span class="text-xs text-theme-muted">"Exchanging code for token..."</span>
                </div>
            })}

            // Polling (auto mode — waiting for callback)
            {move || (flow_status.get() == "polling").then(|| view! {
                <div class="space-y-2">
                    <div class="flex items-center gap-2">
                        <Spinner />
                        <span class="text-xs text-theme-muted">{t.upstream_codex_pkce_waiting()}</span>
                    </div>
                    <button class="btn btn-secondary text-xs" on:click=on_cancel_pkce>
                        {t.upstream_codex_oauth_cancel()}
                    </button>
                </div>
            })}

            // Completed
            {move || (flow_status.get() == "completed").then(|| {
                let em = email.get();
                let acc = account_id.get();
                view! {
                    <div class="space-y-2">
                        <span class="badge badge-success text-xs">{t.upstream_codex_oauth_success()}</span>
                        <div class="text-xs text-theme-muted space-y-1">
                            {em.map(|e| view! { <p>{t.upstream_codex_oauth_email()}: {e}</p> })}
                            {acc.map(|a| view! { <p>{t.upstream_codex_oauth_account()}: {a}</p> })}
                        </div>
                        <p class="text-xs text-accent">{t.upstream_codex_oauth_imported()}</p>
                        <button class="btn btn-secondary text-xs" on:click=move |_| {
                            session_id.set(None); auth_url.set(None); flow_status.set("idle".to_string());
                            email.set(None); account_id.set(None); credential_id.set(None);
                            error_msg.set(String::new()); callback_input.set(String::new());
                        }>{t.upstream_codex_pkce_start()}</button>
                    </div>
                }
            })}

            // Failed / Expired
            {move || {
                let status = flow_status.get();
                (status == "failed" || status == "expired").then(|| {
                    let err = error_msg.get();
                    let label = if status == "expired" { t.upstream_codex_oauth_expired() } else { t.upstream_codex_oauth_failed() };
                    view! {
                        <div class="space-y-2">
                            <span class="badge text-xs text-error">{label}</span>
                            {(!err.is_empty()).then(|| view! { <p class="text-xs text-error">{err}</p> })}
                            <button class="btn btn-secondary text-xs" on:click=move |_| {
                                flow_status.set("idle".to_string()); error_msg.set(String::new());
                            }>{t.upstream_codex_oauth_retry()}</button>
                        </div>
                    }
                })
            }}

            // Saved credentials
            <CredentialList profile_id=profile_id.clone() credentials=credentials loading=creds_loading error_msg=error_msg />
        </div>
    }
}

// ─── Shared Credential List ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CredentialEntry {
    pub id: String,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub expired_at: Option<String>,
    pub disabled: bool,
}

#[component]
fn CredentialList(
    profile_id: String,
    credentials: RwSignal<Vec<CredentialEntry>>,
    loading: RwSignal<bool>,
    error_msg: RwSignal<String>,
) -> impl IntoView {
    let t = use_translations();
    let pid = profile_id.clone();

    view! {
        <div class="border-t border-theme/10 pt-3 mt-3">
            <div class="flex items-center justify-between mb-2">
                <h5 class="text-xs font-semibold text-theme">{t.upstream_codex_oauth_credentials()}</h5>
                {move || loading.get().then(|| view! { <Spinner /> })}
            </div>
            {move || {
                let creds = credentials.get();
                if creds.is_empty() && !loading.get() {
                    view! { <p class="text-xs text-theme-muted">{t.upstream_codex_oauth_no_credentials()}</p> }.into_any()
                } else if !creds.is_empty() {
                    view! {
                        <div class="space-y-1 max-h-48 overflow-y-auto">
                            {creds.into_iter().map(|c| {
                                let cid = c.id.clone();
                                let label = c.email.clone().unwrap_or_else(|| cid.clone());
                                let disabled = c.disabled;
                                let import_pid = pid.clone();
                                view! {
                                    <div class="flex items-center justify-between py-1 px-2 rounded hover:bg-theme/5 text-xs">
                                        <span class="font-mono text-theme-muted truncate max-w-[200px]">{label}</span>
                                        <button class="btn btn-secondary text-xs ml-2 shrink-0"
                                            disabled=disabled
                                            on:click=move |_| {
                                                let id = cid.clone();
                                                let p = import_pid.clone();
                                                leptos::task::spawn_local(async move {
                                                    if let Err(e) = api::import_codex_credential(&p, &id).await {
                                                        error_msg.set(e);
                                                    } else {
                                                        crate::pages::upstream::signal_refresh_key_pool();
                                                    }
                                                });
                                            }
                                        >
                                            {t.upstream_codex_oauth_import()}
                                        </button>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                } else {
                    view! {}.into_any()
                }
            }}
        </div>
    }
}
