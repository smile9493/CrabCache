use leptos::prelude::*;
use leptos_router::components::*;
use leptos_router::path;

use crate::components::sidebar::Sidebar;
use crate::locale::{provide_locale, use_translations};
use crate::pages::keys::KeysPage;
use crate::pages::logs::LogsPage;
use crate::pages::models::ModelsPage;
use crate::pages::overview::OverviewPage;
use crate::pages::routing::RoutingPage;

#[component]
pub fn App() -> impl IntoView {
    provide_locale();

    view! {
        <Router>
            <div class="flex h-screen bg-stone-950 text-stone-200 font-sans">
                <Sidebar />
                <main class="flex-1 overflow-y-auto">
                    <Routes fallback=|| view! { <NotFound /> }>
                        <Route path=path!("/") view=OverviewPage />
                        <Route path=path!("/keys") view=KeysPage />
                        <Route path=path!("/models") view=ModelsPage />
                        <Route path=path!("/routing") view=RoutingPage />
                        <Route path=path!("/logs") view=LogsPage />
                    </Routes>
                </main>
            </div>
        </Router>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="flex flex-col items-center justify-center py-24">
            <div class="text-6xl font-bold text-stone-200 font-mono">{crate::locale::Translations::not_found_title()}</div>
            <p class="mt-4 text-stone-500">{t.not_found_desc()}</p>
            <A href="/" attr:class="mt-6 text-sm text-teal-600 hover:text-teal-700 font-medium">
                {t.not_found_back()}
            </A>
        </div>
    }
}