//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeReport.cs` (C# baseline e441d67a, 183 lines): writes `summary.md`
//! (Markdown tables) and `summary.json` (numeric detail) for one `--perf-analyze` run.
//!
//! The two outputs must be BYTE-IDENTICAL to the C# files (golden-tested), so nothing here may use `serde_json`
//! for output or a `{:?}`/`{}` float format: the JSON is written by a small hand-written writer that reproduces
//! `System.Text.Json` with `WriteIndented = true` and the default encoder (CRLF newlines, two-space indent,
//! `\u00XX`-style escapes for non-ASCII and HTML-sensitive characters, .NET number text). See the contract.
//!
//! Unit W1-20. Contract: contracts/W1-20/contract.md. Depends on `crate::analyze` (AnalysisResult, the
//! NaN-as-null number), `crate::stats` (GroupSummary), `crate::rules` (fmt_fixed / fmt_double, RuleResult).
#![allow(unused_variables, dead_code, reason = "stub: todo!() bodies, remove when filling")]

use std::path::Path;

use crate::analyze::AnalysisResult;
use crate::csv::PerfError;
use crate::stats::GroupSummary;

/// `Environment.NewLine` on Windows: `StringBuilder.AppendLine` and `Utf8JsonWriter` (indented) both use it, so
/// both output files contain CRLF line breaks.
pub const NEW_LINE: &str = "\r\n";

/// `WriteMarkdown` without the file write: the exact text of `summary.md` (ends with [`NEW_LINE`]).
pub fn render_markdown(result: &AnalysisResult) -> String {
    todo!()
}

/// `WriteJson` without the file write: the exact text of `summary.json` (no trailing newline).
pub fn render_json(result: &AnalysisResult) -> String {
    todo!()
}

/// `PerfAnalyzeReport.WriteMarkdown(path, result)`: `File.WriteAllText(path, text)` = UTF-8 without BOM.
/// Failures are `Err(PerfError::Io)`.
pub fn write_markdown(path: &Path, result: &AnalysisResult) -> Result<(), PerfError> {
    todo!()
}

/// `PerfAnalyzeReport.WriteJson(path, result)`: same encoding as [`write_markdown`].
pub fn write_json(path: &Path, result: &AnalysisResult) -> Result<(), PerfError> {
    todo!()
}

/// One `(Phase, Median, Min, Max, Count)` tuple of C# `StartupPhases`.
#[derive(Debug, Clone, PartialEq)]
pub struct StartupPhase {
    pub phase: String,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub count: i32,
}

/// `PerfAnalyzeReport.StartupPhases(summary)` (internal in C#): per-phase median/min/max/count over the group's
/// `startup_runs`; phases in first-seen order across the runs, then a STABLE sort ascending by median.
/// Median = middle value of the ascending values, or the mean of the two middle values for an even count.
pub fn startup_phases(summary: &GroupSummary) -> Vec<StartupPhase> {
    todo!()
}

/// `JsonSerializer.Serialize(string)` with the default `JavaScriptEncoder`, i.e. the way every string, property
/// name and dictionary key is written into `summary.json`: surrounded by `"`; `"`, `&`, `'`, `+`, `<`, `>`, `` ` ``
/// -> `" & ' + < > ``; `\` -> `\\`; `\b \f \n \r \t` -> short escapes; every other
/// control char below U+0020 and U+007F -> `\u00XX`; every non-ASCII char -> `\uXXXX` of its UTF-16 code unit(s)
/// (a supplementary char is two escapes). Hex digits are UPPER case. `/` is NOT escaped.
pub fn json_string_literal(s: &str) -> String {
    todo!()
}
