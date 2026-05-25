use crate::backend::ReasoningBackend;
use serde_json::Value;
use std::collections::HashMap;

const THINKING_BLOCK_START: &str = "<think>\n";
const THINKING_BLOCK_END: &str = "\n</think>\n\n";
const COLLAPSIBLE_THINKING_BLOCK_START: &str = "<details>\n<summary>Thinking</summary>\n\n";
const COLLAPSIBLE_THINKING_BLOCK_END: &str = "\n</details>\n\n";

#[derive(Debug, Clone, Default)]
struct StreamingChoice {
    role: String,
    content: String,
    reasoning_content: String,
    has_reasoning_content: bool,
    tool_calls: Vec<Value>,
    finish_reason: Option<String>,
}

impl StreamingChoice {
    fn to_message(&self) -> Value {
        let mut msg = serde_json::Map::new();
        msg.insert(
            "role".into(),
            Value::String(if self.role.is_empty() {
                "assistant".to_string()
            } else {
                self.role.clone()
            }),
        );
        msg.insert("content".into(), Value::String(self.content.clone()));
        if self.has_reasoning_content {
            msg.insert(
                "reasoning_content".into(),
                Value::String(self.reasoning_content.clone()),
            );
        }
        if !self.tool_calls.is_empty() {
            msg.insert("tool_calls".into(), Value::Array(self.tool_calls.clone()));
        }
        Value::Object(msg)
    }
}

pub struct StreamAccumulator {
    choices: HashMap<usize, StreamingChoice>,
    stored_choices: HashMap<(usize, String), String>,
}

impl Default for StreamAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self {
            choices: HashMap::new(),
            stored_choices: HashMap::new(),
        }
    }

    pub fn ingest_chunk(&mut self, chunk: &Value) {
        let choices = match chunk.get("choices").and_then(|c| c.as_array()) {
            Some(c) => c,
            None => return,
        };

        for raw_choice in choices {
            if !raw_choice.is_object() {
                continue;
            }
            let index = raw_choice
                .get("index")
                .and_then(|i| i.as_u64())
                .unwrap_or(0) as usize;
            let choice = self.choices.entry(index).or_default();

            if let Some(fr) = raw_choice.get("finish_reason").and_then(|f| f.as_str()) {
                choice.finish_reason = Some(fr.to_string());
            }

            let delta = match raw_choice.get("delta") {
                Some(d) if d.is_object() => d,
                _ => continue,
            };

            if let Some(role) = delta.get("role").and_then(|r| r.as_str()) {
                choice.role = role.to_string();
            }
            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                choice.content.push_str(content);
            }
            if let Some(rc) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                choice.has_reasoning_content = true;
                choice.reasoning_content.push_str(rc);
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
                for raw_delta in tool_calls {
                    if !raw_delta.is_object() {
                        continue;
                    }
                    let tc_index = raw_delta
                        .get("index")
                        .and_then(|i| i.as_u64())
                        .unwrap_or(choice.tool_calls.len() as u64)
                        as usize;
                    while choice.tool_calls.len() <= tc_index {
                        choice.tool_calls.push(serde_json::json!({"type": "function", "function": {"name": "", "arguments": ""}}));
                    }
                    let tool_call = &mut choice.tool_calls[tc_index];
                    if let Some(tc) = tool_call.as_object_mut() {
                        if let Some(id) = raw_delta.get("id") {
                            tc.insert("id".into(), id.clone());
                        }
                        if let Some(t) = raw_delta.get("type") {
                            tc.insert("type".into(), t.clone());
                        }
                        if let Some(func_delta) = raw_delta.get("function") {
                            let func = tc.entry(String::from("function")).or_insert_with(|| {
                                let mut m = serde_json::Map::new();
                                m.insert("name".into(), Value::String(String::new()));
                                m.insert("arguments".into(), Value::String(String::new()));
                                Value::Object(m)
                            });
                            if let Some(func_obj) = func.as_object_mut() {
                                if let Some(name) = func_delta.get("name").and_then(|n| n.as_str())
                                {
                                    let existing = func_obj
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    func_obj.insert(
                                        "name".into(),
                                        Value::String(if existing.is_empty() {
                                            name.to_string()
                                        } else {
                                            format!("{existing}{name}")
                                        }),
                                    );
                                }
                                if let Some(args) = func_delta.get("arguments") {
                                    let existing = func_obj
                                        .get("arguments")
                                        .and_then(|a| a.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let new_args = args.as_str().unwrap_or("");
                                    func_obj.insert(
                                        "arguments".into(),
                                        Value::String(format!("{existing}{new_args}")),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn store_reasoning(
        &mut self,
        store: &ReasoningBackend,
        scope: &str,
        cache_namespace: &str,
        prior_messages: &[Value],
    ) -> usize {
        let messages: Vec<Value> = self.messages();
        let mut stored = 0;
        for (index, msg) in messages.iter().enumerate() {
            let stage_rank = |s: &str| -> u8 {
                match s {
                    "tool_call" => 1,
                    "final" => 2,
                    _ => 0,
                }
            };
            let storage_key = (index, scope.to_string());
            if let Some(previous) = self.stored_choices.get(&storage_key) {
                if stage_rank(previous) >= stage_rank("final") {
                    continue;
                }
            }
            let count = store.store_assistant_message(msg, scope, cache_namespace, prior_messages);
            if count > 0 {
                self.stored_choices.insert(storage_key, "final".to_string());
            }
            stored += count;
        }
        stored
    }

    pub fn store_ready_reasoning(
        &mut self,
        store: &ReasoningBackend,
        scope: &str,
        cache_namespace: &str,
        prior_messages: &[Value],
    ) -> usize {
        let messages: Vec<Value> = self.messages();
        let mut stored = 0;
        for (index, msg) in messages.iter().enumerate() {
            let choice = self.choices.get(&index);
            let stage = if choice.is_some_and(|c| c.finish_reason.is_some()) {
                "final"
            } else if choice.is_some_and(has_identified_tool_calls) {
                "tool_call"
            } else {
                continue;
            };

            let stage_rank = |s: &str| -> u8 {
                match s {
                    "tool_call" => 1,
                    "final" => 2,
                    _ => 0,
                }
            };
            let storage_key = (index, scope.to_string());
            if let Some(previous) = self.stored_choices.get(&storage_key) {
                if stage_rank(previous) >= stage_rank(stage) {
                    continue;
                }
            }
            let count = store.store_assistant_message(msg, scope, cache_namespace, prior_messages);
            if count > 0 {
                self.stored_choices.insert(storage_key, stage.to_string());
            }
            stored += count;
        }
        stored
    }

    pub fn messages(&self) -> Vec<Value> {
        let mut entries: Vec<_> = self.choices.iter().collect();
        entries.sort_by_key(|(idx, _)| *idx);
        entries
            .iter()
            .map(|(_, choice)| choice.to_message())
            .collect()
    }
}

fn has_identified_tool_calls(choice: &StreamingChoice) -> bool {
    if !choice.has_reasoning_content || choice.tool_calls.is_empty() {
        return false;
    }
    choice.tool_calls.iter().all(|tc| tc.get("id").is_some())
}

pub struct CursorReasoningDisplayAdapter {
    open_choices: HashMap<usize, bool>,
    last_chunk_metadata: serde_json::Map<String, Value>,
    block_start: String,
    block_end: String,
}

impl CursorReasoningDisplayAdapter {
    pub fn new(collapsible: bool) -> Self {
        Self {
            open_choices: HashMap::new(),
            last_chunk_metadata: serde_json::Map::new(),
            block_start: if collapsible {
                COLLAPSIBLE_THINKING_BLOCK_START.to_string()
            } else {
                THINKING_BLOCK_START.to_string()
            },
            block_end: if collapsible {
                COLLAPSIBLE_THINKING_BLOCK_END.to_string()
            } else {
                THINKING_BLOCK_END.to_string()
            },
        }
    }

    pub fn rewrite_chunk(&mut self, chunk: &mut Value) {
        self.remember_chunk_metadata(chunk);

        let choices = match chunk.get_mut("choices").and_then(|c| c.as_array_mut()) {
            Some(c) => c,
            None => return,
        };

        for raw_choice in choices.iter_mut() {
            if !raw_choice.is_object() {
                continue;
            }

            let index = raw_choice
                .get("index")
                .and_then(|i| i.as_u64())
                .unwrap_or(0) as usize;

            let (reasoning_content, existing_content, has_tool_calls, has_finish) = {
                let delta = match raw_choice.get("delta") {
                    Some(d) if d.is_object() => d,
                    _ => continue,
                };
                let rc = delta
                    .get("reasoning_content")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                let ec = delta
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let htc = delta.get("tool_calls").is_some();
                let hf = raw_choice.get("finish_reason").is_some();
                (rc, ec, htc, hf)
            };

            if let Some(delta) = raw_choice.get_mut("delta") {
                if let Some(obj) = delta.as_object_mut() {
                    // OpenAI-compatible streaming: incremental `delta.content` only (no
                    // per-chunk <details> wrappers). Cursor closes the connection otherwise.
                    if !reasoning_content.is_empty() {
                        if !self.open_choices.contains_key(&index) {
                            self.open_choices.insert(index, true);
                        }
                        if existing_content.is_empty() {
                            obj.insert("content".into(), Value::String(reasoning_content));
                        } else {
                            obj.insert(
                                "content".into(),
                                Value::String(format!("{reasoning_content}{existing_content}")),
                            );
                        }
                    } else if obj.get("role").is_some() && !obj.contains_key("content") {
                        obj.insert("content".into(), Value::String(String::new()));
                    } else if !existing_content.is_empty() {
                        obj.insert("content".into(), Value::String(existing_content));
                    }
                    // Upstream may send `reasoning_content: null` on role chunks; Cursor rejects it.
                    obj.remove("reasoning_content");
                    if obj.get("content").map(|v| v.is_null()).unwrap_or(false) {
                        obj.insert("content".into(), Value::String(String::new()));
                    }

                    let should_close =
                        self.open_choices.contains_key(&index) && (has_tool_calls || has_finish);
                    if should_close {
                        self.open_choices.remove(&index);
                    }
                }
            }
        }
    }

    pub fn flush_chunk(&mut self, model: &str) -> Option<Value> {
        if self.open_choices.is_empty() {
            return None;
        }
        // OpenAI-compatible streams do not need a synthetic closing HTML block.
        self.open_choices.clear();
        let _ = model;
        None
    }

    fn remember_chunk_metadata(&mut self, chunk: &Value) {
        for key in &["id", "object", "created"] {
            if let Some(val) = chunk.get(key) {
                self.last_chunk_metadata
                    .insert(key.to_string(), val.clone());
            }
        }
    }
}

pub fn fold_reasoning_into_content(response_payload: &mut Value, collapsible: bool) {
    let block_start = if collapsible {
        COLLAPSIBLE_THINKING_BLOCK_START
    } else {
        THINKING_BLOCK_START
    };
    let block_end = if collapsible {
        COLLAPSIBLE_THINKING_BLOCK_END
    } else {
        THINKING_BLOCK_END
    };

    let choices = match response_payload
        .get_mut("choices")
        .and_then(|c| c.as_array_mut())
    {
        Some(c) => c,
        None => return,
    };

    for choice in choices.iter_mut() {
        if !choice.is_object() {
            continue;
        }
        let (reasoning, content) = {
            let message = match choice.get("message") {
                Some(m) if m.is_object() => m,
                _ => continue,
            };
            let r = message
                .get("reasoning_content")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            let c = message
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            (r, c)
        };
        if reasoning.is_empty() {
            continue;
        }
        if let Some(message) = choice.get_mut("message") {
            if let Some(obj) = message.as_object_mut() {
                obj.insert(
                    "content".into(),
                    Value::String(format!("{block_start}{reasoning}{block_end}{content}")),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_accumulator_ingest() {
        let mut acc = StreamAccumulator::new();
        let chunk = serde_json::json!({"choices": [{"index": 0, "delta": {"role": "assistant", "content": "Hello"}, "finish_reason": None::<String>}]});
        acc.ingest_chunk(&chunk);
        let msgs = acc.messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0].get("content").and_then(|c| c.as_str()),
            Some("Hello")
        );
    }

    #[test]
    fn store_ready_reasoning_on_tool_call_before_finish() {
        let store =
            ReasoningBackend::open_sqlite(":memory:", Some(3600), Some(1000)).expect("memory db");
        let mut acc = StreamAccumulator::new();
        acc.ingest_chunk(&serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "reasoning_content": "Need tool.",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "read", "arguments": "{}"}
                    }]
                }
            }]
        }));
        let scope = crate::keys::conversation_scope(
            &[serde_json::json!({"role": "user", "content": "go"})],
            "ns-test",
        );
        let stored = acc.store_ready_reasoning(&store, &scope, "ns-test", &[]);
        assert!(stored > 0);
        assert_eq!(
            store.get(&format!("scope:{scope}:tool_call:call_1")),
            Some("Need tool.".to_string())
        );
    }

    #[test]
    fn cursor_adapter_strips_null_reasoning_content() {
        let mut adapter = CursorReasoningDisplayAdapter::new(true);
        let mut chunk = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": "", "reasoning_content": null}
            }]
        });
        adapter.rewrite_chunk(&mut chunk);
        let delta = &chunk["choices"][0]["delta"];
        assert!(delta.get("reasoning_content").is_none());
        assert_eq!(delta.get("content").and_then(|c| c.as_str()), Some(""));
    }

    #[test]
    fn cursor_adapter_preserves_content_when_reasoning_also_present() {
        let mut adapter = CursorReasoningDisplayAdapter::new(true);
        let mut chunk = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": "actual answer", "reasoning_content": "thinking..."}
            }]
        });
        adapter.rewrite_chunk(&mut chunk);
        let delta = &chunk["choices"][0]["delta"];
        assert!(
            delta.get("reasoning_content").is_none(),
            "reasoning_content must be removed"
        );
        assert_eq!(
            delta.get("content").and_then(|c| c.as_str()),
            Some("thinking...actual answer"),
            "both reasoning and original content must be preserved"
        );
    }

    #[test]
    fn test_fold_reasoning_into_content() {
        let mut payload = serde_json::json!({"choices": [{"message": {"role": "assistant", "content": "result", "reasoning_content": "I thought"}}]});
        fold_reasoning_into_content(&mut payload, true);
        let content = payload
            .get("choices")
            .unwrap()
            .get(0)
            .unwrap()
            .get("message")
            .unwrap()
            .get("content")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(content.contains("I thought"));
        assert!(content.contains("<details>"));
        assert!(content.contains("result"));
    }
}
