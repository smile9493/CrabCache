use leptos::prelude::*;

/// Brand crab mark from favicon.svg (replaces emoji for cross-platform visibility).
#[component]
pub fn BrandLogo(#[prop(default = false)] large: bool) -> impl IntoView {
    let class = if large {
        "brand-logo brand-logo-lg shrink-0"
    } else {
        "brand-logo shrink-0"
    };
    view! {
        <img
            src="/style/favicon.svg"
            alt=""
            class=class
            width="32"
            height="32"
            aria-hidden="true"
        />
    }
}
