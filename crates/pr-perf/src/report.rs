//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeReport.cs` (C# baseline e441d67a, 183 lines): writes `summary.md`
//! (Markdown tables) and `summary.json` (numeric detail) for one `--perf-analyze` run.
//!
//! The two outputs must be BYTE-IDENTICAL to the C# files (golden-tested), so nothing here may use `serde_json`
//! for output or a `{:?}`/`{}` float format: the JSON is written by a small hand-written writer that reproduces
//! `System.Text.Json` with `WriteIndented = true` and the default encoder (CRLF newlines, two-space indent,
//! `\u00XX`-style escapes for non-ASCII and HTML-sensitive characters, .NET number text). See the contract.
//!
//! Unit W1-20. Contract: contracts/W1-20/contract.md. Depends on `crate::analyze` (AnalysisResult), `crate::stats`
//! (GroupSummary).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::analyze::AnalysisResult;
use crate::csv::PerfError;
use crate::nav::FolderGenSummary;
use crate::stats::GroupSummary;

/// `Environment.NewLine` on Windows: `StringBuilder.AppendLine` and `Utf8JsonWriter` (indented) both use it, so
/// both output files contain CRLF line breaks.
pub const NEW_LINE: &str = "\r\n";

// ---------------------------------------------------------------------------------------------------------------
// Private number/text helpers. They are local so this unit does not depend on the `todo!()` bodies of the other
// perf units while those are ported in parallel; they follow the documented contracts of `rules::fmt_fixed`,
// `rules::fmt_double`, `stats::nearest_rank`, `GroupKey::to_string`, `GroupSummary::low_sample_warning` and
// `analyze::nan_as_null_double_json` exactly (the golden pins the bytes).
// ---------------------------------------------------------------------------------------------------------------

/// `value.ToString("F<decimals>", Invariant)`.
fn fixed(value: f64, decimals: usize) -> String {
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

/// `double.ToString()` under the invariant culture (the number text System.Text.Json writes).
fn double_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let sci = format!("{:e}", value.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let mut out = String::new();
    if value < 0.0 {
        out.push('-');
    }
    if -5 < exp && exp < 17 {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                out.push_str(&digits);
                out.extend(std::iter::repeat_n('0', int_len - digits.len()));
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-exp - 1) as usize));
            out.push_str(&digits);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    out
}

/// `NaNAsNullDoubleConverter.Write`.
fn nan_null(value: f64) -> String {
    if value.is_finite() {
        double_text(value)
    } else {
        "null".to_owned()
    }
}

/// `GroupKey.ToString()`.
fn key_text(s: &GroupSummary) -> String {
    let k = &s.key;
    match k.preload_workers {
        Some(n) => format!("{}/{}/{}/workers={}", k.scenario, k.mode, k.cond, n),
        None => format!("{}/{}/{}/workers=?", k.scenario, k.mode, k.cond),
    }
}

/// `PerfStats.NearestRank` applied to a possibly empty list (`PercentileOrNaN`).
fn percentile_or_nan(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() || p.is_nan() {
        return f64::NAN;
    }
    let n = sorted.len();
    let p = p.clamp(0.0, 100.0);
    let rank = ((p * n as f64) / 100.0).ceil();
    let rank = if rank < 1.0 { 1 } else { (rank as usize).min(n) };
    sorted[rank - 1]
}

/// .NET `Comparer<double>.Default`: NaN is less than every number and equal to NaN.
fn cmp_dotnet(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

fn ms(value: f64) -> String {
    if value.is_nan() {
        "N/A".to_owned()
    } else {
        fixed(value, 1)
    }
}

fn ms_opt(value: Option<f64>) -> String {
    value.map_or_else(|| "N/A".to_owned(), ms)
}

fn pct(value: f64) -> String {
    if value.is_nan() {
        "N/A".to_owned()
    } else {
        format!("{}%", fixed(value * 100.0, 1))
    }
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn line(sb: &mut String, text: &str) {
    sb.push_str(text);
    sb.push_str(NEW_LINE);
}

/// `WriteMarkdown` without the file write: the exact text of `summary.md` (ends with [`NEW_LINE`]).
pub fn render_markdown(result: &AnalysisResult) -> String {
    let mut sb = String::new();
    line(&mut sb, "# Perf analyze summary");
    line(&mut sb, "");
    line(&mut sb, &format!("- perf-*.csv files: {}", result.csv_file_count));
    line(
        &mut sb,
        &format!("- Groups (scenario/mode/cond/workers): {}", result.groups.len()),
    );
    for g in result.groups.iter().filter(|g| g.summary.dropped_rows > 0) {
        line(
            &mut sb,
            &format!(
                "- WARNING: {}: dropped {} events (listener queue overflow); percentiles and counts for this group are incomplete",
                escape(&key_text(&g.summary)),
                g.summary.dropped_rows
            ),
        );
    }
    line(&mut sb, "");

    line(&mut sb, "## Navigation groups (scenario/mode/cond/workers)");
    line(&mut sb, "");
    line(
        &mut sb,
        "| Group | count | incomplete | first P50 | first P95 | first max | final P50 | final P95 | final max | renderedFrame P50 | renderedFrame P95 | renderedFrame max | hit rate | notes |",
    );
    line(
        &mut sb,
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|",
    );
    for g in &result.groups {
        let s = &g.summary;
        let low = s.count.wrapping_sub(s.incomplete) < 20;
        line(
            &mut sb,
            &format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                escape(&key_text(s)),
                s.count,
                s.incomplete,
                ms(s.first_p50),
                ms(s.first_p95),
                ms(s.first_max),
                ms(s.final_p50),
                ms(s.final_p95),
                ms(s.final_max),
                ms(s.rendered_frame_p50),
                ms(s.rendered_frame_p95),
                ms(s.rendered_frame_max),
                pct(s.hit_rate),
                if low { "N<20" } else { "" }
            ),
        );
    }
    line(&mut sb, "");

    line(&mut sb, "## Breakdown by kind");
    line(&mut sb, "");
    line(&mut sb, "| Group | kind | count |");
    line(&mut sb, "|---|---|---:|");
    for g in &result.groups {
        let mut kinds: Vec<(&String, &i32)> = g.summary.kind_counts.iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(a.1));
        for (kind, n) in kinds {
            line(
                &mut sb,
                &format!("| {} | {} | {} |", escape(&key_text(&g.summary)), escape(kind), n),
            );
        }
    }
    line(&mut sb, "");

    line(
        &mut sb,
        "## Phase share in the slowest 10% of navigations (mean t_x/finalVisual)",
    );
    line(&mut sb, "");
    line(&mut sb, "| Group | phase | share |");
    line(&mut sb, "|---|---|---:|");
    for g in &result.groups {
        let mut phases: Vec<(&String, &f64)> = g.summary.slowest_phase_share.iter().collect();
        phases.sort_by(|a, b| cmp_dotnet(*b.1, *a.1));
        for (phase, v) in phases {
            line(
                &mut sb,
                &format!(
                    "| {} | {} | {} |",
                    escape(&key_text(&g.summary)),
                    escape(phase),
                    pct(*v)
                ),
            );
        }
    }
    line(&mut sb, "");

    line(&mut sb, "## Preload");
    line(&mut sb, "");
    line(
        &mut sb,
        "| Group | worker | items | paused | cancel | queueWait P50 | queueWait P95 | decode P50 | decode P95 |",
    );
    line(&mut sb, "|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for g in &result.groups {
        let s = &g.summary;
        let mut queue_waits: Vec<f64> = s.preload_items.iter().map(|p| p.queue_wait_ms).collect();
        queue_waits.sort_by(|a, b| cmp_dotnet(*a, *b));
        let mut decode: Vec<f64> = s
            .preload_items
            .iter()
            .filter(|p| p.kind == "decoded")
            .map(|p| p.ms)
            .collect();
        decode.sort_by(|a, b| cmp_dotnet(*a, *b));
        line(
            &mut sb,
            &format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                escape(&key_text(s)),
                s.preload_workers.map_or_else(|| "?".to_owned(), |w| w.to_string()),
                s.preload_items.len(),
                s.preload_paused_count,
                s.preload_cancel_count,
                ms(percentile_or_nan(&queue_waits, 50.0)),
                ms(percentile_or_nan(&queue_waits, 95.0)),
                ms(percentile_or_nan(&decode, 50.0)),
                ms(percentile_or_nan(&decode, 95.0)),
            ),
        );
    }
    line(&mut sb, "");

    line(&mut sb, "## Dispatcher (>16ms)");
    line(&mut sb, "");
    line(
        &mut sb,
        "DispatcherLongOp before the first ShowStart/Folder(start) of each file is excluded (e.g. --perf-session builds the WPF window ~700 ms before the scenario starts \u{2014} D06).",
    );
    line(&mut sb, "");
    line(
        &mut sb,
        "| Group | count | (excluded before start) | total ms | top 5 names |",
    );
    line(&mut sb, "|---|---:|---:|---:|---|");
    for g in &result.groups {
        let s = &g.summary;
        // GroupBy(name): ordinal, groups in first-appearance order.
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut groups: Vec<(&str, i32, f64)> = Vec::new();
        for op in &s.dispatcher_long_ops {
            let i = *index.entry(op.name.as_str()).or_insert_with(|| {
                groups.push((op.name.as_str(), 0, 0.0));
                groups.len() - 1
            });
            groups[i].1 += 1;
            groups[i].2 += op.ms;
        }
        groups.sort_by(|a, b| cmp_dotnet(b.2, a.2));
        let top5: Vec<String> = groups
            .iter()
            .take(5)
            .map(|(name, count, total)| format!("{}\u{00D7}{} ({}ms)", escape(name), count, fixed(*total, 1)))
            .collect();
        let total = s.dispatcher_long_ops.iter().fold(0.0, |a, d| a + d.ms);
        line(
            &mut sb,
            &format!(
                "| {} | {} | {} | {} | {} |",
                escape(&key_text(s)),
                s.dispatcher_long_ops.len(),
                s.dispatcher_long_ops_before_start_count,
                fixed(total, 1),
                top5.join("; ")
            ),
        );
    }
    line(&mut sb, "");

    line(&mut sb, "## Folder (T0..T3)");
    line(&mut sb, "");
    line(
        &mut sb,
        "| Group | gen | T1 catalogReady | T2 (T0->first present) | T3 phase | T3 |",
    );
    line(&mut sb, "|---|---:|---:|---:|---|---:|");
    for g in &result.groups {
        let mut gens: Vec<&FolderGenSummary> = g.summary.folder_gens.iter().collect();
        gens.sort_by_key(|f| f.generation);
        for f in gens {
            line(
                &mut sb,
                &format!(
                    "| {} | {} | {} | {} | {} | {} |",
                    escape(&key_text(&g.summary)),
                    f.generation,
                    ms_opt(f.t1_catalog_ready_ms),
                    ms_opt(f.t2_ms),
                    escape(&f.t3_phase),
                    ms_opt(f.t3_ms)
                ),
            );
        }
    }
    line(&mut sb, "");

    line(&mut sb, "## Startup (ms since process start, median across runs)");
    line(&mut sb, "");
    line(&mut sb, "| Group | phase | median | min | max | N |");
    line(&mut sb, "|---|---|---:|---:|---:|---:|");
    for g in &result.groups {
        for p in startup_phases(&g.summary) {
            line(
                &mut sb,
                &format!(
                    "| {} | {} | {} | {} | {} | {} |",
                    escape(&key_text(&g.summary)),
                    escape(&p.phase),
                    fixed(p.median, 1),
                    fixed(p.min, 1),
                    fixed(p.max, 1),
                    p.count
                ),
            );
        }
    }
    line(&mut sb, "");

    line(&mut sb, "## Decision rules (rules.json)");
    line(&mut sb, "");
    line(&mut sb, "| Group | Rule | Triggered | Evidence | Notes |");
    line(&mut sb, "|---|---|---|---|---|");
    for g in &result.groups {
        for r in &g.rules {
            let triggered = match r.triggered {
                None => "N/A",
                Some(true) => "yes",
                Some(false) => "no",
            };
            line(
                &mut sb,
                &format!(
                    "| {} | {} | {} | {} | {} |",
                    escape(&key_text(&g.summary)),
                    escape(&r.rule),
                    triggered,
                    escape(&r.evidence),
                    escape(r.note.as_deref().unwrap_or(""))
                ),
            );
        }
    }
    sb
}

/// A JSON value tree printed like `System.Text.Json` with `WriteIndented = true`.
enum Json {
    Null,
    Bool(bool),
    /// Number text, already formatted.
    Raw(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn int(v: impl std::fmt::Display) -> Json {
        Json::Raw(v.to_string())
    }

    fn dbl(v: f64) -> Json {
        Json::Raw(nan_null(v))
    }

    fn dbl_opt(v: Option<f64>) -> Json {
        v.map_or(Json::Null, Json::dbl)
    }

    fn print(&self, out: &mut String, depth: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Raw(t) => out.push_str(t),
            Json::Str(s) => out.push_str(&json_string_literal(s)),
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                out.push_str(NEW_LINE);
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                        out.push_str(NEW_LINE);
                    }
                    out.extend(std::iter::repeat_n(' ', (depth + 1) * 2));
                    item.print(out, depth + 1);
                }
                out.push_str(NEW_LINE);
                out.extend(std::iter::repeat_n(' ', depth * 2));
                out.push(']');
            }
            Json::Object(members) => {
                if members.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                out.push_str(NEW_LINE);
                for (i, (name, value)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                        out.push_str(NEW_LINE);
                    }
                    out.extend(std::iter::repeat_n(' ', (depth + 1) * 2));
                    out.push_str(&json_string_literal(name));
                    out.push_str(": ");
                    value.print(out, depth + 1);
                }
                out.push_str(NEW_LINE);
                out.extend(std::iter::repeat_n(' ', depth * 2));
                out.push('}');
            }
        }
    }
}

fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn int_opt(v: Option<i32>) -> Json {
    v.map_or(Json::Null, Json::int)
}

fn triple(p50: f64, p95: f64, max: f64) -> Json {
    obj(vec![
        ("p50", Json::dbl(p50)),
        ("p95", Json::dbl(p95)),
        ("max", Json::dbl(max)),
    ])
}

/// `WriteJson` without the file write: the exact text of `summary.json` (no trailing newline).
pub fn render_json(result: &AnalysisResult) -> String {
    let groups: Vec<Json> = result
        .groups
        .iter()
        .map(|g| {
            let s = &g.summary;
            let low = s.count.wrapping_sub(s.incomplete) < 20;
            let folder = s
                .folder_gens
                .iter()
                .map(|f| {
                    obj(vec![
                        ("gen", Json::int(f.generation)),
                        ("t1CatalogReadyMs", Json::dbl_opt(f.t1_catalog_ready_ms)),
                        ("t2Ms", Json::dbl_opt(f.t2_ms)),
                        ("t3Phase", Json::Str(f.t3_phase.clone())),
                        ("t3Ms", Json::dbl_opt(f.t3_ms)),
                    ])
                })
                .collect();
            let startup = startup_phases(s)
                .into_iter()
                .map(|p| {
                    obj(vec![
                        ("phase", Json::Str(p.phase)),
                        ("median", Json::dbl(p.median)),
                        ("min", Json::dbl(p.min)),
                        ("max", Json::dbl(p.max)),
                        ("count", Json::int(p.count)),
                    ])
                })
                .collect();
            let rules = g
                .rules
                .iter()
                .map(|r| {
                    obj(vec![
                        ("rule", Json::Str(r.rule.clone())),
                        ("triggered", r.triggered.map_or(Json::Null, Json::Bool)),
                        ("evidence", Json::Str(r.evidence.clone())),
                        ("note", r.note.clone().map_or(Json::Null, Json::Str)),
                    ])
                })
                .collect();
            obj(vec![
                ("scenario", Json::Str(s.key.scenario.clone())),
                ("mode", Json::Str(s.key.mode.clone())),
                ("cond", Json::Str(s.key.cond.clone())),
                ("preloadWorkers", int_opt(s.key.preload_workers)),
                ("count", Json::int(s.count)),
                ("incomplete", Json::int(s.incomplete)),
                ("lowSampleWarning", Json::Bool(low)),
                ("droppedRows", Json::int(s.dropped_rows)),
                ("firstVisualMs", triple(s.first_p50, s.first_p95, s.first_max)),
                ("finalVisualMs", triple(s.final_p50, s.final_p95, s.final_max)),
                (
                    "renderedFrameMs",
                    triple(s.rendered_frame_p50, s.rendered_frame_p95, s.rendered_frame_max),
                ),
                ("hitRate", Json::dbl(s.hit_rate)),
                (
                    "kindCounts",
                    Json::Object(s.kind_counts.iter().map(|(k, v)| (k.clone(), Json::int(*v))).collect()),
                ),
                (
                    "slowestDecilePhaseShare",
                    Json::Object(
                        s.slowest_phase_share
                            .iter()
                            .map(|(k, v)| (k.clone(), Json::dbl(*v)))
                            .collect(),
                    ),
                ),
                (
                    "preload",
                    obj(vec![
                        ("workers", int_opt(s.preload_workers)),
                        ("items", Json::int(s.preload_items.len())),
                        ("pausedCount", Json::int(s.preload_paused_count)),
                        ("cancelCount", Json::int(s.preload_cancel_count)),
                    ]),
                ),
                ("dispatcherLongOpCount", Json::int(s.dispatcher_long_ops.len())),
                (
                    "dispatcherLongOpsBeforeStartCount",
                    Json::int(s.dispatcher_long_ops_before_start_count),
                ),
                ("gcTimePercent", Json::dbl_opt(s.gc_time_percent)),
                ("frameTimeP95Ms", Json::dbl_opt(s.frame_time_p95_ms)),
                ("folder", Json::Array(folder)),
                ("startup", Json::Array(startup)),
                ("rules", Json::Array(rules)),
            ])
        })
        .collect();
    let root = obj(vec![
        ("csvFileCount", Json::int(result.csv_file_count)),
        ("groups", Json::Array(groups)),
    ]);
    let mut out = String::new();
    root.print(&mut out, 0);
    out
}

/// `PerfAnalyzeReport.WriteMarkdown(path, result)`: `File.WriteAllText(path, text)` = UTF-8 without BOM.
/// Failures are `Err(PerfError::Io)`.
pub fn write_markdown(path: &Path, result: &AnalysisResult) -> Result<(), PerfError> {
    std::fs::write(path, render_markdown(result))?;
    Ok(())
}

/// `PerfAnalyzeReport.WriteJson(path, result)`: same encoding as [`write_markdown`].
pub fn write_json(path: &Path, result: &AnalysisResult) -> Result<(), PerfError> {
    std::fs::write(path, render_json(result))?;
    Ok(())
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
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut groups: Vec<(&str, Vec<f64>)> = Vec::new();
    for run in &summary.startup_runs {
        for (phase, value) in run {
            let i = *index.entry(phase.as_str()).or_insert_with(|| {
                groups.push((phase.as_str(), Vec::new()));
                groups.len() - 1
            });
            groups[i].1.push(*value);
        }
    }
    let mut phases: Vec<StartupPhase> = groups
        .into_iter()
        .map(|(name, mut values)| {
            values.sort_by(|a, b| cmp_dotnet(*a, *b));
            let n = values.len();
            let mid = n / 2;
            let median = if n % 2 == 1 {
                values[mid]
            } else {
                (values[mid - 1] + values[mid]) / 2.0
            };
            StartupPhase {
                phase: name.to_owned(),
                median,
                min: values[0],
                max: values[n - 1],
                count: n as i32,
            }
        })
        .collect();
    phases.sort_by(|a, b| cmp_dotnet(a.median, b.median));
    phases
}

/// `JsonSerializer.Serialize(string)` with the default `JavaScriptEncoder`, i.e. the way every string, property
/// name and dictionary key is written into `summary.json`: surrounded by `"`; `"`, `&`, `'`, `+`, `<`, `>`, `` ` ``
/// are written as `\u00XX`; `\` -> `\\`; `\b \f \n \r \t` -> short escapes; every other control char below
/// U+0020 and U+007F -> `\u00XX`; every non-ASCII char -> `\uXXXX` of its UTF-16 code unit(s) (a supplementary
/// char is two escapes). Hex digits are UPPER case. `/` is NOT escaped.
pub fn json_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '"' | '&' | '\'' | '+' | '<' | '>' | '`' => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c if c.is_ascii() => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{:04X}", *unit);
                }
            }
        }
    }
    out.push('"');
    out
}
