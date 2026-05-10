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
            <div>
                <div class="metric-card-value">{move || value.get()}</div>
                <div class="metric-card-sub">{subtitle}</div>
            </div>
        </div>
    }
}

#[component]
pub fn ProgressBar(
    label: &'static str,
    value: Signal<f64>,
    max: f64,
) -> impl IntoView {
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
        "teal" => "badge badge-info",
        "amber" => "badge badge-warning",
        "rose" => "badge badge-error",
        "violet" => "badge badge-accent",
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