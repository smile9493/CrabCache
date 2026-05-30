//! Translate OpenAI Responses API wire (`POST /v1/responses`) to Chat Completions for
//! non-Codex upstreams (DeepSeek, MiMo, …) and back on the response path.

use bytes::Bytes;
use crab_pipeline::RequestPipeline;
use serde_json::{Map, Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::{ClientWireApi, GatewayContext};
use crate::sse::parse_sse_chunk;

pub fn needs_responses_wire_translate(ctx: &GatewayContext) -> bool {
    ctx.client_wire_api == ClientWireApi::Responses
        && ctx.request_pipeline != Some(RequestPipeline::CodexRelay)
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

/// Stateful translator: Chat Completions SSE → Responses API SSE for Codex-style clients.
pub struct ChatToResponsesSseTranslator {
    response_id: String,
    created_at: i64,
    model: String,
    created_sent: bool,
    completed_sent: bool,
    function_call_index: i32,
    pending_usage: Option<Value>,
}

impl ChatToResponsesSseTranslator {
    pub fn new(model: &str) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Self {
            response_id: format!("resp_{}", uuid::Uuid::new_v4().simple()),
            created_at,
            model: model.to_string(),
            created_sent: false,
            completed_sent: false,
            function_call_index: -1,
            pending_usage: None,
        }
    }

    pub fn translate_chunk(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for event in parse_sse_chunk(chunk) {
            if event.is_done() {
                self.emit_completed(&mut out);
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(event.data) else {
                continue;
            };
            if let Some(usage) = value.get("usage") {
                self.pending_usage = Some(usage.clone());
            }
            self.ensure_created(&mut out);
            let choice = value.pointer("/choices/0");
            let delta = choice.and_then(|c| c.get("delta"));
            if let Some(reasoning) = delta
                .and_then(|d| d.get("reasoning_content"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                append_responses_event(
                    &mut out,
                    "message",
                    json!({
                        "type": "response.reasoning_summary_text.delta",
                        "delta": reasoning,
                    }),
                );
            }
            if let Some(content) = delta
                .and_then(|d| d.get("content"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                append_responses_event(
                    &mut out,
                    "message",
                    json!({ "type": "response.output_text.delta", "delta": content }),
                );
            }
            if let Some(tool_calls) = delta.and_then(|d| d.get("tool_calls")).and_then(|v| v.as_array())
            {
                for tc in tool_calls {
                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
                    {
                        self.function_call_index += 1;
                        let name = tc
                            .pointer("/function/name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        append_responses_event(
                            &mut out,
                            "message",
                            json!({
                                "type": "response.output_item.added",
                                "item": {
                                    "type": "function_call",
                                    "call_id": id,
                                    "name": name,
                                }
                            }),
                        );
                    }
                    if let Some(args) = tc
                        .pointer("/function/arguments")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        append_responses_event(
                            &mut out,
                            "message",
                            json!({
                                "type": "response.function_call_arguments.delta",
                                "delta": args,
                            }),
                        );
                    }
                }
            }
            if choice
                .and_then(|c| c.get("finish_reason"))
                .and_then(|v| v.as_str())
                .is_some_and(|f| !f.is_empty() && f != "null")
            {
                self.emit_completed(&mut out);
            }
        }
        out
    }

    pub fn flush(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        self.emit_completed(&mut out);
        out
    }

    fn ensure_created(&mut self, out: &mut Vec<u8>) {
        if self.created_sent {
            return;
        }
        self.created_sent = true;
        append_responses_event(
            out,
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "created_at": self.created_at,
                    "model": self.model,
                    "status": "in_progress",
                }
            }),
        );
    }

    fn emit_completed(&mut self, out: &mut Vec<u8>) {
        if self.completed_sent {
            return;
        }
        self.completed_sent = true;
        let usage = self
            .pending_usage
            .take()
            .as_ref()
            .map(chat_usage_to_responses)
            .unwrap_or_else(|| json!({ "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }));
        append_responses_event(
            out,
            "message",
            json!({
                "type": "response.completed",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "created_at": self.created_at,
                    "model": self.model,
                    "status": "completed",
                    "usage": usage,
                }
            }),
        );
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

fn append_responses_event(out: &mut Vec<u8>, event: &str, data: Value) {
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
    fn chat_sse_translates_to_responses_events() {
        let sse = concat!(
            "data: ",
            "{\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: ",
            "{\"id\":\"1\",\"choices\":[{\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n",
        );
        let mut tr = ChatToResponsesSseTranslator::new("deepseek-v4-pro");
        let out = tr.translate_chunk(sse.as_bytes());
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("response.created"));
        assert!(text.contains("response.output_text.delta"));
        assert!(text.contains("response.completed"));
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
}
