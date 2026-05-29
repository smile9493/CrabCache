use leptos::prelude::*;
use leptos_router::components::*;
use leptos_router::path;

use crate::auth::{is_authenticated, provide_admin_auth};
use crate::components::auth_gate::AuthGate;
use crate::components::toast::{ToastContainer, provide_toast};
use crate::components::topnav::TopNav;
use crate::locale::{provide_locale, use_translations};
use crate::pages::cache::CachePage;
use crate::pages::keys::KeysPage;
use crate::pages::live::LivePage;
use crate::pages::models::ModelsPage;
use crate::pages::overview::OverviewPage;
use crate::pages::requests::RequestsPage;
use crate::pages::system::SystemPage;
use crate::pages::upstream::UpstreamPage;
use crate::table_density::provide_table_density;
use crate::theme::provide_theme;

/// Global panic signal — set to `Some(message)` when a WASM panic is caught.
static PANIC_SIGNAL: std::sync::OnceLock<RwSignal<Option<String>>> = std::sync::OnceLock::new();

/// Guard against multiple hook installations (component re-renders).
static PANIC_HOOK_INSTALLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Install a panic hook that captures panics into a Leptos signal.
/// Safe to call multiple times — the hook is installed only once.
fn install_panic_hook() {
    if PANIC_HOOK_INSTALLED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return; // Already installed.
    }
    let signal = PANIC_SIGNAL.get_or_init(|| RwSignal::new(None));
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let full = format!("WASM panic: {msg}{location}");
        web_sys::console::error_1(&full.clone().into());
        signal.set(Some(full));
        // Also call the previous hook (console_error_panic_hook).
        prev_hook(info);
    }));
}

#[component]
pub fn App() -> impl IntoView {
    provide_locale();
    provide_theme();
    provide_table_density();
    // Provide toast context at the app root so background callbacks can safely
    // clone the signal handle and avoid `use_context` panics.
    provide_toast();
    install_panic_hook();
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
    let panic = PANIC_SIGNAL.get_or_init(|| RwSignal::new(None));

    view! {
        {move || {
            if let Some(msg) = panic.get() {
                view! { <CrashRecoveryPage message=msg /> }.into_any()
            } else {
                view! {
                    <Router>
                        <div class="app-shell font-sans">
                            <TopNav />
                            <main class="main-content overflow-y-auto theme-scrollbar">
                                <Routes fallback=|| view! { <NotFound /> }>
                                    <Route path=path!("/") view=OverviewPage />
                                    <Route path=path!("/live") view=LivePage />
                                    <Route path=path!("/infra") view=InfraRedirectPage />
                                    <Route path=path!("/keys") view=KeysPage />
                                    <Route path=path!("/models") view=ModelsPage />
                                    <Route path=path!("/system") view=SystemPage />
                                    <Route path=path!("/cache") view=CachePage />
                                    <Route path=path!("/requests") view=RequestsPage />
                                    <Route path=path!("/sessions") view=SessionsRedirectPage />
                                    <Route path=path!("/upstream") view=UpstreamPage />
                                </Routes>
                            </main>
                            <ToastContainer />
                        </div>
                    </Router>
                }.into_any()
            }
        }}
    }
}

#[component]
fn SessionsRedirectPage() -> impl IntoView {
    Effect::new(move |_| {
        if let Some(win) = web_sys::window() {
            let _ = win.location().set_href("/live");
        }
    });
    view! {
        <div class="page-content">
            <div class="glass-card text-sm text-theme-muted">"Redirecting to Live..."</div>
        </div>
    }
}

#[component]
fn InfraRedirectPage() -> impl IntoView {
    Effect::new(move |_| {
        if let Some(win) = web_sys::window() {
            let _ = win.location().set_href("/");
        }
    });
    view! {
        <div class="page-content">
            <div class="glass-card text-sm text-theme-muted">"Redirecting to Overview..."</div>
        </div>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="page-content not-found-page">
            <div class="not-found-card">
                <div class="not-found-code">{crate::locale::Translations::not_found_title()}</div>
                <p class="not-found-desc">{t.not_found_desc()}</p>
                <A href="/" attr:class="btn btn-primary text-sm">
                    {t.not_found_back()}
                </A>
            </div>
        </div>
    }
}

/// Recovery page shown when a WASM panic is caught.
/// Displays a user-friendly message with a reload button and optional error details.
#[component]
fn CrashRecoveryPage(message: String) -> impl IntoView {
    let show_details = RwSignal::new(false);

    view! {
        <div class="page-content flex items-center justify-center min-h-screen">
            <div class="glass-card max-w-lg mx-auto text-center p-8 space-y-6">
                <div class="text-4xl">"⚠"</div>
                <h1 class="text-xl font-semibold text-theme">"Something went wrong"</h1>
                <p class="text-sm text-theme-muted">
                    "The dashboard encountered an unexpected error and cannot continue. "
                    "Reloading the page will usually fix this."
                </p>
                <div class="flex justify-center gap-3">
                    <button
                        class="btn btn-primary text-sm"
                        on:click=move |_| {
                            if let Some(win) = web_sys::window() {
                                let _ = win.location().reload();
                            }
                        }
                    >
                        "Reload Page"
                    </button>
                    <button
                        class="btn btn-secondary text-sm"
                        on:click=move |_| show_details.update(|v| *v = !*v)
                    >
                        {move || if show_details.get() { "Hide Details" } else { "Show Details" }}
                    </button>
                </div>
                {move || show_details.get().then(|| view! {
                    <pre class="text-xs text-left text-theme-muted bg-theme-surface p-3 rounded-lg overflow-x-auto max-h-48 overflow-y-auto font-mono">
                        {message.clone()}
                    </pre>
                })}
            </div>
        </div>
    }
}
