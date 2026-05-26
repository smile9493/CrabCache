//! Routing & Health tab for the Upstream page.

use leptos::prelude::*;
use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::ProfileRoutingView;

/// Renders the "路由与健康" tab content for a given profile.
#[component]
pub fn RoutingTab(profile_id: String) -> impl IntoView {
    let routing: RwSignal<Option<ProfileRoutingView>> = RwSignal::new(None);
    let error: RwSignal<String> = RwSignal::new(String::new());
    let loading: RwSignal<bool> = RwSignal::new(true);

    // Fetch routing data when profile_id changes.
    let pid = profile_id.clone();
    Effect::new(move |_| {
        let pid = pid.clone();
        loading.set(true);
        error.set(String::new());
        leptos::task::spawn_local(async move {
            match api::fetch_profile_routing(&pid).await {
                Ok(data) => {
                    routing.set(Some(data));
                    loading.set(false);
                }
                Err(e) => {
                    error.set(e);
                    loading.set(false);
                }
            }
        });
    });

    // Auto-refresh every 10 seconds.
    let pid_refresh = profile_id.clone();
    let _refresh_handle = set_interval_with_handle(
        move || {
            let pid = pid_refresh.clone();
            leptos::task::spawn_local(async move {
                if let Ok(data) = api::fetch_profile_routing(&pid).await {
                    routing.set(Some(data));
                }
            });
        },
        std::time::Duration::from_secs(10),
    )
    .ok();

    view! {
        <div class="space-y-4">
            // Header status bar
            {move || {
                let t = use_translations();
                match routing.get() {
                    None if loading.get() => view! { <Spinner /> }.into_any(),
                    None => view! { <span></span> }.into_any(),
                    Some(ref data) => {
                        let healthy_count = data.backends.iter().filter(|b| b.healthy).count();
                        let total = data.backends.len();
                        let open_count = data.backends.iter().filter(|b| b.circuit_state == "open").count();
                        let keys_avail = data.key_pool.available;
                        let keys_total = data.key_pool.total;
                        view! {
                            <div class="glass-card">
                                <div class="flex flex-wrap items-center gap-4 text-sm">
                                    <span class="font-semibold text-theme">{data.profile_id.clone()}</span>
                                    <span class={move || if healthy_count == total && total > 0 {
                                        "text-success"
                                    } else {
                                        "text-warning"
                                    }}>
                                        {format!("{}/{} {}", healthy_count, total, t.routing_summary_healthy_label())}
                                    </span>
                                    {if open_count > 0 {
                                        view! {
                                            <span class="text-error font-medium">
                                                {format!("{} {}", open_count, t.routing_summary_circuit_label())}
                                            </span>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }}
                                    <span class="text-theme-muted">
                                        {t.routing_key_pool_summary(keys_avail, keys_total)}
                                    </span>
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}

            // Error display
            {move || {
                let e = error.get();
                if !e.is_empty() {
                    view! { <p class="text-xs text-error">{e}</p> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            // Backends table
            {move || {
                let t = use_translations();
                match routing.get() {
                    None => view! { <span></span> }.into_any(),
                    Some(ref data) => {
                        if data.backends.is_empty() {
                            return view! {
                                <div class="glass-card text-sm text-theme-muted text-center py-8">
                                    {t.routing_no_backends()}
                                </div>
                            }.into_any();
                        }
                        view! {
                            <div class="glass-card">
                                <h3 class="text-base font-semibold text-theme mb-3">
                                    {format!("Ketama {}", t.routing_backend_health())}
                                </h3>
                                <div class="overflow-x-auto">
                                    <table class="table text-sm">
                                        <thead>
                                            <tr>
                                                <th>{t.routing_backend_name()}</th>
                                                <th>{t.routing_backend_addr()}</th>
                                                <th>{t.routing_backend_weight()}</th>
                                                <th>{t.routing_backend_health()}</th>
                                                <th>{t.routing_backend_circuit()}</th>
                                                <th>{t.routing_backend_failures()}</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {data.backends.iter().map(|b| {
                                                let circuit_badge = match b.circuit_state.as_str() {
                                                    "closed" => view! {
                                                        <span class="inline-block px-2 py-0.5 rounded text-xs bg-success/20 text-success">
                                                            {t.routing_circuit_closed()}
                                                        </span>
                                                    }.into_any(),
                                                    "open" => view! {
                                                        <span class="inline-block px-2 py-0.5 rounded text-xs bg-error/20 text-error">
                                                            {t.routing_circuit_open()}
                                                        </span>
                                                    }.into_any(),
                                                    "half_open" | "halfopen" => view! {
                                                        <span class="inline-block px-2 py-0.5 rounded text-xs bg-warning/20 text-warning">
                                                            {t.routing_circuit_half_open()}
                                                        </span>
                                                    }.into_any(),
                                                    _ => view! {
                                                        <span class="inline-block px-2 py-0.5 rounded text-xs bg-theme-muted/20 text-theme-muted">
                                                            {b.circuit_state.clone()}
                                                        </span>
                                                    }.into_any(),
                                                };
                                                let health_badge = if b.healthy {
                                                    view! {
                                                        <span class="inline-block px-2 py-0.5 rounded text-xs bg-success/20 text-success">
                                                            {t.routing_health_healthy()}
                                                        </span>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <span class="inline-block px-2 py-0.5 rounded text-xs bg-error/20 text-error">
                                                            {t.routing_health_unhealthy()}
                                                        </span>
                                                    }.into_any()
                                                };
                                                let failure_class = if b.circuit_state != "closed" && b.consecutive_failures > 0 {
                                                    "text-error font-medium"
                                                } else {
                                                    "text-theme-muted"
                                                };
                                                view! {
                                                    <tr>
                                                        <td class="font-mono text-xs">{b.name.clone()}</td>
                                                        <td class="font-mono text-xs text-theme-muted">{b.addr.clone()}</td>
                                                        <td class="text-center">{b.weight}</td>
                                                        <td>
                                                            {health_badge}
                                                            {if b.latency_ms > 0 {
                                                                view! {
                                                                    <span class="text-xs text-theme-muted ml-1">
                                                                        {t.routing_latency_ms(b.latency_ms)}
                                                                    </span>
                                                                }.into_any()
                                                            } else {
                                                                view! { <span></span> }.into_any()
                                                            }}
                                                        </td>
                                                        <td>{circuit_badge}</td>
                                                        <td class={failure_class}>{b.consecutive_failures}</td>
                                                    </tr>
                                                }
                                            }).collect_view()}
                                        </tbody>
                                    </table>
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}

            // Key pool summary
            {move || {
                let t = use_translations();
                match routing.get() {
                    None => view! { <span></span> }.into_any(),
                    Some(ref data) => {
                        view! {
                            <div class="glass-card">
                                <div class="flex items-center justify-between mb-2">
                                    <h3 class="text-base font-semibold text-theme">
                                        {t.routing_key_pool_summary(data.key_pool.available, data.key_pool.total)}
                                    </h3>
                                    <a
                                        href="/upstream?tab=keys"
                                        class="text-xs text-accent hover:underline cursor-pointer"
                                    >
                                        {t.routing_manage_keys_link()}
                                    </a>
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}

            // Circuit breaker config (read-only)
            {move || {
                let t = use_translations();
                match routing.get() {
                    None => view! { <span></span> }.into_any(),
                    Some(ref data) => {
                        let cb = &data.circuit_breaker;
                        view! {
                            <details class="glass-card">
                                <summary class="text-sm font-semibold text-theme cursor-pointer">
                                    {t.routing_circuit_breaker_title()}
                                </summary>
                                <div class="mt-2 space-y-1 text-xs text-theme-muted">
                                    <p>{t.routing_circuit_breaker_desc()}</p>
                                    <div class="grid grid-cols-3 gap-2 mt-2">
                                        <div>
                                            <span class="font-medium text-theme">failure_threshold</span>
                                            <span class="ml-1">{cb.failure_threshold}</span>
                                        </div>
                                        <div>
                                            <span class="font-medium text-theme">success_threshold</span>
                                            <span class="ml-1">{cb.success_threshold}</span>
                                        </div>
                                        <div>
                                            <span class="font-medium text-theme">timeout</span>
                                            <span class="ml-1">{format!("{}s", cb.timeout_ms / 1000)}</span>
                                        </div>
                                    </div>
                                </div>
                            </details>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}
