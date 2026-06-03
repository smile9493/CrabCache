//! Translate OpenAI Responses API wire (`POST /v1/responses`) to Chat Completions for
//! DeepSeek and MiMo upstreams. **Codex OAuth (`CodexRelay`) is passthrough — no translation here.**

use bytes::Bytes;
use crab_pipeline::RequestPipeline;
use http::{self, Uri};
use pingora_http::RequestHeader;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::context::{ClientWireApi, GatewayContext};
use crate::responses_chain_store::ResponsesChainStore;
use crate::sse::parse_sse_chunk;

/// Upstream path for OpenAI-compatible chat backends (DeepSeek, MiMo, …).
const CHAT_COMPLETIONS_UPSTREAM_PATH: &str = "/v1/chat/completions";
const MIXED_MODE_REASONING_PLACEHOLDER: &str = "(this turn ran without thinking mode)";

/// Three independent data-plane profiles — never share session sanitizers or chain namespaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsesWireProfile {
    /// `CodexRelay` → chatgpt.com OAuth pool; native Responses passthrough.
    Codex,
    /// `CodexDeepSeek` / `CursorDeepSeekV4` / `DeepSeekLight` → api.deepseek.com.
    DeepSeek,
    /// `CodexMimo` / `MimoTokenPlanRelay` / `MimoPaygRelay` → MiMo OpenAI-compatible API.
    Mimo,
}

impl ResponsesWireProfile {
    pub fn chain_namespace(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::DeepSeek => "deepseek",
            Self::Mimo => "mimo",
        }
    }
}

pub fn responses_wire_profile(ctx: &GatewayContext) -> Option<ResponsesWireProfile> {
    match ctx.request_pipeline {
        Some(RequestPipeline::CodexRelay) => Some(ResponsesWireProfile::Codex),
        Some(
            RequestPipeline::CodexDeepSeek
            | RequestPipeline::CursorDeepSeekV4
            | RequestPipeline::DeepSeekLight,
        ) => Some(ResponsesWireProfile::DeepSeek),
        Some(
            RequestPipeline::CodexMimo | RequestPipeline::MimoTokenPlanRelay,
        ) => Some(ResponsesWireProfile::Mimo),
        _ => None,
    }
}

/// Message sanitization when converting Responses `input[]` → Chat `messages[]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsesWireTarget {
    /// DeepSeek upstream: strip dangling assistant `tool_calls` only (keep tool messages).
    DeepSeek,
    /// MiMo upstream: orphan tool filtering (session store may sanitize again downstream).
    Mimo,
}

pub fn responses_wire_target(ctx: &GatewayContext) -> ResponsesWireTarget {
    match responses_wire_profile(ctx) {
        Some(ResponsesWireProfile::Mimo) => ResponsesWireTarget::Mimo,
        _ => ResponsesWireTarget::DeepSeek,
    }
}

pub fn responses_chain_namespace(ctx: &GatewayContext) -> &'static str {
    responses_wire_profile(ctx)
        .map(ResponsesWireProfile::chain_namespace)
        .unwrap_or("other")
}

fn namespaced_chain_id(namespace: &str, response_id: &str) -> String {
    format!("{namespace}:{response_id}")
}

pub fn needs_responses_wire_translate(ctx: &GatewayContext) -> bool {
    match responses_wire_profile(ctx) {
        // Codex OAuth pool: never translate; upstream speaks Responses natively.
        Some(ResponsesWireProfile::Codex) => false,
        Some(ResponsesWireProfile::DeepSeek) => {
            ctx.request_pipeline == Some(RequestPipeline::CodexDeepSeek)
                || ctx.client_wire_api == ClientWireApi::Responses
        }
        Some(ResponsesWireProfile::Mimo) => ctx.client_wire_api == ClientWireApi::Responses,
        None => {
            ctx.client_wire_api == ClientWireApi::Responses
                && ctx.request_pipeline != Some(RequestPipeline::CodexRelay)
        }
    }
}

/// Rewrite upstream request URI from client `/v1/responses` to Chat Completions.
pub fn apply_responses_wire_upstream_request(req: &mut RequestHeader) {
    if let Ok(uri) = CHAT_COMPLETIONS_UPSTREAM_PATH.parse::<Uri>() {
        req.set_uri(uri);
    }
}

/// Expand Codex `previous_response_id` chains using gateway-stored prior `output[]`.
pub async fn apply_responses_chain(
    payload: &mut Value,
    store: &ResponsesChainStore,
    chain_namespace: &str,
) {
    let Some(prev_id) = payload
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let store_key = namespaced_chain_id(chain_namespace, prev_id);
    let Some(prev_output) = store.get(&store_key).await else {
        tracing::debug!(
            previous_response_id = prev_id,
            chain_namespace,
            "responses chain store miss — follow-up may lack prior tool context"
        );
        return;
    };
    let prev_inputs: Vec<Value> = prev_output
        .iter()
        .filter_map(responses_output_item_to_input)
        .collect();
    let mut merged_input = prev_inputs.clone();
    match payload.get("input") {
        Some(Value::String(text)) if !text.is_empty() => {
            merged_input.push(json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text }],
            }));
        }
        Some(Value::Array(items)) if items.is_empty() => {}
        Some(Value::Array(items)) => {
            // Codex normally sends only the delta after `previous_response_id`. If the client
            // resends a full transcript, avoid duplicating the stored chain (OmniRoute #1729).
            if items.len() > prev_inputs.len().saturating_add(2) {
                merged_input = items.clone();
            } else {
                merged_input.extend(items.iter().cloned());
            }
        }
        _ => {}
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("input".into(), Value::Array(merged_input));
        obj.remove("previous_response_id");
    }
}

fn responses_output_item_to_input(item: &Value) -> Option<Value> {
    match item.get("type").and_then(|t| t.as_str())? {
        "message" => Some(json!({
            "type": "message",
            "role": item.get("role").and_then(|v| v.as_str()).unwrap_or("assistant"),
            "content": item.get("content").cloned().unwrap_or(Value::Array(vec![])),
        })),
        "function_call" => Some(json!({
            "type": "function_call",
            "call_id": item.get("call_id").cloned().unwrap_or(Value::String(String::new())),
            "name": item.get("name").cloned().unwrap_or(Value::Null),
            "arguments": item
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}"),
        })),
        "reasoning" => Some(json!({
            "type": "reasoning",
            "id": item.get("id").cloned().unwrap_or(Value::Null),
            "summary": item.get("summary").cloned().unwrap_or(Value::Array(vec![])),
            "encrypted_content": item.get("encrypted_content").cloned().unwrap_or(Value::Null),
        })),
        _ => None,
    }
}

/// Persist completed Responses `output[]` for follow-up requests using `previous_response_id`.
pub fn store_responses_chain_output(
    store: &ResponsesChainStore,
    chain_namespace: &str,
    response_id: &str,
    output: Vec<Value>,
) {
    if response_id.is_empty() {
        return;
    }
    store.put(&namespaced_chain_id(chain_namespace, response_id), output);
}

pub fn store_responses_chain_output_for_ctx(
    store: &ResponsesChainStore,
    ctx: &GatewayContext,
    response_id: &str,
    output: Vec<Value>,
) {
    store_responses_chain_output(store, responses_chain_namespace(ctx), response_id, output);
}

/// Convert a Responses API request body into Chat Completions JSON for upstream relay.
/// Aligned with OmniRoute `openai-responses.ts` (turn grouping, input sanitization).
pub fn responses_payload_to_chat_completions(payload: &Value) -> Value {
    responses_payload_to_chat_completions_for(payload, ResponsesWireTarget::DeepSeek)
}

/// Pipeline-specific Responses → Chat Completions conversion (DeepSeek vs MiMo).
pub fn responses_payload_to_chat_completions_for(
    payload: &Value,
    target: ResponsesWireTarget,
) -> Value {
    if payload
        .get("messages")
        .and_then(|m| m.as_array())
        .is_some_and(|a| !a.is_empty())
    {
        return payload.clone();
    }

    let Some(root) = payload.as_object() else {
        return payload.clone();
    };

    let mut out: Map<String, Value> = root.clone();
    let mut messages: Vec<Value> = Vec::new();

    if let Some(instr) = payload
        .get("instructions")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        messages.push(json!({ "role": "system", "content": instr }));
    }

    match payload.get("input") {
        Some(Value::String(text)) if !text.is_empty() => {
            messages.push(json!({ "role": "user", "content": text }));
        }
        Some(Value::Array(items)) => {
            let sanitized = sanitize_responses_input_items(items);
            messages.extend(convert_responses_input_to_messages(&sanitized));
        }
        _ => {}
    }

    messages = finalize_responses_messages_for_target(messages, target);

    // Responses API fields that Chat Completions backends reject or ignore.
    out.remove("input");
    out.remove("instructions");
    out.remove("previous_response_id");
    out.remove("prompt");
    out.remove("include");
    out.remove("background");
    out.remove("store");
    out.remove("reasoning");
    out.remove("safety_identifier");
    out.insert("messages".into(), Value::Array(messages));

    if let Some(max_out) = payload.get("max_output_tokens") {
        out.insert("max_tokens".into(), max_out.clone());
        out.remove("max_output_tokens");
    } else if let Some(mct) = payload.get("max_completion_tokens") {
        out.insert("max_tokens".into(), mct.clone());
        out.remove("max_completion_tokens");
    }

    if let Some(tools) = payload.get("tools").and_then(|v| v.as_array()) {
        out.insert(
            "tools".into(),
            Value::Array(convert_tools_responses_to_chat(tools)),
        );
    }

    if let Some(tc) = out.get("tool_choice").and_then(|v| v.as_object()).cloned() {
        if tc.get("type").and_then(|t| t.as_str()) == Some("function")
            && tc.get("name").is_some()
            && tc.get("function").is_none()
        {
            out.insert(
                "tool_choice".into(),
                json!({
                    "type": "function",
                    "function": { "name": tc.get("name").cloned().unwrap_or(Value::Null) },
                }),
            );
        }
    }

    crab_reasoning::ensure_codex_file_tools_from_context(&mut out);
    Value::Object(out)
}

fn finalize_responses_messages_for_target(
    messages: Vec<Value>,
    target: ResponsesWireTarget,
) -> Vec<Value> {
    match target {
        ResponsesWireTarget::DeepSeek => strip_dangling_assistant_tool_calls(&messages),
        ResponsesWireTarget::Mimo => {
            let filtered = filter_orphaned_tool_messages(&messages);
            backfill_missing_assistant_reasoning(filtered)
        }
    }
}

/// Drop Codex internal runtime frames (e.g. `phase: commentary`) before upstream relay.
fn sanitize_responses_input_items(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .filter(|item| !is_internal_assistant_message(item))
        .map(sanitize_responses_input_item_ids)
        .collect()
}

fn is_internal_assistant_message(item: &Value) -> bool {
    let Some(obj) = item.as_object() else {
        return false;
    };
    let item_type = match obj.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None if obj.get("role").is_some() => "message",
        _ => return false,
    };
    if item_type != "message" {
        return false;
    }
    if obj.get("role").and_then(|r| r.as_str()) != Some("assistant") {
        return false;
    }
    let phase = obj
        .get("phase")
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    phase == "commentary"
}

fn sanitize_function_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(128)
        .collect()
}

fn sanitize_responses_input_item_ids(item: &Value) -> Value {
    let Some(obj) = item.as_object() else {
        return item.clone();
    };
    let item_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let mut next = obj.clone();
    if (item_type == "function_call" || item_type == "function_call_output")
        && let Some(name) = next.get("name").and_then(|v| v.as_str())
        && (!name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            || name.len() > 128)
    {
        next.insert("name".into(), Value::String(sanitize_function_name(name)));
    }
    let Some(id) = next.get("id").and_then(|v| v.as_str()) else {
        return Value::Object(next);
    };
    let expected_prefix = match item_type {
        "function_call" => "fc_",
        "message" => "msg_",
        "reasoning" => "rs_",
        _ => "",
    };
    if expected_prefix.is_empty() || id.starts_with(expected_prefix) {
        return Value::Object(next);
    }
    next.remove("id");
    Value::Object(next)
}

fn reasoning_item_text(item: &Value) -> String {
    if let Some(text) = item
        .get("encrypted_content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return text.to_string();
    }
    item.get("summary")
        .and_then(|v| v.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter(|part| part.get("type").and_then(|v| v.as_str()) == Some("summary_text"))
                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn append_reasoning_to_assistant(current: &mut Option<Map<String, Value>>, text: String) {
    if text.is_empty() {
        return;
    }
    let entry = current.get_or_insert_with(|| {
        let mut m = Map::new();
        m.insert("role".into(), json!("assistant"));
        m.insert("content".into(), Value::String(String::new()));
        m
    });
    let merged = entry
        .get("reasoning_content")
        .and_then(|v| v.as_str())
        .map(|existing| format!("{existing}{text}"))
        .unwrap_or(text);
    entry.insert("reasoning_content".into(), Value::String(merged));
}

fn backfill_missing_assistant_reasoning(mut messages: Vec<Value>) -> Vec<Value> {
    for msg in &mut messages {
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        if msg
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            continue;
        }
        if let Some(obj) = msg.as_object_mut() {
            obj.insert(
                "reasoning_content".into(),
                Value::String(MIXED_MODE_REASONING_PLACEHOLDER.to_string()),
            );
        }
    }
    messages
}

/// Group `function_call` items into assistant `tool_calls` turns (OmniRoute-compatible).
fn convert_responses_input_to_messages(items: &[Value]) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut current_assistant: Option<Map<String, Value>> = None;

    let flush_assistant = |messages: &mut Vec<Value>, current: &mut Option<Map<String, Value>>| {
        if let Some(msg) = current.take() {
            messages.push(Value::Object(msg));
        }
    };

    for item in items {
        let item_type = item
            .get("type")
            .and_then(|t| t.as_str())
            .or_else(|| item.get("role").map(|_| "message"));

        match item_type {
            Some("reasoning") => {
                append_reasoning_to_assistant(&mut current_assistant, reasoning_item_text(item));
            }
            Some("message") => {
                let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let content = responses_content_to_chat(item.get("content"), role);
                if role == "assistant" {
                    if current_assistant
                        .as_ref()
                        .and_then(|m| m.get("content"))
                        .is_some()
                    {
                        flush_assistant(&mut messages, &mut current_assistant);
                    }
                    let entry = current_assistant.get_or_insert_with(|| {
                        let mut m = Map::new();
                        m.insert("role".into(), json!("assistant"));
                        m
                    });
                    entry.insert("content".into(), content);
                } else {
                    flush_assistant(&mut messages, &mut current_assistant);
                    if content_is_nonempty(&content) {
                        messages.push(json!({ "role": role, "content": content }));
                    }
                }
            }
            Some("function_call") => {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if name.is_empty() {
                    continue;
                }
                let call_id = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = function_call_arguments_to_string(item.get("arguments"));
                let entry = current_assistant.get_or_insert_with(|| {
                    let mut m = Map::new();
                    m.insert("role".into(), json!("assistant"));
                    m.insert("content".into(), Value::Null);
                    m.insert("tool_calls".into(), json!([]));
                    m
                });
                if !entry.get("tool_calls").and_then(|v| v.as_array()).is_some() {
                    entry.insert("tool_calls".into(), json!([]));
                }
                if let Some(arr) = entry.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                    arr.push(json!({
                        "id": call_id,
                        "type": "function",
                        "function": { "name": name, "arguments": arguments },
                    }));
                }
            }
            Some("function_call_output") => {
                flush_assistant(&mut messages, &mut current_assistant);
                let output = item
                    .get("output")
                    .map(content_value_to_string)
                    .unwrap_or_default();
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
                    "content": output,
                }));
            }
            _ => {}
        }
    }

    flush_assistant(&mut messages, &mut current_assistant);
    messages
}

fn function_call_arguments_to_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

pub(crate) fn filter_orphaned_tool_messages(messages: &[Value]) -> Vec<Value> {
    let mut call_ids = std::collections::HashSet::new();
    for msg in messages {
        if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tcs {
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    call_ids.insert(id.to_string());
                }
            }
        }
    }
    messages
        .iter()
        .filter(|msg| {
            if msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
                msg.get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| call_ids.contains(id))
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

/// Drop assistant `tool_calls` whose `tool_call_id` has no matching `tool` message later in the array.
pub(crate) fn strip_dangling_assistant_tool_calls(messages: &[Value]) -> Vec<Value> {
    let tool_ids: std::collections::HashSet<String> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("tool"))
        .filter_map(|m| m.get("tool_call_id").and_then(|v| v.as_str()))
        .map(str::to_string)
        .collect();
    messages
        .iter()
        .cloned()
        .map(|mut msg| {
            if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                return msg;
            }
            let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()).cloned() else {
                return msg;
            };
            let kept: Vec<Value> = tcs
                .into_iter()
                .filter(|tc| {
                    tc.get("id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| tool_ids.contains(id))
                })
                .collect();
            if kept.is_empty() {
                if let Some(obj) = msg.as_object_mut() {
                    obj.remove("tool_calls");
                }
            } else if let Some(obj) = msg.as_object_mut() {
                obj.insert("tool_calls".into(), Value::Array(kept));
            }
            msg
        })
        .collect()
}

pub(crate) fn sanitize_tool_message_chain(messages: Vec<Value>) -> Vec<Value> {
    let stripped = strip_dangling_assistant_tool_calls(&messages);
    filter_orphaned_tool_messages(&stripped)
}

fn responses_content_to_chat(content: Option<&Value>, role: &str) -> Value {
    match content {
        None | Some(Value::Null) => Value::String(String::new()),
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Array(parts)) => {
            let mut out_parts = Vec::new();
            for part in parts {
                match part {
                    Value::String(s) if !s.is_empty() => {
                        out_parts.push(json!({ "type": "text", "text": s }));
                    }
                    Value::Object(obj) => {
                        let part_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        let text = obj
                            .get("text")
                            .or_else(|| obj.get("content"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let mapped = if part_type == "output_text" || role == "assistant" {
                            "text"
                        } else {
                            "text"
                        };
                        out_parts.push(json!({ "type": mapped, "text": text }));
                    }
                    _ => {}
                }
            }
            if out_parts.len() == 1
                && out_parts[0].get("type").and_then(|t| t.as_str()) == Some("text")
            {
                Value::String(
                    out_parts[0]
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string(),
                )
            } else {
                Value::Array(out_parts)
            }
        }
        Some(other) => other.clone(),
    }
}

fn content_value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .or_else(|| item.get("content"))
                    .and_then(|t| t.as_str())
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => value.to_string(),
    }
}

fn content_is_nonempty(content: &Value) -> bool {
    match content {
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

fn convert_tools_responses_to_chat(tools: &[Value]) -> Vec<Value> {
    crab_reasoning::normalize_codex_tools_for_upstream(tools)
}

/// Per-tool-call accumulator (mirrors OmniRoute `responsesTransformer.ts` state).
struct ToolCallState {
    call_id: String,
    name: String,
    args_buf: String,
    item_added: bool,
    item_done: bool,
}

/// Stateful translator: Chat Completions SSE → Responses API SSE (OmniRoute-compatible lifecycle).
pub struct ChatToResponsesSseTranslator {
    seq: u64,
    response_id: String,
    created_at: i64,
    started: bool,
    completed_sent: bool,
    msg_index: i32,
    msg_text: String,
    msg_item_added: bool,
    msg_content_added: bool,
    msg_item_done: bool,
    reasoning_id: Option<String>,
    reasoning_index: i32,
    reasoning_text: String,
    reasoning_part_added: bool,
    reasoning_done: bool,
    in_thinking: bool,
    tool_calls: HashMap<u32, ToolCallState>,
    pending_usage: Option<Value>,
    model: String,
    last_emit_at: Option<Instant>,
    /// Incomplete upstream SSE line buffered across TCP chunks (MiMo passthrough splits).
    upstream_sse_remainder: Vec<u8>,
    done_marker_sent: bool,
    /// Remap MiMo `apply_patch`/`read_file`/`list_dir` → `exec_command` for Codex exec-only clients.
    exec_only_surface: bool,
}

const RESPONSES_SSE_KEEPALIVE: Duration = Duration::from_secs(2);

impl ChatToResponsesSseTranslator {
    pub fn new(model: &str) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Self {
            seq: 0,
            response_id: format!("resp_{}", uuid::Uuid::new_v4().simple()),
            created_at,
            started: false,
            completed_sent: false,
            msg_index: 0,
            msg_text: String::new(),
            msg_item_added: false,
            msg_content_added: false,
            msg_item_done: false,
            reasoning_id: None,
            reasoning_index: -1,
            reasoning_text: String::new(),
            reasoning_part_added: false,
            reasoning_done: false,
            in_thinking: false,
            tool_calls: HashMap::new(),
            pending_usage: None,
            model: model.to_string(),
            last_emit_at: None,
            upstream_sse_remainder: Vec::new(),
            done_marker_sent: false,
            exec_only_surface: false,
        }
    }

    pub fn set_exec_only_surface(&mut self, exec_only: bool) {
        self.exec_only_surface = exec_only;
    }

    fn normalize_downstream_tool_name(&self, name: &str) -> String {
        if self.exec_only_surface
            && crab_reasoning::CODEX_FILE_TOOL_NAMES
                .iter()
                .any(|n| *n == name)
        {
            "exec_command".to_string()
        } else {
            name.to_string()
        }
    }

    pub fn response_id(&self) -> &str {
        &self.response_id
    }

    pub fn is_completed(&self) -> bool {
        self.completed_sent
    }

    pub(crate) fn upstream_sse_remainder_len(&self) -> usize {
        self.upstream_sse_remainder.len()
    }

    pub(crate) fn done_marker_sent(&self) -> bool {
        self.done_marker_sent
    }

    /// Emit `response.created` + `response.in_progress` before upstream body (Codex prefill keepalive).
    pub fn bootstrap_stream(&mut self) -> Vec<u8> {
        if self.started {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.started = true;
        self.emit(
            &mut out,
            json!({
                "type": "response.created",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "created_at": self.created_at,
                    "status": "in_progress",
                    "background": false,
                    "error": null,
                    "model": self.model,
                    "output": [],
                }
            }),
        );
        self.emit(
            &mut out,
            json!({
                "type": "response.in_progress",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "created_at": self.created_at,
                    "status": "in_progress",
                    "model": self.model,
                }
            }),
        );
        out
    }

    pub fn completed_output(&self) -> Vec<Value> {
        let mut output: Vec<Value> = Vec::new();
        if let Some(rs_id) = &self.reasoning_id {
            output.push(json!({
                "id": rs_id,
                "type": "reasoning",
                "summary": [{ "type": "summary_text", "text": self.reasoning_text }],
                "encrypted_content": self.reasoning_text,
            }));
        }
        if self.msg_item_added {
            output.push(json!({
                "id": self.msg_id(self.msg_index),
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "annotations": [], "text": self.msg_text }],
            }));
        }
        for entry in self.tool_calls.values() {
            if entry.call_id.is_empty() {
                continue;
            }
            output.push(json!({
                "id": format!("fc_{}", entry.call_id),
                "type": "function_call",
                "call_id": entry.call_id,
                "name": entry.name,
                "arguments": if entry.args_buf.is_empty() { "{}" } else { &entry.args_buf },
            }));
        }
        output
    }

    pub fn translate_chunk(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.upstream_sse_remainder.extend_from_slice(chunk);
        let mut out = Vec::new();
        self.maybe_emit_keepalive(&mut out);
        while let Some(pos) = self.upstream_sse_remainder.iter().position(|b| *b == b'\n') {
            let line_with_nl: Vec<u8> = self.upstream_sse_remainder.drain(..=pos).collect();
            let mut line = line_with_nl.as_slice();
            if line.ends_with(b"\n") {
                line = &line[..line.len() - 1];
            }
            if line.ends_with(b"\r") {
                line = &line[..line.len() - 1];
            }
            if line.is_empty() {
                continue;
            }
            out.extend(self.translate_sse_line(line));
        }
        out
    }

    /// Flush a trailing upstream line without a newline when upstream closes.
    pub fn flush_upstream_remainder(&mut self) -> Vec<u8> {
        if self.upstream_sse_remainder.is_empty() {
            return Vec::new();
        }
        let tail = std::mem::take(&mut self.upstream_sse_remainder);
        self.translate_sse_line(&tail)
    }

    fn translate_sse_line(&mut self, line: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for event in parse_sse_chunk(line) {
            if event.is_done() {
                self.close_all(&mut out);
                self.emit_completed(&mut out);
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(event.data) else {
                continue;
            };
            if let Some(usage) = value.get("usage") {
                self.pending_usage = Some(usage.clone());
            }
            let Some(choices) = value.get("choices").and_then(|c| c.as_array()) else {
                continue;
            };
            if choices.is_empty() {
                continue;
            }
            let choice = &choices[0];
            let idx = choice.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as i32;
            self.msg_index = idx;

            if !self.started {
                self.started = true;
                if let Some(id) = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    self.response_id = format!("resp_{id}");
                }
                self.emit(
                    &mut out,
                    json!({
                        "type": "response.created",
                        "response": {
                            "id": self.response_id,
                            "object": "response",
                            "created_at": self.created_at,
                            "status": "in_progress",
                            "background": false,
                            "error": null,
                            "model": self.model,
                            "output": [],
                        }
                    }),
                );
                self.emit(
                    &mut out,
                    json!({
                        "type": "response.in_progress",
                        "response": {
                            "id": self.response_id,
                            "object": "response",
                            "created_at": self.created_at,
                            "status": "in_progress",
                            "model": self.model,
                        }
                    }),
                );
            }

            let delta = choice.get("delta");
            if let Some(reasoning) = delta
                .and_then(|d| d.get("reasoning_content"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                self.start_reasoning(&mut out, idx);
                self.emit_reasoning_delta(&mut out, reasoning);
            }

            if let Some(mut content) = delta
                .and_then(|d| d.get("content"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                if content.contains("<think>") {
                    self.in_thinking = true;
                    content = content.replace("<think>", "");
                    self.start_reasoning(&mut out, idx);
                }
                if content.contains("</think>") {
                    let parts: Vec<&str> = content.splitn(2, "</think>").collect();
                    if !parts[0].is_empty() {
                        self.emit_reasoning_delta(&mut out, parts[0]);
                    }
                    self.close_reasoning(&mut out);
                    self.in_thinking = false;
                    content = parts.get(1).copied().unwrap_or("").to_string();
                }
                if self.in_thinking && !content.is_empty() {
                    self.emit_reasoning_delta(&mut out, &content);
                    continue;
                }
                if !content.is_empty() {
                    if self.msg_text.is_empty() {
                        content = content.trim_start().to_string();
                    }
                    if !content.is_empty() {
                        self.emit_text_delta(&mut out, idx, &content);
                    }
                }
            }

            if let Some(tool_calls) = delta
                .and_then(|d| d.get("tool_calls"))
                .and_then(|v| v.as_array())
            {
                self.close_message(&mut out, idx);
                for tc in tool_calls {
                    self.handle_tool_call_delta(&mut out, tc);
                }
            }

            if choice
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .is_some_and(|f| !f.is_empty() && f != "null")
            {
                self.close_all(&mut out);
                self.emit_completed(&mut out);
            }
        }
        out
    }

    pub fn flush(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        self.close_all(&mut out);
        if !self.completed_sent {
            self.emit_completed(&mut out);
        } else {
            out.extend(self.append_done_if_missing());
        }
        out
    }

    /// Append `data: [DONE]` when stream completed mid-chunk but marker not yet sent.
    pub fn append_done_if_missing(&mut self) -> Vec<u8> {
        if self.done_marker_sent {
            return Vec::new();
        }
        self.done_marker_sent = true;
        let mut out = Vec::new();
        append_done_marker(&mut out);
        out
    }

    /// Emit idle heartbeat when upstream stalls between SSE chunks (Codex CLI timeout guard).
    pub fn poll_keepalive(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        self.maybe_emit_keepalive(&mut out);
        out
    }

    fn maybe_emit_keepalive(&mut self, out: &mut Vec<u8>) {
        if !self.started || self.completed_sent {
            return;
        }
        let now = Instant::now();
        if self
            .last_emit_at
            .is_some_and(|last| now.duration_since(last) >= RESPONSES_SSE_KEEPALIVE)
        {
            // Codex CLI expects full Responses SSE blocks (`event:` + JSON), not bare `data:` lines.
            self.emit(
                out,
                json!({
                    "type": "response.in_progress",
                    "response": {
                        "id": self.response_id,
                        "object": "response",
                        "created_at": self.created_at,
                        "status": "in_progress",
                        "model": self.model,
                    }
                }),
            );
        }
    }

    fn touch_emit(&mut self) {
        self.last_emit_at = Some(Instant::now());
    }

    fn emit(&mut self, out: &mut Vec<u8>, mut data: Value) {
        self.seq += 1;
        if let Some(obj) = data.as_object_mut() {
            obj.insert("sequence_number".into(), json!(self.seq));
        }
        append_responses_event(out, data);
        self.touch_emit();
    }

    fn msg_id(&self, idx: i32) -> String {
        format!("msg_{}_{idx}", self.response_id)
    }

    fn start_reasoning(&mut self, out: &mut Vec<u8>, idx: i32) {
        if self.reasoning_id.is_some() {
            return;
        }
        let rs_id = format!("rs_{}_{idx}", self.response_id);
        self.reasoning_id = Some(rs_id.clone());
        self.reasoning_index = idx;
        self.emit(
            out,
            json!({
                "type": "response.output_item.added",
                "output_index": idx,
                "item": { "id": rs_id.clone(), "type": "reasoning", "summary": [] },
            }),
        );
        self.emit(
            out,
            json!({
                "type": "response.reasoning_summary_part.added",
                "item_id": rs_id,
                "output_index": idx,
                "summary_index": 0,
                "part": { "type": "summary_text", "text": "" },
            }),
        );
        self.reasoning_part_added = true;
    }

    fn emit_reasoning_delta(&mut self, out: &mut Vec<u8>, text: &str) {
        if text.is_empty() {
            return;
        }
        self.start_reasoning(out, self.msg_index);
        self.reasoning_text.push_str(text);
        let Some(rs_id) = self.reasoning_id.clone() else {
            return;
        };
        self.emit(
            out,
            json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": rs_id,
                "output_index": self.reasoning_index,
                "summary_index": 0,
                "delta": text,
            }),
        );
    }

    fn close_reasoning(&mut self, out: &mut Vec<u8>) {
        if self.reasoning_done || self.reasoning_id.is_none() {
            return;
        }
        self.reasoning_done = true;
        let rs_id = self.reasoning_id.clone().unwrap_or_default();
        let idx = self.reasoning_index;
        let text = self.reasoning_text.clone();
        self.emit(
            out,
            json!({
                "type": "response.reasoning_summary_text.done",
                "item_id": rs_id,
                "output_index": idx,
                "summary_index": 0,
                "text": text,
            }),
        );
        self.emit(
            out,
            json!({
                "type": "response.reasoning_summary_part.done",
                "item_id": rs_id,
                "output_index": idx,
                "summary_index": 0,
                "part": { "type": "summary_text", "text": text },
            }),
        );
        self.emit(
            out,
            json!({
                "type": "response.output_item.done",
                "output_index": idx,
                "item": {
                    "id": rs_id,
                    "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": text }],
                },
            }),
        );
    }

    fn emit_text_delta(&mut self, out: &mut Vec<u8>, idx: i32, content: &str) {
        let msg_id = self.msg_id(idx);
        if !self.msg_item_added {
            self.msg_item_added = true;
            self.emit(
                out,
                json!({
                    "type": "response.output_item.added",
                    "output_index": idx,
                    "item": { "id": msg_id, "type": "message", "content": [], "role": "assistant" },
                }),
            );
        }
        if !self.msg_content_added {
            self.msg_content_added = true;
            self.emit(
                out,
                json!({
                    "type": "response.content_part.added",
                    "item_id": msg_id,
                    "output_index": idx,
                    "content_index": 0,
                    "part": { "type": "output_text", "annotations": [], "logprobs": [], "text": "" },
                }),
            );
        }
        self.emit(
            out,
            json!({
                "type": "response.output_text.delta",
                "item_id": msg_id,
                "output_index": idx,
                "content_index": 0,
                "delta": content,
                "logprobs": [],
            }),
        );
        self.msg_text.push_str(content);
    }

    fn close_message(&mut self, out: &mut Vec<u8>, idx: i32) {
        if !self.msg_item_added || self.msg_item_done {
            return;
        }
        self.msg_item_done = true;
        let msg_id = self.msg_id(idx);
        let full_text = self.msg_text.clone();
        self.emit(
            out,
            json!({
                "type": "response.output_text.done",
                "item_id": msg_id,
                "output_index": idx,
                "content_index": 0,
                "text": full_text,
                "logprobs": [],
            }),
        );
        self.emit(
            out,
            json!({
                "type": "response.content_part.done",
                "item_id": msg_id,
                "output_index": idx,
                "content_index": 0,
                "part": { "type": "output_text", "annotations": [], "logprobs": [], "text": full_text },
            }),
        );
        self.emit(
            out,
            json!({
                "type": "response.output_item.done",
                "output_index": idx,
                "item": {
                    "id": msg_id,
                    "type": "message",
                    "content": [{ "type": "output_text", "annotations": [], "logprobs": [], "text": full_text }],
                    "role": "assistant",
                },
            }),
        );
    }

    fn handle_tool_call_delta(&mut self, out: &mut Vec<u8>, tc: &Value) {
        let tc_idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let new_call_id = tc
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let func_name = tc
            .pointer("/function/name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let args_delta = tc
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let normalized_name = func_name
            .as_ref()
            .map(|name| self.normalize_downstream_tool_name(name));

        if let Some(entry) = self.tool_calls.get(&tc_idx)
            && let Some(ref new_id) = new_call_id
            && entry.call_id != *new_id
        {
            self.close_tool_call(out, tc_idx);
            self.tool_calls.remove(&tc_idx);
        }

        let entry = self
            .tool_calls
            .entry(tc_idx)
            .or_insert_with(|| ToolCallState {
                call_id: String::new(),
                name: String::new(),
                args_buf: String::new(),
                item_added: false,
                item_done: false,
            });

        if let Some(name) = normalized_name {
            entry.name = name;
        }
        if entry.call_id.is_empty() {
            if let Some(id) = new_call_id {
                entry.call_id = id;
            }
        }
        if let Some(args) = &args_delta {
            entry.args_buf.push_str(args);
        }

        let add_item = !entry.item_added && !entry.call_id.is_empty();
        let add_payload = if add_item {
            entry.item_added = true;
            Some((entry.call_id.clone(), entry.name.clone(), tc_idx))
        } else {
            None
        };
        let emit_args = args_delta.filter(|_| !entry.call_id.is_empty());
        let call_id_for_delta = entry.call_id.clone();

        if let Some((call_id, name, idx)) = add_payload {
            self.emit(
                out,
                json!({
                    "type": "response.output_item.added",
                    "output_index": idx,
                    "item": {
                        "id": format!("fc_{call_id}"),
                        "type": "function_call",
                        "arguments": "",
                        "call_id": call_id,
                        "name": name,
                    },
                }),
            );
        }

        if let Some(args) = emit_args {
            self.emit(
                out,
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": format!("fc_{call_id_for_delta}"),
                    "output_index": tc_idx,
                    "delta": args,
                }),
            );
        }
    }

    fn close_tool_call(&mut self, out: &mut Vec<u8>, tc_idx: u32) {
        let Some(entry) = self.tool_calls.get_mut(&tc_idx) else {
            return;
        };
        if entry.item_done || entry.call_id.is_empty() {
            return;
        }
        entry.item_done = true;
        let call_id = entry.call_id.clone();
        let name = entry.name.clone();
        let args = if entry.args_buf.is_empty() {
            "{}".to_string()
        } else {
            clean_tool_call_arguments(&entry.args_buf)
        };
        self.emit(
            out,
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": format!("fc_{call_id}"),
                "output_index": tc_idx,
                "arguments": args,
            }),
        );
        self.emit(
            out,
            json!({
                "type": "response.output_item.done",
                "output_index": tc_idx,
                "item": {
                    "id": format!("fc_{call_id}"),
                    "type": "function_call",
                    "arguments": args,
                    "call_id": call_id,
                    "name": name,
                },
            }),
        );
    }

    fn close_all(&mut self, out: &mut Vec<u8>) {
        self.close_reasoning(out);
        self.close_message(out, self.msg_index);
        let indices: Vec<u32> = self.tool_calls.keys().copied().collect();
        for idx in indices {
            self.close_tool_call(out, idx);
        }
    }

    fn emit_completed(&mut self, out: &mut Vec<u8>) {
        if self.completed_sent {
            return;
        }
        self.completed_sent = true;

        let mut output: Vec<Value> = Vec::new();
        if let Some(rs_id) = &self.reasoning_id {
            output.push(json!({
                "id": rs_id,
                "type": "reasoning",
                "summary": [{ "type": "summary_text", "text": self.reasoning_text }],
                "encrypted_content": self.reasoning_text,
            }));
        }
        if self.msg_item_added {
            output.push(json!({
                "id": self.msg_id(self.msg_index),
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "annotations": [], "text": self.msg_text }],
            }));
        }
        for entry in self.tool_calls.values() {
            if entry.call_id.is_empty() {
                continue;
            }
            output.push(json!({
                "id": format!("fc_{}", entry.call_id),
                "type": "function_call",
                "call_id": entry.call_id,
                "name": entry.name,
                "arguments": if entry.args_buf.is_empty() { "{}" } else { &entry.args_buf },
            }));
        }

        let mut response = json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": "completed",
            "background": false,
            "error": null,
            "model": self.model,
            "output": output,
        });
        if let Some(usage) = self.pending_usage.take() {
            if let Some(obj) = response.as_object_mut() {
                obj.insert("usage".into(), chat_usage_to_responses(&usage));
            }
        }
        self.emit(
            out,
            json!({ "type": "response.completed", "response": response }),
        );
        if !self.done_marker_sent {
            append_done_marker(out);
            self.done_marker_sent = true;
        }
    }
}

pub fn chat_completions_json_to_responses(body: &Value, model: &str) -> Value {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
    let message = body.pointer("/choices/0/message");
    let content = message
        .and_then(|m| m.get("content"))
        .map(content_value_to_string)
        .unwrap_or_default();
    let mut output = vec![json!({
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": content }],
    })];
    if let Some(reasoning) = message
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        output.insert(
            0,
            json!({
                "type": "reasoning",
                "summary": [{ "type": "summary_text", "text": reasoning }],
                "encrypted_content": reasoning,
            }),
        );
    }
    if let Some(tool_calls) = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array())
    {
        for tc in tool_calls {
            output.push(json!({
                "type": "function_call",
                "call_id": tc.get("id").cloned().unwrap_or(Value::String(String::new())),
                "name": tc.pointer("/function/name").cloned().unwrap_or(Value::Null),
                "arguments": tc.pointer("/function/arguments").and_then(|v| v.as_str()).unwrap_or(""),
            }));
        }
    }
    json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(model),
        "status": "completed",
        "output": output,
        "usage": body.get("usage").map(chat_usage_to_responses).unwrap_or_else(|| json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
        })),
    })
}

pub fn chat_completions_bytes_to_responses(body: &[u8], model: &str) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    serde_json::to_vec(&chat_completions_json_to_responses(&value, model)).ok()
}

pub fn translate_client_bytes_for_responses_wire(
    translator: &mut Option<ChatToResponsesSseTranslator>,
    model: &str,
    bytes: Option<Bytes>,
) -> Option<Bytes> {
    let data = bytes?;
    if data.is_empty() {
        return None;
    }
    if translator.is_none() {
        let mut tr = ChatToResponsesSseTranslator::new(model);
        tr.bootstrap_stream();
        *translator = Some(tr);
    }
    let out = translator
        .as_mut()
        .expect("initialized")
        .translate_chunk(&data);
    if out.is_empty() {
        None
    } else {
        Some(Bytes::from(out))
    }
}

/// Initialize Responses SSE translator at upstream headers (prefill bootstrap for Codex CLI).
pub fn arm_responses_wire_stream(ctx: &mut GatewayContext, model: &str) {
    if ctx.stream.responses_translator.is_some() {
        return;
    }
    let exec_only = ctx.stream.responses_exec_only_surface;
    let mut tr = ChatToResponsesSseTranslator::new(model);
    tr.set_exec_only_surface(exec_only);
    let bootstrap = tr.bootstrap_stream();
    if !bootstrap.is_empty() {
        ctx.stream.responses_wire_bootstrap = Some(bootstrap.clone());
    }
    ctx.stream.responses_translator = Some(tr);
}

pub fn prepend_responses_wire_bootstrap(
    ctx: &mut GatewayContext,
    bytes: Option<Bytes>,
) -> Option<Bytes> {
    if ctx.stream.responses_wire_bootstrap_sent {
        return bytes;
    }
    let Some(bootstrap) = ctx.stream.responses_wire_bootstrap.take() else {
        return bytes;
    };
    ctx.stream.responses_wire_bootstrap_sent = true;
    let mut merged = bootstrap;
    if let Some(data) = bytes {
        merged.extend_from_slice(&data);
    }
    Some(Bytes::from(merged))
}

/// Send Responses SSE bootstrap immediately after upstream peer selection (before MiMo TTFB).
///
/// Keeps Codex CLI alive during long prefill on large chain-expanded bodies; enables the
/// Pingora keepalive tick (`response_written` must be set first).
pub async fn try_send_responses_wire_ttfb_prefill(
    session: &mut pingora_proxy::Session,
    ctx: &mut GatewayContext,
) -> bool {
    if !ctx.is_streaming
        || !needs_responses_wire_translate(ctx)
        || ctx.client_wire_api != ClientWireApi::Responses
        || ctx.stream.responses_wire_bootstrap_sent
        || session.response_written().is_some()
    {
        return false;
    }
    let model = ctx.model.clone();
    arm_responses_wire_stream(ctx, &model);
    let Some(bootstrap) = ctx.stream.responses_wire_bootstrap.take() else {
        return false;
    };
    ctx.stream.responses_wire_bootstrap_sent = true;
    ctx.stream.responses_ttfb_prefill_sent = true;

    let mut header = match pingora_http::ResponseHeader::build(http::StatusCode::OK, Some(8)) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let _ = header.insert_header(http::header::CONTENT_TYPE, "text/event-stream");
    let _ = header.insert_header(http::header::CACHE_CONTROL, "no-cache");
    let _ = header.insert_header("X-Accel-Buffering", "no");
    let _ = header.insert_header("x-request-id", ctx.request_id.clone());
    let _ = header.insert_header("x-cache-status", "miss");

    if session
        .write_response_header(Box::new(header), false)
        .await
        .is_err()
    {
        return false;
    }
    ctx.stream.client_sse_body.extend_from_slice(&bootstrap);
    if session
        .write_response_body(Some(bytes::Bytes::from(bootstrap.clone())), false)
        .await
        .is_err()
    {
        return false;
    }
    tracing::info!(
        request_id = %ctx.request_id,
        bytes = bootstrap.len(),
        "Responses wire TTFB prefill flushed before upstream headers"
    );
    true
}

/// Flush `response.created` / `response.in_progress` immediately after upstream headers (prefill).
pub fn take_early_responses_wire_bootstrap(ctx: &mut GatewayContext) -> Option<Vec<u8>> {
    if !ctx.is_streaming
        || !needs_responses_wire_translate(ctx)
        || ctx.stream.responses_wire_bootstrap_sent
        || ctx.upstream.http_status != Some(200)
    {
        return None;
    }
    let Some(bootstrap) = ctx.stream.responses_wire_bootstrap.take() else {
        return None;
    };
    ctx.stream.responses_wire_bootstrap_sent = true;
    ctx.stream.client_sse_body.extend_from_slice(&bootstrap);
    tracing::info!(
        request_id = %ctx.request_id,
        bytes = bootstrap.len(),
        "early Responses wire bootstrap flushed after upstream headers"
    );
    Some(bootstrap)
}

/// Timer-driven downstream keepalive while upstream is idle mid-stream.
pub fn poll_responses_wire_keepalive(ctx: &mut GatewayContext) -> Option<Vec<u8>> {
    if !ctx.is_streaming
        || !needs_responses_wire_translate(ctx)
        || ctx.upstream.http_status != Some(200)
    {
        return None;
    }
    if !ctx.stream.responses_wire_bootstrap_sent {
        let model = ctx.model.clone();
        if ctx.stream.responses_translator.is_none() {
            arm_responses_wire_stream(ctx, &model);
        }
        if let Some(bootstrap) = ctx.stream.responses_wire_bootstrap.take() {
            ctx.stream.responses_wire_bootstrap_sent = true;
            ctx.stream.client_sse_body.extend_from_slice(&bootstrap);
            tracing::debug!(
                request_id = %ctx.request_id,
                bytes = bootstrap.len(),
                "Responses wire bootstrap flushed on keepalive tick"
            );
            return Some(bootstrap);
        }
    }
    let translator = ctx.stream.responses_translator.as_mut()?;
    if translator.is_completed() {
        return None;
    }
    let out = translator.poll_keepalive();
    if out.is_empty() {
        None
    } else {
        ctx.stream.client_sse_body.extend_from_slice(&out);
        Some(out)
    }
}

/// Whether this Responses stream is eligible for graceful completion synthesis.
fn responses_stream_needs_completed_event(ctx: &GatewayContext) -> bool {
    !crate::sse::sse_bytes_contains_event(&ctx.stream.client_sse_body, "response.completed")
}

fn responses_stream_needs_done_marker(ctx: &GatewayContext) -> bool {
    !ctx.stream
        .client_sse_body
        .windows(6)
        .any(|w| w == b"[DONE]")
}

fn upstream_ok_for_graceful_responses_finalize(ctx: &GatewayContext) -> bool {
    match ctx.upstream.http_status {
        Some(status) if status >= 400 => false,
        Some(200) => true,
        None => {
            ctx.stream.responses_wire_bootstrap_sent
                || !ctx.stream.client_sse_body.is_empty()
                || ctx.stream.responses_translator.is_some()
        }
        Some(_) => true,
    }
}

/// Whether we should try to append a synthetic `response.completed` (MiMo mid-stream reset, etc.).
pub fn should_attempt_graceful_responses_finalize(ctx: &GatewayContext) -> bool {
    ctx.is_streaming
        && needs_responses_wire_translate(ctx)
        && ctx.client_wire_api == ClientWireApi::Responses
        && upstream_ok_for_graceful_responses_finalize(ctx)
        && (responses_stream_needs_completed_event(ctx) || responses_stream_needs_done_marker(ctx))
}

/// Whether a Codex Responses stream still needs a graceful `response.completed` tail.
pub fn should_graceful_finalize_responses_stream(ctx: &GatewayContext) -> bool {
    should_attempt_graceful_responses_finalize(ctx)
        && !ctx
            .stream
            .responses_translator
            .as_ref()
            .is_some_and(|t| t.is_completed())
}

/// Stream already sent `response.completed` + `[DONE]` but upstream aborted before downstream EOS.
pub fn needs_downstream_stream_finish(ctx: &GatewayContext) -> bool {
    ctx.is_streaming
        && needs_responses_wire_translate(ctx)
        && ctx.client_wire_api == ClientWireApi::Responses
        && ctx.stream.responses_wire_bootstrap_sent
        && !responses_stream_needs_completed_event(ctx)
        && !responses_stream_needs_done_marker(ctx)
}

fn append_synthetic_responses_completed(out: &mut Vec<u8>, ctx: &GatewayContext) {
    let model = ctx.model.as_str();
    let resp_id = ctx
        .stream
        .responses_translator
        .as_ref()
        .map(|t| t.response_id().to_string())
        .unwrap_or_else(|| format!("resp_{}", uuid::Uuid::new_v4().simple()));
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let completed = json!({
        "type": "response.completed",
        "response": {
            "id": resp_id,
            "object": "response",
            "created_at": created_at,
            "model": model,
            "status": "completed",
            "output": ctx.stream.responses_translator.as_ref().map(|t| t.completed_output()).unwrap_or_default(),
            "usage": { "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 },
        }
    });
    if let Ok(line) = serde_json::to_string(&completed) {
        out.extend_from_slice(format!("event: response.completed\ndata: {line}\n\n").as_bytes());
        out.extend_from_slice(b"data: [DONE]\n\n");
    }
}

/// Append synthetic `response.completed` (+ `[DONE]`) for an incomplete Responses SSE stream.
pub fn synthesize_responses_completed_tail(
    ctx: &mut GatewayContext,
    chain_store: &ResponsesChainStore,
) -> Option<Vec<u8>> {
    if !should_graceful_finalize_responses_stream(ctx) {
        return None;
    }
    let mut out = Vec::new();
    append_synthetic_responses_completed(&mut out, ctx);
    if out.is_empty() {
        return None;
    }
    ctx.stream.client_sse_body.extend_from_slice(&out);
    if let Some(translator) = ctx.stream.responses_translator.as_ref() {
        store_responses_chain_output_for_ctx(
            chain_store,
            ctx,
            translator.response_id(),
            translator.completed_output(),
        );
    }
    Some(out)
}

/// Merge synthetic completion into an existing downstream body chunk (EOS body filter path).
pub fn merge_graceful_responses_tail(
    ctx: &mut GatewayContext,
    chain_store: &ResponsesChainStore,
    existing: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let tail = synthesize_responses_completed_tail(ctx, chain_store)?;
    let mut merged = existing.map(<[u8]>::to_vec).unwrap_or_default();
    merged.extend_from_slice(&tail);
    Some(merged)
}

/// Build a final Responses SSE tail when upstream aborts before Pingora EOS (MiMo mid-stream reset).
pub fn build_graceful_responses_stream_tail(
    ctx: &mut GatewayContext,
    chain_store: &ResponsesChainStore,
) -> Option<Vec<u8>> {
    if !should_attempt_graceful_responses_finalize(ctx) {
        if needs_downstream_stream_finish(ctx) {
            return Some(Vec::new());
        }
        return None;
    }
    let chain_ns = responses_chain_namespace(ctx);
    let needs_completed = responses_stream_needs_completed_event(ctx);
    let mut out = Vec::new();
    if !ctx.stream.responses_wire_bootstrap_sent {
        if ctx.stream.responses_translator.is_none() {
            let model = ctx.model.clone();
            arm_responses_wire_stream(ctx, &model);
        }
        if let Some(bootstrap) = ctx.stream.responses_wire_bootstrap.take() {
            out.extend_from_slice(&bootstrap);
            ctx.stream.responses_wire_bootstrap_sent = true;
        }
    }
    if let Some(translator) = ctx.stream.responses_translator.as_mut() {
        out.extend_from_slice(&translator.flush_upstream_remainder());
        if needs_completed {
            out.extend_from_slice(&translator.flush());
        } else {
            out.extend_from_slice(&translator.append_done_if_missing());
        }
        if translator.is_completed() {
            store_responses_chain_output(
                chain_store,
                chain_ns,
                translator.response_id(),
                translator.completed_output(),
            );
        }
    }
    ctx.stream.client_sse_body.extend_from_slice(&out);
    if !crate::sse::sse_bytes_contains_event(&ctx.stream.client_sse_body, "response.completed") {
        let before = out.len();
        append_synthetic_responses_completed(&mut out, ctx);
        if out.len() > before {
            ctx.stream.client_sse_body.extend_from_slice(&out[before..]);
            if let Some(translator) = ctx.stream.responses_translator.as_ref() {
                store_responses_chain_output_for_ctx(
                    chain_store,
                    ctx,
                    translator.response_id(),
                    translator.completed_output(),
                );
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        tracing::warn!(
            request_id = %ctx.request_id,
            bytes = out.len(),
            http_status = ?ctx.upstream.http_status,
            "Upstream aborted Responses stream; synthesized completion tail"
        );
        Some(out)
    }
}

fn chat_usage_to_responses(usage: &Value) -> Value {
    let input = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| usage.get("input_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let output = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| usage.get("output_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let mut out = json!({
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(input + output),
    });
    if let Some(cached) = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .and_then(|v| v.as_u64())
        .filter(|&n| n > 0)
    {
        out["input_tokens_details"] = json!({ "cached_tokens": cached });
    }
    if let Some(reasoning) = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .or_else(|| usage.pointer("/output_tokens_details/reasoning_tokens"))
        .and_then(|v| v.as_u64())
        .filter(|&n| n > 0)
    {
        out["output_tokens_details"] = json!({ "reasoning_tokens": reasoning });
    }
    out
}

fn clean_tool_call_arguments(raw: &str) -> String {
    let Ok(mut parsed) = serde_json::from_str::<Value>(raw) else {
        return raw.to_string();
    };
    let Some(obj) = parsed.as_object_mut() else {
        return raw.to_string();
    };
    obj.retain(|_, v| {
        !(v.as_str().is_some_and(|s| s.is_empty()) || v.as_array().is_some_and(|a| a.is_empty()))
    });
    serde_json::to_string(obj).unwrap_or_else(|_| raw.to_string())
}

fn append_done_marker(out: &mut Vec<u8>) {
    out.extend_from_slice(b"data: [DONE]\n\n");
}

/// Write an SSE block. The `event:` line matches JSON `type` (Codex/openai SDK convention).
fn append_responses_event(out: &mut Vec<u8>, data: Value) {
    let event = data
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("message");
    if let Ok(line) = serde_json::to_string(&data) {
        out.extend_from_slice(format!("event: {event}\ndata: {line}\n\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_input_converts_to_messages() {
        let payload = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "instructions": "You are helpful",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }],
            }],
            "max_output_tokens": 128,
        });
        let chat = responses_payload_to_chat_completions(&payload);
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(chat["max_tokens"], 128);
        assert!(chat.get("input").is_none());
    }

    #[test]
    fn mimo_responses_reasoning_round_trips_to_chat_reasoning_content() {
        let payload = json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "input": [
                { "type": "message", "role": "user", "content": "search for cats" },
                {
                    "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": "I should call search" }],
                    "encrypted_content": "FULL reasoning trace"
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "search",
                    "arguments": "{\"q\":\"cats\"}"
                },
                { "type": "function_call_output", "call_id": "call_1", "output": "5 results" },
            ],
            "tools": [{
                "type": "function",
                "name": "search",
                "parameters": { "type": "object" }
            }]
        });
        let chat = responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::Mimo);
        let messages = chat["messages"].as_array().unwrap();
        let assistant = messages
            .iter()
            .find(|m| {
                m.get("role").and_then(|r| r.as_str()) == Some("assistant")
                    && m.get("reasoning_content")
                        .and_then(|v| v.as_str())
                        .is_some()
            })
            .expect("assistant message");
        assert_eq!(
            assistant.get("reasoning_content").and_then(|v| v.as_str()),
            Some("FULL reasoning trace")
        );
        assert!(
            messages
                .iter()
                .any(|m| m.get("tool_calls").and_then(|v| v.as_array()).is_some()),
            "function call history must remain available: {}",
            serde_json::to_string_pretty(&messages).unwrap_or_default()
        );
    }

    #[test]
    fn stored_responses_reasoning_expands_back_into_followup_input() {
        let item = json!({
            "type": "reasoning",
            "summary": [{ "type": "summary_text", "text": "summary" }],
            "encrypted_content": "full"
        });
        let input = responses_output_item_to_input(&item).expect("reasoning preserved");
        assert_eq!(input["type"], "reasoning");
        assert_eq!(input["encrypted_content"], "full");
    }

    #[test]
    fn translator_completed_output_preserves_full_reasoning_for_next_turn() {
        let sse = concat!(
            "data: ",
            "{\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
            "data: ",
            "{\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let mut tr = ChatToResponsesSseTranslator::new("mimo-v2.5-pro");
        let _ = tr.translate_chunk(sse.as_bytes());
        let output = tr.completed_output();
        let reasoning = output
            .iter()
            .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("reasoning"))
            .expect("reasoning output");
        assert_eq!(reasoning["encrypted_content"], "think");
    }

    #[test]
    fn upstream_uri_rewrites_to_chat_completions() {
        let mut req = RequestHeader::build("POST", b"/v1/responses", None).expect("request header");
        apply_responses_wire_upstream_request(&mut req);
        assert_eq!(req.uri.path(), "/v1/chat/completions");
    }

    #[test]
    fn chat_sse_translates_to_responses_events() {
        let sse = concat!(
            "data: ",
            "{\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: ",
            "{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: ",
            "{\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: ",
            "{\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let mut tr = ChatToResponsesSseTranslator::new("deepseek-v4-pro");
        let out = tr.translate_chunk(sse.as_bytes());
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("event: response.created"));
        assert!(text.contains("event: response.in_progress"));
        assert!(text.contains("event: response.output_item.added"));
        assert!(text.contains("event: response.output_text.delta"));
        assert!(text.contains("event: response.output_text.done"));
        assert!(text.contains("event: response.completed"));
        assert!(text.contains("\"sequence_number\""));
        assert!(text.contains("\"text\":\"Hello\""));
        assert!(text.contains("data: [DONE]"));
    }

    #[test]
    fn reasoning_delta_includes_summary_index() {
        let sse = concat!(
            "data: ",
            "{\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
            "data: ",
            "{\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let mut tr = ChatToResponsesSseTranslator::new("deepseek-v4-pro");
        let out = tr.translate_chunk(sse.as_bytes());
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("response.reasoning_summary_part.added"));
        assert!(text.contains("response.reasoning_summary_text.delta"));
        assert!(text.contains("\"summary_index\":0"));
    }

    #[test]
    fn chat_json_converts_to_responses_object() {
        let body = json!({
            "model": "mimo-v2.5",
            "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 2 },
        });
        let resp = chat_completions_json_to_responses(&body, "mimo-v2.5");
        assert_eq!(resp["object"], "response");
        assert_eq!(resp["output"][0]["content"][0]["text"], "OK");
        assert_eq!(resp["usage"]["input_tokens"], 10);
    }

    #[test]
    fn chat_sse_translates_split_across_chunks() {
        let line = concat!(
            "data: ",
            "{\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: ",
            "{\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let split = line.find("choices").expect("marker");
        let mut tr = ChatToResponsesSseTranslator::new("mimo-v2.5-pro");
        let mut out = tr.translate_chunk(line[..split].as_bytes());
        assert!(out.is_empty() || tr.upstream_sse_remainder_len() > 0);
        out.extend(tr.translate_chunk(line[split..].as_bytes()));
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("event: response.output_text.delta"));
        assert!(text.contains("event: response.completed"));
        assert_eq!(tr.upstream_sse_remainder_len(), 0);
    }

    #[test]
    fn finish_reason_emits_done_marker_without_extra_flush() {
        let sse = concat!(
            "data: ",
            "{\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n",
            "data: ",
            "{\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let mut tr = ChatToResponsesSseTranslator::new("gpt-5.4-mini");
        let out = tr.translate_chunk(sse.as_bytes());
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("data: [DONE]"));
        assert!(tr.done_marker_sent());
        let tail = tr.flush();
        assert!(!String::from_utf8_lossy(&tail).contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn responses_chain_expands_previous_response_id() {
        let store = ResponsesChainStore::new_l0_only(16, 60, tokio::runtime::Handle::current());
        store_responses_chain_output(
            &store,
            "deepseek",
            "resp_prev",
            vec![json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "prior" }],
            })],
        );
        let mut payload = json!({
            "model": "gpt-5.4-mini",
            "stream": true,
            "previous_response_id": "resp_prev",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "next" }],
            }],
        });
        apply_responses_chain(&mut payload, &store, "deepseek").await;
        let chat = responses_payload_to_chat_completions(&payload);
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[1]["role"], "user");
        assert!(payload.get("previous_response_id").is_none());
    }

    #[test]
    fn groups_function_calls_into_single_assistant_message() {
        let payload = json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_a",
                    "name": "read_file",
                    "arguments": "{\"path\":\"a\"}",
                },
                {
                    "type": "function_call",
                    "call_id": "call_b",
                    "name": "write_file",
                    "arguments": "{\"path\":\"b\"}",
                },
            ],
        });
        let chat = responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::Mimo);
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn drops_commentary_phase_assistant_messages() {
        let payload = json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "input": [
                {
                    "type": "message",
                    "role": "assistant",
                    "phase": "commentary",
                    "content": [{ "type": "output_text", "text": "internal" }],
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "continue" }],
                },
            ],
        });
        let chat = responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::Mimo);
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn codex_relay_never_translates_responses_wire() {
        use crate::context::{ClientWireApi, GatewayContext};

        let mut ctx = GatewayContext::new("test".to_string());
        ctx.request_pipeline = Some(RequestPipeline::CodexRelay);
        ctx.client_wire_api = ClientWireApi::Responses;
        assert!(!needs_responses_wire_translate(&ctx));
        assert_eq!(
            responses_wire_profile(&ctx),
            Some(ResponsesWireProfile::Codex)
        );
    }

    #[test]
    fn codex_deepseek_always_triggers_translation() {
        use crate::context::{ClientWireApi, GatewayContext};

        let mut ctx = GatewayContext::new("test".to_string());
        ctx.request_pipeline = Some(RequestPipeline::CodexDeepSeek);

        // Even with ChatCompletions wire API, CodexDeepSeek forces translation.
        ctx.client_wire_api = ClientWireApi::ChatCompletions;
        assert!(needs_responses_wire_translate(&ctx));

        // Also works with Responses wire API.
        ctx.client_wire_api = ClientWireApi::Responses;
        assert!(needs_responses_wire_translate(&ctx));
    }

    #[test]
    fn deepseek_light_only_translates_with_responses_wire() {
        use crate::context::{ClientWireApi, GatewayContext};

        let mut ctx = GatewayContext::new("test".to_string());
        ctx.request_pipeline = Some(RequestPipeline::DeepSeekLight);

        ctx.client_wire_api = ClientWireApi::ChatCompletions;
        assert!(!needs_responses_wire_translate(&ctx));

        ctx.client_wire_api = ClientWireApi::Responses;
        assert!(needs_responses_wire_translate(&ctx));
    }

    #[tokio::test]
    async fn graceful_finalize_synthesizes_response_completed() {
        use crate::context::{ClientWireApi, GatewayContext};

        let mut ctx = GatewayContext::new("req-grace".to_string());
        ctx.is_streaming = true;
        ctx.client_wire_api = ClientWireApi::Responses;
        ctx.request_pipeline = Some(RequestPipeline::MimoTokenPlanRelay);
        ctx.upstream.http_status = Some(200);
        ctx.model = "mimo-v2.5-pro".to_string();
        let model = ctx.model.clone();
        arm_responses_wire_stream(&mut ctx, &model);
        ctx.stream.responses_wire_bootstrap = None;
        ctx.stream.responses_wire_bootstrap_sent = true;

        let store = ResponsesChainStore::new_l0_only(16, 60, tokio::runtime::Handle::current());
        let tail = build_graceful_responses_stream_tail(&mut ctx, store.as_ref());
        assert!(tail.is_some(), "expected graceful tail");
        let tail = tail.unwrap();
        assert!(
            crate::sse::sse_bytes_contains_event(&tail, "response.completed"),
            "tail must contain response.completed"
        );
        assert!(
            ctx.stream
                .responses_translator
                .as_ref()
                .is_some_and(|t| t.is_completed())
        );
    }

    #[tokio::test]
    async fn graceful_finalize_when_http_status_missing_but_stream_started() {
        use crate::context::{ClientWireApi, GatewayContext};

        let mut ctx = GatewayContext::new("req-grace-missing-status".to_string());
        ctx.is_streaming = true;
        ctx.client_wire_api = ClientWireApi::Responses;
        ctx.request_pipeline = Some(RequestPipeline::MimoTokenPlanRelay);
        ctx.upstream.http_status = None;
        ctx.model = "mimo-v2.5-pro".to_string();
        let model = ctx.model.clone();
        arm_responses_wire_stream(&mut ctx, &model);
        ctx.stream.responses_wire_bootstrap = None;
        ctx.stream.responses_wire_bootstrap_sent = true;
        ctx.stream
            .client_sse_body
            .extend_from_slice(b"event: ping\n\n");

        let store = ResponsesChainStore::new_l0_only(16, 60, tokio::runtime::Handle::current());
        let tail = build_graceful_responses_stream_tail(&mut ctx, store.as_ref());
        assert!(
            tail.is_some(),
            "expected graceful tail when upstream aborts before http_status is recorded"
        );
    }

    #[test]
    fn custom_apply_patch_survives_responses_to_chat_conversion() {
        let payload = json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "instructions": "Use apply_patch to edit files.",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "fix" }],
            }],
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object", "properties": { "cmd": { "type": "string" } } }
                },
                {
                    "type": "custom",
                    "name": "apply_patch",
                    "description": "Apply patch freeform",
                    "format": { "type": "grammar", "syntax": "lark", "definition": "start: x" }
                },
            ],
        });
        let chat = responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::Mimo);
        let names: Vec<_> = chat["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"exec_command"));
        assert!(names.contains(&"apply_patch"));
    }

    #[test]
    fn instructions_inject_apply_patch_when_missing_from_client_tools() {
        let payload = json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "instructions": "Use apply_patch to edit files.",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "fix" }],
            }],
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                },
            ],
        });
        let chat = responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::Mimo);
        let names: Vec<_> = chat["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"exec_command"));
        assert!(
            !names.contains(&"apply_patch"),
            "exec-only Codex sessions must not inject apply_patch upstream"
        );
    }

    #[test]
    fn exec_only_downstream_remaps_apply_patch_to_exec_command() {
        let mut tr = ChatToResponsesSseTranslator::new("mimo-v2.5-pro");
        tr.set_exec_only_surface(true);
        tr.bootstrap_stream();
        let chunk = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc",
                        "function": { "name": "apply_patch", "arguments": "{\"input\":\"patch\"}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let out = tr.translate_chunk(format!("data: {chunk}\n\n").as_bytes());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"name\":\"exec_command\""), "{text}");
        assert!(!text.contains("\"name\":\"apply_patch\""), "{text}");
    }

    #[test]
    fn sanitize_function_call_names_for_mimo() {
        let payload = json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "input": [{
                "type": "function_call",
                "call_id": "c1",
                "name": "mcp__ns__get.issue",
                "arguments": "{}",
            }],
        });
        let chat = responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::Mimo);
        let name = chat["messages"][0]["tool_calls"][0]["function"]["name"]
            .as_str()
            .unwrap();
        assert_eq!(name, "mcp__ns__get_issue");
    }

    #[tokio::test]
    async fn chain_avoids_duplicating_full_resend_input() {
        let store = ResponsesChainStore::new_l0_only(16, 60, tokio::runtime::Handle::current());
        store_responses_chain_output(
            &store,
            "deepseek",
            "resp_prev",
            vec![json!({
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{}",
            })],
        );
        let mut payload = json!({
            "previous_response_id": "resp_prev",
            "input": (0..5).map(|i| json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": format!("turn {i}") }],
            })).collect::<Vec<_>>(),
        });
        apply_responses_chain(&mut payload, &store, "deepseek").await;
        let items = payload["input"].as_array().unwrap();
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn deepseek_keeps_orphan_tool_messages_mimo_drops_them() {
        let payload = json!({
            "model": "gpt-5.4-mini",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_a",
                    "name": "read_file",
                    "arguments": "{}",
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_a",
                    "output": "ok",
                },
                {
                    "type": "function_call_output",
                    "call_id": "orphan",
                    "output": "extra",
                },
            ],
        });
        let mimo = responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::Mimo);
        let codex =
            responses_payload_to_chat_completions_for(&payload, ResponsesWireTarget::DeepSeek);
        let tool_count = |v: &Value| {
            v["messages"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("tool"))
                        .count()
                })
                .unwrap_or(0)
        };
        assert_eq!(tool_count(&mimo), 1);
        assert_eq!(tool_count(&codex), 2);
    }

    #[tokio::test]
    async fn responses_chain_namespaces_do_not_cross_pipelines() {
        let store = ResponsesChainStore::new_l0_only(16, 60, tokio::runtime::Handle::current());
        store_responses_chain_output(
            &store,
            "mimo",
            "resp_prev",
            vec![json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "mimo only" }],
            })],
        );
        let mut payload = json!({
            "previous_response_id": "resp_prev",
            "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "x" }] }],
        });
        apply_responses_chain(&mut payload, &store, "deepseek").await;
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }
}
