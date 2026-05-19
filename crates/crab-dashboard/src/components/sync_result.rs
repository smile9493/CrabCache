use leptos::prelude::*;

use crate::locale::use_translations;
use crate::types::SyncResult;

#[component]
pub fn SyncResultCard(#[prop(into)] result: SyncResult) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card">
            <h4 class="text-sm font-semibold text-accent mb-4">{t.models_sync_result()}</h4>
            <div class="sync-grid">
                <div>
                    <div class="sync-stat-label">{t.models_added()}</div>
                    <div class="sync-stat-value text-accent">{result.added.len()}</div>
                </div>
                <div>
                    <div class="sync-stat-label">{t.models_removed()}</div>
                    <div class="sync-stat-value text-error">{result.removed.len()}</div>
                </div>
                <div>
                    <div class="sync-stat-label">{t.models_unchanged()}</div>
                    <div class="sync-stat-value text-warning">{result.unchanged}</div>
                </div>
                <div>
                    <div class="sync-stat-label">{t.models_col_status()}</div>
                    <div class="sync-stat-value text-theme">{result.total}</div>
                </div>
            </div>
        </div>
    }
}
