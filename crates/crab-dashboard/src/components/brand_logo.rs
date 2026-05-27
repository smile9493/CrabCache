use leptos::prelude::*;

const BRAND_SRC: &str = "/brand.png";

/// Brand mark shipped as `style/brand.png` (Trunk copies to `/brand.png` in dist).
#[component]
pub fn BrandLogo(#[prop(default = false)] large: bool) -> impl IntoView {
    let (cls, size) = if large {
        ("brand-logo brand-logo-lg shrink-0", 40u16)
    } else {
        ("brand-logo shrink-0", 32u16)
    };

    view! {
        <img
            class=cls
            src=BRAND_SRC
            width=size
            height=size
            alt=""
            loading="eager"
            decoding="async"
        />
    }
}
