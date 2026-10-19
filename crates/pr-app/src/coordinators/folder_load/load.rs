//! Port of `PhotoReview.App.Shared/Coordinators/FolderLoadCoordinator.cs` lines 59 to 270 (`LoadAsync` up to the Explorer
//! `orderSettled` return): the synchronous prefix, the scan job, and the continuation that applies the scan to the catalog
//! (contract `contracts/W8-07a.md`). Target: `coordinators::folder_load::load`.
//!
//! The C# method is cut at its first Explorer branch: everything after the first image request (late Explorer order, the
//! `finally` block) is `reload.rs` (W8-07b), the readability probe is `skipped.rs` (W8-07b).
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::sync::Arc;

use pr_core::abstractions::FileSystem;
use pr_core::model::{ImageSortMode, RawPairMode};
use pr_core::sync::CancelToken;

use super::state::{
    FolderLoadCoordinator, FolderLoadCx, FolderLoadError, FolderLoadEvent, FolderLoadUpdate, FolderPerf, ScanFailure,
    ScanOutput,
};

/// Everything the scan job needs, captured on the UI thread at load start (the settings snapshot of lines 128 to 130).
/// The job never sees the catalog, the sink or the live settings.
pub(super) struct ScanJobInput {
    pub file_system: Arc<dyn FileSystem>,
    /// Normalized folder.
    pub folder: String,
    pub token: CancelToken,
    pub raw_enabled: bool,
    pub webp_heic_enabled: bool,
    /// `Separate` when `raw_enabled` is false (C# `rawEnabled ? RawPairMode : Separate`).
    pub raw_pair_mode: RawPairMode,
    pub sort_mode: ImageSortMode,
    pub perf: Option<Arc<dyn FolderPerf>>,
    pub load_generation: i64,
}

/// The scan job body (runs on `WorkerLane::Scan`; C# `Task.Run(() => { .. })`, lines 131 to 169).
/// Listing predicate = supported image (`is_supported_with(path, raw, webp_heic)`) OR (RAW on and extension `.xmp`,
/// OrdinalIgnoreCase). Once the token is canceled the predicate rejects every entry (the Rust `FileSystem` listing cannot be
/// aborted from the predicate, open question O4) and the job returns `Canceled` after the listing. Sidecars are split from
/// images; entries keep `length`/`last_write_utc` from the listing stat; sort with `sort_entries(&scanned, sort_mode, None)`;
/// the result takes the FIRST scanned entry per case-insensitive path (`ignore_case_key`). Marks "scanned"(count) and "sorted".
pub(super) fn run_scan(input: ScanJobInput) -> Result<ScanOutput, ScanFailure> {
    todo!()
}

impl FolderLoadCoordinator {
    /// C# `LoadAsync(folder, initialPath)`, synchronous prefix (UI thread). Order of effects:
    /// 1. `Disposed` / `BlankFolder` errors (no side effect).
    /// 2. cancel the previous load's token; new token and `load_id`; `clock.next_folder()`; release the previous pending-order
    ///    gate (`finish_load` of the superseded load is NOT run, its events are dropped by `load_id`).
    /// 3. `perf "start"`; normalize the folder (`get_full_path` then `trim_ending_directory_separator`); `directory_exists`.
    ///    A failure here calls `cx.sink.on_failed(folder, ..)` (folder as given for an invalid path, normalized for a
    ///    missing one) and the load is finished.
    /// 4. snapshot of the settings (sort mode, RAW, RAW pair mode, WebP/HEIC), once.
    /// 5. Explorer: only when `uses_explorer_order(sort_mode)`: create the `ExplorerQueryHandle`, publish it to
    ///    `latest_explorer`, submit the query on `WorkerLane::Explorer` BEFORE the scan. Otherwise placeholder snapshot,
    ///    `latest_explorer.set(None)`, perf "explorerSkipped".
    /// 6. submit the scan + sort job on `WorkerLane::Scan`.
    ///
    /// Returns `Ok(Pending)` while waiting for events, `Ok(Finished)` when the load ended synchronously (step 3 failure).
    pub fn load(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        folder: &str,
        initial_path: Option<&str>,
    ) -> Result<FolderLoadUpdate, FolderLoadError> {
        todo!()
    }

    /// Delivers a worker result (UI thread only). Dispatches to `on_scan_finished` (this file), `on_explorer_finished`
    /// (`reload.rs`) and `on_probe_finished` (`skipped.rs`). An event whose `load_id` is not the active load's is dropped
    /// with `FolderLoadStep::Ignored` (INV-9). A disposed coordinator ignores everything.
    pub fn handle_event(&mut self, cx: &mut FolderLoadCx<'_>, event: FolderLoadEvent) -> FolderLoadUpdate {
        todo!()
    }

    /// C# continuation of `await Task.Run(scan)` (lines 170 to 268) for one `ScanFinished` event.
    pub(super) fn on_scan_finished(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        load_id: u64,
        outcome: Result<ScanOutput, ScanFailure>,
    ) -> FolderLoadUpdate {
        todo!()
    }
}
