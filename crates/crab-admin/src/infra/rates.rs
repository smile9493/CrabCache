use crate::infra::types::*;

/// Compute CPU percent and network rates from a raw sample vs previous sample.
pub fn compute_cpu_percent(current: &ContainerRawSample, prev: &ContainerRawSample) -> f64 {
    let delta = current
        .cpu_total_usage
        .saturating_sub(prev.cpu_total_usage);
    let delta_system = current
        .system_cpu_usage
        .saturating_sub(prev.system_cpu_usage);
    let online = current.online_cpus.max(1);

    if delta_system == 0 {
        return 0.0;
    }

    (delta as f64 / delta_system as f64) * online as f64 * 100.0
}

/// Compute bytes-per-second for a cumulative counter.
pub fn compute_bps(current: u64, prev: u64, elapsed_secs: f64) -> Option<f64> {
    if elapsed_secs <= 0.0 {
        return None;
    }
    let delta = current.saturating_sub(prev);
    Some(delta as f64 / elapsed_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_sample(
        cpu: u64,
        sys: u64,
        rx: u64,
        tx: u64,
        online: u32,
    ) -> ContainerRawSample {
        ContainerRawSample {
            cpu_total_usage: cpu,
            system_cpu_usage: sys,
            net_rx_bytes: rx,
            net_tx_bytes: tx,
            online_cpus: online,
            sampled_at: Instant::now(),
        }
    }

    #[test]
    fn test_cpu_percent_normal() {
        let prev = make_sample(100_000_000, 200_000_000, 0, 0, 4);
        let cur = make_sample(110_000_000, 220_000_000, 0, 0, 4);
        // delta=10M, delta_sys=20M => 0.5 * 4 = 2.0, *100 => 200%
        let pct = compute_cpu_percent(&cur, &prev);
        assert!((pct - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_cpu_percent_zero_delta() {
        let prev = make_sample(100, 200, 0, 0, 2);
        let cur = make_sample(100, 200, 0, 0, 2);
        assert_eq!(compute_cpu_percent(&cur, &prev), 0.0);
    }

    #[test]
    fn test_bps_normal() {
        let bps = compute_bps(2000, 1000, 10.0).unwrap();
        assert!((bps - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_bps_zero_elapsed() {
        assert!(compute_bps(2000, 1000, 0.0).is_none());
    }

    #[test]
    fn test_bps_wrap() {
        let bps = compute_bps(1000, 2000, 10.0).unwrap();
        assert!((bps - 0.0).abs() < 0.01);
    }
}
