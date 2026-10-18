//! Nearest-rank percentiles and a small sample summary for the spike measurement.

/// Nearest-rank percentile (`p` in 0..=100) of an unsorted sample; `0.0` for an empty sample.
#[must_use]
pub fn percentile(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// `count=.. p50_ms=.. p95_ms=.. max_ms=..`, invariant formatting (machine-read).
#[must_use]
pub fn summary(samples_ms: &[f64]) -> String {
    let max = samples_ms.iter().copied().fold(0.0_f64, f64::max);
    format!(
        "count={} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        samples_ms.len(),
        percentile(samples_ms, 50.0),
        percentile(samples_ms, 95.0),
        max
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank_of_ten_values() {
        let v: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), 5.0);
        assert_eq!(percentile(&v, 95.0), 10.0);
        assert_eq!(percentile(&v, 10.0), 1.0);
    }

    #[test]
    fn percentile_empty_sample_is_zero() {
        assert_eq!(percentile(&[], 95.0), 0.0);
    }

    #[test]
    fn percentile_ignores_input_order() {
        assert_eq!(percentile(&[9.0, 1.0, 5.0], 50.0), 5.0);
    }
}
