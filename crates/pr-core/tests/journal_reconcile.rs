//! Red tests of W3-03 (`file_actions::reconcile`): the startup reconcile rules of `OperationJournal`, ported from the C#
//! tests of `tests/PhotoReview.Core.Tests/FileActions` and `OperationJournalTests.cs` (baseline d92d6aae). Every test
//! names the C# test it mirrors (`// C#: Class.Method`). They compile against the stubs and FAIL until the bodies land
//! (`todo!()` panics). Contract: `contracts/W3-03.md`.
//!
//! The journal is the REAL `OperationJournal` (W3-01) over `FakeFileSystem`; `#[ignore = "native: ..."]` twins run on the
//! real disk (`cargo test -- --ignored`). No sleeps: blocking is done with channels and `Barrier`.
//!
//! Test-local decorators (`CheckFs`, `HookedRegistry`): the stub's `pr-test-support` has no `file_exists` hook yet
//! (contract open question 3), so the C# `InterceptingFs` and `ArgumentException` fakes are written here, over the
//! real fake.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{
    AppendStream, FileStat, FileSystem, InProcessLiveOperationRegistry, LiveMarker, LiveOperationRegistry, Log,
    ReadSeek, SystemClock,
};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors::{PENDING_UNCONFIRMED, SOURCE_STILL_EXISTS_AFTER_RECOVERY};
use pr_core::file_actions::journal::{
    AppendRetryDelay, EnglishJournalTexts, JournalTexts, OperationJournal, OperationJournalOptions,
};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember, try_parse};
use pr_core::file_actions::reconcile::SETTLED_BY_OLDER_BUILD_TEXT;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::golden::{self, i64_of, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, HookFileSystem, PathHook, RecordingLog, TempRoot};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// The C# `Stamp` of these tests: `2026-09-29T00:00:00Z` plus `secs` (C# `Stamp.AddSeconds`).
fn at(secs: i64) -> DateTimeUtc {
    let base = utc("2026-09-29T00:00:00Z");
    DateTimeUtc::from_ticks(base.ticks() + secs * TICKS_PER_SECOND).expect("stamp in range")
}

fn utc(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text)
        .unwrap_or_else(|| panic!("bad stamp {text}"))
        .0
}

fn jdt(value: DateTimeUtc) -> JournalDateTime {
    JournalDateTime::utc(value)
}

/// C# `new JournalEntry(id, type, state, source, destination, size, lastWrite, timestamp)` with no group and no flags.
fn entry(
    id: &str,
    op: FileOperationType,
    state: JournalState,
    source: &str,
    destination: Option<&str>,
    size: i64,
    stamp: DateTimeUtc,
) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: op,
        state,
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size,
        last_write_utc: jdt(stamp),
        timestamp_utc: jdt(stamp),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// C# `new JournalGroupMember(source, destination, size, lastWrite)`.
fn member(source: &str, destination: Option<&str>, size: i64, stamp: DateTimeUtc) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size,
        last_write_utc: jdt(stamp),
        permanent: false,
    }
}

/// A group entry whose head fields come from the first member (C# `new JournalEntry(..., GroupId, GroupMembers)`).
fn group(
    id: &str,
    op: FileOperationType,
    state: JournalState,
    group_id: &str,
    members: &[JournalGroupMember],
) -> JournalEntry {
    let first = &members[0];
    JournalEntry {
        group_id: Some(group_id.to_owned()),
        group_members: Some(members.to_vec()),
        ..entry(
            id,
            op,
            state,
            &first.source,
            first.destination.as_deref(),
            first.size,
            first.last_write_utc.value,
        )
    }
}

/// `AppPaths` whose journal file is exactly `journal` (the C# `FakeAppPaths`).
fn paths_at(journal: &str) -> AppPaths {
    let mut paths = AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false);
    paths.journal_file = PathBuf::from(journal);
    paths
}

const JOURNAL: &str = r"C:\Users\test\AppData\Local\PhotoReview\Data\operations.jsonl";

/// Options every test uses: no real sleeps between append attempts, diagnostics captured instead of written to the
/// real `FileLog`.
fn quiet() -> OperationJournalOptions {
    let delay: AppendRetryDelay = Arc::new(|_: Duration| {});
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    OperationJournalOptions {
        append_retry_delay: Some(delay),
        log: Some(log),
        ..Default::default()
    }
}

/// The in-memory world of one test: the fake disk and the journal path under it.
struct World {
    disk: Arc<FakeFileSystem>,
    paths: AppPaths,
}

impl World {
    fn new() -> Self {
        Self {
            disk: Arc::new(FakeFileSystem::new()),
            paths: paths_at(JOURNAL),
        }
    }

    fn fs(&self) -> Arc<dyn FileSystem> {
        self.disk.clone()
    }

    /// A file of `size` bytes with the given write time (C# `_fs.AddFile(path, content, time)`).
    fn put(&self, path: &str, size: usize, stamp: DateTimeUtc) {
        self.disk.add_file_at(path, &vec![b'x'; size], stamp);
    }

    fn journal(&self, clock: DateTimeUtc) -> OperationJournal {
        OperationJournal::with_options(&self.paths, self.fs(), Arc::new(FakeClock::at(clock)), quiet())
    }

    fn journal_bytes(&self) -> Vec<u8> {
        self.disk.file_content(&self.paths.journal_file).unwrap_or_default()
    }

    fn journal_line_count(&self) -> usize {
        self.journal_bytes()
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .count()
    }
}

/// C# `InterceptingFs`-style decorator over the fake disk. It adds two test seams the stub's `pr-test-support` does not
/// have yet: a one-shot action on the first `file_exists` of one path (run BEFORE the check, with no lock held, so it may
/// append through another journal), and file checks that panic for some paths (C# `ArgumentException` from a throwing
/// file check, the golden `stat_throws` key).
/// The one-shot action of [`CheckFs::with_first_exists`]: the path it waits for, and what to run.
type FirstExistsHook = (PathBuf, Box<dyn FnOnce() + Send>);

struct CheckFs {
    inner: Arc<dyn FileSystem>,
    on_first_exists: Mutex<Option<FirstExistsHook>>,
    panic_on: Vec<PathBuf>,
}

impl CheckFs {
    fn new(inner: Arc<dyn FileSystem>, panic_on: &[&str]) -> Self {
        Self {
            inner,
            on_first_exists: Mutex::new(None),
            panic_on: panic_on.iter().map(PathBuf::from).collect(),
        }
    }

    fn with_first_exists(self, path: &str, action: impl FnOnce() + Send + 'static) -> Self {
        let boxed: Box<dyn FnOnce() + Send> = Box::new(action);
        *self.on_first_exists.lock().unwrap_or_else(PoisonError::into_inner) = Some((PathBuf::from(path), boxed));
        self
    }

    fn same(a: &Path, b: &Path) -> bool {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    }

    fn check(&self, path: &Path) {
        if self.panic_on.iter().any(|bad| Self::same(bad, path)) {
            panic!("simulated file check failure for {}", path.display());
        }
    }
}

impl FileSystem for CheckFs {
    fn file_exists(&self, path: &Path) -> bool {
        let action = {
            let mut slot = self.on_first_exists.lock().unwrap_or_else(PoisonError::into_inner);
            let hit = matches!(slot.as_ref(), Some((target, _)) if Self::same(target, path));
            if hit {
                slot.take().map(|(_, action)| action)
            } else {
                None
            }
        };
        if let Some(action) = action {
            action();
        }
        self.check(path);
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.check(path);
        self.inner.get_file_stat(path)
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.inner.delete(path)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.inner.open_read_shared(path, buffer_size)
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.inner.open_append(path, durable)
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.inner.write_all_text_atomic(path, text, durable)
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.inner.read_all_text(path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_files(directory, pattern)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_directories(directory)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.inner.create_directory(path)
    }
}

/// C# `InterceptingFs`-style registry (`OnFirstIsLive`): a one-shot action on the first `is_live` call, then the real
/// in-process registry. Used to let another writer change the journal "meanwhile".
struct HookedRegistry {
    inner: InProcessLiveOperationRegistry,
    on_first_is_live: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl HookedRegistry {
    fn new() -> Self {
        Self {
            inner: InProcessLiveOperationRegistry::new(),
            on_first_is_live: Mutex::new(None),
        }
    }
}

impl LiveOperationRegistry for HookedRegistry {
    fn begin(&self, operation_id: &str) -> LiveMarker {
        self.inner.begin(operation_id)
    }

    fn is_live(&self, operation_id: &str) -> bool {
        let action = self
            .on_first_is_live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(action) = action {
            action();
        }
        self.inner.is_live(operation_id)
    }
}

/// The reconcile of one journal instance (C# `ReconcilePendingOperations()` with no cutoff).
fn reconcile(journal: &OperationJournal) -> Vec<JournalEntry> {
    journal
        .reconcile_pending_operations(None)
        .expect("reconcile reads the journal")
}

/// C# `ReadPendingOperations()`: the latest Prepared entries.
fn pending(journal: &OperationJournal) -> Vec<JournalEntry> {
    journal
        .read_pending_and_failed_operations()
        .expect("read journal")
        .into_iter()
        .filter(|e| e.state == JournalState::Prepared)
        .collect()
}

/// C# `ReadFailedOperations()`: the latest Failed entries.
fn failed(journal: &OperationJournal) -> Vec<JournalEntry> {
    journal
        .read_pending_and_failed_operations()
        .expect("read journal")
        .into_iter()
        .filter(|e| e.state == JournalState::Failed)
        .collect()
}

fn single<T>(mut items: Vec<T>) -> T {
    assert_eq!(items.len(), 1, "expected exactly one entry");
    items.remove(0)
}

fn fixture(name: &str) -> String {
    let path = golden::repo_root()
        .join("tests")
        .join("fixtures")
        .join("journal-older-build")
        .join(format!("{name}.jsonl"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalGroupEvidenceAndNulMemberTests (W2-FA-07 part; the W2-FA-04 startup test is in journal_startup_recovery.rs)
// ---------------------------------------------------------------------------------------------------------------------

/// The members of the evidence / NUL tests: `photos\a.jpg -> selected\a.jpg` (10 bytes) and `photos\a.cr2 -> selected\a.cr2`.
fn evidence_members(stamp: DateTimeUtc) -> [JournalGroupMember; 2] {
    [
        member(r"C:\photos\a.jpg", Some(r"C:\selected\a.jpg"), 10, stamp),
        member(r"C:\photos\a.cr2", Some(r"C:\selected\a.cr2"), 100, stamp),
    ]
}

/// What an older build appends: the same Id, no GroupId/GroupMembers.
fn older_build_committed(prepared: &JournalEntry) -> JournalEntry {
    JournalEntry {
        state: JournalState::Committed,
        timestamp_utc: jdt(at(30)),
        group_id: None,
        group_members: None,
        ..prepared.clone()
    }
}

fn line_bytes(entry: &JournalEntry) -> Vec<u8> {
    format!("{}\r\n", entry.to_line()).into_bytes()
}

/// C# `Reconcile_NulInMemberPath_DoesNotAbortReconcile` (native: real disk, `TempRoot`).
#[test]
#[ignore = "native: real disk under TempRoot (W2-FA-07)"]
fn reconcile_nul_in_member_path_does_not_abort_reconcile() {
    // C#: JournalGroupEvidenceAndNulMemberTests.Reconcile_NulInMemberPath_DoesNotAbortReconcile
    let root = TempRoot::new("journal-nul-member");
    let journal_file = root.join("PhotoReview").join("Data").join("operations.jsonl");
    let paths = paths_at(&journal_file.to_string_lossy());
    std::fs::create_dir_all(paths.journal_file.parent().expect("journal dir")).expect("create journal dir");
    let done = root.join("done.jpg");
    std::fs::write(&done, b"12345").expect("write done.jpg");
    // R01c: the moved file keeps the journaled write time. The real disk has no fixed 2026 stamp, so use "now" taken
    // after the file was written (the 2 s tolerance covers the gap).
    let stamp = DateTimeUtc::now();
    let gone_a = root.join("gone-a.jpg").to_string_lossy().into_owned();
    let gone_b = root.join("gone-b.jpg").to_string_lossy().into_owned();
    let bad_sel_a = root.join("sel").join("a\0.jpg").to_string_lossy().into_owned();
    let bad_sel_b = root.join("sel").join("b.jpg").to_string_lossy().into_owned();
    let moved_away = root.join("moved-away.jpg").to_string_lossy().into_owned();
    let done_text = done.to_string_lossy().into_owned();
    let bad_members = [
        member(&gone_a, Some(&bad_sel_a), 5, stamp),
        member(&gone_b, Some(&bad_sel_b), 5, stamp),
    ];
    let bad_prepared = group(
        "bad",
        FileOperationType::Move,
        JournalState::Prepared,
        "capture-bad",
        &bad_members,
    );
    let good = entry(
        "good",
        FileOperationType::Move,
        JournalState::Prepared,
        &moved_away,
        Some(&done_text),
        5,
        stamp,
    );
    let mut text = Vec::new();
    text.extend(line_bytes(&bad_prepared));
    text.extend(line_bytes(&older_build_committed(&bad_prepared)));
    text.extend(line_bytes(&good));
    std::fs::write(&paths.journal_file, &text).expect("write journal");
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(FakeClock::at(DateTimeUtc::now())),
        quiet(),
    );

    let reconciled = reconcile(&journal);

    assert!(
        reconciled
            .iter()
            .any(|e| e.id == "good" && e.state == JournalState::Committed),
        "the entry after the poisoned group line must still reconcile: {reconciled:?}"
    );
}

/// C# `TryParse_NulInMemberPath_IsRejected` is W2-FA-07 / W1-14 (not here). This one is the reconcile half:
/// `Reconcile_RepairOfOneGroupThrows_OtherGroupStillRepaired`.
#[test]
fn reconcile_repair_of_one_group_throws_other_group_still_repaired() {
    // C#: JournalGroupEvidenceAndNulMemberTests.Reconcile_RepairOfOneGroupThrows_OtherGroupStillRepaired
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let poisoned_members = [
        member(r"C:\photos\p.jpg", Some(r"C:\selected\poison.jpg"), 1, at(0)),
        evidence_members(at(0))[0].clone(),
    ];
    let poisoned = group(
        "poisoned",
        FileOperationType::Move,
        JournalState::Prepared,
        "capture-poisoned",
        &poisoned_members,
    );
    let healthy = group(
        "healthy",
        FileOperationType::Move,
        JournalState::Prepared,
        "capture-healthy",
        &evidence_members(at(0)),
    );
    let mut text = Vec::new();
    text.extend(line_bytes(&poisoned));
    text.extend(line_bytes(&older_build_committed(&poisoned)));
    text.extend(line_bytes(&healthy));
    text.extend(line_bytes(&older_build_committed(&healthy)));
    w.disk.add_file_at(&w.paths.journal_file, &text, at(0));
    let journal = OperationJournal::with_options(
        &w.paths,
        Arc::new(CheckFs::new(w.fs(), &[r"C:\selected\poison.jpg"])),
        Arc::new(FakeClock::at(at(60))),
        quiet(),
    );

    let reconciled = reconcile(&journal);

    let only = single(reconciled);
    assert_eq!(only.id, "healthy");
    assert_eq!(only.state, JournalState::Failed);
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalGroupDowngradeTests (ADR 0003 downgrade guard)
// ---------------------------------------------------------------------------------------------------------------------

/// `C:\photos\a.jpg -> C:\selected\a.jpg` (10) and `a.cr2 -> a.cr2` (100), all written at `Stamp`.
fn downgrade_members() -> [JournalGroupMember; 2] {
    evidence_members(at(0))
}

fn downgrade_prepared() -> JournalEntry {
    group(
        "group-op",
        FileOperationType::Move,
        JournalState::Prepared,
        "capture-a",
        &downgrade_members(),
    )
}

/// What an older build appends after reconciling/failing/dismissing: same fields, no group members.
fn older_build_outcome(prepared: &JournalEntry, state: JournalState) -> JournalEntry {
    JournalEntry {
        state,
        group_id: None,
        group_members: None,
        timestamp_utc: jdt(at(30)),
        ..prepared.clone()
    }
}

#[test]
fn reconcile_group_committed_by_older_build_but_second_member_never_moved_surfaces_as_failed_group() {
    // C#: JournalGroupDowngradeTests.Reconcile_GroupCommittedByOlderBuildButSecondMemberNeverMoved_SurfacesAsFailedGroup
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0)); // only the first member moved
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&downgrade_prepared()).expect("append");
    journal
        .append(&older_build_outcome(&downgrade_prepared(), JournalState::Committed))
        .expect("append");

    let surfaced = single(reconcile(&journal));

    assert_eq!(surfaced.state, JournalState::Failed);
    let failed_one = single(failed(&journal));
    assert_eq!(failed_one.id, "group-op");
    assert_eq!(failed_one.group_members.as_deref(), Some(&downgrade_members()[..]));
    assert_eq!(failed_one.group_id.as_deref(), Some("capture-a"));
    assert!(pending(&journal).is_empty());
}

#[test]
fn reconcile_group_committed_by_older_build_and_every_member_moved_stays_committed_with_members_restored() {
    // C#: JournalGroupDowngradeTests.Reconcile_GroupCommittedByOlderBuildAndEveryMemberMoved_StaysCommittedWithMembersRestored
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\selected\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&downgrade_prepared()).expect("append");
    journal
        .append(&older_build_outcome(&downgrade_prepared(), JournalState::Committed))
        .expect("append");

    assert!(reconcile(&journal).is_empty());

    assert!(failed(&journal).is_empty());
    // The repaired line is the newest; the older-build line stays as history.
    let moves = journal.read_committed_moves().expect("read committed moves");
    let newest = moves.last().expect("a committed move");
    assert_eq!(newest.group_members.as_deref(), Some(&downgrade_members()[..]));
}

#[test]
fn reconcile_group_failed_by_older_build_gets_its_members_back_so_recovery_still_sees_the_whole_capture() {
    // C#: JournalGroupDowngradeTests.Reconcile_GroupFailedByOlderBuild_GetsItsMembersBackSoRecoveryStillSeesTheWholeCapture
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&downgrade_prepared()).expect("append");
    let older_failed = JournalEntry {
        error: Some("boom".to_owned()),
        ..older_build_outcome(&downgrade_prepared(), JournalState::Failed)
    };
    journal.append(&older_failed).expect("append");

    assert!(reconcile(&journal).is_empty());

    let failed_one = single(failed(&journal));
    assert_eq!(failed_one.group_members.as_deref(), Some(&downgrade_members()[..]));
    assert_eq!(failed_one.error.as_deref(), Some("boom"));
}

#[test]
fn reconcile_stale_reconcile_failed_follows_older_build_committed_still_repairs_the_group_line() {
    // C#: JournalGroupDowngradeTests.Reconcile_StaleReconcileFailedFollowsOlderBuildCommitted_StillRepairsTheGroupLine
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0)); // the RAW never moved
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&downgrade_prepared()).expect("append");
    journal
        .append(&older_build_outcome(&downgrade_prepared(), JournalState::Committed))
        .expect("append");
    // FA-01 residue: another process's reconcile verdict landed after the Committed line; stale, ignored by readers.
    let stale = JournalEntry {
        error_code: Some(PENDING_UNCONFIRMED.to_owned()),
        timestamp_utc: jdt(at(45)),
        ..older_build_outcome(&downgrade_prepared(), JournalState::Failed)
    };
    journal.append(&stale).expect("append");

    reconcile(&journal);

    let failed_one = single(failed(&journal));
    assert_eq!(
        failed_one.group_members.as_deref(),
        Some(&downgrade_members()[..]),
        "repaired: members back on the newest line"
    );
    assert_eq!(failed_one.error.as_deref(), Some(SETTLED_BY_OLDER_BUILD_TEXT));
}

#[test]
fn reconcile_downgrade_repair_is_applied_once() {
    // C#: JournalGroupDowngradeTests.Reconcile_DowngradeRepair_IsAppliedOnce
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&downgrade_prepared()).expect("append");
    journal
        .append(&older_build_outcome(&downgrade_prepared(), JournalState::Committed))
        .expect("append");
    reconcile(&journal);
    let after_first = w.journal_bytes();

    assert!(reconcile(&journal).is_empty());

    assert_eq!(w.journal_bytes(), after_first, "the second reconcile writes nothing");
    assert_eq!(failed(&journal).len(), 1);
}

#[test]
fn reconcile_dismissed_by_older_build_is_left_alone() {
    // C#: JournalGroupDowngradeTests.Reconcile_DismissedByOlderBuild_IsLeftAlone
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&downgrade_prepared()).expect("append");
    journal
        .append(&older_build_outcome(&downgrade_prepared(), JournalState::Failed))
        .expect("append");
    journal
        .append(&older_build_outcome(&downgrade_prepared(), JournalState::Dismissed))
        .expect("append");
    let before = w.journal_bytes();

    assert!(reconcile(&journal).is_empty());

    assert_eq!(w.journal_bytes(), before);
    assert!(failed(&journal).is_empty());
}

#[test]
fn reconcile_single_file_committed_move_is_not_touched_by_the_guard() {
    // C#: JournalGroupDowngradeTests.Reconcile_SingleFileCommittedMove_IsNotTouchedByTheGuard
    let w = World::new();
    let journal = w.journal(at(60));
    let single_move = entry(
        "single",
        FileOperationType::Move,
        JournalState::Prepared,
        r"C:\photos\b.jpg",
        Some(r"C:\selected\b.jpg"),
        10,
        at(0),
    );
    journal.append(&single_move).expect("append");
    journal
        .append(&JournalEntry {
            state: JournalState::Committed,
            ..single_move.clone()
        })
        .expect("append");
    let before = w.journal_bytes();

    assert!(reconcile(&journal).is_empty());

    assert_eq!(w.journal_bytes(), before);
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalGroupOlderBuildFixtureTests (authentic older-build lines in tests/fixtures/journal-older-build)
// ---------------------------------------------------------------------------------------------------------------------

/// The fixture members: `C:\prfixture\photos\a.jpg -> selected\a.jpg` (10) and `a.cr2` (100).
fn fixture_members() -> [JournalGroupMember; 2] {
    [
        member(
            r"C:\prfixture\photos\a.jpg",
            Some(r"C:\prfixture\selected\a.jpg"),
            10,
            at(0),
        ),
        member(
            r"C:\prfixture\photos\a.cr2",
            Some(r"C:\prfixture\selected\a.cr2"),
            100,
            at(0),
        ),
    ]
}

/// C# `Load(fixture, firstMemberMoved, secondMemberMoved)`: the journal is the fixture's exact bytes.
fn load_fixture(name: &str, first_moved: bool, second_moved: bool) -> (World, OperationJournal) {
    let w = World::new();
    w.put(
        if first_moved {
            r"C:\prfixture\selected\a.jpg"
        } else {
            r"C:\prfixture\photos\a.jpg"
        },
        10,
        at(0),
    );
    w.put(
        if second_moved {
            r"C:\prfixture\selected\a.cr2"
        } else {
            r"C:\prfixture\photos\a.cr2"
        },
        100,
        at(0),
    );
    w.disk
        .add_file_at(&w.paths.journal_file, fixture(name).as_bytes(), at(0));
    let journal = w.journal(at(60));
    (w, journal)
}

fn lines_of(text: &str) -> Vec<&str> {
    text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect()
}

#[test]
fn fixtures_prepared_line_is_what_the_new_serializer_writes_and_older_build_lines_carry_no_group_fields() {
    // C#: JournalGroupOlderBuildFixtureTests.Fixtures_PreparedLineIsWhatTheNewSerializerWrites_AndOlderBuildLinesCarryNoGroupFields
    let w = World::new();
    let journal = w.journal(at(0));
    let members = fixture_members();
    journal
        .append(&group(
            "group-op",
            FileOperationType::Move,
            JournalState::Prepared,
            "capture-a",
            &members,
        ))
        .expect("append");
    let written = String::from_utf8(w.journal_bytes()).expect("utf-8 journal");
    assert_eq!(lines_of(&written), lines_of(&fixture("prepared-group-new-build")));

    for name in [
        "a-first-member-only-committed",
        "b-fully-moved-committed",
        "d-nothing-moved-failed",
        "c-nothing-moved-failed-then-dismissed",
    ] {
        let text = fixture(name);
        let lines = lines_of(&text);
        let prepared: Value = serde_json::from_str(lines[0]).expect("prepared line is JSON");
        assert!(
            prepared.get("GroupMembers").is_some(),
            "{name}: first line carries the members"
        );
        for line in &lines[1..] {
            let doc: Value = serde_json::from_str(line).expect("older line is JSON");
            assert!(
                doc.get("GroupMembers").is_none(),
                "{name}: older build line has no GroupMembers"
            );
            assert!(doc.get("GroupId").is_none(), "{name}: older build line has no GroupId");
        }
    }
}

#[test]
fn older_build_committed_after_only_the_first_member_moved_is_surfaced_as_failed_group() {
    // C#: JournalGroupOlderBuildFixtureTests.OlderBuildCommittedAfterOnlyTheFirstMemberMoved_IsSurfacedAsFailedGroup
    let (_world, journal) = load_fixture("a-first-member-only-committed", true, false);

    let surfaced = single(reconcile(&journal));

    assert_eq!(surfaced.state, JournalState::Failed);
    let failed_one = single(failed(&journal));
    assert_eq!(failed_one.id, "group-op");
    assert_eq!(failed_one.group_id.as_deref(), Some("capture-a"));
    assert_eq!(failed_one.group_members.as_deref(), Some(&fixture_members()[..]));
    assert_eq!(failed_one.error.as_deref(), Some(SETTLED_BY_OLDER_BUILD_TEXT));
    assert_eq!(failed_one.error_code, None);
    assert!(pending(&journal).is_empty());
    assert_eq!(journal.read_pending_and_failed_operations().expect("read").len(), 1);
}

#[test]
fn older_build_committed_after_every_member_moved_stays_committed_with_members_restored() {
    // C#: JournalGroupOlderBuildFixtureTests.OlderBuildCommittedAfterEveryMemberMoved_StaysCommittedWithMembersRestored
    let (w, journal) = load_fixture("b-fully-moved-committed", true, true);

    assert!(reconcile(&journal).is_empty());

    assert!(journal.read_pending_and_failed_operations().expect("read").is_empty());
    let moves = journal.read_committed_moves().expect("read committed moves");
    assert_eq!(
        moves.last().and_then(|m| m.group_members.as_deref()),
        Some(&fixture_members()[..])
    );
    let length_after_first = w.journal_bytes().len();
    assert!(reconcile(&journal).is_empty(), "repaired once, then stable");
    assert_eq!(w.journal_bytes().len(), length_after_first);
}

#[test]
fn older_build_failed_line_gets_its_members_back_and_keeps_its_own_error_code() {
    // C#: JournalGroupOlderBuildFixtureTests.OlderBuildFailedLine_GetsItsMembersBackAndKeepsItsOwnErrorCode
    let (_w, journal) = load_fixture("d-nothing-moved-failed", false, false);

    assert!(reconcile(&journal).is_empty());

    let failed_one = single(failed(&journal));
    assert_eq!(failed_one.group_members.as_deref(), Some(&fixture_members()[..]));
    assert_eq!(failed_one.group_id.as_deref(), Some("capture-a"));
    assert_eq!(failed_one.error_code.as_deref(), Some(PENDING_UNCONFIRMED));
    assert_eq!(
        failed_one.timestamp_utc.value,
        at(30),
        "the older build's verdict time is preserved"
    );
    assert_eq!(journal.read_pending_and_failed_operations().expect("read").len(), 1);
}

#[test]
fn older_build_dismissed_line_is_left_alone() {
    // C#: JournalGroupOlderBuildFixtureTests.OlderBuildDismissedLine_IsLeftAlone
    let (w, journal) = load_fixture("c-nothing-moved-failed-then-dismissed", false, false);
    let before = w.journal_bytes();

    assert!(reconcile(&journal).is_empty());

    assert_eq!(w.journal_bytes(), before);
    assert!(journal.read_pending_and_failed_operations().expect("read").is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalGroupReconcileTests (group verdicts, undo manifests, quarantine of a blank member)
// ---------------------------------------------------------------------------------------------------------------------

fn move_group_members() -> [JournalGroupMember; 2] {
    [
        member(r"C:\photos\a.jpg", Some(r"C:\selected\a.jpg"), 10, at(0)),
        member(r"C:\photos\a.cr2", Some(r"C:\selected\a.cr2"), 100, at(0)),
    ]
}

/// C# `UndoPrepared()`: the undo manifest is already in undo direction (source = original destination).
fn undo_prepared() -> JournalEntry {
    let members = [
        member(r"C:\selected\a.jpg", Some(r"C:\photos\a.jpg"), 10, at(0)),
        member(r"C:\selected\a.cr2", Some(r"C:\photos\a.cr2"), 100, at(0)),
    ];
    JournalEntry {
        undo: Some(true),
        ..group(
            "undo",
            FileOperationType::Move,
            JournalState::Prepared,
            "undo-group",
            &members,
        )
    }
}

#[test]
fn reconcile_group_move_commits_only_when_every_member_reached_its_destination() {
    // C#: JournalGroupReconcileTests.Reconcile_GroupMoveCommitsOnlyWhenEveryMemberReachedItsDestination
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\selected\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    let prepared = group(
        "group-op",
        FileOperationType::Move,
        JournalState::Prepared,
        "capture-a",
        &move_group_members(),
    );
    journal.append(&prepared).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Committed);
    assert_eq!(outcome.group_members, prepared.group_members);
}

#[test]
fn reconcile_partial_group_move_remains_failed_and_visible_as_one_operation() {
    // C#: JournalGroupReconcileTests.Reconcile_PartialGroupMoveRemainsFailedAndVisibleAsOneOperation
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal
        .append(&group(
            "group-op",
            FileOperationType::Move,
            JournalState::Prepared,
            "capture-a",
            &move_group_members(),
        ))
        .expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
    assert_eq!(single(failed(&journal)).group_members.map(|m| m.len()), Some(2));
}

#[test]
fn reconcile_partial_group_undo_uses_reversed_manifest_and_stays_failed() {
    // C#: JournalGroupReconcileTests.Reconcile_PartialGroupUndoUsesReversedManifestAndStaysFailed
    let w = World::new();
    w.put(r"C:\photos\a.jpg", 10, at(0));
    w.put(r"C:\selected\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    let members = [
        member(r"C:\selected\a.jpg", Some(r"C:\photos\a.jpg"), 10, at(0)),
        member(r"C:\selected\a.cr2", Some(r"C:\photos\a.cr2"), 100, at(0)),
    ];
    let undo = JournalEntry {
        undo: Some(true),
        ..group(
            "undo",
            FileOperationType::Move,
            JournalState::Prepared,
            "undo-group",
            &members,
        )
    };
    journal.append(&undo).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
    assert_eq!(single(failed(&journal)).group_members.map(|m| m.len()), Some(2));
}

#[test]
fn reconcile_group_undo_crashed_before_any_move_stays_failed() {
    // C#: JournalGroupReconcileTests.Reconcile_GroupUndoCrashedBeforeAnyMove_StaysFailed
    // Both files still sit at the undo sources: the undo never ran and must not be recorded as Committed.
    let w = World::new();
    w.put(r"C:\selected\a.jpg", 10, at(0));
    w.put(r"C:\selected\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&undo_prepared()).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
    assert_eq!(outcome.error_code.as_deref(), Some(PENDING_UNCONFIRMED));
}

#[test]
fn reconcile_group_undo_completed_but_outcome_not_journaled_commits() {
    // C#: JournalGroupReconcileTests.Reconcile_GroupUndoCompletedButOutcomeNotJournaled_Commits
    // Every member is back at its original location (the undo destination); only the Committed append was lost.
    let w = World::new();
    w.put(r"C:\photos\a.jpg", 10, at(0));
    w.put(r"C:\photos\a.cr2", 100, at(0));
    let journal = w.journal(at(60));
    journal.append(&undo_prepared()).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Committed);
    assert!(pending(&journal).is_empty());
    assert!(failed(&journal).is_empty());
}

#[test]
fn reconcile_group_line_with_blank_member_source_is_quarantined_and_later_pending_entries_still_reconcile() {
    // C#: JournalGroupReconcileTests.Reconcile_GroupLineWithBlankMemberSource_IsQuarantinedAndLaterPendingEntriesStillReconcile
    // Three cases: a null member Source (raw JSON `null`), "", and "  ". The typed model cannot hold a null, so the
    // bad line is written as raw JSON; the parser must reject it and the next entry must still reconcile.
    for source_json in ["null", "\"\"", "\"  \""] {
        let w = World::new();
        w.put(r"C:\selected\b.jpg", 10, at(0));
        let bad = format!(
            r#"{{"Id":"bad-group","Type":"Move","State":"Prepared","Source":"C:\\photos\\a.jpg","Destination":"C:\\selected\\a.jpg","Size":10,"LastWriteUtc":"2026-09-29T00:00:00Z","TimestampUtc":"2026-09-29T00:00:00Z","Error":null,"GroupId":"capture-a","GroupMembers":[{{"Source":{source_json},"Destination":"C:\\selected\\a.jpg","Size":10,"LastWriteUtc":"2026-09-29T00:00:00Z","Permanent":false}}]}}"#
        );
        let good = entry(
            "good",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\b.jpg",
            Some(r"C:\selected\b.jpg"),
            10,
            at(0),
        );
        let mut text = format!("{bad}\r\n").into_bytes();
        text.extend(line_bytes(&good));
        w.disk.add_file_at(&w.paths.journal_file, &text, at(0));
        let journal = w.journal(at(60));

        let reconciled = reconcile(&journal); // used to throw ArgumentException out of the loop

        let outcome = single(reconciled);
        assert_eq!(outcome.id, "good", "member source {source_json}");
        assert_eq!(outcome.state, JournalState::Committed, "member source {source_json}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalReconcileRaceTests (FA-01: another process committed meanwhile)
// ---------------------------------------------------------------------------------------------------------------------

fn race(type_: FileOperationType) {
    let w = World::new();
    let writer = Arc::new(w.journal(at(0))); // process A: owns the active operation
    let prepared = entry(
        "op1",
        type_,
        JournalState::Prepared,
        r"C:\photos\a.jpg",
        Some(r"C:\photos\sel\a.jpg"),
        5,
        at(-100_000),
    );
    writer.append(&prepared).expect("append");

    // Process B has read op1 as Prepared and is judging it (source not yet inspected); now A finishes and commits.
    let committed = JournalEntry {
        state: JournalState::Committed,
        timestamp_utc: jdt(at(0)),
        ..prepared.clone()
    };
    let writer_for_hook = Arc::clone(&writer);
    let check = CheckFs::new(w.fs(), &[]).with_first_exists(r"C:\photos\a.jpg", move || {
        writer_for_hook.append(&committed).expect("writer appends Committed");
    });
    let reconciler = OperationJournal::with_options(&w.paths, Arc::new(check), Arc::new(FakeClock::at(at(0))), quiet());

    let reconciled = reconcile(&reconciler);

    assert!(
        reconciled.is_empty(),
        "{type_:?}: a concurrent commit must not be overwritten by Failed"
    );
    assert!(failed(&reconciler).is_empty());
    assert!(pending(&reconciler).is_empty());
}

#[test]
fn reconcile_concurrent_commit_not_overwritten_by_failed_move() {
    // C#: JournalReconcileRaceTests.Reconcile_ConcurrentCommit_NotOverwrittenByFailed (Move)
    race(FileOperationType::Move);
}

#[test]
fn reconcile_concurrent_commit_not_overwritten_by_failed_copy() {
    // C#: JournalReconcileRaceTests.Reconcile_ConcurrentCommit_NotOverwrittenByFailed (Copy)
    race(FileOperationType::Copy);
}

// ---------------------------------------------------------------------------------------------------------------------
// ReconcileDestinationIdentityTests (R01c: the recorded destination file, 2 s FAT tolerance)
// ---------------------------------------------------------------------------------------------------------------------

const DEST_SOURCE: &str = r"C:\photos\a.jpg";
const DEST_TARGET: &str = r"C:\selected\a.jpg";

#[test]
fn reconcile_single_entry_commits_only_when_the_destination_is_the_recorded_file() {
    // C#: ReconcileDestinationIdentityTests.Reconcile_SingleEntry_CommitsOnlyWhenTheDestinationIsTheRecordedFile
    // (type, destination offset in seconds, committed?)
    let cases = [
        (FileOperationType::Copy, 0, true),
        (FileOperationType::Copy, 1, true),     // FAT rounds the write time to 2 s
        (FileOperationType::Copy, 3600, false), // same size, another file
        (FileOperationType::Move, 0, true),
        (FileOperationType::Move, 3600, false),
    ];
    for (type_, offset, committed) in cases {
        let w = World::new();
        if type_ == FileOperationType::Copy {
            w.put(DEST_SOURCE, 10, at(0));
        }
        w.put(DEST_TARGET, 10, at(offset));
        let journal = w.journal(at(60));
        journal
            .append(&entry(
                "op",
                type_,
                JournalState::Prepared,
                DEST_SOURCE,
                Some(DEST_TARGET),
                10,
                at(0),
            ))
            .expect("append");

        let state = single(reconcile(&journal)).state;

        let expected = if committed {
            JournalState::Committed
        } else {
            JournalState::Failed
        };
        assert_eq!(state, expected, "{type_:?} with destination offset {offset}s");
    }
}

#[test]
fn reconcile_group_entry_commits_only_when_every_destination_is_the_recorded_file() {
    // C#: ReconcileDestinationIdentityTests.Reconcile_GroupEntry_CommitsOnlyWhenEveryDestinationIsTheRecordedFile
    // (type, raw destination offset of the second member in seconds, committed?)
    let cases = [
        (FileOperationType::Copy, 0, true),
        (FileOperationType::Copy, 3600, false),
        (FileOperationType::Move, 0, true),
        (FileOperationType::Move, 3600, false),
    ];
    for (type_, offset, committed) in cases {
        let w = World::new();
        if type_ == FileOperationType::Copy {
            w.put(DEST_SOURCE, 10, at(0));
            w.put(r"C:\photos\a.cr2", 100, at(0));
        }
        w.put(DEST_TARGET, 10, at(0));
        w.put(r"C:\selected\a.cr2", 100, at(offset));
        let members = [
            member(DEST_SOURCE, Some(DEST_TARGET), 10, at(0)),
            member(r"C:\photos\a.cr2", Some(r"C:\selected\a.cr2"), 100, at(0)),
        ];
        let journal = w.journal(at(60));
        journal
            .append(&group("group-op", type_, JournalState::Prepared, "capture", &members))
            .expect("append");

        let state = single(reconcile(&journal)).state;

        let expected = if committed {
            JournalState::Committed
        } else {
            JournalState::Failed
        };
        assert_eq!(
            state, expected,
            "group {type_:?} with second destination offset {offset}s"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// RecoveryIdentityTests (R15: group Recycle undo identity)
// ---------------------------------------------------------------------------------------------------------------------

const JPEG: &str = r"C:\photos\a.jpg";
const RAW: &str = r"C:\photos\a.cr2";

fn undo_recycle_group(state: JournalState, members: &[JournalGroupMember]) -> JournalEntry {
    JournalEntry {
        undo: Some(true),
        destination: None,
        ..group("undo-recycle", FileOperationType::Recycle, state, "g", members)
    }
}

#[test]
fn reconcile_group_recycle_undo_with_foreign_file_at_one_member_stays_failed_while_another_member_is_still_in_the_bin()
{
    // C#: RecoveryIdentityTests.Reconcile_GroupRecycleUndoWithForeignFileAtOneMember_StaysFailedWhileAnotherMemberIsStillInTheBin
    // (foreign size, other write time?)
    for (foreign_size, other_stamp) in [(11, false), (10, true)] {
        let w = World::new();
        w.put(JPEG, 10, at(0));
        w.put(RAW, foreign_size, if other_stamp { at(3 * 3600) } else { at(0) });
        let members = [member(JPEG, None, 10, at(0)), member(RAW, None, 10, at(0))];
        let journal = w.journal(at(300));
        journal
            .append(&undo_recycle_group(JournalState::Prepared, &members))
            .expect("append");

        let outcome = single(reconcile(&journal));

        assert_eq!(
            outcome.state,
            JournalState::Failed,
            "foreign size {foreign_size}, other stamp {other_stamp}"
        );
        assert_eq!(single(failed(&journal)).id, "undo-recycle");
    }
}

#[test]
fn reconcile_group_recycle_undo_with_every_member_back_with_its_identity_commits() {
    // C#: RecoveryIdentityTests.Reconcile_GroupRecycleUndoWithEveryMemberBackWithItsIdentity_Commits
    let w = World::new();
    w.put(JPEG, 10, at(0));
    w.put(RAW, 10, at(0));
    let members = [member(JPEG, None, 10, at(0)), member(RAW, None, 10, at(0))];
    let journal = w.journal(at(300));
    journal
        .append(&undo_recycle_group(JournalState::Prepared, &members))
        .expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Committed);
}

// ---------------------------------------------------------------------------------------------------------------------
// LiveOperationMarkerTests (Q-R27: live markers are skipped, and a retry becoming live is not appended over)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn reconcile_live_marker_skipped_until_released() {
    // C#: LiveOperationMarkerTests.Reconcile_LiveMarker_SkippedUntilReleased (MemberData: Move, Copy, Recycle)
    for type_ in [
        FileOperationType::Move,
        FileOperationType::Copy,
        FileOperationType::Recycle,
    ] {
        let w = World::new();
        let registry = Arc::new(InProcessLiveOperationRegistry::new());
        let options = |registry: &Arc<InProcessLiveOperationRegistry>| OperationJournalOptions {
            live_operations: Some(Arc::clone(registry) as Arc<dyn LiveOperationRegistry>),
            ..quiet()
        };
        let owner =
            OperationJournal::with_options(&w.paths, w.fs(), Arc::new(FakeClock::at(at(0))), options(&registry)); // process A
        let reconciler =
            OperationJournal::with_options(&w.paths, w.fs(), Arc::new(FakeClock::at(at(0))), options(&registry)); // process B
        // A is mid-flight: a Move/Recycle whose source is still there, a Copy whose destination is not written yet.
        let destination = if type_ == FileOperationType::Recycle {
            None
        } else {
            Some(r"C:\photos\sel\a.jpg")
        };
        let prepared = entry(
            "op1",
            type_,
            JournalState::Prepared,
            r"C:\photos\a.jpg",
            destination,
            5,
            at(-86_400),
        );
        let prepared = JournalEntry {
            timestamp_utc: jdt(at(-600)),
            ..prepared
        };
        w.put(r"C:\photos\a.jpg", 5, at(0));
        let marker = registry.begin("op1");
        owner.append(&prepared).expect("append");

        // B started after A prepared (the startup cutoff does not protect A's operation).
        let reconciled = reconciler.reconcile_pending_operations(Some(at(0))).expect("reconcile");

        assert!(reconciled.is_empty(), "{type_:?}: a live operation is not judged");
        assert!(failed(&reconciler).is_empty());
        assert_eq!(single(pending(&reconciler)).id, "op1");

        drop(marker); // A died without an outcome: its marker is gone, the leftover is reconciled as before Q-R27

        let failed_one = single(reconciler.reconcile_pending_operations(Some(at(0))).expect("reconcile"));
        assert_eq!(failed_one.state, JournalState::Failed);
        let expected_code = if type_ == FileOperationType::Recycle {
            SOURCE_STILL_EXISTS_AFTER_RECOVERY
        } else {
            PENDING_UNCONFIRMED
        };
        assert_eq!(failed_one.error_code.as_deref(), Some(expected_code), "{type_:?}");
        assert!(pending(&reconciler).is_empty());
    }
}

#[test]
fn reconcile_retry_becomes_live_during_judgement_not_appended() {
    // C#: LiveOperationMarkerTests.Reconcile_RetryBecomesLiveDuringJudgement_NotAppended
    let w = World::new();
    let registry = Arc::new(InProcessLiveOperationRegistry::new());
    let owner = Arc::new(OperationJournal::with_options(
        &w.paths,
        w.fs(),
        Arc::new(FakeClock::at(at(0))),
        OperationJournalOptions {
            live_operations: Some(Arc::clone(&registry) as Arc<dyn LiveOperationRegistry>),
            ..quiet()
        },
    ));
    let prepared = JournalEntry {
        timestamp_utc: jdt(at(-600)),
        ..entry(
            "op1",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\sel\a.jpg"),
            5,
            at(-86_400),
        )
    };
    w.put(r"C:\photos\a.jpg", 5, at(0));
    owner.append(&prepared).expect("append"); // a dead leftover: no marker

    // Reconcile has read the leftover and is judging it; now Recovery (another process) retries the same Id: marker
    // first, then a fresh Prepared under that Id (JournalTransaction order).
    let retry_marker: Arc<Mutex<Option<LiveMarker>>> = Arc::new(Mutex::new(None));
    let (marker_slot, owner_for_hook, registry_for_hook) =
        (Arc::clone(&retry_marker), Arc::clone(&owner), Arc::clone(&registry));
    let retry_entry = JournalEntry {
        timestamp_utc: jdt(at(-60)),
        ..prepared.clone()
    };
    let check = CheckFs::new(w.fs(), &[]).with_first_exists(r"C:\photos\a.jpg", move || {
        *marker_slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(registry_for_hook.begin("op1"));
        owner_for_hook.append(&retry_entry).expect("retry appends its Prepared");
    });
    let reconciler = OperationJournal::with_options(
        &w.paths,
        Arc::new(check),
        Arc::new(FakeClock::at(at(0))),
        OperationJournalOptions {
            live_operations: Some(Arc::clone(&registry) as Arc<dyn LiveOperationRegistry>),
            ..quiet()
        },
    );

    let reconciled = reconciler.reconcile_pending_operations(Some(at(0))).expect("reconcile");

    assert!(reconciled.is_empty());
    assert!(failed(&reconciler).is_empty());
    assert_eq!(pending(&reconciler).len(), 1);
    assert!(
        retry_marker.lock().unwrap_or_else(PoisonError::into_inner).is_some(),
        "the retry marker was taken"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// OperationJournalMutationTests (reconcile part: group delete / undo, empty member lists, older-build repair)
// ---------------------------------------------------------------------------------------------------------------------

const MJPG: &str = r"C:\photos\a.jpg";
const MRAW: &str = r"C:\photos\a.cr2";
const MOUT_JPG: &str = r"C:\photos\out\a.jpg";
const MOUT_RAW: &str = r"C:\photos\out\a.cr2";

/// C# `RecycleGroup(undo)`: a group Recycle with the two members `a.jpg` (4) and `a.cr2` (8).
fn recycle_group(undo: Option<bool>) -> JournalEntry {
    let members = [member(MJPG, None, 4, at(0)), member(MRAW, None, 8, at(0))];
    JournalEntry {
        undo,
        destination: None,
        size: 4,
        ..group(
            "del",
            FileOperationType::Recycle,
            JournalState::Prepared,
            "capture",
            &members,
        )
    }
}

/// C# `MoveGroup(state, id)`: `a.jpg -> out\a.jpg` (4) and `a.cr2 -> out\a.cr2` (8).
fn move_group_out(state: JournalState, id: &str) -> JournalEntry {
    let members = [
        member(MJPG, Some(MOUT_JPG), 4, at(0)),
        member(MRAW, Some(MOUT_RAW), 8, at(0)),
    ];
    JournalEntry {
        size: 4,
        ..group(id, FileOperationType::Move, state, "capture", &members)
    }
}

fn without_members(entry: JournalEntry) -> JournalEntry {
    JournalEntry {
        group_id: None,
        group_members: None,
        ..entry
    }
}

#[test]
fn reconcile_delete_group_with_every_member_gone_is_committed() {
    // C#: OperationJournalMutationTests.Reconcile_DeleteGroupWithEveryMemberGone_IsCommitted
    let w = World::new();
    let journal = w.journal(at(300));
    journal.append(&recycle_group(None)).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Committed);
    assert_eq!(outcome.error_code, None);
}

#[test]
fn reconcile_delete_group_with_a_member_still_on_disk_fails_with_the_still_exists_code() {
    // C#: OperationJournalMutationTests.Reconcile_DeleteGroupWithAMemberStillOnDisk_FailsWithTheStillExistsCode
    let w = World::new();
    w.put(MRAW, 8, at(0));
    let journal = w.journal(at(300));
    journal.append(&recycle_group(None)).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
    assert_eq!(outcome.error_code.as_deref(), Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY));
}

#[test]
fn reconcile_undo_delete_group_with_every_member_back_is_committed() {
    // C#: OperationJournalMutationTests.Reconcile_UndoDeleteGroupWithEveryMemberBack_IsCommitted
    let w = World::new();
    w.put(MJPG, 4, at(0));
    w.put(MRAW, 8, at(0));
    let journal = w.journal(at(300));
    journal.append(&recycle_group(Some(true))).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Committed);
}

#[test]
fn reconcile_undo_delete_group_with_a_member_still_missing_fails() {
    // C#: OperationJournalMutationTests.Reconcile_UndoDeleteGroupWithAMemberStillMissing_Fails
    let w = World::new();
    w.put(MJPG, 4, at(0));
    let journal = w.journal(at(300));
    journal.append(&recycle_group(Some(true))).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
}

#[test]
fn reconcile_undo_delete_group_with_nothing_back_fails() {
    // C#: OperationJournalMutationTests.Reconcile_UndoDeleteGroupWithNothingBack_Fails
    let w = World::new();
    let journal = w.journal(at(300));
    journal.append(&recycle_group(Some(true))).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
}

#[test]
fn reconcile_pending_move_with_an_empty_member_list_is_judged_as_a_single_move() {
    // C#: OperationJournalMutationTests.Reconcile_PendingMoveWithAnEmptyMemberList_IsJudgedAsASingleMove
    let w = World::new();
    w.put(MJPG, 4, at(0)); // source still there, destination missing: the Move did not happen
    let journal = w.journal(at(300));
    let single_move = JournalEntry {
        group_members: Some(Vec::new()),
        size: 4,
        ..entry(
            "single",
            FileOperationType::Move,
            JournalState::Prepared,
            MJPG,
            Some(MOUT_JPG),
            4,
            at(0),
        )
    };
    journal.append(&single_move).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
    assert_eq!(outcome.error_code.as_deref(), Some(PENDING_UNCONFIRMED));
}

#[test]
fn reconcile_latest_line_with_an_empty_member_list_after_a_group_line_is_repaired_from_the_earlier_members() {
    // C#: OperationJournalMutationTests.Reconcile_LatestLineWithAnEmptyMemberListAfterAGroupLine_IsRepairedFromTheEarlierMembers
    // Sources still present, destinations missing: the capture was NOT fully moved although the latest line says Committed.
    let w = World::new();
    w.put(MJPG, 4, at(0));
    w.put(MRAW, 8, at(0));
    let journal = w.journal(at(300));
    journal
        .append(&move_group_out(JournalState::Failed, "grp"))
        .expect("append");
    let committed_empty = JournalEntry {
        group_id: None,
        group_members: Some(Vec::new()),
        ..move_group_out(JournalState::Committed, "grp")
    };
    journal.append(&committed_empty).expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
    assert_eq!(outcome.error.as_deref(), Some(SETTLED_BY_OLDER_BUILD_TEXT));
    assert_eq!(outcome.group_members.map(|m| m.len()), Some(2));
}

#[test]
fn reconcile_older_build_retried_a_group_and_left_it_prepared_without_members_is_reconciled_with_the_members_and_writes_one_line()
 {
    // C#: OperationJournalMutationTests.Reconcile_OlderBuildRetriedAGroupAndLeftItPreparedWithoutMembers_IsReconciledWithTheMembersAndWritesOneLine
    let w = World::new();
    w.put(MOUT_JPG, 4, at(0));
    w.put(MOUT_RAW, 8, at(0));
    let journal = w.journal(at(300));
    journal
        .append(&move_group_out(JournalState::Failed, "grp"))
        .expect("append");
    journal
        .append(&without_members(move_group_out(JournalState::Prepared, "grp")))
        .expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Committed);
    assert_eq!(outcome.group_members.map(|m| m.len()), Some(2));
    assert_eq!(
        w.journal_line_count(),
        3,
        "Failed, Prepared (older build) and exactly one outcome: no interim Prepared copy"
    );
    assert!(pending(&journal).is_empty());
}

#[test]
fn reconcile_repair_append_fails_with_an_io_error_does_not_throw_and_leaves_the_journal_alone() {
    // C#: OperationJournalMutationTests.Reconcile_RepairAppendFailsWithAnIoError_DoesNotThrowAndLeavesTheJournalAlone
    // (Theory: IOException -> ErrorKind::Other, UnauthorizedAccessException -> ErrorKind::PermissionDenied)
    for kind in [io::ErrorKind::Other, io::ErrorKind::PermissionDenied] {
        let w = World::new();
        w.put(MOUT_JPG, 4, at(0));
        w.put(MOUT_RAW, 8, at(0));
        let journal = w.journal(at(300));
        journal
            .append(&move_group_out(JournalState::Failed, "grp"))
            .expect("append");
        // older build settled it; every member really moved
        journal
            .append(&without_members(move_group_out(JournalState::Committed, "grp")))
            .expect("append");
        w.disk.fail(FsOp::OpenAppend, &w.paths.journal_file, kind);

        let reconciled = reconcile(&journal);

        assert!(reconciled.is_empty(), "{kind:?}");
        w.disk.clear_faults();
        assert_eq!(w.journal_line_count(), 2, "{kind:?}");
    }
}

#[test]
fn reconcile_repair_finds_a_newer_line_appended_by_another_writer_meanwhile_appends_nothing() {
    // C#: OperationJournalMutationTests.Reconcile_RepairFindsANewerLineAppendedByAnotherWriterMeanwhile_AppendsNothing
    let w = World::new();
    w.put(MOUT_JPG, 4, at(0));
    w.put(MOUT_RAW, 8, at(0));
    let registry = Arc::new(HookedRegistry::new());
    let journal = Arc::new(OperationJournal::with_options(
        &w.paths,
        w.fs(),
        Arc::new(FakeClock::at(at(300))),
        OperationJournalOptions {
            live_operations: Some(Arc::clone(&registry) as Arc<dyn LiveOperationRegistry>),
            ..quiet()
        },
    ));
    journal
        .append(&move_group_out(JournalState::Failed, "grp"))
        .expect("append");
    journal
        .append(&without_members(move_group_out(JournalState::Committed, "grp")))
        .expect("append");
    let other_writer = Arc::clone(&journal);
    let other_line = move || {
        let other = JournalEntry {
            timestamp_utc: jdt(at(540)),
            error: Some("other process".to_owned()),
            ..without_members(move_group_out(JournalState::Failed, "grp"))
        };
        other_writer.append(&other).expect("other writer appends");
    };
    let hook: Box<dyn FnOnce() + Send> = Box::new(other_line);
    *registry.on_first_is_live.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);

    let reconciled = reconcile(&journal);

    assert!(reconciled.is_empty());
    assert_eq!(
        w.journal_line_count(),
        3,
        "Failed, Committed (older build) and the other writer's line only"
    );
}

#[test]
fn reconcile_repair_finds_the_id_gone_from_the_journal_meanwhile_does_not_throw_or_append() {
    // C#: OperationJournalMutationTests.Reconcile_RepairFindsTheIdGoneFromTheJournalMeanwhile_DoesNotThrowOrAppend
    let w = World::new();
    w.put(MOUT_JPG, 4, at(0));
    w.put(MOUT_RAW, 8, at(0));
    let registry = Arc::new(HookedRegistry::new());
    let journal = OperationJournal::with_options(
        &w.paths,
        w.fs(),
        Arc::new(FakeClock::at(at(300))),
        OperationJournalOptions {
            live_operations: Some(Arc::clone(&registry) as Arc<dyn LiveOperationRegistry>),
            ..quiet()
        },
    );
    journal
        .append(&move_group_out(JournalState::Failed, "grp"))
        .expect("append");
    journal
        .append(&without_members(move_group_out(JournalState::Committed, "grp")))
        .expect("append");
    let disk = Arc::clone(&w.disk);
    let journal_file = w.paths.journal_file.clone();
    let delete_journal = move || FileSystem::delete(disk.as_ref(), &journal_file).expect("delete journal");
    let hook: Box<dyn FnOnce() + Send> = Box::new(delete_journal);
    *registry.on_first_is_live.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);

    let reconciled = reconcile(&journal);

    assert!(reconciled.is_empty());
    assert!(w.disk.file_content(&w.paths.journal_file).is_none());
}

// ---------------------------------------------------------------------------------------------------------------------
// OperationJournalTests (the reconcile half; the fixture journal was the real %LOCALAPPDATA% in C#, here fakes)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn pending_move_is_committed_only_when_source_absent_and_fingerprint_matches() {
    // C#: OperationJournalTests.PendingMoveIsCommittedWhenSourceAbsentAndFingerprintMatches
    let w = World::new();
    w.put(r"C:\pending-dest\pending-move.jpg", 3, at(0));
    let journal = w.journal(at(60));
    journal
        .append(&entry(
            "pending-move",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\pending-move.jpg",
            Some(r"C:\pending-dest\pending-move.jpg"),
            3,
            at(0),
        ))
        .expect("append");

    let moves = reconcile(&journal);

    assert!(
        moves
            .iter()
            .any(|x| x.id == "pending-move" && x.state == JournalState::Committed)
    );
}

#[test]
fn pending_copy_with_mismatched_destination_is_failed_without_replay() {
    // C#: OperationJournalTests.PendingCopyWithMismatchedDestinationIsFailed
    let w = World::new();
    w.put(r"C:\pending-mismatch-dest.jpg", 2, at(0));
    let journal = w.journal(at(60));
    journal
        .append(&entry(
            "pending-mismatch",
            FileOperationType::Copy,
            JournalState::Prepared,
            r"C:\pending-mismatch.jpg",
            Some(r"C:\pending-mismatch-dest.jpg"),
            3,
            at(0),
        ))
        .expect("append");

    let reconciled = reconcile(&journal);

    assert!(
        reconciled
            .iter()
            .any(|x| x.id == "pending-mismatch" && x.state == JournalState::Failed)
    );
    assert!(pending(&journal).iter().all(|x| x.id != "pending-mismatch"));
}

#[test]
fn pending_move_with_source_still_present_is_failed_without_replay() {
    // C#: OperationJournalTests.PendingMoveWithSourceStillPresentIsFailed
    let w = World::new();
    w.put(r"C:\pending-source-exists.jpg", 3, at(0));
    w.put(r"C:\pending-source-exists-dest.jpg", 3, at(0));
    let journal = w.journal(at(60));
    journal
        .append(&entry(
            "source-exists",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\pending-source-exists.jpg",
            Some(r"C:\pending-source-exists-dest.jpg"),
            3,
            at(0),
        ))
        .expect("append");

    let reconciled = reconcile(&journal);

    assert!(
        reconciled
            .iter()
            .any(|x| x.id == "source-exists" && x.state == JournalState::Failed)
    );
}

#[test]
fn reconcile_recycle_committed_if_source_absent_failed_if_source_present() {
    // C#: OperationJournalTests.ReconcileRecycle
    let w = World::new();
    let journal = w.journal(at(60));
    journal
        .append(&entry(
            "rec-done",
            FileOperationType::Recycle,
            JournalState::Prepared,
            r"C:\photos\absent.jpg",
            None,
            100,
            at(0),
        ))
        .expect("append");
    w.put(r"C:\photos\present.jpg", 5, at(0));
    journal
        .append(&entry(
            "rec-fail",
            FileOperationType::Recycle,
            JournalState::Prepared,
            r"C:\photos\present.jpg",
            None,
            5,
            at(0),
        ))
        .expect("append");

    let reconciled = reconcile(&journal);
    assert_eq!(reconciled.len(), 2);

    let rec_done = reconciled
        .iter()
        .find(|x| x.id == "rec-done")
        .expect("rec-done reconciled");
    assert_eq!(rec_done.state, JournalState::Committed);
    assert_eq!(rec_done.error, None);

    let rec_fail = reconciled
        .iter()
        .find(|x| x.id == "rec-fail")
        .expect("rec-fail reconciled");
    assert_eq!(rec_fail.state, JournalState::Failed);
    // Q-L3: the journal stores a stable code + invariant English; the UI shows the localized text by code.
    assert_eq!(rec_fail.error_code.as_deref(), Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY));
    assert_eq!(
        rec_fail.error.as_deref(),
        Some("The source still exists after session recovery.")
    );
}

#[test]
fn reconcile_move_committed_if_source_absent_and_destination_size_matches() {
    // C#: OperationJournalTests.ReconcileMove
    let w = World::new();
    let journal = w.journal(at(60));
    w.put(r"C:\photos\dest1.jpg", 4, at(0)); // R01c: the destination keeps the journaled write time
    journal
        .append(&entry(
            "move-done",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\src1.jpg",
            Some(r"C:\photos\dest1.jpg"),
            4,
            at(0),
        ))
        .expect("append");

    w.put(r"C:\photos\src2.jpg", 4, at(0));
    w.put(r"C:\photos\dest2.jpg", 4, at(0));
    journal
        .append(&entry(
            "move-fail-source-exists",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\src2.jpg",
            Some(r"C:\photos\dest2.jpg"),
            4,
            at(0),
        ))
        .expect("append");

    w.put(r"C:\photos\dest3.jpg", 2, at(0));
    journal
        .append(&entry(
            "move-fail-size-mismatch",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\src3.jpg",
            Some(r"C:\photos\dest3.jpg"),
            4,
            at(0),
        ))
        .expect("append");

    let reconciled = reconcile(&journal);
    assert_eq!(reconciled.len(), 3);

    let state_of = |id: &str| reconciled.iter().find(|x| x.id == id).expect("reconciled").state;
    assert_eq!(state_of("move-done"), JournalState::Committed);
    assert_eq!(state_of("move-fail-source-exists"), JournalState::Failed);
    assert_eq!(state_of("move-fail-size-mismatch"), JournalState::Failed);
}

#[test]
fn reconcile_copy_committed_even_if_source_still_exists() {
    // C#: OperationJournalTests.ReconcileCopy
    let w = World::new();
    let journal = w.journal(at(60));
    w.put(r"C:\photos\src.jpg", 4, at(0));
    w.put(r"C:\photos\dest.jpg", 4, at(0)); // R01c: the destination keeps the journaled write time
    journal
        .append(&entry(
            "copy-done",
            FileOperationType::Copy,
            JournalState::Prepared,
            r"C:\photos\src.jpg",
            Some(r"C:\photos\dest.jpg"),
            4,
            at(0),
        ))
        .expect("append");
    journal
        .append(&entry(
            "copy-fail",
            FileOperationType::Copy,
            JournalState::Prepared,
            r"C:\photos\src.jpg",
            Some(r"C:\photos\missing.jpg"),
            4,
            at(0),
        ))
        .expect("append");

    let reconciled = reconcile(&journal);
    assert_eq!(reconciled.len(), 2);

    let state_of = |id: &str| reconciled.iter().find(|x| x.id == id).expect("reconciled").state;
    assert_eq!(state_of("copy-done"), JournalState::Committed);
    assert_eq!(state_of("copy-fail"), JournalState::Failed);
}

// ---------------------------------------------------------------------------------------------------------------------
// JournalAppendRobustnessTests, JournalBlankPathTests, JournalErrorCodeTests, JournalConcurrencyTests
// ---------------------------------------------------------------------------------------------------------------------

/// C# `Entry(id)` of JournalAppendRobustnessTests.
fn robust_entry(id: &str) -> JournalEntry {
    entry(
        id,
        FileOperationType::Move,
        JournalState::Prepared,
        &format!(r"C:\photos\{id}.jpg"),
        Some(&format!(r"C:\photos\sel\{id}.jpg")),
        1,
        at(0),
    )
}

#[test]
fn reconcile_append_fails_for_one_entry_returns_the_others() {
    // C#: JournalAppendRobustnessTests.Reconcile_AppendFailsForOneEntry_ReturnsTheOthers
    // The second outcome append hits a full disk: the open-append hook counts the calls (the stub's `fail_times` cannot
    // pick the 2nd call).
    let w = World::new();
    let hooked = Arc::new(HookFileSystem::new(Arc::clone(&w.disk)));
    let fs: Arc<dyn FileSystem> = hooked.clone();
    let journal = OperationJournal::with_options(&w.paths, fs, Arc::new(SystemClock), quiet());
    journal.append(&robust_entry("a")).expect("append");
    journal.append(&robust_entry("b")).expect("append");
    journal.append(&robust_entry("c")).expect("append");

    let appends = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&appends);
    let fail_second_append: PathHook = Arc::new(move |_path: &Path| {
        if counter.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
            Some(io::Error::other("disk full"))
        } else {
            None
        }
    });
    hooked.set_open_append_hook(Some(fail_second_append));

    let reconciled = reconcile(&journal);

    assert_eq!(reconciled.len(), 2);
    hooked.set_open_append_hook(None);
    let leftover = single(pending(&journal)); // the failed one stays Prepared for the next start
    assert!(reconciled.iter().all(|entry| entry.id != leftover.id));
    assert_eq!(single(reconcile(&journal)).id, leftover.id);
}

#[test]
fn reconcile_entry_whose_file_check_throws_does_not_stop_the_other_entries() {
    // C#: JournalBlankPathTests.Reconcile_EntryWhoseFileCheckThrows_DoesNotStopTheOtherEntries
    let w = World::new();
    w.put(r"C:\selected\b.jpg", 10, at(0));
    let check = CheckFs::new(w.fs(), &[r"C:\photos\bad.jpg"]);
    let journal = OperationJournal::with_options(&w.paths, Arc::new(check), Arc::new(FakeClock::at(at(60))), quiet());
    journal
        .append(&entry(
            "bad",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\bad.jpg",
            Some(r"C:\selected\bad.jpg"),
            10,
            at(0),
        ))
        .expect("append");
    journal
        .append(&entry(
            "good",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\b.jpg",
            Some(r"C:\selected\b.jpg"),
            10,
            at(0),
        ))
        .expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.id, "good");
    assert_eq!(outcome.state, JournalState::Committed);
}

/// The Vietnamese UI catalog of JournalErrorCodeTests (`Localizer.Current` = Vietnamese): the stored text stays English.
struct VietnameseUi;

impl JournalTexts for VietnameseUi {
    fn english(&self, key: &str) -> String {
        EnglishJournalTexts.english(key)
    }

    fn localized(&self, key: &str) -> String {
        match key {
            "core.journal.sourceStillExists" => "Nguồn vẫn tồn tại sau khi khôi phục phiên.".to_owned(),
            other => EnglishJournalTexts.english(other),
        }
    }
}

#[test]
fn reconcile_failed_recycle_persists_code_and_english_text_even_when_ui_is_vietnamese() {
    // C#: JournalErrorCodeTests.Reconcile_FailedRecycle_PersistsCodeAndEnglishText_EvenWhenUiIsVietnamese
    let w = World::new();
    w.put(r"C:\photos\present.jpg", 5, at(0));
    let options = OperationJournalOptions {
        texts: Some(Arc::new(VietnameseUi)),
        ..quiet()
    };
    let journal = OperationJournal::with_options(&w.paths, w.fs(), Arc::new(FakeClock::at(at(0))), options);
    journal
        .append(&entry(
            "rec",
            FileOperationType::Recycle,
            JournalState::Prepared,
            r"C:\photos\present.jpg",
            None,
            5,
            at(0),
        ))
        .expect("append");

    reconcile(&journal);

    let text = String::from_utf8(w.journal_bytes()).expect("utf-8 journal");
    let last_line = lines_of(&text).last().copied().expect("a last line");
    assert!(
        last_line.contains(r#""Error":"The source still exists after session recovery.""#),
        "{last_line}"
    );
    assert!(
        last_line.ends_with(r#","ErrorCode":"SourceStillExistsAfterRecovery"}"#),
        "{last_line}"
    );
    let failed_one = single(failed(&journal));
    assert_eq!(
        failed_one.error_code.as_deref(),
        Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY)
    );
    assert_eq!(
        failed_one.error.as_deref(),
        Some("The source still exists after session recovery.")
    );
    assert_eq!(
        journal.texts().localized("core.journal.sourceStillExists"),
        "Nguồn vẫn tồn tại sau khi khôi phục phiên."
    );
}

#[test]
fn reconcile_failed_move_persists_pending_unconfirmed_code() {
    // C#: JournalErrorCodeTests.Reconcile_FailedMove_PersistsPendingUnconfirmedCode
    let w = World::new();
    w.put(r"C:\photos\src.jpg", 4, at(0));
    let journal = w.journal(at(0));
    journal
        .append(&entry(
            "mv",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\src.jpg",
            Some(r"C:\photos\dest.jpg"),
            4,
            at(0),
        ))
        .expect("append");

    let outcome = single(reconcile(&journal));

    assert_eq!(outcome.state, JournalState::Failed);
    assert_eq!(outcome.error_code.as_deref(), Some(PENDING_UNCONFIRMED));
    assert_eq!(
        outcome.error.as_deref(),
        Some("Could not confirm the pending operation; it will not be replayed automatically.")
    );
}

#[test]
#[ignore = "native: two journal instances on PhysicalFileSystem under TempRoot, threads + Barrier"]
fn reconcile_while_other_process_commits_latest_state_stays_committed() {
    // C#: JournalConcurrencyTests.ReconcileWhileOtherProcessCommits_LatestStateStaysCommitted
    const OPERATIONS: usize = 30;
    let root = TempRoot::new("journal-concurrency-reconcile");
    let paths = paths_at(
        &root
            .join("PhotoReview")
            .join("Data")
            .join("operations.jsonl")
            .to_string_lossy(),
    );
    std::fs::create_dir_all(paths.journal_file.parent().expect("journal dir")).expect("create journal dir");
    let photos = root.join("photos");
    let sel = root.join("sel");
    std::fs::create_dir_all(&photos).expect("photos");
    std::fs::create_dir_all(&sel).expect("sel");
    let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
    let owner = Arc::new(OperationJournal::with_options(
        &paths,
        Arc::clone(&fs),
        Arc::new(SystemClock),
        quiet(),
    ));
    let reconciler = Arc::new(OperationJournal::with_options(
        &paths,
        fs,
        Arc::new(SystemClock),
        quiet(),
    ));
    let mut entries = Vec::new();
    for i in 0..OPERATIONS {
        let source = photos.join(format!("{i}.jpg"));
        let destination = sel.join(format!("{i}.jpg"));
        std::fs::write(&source, b"x").expect("write source");
        let prepared = entry(
            &format!("c{i}"),
            FileOperationType::Copy,
            JournalState::Prepared,
            &source.to_string_lossy(),
            Some(&destination.to_string_lossy()),
            1,
            DateTimeUtc::from_ticks(0).expect("epoch"),
        );
        owner.append(&prepared).expect("append");
        entries.push(prepared);
    }

    let barrier = Barrier::new(2);
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait();
            for prepared in &entries {
                let destination = prepared.destination.as_deref().expect("destination");
                std::fs::write(destination, b"x").expect("the copy completes");
                let committed = JournalEntry {
                    state: JournalState::Committed,
                    ..prepared.clone()
                };
                owner.append(&committed).expect("Committed is journaled");
            }
            done.store(true, Ordering::SeqCst);
        });
        scope.spawn(|| {
            barrier.wait();
            // Bounded: a hung writer must fail the test, not hang the run.
            for _ in 0..10_000 {
                if done.load(Ordering::SeqCst) {
                    break;
                }
                let _ = reconciler.reconcile_pending_operations(None);
            }
        });
    });

    let final_journal =
        OperationJournal::with_options(&paths, Arc::new(PhysicalFileSystem), Arc::new(SystemClock), quiet());
    let text = String::from_utf8(std::fs::read(&paths.journal_file).expect("journal exists")).expect("utf-8");
    let mut latest_state = std::collections::HashMap::new();
    for line in lines_of(&text) {
        let parsed = try_parse(line).expect("every line parses (no glued or torn line)");
        latest_state.insert(parsed.id, parsed.state);
    }
    let failed_or_pending = final_journal.read_pending_and_failed_operations().expect("read");
    for prepared in &entries {
        assert_eq!(
            latest_state.get(&prepared.id),
            Some(&JournalState::Committed),
            "{}",
            prepared.id
        );
    }
    assert!(
        failed_or_pending.is_empty(),
        "no false Failed or Prepared after Committed: {failed_or_pending:?}"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/journal-reconcile.json (71 cases, oracle `journal-reconcile`, 1 or 2 runs per case)
// ---------------------------------------------------------------------------------------------------------------------

/// Joins the golden journal lines: `newline` (default CRLF) between lines, a trailing `newline` unless `trailing` is false.
fn golden_journal_bytes(lines: &[Value], newline: &str, trailing: bool) -> Vec<u8> {
    let texts: Vec<&str> = lines
        .iter()
        .map(|l| l.as_str().expect("journal line is a string"))
        .collect();
    let mut text = texts.join(newline);
    if trailing && !texts.is_empty() {
        text.push_str(newline);
    }
    text.into_bytes()
}

#[test]
fn golden_journal_reconcile_cases_match_csharp() {
    // C#: oracle `journal-reconcile` (OperationJournal.ReconcilePendingOperations), tests/golden/journal-reconcile.json
    let golden = golden::load("journal-reconcile");
    for case in &golden.cases {
        let name = str_of(case, "name");
        eprintln!("journal-reconcile golden case: {name}");
        let w = World {
            disk: Arc::new(FakeFileSystem::new()),
            paths: paths_at(str_of(case, "journal_path")),
        };
        let panic_on: Vec<&str> = case["stat_throws"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| p.as_str().expect("path"))
            .collect();
        for file in case["files"].as_array().expect("files") {
            w.put(
                str_of(file, "path"),
                usize::try_from(i64_of(file, "size")).expect("size"),
                utc(str_of(file, "time")),
            );
        }
        let journal_path = w.paths.journal_file.clone();
        if let Some(lines) = case["journal"].as_array() {
            let newline = case["newline"].as_str().unwrap_or("\r\n");
            let trailing = case["trailing"].as_bool().unwrap_or(true);
            let bytes = golden_journal_bytes(lines, newline, trailing);
            w.disk.add_dir(journal_path.parent().expect("journal dir"));
            w.disk.add_file_at(&journal_path, &bytes, utc(str_of(case, "clock")));
        }
        if let Some(count) = case.get("append_failures").and_then(Value::as_u64) {
            w.disk.fail_times(
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
        let options = OperationJournalOptions {
            live_operations: Some(Arc::clone(&registry) as Arc<dyn LiveOperationRegistry>),
            ..quiet()
        };
        let check = CheckFs::new(w.fs(), &panic_on);
        let journal = OperationJournal::with_options(
            &w.paths,
            Arc::new(check),
            Arc::new(FakeClock::at(utc(str_of(case, "clock")))),
            options,
        );
        let cutoff = case.get("cutoff").and_then(Value::as_str).map(utc);

        for run in case["runs"].as_array().expect("runs") {
            let reconciled = journal
                .reconcile_pending_operations(cutoff)
                .expect("reconcile reads the journal");
            let actual: Vec<String> = reconciled.iter().map(JournalEntry::to_line).collect();
            let expected: Vec<String> = run["reconciled"]
                .as_array()
                .expect("reconciled")
                .iter()
                .map(|l| l.as_str().expect("line").to_owned())
                .collect();
            assert_eq!(actual, expected, "case {name}: reconciled lines");
            match run["journal_after"].as_str() {
                None => assert!(
                    w.disk.file_content(&journal_path).is_none(),
                    "case {name}: no journal file"
                ),
                Some(after) => assert_eq!(
                    String::from_utf8(w.disk.file_content(&journal_path).unwrap_or_default()).expect("utf-8"),
                    after,
                    "case {name}: journal bytes after the run"
                ),
            }
        }
        drop(markers);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Parity guards (W3-03 verifier, repair round 1): the C# rules the existing cases left unpinned.
// ---------------------------------------------------------------------------------------------------------------------

/// A journal over `world` whose diagnostics go to `diagnostics` (C# `FileLog` warnings of `TryReconcileAppend` & co).
fn journal_logging_to(
    fs: Arc<dyn FileSystem>,
    diagnostics: &Arc<RecordingLog>,
    live: Option<Arc<dyn LiveOperationRegistry>>,
) -> OperationJournal {
    let log: Arc<dyn Log> = diagnostics.clone();
    OperationJournal::with_options(
        &paths_at(JOURNAL),
        fs,
        Arc::new(FakeClock::at(at(0))),
        OperationJournalOptions {
            log: Some(log),
            live_operations: live,
            ..quiet()
        },
    )
}

fn unconfirmed_single_move(id: &str) -> JournalEntry {
    JournalEntry {
        timestamp_utc: jdt(at(-300)),
        ..entry(
            id,
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\a.jpg",
            Some(r"C:\sel\a.jpg"),
            5,
            at(-86_400),
        )
    }
}

#[test]
fn reconcile_group_move_member_copied_but_source_still_present_is_not_completed() {
    // C#: OperationJournal.IsGroupMemberCompleted, Move => !sourceExists && destinationMatches.
    let world = World::new();
    let stamp = at(-86_400);
    let members = [
        member(r"C:\p\a.jpg", Some(r"C:\s\a.jpg"), 10, stamp),
        member(r"C:\p\a.cr2", Some(r"C:\s\a.cr2"), 20, stamp),
    ];
    world.put(r"C:\s\a.jpg", 10, stamp);
    world.put(r"C:\s\a.cr2", 20, stamp);
    world.put(r"C:\p\a.cr2", 20, stamp); // copied, but the source was never removed
    let journal = world.journal(at(0));
    journal
        .append(&group(
            "g1",
            FileOperationType::Move,
            JournalState::Prepared,
            "g",
            &members,
        ))
        .expect("append");

    let out = single(reconcile(&journal));

    assert_eq!(out.state, JournalState::Failed);
    assert_eq!(out.error_code.as_deref(), Some(PENDING_UNCONFIRMED));
}

#[test]
fn reconcile_recycle_group_with_undo_false_is_judged_as_a_plain_recycle() {
    // C#: `group.Undo != true` -> a Recycle group with Undo = false whose sources are gone is Committed.
    let world = World::new();
    let stamp = at(-86_400);
    let members = [
        member(r"C:\p\a.jpg", None, 10, stamp),
        member(r"C:\p\a.cr2", None, 20, stamp),
    ];
    let journal = world.journal(at(0));
    let mut line = group("g1", FileOperationType::Recycle, JournalState::Prepared, "g", &members);
    line.undo = Some(false);
    journal.append(&line).expect("append");

    let out = single(reconcile(&journal));

    assert_eq!(out.state, JournalState::Committed);
}

#[test]
fn reconcile_members_of_a_stale_reconcile_line_still_feed_the_downgrade_guard() {
    // C#: ComputeLatestEntries records the members of EVERY line (before the FA-01 stale check), so a stale reconcile
    // Failed that carries the members still lets the guard repair an older build's member-less Committed.
    let world = World::new();
    let stamp = at(-86_400);
    let members = [
        member(r"C:\p\a.jpg", Some(r"C:\s\a.jpg"), 10, stamp),
        member(r"C:\p\a.cr2", Some(r"C:\s\a.cr2"), 20, stamp),
    ];
    let journal = world.journal(at(0));
    let mut committed_by_older_build = group("g1", FileOperationType::Move, JournalState::Committed, "g", &members);
    committed_by_older_build.group_id = None;
    committed_by_older_build.group_members = None;
    let mut stale = group("g1", FileOperationType::Move, JournalState::Failed, "g", &members);
    stale.error = Some("x".to_owned());
    stale.error_code = Some(PENDING_UNCONFIRMED.to_owned());
    journal.append(&committed_by_older_build).expect("append");
    journal.append(&stale).expect("append");

    let out = single(reconcile(&journal));

    assert_eq!(out.state, JournalState::Failed);
    assert_eq!(out.error.as_deref(), Some(SETTLED_BY_OLDER_BUILD_TEXT));
}

#[test]
fn reconcile_outcome_append_that_fails_is_logged_to_the_diagnostics_log() {
    // C#: TryReconcileAppend warns "Journal reconcile: could not append outcome for {id}: {message}".
    let world = World::new();
    let diagnostics = Arc::new(RecordingLog::new());
    let journal = journal_logging_to(world.fs(), &diagnostics, None);
    journal.append(&unconfirmed_single_move("a")).expect("append");
    world
        .disk
        .fail(FsOp::OpenAppend, JOURNAL, io::ErrorKind::PermissionDenied);

    let out = reconcile(&journal);

    assert!(out.is_empty());
    let warns: Vec<String> = diagnostics
        .entries()
        .into_iter()
        .filter(|(level, _)| *level == pr_test_support::LogLevel::Warn)
        .map(|(_, message)| message)
        .collect();
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert!(
        warns[0].starts_with("Journal reconcile: could not append outcome for a: "),
        "{warns:?}"
    );
}

#[test]
fn reconcile_live_entry_is_skipped_before_any_file_check() {
    // C# (Q-R27): ReconcileOne checks IsLive before any (possibly slow) file check; a live entry's files are untouched.
    let world = World::new();
    let fs: Arc<dyn FileSystem> = Arc::new(CheckFs::new(world.fs(), &[r"C:\photos\a.jpg", r"C:\sel\a.jpg"]));
    let registry: Arc<dyn LiveOperationRegistry> = Arc::new(InProcessLiveOperationRegistry::new());
    let diagnostics = Arc::new(RecordingLog::new());
    let journal = journal_logging_to(fs, &diagnostics, Some(Arc::clone(&registry)));
    journal.append(&unconfirmed_single_move("a")).expect("append");
    let _marker = registry.begin("a");

    let out = reconcile(&journal);

    assert!(out.is_empty());
    assert!(diagnostics.entries().is_empty(), "{:?}", diagnostics.entries());
}
