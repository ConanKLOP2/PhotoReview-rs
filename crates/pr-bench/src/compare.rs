//! Contract stub for comparing C# and Rust benchmark batches.
//!
//! Full summary parsing and comparison are specified in `contracts/W10-B1.md`.

use std::fmt;

/// A phase row parsed from one `BenchmarkBatchSummary`.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Rows in the summary's `ProfileOutcomes` array.
    pub rows: Vec<PhaseRow>,
    /// Exact dataset manifest identity used for comparability.
    pub dataset: DatasetIdentity,
}

/// Exact dataset identity fields used before timing comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetIdentity {
    /// Fully qualified input folder.
    pub folder: String,
    /// Ordered supported file paths.
    pub files: Vec<String>,
    /// Unsupported extension counts in ordinal key order.
    pub unsupported_extensions: Vec<(String, i64)>,
    /// Supported file limit.
    pub file_limit: i64,
}

/// One benchmark outcome row.
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseRow {
    /// Stable profile identifier.
    pub profile_id: String,
    /// Workload enum value from the summary.
    pub workload: i64,
    /// Result status enum value from the summary.
    pub status: i64,
    /// Raw benchmark samples in milliseconds.
    pub samples_ms: Vec<f64>,
    /// p50 milliseconds.
    pub p50_ms: f64,
    /// p95 milliseconds.
    pub p95_ms: f64,
    /// Maximum milliseconds.
    pub max_ms: f64,
    /// Correctness-only profile flag.
    pub correctness_only: bool,
    /// Diagnostic metrics retained in source order as name/value pairs; never used for the timing gate.
    pub metrics: Vec<DiagnosticMetric>,
}

/// One diagnostic snapshot value; counters and arrays are rendered as normalized scalar/JSON text.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticMetric {
    /// Snapshot field name.
    pub name: String,
    /// Stable normalized value for reporting.
    pub value: String,
}

/// Comparison policy values. Defaults are the W10-B1 proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct Thresholds {
    /// Relative tolerance, default 5 percent.
    pub ratio_tolerance: f64,
    /// Absolute timing noise floor in milliseconds, default 1 ms.
    pub absolute_floor_ms: f64,
    /// Required independent sequential runs, default 3.
    pub minimum_repeats: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            ratio_tolerance: 0.05,
            absolute_floor_ms: 1.0,
            minimum_repeats: 3,
        }
    }
}

/// Timing or exactness outcome for one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Meets the numeric equal-or-better gate.
    Pass,
    /// Difference is below the absolute or relative noise boundary.
    WithinNoise,
    /// Rust exceeds both allowed boundaries.
    Regression,
    /// Rust is faster by more than both boundaries.
    Improvement,
    /// Inputs cannot be compared.
    Incomparable,
    /// Symmetrically unavailable/not implemented profile.
    Skipped,
}

/// One profile's comparison result.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileComparison {
    /// Profile identifier.
    pub profile_id: String,
    /// Final row verdict.
    pub verdict: Verdict,
    /// Human-readable explanation.
    pub reason: String,
    /// C# p50/p95/max in milliseconds.
    pub csharp_ms: [f64; 3],
    /// Rust p50/p95/max in milliseconds.
    pub rust_ms: [f64; 3],
    /// Exact sample count, when available.
    pub sample_count: usize,
}

/// Aggregate report returned by the pure comparison function.
#[derive(Debug, Clone, PartialEq)]
pub struct CompareReport {
    /// Overall verdict.
    pub verdict: Verdict,
    /// Per-profile comparisons in ordinal profile-id order.
    pub profiles: Vec<ProfileComparison>,
    /// Non-fatal comparability notes.
    pub notes: Vec<String>,
}

/// Invalid summary shape or an unavailable comparison operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareError {
    /// Explanation suitable for a command-line error.
    pub message: String,
}

impl fmt::Display for CompareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompareError {}

/// Compare one pair of already parsed summaries.
///
/// This single-pair operation is valid only when `minimum_repeats == 1`; otherwise use
/// [`compare_repeats`]. This compiling contract stub returns a typed error until implementation.
pub fn compare(_csharp: &Summary, _rust: &Summary, _thresholds: &Thresholds) -> Result<CompareReport, CompareError> {
    Err(CompareError {
        message: "summary comparison is not implemented yet".to_owned(),
    })
}

/// Compare paired repeat summaries by index and use the median of each repeat-level statistic.
///
/// Requires equal slice lengths and at least `thresholds.minimum_repeats` pairs. This compiling
/// contract stub returns a typed error until implementation.
pub fn compare_repeats(
    _csharp: &[Summary],
    _rust: &[Summary],
    _thresholds: &Thresholds,
) -> Result<CompareReport, CompareError> {
    Err(CompareError {
        message: "repeat comparison is not implemented yet".to_owned(),
    })
}
