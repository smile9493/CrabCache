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
use crate::pages::upstream::UpstreamPage;
use crate::theme::provide_theme;

#[component]
pub fn App() -> impl IntoView {
    provide_locale();
    provide_theme();

    view! {
        <Router>
            <div class="h-screen font-sans">
                <Sidebar />
                <main class="main-content overflow-y-auto theme-scrollbar">
                    <Routes fallback=|| view! { <NotFound /> }>
                        <Route path=path!("/") view=OverviewPage />
                        <Route path=path!("/keys") view=KeysPage />
                        <Route path=path!("/models") view=ModelsPage />
                        <Route path=path!("/routing") view=RoutingPage />
                        <Route path=path!("/logs") view=LogsPage />
                        <Route path=path!("/upstream") view=UpstreamPage />
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
        <div class="flex flex-col items-center justify-center py-24 bg-theme">
            <div class="text-6xl font-bold text-theme font-mono">{crate::locale::Translations::not_found_title()}</div>
            <p class="mt-4 text-theme-muted">{t.not_found_desc()}</p>
            <A href="/" attr:class="mt-6 text-sm text-accent hover:text-accent font-medium">
                {t.not_found_back()}
            </A>
        </div>
    }
}