//! Port of `PhotoReview.Core/FileActions/OperationJournal.cs` (C# baseline d92d6aae, 855 lines): the append-only
//! operation journal `operations.jsonl` (PARITY D-03, F-11, F-12, F-13, F-15, F-16, F-19, INV-6). Contract:
//! `contracts/W3-01.md`.
//!
//! The line format (one compact JSON object per line, CRLF) is W1-14 (`journal_line`); this module owns everything around
//! it: appending under a per-instance gate with the torn-tail repair and the sharing-violation retry, the full reader
//! (C# `ReadEntries`, `StreamReader` line rules), the latest line per Id, the reverse tail reader of committed Moves,
//! Dismiss, the P02 / FA-01 "append only if still current" guards and compaction (`TryCompact`).
//!
//! NOT here (same C# file, other units):
//! * the startup reconcile half (`ReconcilePendingOperations` .. `WithOutcome`, lines 457-666 and 841-854) is W3-03
//!   (`file_actions::reconcile`); [`OperationJournal::reconcile_pending_operations`] only delegates to it, and
//!   `OperationJournal` implements W3-03's `ReconcileJournal` trait with [`OperationJournal::read_entries`],
//!   [`OperationJournal::append_if`], the accessors and `english_text` (W3-03's `startup_recovery` takes the concrete
//!   `OperationJournal` and calls [`OperationJournal::try_compact`] directly);
//! * the shared line rules (`StartupCommittedMoveLimit`, `TryParseTailCommittedMove`, `IsRecycleOrCopyLine`,
//!   `IsReconcileCode`, `IsStaleReconcileAfterCommitted`) are W3-02 (`file_actions::journal_rules`), used by this module
//!   and by the compaction plan (`file_actions::compaction`, W3-02);
//! * `JournalTransaction` is [`super::transaction`] (this unit, other file).
//!
//! Several `OperationJournal` instances on the same file model several PhotoReview processes: each has its own gate, the
//! file's share modes are the only cross-instance exclusion (exactly as in C#).

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::ThreadId;
use std::time::Duration;

use crate::abstractions::{
    AppendStream, Clock, FileSystem, InProcessLiveOperationRegistry, JournalCompactionFiles, LiveOperationRegistry,
    Log, ReadSeek,
};
use crate::app_paths::AppPaths;
use crate::diagnostics::file_log::FileLog;
use crate::file_actions::journal_line::{self, JournalDateTime, JournalEntry};
use crate::file_actions::journal_rules::{
    STARTUP_COMMITTED_MOVE_LIMIT, is_stale_reconcile_after_committed, try_parse_tail_committed_move,
};
use crate::file_actions::reconcile::{self, ReconcileJournal};
use crate::file_actions::{compaction, errors};
use crate::io::physical::{decode_text_like_dotnet, new_guid_n};
use crate::model::{FileOperationType, JournalDurability, JournalState};
use crate::text::dotnet_path;
use crate::time::{DateTimeUtc, TICKS_PER_SECOND};

// ---------------------------------------------------------------------------------------------------------------
// Constants (C# `private const` / `internal const`; public here so the integration tests can pin them)
// ---------------------------------------------------------------------------------------------------------------

/// C# `FullScanThresholdBytes` (`1 * 1024 * 1024`): a journal whose length is AT LEAST this many bytes (`>=`) is read by
/// the reverse tail reader in [`OperationJournal::read_committed_moves`]; a shorter one by the full reader.
pub const FULL_SCAN_THRESHOLD_BYTES: u64 = 1024 * 1024;
/// First window of the reverse tail reader (C# `256L * 1024`); doubled after every pass.
pub const REVERSE_READ_INITIAL_WINDOW: u64 = 256 * 1024;
/// C# `AppendAttempts`: an append is tried this many times when opening the file fails with a sharing/lock violation.
pub const APPEND_ATTEMPTS: u32 = 5;
/// C# `AppendRetryStep`: the wait after failed attempt `n` (1-based) is `n * APPEND_RETRY_STEP` (10, 20, 30, 40 ms).
pub const APPEND_RETRY_STEP: Duration = Duration::from_millis(10);
/// C# `Environment.NewLine` on Windows. The journal ALWAYS uses CRLF, on every platform (byte-identical files).
pub const JOURNAL_NEW_LINE: &str = "\r\n";
/// Buffer size of every full read (C# `OpenReadShared(path)` default, 64 KiB).
pub const READ_BUFFER_SIZE: usize = 65536;
/// Buffer size of the one-byte tail probe (C# `OpenReadShared(_path, 16)`).
pub const TAIL_PROBE_BUFFER_SIZE: usize = 16;
/// C# `CompactionThresholdBytes` (= [`FULL_SCAN_THRESHOLD_BYTES`]): compaction runs only on a snapshot of at least this
/// many bytes, counted up to and including its LAST `'\n'` (a partial last line does not count).
pub const COMPACTION_THRESHOLD_BYTES: u64 = FULL_SCAN_THRESHOLD_BYTES;
/// C# `CompactionMinGain`: compaction rewrites only when it drops at least this share of the snapshot. The test is
/// `((snapshot - kept) as f64) < (snapshot as f64) * 0.25` => NotWorthIt (exactly a quarter IS compacted).
pub const COMPACTION_MIN_GAIN: f64 = 0.25;
/// C# `StaleCompactionFileAge` (1 h) in .NET ticks: a leftover replacement file is deleted unless `now - last_write <
/// age` (exactly one hour old IS removed).
pub const STALE_COMPACTION_FILE_AGE_TICKS: i64 = 3600 * TICKS_PER_SECOND;
/// C# `CompactionFileSuffix`: replacement files are `<journal path>.<guid N>.compact.tmp` next to the journal.
pub const COMPACTION_FILE_SUFFIX: &str = ".compact.tmp";

// ---------------------------------------------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------------------------------------------

/// C# `DismissOutcome`: `dismissed` are the entries actually appended as Dismissed (the NEW lines: state Dismissed,
/// timestamp = the clock, `error` and `error_code` cleared, every other field as the caller's snapshot); `skipped` are the
/// caller's snapshots, unchanged and in input order, whose current latest line no longer equals the snapshot (R09).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DismissOutcome {
    pub dismissed: Vec<JournalEntry>,
    pub skipped: Vec<JournalEntry>,
}

/// C# `JournalCompactionOutcome`. Every outcome but [`Compacted`](Self::Compacted) left the journal untouched.
/// ONE definition, here: W3-03 (`startup_recovery`) imports it from this module (no twin, no re-export needed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JournalCompactionOutcome {
    /// The journal was replaced by its compacted copy (plus every line appended meanwhile).
    Compacted,
    /// No compaction primitives were given to this journal (tests, tools).
    NotSupported,
    /// Missing, or smaller than [`COMPACTION_THRESHOLD_BYTES`].
    BelowThreshold,
    /// Less than [`COMPACTION_MIN_GAIN`] of the file could be dropped.
    NotWorthIt,
    /// A writer held the journal when compaction tried to lock it; try again later.
    Busy,
    /// The journal's start no longer matches the snapshot (another process compacted it meanwhile).
    Changed,
    /// An I/O error; see [`JournalCompactionResult::error`].
    Failed,
}

impl JournalCompactionOutcome {
    /// C# `Outcome.ToString()`: the variant name (`"Compacted"`, `"Busy"`, ...), used in the startup log line (W3-03).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compacted => "Compacted",
            Self::NotSupported => "NotSupported",
            Self::BelowThreshold => "BelowThreshold",
            Self::NotWorthIt => "NotWorthIt",
            Self::Busy => "Busy",
            Self::Changed => "Changed",
            Self::Failed => "Failed",
        }
    }
}

/// C# `JournalCompactionResult(Outcome, BytesBefore, BytesAfter, Error)`: sizes in bytes (0 when unknown; C# `long`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalCompactionResult {
    pub outcome: JournalCompactionOutcome,
    pub bytes_before: i64,
    pub bytes_after: i64,
    /// C# `ex.Message` of the I/O error of a [`JournalCompactionOutcome::Failed`] (Rust: `err.to_string()`), else `None`.
    pub error: Option<String>,
}

impl JournalCompactionResult {
    /// `new JournalCompactionResult(outcome, before, after)` (no error).
    pub fn new(outcome: JournalCompactionOutcome, bytes_before: i64, bytes_after: i64) -> Self {
        Self {
            outcome,
            bytes_before,
            bytes_after,
            error: None,
        }
    }

    /// `new JournalCompactionResult(Failed, 0, 0, ex.Message)`.
    pub fn failed(error: &io::Error) -> Self {
        Self {
            outcome: JournalCompactionOutcome::Failed,
            bytes_before: 0,
            bytes_after: 0,
            error: Some(error.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Texts (C# `JournalErrors.EnglishText` / `LocalizedText`; pr-core cannot depend on pr-l10n, D13 pattern)
// ---------------------------------------------------------------------------------------------------------------

/// The two catalogs the journal and its transactions need. `english(key)` is the invariant text STORED in the journal next
/// to a code (C# `BuiltInCatalog.EnglishLocalizer`); `localized(key)` is the UI text used as the message of a
/// [`JournalCodedError`](super::errors::JournalCodedError) raised by a transaction (C# `Localizer.Current`).
/// Keys are the catalog keys of [`super::errors::CODE_KEYS`]. The App composition root supplies a pr-l10n backed
/// implementation; [`EnglishJournalTexts`] is the default (what C# uses while the UI language is English).
pub trait JournalTexts: Send + Sync {
    fn english(&self, key: &str) -> String;
    fn localized(&self, key: &str) -> String;
}

/// `(catalog key, en.json text)` of the eight journal error codes, in [`super::errors::CODE_KEYS`] order. Must equal
/// `assets/languages/en.json` (a test reads the asset and compares every pair).
pub const ENGLISH_JOURNAL_TEXTS: [(&str, &str); 8] = [
    (
        "core.journal.sourceStillExists",
        "The source still exists after session recovery.",
    ),
    (
        "core.journal.pendingUnconfirmed",
        "Could not confirm the pending operation; it will not be replayed automatically.",
    ),
    (
        "core.fileAction.verifyFailedSizeChanged",
        "Post-operation check failed: the destination size changed.",
    ),
    ("core.recovery.verifyFailed", "Destination check after retry failed."),
    (
        "core.fileAction.destinationOutsideSource",
        "The destination folder must not be outside the folder being reviewed.",
    ),
    (
        "core.fileAction.moveSourceNotRemoved",
        "The file was copied to the destination, but the original could not be removed (it may be read-only or in use). Both copies now exist; nothing was deleted.",
    ),
    (
        "core.journal.cancelledByUser",
        "The operation was cancelled before it finished.",
    ),
    (
        "core.journal.unrepresentablePath",
        "A file path contains an unpaired UTF-16 surrogate, which cannot be recorded faithfully in the operation journal. Nothing was changed.",
    ),
];

/// Built-in English for both catalogs. An unknown key returns the key itself (the C# `Localizer.Get` fallback).
#[derive(Debug, Default, Clone, Copy)]
pub struct EnglishJournalTexts;

impl JournalTexts for EnglishJournalTexts {
    fn english(&self, key: &str) -> String {
        ENGLISH_JOURNAL_TEXTS
            .iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| key.to_owned(), |(_, text)| (*text).to_owned())
    }

    fn localized(&self, key: &str) -> String {
        self.english(key)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------------------------------------------

/// C# `Func<JournalDurability>`: re-read before EVERY append (a Settings change applies to the next write).
pub type DurabilityProvider = Arc<dyn Fn() -> JournalDurability + Send + Sync>;
/// C# `Action<TimeSpan> appendRetryDelay`: the wait between append attempts (default `std::thread::sleep`).
pub type AppendRetryDelay = Arc<dyn Fn(Duration) + Send + Sync>;
/// C# `internal Action<FileStream> FlushToDisk` (test seam; default `File::sync_all`, C# `Flush(flushToDisk: true)`).
/// Called once per PowerLossSafe append, after the bytes were written, ONLY when the append stream is a real file.
/// The real file is `AppendStream::as_file_mut()` of the stream `FileSystem::open_append` returned (decision P-11,
/// contract open question 1): `Some(file)` => this seam, `None` => a plain `flush`.
pub type FlushToDisk = Arc<dyn Fn(&mut File) -> io::Result<()> + Send + Sync>;

/// The optional C# constructor parameters (`null` = default) plus the ambient C# services made explicit.
/// `Default` gives the C# defaults.
#[derive(Clone, Default)]
pub struct OperationJournalOptions {
    /// Default: always [`JournalDurability::Fast`].
    pub durability: Option<DurabilityProvider>,
    /// Default: `std::thread::sleep`.
    pub append_retry_delay: Option<AppendRetryDelay>,
    /// Default: a fresh [`InProcessLiveOperationRegistry`] (private to this instance).
    pub live_operations: Option<Arc<dyn LiveOperationRegistry>>,
    /// Default: `None` = the journal never compacts ([`JournalCompactionOutcome::NotSupported`]).
    pub compaction_files: Option<Arc<dyn JournalCompactionFiles>>,
    /// Replaces C# `FileLog.Default` (the per-entry reconcile warnings of W3-03). Default:
    /// `diagnostics::file_log::FileLog::default_instance()`.
    pub log: Option<Arc<dyn Log>>,
    /// Default: [`EnglishJournalTexts`].
    pub texts: Option<Arc<dyn JournalTexts>>,
    /// Default: `File::sync_all`. See [`FlushToDisk`].
    pub flush_to_disk: Option<FlushToDisk>,
}

impl fmt::Debug for OperationJournalOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationJournalOptions")
            .field("durability", &self.durability.is_some())
            .field("append_retry_delay", &self.append_retry_delay.is_some())
            .field("live_operations", &self.live_operations.is_some())
            .field("compaction_files", &self.compaction_files.is_some())
            .field("log", &self.log.is_some())
            .field("texts", &self.texts.is_some())
            .field("flush_to_disk", &self.flush_to_disk.is_some())
            .finish()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The per-instance gate (C# `lock (_gate)`, a REENTRANT monitor). Complete in the stub (lead-provided).
// ---------------------------------------------------------------------------------------------------------------

/// C# `lock` is reentrant and the C# code relies on it: `Dismiss` holds the gate and calls `ComputeLatestEntries` and
/// `AppendLines`, which lock again; the compare-and-append guards hold it across `ReadEntries` + `Append`; a test's file
/// system / registry hook may call back into the same journal under the gate. A `std::sync::Mutex` would deadlock, so
/// this is a small owner-thread + depth monitor. It IS held across journal file I/O on purpose: that is the C# design
/// (read-compare-append atomic per instance); R0003's "no lock across I/O" does not apply to it.
#[derive(Debug, Default)]
struct ReentrantGate {
    state: Mutex<GateState>,
    released: Condvar,
}

#[derive(Debug, Default)]
struct GateState {
    owner: Option<ThreadId>,
    depth: usize,
}

#[derive(Debug)]
struct GateGuard<'a> {
    gate: &'a ReentrantGate,
}

impl ReentrantGate {
    fn lock(&self) -> GateGuard<'_> {
        let me = std::thread::current().id();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match state.owner {
                None => {
                    state.owner = Some(me);
                    state.depth = 1;
                    break;
                }
                Some(owner) if owner == me => {
                    state.depth += 1;
                    break;
                }
                Some(_) => {
                    state = self.released.wait(state).unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
        GateGuard { gate: self }
    }
}

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.depth = state.depth.saturating_sub(1);
        if state.depth == 0 {
            state.owner = None;
            self.gate.released.notify_one();
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// OperationJournal
// ---------------------------------------------------------------------------------------------------------------

/// C# `OperationJournal`. Thread-safe; share it as `Arc<OperationJournal>`.
pub struct OperationJournal {
    /// `paths.journal_file`.
    path: PathBuf,
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    gate: ReentrantGate,
    durability: DurabilityProvider,
    append_retry_delay: AppendRetryDelay,
    live_operations: Arc<dyn LiveOperationRegistry>,
    compaction_files: Option<Arc<dyn JournalCompactionFiles>>,
    /// `None` = `FileLog::default_instance()`.
    log: Option<Arc<dyn Log>>,
    texts: Arc<dyn JournalTexts>,
    /// `None` = `File::sync_all`.
    flush_to_disk: Option<FlushToDisk>,
}

impl fmt::Debug for OperationJournal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationJournal")
            .field("path", &self.path)
            .field("compaction", &self.compaction_files.is_some())
            .finish_non_exhaustive()
    }
}

impl OperationJournal {
    /// `new OperationJournal(paths, fileSystem, clock)` with every optional parameter at its default.
    pub fn new(paths: &AppPaths, fs: Arc<dyn FileSystem>, clock: Arc<dyn Clock>) -> Self {
        Self::with_options(paths, fs, clock, OperationJournalOptions::default())
    }

    /// The full C# constructor. The journal file is `paths.journal_file`; nothing is read or created here.
    pub fn with_options(
        paths: &AppPaths,
        fs: Arc<dyn FileSystem>,
        clock: Arc<dyn Clock>,
        options: OperationJournalOptions,
    ) -> Self {
        Self {
            path: paths.journal_file.clone(),
            fs,
            clock,
            gate: ReentrantGate::default(),
            durability: options
                .durability
                .unwrap_or_else(|| Arc::new(|| JournalDurability::Fast)),
            append_retry_delay: options
                .append_retry_delay
                .unwrap_or_else(|| Arc::new(std::thread::sleep)),
            live_operations: options
                .live_operations
                .unwrap_or_else(|| Arc::new(InProcessLiveOperationRegistry::new())),
            compaction_files: options.compaction_files,
            log: options.log,
            texts: options.texts.unwrap_or_else(|| Arc::new(EnglishJournalTexts)),
            flush_to_disk: options.flush_to_disk,
        }
    }

    /// The journal file (C# `_path`).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// C# `_fileSystem` (W3-03's `ReconcileJournal::file_system`).
    pub fn file_system(&self) -> &dyn FileSystem {
        self.fs.as_ref()
    }

    /// C# `_clock` (W3-03's `ReconcileJournal::clock`).
    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    /// C# `Durability`: the mode the NEXT write uses (calls the provider every time).
    pub fn durability(&self) -> JournalDurability {
        (self.durability)()
    }

    /// C# `LiveOperations` (Q-R27): the registry [`JournalTransaction`](super::transaction::JournalTransaction) takes its
    /// marker from and reconcile consults.
    pub fn live_operations(&self) -> &Arc<dyn LiveOperationRegistry> {
        &self.live_operations
    }

    /// The catalogs used for stored English text and for coded-error messages.
    pub fn texts(&self) -> &Arc<dyn JournalTexts> {
        &self.texts
    }

    /// The log C# writes to through `FileLog.Default` (reconcile warnings, W3-03): the injected one, else
    /// `FileLog::default_instance()`.
    pub fn diagnostics_log(&self) -> &dyn Log {
        match self.log.as_deref() {
            Some(log) => log,
            None => FileLog::default_instance(),
        }
    }

    /// C# `Append(entry)` = `AppendLines([entry])`. In this order (each step observable through a counting file system):
    /// 1. (C# `ThrowIfUnrepresentable`: vacuous in Rust, see the contract);
    /// 2. take the gate;
    /// 3. `dir = dotnet_path::get_directory_name(path)`; when `Some(non-empty)`: `fs.create_directory(dir)?` (EVERY append);
    /// 4. text = for each entry `entry.to_line() + "\r\n"`;
    /// 5. `durable = (self.durability)() == PowerLossSafe` (provider called once per append);
    /// 6. open with retry: `fs.open_append(path, durable)`; an error with [`is_sharing_or_lock_violation`] on attempt
    ///    `n < APPEND_ATTEMPTS` calls `append_retry_delay(n * APPEND_RETRY_STEP)` and tries again; any other error, or the
    ///    5th failure, is returned;
    /// 7. torn-tail check UNDER the append handle: `fs.file_exists(path)` and, when it exists, `fs.open_read_shared(path,
    ///    16)`, length 0 -> no repair, else read the LAST byte; not `'\n'` -> prepend `"\r\n"` to the text;
    /// 8. ONE `write_all` of the UTF-8 bytes; 9. durable + real file -> flush-to-disk seam, else `flush()`.
    pub fn append(&self, entry: &JournalEntry) -> io::Result<()> {
        self.append_lines(std::slice::from_ref(entry))
    }

    /// C# `Dismiss(entries)` (F-19, R09). The clock is read ONCE, before the gate. Under the gate: latest-entry map (FA-01
    /// rule, [`Self::read_pending_and_failed_operations`]'s pass); for each input in order, `latest[id] == entry` (value
    /// equality) -> a Dismissed copy, else skipped (duplicates in the input are judged against the same map, so both are
    /// dismissed). Non-empty -> ONE append of all Dismissed lines (one write); empty -> nothing is opened for append and no
    /// directory is created. Files named by the entries are never touched. The live marker is NOT consulted (RV-T05).
    pub fn dismiss(&self, entries: &[JournalEntry]) -> io::Result<DismissOutcome> {
        let now = self.clock.utc_now();
        let mut outcome = DismissOutcome::default();
        let _gate = self.gate.lock();
        let latest = self.compute_latest_entries()?;
        for entry in entries {
            if latest.get(&entry.id) == Some(entry) {
                outcome.dismissed.push(JournalEntry {
                    state: JournalState::Dismissed,
                    timestamp_utc: JournalDateTime::utc(now),
                    error: None,
                    error_code: None,
                    ..entry.clone()
                });
            } else {
                outcome.skipped.push(entry.clone());
            }
        }
        if !outcome.dismissed.is_empty() {
            self.append_lines(&outcome.dismissed)?;
        }
        Ok(outcome)
    }

    /// C# `ReadCommittedMoves`: committed Move lines in file order. Under the gate: missing file -> `Ok(vec![])`; open
    /// `open_read_shared(path, 64 KiB)`; length (`seek(End(0))`, no bytes read) `>= FULL_SCAN_THRESHOLD_BYTES` -> the
    /// reverse tail reader on THAT stream (newest `STARTUP_COMMITTED_MOVE_LIMIT` only, each byte read and parsed once,
    /// byte parser `journal_rules::try_parse_tail_committed_move`). Otherwise the stream is dropped, the gate released, and
    /// the full reader [`Self::read_entries`] collects every committed Move (no limit; `StreamReader` line rules).
    pub fn read_committed_moves(&self) -> io::Result<Vec<JournalEntry>> {
        {
            let _gate = self.gate.lock();
            if !self.fs.file_exists(&self.path) {
                return Ok(Vec::new());
            }
            let mut stream = self.fs.open_read_shared(&self.path, READ_BUFFER_SIZE)?;
            let length = stream.seek(SeekFrom::End(0))?;
            if length >= FULL_SCAN_THRESHOLD_BYTES {
                return read_committed_moves_reverse(stream.as_mut(), length);
            }
            // T5: the stream is dropped and the gate released before the full reader re-opens the file.
        }

        let mut entries = Vec::new();
        self.read_entries(&mut |entry| {
            if entry.op_type == FileOperationType::Move && entry.state == JournalState::Committed {
                entries.push(entry);
            }
        })?;
        Ok(entries)
    }

    /// C# `ReadPendingAndFailedOperations` (R2-F-17): ONE pass ([`Self::read_entries`]) building the latest entry per Id
    /// (FA-01 rule; insertion-ordered, a re-assigned Id keeps its first position); then the Prepared ones, then the Failed
    /// ones, each group in order of the Id's FIRST appearance in the file.
    pub fn read_pending_and_failed_operations(&self) -> io::Result<Vec<JournalEntry>> {
        let latest = self.compute_latest_entries()?;
        let prepared = latest.values().filter(|e| e.state == JournalState::Prepared);
        let failed = latest.values().filter(|e| e.state == JournalState::Failed);
        Ok(prepared.chain(failed).cloned().collect())
    }

    /// C# `ReconcilePendingOperations(preparedBeforeUtc)` (F-14, Q-R27, ADR 0003). Delegates to W3-03:
    /// `reconcile::reconcile_pending_operations(self, prepared_before_utc, self.diagnostics_log())`.
    pub fn reconcile_pending_operations(
        &self,
        prepared_before_utc: Option<DateTimeUtc>,
    ) -> io::Result<Vec<JournalEntry>> {
        reconcile::reconcile_pending_operations(self, prepared_before_utc, self.diagnostics_log())
    }

    /// C# `internal AppendIfUnchangedSince(anchor, ownSince, entry)` (P02). Under the gate: `history` = every line of
    /// `anchor.id` in file order (raw, NO FA-01 skipping); `at` = index of the LAST line equal to `anchor` (none: the whole
    /// history counts as "since"); appends `entry` only when the lines after `at` are exactly `own_since` (same count,
    /// element-wise equal). `Ok(false)` = nothing appended (no directory created, nothing opened for append).
    pub fn append_if_unchanged_since(
        &self,
        anchor: &JournalEntry,
        own_since: &[JournalEntry],
        entry: &JournalEntry,
    ) -> io::Result<bool> {
        let _gate = self.gate.lock();
        let history = self.read_lines_of(&anchor.id)?;
        // None: the anchor is not in the journal, the whole history is "since" (C# FindLastIndex == -1).
        let since_start = history.iter().rposition(|e| e == anchor).map_or(0, |at| at + 1);
        if history[since_start..] != *own_since {
            return Ok(false);
        }
        self.append(entry)?;
        Ok(true)
    }

    /// C# `internal ReadLatestEntry(id)`: under the gate, the effective latest line of `id` (a reconcile-coded Failed right
    /// after a Committed is skipped, FA-01). `Ok(None)` when the Id has no line (or the journal is missing).
    pub fn read_latest_entry(&self, id: &str) -> io::Result<Option<JournalEntry>> {
        let _gate = self.gate.lock();
        let mut latest: Option<JournalEntry> = None;
        self.read_entries(&mut |entry| {
            if entry.id == id && !is_stale_reconcile_after_committed(&entry, latest.as_ref()) {
                latest = Some(entry);
            }
        })?;
        Ok(latest)
    }

    /// C# `TryCompact` (F-16, D-14). Never returns an I/O error: the outcome says what happened (see the contract for
    /// the exact step order and the byte counts of each outcome). Uses W3-02's `compaction::build` for the plan.
    pub fn try_compact(&self) -> JournalCompactionResult {
        let Some(files) = self.compaction_files.as_deref() else {
            return JournalCompactionResult::new(JournalCompactionOutcome::NotSupported, 0, 0);
        };
        // C# catches IOException / UnauthorizedAccessException / PlatformNotSupportedException: every io::Error here.
        // Nothing was renamed (the rename is the last step and atomic): the journal is as it was.
        self.try_compact_with(files)
            .unwrap_or_else(|err| JournalCompactionResult::failed(&err))
    }

    /// C# `ReadEntries(handle)`: under the gate; missing file -> `Ok(())` with no call; otherwise
    /// `open_read_shared(path, 64 KiB)`, read every byte once, split with [`decode_stream_reader_lines`] and call `handle`
    /// for every line `journal_line::try_parse` accepts, in file order. Open/read errors are returned. `handle` runs while
    /// the gate is held (it may call back into this journal on the same thread: the gate is reentrant).
    /// Also W3-03's `ReconcileJournal::read_entries`.
    pub fn read_entries(&self, handle: &mut dyn FnMut(JournalEntry)) -> io::Result<()> {
        let _gate = self.gate.lock();
        if !self.fs.file_exists(&self.path) {
            return Ok(());
        }
        let bytes = {
            let mut stream = self.fs.open_read_shared(&self.path, READ_BUFFER_SIZE)?;
            read_to_end(stream.as_mut())?
        };
        for line in decode_stream_reader_lines(&bytes) {
            if let Some(entry) = journal_line::try_parse(&line) {
                handle(entry);
            }
        }
        Ok(())
    }

    /// W3-03's compare-and-append primitive (`ReconcileJournal::append_if`, used for C# `AppendIfStillPending` and
    /// `AppendIfLatestIs`): under ONE hold of the gate, collect the raw lines of `id` ([`Self::read_entries`], ordinal Id
    /// match, file order), call `decide(&lines)`, and when it returns `Some(entry)` append it exactly like
    /// [`Self::append`] and return `Ok(true)`; `None` -> `Ok(false)`, nothing created or opened for append.
    pub fn append_if(
        &self,
        id: &str,
        decide: &mut dyn FnMut(&[JournalEntry]) -> Option<JournalEntry>,
    ) -> io::Result<bool> {
        let _gate = self.gate.lock();
        let lines = self.read_lines_of(id)?;
        match decide(&lines) {
            Some(entry) => {
                self.append(&entry)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------------------------------------------

    /// The journal path as text (C# `_path`; the app's paths are always valid Unicode).
    fn path_text(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    /// Every accepted line whose Id equals `id` (ordinal), in file order. Raw: no FA-01 skipping.
    fn read_lines_of(&self, id: &str) -> io::Result<Vec<JournalEntry>> {
        let mut lines = Vec::new();
        self.read_entries(&mut |entry| {
            if entry.id == id {
                lines.push(entry);
            }
        })?;
        Ok(lines)
    }

    /// C# `ComputeLatestEntries()`: the effective latest line per Id (FA-01 rule) in order of FIRST appearance.
    fn compute_latest_entries(&self) -> io::Result<LatestEntries> {
        let mut latest = LatestEntries::default();
        self.read_entries(&mut |entry| {
            if !is_stale_reconcile_after_committed(&entry, latest.get(&entry.id)) {
                latest.set(entry);
            }
        })?;
        Ok(latest)
    }

    /// C# `AppendLines(entries)`; see [`Self::append`] for the exact order.
    fn append_lines(&self, entries: &[JournalEntry]) -> io::Result<()> {
        // C# `ThrowIfUnrepresentable` (W2-FA-03) cannot fire: a Rust `String` cannot hold an unpaired surrogate (T19).
        let _gate = self.gate.lock();
        let path_text = self.path_text();
        if let Some(dir) = dotnet_path::get_directory_name(&path_text) {
            if !dir.is_empty() {
                self.fs.create_directory(Path::new(&dir))?;
            }
        }
        let mut text = String::new();
        for entry in entries {
            text.push_str(&entry.to_line());
            text.push_str(JOURNAL_NEW_LINE);
        }
        let durable = (self.durability)() == JournalDurability::PowerLossSafe;
        let mut stream = self.open_append_with_retry(durable)?;
        // A crash can leave a partial last line; the new record then starts on its own line (checked under the
        // append handle, RV-C04).
        if self.tail_lacks_newline()? {
            text.insert_str(0, JOURNAL_NEW_LINE);
        }
        stream.write_all(text.as_bytes())?;
        if durable {
            if let Some(file) = stream.as_file_mut() {
                return match &self.flush_to_disk {
                    Some(flush) => flush(file),
                    None => file.sync_all(),
                };
            }
        }
        stream.flush()
    }

    /// C# `OpenAppendWithRetry(durable)`: up to [`APPEND_ATTEMPTS`] opens; only a sharing/lock violation is retried,
    /// after `attempt * APPEND_RETRY_STEP`.
    fn open_append_with_retry(&self, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        let mut attempt: u32 = 1;
        loop {
            match self.fs.open_append(&self.path, durable) {
                Ok(stream) => return Ok(stream),
                Err(err) if attempt < APPEND_ATTEMPTS && is_sharing_or_lock_violation(&err) => {
                    (self.append_retry_delay)(APPEND_RETRY_STEP * attempt);
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// C# `TailLacksNewline()`: true when the journal exists, is not empty and its last byte is not `'\n'`.
    fn tail_lacks_newline(&self) -> io::Result<bool> {
        if !self.fs.file_exists(&self.path) {
            return Ok(false);
        }
        let mut stream = self.fs.open_read_shared(&self.path, TAIL_PROBE_BUFFER_SIZE)?;
        let length = stream.seek(SeekFrom::End(0))?;
        if length == 0 {
            return Ok(false);
        }
        stream.seek(SeekFrom::Start(length - 1))?;
        let mut byte = [0u8; 1];
        let read = stream.read(&mut byte)?;
        Ok(read == 1 && byte[0] != b'\n')
    }

    /// The body of [`Self::try_compact`] once compaction files are known; every `Err` becomes `Failed`.
    fn try_compact_with(&self, files: &dyn JournalCompactionFiles) -> io::Result<JournalCompactionResult> {
        self.remove_stale_compaction_files()?;
        if !self.fs.file_exists(&self.path) {
            return Ok(JournalCompactionResult::new(
                JournalCompactionOutcome::BelowThreshold,
                0,
                0,
            ));
        }

        let snapshot = {
            let mut stream = self.fs.open_read_shared(&self.path, READ_BUFFER_SIZE)?;
            read_to_end(stream.as_mut())?
        };
        let snapshot_len = len_i64(snapshot.len());
        // A partial last line belongs to the delta.
        let cut = snapshot.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        if (cut as u64) < COMPACTION_THRESHOLD_BYTES {
            return Ok(JournalCompactionResult::new(
                JournalCompactionOutcome::BelowThreshold,
                snapshot_len,
                snapshot_len,
            ));
        }

        let plan = compaction::build(&snapshot[..cut]);
        if (cut as f64 - plan.kept.len() as f64) < (cut as f64) * COMPACTION_MIN_GAIN {
            return Ok(JournalCompactionResult::new(
                JournalCompactionOutcome::NotWorthIt,
                snapshot_len,
                snapshot_len,
            ));
        }

        let temp_path = format!("{}.{}{}", self.path_text(), new_guid_n(), COMPACTION_FILE_SUFFIX);
        // Declared before the gate: dropped (and, when not replaced, deleted) after the gate is released, like C# `using`.
        let mut replacement = files.create_staged_replacement(Path::new(&temp_path))?;
        replacement.write(&plan.kept)?;
        replacement.flush_to_disk()?;

        let _gate = self.gate.lock();
        let mut locked = match files.open_read_deny_writers(&self.path) {
            Ok(stream) => stream,
            Err(err) if is_sharing_or_lock_violation(&err) => {
                return Ok(JournalCompactionResult::new(
                    JournalCompactionOutcome::Busy,
                    snapshot_len,
                    snapshot_len,
                ));
            }
            Err(err) => return Err(err),
        };
        let current = read_to_end(locked.as_mut())?;
        let current_len = len_i64(current.len());
        if current.len() < cut || current[..cut] != snapshot[..cut] {
            return Ok(JournalCompactionResult::new(
                JournalCompactionOutcome::Changed,
                current_len,
                current_len,
            ));
        }
        let delta = &current[cut..];
        let mut new_len = len_i64(plan.kept.len()).saturating_add(len_i64(delta.len()));
        if let Some(&last) = delta.last() {
            replacement.write(delta)?;
            if last != b'\n' {
                replacement.write(JOURNAL_NEW_LINE.as_bytes())?;
                new_len = new_len.saturating_add(len_i64(JOURNAL_NEW_LINE.len()));
            }
            replacement.flush_to_disk()?;
        }
        // The deny-writers stream is still open: no append can land in the old file before the rename.
        replacement.replace_atomically(&self.path)?;
        Ok(JournalCompactionResult::new(
            JournalCompactionOutcome::Compacted,
            current_len,
            new_len,
        ))
    }

    /// C# `RemoveStaleCompactionFiles()`: deletes `<journal>.*.compact.tmp` leftovers at least
    /// [`STALE_COMPACTION_FILE_AGE_TICKS`] old; a delete error is ignored (a compaction in progress, or not ours).
    fn remove_stale_compaction_files(&self) -> io::Result<()> {
        let path_text = self.path_text();
        let Some(dir) = dotnet_path::get_directory_name(&path_text) else {
            return Ok(());
        };
        if dir.is_empty() || !self.fs.directory_exists(Path::new(&dir)) {
            return Ok(());
        }
        let pattern = format!("{}.*{}", dotnet_path::get_file_name(&path_text), COMPACTION_FILE_SUFFIX);
        let files = self.fs.enumerate_files(Path::new(&dir), &pattern)?;
        for file in files {
            if !has_compaction_suffix(&file) {
                continue; // 8.3 pattern quirks
            }
            let Some(stat) = self.fs.get_file_stat(&file) else {
                continue;
            };
            let age = self.clock.utc_now().ticks().saturating_sub(stat.last_write_utc.ticks());
            if age < STALE_COMPACTION_FILE_AGE_TICKS {
                continue;
            }
            // Still open (a compaction in progress after all) or not ours: leave it.
            let _ = self.fs.delete(&file);
        }
        Ok(())
    }
}

impl ReconcileJournal for OperationJournal {
    fn file_system(&self) -> &dyn FileSystem {
        OperationJournal::file_system(self)
    }

    fn clock(&self) -> &dyn Clock {
        OperationJournal::clock(self)
    }

    fn live_operations(&self) -> &dyn LiveOperationRegistry {
        self.live_operations.as_ref()
    }

    fn english_text(&self, code: &str) -> String {
        errors::catalog_key(code).map_or_else(String::new, |k| self.texts.english(k))
    }

    fn read_entries(&self, handle: &mut dyn FnMut(JournalEntry)) -> io::Result<()> {
        OperationJournal::read_entries(self, handle)
    }

    fn append_if(&self, id: &str, decide: &mut dyn FnMut(&[JournalEntry]) -> Option<JournalEntry>) -> io::Result<bool> {
        OperationJournal::append_if(self, id, decide)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Private free helpers
// ---------------------------------------------------------------------------------------------------------------

/// C# `Dictionary<string, JournalEntry>` without removals: insertion-ordered, a re-assigned key keeps its position,
/// ordinal keys.
#[derive(Debug, Default)]
struct LatestEntries {
    entries: Vec<JournalEntry>,
    index: HashMap<String, usize>,
}

impl LatestEntries {
    fn get(&self, id: &str) -> Option<&JournalEntry> {
        self.index.get(id).and_then(|&i| self.entries.get(i))
    }

    fn set(&mut self, entry: JournalEntry) {
        match self.index.get(&entry.id) {
            Some(&i) => {
                if let Some(slot) = self.entries.get_mut(i) {
                    *slot = entry;
                }
            }
            None => {
                self.index.insert(entry.id.clone(), self.entries.len());
                self.entries.push(entry);
            }
        }
    }

    fn values(&self) -> impl Iterator<Item = &JournalEntry> {
        self.entries.iter()
    }
}

/// C# `ReadCommittedMovesReverse(stream)` (CORE-08): every pass reads only the not-yet-parsed prefix
/// `[start, boundary)` and parses it once; `boundary` always sits on a line start.
fn read_committed_moves_reverse(stream: &mut dyn ReadSeek, length: u64) -> io::Result<Vec<JournalEntry>> {
    let mut boundary = length;
    let mut window = REVERSE_READ_INITIAL_WINDOW;
    let mut collected: Vec<JournalEntry> = Vec::new(); // file (ascending) order
    loop {
        let start = length.saturating_sub(window);
        let size = usize::try_from(boundary - start).map_err(io::Error::other)?;
        let mut buffer = vec![0u8; size];
        stream.seek(SeekFrom::Start(start))?;
        stream.read_exact(&mut buffer)?;

        let mut line_start = 0usize;
        let mut new_boundary = boundary;
        if start > 0 {
            match buffer.iter().position(|&b| b == b'\n') {
                None => {
                    // One line longer than the window: nothing parseable yet, widen and re-read the same prefix.
                    window = window.saturating_mul(2);
                    continue;
                }
                Some(first_newline) => {
                    line_start = first_newline + 1;
                    new_boundary = start + line_start as u64;
                }
            }
        }

        let mut chunk = parse_committed_moves(&buffer[line_start..]);
        chunk.append(&mut collected);
        collected = chunk;

        if collected.len() >= STARTUP_COMMITTED_MOVE_LIMIT || start == 0 {
            let skip = collected.len().saturating_sub(STARTUP_COMMITTED_MOVE_LIMIT);
            collected.drain(..skip);
            return Ok(collected);
        }
        boundary = new_boundary;
        window = window.saturating_mul(2);
    }
}

/// C# `ParseCommittedMoves(buffer, offset)`: split at `'\n'` (the last piece may lack it), strip ONE trailing `'\r'`.
fn parse_committed_moves(buffer: &[u8]) -> Vec<JournalEntry> {
    let mut entries = Vec::new();
    let mut remaining = buffer;
    while !remaining.is_empty() {
        let (mut line, rest) = match remaining.iter().position(|&b| b == b'\n') {
            Some(newline) => (&remaining[..newline], &remaining[newline + 1..]),
            None => (remaining, &remaining[remaining.len()..]),
        };
        remaining = rest;
        if let Some(stripped) = line.strip_suffix(b"\r") {
            line = stripped;
        }
        if let Some(entry) = try_parse_tail_committed_move(line) {
            entries.push(entry);
        }
    }
    entries
}

/// C# `ReadToEnd(stream)`: every byte from the current position.
fn read_to_end(stream: &mut dyn ReadSeek) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// A byte count as C# `long`.
fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// C# `file.EndsWith(CompactionFileSuffix, StringComparison.OrdinalIgnoreCase)`.
fn has_compaction_suffix(file: &Path) -> bool {
    let text = file.to_string_lossy();
    text.len()
        .checked_sub(COMPACTION_FILE_SUFFIX.len())
        .and_then(|at| text.get(at..))
        .is_some_and(|tail| crate::text::eq_ignore_case(tail, COMPACTION_FILE_SUFFIX))
}

// ---------------------------------------------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------------------------------------------

/// The lines C# `ReadEntries` hands to the parser: `new StreamReader(stream, Encoding.UTF8,
/// detectEncodingFromByteOrderMarks: true)` + `ReadLine()` until null, over the whole file `bytes`.
///
/// 1. Encoding from the first bytes (BOM removed): `EF BB BF` UTF-8; `FE FF` UTF-16BE; `FF FE 00 00` UTF-32LE; `FF FE`
///    (anything else after it, or fewer than 4 bytes) UTF-16LE; `00 00 FE FF` UTF-32BE; no BOM -> UTF-8.
/// 2. Decode with replacement: invalid UTF-8 -> one U+FFFD per maximal invalid subpart (`String::from_utf8_lossy` does
///    exactly this); UTF-16 lone surrogates and an odd final byte -> U+FFFD; UTF-32 values above 0x10FFFF or surrogates,
///    and a trailing partial unit -> U+FFFD.
/// 3. Split at `"\r\n"`, `"\n"` AND a lone `"\r"` (a line with an inner CR becomes two lines). A final line without a
///    terminator is returned; a terminator at the very end does not produce an empty last line; empty lines in the middle
///    are returned as `""`. An empty input gives no line.
pub fn decode_stream_reader_lines(bytes: &[u8]) -> Vec<String> {
    let text = decode_text_like_dotnet(bytes);
    let mut lines = Vec::new();
    let mut rest = text.as_str();
    while !rest.is_empty() {
        match rest.find(['\r', '\n']) {
            Some(at) => {
                lines.push(rest[..at].to_owned());
                let terminator = if rest[at..].starts_with("\r\n") { 2 } else { 1 };
                rest = &rest[at + terminator..];
            }
            None => {
                lines.push(rest.to_owned());
                rest = "";
            }
        }
    }
    lines
}

/// C# `IsSharingOrLockViolation`: HRESULT 0x80070020 / 0x80070021, i.e. raw OS error 32 (ERROR_SHARING_VIOLATION) or 33
/// (ERROR_LOCK_VIOLATION). Any other error (including one with no OS code) is false.
pub fn is_sharing_or_lock_violation(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(32 | 33))
}
