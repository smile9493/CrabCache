use leptos::prelude::*;
use leptos_router::components::*;

use crate::auth::{logout, use_admin_key};
use crate::components::brand_logo::BrandLogo;
use crate::components::gateway_health::GatewayHealthIndicator;
use crate::components::icons::{Icon, IconName};
use crate::components::theme_switcher::ThemeSwitcher;
use crate::locale::{Translations, use_locale, use_translations};
use crate::table_density::{TableDensity, use_table_density};

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
                <span class="topnav-brand-name brand-text">{Translations::sidebar_brand}</span>
                <span class="topnav-brand-version hidden md:inline">{format!("v{}", env!("DASHBOARD_PKG_VERSION"))}</span>
            </div>

            <nav class=move || {
                if nav_open.get() {
                    "topnav-nav topnav-nav-open"
                } else {
                    "topnav-nav"
                }
            }>
                <span class="topnav-group-label">{move || use_translations().sidebar_group_monitor()}</span>
                <TopNavItem href="/" label=move || use_translations().sidebar_overview() icon=IconName::LayoutDashboard on_navigate=close_on_nav />
                <TopNavItem href="/live" label=move || use_translations().sidebar_live() icon=IconName::Activity on_navigate=close_on_nav />
                <TopNavItem href="/requests" label=move || use_translations().sidebar_requests() icon=IconName::ListChecks on_navigate=close_on_nav />
                <TopNavItem href="/dataplane" label="Data Plane" icon=IconName::Radar on_navigate=close_on_nav />

                <span class="topnav-group-label">{move || use_translations().sidebar_group_config()}</span>
                <TopNavItem href="/keys" label=move || use_translations().sidebar_keys() icon=IconName::KeyRound on_navigate=close_on_nav />
                <TopNavItem href="/upstream" label=move || use_translations().sidebar_upstream() icon=IconName::PlugZap on_navigate=close_on_nav />
                <TopNavItem href="/models" label=move || use_translations().sidebar_models() icon=IconName::Boxes on_navigate=close_on_nav />
                <TopNavItem href="/cache" label=move || use_translations().sidebar_cache() icon=IconName::Database on_navigate=close_on_nav />
                <TopNavItem href="/system" label=move || use_translations().sidebar_system() icon=IconName::Settings on_navigate=close_on_nav />
            </nav>

            <div class="topnav-actions">
                <GatewayHealthIndicator />
                <button
                    on:click=toggle_locale
                    class="icon-btn"
                    title=move || locale.get().label()
                >
                    <Icon name=IconName::Globe class="icon" />
                </button>
                <ThemeSwitcher />
                {move || {
                    let density = use_table_density();
                    let t = use_translations();
                    let label = if density.get() == TableDensity::Compact { t.table_density_comfortable() } else { t.table_density_compact() };
                    view! {
                        <button
                            on:click=move |_| {
                                let next = if density.get() == TableDensity::Compact {
                                    TableDensity::Comfortable
                                } else {
                                    TableDensity::Compact
                                };
                                density.set(next);
                            }
                            class="icon-btn"
                            title=label
                        >
                            <Icon name=IconName::Table class="icon" />
                        </button>
                    }
                }}
                <button
                    on:click=change_admin_key
                    class="icon-btn"
                    title=move || use_translations().sidebar_change_admin_key()
                >
                    <Icon name=IconName::Power class="icon" />
                </button>
                <button
                    type="button"
                    class="topnav-mobile-toggle"
                    aria-label="Menu"
                    on:click=move |_| nav_open.update(|o| *o = !*o)
                >
                    {move || if nav_open.get() {
                        view! { <Icon name=IconName::X class="icon" /> }.into_any()
                    } else {
                        view! { <Icon name=IconName::Menu class="icon" /> }.into_any()
                    }}
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
    icon: IconName,
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
            <span class="topnav-icon"><Icon name=icon class="icon" /></span>
            <span>{label()}</span>
        </A>
    }
}
