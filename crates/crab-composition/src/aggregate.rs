use crate::types::RequestComposition;
use std::collections::HashMap;

/// Summary of request composition across multiple trace entries.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CompositionSummary {
    pub total_entries: usize,
    /// Number of unique `project_id` values seen.
    pub tenant_count: usize,
    /// Number of unique `consumer` values seen.
    pub consumer_count: usize,
    /// Top-N model distribution.
    pub model_distribution: Vec<NamedCount>,
    /// Top-N projects by request volume.
    pub project_distribution: Vec<NamedCount>,
    /// Top-N consumers by request volume.
    pub consumer_distribution: Vec<NamedCount>,
    /// Tool count histogram: bucket -> count.
    pub tool_count_histogram: Vec<BucketCount>,
    /// Message count histogram: bucket -> count.
    pub message_count_histogram: Vec<BucketCount>,
    /// Cursor component detection rates.
    pub component_rates: Vec<ComponentRate>,
    /// Average latency and tokens per entry.
    pub avg_latency_ms: f64,
    pub avg_total_tokens: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NamedCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BucketCount {
    pub bucket_label: String,
    pub count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComponentRate {
    pub component: String,
    pub present_count: usize,
    pub rate: f64,
}

/// Aggregate `RequestComposition` entries into a summary for admin API display.
pub fn aggregate_composition(
    entries: &[(
        RequestComposition,
        /* latency_ms */ f64,
        /* total_tokens */ u64,
    )],
) -> CompositionSummary {
    if entries.is_empty() {
        return CompositionSummary::default();
    }

    let total = entries.len();
    let mut model_counts: HashMap<String, usize> = HashMap::new();
    let mut project_counts: HashMap<String, usize> = HashMap::new();
    let mut consumer_counts: HashMap<String, usize> = HashMap::new();
    let mut tool_count_hist: BucketCounter =
        BucketCounter::new(&["0", "1", "2-5", "6-10", "11-20", "20+"]);
    let mut msg_count_hist: BucketCounter =
        BucketCounter::new(&["0-10", "11-50", "51-100", "100+"]);
    let mut total_latency_ms = 0.0f64;
    let mut total_tokens: u64 = 0;

    let mut rules_count = 0usize;
    let mut skills_count = 0usize;
    let mut mcp_count = 0usize;
    let mut subagent_count = 0usize;

    for (comp, lat, tokens) in entries {
        *model_counts.entry(comp.client_model.clone()).or_insert(0) += 1;
        if let Some(ref pid) = comp.project_id {
            *project_counts.entry(pid.clone()).or_insert(0) += 1;
        }
        *consumer_counts.entry(comp.consumer.clone()).or_insert(0) += 1;

        // Tool count histogram
        let tc = comp.tool_count;
        let tool_bucket = if tc == 0 {
            "0"
        } else if tc == 1 {
            "1"
        } else if tc <= 5 {
            "2-5"
        } else if tc <= 10 {
            "6-10"
        } else if tc <= 20 {
            "11-20"
        } else {
            "20+"
        };
        tool_count_hist.add(tool_bucket);

        // Message count histogram
        let mc = comp.message_count;
        let msg_bucket = if mc <= 10 {
            "0-10"
        } else if mc <= 50 {
            "11-50"
        } else if mc <= 100 {
            "51-100"
        } else {
            "100+"
        };
        msg_count_hist.add(msg_bucket);

        total_latency_ms += lat;
        total_tokens += tokens;

        if comp.components.rules.present {
            rules_count += 1;
        }
        if comp.components.skills.present {
            skills_count += 1;
        }
        if comp.components.mcp.present {
            mcp_count += 1;
        }
        if comp.components.subagent.present {
            subagent_count += 1;
        }
    }

    fn top_n<K: ToString>(map: &HashMap<K, usize>, n: usize) -> Vec<NamedCount> {
        let mut v: Vec<NamedCount> = map
            .iter()
            .map(|(k, c)| NamedCount {
                name: k.to_string(),
                count: *c,
            })
            .collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.count));
        v.truncate(n);
        v
    }

    fn rate(count: usize, total: usize) -> f64 {
        if total == 0 {
            0.0
        } else {
            count as f64 / total as f64
        }
    }

    CompositionSummary {
        total_entries: total,
        tenant_count: if project_counts.is_empty() {
            0
        } else {
            project_counts.len()
        },
        consumer_count: consumer_counts.len(),
        model_distribution: top_n(&model_counts, 10),
        project_distribution: top_n(&project_counts, 10),
        consumer_distribution: top_n(&consumer_counts, 10),
        tool_count_histogram: tool_count_hist.into_vec(),
        message_count_histogram: msg_count_hist.into_vec(),
        component_rates: vec![
            ComponentRate {
                component: "rules".into(),
                present_count: rules_count,
                rate: rate(rules_count, total),
            },
            ComponentRate {
                component: "skills".into(),
                present_count: skills_count,
                rate: rate(skills_count, total),
            },
            ComponentRate {
                component: "mcp".into(),
                present_count: mcp_count,
                rate: rate(mcp_count, total),
            },
            ComponentRate {
                component: "subagent".into(),
                present_count: subagent_count,
                rate: rate(subagent_count, total),
            },
        ],
        avg_latency_ms: total_latency_ms / total as f64,
        avg_total_tokens: total_tokens / total as u64,
    }
}

struct BucketCounter {
    buckets: Vec<(String, usize)>,
}

impl BucketCounter {
    fn new(labels: &[&str]) -> Self {
        Self {
            buckets: labels.iter().map(|l| (l.to_string(), 0)).collect(),
        }
    }

    fn add(&mut self, label: &str) {
        if let Some((_, count)) = self.buckets.iter_mut().find(|(l, _)| l == label) {
            *count += 1;
        }
    }

    fn into_vec(self) -> Vec<BucketCount> {
        self.buckets
            .into_iter()
            .map(|(l, c)| BucketCount {
                bucket_label: l,
                count: c,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{CompositionHints, extract_composition};
    use serde_json::json;

    #[test]
    fn test_aggregate_composition_empty() {
        let summary = aggregate_composition(&[]);
        assert_eq!(summary.total_entries, 0);
    }

    #[test]
    fn test_aggregate_composition_single_entry() {
        let payload = json!({
            "model": "deepseek-v4-pro",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let comp = extract_composition(
            &payload,
            &CompositionHints {
                consumer: "test".into(),
                domain: "default".into(),
                ..Default::default()
            },
        );
        let summary = aggregate_composition(&[(comp, 100.0, 50)]);
        assert_eq!(summary.total_entries, 1);
        assert_eq!(summary.model_distribution.len(), 1);
        assert_eq!(summary.model_distribution[0].name, "deepseek-v4-pro");
        assert_eq!(summary.avg_latency_ms, 100.0);
        assert_eq!(summary.avg_total_tokens, 50);
    }

    #[test]
    fn test_tool_count_histogram() {
        let mut entries = Vec::new();
        let base_payload = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        for tc in &[0u32, 0, 1, 5, 12] {
            let mut payload = base_payload.clone();
            if *tc > 0 {
                let tools: Vec<serde_json::Value> = (0..*tc)
                    .map(|i| json!({"function": {"name": format!("fn_{}", i)}}))
                    .collect();
                payload["tools"] = json!(tools);
            }
            let comp = extract_composition(
                &payload,
                &CompositionHints {
                    consumer: "t".into(),
                    domain: "d".into(),
                    ..Default::default()
                },
            );
            entries.push((comp, 10.0, 5));
        }
        let summary = aggregate_composition(&entries);
        let hist = &summary.tool_count_histogram;
        assert!(hist.iter().any(|b| b.bucket_label == "0" && b.count == 2));
        assert!(hist.iter().any(|b| b.bucket_label == "1" && b.count == 1));
        assert!(hist.iter().any(|b| b.bucket_label == "2-5" && b.count == 1));
        assert!(
            hist.iter()
                .any(|b| b.bucket_label == "11-20" && b.count == 1)
        );
    }
}
