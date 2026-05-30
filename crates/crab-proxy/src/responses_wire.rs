//! Translate OpenAI Responses API wire (`POST /v1/responses`) to Chat Completions for
//! non-Codex upstreams (DeepSeek, MiMo, …) and back on the response path.

use bytes::Bytes;
use crab_pipeline::RequestPipeline;
use http::Uri;
use pingora_http::RequestHeader;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::{ClientWireApi, GatewayContext};
use crate::sse::parse_sse_chunk;

/// Upstream path for OpenAI-compatible chat backends (DeepSeek, MiMo, …).
const CHAT_COMPLETIONS_UPSTREAM_PATH: &str = "/v1/chat/completions";

pub fn needs_responses_wire_translate(ctx: &GatewayContext) -> bool {
    // CodexDeepSeek always forces Responses API translation regardless of client path.
    ctx.request_pipeline == Some(RequestPipeline::CodexDeepSeek)
        || (ctx.client_wire_api == ClientWireApi::Responses
            && ctx.request_pipeline != Some(RequestPipeline::CodexRelay))
}

/// Rewrite upstream request URI from client `/v1/responses` to Chat Completions.
pub fn apply_responses_wire_upstream_request(req: &mut RequestHeader) {
    if let Ok(uri) = CHAT_COMPLETIONS_UPSTREAM_PATH.parse::<Uri>() {
        req.set_uri(uri);
    }
}

/// Convert a Responses API request body into Chat Completions JSON for upstream relay.
pub fn responses_payload_to_chat_completions(payload: &Value) -> Value {
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
            for item in items {
                append_responses_input_item(item, &mut messages);
            }
        }
        _ => {}
    }

    out.remove("input");
    out.remove("instructions");
    out.remove("previous_response_id");
    out.remove("prompt");
    out.insert("messages".into(), Value::Array(messages));

    if let Some(max_out) = payload.get("max_output_tokens") {
        out.insert("max_tokens".into(), max_out.clone());
        out.remove("max_output_tokens");
    }

    if let Some(tools) = payload.get("tools").and_then(|v| v.as_array()) {
        out.insert("tools".into(), Value::Array(convert_tools_responses_to_chat(tools)));
    }

    Value::Object(out)
}

fn append_responses_input_item(item: &Value, messages: &mut Vec<Value>) {
    let item_type = item.get("type").and_then(|t| t.as_str());
    match item_type {
        Some("function_call") => {
            messages.push(json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
                    "type": "function",
                    "function": {
                        "name": item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "arguments": item.get("arguments").and_then(|v| v.as_str()).unwrap_or(""),
                    }
                }]
            }));
        }
        Some("function_call_output") => {
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
        Some("message") | None => {
            let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = responses_content_to_chat(item.get("content"), role);
            if content_is_nonempty(&content) {
                messages.push(json!({ "role": role, "content": content }));
            }
        }
        _ => {}
    }
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
                && out_parts[0]
                    .get("type")
                    .and_then(|t| t.as_str())
                    == Some("text")
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
    tools
        .iter()
        .map(|tool| {
            if tool.get("type").and_then(|t| t.as_str()) == Some("function")
                && tool.get("function").is_none()
            {
                let mut func = Map::new();
                if let Some(name) = tool.get("name") {
                    func.insert("name".into(), name.clone());
                }
                if let Some(desc) = tool.get("description") {
                    func.insert("description".into(), desc.clone());
                }
                if let Some(params) = tool.get("parameters") {
                    func.insert("parameters".into(), params.clone());
                }
                json!({ "type": "function", "function": Value::Object(func) })
            } else {
                tool.clone()
            }
        })
        .collect()
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
    #[allow(dead_code)]
    model: String,
}

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
        }
    }

    pub fn translate_chunk(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for event in parse_sse_chunk(chunk) {
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
                if let Some(id) = value.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
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

            if let Some(tool_calls) = delta.and_then(|d| d.get("tool_calls")).and_then(|v| v.as_array()) {
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
        self.emit_completed(&mut out);
        out
    }

    fn emit(&mut self, out: &mut Vec<u8>, mut data: Value) {
        self.seq += 1;
        if let Some(obj) = data.as_object_mut() {
            obj.insert("sequence_number".into(), json!(self.seq));
        }
        append_responses_event(out, data);
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

        if let Some(entry) = self.tool_calls.get(&tc_idx)
            && let Some(ref new_id) = new_call_id
            && entry.call_id != *new_id
        {
            self.close_tool_call(out, tc_idx);
            self.tool_calls.remove(&tc_idx);
        }

        let entry = self.tool_calls.entry(tc_idx).or_insert_with(|| ToolCallState {
            call_id: String::new(),
            name: String::new(),
            args_buf: String::new(),
            item_added: false,
            item_done: false,
        });

        if let Some(name) = func_name {
            entry.name = name;
        }
        if entry.call_id.is_empty() {
            if let Some(id) = new_call_id {
                entry.call_id = id;
            }
        }

        let should_add_item = !entry.item_added && !entry.call_id.is_empty();
        let add_payload = if should_add_item {
            entry.item_added = true;
            Some((
                entry.call_id.clone(),
                entry.name.clone(),
                tc_idx,
            ))
        } else {
            None
        };

        if let Some(args) = args_delta {
            entry.args_buf.push_str(&args);
            if !entry.call_id.is_empty() {
                let call_id = entry.call_id.clone();
                self.emit(
                    out,
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": format!("fc_{call_id}"),
                        "output_index": tc_idx,
                        "delta": args,
                    }),
                );
            }
        }

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
            entry.args_buf.clone()
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
            "output": output,
        });
        if let Some(usage) = self.pending_usage.take() {
            if let Some(obj) = response.as_object_mut() {
                obj.insert("usage".into(), chat_usage_to_responses(&usage));
            }
        }
        self.emit(out, json!({ "type": "response.completed", "response": response }));
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
            }),
        );
    }
    if let Some(tool_calls) = message.and_then(|m| m.get("tool_calls")).and_then(|v| v.as_array()) {
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
        *translator = Some(ChatToResponsesSseTranslator::new(model));
    }
    let out = translator.as_mut().expect("initialized").translate_chunk(&data);
    if out.is_empty() {
        None
    } else {
        Some(Bytes::from(out))
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
    json!({
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(input + output),
    })
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
    fn upstream_uri_rewrites_to_chat_completions() {
        let mut req =
            RequestHeader::build("POST", b"/v1/responses", None).expect("request header");
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
}
