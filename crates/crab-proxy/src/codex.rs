//! Codex (ChatGPT) upstream integration: Chat Completions ↔ Responses API translation.

use bytes::Bytes;
use http::Uri;
use pingora_http::RequestHeader;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// Upstream path for Codex chat (relative to `https://chatgpt.com`).
pub const CODEX_RESPONSES_PATH: &str = "/backend-api/codex/responses";

pub const CODEX_USER_AGENT: &str = "codex_cli_rs/0.118.0 (Mac OS 26.3.1; arm64) iTerm.app/3.6.9";
pub const CODEX_ORIGINATOR: &str = "codex_cli_rs";

pub trait RequestTranslator {
    type Prepared;

    fn prepare_chat_request(
        &self,
        payload: &Value,
        upstream_model: &str,
        opts: CodexPrepareOptions<'_>,
    ) -> Self::Prepared;

    fn prepare_client_responses(
        &self,
        payload: &Value,
        upstream_model: &str,
        opts: CodexPrepareOptions<'_>,
    ) -> Self::Prepared;
}

pub trait ResponseTranslator {
    fn translate_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes>;
    fn finalize_non_stream(&mut self, raw_sse: &[u8]) -> Option<Vec<u8>>;
}

#[derive(Default, Clone, Copy)]
pub struct CodexTranslator;

impl CodexTranslator {
    pub fn prepare_chat_request(
        self,
        payload: &Value,
        upstream_model: &str,
        opts: CodexPrepareOptions<'_>,
    ) -> CodexPreparedRequest {
        <Self as RequestTranslator>::prepare_chat_request(&self, payload, upstream_model, opts)
    }

    pub fn prepare_client_responses(
        self,
        payload: &Value,
        upstream_model: &str,
        opts: CodexPrepareOptions<'_>,
    ) -> CodexPreparedRequest {
        <Self as RequestTranslator>::prepare_client_responses(&self, payload, upstream_model, opts)
    }
}

/// ChatGPT OAuth accounts reject legacy API slugs like `gpt-5-codex` (CLIProxyAPI uses `gpt-5.5` etc.).
pub fn resolve_codex_upstream_model(model: &str) -> &str {
    let model = crab_pipeline::resolve_codex_display_alias(model).unwrap_or(model);
    match model {
        "gpt-5" | "gpt-5-codex" | "gpt-5-codex-mini" | "gpt-5.1" | "gpt-5.1-codex"
        | "gpt-5.1-codex-max" | "gpt-5.1-codex-mini" | "gpt-5.2" | "gpt-5.2-codex"
        | "gpt-5.3-codex" | "gpt-5.3-codex-spark" => "gpt-5.5",
        other => other,
    }
}

fn trim_u8_slice(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|p| p + 1)
        .unwrap_or(0);
    &s[start..end]
}

#[derive(Debug, Clone, Default)]
pub struct CodexPrepareOptions<'a> {
    pub conversation_id: Option<&'a str>,
    pub prompt_cache_key: Option<&'a str>,
    /// Fallback when body/header omit conversation id (e.g. `client:<sk-cc>` from gateway).
    pub stable_session_id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CodexPreparedRequest {
    pub payload: Value,
    pub model: String,
    /// Session id forwarded to ChatGPT (`Conversation_id` / `Session_id` headers).
    pub session_id: Option<String>,
}

fn non_empty_str(value: Option<&str>) -> Option<&str> {
    value.filter(|s| !s.trim().is_empty())
}

fn resolve_codex_session_id(payload: &Value, opts: &CodexPrepareOptions<'_>) -> Option<String> {
    non_empty_str(payload.get("conversation_id").and_then(|v| v.as_str()))
        .or(opts.conversation_id)
        .map(str::to_string)
        .or_else(|| {
            non_empty_str(payload.get("prompt_cache_key").and_then(|v| v.as_str()))
                .or(opts.prompt_cache_key)
                .map(str::to_string)
        })
        .or_else(|| opts.stable_session_id.map(str::to_string))
}

fn resolve_codex_prompt_cache_key(payload: &Value, opts: &CodexPrepareOptions<'_>) -> Option<String> {
    non_empty_str(payload.get("prompt_cache_key").and_then(|v| v.as_str()))
        .or(opts.prompt_cache_key)
        .map(str::to_string)
        .or_else(|| {
            non_empty_str(payload.get("conversation_id").and_then(|v| v.as_str()))
                .or(opts.conversation_id)
                .map(str::to_string)
        })
        .or_else(|| opts.stable_session_id.map(str::to_string))
}

/// Translate an OpenAI Chat Completions body into Codex Responses API JSON.
pub fn prepare_codex_request(
    payload: &Value,
    upstream_model: &str,
    opts: CodexPrepareOptions<'_>,
) -> CodexPreparedRequest {
    CodexTranslator.prepare_chat_request(payload, upstream_model, opts)
}

fn should_passthrough_responses_input(payload: &Value) -> bool {
    let messages_empty = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .is_none_or(|a| a.is_empty());
    let input_present = payload
        .get("input")
        .and_then(|i| i.as_array())
        .is_some_and(|a| !a.is_empty());
    messages_empty && input_present
}

fn apply_codex_session_fields(out: &mut Value, prompt_cache_key: Option<&str>) {
    if let Some(pck) = prompt_cache_key.filter(|s| !s.is_empty()) {
        out["prompt_cache_key"] = json!(pck);
    }
}

fn ensure_codex_required_input(out: &mut Value, payload: &Value) {
    if codex_payload_has_required_input(out) {
        return;
    }
    if let Some(prompt) = payload
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        out["prompt"] = json!(prompt);
        return;
    }
    if let Some(prompt) = extract_last_user_prompt(payload) {
        out["prompt"] = json!(prompt);
        return;
    }
    // Last resort: inject a synthetic user message so Codex doesn't reject with
    // "Input must be a list" when all messages were system/developer only.
    let fallback_text = out
        .get("instructions")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Continue")
        .to_string();
    if let Some(input) = out.get_mut("input").and_then(|v| v.as_array_mut()) {
        input.push(json!({
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": fallback_text }],
        }));
    }
}

fn codex_payload_has_required_input(out: &Value) -> bool {
    if out
        .get("prompt")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty())
    {
        return true;
    }
    if out
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty())
    {
        return true;
    }
    let Some(input) = out.get("input").and_then(|v| v.as_array()) else {
        return false;
    };
    if input.is_empty() {
        return false;
    }
    input.iter().any(|item| {
        match item.get("type").and_then(|t| t.as_str()) {
            Some("message") => item
                .get("content")
                .and_then(|c| c.as_array())
                .is_some_and(|parts| !parts.is_empty()),
            Some("function_call") | Some("function_call_output") => false,
            Some(_) => true,
            None => false,
        }
    })
}

fn prepare_codex_responses_passthrough(
    payload: &Value,
    upstream_model: &str,
    opts: CodexPrepareOptions<'_>,
) -> CodexPreparedRequest {
    let upstream_model = resolve_codex_upstream_model(upstream_model);
    let session_id = resolve_codex_session_id(payload, &opts);
    let prompt_cache_key = resolve_codex_prompt_cache_key(payload, &opts);
    let effort = payload
        .get("reasoning_effort")
        .or_else(|| payload.pointer("/reasoning/effort"))
        .and_then(|v| v.as_str())
        .unwrap_or("medium");

    let mut out = json!({
        "instructions": payload.get("instructions").and_then(|v| v.as_str()).unwrap_or(""),
        // Codex `/backend-api/codex/responses` rejects `stream: false`; always force streaming.
        "stream": true,
        "store": payload.get("store").and_then(|v| v.as_bool()).unwrap_or(false),
        "parallel_tool_calls": payload.get("parallel_tool_calls").and_then(|v| v.as_bool()).unwrap_or(true),
        "include": payload.get("include").cloned().unwrap_or_else(|| json!(["reasoning.encrypted_content"])),
        "reasoning": payload.get("reasoning").cloned().unwrap_or_else(|| json!({
            "effort": effort,
            "summary": "auto",
        })),
        "model": upstream_model,
        "input": payload.get("input").cloned().unwrap_or_else(|| json!([])),
    });

    apply_codex_session_fields(&mut out, prompt_cache_key.as_deref());
    if let Some(prev) = payload
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        out["previous_response_id"] = json!(prev);
    }
    if let Some(prompt) = payload
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        out["prompt"] = json!(prompt);
    }
    if let Some(tools) = payload.get("tools").and_then(|v| v.as_array())
        && !tools.is_empty()
    {
        out["tools"] = Value::Array(tools.clone());
    }
    if let Some(tc) = payload.get("tool_choice") {
        out["tool_choice"] = tc.clone();
    }
    ensure_codex_required_input(&mut out, payload);

    CodexPreparedRequest {
        payload: out,
        model: upstream_model.to_string(),
        session_id,
    }
}

/// Normalize a client `POST /v1/responses` body for ChatGPT Codex upstream.
pub fn prepare_codex_client_responses(
    payload: &Value,
    upstream_model: &str,
    opts: CodexPrepareOptions<'_>,
) -> CodexPreparedRequest {
    CodexTranslator.prepare_client_responses(payload, upstream_model, opts)
}

fn prepare_codex_from_chat_messages(
    payload: &Value,
    upstream_model: &str,
    opts: CodexPrepareOptions<'_>,
) -> CodexPreparedRequest {
    let upstream_model = resolve_codex_upstream_model(upstream_model);
    let effort = payload
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");

    let mut instructions_parts: Vec<String> = Vec::new();
    let mut input: Vec<Value> = Vec::new();
    if let Some(messages) = payload.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            if role == "tool" {
                let call_id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let output = message_content_text(msg.get("content"));
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                }));
                continue;
            }

            if role == "system" || role == "developer" {
                let text = message_content_text(msg.get("content"));
                if !text.is_empty() {
                    instructions_parts.push(text);
                }
                continue;
            }

            let mut content_parts: Vec<Value> = Vec::new();
            append_message_content_parts(msg, role, &mut content_parts);

            if !content_parts.is_empty() {
                input.push(json!({
                    "type": "message",
                    "role": role,
                    "content": content_parts,
                }));
            }

            if role == "assistant"
                && let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array())
            {
                for tc in tool_calls {
                    if tc.get("type").and_then(|v| v.as_str()) != Some("function") {
                        continue;
                    }
                    let name = tc
                        .pointer("/function/name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let args = tc
                        .pointer("/function/arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    input.push(json!({
                        "type": "function_call",
                        "call_id": tc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": name,
                        "arguments": args,
                    }));
                }
            }
        }
    }

    let session_id = resolve_codex_session_id(payload, &opts);
    let prompt_cache_key = resolve_codex_prompt_cache_key(payload, &opts);
    let instructions = instructions_parts.join("\n\n");

    let mut out = json!({
        "instructions": instructions,
        "stream": true,
        "store": false,
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"],
        "reasoning": {
            "effort": effort,
            "summary": "auto",
        },
        "model": upstream_model,
        "input": input,
    });

    // Codex `/backend-api/codex/responses` rejects `conversation_id` in JSON; session goes in
    // `Conversation_id` / `Session_id` headers + optional `prompt_cache_key` body field.
    apply_codex_session_fields(&mut out, prompt_cache_key.as_deref());
    if let Some(prev) = payload
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        out["previous_response_id"] = json!(prev);
    }

    attach_codex_tools_from_chat(payload, &mut out);
    attach_codex_tool_choice(payload, &mut out);
    ensure_codex_required_input(&mut out, payload);

    CodexPreparedRequest {
        payload: out,
        model: upstream_model.to_string(),
        session_id,
    }
}

impl RequestTranslator for CodexTranslator {
    type Prepared = CodexPreparedRequest;

    fn prepare_chat_request(
        &self,
        payload: &Value,
        upstream_model: &str,
        opts: CodexPrepareOptions<'_>,
    ) -> Self::Prepared {
        if should_passthrough_responses_input(payload) {
            return prepare_codex_responses_passthrough(payload, upstream_model, opts);
        }
        prepare_codex_from_chat_messages(payload, upstream_model, opts)
    }

    fn prepare_client_responses(
        &self,
        payload: &Value,
        upstream_model: &str,
        opts: CodexPrepareOptions<'_>,
    ) -> Self::Prepared {
        prepare_codex_responses_passthrough(payload, upstream_model, opts)
    }
}

fn attach_codex_tools_from_chat(payload: &Value, out: &mut Value) {
    if let Some(tools) = payload.get("tools").and_then(|v| v.as_array())
        && !tools.is_empty()
    {
        let mut mapped = Vec::new();
        for tool in tools {
            let tool_type = tool.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if tool_type != "function" {
                mapped.push(tool.clone());
                continue;
            }
            if let Some(func) = tool.get("function") {
                let mut item = Map::new();
                item.insert("type".into(), Value::String("function".into()));
                if let Some(name) = func.get("name") {
                    item.insert("name".into(), name.clone());
                }
                if let Some(desc) = func.get("description") {
                    item.insert("description".into(), desc.clone());
                }
                if let Some(params) = func.get("parameters") {
                    item.insert("parameters".into(), params.clone());
                }
                if let Some(strict) = func.get("strict") {
                    item.insert("strict".into(), strict.clone());
                }
                mapped.push(Value::Object(item));
            }
        }
        out["tools"] = Value::Array(mapped);
    }
}

fn attach_codex_tool_choice(payload: &Value, out: &mut Value) {
    if let Some(tc) = payload.get("tool_choice") {
        match tc {
            Value::String(s) => out["tool_choice"] = Value::String(s.clone()),
            Value::Object(map) => {
                if map.get("type").and_then(|v| v.as_str()) == Some("function") {
                    let name = map
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    out["tool_choice"] = json!({ "type": "function", "name": name });
                } else {
                    out["tool_choice"] = tc.clone();
                }
            }
            _ => {}
        }
    }
}

fn extract_last_user_prompt(payload: &Value) -> Option<String> {
    let messages = payload.get("messages")?.as_array()?;
    for msg in messages.iter().rev() {
        if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let text = message_content_text(msg.get("content"));
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

fn append_message_content_parts(msg: &Value, role: &str, out: &mut Vec<Value>) {
    let part_type = if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };

    if let Some(content) = msg.get("content") {
        append_content_value(content, role, part_type, out);
    }

    if role == "assistant"
        && out.is_empty()
        && let Some(reasoning) = msg.get("reasoning_content").and_then(|v| v.as_str())
        && !reasoning.is_empty()
    {
        out.push(json!({ "type": "output_text", "text": reasoning }));
    }
}

fn append_content_value(content: &Value, role: &str, part_type: &str, out: &mut Vec<Value>) {
    match content {
        Value::String(text) if !text.is_empty() => {
            out.push(json!({ "type": part_type, "text": text }));
        }
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(text) if !text.is_empty() => {
                        out.push(json!({ "type": part_type, "text": text }));
                    }
                    Value::Object(obj) => {
                        let item_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match item_type {
                            "text" | "input_text" | "output_text" => {
                                if let Some(text) = obj
                                    .get("text")
                                    .or_else(|| obj.get("content"))
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                {
                                    let mapped = if item_type == "output_text" || role == "assistant"
                                    {
                                        "output_text"
                                    } else {
                                        "input_text"
                                    };
                                    out.push(json!({ "type": mapped, "text": text }));
                                }
                            }
                            "image_url" | "input_image" if role == "user" => {
                                let url = obj
                                    .get("image_url")
                                    .and_then(|v| {
                                        v.as_str().or_else(|| {
                                            v.get("url").and_then(|u| u.as_str())
                                        })
                                    });
                                if let Some(url) = url.filter(|s| !s.is_empty()) {
                                    out.push(json!({ "type": "input_image", "image_url": url }));
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = s.len();
    while start < end && s[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && s[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &s[start..end]
}

fn message_content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => parts.push(s.clone()),
                    Value::Object(obj) => {
                        let item_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        let text = obj
                            .get("text")
                            .or_else(|| obj.get("content"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if item_type == "text"
                            || item_type == "input_text"
                            || item_type == "output_text"
                            || !text.is_empty()
                        {
                            parts.push(text.to_string());
                        }
                    }
                    other => parts.push(other.to_string()),
                }
            }
            parts
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        }
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Rewrite upstream request URI and inject Codex-specific headers.
pub fn apply_codex_upstream_request(
    req: &mut RequestHeader,
    account_id: &str,
    is_streaming: bool,
    session_id: Option<&str>,
) {
    if let Ok(uri) = CODEX_RESPONSES_PATH.parse::<Uri>() {
        req.set_uri(uri);
    }
    let _ = req.insert_header("Chatgpt-Account-Id", account_id);
    let _ = req.insert_header("Originator", CODEX_ORIGINATOR);
    if let Some(id) = non_empty_str(session_id) {
        let _ = req.insert_header("Conversation_id", id);
        let _ = req.insert_header("Session_id", id);
    }
    let _ = req.remove_header(&http::header::USER_AGENT);
    let _ = req.insert_header(http::header::USER_AGENT, CODEX_USER_AGENT);
    let accept = if is_streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    let _ = req.remove_header(&http::header::ACCEPT);
    let _ = req.insert_header(http::header::ACCEPT, accept);
    let _ = req.remove_header(&http::header::CONNECTION);
    let _ = req.insert_header(http::header::CONTENT_TYPE, "application/json");
}

/// Stateful translator: Codex Responses SSE → OpenAI Chat Completions SSE chunks.
#[derive(Default)]
pub struct CodexSseTranslator {
    response_id: String,
    created_at: i64,
    model: String,
    function_call_index: i32,
    has_tool_call_announced: bool,
    has_arguments_delta: bool,
    tool_short_to_orig: HashMap<String, String>,
}

impl CodexSseTranslator {
    pub fn new(client_model: &str, original_request: Option<&[u8]>) -> Self {
        let mut translator = Self {
            model: client_model.to_string(),
            function_call_index: -1,
            ..Default::default()
        };
        if let Some(raw) = original_request
            && let Ok(v) = serde_json::from_slice::<Value>(raw)
        {
            translator.tool_short_to_orig = build_tool_reverse_map(&v);
        }
        translator
    }

    /// Process one upstream SSE chunk; returns zero or more client SSE lines.
    pub fn translate_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        let mut out = Vec::new();
        for line in chunk.split(|b| *b == b'\n') {
            let line = line.strip_prefix(b"data:").map(trim_bytes);
            let Some(line) = line else { continue };
            if line.is_empty() || line == b"[DONE]" {
                continue;
            }
            let Ok(event): Result<Value, _> = serde_json::from_slice(line) else {
                continue;
            };
            if let Some(bytes) = self.translate_event(&event) {
                out.push(bytes);
            }
        }
        out
    }

    fn translate_event(&mut self, event: &Value) -> Option<Bytes> {
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let mut chunk = chat_chunk_template(&self.model, &self.response_id, self.created_at);
        match event_type {
            "response.created" => {
                if let Some(resp) = event.get("response") {
                    self.response_id = resp
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.created_at = resp.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0);
                    if let Some(model) = resp.get("model").and_then(|v| v.as_str()) {
                        self.model = model.to_string();
                    }
                }
                return None;
            }
            "response.reasoning_summary_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                    chunk["choices"][0]["delta"]["role"] = json!("assistant");
                    chunk["choices"][0]["delta"]["reasoning_content"] = json!(delta);
                }
            }
            "response.reasoning_summary_text.done" => {
                chunk["choices"][0]["delta"]["role"] = json!("assistant");
                chunk["choices"][0]["delta"]["reasoning_content"] = json!("\n\n");
            }
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                    chunk["choices"][0]["delta"]["role"] = json!("assistant");
                    chunk["choices"][0]["delta"]["content"] = json!(delta);
                }
            }
            "response.output_item.added" => {
                let item = event.get("item")?;
                if item.get("type").and_then(|v| v.as_str()) != Some("function_call") {
                    return None;
                }
                self.function_call_index += 1;
                self.has_tool_call_announced = true;
                self.has_arguments_delta = false;
                let name = restore_tool_name(
                    item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    &self.tool_short_to_orig,
                );
                chunk["choices"][0]["delta"]["role"] = json!("assistant");
                chunk["choices"][0]["delta"]["tool_calls"] = json!([{
                    "index": self.function_call_index,
                    "id": item.get("call_id").cloned().unwrap_or(Value::String(String::new())),
                    "type": "function",
                    "function": { "name": name, "arguments": "" },
                }]);
            }
            "response.function_call_arguments.delta" => {
                self.has_arguments_delta = true;
                let delta = event.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                chunk["choices"][0]["delta"]["tool_calls"] = json!([{
                    "index": self.function_call_index,
                    "function": { "arguments": delta },
                }]);
            }
            "response.function_call_arguments.done" => {
                if self.has_arguments_delta {
                    return None;
                }
                let args = event
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                chunk["choices"][0]["delta"]["tool_calls"] = json!([{
                    "index": self.function_call_index,
                    "function": { "arguments": args },
                }]);
            }
            "response.output_item.done" => {
                let item = event.get("item")?;
                if item.get("type").and_then(|v| v.as_str()) == Some("function_call")
                    && self.has_tool_call_announced
                {
                    self.has_tool_call_announced = false;
                    return None;
                }
                if item.get("type").and_then(|v| v.as_str()) != Some("function_call") {
                    return None;
                }
                self.function_call_index += 1;
                let name = restore_tool_name(
                    item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    &self.tool_short_to_orig,
                );
                chunk["choices"][0]["delta"]["role"] = json!("assistant");
                chunk["choices"][0]["delta"]["tool_calls"] = json!([{
                    "index": self.function_call_index,
                    "id": item.get("call_id").cloned().unwrap_or(Value::String(String::new())),
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": item.get("arguments").and_then(|v| v.as_str()).unwrap_or(""),
                    },
                }]);
            }
            "response.completed" => {
                apply_usage(&mut chunk, event.get("response"));
                let finish = if self.function_call_index >= 0 {
                    "tool_calls"
                } else {
                    "stop"
                };
                chunk["choices"][0]["finish_reason"] = json!(finish);
                chunk["choices"][0]["native_finish_reason"] = json!(finish);
            }
            _ => return None,
        }

        chunk["id"] = json!(self.response_id);
        chunk["created"] = json!(self.created_at);
        chunk["model"] = json!(self.model);
        format_sse_chunk(&chunk)
    }

    /// Build a non-streaming Chat Completions JSON body from accumulated Codex SSE bytes.
    pub fn finalize_non_stream(&mut self, raw_sse: &[u8]) -> Option<Vec<u8>> {
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls: Vec<Value> = Vec::new();
        let mut usage = Value::Null;

        for line in raw_sse.split(|b| *b == b'\n') {
            let line = line.strip_prefix(b"data:").map(trim_bytes);
            let Some(line) = line else { continue };
            if line.is_empty() {
                continue;
            }
            let Ok(event): Result<Value, _> = serde_json::from_slice(line) else {
                continue;
            };
            let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match event_type {
                "response.created" => {
                    if let Some(resp) = event.get("response") {
                        self.response_id = resp
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        self.created_at =
                            resp.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0);
                        if let Some(model) = resp.get("model").and_then(|v| v.as_str()) {
                            self.model = model.to_string();
                        }
                    }
                }
                "response.reasoning_summary_text.delta" => {
                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                        reasoning.push_str(delta);
                    }
                }
                "response.output_text.delta" => {
                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                        content.push_str(delta);
                    }
                }
                "response.completed" => {
                    usage = event
                        .pointer("/response/usage")
                        .cloned()
                        .unwrap_or(Value::Null);
                    if let Some(output) =
                        event.pointer("/response/output").and_then(|v| v.as_array())
                    {
                        for item in output {
                            match item.get("type").and_then(|v| v.as_str()) {
                                Some("message") => {
                                    if let Some(text) = item
                                        .get("content")
                                        .and_then(|c| c.as_array())
                                        .and_then(|arr| {
                                            arr.iter().find(|p| {
                                                p.get("type").and_then(|t| t.as_str())
                                                    == Some("output_text")
                                            })
                                        })
                                        .and_then(|p| p.get("text"))
                                        .and_then(|t| t.as_str())
                                    {
                                        content = text.to_string();
                                    }
                                }
                                Some("reasoning") => {
                                    if let Some(text) = item
                                        .get("summary")
                                        .and_then(|s| s.as_array())
                                        .and_then(|arr| {
                                            arr.iter().find(|p| {
                                                p.get("type").and_then(|t| t.as_str())
                                                    == Some("summary_text")
                                            })
                                        })
                                        .and_then(|p| p.get("text"))
                                        .and_then(|t| t.as_str())
                                    {
                                        reasoning = text.to_string();
                                    }
                                }
                                Some("function_call") => {
                                    let name = restore_tool_name(
                                        item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                        &self.tool_short_to_orig,
                                    );
                                    tool_calls.push(json!({
                                        "id": item.get("call_id").cloned().unwrap_or(Value::String(String::new())),
                                        "type": "function",
                                        "function": {
                                            "name": name,
                                            "arguments": item.get("arguments").and_then(|v| v.as_str()).unwrap_or(""),
                                        },
                                    }));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let finish = if tool_calls.is_empty() {
            "stop"
        } else {
            "tool_calls"
        };
        let mut body = json!({
            "id": self.response_id,
            "object": "chat.completion",
            "created": self.created_at,
            "model": self.model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content,
                },
                "finish_reason": finish,
                "native_finish_reason": finish,
            }],
        });
        if !reasoning.is_empty() {
            body["choices"][0]["message"]["reasoning_content"] = json!(reasoning);
        }
        if !tool_calls.is_empty() {
            body["choices"][0]["message"]["tool_calls"] = Value::Array(tool_calls);
        }
        if !usage.is_null() {
            let mut usage_out = Map::new();
            if let Some(v) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                usage_out.insert("prompt_tokens".into(), json!(v));
            }
            if let Some(v) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                usage_out.insert("completion_tokens".into(), json!(v));
            }
            if let Some(v) = usage.get("total_tokens").and_then(|v| v.as_u64()) {
                usage_out.insert("total_tokens".into(), json!(v));
            }
            body["usage"] = Value::Object(usage_out);
        }
        serde_json::to_vec(&body).ok()
    }
}

fn chat_chunk_template(model: &str, id: &str, created: i64) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": null,
            "native_finish_reason": null,
        }],
    })
}

fn apply_usage(chunk: &mut Value, response: Option<&Value>) {
    let Some(usage) = response.and_then(|r| r.get("usage")) else {
        return;
    };
    if let Some(v) = usage.get("input_tokens") {
        chunk["usage"]["prompt_tokens"] = v.clone();
    }
    if let Some(v) = usage.get("output_tokens") {
        chunk["usage"]["completion_tokens"] = v.clone();
    }
    if let Some(v) = usage.get("total_tokens") {
        chunk["usage"]["total_tokens"] = v.clone();
    }
}

/// Extract usage from raw Codex Responses API SSE (`response.completed` events).
pub fn extract_codex_usage_from_bytes(bytes: &[u8]) -> Option<crate::sse::UsageData> {
    use crate::sse::parse_sse_chunk;
    for event in parse_sse_chunk(bytes) {
        if event.data.trim() == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(event.data).ok()?;
        if value.get("type").and_then(|t| t.as_str()) != Some("response.completed") {
            continue;
        }
        let usage = value.pointer("/response/usage")?;
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let prompt = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let completion = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let prompt_tokens = if prompt > 0 { prompt } else { input };
        let completion_tokens = if completion > 0 { completion } else { output };
        if prompt_tokens == 0 && completion_tokens == 0 {
            continue;
        }
        return Some(crate::sse::UsageData {
            prompt_tokens,
            completion_tokens,
            prompt_cache_hit_tokens: usage
                .get("prompt_cache_hit_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            prompt_cache_miss_tokens: usage
                .get("prompt_cache_miss_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        });
    }
    None
}

fn format_sse_chunk(value: &Value) -> Option<Bytes> {
    let json = serde_json::to_string(value).ok()?;
    Some(Bytes::from(format!("data: {json}\n\n")))
}

fn build_tool_reverse_map(original: &Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(tools) = original.get("tools").and_then(|v| v.as_array()) else {
        return out;
    };
    for tool in tools {
        if tool.get("type").and_then(|v| v.as_str()) != Some("function") {
            continue;
        }
        if let Some(name) = tool.pointer("/function/name").and_then(|v| v.as_str()) {
            out.insert(name.to_string(), name.to_string());
        }
    }
    out
}

fn restore_tool_name(name: &str, reverse: &HashMap<String, String>) -> String {
    reverse
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

impl ResponseTranslator for CodexSseTranslator {
    fn translate_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        CodexSseTranslator::translate_chunk(self, chunk)
    }

    fn finalize_non_stream(&mut self, raw_sse: &[u8]) -> Option<Vec<u8>> {
        CodexSseTranslator::finalize_non_stream(self, raw_sse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_codex_passthrough_responses_input_when_no_messages() {
        let payload = json!({
            "model": "gpt-5.4-mini",
            "stream": true,
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "hello from cursor"}],
            }],
            "tools": [{"type": "function", "name": "read_file", "parameters": {"type": "object"}}],
        });
        let prepared = prepare_codex_request(
            &payload,
            "gpt-5.4-mini",
            CodexPrepareOptions {
                stable_session_id: Some("client:abc"),
                ..Default::default()
            },
        );
        let input = prepared.payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["content"][0]["text"], "hello from cursor");
        assert_eq!(prepared.payload["prompt_cache_key"], "client:abc");
        assert!(prepared.payload.get("conversation_id").is_none());
        assert_eq!(
            prepared.payload["tools"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn prepare_codex_maps_system_to_instructions() {
        let payload = json!({
            "model": "gpt-5",
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "hi"},
            ],
        });
        let prepared = prepare_codex_request(&payload, "gpt-5", CodexPrepareOptions::default());
        let input = prepared.payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(prepared.payload["instructions"], "sys");
        assert_eq!(prepared.payload["store"], false);
        assert_eq!(prepared.payload["stream"], true);
        assert_eq!(prepared.model, "gpt-5.5");
    }

    #[test]
    fn prepare_codex_parses_cursor_input_text_parts() {
        let payload = json!({
            "model": "gpt-5-codex",
            "messages": [{
                "role": "user",
                "content": [{"type": "input_text", "text": "hello"}],
            }],
        });
        let prepared = prepare_codex_request(&payload, "gpt-5-codex", CodexPrepareOptions::default());
        let input = prepared.payload["input"].as_array().unwrap();
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "hello");
    }

    #[test]
    fn prepare_codex_uses_prompt_cache_key_when_messages_empty() {
        let payload = json!({
            "model": "gpt-5-codex",
            "messages": [],
        });
        let prepared = prepare_codex_request(
            &payload,
            "gpt-5-codex",
            CodexPrepareOptions {
                conversation_id: Some("conv-abc"),
                ..Default::default()
            },
        );
        // Empty messages → fallback synthetic user message injected to avoid Codex 400.
        let input = prepared.payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "message");
        assert!(prepared.payload.get("conversation_id").is_none());
        assert_eq!(prepared.payload["prompt_cache_key"], "conv-abc");
        assert_eq!(prepared.session_id.as_deref(), Some("conv-abc"));
    }

    #[test]
    fn prepare_codex_skips_empty_user_messages() {
        let payload = json!({
            "model": "gpt-5-codex",
            "conversation_id": "c1",
            "messages": [
                {"role": "user", "content": [{"type": "input_text", "text": ""}]},
                {"role": "assistant", "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "read", "arguments": "{}"},
                }]},
            ],
        });
        let prepared = prepare_codex_request(&payload, "gpt-5-codex", CodexPrepareOptions::default());
        let input = prepared.payload["input"].as_array().unwrap();
        // Empty user messages are skipped, but a synthetic fallback is injected to avoid Codex 400.
        let message_items: Vec<_> = input.iter().filter(|item| item.get("type") == Some(&json!("message"))).collect();
        assert_eq!(message_items.len(), 1);
        assert!(prepared.payload.get("conversation_id").is_none());
        assert_eq!(prepared.payload["prompt_cache_key"], "c1");
    }

    #[test]
    fn prepare_codex_falls_back_to_prompt_cache_key_session() {
        let payload = json!({
            "model": "gpt-5-codex",
            "prompt_cache_key": "pck-1",
            "messages": [],
        });
        let prepared = prepare_codex_request(&payload, "gpt-5-codex", CodexPrepareOptions::default());
        assert!(prepared.payload.get("conversation_id").is_none());
        assert_eq!(prepared.payload["prompt_cache_key"], "pck-1");
    }

    #[test]
    fn prepare_codex_uses_stable_session_when_no_conversation_id() {
        let payload = json!({
            "model": "gpt-5-codex",
            "messages": [{"role": "assistant", "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "read", "arguments": "{}"},
            }]}],
        });
        let prepared = prepare_codex_request(
            &payload,
            "gpt-5-codex",
            CodexPrepareOptions {
                stable_session_id: Some("client:abc123"),
                ..Default::default()
            },
        );
        assert!(prepared.payload.get("conversation_id").is_none());
        assert_eq!(prepared.payload["prompt_cache_key"], "client:abc123");
        assert_eq!(prepared.session_id.as_deref(), Some("client:abc123"));
    }

    #[test]
    fn prepare_codex_does_not_put_conversation_id_in_body() {
        let payload = json!({
            "model": "gpt-5-codex",
            "messages": [{"role": "user", "content": "hi"}],
        });
        let prepared = prepare_codex_request(
            &payload,
            "gpt-5-codex",
            CodexPrepareOptions {
                stable_session_id: Some("client:xyz"),
                ..Default::default()
            },
        );
        assert!(prepared.payload.get("conversation_id").is_none());
        assert_eq!(prepared.payload["prompt_cache_key"], "client:xyz");
    }

    #[test]
    fn prepare_codex_client_responses_preserves_input_array() {
        let payload = json!({
            "model": "gpt-5.4-mini",
            "stream": true,
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
        });
        let prepared = prepare_codex_client_responses(
            &payload,
            "gpt-5.4-mini",
            CodexPrepareOptions::default(),
        );
        assert_eq!(prepared.model, "gpt-5.4-mini");
        assert!(prepared.payload.get("input").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty()));
    }

    #[test]
    fn resolve_codex_maps_legacy_slug_to_gpt_5_5() {
        assert_eq!(resolve_codex_upstream_model("gpt-5-codex"), "gpt-5.5");
        assert_eq!(resolve_codex_upstream_model("gpt-5.3-codex"), "gpt-5.5");
        assert_eq!(resolve_codex_upstream_model("gpt-5.3-codex-spark"), "gpt-5.5");
    }

    #[test]
    fn finalize_non_stream_assembles_output_text_deltas() {
        let sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_test\",\"created_at\":1,\"model\":\"gpt-5.5\"}}\n\n",
            "event: message\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"pong\"}\n\n",
            "event: message\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        );
        let mut translator = CodexSseTranslator::new("gpt-5-codex", None);
        let out = translator
            .finalize_non_stream(sse.as_bytes())
            .expect("json body");
        let body: Value = serde_json::from_slice(&out).expect("parse json");
        assert_eq!(body["choices"][0]["message"]["content"], "pong");
    }

    #[test]
    fn extract_codex_usage_from_response_completed_sse() {
        let sse = b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":100,\"output_tokens\":20}}}\n\n";
        let usage = super::extract_codex_usage_from_bytes(sse).expect("usage");
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 20);
    }
}
