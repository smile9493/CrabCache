use leptos::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::locale::use_translations;

const ACTION_OPTIONS: &[(&str, &str)] = &[
    ("", "All"),
    ("create_key", "Create Key"),
    ("revoke_key", "Revoke Key"),
    ("put_domain_policies", "Update Policies"),
    ("cache_invalidate", "Cache Invalidate"),
    ("update_cache_config", "Update Config"),
    ("clear_logs", "Clear Logs"),
];

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub timestamp: String,
    pub action: String,
    pub actor: String,
    pub target: Option<String>,
    pub detail: Option<serde_json::Value>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct AuditLogResponse {
    entries: Vec<AuditLogEntry>,
}

fn action_color(action: &str) -> &'static str {
    match action {
        "create_key" => "teal",
        "revoke_key" => "rose",
        "put_domain_policies" => "violet",
        "cache_invalidate" => "amber",
        "update_cache_config" => "blue",
        "clear_logs" => "orange",
        _ => "slate",
    }
}

#[component]
pub fn AuditLogPage() -> impl IntoView {
    let _t = use_translations();
    let entries: RwSignal<Option<Result<Vec<AuditLogEntry>, String>>> = RwSignal::new(None);
    let loading = RwSignal::new(true);
    let action_filter: RwSignal<String> = RwSignal::new(String::new());
    let offset = RwSignal::new(0i64);
    let has_more = RwSignal::new(true);
    let limit: i64 = 50;

    let load_entries = move |append: bool| {
        let current_offset = if append { offset.get() } else { 0 };
        loading.set(true);
        let action = action_filter.get();
        let limit_val = limit;
        let offset_val = current_offset;
        leptos::task::spawn_local(async move {
            let action_opt = if action.is_empty() {
                None
            } else {
                Some(action.clone())
            };
            match api::fetch_audit_logs(limit_val, offset_val, action_opt).await {
                Ok(mut new_entries) => {
                    if append {
                        entries.try_update(|opt| {
                            if let Some(Ok(existing)) = opt {
                                existing.append(&mut new_entries);
                            }
                        });
                        offset.try_set(offset_val + limit_val);
                        has_more.try_set(new_entries.len() >= limit_val as usize);
                    } else {
                        has_more.try_set(new_entries.len() >= limit_val as usize);
                        offset.try_set(limit_val);
                        entries.try_set(Some(Ok(new_entries)));
                    }
                }
                Err(e) => entries.try_set(Some(Err(e))),
            }
            loading.try_set(false);
        });
    };

    load_entries(false);

    let on_filter_change = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        action_filter.set(val.clone());
        offset.set(0);
        load_entries(false);
    };

    view! {
        <div class="page-content space-y-4">
            <PageHeader title=move || _t.audit_title() description=move || _t.audit_desc()>
                <div />
            </PageHeader>

            <div class="glass-card">
                <div class="flex items-center gap-3">
                    <label class="text-sm font-medium text-theme-muted">"Action:"</label>
                    <select
                        class="dash-select"
                        on:change=on_filter_change
                        prop:value=move || action_filter.get()
                    >
                        {ACTION_OPTIONS
                            .iter()
                            .map(|(val, label)| {
                                view! {
                                    <option value=*val>{*label}</option>
                                }
                            })
                            .collect_view()}
                    </select>
                </div>
            </div>

            <div class="glass-card">
                {move || match entries.get() {
                    None => view! { <crate::components::skeleton::SkeletonTable rows=5 cols=4 /> }.into_any(),
                    Some(Err(e)) => view! {
                        <div class="text-error text-sm py-4">{e}</div>
                    }.into_any(),
                    Some(Ok(list)) if list.is_empty() => view! {
                        <div class="text-theme-muted text-sm py-8 text-center">"No audit log entries found"</div>
                    }.into_any(),
                    Some(Ok(list)) => view! {
                        <div class="overflow-x-auto">
                            <table class="dash-table">
                                <thead>
                                    <tr>
                                        <th>"Time"</th>
                                        <th>"Action"</th>
                                        <th>"Actor"</th>
                                        <th>"Target"</th>
                                        <th>"Detail"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {list.iter().map(|entry| {
                                        let action = entry.action.clone();
                                        let color = action_color(&action);
                                        let detail_text = entry.detail.as_ref()
                                            .map(|d| serde_json::to_string_pretty(d).unwrap_or_default())
                                            .unwrap_or_default();
                                        let detail_preview = if detail_text.len() > 80 {
                                            format!("{}…", &detail_text[..80])
                                        } else {
                                            detail_text.clone()
                                        };
                                        view! {
                                            <tr>
                                                <td class="text-xs whitespace-nowrap">{entry.timestamp.clone()}</td>
                                                <td>
                                                    <span class=format!("badge badge-{}", color)>{entry.action.clone()}</span>
                                                </td>
                                                <td class="text-sm">{entry.actor.clone()}</td>
                                                <td class="text-sm text-theme-muted">
                                                    {entry.target.clone().unwrap_or_else(|| "—".to_string())}
                                                </td>
                                                <td class="text-xs max-w-xs truncate" title=detail_text>
                                                    {detail_preview}
                                                </td>
                                            </tr>
                                        }
                                    }).collect_view()}
                                </tbody>
                            </table>
                        </div>
                    }.into_any(),
                }}
            </div>

            {move || if has_more.get() && !loading.get() {
                view! {
                    <button
                        class="dash-btn dash-btn-secondary w-full"
                        on:click=move |_| load_entries(true)
                    >
                        "Load More"
                    </button>
                }.into_any()
            } else if loading.get() {
                view! {
                    <div class="flex justify-center py-4"><Spinner /></div>
                }.into_any()
            } else {
                view! { <div /> }.into_any()
            }}
        </div>
    }
}
