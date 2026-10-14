//! Port of `PhotoReview.PerfAnalysis/PerfAnalyze.cs` (C# baseline e441d67a, 214 lines) and
//! `NaNAsNullDoubleConverter.cs` (18 lines): the `--perf-analyze <runDir> [--rules <json>]` pipeline. Finds every
//! `perf-*.csv` under a run directory, reads and groups them, evaluates the nine decision rules per group and writes
//! `summary.md` + `summary.json` into the run directory.
//!
//! R0003: C# `Task<AnalysisResult> RunAsync(...)` is the blocking [`run`]; the CLI (W7, pr-bench) calls it on its
//! own thread. `PerfAnalyze.IsBurstScenario` is [`is_burst_scenario`].
//!
//! Unit W1-21. Contract: contracts/W1-21/contract.md. Depends on every other `pr-perf` module.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use pr_core::text::eq_ignore_case;

use crate::csv::{self, PerfError};
use crate::nav::{self, DispatcherLongOpRow, FolderGenSummary, NavRecord, PerfFileAnalysis, PreloadItemRow};
use crate::report;
use crate::rules::{self, RuleResult, RulesConfig};
use crate::stats::{GroupKey, GroupSummary, RunFileMeta, nearest_rank};

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
    let name = scenario_key.split('@').next().unwrap_or("");
    let mut chars = name.chars();
    let (Some(c0), Some(c1)) = (chars.next(), chars.next()) else {
        return false;
    };
    let mut prefix = String::with_capacity(8);
    prefix.push(c0);
    prefix.push(c1);
    if !(eq_ignore_case(&prefix, "S3") || eq_ignore_case(&prefix, "S4")) {
        return false;
    }
    match chars.next() {
        None => true,
        Some(c2) => !c2.is_ascii_digit(),
    }
}

/// `StringComparer.Ordinal`: UTF-16 code unit order.
fn cmp_ordinal(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `perf-*.csv`, case-insensitive (the `Directory.EnumerateFiles` pattern on Windows).
/// The `*` matches zero or more chars (`perf-.csv` matches). The `perf-` head and the `.csv` tail cannot overlap
/// (char 4 would have to be both `-` and `.`), so no separate minimum-length check is needed.
fn is_perf_csv_name(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    let tail_chars = chars.len().checked_sub(4).and_then(|start| chars.get(start..));
    let (Some(head_chars), Some(tail_chars)) = (chars.get(..5), tail_chars) else {
        return false;
    };
    let head: String = head_chars.iter().collect();
    let tail: String = tail_chars.iter().collect();
    eq_ignore_case(&head, "perf-") && eq_ignore_case(&tail, ".csv")
}

/// Collects every `perf-*.csv` file below `dir`, skipping any `warmup` directory (case-insensitive).
///
/// `Directory.EnumerateFiles(dir, "perf-*.csv", SearchOption.AllDirectories)` on .NET 10 / Windows (verified with a
/// real harness): directory junctions (`mklink /J`) and directory symlinks ARE descended into (the entry is listed as
/// `run\jn\perf-a.csv`), and a symlink to a file is collected as a file. The link TARGET's type decides, so this uses
/// `fs::metadata` (follows links), not `DirEntry::file_type`. A broken link is neither (skipped).
///
/// DIVERGENCE: cycle protection. .NET has no cycle detection: a junction pointing at an ancestor is recursed until the
/// path length explodes (the probe was still running at ~75k results / 335k-char paths after 40 s, never terminating
/// in practice). Here a directory whose canonical path is already on the current descent chain is not entered again.
/// Two links to the same non-ancestor directory are both listed, as in .NET.
fn collect_csv(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), PerfError> {
    let mut chain: Vec<PathBuf> = Vec::new();
    collect_csv_inner(dir, out, &mut chain)
}

fn collect_csv_inner(dir: &Path, out: &mut Vec<PathBuf>, chain: &mut Vec<PathBuf>) -> Result<(), PerfError> {
    // A canonicalisation failure (racing delete, odd path) cannot be a proven cycle: fall back to the plain path.
    let canon = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if chain.contains(&canon) {
        return Ok(());
    }
    chain.push(canon);
    let result = collect_csv_entries(dir, out, chain);
    chain.pop();
    result
}

fn collect_csv_entries(dir: &Path, out: &mut Vec<PathBuf>, chain: &mut Vec<PathBuf>) -> Result<(), PerfError> {
    for entry in fs::read_dir(dir).map_err(PerfError::Io)? {
        let entry = entry.map_err(PerfError::Io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if !eq_ignore_case(&name, "warmup") {
                collect_csv_inner(&path, out, chain)?;
            }
        } else if meta.is_file() && is_perf_csv_name(&name) {
            out.push(path);
        }
    }
    Ok(())
}
/// Mean of the present values, summed left to right (`None` when none).
fn average_or_none(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut sum = 0.0_f64;
    let mut count = 0_usize;
    for v in values.flatten() {
        sum += v;
        count += 1;
    }
    if count > 0 { Some(sum / count as f64) } else { None }
}

/// `DispatcherLongOps.Count(d => d.Ms > threshold)`: an op of exactly `threshold` ms does not count.
fn count_long_ops(ops: &[DispatcherLongOpRow], threshold: f64) -> i32 {
    ops.iter().filter(|d| d.ms > threshold).count() as i32
}

/// The decode navs of R-DEC / R-CONT: kind contains `SourceMiss` or `InflightJoin` (ordinal, case-sensitive).
fn is_decode_kind(kind: &str) -> bool {
    kind.contains("SourceMiss") || kind.contains("InflightJoin")
}

/// `AverageDecodeMs(g)`.
fn average_decode_ms(g: &GroupSummary) -> Option<f64> {
    average_or_none(
        g.navs
            .iter()
            .filter(|n| !n.incomplete && is_decode_kind(&n.kind) && n.t_decode_ms.is_some())
            .map(|n| n.t_decode_ms),
    )
}

fn sorted_asc(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    values
}

/// `EvaluateAllRules`: the nine rules for one group; R-CONT looks at sibling groups.
fn evaluate_all_rules(cfg: &RulesConfig, s: &GroupSummary, all_groups: &[GroupSummary]) -> Vec<RuleResult> {
    let complete: Vec<&NavRecord> = s.navs.iter().filter(|n| !n.incomplete).collect();
    let pick = |f: &dyn Fn(&NavRecord) -> bool| -> Vec<NavRecord> {
        complete.iter().filter(|n| f(n)).map(|n| (*n).clone()).collect()
    };
    let source_miss = pick(&|n| n.kind.contains("SourceMiss"));
    let decode_navs = pick(&|n| is_decode_kind(&n.kind));
    let ram_hit = pick(&|n| n.kind.contains("RamHit"));
    let disk_hit = pick(&|n| n.kind.contains("DiskCacheHit"));

    let ram_hit_finals = sorted_asc(ram_hit.iter().filter_map(|n| n.final_visual_ms).collect());
    let ram_hit_final_p95 = if ram_hit_finals.is_empty() {
        f64::NAN
    } else {
        nearest_rank(&ram_hit_finals, 95.0)
    };
    let non_ram_hit_share_pct = if complete.is_empty() {
        f64::NAN
    } else {
        100.0 * (complete.len() - ram_hit.len()) as f64 / complete.len() as f64
    };
    let is_burst = is_burst_scenario(&s.key.scenario);

    let inputs = sorted_asc(complete.iter().filter_map(|n| n.t_input_ms).collect());
    let t_input_p95 = if inputs.is_empty() {
        None
    } else {
        Some(nearest_rank(&inputs, 95.0))
    };

    let dispatcher_count = count_long_ops(&s.dispatcher_long_ops, cfg.get("R-THREAD", "dispatcherLongOpMs", 16.0));

    // Same scenario, mode and cond, known worker count (includes `s` itself). First wins on ties.
    let mut low: Option<&GroupSummary> = None;
    let mut high: Option<&GroupSummary> = None;
    for g in all_groups.iter().filter(|g| {
        g.key.scenario == s.key.scenario
            && g.key.mode == s.key.mode
            && g.key.cond == s.key.cond
            && g.preload_workers.is_some()
    }) {
        if low.is_none_or(|l| g.preload_workers < l.preload_workers) {
            low = Some(g);
        }
        if high.is_none_or(|h| g.preload_workers > h.preload_workers) {
            high = Some(g);
        }
    }
    let decode_low = low.and_then(average_decode_ms);
    let decode_high = high.and_then(average_decode_ms);

    vec![
        rules::evaluate_r_io(cfg, &source_miss),
        rules::evaluate_r_dec(cfg, &decode_navs),
        rules::evaluate_r_ui(cfg, &ram_hit, ram_hit_final_p95, s.frame_time_p95_ms),
        rules::evaluate_r_pre(cfg, ram_hit_final_p95, non_ram_hit_share_pct, is_burst),
        rules::evaluate_r_cont(
            cfg,
            decode_low,
            decode_high,
            low.and_then(|g| g.preload_workers),
            high.and_then(|g| g.preload_workers),
        ),
        rules::evaluate_r_thread(cfg, dispatcher_count, t_input_p95),
        rules::evaluate_r_gc(cfg, s.gc_time_percent),
        rules::evaluate_r_disk(cfg, &disk_hit, &source_miss),
        rules::evaluate_r_folder(cfg, &s.folder_gens),
    ]
}

/// Per-group aggregates (the C# `group*` dictionaries), kept in an `IndexMap` so enumeration is first-seen order.
#[derive(Default)]
struct GroupAcc {
    navs: Vec<NavRecord>,
    preload: Vec<PreloadItemRow>,
    paused: i32,
    cancel: i32,
    dispatcher: Vec<DispatcherLongOpRow>,
    before_start: i32,
    folder: Vec<FolderGenSummary>,
    startup: Vec<IndexMap<String, f64>>,
    metas: Vec<RunFileMeta>,
    dropped: i64,
}

impl GroupAcc {
    /// Appends one file's navigation aggregates; a file without startup phases adds no startup run.
    fn add_analysis(&mut self, analysis: PerfFileAnalysis) {
        self.navs.extend(analysis.navs);
        self.preload.extend(analysis.preload_items);
        self.paused = self.paused.wrapping_add(analysis.preload_paused_count);
        self.cancel = self.cancel.wrapping_add(analysis.preload_cancel_count);
        self.dispatcher.extend(analysis.dispatcher_long_ops);
        self.before_start = self
            .before_start
            .wrapping_add(analysis.dispatcher_long_ops_before_start_count);
        self.folder.extend(analysis.folder_gens);
        if !analysis.startup.is_empty() {
            self.startup.push(analysis.startup);
        }
    }
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
    if !run_dir.is_dir() {
        return Err(PerfError::DirectoryNotFound(run_dir.display().to_string()));
    }

    let mut csv_files = Vec::new();
    collect_csv(run_dir, &mut csv_files)?;
    csv_files.sort_by(|a, b| cmp_ordinal(&a.to_string_lossy(), &b.to_string_lossy()));
    if csv_files.is_empty() {
        return Err(PerfError::InvalidOperation(format!(
            "No perf-*.csv files found in {}",
            run_dir.display()
        )));
    }

    let cfg = match rules_path {
        Some(p) if !p.as_os_str().is_empty() => RulesConfig::load(p)?,
        _ => RulesConfig::default(),
    };

    let mut groups: IndexMap<GroupKey, GroupAcc> = IndexMap::new();
    for path in &csv_files {
        let file = match csv::read(path) {
            Ok(f) => f,
            Err(PerfError::Io(e)) => {
                return Err(PerfError::InvalidData(format!(
                    "Could not analyze '{}': {e}",
                    path.display()
                )));
            }
            Err(e) => return Err(e),
        };
        let analysis = nav::build(&file);
        let meta = RunFileMeta::load(&file);

        // Majority vote of the navs' modes: the first mode with the maximum count wins.
        let mut votes: IndexMap<&str, i32> = IndexMap::new();
        for n in &analysis.navs {
            let m = if n.mode.is_empty() { "unknown" } else { n.mode.as_str() };
            *votes.entry(m).or_insert(0) += 1;
        }
        let mode = if analysis.navs.is_empty() {
            if meta.mode.is_empty() {
                "unknown".to_owned()
            } else {
                meta.mode.clone()
            }
        } else {
            let mut best: Option<(&str, i32)> = None;
            for (m, c) in &votes {
                if best.is_none_or(|(_, bc)| *c > bc) {
                    best = Some((m, *c));
                }
            }
            best.map_or_else(|| "unknown".to_owned(), |(m, _)| m.to_owned())
        };

        let key = GroupKey::new(&meta.scenario, &mode, &meta.cond, meta.preload_workers);
        let acc = groups.entry(key).or_default();
        acc.add_analysis(analysis);
        acc.metas.push(meta);
        acc.dropped = acc.dropped.wrapping_add(file.dropped_rows);
    }

    let mut summaries: Vec<GroupSummary> = Vec::with_capacity(groups.len());
    for (key, acc) in groups {
        let mut s = GroupSummary::build(key.clone(), &acc.navs);
        s.preload_workers = key.preload_workers;
        s.dropped_rows = acc.dropped;
        s.gc_time_percent = average_or_none(acc.metas.iter().map(|m| m.gc_time_percent));
        s.frame_time_p95_ms = average_or_none(acc.metas.iter().map(|m| m.frame_time_p95_ms));
        s.preload_items.extend(acc.preload);
        s.preload_paused_count = acc.paused;
        s.preload_cancel_count = acc.cancel;
        s.dispatcher_long_ops.extend(acc.dispatcher);
        s.dispatcher_long_ops_before_start_count = acc.before_start;
        s.folder_gens.extend(acc.folder);
        s.startup_runs.extend(acc.startup);
        summaries.push(s);
    }

    let mut results: Vec<GroupResult> = summaries
        .iter()
        .map(|s| GroupResult {
            summary: s.clone(),
            rules: evaluate_all_rules(&cfg, s, &summaries),
        })
        .collect();
    // Stable ordinal sort; numeric worker order (2 before 10), unknown workers as -1.
    results.sort_by(|a, b| {
        let (ka, kb) = (&a.summary.key, &b.summary.key);
        cmp_ordinal(&ka.scenario, &kb.scenario)
            .then_with(|| cmp_ordinal(&ka.mode, &kb.mode))
            .then_with(|| cmp_ordinal(&ka.cond, &kb.cond))
            .then_with(|| ka.preload_workers.unwrap_or(-1).cmp(&kb.preload_workers.unwrap_or(-1)))
    });

    let result = AnalysisResult {
        groups: results,
        summary_md_path: run_dir.join("summary.md"),
        summary_json_path: run_dir.join("summary.json"),
        csv_file_count: csv_files.len() as i32,
    };
    report::write_markdown(&result.summary_md_path, &result)?;
    report::write_json(&result.summary_json_path, &result)?;
    Ok(result)
}

/// `NaNAsNullDoubleConverter.Write`: the JSON text of a `double` in `summary.json`: the .NET number text
/// ([`crate::rules::fmt_double`]) when finite, else `null`.
pub fn nan_as_null_double_json(value: f64) -> String {
    if value.is_finite() {
        rules::fmt_double(value)
    } else {
        "null".to_owned()
    }
}

/// `NaNAsNullDoubleConverter.Read`: JSON `null` -> `Some(NaN)`, a number -> `Some(value)`, any other token ->
/// `None` (C# `GetDouble` throws `InvalidOperationException`). Unused by the application; ported for completeness.
pub fn nan_as_null_double_read(token: &serde_json::Value) -> Option<f64> {
    match token {
        serde_json::Value::Null => Some(f64::NAN),
        serde_json::Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(ms: f64) -> DispatcherLongOpRow {
        DispatcherLongOpRow {
            ms,
            priority: "Normal".to_owned(),
            name: "op".to_owned(),
        }
    }

    // C#: PerfAnalyze.EvaluateAllRules (`d.Ms > dispatcherThreshold`, strict)
    #[test]
    fn dispatcher_long_op_at_exactly_the_threshold_does_not_count() {
        assert_eq!(count_long_ops(&[op(16.0)], 16.0), 0);
        assert_eq!(count_long_ops(&[op(16.5)], 16.0), 1);
        assert_eq!(count_long_ops(&[op(15.9), op(16.0), op(16.01), op(700.0)], 16.0), 2);
        assert_eq!(count_long_ops(&[op(20.0)], 25.0), 0);
        assert_eq!(count_long_ops(&[], 16.0), 0);
    }

    // C#: PerfAnalyze.EvaluateAllRules / AverageDecodeMs (decode navs = kind contains SourceMiss or InflightJoin)
    #[test]
    fn decode_kinds_are_source_miss_and_inflight_join() {
        assert!(is_decode_kind("SourceMiss"));
        assert!(is_decode_kind("InflightJoin"));
        assert!(is_decode_kind("Thumbnail+InflightJoin"));
        assert!(is_decode_kind("Thumbnail+SourceMiss"));
        assert!(!is_decode_kind("RamHit"));
        assert!(!is_decode_kind("DiskCacheHit"));
        assert!(!is_decode_kind("Unknown"));
        assert!(!is_decode_kind("inflightjoin"));
        assert!(!is_decode_kind("sourcemiss"));
    }

    // C#: PerfAnalyze.RunAsync (`if (analysis.Startup.Count > 0) Add(groupStartup, key).Add(analysis.Startup)`)
    #[test]
    fn a_file_without_startup_phases_adds_no_startup_run() {
        let mut acc = GroupAcc::default();
        acc.add_analysis(PerfFileAnalysis::default());
        assert!(acc.startup.is_empty());

        let mut with_startup = PerfFileAnalysis::default();
        with_startup.startup.insert("firstPresented".to_owned(), 12.5);
        acc.add_analysis(with_startup);
        acc.add_analysis(PerfFileAnalysis::default());
        assert_eq!(acc.startup.len(), 1);
        assert_eq!(acc.startup[0].get("firstPresented"), Some(&12.5));
    }

    // C#: Directory.EnumerateFiles(runDir, "perf-*.csv", ...) name match
    #[test]
    fn perf_csv_name_match() {
        for yes in [
            "perf-.csv",
            "PERF-C.CSV",
            "perf-1.csv",
            "Perf-sample.Csv",
            "perf-\u{e9}.csv",
        ] {
            assert!(is_perf_csv_name(yes), "{yes:?}");
        }
        for no in [
            "perf.csv",
            "perf-csv",
            "perf-",
            ".csv",
            "",
            "perf",
            "xperf-1.csv",
            "perf-1.csv.bak",
            "perf-1.txt",
            "perf_1.csv",
            "perf-.cs",
        ] {
            assert!(!is_perf_csv_name(no), "{no:?}");
        }
    }
}
