use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Real trace record from API calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceRecord {
    pub timestamp_ms: u64,
    pub request_id: String,
    pub model: String,
    pub prompt: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub latency_ms: f64,
    pub cache_hit: bool,
    pub conversation_id: Option<String>,
}

/// Statistics extracted from real trace data.
#[derive(Debug, Clone, Default)]
pub struct TraceStats {
    pub total_requests: usize,
    pub unique_prompts: usize,
    pub total_tokens: usize,
    pub avg_latency_ms: f64,
    pub cache_hit_rate: f64,
    pub prompt_frequency: HashMap<String, usize>,
    pub conversation_distribution: HashMap<String, usize>,
}

impl TraceStats {
    pub fn from_records(records: &[TraceRecord]) -> Self {
        let mut stats = Self {
            total_requests: records.len(),
            ..Default::default()
        };

        let mut total_latency = 0.0;
        let mut cache_hits = 0;

        for record in records {
            stats.total_tokens += record.total_tokens;
            total_latency += record.latency_ms;
            if record.cache_hit {
                cache_hits += 1;
            }

            *stats
                .prompt_frequency
                .entry(record.prompt.clone())
                .or_insert(0) += 1;

            if let Some(ref conv_id) = record.conversation_id {
                *stats
                    .conversation_distribution
                    .entry(conv_id.clone())
                    .or_insert(0) += 1;
            }
        }

        stats.unique_prompts = stats.prompt_frequency.len();
        stats.avg_latency_ms = if records.is_empty() {
            0.0
        } else {
            total_latency / records.len() as f64
        };
        stats.cache_hit_rate = if records.is_empty() {
            0.0
        } else {
            cache_hits as f64 / records.len() as f64
        };

        stats
    }

    /// Calculate repeat ratio from trace data.
    pub fn repeat_ratio(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        let repeats = self
            .prompt_frequency
            .values()
            .map(|&count| count.saturating_sub(1))
            .sum::<usize>();
        repeats as f64 / self.total_requests as f64
    }

    /// Calculate Zipf alpha parameter from frequency distribution.
    pub fn estimate_zipf_alpha(&self) -> f64 {
        if self.prompt_frequency.len() < 10 {
            return 1.0;
        }

        let mut freqs: Vec<usize> = self.prompt_frequency.values().cloned().collect();
        freqs.sort_by(|a, b| b.cmp(a));

        let n = freqs.len() as f64;
        let sum: f64 = freqs.iter().map(|&f| f as f64).sum();

        if sum == 0.0 {
            return 1.0;
        }

        let harmonic: f64 = (1..=freqs.len()).map(|i| 1.0 / i as f64).sum();

        let log_sum: f64 = freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| ((i + 1) as f64).ln() * (f as f64 / sum))
            .sum();

        let alpha = (n.ln() - harmonic * log_sum) / (n.ln() * harmonic - harmonic.powi(2));
        alpha.clamp(0.5, 2.0)
    }

    /// Calculate conversation concentration ratio.
    pub fn conversation_ratio(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        let with_conversation = self.conversation_distribution.values().sum::<usize>();
        with_conversation as f64 / self.total_requests as f64
    }
}

/// Load trace records from JSONL file.
pub fn load_trace_from_file(path: &str) -> anyhow::Result<Vec<TraceRecord>> {
    let content = std::fs::read_to_string(path)?;
    let records: Vec<TraceRecord> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

/// Save trace records to JSONL file.
pub fn save_trace_to_file(records: &[TraceRecord], path: &str) -> anyhow::Result<()> {
    let mut content = String::new();
    for record in records {
        content.push_str(&serde_json::to_string(record)?);
        content.push('\n');
    }
    std::fs::write(path, content)?;
    Ok(())
}

/// Compare real trace stats with simulated pattern.
#[derive(Debug, Clone)]
pub struct ComparisonResult {
    pub real_repeat_ratio: f64,
    pub sim_repeat_ratio: f64,
    pub real_zipf_alpha: f64,
    pub sim_zipf_alpha: f64,
    pub real_conversation_ratio: f64,
    pub sim_conversation_ratio: f64,
    pub real_unique_prompts: usize,
    pub sim_unique_prompts: usize,
}

impl ComparisonResult {
    pub fn compare(real: &TraceStats, sim: &crate::LoadPattern) -> Self {
        Self {
            real_repeat_ratio: real.repeat_ratio(),
            sim_repeat_ratio: sim.repeat_ratio,
            real_zipf_alpha: real.estimate_zipf_alpha(),
            sim_zipf_alpha: sim.zipf_alpha,
            real_conversation_ratio: real.conversation_ratio(),
            sim_conversation_ratio: sim.conversation_ratio,
            real_unique_prompts: real.unique_prompts,
            sim_unique_prompts: sim.unique_queries,
        }
    }

    pub fn print_report(&self) {
        println!("\n=== Trace Comparison Report ===");
        println!("Parameter         | Real Trace | Simulation | Diff");
        println!("------------------|------------|------------|--------");
        println!(
            "repeat_ratio      | {:8.1}%  | {:8.1}%  | {:+.1}%",
            self.real_repeat_ratio * 100.0,
            self.sim_repeat_ratio * 100.0,
            (self.real_repeat_ratio - self.sim_repeat_ratio) * 100.0
        );
        println!(
            "zipf_alpha        | {:8.2}   | {:8.2}   | {:+.2}",
            self.real_zipf_alpha,
            self.sim_zipf_alpha,
            self.real_zipf_alpha - self.sim_zipf_alpha
        );
        println!(
            "conversation_ratio| {:8.1}%  | {:8.1}%  | {:+.1}%",
            self.real_conversation_ratio * 100.0,
            self.sim_conversation_ratio * 100.0,
            (self.real_conversation_ratio - self.sim_conversation_ratio) * 100.0
        );
        println!(
            "unique_prompts    | {:8}   | {:8}   | {:+}",
            self.real_unique_prompts,
            self.sim_unique_prompts,
            self.real_unique_prompts as i64 - self.sim_unique_prompts as i64
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_stats() {
        let records = vec![
            TraceRecord {
                timestamp_ms: 1000,
                request_id: "req-1".to_string(),
                model: "deepseek-v4".to_string(),
                prompt: "What is Rust?".to_string(),
                prompt_tokens: 10,
                completion_tokens: 100,
                total_tokens: 110,
                latency_ms: 150.0,
                cache_hit: false,
                conversation_id: Some("conv-1".to_string()),
            },
            TraceRecord {
                timestamp_ms: 2000,
                request_id: "req-2".to_string(),
                model: "deepseek-v4".to_string(),
                prompt: "What is Rust?".to_string(),
                prompt_tokens: 10,
                completion_tokens: 100,
                total_tokens: 110,
                latency_ms: 5.0,
                cache_hit: true,
                conversation_id: Some("conv-1".to_string()),
            },
            TraceRecord {
                timestamp_ms: 3000,
                request_id: "req-3".to_string(),
                model: "deepseek-v4".to_string(),
                prompt: "Explain Python".to_string(),
                prompt_tokens: 10,
                completion_tokens: 100,
                total_tokens: 110,
                latency_ms: 200.0,
                cache_hit: false,
                conversation_id: None,
            },
        ];

        let stats = TraceStats::from_records(&records);

        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.unique_prompts, 2);
        assert_eq!(stats.total_tokens, 330);
        assert!((stats.avg_latency_ms - 118.33).abs() < 1.0);
        assert!((stats.cache_hit_rate - 0.333).abs() < 0.01);
        assert!((stats.repeat_ratio() - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_trace_save_load() {
        let records = vec![TraceRecord {
            timestamp_ms: 1000,
            request_id: "req-1".to_string(),
            model: "deepseek-v4".to_string(),
            prompt: "Test".to_string(),
            prompt_tokens: 5,
            completion_tokens: 10,
            total_tokens: 15,
            latency_ms: 100.0,
            cache_hit: false,
            conversation_id: None,
        }];

        let temp_path = "/tmp/test_trace.jsonl";
        save_trace_to_file(&records, temp_path).unwrap();
        let loaded = load_trace_from_file(temp_path).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].prompt, "Test");

        std::fs::remove_file(temp_path).ok();
    }
}
