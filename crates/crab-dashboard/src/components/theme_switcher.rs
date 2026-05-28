use crate::theme::{Theme, use_theme_signal};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

#[component]
pub fn ThemeSwitcher() -> impl IntoView {
    let theme = use_theme_signal();
    let is_open = signal(false);
    let dropdown_ref = NodeRef::<leptos::html::Div>::new();

    let toggle = move |_| {
        is_open.1.set(!is_open.0.get());
    };

    let select_theme = move |new_theme: Theme| {
        if new_theme == Theme::System {
            crate::theme::spawn_system_listener(theme);
        }
        theme.set(new_theme);
        is_open.1.set(false);
    };

    let click_outside = {
        let is_open = is_open;
        let dropdown_ref = dropdown_ref;
        move |ev: web_sys::MouseEvent| {
            if !is_open.0.get() { return; }
            if let Some(node) = dropdown_ref.get() {
                let target = ev.target();
                if let Some(target_el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                    if !node.contains(Some(&target_el)) {
                        is_open.1.set(false);
                    }
                }
            }
        }
    };

    let keydown_handler = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" && is_open.0.get() {
            is_open.1.set(false);
        }
    };

    Effect::new(move || {
        let Some(window) = web_sys::window() else { return };
        let Some(document) = window.document() else { return };

        let click_closure =
            wasm_bindgen::closure::Closure::wrap(Box::new(click_outside) as Box<dyn Fn(web_sys::MouseEvent)>);
        let _ = document.add_event_listener_with_callback(
            "mousedown",
            click_closure.as_ref().unchecked_ref(),
        );
        click_closure.forget();

        let key_closure =
            wasm_bindgen::closure::Closure::wrap(Box::new(keydown_handler) as Box<dyn Fn(web_sys::KeyboardEvent)>);
        let _ = document.add_event_listener_with_callback(
            "keydown",
            key_closure.as_ref().unchecked_ref(),
        );
        key_closure.forget();
    });

    view! {
        <div class="theme-switcher" node_ref=dropdown_ref>
            <button
                type="button"
                class="theme-switcher-trigger"
                aria-label="切换主题"
                aria-expanded=move || is_open.0.get()
                on:click=toggle
            >
                <span class=move || format!("theme-swatch {}", theme.get().swatch_class())></span>
            </button>

            <div
                class=move || {
                    if is_open.0.get() {
                        "theme-switcher-dropdown"
                    } else {
                        "hidden"
                    }
                }
                role="listbox"
                aria-label="主题列表"
            >
                {Theme::all().into_iter().map(|t| {
                    let is_active = move || theme.get() == t;
                    view! {
                        <button
                            type="button"
                            role="option"
                            aria-selected=is_active
                            class=move || {
                                if is_active() {
                                    "theme-option active"
                                } else {
                                    "theme-option"
                                }
                            }
                            on:click=move |_| select_theme(t)
                        >
                            <span class=format!("theme-swatch {}", t.swatch_class())></span>
                            <span class="theme-option-copy">
                                <span class="theme-option-label">{t.label()}</span>
                                <span class="theme-option-desc">{t.description()}</span>
                            </span>
                            {move || if is_active() {
                                view! {
                                    <span class="theme-option-check" aria-hidden="true">"✓"</span>
                                }.into_any()
                            } else {
                                view! { <span class="theme-option-check" aria-hidden="true"></span> }.into_any()
                            }}
                        </button>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}

#[component]
pub fn ThemeSwitcherMinimal() -> impl IntoView {
    let theme_signal = use_theme_signal();

    let cycle_theme = move |_| {
        let themes = Theme::concrete();
        let current = theme_signal.get().resolved();
        let idx = themes.iter().position(|&t| t == current).unwrap_or(0);
        let next = themes[(idx + 1) % themes.len()];
        theme_signal.set(next);
    };

    view! {
        <button
            type="button"
            class="theme-switcher-trigger"
            aria-label="循环切换主题"
            on:click=cycle_theme
        >
            <span class=move || format!("theme-swatch {}", theme_signal.get().swatch_class())></span>
        </button>
    }
}
