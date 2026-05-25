use crate::overview::{ConsumerMetricsBucket, DomainMetricsBucket, TierDeltas5m, TimeSeriesPoint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainPolicy {
    pub domain: String,
    pub monthly_token_budget: u64,
    pub monthly_cost_budget_usd: f64,
    pub min_hit_rate: f64,
    pub enabled: bool,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainDetailBundle {
    pub domain: String,
    pub bucket: DomainMetricsBucket,
    pub consumer_buckets: Vec<ConsumerMetricsBucket>,
    pub history_7d: Vec<TimeSeriesPoint>,
    pub history_30d: Vec<TimeSeriesPoint>,
    pub tier_deltas_5m: TierDeltas5m,
    pub policy: Option<DomainPolicy>,
}
