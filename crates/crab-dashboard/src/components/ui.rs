use leptos::prelude::*;

#[component]
pub fn MetricCard(
    title: &'static str,
    value: Signal<String>,
    subtitle: &'static str,
    accent: &'static str,
) -> impl IntoView {
    let border_class = match accent {
        "teal" => "border-l-teal-500",
        "amber" => "border-l-amber-500",
        "rose" => "border-l-rose-500",
        "violet" => "border-l-violet-500",
        _ => "border-l-teal-500",
    };

    view! {
        <div class=format!(
            "bg-stone-900 border border-stone-800 border-l-2 {} rounded-lg p-5",
            border_class
        )>
            <div class="text-xs font-medium text-stone-500 uppercase tracking-wider">{title}</div>
            <div class="mt-2 text-2xl font-bold font-mono tabular-nums text-stone-100">
                {move || value.get()}
            </div>
            <div class="mt-1 text-xs text-stone-500">{subtitle}</div>
        </div>
    }
}

#[component]
pub fn ProgressBar(
    label: &'static str,
    value: Signal<f64>,
    max: f64,
    color: &'static str,
) -> impl IntoView {
    let color_class = match color {
        "teal" => "bg-teal-500",
        "amber" => "bg-amber-500",
        "rose" => "bg-rose-500",
        "violet" => "bg-violet-500",
        _ => "bg-teal-500",
    };

    view! {
        <div class="space-y-1.5">
            <div class="flex justify-between text-xs">
                <span class="text-stone-400">{label}</span>
                <span class="font-mono tabular-nums text-stone-300">
                    {move || format!("{:.1}%", (value.get() / max * 100.0).min(100.0))}
                </span>
            </div>
            <div class="w-full bg-stone-800 rounded-full h-2 overflow-hidden">
                <div
                    class=format!("h-full rounded-full {} transition-all duration-500", color_class)
                    style=move || format!("width: {}%", (value.get() / max * 100.0).min(100.0))
                ></div>
            </div>
        </div>
    }
}

#[component]
pub fn Badge(text: String, color: &'static str) -> impl IntoView {
    let color_class = match color {
        "teal" => "bg-teal-500/10 text-teal-400 border-teal-500/20",
        "amber" => "bg-amber-500/10 text-amber-400 border-amber-500/20",
        "rose" => "bg-rose-500/10 text-rose-400 border-rose-500/20",
        "violet" => "bg-violet-500/10 text-violet-400 border-violet-500/20",
        "stone" => "bg-stone-800 text-stone-400 border-stone-700",
        _ => "bg-stone-800 text-stone-400 border-stone-700",
    };

    view! {
        <span class=format!(
            "inline-flex items-center px-2 py-0.5 rounded text-[11px] font-medium border {}",
            color_class
        )>
            {text}
        </span>
    }
}

#[component]
pub fn Spinner() -> impl IntoView {
    view! {
        <div class="flex items-center justify-center py-12">
            <div class="w-6 h-6 border-2 border-stone-700 border-t-teal-500 rounded-full animate-spin"></div>
        </div>
    }
}

#[component]
pub fn SectionHeader(title: &'static str, description: &'static str) -> impl IntoView {
    view! {
        <div class="mb-6">
            <h2 class="text-lg font-semibold text-stone-100">{title}</h2>
            <p class="mt-1 text-sm text-stone-500">{description}</p>
        </div>
    }
}

#[component]
pub fn EmptyState(message: &'static str) -> impl IntoView {
    view! {
        <div class="flex flex-col items-center justify-center py-16 text-stone-500">
            <span class="text-3xl font-mono">{crate::locale::Translations::empty_state_icon()}</span>
            <p class="mt-3 text-sm">{message}</p>
        </div>
    }
}