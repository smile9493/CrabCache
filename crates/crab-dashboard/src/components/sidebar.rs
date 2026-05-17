use leptos::prelude::*;
use leptos_router::components::*;

use crate::auth::{clear_admin_key, use_admin_key};
use crate::components::gateway_health::GatewayHealthIndicator;
use crate::components::theme_switcher::ThemeSwitcher;
use crate::locale::{use_locale, use_translations, Translations};

pub fn provide_mobile_nav() -> RwSignal<bool> {
    let open = RwSignal::new(false);
    provide_context(open);
    open
}

fn use_mobile_nav() -> RwSignal<bool> {
    use_context::<RwSignal<bool>>().unwrap_or_else(|| RwSignal::new(false))
}

#[component]
pub fn MobileTopBar() -> impl IntoView {
    let nav_open = use_mobile_nav();

    view! {
        <header class="mobile-topbar">
            <button
                type="button"
                class="mobile-menu-btn"
                aria-label="Menu"
                on:click=move |_| nav_open.update(|o| *o = !*o)
            >
                {move || if nav_open.get() { "✕" } else { "☰" }}
            </button>
            <span class="mobile-topbar-title">{Translations::sidebar_brand}</span>
            <ThemeSwitcher />
        </header>
        {move || nav_open.get().then(|| view! {
            <button
                type="button"
                class="sidebar-backdrop"
                aria-label="Close menu"
                on:click=move |_| nav_open.set(false)
            />
        })}
    }
}

#[component]
pub fn Sidebar() -> impl IntoView {
    let locale = use_locale();
    let admin_key = use_admin_key();
    let nav_open = use_mobile_nav();

    let change_admin_key = move |_| {
        clear_admin_key();
        admin_key.set(String::new());
    };

    let toggle_locale = move |_| {
        locale.update(|l| *l = l.next());
    };

    let close_on_nav = move |_| nav_open.set(false);

    view! {
        <aside class=move || {
            if nav_open.get() {
                "sidebar sidebar-open"
            } else {
                "sidebar"
            }
        }>
            <div class="sidebar-header">
                <div class="flex items-center justify-between gap-2">
                    <div class="flex items-center gap-2.5 min-w-0">
                        <img src="/style/favicon.svg" alt="" class="brand-logo" width="28" height="28" />
                        <span class="sidebar-brand truncate">{Translations::sidebar_brand}</span>
                    </div>
                    <div class="hidden md:block">
                        <ThemeSwitcher />
                    </div>
                </div>
                <p class="sidebar-subtitle">{move || use_translations().sidebar_subtitle()}</p>
            </div>

            <nav class="sidebar-nav">
                <div class="nav-group-label">{move || use_translations().sidebar_group_monitor()}</div>
                <NavItem href="/" label=move || use_translations().sidebar_overview() icon="◉" on_navigate=close_on_nav />
                <NavItem href="/logs" label=move || use_translations().sidebar_logs() icon="▣" on_navigate=close_on_nav />

                <div class="nav-group-label">{move || use_translations().sidebar_group_config()}</div>
                <NavItem href="/keys" label=move || use_translations().sidebar_keys() icon="◆" on_navigate=close_on_nav />
                <NavItem href="/upstream" label=move || use_translations().sidebar_upstream() icon="⬡" on_navigate=close_on_nav />
                <NavItem href="/models" label=move || use_translations().sidebar_models() icon="◇" on_navigate=close_on_nav />
                <NavItem href="/routing" label=move || use_translations().sidebar_routing() icon="◈" on_navigate=close_on_nav />

                <div class="nav-group-label">{move || use_translations().sidebar_group_ops()}</div>
                <NavItem href="/cache" label=move || use_translations().sidebar_cache_ops() icon="◎" on_navigate=close_on_nav />
                <NavItem href="/trace" label=move || use_translations().sidebar_trace() icon="◐" on_navigate=close_on_nav />
            </nav>

            <div class="sidebar-footer">
                <GatewayHealthIndicator />
                <button
                    on:click=toggle_locale
                    class="sidebar-footer-btn"
                >
                    {move || format!("🌐 {}", locale.get().label())}
                </button>
                <button
                    on:click=change_admin_key
                    class="sidebar-footer-btn"
                >
                    {move || use_translations().sidebar_change_admin_key()}
                </button>
            </div>
        </aside>
    }
}

#[component]
fn NavItem(
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
                    "nav-item active"
                } else {
                    "nav-item"
                }
            }
        >
            <span class="nav-icon">{icon}</span>
            <span>{label()}</span>
        </A>
    }
}
