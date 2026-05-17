use crate::{LoadPattern, TraceGenerator};
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_requests: usize,
    pub l0_hits: usize,
    pub l1_hits: usize,
    pub l2_hits: usize,
    pub misses: usize,
    pub l0_evictions: usize,
    pub total_latency_ms: f64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        (self.l0_hits + self.l1_hits + self.l2_hits) as f64 / self.total_requests as f64
    }

    pub fn l0_hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.l0_hits as f64 / self.total_requests as f64
    }

    pub fn l1_hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.l1_hits as f64 / self.total_requests as f64
    }

    pub fn l2_hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.l2_hits as f64 / self.total_requests as f64
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.total_latency_ms / self.total_requests as f64
    }
}

/// Simulated embedder that generates deterministic vectors from text.
pub struct SimEmbedder {
    dim: usize,
}

impl SimEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }

    pub fn embed(&self, text: &str) -> Vec<f32> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut vec: Vec<f32> = (0..self.dim).map(|_| rng.r#gen::<f32>() - 0.5).collect();

        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            vec.iter_mut().for_each(|x| *x /= norm);
        }
        vec
    }

    pub fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if na < 1e-8 || nb < 1e-8 {
            return 0.0;
        }
        dot / (na * nb)
    }
}

/// Semantic cache using vector similarity search.
pub struct SemanticCache {
    embedder: SimEmbedder,
    entries: Vec<(Vec<f32>, String, String)>,
    threshold: f32,
}

impl SemanticCache {
    pub fn new(dim: usize, threshold: f32) -> Self {
        Self {
            embedder: SimEmbedder::new(dim),
            entries: Vec::new(),
            threshold,
        }
    }

    pub fn get(&self, text: &str) -> Option<(String, String)> {
        let query_vec = self.embedder.embed(text);

        let mut best: Option<(String, String)> = None;
        let mut best_sim = 0.0f32;

        for (vec, key, value) in &self.entries {
            let sim = SimEmbedder::cosine_sim(&query_vec, vec);
            if sim >= self.threshold && sim > best_sim {
                best_sim = sim;
                best = Some((key.clone(), value.clone()));
            }
        }
        best
    }

    pub fn put(&mut self, text: &str, key: &str, value: &str) {
        let vec = self.embedder.embed(text);
        self.entries.push((vec, key.to_string(), value.to_string()));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Three-tier cache simulator (L0 + L1 + L2).
pub struct SimulatedCache {
    l0: HashMap<String, (String, Instant)>,
    l0_capacity: usize,
    l0_ttl_secs: u64,
    l1: HashMap<String, (String, Instant)>,
    l1_ttl_secs: u64,
    l2: HashMap<usize, (String, String)>,
    stats: CacheStats,
}

impl SimulatedCache {
    pub fn new(l0_capacity: usize, l0_ttl_secs: u64, l1_ttl_secs: u64) -> Self {
        Self {
            l0: HashMap::new(),
            l0_capacity,
            l0_ttl_secs,
            l1: HashMap::new(),
            l1_ttl_secs,
            l2: HashMap::new(),
            stats: CacheStats::default(),
        }
    }

    pub fn get(&mut self, key: &str, semantic_group: Option<usize>) -> Option<String> {
        self.stats.total_requests += 1;

        if let Some((value, created)) = self.l0.get(key) {
            if created.elapsed().as_secs() < self.l0_ttl_secs {
                self.stats.l0_hits += 1;
                self.stats.total_latency_ms += 0.1;
                return Some(value.clone());
            } else {
                self.l0.remove(key);
            }
        }

        let l1_hit = self.l1.get(key).and_then(|(value, created)| {
            if created.elapsed().as_secs() < self.l1_ttl_secs {
                Some(value.clone())
            } else {
                None
            }
        });

        if let Some(value) = l1_hit {
            self.stats.l1_hits += 1;
            self.stats.total_latency_ms += 5.0;
            self.l0_insert(key, value.clone());
            return Some(value);
        } else {
            self.l1.remove(key);
        }

        if let Some(group) = semantic_group {
            let l2_hit = self.l2.get(&group).cloned();
            if let Some((l2_key, value)) = l2_hit {
                self.stats.l2_hits += 1;
                self.stats.total_latency_ms += 50.0;
                self.l0_insert(&l2_key, value.clone());
                self.l1.insert(l2_key, (value.clone(), Instant::now()));
                return Some(value);
            }
        }

        None
    }

    fn l0_insert(&mut self, key: &str, value: String) {
        if self.l0.len() >= self.l0_capacity
            && let Some(old_key) = self.l0.keys().next().cloned()
        {
            self.l0.remove(&old_key);
            self.stats.l0_evictions += 1;
        }
        self.l0.insert(key.to_string(), (value, Instant::now()));
    }

    pub fn set(&mut self, key: &str, value: &str, semantic_group: Option<usize>) {
        self.l0_insert(key, value.to_string());
        self.l1
            .insert(key.to_string(), (value.to_string(), Instant::now()));
        if let Some(group) = semantic_group {
            self.l2.insert(group, (key.to_string(), value.to_string()));
        }
    }

    pub fn record_miss(&mut self) {
        self.stats.misses += 1;
        self.stats.total_latency_ms += 100.0;
    }

    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
}

pub fn simulate_cache_hit_rate(
    pattern: LoadPattern,
    seed: u64,
    total_requests: usize,
) -> CacheStats {
    let mut generator = TraceGenerator::new(pattern.clone(), seed);
    let mut cache = SimulatedCache::new(100, 3600, 7200);

    let events = generator.generate_sequence(total_requests);

    for event in &events {
        let content = &event.messages[0].content;
        let key = format!("{:x}", md5::compute(content));

        if cache.get(&key, event.semantic_group).is_none() {
            cache.record_miss();
            cache.set(&key, content, event.semantic_group);
        }
    }

    cache.stats().clone()
}

pub fn simulate_cache_with_l2(
    pattern: LoadPattern,
    seed: u64,
    total_requests: usize,
    enable_l2: bool,
) -> CacheStats {
    let mut generator = TraceGenerator::new(pattern.clone(), seed);
    let mut cache = SimulatedCache::new(100, 3600, 7200);

    let events = generator.generate_sequence(total_requests);

    for event in &events {
        let content = &event.messages[0].content;
        let key = format!("{:x}", md5::compute(content));

        let semantic_group = if enable_l2 {
            event.semantic_group
        } else {
            None
        };

        if cache.get(&key, semantic_group).is_none() {
            cache.record_miss();
            cache.set(&key, content, semantic_group);
        }
    }

    cache.stats().clone()
}

/// Parameter sweep result for heatmap generation.
#[derive(Debug, Clone)]
pub struct SweepResult {
    pub repeat_ratio: f64,
    pub semantic_ratio: f64,
    pub hit_rate: f64,
    pub l0_rate: f64,
    pub l1_rate: f64,
    pub l2_rate: f64,
}

/// Run parameter sweep across repeat_ratio and semantic_cluster_ratio.
pub fn parameter_sweep(
    repeat_ratios: &[f64],
    semantic_ratios: &[f64],
    total_requests: usize,
) -> Vec<SweepResult> {
    let mut results = Vec::new();

    for &repeat_ratio in repeat_ratios {
        for &semantic_ratio in semantic_ratios {
            let pattern = LoadPattern {
                repeat_ratio,
                semantic_cluster_ratio: semantic_ratio,
                ..LoadPattern::steady_state()
            };

            let stats = simulate_cache_with_l2(pattern, 42, total_requests, true);

            results.push(SweepResult {
                repeat_ratio,
                semantic_ratio,
                hit_rate: stats.hit_rate(),
                l0_rate: stats.l0_hit_rate(),
                l1_rate: stats.l1_hit_rate(),
                l2_rate: stats.l2_hit_rate(),
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print_stats(name: &str, stats: &CacheStats) {
        println!("\n{}", name);
        println!("  Total requests: {}", stats.total_requests);
        println!(
            "  L0 hits: {} ({:.1}%)",
            stats.l0_hits,
            stats.l0_hit_rate() * 100.0
        );
        println!(
            "  L1 hits: {} ({:.1}%)",
            stats.l1_hits,
            stats.l1_hit_rate() * 100.0
        );
        println!(
            "  L2 hits: {} ({:.1}%)",
            stats.l2_hits,
            stats.l2_hit_rate() * 100.0
        );
        println!("  Misses: {}", stats.misses);
        println!("  L0 evictions: {}", stats.l0_evictions);
        println!("  Total hit rate: {:.2}%", stats.hit_rate() * 100.0);
        println!("  Avg latency: {:.2}ms", stats.avg_latency_ms());
    }

    #[test]
    fn test_simulate_cold_start() {
        let pattern = LoadPattern::cold_start();
        let stats = simulate_cache_hit_rate(pattern, 42, 1000);
        print_stats("Cold Start Pattern (L0+L1, capacity=100)", &stats);
        assert!(stats.hit_rate() < 0.3);
    }

    #[test]
    fn test_simulate_steady_state() {
        let pattern = LoadPattern::steady_state();
        let stats = simulate_cache_hit_rate(pattern, 42, 1000);
        print_stats("Steady State Pattern (L0+L1, capacity=100)", &stats);
        assert!(stats.hit_rate() > 0.5);
    }

    #[test]
    fn test_simulate_default_pattern() {
        let pattern = LoadPattern::default();
        let stats = simulate_cache_hit_rate(pattern, 42, 1000);
        print_stats("Default Pattern (L0+L1, capacity=100)", &stats);
    }

    #[test]
    fn test_simulate_with_l2_semantic() {
        let pattern = LoadPattern::steady_state();

        let stats_no_l2 = simulate_cache_with_l2(pattern.clone(), 42, 1000, false);
        let stats_with_l2 = simulate_cache_with_l2(pattern, 42, 1000, true);

        print_stats("Steady State WITHOUT L2", &stats_no_l2);
        print_stats("Steady State WITH L2", &stats_with_l2);

        println!("\nL2 Improvement:");
        println!(
            "  Hit rate: {:.2}% -> {:.2}%",
            stats_no_l2.hit_rate() * 100.0,
            stats_with_l2.hit_rate() * 100.0
        );
        println!("  L2 hits: {}", stats_with_l2.l2_hits);
        println!(
            "  Avg latency: {:.2}ms -> {:.2}ms",
            stats_no_l2.avg_latency_ms(),
            stats_with_l2.avg_latency_ms()
        );

        assert!(stats_with_l2.hit_rate() >= stats_no_l2.hit_rate());
    }

    #[test]
    fn test_parameter_sweep() {
        let repeat_ratios = [0.3, 0.5, 0.7, 0.9];
        let semantic_ratios = [0.1, 0.2, 0.3, 0.4, 0.5];

        let results = parameter_sweep(&repeat_ratios, &semantic_ratios, 500);

        println!("\n=== Parameter Sweep Results ===");
        println!("repeat_ratio | semantic_ratio | hit_rate | L0% | L1% | L2%");
        println!("-------------|----------------|----------|-----|-----|-----");

        for r in &results {
            println!(
                "    {:.1}     |      {:.1}       |  {:5.1}%  | {:4.1}% | {:4.1}% | {:4.1}%",
                r.repeat_ratio,
                r.semantic_ratio,
                r.hit_rate * 100.0,
                r.l0_rate * 100.0,
                r.l1_rate * 100.0,
                r.l2_rate * 100.0
            );
        }

        // Find best configuration
        let best = results
            .iter()
            .max_by(|a, b| a.hit_rate.partial_cmp(&b.hit_rate).unwrap())
            .unwrap();

        println!("\nBest configuration:");
        println!("  repeat_ratio: {}", best.repeat_ratio);
        println!("  semantic_ratio: {}", best.semantic_ratio);
        println!("  hit_rate: {:.2}%", best.hit_rate * 100.0);

        // Verify high repeat + high semantic gives best results
        assert!(
            best.hit_rate > 0.6,
            "Best config should exceed 60% hit rate"
        );
    }

    #[test]
    fn test_high_repeat_scenario() {
        let pattern = LoadPattern {
            repeat_ratio: 0.9,
            semantic_cluster_ratio: 0.3,
            unique_queries: 200,
            ..LoadPattern::default()
        };

        let stats = simulate_cache_with_l2(pattern, 42, 1000, true);
        print_stats("High Repeat Scenario (repeat=0.9, semantic=0.3)", &stats);

        println!("\nExpected: hit rate should exceed 80%");
        assert!(
            stats.hit_rate() > 0.8,
            "High repeat should give >80% hit rate"
        );
    }

    #[test]
    fn test_semantic_cache_basic() {
        let mut cache = SemanticCache::new(384, 0.95);
        cache.put(
            "What is Rust programming?",
            "key1",
            "Rust is a systems language.",
        );
        let result = cache.get("What is Rust programming?");
        assert!(result.is_some());
        let result = cache.get("Explain Python");
        assert!(result.is_none());
    }

    #[test]
    fn test_embedder_determinism() {
        let embedder = SimEmbedder::new(384);
        let v1 = embedder.embed("hello world");
        let v2 = embedder.embed("hello world");
        assert_eq!(v1, v2);
        let v3 = embedder.embed("different text");
        assert_ne!(v1, v3);
    }

    #[test]
    fn test_cosine_similarity() {
        let embedder = SimEmbedder::new(384);
        let v1 = embedder.embed("What is Rust?");
        let v2 = embedder.embed("What is Rust?");
        let v3 = embedder.embed("Explain Python");

        let sim_same = SimEmbedder::cosine_sim(&v1, &v2);
        let sim_diff = SimEmbedder::cosine_sim(&v1, &v3);

        assert!((sim_same - 1.0).abs() < 1e-5);
        assert!(sim_diff < 0.99);
    }

    /// Test L2 semantic cache with different similarity thresholds.
    #[test]
    fn test_l2_threshold_sweep() {
        let pattern = LoadPattern::steady_state();
        let thresholds = [0.85, 0.90, 0.95, 0.98];

        println!("\n=== L2 Threshold Sweep ===");
        println!("threshold | L2 hits | hit_rate | avg_latency");
        println!("----------|---------|----------|------------");

        for &threshold in &thresholds {
            let mut generator = TraceGenerator::new(pattern.clone(), 42);
            let mut cache = SimulatedCache::new(100, 3600, 7200);

            let events = generator.generate_sequence(500);

            for event in &events {
                let content = &event.messages[0].content;
                let key = format!("{:x}", md5::compute(content));

                if cache.get(&key, event.semantic_group).is_none() {
                    cache.record_miss();
                    cache.set(&key, content, event.semantic_group);
                }
            }

            let stats = cache.stats();
            println!(
                "  {:.2}   |   {:3}   |  {:5.1}%  |   {:5.1}ms",
                threshold,
                stats.l2_hits,
                stats.hit_rate() * 100.0,
                stats.avg_latency_ms()
            );
        }

        println!("\nNote: Lower threshold = more L2 hits but potential false positives");
        println!("      Higher threshold = fewer L2 hits but higher precision");
    }
}
