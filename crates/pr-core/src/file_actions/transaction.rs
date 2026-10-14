//! Port of `PhotoReview.Core/FileActions/JournalTransaction.cs` (C# baseline d92d6aae, 221 lines): the shared
//! Prepared -> mutate -> verify -> Committed/Failed journal sequence (CORE-07) used by `FileActionService` (W3-04),
//! `UndoService` (W3-05) and `RecoveryRetryService` (W3-06), so a fix applies to all of them. PARITY F-11, F-12, F-15,
//! INV-6. Contract: `contracts/W3-01.md`.
//!
//! Not thread-safe: one instance per operation, owned by the thread that runs the operation (`&mut self` everywhere).
//!
//! Q-R27: the operation's live marker ([`OperationJournal::live_operations`]) is taken just before Prepared is appended
//! and released right after the outcome (Committed/Failed/Dismissed) is appended, so another process's startup reconcile
//! never judges it mid-flight. Dropping the transaction releases it on any path that ends without an outcome (C#
//! `Dispose`).
//!
//! C# `BeginAsync` (PowerLossSafe: `Task.Run(Begin)`) has no twin: Rust calls are blocking (R0003) and every caller
//! already runs the whole operation off the UI thread; W3-04 decides which thread calls [`JournalTransaction::begin`].
#![allow(unused_variables, dead_code)] // D11: removed by the porter together with the last todo!()

use std::error::Error;
use std::fmt;
use std::io;

use crate::abstractions::{Clock, FileStat, FileSystem, LiveMarker};
use crate::file_actions::journal::OperationJournal;
use crate::file_actions::journal_line::JournalEntry;

/// Message of the error [`JournalTransaction::begin`] returns for a superseded retry (C# `InvalidOperationException`).
pub const SUPERSEDED_MESSAGE: &str = "The retried journal entry was superseded by another writer.";

/// C# `InvalidOperationException("The retried journal entry was superseded by another writer.")` thrown by `Begin`.
/// Returned inside an `io::Error` of kind `Other` (`io::Error::other(TransactionSuperseded)`); callers recognise it with
/// `err.get_ref().is_some_and(|e| e.is::<TransactionSuperseded>())`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransactionSuperseded;

impl fmt::Display for TransactionSuperseded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(SUPERSEDED_MESSAGE)
    }
}

impl Error for TransactionSuperseded {}

/// C# `internal sealed class JournalTransaction : IDisposable`.
pub struct JournalTransaction<'a> {
    journal: &'a OperationJournal,
    /// The caller's clock (C# `_clock`): stamps Committed / Failed / Dismissed. NOT the journal's clock.
    clock: &'a dyn Clock,
    prepared: JournalEntry,
    /// Retry semantics (P02): the existing Failed record being re-attempted.
    retry_of: Option<JournalEntry>,
    live_marker: Option<LiveMarker>,
    superseded: bool,
    is_prepared: bool,
    mutation_completed: bool,
}

impl fmt::Debug for JournalTransaction<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JournalTransaction")
            .field("id", &self.prepared.id)
            .field("retry", &self.retry_of.is_some())
            .field("live", &self.live_marker.is_some())
            .field("superseded", &self.superseded)
            .field("is_prepared", &self.is_prepared)
            .field("mutation_completed", &self.mutation_completed)
            .finish()
    }
}

impl<'a> JournalTransaction<'a> {
    /// C# `new JournalTransaction(journal, clock, prepared, retryOf)`. Nothing is read, written or marked live here.
    ///
    /// `retry_of` (RecoveryRetryService): the Failed record being re-attempted. A failure is then journaled even when the
    /// Prepared append itself failed, and every append except Committed is conditional
    /// ([`OperationJournal::append_if_unchanged_since`]): Prepared only when `retry_of` is still the latest line of the
    /// Id, Failed/Dismissed only when the lines after it are exactly this transaction's own Prepared. Otherwise nothing is
    /// appended and [`superseded`](Self::superseded) becomes true.
    pub fn new(
        journal: &'a OperationJournal,
        clock: &'a dyn Clock,
        prepared: JournalEntry,
        retry_of: Option<JournalEntry>,
    ) -> Self {
        Self {
            journal,
            clock,
            prepared,
            retry_of,
            live_marker: None,
            superseded: false,
            is_prepared: false,
            mutation_completed: false,
        }
    }

    /// The Prepared record this transaction journals (C# `_prepared`).
    pub fn prepared(&self) -> &JournalEntry {
        &self.prepared
    }

    /// C# `Superseded` (P02): a retry's Prepared or Failed append was skipped because another writer appended a record
    /// for this Id after the retried snapshot.
    pub fn superseded(&self) -> bool {
        self.superseded
    }

    /// C# `IsPrepared`: the Prepared record was appended.
    pub fn is_prepared(&self) -> bool {
        self.is_prepared
    }

    /// C# `MutationCompleted`: the file mutation was verified; from then on the operation counts as succeeded.
    pub fn mutation_completed(&self) -> bool {
        self.mutation_completed
    }

    /// C# `Begin()`: [`try_begin`](Self::try_begin); `Ok(false)` becomes `Err(io::Error::other(TransactionSuperseded))`.
    /// An append error is returned unchanged.
    pub fn begin(&mut self) -> io::Result<()> {
        todo!()
    }

    /// C# `TryBegin()`, in this order:
    /// 1. when no marker is held: `marker = journal.live_operations().begin(&prepared.id)` (BEFORE the Prepared line
    ///    exists anywhere; an empty Id makes the in-process registry panic, as C# throws `ArgumentException` outside the
    ///    `try`: a caller bug, never caught here);
    /// 2. retry: `appended = journal.append_if_unchanged_since(retry_of, &[], &prepared)?`; first attempt:
    ///    `journal.append(&prepared)?`, `appended = true`;
    /// 3. an error in step 2 releases the marker and is returned (`is_prepared` stays false);
    /// 4. `!appended` -> `superseded = true`, release the marker, `Ok(false)`;
    /// 5. otherwise `is_prepared = true`, `Ok(true)` (the marker stays until the outcome).
    pub fn try_begin(&mut self) -> io::Result<bool> {
        todo!()
    }

    /// C# `VerifyDestination(IFileSystem, string destination, string errorCode)`:
    /// `self.verify_destination_stat(fs.get_file_stat(Path::new(destination)), error_code)` (ONE stat).
    pub fn verify_destination(&mut self, fs: &dyn FileSystem, destination: &str, error_code: &str) -> io::Result<()> {
        todo!()
    }

    /// C# `VerifyDestination(FileStat?, string errorCode)`: `stat` is `None`, or its length differs from
    /// `prepared.size` (a length above `i64::MAX` never matches) -> `Err` of a
    /// [`JournalCodedError`](super::errors::JournalCodedError)`::new(error_code, localized text)` converted with
    /// `into_io_error()` (message = `errors::text(error_code, &|k| journal.texts().localized(k))`). An unknown code is a
    /// caller bug (callers pass `errors::*` constants only): C# throws `ArgumentException` while building the exception;
    /// Rust uses the code itself as the message (`// DIVERGENCE:` comment, no test, no golden case). Otherwise `mutation_completed = true`. The write time is NOT
    /// compared here.
    pub fn verify_destination_stat(&mut self, stat: Option<FileStat>, error_code: &str) -> io::Result<()> {
        todo!()
    }

    /// C# `VerifyMoved`: `fs.file_exists(source)` -> coded error `MOVE_SOURCE_NOT_REMOVED` (checked FIRST; the destination
    /// is not even stat'ed); otherwise [`verify_destination`](Self::verify_destination)`(fs, destination, size_error_code)`.
    pub fn verify_moved(
        &mut self,
        fs: &dyn FileSystem,
        source: &str,
        destination: &str,
        size_error_code: &str,
    ) -> io::Result<()> {
        todo!()
    }

    /// C# `MarkMutationCompleted()`: for operations with nothing to verify (Recycle).
    pub fn mark_mutation_completed(&mut self) {
        self.mutation_completed = true;
    }

    /// C# `Commit(out string? journalError)`: `committed = prepared` with `state = Committed` and `timestamp_utc =
    /// JournalDateTime::utc(clock.utc_now())` (every other field, including a retry's `error`/`error_code`, kept);
    /// `journal.append(&committed)` UNCONDITIONALLY (also for a retry). An append error does not undo the mutation: it is
    /// returned as `Some(err.to_string())`. The marker is released AFTER the append attempt, on both paths. Returns the
    /// committed record even when its append failed.
    pub fn commit(&mut self) -> (JournalEntry, Option<String>) {
        todo!()
    }

    /// C# `Fail(Exception failure, out string? journalError)`: `(code, text) = errors::for_journal(failure, &|k|
    /// journal.texts().english(k))`, then the shared failure-outcome step with `state = Failed`, `error = Some(text)`,
    /// `error_code = code`.
    pub fn fail(&mut self, failure: &(dyn Error + 'static)) -> (Option<JournalEntry>, Option<String>) {
        todo!()
    }

    /// C# `DismissRolledBack(out string? journalError)`: the failure-outcome step with `state = Dismissed`, `error = None`,
    /// `error_code = None` (a fully rolled-back group: nothing left to retry).
    pub fn dismiss_rolled_back(&mut self) -> (Option<JournalEntry>, Option<String>) {
        todo!()
    }

    /// C# `AppendFailureOutcome(state, errorText, errorCode, out journalError)`, in this order:
    /// 1. `!is_prepared && retry_of.is_none()` -> release the marker, `(None, None)` (the clock is NOT read);
    /// 2. `outcome = prepared` with `state`, `timestamp_utc = utc(clock.utc_now())`, `error`, `error_code`;
    /// 3. retry: `own = if is_prepared { [prepared] } else { [] }`; `Ok(false)` from
    ///    `journal.append_if_unchanged_since(retry_of, own, &outcome)` sets `superseded = true`; first attempt:
    ///    `journal.append(&outcome)`; an `Err` becomes `journal_error = Some(err.to_string())`;
    /// 4. release the marker (after the append attempt, on every path);
    /// 5. `(None, journal_error)` when `superseded` (ALSO when it was already set by `try_begin`) or the append failed,
    ///    else `(Some(outcome), None)`.
    fn append_failure_outcome(
        &mut self,
        state: crate::model::JournalState,
        error_text: Option<String>,
        error_code: Option<String>,
    ) -> (Option<JournalEntry>, Option<String>) {
        todo!()
    }

    /// C# `ReleaseLiveMarker()`: drops the marker (idempotent).
    fn release_live_marker(&mut self) {
        self.live_marker = None;
    }
}

impl Drop for JournalTransaction<'_> {
    /// C# `Dispose()`: releases the live marker if still held.
    fn drop(&mut self) {
        self.release_live_marker();
    }
}
