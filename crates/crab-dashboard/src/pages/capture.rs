//! 包捕获 / Raw Packet Capture — dual-pane viewer with anomaly badges.

use leptos::prelude::*;
use leptos_meta::Style;

use crate::api;
use crate::components::page_header::PageHeader;
use crate::components::ui::*;
use crate::locale::use_translations;
use crate::types::{
    CaptureDetailResponse, CaptureListResponse, CaptureStatsResponse, RawCaptureEntry,
};

// ── Filter form ──────────────────────────────────────────────────────

#[derive(Clone, Default, PartialEq)]
struct CaptureFilterForm {
    consumer: String,
    project_id: String,
    request_hash: String,
    session_fingerprint: String,
    backend_name: String,
    /// When true, list API filters `client_wire_api=responses` (Codex CLI).
    codex_only: bool,
}

impl CaptureFilterForm {
    fn is_active(&self) -> bool {
        !self.consumer.trim().is_empty()
            || !self.project_id.trim().is_empty()
            || !self.request_hash.trim().is_empty()
            || !self.session_fingerprint.trim().is_empty()
            || !self.backend_name.trim().is_empty()
            || self.codex_only
    }
    fn consumer_opt(&self) -> Option<&str> {
        let v = self.consumer.trim();
        if v.is_empty() { None } else { Some(v) }
    }
    fn project_opt(&self) -> Option<&str> {
        let v = self.project_id.trim();
        if v.is_empty() { None } else { Some(v) }
    }
    fn hash_opt(&self) -> Option<&str> {
        let v = self.request_hash.trim();
        if v.is_empty() { None } else { Some(v) }
    }
    fn session_opt(&self) -> Option<&str> {
        let v = self.session_fingerprint.trim();
        if v.is_empty() { None } else { Some(v) }
    }
    fn backend_opt(&self) -> Option<&str> {
        let v = self.backend_name.trim();
        if v.is_empty() { None } else { Some(v) }
    }
    fn client_wire_api_opt(&self) -> Option<&str> {
        if self.codex_only {
            Some("responses")
        } else {
            None
        }
    }
}

// ── Formatting helpers ───────────────────────────────────────────────

fn fmt_ts(entry: &RawCaptureEntry, tz_label: &str) -> String {
    let clock = entry
        .timestamp_beijing
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| crate::datetime::format_ms_china_datetime(entry.timestamp_ms));
    format!("{clock} {tz_label}")
}

fn fmt_delta(v: i64) -> String {
    if v >= 0 {
        format!("+{v}")
    } else {
        format!("{v}")
    }
}

fn entry_is_anomaly(e: &RawCaptureEntry) -> bool {
    e.delta_bytes.abs() > 2048 || e.structure.delta_message_count > 0
}

fn session_short(e: &RawCaptureEntry) -> String {
    e.session_fingerprint
        .as_deref()
        .map(|s| s.chars().take(8).collect())
        .unwrap_or_else(|| "-".to_string())
}

fn backend_short(e: &RawCaptureEntry) -> String {
    e.backend_name.clone().unwrap_or_else(|| "-".to_string())
}

fn wire_short(e: &RawCaptureEntry) -> String {
    match e.client_wire_api.as_deref() {
        Some("responses") => "R".to_string(),
        Some("chat_completions") => "C".to_string(),
        None if e.client_path_suffix.as_deref() == Some("/v1/responses") => "R".to_string(),
        _ => "-".to_string(),
    }
}

// ── Main page ────────────────────────────────────────────────────────

#[component]
pub fn CapturePage() -> impl IntoView {
    let t = use_translations();

    let selected_id: RwSignal<Option<String>> = RwSignal::new(None);
    let list_data: RwSignal<Option<Result<CaptureListResponse, String>>> = RwSignal::new(None);
    let stats_data: RwSignal<Option<Result<CaptureStatsResponse, String>>> = RwSignal::new(None);
    let detail_data: RwSignal<Option<Result<CaptureDetailResponse, String>>> = RwSignal::new(None);
    let detail_loading = RwSignal::new(false);
    let filter_draft = RwSignal::new(CaptureFilterForm::default());
    let active_filter = RwSignal::new(CaptureFilterForm::default());

    let load_list = {
        let active_filter = active_filter;
        move || {
            let f = active_filter.get();
            let consumer = f.consumer_opt().map(|s| s.to_string());
            let project = f.project_opt().map(|s| s.to_string());
            let hash = f.hash_opt().map(|s| s.to_string());
            let session = f.session_opt().map(|s| s.to_string());
            let backend = f.backend_opt().map(|s| s.to_string());
            let wire = f.client_wire_api_opt().map(|s| s.to_string());
            leptos::task::spawn_local(async move {
                list_data.try_set(None);
                let result = api::fetch_capture_list(
                    24,
                    Some(200),
                    consumer.as_deref(),
                    project.as_deref(),
                    hash.as_deref(),
                    session.as_deref(),
                    backend.as_deref(),
                    wire.as_deref(),
                )
                .await;
                list_data.try_set(Some(result));
            });
        }
    };

    let load_stats = move || {
        leptos::task::spawn_local(async move {
            stats_data.try_set(None);
            let result = api::fetch_capture_stats(24).await;
            stats_data.try_set(Some(result));
        });
    };

    let load_detail = move |id: String| {
        detail_loading.set(true);
        detail_data.set(None);
        leptos::task::spawn_local(async move {
            let result = api::fetch_capture_detail(&id).await;
            detail_data.try_set(Some(result));
            detail_loading.try_set(false);
        });
    };

    let reload = move || {
        selected_id.set(None);
        detail_data.set(None);
        load_list();
        load_stats();
    };

    // Initial load.
    reload();

    let apply_filters = move || {
        active_filter.set(filter_draft.get());
        reload();
    };

    let clear_filters = move || {
        filter_draft.set(CaptureFilterForm::default());
        active_filter.set(CaptureFilterForm::default());
        reload();
    };

    view! {
        <Style>{include_str!("../../style/output.css")}</Style>

        <div class="capture-page space-y-4">
            // ── Header ──
            <PageHeader
                title=move || t.capture_title()
                description=move || t.capture_desc()
            >
                <button
                    on:click=move |_| reload()
                    class="btn btn-secondary text-sm"
                >
                    {t.capture_refresh()}
                </button>
            </PageHeader>

            // ── Stats metrics ──
            {move || match stats_data.get() {
                None => ().into_any(),
                Some(Err(ref e)) => {
                    let msg = format!("{}{}", t.capture_error_prefix(), e);
                    view! { <div class="text-xs text-error px-3 py-2 glass-card-flat">{msg}</div> }.into_any()
                }
                Some(Ok(ref s)) => {
                    let reasoning_pct = if s.total_captures > 0 {
                        format!("{:.0}%", s.reasoning_injection_rate)
                    } else {
                        "-".to_string()
                    };
                    let thinking_pct = if s.total_captures > 0 {
                        format!("{:.0}%", s.thinking_markup_rate)
                    } else {
                        "-".to_string()
                    };
                    let avg_delta_str = fmt_delta(s.avg_delta_bytes as i64);
                    view! {
                        <div class="grid grid-cols-2 lg:grid-cols-4 gap-2">
                            <div class="metric-card">
                                <div class="metric-card-label">{t.capture_stat_total()}</div>
                                <div class="metric-card-value">{s.total_captures.to_string()}</div>
                            </div>
                            <div class="metric-card">
                                <div class="metric-card-label">{t.capture_stat_avg_delta()}</div>
                                <div class="metric-card-value">{avg_delta_str}</div>
                            </div>
                            <div class="metric-card">
                                <div class="metric-card-label">{t.capture_stat_reasoning_injected()}</div>
                                <div class="metric-card-value">{reasoning_pct}</div>
                            </div>
                            <div class="metric-card">
                                <div class="metric-card-label">{t.capture_stat_thinking_markup()}</div>
                                <div class="metric-card-value">{thinking_pct}</div>
                            </div>
                        </div>
                    }.into_any()
                }
            }}

            // ── Filter bar ──
            <div class=move || {
                if filter_draft.get().is_active() {
                    "glass-card-flat logs-filter-bar capture-filter-active"
                } else {
                    "glass-card-flat logs-filter-bar"
                }
            }>
                <input
                    type="text"
                    class="input"
                    placeholder=t.capture_filter_consumer()
                    prop:value=move || filter_draft.get().consumer
                    on:input=move |ev| filter_draft.update(|f| f.consumer = event_target_value(&ev))
                />
                <input
                    type="text"
                    class="input"
                    placeholder=t.capture_filter_project()
                    prop:value=move || filter_draft.get().project_id
                    on:input=move |ev| filter_draft.update(|f| f.project_id = event_target_value(&ev))
                />
                <input
                    type="text"
                    class="input"
                    placeholder=t.capture_filter_hash()
                    prop:value=move || filter_draft.get().request_hash
                    on:input=move |ev| filter_draft.update(|f| f.request_hash = event_target_value(&ev))
                />
                <input
                    type="text"
                    class="input"
                    placeholder=t.capture_filter_session()
                    prop:value=move || filter_draft.get().session_fingerprint
                    on:input=move |ev| filter_draft.update(|f| f.session_fingerprint = event_target_value(&ev))
                />
                <input
                    type="text"
                    class="input"
                    placeholder=t.capture_filter_backend()
                    prop:value=move || filter_draft.get().backend_name
                    on:input=move |ev| filter_draft.update(|f| f.backend_name = event_target_value(&ev))
                />
                <div class="logs-filter-actions">
                    <button
                        on:click=move |_| {
                            filter_draft.update(|f| f.codex_only = !f.codex_only);
                            apply_filters();
                        }
                        class=move || {
                            if active_filter.get().codex_only {
                                "btn btn-primary text-xs"
                            } else {
                                "btn btn-secondary text-xs"
                            }
                        }
                    >
                        {t.capture_filter_codex()}
                    </button>
                    <button on:click=move |_| apply_filters() class="btn btn-primary text-xs">
                        {t.capture_filter_apply()}
                    </button>
                    <button
                        on:click=move |_| clear_filters()
                        class="btn btn-secondary text-xs"
                        disabled=move || !active_filter.get().is_active()
                    >
                        {t.capture_filter_clear()}
                    </button>
                </div>
            </div>

            // ── Active filter badge ──
            {move || {
                if active_filter.get().is_active() {
                    let f = active_filter.get();
                    let mut parts = Vec::new();
                    if let Some(v) = f.consumer_opt() { parts.push(format!("consumer={v}")); }
                    if let Some(v) = f.project_opt() { parts.push(format!("project={v}")); }
                    if let Some(v) = f.hash_opt() { parts.push(format!("hash={v}")); }
                    if let Some(v) = f.session_opt() { parts.push(format!("session={v}")); }
                    if let Some(v) = f.backend_opt() { parts.push(format!("backend={v}")); }
                    if f.codex_only { parts.push("wire=responses".into()); }
                    let label = format!("{} {}", t.capture_filter_active(), parts.join(" · "));
                    view! {
                        <div class="px-1">
                            <span class="badge badge-sm badge-info font-mono">{label}</span>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}

            // ── List + Detail split ──
            {move || match list_data.get() {
                None => view! { <Spinner /> }.into_any(),
                Some(Err(ref e)) => {
                    let msg = format!("{}{}", t.capture_error_prefix(), e);
                    view! { <div class="glass-card text-error text-sm">{msg}</div> }.into_any()
                }
                Some(Ok(ref resp)) => {
                    let entries = &resp.entries;
                    if entries.is_empty() {
                        return view! { <EmptyState message=t.capture_empty() /> }.into_any();
                    }
                    let entries_clone = entries.clone();
                    let tz_label = t.capture_time_tz_label();
                    view! {
                        <div class="logs-split">
                            // ── List pane ──
                            <div class="logs-split-list glass-card-flat">
                                <div class="flex items-center gap-2 px-4 pt-2 pb-1">
                                    <h3 class="text-sm font-semibold text-theme">{t.capture_list_title()}</h3>
                                    <span class="text-xs text-theme-muted">
                                        {format!("{} {}", entries_clone.len(), t.capture_records_unit())}
                                    </span>
                                </div>
                                <table class="table table-compact">
                                    <thead>
                                        <tr>
                                            <th>{t.capture_col_time()}</th>
                                            <th>{t.capture_col_session()}</th>
                                            <th>{t.capture_col_wire()}</th>
                                            <th>{t.capture_col_backend()}</th>
                                            <th>{t.capture_col_model()}</th>
                                            <th>{t.capture_col_delta()}</th>
                                            <th>{t.capture_col_duration()}</th>
                                            <th>{t.capture_col_msgs()}</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {entries_clone
                                            .into_iter()
                                            .map(|entry| {
                                                let rid = entry.request_id.clone();
                                                let rid_for_click = rid.clone();
                                                let is_selected = move || {
                                                    selected_id.get().as_deref() == Some(rid.as_str())
                                                };
                                                let anomaly = entry_is_anomaly(&entry);
                                                let client = &entry.structure.client;
                                                let upstream = &entry.structure.upstream;
                                                let session_title = entry.session_fingerprint.clone().unwrap_or_default();
                                                let session_label = session_short(&entry);
                                                let wire_label = wire_short(&entry);
                                                let wire_title = entry
                                                    .client_wire_api
                                                    .clone()
                                                    .or_else(|| entry.client_path_suffix.clone())
                                                    .unwrap_or_default();
                                                let backend_label = backend_short(&entry);
                                                let row_class = move || {
                                                    if is_selected() {
                                                        "logs-row-selected"
                                                    } else if anomaly {
                                                        "logs-row capture-row-anomaly"
                                                    } else {
                                                        "logs-row"
                                                    }
                                                };
                                                view! {
                                                    <tr
                                                        class=row_class
                                                        on:click={
                                                            let rid = rid_for_click.clone();
                                                            move |_| {
                                                                selected_id.set(Some(rid.clone()));
                                                                load_detail(rid.clone());
                                                            }
                                                        }
                                                    >
                                                        <td class="text-xs font-mono text-theme-secondary whitespace-nowrap">
                                                            {fmt_ts(&entry, tz_label)}
                                                        </td>
                                                        <td class="text-xs font-mono text-theme-secondary" title=session_title.clone()>
                                                            {session_label}
                                                        </td>
                                                        <td class="text-xs font-mono text-theme-secondary" title=wire_title.clone()>
                                                            {wire_label}
                                                        </td>
                                                        <td class="text-xs font-mono text-theme-secondary truncate max-w-[5rem]">
                                                            {backend_label}
                                                        </td>
                                                        <td class="text-xs font-mono text-theme truncate max-w-[8rem]">
                                                            {entry.model.clone()}
                                                        </td>
                                                        <td class="text-xs font-mono tabular-nums">
                                                            <DeltaCell value=entry.delta_bytes />
                                                        </td>
                                                        <td class="text-xs font-mono tabular-nums text-theme-secondary">
                                                            {entry.duration_ms.to_string()}
                                                        </td>
                                                        <td class="text-xs font-mono tabular-nums text-theme-secondary">
                                                            {format!("{}/{}", client.message_count, upstream.message_count)}
                                                        </td>
                                                    </tr>
                                                }
                                            })
                                            .collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            </div>

                            // ── Detail pane ──
                            <div class="logs-split-detail glass-card">
                                {move || {
                                    if detail_loading.get() {
                                        return view! { <Spinner /> }.into_any();
                                    }
                                    match detail_data.get() {
                                        None => view! {
                                            <div class="logs-detail-empty">
                                                <p class="text-sm text-theme-muted">{t.capture_select_hint()}</p>
                                            </div>
                                        }.into_any(),
                                        Some(Err(ref e)) => {
                                            let msg = format!("{}{}", t.capture_error_prefix(), e);
                                            view! { <div class="text-sm text-error p-4">{msg}</div> }.into_any()
                                        }
                                        Some(Ok(ref detail)) => {
                                            view! { <CaptureDetailContent detail=detail.clone() /> }.into_any()
                                        }
                                    }
                                }}
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

// ── Delta cell with color ────────────────────────────────────────────

#[component]
fn DeltaCell(value: i64) -> impl IntoView {
    let (text, color) = if value == 0 {
        ("0".to_string(), "text-theme-secondary")
    } else if value > 0 {
        (format!("+{value}"), "text-warning")
    } else {
        (format!("{value}"), "text-info")
    };
    view! { <span class=color>{text}</span> }
}

// ── Detail pane ──────────────────────────────────────────────────────

#[component]
fn CaptureDetailContent(detail: CaptureDetailResponse) -> impl IntoView {
    let t = use_translations();
    let body_tab: RwSignal<usize> = RwSignal::new(0);
    let e = detail.entry.clone();

    let stream_badge = if e.stream {
        view! { <span class="badge badge-info">{t.capture_badge_sse()}</span> }.into_any()
    } else {
        ().into_any()
    };
    let reasoning_badge = if e.structure.reasoning_was_injected {
        view! { <span class="badge badge-warning">{t.capture_badge_reasoning()}</span> }.into_any()
    } else {
        ().into_any()
    };
    let thinking_badge = if e.structure.upstream.has_thinking_markup {
        view! { <span class="badge badge-accent">{t.capture_badge_thinking()}</span> }.into_any()
    } else {
        ().into_any()
    };
    let anomaly_badge = if entry_is_anomaly(&e) {
        view! { <span class="badge badge-error">{t.capture_badge_large_delta()}</span> }.into_any()
    } else {
        ().into_any()
    };

    let stream_val = if e.stream {
        t.capture_bool_yes()
    } else {
        t.capture_bool_no()
    };
    let reasoning_val = e.reasoning_strategy.as_deref().unwrap_or("-");
    let client = &e.structure.client;
    let upstream = &e.structure.upstream;
    let diff = &e.structure;

    let delta_msg_warn = diff.delta_message_count != 0;
    let delta_content_warn = diff.delta_content_chars.unsigned_abs() > 500;
    let delta_reasoning_warn = diff.delta_reasoning_chars != 0;

    let request_id_short: String = e.request_id.chars().take(16).collect();

    view! {
        <div class="logs-detail-inner">
            // ── Header ──
            <div class="logs-detail-header">
                <h3 class="text-sm font-semibold text-theme">{t.capture_detail_title()}</h3>
                <div class="flex items-center gap-1">
                    {stream_badge}
                    {reasoning_badge}
                    {thinking_badge}
                    {anomaly_badge}
                </div>
            </div>

            // ── Metadata ──
            <div class="logs-detail-meta">
                <DetailField label=t.capture_col_model() value=e.model.clone() />
                <DetailField label=t.capture_col_consumer() value=e.consumer.clone().unwrap_or("-".into()) />
                <DetailField label="project" value=e.project_id.clone().unwrap_or("-".into()) />
                <DetailField label=t.capture_col_session() value=e.session_fingerprint.clone().unwrap_or("-".into()) />
                <DetailField label=t.capture_col_backend() value=e.backend_name.clone().unwrap_or("-".into()) />
                <DetailField label=t.capture_col_wire() value={
                    e.client_wire_api.clone()
                        .or_else(|| e.client_path_suffix.clone())
                        .unwrap_or("-".into())
                } />
                <DetailField label="affinity" value=e.affinity_key.clone().unwrap_or("-".into()) />
                <DetailField label="client_key_fp" value=e.client_key_fingerprint.clone().unwrap_or("-".into()) />
                <DetailField label=t.capture_col_duration() value=e.duration_ms.to_string() />
                <DetailField label="ttft_ms" value=e.ttft_ms.map(|v| v.to_string()).unwrap_or("-".into()) />
                <DetailField label="cache" value=e.cache_tier.clone().unwrap_or("-".into()) />
                <DetailField label="coalesce" value={
                    match (e.coalesce_leader, e.coalesced_follower) {
                        (Some(true), _) => "leader".into(),
                        (_, true) => "follower".into(),
                        _ => "-".into(),
                    }
                } />
                <DetailField label="request_id" value=request_id_short />
                <DetailField label=t.capture_meta_stream() value=stream_val.into() />
                <DetailField label=t.capture_meta_reasoning_strategy() value=reasoning_val.into() />
                {e.request_hash.as_ref().map(|h| view! {
                    <DetailField label="request_hash" value=h.clone() />
                })}
            </div>

            // ── Structure diff ──
            <div class="diagnostics-section">
                <h4 class="text-xs font-semibold text-theme mb-1">{t.capture_section_structure()}</h4>
                <table class="table table-xs w-full">
                    <thead>
                        <tr>
                            <th></th>
                            <th class="text-xs text-theme-secondary">{t.capture_structure_client()}</th>
                            <th class="text-xs text-theme-secondary">{t.capture_structure_upstream()}</th>
                            <th class="text-xs text-theme-secondary">{t.capture_structure_delta()}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <StructRow
                            metric=t.capture_row_messages()
                            client=client.message_count.to_string()
                            upstream=upstream.message_count.to_string()
                            delta=fmt_delta(diff.delta_message_count as i64)
                            warn=delta_msg_warn
                        />
                        <StructRow
                            metric=t.capture_row_content_chars()
                            client=client.total_content_chars.to_string()
                            upstream=upstream.total_content_chars.to_string()
                            delta=fmt_delta(diff.delta_content_chars)
                            warn=delta_content_warn
                        />
                        <StructRow
                            metric=t.capture_row_reasoning_chars()
                            client=client.total_reasoning_content_chars.to_string()
                            upstream=upstream.total_reasoning_content_chars.to_string()
                            delta=fmt_delta(diff.delta_reasoning_chars)
                            warn=delta_reasoning_warn
                        />
                        <StructRow
                            metric=t.capture_row_system_chars()
                            client=client.system_chars.to_string()
                            upstream=upstream.system_chars.to_string()
                            delta=fmt_delta(diff.delta_system_chars)
                            warn=false
                        />
                        <StructRow
                            metric=t.capture_row_tool_count()
                            client=client.tool_count.to_string()
                            upstream=upstream.tool_count.to_string()
                            delta=fmt_delta(diff.delta_tool_count as i64)
                            warn=false
                        />
                    </tbody>
                </table>
            </div>

            // ── Bodies ──
            <div class="space-y-2">
                <h4 class="text-xs font-semibold text-theme">{t.capture_section_bodies()}</h4>
                {match (&detail.client_body, &detail.upstream_body) {
                    (None, None) => view! {
                        <div class="text-xs text-theme-muted italic px-2 py-4">{t.capture_no_body()}</div>
                    }.into_any(),
                    (client_opt, upstream_opt) => {
                        let c_text = client_opt.as_deref().unwrap_or("");
                        let u_text = upstream_opt.as_deref().unwrap_or("");
                        let has_both = !c_text.is_empty() && !u_text.is_empty();

                        if has_both {
                            // Side-by-side on wide, tabs on narrow.
                            let c_text_wide = c_text.to_string();
                            let u_text_wide = u_text.to_string();
                            let c_text_narrow = c_text.to_string();
                            let u_text_narrow = u_text.to_string();
                            view! {
                                // Wide: side-by-side
                                <div class="hidden md:grid md:grid-cols-2 md:gap-2">
                                    <div>
                                        <div class="text-xs text-theme-secondary mb-1">{t.capture_client_body()}</div>
                                        <pre class="logs-pre">{c_text_wide}</pre>
                                    </div>
                                    <div>
                                        <div class="text-xs text-theme-secondary mb-1">{t.capture_upstream_body()}</div>
                                        <pre class="logs-pre">{u_text_wide}</pre>
                                    </div>
                                </div>
                                // Narrow: tabs
                                <div class="md:hidden">
                                    <div class="capture-body-tabs">
                                        <button
                                            class=move || if body_tab.get() == 0 { "capture-body-tab active" } else { "capture-body-tab" }
                                            on:click=move |_| body_tab.set(0)
                                        >
                                            {t.capture_client_body()}
                                        </button>
                                        <button
                                            class=move || if body_tab.get() == 1 { "capture-body-tab active" } else { "capture-body-tab" }
                                            on:click=move |_| body_tab.set(1)
                                        >
                                            {t.capture_upstream_body()}
                                        </button>
                                    </div>
                                    {move || {
                                        let text = if body_tab.get() == 0 { c_text_narrow.clone() } else { u_text_narrow.clone() };
                                        view! { <pre class="logs-pre">{text}</pre> }.into_any()
                                    }}
                                </div>
                            }.into_any()
                        } else {
                            let label = if !c_text.is_empty() { t.capture_client_body() } else { t.capture_upstream_body() };
                            let text = if !c_text.is_empty() { c_text.to_string() } else { u_text.to_string() };
                            view! {
                                <div>
                                    <div class="text-xs text-theme-secondary mb-1">{label}</div>
                                    <pre class="logs-pre">{text}</pre>
                                </div>
                            }.into_any()
                        }
                    }
                }}
            </div>
        </div>
    }
}

// ── Detail field ─────────────────────────────────────────────────────

#[component]
fn DetailField(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="detail-field">
            <div class="detail-field-label">{label}</div>
            <div class="detail-field-value">{value}</div>
        </div>
    }
}

// ── Structure diff table row ─────────────────────────────────────────

#[component]
fn StructRow(
    metric: &'static str,
    client: String,
    upstream: String,
    delta: String,
    warn: bool,
) -> impl IntoView {
    let row_class = if warn {
        "capture-structure-row-warn"
    } else {
        ""
    };
    let delta_class = if warn {
        "capture-delta-warn font-semibold"
    } else {
        "text-theme-secondary"
    };
    view! {
        <tr class=row_class>
            <td class="text-xs text-theme-secondary">{metric}</td>
            <td class="text-xs font-mono tabular-nums">{client}</td>
            <td class="text-xs font-mono tabular-nums">{upstream}</td>
            <td class=format!("text-xs font-mono tabular-nums {delta_class}")>{delta}</td>
        </tr>
    }
}
