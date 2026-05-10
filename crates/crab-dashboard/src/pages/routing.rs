use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{CacheConfig, ConnectionConfig, SemanticConfig, RoutingStatus};

#[component]
pub fn RoutingPage() -> impl IntoView {
    let t = use_translations();
    let cache_config: RwSignal<Option<Result<CacheConfig, String>>> = RwSignal::new(None);
    let semantic_config: RwSignal<Option<Result<SemanticConfig, String>>> = RwSignal::new(None);
    let connection_config: RwSignal<Option<Result<ConnectionConfig, String>>> = RwSignal::new(None);
    let routing_status: RwSignal<Option<Result<RoutingStatus, String>>> = RwSignal::new(None);

    leptos::task::spawn_local({
        let cache_config = cache_config.clone();
        async move {
            match api::fetch_cache_config().await {
                Ok(c) => cache_config.set(Some(Ok(c))),
                Err(e) => cache_config.set(Some(Err(e))),
            }
        }
    });

    leptos::task::spawn_local({
        let semantic_config = semantic_config.clone();
        async move {
            match api::fetch_semantic_config().await {
                Ok(c) => semantic_config.set(Some(Ok(c))),
                Err(e) => semantic_config.set(Some(Err(e))),
            }
        }
    });

    leptos::task::spawn_local({
        let connection_config = connection_config.clone();
        async move {
            match api::fetch_connection_config().await {
                Ok(c) => connection_config.set(Some(Ok(c))),
                Err(e) => connection_config.set(Some(Err(e))),
            }
        }
    });

    leptos::task::spawn_local({
        let routing_status = routing_status.clone();
        async move {
            match api::fetch_routing_status().await {
                Ok(s) => routing_status.set(Some(Ok(s))),
                Err(e) => routing_status.set(Some(Err(e))),
            }
        }
    });

    view! {
        <div class="p-6 space-y-6">
            <SectionHeader
                title=t.routing_title()
                description=t.routing_desc()
            />

            <div class="bento-grid-3">
                {move || match cache_config.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! { <div class="text-error text-sm">{format!("Error: {}", e)}</div> }.into_any(),
                    Some(Ok(config)) => view! { <CacheConfigPanel config /> }.into_any(),
                }}
                {move || match semantic_config.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! { <div class="text-error text-sm">{format!("Error: {}", e)}</div> }.into_any(),
                    Some(Ok(config)) => view! { <SemanticConfigPanel config /> }.into_any(),
                }}
                {move || match connection_config.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! { <div class="text-error text-sm">{format!("Error: {}", e)}</div> }.into_any(),
                    Some(Ok(config)) => view! { <ConnectionConfigPanel config /> }.into_any(),
                }}
            </div>

            {move || match routing_status.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! { <div class="text-error text-sm">{format!("Error: {}", e)}</div> }.into_any(),
                Some(Ok(status)) => view! { <RoutingStatusPanel status /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn CacheConfigPanel(config: CacheConfig) -> impl IntoView {
    let t = use_translations();
    let l0_ttl = RwSignal::new(config.l0_ttl_secs);
    let l1_ttl = RwSignal::new(config.l1_ttl_secs);
    let saved = RwSignal::new(false);

    let on_save = move |_| {
        let req = crate::types::UpdateCacheConfigRequest {
            l0_ttl_secs: l0_ttl.get(),
            l1_ttl_secs: l1_ttl.get(),
        };
        leptos::task::spawn_local(async move {
            let _ = api::update_cache_config(&req).await;
            saved.set(true);
        });
    };

    view! {
        <div class="glass-card space-y-4">
            <h3 class="text-sm font-semibold text-theme">{t.routing_cache_config_title()}</h3>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {}s", t.routing_l0_ttl(), l0_ttl.get())}
                </label>
                <input
                    type="range" min="10" max="3600"
                    prop:value=move || l0_ttl.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() { l0_ttl.set(v); }
                    }
                    class="w-full accent-teal-500"
                />
                <div class="flex justify-between text-xs text-theme-muted">
                    <span>"10s"</span><span>"3600s"</span>
                </div>
            </div>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {}s", t.routing_l1_ttl(), l1_ttl.get())}
                </label>
                <input
                    type="range" min="60" max="86400"
                    prop:value=move || l1_ttl.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() { l1_ttl.set(v); }
                    }
                    class="w-full accent-amber-500"
                />
                <div class="flex justify-between text-xs text-theme-muted">
                    <span>"60s"</span><span>"86400s"</span>
                </div>
            </div>
            <div class="flex items-center gap-3">
                <button on:click=on_save class="btn btn-primary text-sm">{t.routing_save()}</button>
                {move || if saved.get() {
                    view! { <span class="text-xs text-accent">{t.routing_saved()}</span> }.into_any()
                } else { view! { <span></span> }.into_any() }}
            </div>
        </div>
    }
}

#[component]
fn SemanticConfigPanel(config: SemanticConfig) -> impl IntoView {
    let t = use_translations();
    let threshold = RwSignal::new(config.similarity_threshold);
    let saved = RwSignal::new(false);

    let on_save = move |_| {
        let req = crate::types::UpdateSemanticConfigRequest {
            similarity_threshold: threshold.get(),
        };
        leptos::task::spawn_local(async move {
            let _ = api::update_semantic_config(&req).await;
            saved.set(true);
        });
    };

    view! {
        <div class="glass-card space-y-4">
            <h3 class="text-sm font-semibold text-theme">{t.routing_semantic_title()}</h3>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {:.2}", t.routing_similarity(), threshold.get())}
                </label>
                <input
                    type="range" min="0.80" max="0.99" step="0.01"
                    prop:value=move || threshold.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() { threshold.set(v); }
                    }
                    class="w-full accent-violet-500"
                />
                <div class="flex justify-between text-xs text-theme-muted">
                    <span>{t.routing_loose()}</span><span>{t.routing_strict()}</span>
                </div>
            </div>
            <div class="bg-theme-tertiary rounded-md p-3">
                <div class="text-xs text-theme-secondary mb-1">{t.routing_impact_label()}</div>
                <div class="text-sm text-theme">
                    {move || {
                        let th = threshold.get();
                        if th > 0.95 { t.routing_impact_high() }
                        else if th > 0.90 { t.routing_impact_balanced() }
                        else { t.routing_impact_loose() }
                    }}
                </div>
            </div>
            <div class="flex items-center gap-3">
                <button on:click=on_save class="btn btn-primary text-sm">{t.routing_save()}</button>
                {move || if saved.get() {
                    view! { <span class="text-xs text-accent">{t.routing_saved()}</span> }.into_any()
                } else { view! { <span></span> }.into_any() }}
            </div>
        </div>
    }
}

#[component]
fn ConnectionConfigPanel(config: ConnectionConfig) -> impl IntoView {
    let t = use_translations();
    let idle = RwSignal::new(config.tcp_keepalive_idle_secs);
    let interval = RwSignal::new(config.tcp_keepalive_interval_secs);
    let count = RwSignal::new(config.tcp_keepalive_count);
    let timeout = RwSignal::new(config.idle_timeout_secs);
    let h2_ping = RwSignal::new(config.h2_ping_interval_secs);
    let saved = RwSignal::new(false);

    let on_save = move |_| {
        let req = crate::types::UpdateConnectionConfigRequest {
            tcp_keepalive_idle_secs: idle.get(),
            tcp_keepalive_interval_secs: interval.get(),
            tcp_keepalive_count: count.get(),
            idle_timeout_secs: timeout.get(),
            h2_ping_interval_secs: h2_ping.get(),
        };
        leptos::task::spawn_local(async move {
            let _ = api::update_connection_config(&req).await;
            saved.set(true);
        });
    };

    view! {
        <div class="glass-card space-y-4">
            <h3 class="text-sm font-semibold text-theme">{t.routing_connection_title()}</h3>
            <p class="text-xs text-theme-muted">{t.routing_connection_desc()}</p>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {}s", t.routing_tcp_keepalive_idle(), idle.get())}
                </label>
                <input
                    type="range" min="10" max="300"
                    prop:value=move || idle.get()
                    on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { idle.set(v); } }
                    class="w-full accent-teal-500"
                />
            </div>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {}s", t.routing_tcp_keepalive_interval(), interval.get())}
                </label>
                <input
                    type="range" min="1" max="60"
                    prop:value=move || interval.get()
                    on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { interval.set(v); } }
                    class="w-full accent-amber-500"
                />
            </div>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {}", t.routing_tcp_keepalive_count(), count.get())}
                </label>
                <input
                    type="range" min="1" max="10"
                    prop:value=move || count.get()
                    on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { count.set(v); } }
                    class="w-full accent-violet-500"
                />
            </div>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {}s", t.routing_idle_timeout(), timeout.get())}
                </label>
                <input
                    type="range" min="10" max="600"
                    prop:value=move || timeout.get()
                    on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { timeout.set(v); } }
                    class="w-full accent-rose-500"
                />
            </div>
            <div>
                <label class="block text-xs text-theme-muted mb-1">
                    {move || format!("{}: {}s", t.routing_h2_ping_interval(), h2_ping.get())}
                </label>
                <input
                    type="range" min="5" max="120"
                    prop:value=move || h2_ping.get()
                    on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse() { h2_ping.set(v); } }
                    class="w-full accent-teal-500"
                />
            </div>
            <div class="flex items-center gap-3">
                <button on:click=on_save class="btn btn-primary text-sm">{t.routing_save()}</button>
                {move || if saved.get() {
                    view! { <span class="text-xs text-accent">{t.routing_saved()}</span> }.into_any()
                } else { view! { <span></span> }.into_any() }}
            </div>
        </div>
    }
}

#[component]
fn RoutingStatusPanel(status: RoutingStatus) -> impl IntoView {
    let t = use_translations();
    let total = status.total_backends.max(1) as f64;

    view! {
        <div class="glass-card space-y-5">
            <h3 class="text-sm font-semibold text-theme">{t.routing_affinity_title()}</h3>
            <div class="grid grid-cols-2 gap-4">
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.routing_active_backends()}</div>
                    <div class="text-xl font-mono tabular-nums text-theme">{format!("{}", status.active_backends)}</div>
                </div>
                <div>
                    <div class="text-xs text-theme-muted mb-1">{t.routing_total_backends()}</div>
                    <div class="text-xl font-mono tabular-nums text-theme">{format!("{}", status.total_backends)}</div>
                </div>
            </div>
            <div>
                <div class="text-xs text-theme-secondary mb-3">{t.routing_distribution()}</div>
                <div class="space-y-2">
                    {status.backends.into_iter().map(|backend| {
                        let pct = backend.request_count as f64 / total * 100.0;
                        let fill_class = if backend.healthy { "progress-bar-fill" } else { "bg-error" };
                        view! {
                            <div class="flex items-center gap-3">
                                <span class="w-24 text-xs text-theme-secondary truncate">{backend.name}</span>
                                <div class="flex-1 progress-bar h-2">
                                    <div
                                        class=format!("h-full rounded-full {}", fill_class)
                                        style=format!("width: {}%", pct.min(100.0))
                                    ></div>
                                </div>
                                <span class="w-16 text-xs font-mono tabular-nums text-theme text-right">
                                    {format!("{}", backend.request_count)}
                                </span>
                                <span class="w-12 text-xs text-theme-muted text-right">{format!("{:.0}%", pct)}</span>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>
        </div>
    }
}