//! Clickable card wrapping a compact chart preview; opens detail modal on activate.

use std::sync::Arc;

use leptos::prelude::*;

use crate::components::chart_detail_modal::ChartDetailModal;
use crate::locale::use_translations;

#[component]
pub fn ChartPreviewCard<P, D>(
    title: String,
    #[prop(optional, into)] subtitle: Option<String>,
    open: RwSignal<bool>,
    preview: P,
    detail: D,
) -> impl IntoView
where
    P: Fn() -> AnyView + Send + Sync + 'static,
    D: Fn() -> AnyView + Send + Sync + 'static,
{
    let t = use_translations();
    let title_modal = title.clone();
    let title_aria = title.clone();
    let detail_arc: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(detail);

    let activate = move |_| open.set(true);

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" || ev.key() == " " {
            open.set(true);
            ev.prevent_default();
        }
    };

    view! {
        <div
            class="chart-preview-card"
            role="button"
            tabindex="0"
            aria-label=format!("{} — {}", title_aria, t.chart_click_to_expand())
            on:click=activate
            on:keydown=on_keydown
        >
            <div class="chart-preview-card-header">
                <h4 class="chart-preview-card-title">{title}</h4>
                <span class="chart-preview-card-hint">{t.chart_click_to_expand()}</span>
            </div>
            <div class="chart-preview-card-body" aria-hidden="true">
                {preview()}
            </div>
        </div>
        <ChartDetailModal
            open=open
            title=title_modal
            subtitle=subtitle
            detail=detail_arc
        />
    }
}
