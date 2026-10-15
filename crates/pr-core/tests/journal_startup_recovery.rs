//! Red tests of W3-03 (`file_actions::startup_recovery`): the startup step (reconcile, then compaction, then the summary
//! log), ported from `tests/PhotoReview.Core.Tests/FileActions/JournalStartupRecoveryTests.cs`, `JournalSmallGapsTests.cs`,
//! `JournalCompactionTests.cs` (the `RunAsync_*` ones), `JournalGroupEvidenceAndNulMemberTests.cs` (the W2-FA-04 startup
//! test) and the golden `tests/golden/journal-startup-recovery.json` (baseline d92d6aae). Every test names the C# test it
//! mirrors (`// C#: Class.Method`). They compile against the stubs and FAIL until the bodies land (`todo!()` panics).
//! Contract: `contracts/W3-03.md`.
//!
//! Each compaction test exists twice: an in-memory twin over `FakeFileSystem` (the default run) and a native twin on the
//! real disk (`#[ignore = "native: ..."]`, needs W3-02 `PhysicalJournalCompactionFiles`). The UndoService half of the two
//! P03 tests needs W3-05 and is added at integration (`TODO(W3-05 integration)`). No sleeps: blocking uses channels.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use pr_core::abstractions::{
    FileSystem, InProcessLiveOperationRegistry, JournalCompactionFiles, LiveMarker, LiveOperationRegistry, Log,
    ReadSeek, StagedReplacement,
};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{AppendRetryDelay, OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::startup_recovery::{self, RECOVERY_THREAD_NAME};
use pr_core::io::journal_compaction_files::PhysicalJournalCompactionFiles;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::golden::{self, i64_of, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, HookFileSystem, LogLevel, PathHook, RecordingLog, TempRoot};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// The C# `BaseTime` / `Stamp` of these tests: `2026-09-29T00:00:00Z` plus `secs`.
fn at(secs: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(utc("2026-09-29T00:00:00Z").ticks() + secs * TICKS_PER_SECOND).expect("stamp in range")
}

fn utc(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text)
        .unwrap_or_else(|| panic!("bad stamp {text}"))
        .0
}

fn jdt(value: DateTimeUtc) -> JournalDateTime {
    JournalDateTime::utc(value)
}

const JOURNAL: &str = r"C:\Users\test\AppData\Local\PhotoReview\Data\operations.jsonl";

fn paths_at(journal: &str) -> AppPaths {
    let mut paths = AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false);
    paths.journal_file = PathBuf::from(journal);
    paths
}

/// C# `Pending(id, type, source, destination, size, prepared)`: last write `WriteTime`, timestamp `prepared`.
fn pending(
    id: &str,
    op: FileOperationType,
    source: &str,
    destination: Option<&str>,
    size: i64,
    prepared: DateTimeUtc,
) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: op,
        state: JournalState::Prepared,
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size,
        last_write_utc: jdt(at(-86_400)),
        timestamp_utc: jdt(prepared),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// The journal line bytes exactly as `OperationJournal::append` writes them (CRLF terminated).
fn line_bytes(entry: &JournalEntry) -> Vec<u8> {
    format!("{}\r\n", entry.to_line()).into_bytes()
}

/// A journal over `fs` with the no-op retry delay, a recording diagnostics log and the given compaction primitives.
fn journal_with(
    fs: Arc<dyn FileSystem>,
    paths: &AppPaths,
    clock: DateTimeUtc,
    compaction: Option<Arc<dyn JournalCompactionFiles>>,
) -> OperationJournal {
    let delay: AppendRetryDelay = Arc::new(|_: Duration| {});
    let diagnostics: Arc<dyn Log> = Arc::new(RecordingLog::new());
    OperationJournal::with_options(
        paths,
        fs,
        Arc::new(FakeClock::at(clock)),
        OperationJournalOptions {
            append_retry_delay: Some(delay),
            compaction_files: compaction,
            log: Some(diagnostics),
            ..Default::default()
        },
    )
}

fn compaction(files: impl JournalCompactionFiles + 'static) -> Option<Arc<dyn JournalCompactionFiles>> {
    Some(Arc::new(files))
}

fn messages(log: &RecordingLog, level: LogLevel) -> Vec<String> {
    log.entries()
        .into_iter()
        .filter(|(l, _)| *l == level)
        .map(|(_, m)| m)
        .collect()
}

/// `2026-09-29` photos on the fake disk: `size` bytes at `stamp`.
fn put(disk: &FakeFileSystem, path: &str, size: usize, stamp: DateTimeUtc) {
    disk.add_file_at(path, &vec![b'x'; size], stamp);
}

// ---------------------------------------------------------------------------------------------------------------------
// The compaction primitives of the in-memory twins (C# `InMemoryCompactionFiles` plus the hooks of the decorator)
// ---------------------------------------------------------------------------------------------------------------------

/// The in-memory replacement primitives over the fake disk (C# `InMemoryCompactionFiles`).
struct MemCompaction {
    fs: Arc<FakeFileSystem>,
}

impl JournalCompactionFiles for MemCompaction {
    fn open_read_deny_writers(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        FileSystem::open_read_shared(self.fs.as_ref(), path, 65_536)
    }

    fn create_staged_replacement(&self, _temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>> {
        Ok(Box::new(MemStaged {
            fs: Arc::clone(&self.fs),
            bytes: Vec::new(),
        }))
    }
}

struct MemStaged {
    fs: Arc<FakeFileSystem>,
    bytes: Vec<u8>,
}

impl StagedReplacement for MemStaged {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn flush_to_disk(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn replace_atomically(&mut self, destination: &Path) -> io::Result<()> {
        self.fs.add_file_at(destination, &self.bytes, at(0));
        Ok(())
    }
}

/// The C# `HookedCompactionFiles` decorator, for either primitives (in memory or physical).
struct Hooked {
    inner: Arc<dyn JournalCompactionFiles>,
    /// Runs at the very start of `open_read_deny_writers`, before delegating (C# `BeforeLock`).
    before_lock: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// `open_read_deny_writers` fails with the sharing violation of a writer holding the file (C# `Busy`, raw os error 32).
    busy: bool,
    /// Every panic of the compaction code (C# `ThrowingCompactionFiles`: an exception that is not an I/O error).
    panic: bool,
    /// `replace_atomically` fails before renaming (C# `ThrowInsteadOfReplace`).
    fail_replace: bool,
}

impl Hooked {
    fn new(inner: Arc<dyn JournalCompactionFiles>) -> Self {
        Self {
            inner,
            before_lock: Mutex::new(None),
            busy: false,
            panic: false,
            fail_replace: false,
        }
    }

    fn before_lock(self, action: impl FnOnce() + Send + 'static) -> Self {
        let boxed: Box<dyn FnOnce() + Send> = Box::new(action);
        *self.before_lock.lock().unwrap_or_else(PoisonError::into_inner) = Some(boxed);
        self
    }
}

impl JournalCompactionFiles for Hooked {
    fn open_read_deny_writers(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        let action = self.before_lock.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(action) = action {
            action();
        }
        if self.panic {
            panic!("simulated compaction bug");
        }
        if self.busy {
            return Err(io::Error::from_raw_os_error(32));
        }
        self.inner.open_read_deny_writers(path)
    }

    fn create_staged_replacement(&self, temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>> {
        let inner = self.inner.create_staged_replacement(temp_path)?;
        Ok(Box::new(HookedStaged {
            inner,
            fail: self.fail_replace,
        }))
    }
}

struct HookedStaged {
    inner: Box<dyn StagedReplacement>,
    fail: bool,
}

impl StagedReplacement for HookedStaged {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write(bytes)
    }

    fn flush_to_disk(&mut self) -> io::Result<()> {
        self.inner.flush_to_disk()
    }

    fn replace_atomically(&mut self, destination: &Path) -> io::Result<()> {
        if self.fail {
            return Err(io::Error::other("simulated crash before replace"));
        }
        self.inner.replace_atomically(destination)
    }
}

/// C# `ThrowingCompactionFiles`: every primitive throws (here: panics), a compaction bug.
struct ThrowingCompaction;

impl JournalCompactionFiles for ThrowingCompaction {
    fn open_read_deny_writers(&self, _path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        panic!("simulated compaction bug");
    }

    fn create_staged_replacement(&self, _temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>> {
        panic!("simulated compaction bug");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Journal content for the compaction tests (C# `BuildCompactibleJournal` / `PreparedEntry`)
// ---------------------------------------------------------------------------------------------------------------------

const COMPACTIBLE_COUNT: i64 = 9000;

fn prepared_entry(i: i64, stamp: DateTimeUtc) -> JournalEntry {
    let id = format!("j{i:06}");
    let padding = "x".repeat(80);
    let folder = i % 50;
    JournalEntry {
        timestamp_utc: jdt(stamp),
        ..pending(
            &id,
            FileOperationType::Move,
            &format!(r"C:\photos\folder-{folder}\{padding}\{id}.jpg"),
            Some(&format!(r"C:\photos\sel\folder-{folder}\{padding}\{id}.jpg")),
            12345,
            stamp,
        )
    }
}

/// `count` Prepared + Committed pairs: half of the lines are droppable, well past the 25% gain and the 1 MiB threshold.
fn compactible_journal(count: i64) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..count {
        let prepared = prepared_entry(i, at(i * 2));
        let committed = JournalEntry {
            state: JournalState::Committed,
            timestamp_utc: jdt(at(i * 2 + 1)),
            ..prepared.clone()
        };
        out.extend(line_bytes(&prepared));
        out.extend(line_bytes(&committed));
    }
    out
}

fn startup_clock() -> FakeClock {
    FakeClock::at(at(3600))
}

fn infos(log: &RecordingLog) -> Vec<String> {
    messages(log, LogLevel::Info)
}

fn warnings(log: &RecordingLog) -> Vec<String> {
    messages(log, LogLevel::Warn)
}

fn errors(log: &RecordingLog) -> Vec<String> {
    messages(log, LogLevel::Error)
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalStartupRecoveryTests
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn run_async_completed_pending_move_committed_but_not_undoable() {
    // C#: JournalStartupRecoveryTests.RunAsync_CompletedPendingMove_CommittedButNotUndoable
    // TODO(W3-05 integration): the UndoService half (`MoveHistory` empty) is added by the lead once W3-05 lands.
    let disk = Arc::new(FakeFileSystem::new());
    put(&disk, r"C:\photos\sel\a.jpg", 5, at(-86_400));
    let paths = paths_at(JOURNAL);
    let journal = journal_with(disk.clone(), &paths, at(3600), None);
    journal
        .append(&pending(
            "m1",
            FileOperationType::Move,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\sel\a.jpg"),
            5,
            at(-300),
        ))
        .expect("append");

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert!(failed.is_empty());
    assert!(
        journal
            .read_pending_and_failed_operations()
            .expect("read")
            .iter()
            .all(|e| e.state != JournalState::Prepared)
    );
    let moves = journal.read_committed_moves().expect("read committed moves");
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].id, "m1");
}

#[test]
fn run_async_journal_has_committed_moves_undo_stack_empty_until_session_registers_a_move() {
    // C#: JournalStartupRecoveryTests.RunAsync_JournalHasCommittedMoves_UndoStackEmptyUntilSessionRegistersAMove
    // TODO(W3-05 integration): the `UndoService` half (the undo stack stays empty, an in-session Move is undoable) is added
    // by the lead once W3-05 lands. The journal half is ported here.
    let disk = Arc::new(FakeFileSystem::new());
    put(&disk, r"C:\photos\sel\old.jpg", 5, at(-86_400));
    let paths = paths_at(JOURNAL);
    let journal = journal_with(disk.clone(), &paths, at(3600), None);
    // Simulates a previous session/run: already Committed before this process's startup began.
    journal
        .append(&JournalEntry {
            state: JournalState::Committed,
            timestamp_utc: jdt(at(-86_400)),
            ..pending(
                "old",
                FileOperationType::Move,
                r"C:\photos\old.jpg",
                Some(r"C:\photos\sel\old.jpg"),
                5,
                at(-86_400),
            )
        })
        .expect("append");

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert!(failed.is_empty());
    assert!(journal.read_pending_and_failed_operations().expect("read").is_empty());
}

#[test]
fn run_async_unconfirmed_pending_returns_failed() {
    // C#: JournalStartupRecoveryTests.RunAsync_UnconfirmedPending_ReturnsFailed
    let disk = Arc::new(FakeFileSystem::new());
    put(&disk, r"C:\photos\b.jpg", 1, at(-86_400)); // recycle never happened
    let paths = paths_at(JOURNAL);
    let journal = journal_with(disk.clone(), &paths, at(3600), None);
    journal
        .append(&pending(
            "r1",
            FileOperationType::Recycle,
            r"C:\photos\b.jpg",
            None,
            1,
            at(-60),
        ))
        .expect("append");

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id, "r1");
    let failed_on_disk = journal.read_pending_and_failed_operations().expect("read");
    assert_eq!(
        failed_on_disk
            .iter()
            .filter(|e| e.state == JournalState::Failed)
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["r1"]
    );
}

#[test]
fn run_async_entry_prepared_after_start_left_pending() {
    // C#: JournalStartupRecoveryTests.RunAsync_EntryPreparedAfterStart_LeftPending
    let disk = Arc::new(FakeFileSystem::new());
    put(&disk, r"C:\photos\c.jpg", 1, at(-86_400));
    let paths = paths_at(JOURNAL);
    let journal = journal_with(disk.clone(), &paths, at(3600), None);
    // Prepared exactly at the startup moment: not judged mid-flight.
    journal
        .append(&pending(
            "live",
            FileOperationType::Move,
            r"C:\photos\c.jpg",
            Some(r"C:\photos\sel\c.jpg"),
            1,
            at(3600),
        ))
        .expect("append");

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert!(failed.is_empty());
    let still_pending = journal.read_pending_and_failed_operations().expect("read");
    assert_eq!(still_pending.len(), 1);
    assert_eq!(still_pending[0].id, "live");
}

#[test]
fn run_async_does_not_block_caller() {
    // C#: JournalStartupRecoveryTests.RunAsync_DoesNotBlockCaller
    // The journal read is blocked by an open-read hook on a channel. `spawn` must return while it is blocked. The wait is
    // bounded (10 s) so a synchronous implementation fails the assertion instead of hanging the run.
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let hooked = Arc::new(HookFileSystem::new(Arc::clone(&disk)));
    let fs: Arc<dyn FileSystem> = hooked.clone();
    let journal = Arc::new(journal_with(fs, &paths, at(3600), None));
    journal
        .append(&pending(
            "m1",
            FileOperationType::Move,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\sel\a.jpg"),
            5,
            at(-300),
        ))
        .expect("append");
    let (release, released) = mpsc::channel::<()>();
    let released = Mutex::new(released);
    let blocker: PathHook = Arc::new(move |_path: &Path| {
        let _ = released
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv_timeout(Duration::from_secs(10));
        None
    });
    hooked.set_open_read_hook(Some(blocker));

    let handle = startup_recovery::spawn(journal, &startup_clock(), None);
    let completed_before_release = handle.is_finished();
    release.send(()).expect("the worker is still waiting for the release");
    let _ = handle.wait();

    assert!(!completed_before_release);
}

#[test]
fn run_async_journal_read_fails_returns_empty() {
    // C#: JournalStartupRecoveryTests.RunAsync_JournalReadFails_ReturnsEmpty
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let hooked = Arc::new(HookFileSystem::new(Arc::clone(&disk)));
    let fs: Arc<dyn FileSystem> = hooked.clone();
    let journal = journal_with(fs, &paths, at(3600), None);
    journal
        .append(&pending(
            "m1",
            FileOperationType::Move,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\sel\a.jpg"),
            5,
            at(-300),
        ))
        .expect("append");
    let locked: PathHook = Arc::new(|_path: &Path| Some(io::Error::other("locked")));
    hooked.set_open_read_hook(Some(locked));

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert!(failed.is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalSmallGapsTests (the startup summary log)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn run_async_nothing_to_reconcile_logs_no_summary_warning() {
    // C#: JournalSmallGapsTests.RunAsync_NothingToReconcile_LogsNoSummaryWarning
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let journal = journal_with(disk, &paths, at(0), None);
    let log = RecordingLog::new();

    let failed = startup_recovery::run(&journal, &FakeClock::at(at(0)), Some(&log));

    assert!(failed.is_empty());
    assert!(warnings(&log).is_empty());
}

#[test]
fn run_async_one_pending_operation_reconciled_as_failed_logs_the_counts() {
    // C#: JournalSmallGapsTests.RunAsync_OnePendingOperationReconciledAsFailed_LogsTheCounts
    let disk = Arc::new(FakeFileSystem::new());
    put(&disk, r"C:\photos\a.jpg", 5, at(-86_400)); // source still there, destination missing
    let paths = paths_at(JOURNAL);
    let journal = journal_with(disk, &paths, at(0), None);
    journal
        .append(&pending(
            "a",
            FileOperationType::Move,
            r"C:\photos\a.jpg",
            Some(r"C:\sel\a.jpg"),
            5,
            at(-300),
        ))
        .expect("append");
    let log = RecordingLog::new();

    let failed = startup_recovery::run(&journal, &FakeClock::at(at(0)), Some(&log));

    assert_eq!(failed.len(), 1);
    assert_eq!(
        warnings(&log),
        ["Startup journal reconcile: 1 pending operation(s), 1 failed."]
    );
}

#[test]
fn run_async_one_pending_operation_reconciled_as_committed_logs_zero_failed() {
    // C#: JournalSmallGapsTests.RunAsync_OnePendingOperationReconciledAsCommitted_LogsZeroFailed
    let disk = Arc::new(FakeFileSystem::new());
    put(&disk, r"C:\sel\a.jpg", 5, at(-86_400)); // already moved
    let paths = paths_at(JOURNAL);
    let journal = journal_with(disk, &paths, at(0), None);
    journal
        .append(&pending(
            "a",
            FileOperationType::Move,
            r"C:\photos\a.jpg",
            Some(r"C:\sel\a.jpg"),
            5,
            at(-300),
        ))
        .expect("append");
    let log = RecordingLog::new();

    let failed = startup_recovery::run(&journal, &FakeClock::at(at(0)), Some(&log));

    assert!(failed.is_empty());
    assert_eq!(
        warnings(&log),
        ["Startup journal reconcile: 1 pending operation(s), 0 failed."]
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalCompactionTests, the RunAsync_* tests: in-memory twins, then native twins
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn run_async_big_journal_with_compaction_files_compacts_and_returns_failed() {
    // C#: JournalCompactionTests.RunAsync_BigJournalWithCompactionFiles_CompactsAndReturnsFailed (in-memory twin)
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let mut bytes = compactible_journal(COMPACTIBLE_COUNT);
    bytes.extend(line_bytes(&pending(
        "recycle-pending",
        FileOperationType::Recycle,
        r"C:\photos\still-there.jpg",
        None,
        1,
        at(-300),
    )));
    let before_len = bytes.len();
    disk.add_file_at(&paths.journal_file, &bytes, at(0));
    put(&disk, r"C:\photos\still-there.jpg", 1, at(0)); // a pending Recycle whose source still exists: must be Failed
    let compacting = Arc::new(MemCompaction { fs: Arc::clone(&disk) });
    let journal = journal_with(disk.clone(), &paths, at(3600), compaction(Hooked::new(compacting)));

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert!(failed.iter().any(|e| e.id == "recycle-pending"));
    let after_len = disk.file_content(&paths.journal_file).expect("journal kept").len();
    assert!(
        after_len < before_len,
        "expected the journal to have been compacted after reconcile"
    );
}

#[test]
#[ignore = "native: real disk, PhysicalJournalCompactionFiles (W3-02), 9000 pairs under TempRoot"]
fn run_async_big_journal_with_compaction_files_compacts_and_returns_failed_native() {
    // C#: JournalCompactionTests.RunAsync_BigJournalWithCompactionFiles_CompactsAndReturnsFailed (native twin)
    let root = TempRoot::new("startup-compaction-big");
    let journal_file = root.join("PhotoReview").join("Data").join("operations.jsonl");
    std::fs::create_dir_all(journal_file.parent().expect("dir")).expect("create dir");
    let paths = paths_at(&journal_file.to_string_lossy());
    let mut bytes = compactible_journal(COMPACTIBLE_COUNT);
    let still_there = root.join("still-there.jpg");
    std::fs::write(&still_there, b"x").expect("write photo");
    bytes.extend(line_bytes(&pending(
        "recycle-pending",
        FileOperationType::Recycle,
        &still_there.to_string_lossy(),
        None,
        1,
        DateTimeUtc::now(),
    )));
    std::fs::write(&paths.journal_file, &bytes).expect("write journal");
    let before_len = bytes.len();
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(FakeClock::at(DateTimeUtc::now())),
        OperationJournalOptions {
            compaction_files: compaction(Hooked::new(Arc::new(PhysicalJournalCompactionFiles))),
            ..Default::default()
        },
    );

    let failed = startup_recovery::run(&journal, &FakeClock::at(DateTimeUtc::now()), None);

    assert!(failed.iter().any(|e| e.id == "recycle-pending"));
    let after_len = std::fs::metadata(&paths.journal_file).expect("journal kept").len();
    assert!(usize::try_from(after_len).expect("fits") < before_len);
}

#[test]
fn run_async_compaction_throws_still_returns_failed() {
    // C#: JournalCompactionTests.RunAsync_CompactionThrows_StillReturnsFailed (in-memory twin)
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let mut bytes = compactible_journal(COMPACTIBLE_COUNT);
    bytes.extend(line_bytes(&pending(
        "recycle-pending-2",
        FileOperationType::Recycle,
        r"C:\photos\still-there2.jpg",
        None,
        1,
        at(-300),
    )));
    disk.add_file_at(&paths.journal_file, &bytes, at(0));
    put(&disk, r"C:\photos\still-there2.jpg", 1, at(0));
    let journal = journal_with(disk.clone(), &paths, at(3600), compaction(ThrowingCompaction));

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert!(failed.iter().any(|e| e.id == "recycle-pending-2"));
}

#[test]
#[ignore = "native: real disk, a compaction primitive that panics (W3-02 physical files for the journal)"]
fn run_async_compaction_throws_still_returns_failed_native() {
    // C#: JournalCompactionTests.RunAsync_CompactionThrows_StillReturnsFailed (native twin)
    let root = TempRoot::new("startup-compaction-throws");
    let journal_file = root.join("PhotoReview").join("Data").join("operations.jsonl");
    std::fs::create_dir_all(journal_file.parent().expect("dir")).expect("create dir");
    let paths = paths_at(&journal_file.to_string_lossy());
    let mut bytes = compactible_journal(COMPACTIBLE_COUNT);
    let still_there = root.join("still-there2.jpg");
    std::fs::write(&still_there, b"x").expect("write photo");
    bytes.extend(line_bytes(&pending(
        "recycle-pending-2",
        FileOperationType::Recycle,
        &still_there.to_string_lossy(),
        None,
        1,
        DateTimeUtc::now(),
    )));
    std::fs::write(&paths.journal_file, &bytes).expect("write journal");
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(FakeClock::at(DateTimeUtc::now())),
        OperationJournalOptions {
            compaction_files: compaction(ThrowingCompaction),
            ..Default::default()
        },
    );

    let failed = startup_recovery::run(&journal, &FakeClock::at(DateTimeUtc::now()), None);

    assert!(failed.iter().any(|e| e.id == "recycle-pending-2"));
}

#[test]
fn run_async_compacted_logs_info_with_byte_counts() {
    // C#: JournalCompactionTests.RunAsync_Compacted_LogsInfoWithByteCounts (in-memory twin)
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let bytes = compactible_journal(COMPACTIBLE_COUNT);
    disk.add_file_at(&paths.journal_file, &bytes, at(0));
    let journal = journal_with(
        disk.clone(),
        &paths,
        at(3600),
        compaction(Hooked::new(Arc::new(MemCompaction { fs: Arc::clone(&disk) }))),
    );
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &startup_clock(), Some(&log));

    let after = disk.file_content(&paths.journal_file).expect("journal kept").len();
    assert!(after < bytes.len());
    assert_eq!(
        infos(&log),
        [format!("Journal compacted: {} -> {after} bytes.", bytes.len())]
    );
    assert!(warnings(&log).is_empty());
    assert!(errors(&log).is_empty());
}

#[test]
#[ignore = "native: real disk, PhysicalJournalCompactionFiles (W3-02)"]
fn run_async_compacted_logs_info_with_byte_counts_native() {
    // C#: JournalCompactionTests.RunAsync_Compacted_LogsInfoWithByteCounts (native twin)
    let root = TempRoot::new("startup-compaction-info");
    let journal_file = root.join("PhotoReview").join("Data").join("operations.jsonl");
    std::fs::create_dir_all(journal_file.parent().expect("dir")).expect("create dir");
    let paths = paths_at(&journal_file.to_string_lossy());
    let bytes = compactible_journal(COMPACTIBLE_COUNT);
    std::fs::write(&paths.journal_file, &bytes).expect("write journal");
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(FakeClock::at(DateTimeUtc::now())),
        OperationJournalOptions {
            compaction_files: compaction(Hooked::new(Arc::new(PhysicalJournalCompactionFiles))),
            ..Default::default()
        },
    );
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &FakeClock::at(DateTimeUtc::now()), Some(&log));

    let after = std::fs::metadata(&paths.journal_file).expect("journal kept").len();
    assert!(usize::try_from(after).expect("fits") < bytes.len());
    assert_eq!(
        infos(&log),
        [format!("Journal compacted: {} -> {after} bytes.", bytes.len())]
    );
    assert!(warnings(&log).is_empty());
    assert!(errors(&log).is_empty());
}

#[test]
fn run_async_compaction_failed_outcome_logs_warning_with_the_error() {
    // C#: JournalCompactionTests.RunAsync_CompactionFailedOutcome_LogsWarningWithTheError (in-memory twin)
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let bytes = compactible_journal(COMPACTIBLE_COUNT);
    disk.add_file_at(&paths.journal_file, &bytes, at(0));
    let mut failing = Hooked::new(Arc::new(MemCompaction { fs: Arc::clone(&disk) }));
    failing.fail_replace = true;
    let journal = journal_with(disk.clone(), &paths, at(3600), compaction(failing));
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &startup_clock(), Some(&log));

    let warning = warnings(&log);
    assert_eq!(warning.len(), 1);
    assert!(
        warning[0].starts_with("Journal compaction failed (journal unchanged): "),
        "{}",
        warning[0]
    );
    assert!(warning[0].contains("simulated crash before replace"), "{}", warning[0]);
    assert!(infos(&log).is_empty());
    assert_eq!(disk.file_content(&paths.journal_file).expect("journal kept"), bytes);
}

#[test]
#[ignore = "native: real disk, replace fails before the rename (W3-02)"]
fn run_async_compaction_failed_outcome_logs_warning_with_the_error_native() {
    // C#: JournalCompactionTests.RunAsync_CompactionFailedOutcome_LogsWarningWithTheError (native twin)
    let root = TempRoot::new("startup-compaction-failed");
    let journal_file = root.join("PhotoReview").join("Data").join("operations.jsonl");
    std::fs::create_dir_all(journal_file.parent().expect("dir")).expect("create dir");
    let paths = paths_at(&journal_file.to_string_lossy());
    let bytes = compactible_journal(COMPACTIBLE_COUNT);
    std::fs::write(&paths.journal_file, &bytes).expect("write journal");
    let mut failing = Hooked::new(Arc::new(PhysicalJournalCompactionFiles));
    failing.fail_replace = true;
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(FakeClock::at(DateTimeUtc::now())),
        OperationJournalOptions {
            compaction_files: compaction(failing),
            ..Default::default()
        },
    );
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &FakeClock::at(DateTimeUtc::now()), Some(&log));

    let warning = warnings(&log);
    assert_eq!(warning.len(), 1);
    assert!(
        warning[0].starts_with("Journal compaction failed (journal unchanged): "),
        "{}",
        warning[0]
    );
    assert!(warning[0].contains("simulated crash before replace"), "{}", warning[0]);
    assert!(infos(&log).is_empty());
    assert_eq!(std::fs::read(&paths.journal_file).expect("journal kept"), bytes);
}

#[test]
fn run_async_compaction_busy_logs_skipped_info() {
    // C#: JournalCompactionTests.RunAsync_CompactionBusy_LogsSkippedInfo (in-memory twin: sharing violation on open)
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let bytes = compactible_journal(COMPACTIBLE_COUNT);
    disk.add_file_at(&paths.journal_file, &bytes, at(0));
    let mut busy = Hooked::new(Arc::new(MemCompaction { fs: Arc::clone(&disk) }));
    busy.busy = true;
    let journal = journal_with(disk.clone(), &paths, at(3600), compaction(busy));
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &startup_clock(), Some(&log));

    assert_eq!(infos(&log), ["Journal compaction skipped (Busy); journal unchanged."]);
    assert!(warnings(&log).is_empty());
    assert_eq!(disk.file_content(&paths.journal_file).expect("journal kept"), bytes);
}

#[test]
#[cfg(windows)]
#[ignore = "native: real disk, a writer holds the journal open with FILE_SHARE_READ (W3-02)"]
fn run_async_compaction_busy_logs_skipped_info_native() {
    // C#: JournalCompactionTests.RunAsync_CompactionBusy_LogsSkippedInfo (native twin: a real append handle)
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    let root = TempRoot::new("startup-compaction-busy");
    let journal_file = root.join("PhotoReview").join("Data").join("operations.jsonl");
    std::fs::create_dir_all(journal_file.parent().expect("dir")).expect("create dir");
    let paths = paths_at(&journal_file.to_string_lossy());
    let bytes = compactible_journal(COMPACTIBLE_COUNT);
    std::fs::write(&paths.journal_file, &bytes).expect("write journal");
    let writer: Arc<Mutex<Option<std::fs::File>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&writer);
    let journal_path = paths.journal_file.clone();
    let hold_append = Hooked::new(Arc::new(PhysicalJournalCompactionFiles)).before_lock(move || {
        let handle = std::fs::OpenOptions::new()
            .append(true)
            .share_mode(FILE_SHARE_READ)
            .open(&journal_path)
            .expect("the writer opens the journal");
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
    });
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(FakeClock::at(DateTimeUtc::now())),
        OperationJournalOptions {
            compaction_files: compaction(hold_append),
            ..Default::default()
        },
    );
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &FakeClock::at(DateTimeUtc::now()), Some(&log));
    drop(writer.lock().unwrap_or_else(PoisonError::into_inner).take());

    assert_eq!(infos(&log), ["Journal compaction skipped (Busy); journal unchanged."]);
    assert!(warnings(&log).is_empty());
    assert_eq!(std::fs::read(&paths.journal_file).expect("journal kept"), bytes);
}

#[test]
fn run_async_compaction_changed_logs_skipped_info() {
    // C#: JournalCompactionTests.RunAsync_CompactionChanged_LogsSkippedInfo (in-memory twin: another process rewrote it)
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    disk.add_file_at(&paths.journal_file, &compactible_journal(COMPACTIBLE_COUNT), at(0));
    let rewritten = compactible_journal(10); // smaller: below the threshold, as a real compaction would leave it
    let writer = Arc::clone(&disk);
    let journal_path = paths.journal_file.clone();
    let changing = Hooked::new(Arc::new(MemCompaction { fs: Arc::clone(&disk) })).before_lock(move || {
        writer.add_file_at(&journal_path, &rewritten, at(0));
    });
    let journal = journal_with(disk.clone(), &paths, at(3600), compaction(changing));
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &startup_clock(), Some(&log));

    assert_eq!(
        infos(&log),
        ["Journal compaction skipped (Changed); journal unchanged."]
    );
}

#[test]
#[ignore = "native: real disk, another writer rewrites the journal before the lock (W3-02)"]
fn run_async_compaction_changed_logs_skipped_info_native() {
    // C#: JournalCompactionTests.RunAsync_CompactionChanged_LogsSkippedInfo (native twin)
    let root = TempRoot::new("startup-compaction-changed");
    let journal_file = root.join("PhotoReview").join("Data").join("operations.jsonl");
    std::fs::create_dir_all(journal_file.parent().expect("dir")).expect("create dir");
    let paths = paths_at(&journal_file.to_string_lossy());
    std::fs::write(&paths.journal_file, compactible_journal(COMPACTIBLE_COUNT)).expect("write journal");
    let rewritten = compactible_journal(10);
    let journal_path = paths.journal_file.clone();
    let changing = Hooked::new(Arc::new(PhysicalJournalCompactionFiles)).before_lock(move || {
        std::fs::write(&journal_path, &rewritten).expect("another process rewrites the journal");
    });
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(FakeClock::at(DateTimeUtc::now())),
        OperationJournalOptions {
            compaction_files: compaction(changing),
            ..Default::default()
        },
    );
    let log = RecordingLog::new();

    startup_recovery::run(&journal, &FakeClock::at(DateTimeUtc::now()), Some(&log));

    assert_eq!(
        infos(&log),
        ["Journal compaction skipped (Changed); journal unchanged."]
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalGroupEvidenceAndNulMemberTests: the W2-FA-04 startup test (a repair append fails, compaction keeps the evidence)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn startup_repair_append_fails_compaction_keeps_evidence_next_start_surfaces_failed_group() {
    // C#: JournalGroupEvidenceAndNulMemberTests.Startup_RepairAppendFails_CompactionKeepsEvidence_NextStartSurfacesFailedGroup
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    put(&disk, r"C:\selected\a.jpg", 10, at(0)); // first member moved
    put(&disk, r"C:\photos\a.cr2", 100, at(0)); // second member never moved
    let members = [
        JournalGroupMember {
            source: r"C:\photos\a.jpg".to_owned(),
            destination: Some(r"C:\selected\a.jpg".to_owned()),
            size: 10,
            last_write_utc: jdt(at(0)),
            permanent: false,
        },
        JournalGroupMember {
            source: r"C:\photos\a.cr2".to_owned(),
            destination: Some(r"C:\selected\a.cr2".to_owned()),
            size: 100,
            last_write_utc: jdt(at(0)),
            permanent: false,
        },
    ];
    let prepared = JournalEntry {
        group_id: Some("capture-g".to_owned()),
        group_members: Some(members.to_vec()),
        ..pending(
            "g",
            FileOperationType::Move,
            &members[0].source,
            members[0].destination.as_deref(),
            10,
            at(0),
        )
    };
    // Filler: droppable Prepared/Committed pairs, enough for a compaction past 1 MiB and 25% gain.
    let mut bytes = Vec::new();
    let padding = "p".repeat(60);
    for i in 0..4000 {
        let filler = pending(
            &format!("f{i:05}"),
            FileOperationType::Move,
            &format!(r"C:\photos\{padding}\f{i:05}.jpg"),
            Some(&format!(r"C:\selected\{padding}\f{i:05}.jpg")),
            1,
            at(0),
        );
        bytes.extend(line_bytes(&filler));
        bytes.extend(line_bytes(&JournalEntry {
            state: JournalState::Committed,
            timestamp_utc: jdt(at(1)),
            ..filler
        }));
    }
    bytes.extend(line_bytes(&prepared));
    bytes.extend(line_bytes(&JournalEntry {
        state: JournalState::Committed,
        timestamp_utc: jdt(at(30)),
        group_id: None,
        group_members: None,
        ..prepared.clone()
    }));
    disk.add_file_at(&paths.journal_file, &bytes, at(0));
    let size_before = disk.peek_stat(&paths.journal_file).expect("journal").length;
    let journal = journal_with(
        disk.clone(),
        &paths,
        at(60),
        compaction(MemCompaction { fs: Arc::clone(&disk) }),
    );

    disk.fail(FsOp::OpenAppend, &paths.journal_file, io::ErrorKind::Other); // the repair append fails (disk full / antivirus lock)
    let failed_at_first_start = startup_recovery::run(&journal, &FakeClock::at(at(60)), None);
    disk.clear_faults();

    assert!(failed_at_first_start.is_empty());
    let size_after = disk.peek_stat(&paths.journal_file).expect("journal").length;
    assert!(
        size_after < size_before,
        "the filler pairs must have been compacted away"
    );
    let surfaced = journal
        .reconcile_pending_operations(None)
        .expect("the next start, appends work again");
    assert_eq!(surfaced.len(), 1);
    assert_eq!(surfaced[0].id, "g");
    assert_eq!(surfaced[0].state, JournalState::Failed);
    let failed_one = journal.read_pending_and_failed_operations().expect("read");
    assert_eq!(
        failed_one
            .iter()
            .find(|e| e.state == JournalState::Failed)
            .and_then(|e| e.group_members.clone()),
        Some(members.to_vec())
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/journal-startup-recovery.json (11 cases, oracle `journal-startup-recovery`)
// ---------------------------------------------------------------------------------------------------------------------

/// The `log` entries of a golden case, compared with what `RecordingLog` captured (`Error` entries carry `": {error}"`).
fn assert_log_matches(name: &str, expected: &[Value], actual: &[(LogLevel, String)]) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "case {name}: log entry count, got {actual:?}"
    );
    for (want, (level, text)) in expected.iter().zip(actual) {
        let want_level = match str_of(want, "level") {
            "Info" => LogLevel::Info,
            "Warn" => LogLevel::Warn,
            "Error" => LogLevel::Error,
            other => panic!("case {name}: unknown golden log level {other}"),
        };
        assert_eq!(*level, want_level, "case {name}: log level");
        let message = str_of(want, "message");
        if want["exception_type"].is_null() {
            assert_eq!(text, message, "case {name}: log message");
        } else {
            assert!(
                text.starts_with(&format!("{message}: ")),
                "case {name}: expected `{message}: <error>`, got `{text}`"
            );
        }
    }
}

#[test]
fn golden_journal_startup_recovery_cases_match_csharp() {
    // C#: oracle `journal-startup-recovery` (JournalStartupRecovery.RunAsync with a recording log, no compaction files),
    // tests/golden/journal-startup-recovery.json
    let golden = golden::load("journal-startup-recovery");
    for case in &golden.cases {
        let name = str_of(case, "name");
        eprintln!("journal-startup-recovery golden case: {name}");
        let disk = Arc::new(FakeFileSystem::new());
        let paths = paths_at(str_of(case, "journal_path"));
        for file in case["files"].as_array().expect("files") {
            put(
                &disk,
                str_of(file, "path"),
                usize::try_from(i64_of(file, "size")).expect("size"),
                utc(str_of(file, "time")),
            );
        }
        let journal_path = paths.journal_file.clone();
        if let Some(lines) = case["journal"].as_array() {
            let text: Vec<&str> = lines.iter().map(|l| l.as_str().expect("journal line")).collect();
            let mut bytes = String::new();
            for line in &text {
                bytes.push_str(line);
                bytes.push_str("\r\n");
            }
            disk.add_file_at(&journal_path, bytes.as_bytes(), utc(str_of(case, "clock")));
        }
        if case["read_fault"].as_bool() == Some(true) {
            disk.fail(FsOp::OpenRead, &journal_path, io::ErrorKind::Other);
        }
        if let Some(count) = case.get("append_failures").and_then(Value::as_u64) {
            disk.fail_times(
                FsOp::OpenAppend,
                &journal_path,
                io::ErrorKind::Other,
                usize::try_from(count).expect("count"),
            );
        }
        let registry = Arc::new(InProcessLiveOperationRegistry::new());
        let markers: Vec<LiveMarker> = case["live"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|id| registry.begin(id.as_str().expect("id")))
            .collect();
        let journal_options = OperationJournalOptions {
            live_operations: Some(Arc::clone(&registry) as Arc<dyn LiveOperationRegistry>),
            ..Default::default()
        };
        let journal = OperationJournal::with_options(
            &paths,
            disk.clone(),
            Arc::new(FakeClock::at(utc(str_of(case, "clock")))),
            OperationJournalOptions {
                append_retry_delay: Some(Arc::new(|_: Duration| {})),
                ..journal_options
            },
        );
        let startup = FakeClock::at(utc(case["startup_clock"]
            .as_str()
            .unwrap_or_else(|| str_of(case, "clock"))));
        let log = RecordingLog::new();
        let with_log = case["with_log"].as_bool().unwrap_or(true);

        let diagnostics: Option<&dyn Log> = if with_log { Some(&log) } else { None };
        let failed = startup_recovery::run(&journal, &startup, diagnostics);

        let actual_failed: Vec<String> = failed.iter().map(JournalEntry::to_line).collect();
        let expected_failed: Vec<String> = case["failed"]
            .as_array()
            .expect("failed")
            .iter()
            .map(|l| l.as_str().expect("line").to_owned())
            .collect();
        assert_eq!(actual_failed, expected_failed, "case {name}: failed lines");
        if with_log {
            let expected_log = case["log"].as_array().expect("log");
            let captured = log.entries();
            assert_log_matches(name, expected_log, &captured);
        } else {
            assert!(log.entries().is_empty(), "case {name}: nothing is logged without a log");
        }
        match case["journal_after"].as_str() {
            None => assert!(
                disk.file_content(&journal_path).is_none(),
                "case {name}: no journal file"
            ),
            Some(after) => assert_eq!(
                String::from_utf8(disk.file_content(&journal_path).unwrap_or_default()).expect("utf-8"),
                after,
                "case {name}: journal bytes after startup"
            ),
        }
        drop(markers);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// spawn(): the worker thread and the spawn-time cutoff (C# RunAsync captures the start on the caller thread)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn spawn_reads_the_clock_on_the_calling_thread_and_names_the_worker() {
    // C#: JournalStartupRecoveryTests.RunAsync_DoesNotBlockCaller (cutoff side); RECOVERY_THREAD_NAME is the thread name
    let disk = Arc::new(FakeFileSystem::new());
    let paths = paths_at(JOURNAL);
    let journal = Arc::new(journal_with(disk.clone(), &paths, at(3600), None));
    assert_eq!(RECOVERY_THREAD_NAME, "PhotoReview.JournalRecovery");

    let handle = startup_recovery::spawn(journal, &startup_clock(), None);

    assert!(handle.wait().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------
// Parity guards (W3-03 verifier, repair round 1): C# RunAsync's try covers the summary warning, compaction runs after a
// successful reconcile only, and the spawned handle reports its result and completion.
// ---------------------------------------------------------------------------------------------------------------------

/// A `Log` whose `warn` panics (C#: an `ILog` whose `Warn` throws); `error` records the message.
#[derive(Default)]
struct WarnPanicsLog {
    errors: Mutex<Vec<String>>,
}

impl Log for WarnPanicsLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, _message: &str) {
        panic!("warn sink failed");
    }

    fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
        self.errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message.to_owned());
    }
}

/// One pending Move whose source is still on disk and whose destination is missing: reconciled as Failed.
fn unconfirmed_move(disk: &FakeFileSystem, id: &str) -> JournalEntry {
    put(disk, r"C:\photos\a.jpg", 5, at(-86_400));
    pending(
        id,
        FileOperationType::Move,
        r"C:\photos\a.jpg",
        Some(r"C:\sel\a.jpg"),
        5,
        at(-300),
    )
}

#[test]
fn run_async_summary_warning_that_panics_is_caught_like_the_csharp_try() {
    // C#: JournalStartupRecovery.RunAsync, `log?.Warn(...)` sits inside `try { } catch (Exception)`: "Never throws".
    let disk = Arc::new(FakeFileSystem::new());
    let journal = journal_with(disk.clone(), &paths_at(JOURNAL), at(0), None);
    journal.append(&unconfirmed_move(&disk, "a")).expect("append");
    let log = WarnPanicsLog::default();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        startup_recovery::run(&journal, &startup_clock(), Some(&log))
    }));

    let failed = result.expect("run never panics: the throwing Warn is caught");
    assert!(failed.is_empty(), "C# returns [] from the catch");
    assert_eq!(
        *log.errors.lock().unwrap_or_else(PoisonError::into_inner),
        ["Startup journal reconcile failed"]
    );
}

#[test]
fn spawn_wait_returns_the_failed_entries() {
    // C#: `await JournalStartupRecovery.RunAsync(...)` yields the entries reconcile marked Failed.
    let disk = Arc::new(FakeFileSystem::new());
    let journal = Arc::new(journal_with(disk.clone(), &paths_at(JOURNAL), at(0), None));
    journal.append(&unconfirmed_move(&disk, "a")).expect("append");

    let failed = startup_recovery::spawn(journal, &startup_clock(), None).wait();

    assert_eq!(failed.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(failed[0].state, JournalState::Failed);
}

/// A `Log` whose `warn` reports that it was reached, then blocks until the test releases it.
struct GateLog {
    reached: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl Log for GateLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, _message: &str) {
        let _ = self.reached.lock().unwrap_or_else(PoisonError::into_inner).send(());
        let _ = self.release.lock().unwrap_or_else(PoisonError::into_inner).recv();
    }

    fn error(&self, _message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {}
}

#[test]
fn spawn_handle_is_finished_only_once_the_work_is_done() {
    // C#: Task.IsCompleted is false while the worker runs and true once it returned.
    let disk = Arc::new(FakeFileSystem::new());
    let journal = Arc::new(journal_with(disk.clone(), &paths_at(JOURNAL), at(0), None));
    journal.append(&unconfirmed_move(&disk, "a")).expect("append");
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let log: Arc<dyn Log> = Arc::new(GateLog {
        reached: Mutex::new(reached_tx),
        release: Mutex::new(release_rx),
    });

    let handle = startup_recovery::spawn(journal, &startup_clock(), Some(log));
    reached_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the worker reaches the summary warning");
    assert!(!handle.is_finished(), "the worker is still inside the summary warning");
    release_tx.send(()).expect("release the worker");

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !handle.is_finished() {
        assert!(std::time::Instant::now() < deadline, "is_finished never became true");
        std::thread::yield_now();
    }
    assert_eq!(handle.wait().len(), 1);
}

#[test]
fn run_async_reconcile_panic_is_logged_with_the_panic_text() {
    // C#: the catch logs the exception object, message included ("{message}: {error}" in RecordingLog).
    let disk = Arc::new(FakeFileSystem::new());
    let hooked = Arc::new(HookFileSystem::new(Arc::clone(&disk)));
    let journal = journal_with(hooked.clone(), &paths_at(JOURNAL), at(0), None);
    journal.append(&unconfirmed_move(&disk, "a")).expect("append");
    let boom: PathHook = Arc::new(|_path: &Path| panic!("journal read exploded"));
    hooked.set_open_read_hook(Some(boom));
    let log = RecordingLog::new();

    let failed = startup_recovery::run(&journal, &startup_clock(), Some(&log));

    assert!(failed.is_empty());
    let errors = errors(&log);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("Startup journal reconcile failed"), "{errors:?}");
    assert!(errors[0].contains("journal read exploded"), "{errors:?}");
}

#[test]
fn run_async_compaction_runs_after_reconcile_so_the_settled_prepared_line_is_dropped() {
    // C#: RunAsync "the quiet moment: after reconcile appended its outcomes": the outcome is part of the compacted file.
    let disk = Arc::new(FakeFileSystem::new());
    let mut bytes = compactible_journal(COMPACTIBLE_COUNT);
    // A pending Recycle whose source is gone: reconcile appends its Committed outcome.
    bytes.extend(line_bytes(&pending(
        "rp",
        FileOperationType::Recycle,
        r"C:\photos\gone.jpg",
        None,
        5,
        at(-300),
    )));
    disk.add_file_at(JOURNAL, &bytes, at(0));
    let journal = journal_with(
        disk.clone(),
        &paths_at(JOURNAL),
        at(0),
        compaction(MemCompaction { fs: Arc::clone(&disk) }),
    );

    let failed = startup_recovery::run(&journal, &startup_clock(), None);

    assert!(failed.is_empty());
    let after = disk.file_content(JOURNAL).expect("journal");
    assert!(after.len() < bytes.len(), "compacted");
    let text = String::from_utf8_lossy(&after);
    assert_eq!(
        text.matches("\"Id\":\"rp\"").count(),
        1,
        "only the Committed outcome of rp survives the compaction"
    );
}

#[test]
fn run_async_failed_reconcile_skips_the_compaction() {
    // C#: the exception leaves Task.Run before Compact runs.
    let disk = Arc::new(FakeFileSystem::new());
    let bytes = compactible_journal(COMPACTIBLE_COUNT);
    disk.add_file_at(JOURNAL, &bytes, at(0));
    let hooked = Arc::new(HookFileSystem::new(Arc::clone(&disk)));
    let journal = journal_with(
        hooked.clone(),
        &paths_at(JOURNAL),
        at(0),
        compaction(MemCompaction { fs: Arc::clone(&disk) }),
    );
    // Only the first read fails (the reconcile's); a compaction would read the journal again afterwards.
    let calls = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&calls);
    let first_fails: PathHook = Arc::new(move |_path: &Path| {
        let mut n = counter.lock().unwrap_or_else(PoisonError::into_inner);
        *n += 1;
        (*n == 1).then(|| io::Error::other("locked"))
    });
    hooked.set_open_read_hook(Some(first_fails));
    let log = RecordingLog::new();

    let failed = startup_recovery::run(&journal, &startup_clock(), Some(&log));

    assert!(failed.is_empty());
    assert_eq!(
        disk.file_content(JOURNAL).expect("journal"),
        bytes,
        "journal must be untouched"
    );
    let entries = log.entries();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert!(
        entries[0].1.starts_with("Startup journal reconcile failed"),
        "{entries:?}"
    );
}
