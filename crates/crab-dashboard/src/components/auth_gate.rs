use leptos::prelude::*;

use crate::auth::{save_admin_key, use_admin_key};
use crate::locale::use_translations;

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
        <div class="min-h-screen flex items-center justify-center bg-theme p-6">
            <div class="w-full max-w-md metric-card p-8 space-y-6">
                <div>
                    <h1 class="text-xl font-semibold text-theme">{t.auth_title()}</h1>
                    <p class="mt-2 text-sm text-theme-muted">{t.auth_desc()}</p>
                </div>
                <div class="space-y-2">
                    <label class="block text-xs text-theme-muted">{t.auth_key_label()}</label>
                    <input
                        type="password"
                        class="w-full px-3 py-2 rounded-lg border border-theme bg-theme-secondary text-theme text-sm"
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
                        view! {
                            <p class="text-sm text-rose-500">{error.get()}</p>
                        }.into_any()
                    }
                }}
                <button
                    type="button"
                    class="w-full py-2 rounded-lg bg-accent text-white text-sm font-medium hover:opacity-90 transition-opacity"
                    on:click=submit_click
                >
                    {t.auth_submit()}
                </button>
                <DevDefaultKeyButton admin_key=admin_key />
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
            <div class="space-y-2">
                {move || {
                    if error.get().is_empty() {
                        ().into_any()
                    } else {
                        view! {
                            <p class="text-sm text-rose-500">{error.get()}</p>
                        }.into_any()
                    }
                }}
                <button
                    type="button"
                    class="w-full py-2 rounded-lg border border-theme text-theme-secondary text-sm hover:bg-theme-secondary transition-colors"
                    on:click=use_dev_default
                >
                    {t.auth_dev_default()}
                </button>
            </div>
        };
    }

    #[cfg(not(debug_assertions))]
    view! { <span class="hidden" aria-hidden="true"></span> }
}
