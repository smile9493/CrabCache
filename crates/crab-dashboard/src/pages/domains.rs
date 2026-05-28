use leptos::prelude::*;

use crate::api;
use crate::locale::use_translations;
use crate::pages::overview::format_number;
use crate::types::{DomainDetailBundle, DomainPolicy};

#[component]
fn DomainDetailBody(bundle: DomainDetailBundle, policy_save_tick: RwSignal<u32>) -> impl IntoView {
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
        <DomainPolicyEditor
            domain=bundle.domain.clone()
            initial=bundle.policy.clone()
            after_save=policy_save_tick
        />
    }
}

#[component]
fn DomainPolicyEditor(
    domain: String,
    initial: Option<DomainPolicy>,
    #[prop(optional)] feedback: Option<RwSignal<String>>,
    #[prop(optional)] after_save: Option<RwSignal<u32>>,
) -> impl IntoView {
    let t = use_translations();
    let local_feedback = RwSignal::new(String::new());
    let feedback = feedback.unwrap_or(local_feedback);
    let profile_ids: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let saving = RwSignal::new(false);

    let init = initial.clone().unwrap_or(DomainPolicy {
        domain: domain.clone(),
        monthly_token_budget: 0,
        monthly_cost_budget_usd: 0.0,
        min_hit_rate: 0.0,
        enabled: true,
        pipeline: None,
        upstream_profile: None,
    });

    let token_budget = RwSignal::new(init.monthly_token_budget);
    let cost_budget = RwSignal::new(init.monthly_cost_budget_usd);
    let min_hit = RwSignal::new(init.min_hit_rate);
    let enabled = RwSignal::new(init.enabled);
    let pipeline = RwSignal::new(match init.pipeline {
        Some(p) if !p.is_empty() => p,
        _ => "auto".to_string(),
    });
    let upstream_profile = RwSignal::new(init.upstream_profile.unwrap_or_default());

    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Ok(cfg) = api::fetch_pipeline_runtime().await {
                profile_ids.set(cfg.profiles.into_iter().map(|p| p.id).collect());
            }
        });
    });

    let domain_label = domain.clone();
    let can_delete = initial.is_some();
    let on_save = {
        let domain = domain.clone();
        let saved_msg = t.domains_policy_saved().to_string();
        let after_save = after_save;
        move |_| {
            saving.set(true);
            let policy = DomainPolicy {
                domain: domain.clone(),
                monthly_token_budget: token_budget.get(),
                monthly_cost_budget_usd: cost_budget.get(),
                min_hit_rate: min_hit.get(),
                enabled: enabled.get(),
                pipeline: {
                    let p = pipeline.get();
                    if p.is_empty() || p == "auto" {
                        None
                    } else {
                        Some(p)
                    }
                },
                upstream_profile: {
                    let p = upstream_profile.get();
                    if p.is_empty() { None } else { Some(p) }
                },
            };
            let saved_msg = saved_msg.clone();
            leptos::task::spawn_local(async move {
                match api::upsert_domain_policy(policy).await {
                    Ok(_) => {
                        feedback.set(saved_msg);
                        if let Some(n) = after_save {
                            n.update(|v| *v += 1);
                        }
                    }
                    Err(e) => feedback.set(e),
                }
                saving.set(false);
            });
        }
    };

    view! {
        <div class="glass-card space-y-4">
            <h3 class="text-sm font-semibold text-theme">
                {t.domains_policy_edit_title()} " — " <span class="font-mono">{domain_label}</span>
            </h3>
            {move || (!feedback.get().is_empty() && after_save.is_none()).then(|| view! {
                <p class="text-xs text-accent">{feedback.get()}</p>
            })}
            <div class="grid grid-cols-2 gap-4 max-w-2xl">
                <div>
                    <label class="block text-xs text-theme-muted mb-1">{t.domains_policy_budget_tokens()}</label>
                    <input
                        type="number"
                        class="input w-full"
                        prop:value=move || token_budget.get().to_string()
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse() {
                                token_budget.set(v);
                            }
                        }
                    />
                </div>
                <div>
                    <label class="block text-xs text-theme-muted mb-1">{t.domains_policy_budget_cost()}</label>
                    <input
                        type="number"
                        step="0.01"
                        class="input w-full"
                        prop:value=move || cost_budget.get().to_string()
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse() {
                                cost_budget.set(v);
                            }
                        }
                    />
                </div>
                <div>
                    <label class="block text-xs text-theme-muted mb-1">{t.domains_policy_min_hit_rate()}</label>
                    <input
                        type="number"
                        step="0.01"
                        min="0"
                        max="1"
                        class="input w-full"
                        prop:value=move || min_hit.get().to_string()
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse() {
                                min_hit.set(v);
                            }
                        }
                    />
                </div>
                <div class="flex items-end">
                    <label class="flex items-center gap-2 text-xs text-theme-muted">
                        <input
                            type="checkbox"
                            prop:checked=move || enabled.get()
                            on:change=move |ev| enabled.set(event_target_checked(&ev))
                        />
                        {t.domains_policy_enabled()}
                    </label>
                </div>
                <div>
                    <label class="block text-xs text-theme-muted mb-1">{t.keys_pipeline_label()}</label>
                    <select
                        class="input w-full"
                        prop:value=move || pipeline.get()
                        on:change=move |ev| pipeline.set(event_target_value(&ev))
                    >
                        <option value="auto">{t.keys_override_auto()}</option>
                        <option value="cursor_deepseek_v4">"cursor_deepseek_v4"</option>
                        <option value="deepseek_light">"deepseek_light"</option>
                        <option value="mimo_relay">"mimo_relay"</option>
                        <option value="mimo_token_plan_relay">"mimo_token_plan_relay"</option>
                        <option value="mimo_payg_relay">"mimo_payg_relay"</option>
                        <option value="generic_relay">"generic_relay"</option>
                    </select>
                </div>
                <div>
                    <label class="block text-xs text-theme-muted mb-1">{t.keys_upstream_profile_label()}</label>
                    <select
                        class="input w-full"
                        prop:value=move || upstream_profile.get()
                        on:change=move |ev| upstream_profile.set(event_target_value(&ev))
                    >
                        <option value="">{t.keys_override_auto()}</option>
                        {move || profile_ids.get().into_iter().map(|id| {
                            view! { <option value=id.clone()>{id.clone()}</option> }
                        }).collect_view()}
                    </select>
                </div>
            </div>
            <div class="flex gap-2">
                <button
                    class="btn btn-primary text-sm"
                    disabled=move || saving.get()
                    on:click=on_save
                >
                    {move || if saving.get() { "…" } else { t.domains_policy_save() }}
                </button>
                {move || can_delete.then(|| {
                    let domain_del = domain.clone();
                    let deleted_msg = t.domains_policy_deleted().to_string();
                    let after_save = after_save;
                    view! {
                        <button
                            class="btn btn-secondary text-sm"
                            disabled=move || saving.get()
                            on:click=move |_| {
                                saving.set(true);
                                let deleted_msg = deleted_msg.clone();
                                let domain_del = domain_del.clone();
                                leptos::task::spawn_local(async move {
                                    match api::delete_domain_policy(&domain_del).await {
                                        Ok(_) => {
                                            feedback.set(deleted_msg);
                                            if let Some(n) = after_save {
                                                n.update(|v| *v += 1);
                                            }
                                        }
                                        Err(e) => feedback.set(e),
                                    }
                                    saving.set(false);
                                });
                            }
                        >
                            {t.domains_policy_delete()}
                        </button>
                    }
                })}
            </div>
        </div>
    }
}

#[component]
pub fn DomainOverviewTableInline(
    metrics: crate::types::MetricsSnapshot,
    #[prop(optional)] on_domain_click: Option<Callback<String>>,
) -> impl IntoView {
    let t = use_translations();
    let buckets = metrics.domain_buckets.clone();
    view! {
        <div class="glass-card">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-sm font-semibold text-theme">{t.domains_table_title()}</h3>
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
                                    let domain_name = b.domain.clone();
                                    let qps = if b.qps_5m > 0.0 {
                                        format!("{:.2}/s", b.qps_5m)
                                    } else {
                                        "—".to_string()
                                    };
                                    let has_click = on_domain_click.is_some();
                                    let domain_for_click = domain_name.clone();
                                    let cb = on_domain_click;
                                    view! {
                                        <tr
                                            class=if has_click { "border-b border-theme/50 cursor-pointer hover:bg-theme-hover" } else { "border-b border-theme/50" }
                                            on:click=move |_| {
                                                if let Some(ref cb) = cb {
                                                    cb.run(domain_for_click.clone());
                                                }
                                            }
                                        >
                                            <td class="py-2 pr-4 font-mono text-theme">{domain_name}</td>
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

/// Slide-in drawer for domain detail, used in Overview page.
#[component]
pub fn DomainDetailDrawer(domain: RwSignal<Option<String>>) -> impl IntoView {
    let t = use_translations();
    let detail: RwSignal<Option<Result<DomainDetailBundle, String>>> = RwSignal::new(None);
    let policy_save_tick = RwSignal::new(0u32);
    let visible = move || domain.get().is_some();

    let load_detail = move |name: String| {
        leptos::task::spawn_local(async move {
            detail.set(Some(api::fetch_domain_detail(&name).await));
        });
    };

    Effect::new(move |_| {
        let _ = policy_save_tick.get();
        if let Some(name) = domain.get() {
            load_detail(name);
        }
    });

    let close = move |_| domain.set(None);

    view! {
        <Show when=visible>
            <div class="fixed inset-0 z-50 flex justify-end">
                <div class="absolute inset-0 bg-black/30" on:click=close></div>
                <div class="relative w-full max-w-2xl bg-[var(--bg-primary)] shadow-xl overflow-y-auto">
                    <div class="sticky top-0 flex items-center justify-between p-4 border-b border-[var(--border-color)] bg-[var(--bg-primary)] z-10">
                        <h2 class="text-sm font-semibold font-mono text-[var(--text-primary)]">
                            {move || domain.get().unwrap_or_default()}
                        </h2>
                        <button class="text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] px-2 py-1" on:click=close>
                            {t.domains_back()}
                        </button>
                    </div>

                    <div class="p-4 space-y-4">
                        {move || match detail.get() {
                            None => view! { <crate::components::skeleton::SkeletonTable rows=5 cols=4 /> }.into_any(),
                            Some(Ok(bundle)) => view! {
                                <DomainDetailBody bundle=bundle policy_save_tick=policy_save_tick />
                            }.into_any(),
                            Some(Err(e)) => view! {
                                <p class="text-sm text-warning">{e}</p>
                            }.into_any(),
                        }}
                    </div>
                </div>
            </div>
        </Show>
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
