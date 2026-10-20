//! Port of `PhotoReview.App.Shared/Coordinators/DuplicateCleanupController.cs` (305 lines): duplicate check, review dialog,
//! batch recycle, reload, and "Clear cache" (contract `contracts/W8-07f.md`). Target: `coordinators::duplicate_cleanup`.
//!
//! D-W8-2: the C# `async` methods become event-driven steps. Hashing (`DuplicateFinder::find`), each recycle and the disk-cache
//! clear run on a [`WorkerPort`]; their results return as [`DuplicateCleanupEvent`]s to `handle_event` on the UI thread. The C#
//! `IUiScheduler.InvokeAsync(ShowBatchReview)` hop disappears: the continuation already runs on the UI thread, so the dialog
//! is called directly and the `IUiScheduler` dependency is dropped. The catalog and the sink are borrowed per call (D-W8-1).
//! The file-action gate is held by the CALLER (view model), as for `FileActionController`.

use std::any::Any;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{BatchReviewItem, DialogService, FileSystem, Log};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::file_actions::duplicates::{self, HashFailure};
use pr_core::file_actions::types::{FileActionRequest, FileActionResult};
use pr_core::model::FileOperationType;
use pr_core::sync::{CancelToken, Canceled};
use pr_imaging::cache::thumbnails::ThumbnailCache;
use pr_imaging::service::PreviewImageService;
use pr_l10n::keys;
use pr_l10n::template::LocArg;

use crate::coordinators::file_actions::recycle::TextSource;
use crate::coordinators::folder_load::state::{WorkerLane, WorkerPort};
use crate::coordinators::traits::{IDuplicateCleanupSink, IFileActionExecutor, IPreloadController};
use crate::misc::file_hash::{FileHashError, FileHasher};

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
    /// C# `preHashGeneration` of this run (a superseded run keeps its own).
    pub pre_hash_generation: i64,
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
    pub(crate) next_ticket: u64,
    /// Ticket of the check/batch in progress.
    pub(crate) active_ticket: Option<u64>,
    /// Ticket of the "Clear cache" disk job in progress.
    cache_ticket: Option<u64>,
    /// Runs replaced by a newer check while hashing: `(ticket, pre_hash_generation)`. Their (canceled) result is dropped quietly.
    superseded: Vec<(u64, i64)>,
}

impl fmt::Debug for DuplicateCleanupController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DuplicateCleanupController")
            .field("batch_running", &self.batch.is_some())
            .field("active_ticket", &self.active_ticket)
            .finish_non_exhaustive()
    }
}

/// The text of a caught panic (the Rust image of `UserFacingError.Describe(ex)` for an unexpected exception).
fn describe_panic(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unexpected error".to_string()
    }
}

/// C# `Path.GetFileName`.
fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn lock_gate(gate: &Mutex<Option<HashSlot>>) -> MutexGuard<'_, Option<HashSlot>> {
    gate.lock().unwrap_or_else(PoisonError::into_inner)
}

impl DuplicateCleanupController {
    /// C# constructor. Stores the collaborators, starts nothing.
    pub fn new(deps: DuplicateCleanupDeps) -> Self {
        Self {
            deps,
            hash_gate: Mutex::new(None),
            batch: None,
            next_ticket: 0,
            active_ticket: None,
            cache_ticket: None,
            superseded: Vec::new(),
        }
    }

    /// C# `CancelDuplicateCheck()` (Q-R25): cancels the token of a check that is HASHING; true while one is hashing (so Esc
    /// consumes the key), false otherwise, in particular once hashing ended. Thread-safe (`&self`, the gate is a `Mutex`).
    pub fn cancel_duplicate_check(&self) -> bool {
        let gate = lock_gate(&self.hash_gate);
        match gate.as_ref() {
            Some(slot) => {
                slot.token.cancel();
                true
            }
            None => false,
        }
    }

    fn status(&self, cx: &mut DuplicateCleanupCx<'_>, key: &str, args: &[LocArg]) {
        let text = self.deps.texts.format(key, args);
        cx.sink.set_status_text(&text);
    }

    fn late_status(&self, cx: &mut DuplicateCleanupCx<'_>, succeeded: i32, failed: usize) {
        let text = self.deps.texts.format(
            keys::STATUS_LATE_BATCH_RECYCLED,
            &[
                LocArg::new("done", succeeded),
                LocArg::new("failed", i32::try_from(failed).unwrap_or(i32::MAX)),
            ],
        );
        cx.sink.show_late_action_status(&text);
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
        let Some(executor) = self.deps.file_actions.clone() else {
            return DuplicateCleanupStep::Done;
        };
        if executor.is_busy() {
            return DuplicateCleanupStep::Done;
        }
        // A batch or a cache clear of this controller is still running: never interleave a second run with it.
        if self.batch.is_some() || self.cache_ticket.is_some() {
            return DuplicateCleanupStep::Done;
        }
        if cx.catalog.count() == 0 {
            return DuplicateCleanupStep::Done;
        }

        let pre_hash_generation = self.deps.clock.current_folder();
        let candidates = cx.catalog.paths();
        if candidates.is_empty() {
            return DuplicateCleanupStep::Done;
        }

        self.next_ticket += 1;
        let ticket = self.next_ticket;
        let token = CancelToken::new();
        {
            let mut gate = lock_gate(&self.hash_gate);
            // A fresh check supersedes a previous one that is somehow still hashing (the file-action gate normally prevents it).
            if let Some(previous) = gate.take() {
                previous.token.cancel();
                self.superseded.push((previous.ticket, previous.pre_hash_generation));
            }
            *gate = Some(HashSlot {
                ticket,
                token: token.clone(),
                pre_hash_generation,
            });
        }
        self.active_ticket = Some(ticket);
        self.status(cx, keys::STATUS_DUPLICATE_CHECK_RUNNING, &[]);

        let clock = self.deps.clock.clone();
        let hasher = self.deps.hash_service.clone();
        let file_system = self.deps.file_system.clone();
        let job = Box::new(move || {
            let mut service_missing = false;
            let result = duplicates::find(
                &candidates,
                remove_numbered,
                &mut |path, hash_token| {
                    // Minimize disk reads: once the folder changed the result is discarded anyway, so stop hashing the
                    // remaining same-size files instead of reading them all.
                    if !clock.is_folder_current(pre_hash_generation) {
                        return Err(HashFailure::Canceled);
                    }
                    let Some(hasher) = &hasher else {
                        service_missing = true;
                        return Err(HashFailure::Canceled);
                    };
                    hasher.get(Path::new(path), hash_token).map_err(|error| match error {
                        FileHashError::Canceled => HashFailure::Canceled,
                        other => HashFailure::Unreadable(io::Error::other(other.to_string())),
                    })
                },
                file_system.as_ref(),
                &token,
            );
            let outcome = if service_missing {
                HashOutcome::HashServiceMissing
            } else {
                match result {
                    // Esc pressed just as the last hash finished: honor the user's intent, apply nothing.
                    Ok(list) if token.check().is_ok() => HashOutcome::Found(list),
                    Ok(_) | Err(Canceled) => HashOutcome::Canceled,
                }
            };
            DuplicateCleanupEvent::HashFinished { ticket, outcome }
        });
        self.deps.worker.submit(WorkerLane::Hashing, job);
        DuplicateCleanupStep::Pending
    }

    /// Delivers a worker result (UI thread); a stale ticket is dropped (returns `Pending`).
    pub fn handle_event(
        &mut self,
        cx: &mut DuplicateCleanupCx<'_>,
        event: DuplicateCleanupEvent,
    ) -> DuplicateCleanupStep {
        match event {
            DuplicateCleanupEvent::HashFinished { ticket, outcome } => self.on_hash_finished(cx, ticket, outcome),
            DuplicateCleanupEvent::Recycled { ticket, index, outcome } => self.on_recycled(cx, ticket, index, outcome),
            DuplicateCleanupEvent::CacheCleared { ticket, outcome } => {
                if self.cache_ticket != Some(ticket) {
                    return DuplicateCleanupStep::Pending;
                }
                self.cache_ticket = None;
                match outcome {
                    CacheClearOutcome::Cleared => {
                        self.status(cx, keys::STATUS_CACHE_CLEARED, &[]);
                        DuplicateCleanupStep::Done
                    }
                    CacheClearOutcome::Panicked(text) => {
                        self.deps.log.error("Clear cache failed", None);
                        self.status(
                            cx,
                            keys::STATUS_ACTION_FAILED,
                            &[
                                LocArg::new("actionName", self.deps.texts.get(keys::DIALOG_CLEAR_CACHE_TITLE)),
                                LocArg::new("message", text),
                            ],
                        );
                        DuplicateCleanupStep::Done
                    }
                    // C# lets the `OperationCanceledException` escape: no status.
                    CacheClearOutcome::Canceled => DuplicateCleanupStep::Canceled,
                }
            }
        }
    }

    /// The shell reports the end of the folder load that `IDuplicateCleanupSink::reload_folder` started (this replaces the
    /// `await ReloadFolderAsync` of the C#; open question O1). `still_current` = no other folder load started meanwhile (C# bool
    /// result). `false` -> `show_late_action_status(StatusLateBatchRecycled(done, failed))`; `true` -> status `BatchDone` and the
    /// error dialog, exactly as when no reload was needed.
    pub fn on_reload_finished(&mut self, cx: &mut DuplicateCleanupCx<'_>, still_current: bool) -> DuplicateCleanupStep {
        let awaiting = self.batch.as_ref().is_some_and(|b| b.awaiting_reload);
        if !awaiting {
            return if self.active_ticket.is_none() && self.cache_ticket.is_none() {
                DuplicateCleanupStep::Done
            } else {
                DuplicateCleanupStep::Pending
            };
        }
        if still_current {
            self.report_batch_done(cx)
        } else {
            // D-03: another folder was opened during the reload; the batch is done on disk, tell the user late.
            let (succeeded, failed) = self.batch.as_ref().map_or((0, 0), |b| (b.succeeded, b.failures.len()));
            self.late_status(cx, succeeded, failed);
            self.end_batch()
        }
    }

    fn end_batch(&mut self) -> DuplicateCleanupStep {
        self.batch = None;
        self.active_ticket = None;
        DuplicateCleanupStep::Done
    }

    /// C# `ClearCacheAsync()`: a dialog service asks `DialogClearCache*` first (declined -> `Done`). Then, in this order:
    /// `preload.cancel()`, `thumbnail_cache.clear_memory()`, `preview_service.clear_cache()`,
    /// `preview_service.clear_source_bytes_cache()`, `preload.clear_preloaded_keys()`, and the disk clear is submitted on
    /// `WorkerLane::CacheClear`; the status `CacheCleared` is written when `CacheCleared{Cleared}` arrives.
    pub fn clear_cache(&mut self, _cx: &mut DuplicateCleanupCx<'_>) -> DuplicateCleanupStep {
        // Another run of this controller is in progress (the caller's gate normally prevents it): do not stack a second one.
        if self.active_ticket.is_some() || self.batch.is_some() || self.cache_ticket.is_some() {
            return DuplicateCleanupStep::Done;
        }
        if let Some(dialog) = &self.deps.dialog_service {
            let confirmed = dialog.show_confirmation(
                &self.deps.texts.get(keys::DIALOG_CLEAR_CACHE_TITLE),
                &self.deps.texts.get(keys::DIALOG_CLEAR_CACHE_MESSAGE),
            );
            if !confirmed {
                return DuplicateCleanupStep::Done;
            }
        }

        if let Some(preload) = &self.deps.preload_controller {
            preload.cancel();
        }
        // RAM first (cheap, and it also bumps the epochs so in-flight writers stop persisting) ...
        if let Some(thumbnails) = &self.deps.thumbnail_cache {
            thumbnails.clear_memory();
        }
        if let Some(preview) = &self.deps.preview_service {
            preview.clear_cache();
            // R2-F-16: "Clear cache" also has to drop the source-bytes RAM cache.
            preview.clear_source_bytes_cache();
        }
        if let Some(preload) = &self.deps.preload_controller {
            preload.clear_preloaded_keys();
        }

        // ... then the directory deletes off the UI thread.
        self.next_ticket += 1;
        let ticket = self.next_ticket;
        self.cache_ticket = Some(ticket);
        let clear_disk = self.deps.clear_disk_caches.clone();
        let thumbnails = self.deps.thumbnail_cache.clone();
        let preview = self.deps.preview_service.clone();
        let job = Box::new(move || {
            let run = catch_unwind(AssertUnwindSafe(|| match &clear_disk {
                Some(clear) => clear(),
                None => {
                    if let Some(thumbnails) = &thumbnails {
                        thumbnails.clear_disk();
                    }
                    if let Some(preview) = &preview {
                        preview.clear_disk();
                    }
                }
            }));
            let outcome = match run {
                Ok(()) => CacheClearOutcome::Cleared,
                Err(payload) => CacheClearOutcome::Panicked(describe_panic(payload.as_ref())),
            };
            DuplicateCleanupEvent::CacheCleared { ticket, outcome }
        });
        self.deps.worker.submit(WorkerLane::CacheClear, job);
        DuplicateCleanupStep::Pending
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
        // C# `finally`: the slot is released only if it is still this run's slot.
        let owned = {
            let mut gate = lock_gate(&self.hash_gate);
            if gate.as_ref().is_some_and(|slot| slot.ticket == ticket) {
                gate.take()
            } else {
                None
            }
        };
        let (pre_hash_generation, token_canceled, superseded) = match owned {
            Some(slot) => {
                self.active_ticket = None;
                (slot.pre_hash_generation, slot.token.is_canceled(), false)
            }
            None => match self.superseded.iter().position(|(t, _)| *t == ticket) {
                Some(position) => {
                    let (_, generation) = self.superseded.remove(position);
                    (generation, true, true)
                }
                // A result nobody waits for any more.
                None => return DuplicateCleanupStep::Pending,
            },
        };
        // A superseded run reports to nobody: the newer run is still pending.
        let end = |superseded: bool| {
            if superseded {
                DuplicateCleanupStep::Pending
            } else {
                DuplicateCleanupStep::Done
            }
        };
        let folder_current = self.deps.clock.is_folder_current(pre_hash_generation);

        // Esc pressed between the end of the job and this continuation: honor the user's intent.
        let outcome = match outcome {
            HashOutcome::Found(_) if token_canceled => HashOutcome::Canceled,
            other => other,
        };
        let list = match outcome {
            HashOutcome::Found(list) => list,
            HashOutcome::Canceled => {
                if !folder_current {
                    self.status(cx, keys::STATUS_DUPLICATE_CHECK_CANCELED_FOLDER_CHANGED, &[]);
                } else if token_canceled {
                    // A run superseded by a fresh check stays silent: the newer run owns the status text.
                    if !superseded {
                        self.status(cx, keys::STATUS_DUPLICATE_CHECK_CANCELED, &[]);
                    }
                } else {
                    self.status(
                        cx,
                        keys::STATUS_DUPLICATE_CHECK_FAILED,
                        &[LocArg::new("message", Canceled.to_string())],
                    );
                }
                return end(superseded);
            }
            HashOutcome::HashServiceMissing => {
                let message = self.deps.texts.get(keys::ERR_IO_HASH_SERVICE_MISSING);
                self.status(
                    cx,
                    keys::STATUS_DUPLICATE_CHECK_FAILED,
                    &[LocArg::new("message", message)],
                );
                return end(superseded);
            }
            HashOutcome::Failed(text) => {
                self.status(cx, keys::STATUS_DUPLICATE_CHECK_FAILED, &[LocArg::new("message", text)]);
                return end(superseded);
            }
        };

        // Stale Folder Guard: the folder changed while hashing, drop the result.
        if !folder_current {
            self.status(cx, keys::STATUS_DUPLICATE_CHECK_CANCELED_FOLDER_CHANGED, &[]);
            return DuplicateCleanupStep::Done;
        }
        if list.is_empty() {
            self.status(cx, keys::STATUS_NO_DUPLICATES_FOUND, &[]);
            return DuplicateCleanupStep::Done;
        }
        let Some(executor) = self.deps.file_actions.clone() else {
            return DuplicateCleanupStep::Done;
        };

        // Q-R14: batch cleanup never deletes permanently. Say so once, up front.
        if let Some(without_bin) = list.iter().find(|path| executor.lacks_recycle_bin(path)) {
            self.status(cx, keys::STATUS_BATCH_CANCELED, &[]);
            if let Some(dialog) = &self.deps.dialog_service {
                dialog.show_error(
                    &self.deps.texts.get(keys::DIALOG_BATCH_ERRORS_TITLE),
                    &self.deps.texts.format(
                        keys::CORE_RECYCLE_UNSUPPORTED_DRIVE,
                        &[LocArg::new("name", file_name_of(without_bin))],
                    ),
                );
            }
            return DuplicateCleanupStep::Done;
        }

        let mut remove = list;
        if let Some(dialog) = self.deps.dialog_service.clone() {
            // R7-4: the review dialog is modal; a forwarded open can switch the folder meanwhile.
            let dialog_folder_generation = self.deps.clock.current_folder();
            let items: Vec<BatchReviewItem> = remove
                .iter()
                .map(|path| BatchReviewItem {
                    path: path.into(),
                    length: cx
                        .catalog
                        .find(path)
                        .and_then(|entry| entry.length)
                        .and_then(|length| u64::try_from(length).ok()),
                })
                .collect();
            let confirmed = dialog.show_batch_review(&items);
            if !confirmed {
                self.status(cx, keys::STATUS_BATCH_CANCELED, &[]);
                return DuplicateCleanupStep::Done;
            }
            if !self.deps.clock.is_folder_current(dialog_folder_generation) {
                self.status(cx, keys::STATUS_DUPLICATE_CHECK_CANCELED_FOLDER_CHANGED, &[]);
                return DuplicateCleanupStep::Done;
            }
            if executor.is_busy() {
                return DuplicateCleanupStep::Done;
            }
            remove.retain(|path| cx.catalog.index_of(path) >= 0);
            if remove.is_empty() {
                self.status(cx, keys::STATUS_BATCH_CANCELED, &[]);
                return DuplicateCleanupStep::Done;
            }
        }

        self.active_ticket = Some(ticket);
        self.start_batch(cx, remove)
    }

    /// C# lines 204 to 214: `clock.stop_for_action()`, `preload.cancel()`, `action_folder_generation = clock.current_folder()`,
    /// then the first recycle job.
    pub(crate) fn start_batch(
        &mut self,
        _cx: &mut DuplicateCleanupCx<'_>,
        remove: Vec<String>,
    ) -> DuplicateCleanupStep {
        let Some(ticket) = self.active_ticket else {
            return DuplicateCleanupStep::Done;
        };
        self.deps.clock.stop_for_action();
        if let Some(preload) = &self.deps.preload_controller {
            preload.cancel();
        }
        let action_folder_generation = self.deps.clock.current_folder();
        self.batch = Some(Batch {
            ticket,
            remove,
            action_folder_generation,
            index: 0,
            succeeded: 0,
            failures: Vec::new(),
            awaiting_reload: false,
        });
        self.submit_recycle()
    }

    /// Submits the recycle job of the file at `batch.index`; ends the batch if the executor is gone.
    fn submit_recycle(&mut self) -> DuplicateCleanupStep {
        let (Some(executor), Some(batch)) = (self.deps.file_actions.clone(), self.batch.as_ref()) else {
            return self.end_batch();
        };
        let Some(path) = batch.remove.get(batch.index).cloned() else {
            return self.end_batch();
        };
        let (ticket, index) = (batch.ticket, batch.index);
        let job = Box::new(move || {
            // ADR 0005: the executor journals (fsync) before it acts; keep that off the UI thread. A panic is one file's failure.
            let run = catch_unwind(AssertUnwindSafe(|| {
                executor.execute(
                    &FileActionRequest::new(path, FileOperationType::Recycle),
                    &CancelToken::none(),
                )
            }));
            let outcome = match run {
                Ok(result) => RecycleOutcome::Result(result),
                Err(payload) => RecycleOutcome::Panicked(describe_panic(payload.as_ref())),
            };
            DuplicateCleanupEvent::Recycled { ticket, index, outcome }
        });
        self.deps.worker.submit(WorkerLane::FileAction, job);
        DuplicateCleanupStep::Pending
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
        let Some(batch) = self.batch.as_mut() else {
            return DuplicateCleanupStep::Pending;
        };
        if batch.ticket != ticket || batch.index != index || batch.awaiting_reload {
            return DuplicateCleanupStep::Pending;
        }
        let Some(path) = batch.remove.get(index).cloned() else {
            return self.end_batch();
        };
        match outcome {
            RecycleOutcome::Result(result) if result.succeeded => {
                batch.succeeded += 1;
                // The file is in the Recycle Bin: drop its previews/source bytes from RAM and its preload keys.
                if let Some(preview) = &self.deps.preview_service {
                    let preload = self.deps.preload_controller.clone();
                    let evicted = preview.evict_cached_path(
                        &path,
                        Some(&|normalized: &str| {
                            if let Some(preload) = &preload {
                                preload.remove_preloaded_keys_for_path(normalized);
                            }
                        }),
                    );
                    if let Err(error) = evicted {
                        // D-07f-2: the file is already in the Recycle Bin; a cache error must not abort the batch.
                        self.deps.log.error(
                            &format!("Duplicate cleanup cache eviction failed: {path}"),
                            Some(&error),
                        );
                    }
                }
            }
            RecycleOutcome::Result(result) => {
                batch
                    .failures
                    .push(format!("{}: {}", file_name_of(&path), result.error.unwrap_or_default()));
            }
            RecycleOutcome::Panicked(text) => {
                // One unexpected throw must not abort the batch (the rest of the files, the reload and the summary).
                self.deps
                    .log
                    .error(&format!("Duplicate cleanup recycle threw: {path}"), None);
                batch.failures.push(format!("{}: {text}", file_name_of(&path)));
            }
            // C# lets the `OperationCanceledException` escape: the batch stops, nothing is reported as a file failure.
            RecycleOutcome::Canceled => {
                self.batch = None;
                self.active_ticket = None;
                return DuplicateCleanupStep::Canceled;
            }
        }

        batch.index += 1;
        if batch.index < batch.remove.len() {
            self.submit_recycle()
        } else {
            self.finish_batch(cx)
        }
    }

    /// C# lines 242 to 272: folder no longer the action folder -> `show_late_action_status(StatusLateBatchRecycled(done, failed))`
    /// when anything happened, nothing else; else reload first (`sink.reload_folder(parent of the first file)`, only when
    /// something succeeded) because opening a folder clears the status line, then status `BatchDone(done, failed)` and, with
    /// failures and a dialog service, `show_error(DialogBatchErrorsTitle, failures joined with CRLF (carriage return, line feed))`.
    pub(crate) fn finish_batch(&mut self, cx: &mut DuplicateCleanupCx<'_>) -> DuplicateCleanupStep {
        let Some(batch) = self.batch.as_ref() else {
            return DuplicateCleanupStep::Done;
        };
        let (succeeded, failed) = (batch.succeeded, batch.failures.len());
        if !self.deps.clock.is_folder_current(batch.action_folder_generation) {
            // RV-A10 / RV-D5: the files are already in the Recycle Bin; the batch is never registered for Undo and nothing is
            // reloaded or touched in the folder the user is looking at now.
            if succeeded > 0 || failed > 0 {
                self.late_status(cx, succeeded, failed);
            }
            return self.end_batch();
        }

        // Reload first: opening a folder clears the status line, which would wipe the result of the batch.
        if succeeded > 0 {
            let folder = batch
                .remove
                .first()
                .and_then(|first| Path::new(first).parent())
                .map(|parent| parent.to_string_lossy().into_owned())
                .filter(|parent| !parent.is_empty());
            if let Some(folder) = folder {
                if !cx.sink.reload_folder(&folder) {
                    self.late_status(cx, succeeded, failed);
                    return self.end_batch();
                }
                if let Some(batch) = self.batch.as_mut() {
                    batch.awaiting_reload = true;
                }
                return DuplicateCleanupStep::Pending;
            }
        }
        self.report_batch_done(cx)
    }

    /// The last lines of the C# method: status `BatchDone`, then the failure list in one error dialog.
    fn report_batch_done(&mut self, cx: &mut DuplicateCleanupCx<'_>) -> DuplicateCleanupStep {
        let (succeeded, failures) = self
            .batch
            .as_ref()
            .map_or((0, Vec::new()), |b| (b.succeeded, b.failures.clone()));
        self.status(
            cx,
            keys::STATUS_BATCH_DONE,
            &[
                LocArg::new("succeeded", succeeded),
                LocArg::new("failures", i32::try_from(failures.len()).unwrap_or(i32::MAX)),
            ],
        );
        if !failures.is_empty()
            && let Some(dialog) = &self.deps.dialog_service
        {
            dialog.show_error(
                &self.deps.texts.get(keys::DIALOG_BATCH_ERRORS_TITLE),
                &failures.join("\r\n"),
            );
        }
        self.end_batch()
    }
}
