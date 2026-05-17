use leptos::prelude::*;

/// Page title row with optional action slot (refresh, buttons).
#[component]
pub fn PageHeader(
    title: impl Fn() -> &'static str + Send + 'static,
    description: impl Fn() -> &'static str + Send + 'static,
    children: Children,
) -> impl IntoView {
    view! {
        <header class="page-header">
            <div class="page-header-text">
                <h1 class="page-header-title">{title}</h1>
                <p class="page-header-desc">{description}</p>
            </div>
            <div class="page-header-actions">{children()}</div>
        </header>
    }
}
