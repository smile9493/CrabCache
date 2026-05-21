use leptos::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{BackendEndpoint, CacheConfig, ConnectionConfig, PutBackendsRequest, RoutingStatus, SemanticConfig};

#[component]
pub fn RoutingPage() -> impl IntoView {
    let t = use_translations();
    let cache_config: RwSignal<Option<Result<CacheConfig, String>>> = RwSignal::new(None);
    let semantic_config: RwSignal<Option<Result<SemanticConfig, String>>> = RwSignal::new(None);
    let connection_config: RwSignal<Option<Result<ConnectionConfig, String>>> = RwSignal::new(None);
    let routing_status: RwSignal<Option<Result<RoutingStatus, String>>> = RwSignal::new(None);
    let feedback: RwSignal<String> = RwSignal::new(String::new());

    let reload_all = move || {
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
    };

    reload_all();

    view! {
        <div class="page-content space-y-6">
            <PageHeader
                title=move || t.routing_title()
                description=move || t.routing_desc()
            >
                <button
                    on:click=move |_| reload_all()
                    class="btn btn-secondary text-xs"
                >
                    {t.overview_refresh()}
                </button>
            </PageHeader>

            <Alert variant="info" message=feedback.into() />

            <section class="config-section">
                <h3 class="config-section-title">{t.routing_cache_config_title()}</h3>
                <div class="config-grid-2">
                    {move || match cache_config.get() {
                        None => view! { <div class="config-card glass-card"><Spinner /></div> }.into_any(),
                        Some(Err(e)) => view! {
                            <div class="config-card glass-card text-error text-sm">{e}</div>
                        }.into_any(),
                        Some(Ok(config)) => view! {
                            <CacheConfigPanel config feedback />
                        }.into_any(),
                    }}
                    {move || match semantic_config.get() {
                        None => view! { <div class="config-card glass-card"><Spinner /></div> }.into_any(),
                        Some(Err(e)) => view! {
                            <div class="config-card glass-card text-error text-sm">{e}</div>
                        }.into_any(),
                        Some(Ok(config)) => view! {
                            <SemanticConfigPanel config feedback />
                        }.into_any(),
                    }}
                </div>
            </section>

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
fn CacheConfigPanel(config: CacheConfig, feedback: RwSignal<String>) -> impl IntoView {
    let t = use_translations();
    let l0_ttl = RwSignal::new(config.l0_ttl_secs);
    let l1_ttl = RwSignal::new(config.l1_ttl_secs);

    let on_save = move |_| {
        let req = crate::types::UpdateCacheConfigRequest {
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
    let threshold = RwSignal::new(config.similarity_threshold);

    let on_save = move |_| {
        let req = crate::types::UpdateSemanticConfigRequest {
            enabled: None,
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
                    value=idle
                    min=10
                    max=300
                    min_hint="10s"
                    max_hint="300s"
                    accent="gold"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_tcp_keepalive_interval(), interval.get())
                    value=interval
                    min=1
                    max=60
                    min_hint="1s"
                    max_hint="60s"
                    accent="amber"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}", t.routing_tcp_keepalive_count(), count.get())
                    value=count
                    min=1
                    max=10
                    min_hint="1"
                    max_hint="10"
                    accent="violet"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_idle_timeout(), timeout.get())
                    value=timeout
                    min=10
                    max=600
                    min_hint="10s"
                    max_hint="600s"
                    accent="rose"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", t.routing_h2_ping_interval(), h2_ping.get())
                    value=h2_ping
                    min=5
                    max=120
                    min_hint="5s"
                    max_hint="120s"
                    accent="gold"
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
                                        <div
                                            class=fill_class
                                            style=format!("width: {}%", pct.min(100.0))
                                        ></div>
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
                                        <input
                                            type="text"
                                            class="input w-full text-sm"
                                            prop:value=move || name_sig.get()
                                            on:input=move |e| name_sig.set(event_target_value(&e))
                                            placeholder="backend-1"
                                        />
                                    </td>
                                    <td class="py-2 pr-2">
                                        <input
                                            type="text"
                                            class="input w-full text-sm font-mono"
                                            prop:value=move || addr_sig.get()
                                            on:input=move |e| addr_sig.set(event_target_value(&e))
                                            placeholder="127.0.0.1:443"
                                        />
                                    </td>
                                    <td class="py-2 pr-2">
                                        <input
                                            type="number"
                                            class="input w-full text-sm"
                                            prop:value=move || weight_sig.get().to_string()
                                            on:input=move |e| {
                                                if let Ok(v) = event_target_value(&e).parse::<u32>() {
                                                    weight_sig.set(v);
                                                }
                                            }
                                            min="1"
                                            max="100"
                                        />
                                    </td>
                                    <td class="py-2 text-center">
                                        <button
                                            on:click=move |_| on_remove(remove_idx)
                                            class="btn btn-ghost text-xs text-error"
                                        >
                                            "✕"
                                        </button>
                                    </td>
                                </tr>
                            }
                        }).collect::<Vec<_>>()}
                    </tbody>
                </table>

                <div class="flex items-center justify-between mt-4 pt-3 border-t border-theme-border/50">
                    <button
                        on:click=on_add
                        class="btn btn-secondary text-xs"
                    >
                        "Add Backend"
                    </button>
                    <div class="flex items-center gap-3">
                        {move || if !feedback.get().is_empty() {
                            view! {
                                <span class="text-xs text-theme-secondary">{feedback.get()}</span>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                        <button
                            on:click=on_save
                            disabled=move || saving.get()
                            class="btn btn-primary text-sm"
                        >
                            {t.routing_save()}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}
