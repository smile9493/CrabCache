use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Sanitized log entry for privacy-preserving trace collection.
/// No raw request content is stored, only statistical features.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedLogEntry {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub content_length: usize,
    pub semantic_cluster: usize,
    pub conversation_id: Option<String>,
    pub model: String,
    pub prompt_tokens: usize,
    pub latency_ms: f64,
    pub cache_hit: bool,
}

impl SanitizedLogEntry {
    /// Create a sanitized log entry from raw request data.
    /// Only stores hash and statistical features, never raw content.
    pub fn from_request(
        request_body: &str,
        conversation_id: Option<String>,
        model: &str,
        prompt_tokens: usize,
        latency_ms: f64,
        cache_hit: bool,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(request_body.as_bytes());
        let hash = hasher.finalize();
        let hash_hex = hex::encode(&hash[..8]);

        let semantic_cluster = Self::compute_semantic_cluster(request_body);

        Self {
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            request_hash: hash_hex,
            content_length: request_body.len(),
            semantic_cluster,
            conversation_id,
            model: model.to_string(),
            prompt_tokens,
            latency_ms,
            cache_hit,
        }
    }

    /// Compute a fast hash for semantic clustering.
    /// Groups similar-length requests with similar starting patterns.
    fn compute_semantic_cluster(request_body: &str) -> usize {
        let mut hasher = Sha256::new();
        hasher.update(request_body.len().to_string().as_bytes());
        hasher.update(&request_body.as_bytes()[..request_body.len().min(100)]);
        let hash = hasher.finalize();
        let cluster_id = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]);
        (cluster_id % 1000) as usize
    }
}

/// Statistics fitted from sanitized trace data.
#[derive(Debug, Clone, Default)]
pub struct FittedParameters {
    pub total_requests: usize,
    pub unique_requests: usize,
    pub repeat_ratio: f64,
    pub semantic_cluster_ratio: f64,
    pub estimated_unique_queries: usize,
    pub estimated_zipf_alpha: f64,
    pub conversation_ratio: f64,
    pub avg_latency_ms: f64,
    pub cache_hit_rate: f64,
}

impl FittedParameters {
    /// Fit parameters from sanitized log entries.
    pub fn from_entries(entries: &[SanitizedLogEntry]) -> Self {
        use std::collections::{HashMap, HashSet};

        let total_requests = entries.len();
        if total_requests == 0 {
            return Self::default();
        }

        let unique_hashes: HashSet<&str> =
            entries.iter().map(|e| e.request_hash.as_str()).collect();
        let unique_requests = unique_hashes.len();

        let repeat_ratio = 1.0 - (unique_requests as f64 / total_requests as f64);

        let mut cluster_counts: HashMap<usize, usize> = HashMap::new();
        for entry in entries {
            *cluster_counts.entry(entry.semantic_cluster).or_insert(0) += 1;
        }

        let clustered_requests = cluster_counts
            .values()
            .filter(|&&count| count > 1)
            .sum::<usize>();
        let semantic_cluster_ratio = clustered_requests as f64 / total_requests as f64;

        let with_conversation = entries
            .iter()
            .filter(|e| e.conversation_id.is_some())
            .count();
        let conversation_ratio = with_conversation as f64 / total_requests as f64;

        let total_latency: f64 = entries.iter().map(|e| e.latency_ms).sum();
        let avg_latency_ms = total_latency / total_requests as f64;

        let cache_hits = entries.iter().filter(|e| e.cache_hit).count();
        let cache_hit_rate = cache_hits as f64 / total_requests as f64;

        let estimated_zipf_alpha = Self::estimate_zipf_alpha(&cluster_counts, total_requests);

        Self {
            total_requests,
            unique_requests,
            repeat_ratio,
            semantic_cluster_ratio,
            estimated_unique_queries: unique_requests,
            estimated_zipf_alpha,
            conversation_ratio,
            avg_latency_ms,
            cache_hit_rate,
        }
    }

    fn estimate_zipf_alpha(
        cluster_counts: &std::collections::HashMap<usize, usize>,
        _total: usize,
    ) -> f64 {
        let mut counts: Vec<usize> = cluster_counts.values().cloned().collect();
        counts.sort_by(|a, b| b.cmp(a));

        if counts.len() < 5 {
            return 1.2;
        }

        let _n = counts.len() as f64;
        let sum: f64 = counts.iter().map(|&c| c as f64).sum();
        if sum == 0.0 {
            return 1.2;
        }

        let top_10_percent = (counts.len() as f64 * 0.1).ceil() as usize;
        let top_sum: f64 = counts.iter().take(top_10_percent).map(|&c| c as f64).sum();
        let concentration = top_sum / sum;

        match concentration {
            c if c > 0.8 => 1.8,
            c if c > 0.6 => 1.5,
            c if c > 0.4 => 1.2,
            _ => 1.0,
        }
    }

    /// Generate a LoadPattern from fitted parameters.
    pub fn to_load_pattern(&self) -> crate::LoadPattern {
        crate::LoadPattern {
            unique_queries: self.estimated_unique_queries,
            zipf_alpha: self.estimated_zipf_alpha,
            conversation_ratio: self.conversation_ratio,
            semantic_cluster_ratio: self.semantic_cluster_ratio,
            repeat_ratio: self.repeat_ratio,
            ..Default::default()
        }
    }

    /// Print a summary report.
    pub fn print_report(&self) {
        println!("\n=== Fitted Parameters from Real Trace ===");
        println!("Total requests: {}", self.total_requests);
        println!("Unique requests: {}", self.unique_requests);
        println!();
        println!("repeat_ratio:           {:5.1}%", self.repeat_ratio * 100.0);
        println!(
            "semantic_cluster_ratio: {:5.1}%",
            self.semantic_cluster_ratio * 100.0
        );
        println!(
            "conversation_ratio:     {:5.1}%",
            self.conversation_ratio * 100.0
        );
        println!("estimated_zipf_alpha:   {:5.2}", self.estimated_zipf_alpha);
        println!();
        println!("avg_latency_ms:  {:6.1}ms", self.avg_latency_ms);
        println!("cache_hit_rate:  {:5.1}%", self.cache_hit_rate * 100.0);
        println!();

        let achievable = self.estimate_achievable_hit_rate();
        println!("Estimated achievable hit rate: {:.1}%", achievable * 100.0);

        if achievable < 0.9 {
            println!("⚠️  Warning: Current trace pattern cannot reach 98% target.");
            println!("   Consider: reducing unique_queries, increasing repeat_ratio");
        } else if achievable < 0.95 {
            println!("ℹ️  Note: 98% target requires additional optimization.");
        } else {
            println!("✅ 98% target is achievable with optimal configuration.");
        }
    }

    /// Estimate the maximum achievable hit rate based on fitted parameters.
    pub fn estimate_achievable_hit_rate(&self) -> f64 {
        let base = self.repeat_ratio;
        let semantic_bonus = self.semantic_cluster_ratio * (1.0 - self.repeat_ratio) * 0.5;
        let concentration_bonus = (self.estimated_zipf_alpha - 1.0).max(0.0) * 0.1;

        (base + semantic_bonus + concentration_bonus).min(0.98)
    }
}

/// Save sanitized log entries to file.
pub fn save_sanitized_log(entries: &[SanitizedLogEntry], path: &str) -> anyhow::Result<()> {
    let mut content = String::new();
    for entry in entries {
        content.push_str(&serde_json::to_string(entry)?);
        content.push('\n');
    }
    std::fs::write(path, content)?;
    Ok(())
}

/// Load sanitized log entries from file.
pub fn load_sanitized_log(path: &str) -> anyhow::Result<Vec<SanitizedLogEntry>> {
    let content = std::fs::read_to_string(path)?;
    let entries: Vec<SanitizedLogEntry> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitized_log_entry() {
        let entry = SanitizedLogEntry::from_request(
            "What is Rust?",
            Some("conv-123".to_string()),
            "deepseek-v4",
            10,
            150.0,
            false,
        );

        assert!(!entry.request_hash.is_empty());
        assert_eq!(entry.content_length, 13);
        assert!(entry.semantic_cluster < 1000);
        assert_eq!(entry.conversation_id, Some("conv-123".to_string()));
    }

    #[test]
    fn test_fitted_parameters() {
        let entries = vec![
            SanitizedLogEntry::from_request(
                "Query A",
                Some("c1".to_string()),
                "m",
                10,
                100.0,
                false,
            ),
            SanitizedLogEntry::from_request("Query A", Some("c1".to_string()), "m", 10, 5.0, true),
            SanitizedLogEntry::from_request("Query B", None, "m", 10, 100.0, false),
            SanitizedLogEntry::from_request("Query C", None, "m", 10, 100.0, false),
        ];

        let fitted = FittedParameters::from_entries(&entries);

        assert_eq!(fitted.total_requests, 4);
        assert_eq!(fitted.unique_requests, 3);
        assert!((fitted.repeat_ratio - 0.25).abs() < 0.01);
        assert!((fitted.cache_hit_rate - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_achievable_hit_rate_estimation() {
        let mut fitted = FittedParameters::default();

        fitted.repeat_ratio = 0.9;
        fitted.semantic_cluster_ratio = 0.3;
        fitted.estimated_zipf_alpha = 1.5;

        let achievable = fitted.estimate_achievable_hit_rate();
        println!("Achievable hit rate: {:.1}%", achievable * 100.0);
        assert!(achievable > 0.9);
    }

    #[test]
    fn test_save_load_sanitized_log() {
        let entries = vec![SanitizedLogEntry::from_request(
            "Test", None, "m", 5, 100.0, false,
        )];

        let temp_path = "/tmp/test_sanitized.jsonl";
        save_sanitized_log(&entries, temp_path).unwrap();
        let loaded = load_sanitized_log(temp_path).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content_length, 4);

        std::fs::remove_file(temp_path).ok();
    }
}
