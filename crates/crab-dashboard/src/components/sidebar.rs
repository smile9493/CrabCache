use leptos::prelude::*;
use leptos_router::components::*;

use crate::locale::{use_locale, use_translations, Translations};

#[component]
pub fn Sidebar() -> impl IntoView {
    let locale = use_locale();

    let toggle_locale = move |_| {
        locale.update(|l| *l = l.next());
    };

    view! {
        <aside class="w-56 bg-stone-900 border-r border-stone-800 flex flex-col shrink-0">
            <div class="px-5 py-5 border-b border-stone-800">
                <div class="flex items-center gap-2.5">
                    <span class="text-teal-500 text-xl font-bold font-mono">{Translations::sidebar_brand}</span>
                </div>
                <p class="text-[11px] text-stone-500 mt-1 font-mono">{move || use_translations().sidebar_subtitle()}</p>
            </div>

            <nav class="flex-1 px-3 py-4 space-y-1">
                <NavItem href="/" label=move || use_translations().sidebar_overview() icon="●" />
                <NavItem href="/keys" label=move || use_translations().sidebar_keys() icon="◆" />
                <NavItem href="/models" label=move || use_translations().sidebar_models() icon="◉" />
                <NavItem href="/routing" label=move || use_translations().sidebar_routing() icon="◈" />
                <NavItem href="/logs" label=move || use_translations().sidebar_logs() icon="▣" />
            </nav>

            <div class="px-4 py-3 border-t border-stone-800 space-y-2">
                <div class="flex items-center gap-2 text-xs text-stone-500">
                    <span class="w-1.5 h-1.5 rounded-full bg-teal-500 animate-pulse"></span>
                    {move || use_translations().sidebar_online()}
                </div>
                <button
                    on:click=toggle_locale
                    class="w-full text-xs text-stone-400 hover:text-stone-200 hover:bg-stone-800 rounded px-2 py-1 transition-colors text-left"
                >
                    {move || format!("🌐 {}", locale.get().label())}
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
                    "flex items-center gap-2.5 px-3 py-2 rounded-md text-sm font-medium \
                     bg-stone-800 text-teal-400 transition-colors"
                } else {
                    "flex items-center gap-2.5 px-3 py-2 rounded-md text-sm font-medium \
                     text-stone-400 hover:text-stone-200 hover:bg-stone-800/50 transition-colors"
                }
            }
        >
            <span class="text-xs font-mono">{icon}</span>
            <span>{label()}</span>
        </A>
    }
}