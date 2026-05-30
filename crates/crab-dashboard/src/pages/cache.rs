use leptos::prelude::*;
use std::sync::Arc;

use crate::api;
use crate::components::canvas_line_chart::CanvasLineChart;
use crate::components::donut_chart::{DonutChart, DonutSegment};
use crate::components::line_chart::ChartSeries;
use crate::components::scatter_chart::{ScatterChart, ScatterPoint};
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{
    BackendEndpoint, CacheConfig, CacheOpsView, ConnectionConfig, FingerprintConfigBody,
    InvalidateCacheBody, PricingConfigView, PutBackendsRequest, RoutingStatus, SemanticConfig,
    StreamCacheToggle, TimeSeriesPoint, TraceAnalysis, UpdateCacheConfigRequest,
    UpdateSemanticConfigRequest,
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
    let _t = use_translations();
    let feedback: RwSignal<String> = RwSignal::new(String::new());

    let cache_config: RwSignal<Option<Result<CacheConfig, String>>> = RwSignal::new(None);
    let semantic_config: RwSignal<Option<Result<SemanticConfig, String>>> = RwSignal::new(None);
    let ops: RwSignal<Option<Result<CacheOpsView, String>>> = RwSignal::new(None);

    leptos::task::spawn_local(async move {
        match api::fetch_cache_config().await {
            Ok(c) => cache_config.try_set(Some(Ok(c))),
            Err(e) => cache_config.try_set(Some(Err(e))),
        };
    });
    leptos::task::spawn_local(async move {
        match api::fetch_semantic_config().await {
            Ok(c) => semantic_config.try_set(Some(Ok(c))),
            Err(e) => semantic_config.try_set(Some(Err(e))),
        };
    });
    leptos::task::spawn_local(async move {
        match api::fetch_cache_ops().await {
            Ok(v) => ops.try_set(Some(Ok(v))),
            Err(e) => ops.try_set(Some(Err(e))),
        };
    });

    let reload_ops: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        leptos::task::spawn_local(async move {
            match api::fetch_cache_ops().await {
                Ok(v) => ops.try_set(Some(Ok(v))),
                Err(e) => ops.try_set(Some(Err(e))),
            };
        });
    });

    view! {
        <div class="space-y-6">
            <Alert variant="info" message=feedback.into() />

            // Main configuration grid
            <div class="cache-card-grid">
                // TTL Configuration Card
                {move || match cache_config.get() {
                    None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="glass-card text-error text-sm p-4">{e}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! { <TtlConfigPanel config feedback /> }.into_any(),
                }}

                // Semantic Cache Configuration Card
                {move || match semantic_config.get() {
                    None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="glass-card text-error text-sm p-4">{e}</div>
                    }.into_any(),
                    Some(Ok(config)) => view! { <SemanticConfigPanel config feedback /> }.into_any(),
                }}

                // Pricing Configuration Card
                <div class="glass-card">
                    <PricingConfigCard />
                </div>

                // Fingerprint & Stream Cache Card
                {move || {
                    let reload_ops = Arc::clone(&reload_ops);
                    match ops.get() {
                        None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                        Some(Err(e)) => view! {
                            <div class="glass-card text-error text-sm p-4">{e}</div>
                        }.into_any(),
                        Some(Ok(view)) => {
                            let fp_version = RwSignal::new(view.fingerprint_version.to_string());
                            let fp_normalize = RwSignal::new(view.fingerprint_normalize);
                            let stream_enabled = RwSignal::new(view.stream_cache_enabled);

                            view! {
                                <FingerprintStreamCard
                                    fp_version
                                    fp_normalize
                                    stream_enabled
                                    feedback
                                    reload_ops
                                />
                            }.into_any()
                        }
                    }
                }}
            </div>
        </div>
    }
}

#[component]
fn FingerprintStreamCard(
    fp_version: RwSignal<String>,
    fp_normalize: RwSignal<bool>,
    stream_enabled: RwSignal<bool>,
    feedback: RwSignal<String>,
    reload_ops: Arc<dyn Fn() + Send + Sync>,
) -> impl IntoView {
    let t = use_translations();

    view! {
        <div class="glass-card">
            // Card header with icon
            <div class="card-header-with-icon">
                <div class="card-header-icon">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                        <path fill-rule="evenodd" d="M10 1a4.5 4.5 0 00-4.5 4.5V9H5a2 2 0 00-2 2v6a2 2 0 002 2h10a2 2 0 002-2v-6a2 2 0 00-2-2h-.5V5.5A4.5 4.5 0 0010 1zm3 8V5.5a3 3 0 10-6 0V9h6z" clip-rule="evenodd" />
                    </svg>
                </div>
                <div class="card-header-text">
                    <div class="card-header-title">{t.cache_ops_fingerprint_title()}</div>
                    <div class="card-header-desc">"Fingerprint version and stream cache settings"</div>
                </div>
            </div>

            // Fingerprint version input
            <div class="cache-form-group">
                <label class="cache-form-label">{t.cache_ops_fingerprint_version()}</label>
                <input
                    type="number"
                    class="cache-form-input"
                    prop:value=move || fp_version.get()
                    on:input=move |ev| fp_version.set(event_target_value(&ev))
                />
            </div>

            // Normalize checkbox
            <label class="toggle-switch mt-4">
                <input
                    type="checkbox"
                    class="hidden"
                    prop:checked=move || fp_normalize.get()
                    on:change=move |ev| fp_normalize.set(event_target_checked(&ev))
                />
                <div class=move || if fp_normalize.get() { "toggle-track active" } else { "toggle-track" }>
                    <div class="toggle-thumb"></div>
                </div>
                <span class="toggle-label">{t.cache_ops_normalize()}</span>
            </label>

            // Stream cache toggle
            <div class="mt-4 pt-4 border-t border-theme">
                <label class="toggle-switch">
                    <input
                        type="checkbox"
                        class="hidden"
                        prop:checked=move || stream_enabled.get()
                        on:change=move |ev| {
                            let enabled = event_target_checked(&ev);
                            stream_enabled.set(enabled);
                            let reload_ops = Arc::clone(&reload_ops);
                            leptos::task::spawn_local(async move {
                                match api::update_stream_cache(&StreamCacheToggle { enabled }).await {
                                    Ok(_) => {
                                        feedback.try_set(t.routing_saved().to_string());
                                        (reload_ops)();
                                    }
                                    Err(e) => { feedback.try_set(e); }
                                };
                            });
                        }
                    />
                    <div class=move || if stream_enabled.get() { "toggle-track active" } else { "toggle-track" }>
                        <div class="toggle-thumb"></div>
                    </div>
                    <span class="toggle-label">{t.cache_ops_stream_cache()}</span>
                </label>
            </div>

            // Action bar
            <div class="card-action-bar">
                <span class="feedback-text">{move || feedback.get()}</span>
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
                                Ok(_) => feedback.try_set(t.routing_saved().to_string()),
                                Err(e) => feedback.try_set(e),
                            };
                        });
                    }
                >
                    {t.routing_save()}
                </button>
            </div>
        </div>
    }
}

#[component]
fn PricingConfigCard() -> impl IntoView {
    let t = use_translations();
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let data: RwSignal<Option<PricingConfigView>> = RwSignal::new(None);

    let input_price = RwSignal::new(0.55f64);
    let output_price = RwSignal::new(2.19f64);

    let load = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_cache_pricing_config().await {
                Ok(p) => {
                    input_price.set(p.default_input_price_per_million);
                    output_price.set(p.default_output_price_per_million);
                    data.set(Some(p));
                }
                Err(e) => feedback.set(e),
            }
        });
    };

    let save = move |_| {
        leptos::task::spawn_local(async move {
            let base = data.get().unwrap_or(PricingConfigView {
                default_input_price_per_million: 0.55,
                default_output_price_per_million: 2.19,
                model_overrides: vec![],
            });
            let req = PricingConfigView {
                default_input_price_per_million: input_price.get(),
                default_output_price_per_million: output_price.get(),
                model_overrides: base.model_overrides,
            };
            match api::update_cache_pricing_config(&req).await {
                Ok(p) => {
                    data.set(Some(p));
                    feedback.set(t.routing_saved().to_string());
                }
                Err(e) => feedback.set(e),
            }
        });
    };

    load();

    view! {
        // Card header with icon
        <div class="card-header-with-icon">
            <div class="card-header-icon">
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                    <path d="M8.433 7.418c.155-.103.346-.196.567-.267v1.698a2.305 2.305 0 01-.567-.267C8.07 8.34 8 8.114 8 8c0-.114.07-.34.433-.582zM11 12.849v-1.698c.22.071.412.164.567.267.364.243.433.468.433.582 0 .114-.07.34-.433.582a2.305 2.305 0 01-.567.267z" />
                    <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm1-13a1 1 0 10-2 0v.092a4.535 4.535 0 00-1.676.662C6.602 6.234 6 7.009 6 8c0 .99.602 1.765 1.324 2.246.48.32 1.054.545 1.676.662v1.941c-.391-.127-.68-.317-.843-.504a1 1 0 10-1.51 1.31c.562.649 1.413 1.076 2.353 1.253V15a1 1 0 102 0v-.092a4.535 4.535 0 001.676-.662C13.398 13.766 14 12.991 14 12c0-.99-.602-1.765-1.324-2.246A4.535 4.535 0 0011 9.092V7.151c.391.127.68.317.843.504a1 1 0 101.511-1.31c-.563-.649-1.413-1.076-2.354-1.253V5z" clip-rule="evenodd" />
                </svg>
            </div>
            <div class="card-header-text">
                <div class="card-header-title">"Pricing"</div>
                <div class="card-header-desc">"Default input/output price per million tokens"</div>
            </div>
        </div>

        // Price configuration
        <div class="space-y-4">
            <ConfigRangeF64
                label=move || format!("Input: ${:.4}/M tok", input_price.get())
                value=input_price
                min=0.01 max=10.0 step=0.01
                min_hint="$0.01" max_hint="$10.00"
                accent="blue"
            />
            <ConfigRangeF64
                label=move || format!("Output: ${:.4}/M tok", output_price.get())
                value=output_price
                min=0.01 max=20.0 step=0.01
                min_hint="$0.01" max_hint="$20.00"
                accent="purple"
            />
        </div>

        // Action bar
        <div class="card-action-bar">
            <span class="feedback-text">{move || {
                let msg = feedback.get();
                if !msg.is_empty() {
                    view! { <span class="text-xs text-blue-400">{msg}</span> }.into_any()
                } else {
                    ().into_any()
                }
            }}</span>
            <button on:click=save class="btn btn-primary text-sm">{t.routing_save()}</button>
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
            consumer_model_overrides: config.consumer_model_overrides.clone(),
        };
        leptos::task::spawn_local(async move {
            match api::update_cache_config(&req).await {
                Ok(_) => feedback.try_set(t.routing_saved().to_string()),
                Err(e) => feedback.try_set(e),
            };
        });
    };

    view! {
        <div class="glass-card">
            // Card header with icon
            <div class="card-header-with-icon">
                <div class="card-header-icon">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                        <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm1-12a1 1 0 10-2 0v4a1 1 0 00.293.707l2.828 2.829a1 1 0 101.415-1.415L11 9.586V6z" clip-rule="evenodd" />
                    </svg>
                </div>
                <div class="card-header-text">
                    <div class="card-header-title">{t.routing_cache_config_title()}</div>
                    <div class="card-header-desc">"Configure cache TTL for L0 and L1 layers"</div>
                </div>
            </div>

            // TTL configuration
            <div class="space-y-4">
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

            // Action bar
            <div class="card-action-bar">
                <span class="feedback-text">{move || feedback.get()}</span>
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
    let ttl = RwSignal::new(config.ttl_secs);
    let min_chars = RwSignal::new(config.min_query_chars as u64);
    let max_chars = RwSignal::new(config.max_query_chars as u64);
    let max_embeds = RwSignal::new(config.max_concurrent_embeds as u64);

    let on_save = move |_| {
        let req = UpdateSemanticConfigRequest {
            enabled: Some(enabled.get()),
            similarity_threshold: threshold.get(),
            ttl_secs: ttl.get(),
            min_query_chars: min_chars.get() as usize,
            max_query_chars: max_chars.get() as usize,
            max_concurrent_embeds: max_embeds.get() as usize,
        };
        leptos::task::spawn_local(async move {
            match api::update_semantic_config(&req).await {
                Ok(_) => feedback.try_set(t.routing_saved().to_string()),
                Err(e) => feedback.try_set(e),
            };
        });
    };

    view! {
        <div class="glass-card">
            // Card header with icon
            <div class="card-header-with-icon">
                <div class="card-header-icon">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                        <path d="M9 9a2 2 0 114 0 2 2 0 01-4 0z" />
                        <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm1-13a1 1 0 10-2 0v.092a4.535 4.535 0 00-1.676.662C6.602 6.234 6 7.009 6 8c0 .99.602 1.765 1.324 2.246.48.32 1.054.545 1.676.662v1.941c-.391-.127-.68-.317-.843-.504a1 1 0 10-1.51 1.31c.562.649 1.413 1.076 2.353 1.253V15a1 1 0 102 0v-.092a4.535 4.535 0 001.676-.662C13.398 13.766 14 12.991 14 12c0-.99-.602-1.765-1.324-2.246A4.535 4.535 0 0011 9.092V7.151c.391.127.68.317.843.504a1 1 0 101.511-1.31c-.563-.649-1.413-1.076-2.354-1.253V5z" clip-rule="evenodd" />
                    </svg>
                </div>
                <div class="card-header-text">
                    <div class="card-header-title">{t.routing_semantic_title()}</div>
                    <div class="card-header-desc">"Configure L2 semantic cache parameters"</div>
                </div>
            </div>

            // Enable toggle
            <label class="toggle-switch">
                <input
                    type="checkbox"
                    class="hidden"
                    prop:checked=move || enabled.get()
                    on:change=move |ev| enabled.set(event_target_checked(&ev))
                />
                <div class=move || if enabled.get() { "toggle-track active" } else { "toggle-track" }>
                    <div class="toggle-thumb"></div>
                </div>
                <span class="toggle-label">{t.cache_ops_semantic_enabled()}</span>
            </label>

            // Configuration parameters
            <div class="space-y-4 mt-4">
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
                <ConfigRangeU64
                    label=move || format!("{}: {}s", "TTL", ttl.get())
                    value=ttl min=60 max=2592000 min_hint="60s" max_hint="30d" accent="violet"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}", "Min Query Chars", min_chars.get())
                    value=min_chars min=8 max=256 min_hint="8" max_hint="256" accent="violet"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}", "Max Query Chars", max_chars.get())
                    value=max_chars min=512 max=16384 min_hint="512" max_hint="16384" accent="violet"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}", "Max Concurrent Embeds", max_embeds.get())
                    value=max_embeds min=1 max=16 min_hint="1" max_hint="16" accent="violet"
                />
            </div>

            // Impact hint
            <div class="semantic-impact">
                <div class="semantic-impact-label">{t.routing_impact_label()}</div>
                <div class="semantic-impact-value">
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

            // Action bar
            <div class="card-action-bar">
                <span class="feedback-text">{move || feedback.get()}</span>
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
    let _t = use_translations();
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let connection_config: RwSignal<Option<Result<ConnectionConfig, String>>> = RwSignal::new(None);
    let routing_status: RwSignal<Option<Result<RoutingStatus, String>>> = RwSignal::new(None);

    leptos::task::spawn_local(async move {
        match api::fetch_connection_config().await {
            Ok(c) => connection_config.try_set(Some(Ok(c))),
            Err(e) => connection_config.try_set(Some(Err(e))),
        };
    });
    leptos::task::spawn_local(async move {
        match api::fetch_routing_status().await {
            Ok(s) => routing_status.try_set(Some(Ok(s))),
            Err(e) => routing_status.try_set(Some(Err(e))),
        };
    });

    view! {
        <div class="space-y-6">
            <Alert variant="info" message=feedback.into() />

            // Connection configuration card
            {move || match connection_config.get() {
                None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm p-4">{e}</div>
                }.into_any(),
                Some(Ok(config)) => view! {
                    <ConnectionConfigPanel config feedback />
                }.into_any(),
            }}

            // Backend routing cards
            {move || match routing_status.get() {
                None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm p-4">{e}</div>
                }.into_any(),
                Some(Ok(status)) => view! {
                    <div class="cache-card-grid">
                        <RoutingStatusPanel status=status.clone() />
                        <BackendEditPanel status feedback />
                    </div>
                }.into_any(),
            }}
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
    let force_http1 = RwSignal::new(config.upstream_force_http1);
    let disable_keepalive = RwSignal::new(config.upstream_disable_keepalive);
    let req_timeout = RwSignal::new(config.upstream_request_timeout_secs);
    let write_timeout = RwSignal::new(config.upstream_write_timeout_secs);
    let conn_timeout = RwSignal::new(config.upstream_connection_timeout_secs);

    let on_save = move |_| {
        let req = crate::types::UpdateConnectionConfigRequest {
            tcp_keepalive_idle_secs: idle.get(),
            tcp_keepalive_interval_secs: interval.get(),
            tcp_keepalive_count: count.get() as usize,
            idle_timeout_secs: timeout.get(),
            h2_ping_interval_secs: h2_ping.get(),
            upstream_force_http1: force_http1.get(),
            upstream_disable_keepalive: disable_keepalive.get(),
            upstream_request_timeout_secs: req_timeout.get(),
            upstream_write_timeout_secs: write_timeout.get(),
            upstream_connection_timeout_secs: conn_timeout.get(),
        };
        leptos::task::spawn_local(async move {
            match api::update_connection_config(&req).await {
                Ok(_) => feedback.try_set(t.routing_saved().to_string()),
                Err(e) => feedback.try_set(e),
            };
        });
    };

    view! {
        <div class="glass-card">
            // Card header with icon
            <div class="card-header-with-icon">
                <div class="card-header-icon">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                        <path fill-rule="evenodd" d="M11.3 1.046A1 1 0 0112 2v5h4a1 1 0 01.82 1.573l-7 10A1 1 0 018 18v-5H4a1 1 0 01-.82-1.573l7-10a1 1 0 011.12-.38z" clip-rule="evenodd" />
                    </svg>
                </div>
                <div class="card-header-text">
                    <div class="card-card-header-title">{t.routing_connection_title()}</div>
                    <div class="card-header-desc">{t.routing_connection_desc()}</div>
                </div>
            </div>

            // Connection parameters grid
            <div class="config-grid-form">
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

            // Toggle options
            <div class="mt-4 space-y-3">
                <label class="toggle-switch">
                    <input
                        type="checkbox"
                        class="hidden"
                        prop:checked=move || force_http1.get()
                        on:change=move |ev| force_http1.set(event_target_checked(&ev))
                    />
                    <div class=move || if force_http1.get() { "toggle-track active" } else { "toggle-track" }>
                        <div class="toggle-thumb"></div>
                    </div>
                    <span class="toggle-label">"Upstream Force HTTP/1.1"</span>
                </label>
                <label class="toggle-switch">
                    <input
                        type="checkbox"
                        class="hidden"
                        prop:checked=move || disable_keepalive.get()
                        on:change=move |ev| disable_keepalive.set(event_target_checked(&ev))
                    />
                    <div class=move || if disable_keepalive.get() { "toggle-track active" } else { "toggle-track" }>
                        <div class="toggle-thumb"></div>
                    </div>
                    <span class="toggle-label">"Upstream Disable Keepalive"</span>
                </label>
            </div>

            // Timeout parameters
            <div class="config-grid-form mt-4">
                <ConfigRangeU64
                    label=move || format!("{}: {}s", "Request Timeout", req_timeout.get())
                    value=req_timeout min=10 max=600 min_hint="10s" max_hint="600s" accent="blue"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", "Write Timeout", write_timeout.get())
                    value=write_timeout min=10 max=600 min_hint="10s" max_hint="600s" accent="cyan"
                />
                <ConfigRangeU64
                    label=move || format!("{}: {}s", "Connection Timeout", conn_timeout.get())
                    value=conn_timeout min=5 max=120 min_hint="5s" max_hint="120s" accent="teal"
                />
            </div>

            // Action bar
            <div class="card-action-bar">
                <span class="feedback-text">{move || feedback.get()}</span>
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
        <div class="glass-card">
            // Card header with icon
            <div class="card-header-with-icon">
                <div class="card-header-icon">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                        <path d="M13 6a3 3 0 11-6 0 3 3 0 016 0zM18 8a2 2 0 11-4 0 2 2 0 014 0zM14 15a4 4 0 00-8 0v3h8v-3zM6 8a2 2 0 11-4 0 2 2 0 014 0zM16 18v-3a5.972 5.972 0 00-.75-2.906A3.005 3.005 0 0119 15v3h-3zM4.75 12.094A5.973 5.973 0 004 15v3H1v-3a3 3 0 013.75-2.906z" />
                    </svg>
                </div>
                <div class="card-header-text">
                    <div class="card-header-title">{t.routing_affinity_title()}</div>
                    <div class="card-header-desc">"Backend routing status and distribution"</div>
                </div>
            </div>

            // Status indicators
            <div class="card-status">
                <div class="card-status-dot active"></div>
                <span class="card-status-text">
                    {format!("{} active of {} backends", status.active_backends, status.total_backends)}
                </span>
            </div>

            // Backend distribution
            <div class="backend-distribution">
                <div class="text-xs font-medium text-theme-muted mb-2">{t.routing_distribution()}</div>
                {status.backends.into_iter().map(|backend| {
                    let pct = backend.request_count as f64 / total * 100.0;
                    let bar_class = if backend.healthy {
                        "backend-bar-fill"
                    } else {
                        "backend-bar-fill"
                    };
                    let bar_style = if !backend.healthy {
                        "background: var(--cc-error)"
                    } else {
                        ""
                    };
                    view! {
                        <div class="backend-row">
                            <span class="backend-name">{backend.name}</span>
                            <div class="backend-bar">
                                <div class=bar_class style=format!("width: {}% {}", pct.min(100.0), bar_style)></div>
                            </div>
                            <div class="backend-stats">
                                <span class="backend-count">{format!("{}", backend.request_count)}</span>
                                <span class="backend-pct">{format!("{:.0}%", pct)}</span>
                            </div>
                        </div>
                    }
                }).collect::<Vec<_>>()}
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
                Ok(_) => feedback.try_set(t.routing_saved().to_string()),
                Err(e) => feedback.try_set(e),
            };
            saving.try_set(false);
        });
    };

    view! {
        <div class="glass-card">
            // Card header with icon
            <div class="card-header-with-icon">
                <div class="card-header-icon">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                        <path fill-rule="evenodd" d="M11.49 3.17c-.38-1.56-2.6-1.56-2.98 0a1.532 1.532 0 01-2.286.948c-1.372-.836-2.942.734-2.106 2.106.54.886.061 2.042-.947 2.287-1.561.379-1.561 2.6 0 2.978a1.532 1.532 0 01.947 2.287c-.836 1.372.734 2.942 2.106 2.106a1.532 1.532 0 012.287.947c.379 1.561 2.6 1.561 2.978 0a1.533 1.533 0 012.287-.947c1.372.836 2.942-.734 2.106-2.106a1.533 1.533 0 01.947-2.287c1.561-.379 1.561-2.6 0-2.978a1.532 1.532 0 01-.947-2.287c.836-1.372-.734-2.942-2.106-2.106a1.532 1.532 0 01-2.287-.947zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd" />
                    </svg>
                </div>
                <div class="card-header-text">
                    <div class="card-header-title">"Backend Endpoints"</div>
                    <div class="card-header-desc">"Edit backend endpoints and weights"</div>
                </div>
            </div>

            // Backend table
            <div class="overflow-x-auto">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="text-left text-theme-muted border-b border-theme">
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
                                <tr class="border-b border-theme/50">
                                    <td class="py-2 pr-2">
                                        <input type="text" class="cache-form-input"
                                            prop:value=move || name_sig.get()
                                            on:input=move |e| name_sig.set(event_target_value(&e))
                                            placeholder="backend-1"
                                        />
                                    </td>
                                    <td class="py-2 pr-2">
                                        <input type="text" class="cache-form-input font-mono"
                                            prop:value=move || addr_sig.get()
                                            on:input=move |e| addr_sig.set(event_target_value(&e))
                                            placeholder="127.0.0.1:443"
                                        />
                                    </td>
                                    <td class="py-2 pr-2">
                                        <input type="number" class="cache-form-input"
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
            </div>

            // Action bar
            <div class="card-action-bar">
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
    }
}

// ---------------------------------------------------------------------------
// Tier hit-rate trend (L0/L1/L2) from overview timeseries
// ---------------------------------------------------------------------------

#[component]
fn CacheTierHitRateChart() -> impl IntoView {
    let window = RwSignal::new("24h".to_string());
    let points: RwSignal<Vec<TimeSeriesPoint>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);

    Effect::new(move |_| {
        let w = window.get();
        loading.set(true);
        leptos::task::spawn_local(async move {
            match api::fetch_overview_timeseries(&w).await {
                Ok(resp) => points.try_set(resp.points),
                Err(_) => points.try_set(Vec::new()),
            };
            loading.try_set(false);
        });
    });

    let x_labels = Signal::derive(move || {
        points
            .get()
            .iter()
            .map(|p| p.timestamp.clone())
            .collect::<Vec<_>>()
    });
    let tier_series = Signal::derive(move || {
        let pts = points.get();
        vec![
            ChartSeries {
                label: "L0".to_string(),
                color: "var(--cc-tier-l0)".to_string(),
                values: pts.iter().map(|p| Some(p.l0_hit_rate)).collect(),
                dashed: false,
                fill: false,
            },
            ChartSeries {
                label: "L1".to_string(),
                color: "var(--cc-tier-l1)".to_string(),
                values: pts.iter().map(|p| Some(p.l1_hit_rate)).collect(),
                dashed: false,
                fill: false,
            },
            ChartSeries {
                label: "L2".to_string(),
                color: "var(--cc-tier-l2)".to_string(),
                values: pts.iter().map(|p| Some(p.l2_hit_rate)).collect(),
                dashed: false,
                fill: false,
            },
        ]
    });

    view! {
        <div class="cache-chart-container">
            // Chart header with icon and controls
            <div class="cache-chart-header">
                <div class="card-header-with-icon mb-0 pb-0 border-b-0">
                    <div class="card-header-icon">
                        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                            <path d="M2 11a1 1 0 011-1h2a1 1 0 011 1v5a1 1 0 01-1 1H3a1 1 0 01-1-1v-5zM8 7a1 1 0 011-1h2a1 1 0 011 1v9a1 1 0 01-1 1H9a1 1 0 01-1-1V7zM14 4a1 1 0 011-1h2a1 1 0 011 1v12a1 1 0 01-1 1h-2a1 1 0 01-1-1V4z" />
                        </svg>
                    </div>
                    <div class="card-header-text">
                        <div class="card-header-title">"L0 / L1 / L2 Hit Rate Trend"</div>
                        <div class="card-header-desc">"Per-bucket share of requests served from each cache tier"</div>
                    </div>
                </div>
                <div class="cache-chart-controls">
                    <button
                        type="button"
                        class=move || if window.get() == "1h" { "cache-chart-btn active" } else { "cache-chart-btn" }
                        on:click=move |_| window.set("1h".to_string())
                    >
                        "1h"
                    </button>
                    <button
                        type="button"
                        class=move || if window.get() == "24h" { "cache-chart-btn active" } else { "cache-chart-btn" }
                        on:click=move |_| window.set("24h".to_string())
                    >
                        "24h"
                    </button>
                    <button
                        type="button"
                        class=move || if window.get() == "7d" { "cache-chart-btn active" } else { "cache-chart-btn" }
                        on:click=move |_| window.set("7d".to_string())
                    >
                        "7d"
                    </button>
                </div>
            </div>

            // Chart content
            {move || if loading.get() {
                view! { <crate::components::skeleton::SkeletonChart /> }.into_any()
            } else {
                view! {
                    <CanvasLineChart
                        x_labels=x_labels
                        series=tier_series
                        height_px=200
                        y_unit="%"
                        y_min=Some(0.0)
                        y_max=Some(100.0)
                        empty_message="Collecting tier metrics…"
                    />
                }.into_any()
            }}
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
                Ok(v) => ops.try_set(Some(Ok(v))),
                Err(e) => ops.try_set(Some(Err(e))),
            };
        });
    };

    reload();

    view! {
        <div class="space-y-6">
            // Chart section
            <CacheTierHitRateChart />

            // Alert message
            <Alert variant="info" message=message.into() />

            // Cache invalidation card
            {move || match ops.get() {
                None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm p-4">{e}</div>
                }.into_any(),
                Some(Ok(view)) => {
                    let last = view.last_invalidate.clone();
                    view! {
                        <div class="glass-card">
                            // Card header with icon
                            <div class="card-header-with-icon">
                                <div class="card-header-icon">
                                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                                        <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                    </svg>
                                </div>
                                <div class="card-header-text">
                                    <div class="card-header-title">{t.cache_ops_invalidate_title()}</div>
                                    <div class="card-header-desc">"Clear cached data by scope"</div>
                                </div>
                            </div>

                            // Scope input
                            <div class="cache-form-group">
                                <label class="cache-form-label">{t.cache_ops_scope()}</label>
                                <input
                                    type="text"
                                    class="cache-form-input font-mono"
                                    placeholder="all"
                                    prop:value=move || scope.get()
                                    on:input=move |ev| scope.set(event_target_value(&ev))
                                />
                            </div>

                            // Invalidate button
                            <button
                                class="btn btn-secondary text-xs mt-4"
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
                                                    message.try_set(format!("{}: {}", r.scope, r.status));
                                                    reload();
                                                }
                                                Err(e) => { message.try_set(e); }
                                            };
                                        });
                                    }
                                }
                            >
                                {t.cache_ops_invalidate_btn()}
                            </button>

                            // Status section
                            <div class="mt-4 pt-4 border-t border-theme">
                                {if view.invalidate_all_in_progress {
                                    view! {
                                        <div class="card-status">
                                            <div class="card-status-dot warning"></div>
                                            <span class="card-status-text">{t.cache_ops_invalidate_running()}</span>
                                        </div>
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }}
                                {if let Some(job) = view.invalidate_job.clone() {
                                    view! {
                                        <div class="metric-row">
                                            <span class="metric-label">{t.cache_ops_invalidate_job()}</span>
                                            <span class="metric-value">{format!("{} — {}", job.scope, job.phase)}</span>
                                        </div>
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }}
                                <div class="metric-row">
                                    <span class="metric-label">{t.cache_ops_last_invalidate()}</span>
                                    <span class="metric-value">
                                        {if let Some(li) = last {
                                            format!("scope={} status={}", li.scope, li.status)
                                        } else {
                                            t.cache_ops_none().to_string()
                                        }}
                                    </span>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                }
            }}

            // Confirm all modal
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
                                                message.try_set(format!("{}: {}", r.scope, r.status));
                                                reload();
                                            }
                                            Err(e) => { message.try_set(e); }
                                        };
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
    let cached_etag = RwSignal::new(String::new());

    let load_analysis = move || {
        let etag = cached_etag.get_untracked();
        leptos::task::spawn_local(async move {
            match api::fetch_trace_analysis_etag(24, &etag).await {
                Ok(result) => {
                    cached_etag.try_set(result.etag);
                    if let Some(a) = result.analysis {
                        analysis.try_set(Some(Ok(a)));
                    }
                }
                Err(e) => { analysis.try_set(Some(Err(e))); }
            };
        });
    };

    load_analysis();

    view! {
        <div class="space-y-6">
            // Header with refresh button
            <div class="flex items-center justify-between">
                <p class="text-xs text-theme-muted max-w-md hidden md:block">
                    {t.trace_hours_note()}
                </p>
                <button on:click=move |_| load_analysis() class="btn btn-secondary text-sm">
                    {t.trace_refresh()}
                </button>
            </div>

            // Trace analysis content
            {move || match analysis.get() {
                None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm p-4">
                        {format!("{}: {}", use_translations().trace_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(data)) => {
                    let t = use_translations();
                    let cluster_dist = data.cluster_distribution.clone();
                    view! {
                        <div class="space-y-6">
                            // Statistics grid
                            <div class="trace-stats-grid">
                                <div class="trace-stat-card">
                                    <div class="trace-stat-label">{t.trace_total_requests()}</div>
                                    <div class="trace-stat-value">{format!("{}", data.total_requests)}</div>
                                </div>
                                <div class="trace-stat-card">
                                    <div class="trace-stat-label">{t.trace_unique_requests()}</div>
                                    <div class="trace-stat-value">{format!("{}", data.unique_requests)}</div>
                                </div>
                                <div class="trace-stat-card">
                                    <div class="trace-stat-label">{t.trace_repeat_ratio()}</div>
                                    <div class="trace-stat-value accent">{format!("{:.1}%", data.repeat_ratio * 100.0)}</div>
                                </div>
                                <div class="trace-stat-card">
                                    <div class="trace-stat-label flex items-center gap-1">
                                        {t.trace_estimated_hit_rate()}
                                        <span class="cursor-help" title=t.trace_estimated_hit_rate_hint()>"?"</span>
                                    </div>
                                    <div class="trace-stat-value success">{format!("{:.1}%", data.estimated_hit_rate * 100.0)}</div>
                                </div>
                            </div>

                            // Secondary metrics
                            <div class="cache-card-grid">
                                <div class="glass-card">
                                    <div class="card-header-with-icon">
                                        <div class="card-header-icon">
                                            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                                                <path d="M9 9a2 2 0 114 0 2 2 0 01-4 0z" />
                                                <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm1-13a1 1 0 10-2 0v.092a4.535 4.535 0 00-1.676.662C6.602 6.234 6 7.009 6 8c0 .99.602 1.765 1.324 2.246.48.32 1.054.545 1.676.662v1.941c-.391-.127-.68-.317-.843-.504a1 1 0 10-1.51 1.31c.562.649 1.413 1.076 2.353 1.253V15a1 1 0 102 0v-.092a4.535 4.535 0 001.676-.662C13.398 13.766 14 12.991 14 12c0-.99-.602-1.765-1.324-2.246A4.535 4.535 0 0011 9.092V7.151c.391.127.68.317.843.504a1 1 0 101.511-1.31c-.563-.649-1.413-1.076-2.354-1.253V5z" clip-rule="evenodd" />
                                            </svg>
                                        </div>
                                        <div class="card-header-text">
                                            <div class="card-header-title">{t.trace_semantic_ratio()}</div>
                                            <div class="card-header-desc">"Semantic cluster distribution"</div>
                                        </div>
                                    </div>
                                    <div class="metric-row">
                                        <span class="metric-label">{t.trace_semantic_ratio()}</span>
                                        <span class="metric-value">{format!("{:.1}%", data.semantic_cluster_ratio * 100.0)}</span>
                                    </div>
                                    <div class="metric-row">
                                        <span class="metric-label">{t.trace_zipf_alpha()}</span>
                                        <span class="metric-value">{format!("{:.2}", data.estimated_zipf_alpha)}</span>
                                    </div>
                                    <div class="metric-row">
                                        <span class="metric-label">{t.trace_cache_hit_ratio()}</span>
                                        <span class="metric-value">{format!("{:.1}%", data.cache_hit_ratio * 100.0)}</span>
                                    </div>
                                </div>

                                <div class="glass-card">
                                    <div class="card-header-with-icon">
                                        <div class="card-header-icon">
                                            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                                                <path fill-rule="evenodd" d="M11.3 1.046A1 1 0 0112 2v5h4a1 1 0 01.82 1.573l-7 10A1 1 0 018 18v-5H4a1 1 0 01-.82-1.573l7-10a1 1 0 011.12-.38z" clip-rule="evenodd" />
                                            </svg>
                                        </div>
                                        <div class="card-header-text">
                                            <div class="card-header-title">{t.trace_avg_metrics()}</div>
                                            <div class="card-header-desc">"Average performance metrics"</div>
                                        </div>
                                    </div>
                                    <div class="metric-row">
                                        <span class="metric-label">{t.trace_avg_latency()}</span>
                                        <span class="metric-value">{format!("{:.1}ms", data.avg_latency_ms)}</span>
                                    </div>
                                    <div class="metric-row">
                                        <span class="metric-label">{t.trace_avg_tokens()}</span>
                                        <span class="metric-value">{format!("{:.0}", data.avg_prompt_tokens)}</span>
                                    </div>
                                </div>
                            </div>

                            // Top models card
                            <div class="glass-card">
                                <div class="card-header-with-icon">
                                    <div class="card-header-icon">
                                        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                                            <path d="M9 4.804A7.968 7.968 0 005.5 4c-1.255 0-2.443.29-3.5.804v10A7.969 7.969 0 015.5 14c1.669 0 3.218.51 4.5 1.385A7.962 7.962 0 0114.5 14c1.255 0 2.443.29 3.5.804v-10A7.968 7.968 0 0014.5 4c-1.255 0-2.443.29-3.5.804V12a1 1 0 11-2 0V4.804z" />
                                        </svg>
                                    </div>
                                    <div class="card-header-text">
                                        <div class="card-header-title">{t.trace_top_models()}</div>
                                        <div class="card-header-desc">"Most frequently used models"</div>
                                    </div>
                                </div>
                                <div class="space-y-2">
                                    {data.top_models.iter().map(|m| {
                                        view! {
                                            <div class="metric-row">
                                                <span class="metric-label">{m.model.clone()}</span>
                                                <div class="flex items-center gap-2">
                                                    <span class="text-xs font-mono text-theme-muted">{format!("{}", m.count)}</span>
                                                    <span class="text-xs font-mono text-accent">{format!("{:.1}%", m.percentage)}</span>
                                                </div>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            </div>

                            // Cluster distribution chart
                            <div class="glass-card">
                                <div class="card-header-with-icon">
                                    <div class="card-header-icon">
                                        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                                            <path d="M2 11a1 1 0 011-1h2a1 1 0 011 1v5a1 1 0 01-1 1H3a1 1 0 01-1-1v-5zM8 7a1 1 0 011-1h2a1 1 0 011 1v9a1 1 0 01-1 1H9a1 1 0 01-1-1V7zM14 4a1 1 0 011-1h2a1 1 0 011 1v12a1 1 0 01-1 1h-2a1 1 0 01-1-1V4z" />
                                        </svg>
                                    </div>
                                    <div class="card-header-text">
                                        <div class="card-header-title">{t.trace_cluster_distribution()}</div>
                                        <div class="card-header-desc">"Request cluster distribution"</div>
                                    </div>
                                </div>
                                {if cluster_dist.is_empty() {
                                    view! { <p class="text-xs text-theme-muted">"No cluster data"</p> }.into_any()
                                } else {
                                    let segment_colors: [&str; 5] = [
                                        "var(--accent-primary)",
                                        "var(--warning)",
                                        "var(--info)",
                                        "var(--success)",
                                        "var(--error)",
                                    ];
                                    let top_count = 5.min(cluster_dist.len());
                                    let mut segments: Vec<DonutSegment> = cluster_dist.iter().take(top_count).enumerate().map(|(i, c)| {
                                        DonutSegment {
                                            label: format!("C{}", c.cluster_id),
                                            value: c.count as f64,
                                            color: segment_colors[i % segment_colors.len()],
                                        }
                                    }).collect();
                                    if cluster_dist.len() > top_count {
                                        let other_count: usize = cluster_dist[top_count..].iter().map(|c| c.count).sum();
                                        segments.push(DonutSegment {
                                            label: "Other".into(),
                                            value: other_count as f64,
                                            color: "var(--cc-text-muted)",
                                        });
                                    }
                                    view! {
                                        <DonutChart
                                            segments=segments
                                            center_label=format!("{} clusters", cluster_dist.len())
                                        />
                                    }.into_any()
                                }}
                            </div>

                            // Zipf distribution chart
                            {if !data.zipf_log_points.is_empty() {
                                let zipf_points = std::sync::Arc::new(data.zipf_log_points.clone());
                                let slope = data.zipf_regression_slope;
                                let intercept = data.zipf_regression_intercept;
                                let scatter_points = {
                                    let zipf_points = std::sync::Arc::clone(&zipf_points);
                                    Signal::derive(move || {
                                        zipf_points
                                            .iter()
                                            .enumerate()
                                            .map(|(i, p)| ScatterPoint {
                                                x: p.log_rank,
                                                y: p.log_freq,
                                                color: "var(--accent-primary)",
                                                label: format!("#{}", i + 1),
                                            })
                                            .collect()
                                    })
                                };
                                view! {
                                    <div class="cache-chart-container">
                                        <div class="cache-chart-header">
                                            <div class="card-header-with-icon mb-0 pb-0 border-b-0">
                                                <div class="card-header-icon">
                                                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                                                        <path d="M2 11a1 1 0 011-1h2a1 1 0 011 1v5a1 1 0 01-1 1H3a1 1 0 01-1-1v-5zM8 7a1 1 0 011-1h2a1 1 0 011 1v9a1 1 0 01-1 1H9a1 1 0 01-1-1V7zM14 4a1 1 0 011-1h2a1 1 0 011 1v12a1 1 0 01-1 1h-2a1 1 0 01-1-1V4z" />
                                                    </svg>
                                                </div>
                                                <div class="card-header-text">
                                                    <div class="card-header-title">{t.trace_zipf_chart()}</div>
                                                    <div class="card-header-desc">"Zipf distribution analysis"</div>
                                                </div>
                                            </div>
                                        </div>
                                        <ScatterChart
                                            points=scatter_points
                                            x_label="ln(rank)".to_string()
                                            y_label="ln(freq)".to_string()
                                            height_px=200
                                            empty_message=""
                                            fit_line=Some((slope, intercept))
                                        />
                                        <div class="text-xs text-theme-muted font-mono mt-2">
                                            {format!("Fit: slope = {:.2}, intercept = {:.2}", slope, intercept)}
                                        </div>
                                    </div>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}

                            // DeepSeek audit card
                            {data.deepseek_user_id.clone().map(|audit| {
                                let ok = audit.isolation_ok;
                                let conclusion = audit.conclusion.clone();
                                let top_projects = audit.top_project_ids.clone();
                                let breakdown = audit.audit_breakdown.clone();
                                view! {
                                    <div class="glass-card">
                                        <div class="card-header-with-icon">
                                            <div class="card-header-icon">
                                                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
                                                    <path fill-rule="evenodd" d="M2.166 4.999A11.954 11.954 0 0010 1.944 11.954 11.954 0 0017.834 5c.11.65.166 1.32.166 2.001 0 5.225-3.34 9.67-8 11.317C5.34 16.67 2 12.225 2 7c0-.682.057-1.35.166-2.001zm11.541 3.708a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" />
                                                </svg>
                                            </div>
                                            <div class="card-header-text">
                                                <div class="card-header-title">{t.trace_deepseek_user_id_title()}</div>
                                                <div class="card-header-desc">{t.trace_deepseek_user_id_hint()}</div>
                                            </div>
                                        </div>

                                        // Status indicator
                                        <div class="card-status">
                                            <div class=move || if ok { "card-status-dot active" } else { "card-status-dot warning" }></div>
                                            <span class="card-status-text">
                                                {move || if ok { t.trace_isolation_ok() } else { t.trace_isolation_fail() }}
                                                <span class="text-theme-muted font-normal ml-2">{conclusion.clone()}</span>
                                            </span>
                                        </div>

                                        // Audit metrics
                                        <div class="trace-stats-grid">
                                            <div class="trace-stat-card">
                                                <div class="trace-stat-label">{t.trace_deepseek_requests()}</div>
                                                <div class="trace-stat-value">{audit.deepseek_requests}</div>
                                            </div>
                                            <div class="trace-stat-card">
                                                <div class="trace-stat-label">{t.trace_upstream_user_id_ratio()}</div>
                                                <div class="trace-stat-value">{format!("{:.1}%", audit.upstream_user_id_ratio * 100.0)}</div>
                                            </div>
                                            <div class="trace-stat-card">
                                                <div class="trace-stat-label">{t.trace_missing_project_id()}</div>
                                                <div class="trace-stat-value">{audit.missing_project_id}</div>
                                            </div>
                                            <div class="trace-stat-card">
                                                <div class="trace-stat-label">{t.trace_client_user_id_leaks()}</div>
                                                <div class="trace-stat-value">{audit.client_user_id_leaks}</div>
                                            </div>
                                        </div>

                                        // Audit breakdown
                                        <div class="cache-card-grid">
                                            <div>
                                                <h4 class="text-xs font-semibold text-theme-muted mb-2">{t.trace_audit_injected()}</h4>
                                                <div class="text-sm font-mono space-y-1">
                                                    <div class="metric-row">
                                                        <span class="metric-label">injected</span>
                                                        <span class="metric-value">{breakdown.injected}</span>
                                                    </div>
                                                    <div class="metric-row">
                                                        <span class="metric-label">absent</span>
                                                        <span class="metric-value">{breakdown.absent}</span>
                                                    </div>
                                                    <div class="metric-row">
                                                        <span class="metric-label">stripped_client</span>
                                                        <span class="metric-value">{breakdown.stripped_client}</span>
                                                    </div>
                                                    <div class="metric-row">
                                                        <span class="metric-label">mismatch</span>
                                                        <span class="metric-value">{breakdown.mismatch}</span>
                                                    </div>
                                                </div>
                                            </div>
                                            <div>
                                                <h4 class="text-xs font-semibold text-theme-muted mb-2">{t.trace_top_project_ids()}</h4>
                                                <div class="space-y-1">
                                                    {top_projects.into_iter().map(|p| {
                                                        view! {
                                                            <div class="metric-row">
                                                                <span class="metric-label truncate">{p.project_id}</span>
                                                                <span class="metric-value">{format!("{} ({:.1}%)", p.count, p.percentage)}</span>
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
