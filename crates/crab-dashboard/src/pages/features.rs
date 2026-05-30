use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::FeaturesConfigView;

#[component]
pub fn FeaturesTab() -> impl IntoView {
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
    let _body_quick = RwSignal::new(false);

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

    load();

    view! {
        <div class="space-y-6">
            <h3 class="text-lg font-semibold">"Experimental Features"</h3>
            <p class="text-xs text-theme-secondary mb-4">
                "Toggle experimentellen Datenflächen für MiMo, Cache-Verbindung und reservierte Funktionen."
            </p>

            // ── MiMo group ──
            <div class="config-card glass-card">
                <div class="config-card-head">
                    <h4 class="config-card-title">"MiMo / Context"</h4>
                </div>
                <div class="config-card-body space-y-3">
                    <ToggleSwitch label="Context Compression" value=mimo_compression />
                    <ConfigRangeU64
                        label=move || format!("Compression Threshold: {}", mimo_compression_threshold.get())
                        value=mimo_compression_threshold min=2 max=20 min_hint="2" max_hint="20" accent="blue"
                    />
                    <ToggleSwitch label="Retire Prefix Messages" value=mimo_retire_prefix />
                    <ConfigRangeU64
                        label=move || format!("Keep Recent Turns: {}", mimo_keep_turns.get())
                        value=mimo_keep_turns min=1 max=20 min_hint="1" max_hint="20" accent="blue"
                    />
                    <ToggleSwitch label="Session Store" value=mimo_session />
                    <ConfigRangeU64
                        label=move || format!("Session TTL (s): {}", mimo_session_ttl.get())
                        value=mimo_session_ttl min=60 max=604800 min_hint="1m" max_hint="7d" accent="blue"
                    />
                    <ConfigRangeU64
                        label=move || format!("Session Max Msgs: {}", mimo_session_msgs.get())
                        value=mimo_session_msgs min=10 max=1000 min_hint="10" max_hint="1000" accent="blue"
                    />
                    <ToggleSwitch label="Upstream Request Gzip" value=upstream_gzip />
                    <ConfigRangeU64
                        label=move || format!("Gzip Min Bytes: {}", upstream_gzip_min.get())
                        value=upstream_gzip_min min=256 max=65536 min_hint="256" max_hint="64KB" accent="blue"
                    />
                </div>
            </div>

            // ── Cache group ──
            <div class="config-card glass-card">
                <div class="config-card-head">
                    <h4 class="config-card-title">"Cache"</h4>
                </div>
                <div class="config-card-body space-y-3">
                    <ToggleSwitch label="Prefix-aware L0" value=prefix_aware />
                    <ToggleSwitch label="Delta Cache" value=delta_cache />
                    <ToggleSwitch label="Streaming Body Forward" value=streaming_forward />
                    <ConfigRangeU64
                        label=move || format!("Passthrough Prefix (bytes): {}", passthrough_bytes.get())
                        value=passthrough_bytes min=128 max=8192 min_hint="128" max_hint="8KB" accent="blue"
                    />
                </div>
            </div>

            // ── Connection group ──
            <div class="config-card glass-card">
                <div class="config-card-head">
                    <h4 class="config-card-title">"Connection"</h4>
                </div>
                <div class="config-card-body space-y-3">
                    <ToggleSwitch label="Connection Prewarm" value=conn_prewarm />
                    <ToggleSwitch label="Affinity Prompt Cache Feedback" value=affinity_feedback />
                </div>
            </div>

            // ── Reserved group ──
            <div class="config-card glass-card">
                <div class="config-card-head">
                    <h4 class="config-card-title">"Reserved / P3"</h4>
                </div>
                <div class="config-card-body space-y-3">
                    <ToggleSwitch label="io_uring Backend" value=io_uring />
                    <ToggleSwitch label="WASM Filters" value=wasm />
                </div>
            </div>

            {move || {
                let msg = feedback.get();
                if !msg.is_empty() {
                    view! { <p class="text-xs text-blue-400">{msg}</p> }.into_any()
                } else {
                    ().into_any()
                }
            }}

            <button on:click=save class="btn btn-primary text-sm">{t.routing_save()}</button>
        </div>
    }
}

/// Simple toggle switch component for boolean signals.
#[component]
fn ToggleSwitch(label: &'static str, value: RwSignal<bool>) -> impl IntoView {
    view! {
        <label class="flex items-center gap-2 text-xs text-theme-secondary">
            <input
                type="checkbox"
                prop:checked=move || value.get()
                on:change=move |ev| value.set(event_target_checked(&ev))
            />
            {label}
        </label>
    }
}
