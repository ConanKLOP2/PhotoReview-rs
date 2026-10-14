//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeNav.cs` (C# baseline e441d67a, 368 lines): reassembles the rows of
//! one `perf-*.csv` into navigation records, preload/dispatcher/folder/startup aggregates.
//!
//! In C# the builder is the static class `PerfAnalyzeNavBuilder`; here its methods are the free functions
//! [`build`] and [`classify_kind`]. Plan references in the C# comments ("plan muc 3", "D11 spec item 2") point at
//! removed docs; the C# code is the spec.
//!
//! Unit W1-19. Contract: contracts/W1-19/contract.md. Depends on `crate::csv` only.
#![allow(unused_variables, dead_code, reason = "stub: todo!() bodies, remove when filling")]

use indexmap::IndexMap;

use crate::csv::{PerfCsvFile, PerfRow};

/// One navigation (one `ShowImageAsync` token) reassembled from its perf events (C# `NavRecord`).
/// All `*_ms` values are milliseconds; `None` is C# `null`.
#[derive(Debug, Clone, PartialEq)]
pub struct NavRecord {
    /// `Nav`: the navigation id (always > 0 for records produced by [`build`]).
    pub nav: i64,
    /// `Mode`: the `text` column of the nav's first `ShowStart` row; `""` when it has no ShowStart.
    pub mode: String,
    pub t_input_ms: Option<f64>,
    pub t_pre_ms: Option<f64>,
    pub t_stat_ms: Option<f64>,
    pub lookup_result: String,
    pub t_thumb_ms: Option<f64>,
    pub has_thumbnail: bool,
    pub t_join_ms: Option<f64>,
    pub t_disk_ms: Option<f64>,
    pub t_read_ms: Option<f64>,
    pub t_decode_ms: Option<f64>,
    pub t_verify_ms: Option<f64>,
    pub t_assign_ms: Option<f64>,
    pub t_render_ms: Option<f64>,
    /// `TRenderFrameMs`: from the `RenderedFrame` event (the frame containing the new image finished rendering).
    pub t_render_frame_ms: Option<f64>,
    /// `PostMs`: `PostEnd` rows by `text`; C# `Dictionary` insertion order, a repeated key keeps its position.
    pub post_ms: IndexMap<String, f64>,
    pub first_visual_ms: Option<f64>,
    pub final_visual_ms: Option<f64>,
    /// True until a `Presented` row with text `final` or `compare` is matched. NOTE the C# default is `true`.
    pub incomplete: bool,
    /// `RamHit | InflightJoin | DiskCacheHit | SourceMiss | Unknown`, prefixed `Thumbnail+` when a thumbnail was shown.
    pub kind: String,
}

impl Default for NavRecord {
    /// The C# property initializers: `Incomplete = true`, `Kind = "Unknown"`, everything else empty/absent.
    fn default() -> Self {
        Self {
            nav: 0,
            mode: String::new(),
            t_input_ms: None,
            t_pre_ms: None,
            t_stat_ms: None,
            lookup_result: String::new(),
            t_thumb_ms: None,
            has_thumbnail: false,
            t_join_ms: None,
            t_disk_ms: None,
            t_read_ms: None,
            t_decode_ms: None,
            t_verify_ms: None,
            t_assign_ms: None,
            t_render_ms: None,
            t_render_frame_ms: None,
            post_ms: IndexMap::new(),
            first_visual_ms: None,
            final_visual_ms: None,
            incomplete: true,
            kind: "Unknown".to_owned(),
        }
    }
}

/// `PreloadItemRow(Slot, PathId, QueueWaitMs, Kind, Ms)`.
#[derive(Debug, Clone, PartialEq)]
pub struct PreloadItemRow {
    pub slot: i32,
    pub path_id: String,
    pub queue_wait_ms: f64,
    pub kind: String,
    pub ms: f64,
}

/// `DispatcherLongOpRow(Ms, Priority, Name)`.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatcherLongOpRow {
    pub ms: f64,
    pub priority: String,
    pub name: String,
}

/// `FolderGenSummary`: one folder-open generation (T0 = `Folder start`, T1 catalog ready, T2 first image
/// presented, T3 Explorer order applied). C# property `Gen` is `generation` here (`gen` is a reserved word in Rust 2024).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FolderGenSummary {
    pub generation: i64,
    pub t1_catalog_ready_ms: Option<f64>,
    pub t2_ms: Option<f64>,
    pub t3_ms: Option<f64>,
    pub t3_phase: String,
}

/// Everything reassembled from one `perf-*.csv` (C# `PerfFileAnalysis`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PerfFileAnalysis {
    /// In order of the first row that mentions each nav id.
    pub navs: Vec<NavRecord>,
    pub preload_items: Vec<PreloadItemRow>,
    pub preload_paused_count: i32,
    pub preload_cancel_count: i32,
    /// In order of the first `Folder` row of each distinct `nav`-column text.
    pub folder_gens: Vec<FolderGenSummary>,
    pub dispatcher_long_ops: Vec<DispatcherLongOpRow>,
    /// `DispatcherLongOp` rows dropped because they precede the first `ShowStart` / `Folder start` of the file.
    pub dispatcher_long_ops_before_start_count: i32,
    /// `Startup`: phase -> ms since process start (first row per phase), plus the derived `firstPresented`.
    /// C# `Dictionary` insertion order; assigning an existing key keeps its position.
    pub startup: IndexMap<String, f64>,
}

/// `KeyInputIndex` (internal in C#): `KeyInput` rows bucketed per thread and sorted by `qpc_ticks` (stable, so rows
/// with equal timestamps keep file order), so "nearest KeyInput at or before a ShowStart" is a binary search.
#[derive(Debug, Clone)]
pub struct KeyInputIndex {
    by_thread: std::collections::HashMap<i32, Vec<PerfRow>>,
}

impl KeyInputIndex {
    /// Groups by `thread` and stably sorts each bucket by `qpc_ticks`.
    pub fn new(key_inputs: &[PerfRow]) -> Self {
        todo!()
    }

    /// The KeyInput on `thread` with the greatest `qpc_ticks <= qpc_ticks`; among rows with that same timestamp the
    /// FIRST in file order. `None` when the thread has none at or before.
    pub fn nearest_at_or_before(&self, thread: i32, qpc_ticks: i64) -> Option<&PerfRow> {
        todo!()
    }
}

/// `PerfAnalyzeNavBuilder.Build(file)`.
pub fn build(file: &PerfCsvFile) -> PerfFileAnalysis {
    todo!()
}

/// `PerfAnalyzeNavBuilder.ClassifyKind(nav)`: `RamHit` / `InflightJoin` / `DiskCacheHit` / `SourceMiss` / `Unknown`
/// from `lookup_result` (+ `t_disk_ms`, `t_decode_ms` presence for `miss`), prefixed `Thumbnail+` when `has_thumbnail`.
pub fn classify_kind(nav: &NavRecord) -> String {
    todo!()
}
