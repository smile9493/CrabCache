use leptos::prelude::*;

use crate::api;
use crate::components::ui::{Alert, SectionHeader, Spinner};
use crate::locale::use_translations;
use crate::types::{
    CacheOpsView, FingerprintConfigBody, InvalidateCacheBody, StreamCacheToggle,
};

#[component]
pub fn CacheOpsPage() -> impl IntoView {
    let t = use_translations();
    let ops: RwSignal<Option<Result<CacheOpsView, String>>> = RwSignal::new(None);
    let message: RwSignal<String> = RwSignal::new(String::new());
    let show_confirm_all = RwSignal::new(false);

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
        <div class="page-content space-y-6">
            <SectionHeader title=t.cache_ops_title() description=t.cache_ops_desc() />
            <Alert variant="info" message=message.into() />

            {move || match ops.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(view)) => {
                    let fp_version = RwSignal::new(view.fingerprint_version.to_string());
                    let fp_normalize = RwSignal::new(view.fingerprint_normalize);
                    let stream_enabled = RwSignal::new(view.stream_cache_enabled);
                    let scope = RwSignal::new(String::new());
                    let last = view.last_invalidate.clone();

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
                                                Ok(_) => {
                                                    message.set(t.routing_saved().to_string());
                                                    reload();
                                                }
                                                Err(e) => { message.set(e); }
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
                                                        message.set(t.routing_saved().to_string());
                                                        reload();
                                                    }
                                                    Err(e) => { message.set(e); }
                                                }
                                            });
                                        }
                                    />
                                    {t.cache_ops_stream_cache()}
                                </label>
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
                        </div>

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
                        </div>

                        {move || show_confirm_all.get().then(|| view! {
                            <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
                                <div class="glass-card max-w-md w-full space-y-4">
                                    <h4 class="text-sm font-semibold text-theme">{t.cache_ops_confirm_all_title()}</h4>
                                    <p class="text-xs text-theme-muted">{t.cache_ops_confirm_all_body()}</p>
                                    <div class="flex gap-2 justify-end">
                                        <button
                                            class="btn btn-secondary text-xs"
                                            on:click=move |_| show_confirm_all.set(false)
                                        >
                                            {t.cache_ops_confirm_cancel()}
                                        </button>
                                        <button
                                            class="btn btn-primary text-xs"
                                            on:click=move |_| {
                                                show_confirm_all.set(false);
                                                leptos::task::spawn_local(async move {
                                                    match api::invalidate_cache(&InvalidateCacheBody {
                                                        scope: "all".to_string(),
                                                    }).await {
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
                    }.into_any()
                }
            }}
        </div>
    }
}
