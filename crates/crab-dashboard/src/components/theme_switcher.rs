use leptos::prelude::*;
use crate::theme::{use_theme_signal, Theme};

#[component]
pub fn ThemeSwitcher() -> impl IntoView {
    let theme = use_theme_signal();
    let is_open = signal(false);

    let toggle = move |_| {
        is_open.1.set(!is_open.0.get());
    };

    let select_theme = move |new_theme: Theme| {
        theme.set(new_theme);
        is_open.1.set(false);
    };

    view! {
        <div class="theme-switcher">
            <button
                class="theme-switcher-trigger"
                on:click=toggle
            >
                {move || theme.get().icon()}
            </button>

            <div
                class=move || {
                    if is_open.0.get() {
                        "theme-switcher-dropdown"
                    } else {
                        "hidden"
                    }
                }
            >
                {Theme::all().into_iter().map(|t| {
                    let is_active = move || theme.get() == t;
                    view! {
                        <button
                            class=move || {
                                if is_active() {
                                    "theme-option active"
                                } else {
                                    "theme-option"
                                }
                            }
                            on:click=move |_| select_theme(t)
                        >
                            <span class="theme-option-icon">{t.icon()}</span>
                            <span class="flex-1 text-left text-sm">{t.label()}</span>
                            {move || if is_active() {
                                view! {
                                    <span class="text-[10px] text-accent">OK</span>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
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
        let themes = Theme::all();
        let current = theme_signal.get();
        let idx = themes.iter().position(|&t| t == current).unwrap_or(0);
        let next = themes[(idx + 1) % themes.len()];
        theme_signal.set(next);
    };

    view! {
        <button
            class="theme-switcher-trigger"
            on:click=cycle_theme
        >
            {move || theme_signal.get().icon()}
        </button>
    }
}