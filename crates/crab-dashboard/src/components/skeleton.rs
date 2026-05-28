use leptos::prelude::*;

/// A single skeleton placeholder block with shimmer animation.
#[component]
pub fn SkeletonBlock(
    #[prop(default = "100%")] width: &'static str,
    #[prop(default = "1rem")] height: &'static str,
    #[prop(default = false)] rounded: bool,
) -> impl IntoView {
    let cls = if rounded {
        "skeleton-block skeleton-block-rounded"
    } else {
        "skeleton-block"
    };
    view! {
        <div class=cls style=format!("width: {}; height: {}", width, height)></div>
    }
}

/// Skeleton placeholder mimicking a MetricCard layout (label + value + subtitle).
#[component]
pub fn SkeletonMetricCard() -> impl IntoView {
    view! {
        <div class="metric-card skeleton-card-inner">
            <div class="skeleton-block" style="width: 60%; height: 0.625rem"></div>
            <div class="skeleton-block" style="width: 80%; height: 1.75rem; margin-top: 0.5rem"></div>
            <div class="skeleton-block" style="width: 45%; height: 0.5rem; margin-top: 0.4rem"></div>
        </div>
    }
}

/// Full bento-grid skeleton matching the MetricsBento layout (1 hero + 4 standard cells).
#[component]
pub fn SkeletonBento() -> impl IntoView {
    view! {
        <div class="bento-grid">
            // Hero cell (QPS) — spans 2x2
            <div class="bento-cell-hero">
                <div class="metric-card h-full skeleton-card-inner">
                    <div class="skeleton-block" style="width: 40%; height: 0.625rem"></div>
                    <div class="skeleton-block" style="width: 70%; height: 2rem; margin-top: 0.75rem"></div>
                    <div class="skeleton-block" style="width: 55%; height: 0.5rem; margin-top: 0.5rem"></div>
                    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 0.75rem; margin-top: 1rem">
                        <div>
                            <div class="skeleton-block" style="width: 80%; height: 0.5rem"></div>
                            <div class="skeleton-block" style="width: 60%; height: 1.25rem; margin-top: 0.35rem"></div>
                        </div>
                        <div>
                            <div class="skeleton-block" style="width: 80%; height: 0.5rem"></div>
                            <div class="skeleton-block" style="width: 60%; height: 1.25rem; margin-top: 0.35rem"></div>
                        </div>
                    </div>
                </div>
            </div>
            // Standard cells
            <div class="bento-cell"><SkeletonMetricCard /></div>
            <div class="bento-cell"><SkeletonMetricCard /></div>
            <div class="bento-cell"><SkeletonMetricCard /></div>
            <div class="bento-cell"><SkeletonMetricCard /></div>
        </div>
    }
}

/// Generic skeleton grid for secondary sections (2/3/4 columns).
#[component]
pub fn SkeletonGrid(
    #[prop(default = 3)] cols: u32,
    #[prop(default = 2)] rows: u32,
    #[prop(default = "120px")] card_height: &'static str,
) -> impl IntoView {
    let grid_class = match cols {
        2 => "bento-grid-2",
        3 => "bento-grid-3",
        4 => "bento-grid",
        _ => "bento-grid-3",
    };
    let total = (cols * rows) as usize;
    view! {
        <div class=grid_class>
            {(0..total).map(|_| view! {
                <div class="bento-cell">
                    <div class="glass-card skeleton-card-inner" style=format!("min-height: {}", card_height)>
                        <div class="skeleton-block" style="width: 50%; height: 0.625rem"></div>
                        <div class="skeleton-block" style="width: 75%; height: 1rem; margin-top: 0.75rem"></div>
                        <div class="skeleton-block" style="width: 40%; height: 0.5rem; margin-top: 0.5rem"></div>
                    </div>
                </div>
            }).collect_view()}
        </div>
    }
}

/// Skeleton for a chart area (timeseries or latency chart).
#[component]
pub fn SkeletonChart(#[prop(default = "240px")] height: &'static str) -> impl IntoView {
    view! {
        <div class="glass-card skeleton-card-inner">
            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem">
                <div class="skeleton-block" style="width: 30%; height: 0.875rem"></div>
                <div style="display: flex; gap: 0.5rem">
                    <div class="skeleton-block skeleton-block-rounded" style="width: 3rem; height: 1.5rem"></div>
                    <div class="skeleton-block skeleton-block-rounded" style="width: 3rem; height: 1.5rem"></div>
                    <div class="skeleton-block skeleton-block-rounded" style="width: 3rem; height: 1.5rem"></div>
                </div>
            </div>
            <div class="skeleton-block" style=format!("width: 100%; height: {}; border-radius: var(--radius-sm)", height)></div>
        </div>
    }
}

/// Full overview page skeleton (Status tab view).
#[component]
pub fn SkeletonOverview() -> impl IntoView {
    view! {
        <div class="space-y-6">
            // Health strip skeleton
            <div class="glass-card skeleton-card-inner" style="height: 2.5rem; display: flex; align-items: center; gap: 0.75rem">
                <div class="skeleton-block skeleton-block-rounded" style="width: 0.5rem; height: 0.5rem"></div>
                <div class="skeleton-block" style="width: 20%; height: 0.75rem"></div>
            </div>
            // Metrics bento
            <SkeletonBento />
            // Ops row skeleton
            <div style="display: grid; grid-template-columns: repeat(4, 1fr); gap: 1rem">
                {(0..4).map(|_| view! {
                    <div class="glass-card skeleton-card-inner" style="min-height: 80px">
                        <div class="skeleton-block" style="width: 50%; height: 0.5rem"></div>
                        <div class="skeleton-block" style="width: 70%; height: 1.25rem; margin-top: 0.5rem"></div>
                    </div>
                }).collect_view()}
            </div>
            // Chart skeleton
            <SkeletonChart />
            // Secondary grid
            <SkeletonGrid cols=3 rows=1 card_height="140px" />
        </div>
    }
}

/// Live page skeleton (summary cards + chart).
#[component]
pub fn SkeletonLive() -> impl IntoView {
    view! {
        <div class="space-y-6">
            // Summary cards row
            <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 1rem">
                {(0..6).map(|_| view! {
                    <SkeletonMetricCard />
                }).collect_view()}
            </div>
            // Chart skeleton
            <SkeletonChart height="200px" />
        </div>
    }
}

/// Skeleton table with header row and N data rows.
#[component]
pub fn SkeletonTable(
    #[prop(default = 5)] rows: usize,
    #[prop(default = 4)] cols: usize,
) -> impl IntoView {
    let grid_style = format!("display: grid; grid-template-columns: repeat({}, 1fr); gap: 1rem; padding: 0.625rem 1rem;", cols);
    view! {
        <div class="glass-card skeleton-card-inner" style="overflow: hidden; padding: 0">
            <div style=format!("{} border-bottom: 1px solid var(--cc-border-light); background: var(--cc-bg-elevated)", grid_style)>
                {(0..cols).map(|_| view! {
                    <div class="skeleton-block" style="width: 70%; height: 0.5rem"></div>
                }).collect_view()}
            </div>
            {(0..rows).map(|_| view! {
                <div style=format!("{} border-bottom: 1px solid var(--cc-border-light)", grid_style)>
                    {(0..cols).map(|c| {
                        let w = if c == 0 { "85%" } else if c == cols - 1 { "40%" } else { "65%" };
                        view! { <div class="skeleton-block" style=format!("width: {}; height: 0.75rem", w)></div> }
                    }).collect_view()}
                </div>
            }).collect_view()}
        </div>
    }
}

/// Skeleton for a single config/form card.
#[component]
pub fn SkeletonFormCard() -> impl IntoView {
    view! {
        <div class="glass-card skeleton-card-inner" style="min-height: 200px">
            <div class="skeleton-block" style="width: 35%; height: 0.875rem"></div>
            <div style="display: flex; flex-direction: column; gap: 1rem; margin-top: 1.25rem">
                <div>
                    <div class="skeleton-block" style="width: 25%; height: 0.5rem; margin-bottom: 0.375rem"></div>
                    <div class="skeleton-block" style="width: 100%; height: 2rem; border-radius: var(--radius-sm)"></div>
                </div>
                <div>
                    <div class="skeleton-block" style="width: 20%; height: 0.5rem; margin-bottom: 0.375rem"></div>
                    <div class="skeleton-block" style="width: 100%; height: 2rem; border-radius: var(--radius-sm)"></div>
                </div>
                <div>
                    <div class="skeleton-block" style="width: 30%; height: 0.5rem; margin-bottom: 0.375rem"></div>
                    <div class="skeleton-block" style="width: 60%; height: 2rem; border-radius: var(--radius-sm)"></div>
                </div>
            </div>
        </div>
    }
}
