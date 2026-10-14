//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeStats.cs` (C# baseline e441d67a, 249 lines): per-run metadata
//! (`RunFileMeta`), nearest-rank percentiles and slowest-decile phase shares (`PerfStats`), and the per-group
//! summary (`GroupKey`, `GroupSummary`).
//!
//! In C# `PerfStats` is a static class; its members are the free functions [`nearest_rank`],
//! [`slowest_decile_phase_share`] and [`phase_getters`].
//!
//! Unit W1-17. Contract: contracts/W1-17/contract.md. Depends on `crate::csv` and `crate::nav`.

use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use pr_core::text::eq_ignore_case;
use serde_json::{Map, Value};

use crate::csv::{PerfCsvFile, decode_bytes, parse_integer_i32};
use crate::nav::{DispatcherLongOpRow, FolderGenSummary, NavRecord, PreloadItemRow};

/// `(scenario, mode, cond)` + sampling knobs of one perf run, read from the files that sit next to `perf-*.csv`
/// (C# `RunFileMeta`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunFileMeta {
    /// `"unknown"` when nothing names it.
    pub scenario: String,
    /// `""` when nothing names it (the analyzer later substitutes the majority mode of the navigations).
    pub mode: String,
    /// `"unknown"` or the session's condition, or `cold-diskcache | cold-app | cold-os | warm` recovered from the
    /// name of the directory two levels above the CSV.
    pub cond: String,
    pub preload_workers: Option<i32>,
    pub gc_time_percent: Option<f64>,
    pub frame_time_p95_ms: Option<f64>,
}

impl RunFileMeta {
    /// `RunFileMeta.Load(file)`. Reads `session.json` and `process.json` from the directory of `file.path` (blocking
    /// file reads). Never fails: a missing, unreadable, malformed or wrongly shaped file leaves the defaults (see the
    /// contract for exactly which later fields are skipped when a value has the wrong JSON type).
    pub fn load(file: &PerfCsvFile) -> RunFileMeta {
        // C# `Path.GetDirectoryName(file.Path) ?? ""`: a bare file name has the parent "".
        let dir: PathBuf = file.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut meta = RunFileMeta {
            scenario: "unknown".to_owned(),
            mode: String::new(),
            cond: "unknown".to_owned(),
            preload_workers: None,
            gc_time_percent: None,
            frame_time_p95_ms: None,
        };

        // An Err(Abort) stops that file's reading; the values assigned before it stay in `meta`.
        if let Some(root) = read_json_object(&dir.join("session.json")) {
            let _ = read_session(&root, &mut meta);
        }

        // cond from the name of the cell directory (the parent of `dir`), run-matrix.ps1 layout.
        if meta.cond == "unknown" {
            let cell = dir
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            for known in ["cold-diskcache", "cold-app", "cold-os", "warm"] {
                if ends_with_ignore_case(&cell, known) {
                    meta.cond = known.to_owned();
                    break;
                }
            }
        }

        // Header fallback: only when the session did not give a worker count.
        if meta.preload_workers.is_none() {
            if let Some(v) = file
                .diag_flags
                .get("PHOTOREVIEW_DIAG_PRELOAD_WORKERS")
                .and_then(parse_integer_i32)
            {
                meta.preload_workers = Some(v);
            }
        }

        if let Some(root) = read_json_object(&dir.join("process.json")) {
            let _ = read_process(&root, &mut meta);
        }

        meta
    }
}

/// A JSON property of the wrong type where C# `TryGetInt32` / `TryGetDouble` throw `InvalidOperationException`.
/// It ends the reading of that file (the `catch` in C# `RunFileMeta.Load`).
#[derive(Debug)]
struct Abort;

/// Reads a file that must hold a JSON object. Missing, unreadable, malformed, or non-object content gives `None`
/// (C# `File.Exists` plus the `catch` around `JsonDocument.Parse`).
fn read_json_object(path: &Path) -> Option<Map<String, Value>> {
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let text = decode_bytes(&bytes);
    // C# `JsonDocument.Parse` (MaxDepth 64) throws on deeper nesting, so such a file is ignored.
    if exceeds_json_depth(&text, 64) {
        return None;
    }
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// True when `{`/`[` nesting (outside strings) goes deeper than `max` levels.
fn exceeds_json_depth(text: &str, max: usize) -> bool {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for b in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > max {
                    return true;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// C# `string.EndsWith("-" + known, OrdinalIgnoreCase)`.
fn ends_with_ignore_case(text: &str, known: &str) -> bool {
    let suffix = format!("-{known}");
    let count = suffix.chars().count();
    let tail: String = {
        let mut chars: Vec<char> = text.chars().rev().take(count).collect();
        chars.reverse();
        chars.into_iter().collect()
    };
    eq_ignore_case(&tail, &suffix)
}

/// The `session.json` steps of `RunFileMeta.Load`, in C# order.
fn read_session(root: &Map<String, Value>, meta: &mut RunFileMeta) -> Result<(), Abort> {
    let str_of = |name: &str| root.get(name).and_then(Value::as_str);

    // scenario = scenario ?? alias ?? scenario (an empty string is a value).
    if let Some(s) = str_of("scenario").or_else(|| str_of("alias")) {
        meta.scenario = s.to_owned();
    }
    if let Some(a) = str_of("folderAlias").filter(|a| !a.is_empty()) {
        meta.scenario = format!("{}@{a}", meta.scenario);
    }
    if let Some(m) = str_of("mode") {
        meta.mode = m.to_owned();
    }
    if let Some(c) = str_of("cond").or_else(|| str_of("condition")) {
        meta.cond = c.to_owned();
    }

    match root.get("preloadWorkers") {
        Some(Value::Number(n)) => match n.as_i64().and_then(|v| i32::try_from(v).ok()) {
            Some(v) => meta.preload_workers = Some(v),
            // A number that is not an i32 integer (4.0, 1E2, 2147483648) falls through to the env lookup.
            None => env_preload_workers(root, meta),
        },
        // A string, null, bool, array or object: C# TryGetInt32 throws, the session reading ends here.
        Some(_) => return Err(Abort),
        None => env_preload_workers(root, meta),
    }
    Ok(())
}

/// `diag`, then `env`: the first object holding `PHOTOREVIEW_DIAG_PRELOAD_WORKERS` whose value is an i32 number or a
/// string that parses as an i32 decides. Other value types go on to the next object.
fn env_preload_workers(root: &Map<String, Value>, meta: &mut RunFileMeta) {
    for name in ["diag", "env"] {
        let Some(obj) = root.get(name).and_then(Value::as_object) else {
            continue;
        };
        let Some(value) = obj.get("PHOTOREVIEW_DIAG_PRELOAD_WORKERS") else {
            continue;
        };
        let found = match value {
            Value::Number(n) => n.as_i64().and_then(|v| i32::try_from(v).ok()),
            Value::String(s) => parse_integer_i32(s),
            _ => None,
        };
        if let Some(v) = found {
            meta.preload_workers = Some(v);
            break;
        }
    }
}

/// The `process.json` steps of `RunFileMeta.Load`, in C# order.
fn read_process(root: &Map<String, Value>, meta: &mut RunFileMeta) -> Result<(), Abort> {
    if let Some(g) = root.get("gcTimePercent") {
        meta.gc_time_percent = Some(g.as_f64().ok_or(Abort)?);
    } else if let Some(gp) = root.get("gcPauseDeltaMs") {
        let gp = gp.as_f64().ok_or(Abort)?;
        if let Some(el) = root.get("elapsedMs") {
            let el = el.as_f64().ok_or(Abort)?;
            if el > 0.0 {
                // Divide first, then multiply (C# `gpd / eld * 100.0`).
                meta.gc_time_percent = Some(gp / el * 100.0);
            }
        }
    }
    if let Some(f) = root.get("frameTimeP95Ms") {
        meta.frame_time_p95_ms = Some(f.as_f64().ok_or(Abort)?);
    }
    Ok(())
}

/// `double.CompareTo` as a sort key: NaN sorts first and equal values (including -0.0 and 0.0) keep their order.
fn double_compare(a: &f64, b: &f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
    }
}

/// `(P50, P95, max)` of the values, or `None` when there are none (`OrderBy` + `Count > 0` in C#).
fn percentile_triple(values: impl Iterator<Item = f64>) -> Option<(f64, f64, f64)> {
    let mut sorted: Vec<f64> = values.collect();
    sorted.sort_by(double_compare);
    let max = sorted.last().copied()?;
    Some((nearest_rank(&sorted, 50.0), nearest_rank(&sorted, 95.0), max))
}

/// `PerfStats.NearestRank`: the P-th percentile of an ascending slice is the element at 1-based rank
/// `ceil(P/100 * N)`. `NaN` for an empty slice or a NaN percentile. The percentile is clamped to `[0, 100]`.
/// The rank is computed as `ceil(clamp(p) * n / 100.0)` (multiply first, divide last) and clamped to `1..=n`.
pub fn nearest_rank(sorted_ascending: &[f64], percentile: f64) -> f64 {
    if sorted_ascending.is_empty() || percentile.is_nan() {
        return f64::NAN;
    }
    let n = sorted_ascending.len();
    // Multiply first, divide last: `p / 100.0 * n` rounds 0.07 * 100 up to 7.000000000000001 (rank 8, not 7).
    let rank = (percentile.clamp(0.0, 100.0) * n as f64 / 100.0).ceil() as i64;
    let rank = rank.clamp(1, n as i64);
    sorted_ascending[(rank - 1) as usize]
}

/// `PerfStats.SlowestDecilePhaseShare`: for the slowest `ceil(0.10 * N)` (at least 1) navigations (those with
/// `final_visual_ms > 0`, ordered ascending by it with a STABLE sort), the mean of `phase / final_visual` per phase,
/// in [`phase_getters`] order. A phase that no navigation of the slice measured is omitted.
/// Callers pass complete navigations only.
pub fn slowest_decile_phase_share(complete_navs: &[NavRecord]) -> IndexMap<String, f64> {
    let mut eligible: Vec<(&NavRecord, f64)> = complete_navs
        .iter()
        .filter_map(|n| match n.final_visual_ms {
            Some(v) if v > 0.0 => Some((n, v)),
            _ => None,
        })
        .collect();
    eligible.sort_by(|a, b| double_compare(&a.1, &b.1));

    let mut result = IndexMap::new();
    if eligible.is_empty() {
        return result;
    }
    // `Math.Max(1, (int)Math.Ceiling(count * 0.10))`: keep the double multiplication.
    let slice_count = ((eligible.len() as f64 * 0.10).ceil() as usize).max(1);
    let slow = &eligible[eligible.len() - slice_count..];

    for (name, get) in phase_getters() {
        if !slow.iter().any(|(nav, _)| get(nav).is_some()) {
            continue;
        }
        // LINQ `Average`: left-to-right sum from 0.0, then divide by the count.
        let mut sum = 0.0_f64;
        for (nav, final_ms) in slow {
            sum += get(nav).unwrap_or(0.0) / *final_ms;
        }
        result.insert((*name).to_owned(), sum / slow.len() as f64);
    }
    result
}

/// One phase accessor of [`phase_getters`].
pub type PhaseGetter = fn(&NavRecord) -> Option<f64>;

/// `PerfStats.PhaseGetters()`: the twelve phases that are shares of key-to-present time, in this order:
/// `t_input t_pre t_stat t_thumb t_join t_disk t_read t_decode t_verify t_assign t_render t_renderFrame`.
/// (`t_post` is deliberately not part of it.)
pub fn phase_getters() -> &'static [(&'static str, PhaseGetter)] {
    static PHASES: [(&str, PhaseGetter); 12] = [
        ("t_input", |n| n.t_input_ms),
        ("t_pre", |n| n.t_pre_ms),
        ("t_stat", |n| n.t_stat_ms),
        ("t_thumb", |n| n.t_thumb_ms),
        ("t_join", |n| n.t_join_ms),
        ("t_disk", |n| n.t_disk_ms),
        ("t_read", |n| n.t_read_ms),
        ("t_decode", |n| n.t_decode_ms),
        ("t_verify", |n| n.t_verify_ms),
        ("t_assign", |n| n.t_assign_ms),
        ("t_render", |n| n.t_render_ms),
        ("t_renderFrame", |n| n.t_render_frame_ms),
    ];
    &PHASES
}

/// `(scenario, mode, cond, preload worker count)`: the key navigations are bucketed by (C# `GroupKey` record struct).
/// Equality and hashing are plain ordinal (`==` on the strings) and include `preload_workers`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupKey {
    pub scenario: String,
    pub mode: String,
    pub cond: String,
    pub preload_workers: Option<i32>,
}

impl GroupKey {
    pub fn new(scenario: &str, mode: &str, cond: &str, preload_workers: Option<i32>) -> Self {
        Self {
            scenario: scenario.to_owned(),
            mode: mode.to_owned(),
            cond: cond.to_owned(),
            preload_workers,
        }
    }
}

impl fmt::Display for GroupKey {
    /// `ToString()`: `{scenario}/{mode}/{cond}/workers={n}` with `?` when `preload_workers` is `None`
    /// (the number in plain invariant decimal, `-1` for -1).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.preload_workers {
            Some(w) => write!(f, "{}/{}/{}/workers={w}", self.scenario, self.mode, self.cond),
            None => write!(f, "{}/{}/{}/workers=?", self.scenario, self.mode, self.cond),
        }
    }
}

/// Aggregates of one group (C# `GroupSummary`). Percentile/rate fields are `NaN` when there is nothing to compute
/// them from (C# initializer `double.NaN`). Counts are `i32` like C# `int`.
#[derive(Debug, Clone)]
pub struct GroupSummary {
    pub key: GroupKey,
    pub count: i32,
    pub incomplete: i32,
    pub first_p50: f64,
    pub first_p95: f64,
    pub first_max: f64,
    pub final_p50: f64,
    pub final_p95: f64,
    pub final_max: f64,
    pub rendered_frame_p50: f64,
    pub rendered_frame_p95: f64,
    pub rendered_frame_max: f64,
    /// Count of complete navigations by `kind`, first-seen order.
    pub kind_counts: IndexMap<String, i32>,
    /// `t_x` -> mean share, in [`phase_getters`] order.
    pub slowest_phase_share: IndexMap<String, f64>,
    pub hit_rate: f64,
    pub preload_workers: Option<i32>,
    pub gc_time_percent: Option<f64>,
    pub frame_time_p95_ms: Option<f64>,
    pub preload_items: Vec<PreloadItemRow>,
    pub preload_paused_count: i32,
    pub preload_cancel_count: i32,
    pub dispatcher_long_ops: Vec<DispatcherLongOpRow>,
    pub dispatcher_long_ops_before_start_count: i32,
    /// Sum of the `# dropped=N` trailers of the group's files.
    pub dropped_rows: i64,
    pub folder_gens: Vec<FolderGenSummary>,
    /// One entry per `perf-*.csv` that had Startup rows (phase -> ms since process start).
    pub startup_runs: Vec<IndexMap<String, f64>>,
    pub navs: Vec<NavRecord>,
}

impl GroupSummary {
    /// The C# object initializer state for `new GroupSummary { Key = key }`: every `f64` is NaN, every
    /// collection empty, every count 0, every option `None`.
    pub fn new(key: GroupKey) -> Self {
        Self {
            key,
            count: 0,
            incomplete: 0,
            first_p50: f64::NAN,
            first_p95: f64::NAN,
            first_max: f64::NAN,
            final_p50: f64::NAN,
            final_p95: f64::NAN,
            final_max: f64::NAN,
            rendered_frame_p50: f64::NAN,
            rendered_frame_p95: f64::NAN,
            rendered_frame_max: f64::NAN,
            kind_counts: IndexMap::new(),
            slowest_phase_share: IndexMap::new(),
            hit_rate: f64::NAN,
            preload_workers: None,
            gc_time_percent: None,
            frame_time_p95_ms: None,
            preload_items: Vec::new(),
            preload_paused_count: 0,
            preload_cancel_count: 0,
            dispatcher_long_ops: Vec::new(),
            dispatcher_long_ops_before_start_count: 0,
            dropped_rows: 0,
            folder_gens: Vec::new(),
            startup_runs: Vec::new(),
            navs: Vec::new(),
        }
    }

    /// `LowSampleWarning`: `count - incomplete < 20` (the "N<20" marker counts complete navigations).
    pub fn low_sample_warning(&self) -> bool {
        // C# int arithmetic is unchecked: wrapping mirrors it.
        self.count.wrapping_sub(self.incomplete) < 20
    }

    /// `GroupSummary.Build(key, navs)`: copies `navs`, fills `count`, `incomplete`, the first/final/rendered-frame
    /// percentiles and maxima of the complete navigations, `kind_counts`, `slowest_phase_share` and `hit_rate`.
    /// Everything else stays at [`GroupSummary::new`] values (the analyzer fills it later).
    pub fn build(key: GroupKey, navs: &[NavRecord]) -> GroupSummary {
        let mut summary = GroupSummary::new(key);
        summary.navs = navs.to_vec();
        summary.count = navs.len() as i32;
        summary.incomplete = navs.iter().filter(|n| n.incomplete).count() as i32;

        let complete: Vec<NavRecord> = navs.iter().filter(|n| !n.incomplete).cloned().collect();

        if let Some((p50, p95, max)) = percentile_triple(complete.iter().filter_map(|n| n.first_visual_ms)) {
            summary.first_p50 = p50;
            summary.first_p95 = p95;
            summary.first_max = max;
        }
        if let Some((p50, p95, max)) = percentile_triple(complete.iter().filter_map(|n| n.final_visual_ms)) {
            summary.final_p50 = p50;
            summary.final_p95 = p95;
            summary.final_max = max;
        }
        if let Some((p50, p95, max)) = percentile_triple(complete.iter().filter_map(|n| n.t_render_frame_ms)) {
            summary.rendered_frame_p50 = p50;
            summary.rendered_frame_p95 = p95;
            summary.rendered_frame_max = max;
        }

        for nav in &complete {
            *summary.kind_counts.entry(nav.kind.clone()).or_insert(0) += 1;
        }

        summary.slowest_phase_share = slowest_decile_phase_share(&complete);

        if !complete.is_empty() {
            let ram_hits = complete.iter().filter(|n| n.kind.contains("RamHit")).count();
            summary.hit_rate = ram_hits as f64 / complete.len() as f64;
        }
        summary
    }
}
