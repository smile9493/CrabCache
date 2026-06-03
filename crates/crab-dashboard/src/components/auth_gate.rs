use leptos::prelude::*;

use crate::api;
use crate::auth::{complete_login, use_admin_key};
use crate::components::brand_logo::BrandLogo;
use crate::components::crab_particles::CrabParticles;
use crate::components::theme_switcher::ThemeSwitcher;
use crate::locale::{Translations, use_translations};
use crate::theme::use_theme_signal;

#[component]
pub fn AuthGate() -> impl IntoView {
    let t = use_translations();
    let admin_key = use_admin_key();
    let input = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let verifying = RwSignal::new(false);
    let shake = RwSignal::new(false);
    let theme = use_theme_signal();

    let do_submit = {
        let t = t;
        move || {
            let value = input.get().trim().to_string();
            if value.is_empty() {
                error.set(t.auth_error_empty().to_string());
                shake.set(true);
                return;
            }
            if verifying.get_untracked() {
                return;
            }
            verifying.set(true);
            error.set(String::new());
            leptos::task::spawn_local(async move {
                let result = api::verify_admin_key(&value).await;
                match result {
                    Ok(()) => match complete_login(&value) {
                        Ok(()) => {
                            error.set(String::new());
                            admin_key.set(value);
                        }
                        Err(msg) => {
                            error.set(msg);
                            shake.set(true);
                        }
                    },
                    Err(e) if e == "invalid_admin_key" => {
                        error.set(t.auth_error_invalid().to_string());
                        shake.set(true);
                    }
                    Err(e) => {
                        error.set(e);
                        shake.set(true);
                    }
                }
                verifying.set(false);
            });
        }
    };

    let submit_click = {
        let do_submit = do_submit;
        move |_| do_submit()
    };

    let submit_keydown = {
        let do_submit = do_submit;
        move |ev: web_sys::KeyboardEvent| {
            if ev.key() == "Enter" {
                do_submit();
            }
        }
    };

    view! {
        <div class="auth-shell">
            <div class="auth-backdrop" aria-hidden="true">
                <div class="auth-cyber-grid"></div>
                <CrabParticles />
                <div class="auth-scanline"></div>
                <div class="auth-vignette"></div>
            </div>

            <header class="auth-toolbar">
                <div class="auth-toolbar-brand">
                    <span class="auth-status-dot" aria-hidden="true"></span>
                    <span class="auth-toolbar-label">"Gateway Control"</span>
                </div>
                <ThemeSwitcher />
            </header>

            <div class="auth-stage">
                <div class="auth-container">
                    <div class="auth-brand-panel">
                        <div class="auth-brand-content">
                            <div class="auth-brand-mark">
                                <BrandLogo large=true />
                            </div>
                            <h1 class="auth-brand-name">{Translations::sidebar_brand}</h1>
                            <p class="auth-brand-tagline">{t.auth_tagline()}</p>
                            <div class="auth-brand-divider" aria-hidden="true"></div>
                            <div class="auth-brand-features">
                                <div class="auth-brand-feature">
                                    <span class="auth-brand-feature-icon" aria-hidden="true">"◆"</span>
                                    <span class="auth-brand-feature-text">"LLM API Gateway"</span>
                                </div>
                                <div class="auth-brand-feature">
                                    <span class="auth-brand-feature-icon" aria-hidden="true">"◆"</span>
                                    <span class="auth-brand-feature-text">"Multi-Provider"</span>
                                </div>
                                <div class="auth-brand-feature">
                                    <span class="auth-brand-feature-icon" aria-hidden="true">"◆"</span>
                                    <span class="auth-brand-feature-text">"DeepSeek V4"</span>
                                </div>
                            </div>
                            <p class="auth-brand-footnote">
                                {move || format!("Theme: {}", theme.get().label())}
                            </p>
                        </div>
                    </div>

                    <div class="auth-form-panel">
                        <div
                            class=move || {
                                if shake.get() {
                                    "auth-form-card auth-shake"
                                } else {
                                    "auth-form-card"
                                }
                            }
                            on:animationend=move |_| shake.set(false)
                        >
                            <div class="auth-form-header">
                                <div class="auth-form-badge">"SECURE ADMIN"</div>
                                <h2 class="auth-form-title">{t.auth_title()}</h2>
                                <p class="auth-form-desc">{t.auth_desc()}</p>
                            </div>

                            <div class="auth-form-body">
                                <div class="auth-input-group">
                                    <label class="auth-input-label" for="admin-key-input">
                                        {t.auth_key_label()}
                                    </label>
                                    <div class="auth-input-wrap">
                                        <span class="auth-input-icon" aria-hidden="true">"⌁"</span>
                                        <input
                                            id="admin-key-input"
                                            type="password"
                                            class="auth-input"
                                            placeholder=t.auth_key_placeholder()
                                            prop:value=move || input.get()
                                            on:input=move |ev| {
                                                input.set(event_target_value(&ev));
                                                error.set(String::new());
                                            }
                                            on:keydown=submit_keydown
                                            autocomplete="current-password"
                                        />
                                        <div class="auth-input-focus-line" aria-hidden="true"></div>
                                    </div>
                                </div>

                                {move || {
                                    if error.get().is_empty() {
                                        ().into_any()
                                    } else {
                                        view! {
                                            <div class="auth-error" role="alert">
                                                <span class="auth-error-icon" aria-hidden="true">"◉"</span>
                                                <span>{error.get()}</span>
                                            </div>
                                        }.into_any()
                                    }
                                }}

                                <button
                                    type="button"
                                    class="auth-submit-btn"
                                    on:click=submit_click
                                    disabled=move || verifying.get()
                                >
                                    {move || {
                                        if verifying.get() {
                                            view! {
                                                <span class="auth-submit-spinner" aria-hidden="true"></span>
                                                <span>{t.auth_verifying()}</span>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <span>{t.auth_submit()}</span>
                                                <span class="auth-submit-arrow" aria-hidden="true">"→"</span>
                                            }.into_any()
                                        }
                                    }}
                                </button>

                                <p class="auth-form-hint">
                                    "Bearer token stored locally in this browser only."
                                </p>

                                <DevDefaultKeyButton admin_key=admin_key />
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn DevDefaultKeyButton(admin_key: RwSignal<String>) -> impl IntoView {
    #[cfg(debug_assertions)]
    {
        let t = use_translations();
        let error = RwSignal::new(String::new());

        let use_dev_default = move |_| {
            const DEV_KEY: &str = "admin";
            leptos::task::spawn_local(async move {
                if api::verify_admin_key(DEV_KEY).await.is_ok() {
                    if complete_login(DEV_KEY).is_ok() {
                        error.set(String::new());
                        admin_key.set(DEV_KEY.to_string());
                    } else {
                        error.set(t.auth_error_save().to_string());
                    }
                } else {
                    error.set(t.auth_error_invalid().to_string());
                }
            });
        };

        return view! {
            <div class="auth-dev-section">
                {move || {
                    if error.get().is_empty() {
                        ().into_any()
                    } else {
                        view! { <p class="auth-error">{error.get()}</p> }.into_any()
                    }
                }}
                <button type="button" class="auth-dev-btn" on:click=use_dev_default>
                    <span class="auth-dev-icon" aria-hidden="true">"⚡"</span>
                    <span>{t.auth_dev_default()}</span>
                </button>
            </div>
        };
    }

    #[cfg(not(debug_assertions))]
    view! { <span class="hidden" aria-hidden="true"></span> }
}
