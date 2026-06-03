//! Shared percentile / summary helpers for trace analysis.

/// Latency sample set with summary printing.
#[derive(Debug, Default)]
pub struct LatencyStats {
    pub name: String,
    values: Vec<f64>,
}

impl LatencyStats {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: Vec::new(),
        }
    }

    pub fn add(&mut self, v: Option<f64>) {
        if let Some(v) = v.filter(|x| x.is_finite()) {
            self.values.push(v);
        }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn median(&self) -> Option<f64> {
        percentile(&self.values, 0.5)
    }

    pub fn summary_line(&self) -> String {
        if self.values.is_empty() {
            return format!("{}: (no samples)", self.name);
        }
        let mut s = self.values.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = s.len();
        let p50 = s[(n * 50 / 100).min(n.saturating_sub(1))];
        let p90 = s[(n * 90 / 100).min(n.saturating_sub(1))];
        let mean: f64 = s.iter().sum::<f64>() / n as f64;
        format!(
            "{}: n={n} min={:.0} p50={p50:.0} p90={p90:.0} max={:.0} mean={mean:.0}",
            self.name,
            s[0],
            s[n - 1]
        )
    }
}

/// Linear-interpolation percentile on sorted values.
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut s = values.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((s.len() - 1) as f64 * p).round() as usize;
    Some(s[idx.min(s.len() - 1)])
}

/// Median of optional field across trace rows.
pub fn median_field<T, F>(rows: &[T], f: F) -> Option<f64>
where
    F: Fn(&T) -> Option<f64>,
{
    let vals: Vec<f64> = rows.iter().filter_map(f).collect();
    percentile(&vals, 0.5)
}
