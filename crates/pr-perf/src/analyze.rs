//! Port of `PhotoReview.PerfAnalysis/PerfAnalyze.cs` (C# baseline e441d67a, 214 lines) and
//! `NaNAsNullDoubleConverter.cs` (18 lines): the `--perf-analyze <runDir> [--rules <json>]` pipeline. Finds every
//! `perf-*.csv` under a run directory, reads and groups them, evaluates the nine decision rules per group and writes
//! `summary.md` + `summary.json` into the run directory.
//!
//! R0003: C# `Task<AnalysisResult> RunAsync(...)` is the blocking [`run`]; the CLI (W7, pr-bench) calls it on its
//! own thread. `PerfAnalyze.IsBurstScenario` is [`is_burst_scenario`].
//!
//! Unit W1-21. Contract: contracts/W1-21/contract.md. Depends on every other `pr-perf` module.
#![allow(unused_variables, dead_code, reason = "stub: todo!() bodies, remove when filling")]

use std::path::{Path, PathBuf};

use crate::csv::PerfError;
use crate::rules::RuleResult;
use crate::stats::GroupSummary;

/// `PerfAnalyze.GroupResult`.
#[derive(Debug, Clone)]
pub struct GroupResult {
    pub summary: GroupSummary,
    /// The nine rules in the order R-IO, R-DEC, R-UI, R-PRE, R-CONT, R-THREAD, R-GC, R-DISK, R-FOLDER.
    pub rules: Vec<RuleResult>,
}

/// `PerfAnalyze.AnalysisResult`.
#[derive(Debug, Clone, Default)]
pub struct AnalysisResult {
    pub groups: Vec<GroupResult>,
    /// `Path.Combine(runDir, "summary.md")`.
    pub summary_md_path: PathBuf,
    /// `Path.Combine(runDir, "summary.json")`.
    pub summary_json_path: PathBuf,
    pub csv_file_count: i32,
}

/// `PerfAnalyze.IsBurstScenario(scenarioKey)`: S3 and S4 are the burst scenarios. The key may carry a folder alias
/// (`S3@F1`): only the part before the first `@` is looked at; it must start with `S3` or `S4` (ASCII case-insensitive)
/// and must not continue with an ASCII digit (`S30`, `S34` are not burst; `S3-burst`, `S4x`, `S3` are).
pub fn is_burst_scenario(scenario_key: &str) -> bool {
    todo!()
}

/// `PerfAnalyze.RunAsync(runDir, rulesPath)`.
///
/// 1. `run_dir` must be an existing directory, else `Err(PerfError::DirectoryNotFound(run_dir))`.
/// 2. Every `perf-*.csv` below it (recursive, name matched case-insensitively), except files under a directory named
///    `warmup` (case-insensitive), sorted by full path with ordinal UTF-16 comparison. None -> `Err(InvalidOperation)`.
/// 3. `rules_path` of `None` or empty -> default thresholds, else [`crate::rules::RulesConfig::load`].
/// 4. Per file: read, build navigations, load metadata, add to the group `(scenario, mode, cond, workers)`.
/// 5. Summaries, the nine rules per group, groups sorted by (scenario, mode, cond, workers ?? -1), then
///    `summary.md` and `summary.json` are written into `run_dir`.
pub fn run(run_dir: &Path, rules_path: Option<&Path>) -> Result<AnalysisResult, PerfError> {
    todo!()
}

/// `NaNAsNullDoubleConverter.Write`: the JSON text of a `double` in `summary.json`: the .NET number text
/// ([`crate::rules::fmt_double`]) when finite, else `null`.
pub fn nan_as_null_double_json(value: f64) -> String {
    todo!()
}

/// `NaNAsNullDoubleConverter.Read`: JSON `null` -> `Some(NaN)`, a number -> `Some(value)`, any other token ->
/// `None` (C# `GetDouble` throws `InvalidOperationException`). Unused by the application; ported for completeness.
pub fn nan_as_null_double_read(token: &serde_json::Value) -> Option<f64> {
    todo!()
}
