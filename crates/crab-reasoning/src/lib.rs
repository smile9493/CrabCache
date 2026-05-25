mod backend;
mod keys;
mod normalize;
mod redis_store;
mod store;
mod streaming;
mod transform;

pub use backend::ReasoningBackend;
pub use keys::{
    conversation_scope, message_signature, portable_reasoning_keys, resolve_reasoning_scope,
    scoped_reasoning_keys, tool_call_ids, tool_call_names, tool_call_signature,
};
pub use normalize::{
    GenericPreparedRequest, LightPreparedRequest, PreparedRequest, normalize_messages,
    normalize_mimo_model, normalize_tool_choice_for_deepseek, parse_deepseek_v4_thinking_suffix,
    prepare_generic_request, prepare_light_request, prepare_mimo_request, prepare_upstream_request,
    strip_cursor_thinking_blocks,
};
pub use store::ReasoningStore;
pub use streaming::{
    CursorReasoningDisplayAdapter, StreamAccumulator, fold_reasoning_into_content,
};
pub use transform::{
    RecoveryNoticeContent, completion_message_content_has_thinking_markup,
    record_response_reasoning, response_body_has_thinking_markup, rewrite_response_body,
    rewrite_sse_chunk, sanitize_client_completion, sanitize_client_message_content,
    strip_reasoning_delta_for_client, strip_reasoning_from_completion_value,
    strip_silent_sse_chunk_for_client,
};
