use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{
    BackendEndpoint, CacheConfig, CacheOpsView, ConnectionConfig, FingerprintConfigBody,
    InvalidateCacheBody, PutBackendsRequest, RoutingStatus, SemanticConfig, StreamCacheToggle,
    TraceAnalysis, UpdateCacheConfigRequest, UpdateSemanticConfigRequest,
};

#[component]
pub fn CachePage() -> impl IntoView {
    let t = use_translations();
    let active_tab: RwSignal<usize> = RwSignal::new(0);
    let tab_labels = vec![
        t.tab_config().to_string(),
        t.tab_routing().to_string(),
        t.tab_ops().to_string(),
        t.tab_trace().to_string(),
    ];

    init_tab_from_query(
        active_tab,
        &[("config", 0), ("routing", 1), ("ops", 2), ("trace", 3)],
    );

    view! {
        <div class="page-content space-y-6">
            <SectionHeader title=t.cache_ops_title() description=t.cache_ops_desc() />
            <TabBar tabs=tab_labels active=active_tab />
            {move || match active_tab.get() {
                0 => view! { <ConfigTab /> }.into_any(),
                1 => view! { <RoutingTab /> }.into_any(),
                2 => view! { <OpsTab /> }.into_any(),
                _ => view! { <TraceTab /> }.into_any(),
            }}
        </div>
    }
}

// ---------------------------------------------------------------------------
// Tab 0: Config — TTL, Semantic, Fingerprint, Stream Cache
// ---------------------------------------------------------------------------

#[component]
fn ConfigTab() -> impl IntoView {
    let t = use_translations();
    let feedback: RwSignal<String> = RwSignal::new(String::new());

    // Cache config (sliders from Routing)
    let cache_config: RwSignal<Option<Result<CacheConfig, String>>> = RwSignal::new(None);
    // Semantic config (slider from Routing + enabled toggle from CacheOps)
    let semantic_config: RwSignal<Option<Result<SemanticConfig, String>>> = RwSignal::new(None);
    // Fingerprint / stream cache from CacheOps
    let ops: RwSignal<Option<Result<CacheOpsView, String>>> = RwSignal::new(None);

    leptos::task::spawn_local(async move {
        match api::fetch_cache_config().await {
            Ok(c) => cache_config.set(Some(Ok(c))),
            Err(e) => cache_config.set(Some(Err(e))),
        }
    });
    leptos::task::spawn_local(async move {
        match api::fetch_semantic_config().await {
            Ok(c) => semantic_config.set(Some(Ok(c))),
            Err(e) => semantic_config.set(Some(Err(e))),
        }
    });
    leptos::task::spawn_local(async move {
        match api::fetch_cache_ops().await {
            Ok(v) => ops.set(Some(Ok(v))),
            Err(e) => ops.set(Some(Err(e))),
        }
    });

    let reload_ops = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_cache_ops().await {
                Ok(v) => ops.set(Some(Ok(v))),
                Err(e) => ops.set(Some(Err(e))),
            }
        });
    };

    view! {
        <div class="space-y-6">
            <Alert variant="info" message=feedback.into() />

            <div class="config-grid-2">
                // TTL config (slider version from Routing)
                {move || match cache_config.get() {
                    None => view! { <div class="config-card glass-card"><Spinner /></div> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="config-card glass-card text-error text-sm">{e}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! { <TtlConfigPanel config feedback /> }.into_any(),
                }}

                // Semantic config (slider + enabled toggle)
                {move || match semantic_config.get() {
                    None => view! { <div class="config-card glass-card"><Spinner /></div> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="config-card glass-card text-error text-sm">{e}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! { <SemanticConfigPanel config feedback /> }.into_any(),
                }}
            </div>

            // Fingerprint + Stream cache (from CacheOps)
            {move || match ops.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(view)) => {
                    let fp_version = RwSignal::new(view.fingerprint_version.to_string());
                    let fp_normalize = RwSignal::new(view.fingerprint_normalize);
                    let stream_enabled = RwSignal::new(view.stream_cache_enabled);

                    view! {
                        <div class="bento-grid-2">
                            <div class="glass-card space-y-4">
                                <h3 class="text-sm font-semibold text-theme">{t.cache_ops_fingerprint_title()}</h3>
                                <label class="block text-xs text-theme-muted">
                                    {t.cache_ops_fingerprint_version()}
                                    <input
                                        type="number"
                                        class="input mt-1 w-full"
                                        prop:value=move || fp_version.get()
                                        on:input=move |ev| fp_version.set(event_target_value(&ev))
                                    />
                                </label>
                                <label class="flex items-center gap-2 text-xs text-theme-secondary">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || fp_normalize.get()
                                        on:change=move |ev| fp_normalize.set(event_target_checked(&ev))
                                    />
                                    {t.cache_ops_normalize()}
                                </label>
                                <button
                                    class="btn btn-primary text-xs"
                                    on:click=move |_| {
                                        let version = fp_version.get().parse().unwrap_or(1);
                                        let normalize = fp_normalize.get();
                                        leptos::task::spawn_local(async move {
                                            match api::update_fingerprint(&FingerprintConfigBody {
                                                version,
                                                normalize_content: normalize,
                                            }).await {
                                                Ok(_) => feedback.set(t.routing_saved().to_string()),
                                                Err(e) => feedback.set(e),
                                            }
                                        });
                                    }
                                >
                                    {t.routing_save()}
                                </button>
                            </div>

                            <div class="glass-card space-y-4">
                                <h3 class="text-sm font-semibold text-theme">{t.cache_ops_stream_cache()}</h3>
                                <label class="flex items-center gap-2 text-xs text-theme-secondary">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || stream_enabled.get()
                                        on:change=move |ev| {
                                            let enabled = event_target_checked(&ev);
                                            stream_enabled.set(enabled);
                                            leptos::task::spawn_local(async move {
                                                match api::update_stream_cache(&StreamCacheToggle { enabled }).await {
                                                    Ok(_) => {
                                                        feedback.set(t.routing_saved().to_string());
                                                        reload_ops();
                                                    }
                                                    Err(e) => { feedback.set(e); }
                                                }
                                            });
                                        }
                                    />
                                    {t.cache_ops_stream_cache()}
                                </label>
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn TtlConfigPanel(config: CacheConfig, feedback: RwSignal<String>) -> impl IntoView {
    let t = use_translations();
    let l0_ttl = RwSignal::new(config.l0_ttl_secs);
    let l1_ttl = RwSignal::new(config.l1_ttl_secs);

    let on_save = move |_| {
        let req = UpdateCacheConfigRequest {
            l0_ttl_secs: l0_ttl.get(),
            l1_ttl_secs: l1_ttl.get(),
            model_overrides: config.model_overrides.clone(),
            consumer_overrides: config.consumer_overrides.clone(),
        };
        leptos::task::spawn_local(async move {
            match api::update_cache_config(&req).await {
                Ok(_) => feedback.set(t.routing_saved().to_string()),
                Err(e) => feedback.set(e),
            }
        });
    };

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h4 class="config-card-title">{t.routing_cache_config_title()}</h4>
            </div>
            <div class="config-card-body space-y-4">
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_l0_ttl(), l0_ttl.get())
                    value=l0_ttl
                    min=10
                    max=3600
                    min_hint="10s"
                    max_hint="3600s"
                    accent="gold"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_l1_ttl(), l1_ttl.get())
                    value=l1_ttl
                    min=60
                    max=86400
                    min_hint="60s"
                    max_hint="86400s"
                    accent="amber"
                />
            </div>
            <div class="config-card-foot">
                <button on:click=on_save class="btn btn-primary text-sm">{t.routing_save()}</button>
            </div>
        </div>
    }
}

#[component]
fn SemanticConfigPanel(config: SemanticConfig, feedback: RwSignal<String>) -> impl IntoView {
    let t = use_translations();
    let enabled = RwSignal::new(config.enabled);
    let threshold = RwSignal::new(config.similarity_threshold);

    let on_save = move |_| {
        let req = UpdateSemanticConfigRequest {
            enabled: Some(enabled.get()),
            similarity_threshold: threshold.get(),
        };
        leptos::task::spawn_local(async move {
            match api::update_semantic_config(&req).await {
                Ok(_) => feedback.set(t.routing_saved().to_string()),
                Err(e) => feedback.set(e),
            }
        });
    };

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h4 class="config-card-title">{t.routing_semantic_title()}</h4>
            </div>
            <div class="config-card-body space-y-4">
                <label class="flex items-center gap-2 text-xs text-theme-secondary">
                    <input
                        type="checkbox"
                        prop:checked=move || enabled.get()
                        on:change=move |ev| enabled.set(event_target_checked(&ev))
                    />
                    {t.cache_ops_semantic_enabled()}
                </label>
                <ConfigRangeF64
                    label=move || format!("{}: {:.2}", t.routing_similarity(), threshold.get())
                    value=threshold
                    min=0.80
                    max=0.99
                    step=0.01
                    min_hint=t.routing_loose()
                    max_hint=t.routing_strict()
                    accent="violet"
                />
                <div class="impact-hint">
                    <div class="text-xs text-theme-secondary mb-1">{t.routing_impact_label()}</div>
                    <div class="text-sm text-theme">
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
            </div>
            <div class="config-card-foot">
                <button on:click=on_save class="btn btn-primary text-sm">{t.routing_save()}</button>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Tab 1: Routing — Backend endpoints + Connection config
// ---------------------------------------------------------------------------

#[component]
fn RoutingTab() -> impl IntoView {
    let t = use_translations();
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let connection_config: RwSignal<Option<Result<ConnectionConfig, String>>> = RwSignal::new(None);
    let routing_status: RwSignal<Option<Result<RoutingStatus, String>>> = RwSignal::new(None);

    leptos::task::spawn_local(async move {
        match api::fetch_connection_config().await {
            Ok(c) => connection_config.set(Some(Ok(c))),
            Err(e) => connection_config.set(Some(Err(e))),
        }
    });
    leptos::task::spawn_local(async move {
        match api::fetch_routing_status().await {
            Ok(s) => routing_status.set(Some(Ok(s))),
            Err(e) => routing_status.set(Some(Err(e))),
        }
    });

    view! {
        <div class="space-y-6">
            <Alert variant="info" message=feedback.into() />

            <section class="config-section">
                <h3 class="config-section-title">{t.routing_connection_title()}</h3>
                {move || match connection_config.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="config-card glass-card text-error text-sm">{e}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! {
                        <ConnectionConfigPanel config feedback />
                    }.into_any(),
                }}
            </section>

            <section class="config-section">
                {move || match routing_status.get() {
                    None => view! { <Spinner /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="config-card glass-card text-error text-sm">{e}</div>
                    }.into_any(),
                    Some(Ok(status)) => view! {
                        <RoutingStatusPanel status=status.clone() />
                        <BackendEditPanel status feedback />
                    }.into_any(),
                }}
            </section>
        </div>
    }
}

#[component]
fn ConnectionConfigPanel(config: ConnectionConfig, feedback: RwSignal<String>) -> impl IntoView {
    let t = use_translations();
    let idle = RwSignal::new(config.tcp_keepalive_idle_secs);
    let interval = RwSignal::new(config.tcp_keepalive_interval_secs);
    let count = RwSignal::new(config.tcp_keepalive_count as u64);
    let timeout = RwSignal::new(config.idle_timeout_secs);
    let h2_ping = RwSignal::new(config.h2_ping_interval_secs);

    let on_save = move |_| {
        let req = crate::types::UpdateConnectionConfigRequest {
            tcp_keepalive_idle_secs: idle.get(),
            tcp_keepalive_interval_secs: interval.get(),
            tcp_keepalive_count: count.get() as usize,
            idle_timeout_secs: timeout.get(),
            h2_ping_interval_secs: h2_ping.get(),
        };
        leptos::task::spawn_local(async move {
            match api::update_connection_config(&req).await {
                Ok(_) => feedback.set(t.routing_saved().to_string()),
                Err(e) => feedback.set(e),
            }
        });
    };

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h4 class="config-card-title">{t.routing_connection_title()}</h4>
                <p class="config-card-desc">{t.routing_connection_desc()}</p>
            </div>
            <div class="config-card-body config-grid-form">
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_tcp_keepalive_idle(), idle.get())
                    value=idle min=10 max=300 min_hint="10s" max_hint="300s" accent="gold"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_tcp_keepalive_interval(), interval.get())
                    value=interval min=1 max=60 min_hint="1s" max_hint="60s" accent="amber"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}", t.routing_tcp_keepalive_count(), count.get())
                    value=count min=1 max=10 min_hint="1" max_hint="10" accent="violet"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_idle_timeout(), timeout.get())
                    value=timeout min=10 max=600 min_hint="10s" max_hint="600s" accent="rose"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_h2_ping_interval(), h2_ping.get())
                    value=h2_ping min=5 max=120 min_hint="5s" max_hint="120s" accent="gold"
                />
            </div>
            <div class="config-card-foot">
                <button on:click=on_save class="btn btn-primary text-sm">{t.routing_save()}</button>
            </div>
        </div>
    }
}

#[component]
fn RoutingStatusPanel(status: RoutingStatus) -> impl IntoView {
    let t = use_translations();
    let total = status.total_backends.max(1) as f64;

    view! {
        <div class="config-card glass-card">
            <div class="config-card-head">
                <h4 class="config-card-title">{t.routing_affinity_title()}</h4>
            </div>
            <div class="config-card-body space-y-5">
                <div class="stat-pair-row">
                    <div class="stat-pair">
                        <div class="stat-pair-label">{t.routing_active_backends()}</div>
                        <div class="stat-pair-value">{format!("{}", status.active_backends)}</div>
                    </div>
                    <div class="stat-pair">
                        <div class="stat-pair-label">{t.routing_total_backends()}</div>
                        <div class="stat-pair-value">{format!("{}", status.total_backends)}</div>
                    </div>
                </div>
                <div>
                    <div class="text-xs text-theme-secondary mb-3">{t.routing_distribution()}</div>
                    <div class="space-y-2">
                        {status.backends.into_iter().map(|backend| {
                            let pct = backend.request_count as f64 / total * 100.0;
                            let fill_class = if backend.healthy {
                                "progress-bar-fill"
                            } else {
                                "progress-bar-fill progress-bar-fill-error"
                            };
                            view! {
                                <div class="backend-row">
                                    <span class="backend-row-name">{backend.name}</span>
                                    <div class="flex-1 progress-bar h-2">
                                        <div class=fill_class style=format!("width: {}%", pct.min(100.0))></div>
                                    </div>
                                    <span class="backend-row-count">{format!("{}", backend.request_count)}</span>
                                    <span class="backend-row-pct">{format!("{:.0}%", pct)}</span>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn BackendEditPanel(status: RoutingStatus, feedback: RwSignal<String>) -> impl IntoView {
    let t = use_translations();

    let backends: RwSignal<Vec<(RwSignal<String>, RwSignal<String>, RwSignal<u32>)>> = {
        let pairs: Vec<(RwSignal<String>, RwSignal<String>, RwSignal<u32>)> = status
            .backends
            .into_iter()
            .map(|b| {
                (
                    RwSignal::new(b.name),
                    RwSignal::new(b.addr),
                    RwSignal::new(1),
                )
            })
            .collect();
        RwSignal::new(pairs)
    };
    let saving: RwSignal<bool> = RwSignal::new(false);

    let on_add = move |_| {
        backends.update(|list| {
            list.push((
                RwSignal::new(String::new()),
                RwSignal::new(String::new()),
                RwSignal::new(1),
            ));
        });
    };

    let on_remove = move |idx: usize| {
        backends.update(|list| {
            if idx < list.len() {
                list.remove(idx);
            }
        });
    };

    let on_save = move |_| {
        saving.set(true);
        let pairs = backends.get();
        let req = PutBackendsRequest {
            backends: pairs
                .into_iter()
                .map(|(name, addr, weight)| BackendEndpoint {
                    name: name.get(),
                    addr: addr.get(),
                    weight: weight.get(),
                })
                .filter(|b| !b.name.is_empty() || !b.addr.is_empty())
                .collect(),
        };
        leptos::task::spawn_local(async move {
            match api::update_routing_backends(&req).await {
                Ok(_) => feedback.set(t.routing_saved().to_string()),
                Err(e) => feedback.set(e),
            }
            saving.set(false);
        });
    };

    view! {
        <div class="config-card glass-card mt-4">
            <div class="config-card-head">
                <h4 class="config-card-title">{t.routing_affinity_title()}</h4>
                <p class="config-card-desc">"Edit backend endpoints"</p>
            </div>
            <div class="config-card-body">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="text-left text-theme-secondary border-b border-theme-border">
                            <th class="pb-2 pr-2 font-medium">"Name"</th>
                            <th class="pb-2 pr-2 font-medium">"Address"</th>
                            <th class="pb-2 pr-2 font-medium w-16">"Weight"</th>
                            <th class="pb-2 w-10"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {move || backends.get().into_iter().enumerate().map(|(idx, (name_sig, addr_sig, weight_sig))| {
                            let remove_idx = idx;
                            view! {
                                <tr class="border-b border-theme-border/50">
                                    <td class="py-2 pr-2">
                                        <input type="text" class="input w-full text-sm"
                                            prop:value=move || name_sig.get()
                                            on:input=move |e| name_sig.set(event_target_value(&e))
                                            placeholder="backend-1"
                                        />
                                    </td>
                                    <td class="py-2 pr-2">
                                        <input type="text" class="input w-full text-sm font-mono"
                                            prop:value=move || addr_sig.get()
                                            on:input=move |e| addr_sig.set(event_target_value(&e))
                                            placeholder="127.0.0.1:443"
                                        />
                                    </td>
                                    <td class="py-2 pr-2">
                                        <input type="number" class="input w-full text-sm"
                                            prop:value=move || weight_sig.get().to_string()
                                            on:input=move |e| {
                                                if let Ok(v) = event_target_value(&e).parse::<u32>() { weight_sig.set(v); }
                                            }
                                            min="1" max="100"
                                        />
                                    </td>
                                    <td class="py-2 text-center">
                                        <button on:click=move |_| on_remove(remove_idx) class="btn btn-ghost text-xs text-error">
                                            "\u{2715}"
                                        </button>
                                    </td>
                                </tr>
                            }
                        }).collect::<Vec<_>>()}
                    </tbody>
                </table>

                <div class="flex items-center justify-between mt-4 pt-3 border-t border-theme-border/50">
                    <button on:click=on_add class="btn btn-secondary text-xs">"Add Backend"</button>
                    <div class="flex items-center gap-3">
                        {move || if !feedback.get().is_empty() {
                            view! { <span class="text-xs text-theme-secondary">{feedback.get()}</span> }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                        <button on:click=on_save disabled=move || saving.get() class="btn btn-primary text-sm">
                            {t.routing_save()}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Tab 2: Ops — Cache invalidation
// ---------------------------------------------------------------------------

#[component]
fn OpsTab() -> impl IntoView {
    let t = use_translations();
    let ops: RwSignal<Option<Result<CacheOpsView, String>>> = RwSignal::new(None);
    let message: RwSignal<String> = RwSignal::new(String::new());
    let show_confirm_all = RwSignal::new(false);
    let scope = RwSignal::new(String::new());

    let reload = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_cache_ops().await {
                Ok(v) => ops.set(Some(Ok(v))),
                Err(e) => ops.set(Some(Err(e))),
            }
        });
    };

    reload();

    view! {
        <div class="space-y-6">
            <Alert variant="info" message=message.into() />

            {move || match ops.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(view)) => {
                    let last = view.last_invalidate.clone();
                    view! {
                        <div class="glass-card space-y-4">
                            <h3 class="text-sm font-semibold text-theme">{t.cache_ops_invalidate_title()}</h3>
                            <label class="block text-xs text-theme-muted">
                                {t.cache_ops_scope()}
                                <input
                                    type="text"
                                    class="input mt-1 w-full font-mono"
                                    placeholder="all"
                                    prop:value=move || scope.get()
                                    on:input=move |ev| scope.set(event_target_value(&ev))
                                />
                            </label>
                            <button
                                class="btn btn-secondary text-xs"
                                on:click=move |_| {
                                    let s = scope.get().trim().to_string();
                                    if s.is_empty() {
                                        message.set(t.cache_ops_scope_required().to_string());
                                        return;
                                    }
                                    if s == "all" {
                                        show_confirm_all.set(true);
                                    } else {
                                        let scope_val = s.clone();
                                        leptos::task::spawn_local(async move {
                                            match api::invalidate_cache(&InvalidateCacheBody { scope: scope_val }).await {
                                                Ok(r) => {
                                                    message.set(format!("{}: {}", r.scope, r.status));
                                                    reload();
                                                }
                                                Err(e) => { message.set(e); }
                                            }
                                        });
                                    }
                                }
                            >
                                {t.cache_ops_invalidate_btn()}
                            </button>

                            <div class="text-xs text-theme-muted border-t border-theme pt-3 space-y-3">
                                {if view.invalidate_all_in_progress {
                                    view! {
                                        <div class="text-warning font-medium">{t.cache_ops_invalidate_running()}</div>
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }}
                                {if let Some(job) = view.invalidate_job.clone() {
                                    view! {
                                        <div>
                                            <div class="font-medium text-theme-secondary mb-1">{t.cache_ops_invalidate_job()}</div>
                                            <div>{format!("{} — {}", job.scope, job.phase)}</div>
                                            {job.error.map(|e| view! { <div class="text-error">{e}</div> })}
                                        </div>
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }}
                                <div>
                                    <div class="font-medium text-theme-secondary mb-1">{t.cache_ops_last_invalidate()}</div>
                                    {if let Some(li) = last {
                                        view! {
                                            <div>{format!("scope={} status={}", li.scope, li.status)}</div>
                                            <div class="text-theme-muted">{format!("at={}", li.at_secs)}</div>
                                            {li.error.map(|e| view! { <div class="text-error">{e}</div> })}
                                        }.into_any()
                                    } else {
                                        view! { <div>{t.cache_ops_none()}</div> }.into_any()
                                    }}
                                </div>
                            </div>
                        </div>
                    }.into_any()
                }
            }}

            {move || show_confirm_all.get().then(|| view! {
                <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
                    <div class="glass-card max-w-md w-full space-y-4">
                        <h4 class="text-sm font-semibold text-theme">{t.cache_ops_confirm_all_title()}</h4>
                        <p class="text-xs text-theme-muted">{t.cache_ops_confirm_all_body()}</p>
                        <div class="flex gap-2 justify-end">
                            <button class="btn btn-secondary text-xs" on:click=move |_| show_confirm_all.set(false)>
                                {t.cache_ops_confirm_cancel()}
                            </button>
                            <button
                                class="btn btn-primary text-xs"
                                on:click=move |_| {
                                    show_confirm_all.set(false);
                                    leptos::task::spawn_local(async move {
                                        match api::invalidate_cache(&InvalidateCacheBody { scope: "all".to_string() }).await {
                                            Ok(r) => {
                                                message.set(format!("{}: {}", r.scope, r.status));
                                                reload();
                                            }
                                            Err(e) => { message.set(e); }
                                        }
                                    });
                                }
                            >
                                {t.cache_ops_confirm_ok()}
                            </button>
                        </div>
                    </div>
                </div>
            })}
        </div>
    }
}

// ---------------------------------------------------------------------------
// Tab 3: Trace — Trace analysis
// ---------------------------------------------------------------------------

#[component]
fn TraceTab() -> impl IntoView {
    let t = use_translations();
    let analysis: RwSignal<Option<Result<TraceAnalysis, String>>> = RwSignal::new(None);

    let load_analysis = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_trace_analysis(24).await {
                Ok(a) => analysis.set(Some(Ok(a))),
                Err(e) => analysis.set(Some(Err(e))),
            }
        });
    };

    load_analysis();

    view! {
        <div class="space-y-6">
            <div class="flex items-center justify-between">
                <p class="text-xs text-theme-muted max-w-md hidden md:block">
                    {t.trace_hours_note()}
                </p>
                <button on:click=move |_| load_analysis() class="btn btn-secondary text-sm">
                    {t.trace_refresh()}
                </button>
            </div>

            {move || match analysis.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().trace_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(data)) => {
                    let t = use_translations();
                    let cluster_dist = data.cluster_distribution.clone();
                    view! {
                        <div class="space-y-6">
                            <div class="bento-grid-4">
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_total_requests()}</div>
                                    <div class="text-2xl font-bold text-theme font-mono">{format!("{}", data.total_requests)}</div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_unique_requests()}</div>
                                    <div class="text-2xl font-bold text-theme font-mono">{format!("{}", data.unique_requests)}</div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_repeat_ratio()}</div>
                                    <div class="text-2xl font-bold text-accent font-mono">{format!("{:.1}%", data.repeat_ratio * 100.0)}</div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_estimated_hit_rate()}</div>
                                    <div class="text-2xl font-bold text-green-500 font-mono">{format!("{:.1}%", data.estimated_hit_rate * 100.0)}</div>
                                </div>
                            </div>

                            <div class="bento-grid-3">
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_semantic_ratio()}</div>
                                    <div class="text-lg font-semibold text-theme font-mono">{format!("{:.1}%", data.semantic_cluster_ratio * 100.0)}</div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_zipf_alpha()}</div>
                                    <div class="text-lg font-semibold text-theme font-mono">{format!("{:.2}", data.estimated_zipf_alpha)}</div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_cache_hit_ratio()}</div>
                                    <div class="text-lg font-semibold text-theme font-mono">{format!("{:.1}%", data.cache_hit_ratio * 100.0)}</div>
                                </div>
                            </div>

                            <div class="bento-grid-2">
                                <div class="bento-cell">
                                    <h3 class="text-sm font-semibold text-theme mb-3">{t.trace_avg_metrics()}</h3>
                                    <div class="space-y-2">
                                        <div class="flex justify-between">
                                            <span class="text-xs text-theme-muted">{t.trace_avg_latency()}</span>
                                            <span class="text-sm font-mono text-theme">{format!("{:.1}ms", data.avg_latency_ms)}</span>
                                        </div>
                                        <div class="flex justify-between">
                                            <span class="text-xs text-theme-muted">{t.trace_avg_tokens()}</span>
                                            <span class="text-sm font-mono text-theme">{format!("{:.0}", data.avg_prompt_tokens)}</span>
                                        </div>
                                    </div>
                                </div>
                                <div class="bento-cell">
                                    <h3 class="text-sm font-semibold text-theme mb-3">{t.trace_top_models()}</h3>
                                    <div class="space-y-2">
                                        {data.top_models.iter().map(|m| {
                                            view! {
                                                <div class="flex justify-between items-center">
                                                    <span class="text-xs text-theme">{m.model.clone()}</span>
                                                    <div class="flex items-center gap-2">
                                                        <span class="text-xs font-mono text-theme-muted">{format!("{}", m.count)}</span>
                                                        <span class="text-xs font-mono text-accent">{format!("{:.1}%", m.percentage)}</span>
                                                    </div>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            </div>

                            <div class="glass-card">
                                <h3 class="text-sm font-semibold text-theme mb-3">{t.trace_cluster_distribution()}</h3>
                                <div class="space-y-2">
                                    {cluster_dist.into_iter().map(|c| {
                                        view! {
                                            <div class="flex items-center gap-3">
                                                <span class="text-xs font-mono text-theme-muted w-20">{format!("Cluster {}", c.cluster_id)}</span>
                                                <div class="flex-1 h-2 bg-theme-tertiary rounded-full overflow-hidden">
                                                    <div class="h-full bg-accent rounded-full" style=move || format!("width: {}%", c.percentage) />
                                                </div>
                                                <span class="text-xs font-mono text-theme w-16 text-right">{format!("{}", c.count)}</span>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            </div>

                            {data.deepseek_user_id.clone().map(|audit| {
                                let ok = audit.isolation_ok;
                                let conclusion = audit.conclusion.clone();
                                let top_projects = audit.top_project_ids.clone();
                                let breakdown = audit.audit_breakdown.clone();
                                view! {
                                    <div class="glass-card space-y-4">
                                        <div>
                                            <h3 class="text-sm font-semibold text-theme">{t.trace_deepseek_user_id_title()}</h3>
                                            <p class="text-xs text-theme-muted mt-1">{t.trace_deepseek_user_id_hint()}</p>
                                        </div>
                                        <div class=move || if ok { "text-sm font-medium text-accent" } else { "text-sm font-medium text-warning" }>
                                            {move || if ok { t.trace_isolation_ok() } else { t.trace_isolation_fail() }}
                                            <span class="text-theme-muted font-normal ml-2">{conclusion.clone()}</span>
                                        </div>
                                        <div class="bento-grid-4">
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">{t.trace_deepseek_requests()}</div>
                                                <div class="text-xl font-bold font-mono text-theme">{audit.deepseek_requests}</div>
                                            </div>
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">{t.trace_upstream_user_id_ratio()}</div>
                                                <div class="text-xl font-bold font-mono text-theme">{format!("{:.1}%", audit.upstream_user_id_ratio * 100.0)}</div>
                                            </div>
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">{t.trace_missing_project_id()}</div>
                                                <div class="text-xl font-bold font-mono text-theme">{audit.missing_project_id}</div>
                                            </div>
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">{t.trace_client_user_id_leaks()}</div>
                                                <div class="text-xl font-bold font-mono text-theme">{audit.client_user_id_leaks}</div>
                                            </div>
                                        </div>
                                        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                            <div>
                                                <h4 class="text-xs font-semibold text-theme-muted mb-2">{t.trace_audit_injected()}</h4>
                                                <div class="text-sm font-mono space-y-1">
                                                    <div class="flex justify-between"><span>injected</span><span>{breakdown.injected}</span></div>
                                                    <div class="flex justify-between"><span>absent</span><span>{breakdown.absent}</span></div>
                                                    <div class="flex justify-between"><span>stripped_client</span><span>{breakdown.stripped_client}</span></div>
                                                    <div class="flex justify-between"><span>mismatch</span><span>{breakdown.mismatch}</span></div>
                                                </div>
                                            </div>
                                            <div>
                                                <h4 class="text-xs font-semibold text-theme-muted mb-2">{t.trace_top_project_ids()}</h4>
                                                <div class="space-y-1">
                                                    {top_projects.into_iter().map(|p| {
                                                        view! {
                                                            <div class="flex justify-between text-xs font-mono">
                                                                <span class="text-theme truncate pr-2">{p.project_id}</span>
                                                                <span class="text-theme-muted">{format!("{} ({:.1}%)", p.count, p.percentage)}</span>
                                                            </div>
                                                        }
                                                    }).collect_view()}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                }.into_any()
                            })}
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
