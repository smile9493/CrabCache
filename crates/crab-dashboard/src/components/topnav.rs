use leptos::prelude::*;
use leptos_router::components::*;

use crate::auth::{logout, use_admin_key};
use crate::components::brand_logo::BrandLogo;
use crate::components::gateway_health::GatewayHealthIndicator;
use crate::components::theme_switcher::ThemeSwitcher;
use crate::locale::{Translations, use_locale, use_translations};

fn init_mobile_nav() -> RwSignal<bool> {
    RwSignal::new(false)
}

#[component]
pub fn TopNav() -> impl IntoView {
    let locale = use_locale();
    let admin_key = use_admin_key();
    let nav_open = init_mobile_nav();

    let change_admin_key = move |_| {
        logout();
        admin_key.set(String::new());
    };

    let toggle_locale = move |_| {
        locale.update(|l| *l = l.next());
    };

    let close_on_nav = move |_| nav_open.set(false);

    view! {
        <header class="topnav">
            <div class="topnav-brand">
                <BrandLogo />
                <span class="topnav-brand-name brand-gradient-text">{Translations::sidebar_brand}</span>
                <span class="topnav-brand-version hidden md:inline">"v0.1.0"</span>
            </div>

            <nav class=move || {
                if nav_open.get() {
                    "topnav-nav topnav-nav-open"
                } else {
                    "topnav-nav"
                }
            }>
                <span class="topnav-group-label">{move || use_translations().sidebar_group_monitor()}</span>
                <TopNavItem href="/" label=move || use_translations().sidebar_overview() icon="◉" on_navigate=close_on_nav />
                <TopNavItem href="/live" label=move || use_translations().sidebar_live() icon="◔" on_navigate=close_on_nav />
                <TopNavItem href="/requests" label=move || use_translations().sidebar_requests() icon="▣" on_navigate=close_on_nav />
                <TopNavItem href="/sessions" label=move || "Session Monitor" icon="◎" on_navigate=close_on_nav />

                <span class="topnav-group-label">{move || use_translations().sidebar_group_config()}</span>
                <TopNavItem href="/keys" label=move || use_translations().sidebar_keys() icon="◆" on_navigate=close_on_nav />
                <TopNavItem href="/upstream" label=move || use_translations().sidebar_upstream() icon="⬡" on_navigate=close_on_nav />
                <TopNavItem href="/models" label=move || use_translations().sidebar_models() icon="◇" on_navigate=close_on_nav />
                <TopNavItem href="/cache" label=move || use_translations().sidebar_cache() icon="◈" on_navigate=close_on_nav />
                <TopNavItem href="/system" label=move || use_translations().sidebar_system() icon="⚙" on_navigate=close_on_nav />
            </nav>

            <div class="topnav-actions">
                <GatewayHealthIndicator />
                <button
                    on:click=toggle_locale
                    class="icon-btn"
                    title=move || locale.get().label()
                >
                    "🌐"
                </button>
                <ThemeSwitcher />
                <button
                    on:click=change_admin_key
                    class="icon-btn"
                    title=move || use_translations().sidebar_change_admin_key()
                >
                    "⏻"
                </button>
                <button
                    type="button"
                    class="topnav-mobile-toggle"
                    aria-label="Menu"
                    on:click=move |_| nav_open.update(|o| *o = !*o)
                >
                    {move || if nav_open.get() { "✕" } else { "☰" }}
                </button>
            </div>
        </header>

        {move || nav_open.get().then(|| view! {
            <button
                type="button"
                class="topnav-mobile-backdrop"
                aria-label="Close menu"
                on:click=move |_| nav_open.set(false)
            />
        })}
    }
}

#[component]
fn TopNavItem(
    href: &'static str,
    label: impl Fn() -> &'static str + Send + 'static,
    icon: &'static str,
    on_navigate: impl Fn(web_sys::MouseEvent) + 'static,
) -> impl IntoView {
    let location = leptos_router::hooks::use_location();
    let is_active = move || location.pathname.get() == href;

    view! {
        <A
            href=href
            on:click=on_navigate
            attr:class=move || {
                if is_active() {
                    "topnav-item active"
                } else {
                    "topnav-item"
                }
            }
        >
            <span class="topnav-icon">{icon}</span>
            <span>{label()}</span>
        </A>
    }
}
