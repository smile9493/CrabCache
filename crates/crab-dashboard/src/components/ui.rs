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

/// Horizontal tab bar. `active` is the index of the selected tab.
#[component]
pub fn TabBar(
    tabs: Vec<&'static str>,
    active: RwSignal<usize>,
) -> impl IntoView {
    view! {
        <div class="tab-bar">
            {tabs.into_iter().enumerate().map(|(i, label)| {
                let is_active = move || active.get() == i;
                view! {
                    <button
                        class=move || {
                            if is_active() {
                                "tab-item tab-item-active"
                            } else {
                                "tab-item"
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
