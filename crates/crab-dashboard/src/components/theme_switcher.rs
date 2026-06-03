use crate::theme::{Theme, use_theme_signal};
use leptos::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::JsCast;

#[component]
pub fn ThemeSwitcher() -> impl IntoView {
    let theme = use_theme_signal();
    let is_open = signal(false);
    let dropdown_ref = NodeRef::<leptos::html::Div>::new();
    let alive: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));

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

    let click_outside: Arc<dyn Fn(web_sys::MouseEvent) + Send + Sync> = {
        let is_open = is_open;
        let dropdown_ref = dropdown_ref;
        let alive = Arc::clone(&alive);
        Arc::new(move |ev: web_sys::MouseEvent| {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            if !is_open.0.get() {
                return;
            }
            if let Some(node) = dropdown_ref.get() {
                let target = ev.target();
                if let Some(target_el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                {
                    if !node.contains(Some(&target_el)) {
                        is_open.1.set(false);
                    }
                }
            }
        })
    };

    let keydown_handler: Arc<dyn Fn(web_sys::KeyboardEvent) + Send + Sync> = {
        let alive = Arc::clone(&alive);
        Arc::new(move |ev: web_sys::KeyboardEvent| {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            if ev.key() == "Escape" && is_open.0.get() {
                is_open.1.set(false);
            }
        })
    };

    // Register event listeners once at component level (not inside Effect)
    // to avoid accumulating listeners on Effect re-runs.
    // Save js_sys::Function references so we can remove them in on_cleanup.
    let mut click_fn: Option<js_sys::Function> = None;
    let mut key_fn: Option<js_sys::Function> = None;

    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            let click_outside = Arc::clone(&click_outside);
            let click_closure =
                wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::MouseEvent| {
                    (click_outside)(ev);
                })
                    as Box<dyn Fn(web_sys::MouseEvent)>);
            let _ = document
                .add_event_listener_with_callback("mousedown", click_closure.as_ref().unchecked_ref());
            click_fn = Some(click_closure.into_js_value().unchecked_into());
            // Closure is NOT forgotten — we clean it up below.

            let keydown_handler = Arc::clone(&keydown_handler);
            let key_closure =
                wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::KeyboardEvent| {
                    (keydown_handler)(ev);
                })
                    as Box<dyn Fn(web_sys::KeyboardEvent)>);
            let _ = document
                .add_event_listener_with_callback("keydown", key_closure.as_ref().unchecked_ref());
            key_fn = Some(key_closure.into_js_value().unchecked_into());
        }
    }

    on_cleanup(move || {
        alive.store(false, Ordering::Relaxed);
        // Remove event listeners to prevent memory leaks.
        if let Some(window) = web_sys::window() {
            if let Some(document) = window.document() {
                if let Some(f) = &click_fn {
                    let _ = document.remove_event_listener_with_callback("mousedown", f);
                }
                if let Some(f) = &key_fn {
                    let _ = document.remove_event_listener_with_callback("keydown", f);
                }
            }
        }
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
