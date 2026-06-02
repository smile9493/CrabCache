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
    let active_tab: RwSignal<usize> = RwSignal::new(0);
    let tab_labels = vec![
        t.ds_tab_themes().to_string(),
        t.ds_tab_tokens().to_string(),
        t.ds_tab_components().to_string(),
        t.ds_tab_layout().to_string(),
    ];

    init_tab_from_query(
        active_tab,
        &[
            ("themes", 0),
            ("tokens", 1),
            ("components", 2),
            ("layout", 3),
        ],
    );

    view! {
        <div class="page-content ds-page space-y-6">
            <SectionHeader title=t.ds_title() description=t.ds_desc() />
            <TabBar tabs=tab_labels active=active_tab />
            {move || match active_tab.get() {
                0 => view! { <ThemesTab /> }.into_any(),
                1 => view! { <TokensTab /> }.into_any(),
                2 => view! { <ComponentsTab /> }.into_any(),
                _ => view! { <LayoutTab /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn ThemesTab() -> impl IntoView {
    let t = use_translations();
    let theme = use_theme_signal();
    let resolved = move || theme.get().resolved();

    view! {
        <div class="tab-panel">
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
                                    class=move || {
                                        if is_active() {
                                            "ds-theme-card active"
                                        } else {
                                            "ds-theme-card"
                                        }
                                    }
                                    on:click=move |_| {
                                        if th == Theme::System {
                                            spawn_system_listener(theme_signal);
                                        }
                                        theme_signal.set(th);
                                    }
                                >
                                    <span class=format!("theme-swatch {}", th.swatch_class())></span>
                                    <span class="ds-theme-card-copy">
                                        <span class="ds-theme-card-label">{th.label()}</span>
                                        <span class="ds-theme-card-desc">{th.description()}</span>
                                    </span>
                                    <span class="ds-theme-card-check" aria-hidden="true">
                                        {move || if is_active() { "✓" } else { "" }}
                                    </span>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                    <div class="ds-preview-strip" aria-live="polite">
                        <span class="ds-preview-strip-label">{t.ds_active_theme()}</span>
                        <span class="font-mono text-sm text-theme">{move || resolved().label()}</span>
                        <span class="ds-preview-chip" style="background: var(--cc-bg-card); color: var(--cc-text);">
                            {t.ds_preview_surface()}
                        </span>
                        <span class="ds-preview-chip" style="background: var(--cc-accent-muted); color: var(--cc-accent);">
                            {t.ds_preview_accent()}
                        </span>
                        <span class="ds-preview-chip border border-theme">
                            <span class="ds-swatch-dot" style="background: var(--cc-success);"></span>
                            {t.ds_preview_success()}
                        </span>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn TokensTab() -> impl IntoView {
    let t = use_translations();
    let theme = use_theme_signal();
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
        <div class="tab-panel">
            <div class="cache-card-grid">
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
                                                <button
                                                    type="button"
                                                    class="ds-token-swatch"
                                                    style=move || {
                                                        let _ = refresh.get();
                                                        format!("background: {}", read_css_var(&name_style))
                                                    }
                                                    title=t.ds_copy_token()
                                                    aria-label=format!("{}: {}", t.ds_copy_token(), name_aria)
                                                    on:click={
                                                        let copy_var = name.clone();
                                                        move |_| copy(copy_var.clone())
                                                    }
                                                ></button>
                                                <code class="ds-token-name">{name.clone()}</code>
                                                <span class="ds-token-value font-mono">
                                                    {move || {
                                                        let _ = refresh.get();
                                                        read_css_var(&name_value)
                                                    }}
                                                </span>
                                                <button
                                                    type="button"
                                                    class="btn btn-ghost btn-sm"
                                                    on:click={
                                                        let copy_var = name.clone();
                                                        move |_| copy(copy_var.clone())
                                                    }
                                                >
                                                    {t.ds_copy()}
                                                </button>
                                            </li>
                                        }
                                    }).collect_view()}
                                </ul>
                            </div>
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

#[component]
fn ComponentsTab() -> impl IntoView {
    let t = use_translations();
    let alert_msg = RwSignal::new(String::new());
    let demo_value = RwSignal::new(42.0f64);

    view! {
        <div class="tab-panel">
            <div class="cache-card-grid">
                // Buttons
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_spec_buttons()}</h3>
                        <p class="config-card-desc">{t.ds_spec_buttons_hint()}</p>
                    </div>
                    <div class="config-card-body">
                        <div class="flex flex-wrap gap-2">
                            <button type="button" class="btn btn-primary">{t.ds_btn_primary()}</button>
                            <button type="button" class="btn btn-secondary">{t.ds_btn_secondary()}</button>
                            <button type="button" class="btn btn-ghost">{t.ds_btn_ghost()}</button>
                            <button type="button" class="btn btn-danger">{t.ds_btn_danger()}</button>
                            <button type="button" class="btn btn-primary" disabled=true>{t.ds_btn_disabled()}</button>
                        </div>
                    </div>
                </div>

                // Badges
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_spec_badges()}</h3>
                        <p class="config-card-desc">{t.ds_spec_badges_hint()}</p>
                    </div>
                    <div class="config-card-body">
                        <div class="flex flex-wrap gap-2">
                            <Badge text=t.ds_badge_default().to_string() color="stone" />
                            <Badge text=t.ds_badge_info().to_string() color="info" />
                            <Badge text=t.ds_badge_success().to_string() color="success" />
                            <Badge text=t.ds_badge_warning().to_string() color="warning" />
                            <Badge text=t.ds_badge_error().to_string() color="error" />
                            <Badge text=t.ds_badge_accent().to_string() color="accent" />
                        </div>
                    </div>
                </div>

                // Alerts
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_spec_alerts()}</h3>
                        <p class="config-card-desc">{t.ds_spec_alerts_hint()}</p>
                    </div>
                    <div class="config-card-body space-y-2">
                        <div class="alert alert-info" role="status">{t.ds_alert_info()}</div>
                        <div class="alert alert-success" role="status">{t.ds_alert_success()}</div>
                        <div class="alert alert-warning" role="status">{t.ds_alert_warning()}</div>
                        <div class="alert alert-error" role="alert">{t.ds_alert_error()}</div>
                        <button
                            type="button"
                            class="btn btn-secondary btn-sm"
                            on:click=move |_| alert_msg.set(t.ds_alert_dynamic().to_string())
                        >
                            {t.ds_alert_trigger()}
                        </button>
                        <Alert variant="info" message=Signal::derive(move || alert_msg.get()) />
                    </div>
                </div>

                // Forms
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_spec_forms()}</h3>
                        <p class="config-card-desc">{t.ds_spec_forms_hint()}</p>
                    </div>
                    <div class="config-card-body space-y-3">
                        <input type="text" class="input w-full" placeholder=t.ds_input_placeholder() />
                        <ConfigRangeF64
                            label=move || t.ds_range_label().to_string()
                            value=demo_value
                            min=0.0
                            max=100.0
                            step=1.0
                            min_hint="0"
                            max_hint="100"
                            accent="accent"
                        />
                    </div>
                </div>

                // Metrics
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_spec_metrics()}</h3>
                        <p class="config-card-desc">{t.ds_spec_metrics_hint()}</p>
                    </div>
                    <div class="config-card-body space-y-3">
                        <MetricCard
                            title=t.ds_metric_hit()
                            value=Signal::derive(|| "98.2%".to_string())
                            subtitle=t.ds_metric_hit_sub()
                        />
                        <ProgressBar label=t.ds_progress_label() value=demo_value.into() max=100.0 />
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn LayoutTab() -> impl IntoView {
    let t = use_translations();
    let density = use_table_density();

    view! {
        <div class="tab-panel">
            <div class="cache-card-grid">
                // Density
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_density_title()}</h3>
                        <p class="config-card-desc">{t.ds_density_desc()}</p>
                    </div>
                    <div class="config-card-body flex flex-wrap gap-2">
                        <button
                            type="button"
                            class=move || {
                                if density.get() == TableDensity::Comfortable {
                                    "btn btn-primary"
                                } else {
                                    "btn btn-secondary"
                                }
                            }
                            on:click=move |_| density.set(TableDensity::Comfortable)
                        >
                            {t.table_density_comfortable()}
                        </button>
                        <button
                            type="button"
                            class=move || {
                                if density.get() == TableDensity::Compact {
                                    "btn btn-primary"
                                } else {
                                    "btn btn-secondary"
                                }
                            }
                            on:click=move |_| density.set(TableDensity::Compact)
                        >
                            {t.table_density_compact()}
                        </button>
                    </div>
                </div>

                // Table preview
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_table_preview()}</h3>
                    </div>
                    <div class="config-card-body">
                        <div class="overflow-x-auto">
                            <table class="data-table w-full">
                                <thead>
                                    <tr>
                                        <th>{t.ds_col_model()}</th>
                                        <th>{t.ds_col_tier()}</th>
                                        <th class="text-right">{t.ds_col_latency()}</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td class="font-mono text-sm">"deepseek-v4"</td>
                                        <td><Badge text="L0".to_string() color="accent" /></td>
                                        <td class="text-right font-mono tabular-nums">"0.08 ms"</td>
                                    </tr>
                                    <tr>
                                        <td class="font-mono text-sm">"gpt-4o"</td>
                                        <td><Badge text="L1".to_string() color="info" /></td>
                                        <td class="text-right font-mono tabular-nums">"2.4 ms"</td>
                                    </tr>
                                    <tr>
                                        <td class="font-mono text-sm">"claude-sonnet"</td>
                                        <td><Badge text="miss".to_string() color="stone" /></td>
                                        <td class="text-right font-mono tabular-nums">"842 ms"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </div>
                </div>

                // Spacing scale
                <div class="config-card glass-card">
                    <div class="config-card-head">
                        <h3 class="config-card-title">{t.ds_spacing_title()}</h3>
                    </div>
                    <div class="config-card-body">
                        <ul class="ds-spacing-scale">
                            <li><span class="ds-spacing-bar" style="width: var(--cc-spacing-xs);"></span><code>--cc-spacing-xs</code></li>
                            <li><span class="ds-spacing-bar" style="width: var(--cc-spacing-sm);"></span><code>--cc-spacing-sm</code></li>
                            <li><span class="ds-spacing-bar" style="width: var(--cc-spacing-md);"></span><code>--cc-spacing-md</code></li>
                            <li><span class="ds-spacing-bar" style="width: var(--cc-spacing-lg);"></span><code>--cc-spacing-lg</code></li>
                            <li><span class="ds-spacing-bar" style="width: var(--cc-spacing-xl);"></span><code>--cc-spacing-xl</code></li>
                        </ul>
                    </div>
                </div>
            </div>
        </div>
    }
}
