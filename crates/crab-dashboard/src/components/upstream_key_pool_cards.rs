use crate::locale::use_translations;
use crate::types::{KeyQuotaInfo, UpstreamKeyView};
use leptos::prelude::*;

fn short_account_id(account_id: &str) -> String {
    let t = account_id.trim();
    if t.is_empty() || t == "default" {
        return "default".to_string();
    }
    if t.len() <= 12 {
        return t.to_string();
    }
    format!("{}…{}", &t[..8], &t[t.len() - 4..])
}

fn plan_badge_class(plan: &str) -> &'static str {
    match plan.to_ascii_lowercase().as_str() {
        "plus" | "pro" | "team" => "badge badge-accent",
        "free" => "badge",
        _ => "badge badge-muted",
    }
}

#[component]
fn CodexRateQuotaBar(
    label: &'static str,
    used_percent: Option<f64>,
    reset_secs: Option<u64>,
) -> impl IntoView {
    let used = used_percent.unwrap_or(0.0).clamp(0.0, 100.0);
    let remaining = (100.0 - used).clamp(0.0, 100.0);
    let bar_color = if remaining >= 30.0 {
        "upstream-quota-fill-high"
    } else if remaining >= 10.0 {
        "upstream-quota-fill-medium"
    } else {
        "upstream-quota-fill-low"
    };
    let reset_label = reset_secs.map(|s| format!("{s}s"));

    view! {
        <div class="upstream-quota-row">
            <div class="flex items-center justify-between gap-2 mb-1">
                <span class="text-xs text-theme-muted">{label}</span>
                <span class="text-xs font-mono">{format!("{remaining:.0}%")}</span>
            </div>
            <div class="upstream-quota-track">
                <div class=format!("upstream-quota-fill {bar_color}") style=format!("width: {remaining:.1}%")></div>
            </div>
            {reset_label.map(|r| view! {
                <div class="text-[10px] text-theme-muted mt-0.5 font-mono">{format!("reset {r}")}</div>
            })}
        </div>
    }
}

#[component]
fn KeyQuotaSection(quota: KeyQuotaInfo) -> impl IntoView {
    let t = use_translations();
    if quota.primary_used_percent.is_some() || quota.secondary_used_percent.is_some() {
        return view! {
            <div class="space-y-2 mt-3">
                <CodexRateQuotaBar
                    label=t.upstream_pool_quota_primary()
                    used_percent=quota.primary_used_percent
                    reset_secs=quota.primary_reset_after_secs
                />
                <CodexRateQuotaBar
                    label=t.upstream_pool_quota_secondary()
                    used_percent=quota.secondary_used_percent
                    reset_secs=quota.secondary_reset_after_secs
                />
            </div>
        }
        .into_any();
    }

    view! {
        <div class="mt-3 text-xs text-theme-muted">{t.upstream_quota_na()}</div>
    }
    .into_any()
}

#[component]
fn KeyTestStatus(
    key_id: String,
    enabled: bool,
    key_testing: ReadSignal<std::collections::HashMap<String, bool>>,
    key_test_results: ReadSignal<std::collections::HashMap<String, crate::types::UpstreamTestResult>>,
) -> impl IntoView {
    let t = use_translations();
    move || {
        let testing = key_testing.get().get(&key_id).copied().unwrap_or(false);
        if testing {
            return view! {
                <div class="mt-2 text-xs text-accent animate-pulse">{t.upstream_pool_testing()}</div>
            }
            .into_any();
        }
        let results = key_test_results.get();
        if let Some(tr) = results.get(&key_id) {
            if let Some(err) = &tr.error {
                return view! {
                    <div class="mt-2 text-xs text-error break-words">{err.clone()}</div>
                }
                .into_any();
            }
            if tr.ok {
                let detail = tr
                    .model_count
                    .map(|n| format!(" · {} models", n))
                    .unwrap_or_default();
                return view! {
                    <div class="mt-2 text-xs text-accent">
                        {t.upstream_pool_test_ok()} " · " {tr.latency_ms} "ms" {detail}
                    </div>
                }
                .into_any();
            }
            return view! {
                <div class="mt-2 text-xs text-error">{t.upstream_quota_test_failed()}</div>
            }
            .into_any();
        }
        if !enabled {
            return view! {
                <div class="mt-2 text-xs text-theme-muted">{t.upstream_pool_key_disabled_hint()}</div>
            }
            .into_any();
        }
        ().into_any()
    }
}

#[component]
pub fn UpstreamKeyPoolCards(
    keys: Vec<UpstreamKeyView>,
    on_toggle: Callback<(String, bool)>,
    on_test: Callback<String>,
    on_delete_confirm: Callback<String>,
    key_testing: ReadSignal<std::collections::HashMap<String, bool>>,
    key_test_results: ReadSignal<std::collections::HashMap<String, crate::types::UpstreamTestResult>>,
    delete_confirm_id: RwSignal<Option<String>>,
    key_deleting: ReadSignal<std::collections::HashMap<String, bool>>,
) -> impl IntoView {
    let t = use_translations();

    view! {
        <div class="upstream-key-pool-grid">
            {keys.into_iter().map(|k| {
                let key_id = k.id.clone();
                let key_id_toggle = k.id.clone();
                let key_id_test = k.id.clone();
                let key_id_delete = k.id.clone();
                let key_id_status = k.id.clone();
                let enabled = k.enabled;
                let models = k.models.clone();
                let email = k.email.clone();
                let plan = k.plan_type.clone();
                let quota = k.quota.clone();
                let key_id_quota = k.id.clone();
                let account_short = short_account_id(&k.account_id);
                let card_class = if enabled {
                    "upstream-key-card"
                } else {
                    "upstream-key-card upstream-key-card-disabled"
                };
                view! {
                    <div class=card_class>
                        <div class="flex items-start justify-between gap-2 mb-2">
                            <div class="min-w-0">
                                <div class="flex items-center gap-2 flex-wrap">
                                    <span class="upstream-key-card-id">{key_id.clone()}</span>
                                    {plan.as_ref().map(|p| view! {
                                        <span class=plan_badge_class(p)>{p.clone()}</span>
                                    })}
                                    <span class=move || if enabled { "badge badge-success text-xs" } else { "badge text-xs" }>
                                        {if enabled { t.upstream_key_status_enabled() } else { t.upstream_key_status_disabled() }}
                                    </span>
                                </div>
                                <div class="text-xs text-theme-muted font-mono mt-1 truncate">{k.preview.clone()}</div>
                                {email.map(|em| view! {
                                    <div class="text-xs text-theme-muted mt-1 truncate">{em}</div>
                                })}
                                <div class="text-[11px] text-theme-muted font-mono mt-1">
                                    {t.upstream_pool_col_account()}: {account_short}
                                </div>
                            </div>
                            <label class="flex items-center gap-1 shrink-0">
                                <input type="checkbox" prop:checked=enabled on:change=move |_| {
                                    on_toggle.run((key_id_toggle.clone(), !enabled));
                                } />
                            </label>
                        </div>

                        {(!models.is_empty()).then(|| view! {
                            <div class="mt-2">
                                <div class="text-[11px] text-theme-muted mb-1">
                                    {t.upstream_pool_col_models()} ({models.len()})
                                </div>
                                <div class="upstream-key-model-chips">
                                    {models.iter().take(8).map(|m| view! {
                                        <span class="upstream-key-model-chip">{m.clone()}</span>
                                    }).collect_view()}
                                    {(models.len() > 8).then(|| view! {
                                        <span class="upstream-key-model-chip">{"+"}{models.len() - 8}</span>
                                    })}
                                </div>
                            </div>
                        })}

                        {move || {
                            let results = key_test_results.get();
                            let display_quota = results
                                .get(&key_id_quota)
                                .and_then(|r| r.quota.clone())
                                .or_else(|| quota.clone());
                            match display_quota {
                                Some(q) => view! { <KeyQuotaSection quota=q /> }.into_any(),
                                None => ().into_any(),
                            }
                        }}

                        <KeyTestStatus
                            key_id=key_id_status
                            enabled=enabled
                            key_testing=key_testing
                            key_test_results=key_test_results
                        />

                        <div class="flex items-center gap-3 mt-3 pt-3 border-t border-theme/10 text-xs text-theme-muted">
                            <span>{t.upstream_pool_col_inflight()}: <span class="font-mono text-theme">{k.inflight}</span></span>
                            { (k.cooldown_remaining_secs > 0).then(|| view! {
                                <span class="text-warning font-mono">{format!("CD {}s", k.cooldown_remaining_secs)}</span>
                            })}
                        </div>

                        <div class="mt-3 flex flex-col gap-2">
                            {move || {
                                let testing = key_testing.get().get(&key_id_test).copied().unwrap_or(false);
                                let deleting = key_deleting.get().get(&key_id_delete).copied().unwrap_or(false);
                                let confirm = delete_confirm_id.get().as_deref() == Some(key_id_delete.as_str());
                                let kid_click = key_id_test.clone();
                                let kid_del = key_id_delete.clone();
                                view! {
                                    <button type="button" class="btn btn-secondary text-xs w-full"
                                        disabled=testing || !enabled
                                        on:click=move |_| on_test.run(kid_click.clone())
                                    >
                                        {if testing { t.upstream_pool_testing() } else { t.upstream_pool_col_test() }}
                                    </button>
                                    {if confirm {
                                        view! {
                                            <div class="flex gap-2">
                                                <button type="button" class="btn btn-danger text-xs flex-1"
                                                    disabled=deleting
                                                    on:click=move |_| on_delete_confirm.run(kid_del.clone())
                                                >
                                                    {if deleting { "..." } else { t.upstream_pool_delete_confirm() }}
                                                </button>
                                                <button type="button" class="btn btn-secondary text-xs flex-1"
                                                    on:click=move |_| delete_confirm_id.set(None)
                                                >
                                                    {t.upstream_pool_delete_cancel()}
                                                </button>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <button type="button" class="btn btn-secondary text-xs w-full text-error border border-error/30"
                                                disabled=deleting
                                                on:click=move |_| delete_confirm_id.set(Some(kid_del.clone()))
                                            >
                                                {t.upstream_pool_delete_key()}
                                            </button>
                                        }.into_any()
                                    }}
                                }
                            }}
                        </div>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}
