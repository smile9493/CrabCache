//! Request/response format translation layer for CrabCache.
//!
//! Supports wire format detection and translation between common LLM API formats
//! (Chat Completions, Responses API, custom formats). Used to normalize inbound
//! requests into a canonical form before pipeline processing and to translate
//! responses back to the client's expected wire format.

pub mod role_normalization;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during translation.
#[derive(Debug, Error)]
pub enum TranslationError {
    #[error("unsupported wire format: {0}")]
    UnsupportedFormat(String),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("invalid message structure: {0}")]
    InvalidStructure(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Supported wire formats for LLM API endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireFormat {
    /// OpenAI Chat Completions API (`/v1/chat/completions`).
    ChatCompletions,
    /// OpenAI Responses API (`/v1/responses`).
    Responses,
    /// Anthropic Messages API (`/v1/messages`).
    Anthropic,
    /// Internal canonical format (normalized).
    Canonical,
}

impl WireFormat {
    /// Detect the wire format from a request path and method.
    pub fn detect(path: &str, method: &str) -> Self {
        if method == "POST" {
            match path {
                p if p.ends_with("/v1/responses") || p == "/v1/responses" => Self::Responses,
                p if p.ends_with("/v1/messages") || p == "/v1/messages" => Self::Anthropic,
                p if p.ends_with("/v1/chat/completions") || p == "/chat/completions" => {
                    Self::ChatCompletions
                }
                _ => Self::ChatCompletions,
            }
        } else {
            Self::ChatCompletions
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
            Self::Anthropic => "anthropic",
            Self::Canonical => "canonical",
        }
    }

    /// Parse a wire format from a string (case-insensitive).
    /// Defaults to `ChatCompletions` for unrecognized values.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "chat_completions" | "chat" | "completions" => Self::ChatCompletions,
            "responses" => Self::Responses,
            "anthropic" | "messages" => Self::Anthropic,
            "canonical" => Self::Canonical,
            _ => Self::ChatCompletions,
        }
    }
}

/// Declares the translation requirements for a pipeline.
///
/// Each pipeline can specify which wire formats it expects for inbound,
/// outbound, and response directions. Phase 2 will use this to route
/// requests through the appropriate translator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineTranslation {
    /// Wire format the client sends.
    pub inbound: WireFormat,
    /// Wire format sent to the upstream provider.
    pub outbound: WireFormat,
    /// Wire format the upstream returns (translated back to client's format).
    pub response: WireFormat,
}

/// A request translator converts from a wire format to the canonical format.
pub trait RequestTranslator: Send + Sync {
    /// The source wire format this translator handles.
    fn source_format(&self) -> WireFormat;

    /// Translate a request payload into the canonical form.
    fn translate_request(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, TranslationError>;

    /// Check if this translator can handle the given payload.
    fn can_translate(&self, _payload: &serde_json::Value) -> bool {
        true
    }
}

/// A response translator converts from the canonical format back to the wire format.
pub trait ResponseTranslator: Send + Sync {
    /// The target wire format this translator produces.
    fn target_format(&self) -> WireFormat;

    /// Translate a canonical response back to the target wire format.
    fn translate_response(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, TranslationError>;
}

/// Registry of request and response translators, keyed by wire format.
pub struct TranslatorRegistry {
    request_translators: HashMap<WireFormat, Box<dyn RequestTranslator>>,
    response_translators: HashMap<WireFormat, Box<dyn ResponseTranslator>>,
}

impl TranslatorRegistry {
    pub fn new() -> Self {
        Self {
            request_translators: HashMap::new(),
            response_translators: HashMap::new(),
        }
    }

    /// Register a request translator for a wire format.
    pub fn register_request_translator(&mut self, translator: Box<dyn RequestTranslator>) {
        let format = translator.source_format();
        self.request_translators.insert(format, translator);
    }

    /// Register a response translator for a wire format.
    pub fn register_response_translator(&mut self, translator: Box<dyn ResponseTranslator>) {
        let format = translator.target_format();
        self.response_translators.insert(format, translator);
    }

    /// Translate a request from the detected wire format to canonical.
    pub fn translate_request(
        &self,
        format: WireFormat,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, TranslationError> {
        if format == WireFormat::Canonical || format == WireFormat::ChatCompletions {
            return Ok(payload.clone());
        }
        if let Some(translator) = self.request_translators.get(&format) {
            if translator.can_translate(payload) {
                translator.translate_request(payload)
            } else {
                Ok(payload.clone())
            }
        } else {
            Ok(payload.clone())
        }
    }

    /// Translate a response from canonical back to the target wire format.
    pub fn translate_response(
        &self,
        format: WireFormat,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, TranslationError> {
        if format == WireFormat::Canonical || format == WireFormat::ChatCompletions {
            return Ok(payload.clone());
        }
        if let Some(translator) = self.response_translators.get(&format) {
            translator.translate_response(payload)
        } else {
            Ok(payload.clone())
        }
    }
}

impl Default for TranslatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_chat_completions() {
        assert_eq!(
            WireFormat::detect("/v1/chat/completions", "POST"),
            WireFormat::ChatCompletions
        );
    }

    #[test]
    fn detect_responses() {
        assert_eq!(
            WireFormat::detect("/v1/responses", "POST"),
            WireFormat::Responses
        );
    }

    #[test]
    fn detect_anthropic() {
        assert_eq!(
            WireFormat::detect("/v1/messages", "POST"),
            WireFormat::Anthropic
        );
    }

    #[test]
    fn registry_passthrough_for_chat_completions() {
        let registry = TranslatorRegistry::new();
        let payload = serde_json::json!({"model": "test"});
        let result = registry
            .translate_request(WireFormat::ChatCompletions, &payload)
            .unwrap();
        assert_eq!(result, payload);
    }

    #[test]
    fn registry_passthrough_for_unknown_format() {
        let registry = TranslatorRegistry::new();
        let payload = serde_json::json!({"model": "test"});
        let result = registry
            .translate_request(WireFormat::Anthropic, &payload)
            .unwrap();
        assert_eq!(result, payload);
    }
}
