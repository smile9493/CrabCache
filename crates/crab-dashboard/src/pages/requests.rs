use leptos::prelude::*;

use crate::components::ui::*;
use crate::locale::use_translations;
use crate::pages::capture::CapturePage;
use crate::pages::composition::CompositionPage;
use crate::pages::logs::LogsPage;
use crate::pages::logs_manage::LogsManagePage;

#[component]
pub fn RequestsPage() -> impl IntoView {
    let t = use_translations();
    let active_tab: RwSignal<usize> = RwSignal::new(0);
    let tab_labels = vec![
        t.tab_logs().to_string(),
        t.tab_insights().to_string(),
        t.tab_capture().to_string(),
        t.tab_log_manage().to_string(),
    ];

    init_tab_from_query(
        active_tab,
        &[
            ("logs", 0),
            ("insights", 1),
            ("composition", 1),
            ("capture", 2),
            ("manage", 3),
        ],
    );

    view! {
        <div class="page-content space-y-4">
            <TabBar tabs=tab_labels active=active_tab />
            {move || match active_tab.get() {
                0 => view! { <LogsPage /> }.into_any(),
                1 => view! { <CompositionPage /> }.into_any(),
                2 => view! { <CapturePage /> }.into_any(),
                _ => view! { <LogsManagePage /> }.into_any(),
            }}
        </div>
    }
}
