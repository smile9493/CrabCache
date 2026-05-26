use leptos::prelude::*;

#[component]
pub fn MetricCard(
    title: &'static str,
    value: Signal<String>,
    subtitle: &'static str,
) -> impl IntoView {
    view! {
        <div class="metric-card">
            <div class="metric-card-label">{title}</div>
            <div class="metric-card-value">{move || value.get()}</div>
            <div class="metric-card-sub">{subtitle}</div>
        </div>
    }
}

/// Panel section title row (title + optional right meta).
#[component]
pub fn PanelHeader(
    title: impl Fn() -> String + Send + Sync + 'static,
    #[prop(optional)] meta: Option<impl Fn() -> String + Send + Sync + 'static>,
) -> impl IntoView {
    view! {
        <div class="panel-header">
            <span>{move || title()}</span>
            {meta.map(|m| view! {
                <span class="panel-header-meta">{m}</span>
            })}
        </div>
    }
}

#[component]
pub fn ProgressBar(label: &'static str, value: Signal<f64>, max: f64) -> impl IntoView {
    view! {
        <div class="space-y-1.5">
            <div class="flex justify-between items-baseline">
                <span class="text-xs text-theme-secondary">{label}</span>
                <span class="text-xs font-mono tabular-nums text-theme">
                    {move || format!("{:.1}%", (value.get() / max * 100.0).min(100.0))}
                </span>
            </div>
            <div class="progress-bar">
                <div
                    class="progress-bar-fill"
                    style=move || format!("width: {}%", (value.get() / max * 100.0).min(100.0))
                ></div>
            </div>
        </div>
    }
}

#[component]
pub fn Badge(text: String, color: &'static str) -> impl IntoView {
    let color_class = match color {
        "teal" | "info" => "badge badge-info",
        "amber" | "warning" => "badge badge-warning",
        "rose" | "error" => "badge badge-error",
        "green" | "success" => "badge badge-success",
        "violet" | "accent" => "badge badge-accent",
        "stone" => "badge",
        _ => "badge",
    };

    view! {
        <span class=color_class>
            {text}
        </span>
    }
}

#[component]
pub fn Spinner() -> impl IntoView {
    view! {
        <div class="flex items-center justify-center py-12">
            <div class="spinner"></div>
        </div>
    }
}

#[component]
pub fn SectionHeader(title: &'static str, description: &'static str) -> impl IntoView {
    view! {
        <div class="section-header">
            <h2>{title}</h2>
            <p>{description}</p>
        </div>
    }
}

#[component]
pub fn EmptyState(message: &'static str) -> impl IntoView {
    view! {
        <div class="empty-state">
            <div class="empty-state-icon">{crate::locale::Translations::empty_state_icon()}</div>
            <div class="empty-state-title">{message}</div>
        </div>
    }
}

#[component]
pub fn ConfigRangeU64(
    label: impl Fn() -> String + Send + Sync + 'static,
    value: RwSignal<u64>,
    min: u64,
    max: u64,
    min_hint: &'static str,
    max_hint: &'static str,
    accent: &'static str,
) -> impl IntoView {
    let range_class = format!("form-range form-range-{}", accent);
    view! {
        <div class="form-range-block">
            <label class="form-range-label">{move || label()}</label>
            <input
                type="range"
                min=min
                max=max
                prop:value=move || value.get()
                on:input=move |ev| {
                    if let Ok(v) = event_target_value(&ev).parse() {
                        value.set(v);
                    }
                }
                class=range_class.clone()
            />
            <div class="form-range-hints">
                <span>{min_hint}</span>
                <span>{max_hint}</span>
            </div>
        </div>
    }
}

#[component]
pub fn ConfigRangeF64(
    label: impl Fn() -> String + Send + Sync + 'static,
    value: RwSignal<f64>,
    min: f64,
    max: f64,
    step: f64,
    min_hint: &'static str,
    max_hint: &'static str,
    accent: &'static str,
) -> impl IntoView {
    let range_class = format!("form-range form-range-{}", accent);
    view! {
        <div class="form-range-block">
            <label class="form-range-label">{move || label()}</label>
            <input
                type="range"
                min=min
                max=max
                step=step
                prop:value=move || value.get()
                on:input=move |ev| {
                    if let Ok(v) = event_target_value(&ev).parse() {
                        value.set(v);
                    }
                }
                class=range_class.clone()
            />
            <div class="form-range-hints">
                <span>{min_hint}</span>
                <span>{max_hint}</span>
            </div>
        </div>
    }
}

/// Set `active` from `?tab=` when the value matches a `(name, index)` pair.
pub fn init_tab_from_query(active: RwSignal<usize>, tabs: &[(&str, usize)]) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(search) = window.location().search() else {
        return;
    };
    let query = search.trim_start_matches('?');
    let tab_value = query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == "tab").then_some(v)
    });
    let Some(value) = tab_value else {
        return;
    };
    for (name, index) in tabs {
        if *name == value {
            active.set(*index);
            return;
        }
    }
}

/// Horizontal tab bar. `active` is the index of the selected tab.
#[component]
pub fn TabBar(tabs: Vec<String>, active: RwSignal<usize>) -> impl IntoView {
    view! {
        <div
            class="flex flex-wrap gap-0.5 border-b border-theme-border px-1 mb-4"
            role="tablist"
        >
            {tabs.into_iter().enumerate().map(|(i, label)| {
                let is_active = move || active.get() == i;
                view! {
                    <button
                        type="button"
                        role="tab"
                        aria-selected=is_active
                        class=move || {
                            if is_active() {
                                "px-4 py-2 text-sm font-semibold border-b-2 border-[var(--cc-accent)] text-[var(--cc-accent-bright)] bg-transparent cursor-pointer"
                            } else {
                                "px-4 py-2 text-sm font-medium text-theme-muted hover:text-theme bg-transparent border-b-2 border-transparent cursor-pointer"
                            }
                        }
                        on:click=move |_| active.set(i)
                    >
                        {label}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}

#[component]
pub fn Alert(variant: &'static str, message: Signal<String>) -> impl IntoView {
    let class = match variant {
        "success" => "alert alert-success",
        "warning" => "alert alert-warning",
        "error" => "alert alert-error",
        _ => "alert alert-info",
    };
    view! {
        {move || {
            if message.get().is_empty() {
                ().into_any()
            } else {
                view! { <div class=class role="alert">{message.get()}</div> }.into_any()
            }
        }}
    }
}

#[component]
pub fn Tooltip(
    text: Signal<String>,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="tooltip-wrapper">
            {children()}
            <div class="tooltip-content">
                {move || text.get()}
            </div>
        </div>
    }
}

#[component]
pub fn TrendMetricCard(
    title: &'static str,
    value: Signal<String>,
    subtitle: &'static str,
    #[prop(optional)]
    trend_pct: Option<f64>,
    #[prop(optional)]
    trend_label: Option<&'static str>,
) -> impl IntoView {
    view! {
        <div class="metric-card">
            <div class="metric-card-label">{title}</div>
            <div class="metric-card-value">{move || value.get()}</div>
            <div class="metric-card-footer">
                <div class="metric-card-sub">{subtitle}</div>
                {trend_pct.map(|pct| {
                    let cls = if pct > 0.01 {
                        "metric-card-trend trend-up"
                    } else if pct < -0.01 {
                        "metric-card-trend trend-down"
                    } else {
                        "metric-card-trend trend-neutral"
                    };
                    let arrow = if pct > 0.01 { "↑" } else if pct < -0.01 { "↓" } else { "→" };
                    view! {
                        <span class=cls>
                            {arrow} {format!("{:+.1}%", pct)}
                            {trend_label.map(|l| view! { <span class="text-theme-muted ml-1">{l}</span> })}
                        </span>
                    }
                })}
            </div>
        </div>
    }
}
