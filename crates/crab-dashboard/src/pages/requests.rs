use leptos::prelude::*;

use crate::components::ui::*;
use crate::pages::composition::CompositionPage;
use crate::pages::logs::LogsPage;

#[component]
pub fn RequestsPage() -> impl IntoView {
    let active_tab: RwSignal<usize> = RwSignal::new(0);

    view! {
        <div class="page-content space-y-4">
            <TabBar
                tabs=vec!["Logs", "Insights"]
                active=active_tab
            />
            <div class=move || if active_tab.get() == 0 { "" } else { "hidden" }>
                <LogsPage />
            </div>
            <div class=move || if active_tab.get() == 1 { "" } else { "hidden" }>
                <CompositionPage />
            </div>
        </div>
    }
}
