//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeRules.cs` (C# baseline e441d67a, 201 lines): the rules
//! configuration (`RulesConfig`, `tools/diag/rules.json`) and the nine pure decision-rule evaluators (`PerfRules`).
//!
//! In C# `PerfRules` is a static class; its `EvaluateRXxx` methods are the free functions `evaluate_r_xxx`.
//!
//! This module also owns the two number formatters every `pr-perf` text output relies on, because the rules'
//! evidence strings are where they matter most: [`fmt_fixed`] (`{v:F1}` / `{v:F2}`) and [`fmt_double`] (`{v}`,
//! i.e. `double.ToString()` under the invariant culture, which is also what System.Text.Json writes).
//!
//! Unit W1-18. Contract: contracts/W1-18/contract.md. Depends on `crate::csv` (PerfError) and `crate::nav`.
#![allow(unused_variables, dead_code, reason = "stub: todo!() bodies, remove when filling")]

use std::collections::HashMap;
use std::path::Path;

use crate::csv::PerfError;
use crate::nav::{FolderGenSummary, NavRecord};

/// Fixed-point text of `value` with `decimals` digits, like C# `value.ToString("F<decimals>", Invariant)` /
/// `$"{value:F1}"` under `FormattableString.Invariant`: the exact decimal expansion of the double rounded
/// half-to-even on exact ties (this is what Rust's `{:.N}` does), `-0.0` -> `-0.0`, a negative value that rounds to
/// zero keeps its sign (`-0.04` -> `-0.0`); NaN -> `NaN`, +inf -> `Infinity`, -inf -> `-Infinity`.
pub fn fmt_fixed(value: f64, decimals: usize) -> String {
    todo!()
}

/// `double.ToString()` / `"R"` / interpolation `{value}` under the invariant culture, and the number text
/// System.Text.Json writes: the shortest decimal digits that round-trip, laid out as plain decimal when the decimal
/// exponent `e` of the first digit satisfies `-5 < e < 17` (i.e. `1e-4` -> `0.0001`, `1e16` -> `10000000000000000`),
/// otherwise scientific `d.dddE+XX` / `d.dddE-XX` (mantissa `d` or `d.ddd`, `E`, sign always present, exponent at
/// least two digits): `1e-5` -> `1E-05`, `1e17` -> `1E+17`, `1.5e300` -> `1.5E+300`, `2.5e-7` -> `2.5E-07`.
/// `-0.0` -> `-0`. NaN -> `NaN`, +inf -> `Infinity`, -inf -> `-Infinity`.
pub fn fmt_double(value: f64) -> String {
    todo!()
}

/// `RulesConfig`: nested `{rule: {key: number}}` thresholds loaded from a JSON file (`tools/diag/rules.json`).
#[derive(Debug, Clone, Default)]
pub struct RulesConfig {
    data: HashMap<String, HashMap<String, f64>>,
}

impl RulesConfig {
    /// `Get(rule, key, default)`: the configured number, else `default`. Rule and key match case-sensitively.
    pub fn get(&self, rule: &str, key: &str, default: f64) -> f64 {
        todo!()
    }

    /// `RulesConfig.Load(path)`: reads the file (blocking) and calls [`RulesConfig::parse`]. A file-system failure is
    /// `Err(PerfError::Io)` (not wrapped).
    pub fn load(path: &Path) -> Result<RulesConfig, PerfError> {
        todo!()
    }

    /// The parsing half of `Load`. `path_for_message` is only used in the error text. A JSON syntax error, a non-object
    /// root, a section that is not an object or `null`, or a value that is not a number is
    /// `Err(PerfError::InvalidData("Invalid rules file '<path>': <detail>"))`. JSON `null` as the whole document and
    /// `"R-X": null` sections mean "all defaults". A UTF-8 BOM is ignored (`File.ReadAllText`).
    pub fn parse(json: &str, path_for_message: &str) -> Result<RulesConfig, PerfError> {
        todo!()
    }
}

/// Result of one rule for one group (C# record `RuleResult(Rule, Triggered, Evidence, Note = null)`).
/// `triggered == None` means "not enough data" (N/A).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleResult {
    pub rule: String,
    pub triggered: Option<bool>,
    pub evidence: String,
    pub note: Option<String>,
}

impl RuleResult {
    pub fn new(rule: &str, triggered: Option<bool>, evidence: impl Into<String>, note: Option<&str>) -> Self {
        Self {
            rule: rule.to_owned(),
            triggered,
            evidence: evidence.into(),
            note: note.map(str::to_owned),
        }
    }
}

/// `EvaluateRIo`: SourceMiss navigations, mean `t_read / final_visual` share (percent) >= `sourceMissIoSharePct` (40).
pub fn evaluate_r_io(cfg: &RulesConfig, source_miss_navs: &[NavRecord]) -> RuleResult {
    todo!()
}

/// `EvaluateRDec`: SourceMiss/InflightJoin navigations, mean `t_decode / final_visual` share >= `decodeSharePct` (40).
pub fn evaluate_r_dec(cfg: &RulesConfig, decode_navs: &[NavRecord]) -> RuleResult {
    todo!()
}

/// `EvaluateRUi`: RamHit navigations: mean `(t_input + t_assign + t_render) / final_visual` share >= `ramHitUiSharePct`
/// (40), or `ram_hit_final_p95 > ramHitP95Ms` (50), or `frame_time_p95_ms > frameTimeP95Ms` (33).
pub fn evaluate_r_ui(
    cfg: &RulesConfig,
    ram_hit_navs: &[NavRecord],
    ram_hit_final_p95: f64,
    frame_time_p95_ms: Option<f64>,
) -> RuleResult {
    todo!()
}

/// `EvaluateRPre`: `ram_hit_final_p95 <= ramHitP95MaxMs` (50) and the non-RamHit share >= the burst (`nonRamHitShareS3S4Pct`
/// 30) or paced (`nonRamHitShareS2Pct` 10) threshold.
pub fn evaluate_r_pre(
    cfg: &RulesConfig,
    ram_hit_final_p95: f64,
    non_ram_hit_share_pct: f64,
    is_burst_scenario: bool,
) -> RuleResult {
    todo!()
}

/// `EvaluateRCont`: `decode(high workers) / max(decode(low workers), 0.0001) >= decodeSlowdownFactor` (1.3).
pub fn evaluate_r_cont(
    cfg: &RulesConfig,
    decode_ms_low_workers: Option<f64>,
    decode_ms_high_workers: Option<f64>,
    low_workers: Option<i32>,
    high_workers: Option<i32>,
) -> RuleResult {
    todo!()
}

/// `EvaluateRThread`: `dispatcher_long_op_count > 0` or `t_input_p95_ms > inputP95Ms` (16).
pub fn evaluate_r_thread(cfg: &RulesConfig, dispatcher_long_op_count: i32, t_input_p95_ms: Option<f64>) -> RuleResult {
    todo!()
}

/// `EvaluateRGc`: `gc_time_percent >= gcTimePct` (10); N/A when `None`.
pub fn evaluate_r_gc(cfg: &RulesConfig, gc_time_percent: Option<f64>) -> RuleResult {
    todo!()
}

/// `EvaluateRDisk`: mean `t_disk` of the DiskCacheHit navs >= `diskVsReadDecodeFactor` (1.0) * mean `(t_read ?? 0) + t_decode`
/// of the SourceMiss navs.
pub fn evaluate_r_disk(
    cfg: &RulesConfig,
    disk_cache_hit_navs: &[NavRecord],
    source_miss_navs: &[NavRecord],
) -> RuleResult {
    todo!()
}

/// `EvaluateRFolder`: the worst (largest, first on ties) `t2_ms` is > `folderT0ToT2Ms` (1000); N/A without any T2.
pub fn evaluate_r_folder(cfg: &RulesConfig, gens: &[FolderGenSummary]) -> RuleResult {
    todo!()
}
