//! Analytics tab content for the Overview page.
//!
//! Extracted from `overview.rs` to keep the main page component lean and
//! allow Leptos to defer DOM/JS initialization until the tab is activated.

use crate::types::{
    MetricsSnapshot, OverviewOpsMetrics, OverviewSuggestion, PrefixCacheMetricsSnapshot,
    SemanticConfig, TimeSeriesPoint,
};
use leptos::prelude::*;

#[component]
pub fn OverviewAnalytics(
    suggestions_memo: Memo<Option<Vec<OverviewSuggestion>>>,
    ts_points: RwSignal<Vec<TimeSeriesPoint>>,
    ts_window: RwSignal<String>,
    prefix_memo: Memo<Option<PrefixCacheMetricsSnapshot>>,
    metrics_memo: Memo<Option<MetricsSnapshot>>,
    ops_memo: Memo<Option<OverviewOpsMetrics>>,
    semantic_memo: Memo<Option<SemanticConfig>>,
    selected_domain: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="space-y-6">
            {move || suggestions_memo.get().map(|s| {
                view! {
                    <super::overview::TimeSeriesChart
                        points=ts_points
                        selected_view=ts_window
                        suggestions=s
                    />
                }
            })}
            {move || prefix_memo.get().zip(metrics_memo.get()).map(|(pref, _m)| view! {
                <super::overview::PrefixCacheCard prefix=pref.clone() />
            })}
            {move || metrics_memo.get().zip(prefix_memo.get()).map(|(m, pref)| view! {
                <super::overview::TokenStats metrics=m.clone() prefix=pref.clone() />
            })}
            {move || metrics_memo.get().zip(ops_memo.get()).map(|(m, ops)| view! {
                <div class="bento-grid-2">
                    <super::overview::CoalescingCard metrics=m.clone() ops=ops.clone() />
                    <super::overview::SemanticCacheCard metrics=m.clone() semantic=semantic_memo.get().unwrap_or(SemanticConfig { enabled: false, similarity_threshold: 0.9 }) />
                </div>
            })}
            {move || metrics_memo.get().map(|m| view! {
                <super::overview::ConsumerHitTable metrics=m.clone() />
            })}
            {move || metrics_memo.get().map(|m| {
                let cb = Callback::new(move |domain: String| {
                    selected_domain.set(Some(domain));
                });
                view! {
                    <super::domains::DomainOverviewTableInline metrics=m.clone() on_domain_click=cb />
                }
            })}
            <super::domains::DomainDetailDrawer domain=selected_domain />
            {move || metrics_memo.get().zip(ops_memo.get()).map(|(m, ops)| view! {
                <div class="bento-grid-3">
                    <div class="bento-cell">
                        <super::overview::CacheHitSection metrics=m.clone() />
                    </div>
                    <div class="bento-cell">
                        <super::overview::CostSavingsSection ops=ops.clone() />
                    </div>
                    <div class="bento-cell">
                        <super::overview::LatencySection metrics=m.clone() />
                    </div>
                </div>
            })}
            {move || ops_memo.get().map(|ops| view! {
                <div class="bento-grid-2">
                    <super::overview::UpstreamKeyStrip ops=ops.clone() />
                    <super::overview::PrefixHealthCard ops=ops.clone() />
                </div>
            })}
        </div>
    }
}
