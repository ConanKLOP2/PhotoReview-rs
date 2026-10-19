//! Port of `PhotoReview.App.Shared/Coordinators/DuplicateCleanupController.cs` (305 lines): duplicate check, review dialog,
//! batch recycle, reload, and "Clear cache" (contract `contracts/W8-07f.md`). Target: `coordinators::duplicate_cleanup`.
//!
//! D-W8-2: the C# `async` methods become event-driven steps. Hashing (`DuplicateFinder::find`), each recycle and the disk-cache
//! clear run on a [`WorkerPort`]; their results return as [`DuplicateCleanupEvent`]s to `handle_event` on the UI thread. The C#
//! `IUiScheduler.InvokeAsync(ShowBatchReview)` hop disappears: the continuation already runs on the UI thread, so the dialog
//! is called directly and the `IUiScheduler` dependency is dropped. The catalog and the sink are borrowed per call (D-W8-1).
//! The file-action gate is held by the CALLER (view model), as for `FileActionController`.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::fmt;
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{DialogService, FileSystem, Log};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::file_actions::types::FileActionResult;
use pr_core::sync::CancelToken;
use pr_imaging::cache::thumbnails::ThumbnailCache;
use pr_imaging::service::PreviewImageService;

use crate::coordinators::file_actions::recycle::TextSource;
use crate::coordinators::folder_load::state::{WorkerLane, WorkerPort};
use crate::coordinators::traits::{IDuplicateCleanupSink, IFileActionExecutor, IPreloadController};
use crate::misc::file_hash::FileHasher;

/// What every call needs from the shell, borrowed per call.
pub struct DuplicateCleanupCx<'a> {
    pub catalog: &'a mut ReviewCatalog,
    pub sink: &'a mut dyn IDuplicateCleanupSink,
}

impl fmt::Debug for DuplicateCleanupCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DuplicateCleanupCx").finish_non_exhaustive()
    }
}

/// How far an entry point got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateCleanupStep {
    /// Worker job(s) or a folder reload are outstanding: wait for `handle_event` / `on_reload_finished`.
    Pending,
    /// The operation is over (the C# task completed normally, including every early `return`). The caller releases its gate.
    Done,
    /// C# `OperationCanceledException` escaped from `ClearCacheAsync` (a cancelled disk clear is NOT reported as a failure).
    Canceled,
}

/// Why hashing ended (decided on the worker; classified on the UI thread, see `handle_event`).
#[derive(Debug)]
pub enum HashOutcome {
    /// `DuplicateFinder::find` returned the files to remove (possibly empty).
    Found(Vec<String>),
    /// `find` returned `Canceled`: the token fired, the folder generation moved (the hash closure checks it per file), or the
    /// token was canceled just after the last hash (C# `cts.Token.ThrowIfCancellationRequested()`).
    Canceled,
    /// No hash service while a hash was needed (C# `UserFacingError.Localized(InvalidOperationException, ErrIoHashServiceMissing)`).
    HashServiceMissing,
    /// Any other failure; the text is `UserFacingError.Describe`.
    Failed(String),
}

/// One recycle's outcome (C# `Task.Run(() => ExecuteAsync(request))`).
#[derive(Debug)]
pub enum RecycleOutcome {
    Result(FileActionResult),
    Canceled,
    /// C# unexpected exception; text = `UserFacingError.Describe`.
    Panicked(String),
}

/// The disk part of "Clear cache" (C# `Task.Run(ClearDiskCaches)`).
#[derive(Debug)]
pub enum CacheClearOutcome {
    Cleared,
    Canceled,
    Panicked(String),
}

/// Worker results delivered on the UI thread; `ticket` identifies the run (a stale ticket is dropped).
#[derive(Debug)]
pub enum DuplicateCleanupEvent {
    HashFinished {
        ticket: u64,
        outcome: HashOutcome,
    },
    Recycled {
        ticket: u64,
        index: usize,
        outcome: RecycleOutcome,
    },
    CacheCleared {
        ticket: u64,
        outcome: CacheClearOutcome,
    },
}

/// The collaborators of the C# constructor (`catalog` and `sink` arrive per call; `IUiScheduler` is dropped, see module doc).
pub struct DuplicateCleanupDeps {
    pub clock: Arc<GenerationClock>,
    /// `None`: `remove_duplicates` returns at once without any status (C# `_fileActions is null`).
    pub file_actions: Option<Arc<dyn IFileActionExecutor + Send + Sync>>,
    pub hash_service: Option<Arc<dyn FileHasher>>,
    pub file_system: Arc<dyn FileSystem>,
    pub dialog_service: Option<Arc<dyn DialogService>>,
    pub preload_controller: Option<Arc<dyn IPreloadController>>,
    pub thumbnail_cache: Option<Arc<ThumbnailCache>>,
    pub preview_service: Option<Arc<PreviewImageService>>,
    /// C# `ClearDiskCaches` (test seam). `None` = the default: `thumbnail_cache.clear_disk()` then `preview_service.clear_disk()`.
    pub clear_disk_caches: Option<Arc<dyn Fn() + Send + Sync>>,
    pub worker: Arc<dyn WorkerPort<DuplicateCleanupEvent>>,
    pub texts: Arc<dyn TextSource>,
    pub log: Arc<dyn Log>,
}

impl fmt::Debug for DuplicateCleanupDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DuplicateCleanupDeps").finish_non_exhaustive()
    }
}

/// The check that is hashing right now (C# `_hashCts`, guarded by `_hashGate`).
#[derive(Debug)]
pub(crate) struct HashSlot {
    pub ticket: u64,
    pub token: CancelToken,
}

/// The recycle batch in progress (the C# locals of the `foreach` loop).
#[derive(Debug)]
pub(crate) struct Batch {
    pub ticket: u64,
    /// The files to recycle, after the review dialog.
    pub remove: Vec<String>,
    /// C# `actionFolderGeneration`, read after `stop_for_action`.
    pub action_folder_generation: i64,
    /// Index of the file whose job is running.
    pub index: usize,
    pub succeeded: i32,
    /// `"<file name>: <error>"` lines.
    pub failures: Vec<String>,
    /// Set after a successful `reload_folder`: the final status waits for `on_reload_finished`.
    pub awaiting_reload: bool,
}

/// C# `DuplicateCleanupController`. Owned by the UI thread; not `Send`.
pub struct DuplicateCleanupController {
    pub(crate) deps: DuplicateCleanupDeps,
    /// C# `_hashGate` + `_hashCts`.
    pub(crate) hash_gate: Mutex<Option<HashSlot>>,
    pub(crate) batch: Option<Batch>,
    /// The folder generation captured before hashing (C# `preHashGeneration`) of the run in progress.
    pub(crate) pre_hash_generation: i64,
    pub(crate) remove_numbered: bool,
    pub(crate) next_ticket: u64,
    /// Ticket of the run (check/batch or clear cache) in progress.
    pub(crate) active_ticket: Option<u64>,
}

impl fmt::Debug for DuplicateCleanupController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DuplicateCleanupController")
            .field("batch_running", &self.batch.is_some())
            .field("active_ticket", &self.active_ticket)
            .finish_non_exhaustive()
    }
}

impl DuplicateCleanupController {
    /// C# constructor. Stores the collaborators, starts nothing.
    pub fn new(deps: DuplicateCleanupDeps) -> Self {
        todo!()
    }

    /// C# `CancelDuplicateCheck()` (Q-R25): cancels the token of a check that is HASHING; true while one is hashing (so Esc
    /// consumes the key), false otherwise, in particular once hashing ended. Thread-safe (`&self`, the gate is a `Mutex`).
    pub fn cancel_duplicate_check(&self) -> bool {
        todo!()
    }

    /// C# `RemoveDuplicatesAsync(removeNumbered)` up to the hash job: returns `Done` without any status when there is no
    /// executor, the executor is busy, or the catalog is empty. Else `pre_hash_generation = clock.current_folder()`,
    /// `candidates = catalog.paths()`, a new token replaces (and cancels) a previous one, status `DuplicateCheckRunning`, and
    /// the hash job (`pr_core::file_actions::duplicates::find`, then `token.check()`) is submitted on `WorkerLane::Hashing`.
    pub fn remove_duplicates(
        &mut self,
        cx: &mut DuplicateCleanupCx<'_>,
        remove_numbered: bool,
    ) -> DuplicateCleanupStep {
        todo!()
    }

    /// Delivers a worker result (UI thread); a stale ticket is dropped (returns `Pending`).
    pub fn handle_event(
        &mut self,
        cx: &mut DuplicateCleanupCx<'_>,
        event: DuplicateCleanupEvent,
    ) -> DuplicateCleanupStep {
        todo!()
    }

    /// The shell reports the end of the folder load that `IDuplicateCleanupSink::reload_folder` started (this replaces the
    /// `await ReloadFolderAsync` of the C#; open question O1). `still_current` = no other folder load started meanwhile (C# bool
    /// result). `false` -> `show_late_action_status(StatusLateBatchRecycled(done, failed))`; `true` -> status `BatchDone` and the
    /// error dialog, exactly as when no reload was needed.
    pub fn on_reload_finished(&mut self, cx: &mut DuplicateCleanupCx<'_>, still_current: bool) -> DuplicateCleanupStep {
        todo!()
    }

    /// C# `ClearCacheAsync()`: a dialog service asks `DialogClearCache*` first (declined -> `Done`). Then, in this order:
    /// `preload.cancel()`, `thumbnail_cache.clear_memory()`, `preview_service.clear_cache()`,
    /// `preview_service.clear_source_bytes_cache()`, `preload.clear_preloaded_keys()`, and the disk clear is submitted on
    /// `WorkerLane::CacheClear`; the status `CacheCleared` is written when `CacheCleared{Cleared}` arrives.
    pub fn clear_cache(&mut self, cx: &mut DuplicateCleanupCx<'_>) -> DuplicateCleanupStep {
        todo!()
    }

    /// Continuation of the hash job (C# lines 126 to 202): classifies the outcome (folder changed first, then user cancel /
    /// superseded, then failure), re-checks the folder generation, handles "no duplicates", the up-front no-recycle-bin refusal
    /// (batch never deletes permanently, Q-R14), the review dialog with its R7-4 re-check and the catalog re-filter.
    pub(crate) fn on_hash_finished(
        &mut self,
        cx: &mut DuplicateCleanupCx<'_>,
        ticket: u64,
        outcome: HashOutcome,
    ) -> DuplicateCleanupStep {
        todo!()
    }

    /// C# lines 204 to 214: `clock.stop_for_action()`, `preload.cancel()`, `action_folder_generation = clock.current_folder()`,
    /// then the first recycle job.
    pub(crate) fn start_batch(&mut self, cx: &mut DuplicateCleanupCx<'_>, remove: Vec<String>) -> DuplicateCleanupStep {
        todo!()
    }

    /// C# loop body (lines 213 to 239) for one `Recycled` event; evicts the recycled file from the preview/preload caches
    /// (`preview_service.evict_cached_path(path, Some(preload.remove_preloaded_keys_for_path))`; an `Err` is logged and the batch
    /// continues, open question O3), then submits the next job or calls `finish_batch`.
    pub(crate) fn on_recycled(
        &mut self,
        cx: &mut DuplicateCleanupCx<'_>,
        ticket: u64,
        index: usize,
        outcome: RecycleOutcome,
    ) -> DuplicateCleanupStep {
        todo!()
    }

    /// C# lines 242 to 272: folder no longer the action folder -> `show_late_action_status(StatusLateBatchRecycled(done, failed))`
    /// when anything happened, nothing else; else reload first (`sink.reload_folder(parent of the first file)`, only when
    /// something succeeded) because opening a folder clears the status line, then status `BatchDone(done, failed)` and, with
    /// failures and a dialog service, `show_error(DialogBatchErrorsTitle, failures joined with CRLF (carriage return, line feed))`.
    pub(crate) fn finish_batch(&mut self, cx: &mut DuplicateCleanupCx<'_>) -> DuplicateCleanupStep {
        todo!()
    }
}
