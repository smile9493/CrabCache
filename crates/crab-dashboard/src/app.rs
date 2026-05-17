use leptos::prelude::*;
use leptos_router::components::*;
use leptos_router::path;

use crate::auth::{is_authenticated, provide_admin_auth};
use crate::components::auth_gate::AuthGate;
use crate::components::sidebar::{MobileTopBar, Sidebar, provide_mobile_nav};
use crate::locale::{provide_locale, use_translations};
use crate::pages::cache_ops::CacheOpsPage;
use crate::pages::keys::KeysPage;
use crate::pages::logs::LogsPage;
use crate::pages::models::ModelsPage;
use crate::pages::overview::OverviewPage;
use crate::pages::routing::RoutingPage;
use crate::pages::trace::TracePage;
use crate::pages::upstream::UpstreamPage;
use crate::theme::provide_theme;

#[component]
pub fn App() -> impl IntoView {
    provide_locale();
    provide_theme();
    let admin_key = provide_admin_auth();

    view! {
        {move || {
            if is_authenticated(&admin_key) {
                view! { <AuthenticatedShell /> }.into_any()
            } else {
                view! { <AuthGate /> }.into_any()
            }
        }}
    }
}

#[component]
fn AuthenticatedShell() -> impl IntoView {
    provide_mobile_nav();
    view! {
        <Router>
            <div class="app-shell font-sans">
                <MobileTopBar />
                <Sidebar />
                <main class="main-content overflow-y-auto theme-scrollbar">
                    <Routes fallback=|| view! { <NotFound /> }>
                        <Route path=path!("/") view=OverviewPage />
                        <Route path=path!("/keys") view=KeysPage />
                        <Route path=path!("/models") view=ModelsPage />
                        <Route path=path!("/routing") view=RoutingPage />
                        <Route path=path!("/cache") view=CacheOpsPage />
                        <Route path=path!("/logs") view=LogsPage />
                        <Route path=path!("/trace") view=TracePage />
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
        <div class="page-content not-found-page">
            <div class="not-found-card glass-card-raised">
                <div class="not-found-code">{crate::locale::Translations::not_found_title()}</div>
                <p class="not-found-desc">{t.not_found_desc()}</p>
                <A href="/" attr:class="btn btn-primary text-sm">
                    {t.not_found_back()}
                </A>
            </div>
        </div>
    }
}
