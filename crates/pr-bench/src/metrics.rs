//! Port of `PhotoReview.Benchmarking/BenchmarkMetrics.cs` (C# baseline d92d6aae, 54 lines): subtracting a baseline
//! `ReviewMetricsSnapshot` from a later one (the benchmark engine uses it to drop the warm-up iterations).
//!
//! Unit W7-01b. Contract: `contracts/W7-01b.md`. `BenchmarkStatistics.Percentile` is NOT here: it is already ported
//! (`pr_core::diagnostics::bench_stats::percentile`, W1-15).
#![allow(unused_variables, dead_code)] // D11: stub with todo!() bodies; the porter removes this line with the last todo!().

use std::fmt;

use pr_core::diagnostics::metrics::ReviewMetricsSnapshot;

/// C# `ToDictionary` throws `ArgumentException` when the BASELINE snapshot repeats a key. The text names the repeated
/// key as spelled in the baseline entry that was seen second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkMetricsError {
    /// Two `top_source_opens` baseline entries whose paths are equal under OrdinalIgnoreCase.
    DuplicateSourceOpenPath(String),
    /// Two `present_histogram` baseline buckets with the same label (ordinal, case-sensitive).
    DuplicatePresentBucketLabel(String),
}

impl fmt::Display for BenchmarkMetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSourceOpenPath(key) => write!(f, "duplicate path in baseline TopSourceOpens: {key}"),
            Self::DuplicatePresentBucketLabel(key) => write!(f, "duplicate label in baseline PresentHistogram: {key}"),
        }
    }
}

impl std::error::Error for BenchmarkMetricsError {}

/// C# `BenchmarkMetrics.Since(current, baseline)`: what was recorded after `baseline` was taken.
///
/// * Every scalar counter is `current - baseline` with wrapping `i64` arithmetic (no clamp: a negative delta stays negative).
/// * `decoder_fallbacks`: current order, one entry per current backend whose delta is strictly positive.
/// * `top_source_opens`: each current entry minus the baseline count of the same path (OrdinalIgnoreCase, 0 when the
///   baseline did not list it), entries left with `count <= 0` dropped, then count descending and path
///   OrdinalIgnoreCase ascending (stable).
/// * `present_histogram`: current order, each bucket minus the baseline bucket with the same label (ordinal, 0 when
///   missing); nothing is dropped and counts may be zero or negative.
///
/// Errors only when the baseline has duplicate keys (see [`BenchmarkMetricsError`]); `current` may repeat keys.
pub fn since(
    current: &ReviewMetricsSnapshot,
    baseline: &ReviewMetricsSnapshot,
) -> Result<ReviewMetricsSnapshot, BenchmarkMetricsError> {
    todo!("W7-01b")
}
