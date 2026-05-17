use tracing::debug;

/// Configuration for the semantic cache entry gate.
///
/// The gate decides whether a request is worth embedding and searching
/// the L2 semantic cache, based on query length and cache miss status.
#[derive(Debug, Clone)]
pub struct SemanticGateConfig {
    /// Minimum number of characters required to trigger L2 search.
    /// Queries shorter than this are skipped.
    pub min_query_chars: usize,
    /// Maximum number of characters allowed for L2 search.
    /// Queries longer than this are skipped.
    pub max_query_chars: usize,
    /// When true, L2 search is only attempted when L0/L1 exact cache misses.
    /// When false, L2 is attempted on every request.
    pub embed_only_on_exact_miss: bool,
}

impl Default for SemanticGateConfig {
    fn default() -> Self {
        Self {
            min_query_chars: 32,
            max_query_chars: 8192,
            embed_only_on_exact_miss: true,
        }
    }
}

/// Result of evaluating the semantic gate for a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    /// Proceed with L2 semantic search.
    Pass,
    /// Skip L2 because the query is too short.
    TooShort,
    /// Skip L2 because the query is too long.
    TooLong,
    /// Skip L2 because embed_only_on_exact_miss is set and we had a hit.
    NotExactMiss,
}

/// Evaluate whether a query should proceed to L2 semantic search.
pub fn evaluate_semantic_gate(
    config: &SemanticGateConfig,
    query_text: &str,
    had_l0_l1_hit: bool,
) -> GateDecision {
    if config.embed_only_on_exact_miss && had_l0_l1_hit {
        debug!(
            query_len = query_text.len(),
            "Semantic gate: skipped because L0/L1 hit"
        );
        return GateDecision::NotExactMiss;
    }

    if config.min_query_chars > 0 && query_text.len() < config.min_query_chars {
        debug!(
            query_len = query_text.len(),
            min = config.min_query_chars,
            "Semantic gate: query too short"
        );
        return GateDecision::TooShort;
    }

    if config.max_query_chars > 0 && query_text.len() > config.max_query_chars {
        debug!(
            query_len = query_text.len(),
            max = config.max_query_chars,
            "Semantic gate: query too long"
        );
        return GateDecision::TooLong;
    }

    debug!(query_len = query_text.len(), "Semantic gate: pass");
    GateDecision::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_pass() {
        let config = SemanticGateConfig {
            min_query_chars: 5,
            max_query_chars: 100,
            embed_only_on_exact_miss: false,
        };
        assert_eq!(
            evaluate_semantic_gate(&config, "hello world", false),
            GateDecision::Pass
        );
    }

    #[test]
    fn test_gate_too_short() {
        let config = SemanticGateConfig {
            min_query_chars: 10,
            max_query_chars: 100,
            embed_only_on_exact_miss: false,
        };
        assert_eq!(
            evaluate_semantic_gate(&config, "hi", false),
            GateDecision::TooShort
        );
    }

    #[test]
    fn test_gate_too_long() {
        let config = SemanticGateConfig {
            min_query_chars: 5,
            max_query_chars: 10,
            embed_only_on_exact_miss: false,
        };
        assert_eq!(
            evaluate_semantic_gate(&config, "hello world this is too long", false),
            GateDecision::TooLong
        );
    }

    #[test]
    fn test_gate_not_exact_miss() {
        let config = SemanticGateConfig {
            min_query_chars: 5,
            max_query_chars: 100,
            embed_only_on_exact_miss: true,
        };
        assert_eq!(
            evaluate_semantic_gate(&config, "hello world", true),
            GateDecision::NotExactMiss
        );
    }

    #[test]
    fn test_gate_zero_min_does_not_filter() {
        let config = SemanticGateConfig {
            min_query_chars: 0,
            max_query_chars: 100,
            embed_only_on_exact_miss: false,
        };
        assert_eq!(
            evaluate_semantic_gate(&config, "a", false),
            GateDecision::Pass
        );
    }

    #[test]
    fn test_gate_zero_max_does_not_filter() {
        let config = SemanticGateConfig {
            min_query_chars: 5,
            max_query_chars: 0,
            embed_only_on_exact_miss: false,
        };
        assert_eq!(
            evaluate_semantic_gate(&config, "hello world this is a very long query", false),
            GateDecision::Pass
        );
    }
}
