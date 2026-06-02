//! Design system management: themes, tokens, component specs, layout density.

use leptos::prelude::*;

use crate::clipboard::copy_text;
use crate::components::toast::{ToastKind, show_toast, try_use_toast};
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::table_density::{TableDensity, use_table_density};
use crate::theme::{Theme, spawn_system_listener, use_theme_signal};

fn read_css_var(name: &str) -> String {
    let Some(window) = web_sys::window() else {
        return "—".to_string();
    };
    let Some(document) = window.document() else {
        return "—".to_string();
    };
    let Some(root) = document.document_element() else {
        return "—".to_string();
    };
    let Ok(Some(styles)) = window.get_computed_style(&root) else {
        return "—".to_string();
    };
    styles
        .get_property_value(name)
        .unwrap_or_default()
        .trim()
        .to_string()
}

const TOKEN_GROUPS: &[(&str, &[&str])] = &[
    (
        "surface",
        &[
            "--cc-bg",
            "--cc-bg-topnav",
            "--cc-bg-card",
            "--cc-bg-elevated",
            "--cc-bg-input",
            "--cc-border",
            "--cc-border-light",
        ],
    ),
    (
        "text",
        &["--cc-text", "--cc-text-muted", "--cc-accent", "--cc-accent-bright"],
    ),
    (
        "semantic",
        &[
            "--cc-success",
            "--cc-info",
            "--cc-warning",
            "--cc-error",
            "--cc-purple",
        ],
    ),
    (
        "cache_tiers",
        &[
            "--cc-tier-l0",
            "--cc-tier-l1",
            "--cc-tier-l2",
            "--cc-tier-l3",
            "--cc-tier-miss",
        ],
    ),
    (
        "layout",
        &[
            "--radius-sm",
            "--radius-md",
            "--radius-lg",
            "--cc-spacing-sm",
            "--cc-spacing-md",
            "--cc-spacing-lg",
            "--transition-speed",
        ],
    ),
];

/// Locale-keyed descriptions for each token group (card head subtitle).
fn token_group_desc(t: crate::locale::Translations, group_id: &str) -> &'static str {
    match group_id {
        "surface" => t.ds_token_desc_surface(),
        "text" => t.ds_token_desc_text(),
        "semantic" => t.ds_token_desc_semantic(),
        "cache_tiers" => t.ds_token_desc_tiers(),
        _ => t.ds_token_desc_layout(),
    }
}

#[component]
pub fn DesignSystemPage() -> impl IntoView {
    let t = use_translations();
    let theme = use_theme_signal();
    let resolved = move || theme.get().resolved();
    let density = use_table_density();
    let refresh: RwSignal<u32> = RwSignal::new(0);

    Effect::new(move |_| {
        let _ = theme.get();
        refresh.update(|n| *n = n.wrapping_add(1));
    });

    let toast_copy = move |text: String| {
        if copy_text(&text) {
            if let Some(toast) = try_use_toast() {
                show_toast(toast, ToastKind::Success, &t.ds_copy_ok());
            }
        }
    };

    view! {
        <>
            // ── Themes ──
            <div class="config-card glass-card">
                <div class="config-card-head">
                    <h3 class="config-card-title">{t.ds_tab_themes()}</h3>
                    <p class="config-card-desc">{t.ds_themes_lead()}</p>
                </div>
                <div class="config-card-body space-y-4">
                    <div class="ds-theme-grid" role="listbox" aria-label=t.ds_tab_themes()>
                        {Theme::all().into_iter().map(|th| {
                            let is_active = move || theme.get() == th;
                            let theme_signal = theme;
                            view! {
                                <button
                                    type="button"
                                    role="option"
                                    aria-selected=is_active
                                    class=move || if is_active() { "ds-theme-card active" } else { "ds-theme-card" }
                                    on:click=move |_| {
                                        if th == Theme::System { spawn_system_listener(theme_signal); }
                                        theme_signal.set(th);
                                    }
                                >
                                    <span class=format!("theme-swatch {}", th.swatch_class())></span>
                                    <span class="ds-theme-card-copy">
                                        <span class="ds-theme-card-label">{th.label()}</span>
                                        <span class="ds-theme-card-desc">{th.description()}</span>
                                    </span>
                                    <span class="ds-theme-card-check" aria-hidden="true">
                                        {move || if is_active() { "\u{2713}" } else { "" }}
                                    </span>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                    <div class="ds-preview-strip" aria-live="polite">
                        <span class="ds-preview-strip-label">{t.ds_active_theme()}</span>
                        <span class="font-mono text-sm text-theme">{move || resolved().label()}</span>
                        <span class="ds-preview-chip" style="background: var(--cc-bg-card); color: var(--cc-text);">{t.ds_preview_surface()}</span>
                        <span class="ds-preview-chip" style="background: var(--cc-accent-muted); color: var(--cc-accent);">{t.ds_preview_accent()}</span>
                        <span class="ds-preview-chip border border-theme">
                            <span class="ds-swatch-dot" style="background: var(--cc-success);"></span>
                            {t.ds_preview_success()}
                        </span>
                    </div>
                </div>
            </div>

            // ── Tokens (inline from TokensTab) ──
            {TOKEN_GROUPS.iter().map(|(group_id, vars)| {
                let title = match *group_id {
                    "surface" => t.ds_token_group_surface(),
                    "text" => t.ds_token_group_text(),
                    "semantic" => t.ds_token_group_semantic(),
                    "cache_tiers" => t.ds_token_group_tiers(),
                    _ => t.ds_token_group_layout(),
                };
                let desc = token_group_desc(t, group_id);
                view! {
                    <div class="config-card glass-card">
                        <div class="config-card-head">
                            <h3 class="config-card-title">{title}</h3>
                            <p class="config-card-desc">{desc}</p>
                        </div>
                        <div class="config-card-body">
                            <ul class="ds-token-list">
                                {vars.iter().map(|var_name| {
                                    let name = (*var_name).to_string();
                                    let name_style = name.clone();
                                    let name_value = name.clone();
                                    let name_aria = name.clone();
                                    let copy = toast_copy;
                                    view! {
                                        <li class="ds-token-row">
                                            <button type="button" class="ds-token-swatch"
                                                style=move || { let _ = refresh.get(); format!("background: {}", read_css_var(&name_style)) }
                                                title=t.ds_copy_token()
                                                aria-label=format!("{}: {}", t.ds_copy_token(), name_aria)
                                                on:click={ let v = name.clone(); move |_| copy(v.clone()) }
                                            ></button>
                                            <code class="ds-token-name">{name.clone()}</code>
                                            <span class="ds-token-value font-mono">{move || { let _ = refresh.get(); read_css_var(&name_value) }}</span>
                                            <button type="button" class="btn btn-ghost btn-sm"
                                                on:click={ let v = name.clone(); move |_| copy(v.clone()) }
                                            >{t.ds_copy()}</button>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        </div>
                    </div>
                }
            }).collect_view()}

            // ── Layout: Density ──
            <div class="config-card glass-card">
                <div class="config-card-head">
                    <h3 class="config-card-title">{t.ds_density_title()}</h3>
                    <p class="config-card-desc">{t.ds_density_desc()}</p>
                </div>
                <div class="config-card-body flex flex-wrap gap-2">
                    <button type="button"
                        class=move || if density.get() == TableDensity::Comfortable { "btn btn-primary" } else { "btn btn-secondary" }
                        on:click=move |_| density.set(TableDensity::Comfortable)
                    >{t.table_density_comfortable()}</button>
                    <button type="button"
                        class=move || if density.get() == TableDensity::Compact { "btn btn-primary" } else { "btn btn-secondary" }
                        on:click=move |_| density.set(TableDensity::Compact)
                    >{t.table_density_compact()}</button>
                </div>
            </div>
        </>
    }
}
