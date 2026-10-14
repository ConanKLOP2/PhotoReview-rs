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
    if values.is_empty() {
        return Ok(0.0);
    }
    // C# `!(percentile >= 0 && percentile <= 1)`: NaN and the infinities fail; -0.0 passes.
    if !(0.0..=1.0).contains(&percentile) {
        return Err(PercentileOutOfRange);
    }

    // Stable ascending sort (LINQ `OrderBy` is stable too), ordered like `double.CompareTo`.
    let mut ordered = values.to_vec();
    ordered.sort_by(|a, b| compare_like_dotnet(*a, *b));

    let position = (ordered.len() - 1) as f64 * percentile;
    // 0 <= position <= len - 1 because percentile is in 0..=1 and is not NaN, so both indices are in bounds.
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        Ok(ordered[lower])
    } else {
        Ok(ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower as f64))
    }
}

/// The ordering of .NET `double.CompareTo`: `-0.0` equals `0.0`, NaN equals NaN and sorts before every number.
fn compare_like_dotnet(a: f64, b: f64) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else if a == b {
        Ordering::Equal
    } else if a.is_nan() {
        if b.is_nan() { Ordering::Equal } else { Ordering::Less }
    } else {
        Ordering::Greater
    }
}
