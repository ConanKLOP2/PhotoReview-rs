//! Port of `PhotoReview.Core/Diagnostics/BenchmarkStatistics.cs` (C# baseline e441d67a).
//!
//! The percentile used by the benchmark reports: sort ascending, then LINEAR interpolation between the two nearest
//! ranks (the "inclusive" definition: `position = (n - 1) * p`). It is not nearest-rank and it does no rounding at
//! all, so there is no banker's-rounding question here; the only exactness issues are IEEE `f64` arithmetic order,
//! NaN ordering and the order of the argument checks, all spelled out in contract.md and pinned by the
//! `percentile` golden.

use std::fmt;

/// C# `ArgumentOutOfRangeException(nameof(percentile))`: the percentile is not within `0.0..=1.0` (NaN included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PercentileOutOfRange;

impl fmt::Display for PercentileOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("percentile must be between 0 and 1")
    }
}

impl std::error::Error for PercentileOutOfRange {}

/// C# `BenchmarkStatistics.Percentile(IReadOnlyList<double> values, double percentile)`.
///
/// * `values` empty -> `Ok(0.0)` WITHOUT looking at `percentile` (even NaN or 7.0).
/// * otherwise `percentile` must satisfy `percentile >= 0.0 && percentile <= 1.0`, else `Err(PercentileOutOfRange)`
///   (NaN and the infinities fail the check).
/// * `values` is not modified; the result comes from an ascending, STABLE sort that orders like .NET
///   `double.CompareTo` (NaN before every number, NaN equal to NaN, `-0.0` equal to `0.0`).
pub fn percentile(values: &[f64], percentile: f64) -> Result<f64, PercentileOutOfRange> {
    let _ = (values, percentile);
    todo!("port BenchmarkStatistics.Percentile")
}
