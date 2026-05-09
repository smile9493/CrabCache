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

            <div class="grid grid-cols-3 gap-6">
                {move || match cache_config.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="text-rose-400 text-sm">{format!("Error: {}", e)}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! { <CacheConfigPanel config /> }.into_any(),
                }}

                {move || match semantic_config.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="text-rose-400 text-sm">{format!("Error: {}", e)}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! { <SemanticConfigPanel config /> }.into_any(),
                }}

                {move || match connection_config.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="text-rose-400 text-sm">{format!("Error: {}", e)}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! { <ConnectionConfigPanel config /> }.into_any(),
                }}
            </div>

            {move || match routing_status.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="text-rose-400 text-sm">{format!("Error: {}", e)}</div>
                }.into_any(),
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
        <div class="bg-stone-900 border border-stone-800 rounded-lg p-5 space-y-4">
            <h3 class="text-sm font-semibold text-stone-200">{t.routing_cache_config_title()}</h3>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {}s", t.routing_l0_ttl(), l0_ttl.get())}
                </label>
                <input
                    type="range"
                    min="10"
                    max="3600"
                    prop:value=move || l0_ttl.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            l0_ttl.set(v);
                        }
                    }
                    class="w-full accent-teal-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>"10s"</span>
                    <span>"3600s"</span>
                </div>
            </div>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {}s", t.routing_l1_ttl(), l1_ttl.get())}
                </label>
                <input
                    type="range"
                    min="60"
                    max="86400"
                    prop:value=move || l1_ttl.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            l1_ttl.set(v);
                        }
                    }
                    class="w-full accent-amber-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>"60s"</span>
                    <span>"86400s"</span>
                </div>
            </div>

            <div class="flex items-center gap-3">
                <button
                    on:click=on_save
                    class="px-4 py-2 bg-teal-600 hover:bg-teal-700 text-white text-sm font-medium rounded-lg transition-colors"
                >
                    {t.routing_save()}
                </button>
                {move || if saved.get() {
                    view! { <span class="text-xs text-teal-400">{t.routing_saved()}</span> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
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
        <div class="bg-stone-900 border border-stone-800 rounded-lg p-5 space-y-4">
            <h3 class="text-sm font-semibold text-stone-200">{t.routing_semantic_title()}</h3>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {:.2}", t.routing_similarity(), threshold.get())}
                </label>
                <input
                    type="range"
                    min="0.80"
                    max="0.99"
                    step="0.01"
                    prop:value=move || threshold.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            threshold.set(v);
                        }
                    }
                    class="w-full accent-violet-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>{t.routing_loose()}</span>
                    <span>{t.routing_strict()}</span>
                </div>
            </div>

            <div class="bg-stone-800 rounded-md p-3">
                <div class="text-xs text-stone-400 mb-1">{t.routing_impact_label()}</div>
                <div class="text-sm text-stone-300">
                    {move || {
                        let th = threshold.get();
                        if th > 0.95 {
                            t.routing_impact_high()
                        } else if th > 0.90 {
                            t.routing_impact_balanced()
                        } else {
                            t.routing_impact_loose()
                        }
                    }}
                </div>
            </div>

            <div class="flex items-center gap-3">
                <button
                    on:click=on_save
                    class="px-4 py-2 bg-teal-600 hover:bg-teal-700 text-white text-sm font-medium rounded-lg transition-colors"
                >
                    {t.routing_save()}
                </button>
                {move || if saved.get() {
                    view! { <span class="text-xs text-teal-400">{t.routing_saved()}</span> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </div>
        </div>
    }
}

#[component]
fn ConnectionConfigPanel(config: ConnectionConfig) -> impl IntoView {
    let t = use_translations();
    let tcp_keepalive_idle = RwSignal::new(config.tcp_keepalive_idle_secs);
    let tcp_keepalive_interval = RwSignal::new(config.tcp_keepalive_interval_secs);
    let tcp_keepalive_count = RwSignal::new(config.tcp_keepalive_count);
    let idle_timeout = RwSignal::new(config.idle_timeout_secs);
    let h2_ping_interval = RwSignal::new(config.h2_ping_interval_secs);
    let saved = RwSignal::new(false);

    let on_save = move |_| {
        let req = crate::types::UpdateConnectionConfigRequest {
            tcp_keepalive_idle_secs: tcp_keepalive_idle.get(),
            tcp_keepalive_interval_secs: tcp_keepalive_interval.get(),
            tcp_keepalive_count: tcp_keepalive_count.get(),
            idle_timeout_secs: idle_timeout.get(),
            h2_ping_interval_secs: h2_ping_interval.get(),
        };
        leptos::task::spawn_local(async move {
            let _ = api::update_connection_config(&req).await;
            saved.set(true);
        });
    };

    view! {
        <div class="bg-stone-900 border border-stone-800 rounded-lg p-5 space-y-4">
            <h3 class="text-sm font-semibold text-stone-200">{t.routing_connection_title()}</h3>
            <p class="text-xs text-stone-500">{t.routing_connection_desc()}</p>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {}s", t.routing_tcp_keepalive_idle(), tcp_keepalive_idle.get())}
                </label>
                <input
                    type="range"
                    min="10"
                    max="300"
                    prop:value=move || tcp_keepalive_idle.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            tcp_keepalive_idle.set(v);
                        }
                    }
                    class="w-full accent-teal-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>"10s"</span>
                    <span>"300s"</span>
                </div>
            </div>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {}s", t.routing_tcp_keepalive_interval(), tcp_keepalive_interval.get())}
                </label>
                <input
                    type="range"
                    min="1"
                    max="60"
                    prop:value=move || tcp_keepalive_interval.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            tcp_keepalive_interval.set(v);
                        }
                    }
                    class="w-full accent-amber-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>"1s"</span>
                    <span>"60s"</span>
                </div>
            </div>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {}", t.routing_tcp_keepalive_count(), tcp_keepalive_count.get())}
                </label>
                <input
                    type="range"
                    min="1"
                    max="10"
                    prop:value=move || tcp_keepalive_count.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            tcp_keepalive_count.set(v);
                        }
                    }
                    class="w-full accent-violet-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>"1"</span>
                    <span>"10"</span>
                </div>
            </div>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {}s", t.routing_idle_timeout(), idle_timeout.get())}
                </label>
                <input
                    type="range"
                    min="10"
                    max="600"
                    prop:value=move || idle_timeout.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            idle_timeout.set(v);
                        }
                    }
                    class="w-full accent-rose-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>"10s"</span>
                    <span>"600s"</span>
                </div>
            </div>

            <div>
                <label class="block text-xs text-stone-400 mb-1">
                    {move || format!("{}: {}s", t.routing_h2_ping_interval(), h2_ping_interval.get())}
                </label>
                <input
                    type="range"
                    min="5"
                    max="120"
                    prop:value=move || h2_ping_interval.get()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse() {
                            h2_ping_interval.set(v);
                        }
                    }
                    class="w-full accent-teal-500"
                />
                <div class="flex justify-between text-xs text-stone-600">
                    <span>"5s"</span>
                    <span>"120s"</span>
                </div>
            </div>

            <div class="flex items-center gap-3">
                <button
                    on:click=on_save
                    class="px-4 py-2 bg-teal-600 hover:bg-teal-700 text-white text-sm font-medium rounded-lg transition-colors"
                >
                    {t.routing_save()}
                </button>
                {move || if saved.get() {
                    view! { <span class="text-xs text-teal-400">{t.routing_saved()}</span> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </div>
        </div>
    }
}

#[component]
fn RoutingStatusPanel(status: RoutingStatus) -> impl IntoView {
    let t = use_translations();
    let total = status.total_backends.max(1) as f64;

    view! {
        <div class="bg-stone-900 border border-stone-800 rounded-lg p-5 space-y-4">
            <h3 class="text-sm font-semibold text-stone-200">{t.routing_affinity_title()}</h3>

            <div class="grid grid-cols-2 gap-4">
                <div>
                    <div class="text-xs text-stone-500 mb-1">{t.routing_active_backends()}</div>
                    <div class="text-xl font-mono tabular-nums text-stone-100">
                        {format!("{}", status.active_backends)}
                    </div>
                </div>
                <div>
                    <div class="text-xs text-stone-500 mb-1">{t.routing_total_backends()}</div>
                    <div class="text-xl font-mono tabular-nums text-stone-100">
                        {format!("{}", status.total_backends)}
                    </div>
                </div>
            </div>

            <div>
                <div class="text-xs text-stone-400 mb-2">{t.routing_distribution()}</div>
                <div class="space-y-2">
                    {status.backends.into_iter().map(|backend| {
                        let pct = backend.request_count as f64 / total * 100.0;
                        let color = if backend.healthy { "bg-teal-500" } else { "bg-rose-500" };
                        view! {
                            <div class="flex items-center gap-3">
                                <span class="w-24 text-xs text-stone-400 truncate">{backend.name}</span>
                                <div class="flex-1 bg-stone-800 rounded-full h-2 overflow-hidden">
                                    <div
                                        class=format!("h-full rounded-full {}", color)
                                        style=format!("width: {}%", pct.min(100.0))
                                    ></div>
                                </div>
                                <span class="w-16 text-xs font-mono tabular-nums text-stone-300 text-right">
                                    {format!("{}", backend.request_count)}
                                </span>
                                <span class="w-12 text-xs text-stone-500 text-right">
                                    {format!("{:.0}%", pct)}
                                </span>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>
        </div>
    }
}