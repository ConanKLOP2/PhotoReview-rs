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

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::abstractions::{Clock, Log, panic_message};
use crate::file_actions::journal::{JournalCompactionOutcome, JournalCompactionResult, OperationJournal};
use crate::file_actions::journal_line::JournalEntry;
use crate::model::JournalState;
use crate::time::DateTimeUtc;

/// Name of the worker thread [`spawn`] starts (diagnostics only; C# used an anonymous pool thread).
pub const RECOVERY_THREAD_NAME: &str = "PhotoReview.JournalRecovery";

/// C# `await JournalStartupRecovery.RunAsync(journal, clock, log)` without the thread hop: the blocking body. Reads
/// `clock.utc_now()` once, then `run_with_cutoff(journal, that, log)`.
pub fn run(journal: &OperationJournal, clock: &dyn Clock, log: Option<&dyn Log>) -> Vec<JournalEntry> {
    run_with_cutoff(journal, clock.utc_now(), log)
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
    // Everything C# `RunAsync` has inside its `try`, the summary warning included: a `Log` whose `warn` panics is
    // caught here like C#'s `catch (Exception)` (logged as a reconcile failure, empty result), never escaping.
    let attempt = catch_unwind(AssertUnwindSafe(|| {
        let reconciled = journal.reconcile_pending_operations(Some(started_utc))?;
        // The quiet moment: after reconcile appended its outcomes, still on this worker thread.
        compact(journal, log);
        Ok::<_, std::io::Error>(summarize(reconciled, log))
    }));
    match attempt {
        Ok(Ok(failed)) => failed,
        Ok(Err(err)) => {
            // A locked/unreadable journal must not take the viewer down; the Recovery window can still be opened.
            if let Some(log) = log {
                log.error("Startup journal reconcile failed", Some(&err));
            }
            Vec::new()
        }
        Err(payload) => {
            if let Some(log) = log {
                let err = PanicError(panic_message(payload.as_ref()));
                log.error("Startup journal reconcile failed", Some(&err));
            }
            Vec::new()
        }
    }
}

/// The tail of C# `RunAsync`'s `try`: keep the Failed entries (in order) and, when anything was reconciled, warn
/// `"Startup journal reconcile: {total} pending operation(s), {failed} failed."`.
fn summarize(reconciled: Vec<JournalEntry>, log: Option<&dyn Log>) -> Vec<JournalEntry> {
    let total = reconciled.len();
    let failed: Vec<JournalEntry> = reconciled
        .into_iter()
        .filter(|entry| entry.state == JournalState::Failed)
        .collect();
    if total > 0 {
        if let Some(log) = log {
            log.warn(&format!(
                "Startup journal reconcile: {total} pending operation(s), {} failed.",
                failed.len()
            ));
        }
    }
    failed
}

/// C# `RunAsync` as the App calls it (fire and continue): reads `clock.utc_now()` NOW, on the calling thread, then runs
/// [`run_with_cutoff`] on a new thread named [`RECOVERY_THREAD_NAME`] and returns at once. Never fails: when the thread
/// cannot be spawned, logs `"Startup journal reconcile failed"` with the spawn error and returns a handle whose
/// [`wait`](StartupRecoveryHandle::wait) yields an empty list.
pub fn spawn(journal: Arc<OperationJournal>, clock: &dyn Clock, log: Option<Arc<dyn Log>>) -> StartupRecoveryHandle {
    // The cutoff is the moment startup began, read here, not when the worker gets scheduled.
    let started_utc = clock.utc_now();
    let worker_log = log.clone();
    let spawned = std::thread::Builder::new()
        .name(RECOVERY_THREAD_NAME.to_owned())
        .spawn(move || run_with_cutoff(&journal, started_utc, worker_log.as_deref()));
    match spawned {
        Ok(thread) => StartupRecoveryHandle { thread: Some(thread) },
        Err(err) => {
            if let Some(log) = log.as_deref() {
                log.error("Startup journal reconcile failed", Some(&err));
            }
            StartupRecoveryHandle { thread: None }
        }
    }
}

/// C# `Compact(journal, log)`: a compaction problem never affects startup (the journal is left as it was and the next
/// start tries again). `journal.try_compact()` inside `catch_unwind`; a panic (C#: an exception other than
/// IO/UnauthorizedAccess/PlatformNotSupported, i.e. a compaction bug) -> `log.error("Journal compaction failed",
/// Some(&PanicError))` and return; otherwise [`log_compaction_result`].
fn compact(journal: &OperationJournal, log: Option<&dyn Log>) {
    match catch_unwind(AssertUnwindSafe(|| journal.try_compact())) {
        Ok(result) => log_compaction_result(&result, log),
        Err(payload) => {
            // Startup must not lose the reconcile result over an unexpected compaction bug.
            if let Some(log) = log {
                let err = PanicError(panic_message(payload.as_ref()));
                log.error("Journal compaction failed", Some(&err));
            }
        }
    }
}

/// The `switch (result.Outcome)` of C# `Compact`:
/// * `Compacted` -> info `"Journal compacted: {bytes_before} -> {bytes_after} bytes."` (plain integers, no grouping);
/// * `Failed` -> warn `"Journal compaction failed (journal unchanged): {error}"` (`None` error = empty text, C# string
///   concatenation with null);
/// * `Busy` / `Changed` -> info `"Journal compaction skipped ({name}); journal unchanged."` with `name` =
///   W3-01 `JournalCompactionOutcome::as_str` (C# `Outcome.ToString()`, the variant name);
/// * `NotSupported` / `BelowThreshold` / `NotWorthIt` -> nothing.
fn log_compaction_result(result: &JournalCompactionResult, log: Option<&dyn Log>) {
    let Some(log) = log else {
        return;
    };
    match result.outcome {
        JournalCompactionOutcome::Compacted => log.info(&format!(
            "Journal compacted: {} -> {} bytes.",
            result.bytes_before, result.bytes_after
        )),
        JournalCompactionOutcome::Failed => log.warn(&format!(
            "Journal compaction failed (journal unchanged): {}",
            result.error.as_deref().unwrap_or_default()
        )),
        JournalCompactionOutcome::Busy | JournalCompactionOutcome::Changed => log.info(&format!(
            "Journal compaction skipped ({}); journal unchanged.",
            result.outcome.as_str()
        )),
        JournalCompactionOutcome::NotSupported
        | JournalCompactionOutcome::BelowThreshold
        | JournalCompactionOutcome::NotWorthIt => {}
    }
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
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Waits for the result (C# `await`): the entries reconcile marked Failed. Never call it on the UI thread. A panic
    /// that escaped the worker (impossible by construction: [`run_with_cutoff`] catches them) yields an empty list.
    pub fn wait(self) -> Vec<JournalEntry> {
        self.thread
            .map(|thread| thread.join().unwrap_or_default())
            .unwrap_or_default()
    }
}
