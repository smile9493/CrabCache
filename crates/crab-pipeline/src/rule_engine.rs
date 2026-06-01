use crate::client_kind::{matches_model_pattern, ClientKind};
use crate::types::{RequestPipeline, UpstreamProvider};
use crab_translator::WireFormat;
use serde::{Deserialize, Serialize};

/// A single declarative pipeline selection rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRule {
    /// Rule name (for logging / diagnostics).
    pub name: String,
    /// Match conditions — ALL must be satisfied for the rule to fire.
    #[serde(rename = "match")]
    pub match_conditions: PipelineMatchConditions,
    /// Pipeline to select when this rule matches.
    pub pipeline: RequestPipeline,
    /// Priority (lower = higher priority; default 100).
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_priority() -> u32 {
    100
}

/// Match conditions for a pipeline rule. All fields are AND-ed.
/// `None` means "match any" (wildcard).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineMatchConditions {
    /// Client kind filter (None = any client).
    pub client: Option<Vec<ClientKind>>,
    /// Upstream provider filter (None = any provider).
    pub provider: Option<Vec<UpstreamProvider>>,
    /// Model name glob patterns (None = any model).
    /// Supports `*` at end (prefix) or beginning (suffix).
    pub model_pattern: Option<Vec<String>>,
    /// Wire format filter (None = any format).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_format: Option<Vec<WireFormat>>,
}

/// Input context for rule engine matching (extracted from PipelineRequestContext).
pub struct RuleMatchInput<'a> {
    pub client_kind: ClientKind,
    pub provider: UpstreamProvider,
    pub model: &'a str,
    pub wire_format: WireFormat,
}

/// Declarative pipeline rule engine — replaces hardcoded if/match chains in `select.rs`.
///
/// Rules are evaluated in priority order (ascending). The first rule whose
/// ALL match conditions are satisfied wins. If no rule matches, the engine
/// returns `None` and the caller falls back to `auto_pipeline_legacy`.
#[derive(Debug, Clone)]
pub struct PipelineRuleEngine {
    /// Rules sorted by priority (ascending).
    rules: Vec<PipelineRule>,
}

impl PipelineRuleEngine {
    /// Create a new rule engine from a list of rules.
    /// Rules are sorted by priority (ascending) on construction.
    pub fn new(mut rules: Vec<PipelineRule>) -> Self {
        rules.sort_by_key(|r| r.priority);
        Self { rules }
    }

    /// Evaluate rules against the given input.
    /// Returns the first matching pipeline and the rule name, or `None` if no rule matches.
    pub fn select(&self, input: &RuleMatchInput<'_>) -> Option<(RequestPipeline, &str)> {
        for rule in &self.rules {
            if rule_matches(&rule.match_conditions, input) {
                return Some((rule.pipeline, rule.name.as_str()));
            }
        }
        None
    }

    /// Return the current rule list (for serialization / Management API).
    pub fn rules(&self) -> &[PipelineRule] {
        &self.rules
    }

    /// Build a default rule engine that replicates `auto_pipeline_legacy` behavior.
    pub fn default_rules() -> Self {
        Self::new(vec![
            PipelineRule {
                name: "cursor_deepseek_v4".into(),
                priority: 10,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Cursor]),
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: Some(vec!["deepseek-v4-*".into()]),
                    wire_format: None,
                },
                pipeline: RequestPipeline::CursorDeepSeekV4,
            },
            PipelineRule {
                name: "codex_relay".into(),
                priority: 20,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Codex]),
                    provider: Some(vec![UpstreamProvider::Codex]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::CodexRelay,
            },
            PipelineRule {
                name: "codex_to_deepseek".into(),
                priority: 25,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Codex]),
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: Some(vec!["deepseek-*".into()]),
                    wire_format: None,
                },
                pipeline: RequestPipeline::CodexDeepSeek,
            },
            PipelineRule {
                name: "codex_to_mimo".into(),
                priority: 26,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Codex]),
                    provider: Some(vec![UpstreamProvider::Mimo]),
                    model_pattern: Some(vec!["mimo-*".into(), "xiaomi/*".into()]),
                    wire_format: None,
                },
                pipeline: RequestPipeline::CodexMimo,
            },
            PipelineRule {
                name: "deepseek_light".into(),
                priority: 30,
                match_conditions: PipelineMatchConditions {
                    client: None,
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::DeepSeekLight,
            },
            PipelineRule {
                name: "mimo_relay".into(),
                priority: 40,
                match_conditions: PipelineMatchConditions {
                    client: None,
                    provider: Some(vec![UpstreamProvider::Mimo]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::MimoTokenPlanRelay,
            },
        ])
    }
}

/// Check if a rule's match conditions are all satisfied.
fn rule_matches(conditions: &PipelineMatchConditions, input: &RuleMatchInput<'_>) -> bool {
    // Client kind filter
    if let Some(ref clients) = conditions.client {
        if !clients.contains(&input.client_kind) {
            return false;
        }
    }

    // Provider filter
    if let Some(ref providers) = conditions.provider {
        if !providers.contains(&input.provider) {
            return false;
        }
    }

    // Model pattern filter (OR: any pattern matching is sufficient)
    if let Some(ref patterns) = conditions.model_pattern {
        if !patterns
            .iter()
            .any(|p| matches_model_pattern(input.model, p))
        {
            return false;
        }
    }

    // Wire format filter
    if let Some(ref formats) = conditions.wire_format {
        if !formats.contains(&input.wire_format) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_input(client_kind: ClientKind, provider: UpstreamProvider, model: &str) -> RuleMatchInput<'_> {
        RuleMatchInput {
            client_kind,
            provider,
            model,
            wire_format: WireFormat::ChatCompletions,
        }
    }

    fn rule_engine_with_legacy_defaults() -> PipelineRuleEngine {
        PipelineRuleEngine::new(vec![
            PipelineRule {
                name: "cursor_deepseek_v4".into(),
                priority: 10,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Cursor]),
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: Some(vec!["deepseek-v4-*".into()]),
                    wire_format: None,
                },
                pipeline: RequestPipeline::CursorDeepSeekV4,
            },
            PipelineRule {
                name: "codex_relay".into(),
                priority: 20,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Codex]),
                    provider: Some(vec![UpstreamProvider::Codex]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::CodexRelay,
            },
            PipelineRule {
                name: "codex_to_deepseek".into(),
                priority: 25,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Codex]),
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: Some(vec!["deepseek-*".into()]),
                    wire_format: None,
                },
                pipeline: RequestPipeline::CodexDeepSeek,
            },
            PipelineRule {
                name: "codex_to_mimo".into(),
                priority: 26,
                match_conditions: PipelineMatchConditions {
                    client: Some(vec![ClientKind::Codex]),
                    provider: Some(vec![UpstreamProvider::Mimo]),
                    model_pattern: Some(vec!["mimo-*".into(), "xiaomi/*".into()]),
                    wire_format: None,
                },
                pipeline: RequestPipeline::CodexMimo,
            },
            PipelineRule {
                name: "deepseek_light".into(),
                priority: 30,
                match_conditions: PipelineMatchConditions {
                    client: None,
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::DeepSeekLight,
            },
            PipelineRule {
                name: "mimo_relay".into(),
                priority: 40,
                match_conditions: PipelineMatchConditions {
                    client: None,
                    provider: Some(vec![UpstreamProvider::Mimo]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::MimoTokenPlanRelay,
            },
        ])
    }

    #[test]
    fn cursor_deepseek_v4_matches() {
        let engine = rule_engine_with_legacy_defaults();
        let input = default_input(ClientKind::Cursor, UpstreamProvider::Deepseek, "deepseek-v4-pro");
        let (pipeline, name) = engine.select(&input).unwrap();
        assert_eq!(pipeline, RequestPipeline::CursorDeepSeekV4);
        assert_eq!(name, "cursor_deepseek_v4");
    }

    #[test]
    fn deepseek_chat_matches_light() {
        let engine = rule_engine_with_legacy_defaults();
        let input = default_input(ClientKind::Generic, UpstreamProvider::Deepseek, "deepseek-chat");
        let (pipeline, name) = engine.select(&input).unwrap();
        assert_eq!(pipeline, RequestPipeline::DeepSeekLight);
        assert_eq!(name, "deepseek_light");
    }

    #[test]
    fn codex_relay_matches() {
        let engine = rule_engine_with_legacy_defaults();
        let input = default_input(ClientKind::Codex, UpstreamProvider::Codex, "gpt-5");
        let (pipeline, name) = engine.select(&input).unwrap();
        assert_eq!(pipeline, RequestPipeline::CodexRelay);
        assert_eq!(name, "codex_relay");
    }

    #[test]
    fn codex_deepseek_matches() {
        let engine = rule_engine_with_legacy_defaults();
        let input = default_input(ClientKind::Codex, UpstreamProvider::Deepseek, "deepseek-v4-pro");
        let (pipeline, _) = engine.select(&input).unwrap();
        assert_eq!(pipeline, RequestPipeline::CodexDeepSeek);
    }

    #[test]
    fn codex_mimo_matches() {
        let engine = rule_engine_with_legacy_defaults();
        let input = default_input(ClientKind::Codex, UpstreamProvider::Mimo, "mimo-v2.5-pro");
        let (pipeline, _) = engine.select(&input).unwrap();
        assert_eq!(pipeline, RequestPipeline::CodexMimo);
    }

    #[test]
    fn mimo_generic_matches_relay() {
        let engine = rule_engine_with_legacy_defaults();
        let input = default_input(ClientKind::Generic, UpstreamProvider::Mimo, "mimo-v2-flash");
        let (pipeline, name) = engine.select(&input).unwrap();
        assert_eq!(pipeline, RequestPipeline::MimoTokenPlanRelay);
        assert_eq!(name, "mimo_relay");
    }

    #[test]
    fn no_match_returns_none() {
        let engine = rule_engine_with_legacy_defaults();
        let input = default_input(ClientKind::Generic, UpstreamProvider::Openai, "gpt-4o");
        assert!(engine.select(&input).is_none());
    }

    #[test]
    fn priority_ordering() {
        // A rule with lower priority number wins
        let engine = PipelineRuleEngine::new(vec![
            PipelineRule {
                name: "low_priority".into(),
                priority: 100,
                match_conditions: PipelineMatchConditions {
                    client: None,
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::DeepSeekLight,
            },
            PipelineRule {
                name: "high_priority".into(),
                priority: 1,
                match_conditions: PipelineMatchConditions {
                    client: None,
                    provider: Some(vec![UpstreamProvider::Deepseek]),
                    model_pattern: None,
                    wire_format: None,
                },
                pipeline: RequestPipeline::CursorDeepSeekV4,
            },
        ]);
        let input = default_input(ClientKind::Generic, UpstreamProvider::Deepseek, "deepseek-chat");
        let (_, name) = engine.select(&input).unwrap();
        assert_eq!(name, "high_priority");
    }

    #[test]
    fn wire_format_filter_matches() {
        let engine = PipelineRuleEngine::new(vec![PipelineRule {
            name: "responses_only".into(),
            priority: 10,
            match_conditions: PipelineMatchConditions {
                client: None,
                provider: None,
                model_pattern: None,
                wire_format: Some(vec![WireFormat::Responses]),
            },
            pipeline: RequestPipeline::CodexRelay,
        }]);
        // Responses format matches
        let input = RuleMatchInput {
            client_kind: ClientKind::Generic,
            provider: UpstreamProvider::Other,
            model: "gpt-5",
            wire_format: WireFormat::Responses,
        };
        assert!(engine.select(&input).is_some());
        // ChatCompletions format does not match
        let input2 = default_input(ClientKind::Generic, UpstreamProvider::Other, "gpt-5");
        assert!(engine.select(&input2).is_none());
    }

    #[test]
    fn serialization_roundtrip() {
        let engine = rule_engine_with_legacy_defaults();
        let json = serde_json::to_string(&engine.rules()[0]).unwrap();
        let rule: PipelineRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule.name, "cursor_deepseek_v4");
        assert_eq!(rule.priority, 10);
    }
}
