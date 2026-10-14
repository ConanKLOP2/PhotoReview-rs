//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeStats.cs` (C# baseline e441d67a, 249 lines): per-run metadata
//! (`RunFileMeta`), nearest-rank percentiles and slowest-decile phase shares (`PerfStats`), and the per-group
//! summary (`GroupKey`, `GroupSummary`).
//!
//! In C# `PerfStats` is a static class; its members are the free functions [`nearest_rank`],
//! [`slowest_decile_phase_share`] and [`phase_getters`].
//!
//! Unit W1-17. Contract: contracts/W1-17/contract.md. Depends on `crate::csv` and `crate::nav`.
#![allow(unused_variables, dead_code, reason = "stub: todo!() bodies, remove when filling")]

use std::fmt;

use indexmap::IndexMap;

use crate::csv::PerfCsvFile;
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
        todo!()
    }
}

/// `PerfStats.NearestRank`: the P-th percentile of an ascending slice is the element at 1-based rank
/// `ceil(P/100 * N)`. `NaN` for an empty slice or a NaN percentile. The percentile is clamped to `[0, 100]`.
/// The rank is computed as `ceil(clamp(p) * n / 100.0)` (multiply first, divide last) and clamped to `1..=n`.
pub fn nearest_rank(sorted_ascending: &[f64], percentile: f64) -> f64 {
    todo!()
}

/// `PerfStats.SlowestDecilePhaseShare`: for the slowest `ceil(0.10 * N)` (at least 1) navigations (those with
/// `final_visual_ms > 0`, ordered ascending by it with a STABLE sort), the mean of `phase / final_visual` per phase,
/// in [`phase_getters`] order. A phase that no navigation of the slice measured is omitted.
/// Callers pass complete navigations only.
pub fn slowest_decile_phase_share(complete_navs: &[NavRecord]) -> IndexMap<String, f64> {
    todo!()
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
        todo!()
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
        todo!()
    }

    /// `GroupSummary.Build(key, navs)`: copies `navs`, fills `count`, `incomplete`, the first/final/rendered-frame
    /// percentiles and maxima of the complete navigations, `kind_counts`, `slowest_phase_share` and `hit_rate`.
    /// Everything else stays at [`GroupSummary::new`] values (the analyzer fills it later).
    pub fn build(key: GroupKey, navs: &[NavRecord]) -> GroupSummary {
        todo!()
    }
}
