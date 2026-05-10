use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trace-based load pattern parameters extracted from production logs.
///
/// These parameters drive the statistical load generator to produce
/// request sequences that approximate real-world behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadPattern {
    /// Number of unique queries in the working set.
    pub unique_queries: usize,

    /// Zipf distribution alpha parameter (skewness).
    /// 1.0 = moderate skew, 1.5 = highly concentrated.
    pub zipf_alpha: f64,

    /// Fraction of requests carrying a conversation_id (affects routing).
    pub conversation_ratio: f64,

    /// Fraction of requests that belong to semantic clusters.
    pub semantic_cluster_ratio: f64,

    /// Target requests per second (Poisson arrival rate lambda).
    pub arrival_rate: f64,

    /// Fraction of requests that are exact repeats (temporal locality).
    pub repeat_ratio: f64,

    /// Number of concurrent clients.
    pub concurrency: usize,
}

impl Default for LoadPattern {
    fn default() -> Self {
        Self {
            unique_queries: 1000,
            zipf_alpha: 1.2,
            conversation_ratio: 0.4,
            semantic_cluster_ratio: 0.15,
            arrival_rate: 100.0,
            repeat_ratio: 0.3,
            concurrency: 50,
        }
    }
}

impl LoadPattern {
    /// Conservative pattern for cold-start testing.
    pub fn cold_start() -> Self {
        Self {
            unique_queries: 5000,
            zipf_alpha: 0.8,
            conversation_ratio: 0.1,
            semantic_cluster_ratio: 0.05,
            arrival_rate: 50.0,
            repeat_ratio: 0.05,
            concurrency: 20,
        }
    }

    /// Hot-cache pattern for steady-state testing.
    pub fn steady_state() -> Self {
        Self {
            unique_queries: 500,
            zipf_alpha: 1.5,
            conversation_ratio: 0.6,
            semantic_cluster_ratio: 0.25,
            arrival_rate: 200.0,
            repeat_ratio: 0.6,
            concurrency: 100,
        }
    }
}

/// A single request event in a trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub timestamp_ms: u64,
    pub conversation_id: Option<String>,
    pub model: String,
    pub messages: Vec<Message>,
    /// Semantic group ID for L2 cache testing.
    /// Requests with the same semantic_group should hit L2 even if text differs.
    pub semantic_group: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Load generator that produces request sequences based on statistical patterns.
///
/// This does NOT implement cache logic — it only generates realistic request workloads
/// that can be fed into the real CrabCache system for testing.
pub struct TraceGenerator {
    pattern: LoadPattern,
    query_templates: Vec<String>,
    semantic_clusters: Vec<Vec<String>>,
    rng: rand::rngs::StdRng,
}

impl TraceGenerator {
    pub fn new(pattern: LoadPattern, seed: u64) -> Self {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let query_templates = Self::generate_templates(&pattern, &mut rng);
        let semantic_clusters = Self::generate_semantic_clusters(&query_templates, &mut rng);

        Self {
            pattern,
            query_templates,
            semantic_clusters,
            rng,
        }
    }

    /// Generate the next request event.
    pub fn next_event(&mut self, index: usize) -> TraceEvent {
        let is_repeat = self.rng.r#gen::<f64>() < self.pattern.repeat_ratio;
        let is_conversation = self.rng.r#gen::<f64>() < self.pattern.conversation_ratio;
        let is_semantic = self.rng.r#gen::<f64>() < self.pattern.semantic_cluster_ratio;

        let conversation_id = if is_conversation {
            Some(format!("conv-{}", self.rng.r#gen::<u64>() % 100))
        } else {
            None
        };

        let (content, semantic_group) = if is_semantic && !self.semantic_clusters.is_empty() {
            let cluster_idx = self.rng.r#gen::<usize>() % self.semantic_clusters.len();
            let variant_idx = self.rng.r#gen::<usize>() % self.semantic_clusters[cluster_idx].len();
            (
                self.semantic_clusters[cluster_idx][variant_idx].clone(),
                Some(cluster_idx),
            )
        } else if is_repeat {
            let idx = self.zipf_sample();
            (self.query_templates[idx].clone(), None)
        } else {
            let idx = self.zipf_sample();
            (
                format!("{} (variant {})", self.query_templates[idx], index),
                None,
            )
        };

        TraceEvent {
            timestamp_ms: index as u64 * 1000 / self.pattern.arrival_rate as u64,
            conversation_id,
            model: "deepseek-v4-pro".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content,
            }],
            semantic_group,
        }
    }

    /// Generate a sequence of events.
    pub fn generate_sequence(&mut self, count: usize) -> Vec<TraceEvent> {
        (0..count).map(|i| self.next_event(i)).collect()
    }

    fn zipf_sample(&mut self) -> usize {
        let n = self.query_templates.len() as f64;
        let alpha = self.pattern.zipf_alpha;
        let max_prob = 1.0f64;

        loop {
            let rank = self.rng.r#gen::<f64>() * n;
            let prob = (1.0 / (rank + 1.0)).powf(alpha);
            if self.rng.r#gen::<f64>() < prob / max_prob {
                return (rank as usize).min(self.query_templates.len() - 1);
            }
        }
    }

    fn generate_templates(pattern: &LoadPattern, _rng: &mut rand::rngs::StdRng) -> Vec<String> {
        let topics = [
            "quantum computing",
            "machine learning",
            "Rust programming",
            "blockchain",
            "climate change",
            "neural networks",
            "cloud architecture",
            "data structures",
            "API design",
            "distributed systems",
        ];

        (0..pattern.unique_queries)
            .map(|i| {
                let topic = topics[i % topics.len()];
                format!("Explain {} concept {}", topic, i)
            })
            .collect()
    }

    fn generate_semantic_clusters(
        templates: &[String],
        _rng: &mut rand::rngs::StdRng,
    ) -> Vec<Vec<String>> {
        let mut clusters = Vec::new();
        let cluster_count = templates.len() / 20;

        for i in 0..cluster_count {
            let base = templates[i].clone();
            let variants = vec![
                format!("What is {}?", base),
                format!("How does {} work?", base),
                format!("Explain {} in simple terms", base),
                format!("Can you describe {}?", base),
                format!("Tell me about {}", base),
            ];
            clusters.push(variants);
        }

        clusters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_pattern_defaults() {
        let pattern = LoadPattern::default();
        assert_eq!(pattern.unique_queries, 1000);
        assert!(pattern.zipf_alpha > 1.0);
    }

    #[test]
    fn test_trace_generator_determinism() {
        let pattern = LoadPattern::default();
        let mut generator1 = TraceGenerator::new(pattern.clone(), 42);
        let mut generator2 = TraceGenerator::new(pattern, 42);

        let seq1 = generator1.generate_sequence(100);
        let seq2 = generator2.generate_sequence(100);

        assert_eq!(seq1.len(), seq2.len());
        for (a, b) in seq1.iter().zip(seq2.iter()) {
            assert_eq!(a.messages[0].content, b.messages[0].content);
        }
    }

    #[test]
    fn test_zipf_distribution_skew() {
        let pattern = LoadPattern::steady_state();
        let mut generator = TraceGenerator::new(pattern, 42);
        let seq = generator.generate_sequence(10000);

        let mut freq = HashMap::new();
        for event in &seq {
            *freq.entry(event.messages[0].content.clone()).or_insert(0) += 1;
        }

        let max_freq = freq.values().max().copied().unwrap_or(0);
        let min_freq = freq.values().min().copied().unwrap_or(0);

        assert!(
            max_freq > min_freq * 10,
            "Zipf distribution should show significant skew"
        );
    }

    #[test]
    fn test_conversation_ratio() {
        let pattern = LoadPattern {
            conversation_ratio: 0.5,
            ..Default::default()
        };
        let mut generator = TraceGenerator::new(pattern, 42);
        let seq = generator.generate_sequence(1000);

        let with_conv = seq.iter().filter(|e| e.conversation_id.is_some()).count();
        let ratio = with_conv as f64 / seq.len() as f64;

        assert!(
            (ratio - 0.5).abs() < 0.1,
            "Conversation ratio should be approximately 0.5, got {}",
            ratio
        );
    }
}
