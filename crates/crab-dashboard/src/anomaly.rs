use crate::components::toast::{ToastKind, show_toast, use_toast};
use crate::types::OverviewCore;

/// Lightweight snapshot for anomaly detection — only the fields we compare.
#[derive(Clone, Default)]
struct Snapshot {
    hit_rate_5m: f64,
    qps_5m: f64,
    latency_upstream_ms: f64,
    healthy: bool,
}

/// Anomaly detected by the frontend detector.
pub(crate) struct Anomaly {
    kind: ToastKind,
    message: String,
}

/// Tracks previous snapshot and cooldown timestamps.
pub struct AnomalyDetector {
    prev: Option<Snapshot>,
    last_hit_rate_alert: f64,   // js_sys::Date::now() timestamp
    last_qps_alert: f64,
    last_latency_alert: f64,
    last_health_alert: f64,
}

const COOLDOWN_MS: f64 = 60_000.0;

impl AnomalyDetector {
    pub fn new() -> Self {
        Self {
            prev: None,
            last_hit_rate_alert: 0.0,
            last_qps_alert: 0.0,
            last_latency_alert: 0.0,
            last_health_alert: 0.0,
        }
    }

    /// Check for anomalies against the previous snapshot. Returns anomaly messages.
    pub(crate) fn check(&mut self, core: &OverviewCore) -> Vec<Anomaly> {
        let curr = Snapshot {
            hit_rate_5m: core.metrics.hit_rate_5m,
            qps_5m: core.metrics.qps_5m,
            latency_upstream_ms: core.metrics.latency_upstream_ms,
            healthy: core.health.healthy,
        };

        let mut anomalies = Vec::new();
        let now = js_sys::Date::now();

        if let Some(ref prev) = self.prev {
            // Hit rate drop > 15 percentage points
            if prev.hit_rate_5m > 0.1
                && (prev.hit_rate_5m - curr.hit_rate_5m) > 0.15
                && now - self.last_hit_rate_alert > COOLDOWN_MS
            {
                self.last_hit_rate_alert = now;
                anomalies.push(Anomaly {
                    kind: ToastKind::Warning,
                    message: format!(
                        "Hit rate dropped: {:.1}% \u{2192} {:.1}%",
                        prev.hit_rate_5m * 100.0,
                        curr.hit_rate_5m * 100.0
                    ),
                });
            }

            // QPS spike > 3x
            if prev.qps_5m > 0.5
                && curr.qps_5m > prev.qps_5m * 3.0
                && now - self.last_qps_alert > COOLDOWN_MS
            {
                self.last_qps_alert = now;
                anomalies.push(Anomaly {
                    kind: ToastKind::Warning,
                    message: format!("QPS spiked: {:.1} \u{2192} {:.1}", prev.qps_5m, curr.qps_5m),
                });
            }

            // Health state change
            if prev.healthy && !curr.healthy && now - self.last_health_alert > COOLDOWN_MS {
                self.last_health_alert = now;
                anomalies.push(Anomaly {
                    kind: ToastKind::Error,
                    message: "Gateway health check failed".to_string(),
                });
            } else if !prev.healthy && curr.healthy && now - self.last_health_alert > COOLDOWN_MS {
                self.last_health_alert = now;
                anomalies.push(Anomaly {
                    kind: ToastKind::Info,
                    message: "Gateway recovered".to_string(),
                });
            }
        }

        // Latency spike (absolute threshold, no previous needed)
        if curr.latency_upstream_ms > 5000.0
            && now - self.last_latency_alert > COOLDOWN_MS
        {
            self.last_latency_alert = now;
            anomalies.push(Anomaly {
                kind: ToastKind::Error,
                message: format!("Upstream latency spike: {:.0}ms", curr.latency_upstream_ms),
            });
        }

        self.prev = Some(curr);
        anomalies
    }
}

/// Run anomaly detection on new data and fire toasts for any detected anomalies.
pub fn detect_and_toast(core: &OverviewCore) {
    // We use a thread-local static to persist the detector across calls.
    thread_local! {
        static DETECTOR: std::cell::RefCell<AnomalyDetector> = std::cell::RefCell::new(AnomalyDetector::new());
    }

    DETECTOR.with(|d| {
        let mut d = d.borrow_mut();
        let anomalies = d.check(core);
        if !anomalies.is_empty() {
            let toast = use_toast();
            for a in anomalies {
                show_toast(toast, a.kind, &a.message);
            }
        }
    });
}
