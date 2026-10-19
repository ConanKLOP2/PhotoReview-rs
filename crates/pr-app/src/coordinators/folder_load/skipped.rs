//! Port of `PhotoReview.App.Shared/Coordinators/FolderLoadCoordinator.cs` lines 347 to 468: `ReadabilityProbe`,
//! `StartReadabilityProbe`, `ApplyReadabilityProbeAsync` (AR16, IO05; contract `contracts/W8-07b.md`).
//! Target: `coordinators::folder_load::skipped`.
//!
//! `SkippedEntry` is NOT redefined here: the one in `pr_core::abstractions` is used (path `PathBuf`, reason text, kind).
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::sync::Arc;

use pr_core::abstractions::{FileSystem, SkippedEntry};
use pr_core::sync::CancelToken;

use super::state::{FolderLoadCoordinator, FolderLoadCx, FolderLoadUpdate, FolderPerf};

/// The probe job body (runs on `WorkerLane::Probe`; C# `StartReadabilityProbe`): probes `paths` with at most
/// `PROBE_PARALLELISM` threads, one `try_probe_readable` each, checking the token before every file. A failed probe whose file
/// still exists (`file_exists`) is unreadable (reason `""` when the text is empty); a file that is gone is NOT reported.
/// The result is in listing order (index order), never in completion order, kind `SkippedKind::FileUnreadable`.
/// Marks "probeStart"(count) and "probed"(unreadable count). `Err(Canceled)` when the token fired (C# `Parallel.For` throws).
pub(super) fn run_probe(
    file_system: Arc<dyn FileSystem>,
    paths: Arc<[String]>,
    token: CancelToken,
    perf: Option<Arc<dyn FolderPerf>>,
    load_generation: i64,
) -> Result<Vec<SkippedEntry>, pr_core::sync::Canceled> {
    todo!()
}

impl FolderLoadCoordinator {
    /// C# `ReadabilityProbe` not completed: a probe was started and has not been applied or dropped yet.
    pub fn probe_pending(&self) -> bool {
        todo!()
    }

    /// C# `StartReadabilityProbe` call site (line 249): submits `run_probe` on `WorkerLane::Probe`. Called by
    /// `on_scan_finished` right after the first `sink.present` of a non-empty catalog, never for an empty folder.
    pub(super) fn start_probe(&mut self) {
        todo!()
    }

    /// Delivers `ProbeFinished` (UI thread). A stale `load_id` is dropped (a failed stale probe is only logged, C#
    /// `FireAndLog`). If the load is still running the result is stored (`ProbeState::Ready`) and applied by `finish_load`;
    /// if the load already finished and published the probe, it is applied now.
    pub(super) fn on_probe_finished(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        load_id: u64,
        outcome: Result<Vec<SkippedEntry>, Option<String>>,
    ) -> FolderLoadUpdate {
        todo!()
    }

    /// C# `ApplyReadabilityProbeAsync` (lines 397 to 468), UI thread. `Err(None)` (canceled) returns silently; `Err(Some)`
    /// logs `error("Readability probe failed in '<folder>'; the catalog keeps every listed file")` and removes nothing. Then,
    /// gated by the LOAD token only (never by the folder generation, a file action bumps that without changing the folder):
    /// canceled -> perf "probeDropped"; no unreadable file -> nothing; `catalog.remove_paths(unreadable paths)` -> empty ->
    /// nothing (already gone); else `log.warn` line, `sink.on_files_skipped(folder, listing_skipped + probe_skipped)` (probe
    /// entries limited to the removed paths, OrdinalIgnoreCase), perf "probeApplied"(removed count), then
    /// `sink.on_unreadable_removed(&removed, current_removed)` where `current_removed` = the current path before removal is in
    /// the removed set.
    pub(super) fn apply_probe(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        load_id: u64,
        outcome: Result<Vec<SkippedEntry>, Option<String>>,
    ) {
        todo!()
    }
}
