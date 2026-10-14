//! Port of `PhotoReview.Core/FileActions/JournalStartupRecovery.cs` (C# baseline d92d6aae, 75 lines): the INV-6 /
//! ADR 0003 startup step. Reconcile every pending journal operation (full scan, `OperationJournal::
//! reconcile_pending_operations`, rules in [`super::reconcile`]), then compact the journal (the quiet moment, same worker
//! thread), and hand the entries reconcile had to mark Failed back to the caller, which offers the Recovery window
//! (PARITY C-27, App wave). Never fails: a locked or unreadable journal is logged and gives an empty list. Decision P03:
//! this step does NOT seed the Undo history (Undo is limited to the current session). Contract: `contracts/W3-03.md`.
//!
//! C# `RunAsync` captures the start time on the caller thread, then runs the journal work on the thread pool so the first
//! image is not delayed. Rust (R0003): [`spawn`] does the same on one short-lived named thread and returns a
//! [`StartupRecoveryHandle`]; [`run`] / [`run_with_cutoff`] are the blocking body for callers already off the UI thread.

// TEMPORARY (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::fmt;
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::abstractions::{Clock, Log};
use crate::file_actions::journal::{JournalCompactionResult, OperationJournal};
use crate::file_actions::journal_line::JournalEntry;
use crate::time::DateTimeUtc;

/// Name of the worker thread [`spawn`] starts (diagnostics only; C# used an anonymous pool thread).
pub const RECOVERY_THREAD_NAME: &str = "PhotoReview.JournalRecovery";

/// C# `await JournalStartupRecovery.RunAsync(journal, clock, log)` without the thread hop: the blocking body. Reads
/// `clock.utc_now()` once, then `run_with_cutoff(journal, that, log)`.
pub fn run(journal: &OperationJournal, clock: &dyn Clock, log: Option<&dyn Log>) -> Vec<JournalEntry> {
    todo!()
}

/// The body of C# `RunAsync` after `startedUtc` was taken. In order:
/// 1. inside `catch_unwind`: `reconciled = journal.reconcile_pending_operations(Some(started_utc))`; on `Ok`, the
///    compaction of step 2 runs on this same thread BEFORE anything is logged here;
/// 2. [`compact`]`(journal, log)`;
/// 3. `failed` = the reconciled entries with `state == Failed`, in order; when `reconciled` is not empty, warn
///    `"Startup journal reconcile: {reconciled.len()} pending operation(s), {failed.len()} failed."`; return `failed`.
///
/// A reconcile `Err` or a panic in step 1 -> `log.error("Startup journal reconcile failed", Some(&err))` (`io::Error`, or
/// a [`PanicError`] holding the panic text), NO compaction, empty result. `log == None` logs nothing (C# `log?.`); the
/// per-entry reconcile warnings go to the journal's own diagnostics log (W3-01 option `log`), not to `log`.
pub fn run_with_cutoff(
    journal: &OperationJournal,
    started_utc: DateTimeUtc,
    log: Option<&dyn Log>,
) -> Vec<JournalEntry> {
    todo!()
}

/// C# `RunAsync` as the App calls it (fire and continue): reads `clock.utc_now()` NOW, on the calling thread, then runs
/// [`run_with_cutoff`] on a new thread named [`RECOVERY_THREAD_NAME`] and returns at once. Never fails: when the thread
/// cannot be spawned, logs `"Startup journal reconcile failed"` with the spawn error and returns a handle whose
/// [`wait`](StartupRecoveryHandle::wait) yields an empty list.
pub fn spawn(journal: Arc<OperationJournal>, clock: &dyn Clock, log: Option<Arc<dyn Log>>) -> StartupRecoveryHandle {
    todo!()
}

/// C# `Compact(journal, log)`: a compaction problem never affects startup (the journal is left as it was and the next
/// start tries again). `journal.try_compact()` inside `catch_unwind`; a panic (C#: an exception other than
/// IO/UnauthorizedAccess/PlatformNotSupported, i.e. a compaction bug) -> `log.error("Journal compaction failed",
/// Some(&PanicError))` and return; otherwise [`log_compaction_result`].
fn compact(journal: &OperationJournal, log: Option<&dyn Log>) {
    todo!()
}

/// The `switch (result.Outcome)` of C# `Compact`:
/// * `Compacted` -> info `"Journal compacted: {bytes_before} -> {bytes_after} bytes."` (plain integers, no grouping);
/// * `Failed` -> warn `"Journal compaction failed (journal unchanged): {error}"` (`None` error = empty text, C# string
///   concatenation with null);
/// * `Busy` / `Changed` -> info `"Journal compaction skipped ({name}); journal unchanged."` with `name` =
///   W3-01 `JournalCompactionOutcome::as_str` (C# `Outcome.ToString()`, the variant name);
/// * `NotSupported` / `BelowThreshold` / `NotWorthIt` -> nothing.
fn log_compaction_result(result: &JournalCompactionResult, log: Option<&dyn Log>) {
    todo!()
}

/// A caught panic as the `Error` handed to `Log::error` (C#: the exception object). Display = the panic payload text
/// (`abstractions::panic_message`), so `RecordingLog` prints `"{message}: {payload}"`.
#[derive(Debug)]
pub struct PanicError(pub String);

impl fmt::Display for PanicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PanicError {}

/// The running startup recovery (C#: the `Task<IReadOnlyList<JournalEntry>>`).
#[derive(Debug)]
pub struct StartupRecoveryHandle {
    /// `None`: the thread could not be spawned (the result is empty).
    thread: Option<JoinHandle<Vec<JournalEntry>>>,
}

impl StartupRecoveryHandle {
    /// C# `Task.IsCompleted`. True when the work finished (or never started).
    pub fn is_finished(&self) -> bool {
        todo!()
    }

    /// Waits for the result (C# `await`): the entries reconcile marked Failed. Never call it on the UI thread. A panic
    /// that escaped the worker (impossible by construction: [`run_with_cutoff`] catches them) yields an empty list.
    pub fn wait(self) -> Vec<JournalEntry> {
        todo!()
    }
}
