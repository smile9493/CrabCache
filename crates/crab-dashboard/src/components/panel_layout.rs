use leptos::prelude::*;

/// A Sing-box-style card panel with a header (title + optional actions) and body.
#[component]
pub fn DashCard(
    title: String,
    #[prop(optional)] meta: Option<String>,
    #[prop(optional)] flush: bool,
    #[prop(optional)] children: Option<Children>,
    #[prop(optional)] actions: Option<Children>,
) -> impl IntoView {
    let body_class = if flush {
        "dash-card-body-flush"
    } else {
        "dash-card-body"
    };
    view! {
        <div class="dash-card">
            <div class="dash-card-header">
                <span class="dash-card-title">{title}</span>
                <div class="dash-card-actions">
                    {meta.map(|m| view! { <span class="panel-header-meta">{m}</span> })}
                    {actions.map(|a| view! { {a()} })}
                </div>
            </div>
            <div class=body_class>
                {children.map(|c| view! { {c()} })}
            </div>
        </div>
    }
}

/// A Sing-box-style card with only a body (no header), useful for embeds.
#[component]
pub fn DashCardBody(
    #[prop(optional)] flush: bool,
    children: Children,
) -> impl IntoView {
    let class = if flush {
        "dash-card-body-flush"
    } else {
        "dash-card-body"
    };
    view! {
        <div class="dash-card">
            <div class=class>{children()}</div>
        </div>
    }
}

/// Responsive grid wrapper (3-col / 2-col responsive).
#[component]
pub fn DashboardGrid(
    #[prop(default = 3)] cols: u8,
    #[prop(optional)] class_extra: Option<String>,
    children: Children,
) -> impl IntoView {
    let grid_class = match cols {
        2 => "dashboard-grid-2",
        _ => "dashboard-grid",
    };
    let cls = match class_extra {
        Some(extra) => format!("{grid_class} {extra}"),
        None => grid_class.to_string(),
    };
    view! { <div class=cls>{children()}</div> }
}

/// A clickable list row with left content and right actions.
#[component]
pub fn DashItem(
    #[prop(optional)] active: bool,
    left: Children,
    #[prop(optional)] meta: Option<String>,
    #[prop(optional)] actions: Option<Children>,
) -> impl IntoView {
    let class = if active {
        "dash-item active"
    } else {
        "dash-item"
    };
    view! {
        <div class=class>
            <div class="dash-item-left">
                {left()}
                {meta.map(|m| view! { <span class="dash-item-meta">{m}</span> })}
            </div>
            <div class="dash-item-actions">
                {actions.map(|a| view! { {a()} })}
            </div>
        </div>
    }
}

/// A generic pill bar for single-select options (e.g. time windows, tabs).
#[component]
pub fn DashPillBar(
    pills: Vec<String>,
    active: RwSignal<usize>,
    #[prop(optional)] small: bool,
) -> impl IntoView {
    let base = if small { "dash-pill dash-pill-sm" } else { "dash-pill" };
    view! {
        <div class="dash-pill-bar" role="tablist">
            {pills.into_iter().enumerate().map(|(i, label)| {
                let is_active = move || active.get() == i;
                let label_clone = label.clone();
                view! {
                    <button
                        type="button"
                        role="tab"
                        aria-selected=is_active
                        class=move || {
                            if is_active() {
                                format!("{base} dash-pill-active")
                            } else {
                                base.to_string()
                            }
                        }
                        on:click=move |_| active.set(i)
                    >
                        {label_clone}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}

/// A small stat tile with optional color variant (accent / teal / orange / green / muted / warn).
#[component]
pub fn StatTile(
    label: String,
    value: String,
    #[prop(default = "muted")] color: &'static str,
) -> impl IntoView {
    let color_class = format!("dash-stat-tile dash-stat-tile-{color}");
    view! {
        <div class=color_class>
            <span class="dash-stat-tile-label">{label}</span>
            <span class="dash-stat-tile-value">{value}</span>
        </div>
    }
}
