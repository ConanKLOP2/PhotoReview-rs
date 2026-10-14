//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeRules.cs` (C# baseline e441d67a, 201 lines): the rules
//! configuration (`RulesConfig`, `tools/diag/rules.json`) and the nine pure decision-rule evaluators (`PerfRules`).
//!
//! In C# `PerfRules` is a static class; its `EvaluateRXxx` methods are the free functions `evaluate_r_xxx`.
//!
//! This module also owns the two number formatters every `pr-perf` text output relies on, because the rules'
//! evidence strings are where they matter most: [`fmt_fixed`] (`{v:F1}` / `{v:F2}`) and [`fmt_double`] (`{v}`,
//! i.e. `double.ToString()` under the invariant culture, which is also what System.Text.Json writes).
//!
//! Unit W1-18. Contract: contracts/W1-18.md. Depends on `crate::csv` (PerfError) and `crate::nav`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::csv::PerfError;
use crate::nav::{FolderGenSummary, NavRecord};

/// Fixed-point text of `value` with `decimals` digits, like C# `value.ToString("F<decimals>", Invariant)` /
/// `$"{value:F1}"` under `FormattableString.Invariant`: the exact decimal expansion of the double rounded
/// half-to-even on exact ties (this is what Rust's `{:.N}` does), `-0.0` -> `-0.0`, a negative value that rounds to
/// zero keeps its sign (`-0.04` -> `-0.0`); NaN -> `NaN`, +inf -> `Infinity`, -inf -> `-Infinity`.
pub fn fmt_fixed(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "Infinity".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        format!("{value:.decimals$}")
    }
}

/// `double.ToString()` / `"R"` / interpolation `{value}` under the invariant culture, and the number text
/// System.Text.Json writes: the shortest decimal digits that round-trip, laid out as plain decimal when the decimal
/// exponent `e` of the first digit satisfies `-5 < e < 17` (i.e. `1e-4` -> `0.0001`, `1e16` -> `10000000000000000`),
/// otherwise scientific `d.dddE+XX` / `d.dddE-XX` (mantissa `d` or `d.ddd`, `E`, sign always present, exponent at
/// least two digits): `1e-5` -> `1E-05`, `1e17` -> `1E+17`, `1.5e300` -> `1.5E+300`, `2.5e-7` -> `2.5E-07`.
/// `-0.0` -> `-0`. NaN -> `NaN`, +inf -> `Infinity`, -inf -> `-Infinity`.
pub fn fmt_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // `{:e}` without precision is the shortest round-trip form, e.g. `1.2345e-5`, `4e1`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent_text) = scientific.split_once('e').expect("LowerExp output always contains 'e'");
    let exponent: i32 = exponent_text.parse().expect("LowerExp exponent is an integer");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();

    let body = if exponent >= 17 || exponent <= -5 {
        let mut text = digits[..1].to_owned();
        if digits.len() > 1 {
            text.push('.');
            text.push_str(&digits[1..]);
        }
        text.push('E');
        text.push(if exponent < 0 { '-' } else { '+' });
        text.push_str(&format!("{:02}", exponent.unsigned_abs()));
        text
    } else if exponent >= 0 {
        let integer_len = exponent as usize + 1;
        let mut text = String::new();
        if digits.len() <= integer_len {
            text.push_str(&digits);
            text.extend(std::iter::repeat_n('0', integer_len - digits.len()));
        } else {
            text.push_str(&digits[..integer_len]);
            text.push('.');
            text.push_str(&digits[integer_len..]);
        }
        text
    } else {
        // -5 < exponent < 0
        let zeros = (-exponent - 1) as usize;
        format!("0.{}{digits}", "0".repeat(zeros))
    };
    format!("{sign}{body}")
}

/// `RulesConfig`: nested `{rule: {key: number}}` thresholds loaded from a JSON file (`tools/diag/rules.json`).
#[derive(Debug, Clone, Default)]
pub struct RulesConfig {
    data: HashMap<String, HashMap<String, f64>>,
}

impl RulesConfig {
    /// `Get(rule, key, default)`: the configured number, else `default`. Rule and key match case-sensitively.
    pub fn get(&self, rule: &str, key: &str, default: f64) -> f64 {
        self.data
            .get(rule)
            .and_then(|section| section.get(key))
            .copied()
            .unwrap_or(default)
    }

    /// `RulesConfig.Load(path)`: reads the file (blocking) and calls [`RulesConfig::parse`]. A file-system failure is
    /// `Err(PerfError::Io)` (not wrapped).
    pub fn load(path: &Path) -> Result<RulesConfig, PerfError> {
        let bytes = std::fs::read(path)?;
        let text = decode_text(&bytes);
        Self::parse(&text, &path.display().to_string())
    }

    /// The parsing half of `Load`. `path_for_message` is only used in the error text. A JSON syntax error, a non-object
    /// root, a section that is not an object or `null`, or a value that is not a number is
    /// `Err(PerfError::InvalidData("Invalid rules file '<path>': <detail>"))`. JSON `null` as the whole document and
    /// `"R-X": null` sections mean "all defaults". A UTF-8 BOM is ignored (`File.ReadAllText`).
    pub fn parse(json: &str, path_for_message: &str) -> Result<RulesConfig, PerfError> {
        let invalid =
            |detail: String| PerfError::InvalidData(format!("Invalid rules file '{path_for_message}': {detail}"));
        let text = json.strip_prefix('\u{feff}').unwrap_or(json);
        let root: Value = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
        let mut data = HashMap::new();
        match root {
            Value::Null => {}
            Value::Object(rules) => {
                for (rule, section) in rules {
                    match section {
                        // A null section ("R-X": null) means "all defaults".
                        Value::Null => {}
                        Value::Object(keys) => {
                            let mut values = HashMap::with_capacity(keys.len());
                            for (key, value) in keys {
                                let number = value
                                    .as_f64()
                                    .ok_or_else(|| invalid(format!("value of '{rule}.{key}' is not a number")))?;
                                values.insert(key, number);
                            }
                            data.insert(rule, values);
                        }
                        _ => return Err(invalid(format!("section '{rule}' is not an object"))),
                    }
                }
            }
            _ => return Err(invalid("root is not an object".to_owned())),
        }
        Ok(RulesConfig { data })
    }
}

/// The decoding `File.ReadAllText` applies to a UTF-8 file: a leading UTF-8 BOM is removed and invalid sequences
/// become U+FFFD. Same rule as `csv::decode_bytes` (W1-16), kept local so this unit does not depend on an unmerged unit.
fn decode_text(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(body).into_owned()
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

/// `"12.3ms"`, or `"N/A"` for an absent or NaN value (C# `FmtMs`).
fn fmt_ms(value: Option<f64>) -> String {
    match value {
        Some(v) if !v.is_nan() => format!("{}ms", fmt_fixed(v, 1)),
        _ => "N/A".to_owned(),
    }
}

/// C# `FinalVisualMs is > 0`: the final visual time when it is present and positive (NaN is not).
fn positive_final(nav: &NavRecord) -> Option<f64> {
    nav.final_visual_ms.filter(|v| *v > 0.0)
}

/// LINQ `Average` on doubles: left-to-right sum (starting at +0.0), then divided by the count.
fn average(values: &[f64]) -> f64 {
    values.iter().fold(0.0, |sum, v| sum + v) / values.len() as f64
}

/// `Comparer<double>.Default` (used by LINQ `MaxBy`): NaN sorts below every number and equals another NaN.
fn compare_like_csharp(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

/// `EvaluateRIo`: SourceMiss navigations, mean `t_read / final_visual` share (percent) >= `sourceMissIoSharePct` (40).
pub fn evaluate_r_io(cfg: &RulesConfig, source_miss_navs: &[NavRecord]) -> RuleResult {
    let share_threshold = cfg.get("R-IO", "sourceMissIoSharePct", 40.0);
    let shares: Vec<f64> = source_miss_navs
        .iter()
        .filter_map(|n| Some(n.t_read_ms? / positive_final(n)?))
        .collect();
    if shares.is_empty() {
        return RuleResult::new(
            "R-IO",
            None,
            format!("0/{} SourceMiss navs have t_read", source_miss_navs.len()),
            Some(
                "No source-read timing; only available with PHOTOREVIEW_DIAG_PREREAD. Use the Procmon D02 data to derive open counts.",
            ),
        );
    }
    let avg_share = average(&shares) * 100.0;
    RuleResult::new(
        "R-IO",
        Some(avg_share >= share_threshold),
        format!(
            "avg(t_read/finalVisual)={}% over {} SourceMiss navs (threshold {}%)",
            fmt_fixed(avg_share, 1),
            shares.len(),
            fmt_double(share_threshold)
        ),
        None,
    )
}

/// `EvaluateRDec`: SourceMiss/InflightJoin navigations, mean `t_decode / final_visual` share >= `decodeSharePct` (40).
pub fn evaluate_r_dec(cfg: &RulesConfig, decode_navs: &[NavRecord]) -> RuleResult {
    let share_threshold = cfg.get("R-DEC", "decodeSharePct", 40.0);
    let shares: Vec<f64> = decode_navs
        .iter()
        .filter_map(|n| Some(n.t_decode_ms? / positive_final(n)?))
        .collect();
    if shares.is_empty() {
        return RuleResult::new(
            "R-DEC",
            None,
            "0 SourceMiss/InflightJoin navs have t_decode",
            Some("Not enough decode data."),
        );
    }
    let avg_share = average(&shares) * 100.0;
    RuleResult::new(
        "R-DEC",
        Some(avg_share >= share_threshold),
        format!(
            "avg(t_decode/finalVisual)={}% over {} navs (threshold {}%)",
            fmt_fixed(avg_share, 1),
            shares.len(),
            fmt_double(share_threshold)
        ),
        None,
    )
}

/// `EvaluateRUi`: RamHit navigations: mean `(t_input + t_assign + t_render) / final_visual` share >= `ramHitUiSharePct`
/// (40), or `ram_hit_final_p95 > ramHitP95Ms` (50), or `frame_time_p95_ms > frameTimeP95Ms` (33).
pub fn evaluate_r_ui(
    cfg: &RulesConfig,
    ram_hit_navs: &[NavRecord],
    ram_hit_final_p95: f64,
    frame_time_p95_ms: Option<f64>,
) -> RuleResult {
    let share_threshold = cfg.get("R-UI", "ramHitUiSharePct", 40.0);
    let p95_threshold = cfg.get("R-UI", "ramHitP95Ms", 50.0);
    let frame_threshold = cfg.get("R-UI", "frameTimeP95Ms", 33.0);

    let shares: Vec<f64> = ram_hit_navs
        .iter()
        .filter_map(|n| {
            let final_ms = positive_final(n)?;
            let ui = n.t_input_ms.unwrap_or(0.0) + n.t_assign_ms.unwrap_or(0.0) + n.t_render_ms.unwrap_or(0.0);
            Some(ui / final_ms)
        })
        .collect();
    let avg_share = if shares.is_empty() {
        None
    } else {
        Some(average(&shares) * 100.0)
    };

    let by_share = avg_share.is_some_and(|s| s >= share_threshold);
    let by_p95 = !ram_hit_final_p95.is_nan() && ram_hit_final_p95 > p95_threshold;
    let by_frame = frame_time_p95_ms.is_some_and(|f| f > frame_threshold);

    let share_text = match avg_share {
        Some(s) => format!("{}%", fmt_fixed(s, 1)),
        None => "N/A".to_owned(),
    };
    // No NaN check here on purpose: C# prints a NaN frame time as `NaNms`.
    let frame_text = match frame_time_p95_ms {
        Some(f) => format!("{}ms", fmt_fixed(f, 1)),
        None => "N/A".to_owned(),
    };
    let evidence = format!(
        "avg((t_input+t_assign+t_render)/finalVisual)={share_text} (threshold {}%); RamHit finalVisual P95={} (threshold {}ms); frameTime P95={frame_text} (threshold {}ms)",
        fmt_double(share_threshold),
        fmt_ms(Some(ram_hit_final_p95)),
        fmt_double(p95_threshold),
        fmt_double(frame_threshold)
    );

    if avg_share.is_none() && ram_hit_final_p95.is_nan() && frame_time_p95_ms.is_none() {
        return RuleResult::new(
            "R-UI",
            None,
            evidence,
            Some("No RamHit navs or frame-time data in this group."),
        );
    }
    RuleResult::new("R-UI", Some(by_share || by_p95 || by_frame), evidence, None)
}

/// `EvaluateRPre`: `ram_hit_final_p95 <= ramHitP95MaxMs` (50) and the non-RamHit share >= the burst (`nonRamHitShareS3S4Pct`
/// 30) or paced (`nonRamHitShareS2Pct` 10) threshold.
pub fn evaluate_r_pre(
    cfg: &RulesConfig,
    ram_hit_final_p95: f64,
    non_ram_hit_share_pct: f64,
    is_burst_scenario: bool,
) -> RuleResult {
    let ram_hit_max = cfg.get("R-PRE", "ramHitP95MaxMs", 50.0);
    let (threshold_key, default_threshold, scenario) = if is_burst_scenario {
        ("nonRamHitShareS3S4Pct", 30.0, "S3/S4")
    } else {
        ("nonRamHitShareS2Pct", 10.0, "S2")
    };
    let threshold = cfg.get("R-PRE", threshold_key, default_threshold);

    if ram_hit_final_p95.is_nan() {
        return RuleResult::new("R-PRE", None, "No RamHit navs in this group.", None);
    }
    let triggered = ram_hit_final_p95 <= ram_hit_max && non_ram_hit_share_pct >= threshold;
    RuleResult::new(
        "R-PRE",
        Some(triggered),
        format!(
            "RamHit finalVisual P95={}ms (threshold ≤{}ms); non-RamHit share={}% (threshold ≥{}% for {scenario})",
            fmt_fixed(ram_hit_final_p95, 1),
            fmt_double(ram_hit_max),
            fmt_fixed(non_ram_hit_share_pct, 1),
            fmt_double(threshold)
        ),
        None,
    )
}

/// The worker count as C# interpolates an `int?`: the decimal number, or the empty text for `null`.
fn worker_text(workers: Option<i32>) -> String {
    workers.map(|w| w.to_string()).unwrap_or_default()
}

/// `EvaluateRCont`: `decode(high workers) / max(decode(low workers), 0.0001) >= decodeSlowdownFactor` (1.3).
pub fn evaluate_r_cont(
    cfg: &RulesConfig,
    decode_ms_low_workers: Option<f64>,
    decode_ms_high_workers: Option<f64>,
    low_workers: Option<i32>,
    high_workers: Option<i32>,
) -> RuleResult {
    let factor = cfg.get("R-CONT", "decodeSlowdownFactor", 1.3);
    match (decode_ms_low_workers, decode_ms_high_workers) {
        (Some(low), Some(high)) if low_workers != high_workers => {
            // C# Math.Max propagates NaN; f64::max would ignore it, so NaN is kept explicitly.
            let low_clamped = if low.is_nan() { f64::NAN } else { low.max(0.0001) };
            let ratio = high / low_clamped;
            RuleResult::new(
                "R-CONT",
                Some(ratio >= factor),
                format!(
                    "t_decode(workers={})={}ms vs t_decode(workers={})={}ms, ratio={} (threshold {})",
                    worker_text(high_workers),
                    fmt_fixed(high, 1),
                    worker_text(low_workers),
                    fmt_fixed(low, 1),
                    fmt_fixed(ratio, 2),
                    fmt_double(factor)
                ),
                None,
            )
        }
        _ => RuleResult::new(
            "R-CONT",
            None,
            "Missing a pair of runs with different PHOTOREVIEW_DIAG_PRELOAD_WORKERS to compare.",
            Some(
                "Needs at least 2 runs of the same scenario/mode with different PHOTOREVIEW_DIAG_PRELOAD_WORKERS (D07).",
            ),
        ),
    }
}

/// `EvaluateRThread`: `dispatcher_long_op_count > 0` or `t_input_p95_ms > inputP95Ms` (16).
pub fn evaluate_r_thread(cfg: &RulesConfig, dispatcher_long_op_count: i32, t_input_p95_ms: Option<f64>) -> RuleResult {
    let ms_threshold = cfg.get("R-THREAD", "dispatcherLongOpMs", 16.0);
    let input_threshold = cfg.get("R-THREAD", "inputP95Ms", 16.0);
    let by_dispatcher = dispatcher_long_op_count > 0;
    let by_input = t_input_p95_ms.is_some_and(|t| t > input_threshold);
    let input_text = match t_input_p95_ms {
        Some(v) => format!("{}ms", fmt_fixed(v, 1)),
        None => "N/A".to_owned(),
    };
    let evidence = format!(
        "DispatcherLongOp>{}ms count={dispatcher_long_op_count}; t_input P95={input_text} (threshold {}ms)",
        fmt_double(ms_threshold),
        fmt_double(input_threshold)
    );
    if dispatcher_long_op_count == 0 && t_input_p95_ms.is_none() {
        return RuleResult::new(
            "R-THREAD",
            None,
            evidence,
            Some("No DispatcherLongOp or t_input in this group."),
        );
    }
    RuleResult::new("R-THREAD", Some(by_dispatcher || by_input), evidence, None)
}

/// `EvaluateRGc`: `gc_time_percent >= gcTimePct` (10); N/A when `None`.
pub fn evaluate_r_gc(cfg: &RulesConfig, gc_time_percent: Option<f64>) -> RuleResult {
    let threshold = cfg.get("R-GC", "gcTimePct", 10.0);
    match gc_time_percent {
        None => RuleResult::new(
            "R-GC",
            None,
            "No process.json/% time in GC for this group.",
            Some("Needs dotnet-counters (D09) or process.json (D06)."),
        ),
        Some(v) => RuleResult::new(
            "R-GC",
            Some(v >= threshold),
            format!(
                "% time in GC={}% (threshold {}%)",
                fmt_fixed(v, 1),
                fmt_double(threshold)
            ),
            None,
        ),
    }
}

/// `EvaluateRDisk`: mean `t_disk` of the DiskCacheHit navs >= `diskVsReadDecodeFactor` (1.0) * mean `(t_read ?? 0) + t_decode`
/// of the SourceMiss navs.
pub fn evaluate_r_disk(
    cfg: &RulesConfig,
    disk_cache_hit_navs: &[NavRecord],
    source_miss_navs: &[NavRecord],
) -> RuleResult {
    let factor = cfg.get("R-DISK", "diskVsReadDecodeFactor", 1.0);
    let disk: Vec<f64> = disk_cache_hit_navs.iter().filter_map(|n| n.t_disk_ms).collect();
    let read_decode: Vec<f64> = source_miss_navs
        .iter()
        .filter_map(|n| Some(n.t_read_ms.unwrap_or(0.0) + n.t_decode_ms?))
        .collect();
    if disk.is_empty() || read_decode.is_empty() {
        return RuleResult::new(
            "R-DISK",
            None,
            format!(
                "diskCacheHit navs={}, sourceMiss navs={}",
                disk.len(),
                read_decode.len()
            ),
            Some("Needs at least one DiskCacheHit nav and one SourceMiss nav in the same group."),
        );
    }
    let avg_disk = average(&disk);
    let avg_read_decode = average(&read_decode);
    RuleResult::new(
        "R-DISK",
        Some(avg_disk >= factor * avg_read_decode),
        format!(
            "avg(t_disk)={}ms vs avg(t_read+t_decode)={}ms (threshold factor {})",
            fmt_fixed(avg_disk, 1),
            fmt_fixed(avg_read_decode, 1),
            fmt_double(factor)
        ),
        None,
    )
}

/// `EvaluateRFolder`: the worst (largest, first on ties) `t2_ms` is > `folderT0ToT2Ms` (1000); N/A without any T2.
pub fn evaluate_r_folder(cfg: &RulesConfig, gens: &[FolderGenSummary]) -> RuleResult {
    let threshold = cfg.get("R-FOLDER", "folderT0ToT2Ms", 1000.0);
    // C# MaxBy: a later generation replaces the worst only when strictly greater, so the first maximum wins.
    let mut worst: Option<(&FolderGenSummary, f64)> = None;
    for folder_gen in gens {
        let Some(t2) = folder_gen.t2_ms else { continue };
        let replace = match worst {
            None => true,
            Some((_, best)) => compare_like_csharp(t2, best) == Ordering::Greater,
        };
        if replace {
            worst = Some((folder_gen, t2));
        }
    }
    let Some((worst, t2)) = worst else {
        return RuleResult::new(
            "R-FOLDER",
            None,
            "No folder generation has T2 (first image presented).",
            None,
        );
    };
    RuleResult::new(
        "R-FOLDER",
        Some(t2 > threshold),
        format!(
            "worst T0->T2={} (gen={}, T1 catalogReady={}, T3 {}={}; threshold {}ms)",
            fmt_ms(Some(t2)),
            worst.generation,
            fmt_ms(worst.t1_catalog_ready_ms),
            worst.t3_phase,
            fmt_ms(worst.t3_ms),
            fmt_double(threshold)
        ),
        None,
    )
}
