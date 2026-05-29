use leptos::prelude::*;

use crate::api;
use crate::components::ui::*;
use crate::locale::use_translations;

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[component]
pub fn LogsManagePage() -> impl IntoView {
    let usage: RwSignal<Option<Result<crate::types::LogDiskUsage, String>>> = RwSignal::new(None);
    let retention: RwSignal<Option<Result<crate::types::RetentionPolicy, String>>> =
        RwSignal::new(None);
    let feedback: RwSignal<String> = RwSignal::new(String::new());

    // Load data on mount
    {
        let usage = usage;
        leptos::task::spawn_local(async move {
            match api::fetch_log_disk_usage().await {
                Ok(v) => usage.set(Some(Ok(v))),
                Err(e) => usage.set(Some(Err(e))),
            }
        });
    }
    {
        let retention = retention;
        leptos::task::spawn_local(async move {
            match api::fetch_retention_policy().await {
                Ok(v) => retention.set(Some(Ok(v))),
                Err(e) => retention.set(Some(Err(e))),
            }
        });
    }

    view! {
        <div class="space-y-6">
            <Alert variant="info" message=feedback.into() />

            // Disk Usage Section
            {move || match usage.get() {
                None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(u)) => view! { <DiskUsageCard usage=u /> }.into_any(),
            }}

            // Retention Policy Section
            {move || match retention.get() {
                None => view! { <crate::components::skeleton::SkeletonFormCard /> }.into_any(),
                Some(Err(e)) => view! {
                    <div class="glass-card text-error text-sm">{e}</div>
                }.into_any(),
                Some(Ok(policy)) => view! {
                    <RetentionPolicyCard
                        policy
                        feedback
                    />
                }.into_any(),
            }}

            // Manual Clear Section
            <ManualClearCard feedback usage />
        </div>
    }
}

#[component]
fn DiskUsageCard(usage: crate::types::LogDiskUsage) -> impl IntoView {
    let t = use_translations();
    let total = usage.total_bytes as f64;
    let trace_pct = if total > 0.0 {
        usage.trace_bytes as f64 / total * 100.0
    } else {
        0.0
    };
    let debug_pct = if total > 0.0 {
        usage.debug_trace_bytes as f64 / total * 100.0
    } else {
        0.0
    };
    let cap_idx_pct = if total > 0.0 {
        usage.capture_index_bytes as f64 / total * 100.0
    } else {
        0.0
    };
    let cap_body_pct = if total > 0.0 {
        usage.capture_body_bytes as f64 / total * 100.0
    } else {
        0.0
    };
    let total_str = format_bytes(usage.total_bytes);

    view! {
        <div class="glass-card space-y-4">
            <PanelHeader
                title=move || t.logs_manage_disk_usage().to_string()
                meta=move || total_str.clone()
            />

            <div class="space-y-3">
                <UsageRow
                    label=t.logs_manage_trace_logs()
                    bytes=usage.trace_bytes
                    file_count=usage.trace_file_count
                    pct=trace_pct
                    color="var(--cc-accent)"
                />
                <UsageRow
                    label=t.logs_manage_debug_trace()
                    bytes=usage.debug_trace_bytes
                    file_count=usage.debug_trace_file_count
                    pct=debug_pct
                    color="var(--cc-accent-bright)"
                />
                <UsageRow
                    label=t.logs_manage_capture_index()
                    bytes=usage.capture_index_bytes
                    file_count=0
                    pct=cap_idx_pct
                    color="var(--cc-warning)"
                />
                <UsageRow
                    label=t.logs_manage_capture_bodies()
                    bytes=usage.capture_body_bytes
                    file_count=usage.capture_body_file_count
                    pct=cap_body_pct
                    color="var(--cc-error)"
                />
            </div>

            // Stacked bar
            <div class="progress-bar" style="height: 12px;">
                <div style=format!(
                    "width: {:.1}%; background: var(--cc-accent); height: 100%; display: inline-block;",
                    trace_pct
                )></div>
                <div style=format!(
                    "width: {:.1}%; background: var(--cc-accent-bright); height: 100%; display: inline-block;",
                    debug_pct
                )></div>
                <div style=format!(
                    "width: {:.1}%; background: var(--cc-warning); height: 100%; display: inline-block;",
                    cap_idx_pct
                )></div>
                <div style=format!(
                    "width: {:.1}%; background: var(--cc-error); height: 100%; display: inline-block;",
                    cap_body_pct
                )></div>
            </div>
        </div>
    }
}

#[component]
fn UsageRow(
    label: &'static str,
    bytes: u64,
    file_count: usize,
    pct: f64,
    color: &'static str,
) -> impl IntoView {
    let t = use_translations();
    let bytes_str = format_bytes(bytes);
    let pct_str = format!("{:.1}%", pct);
    view! {
        <div class="flex items-center justify-between text-sm">
            <div class="flex items-center gap-2">
                <span style=format!("width: 10px; height: 10px; border-radius: 2px; background: {}", color)></span>
                <span class="text-theme">{label}</span>
                {if file_count > 0 {
                    let count_str = format!("({} {})", file_count, t.logs_manage_files());
                    view! { <span class="text-theme-muted">{count_str}</span> }.into_any()
                } else {
                    ().into_any()
                }}
            </div>
            <div class="flex items-center gap-3">
                <span class="text-theme-secondary font-mono tabular-nums">{bytes_str}</span>
                <span class="text-theme-muted text-xs font-mono tabular-nums">{pct_str}</span>
            </div>
        </div>
    }
}

#[component]
fn RetentionPolicyCard(
    policy: crate::types::RetentionPolicy,
    feedback: RwSignal<String>,
) -> impl IntoView {
    let t = use_translations();
    let max_age = RwSignal::new(policy.max_age_hours.to_string());
    let max_disk = RwSignal::new(policy.max_disk_mb.to_string());
    let max_trace = RwSignal::new(policy.max_trace_files.to_string());
    let max_capture = RwSignal::new(policy.max_capture_body_files.to_string());
    let pg_retention_days = RwSignal::new(policy.pg_retention_days.to_string());
    let compress_before_delete = RwSignal::new(policy.compress_before_delete);
    let compressed_retention_days = RwSignal::new(policy.compressed_retention_days.to_string());
    let saving = RwSignal::new(false);

    let on_save = move |_| {
        let age_val = max_age.get().parse::<u32>().unwrap_or(0);
        let disk_val = max_disk.get().parse::<u32>().unwrap_or(0);
        let trace_val = max_trace.get().parse::<usize>().unwrap_or(0);
        let capture_val = max_capture.get().parse::<usize>().unwrap_or(0);
        let pg_days = pg_retention_days.get().parse::<u64>().unwrap_or(0);
        let compressed_days = compressed_retention_days.get().parse::<u64>().unwrap_or(0);

        let req = crate::types::RetentionPolicy {
            max_age_hours: age_val,
            max_disk_mb: disk_val,
            max_trace_files: trace_val,
            max_capture_body_files: capture_val,
            pg_retention_days: pg_days,
            compress_before_delete: compress_before_delete.get(),
            compressed_retention_days: compressed_days,
        };

        saving.set(true);
        let feedback = feedback;
        leptos::task::spawn_local(async move {
            match api::update_retention_policy(&req).await {
                Ok(_) => feedback.set(t.logs_manage_retention_saved().to_string()),
                Err(e) => feedback.set(e),
            }
            saving.set(false);
        });
    };

    view! {
        <div class="glass-card space-y-4">
            <PanelHeader
                title=move || t.logs_manage_retention_policy().to_string()
                meta=move || t.logs_manage_auto_cleanup().to_string()
            />

            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="space-y-1.5">
                    <label class="text-xs text-theme-secondary">{t.logs_manage_max_age_label()}</label>
                    <input
                        type="number"
                        class="form-input w-full"
                        min="0"
                        prop:value=move || max_age.get()
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            max_age.set(v);
                        }
                    />
                </div>
                <div class="space-y-1.5">
                    <label class="text-xs text-theme-secondary">{t.logs_manage_max_disk_label()}</label>
                    <input
                        type="number"
                        class="form-input w-full"
                        min="0"
                        prop:value=move || max_disk.get()
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            max_disk.set(v);
                        }
                    />
                </div>
                <div class="space-y-1.5">
                    <label class="text-xs text-theme-secondary">{t.logs_manage_max_trace_files()}</label>
                    <input
                        type="number"
                        class="form-input w-full"
                        min="0"
                        prop:value=move || max_trace.get()
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            max_trace.set(v);
                        }
                    />
                </div>
                <div class="space-y-1.5">
                    <label class="text-xs text-theme-secondary">{t.logs_manage_max_capture_files()}</label>
                    <input
                        type="number"
                        class="form-input w-full"
                        min="0"
                        prop:value=move || max_capture.get()
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            max_capture.set(v);
                        }
                    />
                </div>
                <div class="space-y-1.5">
                    <label class="text-xs text-theme-secondary">{t.logs_manage_pg_retention_days()}</label>
                    <input
                        type="number"
                        class="form-input w-full"
                        min="0"
                        prop:value=move || pg_retention_days.get()
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            pg_retention_days.set(v);
                        }
                    />
                </div>
                <div class="space-y-1.5 flex items-end">
                    <label class="flex items-center gap-2 text-xs text-theme-secondary cursor-pointer">
                        <input
                            type="checkbox"
                            prop:checked=move || compress_before_delete.get()
                            on:change=move |ev| {
                                compress_before_delete.set(event_target_checked(&ev));
                            }
                        />
                        {t.logs_manage_compress_before_delete()}
                    </label>
                </div>
                {move || if compress_before_delete.get() {
                    view! {
                        <div class="space-y-1.5">
                            <label class="text-xs text-theme-secondary">{t.logs_manage_compressed_retention_days()}</label>
                            <input
                                type="number"
                                class="form-input w-full"
                                min="0"
                                prop:value=move || compressed_retention_days.get()
                                on:input=move |ev| {
                                    let v = event_target_value(&ev);
                                    compressed_retention_days.set(v);
                                }
                            />
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </div>

            <button
                on:click=on_save
                disabled=move || saving.get()
                class="btn btn-primary text-sm"
            >
                {move || if saving.get() { t.logs_manage_save_policy_saving() } else { t.logs_manage_save_policy() }}
            </button>
        </div>
    }
}

#[component]
fn ManualClearCard(
    feedback: RwSignal<String>,
    usage: RwSignal<Option<Result<crate::types::LogDiskUsage, String>>>,
) -> impl IntoView {
    let t = use_translations();
    let target: RwSignal<String> = RwSignal::new("all".to_string());
    let older_than: RwSignal<String> = RwSignal::new(String::new());
    let clearing = RwSignal::new(false);
    let show_confirm = RwSignal::new(false);
    let last_result: RwSignal<Option<String>> = RwSignal::new(None);

    let do_clear = move || {
        let target_val = target.get();
        let older_val = older_than.get().parse::<u32>().ok();

        clearing.set(true);
        show_confirm.set(false);

        let feedback = feedback;
        let usage = usage;
        leptos::task::spawn_local(async move {
            match api::clear_logs(&target_val, older_val).await {
                Ok(resp) => {
                    let msg = t
                        .logs_manage_cleared_fmt()
                        .replacen("{}", &resp.deleted_files.len().to_string(), 1)
                        .replacen("{}", &format_bytes(resp.freed_bytes), 1);
                    feedback.set(msg.clone());
                    last_result.set(Some(msg));
                    // Reload disk usage
                    leptos::task::spawn_local(async move {
                        match api::fetch_log_disk_usage().await {
                            Ok(v) => usage.set(Some(Ok(v))),
                            Err(e) => usage.set(Some(Err(e))),
                        }
                    });
                }
                Err(e) => {
                    feedback.set(e.clone());
                    let err_msg = t.logs_manage_error_fmt().replacen("{}", &e, 1);
                    last_result.set(Some(err_msg));
                }
            }
            clearing.set(false);
        });
    };

    view! {
        <div class="glass-card space-y-4">
            <h3 class="text-sm font-medium text-theme">{t.logs_manage_manual_cleanup()}</h3>

            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="space-y-1.5">
                    <label class="text-xs text-theme-secondary">{t.logs_manage_target_label()}</label>
                    <select
                        class="form-input w-full"
                        on:change=move |ev| target.set(event_target_value(&ev))
                    >
                        <option value="all" selected=move || target.get() == "all">{t.logs_manage_target_all()}</option>
                        <option value="trace_rotated" selected=move || target.get() == "trace_rotated">{t.logs_manage_target_trace()}</option>
                        <option value="debug_rotated" selected=move || target.get() == "debug_rotated">{t.logs_manage_target_debug()}</option>
                        <option value="capture" selected=move || target.get() == "capture">{t.logs_manage_target_capture()}</option>
                    </select>
                </div>
                <div class="space-y-1.5">
                    <label class="text-xs text-theme-secondary">{t.logs_manage_older_than_label()}</label>
                    <input
                        type="number"
                        class="form-input w-full"
                        min="1"
                        placeholder=t.logs_manage_older_than_placeholder().to_string()
                        prop:value=move || older_than.get()
                        on:input=move |ev| older_than.set(event_target_value(&ev))
                    />
                </div>
            </div>

            {move || {
                if show_confirm.get() {
                    let confirm_clear = {
                        let do_clear = do_clear;
                        move |_| do_clear()
                    };
                    view! {
                        <div class="flex items-center gap-3 p-3 rounded-lg" style="background: var(--cc-error-bg, rgba(239,68,68,0.1));">
                            <span class="text-sm text-theme">{t.logs_manage_confirm_msg()}</span>
                            <button
                                on:click=confirm_clear
                                disabled=move || clearing.get()
                                class="btn btn-sm text-sm"
                                style="background: var(--cc-error); color: white;"
                            >
                                {move || if clearing.get() { t.logs_manage_clearing() } else { t.cache_ops_confirm_ok() }}
                            </button>
                            <button
                                on:click=move |_| show_confirm.set(false)
                                class="btn btn-sm text-sm"
                            >
                                {t.cache_ops_confirm_cancel()}
                            </button>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <button
                            on:click=move |_| show_confirm.set(true)
                            disabled=move || clearing.get()
                            class="btn btn-sm text-sm"
                            style="background: var(--cc-error); color: white;"
                        >
                            {t.logs_manage_clear_btn()}
                        </button>
                    }.into_any()
                }
            }}

            {move || last_result.get().map(|msg| view! {
                <div class="text-sm text-theme-secondary">{msg}</div>
            })}
        </div>
    }
}
