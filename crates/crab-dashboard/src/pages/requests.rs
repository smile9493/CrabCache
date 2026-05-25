use leptos::prelude::*;

use crate::components::ui::*;
use crate::locale::use_translations;
use crate::pages::composition::CompositionPage;
use crate::pages::logs::LogsPage;

#[component]
pub fn RequestsPage() -> impl IntoView {
    let t = use_translations();
    let active_tab: RwSignal<usize> = RwSignal::new(0);
    let tab_labels = vec![t.tab_logs().to_string(), t.tab_insights().to_string()];

    init_tab_from_query(
        active_tab,
        &[("logs", 0), ("insights", 1), ("composition", 1)],
    );

    view! {
        <div class="page-content space-y-4">
            <TabBar tabs=tab_labels active=active_tab />
            {move || match active_tab.get() {
                0 => view! { <LogsPage /> }.into_any(),
                _ => view! { <CompositionPage /> }.into_any(),
            }}
        </div>
    }
}
