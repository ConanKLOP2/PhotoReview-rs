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

use std::error::Error;
use std::fmt;
use std::io;

use crate::abstractions::{Clock, FileStat, FileSystem, LiveMarker};
use crate::file_actions::case_only_rename::CaseOnlyRename;
use crate::file_actions::errors::{self, JournalCodedError};
use crate::file_actions::journal::OperationJournal;
use crate::file_actions::journal_line::{JournalDateTime, JournalEntry};
use crate::model::JournalState;

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
        if self.try_begin()? {
            Ok(())
        } else {
            Err(io::Error::other(TransactionSuperseded))
        }
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
        // Before the Prepared line exists anywhere: a reconcile that can read it can also see the marker.
        if self.live_marker.is_none() {
            self.live_marker = Some(self.journal.live_operations().begin(&self.prepared.id));
        }
        let appended = match &self.retry_of {
            Some(retry_of) => self.journal.append_if_unchanged_since(retry_of, &[], &self.prepared),
            None => self.journal.append(&self.prepared).map(|()| true),
        };
        let appended = match appended {
            Ok(appended) => appended,
            Err(err) => {
                self.release_live_marker(); // nothing pending was written (or a retry's Failed follows)
                return Err(err);
            }
        };
        if !appended {
            self.superseded = true;
            self.release_live_marker();
            return Ok(false);
        }
        self.is_prepared = true;
        Ok(true)
    }

    /// C# `VerifyDestination(IFileSystem, string destination, string errorCode)`:
    /// `self.verify_destination_stat(fs.try_get_file_stat(Path::new(destination))?, error_code)` (ONE stat; P-12: a stat
    /// that cannot be inspected is returned as the error, like the C# `GetFileStat` throw).
    pub fn verify_destination(&mut self, fs: &dyn FileSystem, destination: &str, error_code: &str) -> io::Result<()> {
        let stat = fs.try_get_file_stat(std::path::Path::new(destination))?;
        self.verify_destination_stat(stat, error_code)
    }

    /// C# `VerifyDestination(FileStat?, string errorCode)`: `stat` is `None`, or its length differs from
    /// `prepared.size` (a length above `i64::MAX` never matches) -> `Err` of a
    /// [`JournalCodedError`](super::errors::JournalCodedError)`::new(error_code, localized text)` converted with
    /// `into_io_error()` (message = `errors::text(error_code, &|k| journal.texts().localized(k))`). An unknown code is a
    /// caller bug (callers pass `errors::*` constants only): C# throws `ArgumentException` while building the exception;
    /// Rust uses the code itself as the message (`// DIVERGENCE:` comment, no test, no golden case). Otherwise `mutation_completed = true`. The write time is NOT
    /// compared here.
    pub fn verify_destination_stat(&mut self, stat: Option<FileStat>, error_code: &str) -> io::Result<()> {
        let size_matches =
            stat.is_some_and(|s| i64::try_from(s.length).is_ok_and(|length| length == self.prepared.size));
        if !size_matches {
            return Err(self.coded_error(error_code));
        }
        self.mutation_completed = true;
        Ok(())
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
        if CaseOnlyRename::exists_as(fs, source, Some(destination))? {
            return Err(self.coded_error(errors::MOVE_SOURCE_NOT_REMOVED));
        }
        self.verify_destination(fs, destination, size_error_code)
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
        // DIVERGENCE: C# also catches a THROWING clock here (JournalCommitClockFailureTests); `Clock::utc_now` cannot
        // fail (T20), so there is no such path.
        let committed = JournalEntry {
            state: JournalState::Committed,
            timestamp_utc: JournalDateTime::utc(self.clock.utc_now()),
            ..self.prepared.clone()
        };
        let journal_error = self.journal.append(&committed).err().map(|err| err.to_string());
        self.release_live_marker(); // only after the outcome line: until then the entry is Prepared and must look live
        (committed, journal_error)
    }

    /// C# `Fail(Exception failure, out string? journalError)`: `(code, text) = errors::for_journal(failure, &|k|
    /// journal.texts().english(k))`, then the shared failure-outcome step with `state = Failed`, `error = Some(text)`,
    /// `error_code = code`.
    pub fn fail(&mut self, failure: &(dyn Error + 'static)) -> (Option<JournalEntry>, Option<String>) {
        let texts = self.journal.texts();
        let (code, text) = errors::for_journal(failure, &|key| texts.english(key));
        self.append_failure_outcome(JournalState::Failed, Some(text), code.map(str::to_owned))
    }

    /// C# `DismissRolledBack(out string? journalError)`: the failure-outcome step with `state = Dismissed`, `error = None`,
    /// `error_code = None` (a fully rolled-back group: nothing left to retry).
    pub fn dismiss_rolled_back(&mut self) -> (Option<JournalEntry>, Option<String>) {
        self.append_failure_outcome(JournalState::Dismissed, None, None)
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
        if !self.is_prepared && self.retry_of.is_none() {
            self.release_live_marker();
            return (None, None);
        }
        let outcome = JournalEntry {
            state,
            timestamp_utc: JournalDateTime::utc(self.clock.utc_now()),
            error: error_text,
            error_code,
            ..self.prepared.clone()
        };
        let appended = match &self.retry_of {
            Some(retry_of) => {
                let own: &[JournalEntry] = if self.is_prepared {
                    std::slice::from_ref(&self.prepared)
                } else {
                    &[]
                };
                self.journal.append_if_unchanged_since(retry_of, own, &outcome)
            }
            None => self.journal.append(&outcome).map(|()| true),
        };
        let journal_error = match appended {
            Ok(true) => None,
            Ok(false) => {
                self.superseded = true;
                None
            }
            Err(err) => Some(err.to_string()),
        };
        self.release_live_marker(); // only after the outcome line (see commit)
        // A record is returned only when its line really reached the journal (T10: also `None` when `try_begin` already
        // marked this retry superseded, even if this append went through).
        if self.superseded || journal_error.is_some() {
            (None, journal_error)
        } else {
            (Some(outcome), None)
        }
    }

    /// C# `new JournalCodedException(code)`: the coded error with the localized text of `code` as its message.
    fn coded_error(&self, code: &str) -> io::Error {
        // DIVERGENCE: C# throws `ArgumentException` for an unknown code while building the exception (a caller bug:
        // callers pass `errors::*` constants only); Rust uses the code itself as the message (T21).
        let texts = self.journal.texts();
        let message = errors::text(code, &|key| texts.localized(key)).unwrap_or_else(|_| code.to_owned());
        JournalCodedError::new(code, message).into_io_error()
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
