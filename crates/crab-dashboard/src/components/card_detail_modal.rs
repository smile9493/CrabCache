//! Center modal for overview metric drill-down detail.

use std::sync::Arc;

use leptos::prelude::*;

use crate::locale::use_translations;

#[component]
fn CardDetailModalBody(
    open: RwSignal<bool>,
    title: String,
    subtitle: Option<String>,
    detail: Arc<dyn Fn() -> AnyView + Send + Sync>,
) -> impl IntoView {
    let close_label = use_translations().chart_detail_close();
    let detail_render = detail.clone();
    let aria_label = title.clone();

    view! {
        <div
            class="card-detail-overlay"
            role="dialog"
            aria-modal="true"
            aria-label=aria_label
            aria-hidden=move || (!open.get()).to_string()
            on:click=move |_| open.set(false)
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if ev.key() == "Escape" {
                    open.set(false);
                    ev.prevent_default();
                }
            }
            tabindex="-1"
        >
            <div
                class="card-detail-modal glass-card"
                on:click=move |ev| { ev.stop_propagation(); }
            >
                <div class="card-detail-header">
                    <div class="min-w-0">
                        <h3 class="card-detail-title">{title}</h3>
                        {subtitle.map(|s| view! { <p class="card-detail-subtitle">{s}</p> })}
                    </div>
                    <button
                        type="button"
                        class="btn btn-secondary text-xs shrink-0"
                        on:click=move |_| open.set(false)
                    >
                        {close_label}
                    </button>
                </div>
                <div class="card-detail-body">
                    {detail_render()}
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn CardDetailModal(
    open: RwSignal<bool>,
    title: String,
    subtitle: Option<String>,
    detail: Arc<dyn Fn() -> AnyView + Send + Sync>,
) -> impl IntoView {
    view! {
        <div class="card-detail-host" class:card-detail-host--hidden=move || !open.get()>
            <CardDetailModalBody open title subtitle detail />
        </div>
    }
}
