//! Port of `PhotoReview.App.Shared/Coordinators/FolderLoadCoordinator.cs` lines 59 to 270 (`LoadAsync` up to the Explorer
//! `orderSettled` return): the synchronous prefix, the scan job, and the continuation that applies the scan to the catalog
//! (contract `contracts/W8-07a.md`). Target: `coordinators::folder_load::load`.
//!
//! The C# method is cut at its first Explorer branch: everything after the first image request (late Explorer order, the
//! `finally` block) is `reload.rs` (W8-07b), the readability probe is `skipped.rs` (W8-07b).

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{FileSystem, SkippedEntry, SkippedKind};
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::catalog::snapshot_validator::try_validate;
use pr_core::catalog::sorting::sort_entries;
use pr_core::catalog::types::{CatalogEntry, ExplorerQueryHandle, get_extension, is_supported_with};
use pr_core::file_actions::errors::path_to_journal_string;
use pr_core::model::{ImageSortMode, RawPairMode, uses_explorer_order};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{get_full_path, trim_ending_directory_separator};
use pr_core::text::{eq_ignore_case, ignore_case_key};
use pr_core::time::DateTimeUtc;

use super::state::{
    ActiveLoad, EXPLORER_BATCH_SIZE, EXPLORER_QUERY_TIMEOUT, ExplorerSlot, FolderLoadCoordinator, FolderLoadCx,
    FolderLoadError, FolderLoadEvent, FolderLoadStep, FolderLoadUpdate, FolderPerf, LOGGED_SKIPPED_LIMIT, LoadPhase,
    ProbeState, ScanFailure, ScanOutput, WorkerLane,
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

fn mark(perf: &Option<Arc<dyn FolderPerf>>, generation: i64, phase: &str, value: Option<i64>) {
    if let Some(perf) = perf {
        perf.mark(generation, phase, value, "");
    }
}

fn is_xmp(path: &str) -> bool {
    eq_ignore_case(get_extension(path), ".xmp")
}

/// The text of a caught panic payload.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unexpected error".to_string()
    }
}

fn count_i32(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// The scan job body (runs on `WorkerLane::Scan`; C# `Task.Run(() => { .. })`, lines 131 to 169).
/// Listing predicate = supported image (`is_supported_with(path, raw, webp_heic)`) OR (RAW on and extension `.xmp`,
/// OrdinalIgnoreCase). Once the token is canceled the predicate rejects every entry (the Rust `FileSystem` listing cannot be
/// aborted from the predicate, open question O4) and the job returns `Canceled` after the listing. Sidecars are split from
/// images; entries keep `length`/`last_write_utc` from the listing stat; sort with `sort_entries(&scanned, sort_mode, None)`;
/// the result takes the FIRST scanned entry per case-insensitive path (`ignore_case_key`). Marks "scanned"(count) and "sorted".
pub(super) fn run_scan(input: ScanJobInput) -> Result<ScanOutput, ScanFailure> {
    let ScanJobInput {
        file_system,
        folder,
        token,
        raw_enabled,
        webp_heic_enabled,
        raw_pair_mode,
        sort_mode,
        perf,
        load_generation,
    } = input;

    let mut skipped = Vec::new();
    let listed = {
        let include = |path: &Path| {
            // RV-A07 (D-07a-3): the predicate runs once per directory entry; after a cancel it accepts nothing.
            if token.is_canceled() {
                return false;
            }
            let text = path.to_string_lossy();
            is_supported_with(&text, raw_enabled, webp_heic_enabled) || (raw_enabled && is_xmp(&text))
        };
        file_system.enumerate_files_with_stat(Path::new(&folder), &include, &mut |entry| skipped.push(entry))
    };
    if token.is_canceled() {
        return Err(ScanFailure::Canceled);
    }
    let all_scanned = listed.map_err(|error| ScanFailure::Failed(FolderLoadError::Io(error)))?;

    let mut sidecar_paths = Vec::new();
    let mut scanned = Vec::new();
    for (path, stat) in all_scanned {
        let lossy = path.to_string_lossy();
        let relevant = is_xmp(&lossy) || is_supported_with(&lossy, raw_enabled, webp_heic_enabled);
        // ST-PATHLOSSY: a name with an unpaired UTF-16 surrogate cannot be a `String`; `to_string_lossy` would give it
        // the name of ANOTHER file (a sibling really named with U+FFFD) and every later Move/Copy/Recycle/Undo would
        // act on that one. C# refuses such a path (`UnrepresentablePath`) before it mutates anything. The entry is not
        // loaded and is reported as skipped, so it is never reachable by an operation.
        let text = match path_to_journal_string(&path) {
            Ok(text) => text,
            Err(error) => {
                if relevant {
                    skipped.push(SkippedEntry {
                        path,
                        reason: error.message,
                        kind: SkippedKind::FileUnreadable,
                    });
                }
                continue;
            }
        };
        if is_xmp(&text) {
            sidecar_paths.push(text.clone());
        }
        if !is_supported_with(&text, raw_enabled, webp_heic_enabled) {
            continue;
        }
        let Ok(mut entry) = CatalogEntry::new(text) else {
            continue;
        };
        if let Some(stat) = stat {
            entry.length = Some(i64::try_from(stat.length).unwrap_or(i64::MAX));
            entry.last_write_utc = Some(stat.last_write_utc);
        }
        scanned.push(entry);
    }
    mark(&perf, load_generation, "scanned", i64::try_from(scanned.len()).ok());
    if token.is_canceled() {
        return Err(ScanFailure::Canceled);
    }

    // Sort the scanned entries directly so stat metadata travels with each item; every sorted entry is replaced by the
    // FIRST scanned entry with the same case-insensitive path (C# `firstByPath` with `TryAdd`).
    let sorted = sort_entries(&scanned, sort_mode, None);
    let mut first_by_path: HashMap<String, usize> = HashMap::with_capacity(scanned.len());
    for (index, entry) in scanned.iter().enumerate() {
        first_by_path.entry(ignore_case_key(entry.path())).or_insert(index);
    }
    let entries: Vec<CatalogEntry> = sorted
        .iter()
        .map(|entry| {
            first_by_path
                .get(&ignore_case_key(entry.path()))
                .and_then(|index| scanned.get(*index))
                .unwrap_or(entry)
                .clone()
        })
        .collect();
    mark(&perf, load_generation, "sorted", None);

    let scanned_files = scanned.iter().map(|entry| entry.path().to_owned()).collect();
    Ok(ScanOutput {
        scanned_files,
        entries,
        sidecar_paths,
        skipped,
        raw_pair_mode,
    })
}

impl FolderLoadCoordinator {
    pub(super) fn perf_mark(&self, generation: i64, phase: &str, value: Option<i64>) {
        mark(&self.deps.perf, generation, phase, value);
    }

    pub(super) fn perf_mark_value(&self, generation: i64, phase: &str, value: usize) {
        mark(&self.deps.perf, generation, phase, i64::try_from(value).ok());
    }

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
        if self.disposed {
            return Err(FolderLoadError::Disposed);
        }
        if folder.trim().is_empty() {
            return Err(FolderLoadError::BlankFolder);
        }

        if let Some(previous) = self.active.take() {
            previous.token.cancel();
        }
        let token = CancelToken::new();
        let load_generation = self.deps.clock.next_folder();
        // A superseded load's gate must not strand navigation that is waiting on it.
        let order_gate_released = std::mem::take(&mut self.order_gate_open);
        self.next_load_id += 1;
        let load_id = self.next_load_id;
        let finished = FolderLoadUpdate {
            step: FolderLoadStep::Finished,
            order_gate_released,
        };
        self.perf_mark(load_generation, "start", None);

        // Trim after get_full_path: trimming first turned the root "C:\" into "C:" (the drive's current directory).
        let full = match get_full_path(folder) {
            Ok(full) => full,
            Err(error) => {
                cx.sink.on_failed(folder, &FolderLoadError::InvalidPath(error));
                return Ok(finished);
            }
        };
        let folder = trim_ending_directory_separator(&full).to_owned();
        if !self.deps.file_system.directory_exists(Path::new(&folder)) {
            let error = FolderLoadError::DirectoryNotFound { folder: folder.clone() };
            cx.sink.on_failed(&folder, &error);
            return Ok(finished);
        }

        // Snapshot once per load: the scan runs on a worker while the catalog is filled on the UI thread later; reading the
        // live settings in both places lets a Settings change in between list RAW files but never pair them.
        let settings = self.deps.settings_store.current();
        let sort_mode = settings.image_sort_mode_or_default();
        let raw_enabled = settings.raw_support_enabled;
        let raw_pair_mode = if raw_enabled {
            settings.raw_pair_mode
        } else {
            RawPairMode::Separate
        };
        let webp_heic_enabled = settings.webp_heic_support_enabled;

        // perf(startup): the Explorer query needs only the folder, so it is submitted before the scan.
        let explorer = if uses_explorer_order(sort_mode) {
            let handle = ExplorerQueryHandle::new();
            if let Some(latest) = &self.deps.latest_explorer {
                latest.set(Some(handle.clone()));
            }
            self.submit_explorer_query(load_id, load_generation, &folder, &token, handle.clone());
            ExplorerSlot::Pending(handle)
        } else {
            // Default / NameAscending / NameDescending are decided by the app: the query is never started and an already
            // completed "unavailable" result takes the early path (order settled, no gate, ReplaceOrder never called).
            self.perf_mark(load_generation, "explorerSkipped", None);
            if let Some(latest) = &self.deps.latest_explorer {
                latest.set(None); // Diagnostics shows "not queried" rather than the placeholder result
            }
            ExplorerSlot::Unavailable(Arc::new(ExplorerViewSnapshot {
                folder: folder.clone(),
                ordered_paths: Vec::new(),
                sort_columns: Vec::new(),
                group_state: ExplorerGroupState::None,
                status: ExplorerOrderStatus::NativeViewUnavailable,
                reason: Some("Explorer order not used by this sort mode".to_owned()),
                captured_utc: DateTimeUtc::now(),
            }))
        };

        let input = ScanJobInput {
            file_system: Arc::clone(&self.deps.file_system),
            folder: folder.clone(),
            token: token.clone(),
            raw_enabled,
            webp_heic_enabled,
            raw_pair_mode,
            sort_mode,
            perf: self.deps.perf.clone(),
            load_generation,
        };
        self.deps.worker.submit(
            WorkerLane::Scan,
            Box::new(move || FolderLoadEvent::ScanFinished {
                load_id,
                // A panic in the listing or the sort must still finish the load: without `ScanFinished` the load would
                // stay in `Scanning` for good (the user could not open a folder again until a new load superseded it).
                outcome: catch_unwind(AssertUnwindSafe(|| run_scan(input))).unwrap_or_else(|payload| {
                    Err(ScanFailure::Failed(FolderLoadError::Io(std::io::Error::other(
                        panic_text(payload.as_ref()),
                    ))))
                }),
            }),
        );

        self.active = Some(ActiveLoad {
            load_id,
            folder,
            initial_path: initial_path.map(str::to_owned),
            token,
            load_generation,
            phase: LoadPhase::Scanning,
            explorer,
            skipped: Vec::new(),
            scanned_files: Arc::from(Vec::new()),
            interaction_generation: 0,
            presentation_generation: 0,
            owns_order_gate: false,
            probe: ProbeState::None,
        });
        Ok(FolderLoadUpdate {
            step: FolderLoadStep::Pending,
            order_gate_released,
        })
    }

    /// Submits the Explorer query on its own lane. The job completes the shared handle (also published to
    /// `LatestExplorerSnapshot`) and returns the snapshot; a panic in the provider is a failed query (`None`).
    fn submit_explorer_query(
        &self,
        load_id: u64,
        load_generation: i64,
        folder: &str,
        token: &CancelToken,
        handle: ExplorerQueryHandle,
    ) {
        let provider = Arc::clone(&self.deps.explorer_order);
        let perf = self.deps.perf.clone();
        let folder = folder.to_owned();
        let token = token.clone();
        self.deps.worker.submit(
            WorkerLane::Explorer,
            Box::new(move || {
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    provider.try_get_snapshot_progressive(
                        Path::new(&folder),
                        EXPLORER_QUERY_TIMEOUT,
                        None,
                        EXPLORER_BATCH_SIZE,
                        &token,
                    )
                }));
                let snapshot = match outcome {
                    Ok(snapshot) => {
                        if let Some(perf) = &perf {
                            let count = i64::try_from(snapshot.ordered_paths.len()).ok();
                            perf.mark(
                                load_generation,
                                "explorerSnapshot",
                                count,
                                &format!("{:?}", snapshot.status),
                            );
                        }
                        handle.complete(snapshot);
                        handle.snapshot()
                    }
                    Err(_) => {
                        if let Some(perf) = &perf {
                            perf.mark(load_generation, "explorerSnapshot", None, "Panic");
                        }
                        handle.fail();
                        None
                    }
                };
                FolderLoadEvent::ExplorerFinished { load_id, snapshot }
            }),
        );
    }

    /// Delivers a worker result (UI thread only). Dispatches to `on_scan_finished` (this file), `on_explorer_finished`
    /// (`reload.rs`) and `on_probe_finished` (`skipped.rs`). An event whose `load_id` is not the active load's is dropped
    /// with `FolderLoadStep::Ignored` (INV-9). A disposed coordinator ignores everything.
    pub fn handle_event(&mut self, cx: &mut FolderLoadCx<'_>, event: FolderLoadEvent) -> FolderLoadUpdate {
        if self.disposed {
            return FolderLoadUpdate {
                step: FolderLoadStep::Ignored,
                order_gate_released: false,
            };
        }
        match event {
            FolderLoadEvent::ScanFinished { load_id, outcome } => self.on_scan_finished(cx, load_id, outcome),
            FolderLoadEvent::ExplorerFinished { load_id, snapshot } => self.on_explorer_finished(cx, load_id, snapshot),
            FolderLoadEvent::ProbeFinished { load_id, outcome } => self.on_probe_finished(cx, load_id, outcome),
        }
    }

    /// C# continuation of `await Task.Run(scan)` (lines 170 to 268) for one `ScanFinished` event.
    pub(super) fn on_scan_finished(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        load_id: u64,
        outcome: Result<ScanOutput, ScanFailure>,
    ) -> FolderLoadUpdate {
        // Guard 1 (INV-9): only the active load, and only while it waits for its scan.
        let Some(active) = self
            .active
            .as_ref()
            .filter(|a| a.load_id == load_id && a.phase == LoadPhase::Scanning)
        else {
            return FolderLoadUpdate {
                step: FolderLoadStep::Ignored,
                order_gate_released: false,
            };
        };
        let token = active.token.clone();
        let load_generation = active.load_generation;
        let folder = active.folder.clone();
        let initial_path = active.initial_path.clone();

        let scan = match outcome {
            Ok(scan) => scan,
            Err(ScanFailure::Canceled) => return self.finish_load(cx, load_id),
            Err(ScanFailure::Failed(error)) => {
                self.report_load_error(cx, load_id, &error);
                return self.finish_load(cx, load_id);
            }
        };
        self.perf_mark(load_generation, "sortResumed", None);

        // A file opened directly keeps its real place in the sorted order: the resume below looks it up by path.
        let requested_path = match initial_path.as_deref().map(get_full_path).transpose() {
            Ok(path) => path,
            Err(error) => {
                self.report_load_error(cx, load_id, &FolderLoadError::InvalidPath(error));
                return self.finish_load(cx, load_id);
            }
        };

        // Guard 2 (token) and guard 3 (folder generation): the load was superseded or the folder changed under it.
        if token.is_canceled() || !self.deps.clock.is_folder_current(load_generation) {
            return self.finish_load(cx, load_id);
        }

        let ScanOutput {
            scanned_files,
            entries,
            sidecar_paths,
            skipped,
            raw_pair_mode,
        } = scan;

        cx.sink.reset_caches();
        if let Some(writer) = &self.deps.session_writer {
            writer.flush(); // a pending write for this folder must be visible to load
        }
        let session = match self.deps.session_store.load(&folder) {
            Ok(session) => session,
            Err(_) => {
                self.report_load_error(cx, load_id, &FolderLoadError::BlankFolder);
                return self.finish_load(cx, load_id);
            }
        };
        cx.catalog.reset(entries, raw_pair_mode, Some(&sidecar_paths));
        cx.sink
            .on_catalog_ready(&folder, count_i32(cx.catalog.count()), &session);
        if !skipped.is_empty() {
            let listed: Vec<String> = skipped
                .iter()
                .take(LOGGED_SKIPPED_LIMIT)
                .map(|entry| format!("{} ({})", entry.path.display(), entry.reason))
                .collect();
            self.deps.log.warn(&format!(
                "Folder scan skipped {} unreadable entr(y/ies) in '{}': {}",
                skipped.len(),
                folder,
                listed.join("; ")
            ));
            cx.sink.on_files_skipped(&folder, &skipped);
        }
        self.perf_mark(load_generation, "catalogReady", None);

        let interaction_generation = self.deps.clock.current_interaction();
        let mut resume_path = requested_path.or_else(|| session.current_path.clone());
        let scanned_files: Arc<[String]> = Arc::from(scanned_files);

        // perf(startup): with the batched Explorer read the snapshot is usually in before the catalog. Applying it before
        // the first frame reaches the same end state as the late path without the transient fallback frame.
        let mut explorer_failed = false;
        let mut early = None;
        let mut open_gate = false;
        if let Some(active) = self.active.as_mut() {
            active.skipped = skipped;
            active.scanned_files = Arc::clone(&scanned_files);
            active.interaction_generation = interaction_generation;
            match &active.explorer {
                ExplorerSlot::Unavailable(snapshot) | ExplorerSlot::Done(snapshot) => {
                    early = Some(Arc::clone(snapshot));
                }
                ExplorerSlot::Pending(handle) => {
                    early = handle.snapshot();
                    if let Some(snapshot) = &early {
                        active.explorer = ExplorerSlot::Done(Arc::clone(snapshot));
                    }
                }
                ExplorerSlot::Failed => explorer_failed = true,
            }
            if early.is_none() && !explorer_failed && initial_path.is_some() {
                // INV-9 (file open): the requested file is presented right away instead of after the snapshot, but
                // navigation/file actions wait for this gate.
                active.owns_order_gate = true;
                open_gate = true;
            }
        }
        if explorer_failed {
            // C#: awaiting a faulted task throws before the first image is requested.
            let error = FolderLoadError::Io(std::io::Error::other("The Explorer order query failed."));
            self.report_load_error(cx, load_id, &error);
            return self.finish_load(cx, load_id);
        }
        if open_gate {
            self.order_gate_open = true;
        }
        let mut order_settled = false;
        if let Some(snapshot) = early {
            self.perf_mark(load_generation, "explorerAwaited", None);
            order_settled = true;
            match try_validate(&snapshot, &scanned_files) {
                Ok(order) => {
                    if cx.catalog.replace_order(&order) {
                        cx.sink
                            .on_order_applied(count_i32(order.len()), cx.catalog.current_index(), false);
                        self.perf_mark(load_generation, "explorerApplied", None);
                        if initial_path.is_none() {
                            resume_path = None;
                        }
                    } else {
                        self.perf_mark(load_generation, "explorerIgnored", None);
                    }
                }
                Err(_) => self.perf_mark(load_generation, "explorerFallback", None),
            }
        }

        if cx.catalog.count() > 0 {
            let resume_index = resume_path.as_deref().map_or(-1, |path| cx.catalog.index_of(path));
            let target_index = if resume_index >= 0 { resume_index } else { 0 };
            cx.catalog.set_current(target_index);
            let presentation_generation = self.deps.clock.current_navigation();
            self.perf_mark(load_generation, "presentStart", None);
            cx.sink.present(target_index, presentation_generation);
            self.perf_mark(load_generation, "presentDone", None);
            // AR16: the per-file open probe starts only now, off the first-visual path.
            self.start_probe();
        } else {
            self.deps.clock.next_navigation();
            // Q-R47: tell "no images at all" from "only subfolders" (non-recursive on purpose). A failed listing means
            // "no subfolders known": OnEmpty's plain message is still correct.
            let subfolder_count = self
                .deps
                .file_system
                .enumerate_directories(Path::new(&folder))
                .map_or(0, |directories| directories.len());
            if subfolder_count > 0 {
                cx.sink
                    .on_empty_with_subfolders(&folder, &session, count_i32(subfolder_count));
            } else {
                cx.sink.on_empty(&folder, &session);
            }
        }

        if order_settled {
            return self.finish_load(cx, load_id);
        }

        // Hand-off to the late Explorer order (reload.rs). The presentation generation is read AFTER the first
        // present/empty branch (C# line 273).
        let presentation_generation = self.deps.clock.current_navigation();
        let mut ready = false;
        if let Some(active) = self.active.as_mut() {
            active.presentation_generation = presentation_generation;
            active.phase = LoadPhase::AwaitingOrder;
            if let ExplorerSlot::Pending(handle) = &active.explorer {
                if let Some(snapshot) = handle.snapshot() {
                    active.explorer = ExplorerSlot::Done(snapshot);
                }
            }
            ready = matches!(active.explorer, ExplorerSlot::Done(_));
        }
        if ready {
            return self.apply_late_order(cx);
        }
        FolderLoadUpdate::default()
    }
}
