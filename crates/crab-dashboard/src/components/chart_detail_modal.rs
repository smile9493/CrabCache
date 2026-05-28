//! Full-screen modal for enlarged interactive charts.

use std::sync::Arc;

use leptos::prelude::*;

use crate::locale::use_translations;

#[component]
fn ChartDetailModalBody(
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
            class="chart-detail-overlay"
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
                class="chart-detail-modal glass-card"
                on:click=move |ev| { ev.stop_propagation(); }
            >
                <div class="chart-detail-header">
                    <div class="min-w-0">
                        <h3 class="chart-detail-title">{title}</h3>
                        {subtitle.map(|s| view! { <p class="chart-detail-subtitle">{s}</p> })}
                    </div>
                    <button
                        type="button"
                        class="btn btn-secondary text-xs shrink-0"
                        on:click=move |_| open.set(false)
                    >
                        {close_label}
                    </button>
                </div>
                <div class="chart-detail-body">
                    {detail_render()}
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn ChartDetailModal(
    open: RwSignal<bool>,
    title: String,
    subtitle: Option<String>,
    detail: Arc<dyn Fn() -> AnyView + Send + Sync>,
) -> impl IntoView {
    view! {
        <div class="chart-detail-host" class:chart-detail-host--hidden=move || !open.get()>
            <ChartDetailModalBody open title subtitle detail />
        </div>
    }
}