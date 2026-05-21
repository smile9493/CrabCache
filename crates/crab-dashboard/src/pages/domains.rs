use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::pages::overview::format_number;
use crate::types::{DomainDetailBundle, DomainMetricsBucket};

#[component]
pub fn DomainsListPage() -> impl IntoView {
    let t = use_translations();
    let domains: RwSignal<Option<Result<Vec<DomainMetricsBucket>, String>>> = RwSignal::new(None);

    let load = move || {
        leptos::task::spawn_local(async move {
            domains.set(Some(api::fetch_domains().await));
        });
    };
    load();

    view! {
        <div class="page-content space-y-6">
            <div>
                <h1 class="page-title">{t.domains_page_title()}</h1>
                <p class="page-desc">{t.domains_page_desc()}</p>
            </div>
            <DomainCompareChart domains=domains />
            <DomainOverviewTable domains=domains />
        </div>
    }
}

#[component]
pub fn DomainDetailPage() -> impl IntoView {
    let t = use_translations();
    let location = use_location();
    let domain_name = move || {
        location
            .pathname
            .get()
            .strip_prefix("/domains/")
            .unwrap_or("")
            .to_string()
    };
    let detail: RwSignal<Option<Result<DomainDetailBundle, String>>> = RwSignal::new(None);

    Effect::new(move |_| {
        let name = domain_name();
        if name.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            detail.set(Some(api::fetch_domain_detail(&name).await));
        });
    });

    view! {
        <div class="page-content space-y-6">
            <A href="/domains" attr:class="text-sm text-accent hover:underline">
                {t.domains_back()}
            </A>
            <h1 class="page-title font-mono">{domain_name}</h1>
            {move || match detail.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Ok(d)) => view! {
                    <DomainDetailBody bundle=d />
                }.into_any(),
                Some(Err(e)) => view! {
                    <p class="text-sm text-warning">{e}</p>
                }.into_any(),
            }}
        </div>
    }
}

#[component]
fn DomainDetailBody(bundle: DomainDetailBundle) -> impl IntoView {
    let t = use_translations();
    let b = bundle.bucket.clone();
    let tier = bundle.tier_deltas_5m;
    let total_tier = (tier.l0 + tier.l1 + tier.l2 + tier.miss).max(1) as f64;

    view! {
        <div class="bento-grid-2">
            <div class="glass-card">
                <h3 class="text-sm font-semibold text-theme mb-3">{t.domains_hit_rate()}</h3>
                <div class="text-3xl font-mono text-accent">{format!("{:.1}%", b.hit_ratio * 100.0)}</div>
                <p class="text-xs text-theme-muted mt-2">
                    "tokens: " {format_number(b.hit_tokens)} " hit / " {format_number(b.miss_tokens)} " miss"
                </p>
            </div>
            <div class="glass-card">
                <h3 class="text-sm font-semibold text-theme mb-3">{t.domains_cost_saved()}</h3>
                <div class="text-3xl font-mono text-accent">{format!("${:.2}", b.cost_saved_usd)}</div>
                <p class="text-xs text-theme-muted mt-2">"QPS 5m: " {format!("{:.2}", b.qps_5m)}</p>
            </div>
        </div>
        <div class="glass-card">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.domains_tier_breakdown()}</h3>
            <div class="flex gap-2 h-8 rounded overflow-hidden">
                <div class="bg-accent/80" style=format!("width: {}%", tier.l0 as f64 / total_tier * 100.0) title="L0"></div>
                <div class="bg-accent/50" style=format!("width: {}%", tier.l1 as f64 / total_tier * 100.0) title="L1"></div>
                <div class="bg-accent/30" style=format!("width: {}%", tier.l2 as f64 / total_tier * 100.0) title="L2"></div>
                <div class="bg-theme-muted/40" style=format!("width: {}%", tier.miss as f64 / total_tier * 100.0) title="miss"></div>
            </div>
            <p class="text-xs text-theme-muted mt-2">
                "L0=" {tier.l0} " L1=" {tier.l1} " L2=" {tier.l2} " miss=" {tier.miss}
            </p>
        </div>
        {(!bundle.history_7d.is_empty()).then(|| view! {
            <div class="glass-card">
                <h3 class="text-sm font-semibold text-theme mb-2">"7d token hit rate"</h3>
                <div class="text-xs font-mono text-theme-muted space-y-1 max-h-40 overflow-y-auto">
                    {bundle.history_7d.iter().rev().take(12).map(|p| view! {
                        <div>{p.timestamp.clone()} " " {format!("{:.0}%", p.hit_rate * 100.0)}</div>
                    }).collect::<Vec<_>>()}
                </div>
            </div>
        })}
        <DomainConsumerTable buckets=bundle.consumer_buckets.clone() />
        {bundle.policy.is_some().then(|| view! {
            <div class="glass-card text-sm text-theme-muted">
                "Policy: enabled, min hit rate configured"
            </div>
        })}
    }
}

#[component]
fn DomainCompareChart(
    domains: RwSignal<Option<Result<Vec<DomainMetricsBucket>, String>>>,
) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.domains_compare_title()}</h3>
            {move || match domains.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Ok(buckets)) if buckets.is_empty() => view! {
                    <p class="text-sm text-theme-muted">{t.overview_no_data()}</p>
                }.into_any(),
                Some(Ok(buckets)) => {
                    let max_rate = buckets.iter().map(|b| b.hit_ratio).fold(0.0f64, f64::max).max(0.01);
                    let rows: Vec<(String, f64)> = buckets
                        .iter()
                        .take(12)
                        .map(|b| (b.domain.clone(), b.hit_ratio))
                        .collect();
                    view! {
                        <div class="space-y-3">
                            {rows.into_iter().map(|(domain, ratio)| {
                                let w = ratio / max_rate * 100.0;
                                view! {
                                    <div>
                                        <div class="flex justify-between text-xs mb-1">
                                            <A href=format!("/domains/{domain}") attr:class="font-mono text-accent hover:underline">
                                                {domain.clone()}
                                            </A>
                                            <span class="font-mono tabular-nums">{format!("{:.1}%", ratio * 100.0)}</span>
                                        </div>
                                        <div class="h-3 bg-theme-muted/20 rounded">
                                            <div class="h-3 bg-accent rounded" style=format!("width: {w:.0}%")></div>
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }.into_any()
                }
                Some(Err(e)) => view! { <p class="text-sm text-warning">{e}</p> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn DomainOverviewTable(
    domains: RwSignal<Option<Result<Vec<DomainMetricsBucket>, String>>>,
) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.domains_table_title()}</h3>
            {move || match domains.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Ok(buckets)) if buckets.is_empty() => view! {
                    <p class="text-sm text-theme-muted">{t.overview_no_data()}</p>
                }.into_any(),
                Some(Ok(buckets)) => view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-theme-muted border-b border-theme">
                                    <th class="pb-2 pr-4">{t.domains_col_domain()}</th>
                                    <th class="pb-2 pr-4">{t.domains_col_hit_rate()}</th>
                                    <th class="pb-2 pr-4">{t.domains_col_cost()}</th>
                                    <th class="pb-2 pr-4">{t.domains_col_qps()}</th>
                                    <th class="pb-2">{t.domains_col_alert()}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {buckets.into_iter().map(|b| {
                                    let alert = b.alert.clone().unwrap_or_else(|| "—".to_string());
                                    let qps = if b.qps_5m > 0.0 {
                                        format!("{:.2}/s", b.qps_5m)
                                    } else {
                                        "—".to_string()
                                    };
                                    view! {
                                        <tr class="border-b border-theme/50">
                                            <td class="py-2 pr-4">
                                                <A href=format!("/domains/{}", b.domain) attr:class="font-mono text-accent hover:underline">
                                                    {b.domain}
                                                </A>
                                            </td>
                                            <td class="py-2 pr-4 font-mono tabular-nums text-accent">
                                                {format!("{:.1}%", b.hit_ratio * 100.0)}
                                            </td>
                                            <td class="py-2 pr-4 font-mono tabular-nums">{format!("${:.2}", b.cost_saved_usd)}</td>
                                            <td class="py-2 pr-4 font-mono tabular-nums">{qps}</td>
                                            <td class="py-2 font-mono text-warning">{alert}</td>
                                        </tr>
                                    }
                                }).collect::<Vec<_>>()}
                            </tbody>
                        </table>
                    </div>
                }.into_any(),
                Some(Err(e)) => view! { <p class="text-sm text-warning">{e}</p> }.into_any(),
            }}
        </div>
    }
}

#[component]
pub fn DomainOverviewTableInline(metrics: crate::types::MetricsSnapshot) -> impl IntoView {
    let t = use_translations();
    let buckets = metrics.domain_buckets.clone();
    view! {
        <div class="glass-card">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-sm font-semibold text-theme">{t.domains_table_title()}</h3>
                <A href="/domains" attr:class="text-xs text-accent hover:underline">{t.domains_view_all()}</A>
            </div>
            {if buckets.is_empty() {
                view! {
                    <p class="text-sm text-theme-muted">{t.overview_no_data()}</p>
                }.into_any()
            } else {
                view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="text-left text-xs text-theme-muted border-b border-theme">
                                    <th class="pb-2 pr-4">{t.domains_col_domain()}</th>
                                    <th class="pb-2 pr-4">{t.domains_col_hit_rate()}</th>
                                    <th class="pb-2 pr-4">{t.domains_col_cost()}</th>
                                    <th class="pb-2">{t.domains_col_qps()}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {buckets.into_iter().take(8).map(|b| {
                                    let qps = if b.qps_5m > 0.0 {
                                        format!("{:.2}/s", b.qps_5m)
                                    } else {
                                        "—".to_string()
                                    };
                                    view! {
                                        <tr class="border-b border-theme/50">
                                            <td class="py-2 pr-4 font-mono text-theme">{b.domain}</td>
                                            <td class="py-2 pr-4 font-mono tabular-nums text-accent">
                                                {format!("{:.1}%", b.hit_ratio * 100.0)}
                                            </td>
                                            <td class="py-2 pr-4 font-mono tabular-nums">{format!("${:.2}", b.cost_saved_usd)}</td>
                                            <td class="py-2 font-mono tabular-nums">{qps}</td>
                                        </tr>
                                    }
                                }).collect::<Vec<_>>()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn DomainConsumerTable(buckets: Vec<crate::types::ConsumerMetricsBucket>) -> impl IntoView {
    let t = use_translations();
    view! {
        <div class="glass-card">
            <h3 class="text-sm font-semibold text-theme mb-4">{t.overview_consumer_table_title()}</h3>
            {if buckets.is_empty() {
                view! { <p class="text-sm text-theme-muted">{t.overview_no_data()}</p> }.into_any()
            } else {
                view! {
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="text-left text-xs text-theme-muted border-b border-theme">
                                <th class="pb-2 pr-4">{t.overview_consumer_col()}</th>
                                <th class="pb-2 pr-4">"hit"</th>
                                <th class="pb-2 pr-4">"miss"</th>
                                <th class="pb-2">"ratio"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {buckets.into_iter().map(|b| view! {
                                <tr class="border-b border-theme/50">
                                    <td class="py-2 pr-4 font-mono">{b.consumer}</td>
                                    <td class="py-2 pr-4 font-mono tabular-nums">{format_number(b.hit_tokens)}</td>
                                    <td class="py-2 pr-4 font-mono tabular-nums">{format_number(b.miss_tokens)}</td>
                                    <td class="py-2 font-mono tabular-nums">{format!("{:.1}%", b.hit_ratio * 100.0)}</td>
                                </tr>
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                }.into_any()
            }}
        </div>
    }
}
