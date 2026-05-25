use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::TraceAnalysis;

#[component]
pub fn TracePage() -> impl IntoView {
    let t = use_translations();
    let analysis: RwSignal<Option<Result<TraceAnalysis, String>>> = RwSignal::new(None);

    let load_analysis = move || {
        leptos::task::spawn_local(async move {
            match api::fetch_trace_analysis(24).await {
                Ok(a) => analysis.set(Some(Ok(a))),
                Err(e) => analysis.set(Some(Err(e))),
            }
        });
    };

    load_analysis();

    view! {
        <div class="page-content space-y-6">
            <div class="flex items-center justify-between">
                <SectionHeader
                    title=t.trace_title()
                    description=t.trace_desc()
                />
                <p class="text-xs text-theme-muted max-w-md text-right hidden md:block">
                    {t.trace_hours_note()}
                </p>
                <button
                    on:click=move |_| load_analysis()
                    class="btn btn-secondary text-sm"
                >
                    {t.trace_refresh()}
                </button>
            </div>

            {move || match analysis.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">
                        {format!("{}: {}", use_translations().trace_load_error(), e)}
                    </div>
                }.into_any(),
                Some(Ok(data)) => {
                    let t = use_translations();
                    let cluster_dist = data.cluster_distribution.clone();
                    view! {
                        <div class="space-y-6">
                            <div class="bento-grid-4">
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_total_requests()}</div>
                                    <div class="text-2xl font-bold text-theme font-mono">
                                        {format!("{}", data.total_requests)}
                                    </div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_unique_requests()}</div>
                                    <div class="text-2xl font-bold text-theme font-mono">
                                        {format!("{}", data.unique_requests)}
                                    </div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_repeat_ratio()}</div>
                                    <div class="text-2xl font-bold text-accent font-mono">
                                        {format!("{:.1}%", data.repeat_ratio * 100.0)}
                                    </div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_estimated_hit_rate()}</div>
                                    <div class="text-2xl font-bold text-green-500 font-mono">
                                        {format!("{:.1}%", data.estimated_hit_rate * 100.0)}
                                    </div>
                                </div>
                            </div>

                            <div class="bento-grid-3">
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_semantic_ratio()}</div>
                                    <div class="text-lg font-semibold text-theme font-mono">
                                        {format!("{:.1}%", data.semantic_cluster_ratio * 100.0)}
                                    </div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_zipf_alpha()}</div>
                                    <div class="text-lg font-semibold text-theme font-mono">
                                        {format!("{:.2}", data.estimated_zipf_alpha)}
                                    </div>
                                </div>
                                <div class="bento-cell">
                                    <div class="text-xs text-theme-muted mb-1">{t.trace_cache_hit_ratio()}</div>
                                    <div class="text-lg font-semibold text-theme font-mono">
                                        {format!("{:.1}%", data.cache_hit_ratio * 100.0)}
                                    </div>
                                </div>
                            </div>

                            <div class="bento-grid-2">
                                <div class="bento-cell">
                                    <h3 class="text-sm font-semibold text-theme mb-3">{t.trace_avg_metrics()}</h3>
                                    <div class="space-y-2">
                                        <div class="flex justify-between">
                                            <span class="text-xs text-theme-muted">{t.trace_avg_latency()}</span>
                                            <span class="text-sm font-mono text-theme">
                                                {format!("{:.1}ms", data.avg_latency_ms)}
                                            </span>
                                        </div>
                                        <div class="flex justify-between">
                                            <span class="text-xs text-theme-muted">{t.trace_avg_tokens()}</span>
                                            <span class="text-sm font-mono text-theme">
                                                {format!("{:.0}", data.avg_prompt_tokens)}
                                            </span>
                                        </div>
                                    </div>
                                </div>

                                <div class="bento-cell">
                                    <h3 class="text-sm font-semibold text-theme mb-3">{t.trace_top_models()}</h3>
                                    <div class="space-y-2">
                                        {data.top_models.iter().map(|m| {
                                            view! {
                                                <div class="flex justify-between items-center">
                                                    <span class="text-xs text-theme">{m.model.clone()}</span>
                                                    <div class="flex items-center gap-2">
                                                        <span class="text-xs font-mono text-theme-muted">
                                                            {format!("{}", m.count)}
                                                        </span>
                                                        <span class="text-xs font-mono text-accent">
                                                            {format!("{:.1}%", m.percentage)}
                                                        </span>
                                                    </div>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            </div>

                            <div class="glass-card">
                                <h3 class="text-sm font-semibold text-theme mb-3">{t.trace_cluster_distribution()}</h3>
                                <div class="space-y-2">
                                    {cluster_dist.into_iter().map(|c| {
                                        view! {
                                            <div class="flex items-center gap-3">
                                                <span class="text-xs font-mono text-theme-muted w-20">
                                                    {format!("Cluster {}", c.cluster_id)}
                                                </span>
                                                <div class="flex-1 h-2 bg-theme-tertiary rounded-full overflow-hidden">
                                                    <div
                                                        class="h-full bg-accent rounded-full"
                                                        style=move || format!("width: {}%", c.percentage)
                                                    />
                                                </div>
                                                <span class="text-xs font-mono text-theme w-16 text-right">
                                                    {format!("{}", c.count)}
                                                </span>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            </div>

                            {data.deepseek_user_id.clone().map(|audit| {
                                let ok = audit.isolation_ok;
                                let conclusion = audit.conclusion.clone();
                                let top_projects = audit.top_project_ids.clone();
                                let breakdown = audit.audit_breakdown.clone();
                                view! {
                                    <div class="glass-card space-y-4">
                                        <div>
                                            <h3 class="text-sm font-semibold text-theme">
                                                {t.trace_deepseek_user_id_title()}
                                            </h3>
                                            <p class="text-xs text-theme-muted mt-1">
                                                {t.trace_deepseek_user_id_hint()}
                                            </p>
                                        </div>
                                        <div class=move || if ok {
                                            "text-sm font-medium text-accent"
                                        } else {
                                            "text-sm font-medium text-warning"
                                        }>
                                            {move || if ok {
                                                t.trace_isolation_ok()
                                            } else {
                                                t.trace_isolation_fail()
                                            }}
                                            <span class="text-theme-muted font-normal ml-2">
                                                {conclusion.clone()}
                                            </span>
                                        </div>
                                        <div class="bento-grid-4">
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">
                                                    {t.trace_deepseek_requests()}
                                                </div>
                                                <div class="text-xl font-bold font-mono text-theme">
                                                    {audit.deepseek_requests}
                                                </div>
                                            </div>
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">
                                                    {t.trace_upstream_user_id_ratio()}
                                                </div>
                                                <div class="text-xl font-bold font-mono text-theme">
                                                    {format!("{:.1}%", audit.upstream_user_id_ratio * 100.0)}
                                                </div>
                                            </div>
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">
                                                    {t.trace_missing_project_id()}
                                                </div>
                                                <div class="text-xl font-bold font-mono text-theme">
                                                    {audit.missing_project_id}
                                                </div>
                                            </div>
                                            <div class="bento-cell">
                                                <div class="text-xs text-theme-muted mb-1">
                                                    {t.trace_client_user_id_leaks()}
                                                </div>
                                                <div class="text-xl font-bold font-mono text-theme">
                                                    {audit.client_user_id_leaks}
                                                </div>
                                            </div>
                                        </div>
                                        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                            <div>
                                                <h4 class="text-xs font-semibold text-theme-muted mb-2">
                                                    {t.trace_audit_injected()}
                                                </h4>
                                                <div class="text-sm font-mono space-y-1">
                                                    <div class="flex justify-between">
                                                        <span>injected</span>
                                                        <span>{breakdown.injected}</span>
                                                    </div>
                                                    <div class="flex justify-between">
                                                        <span>absent</span>
                                                        <span>{breakdown.absent}</span>
                                                    </div>
                                                    <div class="flex justify-between">
                                                        <span>stripped_client</span>
                                                        <span>{breakdown.stripped_client}</span>
                                                    </div>
                                                    <div class="flex justify-between">
                                                        <span>mismatch</span>
                                                        <span>{breakdown.mismatch}</span>
                                                    </div>
                                                </div>
                                            </div>
                                            <div>
                                                <h4 class="text-xs font-semibold text-theme-muted mb-2">
                                                    {t.trace_top_project_ids()}
                                                </h4>
                                                <div class="space-y-1">
                                                    {top_projects.into_iter().map(|p| {
                                                        view! {
                                                            <div class="flex justify-between text-xs font-mono">
                                                                <span class="text-theme truncate pr-2">
                                                                    {p.project_id}
                                                                </span>
                                                                <span class="text-theme-muted">
                                                                    {format!("{} ({:.1}%)", p.count, p.percentage)}
                                                                </span>
                                                            </div>
                                                        }
                                                    }).collect_view()}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                }.into_any()
                            })}
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
