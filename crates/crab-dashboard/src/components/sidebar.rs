use leptos::prelude::*;
use leptos_router::components::*;

use crate::auth::{clear_admin_key, use_admin_key};
use crate::components::theme_switcher::ThemeSwitcher;
use crate::locale::{use_locale, use_translations, Translations};

#[component]
pub fn Sidebar() -> impl IntoView {
    let locale = use_locale();
    let admin_key = use_admin_key();

    let change_admin_key = move |_| {
        clear_admin_key();
        admin_key.set(String::new());
    };

    let toggle_locale = move |_| {
        locale.update(|l| *l = l.next());
    };

    view! {
        <aside class="sidebar">
            <div class="sidebar-header">
                <div class="flex items-center justify-between">
                    <div class="flex items-center gap-2.5">
                        <span class="sidebar-brand">{Translations::sidebar_brand}</span>
                    </div>
                    <ThemeSwitcher />
                </div>
                <p class="sidebar-subtitle">{move || use_translations().sidebar_subtitle()}</p>
            </div>

            <nav class="sidebar-nav space-y-1">
                <NavItem href="/" label=move || use_translations().sidebar_overview() icon="●" />
                <NavItem href="/keys" label=move || use_translations().sidebar_keys() icon="◆" />
                <NavItem href="/upstream" label=move || use_translations().sidebar_upstream() icon="⬡" />
                <NavItem href="/models" label=move || use_translations().sidebar_models() icon="◉" />
                <NavItem href="/routing" label=move || use_translations().sidebar_routing() icon="◈" />
                <NavItem href="/cache" label=move || use_translations().sidebar_cache_ops() icon="◎" />
                <NavItem href="/logs" label=move || use_translations().sidebar_logs() icon="▣" />
                <NavItem href="/trace" label=move || use_translations().sidebar_trace() icon="◈" />
            </nav>

            <div class="sidebar-footer">
                <div class="flex items-center gap-2 mb-2">
                    <span class="online-dot"></span>
                    <span class="online-label">{move || use_translations().sidebar_online()}</span>
                </div>
                <button
                    on:click=toggle_locale
                    class="text-xs text-theme-muted hover:text-theme transition-colors w-full text-left py-1"
                >
                    {move || format!("🌐 {}", locale.get().label())}
                </button>
                <button
                    on:click=change_admin_key
                    class="text-xs text-theme-muted hover:text-theme transition-colors w-full text-left py-1"
                >
                    {move || use_translations().sidebar_change_admin_key()}
                </button>
            </div>
        </aside>
    }
}

#[component]
fn NavItem(href: &'static str, label: impl Fn() -> &'static str + Send + 'static, icon: &'static str) -> impl IntoView {
    let location = leptos_router::hooks::use_location();
    let is_active = move || location.pathname.get() == href;

    view! {
        <A
            href=href
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