use leptos::prelude::*;

use crate::auth::{save_admin_key, use_admin_key};
use crate::locale::{Translations, use_translations};

#[component]
pub fn AuthGate() -> impl IntoView {
    let t = use_translations();
    let admin_key = use_admin_key();
    let input = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());

    let do_submit = {
        let t = t;
        move || {
            let value = input.get().trim().to_string();
            if value.is_empty() {
                error.set(t.auth_error_empty().to_string());
                return;
            }
            match save_admin_key(&value) {
                Ok(()) => {
                    error.set(String::new());
                    admin_key.set(value);
                }
                Err(msg) => error.set(msg),
            }
        }
    };

    let submit_click = {
        let do_submit = do_submit.clone();
        move |_| do_submit()
    };

    let submit_keydown = {
        let do_submit = do_submit.clone();
        move |ev: web_sys::KeyboardEvent| {
            if ev.key() == "Enter" {
                do_submit();
            }
        }
    };

    view! {
        <div class="auth-screen">
            <div class="auth-screen-glow" aria-hidden="true"></div>
            <div class="auth-card glass-card-raised">
                <div class="auth-brand">
                    <img src="/style/favicon.svg" alt="" class="brand-logo brand-logo-lg" width="40" height="40" />
                    <div>
                        <h1 class="auth-title">{Translations::sidebar_brand}</h1>
                        <p class="auth-tagline">{t.auth_tagline()}</p>
                    </div>
                </div>
                <div class="auth-form space-y-4">
                    <div>
                        <h2 class="text-base font-semibold text-theme">{t.auth_title()}</h2>
                        <p class="mt-1 text-sm text-theme-muted">{t.auth_desc()}</p>
                    </div>
                    <div class="space-y-2">
                        <label class="block text-xs text-theme-muted">{t.auth_key_label()}</label>
                        <input
                            type="password"
                            class="input"
                            placeholder=t.auth_key_placeholder()
                            prop:value=move || input.get()
                            on:input=move |ev| {
                                input.set(event_target_value(&ev));
                                error.set(String::new());
                            }
                            on:keydown=submit_keydown
                        />
                    </div>
                    {move || {
                        if error.get().is_empty() {
                            ().into_any()
                        } else {
                            view! { <p class="text-sm text-error">{error.get()}</p> }.into_any()
                        }
                    }}
                    <button type="button" class="btn btn-primary w-full" on:click=submit_click>
                        {t.auth_submit()}
                    </button>
                    <DevDefaultKeyButton admin_key=admin_key />
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
            if save_admin_key(DEV_KEY).is_ok() {
                error.set(String::new());
                admin_key.set(DEV_KEY.to_string());
            } else {
                error.set(t.auth_error_save().to_string());
            }
        };

        return view! {
            <div class="space-y-2 pt-2 border-t border-theme-light">
                {move || {
                    if error.get().is_empty() {
                        ().into_any()
                    } else {
                        view! { <p class="text-sm text-error">{error.get()}</p> }.into_any()
                    }
                }}
                <button type="button" class="btn btn-secondary w-full" on:click=use_dev_default>
                    {t.auth_dev_default()}
                </button>
            </div>
        };
    }

    #[cfg(not(debug_assertions))]
    view! { <span class="hidden" aria-hidden="true"></span> }
}
