use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::FeaturesConfigView;

/// Feature config card: all experimental feature toggles in a single card.
#[component]
pub fn FeaturesGrid() -> impl IntoView {
    let t = use_translations();
    let feedback: RwSignal<String> = RwSignal::new(String::new());
    let loaded = RwSignal::new(false);
    let cfg: RwSignal<Option<FeaturesConfigView>> = RwSignal::new(None);

    // MIMO group
    let mimo_compression = RwSignal::new(false);
    let mimo_compression_threshold = RwSignal::new(6u64);
    let mimo_retire_prefix = RwSignal::new(false);
    let mimo_keep_turns = RwSignal::new(6u64);
    let mimo_session = RwSignal::new(false);
    let mimo_session_ttl = RwSignal::new(86400u64);
    let mimo_session_msgs = RwSignal::new(200u64);

    // Cache group
    let prefix_aware = RwSignal::new(false);
    let delta_cache = RwSignal::new(false);

    // Connection group
    let conn_prewarm = RwSignal::new(false);
    let affinity_feedback = RwSignal::new(false);
    let streaming_forward = RwSignal::new(false);
    let upstream_gzip = RwSignal::new(false);
    let upstream_gzip_min = RwSignal::new(4096u64);
    let passthrough_bytes = RwSignal::new(1024u64);

    // Routing group
    let route_strategy = RwSignal::new("round_robin".to_string());
    let load_aware_routing = RwSignal::new(false);
    let max_concurrent = RwSignal::new(0u64);
    let weight_health = RwSignal::new(0.2f64);
    let weight_latency = RwSignal::new(0.2f64);
    let weight_load = RwSignal::new(0.2f64);
    let weight_affinity = RwSignal::new(0.2f64);
    let weight_429 = RwSignal::new(0.2f64);

    // Preflight group
    let preflight_enabled = RwSignal::new(false);
    let preflight_health = RwSignal::new(true);
    let preflight_429 = RwSignal::new(true);

    // Reserved group
    let io_uring = RwSignal::new(false);
    let wasm = RwSignal::new(false);

    let load = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_features_config().await {
                Ok(c) => {
                    prefix_aware.set(c.prefix_aware_cache);
                    streaming_forward.set(c.streaming_body_forward);
                    conn_prewarm.set(c.connection_prewarm);
                    affinity_feedback.set(c.affinity_prompt_cache_feedback);
                    delta_cache.set(c.delta_cache);
                    io_uring.set(c.io_uring_backend);
                    wasm.set(c.wasm_filters);
                    mimo_compression.set(c.mimo_context_compression);
                    mimo_compression_threshold.set(c.mimo_compression_threshold as u64);
                    upstream_gzip.set(c.upstream_request_gzip);
                    upstream_gzip_min.set(c.upstream_request_gzip_min_bytes as u64);
                    mimo_retire_prefix.set(c.mimo_retire_prefix_messages);
                    mimo_keep_turns.set(c.mimo_keep_recent_turns as u64);
                    mimo_session.set(c.mimo_session_store);
                    mimo_session_ttl.set(c.mimo_session_store_ttl_secs);
                    mimo_session_msgs.set(c.mimo_session_store_max_messages as u64);
                    passthrough_bytes.set(c.passthrough_prefix_bytes as u64);
                    route_strategy.set(c.backend_route_strategy.clone());
                    load_aware_routing.set(c.backend_load_aware_routing_enabled);
                    max_concurrent.set(c.backend_max_concurrent_requests as u64);
                    weight_health.set(c.backend_health_weight);
                    weight_latency.set(c.backend_latency_weight);
                    weight_load.set(c.backend_load_weight);
                    weight_affinity.set(c.backend_affinity_weight);
                    weight_429.set(c.backend_rate_429_weight);
                    preflight_enabled.set(c.preflight_enabled);
                    preflight_health.set(c.preflight_check_health);
                    preflight_429.set(c.preflight_check_429_cooldown);
                    cfg.set(Some(c));
                    loaded.set(true);
                }
                Err(e) => feedback.set(e),
            }
        });
    };

    let save = move |_| {
        leptos::task::spawn_local(async move {
            let req = FeaturesConfigView {
                prefix_aware_cache: prefix_aware.get(),
                streaming_body_forward: streaming_forward.get(),
                connection_prewarm: conn_prewarm.get(),
                affinity_prompt_cache_feedback: affinity_feedback.get(),
                delta_cache: delta_cache.get(),
                io_uring_backend: io_uring.get(),
                wasm_filters: wasm.get(),
                mimo_context_compression: mimo_compression.get(),
                mimo_compression_threshold: mimo_compression_threshold.get() as usize,
                upstream_request_gzip: upstream_gzip.get(),
                upstream_request_gzip_min_bytes: upstream_gzip_min.get() as usize,
                mimo_retire_prefix_messages: mimo_retire_prefix.get(),
                mimo_keep_recent_turns: mimo_keep_turns.get() as usize,
                mimo_session_store: mimo_session.get(),
                mimo_session_store_ttl_secs: mimo_session_ttl.get(),
                mimo_session_store_max_messages: mimo_session_msgs.get() as usize,
                passthrough_prefix_bytes: passthrough_bytes.get() as usize,
                backend_route_strategy: route_strategy.get(),
                backend_load_aware_routing_enabled: load_aware_routing.get(),
                backend_max_concurrent_requests: max_concurrent.get() as usize,
                backend_health_weight: weight_health.get(),
                backend_latency_weight: weight_latency.get(),
                backend_load_weight: weight_load.get(),
                backend_affinity_weight: weight_affinity.get(),
                backend_rate_429_weight: weight_429.get(),
                preflight_enabled: preflight_enabled.get(),
                preflight_check_health: preflight_health.get(),
                preflight_check_429_cooldown: preflight_429.get(),
            };
            match api::update_features_config(&req).await {
                Ok(c) => {
                    cfg.set(Some(c));
                    feedback.set(t.routing_saved().to_string());
                }
                Err(e) => feedback.set(e),
            }
        });
    };

    let show_weights = move || route_strategy.get() == "weighted_score";

    load();

    // Pre-compute locale signals
    let lbl_mimo_compression = Signal::derive(move || t.feat_mimo_compression().to_string());
    let lbl_mimo_threshold = Signal::derive(move || format!("{}: {}", t.feat_mimo_threshold(), mimo_compression_threshold.get()));
    let lbl_mimo_retire = Signal::derive(move || t.feat_mimo_retire_prefix().to_string());
    let lbl_mimo_keep = Signal::derive(move || format!("{}: {}", t.feat_mimo_keep_turns(), mimo_keep_turns.get()));
    let lbl_mimo_session = Signal::derive(move || t.feat_mimo_session().to_string());
    let lbl_mimo_ttl = Signal::derive(move || format!("{}: {}", t.feat_mimo_session_ttl(), mimo_session_ttl.get()));
    let lbl_mimo_msgs = Signal::derive(move || format!("{}: {}", t.feat_mimo_session_msgs(), mimo_session_msgs.get()));
    let lbl_mimo_gzip = Signal::derive(move || t.feat_mimo_gzip().to_string());
    let lbl_mimo_gzip_min = Signal::derive(move || format!("{}: {}", t.feat_mimo_gzip_min(), upstream_gzip_min.get()));
    let lbl_prefix_aware = Signal::derive(move || t.feat_prefix_aware().to_string());
    let lbl_delta_cache = Signal::derive(move || t.feat_delta_cache().to_string());
    let lbl_streaming = Signal::derive(move || t.feat_streaming_forward().to_string());
    let lbl_passthrough = Signal::derive(move || format!("{}: {}", t.feat_passthrough_bytes(), passthrough_bytes.get()));
    let lbl_conn_prewarm = Signal::derive(move || t.feat_conn_prewarm().to_string());
    let lbl_affinity = Signal::derive(move || t.feat_affinity_feedback().to_string());
    let lbl_load_aware = Signal::derive(move || t.feat_load_aware().to_string());
    let lbl_max_conc = Signal::derive(move || {
        let v = max_concurrent.get();
        if v == 0 { t.feat_max_concurrent_unlimited().to_string() } else { format!("{}: {}", t.feat_max_concurrent(), v) }
    });
    let lbl_weight_health = Signal::derive(move || format!("{}: {:.2}", t.feat_weight_health(), weight_health.get()));
    let lbl_weight_latency = Signal::derive(move || format!("{}: {:.2}", t.feat_weight_latency(), weight_latency.get()));
    let lbl_weight_load = Signal::derive(move || format!("{}: {:.2}", t.feat_weight_load(), weight_load.get()));
    let lbl_weight_affinity = Signal::derive(move || format!("{}: {:.2}", t.feat_weight_affinity(), weight_affinity.get()));
    let lbl_weight_429 = Signal::derive(move || format!("{}: {:.2}", t.feat_weight_429(), weight_429.get()));
    let lbl_preflight = Signal::derive(move || t.feat_preflight_enabled().to_string());
    let lbl_preflight_health = Signal::derive(move || t.feat_preflight_health().to_string());
    let lbl_preflight_429 = Signal::derive(move || t.feat_preflight_429().to_string());
    let lbl_io_uring = Signal::derive(move || t.feat_io_uring().to_string());
    let lbl_wasm = Signal::derive(move || t.feat_wasm_filters().to_string());

    view! {
        <div class="config-card glass-card" style="grid-column: 1 / -1;">
            <div class="config-card-head">
                <h3 class="config-card-title">{t.feat_group_mimo()}</h3>
            </div>
            <div class="config-card-body space-y-4">
                // ── MiMo / Context ──
                <div class="feat-group">
                    <h5 class="feat-group-title">{t.feat_group_mimo()}</h5>
                    <ToggleSwitch label=lbl_mimo_compression value=mimo_compression />
                    <ConfigRangeU64 label=move || lbl_mimo_threshold.get() value=mimo_compression_threshold min=2 max=20 min_hint="2" max_hint="20" accent="accent" />
                    <ToggleSwitch label=lbl_mimo_retire value=mimo_retire_prefix />
                    <ConfigRangeU64 label=move || lbl_mimo_keep.get() value=mimo_keep_turns min=1 max=20 min_hint="1" max_hint="20" accent="accent" />
                    <ToggleSwitch label=lbl_mimo_session value=mimo_session />
                    <ConfigRangeU64 label=move || lbl_mimo_ttl.get() value=mimo_session_ttl min=60 max=604800 min_hint="1m" max_hint="7d" accent="accent" />
                    <ConfigRangeU64 label=move || lbl_mimo_msgs.get() value=mimo_session_msgs min=10 max=1000 min_hint="10" max_hint="1000" accent="accent" />
                    <ToggleSwitch label=lbl_mimo_gzip value=upstream_gzip />
                    <ConfigRangeU64 label=move || lbl_mimo_gzip_min.get() value=upstream_gzip_min min=256 max=65536 min_hint="256" max_hint="64KB" accent="accent" />
                </div>

                // ── Cache ──
                <div class="feat-group">
                    <h5 class="feat-group-title">{t.feat_group_cache()}</h5>
                    <ToggleSwitch label=lbl_prefix_aware value=prefix_aware />
                    <ToggleSwitch label=lbl_delta_cache value=delta_cache />
                    <ToggleSwitch label=lbl_streaming value=streaming_forward />
                    <ConfigRangeU64 label=move || lbl_passthrough.get() value=passthrough_bytes min=128 max=8192 min_hint="128" max_hint="8KB" accent="accent" />
                </div>

                // ── Connection ──
                <div class="feat-group">
                    <h5 class="feat-group-title">{t.feat_group_connection()}</h5>
                    <ToggleSwitch label=lbl_conn_prewarm value=conn_prewarm />
                    <ToggleSwitch label=lbl_affinity value=affinity_feedback />
                </div>

                // ── Routing ──
                <div class="feat-group">
                    <h5 class="feat-group-title">{t.feat_group_routing()}</h5>
                    <ToggleSwitch label=lbl_load_aware value=load_aware_routing />
                    <ConfigRangeU64 label=move || lbl_max_conc.get() value=max_concurrent min=0 max=1000 min_hint="0=unlimited" max_hint="1000" accent="accent" />
                    <ConfigSelect label=t.feat_route_strategy() value=route_strategy options=vec![("round_robin", "Round Robin"), ("least_load", "Least Load"), ("weighted_score", "Weighted Score")] />
                    {move || {
                        if show_weights() {
                            view! {
                                <div class="pl-4 space-y-2 mt-2" style="border-left: 2px solid var(--cc-border);">
                                    <ConfigRangeF64 label=move || lbl_weight_health.get() value=weight_health min=0.0 max=1.0 step=0.05 min_hint="0.0" max_hint="1.0" accent="green" />
                                    <ConfigRangeF64 label=move || lbl_weight_latency.get() value=weight_latency min=0.0 max=1.0 step=0.05 min_hint="0.0" max_hint="1.0" accent="green" />
                                    <ConfigRangeF64 label=move || lbl_weight_load.get() value=weight_load min=0.0 max=1.0 step=0.05 min_hint="0.0" max_hint="1.0" accent="green" />
                                    <ConfigRangeF64 label=move || lbl_weight_affinity.get() value=weight_affinity min=0.0 max=1.0 step=0.05 min_hint="0.0" max_hint="1.0" accent="green" />
                                    <ConfigRangeF64 label=move || lbl_weight_429.get() value=weight_429 min=0.0 max=1.0 step=0.05 min_hint="0.0" max_hint="1.0" accent="green" />
                                </div>
                            }.into_any()
                        } else { ().into_any() }
                    }}
                </div>

                // ── Preflight ──
                <div class="feat-group">
                    <h5 class="feat-group-title">{t.feat_group_preflight()}</h5>
                    <ToggleSwitch label=lbl_preflight value=preflight_enabled />
                    <ToggleSwitch label=lbl_preflight_health value=preflight_health />
                    <ToggleSwitch label=lbl_preflight_429 value=preflight_429 />
                </div>

                // ── Reserved ──
                <div class="feat-group">
                    <h5 class="feat-group-title">{t.feat_group_reserved()}</h5>
                    <ToggleSwitch label=lbl_io_uring value=io_uring />
                    <ToggleSwitch label=lbl_wasm value=wasm />
                </div>

                // ── Save ──
                <div class="flex items-center gap-3 pt-2" style="border-top: 1px solid var(--cc-border-light);">
                    <button on:click=save class="btn btn-primary text-sm">{t.routing_save()}</button>
                    {move || {
                        let msg = feedback.get();
                        if !msg.is_empty() {
                            view! { <p class="text-xs" style="color: var(--cc-accent);">{msg}</p> }.into_any()
                        } else { ().into_any() }
                    }}
                </div>
            </div>
        </div>
    }
}

/// Toggle switch component accepting a locale-driven label signal.
#[component]
fn ToggleSwitch(label: Signal<String>, value: RwSignal<bool>) -> impl IntoView {
    view! {
        <label class="flex items-center gap-2 text-xs cursor-pointer select-none" style="color: var(--cc-text-muted);">
            <input type="checkbox" prop:checked=move || value.get() on:change=move |ev| value.set(event_target_checked(&ev)) />
            {move || label.get()}
        </label>
    }
}
