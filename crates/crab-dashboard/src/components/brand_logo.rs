use leptos::prelude::*;

/// Inline SVG crab mark — avoids path-resolution issues with Trunk asset hashing.
#[component]
pub fn BrandLogo(#[prop(default = false)] large: bool) -> impl IntoView {
    let (cls, size) = if large {
        ("brand-logo brand-logo-lg shrink-0", "40")
    } else {
        ("brand-logo shrink-0", "32")
    };
    view! {
        <svg
            class=cls
            width=size
            height=size
            viewBox="0 0 32 32"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
            aria-hidden="true"
            role="img"
        >
            <rect width="32" height="32" rx="8" fill="#1a1814"/>
            <path
                d="M6 14c2-3 4-4 6-3.5 1.5.4 2.5 1.5 3 3 .8-1.2 \
                   2-2 4-2 2.2 0 4 1.2 5 3.2 1.2-1 2.8-1.5 4.5-1 \
                   2.5.7 4 2.8 4 5.5 0 4.5-3.5 8-8 8H10c-3.5 0-6-2.5 \
                   -6-6 0-2.2 1-4.2 2-4.2z"
                fill="#c9a227"
                opacity=".95"
            />
            <circle cx="11" cy="17" r="1.1" fill="#1a1814"/>
            <circle cx="21" cy="17" r="1.1" fill="#1a1814"/>
            <path
                d="M4 12l-2 1.5M28 12l2 1.5M5 20l-2.5 2M27 20l2.5 2"
                stroke="#c9a227"
                stroke-width="1.2"
                stroke-linecap="round"
            />
        </svg>
    }
}
