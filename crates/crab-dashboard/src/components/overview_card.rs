//! Clickable overview metric card; opens center detail modal on activate.

use std::sync::Arc;

use leptos::prelude::*;

use crate::components::card_detail_modal::CardDetailModal;
use crate::locale::use_translations;

#[component]
pub fn OverviewMetricCard<P, D>(
    label: String,
    #[prop(into)] headline: MaybeSignal<String>,
    #[prop(optional)] subtitle: Option<String>,
    open: RwSignal<bool>,
    #[prop(optional)] on_open: Option<Callback<()>>,
    preview: P,
    detail: D,
) -> impl IntoView
where
    P: Fn() -> AnyView + Send + Sync + 'static,
    D: Fn() -> AnyView + Send + Sync + 'static,
{
    let t = use_translations();
    let title_modal = label.clone();
    let label_aria = label.clone();
    let detail_arc: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(detail);

    let activate = move |_| {
        if let Some(ref cb) = on_open {
            cb.run(());
        }
        open.set(true);
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" || ev.key() == " " {
            if let Some(ref cb) = on_open {
                cb.run(());
            }
            open.set(true);
            ev.prevent_default();
        }
    };

    view! {
        <div
            class="overview-metric-card"
            role="button"
            tabindex="0"
            aria-label=format!("{} — {}", label_aria, t.overview_card_click_detail())
            on:click=activate
            on:keydown=on_keydown
        >
            <div class="overview-metric-card-top">
                <span class="overview-metric-card-label">{label}</span>
            </div>
            <div class="overview-metric-card-value">{move || headline.get()}</div>
            <div class="overview-metric-card-preview" aria-hidden="true">
                {preview()}
            </div>
            <span class="overview-metric-card-hint">{t.overview_card_click_detail()}</span>
        </div>
        <CardDetailModal
            open=open
            title=title_modal
            subtitle=subtitle
            detail=detail_arc
        />
    }
}
