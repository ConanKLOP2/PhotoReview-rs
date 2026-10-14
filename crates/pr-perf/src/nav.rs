//! Port of `PhotoReview.PerfAnalysis/PerfAnalyzeNav.cs` (C# baseline e441d67a, 368 lines): reassembles the rows of
//! one `perf-*.csv` into navigation records, preload/dispatcher/folder/startup aggregates.
//!
//! In C# the builder is the static class `PerfAnalyzeNavBuilder`; here its methods are the free functions
//! [`build`] and [`classify_kind`]. Plan references in the C# comments ("plan muc 3", "D11 spec item 2") point at
//! removed docs; the C# code is the spec.
//!
//! Unit W1-19. Contract: contracts/W1-19/contract.md. Depends on `crate::csv` only.

use indexmap::IndexMap;

use crate::csv::{PerfCsvFile, PerfRow, parse_integer_i64};

/// C# `KeyInputMatchWindowMs`: the nearest KeyInput on the same thread strictly at or before ShowStart's timestamp,
/// within this many milliseconds (D11 spec item 2).
const KEY_INPUT_MATCH_WINDOW_MS: f64 = 500.0;

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
        let mut by_thread: std::collections::HashMap<i32, Vec<PerfRow>> = std::collections::HashMap::new();
        for row in key_inputs {
            by_thread.entry(row.thread).or_default().push(row.clone());
        }
        for rows in by_thread.values_mut() {
            // Stable sort: KeyInputs with an identical timestamp keep file order (C# OrderBy).
            rows.sort_by_key(|r| r.qpc_ticks);
        }
        Self { by_thread }
    }

    /// The KeyInput on `thread` with the greatest `qpc_ticks <= qpc_ticks`; among rows with that same timestamp the
    /// FIRST in file order. `None` when the thread has none at or before.
    pub fn nearest_at_or_before(&self, thread: i32, qpc_ticks: i64) -> Option<&PerfRow> {
        let rows = self.by_thread.get(&thread)?;
        // Rows are sorted by qpc_ticks, so both boundaries are binary searches (C# FirstIndexWhere).
        let upper = rows.partition_point(|r| r.qpc_ticks <= qpc_ticks);
        if upper == 0 {
            return None;
        }
        let best = rows.get(upper - 1)?.qpc_ticks;
        rows.get(rows.partition_point(|r| r.qpc_ticks < best))
    }
}

/// `PerfAnalyzeNavBuilder.Build(file)`.
pub fn build(file: &PerfCsvFile) -> PerfFileAnalysis {
    let mut result = PerfFileAnalysis::default();
    let mut by_nav: IndexMap<i64, Vec<&PerfRow>> = IndexMap::new();
    let mut key_inputs: Vec<PerfRow> = Vec::new();
    let mut folder_rows: Vec<&PerfRow> = Vec::new();
    let mut presented_global: Vec<&PerfRow> = Vec::new();
    let mut startup_anchor: Option<&PerfRow> = None;

    // D06 driver note: DispatcherLongOp rows before the first ShowStart / Folder(start) are driver setup
    // (the STA harness creating the window), not app UI-thread contention. Case-sensitive event and text matches.
    let first_relevant_qpc = file
        .rows
        .iter()
        .filter(|r| r.event == "ShowStart" || (r.event == "Folder" && r.text == "start"))
        .map(|r| r.qpc_ticks)
        .min();

    for row in &file.rows {
        match row.event.as_str() {
            "KeyInput" => key_inputs.push(row.clone()),
            "PreloadItem" => result.preload_items.push(PreloadItemRow {
                // C# `(int)(ANum ?? 0)`: `as` truncates toward zero and saturates, like .NET 9+.
                slot: row.a_num().unwrap_or(0.0) as i32,
                path_id: row.path_id.clone(),
                queue_wait_ms: row.b_num().unwrap_or(0.0),
                kind: row.text.clone(),
                ms: row.c_num().unwrap_or(0.0),
            }),
            "PreloadPaused" => result.preload_paused_count = result.preload_paused_count.wrapping_add(1),
            "PreloadCancel" => result.preload_cancel_count = result.preload_cancel_count.wrapping_add(1),
            "DispatcherLongOp" => match first_relevant_qpc {
                Some(threshold) if row.qpc_ticks < threshold => {
                    result.dispatcher_long_ops_before_start_count =
                        result.dispatcher_long_ops_before_start_count.wrapping_add(1);
                }
                _ => {
                    // C# `Split(';', 2)`: no ';' means the whole text is the priority and the name is empty.
                    let (priority, name) = row.text.split_once(';').unwrap_or((row.text.as_str(), ""));
                    result.dispatcher_long_ops.push(DispatcherLongOpRow {
                        ms: row.a_num().unwrap_or(0.0),
                        priority: priority.to_owned(),
                        name: name.to_owned(),
                    });
                }
            },
            "Folder" => folder_rows.push(row),
            "Startup" => {
                // Only the first accepted row per phase counts; the anchor is the first accepted row overall.
                if let Some(since_start) = row.a_num() {
                    if !result.startup.contains_key(&row.text) {
                        result.startup.insert(row.text.clone(), since_start);
                        if startup_anchor.is_none() {
                            startup_anchor = Some(row);
                        }
                    }
                }
            }
            _ => {
                if row.event == "Presented" {
                    presented_global.push(row);
                }
                // nav <= 0 or unparseable: preload work not tied to a navigation (only nav > 0 makes a record).
                if let Some(id) = row.nav_id().filter(|id| *id > 0) {
                    by_nav.entry(id).or_default().push(row);
                }
            }
        }
    }

    result.folder_gens = build_folder_summaries(file, &folder_rows, &presented_global);

    // Startup `firstPresented`: the earliest Presented on the process-start timeline via the anchor row.
    if let Some(anchor) = startup_anchor {
        let first_presented = presented_global.iter().copied().min_by_key(|p| p.qpc_ticks);
        if let (Some(anchor_ms), Some(first)) = (anchor.a_num(), first_presented) {
            let ms = anchor_ms + file.qpc_to_ms(first.qpc_ticks.wrapping_sub(anchor.qpc_ticks));
            result.startup.insert("firstPresented".to_owned(), ms);
        }
    }

    let key_input_index = KeyInputIndex::new(&key_inputs);
    for (nav_id, rows) in by_nav {
        result
            .navs
            .push(build_nav_record(file, nav_id, &rows, &key_input_index));
    }
    result
}

/// C# `BuildFolderSummaries`: one summary per distinct raw `nav` text of the `Folder` rows (first-appearance order),
/// skipping texts that do not parse as an integer.
fn build_folder_summaries(
    file: &PerfCsvFile,
    folder_rows: &[&PerfRow],
    presented: &[&PerfRow],
) -> Vec<FolderGenSummary> {
    let mut groups: IndexMap<&str, Vec<&PerfRow>> = IndexMap::new();
    for row in folder_rows {
        groups.entry(row.nav.as_str()).or_default().push(row);
    }

    let mut output = Vec::new();
    for (key, mut rows) in groups {
        let Some(generation) = parse_integer_i64(key) else {
            continue;
        };
        rows.sort_by_key(|r| r.qpc_ticks); // stable

        let mut summary = FolderGenSummary {
            generation,
            ..FolderGenSummary::default()
        };
        summary.t1_catalog_ready_ms = rows.iter().find(|r| r.text == "catalogReady").and_then(|r| r.a_num());
        if let Some(t3) = rows.iter().find(|r| {
            matches!(
                r.text.as_str(),
                "explorerApplied" | "explorerFallback" | "explorerIgnored"
            )
        }) {
            summary.t3_ms = t3.a_num();
            summary.t3_phase = t3.text.clone();
        }
        if let Some(start) = rows.iter().find(|r| r.text == "start") {
            // An equal timestamp counts; the first in file order wins on ties (min_by_key keeps the first minimum).
            let first = presented
                .iter()
                .copied()
                .filter(|p| p.qpc_ticks >= start.qpc_ticks)
                .min_by_key(|p| p.qpc_ticks);
            if let Some(fp) = first {
                summary.t2_ms = Some(file.qpc_to_ms(fp.qpc_ticks.wrapping_sub(start.qpc_ticks)));
            }
        }
        output.push(summary);
    }
    output
}

/// C# `BuildNavRecord`: one navigation from its rows (already grouped by nav id, in file order).
fn build_nav_record(file: &PerfCsvFile, nav_id: i64, rows: &[&PerfRow], index: &KeyInputIndex) -> NavRecord {
    let mut ordered: Vec<&PerfRow> = rows.to_vec();
    ordered.sort_by_key(|r| r.qpc_ticks); // stable: file order breaks ties
    let mut rec = NavRecord {
        nav: nav_id,
        ..NavRecord::default()
    };
    let Some(first) = ordered.first() else {
        return rec;
    };

    let show_start = ordered.iter().copied().find(|r| r.event == "ShowStart");
    let mut t0_qpc = match show_start {
        Some(ss) => ss.qpc_ticks,
        None => first.qpc_ticks,
    };
    if let Some(ss) = show_start {
        rec.mode = ss.text.clone();

        let matched = match index.nearest_at_or_before(ss.thread, ss.qpc_ticks) {
            Some(k) if file.qpc_to_ms(ss.qpc_ticks.wrapping_sub(k.qpc_ticks)) > KEY_INPUT_MATCH_WINDOW_MS => None,
            other => other,
        };
        if let Some(k) = matched {
            rec.t_input_ms = k.a_num();
            rec.t_pre_ms = Some(file.qpc_to_ms(ss.qpc_ticks.wrapping_sub(k.qpc_ticks)));
            t0_qpc = k.qpc_ticks;
        }
    }

    let mut pending_render: Option<f64> = None;
    let mut first_presented_seen = false;
    for row in ordered.iter().copied() {
        match row.event.as_str() {
            "Stat" => rec.t_stat_ms = row.a_num(),
            "Lookup" => rec.lookup_result = row.text.clone(),
            "ThumbEnd" => {
                // A specific source (ram|disk|embedded|none) beats the MainWindow "unknown" total; a later "unknown"
                // never overwrites an existing value, and an "unknown" with an empty `a` leaves None.
                if row.text != "unknown" || rec.t_thumb_ms.is_none() {
                    rec.t_thumb_ms = row.a_num();
                }
            }
            "JoinEnd" => rec.t_join_ms = row.a_num(),
            "DiskCacheRead" => rec.t_disk_ms = row.a_num(),
            "SourceRead" => rec.t_read_ms = row.a_num(),
            "Decode" => rec.t_decode_ms = row.a_num(),
            "Verify" => rec.t_verify_ms = row.a_num(),
            "Assign" => rec.t_assign_ms = row.a_num(),
            "Rendered" => pending_render = row.a_num(),
            "Presented" => {
                if row.text == "thumbnail" {
                    rec.has_thumbnail = true;
                }
                if !first_presented_seen {
                    first_presented_seen = true;
                    rec.first_visual_ms = Some(file.qpc_to_ms(row.qpc_ticks.wrapping_sub(t0_qpc)));
                }
                if row.text == "final" || row.text == "compare" {
                    rec.final_visual_ms = Some(file.qpc_to_ms(row.qpc_ticks.wrapping_sub(t0_qpc)));
                    rec.t_render_ms = pending_render;
                    rec.incomplete = false;
                }
                pending_render = None;
            }
            "PostEnd" => {
                // Insert-or-replace: a repeated phase keeps its first position (IndexMap).
                rec.post_ms.insert(row.text.clone(), row.a_num().unwrap_or(0.0));
            }
            // RenderedFrame arrives after Presented, so it is assigned directly.
            "RenderedFrame" => rec.t_render_frame_ms = row.a_num(),
            _ => {}
        }
    }

    rec.kind = classify_kind(&rec);
    rec
}

/// `PerfAnalyzeNavBuilder.ClassifyKind(nav)`: `RamHit` / `InflightJoin` / `DiskCacheHit` / `SourceMiss` / `Unknown`
/// from `lookup_result` (+ `t_disk_ms`, `t_decode_ms` presence for `miss`), prefixed `Thumbnail+` when `has_thumbnail`.
pub fn classify_kind(nav: &NavRecord) -> String {
    let base = match nav.lookup_result.as_str() {
        "ramHit" => "RamHit",
        "inflight" => "InflightJoin",
        "miss" => {
            if nav.t_disk_ms.is_some() {
                "DiskCacheHit"
            } else if nav.t_decode_ms.is_some() {
                "SourceMiss"
            } else {
                "InflightJoin"
            }
        }
        _ => "Unknown",
    };
    if nav.has_thumbnail {
        format!("Thumbnail+{base}")
    } else {
        base.to_owned()
    }
}
