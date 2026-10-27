//! W3-09: the crash / fault-injection matrices of the file-action pipeline, over the merged Wave 3 code
//! (contracts/W3-09.md). Ports of `tests/PhotoReview.Core.Tests/FileActions/` (C# baseline d92d6aae):
//! `CrashMatrixTests.cs`, `FileActionFaultInjectionTests.cs`, `FileActionFaultInjectionRecoveryTests.cs`,
//! `FileActionFaultInjectionGapTests.cs` and their helpers `FaultInjectionRig.cs` (`StepClock`, `FaultBin`, `FaultRig`),
//! `FaultInjectionMatrix.cs`, `FaultInjectionSecondOrder.cs`, `FaultInvariants.cs` and `Fakes/CrashPointFileSystem.cs`.
//! W9-R3 ports the 7eaee2a2 rename deltas in `FaultInvariants.cs` / `FaultInjectionMatrix.cs` and the two R3 matrix scenarios.
//!
//! These are the only tests where the units of Wave 3 run together: `FileActionService` (W3-04) writes the journal
//! (W3-01, through `JournalTransaction`), `UndoService` (W3-05) and `RecoveryRetryService` (W3-06) act on it, and a
//! "restart" runs the startup reconcile (W3-03) over what a crash left on the disk. Each scenario is run once per
//! (numbered mutating call x fault kind): the process dies before, during (torn/partial) or after the call, or the call
//! fails once and the process lives on. After each run the data-safety invariants are checked (labels of the Wave 3
//! plan, docs/MULTI-AGENT-PLAN.md §3; docs/architecture.md files the journal rule under its INV-6): INV-3 (a Prepared
//! line is on disk before the first file mutation: nothing moved without a journal line), INV-4 (one gate shared by the
//! service, undo and retry), INV-5/6 (an undo never reports a file restored that is not complete there, never replays,
//! and the in-memory stack stays truthful), no photo lost / duplicated / silently overwritten, every journal verdict
//! true on the disk, nothing Prepared and a second reconcile a no-op after restart, and the user's next steps (Retry of
//! every Recovery item, a same-named foreign photo moved into the same folder) are safe.
//!
//! In-memory only: [`FakeFileSystem`] behind [`FaultInjectionFileSystem`] (or the local [`CrashPointFs`]) and fake
//! Recycle Bins ([`FaultBin`], [`DeletingBin`]). Never the real disk, never the real Recycle Bin (AGENTS.md).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{AppendStream, Clock, FileStat, FileSystem, NullLog, ReadSeek, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::journal::{DismissOutcome, OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{self, JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_retry::{EnglishRecoveryRetryTexts, RecoveryRetryService};
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, FileActionRequest, FileActionResult, UndoResult,
};
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoService};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_core::text::{eq_ignore_case, ignore_case_key};
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::{FakeClock, FakeFileSystem, FaultInjectionFileSystem, FaultKind, FaultPlan};

use FileOperationType::{Copy, Move, Recycle};

// ---------------------------------------------------------------------------------------------------------------
// Shared basics
// ---------------------------------------------------------------------------------------------------------------

const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SECOND;

fn utc(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text).expect("valid literal").0
}

/// C# `FaultRig.Paths` / `CrashMatrixTests.Paths`: `new AppPaths(@"C:\Users\test\AppData\Local")`.
fn paths() -> AppPaths {
    AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The journal of a test process: no retry sleeps (C# `appendRetryDelay: _ => { }`) and no real log file.
fn new_journal(fs: Arc<dyn FileSystem>, clock: Arc<dyn Clock>) -> OperationJournal {
    OperationJournal::with_options(
        &paths(),
        fs,
        clock,
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(|_| {})),
            log: Some(Arc::new(NullLog)),
            ..OperationJournalOptions::default()
        },
    )
}

/// Every journal line in file order, torn lines skipped (C# `JournalLineParser.TryParse(line.TrimEnd('\r'))`).
fn parse_journal(disk: &FakeFileSystem) -> Vec<JournalEntry> {
    let Some(bytes) = disk.file_content(paths().journal_file) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .split('\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| journal_line::try_parse(line.trim_end_matches('\r')))
        .collect()
}

/// Latest line of every Id ("last line wins"), in the order of each Id's first appearance (C# `Dictionary`).
fn latest_of(lines: Vec<JournalEntry>) -> Vec<JournalEntry> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut latest: Vec<JournalEntry> = Vec::new();
    for line in lines {
        match index.get(&line.id) {
            Some(&i) => latest[i] = line,
            None => {
                index.insert(line.id.clone(), latest.len());
                latest.push(line);
            }
        }
    }
    latest
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn join(folder: &str, name: &str) -> String {
    format!(r"{folder}\{name}")
}

fn assert_no_violations(name: &str, violations: &[String], runs: usize, what: &str) {
    if name.starts_with("Rename") {
        eprintln!("{name}: {runs} {what}, {} invariant violations", violations.len());
    }
    assert!(
        violations.is_empty(),
        "{name}: {} invariant violation(s) over {runs} {what}:\n{}",
        violations.len(),
        violations.iter().take(25).cloned().collect::<Vec<_>>().join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// FaultInjectionRig.cs: StepClock, FaultBin, FaultRig
// ---------------------------------------------------------------------------------------------------------------

/// C# `StepClock`: every read is one second later than the previous one (2026-10-01T00:00:01Z first), so the journal
/// lines of one run are totally ordered.
#[derive(Debug, Default)]
struct StepClock {
    reads: AtomicI64,
}

impl Clock for StepClock {
    fn utc_now(&self) -> DateTimeUtc {
        let n = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        utc("2026-10-01T00:00:00Z")
            .checked_add_ticks(n * TICKS_PER_SECOND)
            .expect("step clock in range")
    }
}

#[derive(Debug, Clone)]
struct BinItem {
    path: String,
    text: Vec<u8>,
    stat: FileStat,
}

#[derive(Debug, Default)]
struct BinState {
    items: Vec<BinItem>,
    /// C# `!VolumeHasBin`: a removable/network volume; only an explicit opt-in may then delete, permanently.
    no_bin: bool,
    /// Paths the service deleted PERMANENTLY (the one intended, user-confirmed loss; never in `items`).
    permanently_deleted: Vec<String>,
}

/// C# `FaultBin`: a fake Recycle Bin (never the real one) whose SEND, RESTORE and permanent DELETE are numbered
/// mutations of the fault injector, so a crash can hit "deleted from the folder but not yet in the bin"-style windows
/// exactly as the injector decides. Its content survives a "process restart" (it is OS state, not process state).
///
/// DIVERGENCE (P-13): a restore that fails returns `Err` (the Rust `RecycleBin::try_restore` contract) where the C#
/// fake, like the C# `WindowsRecycleBin`, swallowed the failure into `false`.
struct FaultBin {
    fs: Arc<FaultInjectionFileSystem>,
    state: Mutex<BinState>,
}

impl FaultBin {
    fn new(fs: Arc<FaultInjectionFileSystem>) -> Self {
        Self {
            fs,
            state: Mutex::new(BinState::default()),
        }
    }

    fn items(&self) -> Vec<BinItem> {
        lock(&self.state).items.clone()
    }

    fn permanently_deleted(&self) -> Vec<String> {
        lock(&self.state).permanently_deleted.clone()
    }

    fn set_no_bin(&self) {
        lock(&self.state).no_bin = true;
    }
}

impl RecycleBin for FaultBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        let stat = self
            .fs
            .try_get_file_stat(path)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "gone"))?;
        let text = self.fs.read_all_text(path)?.into_bytes();
        let disk = Arc::clone(self.fs.disk());
        let mut item = Some(BinItem {
            path: path.to_string_lossy().into_owned(),
            text,
            stat,
        });
        self.fs.mutate(
            "recycle",
            &mut || {
                disk.delete(path)?;
                if let Some(item) = item.take() {
                    lock(&self.state).items.push(item);
                }
                Ok(())
            },
            None,
            None,
        )
    }

    fn can_recycle(&self, _path: &Path) -> bool {
        !lock(&self.state).no_bin
    }

    fn delete_permanently(&self, path: &Path) -> io::Result<()> {
        assert!(
            lock(&self.state).no_bin,
            "delete_permanently called for a volume that has a Recycle Bin"
        );
        let disk = Arc::clone(self.fs.disk());
        self.fs.mutate(
            "delete-permanent",
            &mut || {
                disk.delete(path)?;
                lock(&self.state)
                    .permanently_deleted
                    .push(path.to_string_lossy().into_owned());
                Ok(())
            },
            None,
            None,
        )
    }

    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        let original = original_path.to_string_lossy().into_owned();
        let matches = |item: &BinItem| {
            eq_ignore_case(&item.path, &original)
                && item.stat.length == expected_size
                && item.stat.last_write_utc == expected_last_write_utc
        };
        let found = lock(&self.state).items.iter().any(matches);
        if !found || self.fs.file_exists(original_path) {
            return Ok(false);
        }
        let disk = Arc::clone(self.fs.disk());
        self.fs.mutate(
            "restore",
            &mut || {
                let mut state = lock(&self.state);
                if let Some(index) = state.items.iter().position(matches) {
                    let item = state.items.remove(index);
                    drop(state);
                    disk.add_file_at(original_path, &item.text, item.stat.last_write_utc);
                }
                Ok(())
            },
            None,
            None,
        )?;
        Ok(true)
    }
}

/// What an action of a scenario returned (C# `List<object> Outcomes`).
#[derive(Debug, Clone)]
enum Outcome {
    Action(FileActionResult),
    Group(CaptureGroupActionResult),
    Undo(UndoResult),
    Dismiss(DismissOutcome),
}

const PHOTOS: &str = r"C:\photos";
const SEL: &str = r"C:\photos\sel";
const OTHER: &str = r"C:\other";
const SEL_SUB: &str = r"C:\photos\sel\sub";
const FOREIGN: &str = r"C:\elsewhere";
const A: &str = r"C:\photos\a.jpg";
const A_SEL: &str = r"C:\photos\sel\a.jpg";
const B: &str = r"C:\photos\b.jpg";
const GROUP_NAMES: [&str; 3] = ["a.jpg", "a.cr2", "a.xmp"];

/// C# `FaultRig`: one simulated machine. Every photo has a unique name and a unique content derived from it, which is
/// what lets the invariants tell "lost", "duplicated" and "overwritten" apart. [`Rig::restart`] abandons every
/// in-memory object (the process died) and builds new ones over what survived on the disk.
struct Rig {
    disk: Arc<FakeFileSystem>,
    fs: Arc<FaultInjectionFileSystem>,
    bin: Arc<FaultBin>,
    /// The user's cancellation (C# `Cts`): a `CancelAfter` fault cancels it right after the numbered call's effect.
    token: Arc<Mutex<CancelToken>>,
    clock: Arc<StepClock>,
    journal: Arc<OperationJournal>,
    service: Arc<FileActionService>,
    undo: UndoService,
    outcomes: Vec<Outcome>,
    /// True in the single-Delete undo scenario: Ctrl+Z restores the file without any journal line, so its Committed
    /// Delete line legitimately stays.
    single_recycle_may_be_restored: bool,
    /// (name, content) of every seeded photo, in seed order.
    contents: Vec<(String, String)>,
    seed_paths: Vec<String>,
}

impl Rig {
    fn new() -> Self {
        let disk = Arc::new(FakeFileSystem::new());
        let fs = Arc::new(FaultInjectionFileSystem::new(Arc::clone(&disk)));
        let token = Arc::new(Mutex::new(CancelToken::new()));
        let hook_token = Arc::clone(&token);
        fs.set_cancel_hook(Some(Arc::new(move || lock(&hook_token).cancel())));
        let bin = Arc::new(FaultBin::new(Arc::clone(&fs)));
        let clock = Arc::new(StepClock::default());
        let (journal, service, undo) = Self::services(&fs, &bin, &clock);
        Self {
            disk,
            fs,
            bin,
            token,
            clock,
            journal,
            service,
            undo,
            outcomes: Vec::new(),
            single_recycle_may_be_restored: false,
            contents: Vec::new(),
            seed_paths: Vec::new(),
        }
    }

    /// The process's services: one journal, one `FileActionService` whose gate `UndoService` shares (INV-4).
    fn services(
        fs: &Arc<FaultInjectionFileSystem>,
        bin: &Arc<FaultBin>,
        clock: &Arc<StepClock>,
    ) -> (Arc<OperationJournal>, Arc<FileActionService>, UndoService) {
        let journal = Arc::new(new_journal(fs.clone(), clock.clone()));
        let service = Arc::new(FileActionService::new(
            Arc::clone(&journal),
            fs.clone(),
            clock.clone(),
            bin.clone(),
        ));
        let undo = UndoService::new(
            Arc::clone(&journal),
            fs.clone(),
            bin.clone(),
            Arc::new(EnglishUndoTexts),
        )
        .with_gate(service.clone())
        .with_clock(clock.clone());
        (journal, service, undo)
    }

    fn seed(mut self, folder: &str, names: &[&str]) -> Self {
        for name in names {
            let path = join(folder, name);
            let content = format!("{name}:{}", "#".repeat(12 + self.contents.len()));
            self.contents.push(((*name).to_owned(), content.clone()));
            self.seed_paths.push(path.clone());
            let days = self.contents.len() as i64;
            let stamp = utc("2026-09-01T00:00:00Z")
                .checked_add_ticks(days * TICKS_PER_DAY)
                .expect("in range");
            self.disk.add_file_at(&path, content.as_bytes(), stamp);
        }
        self
    }

    fn token(&self) -> CancelToken {
        lock(&self.token).clone()
    }

    fn new_journal(&self) -> OperationJournal {
        new_journal(self.fs.clone(), self.clock.clone())
    }

    /// C# `Restart`: healthy file system, brand-new in-memory objects on the surviving disk.
    fn restart(&mut self) -> Arc<OperationJournal> {
        self.fs.arm(None);
        *lock(&self.token) = CancelToken::new();
        let (journal, service, undo) = Self::services(&self.fs, &self.bin, &self.clock);
        self.journal = journal;
        self.service = service;
        self.undo = undo;
        Arc::clone(&self.journal)
    }

    fn retry_service(&self) -> RecoveryRetryService {
        RecoveryRetryService::new(
            Arc::clone(&self.journal),
            self.fs.clone(),
            self.clock.clone(),
            Arc::new(EnglishRecoveryRetryTexts),
        )
        .with_recycle_bin(self.bin.clone())
    }

    fn content_of(&self, path_or_name: &str) -> Option<&str> {
        let name = file_name(path_or_name);
        self.contents
            .iter()
            .find(|(known, _)| eq_ignore_case(known, name))
            .map(|(_, content)| content.as_str())
    }

    /// The file sits at `path` complete (its exact original content).
    fn has_full(&self, path: &str) -> bool {
        match (self.content_of(path), self.disk.file_content(path)) {
            (Some(content), Some(bytes)) => bytes == content.as_bytes(),
            _ => false,
        }
    }

    fn exists(&self, path: &str) -> bool {
        self.disk.file_exists(Path::new(path))
    }

    fn bin_count(&self, path: &str) -> usize {
        let Some(content) = self.content_of(path) else { return 0 };
        self.bin
            .items()
            .iter()
            .filter(|item| eq_ignore_case(&item.path, path) && item.text == content.as_bytes())
            .count()
    }

    /// Every file directly in the scenario folders (plus `extra`): path key => content.
    fn snapshot(&self, extra: &[&str]) -> BTreeMap<String, Vec<u8>> {
        let folders: Vec<&Path> = [PHOTOS, SEL, OTHER, SEL_SUB]
            .iter()
            .chain(extra.iter())
            .map(|folder| Path::new(*folder))
            .collect();
        self.fs.snapshot(&folders)
    }

    fn journal_lines(&self) -> Vec<JournalEntry> {
        parse_journal(&self.disk)
    }

    fn journal_text(&self) -> Vec<u8> {
        self.disk.file_content(paths().journal_file).unwrap_or_default()
    }

    fn latest(&self) -> Vec<JournalEntry> {
        latest_of(self.journal_lines())
    }

    fn failed_entries(&self) -> Vec<JournalEntry> {
        self.latest()
            .into_iter()
            .filter(|e| e.state == JournalState::Failed)
            .collect()
    }
}

/// C# `FaultRig.MembersOf`: the group members, or the single entry as one member.
fn members_of(entry: &JournalEntry) -> Vec<JournalGroupMember> {
    match &entry.group_members {
        Some(members) if !members.is_empty() => members.clone(),
        _ => vec![JournalGroupMember {
            source: entry.source.clone(),
            destination: entry.destination.clone(),
            size: entry.size,
            last_write_utc: entry.last_write_utc,
            permanent: false,
        }],
    }
}

fn capture(xmp: bool) -> CaptureGroup {
    CaptureGroup::new(
        join(PHOTOS, "a.jpg"),
        join(PHOTOS, "a.cr2"),
        xmp.then(|| join(PHOTOS, "a.xmp")),
    )
    .expect("valid capture group")
}

// ---------------------------------------------------------------------------------------------------------------
// Scenario steps (the C# `Do`, `DoGroup`, `Undo`, `Retry`, ... helpers). `Err` = something escaped the code under
// test (C#: an exception left the service).
// ---------------------------------------------------------------------------------------------------------------

type Create = dyn Fn() -> Rig;
type Act = dyn Fn(&mut Rig) -> Result<(), String>;
type Extra = dyn Fn(&mut Rig, &str, &mut Vec<String>);

fn single_rig() -> Rig {
    Rig::new().seed(PHOTOS, &["a.jpg", "b.jpg"])
}

fn group_rig() -> Rig {
    Rig::new().seed(PHOTOS, &["a.jpg", "a.cr2", "a.xmp", "b.jpg"])
}

#[derive(Clone, Copy)]
struct Opts {
    cancellable: bool,
    permanent: bool,
}

const PLAIN: Opts = Opts {
    cancellable: false,
    permanent: false,
};
const CANCELLABLE: Opts = Opts {
    cancellable: true,
    permanent: false,
};

fn do_single(
    rig: &mut Rig,
    source: &str,
    operation: FileOperationType,
    destination: Option<&str>,
    opts: Opts,
) -> Result<(), String> {
    let request = FileActionRequest {
        source: source.to_owned(),
        operation,
        destination: destination.map(str::to_owned),
        allow_permanent_delete: opts.permanent,
    };
    let cancel = if opts.cancellable {
        rig.token()
    } else {
        CancelToken::none()
    };
    let result = rig.service.execute(&request, &cancel);
    rig.outcomes.push(Outcome::Action(result));
    Ok(())
}

fn do_group(rig: &mut Rig, operation: FileOperationType, destination: Option<&str>, opts: Opts) -> Result<(), String> {
    let request = CaptureGroupActionRequest {
        group: capture(true),
        operation,
        destination: destination.map(str::to_owned),
        allow_permanent_delete: opts.permanent,
        destination_file_names: None,
    };
    let cancel = if opts.cancellable {
        rig.token()
    } else {
        CancelToken::none()
    };
    let result = rig.service.execute_group(&request, &cancel);
    rig.outcomes.push(Outcome::Group(result));
    Ok(())
}

fn move_a(rig: &mut Rig) -> Result<(), String> {
    do_single(rig, A, Move, Some("sel"), PLAIN)
}

fn copy_a(rig: &mut Rig) -> Result<(), String> {
    do_single(rig, A, Copy, Some("sel"), PLAIN)
}

fn recycle_a(rig: &mut Rig) -> Result<(), String> {
    do_single(rig, A, Recycle, None, PLAIN)
}

fn group_move(rig: &mut Rig) -> Result<(), String> {
    do_group(rig, Move, Some("sel"), PLAIN)
}

fn group_copy(rig: &mut Rig) -> Result<(), String> {
    do_group(rig, Copy, Some("sel"), PLAIN)
}

fn group_recycle(rig: &mut Rig) -> Result<(), String> {
    do_group(rig, Recycle, None, PLAIN)
}

/// C# `FileActionFaultInjectionRecoveryTests.Undo`: Ctrl+Z.
fn undo_last(rig: &mut Rig) -> Result<(), String> {
    let result = rig.undo.undo_last();
    rig.outcomes.push(Outcome::Undo(result));
    Ok(())
}

/// C# `Retry` / `CancellableRetry`: "Retry" on the first Recovery item.
fn retry_first(rig: &mut Rig, cancellable: bool) -> Result<(), String> {
    let failed = rig
        .failed_entries()
        .into_iter()
        .next()
        .ok_or_else(|| "no Failed entry to retry".to_owned())?;
    let cancel = if cancellable { rig.token() } else { CancelToken::none() };
    rig.retry_service()
        .retry_move_or_copy(&failed, false, &cancel)
        .map(|_| ())
        .map_err(|_| "Canceled escaped the retry (C# TaskCanceledException)".to_owned())
}

/// C# `DismissFailed`: a failed dismiss is an `Err` and the item simply stays in the Recovery window.
fn dismiss_failed(rig: &mut Rig) -> Result<(), String> {
    let failed = rig.failed_entries();
    if let Ok(outcome) = rig.journal.dismiss(&failed) {
        rig.outcomes.push(Outcome::Dismiss(outcome));
    }
    Ok(())
}

/// C# `AfterAction`: the action ran healthy and was registered for Ctrl+Z; only the undo itself is numbered.
fn after_action(group: bool, operation: FileOperationType) -> Rig {
    let mut rig = if group { group_rig() } else { single_rig() };
    let destination = (operation == Move).then_some("sel");
    if group {
        let request = CaptureGroupActionRequest {
            destination: destination.map(str::to_owned),
            ..CaptureGroupActionRequest::new(capture(true), operation)
        };
        let done = rig.service.execute_group(&request, &CancelToken::none());
        assert!(done.succeeded, "{:?}", done.error);
        rig.undo.register_group(&done);
    } else {
        let request = FileActionRequest {
            destination: destination.map(str::to_owned),
            ..FileActionRequest::new(A, operation)
        };
        let done = rig.service.execute(&request, &CancelToken::none());
        assert!(done.succeeded, "{:?}", done.error);
        rig.undo.register(&done);
    }
    rig.outcomes.clear();
    rig.single_recycle_may_be_restored = !group && operation == Recycle;
    rig
}

fn failed_entry(id: &str, operation: FileOperationType, member: &JournalGroupMember, now: DateTimeUtc) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: operation,
        state: JournalState::Failed,
        source: member.source.clone(),
        destination: member.destination.clone(),
        size: member.size,
        last_write_utc: member.last_write_utc,
        timestamp_utc: JournalDateTime::utc(now),
        error: Some("boom".to_owned()),
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

fn member(rig: &Rig, source: &str, destination: Option<String>) -> JournalGroupMember {
    let stat = rig.disk.peek_stat(source).expect("seeded");
    JournalGroupMember {
        source: source.to_owned(),
        destination,
        size: stat.length as i64,
        last_write_utc: JournalDateTime::utc(stat.last_write_utc),
        permanent: false,
    }
}

/// C# `WithFailedSingle`: a Recovery item for a Move/Copy of a.jpg to sel that failed before touching anything.
fn with_failed_single(operation: FileOperationType) -> Rig {
    let rig = single_rig();
    let one = member(&rig, A, Some(A_SEL.to_owned()));
    let now = rig.clock.utc_now();
    rig.journal
        .append(&failed_entry("failed1", operation, &one, now))
        .expect("healthy append");
    rig
}

/// C# `WithHalfDoneGroup`: a group action that died after its first member (a.jpg already moved to sel, or already in
/// the bin); the RAW and the XMP untouched.
fn with_half_done_group(operation: FileOperationType) -> Rig {
    let rig = group_rig();
    let members: Vec<JournalGroupMember> = GROUP_NAMES
        .iter()
        .map(|name| {
            let destination = (operation != Recycle).then(|| join(SEL, name));
            member(&rig, &join(PHOTOS, name), destination)
        })
        .collect();
    let first = &members[0];
    if operation == Move {
        rig.disk.create_directory(Path::new(SEL)).expect("mkdir");
        rig.disk
            .move_file(
                Path::new(&first.source),
                Path::new(first.destination.as_deref().expect("dest")),
            )
            .expect("move");
    } else {
        let text = rig.disk.file_content(&first.source).expect("seeded");
        lock(&rig.bin.state).items.push(BinItem {
            path: first.source.clone(),
            text,
            stat: FileStat {
                length: first.size as u64,
                last_write_utc: first.last_write_utc.value,
            },
        });
        rig.disk.delete(Path::new(&first.source)).expect("delete");
    }
    let now = rig.clock.utc_now();
    let entry = JournalEntry {
        group_id: Some("g1".to_owned()),
        group_members: Some(members.clone()),
        ..failed_entry("failedgroup", operation, first, now)
    };
    rig.journal.append(&entry).expect("healthy append");
    rig
}

fn no_bin_single() -> Rig {
    let rig = single_rig();
    rig.bin.set_no_bin();
    rig
}

fn no_bin_group() -> Rig {
    let rig = group_rig();
    rig.bin.set_no_bin();
    rig
}

fn permanent_group(rig: &mut Rig, cancellable: bool) -> Result<(), String> {
    do_group(
        rig,
        Recycle,
        None,
        Opts {
            cancellable,
            permanent: true,
        },
    )
}

// ---------------------------------------------------------------------------------------------------------------
// FaultInvariants.cs
// ---------------------------------------------------------------------------------------------------------------

/// A non-Copy operation (a later Delete/Move of the same file, "Copy then Delete") legitimately removed the source.
fn source_left_by_later_op(lines: &[JournalEntry], source: &str) -> bool {
    lines
        .iter()
        .any(|line| line.op_type != Copy && members_of(line).iter().any(|m| eq_ignore_case(&m.source, source)))
}

fn is_permanently_deleted(rig: &Rig, path: &str) -> bool {
    rig.bin
        .permanently_deleted()
        .iter()
        .any(|gone| eq_ignore_case(gone, path))
}

/// C# 7eaee2a2 `FaultInvariants.SourceStillThere`.
fn source_still_there(rig: &Rig, source: &str, destination: Option<&str>) -> bool {
    // C# 7eaee2a2 FaultInvariants.SourceStillThere: inspect the disk, not the crashed process wrapper.
    if !rig.exists(source) {
        return false;
    }
    let Some(destination) = destination else {
        return true;
    };
    if source == destination || !eq_ignore_case(source, destination) {
        return true;
    }
    let stored = rig
        .disk
        .get_stored_file_names(Path::new(source))
        .expect("fake stored spelling");
    stored.len() != 1 || stored[0] != file_name(destination)
}

/// C# `CheckSucceededEffect`: the effect a SUCCESS report (whole action, or one completed group member) promised.
fn check_succeeded_effect(
    rig: &Rig,
    operation: FileOperationType,
    (source, destination): (&str, Option<&str>),
    permanent: bool,
    ctx: &str,
    v: &mut Vec<String>,
) {
    let dest_full = destination.is_some_and(|d| rig.has_full(d));
    match operation {
        Move => {
            if source_still_there(rig, source, destination) {
                v.push(format!(
                    "{ctx}: Move reported done but the source {source} is still there"
                ));
            }
            if !dest_full {
                v.push(format!(
                    "{ctx}: Move reported done but {destination:?} is not the complete file"
                ));
            }
        }
        Copy => {
            if !dest_full {
                v.push(format!(
                    "{ctx}: Copy reported done but {destination:?} is not the complete file"
                ));
            }
            if !source_left_by_later_op(&rig.journal_lines(), source) && !rig.has_full(source) {
                v.push(format!(
                    "{ctx}: Copy reported done but the source {source} is missing or damaged"
                ));
            }
        }
        Recycle => {
            if rig.exists(source) {
                v.push(format!(
                    "{ctx}: Delete reported done but {source} is still in the folder"
                ));
            }
            if permanent {
                if !is_permanently_deleted(rig, source) || rig.bin_count(source) != 0 {
                    v.push(format!(
                        "{ctx}: permanent Delete reported done but {source} was not deleted permanently exactly like that"
                    ));
                }
            } else if rig.bin_count(source) != 1 {
                v.push(format!(
                    "{ctx}: Delete reported done but {source} is not (exactly once) in the Recycle Bin"
                ));
            }
        }
    }
}

/// C# `CheckState`: no photo lost, nothing moved/created without a journal line, nothing Prepared after recovery, every
/// Committed line true on the disk.
fn check_state(rig: &Rig, ctx: &str, v: &mut Vec<String>, after_recovery: bool) {
    let files = rig.snapshot(&[]);
    let lines = rig.journal_lines();
    let latest = latest_of(lines.clone());
    let bin_items = rig.bin.items();
    let gone_permanently = rig.bin.permanently_deleted();

    if after_recovery {
        for pending in latest.iter().filter(|e| e.state == JournalState::Prepared) {
            v.push(format!(
                "{ctx}: {:?} of {} stayed Prepared after reconcile",
                pending.op_type, pending.source
            ));
        }
    }

    // NO LOSS / NO SILENT OVERWRITE: every photo exists complete somewhere (folder or bin).
    for (name, content) in &rig.contents {
        let in_folder = files.values().any(|text| text == content.as_bytes());
        let in_bin = bin_items.iter().any(|item| item.text == content.as_bytes());
        // A permanent delete (volume without a bin, user opted in) is the one INTENDED loss.
        let gone = gone_permanently
            .iter()
            .any(|path| eq_ignore_case(file_name(path), name));
        if !in_folder && !in_bin && !gone {
            v.push(format!(
                "{ctx}: PHOTO LOST: no complete {name} anywhere (folders or Recycle Bin)"
            ));
        }
    }

    // NOTHING HAPPENS WITHOUT A JOURNAL LINE (INV-3): Prepared is written before the first mutation.
    let mut named: HashSet<String> = HashSet::new();
    for line in &lines {
        for m in members_of(line) {
            named.insert(ignore_case_key(&m.source));
            if let Some(destination) = &m.destination {
                named.insert(ignore_case_key(destination));
            }
        }
    }
    let seeds: HashSet<String> = rig.seed_paths.iter().map(|p| ignore_case_key(p)).collect();
    for seed in &rig.seed_paths {
        if !rig.has_full(seed) && !named.contains(&ignore_case_key(seed)) {
            v.push(format!(
                "{ctx}: {seed} left its place with NO journal line (unjournaled mutation)"
            ));
        }
    }
    for path in files.keys() {
        if !seeds.contains(path) && !named.contains(path) {
            v.push(format!(
                "{ctx}: {path} exists but no journal line mentions it (unjournaled mutation/partial file)"
            ));
        }
    }

    // EVERY JOURNAL VERDICT IS TRUE.
    for entry in latest.iter().filter(|e| e.state != JournalState::Dismissed) {
        let entry_members = members_of(entry);
        // An undo (journaled for Move and group Delete) legitimately reverses its original line, which stays Committed.
        let superseded_by_undo = entry.undo != Some(true)
            && entry.op_type != Copy
            && lines.iter().any(|u| {
                u.undo == Some(true)
                    && u.id != entry.id
                    && u.op_type == entry.op_type
                    && members_of(u).iter().any(|um| {
                        entry_members.iter().any(|em| {
                            let target = if entry.op_type == Move {
                                em.destination.as_deref()
                            } else {
                                Some(em.source.as_str())
                            };
                            target.is_some_and(|t| eq_ignore_case(&um.source, t))
                        })
                    })
            });
        for m in &entry_members {
            // C# 7eaee2a2 FaultInvariants: a later Move/rename may supersede this destination.
            let moved_on = entry.op_type == Move
                && m.destination.as_deref().is_some_and(|destination| {
                    let entry_at = lines.iter().rposition(|line| line.id == entry.id).unwrap_or(0);
                    latest.iter().any(|other| {
                        other.id != entry.id
                            && other.op_type == Move
                            && matches!(
                                other.state,
                                JournalState::Committed | JournalState::Prepared | JournalState::Failed
                            )
                            && lines
                                .iter()
                                .rposition(|line| line.id == other.id)
                                .is_some_and(|at| at > entry_at)
                            && members_of(other).iter().any(|member| {
                                eq_ignore_case(&member.source, destination)
                                    && member.destination.as_deref() != Some(member.source.as_str())
                            })
                            && (other.state == JournalState::Committed || !rig.exists(destination))
                    })
                });
            match entry.op_type {
                Move if entry.state == JournalState::Committed && !superseded_by_undo && !moved_on => {
                    if source_still_there(rig, &m.source, m.destination.as_deref()) {
                        v.push(format!(
                            "{ctx}: Move {} is Committed but the source is still there",
                            m.source
                        ));
                    }
                    if !m.destination.as_deref().is_some_and(|d| rig.has_full(d)) {
                        v.push(format!(
                            "{ctx}: Move {} is Committed but {:?} is not the complete file",
                            m.source, m.destination
                        ));
                    }
                }
                Move => {}
                Copy => {
                    if !source_left_by_later_op(&lines, &m.source) && !rig.has_full(&m.source) {
                        v.push(format!(
                            "{ctx}: Copy of {} (state {:?}) left its source missing or damaged",
                            m.source, entry.state
                        ));
                    }
                    if entry.state == JournalState::Committed
                        && !m.destination.as_deref().is_some_and(|d| rig.has_full(d))
                    {
                        v.push(format!(
                            "{ctx}: Copy {} is Committed but {:?} is not the complete file",
                            m.source, m.destination
                        ));
                    }
                }
                Recycle => {
                    // The photo is in exactly one place (folder or bin) in EVERY state: never both, never neither.
                    let permanent_delete = entry.permanent == Some(true) || m.permanent;
                    let elsewhere = if permanent_delete {
                        usize::from(is_permanently_deleted(rig, &m.source))
                    } else {
                        rig.bin_count(&m.source)
                    };
                    let mut places = usize::from(rig.has_full(&m.source)) + elsewhere;
                    if permanent_delete && rig.bin_count(&m.source) != 0 {
                        v.push(format!(
                            "{ctx}: permanent Delete of {} put a copy in the Recycle Bin",
                            m.source
                        ));
                    }
                    // Another JOURNALED Move of the same file may have legitimately put it elsewhere meanwhile.
                    if places == 0
                        && lines.iter().any(|other| {
                            other.op_type == Move
                                && other.id != entry.id
                                && members_of(other).iter().any(|om| {
                                    eq_ignore_case(&om.source, &m.source)
                                        && om.destination.as_deref().is_some_and(|d| rig.has_full(d))
                                })
                        })
                    {
                        places = 1;
                    }
                    if places != 1 {
                        v.push(format!(
                            "{ctx}: Delete of {} (state {:?}, undo={:?}) leaves the photo in {places} places",
                            m.source, entry.state, entry.undo
                        ));
                    }
                    // Committed means it left the folder (a single Delete that Ctrl+Z restored is not journaled again).
                    if entry.state == JournalState::Committed
                        && entry.undo != Some(true)
                        && !superseded_by_undo
                        && !rig.single_recycle_may_be_restored
                        && rig.exists(&m.source)
                    {
                        v.push(format!(
                            "{ctx}: Delete of {} is Committed but the file is still in the folder",
                            m.source
                        ));
                    }
                }
            }
        }
    }
}

/// C# `FollowUpsAsync`: what a user does after the failure, in a healthy new process: retry every Recovery item, then
/// move a DIFFERENT photo that has the same file name into the same folder.
fn follow_ups(rig: &mut Rig, ctx: &str, v: &mut Vec<String>) {
    let retry = rig.retry_service();
    for failed in rig.failed_entries() {
        if retry.retry_move_or_copy(&failed, false, &CancelToken::none()).is_err() {
            v.push(format!(
                "{ctx}: Recovery retry of {:?} {} escaped as Canceled",
                failed.op_type, failed.source
            ));
        }
    }

    check_state(rig, &format!("{ctx} (after retrying every Recovery item)"), v, true);
    let settled = rig.journal_text();
    let _ = rig.journal.reconcile_pending_operations(None);
    if rig.journal_text() != settled {
        v.push(format!("{ctx}: reconcile after the retries appended lines"));
    }

    const FOREIGN_CONTENT: &[u8] = b"FOREIGN-PHOTO-THAT-MUST-SURVIVE";
    for name in ["a.jpg", "a.cr2", "a.xmp", "b.jpg"] {
        let before = rig.snapshot(&[FOREIGN]);
        let foreign_path = join(FOREIGN, name);
        rig.disk
            .add_file_at(&foreign_path, FOREIGN_CONTENT, utc("2026-08-01T00:00:00Z"));
        let request = FileActionRequest {
            destination: Some(SEL.to_owned()),
            ..FileActionRequest::new(foreign_path.clone(), Move)
        };
        let result = rig.service.execute(&request, &CancelToken::none());
        let after = rig.snapshot(&[FOREIGN]);
        for (path, text) in &before {
            if after.get(path) != Some(text) {
                v.push(format!(
                    "{ctx}: moving a same-named foreign photo (success={}) changed or removed {path}: SILENT OVERWRITE/LOSS",
                    result.succeeded
                ));
            }
        }
        if !after.values().any(|text| text == FOREIGN_CONTENT) {
            v.push(format!("{ctx}: the foreign photo {name} was lost by its own Move"));
        }
        let _ = rig.disk.delete(Path::new(&foreign_path));
        let moved_foreign = join(SEL, name);
        if rig.disk.file_content(&moved_foreign).as_deref() == Some(FOREIGN_CONTENT) {
            let _ = rig.disk.delete(Path::new(&moved_foreign));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// FaultInjectionMatrix.cs
// ---------------------------------------------------------------------------------------------------------------

/// C# `FaultInjectionMatrix.Kinds`: every kind except `CancelAfter`.
const DEFAULT_KINDS: [FaultKind; 7] = [
    FaultKind::CrashBefore,
    FaultKind::CrashAfter,
    FaultKind::CrashPartial,
    FaultKind::CrashMoveCopiedNotDeleted,
    FaultKind::FailOnce,
    FaultKind::FailPartial,
    FaultKind::FailUnauthorized,
];

const CANCEL_ONLY: [FaultKind; 1] = [FaultKind::CancelAfter];

/// The process lives on after this kind of fault.
fn is_alive_kind(kind: FaultKind) -> bool {
    matches!(
        kind,
        FaultKind::FailOnce | FaultKind::FailPartial | FaultKind::FailUnauthorized | FaultKind::CancelAfter
    )
}

/// C# `CheckOutcomes`: what the code under test REPORTED must be the truth on the disk exactly as the failure left it.
fn check_outcomes(rig: &Rig, ctx: &str, v: &mut Vec<String>) {
    for outcome in &rig.outcomes {
        match outcome {
            Outcome::Action(result) if result.no_change => {}
            Outcome::Action(result) if result.succeeded && !result.rejected => check_succeeded_effect(
                rig,
                result.operation,
                (&result.source, result.destination_path.as_deref()),
                result.permanently_deleted,
                &format!("{ctx} (reported success)"),
                v,
            ),
            Outcome::Action(failed) if !failed.succeeded && failed.operation == Move && !rig.fs.dead() => {
                let present = source_still_there(rig, &failed.source, failed.destination_path.as_deref());
                if failed.source_removed == present {
                    v.push(format!(
                        "{ctx}: failed Move reports source_removed={} but the source {} on disk",
                        failed.source_removed,
                        if present { "is" } else { "is not" }
                    ));
                }
            }
            Outcome::Action(_) => {}
            Outcome::Undo(undo) => {
                // Whatever an undo reports as back in place must really be complete there (INV-5).
                let restored = match &undo.restored_paths {
                    Some(paths) => paths.clone(),
                    None if undo.succeeded => vec![undo.source.clone()],
                    None => Vec::new(),
                };
                for path in restored {
                    if !rig.has_full(&path) {
                        v.push(format!(
                            "{ctx}: Undo reported {path} restored but it is not the complete file there"
                        ));
                    }
                }
            }
            Outcome::Group(group) => {
                for m in group
                    .members
                    .iter()
                    .filter(|m| m.state_known && (m.completed || group.succeeded))
                {
                    check_succeeded_effect(
                        rig,
                        group.operation,
                        (&m.member.source, m.member.destination.as_deref()),
                        m.member.permanent,
                        &format!("{ctx} (reported member success)"),
                        v,
                    );
                }
            }
            Outcome::Dismiss(_) => {}
        }
    }
}

/// C# `VerifyAsync`.
fn verify(rig: &mut Rig, ctx: &str, v: &mut Vec<String>, extra: Option<&Extra>, outcomes: bool) {
    if outcomes {
        check_outcomes(rig, ctx, v);
    }
    if let Some(extra) = extra {
        extra(rig, ctx, v);
    }
    check_state(rig, &format!("{ctx} (right after the fault)"), v, false);

    let journal = rig.restart();
    let _ = journal.reconcile_pending_operations(None);
    check_state(rig, &format!("{ctx} (after restart+reconcile)"), v, true);
    let settled = rig.journal_text();
    let _ = journal.reconcile_pending_operations(None);
    let _ = rig.new_journal().reconcile_pending_operations(None);
    if rig.journal_text() != settled {
        v.push(format!(
            "{ctx}: reconcile is NOT idempotent (a second pass appended journal lines)"
        ));
    }

    follow_ups(rig, ctx, v);
}

/// C# `AssertAllFaultPointsAsync`: runs `act` on a fresh rig for every injection point. `create` builds the rig AND
/// runs the healthy part of the scenario (never faulted); only `act` is numbered.
fn assert_all_fault_points(
    name: &str,
    create: &Create,
    act: &Act,
    extra: Option<&Extra>,
    kinds: &[FaultKind],
    min_calls: usize,
) {
    let mut violations = Vec::new();
    let mut probe = create();
    probe.fs.arm(None);
    if let Err(escaped) = act(&mut probe) {
        violations.push(format!("{name} [healthy]: {escaped}"));
    }
    let calls = probe.fs.kinds();
    assert!(
        calls.len() >= min_calls,
        "{name}: the scenario must perform at least {min_calls} mutating calls (saw {}); the matrix would prove nothing",
        calls.len()
    );
    // Healthy run: nothing may be violated either (guards the checker itself against false positives).
    verify(&mut probe, &format!("{name} [healthy]"), &mut violations, extra, true);

    let mut runs = 0;
    for point in 1..=calls.len() {
        let call = &calls[point - 1];
        for &kind in kinds {
            if !kind.applies(call) {
                continue;
            }
            runs += 1;
            let ctx = format!("{name} [{kind:?} at call #{point}/{} = {call}]", calls.len());
            let mut rig = create();
            rig.fs.arm(Some(FaultPlan { at: point, kind }));
            let alive = is_alive_kind(kind);
            if let Err(escaped) = act(&mut rig)
                && (alive || !rig.fs.dead())
            {
                violations.push(format!("{ctx}: an I/O failure ESCAPED the code under test: {escaped}"));
                continue;
            }
            verify(&mut rig, &ctx, &mut violations, extra, true);
        }
    }

    assert!(runs >= calls.len(), "{name}: only {runs} injection runs");
    assert_no_violations(name, &violations, runs, "injection runs");
}

fn matrix(name: &str, create: &Create, act: &Act) {
    assert_all_fault_points(name, create, act, None, &DEFAULT_KINDS, 2);
}

/// One comparable line per live journal verdict (Dismissed lines and Ids excluded: Ids differ between two runs).
fn summary(rig: &Rig) -> String {
    let mut lines: Vec<String> = rig
        .latest()
        .iter()
        .filter(|e| e.state != JournalState::Dismissed)
        .map(|e| {
            let members: Vec<String> = members_of(e)
                .iter()
                .map(|m| format!("{}>{:?}", m.source, m.destination))
                .collect();
            format!("{:?}|{:?}|undo={:?}|{}", e.op_type, e.state, e.undo, members.join(","))
        })
        .collect();
    lines.sort();
    lines.join(" ; ")
}

/// The process died at `plan` while running `act` (the disk shows what that kind of death leaves).
fn crashed(create: &Create, act: &Act, plan: FaultPlan) -> Rig {
    let mut rig = create();
    rig.fs.arm(Some(plan));
    let _ = act(&mut rig);
    rig
}

/// C# `AssertReconcileInterruptedAsync`: the startup reconcile itself is interrupted at every one of its own mutating
/// calls, for every crash point of `act`: the next, healthy start must still reach exactly the verdicts an
/// uninterrupted reconcile reaches, lose nothing, and be idempotent.
fn assert_reconcile_interrupted(name: &str, create: &Create, act: &Act) {
    let mut violations = Vec::new();
    let mut probe = create();
    probe.fs.arm(None);
    let _ = act(&mut probe);
    let outer_calls = probe.fs.kinds();
    let mut runs = 0;
    for point in 1..=outer_calls.len() {
        for outer in [FaultKind::CrashBefore, FaultKind::CrashAfter] {
            if !outer.applies(&outer_calls[point - 1]) {
                continue;
            }
            let outer_plan = FaultPlan { at: point, kind: outer };
            let mut clean = crashed(create, act, outer_plan);
            let _ = clean.restart().reconcile_pending_operations(None);
            let expected = summary(&clean);
            let reconcile_calls = clean.fs.kinds();
            for inner in 1..=reconcile_calls.len() {
                let call = &reconcile_calls[inner - 1];
                for kind in [FaultKind::CrashBefore, FaultKind::CrashPartial, FaultKind::FailOnce] {
                    if !kind.applies(call) {
                        continue;
                    }
                    runs += 1;
                    let ctx = format!(
                        "{name} [{outer:?} at call #{point}, then reconcile {kind:?} at its call #{inner}/{} = {call}]",
                        reconcile_calls.len()
                    );
                    let mut rig = crashed(create, act, outer_plan);
                    let journal = rig.restart();
                    rig.fs.arm(Some(FaultPlan { at: inner, kind }));
                    let _ = journal.reconcile_pending_operations(None);
                    let _ = rig.restart().reconcile_pending_operations(None);
                    let actual = summary(&rig);
                    if actual != expected {
                        violations.push(format!(
                            "{ctx}: interrupted reconcile then healthy reconcile reached a different verdict.\n  expected: {expected}\n  actual:   {actual}"
                        ));
                    }
                    verify(&mut rig, &ctx, &mut violations, None, false);
                }
            }
        }
    }
    assert!(
        runs > 0,
        "{name}: the reconcile made no mutating call at any crash point; nothing was interrupted"
    );
    assert_no_violations(name, &violations, runs, "interrupted reconciles");
}

// ---------------------------------------------------------------------------------------------------------------
// FaultInjectionSecondOrder.cs
// ---------------------------------------------------------------------------------------------------------------

/// The process dies at the numbered call.
const CRASH_KINDS: [FaultKind; 4] = [
    FaultKind::CrashBefore,
    FaultKind::CrashAfter,
    FaultKind::CrashPartial,
    FaultKind::CrashMoveCopiedNotDeleted,
];

/// What can happen to the follow-up step: it dies (cleanly, torn, mid cross-volume move) or fails once.
const FOLLOW_UP_KINDS: [FaultKind; 6] = [
    FaultKind::CrashBefore,
    FaultKind::CrashPartial,
    FaultKind::CrashMoveCopiedNotDeleted,
    FaultKind::FailOnce,
    FaultKind::FailPartial,
    FaultKind::FailUnauthorized,
];

/// A healthy start after the crash: new objects, startup reconcile (the Recovery window then lists what is Failed).
fn started_again(mut rig: Rig) -> Rig {
    let _ = rig.restart().reconcile_pending_operations(None);
    rig.outcomes.clear();
    rig.fs.arm(None);
    rig
}

/// "Retry" on every Recovery item, like the user pressing the button on each row.
fn retry_all(rig: &mut Rig) -> Result<(), String> {
    let retry = rig.retry_service();
    for failed in rig.failed_entries() {
        retry
            .retry_move_or_copy(&failed, false, &CancelToken::none())
            .map_err(|_| "Canceled escaped the retry".to_owned())?;
    }
    Ok(())
}

/// C# `AssertRetryInterruptedAsync`: for every crash point of `act`, the Recovery retry the user starts after the
/// next start is itself interrupted at each of its own mutating calls; the start after THAT must be consistent.
fn assert_retry_interrupted(name: &str, create: &Create, act: &Act) {
    let mut violations = Vec::new();
    let mut probe = create();
    probe.fs.arm(None);
    let _ = act(&mut probe);
    let outer_calls = probe.fs.kinds();
    let mut runs = 0;
    let mut crash_states_with_retry_work = 0;
    for point in 1..=outer_calls.len() {
        for outer in CRASH_KINDS {
            if !outer.applies(&outer_calls[point - 1]) {
                continue;
            }
            let outer_plan = FaultPlan { at: point, kind: outer };
            let mut clean = started_again(crashed(create, act, outer_plan));
            let _ = retry_all(&mut clean);
            let retry_calls = clean.fs.kinds();
            if retry_calls.is_empty() {
                continue; // the reconcile alone settled it: no Recovery item to retry
            }
            crash_states_with_retry_work += 1;
            for inner in 1..=retry_calls.len() {
                let call = &retry_calls[inner - 1];
                for kind in FOLLOW_UP_KINDS {
                    if !kind.applies(call) {
                        continue;
                    }
                    runs += 1;
                    let ctx = format!(
                        "{name} [{outer:?} at call #{point}/{} = {}; then the retry {kind:?} at its call #{inner}/{} = {call}]",
                        outer_calls.len(),
                        outer_calls[point - 1],
                        retry_calls.len()
                    );
                    let mut rig = started_again(crashed(create, act, outer_plan));
                    rig.fs.arm(Some(FaultPlan { at: inner, kind }));
                    if let Err(escaped) = retry_all(&mut rig)
                        && !rig.fs.dead()
                    {
                        violations.push(format!("{ctx}: an I/O failure ESCAPED the retry: {escaped}"));
                        continue;
                    }
                    verify(&mut rig, &ctx, &mut violations, None, false);
                }
            }
        }
    }
    assert!(
        crash_states_with_retry_work > 0,
        "{name}: no crash point left a Recovery item to retry; the matrix proved nothing"
    );
    assert!(runs > 0, "{name}: no retry run");
    assert_no_violations(name, &violations, runs, "interrupted retries");
}

/// C# `AssertDoubleFaultAsync`: crash of `first` at every point, a healthy restart, then the user acts again (`second`)
/// and THAT is faulted at every call by every kind.
fn assert_double_fault(name: &str, create: &Create, first: &Act, second: &Act) {
    let mut violations = Vec::new();
    let mut probe = create();
    probe.fs.arm(None);
    let _ = first(&mut probe);
    let first_calls = probe.fs.kinds();
    let mut runs = 0;
    for point in 1..=first_calls.len() {
        for outer in CRASH_KINDS {
            if !outer.applies(&first_calls[point - 1]) {
                continue;
            }
            let outer_plan = FaultPlan { at: point, kind: outer };
            let mut clean = started_again(crashed(create, first, outer_plan));
            let _ = second(&mut clean);
            let second_calls = clean.fs.kinds();
            for inner in 1..=second_calls.len() {
                let call = &second_calls[inner - 1];
                for kind in DEFAULT_KINDS {
                    if !kind.applies(call) {
                        continue;
                    }
                    runs += 1;
                    let ctx = format!(
                        "{name} [{outer:?} at call #{point}/{}; restart; again {kind:?} at call #{inner}/{} = {call}]",
                        first_calls.len(),
                        second_calls.len()
                    );
                    let mut rig = started_again(crashed(create, first, outer_plan));
                    rig.fs.arm(Some(FaultPlan { at: inner, kind }));
                    let alive = is_alive_kind(kind);
                    if let Err(escaped) = second(&mut rig)
                        && (alive || !rig.fs.dead())
                    {
                        violations.push(format!("{ctx}: an I/O failure ESCAPED the service: {escaped}"));
                        continue;
                    }
                    verify(&mut rig, &ctx, &mut violations, None, true);
                }
            }
        }
    }
    assert!(
        runs > 0,
        "{name}: the second action made no mutating call at any crash point"
    );
    assert_no_violations(name, &violations, runs, "double-fault runs");
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionFaultInjectionTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionFaultInjectionTests.Move_Matrix ("Move: every fault point keeps the photo and tells the truth")
#[test]
fn move_matrix() {
    matrix("Move", &single_rig, &move_a);
}

// C#: FileActionFaultInjectionTests.Copy_Matrix
#[test]
fn copy_matrix() {
    matrix("Copy", &single_rig, &copy_a);
}

// C#: FileActionFaultInjectionTests.Recycle_Matrix
#[test]
fn recycle_matrix() {
    matrix("Recycle", &single_rig, &recycle_a);
}

// C#: FileActionFaultInjectionTests.TwoMoves_Matrix ("a fault in the second never harms the first")
#[test]
fn two_moves_matrix() {
    matrix("Move+Move", &single_rig, &|rig| {
        move_a(rig)?;
        do_single(rig, B, Move, Some("sel"), PLAIN)
    });
}

// C#: FileActionFaultInjectionTests.CopyThenDelete_Matrix
#[test]
fn copy_then_delete_matrix() {
    matrix("Copy+Delete", &single_rig, &|rig| {
        copy_a(rig)?;
        recycle_a(rig)
    });
}

// C#: FileActionFaultInjectionTests.GroupMove_Matrix
#[test]
fn group_move_matrix() {
    matrix("GroupMove", &group_rig, &group_move);
}

// C#: FileActionFaultInjectionTests.GroupCopy_Matrix
#[test]
fn group_copy_matrix() {
    matrix("GroupCopy", &group_rig, &group_copy);
}

// C#: FileActionFaultInjectionTests.GroupRecycle_Matrix
#[test]
fn group_recycle_matrix() {
    matrix("GroupRecycle", &group_rig, &group_recycle);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionFaultInjectionRecoveryTests.cs
// ---------------------------------------------------------------------------------------------------------------

/// C# `CheckUndoRetryable` (INV-6): after a transient failure of an undo the in-memory stack is truthful and the undo
/// retryable: a single Move stays on the stack exactly while its file is still at the destination, and a second Ctrl+Z
/// (the fault is over) completes the undo.
fn check_undo_retryable(rig: &mut Rig, ctx: &str, v: &mut Vec<String>) {
    if rig.fs.dead() {
        return; // a dead process has no stack to keep consistent
    }
    let first = rig.outcomes.iter().rev().find_map(|o| match o {
        Outcome::Undo(undo) => Some(undo.clone()),
        _ => None,
    });
    let Some(first) = first.filter(|undo| !undo.succeeded) else {
        return;
    };
    // A cross-volume Move cut short can leave a truncated file at its destination; it is never deleted (D-06) and keeps
    // that path occupied, so "retryable" does not apply until the user clears it.
    if rig
        .snapshot(&[])
        .iter()
        .any(|(path, text)| rig.content_of(path).map(str::as_bytes) != Some(text.as_slice()))
    {
        return;
    }
    if let Some((source, destination)) = rig.undo.move_history().last().cloned() {
        if rig.exists(&source) || !rig.has_full(&destination) {
            v.push(format!(
                "{ctx}: the undo stack still offers {destination} -> {source} but the disk no longer matches (file at source={})",
                rig.exists(&source)
            ));
        }
    } else if !rig.undo.has_last_undo_action() && first.operation == Some(Move) && rig.exists(A_SEL) && !rig.exists(A) {
        v.push(format!(
            "{ctx}: the failed undo DROPPED the stack entry although the photo is still in the destination folder"
        ));
    }

    let second = rig.undo.undo_last();
    if !second.succeeded && rig.undo.has_last_undo_action() {
        v.push(format!(
            "{ctx}: a second Ctrl+Z after a transient failure did not complete the undo: {:?}",
            second.error_message
        ));
    }
}

// C#: FileActionFaultInjectionRecoveryTests.UndoMove_Matrix
#[test]
fn undo_move_matrix() {
    assert_all_fault_points(
        "UndoMove",
        &|| after_action(false, Move),
        &undo_last,
        Some(&check_undo_retryable),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionRecoveryTests.UndoGroupMove_Matrix
#[test]
fn undo_group_move_matrix() {
    assert_all_fault_points(
        "UndoGroupMove",
        &|| after_action(true, Move),
        &undo_last,
        Some(&check_undo_retryable),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionRecoveryTests.UndoRecycle_Matrix
#[test]
fn undo_recycle_matrix() {
    assert_all_fault_points(
        "UndoRecycle",
        &|| after_action(false, Recycle),
        &undo_last,
        Some(&check_undo_retryable),
        &DEFAULT_KINDS,
        1,
    );
}

// C#: FileActionFaultInjectionRecoveryTests.UndoGroupRecycle_Matrix
#[test]
fn undo_group_recycle_matrix() {
    assert_all_fault_points(
        "UndoGroupRecycle",
        &|| after_action(true, Recycle),
        &undo_last,
        Some(&check_undo_retryable),
        &DEFAULT_KINDS,
        2,
    );
}

fn retry(rig: &mut Rig) -> Result<(), String> {
    retry_first(rig, false)
}

// C#: FileActionFaultInjectionRecoveryTests.RetryMove_Matrix
#[test]
fn retry_move_matrix() {
    matrix("RetryMove", &|| with_failed_single(Move), &retry);
}

// C#: FileActionFaultInjectionRecoveryTests.RetryCopy_Matrix
#[test]
fn retry_copy_matrix() {
    matrix("RetryCopy", &|| with_failed_single(Copy), &retry);
}

// C#: FileActionFaultInjectionRecoveryTests.RetryGroupMove_Matrix
#[test]
fn retry_group_move_matrix() {
    matrix("RetryGroupMove", &|| with_half_done_group(Move), &retry);
}

// C#: FileActionFaultInjectionRecoveryTests.RetryGroupRecycle_Matrix
#[test]
fn retry_group_recycle_matrix() {
    matrix("RetryGroupRecycle", &|| with_half_done_group(Recycle), &retry);
}

// C#: FileActionFaultInjectionRecoveryTests.ReconcileInterrupted_Move
#[test]
fn reconcile_interrupted_move() {
    assert_reconcile_interrupted("ReconcileMove", &single_rig, &move_a);
}

// C#: FileActionFaultInjectionRecoveryTests.ReconcileInterrupted_Copy
#[test]
fn reconcile_interrupted_copy() {
    assert_reconcile_interrupted("ReconcileCopy", &single_rig, &copy_a);
}

// C#: FileActionFaultInjectionRecoveryTests.ReconcileInterrupted_GroupMove
#[test]
fn reconcile_interrupted_group_move() {
    assert_reconcile_interrupted("ReconcileGroupMove", &group_rig, &group_move);
}

// C#: FileActionFaultInjectionRecoveryTests.ReconcileInterrupted_GroupRecycle
#[test]
fn reconcile_interrupted_group_recycle() {
    assert_reconcile_interrupted("ReconcileGroupRecycle", &group_rig, &group_recycle);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionFaultInjectionGapTests.cs
// ---------------------------------------------------------------------------------------------------------------

// ---- 1. The Recovery retry is itself interrupted after a REAL crash ----

// C#: FileActionFaultInjectionGapTests.RetryInterrupted_Move
#[test]
fn retry_interrupted_move() {
    assert_retry_interrupted("Move>retry", &single_rig, &move_a);
}

// C#: FileActionFaultInjectionGapTests.RetryInterrupted_Copy
#[test]
fn retry_interrupted_copy() {
    assert_retry_interrupted("Copy>retry", &single_rig, &copy_a);
}

// C#: FileActionFaultInjectionGapTests.RetryInterrupted_GroupMove
#[test]
fn retry_interrupted_group_move() {
    assert_retry_interrupted("GroupMove>retry", &group_rig, &group_move);
}

// C#: FileActionFaultInjectionGapTests.RetryInterrupted_GroupCopy
#[test]
fn retry_interrupted_group_copy() {
    assert_retry_interrupted("GroupCopy>retry", &group_rig, &group_copy);
}

// C#: FileActionFaultInjectionGapTests.RetryInterrupted_GroupRecycle
#[test]
fn retry_interrupted_group_recycle() {
    assert_retry_interrupted("GroupRecycle>retry", &group_rig, &group_recycle);
}

// C#: FileActionFaultInjectionGapTests.RetryInterrupted_UndoMove
#[test]
fn retry_interrupted_undo_move() {
    assert_retry_interrupted("UndoMove>retry", &|| after_action(false, Move), &undo_last);
}

// C#: FileActionFaultInjectionGapTests.RetryInterrupted_UndoGroupMove
#[test]
fn retry_interrupted_undo_group_move() {
    assert_retry_interrupted("UndoGroupMove>retry", &|| after_action(true, Move), &undo_last);
}

// ---- 1b. The startup reconcile is interrupted ----

// C#: FileActionFaultInjectionGapTests.ReconcileInterrupted_GroupCopy
#[test]
fn reconcile_interrupted_group_copy() {
    assert_reconcile_interrupted("ReconcileGroupCopy", &group_rig, &group_copy);
}

// C#: FileActionFaultInjectionGapTests.ReconcileInterrupted_Recycle
#[test]
fn reconcile_interrupted_recycle() {
    assert_reconcile_interrupted("ReconcileRecycle", &single_rig, &recycle_a);
}

// C#: FileActionFaultInjectionGapTests.ReconcileInterrupted_PermanentDelete
#[test]
fn reconcile_interrupted_permanent_delete() {
    assert_reconcile_interrupted("ReconcilePermanent", &no_bin_group, &|rig| permanent_group(rig, false));
}

// C#: FileActionFaultInjectionGapTests.ReconcileInterrupted_UndoMove
#[test]
fn reconcile_interrupted_undo_move() {
    assert_reconcile_interrupted("ReconcileUndoMove", &|| after_action(false, Move), &undo_last);
}

// C#: FileActionFaultInjectionGapTests.ReconcileInterrupted_UndoGroupMove
#[test]
fn reconcile_interrupted_undo_group_move() {
    assert_reconcile_interrupted("ReconcileUndoGroupMove", &|| after_action(true, Move), &undo_last);
}

// C#: FileActionFaultInjectionGapTests.ReconcileInterrupted_UndoGroupRecycle
#[test]
fn reconcile_interrupted_undo_group_recycle() {
    assert_reconcile_interrupted("ReconcileUndoGroupRecycle", &|| after_action(true, Recycle), &undo_last);
}

// ---- 2. Two faulted starts in a row ----

// C#: FileActionFaultInjectionGapTests.DoubleFault_SameMoveAgain
#[test]
fn double_fault_same_move_again() {
    assert_double_fault("Move>Move again", &single_rig, &move_a, &move_a);
}

// C#: FileActionFaultInjectionGapTests.DoubleFault_OtherMove
#[test]
fn double_fault_other_move() {
    assert_double_fault("Move>Move b", &single_rig, &move_a, &|rig| {
        do_single(rig, B, Move, Some("sel"), PLAIN)
    });
}

// C#: FileActionFaultInjectionGapTests.DoubleFault_SameCopyAgain
#[test]
fn double_fault_same_copy_again() {
    assert_double_fault("Copy>Copy again", &single_rig, &copy_a, &copy_a);
}

// C#: FileActionFaultInjectionGapTests.DoubleFault_SameRecycleAgain
#[test]
fn double_fault_same_recycle_again() {
    assert_double_fault("Recycle>Recycle again", &single_rig, &recycle_a, &recycle_a);
}

// C#: FileActionFaultInjectionGapTests.DoubleFault_SameGroupMoveAgain
#[test]
fn double_fault_same_group_move_again() {
    assert_double_fault("GroupMove>GroupMove again", &group_rig, &group_move, &group_move);
}

// C#: FileActionFaultInjectionGapTests.DoubleFault_GroupMoveThenGroupRecycle
#[test]
fn double_fault_group_move_then_group_recycle() {
    assert_double_fault("GroupMove>GroupRecycle", &group_rig, &group_move, &group_recycle);
}

// ---- 3. Cancellation as a fault (the process lives; the token is cancelled right after each numbered call) ----

fn cancel_matrix(name: &str, create: &Create, act: &Act, extra: Option<&Extra>) {
    assert_all_fault_points(name, create, act, extra, &CANCEL_ONLY, 2);
}

// C#: FileActionFaultInjectionGapTests.Cancel_Move
#[test]
fn cancel_move() {
    cancel_matrix(
        "Cancel Move",
        &single_rig,
        &|rig| do_single(rig, A, Move, Some("sel"), CANCELLABLE),
        None,
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_Copy
#[test]
fn cancel_copy() {
    cancel_matrix(
        "Cancel Copy",
        &single_rig,
        &|rig| do_single(rig, A, Copy, Some("sel"), CANCELLABLE),
        None,
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_Recycle
#[test]
fn cancel_recycle() {
    cancel_matrix(
        "Cancel Recycle",
        &single_rig,
        &|rig| do_single(rig, A, Recycle, None, CANCELLABLE),
        None,
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_GroupMove
#[test]
fn cancel_group_move() {
    cancel_matrix(
        "Cancel GroupMove",
        &group_rig,
        &|rig| do_group(rig, Move, Some("sel"), CANCELLABLE),
        None,
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_GroupCopy
#[test]
fn cancel_group_copy() {
    cancel_matrix(
        "Cancel GroupCopy",
        &group_rig,
        &|rig| do_group(rig, Copy, Some("sel"), CANCELLABLE),
        None,
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_GroupRecycle
#[test]
fn cancel_group_recycle() {
    cancel_matrix(
        "Cancel GroupRecycle",
        &group_rig,
        &|rig| do_group(rig, Recycle, None, CANCELLABLE),
        None,
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_RetryMove
#[test]
fn cancel_retry_move() {
    cancel_matrix(
        "Cancel RetryMove",
        &|| with_failed_single(Move),
        &|rig| retry_first(rig, true),
        None,
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_RetryGroupMove
#[test]
fn cancel_retry_group_move() {
    cancel_matrix(
        "Cancel RetryGroupMove",
        &|| with_half_done_group(Move),
        &|rig| retry_first(rig, true),
        None,
    );
}

/// Index (1-based) of the first call of `kind` in a healthy run of `act`.
fn first_call_of(create: &Create, act: &Act, kind: &str) -> usize {
    let mut probe = create();
    probe.fs.arm(None);
    let _ = act(&mut probe);
    1 + probe
        .fs
        .kinds()
        .iter()
        .position(|k| k == kind)
        .unwrap_or_else(|| panic!("the scenario makes no {kind} call"))
}

// C#: FileActionFaultInjectionGapTests.CancelAfterPrepared_GroupMove_IsObservedAndRolledBack
#[test]
fn cancel_after_prepared_group_move_is_observed_and_rolled_back() {
    let at = first_call_of(&group_rig, &group_move, "append");
    let mut rig = group_rig();
    rig.fs.arm(Some(FaultPlan {
        at,
        kind: FaultKind::CancelAfter,
    }));
    do_group(&mut rig, Move, Some("sel"), CANCELLABLE).expect("no escape");
    assert!(rig.token().is_canceled());
    let Some(Outcome::Group(result)) = rig.outcomes.last() else {
        panic!("group outcome")
    };
    assert!(!result.succeeded);
    assert!(!rig.fs.kinds().iter().any(|k| k == "move"));
    for (name, _) in &rig.contents {
        assert!(rig.has_full(&join(PHOTOS, name)), "{name} stays complete in place");
    }
    assert!(
        rig.snapshot(&[])
            .keys()
            .all(|k| !k.starts_with(&ignore_case_key(r"C:\photos\sel\")))
    );
    assert!(
        !rig.latest()
            .iter()
            .any(|e| matches!(e.state, JournalState::Prepared | JournalState::Committed))
    );
}

// C#: FileActionFaultInjectionGapTests.CancelAfterPrepared_SingleMove_IsObserved
#[test]
fn cancel_after_prepared_single_move_is_observed() {
    let at = first_call_of(&single_rig, &move_a, "append");
    let mut rig = single_rig();
    rig.fs.arm(Some(FaultPlan {
        at,
        kind: FaultKind::CancelAfter,
    }));
    do_single(&mut rig, A, Move, Some("sel"), CANCELLABLE).expect("no escape");
    let Some(Outcome::Action(result)) = rig.outcomes.last() else {
        panic!("action outcome")
    };
    assert!(!result.succeeded);
    assert!(rig.has_full(A));
    assert!(!rig.exists(A_SEL));
    assert!(!rig.latest().iter().any(|e| e.state == JournalState::Prepared));
}

// C#: FileActionFaultInjectionGapTests.Unauthorized_AtTheFileOperation_IsACleanFailure (Theory: Move, Copy)
#[test]
fn unauthorized_at_the_file_operation_is_a_clean_failure() {
    for (operation, kind) in [(Move, "move"), (Copy, "copy")] {
        let act = move |rig: &mut Rig| do_single(rig, A, operation, Some("sel"), PLAIN);
        let at = first_call_of(&single_rig, &act, kind);
        let mut rig = single_rig();
        rig.fs.arm(Some(FaultPlan {
            at,
            kind: FaultKind::FailUnauthorized,
        }));
        act(&mut rig).expect("no escape");
        let Some(Outcome::Action(result)) = rig.outcomes.last() else {
            panic!("action outcome")
        };
        assert!(!result.succeeded, "{operation:?}");
        assert_eq!(rig.fs.kinds()[at - 1], kind, "the fault hit the file operation itself");
        assert!(rig.has_full(A));
        assert!(!rig.exists(A_SEL));
        let latest = rig.latest();
        assert_eq!(latest.len(), 1, "{operation:?}: one journaled operation");
        assert_eq!(latest[0].state, JournalState::Failed);
    }
}

/// C# `PermanentStaysInScope`: only the capture's own files may be deleted, never via the bin, and the bystander photo
/// stays complete.
fn permanent_stays_in_scope(rig: &mut Rig, ctx: &str, v: &mut Vec<String>) {
    if !rig.bin.items().is_empty() {
        v.push(format!("{ctx}: a permanent delete put something in the Recycle Bin"));
    }
    if !rig.has_full(B) {
        v.push(format!(
            "{ctx}: the bystander photo b.jpg was harmed by the permanent delete of a.*"
        ));
    }
    for gone in rig.bin.permanently_deleted() {
        if !ignore_case_key(file_name(&gone)).starts_with("A.") {
            v.push(format!(
                "{ctx}: {gone} was deleted permanently but is not part of the action"
            ));
        }
    }
}

// C#: FileActionFaultInjectionGapTests.Cancel_PermanentDelete
#[test]
fn cancel_permanent_delete() {
    let opts = Opts {
        cancellable: true,
        permanent: true,
    };
    cancel_matrix(
        "Cancel PermanentDelete",
        &no_bin_single,
        &move |rig| do_single(rig, A, Recycle, None, opts),
        Some(&permanent_stays_in_scope),
    );
}

// C#: FileActionFaultInjectionGapTests.Cancel_PermanentDeleteGroup
#[test]
fn cancel_permanent_delete_group() {
    cancel_matrix(
        "Cancel PermanentDeleteGroup",
        &no_bin_group,
        &|rig| permanent_group(rig, true),
        Some(&permanent_stays_in_scope),
    );
}

// ---- 4. Dismiss of a Recovery item (an append of its own) ----

/// C# `DismissIsTruthful`: an acknowledged dismiss is on the disk as the item's LATEST verdict; an unacknowledged one
/// leaves the item Failed.
fn dismiss_is_truthful(rig: &mut Rig, ctx: &str, v: &mut Vec<String>) {
    let acknowledged: Vec<JournalEntry> = rig
        .outcomes
        .iter()
        .filter_map(|o| match o {
            Outcome::Dismiss(outcome) => Some(outcome.dismissed.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let latest = rig.latest();
    for entry in &acknowledged {
        let state = latest.iter().find(|e| e.id == entry.id).map(|e| e.state);
        if state != Some(JournalState::Dismissed) {
            v.push(format!(
                "{ctx}: Dismiss reported {} dismissed but the journal's latest verdict is {state:?}",
                entry.source
            ));
        }
    }
    if !rig.fs.dead() && acknowledged.is_empty() && !latest.iter().any(|e| e.state == JournalState::Failed) {
        v.push(format!(
            "{ctx}: Dismiss was not acknowledged yet the Failed item vanished from the journal"
        ));
    }
}

// C#: FileActionFaultInjectionGapTests.Dismiss_FailedMove
#[test]
fn dismiss_failed_move() {
    assert_all_fault_points(
        "Dismiss failed Move",
        &|| with_failed_single(Move),
        &dismiss_failed,
        Some(&dismiss_is_truthful),
        &DEFAULT_KINDS,
        1,
    );
}

// C#: FileActionFaultInjectionGapTests.Dismiss_HalfMovedGroup
#[test]
fn dismiss_half_moved_group() {
    assert_all_fault_points(
        "Dismiss half group",
        &|| with_half_done_group(Move),
        &dismiss_failed,
        Some(&dismiss_is_truthful),
        &DEFAULT_KINDS,
        1,
    );
}

// ---- 5. Permanent delete on a volume without a Recycle Bin (the one intended loss) ----

// C#: FileActionFaultInjectionGapTests.PermanentDelete_Single
#[test]
fn permanent_delete_single() {
    let opts = Opts {
        cancellable: false,
        permanent: true,
    };
    assert_all_fault_points(
        "PermanentDelete",
        &no_bin_single,
        &move |rig| do_single(rig, A, Recycle, None, opts),
        Some(&permanent_stays_in_scope),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.PermanentDelete_Group
#[test]
fn permanent_delete_group() {
    assert_all_fault_points(
        "PermanentDeleteGroup",
        &no_bin_group,
        &|rig| permanent_group(rig, false),
        Some(&permanent_stays_in_scope),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.PermanentDelete_NotOptedIn_IsRefusedWithoutAnyMutation
#[test]
fn permanent_delete_not_opted_in_is_refused_without_any_mutation() {
    let mut single = no_bin_single();
    single.fs.arm(None);
    recycle_a(&mut single).expect("no escape");
    let Some(Outcome::Action(one)) = single.outcomes.last() else {
        panic!("action outcome")
    };
    assert!(!one.succeeded);
    assert!(single.has_full(A));
    assert_eq!(single.fs.mutations(), 0);
    assert!(single.bin.permanently_deleted().is_empty());

    let mut group = no_bin_group();
    group.fs.arm(None);
    group_recycle(&mut group).expect("no escape");
    let Some(Outcome::Group(many)) = group.outcomes.last() else {
        panic!("group outcome")
    };
    assert!(!many.succeeded);
    for (name, _) in &group.contents {
        assert!(group.has_full(&join(PHOTOS, name)));
    }
    assert_eq!(group.fs.mutations(), 0);
    assert!(group.bin.permanently_deleted().is_empty());
    assert!(group.journal_lines().is_empty());
}

// ---- 6. The destination folder an action creates ----

fn with_existing_empty_destination() -> Rig {
    let rig = group_rig();
    rig.disk.add_dir(SEL);
    rig
}

/// A folder that existed before the action (the user's own, even empty) is never removed by any failure or rollback.
fn pre_existing_folder_survives(rig: &mut Rig, ctx: &str, v: &mut Vec<String>) {
    if !rig.disk.directory_exists(Path::new(SEL)) {
        v.push(format!(
            "{ctx}: the destination folder {SEL} existed before the action and was removed by it"
        ));
    }
}

/// A failure that happens BEFORE the Prepared line exists (the process lives) leaves no folder this call created.
fn created_folder_removed_before_prepared(rig: &mut Rig, ctx: &str, v: &mut Vec<String>) {
    if rig.fs.dead() || !rig.journal_lines().is_empty() {
        return;
    }
    let failed = rig.outcomes.iter().any(|o| match o {
        Outcome::Action(r) => !r.succeeded,
        Outcome::Group(r) => !r.succeeded,
        _ => false,
    });
    if !failed {
        return;
    }
    for folder in [SEL_SUB, SEL] {
        if rig.disk.directory_exists(Path::new(folder)) {
            v.push(format!(
                "{ctx}: the action failed before its journal line and left the folder it created: {folder}"
            ));
        }
    }
}

// C#: FileActionFaultInjectionGapTests.Folder_ExistingSurvives_Move
#[test]
fn folder_existing_survives_move() {
    assert_all_fault_points(
        "Folder kept (Move)",
        &with_existing_empty_destination,
        &move_a,
        Some(&pre_existing_folder_survives),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.Folder_ExistingSurvives_GroupMove
#[test]
fn folder_existing_survives_group_move() {
    assert_all_fault_points(
        "Folder kept (GroupMove)",
        &with_existing_empty_destination,
        &group_move,
        Some(&pre_existing_folder_survives),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.Folder_ExistingSurvives_GroupCopy
#[test]
fn folder_existing_survives_group_copy() {
    assert_all_fault_points(
        "Folder kept (GroupCopy)",
        &with_existing_empty_destination,
        &group_copy,
        Some(&pre_existing_folder_survives),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.Folder_CreatedIsRemoved_Move
#[test]
fn folder_created_is_removed_move() {
    assert_all_fault_points(
        "Folder removed (Move)",
        &single_rig,
        &|rig| do_single(rig, A, Move, Some(r"sel\sub"), PLAIN),
        Some(&created_folder_removed_before_prepared),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.Folder_CreatedIsRemoved_Copy
#[test]
fn folder_created_is_removed_copy() {
    assert_all_fault_points(
        "Folder removed (Copy)",
        &single_rig,
        &|rig| do_single(rig, A, Copy, Some(r"sel\sub"), PLAIN),
        Some(&created_folder_removed_before_prepared),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.Folder_CreatedIsRemoved_GroupMove
#[test]
fn folder_created_is_removed_group_move() {
    assert_all_fault_points(
        "Folder removed (GroupMove)",
        &group_rig,
        &|rig| do_group(rig, Move, Some(r"sel\sub"), PLAIN),
        Some(&created_folder_removed_before_prepared),
        &DEFAULT_KINDS,
        2,
    );
}

// C#: FileActionFaultInjectionGapTests.Folder_CreatedIsRemoved_GroupCopy
#[test]
fn folder_created_is_removed_group_copy() {
    assert_all_fault_points(
        "Folder removed (GroupCopy)",
        &group_rig,
        &|rig| do_group(rig, Copy, Some(r"sel\sub"), PLAIN),
        Some(&created_folder_removed_before_prepared),
        &DEFAULT_KINDS,
        2,
    );
}

// ---------------------------------------------------------------------------------------------------------------
// CrashMatrixTests.cs (+ Fakes/CrashPointFileSystem.cs)
// ---------------------------------------------------------------------------------------------------------------

const CM_SOURCE: &str = r"C:\photos\a.jpg";
const CM_DESTINATION: &str = r"C:\photos\sel\a.jpg";
const CM_CONTENT: &[u8] = b"0123456789";

fn simulated_crash() -> io::Error {
    io::Error::other("simulated crash")
}

/// Port of `Fakes/CrashPointFileSystem.cs`: numbers every mutating call (`create_directory`, `move_file`, `copy_file`,
/// `delete`, `open_append`, `write_all_text_atomic`). When `crash_before` is N, mutation N (1-based) and everything after
/// it (reads included) fail: the process "died" right before that call. `torn_at` makes the first append whose mutation
/// number is at least that value write only half of its bytes and then fail (disk full); the process lives on. Like
/// `FaultInjectionFileSystem`, the non-failing Rust signatures of a dead process answer "missing" (`false` / `None`).
struct CrashPointFs {
    inner: Arc<FakeFileSystem>,
    mutations: AtomicUsize,
    crash_before: AtomicUsize,
    torn_at: AtomicUsize,
    torn_done: AtomicBool,
}

impl CrashPointFs {
    fn new(inner: Arc<FakeFileSystem>) -> Self {
        Self {
            inner,
            mutations: AtomicUsize::new(0),
            crash_before: AtomicUsize::new(usize::MAX),
            torn_at: AtomicUsize::new(usize::MAX),
            torn_done: AtomicBool::new(false),
        }
    }

    fn mutations(&self) -> usize {
        self.mutations.load(Ordering::SeqCst)
    }

    fn crashed(&self) -> bool {
        self.mutations() >= self.crash_before.load(Ordering::SeqCst)
    }

    fn mutate(&self) -> io::Result<()> {
        let n = self.mutations.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= self.crash_before.load(Ordering::SeqCst) {
            return Err(simulated_crash());
        }
        Ok(())
    }

    fn read(&self) -> io::Result<()> {
        if self.crashed() { Err(simulated_crash()) } else { Ok(()) }
    }
}

/// Writes the first half of the buffer to the inner stream, then fails like a full disk.
struct TornStream {
    inner: Box<dyn AppendStream>,
}

impl Write for TornStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write_all(&buf[..buf.len() / 2])?;
        self.inner.flush()?;
        Err(io::Error::other("There is not enough space on the disk."))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl AppendStream for TornStream {}

impl FileSystem for CrashPointFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.read().is_ok() && self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.read().is_ok() && self.inner.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.try_get_file_stat(path).ok().flatten()
    }

    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        self.read()?;
        self.inner.try_get_file_stat(path)
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.mutate()?;
        self.inner.move_file(source, destination)
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.mutate()?;
        self.inner.copy_file(source, destination)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.mutate()?;
        self.inner.delete(path)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.read()?;
        self.inner.open_read_shared(path, buffer_size)
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.mutate()?;
        let stream = self.inner.open_append(path, durable)?;
        if self.torn_done.load(Ordering::SeqCst) || self.mutations() < self.torn_at.load(Ordering::SeqCst) {
            return Ok(stream);
        }
        self.torn_done.store(true, Ordering::SeqCst);
        Ok(Box::new(TornStream { inner: stream }))
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.mutate()?;
        self.inner.write_all_text_atomic(path, text, durable)
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.read()?;
        self.inner.read_all_text(path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.read()?;
        self.inner.enumerate_files(directory, pattern)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.read()?;
        self.inner.enumerate_directories(directory)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.mutate()?;
        self.inner.create_directory(path)
    }

    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        self.inner.try_delete_empty_directory(path)
    }

    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.read()?;
        self.inner.resolve_real_path(path)
    }
}

/// C# `CrashMatrixTests.DeletingBin`: "send" is a real (crash-numbered) delete on the fake file system; nothing is
/// ever restorable.
struct DeletingBin {
    fs: Arc<CrashPointFs>,
}

impl RecycleBin for DeletingBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        self.fs.delete(path)
    }

    fn try_restore(&self, _original_path: &Path, _expected_size: u64, _expected: DateTimeUtc) -> io::Result<bool> {
        Ok(false)
    }
}

struct CrashRig {
    disk: Arc<FakeFileSystem>,
    fs: Arc<CrashPointFs>,
    clock: Arc<FakeClock>,
    journal: Arc<OperationJournal>,
    service: Arc<FileActionService>,
}

fn cm_clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::at(utc("2026-09-26T01:00:00Z")))
}

fn cm_stamp() -> DateTimeUtc {
    utc("2026-09-01T00:00:00Z")
}

fn crash_rig(source_exists: bool) -> CrashRig {
    let disk = Arc::new(FakeFileSystem::new());
    if source_exists {
        disk.add_file_at(CM_SOURCE, CM_CONTENT, cm_stamp());
    }
    let fs = Arc::new(CrashPointFs::new(Arc::clone(&disk)));
    let clock = cm_clock();
    let journal = Arc::new(new_journal(fs.clone(), clock.clone()));
    let bin = Arc::new(DeletingBin { fs: Arc::clone(&fs) });
    let service = Arc::new(FileActionService::new(
        Arc::clone(&journal),
        fs.clone(),
        clock.clone(),
        bin,
    ));
    CrashRig {
        disk,
        fs,
        clock,
        journal,
        service,
    }
}

fn cm_request(operation: FileOperationType) -> FileActionRequest {
    FileActionRequest {
        destination: (operation != Recycle).then(|| "sel".to_owned()),
        ..FileActionRequest::new(CM_SOURCE, operation)
    }
}

/// Independent reference: latest journal record per Id, parsed straight from the file (torn lines skipped).
fn latest_by_id(disk: &FakeFileSystem) -> HashMap<String, JournalEntry> {
    parse_journal(disk)
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect()
}

fn pending_ids(journal: &OperationJournal) -> Vec<String> {
    let mut ids: Vec<String> = journal
        .read_pending_and_failed_operations()
        .expect("readable journal")
        .into_iter()
        .filter(|e| e.state == JournalState::Prepared)
        .map(|e| e.id)
        .collect();
    ids.sort();
    ids
}

/// The recent committed Moves whose file is still at the destination and not back at the source, newest kept per
/// destination (undo entries excluded): the journal-vs-disk consistency filter of the removed C# `ReadStartupHistory`.
fn consistent_committed_moves(journal: &OperationJournal, disk: &FakeFileSystem) -> Vec<JournalEntry> {
    let mut history: Vec<JournalEntry> = Vec::new();
    for entry in journal.read_committed_moves().expect("readable journal") {
        let Some(destination) = entry.destination.clone().filter(|d| !d.is_empty()) else {
            continue;
        };
        if entry.undo == Some(true) {
            continue;
        }
        if disk.file_exists(Path::new(&destination)) && !disk.file_exists(Path::new(&entry.source)) {
            history.retain(|older| {
                !older
                    .destination
                    .as_deref()
                    .is_some_and(|d| eq_ignore_case(d, &destination))
            });
            history.push(entry);
        }
    }
    history
}

/// C# `AssertRestartInvariants`: a healthy process on the surviving files, startup reconcile, then no photo lost, no
/// operation Prepared, and the recorded verdict agrees with the disk.
fn assert_restart_invariants(disk: &Arc<FakeFileSystem>, operation: FileOperationType, context: &str) {
    let journal = new_journal(disk.clone(), cm_clock());
    let _ = journal.reconcile_pending_operations(None);
    assert!(
        pending_ids(&journal).is_empty(),
        "{context}: an operation stayed Prepared after reconcile"
    );

    let source_present = disk.file_exists(Path::new(CM_SOURCE));
    let destination_present = disk.file_exists(Path::new(CM_DESTINATION));
    let latest = latest_by_id(disk);
    if operation == Recycle {
        for entry in latest.values().filter(|e| e.op_type == Recycle) {
            assert_eq!(
                entry.state == JournalState::Committed,
                !source_present,
                "{context}: recycle state {:?} vs source present={source_present}",
                entry.state
            );
        }
        // A photo that is gone from the folder must have been journaled (Prepared before the delete), never silently.
        assert!(
            source_present
                || latest
                    .values()
                    .any(|e| e.op_type == Recycle && e.state == JournalState::Committed),
            "{context}: recycled without any journal record"
        );
        return;
    }

    assert!(
        source_present || destination_present,
        "{context}: the photo was lost (neither source nor destination exists)"
    );
    if destination_present {
        assert_eq!(
            disk.file_content(CM_DESTINATION).as_deref(),
            Some(CM_CONTENT),
            "{context}"
        );
    }
    if source_present {
        assert_eq!(disk.file_content(CM_SOURCE).as_deref(), Some(CM_CONTENT), "{context}");
    }
    if operation == Copy {
        assert!(source_present, "{context}: a Copy removed its source");
    }

    let done = destination_present && (operation == Copy || !source_present);
    assert!(
        !done
            || latest
                .values()
                .any(|e| e.op_type == operation && e.state == JournalState::Committed),
        "{context}: the file operation happened without a journal record"
    );
    for entry in latest
        .values()
        .filter(|e| e.op_type == operation && e.id != "later" && !eq_ignore_case(&e.source, CM_DESTINATION))
    {
        assert_eq!(
            entry.state == JournalState::Committed,
            done,
            "{context}: journal says {:?} but done={done}",
            entry.state
        );
    }
}

// C#: CrashMatrixTests.ActionCrashedBeforeEachCall_RestartIsConsistent (Theory: Move, Copy, Recycle)
fn action_crashed_before_each_call_restart_is_consistent(operation: FileOperationType) {
    let request = cm_request(operation);
    let probe = crash_rig(true);
    let ok = probe.service.execute(&request, &CancelToken::none());
    assert!(ok.succeeded, "{:?}", ok.error);
    let total = probe.fs.mutations();
    assert!(
        total >= 3,
        "the matrix must cover mkdir, Prepared, the file op and Committed"
    );

    for crash in 1..=total {
        let rig = crash_rig(true);
        rig.fs.crash_before.store(crash, Ordering::SeqCst);
        let _ = rig.service.execute(&request, &CancelToken::none()); // the outcome is irrelevant: the process is dead
        assert_restart_invariants(
            &rig.disk,
            operation,
            &format!("{operation:?} crash before call #{crash}/{total}"),
        );
    }
}

#[test]
fn action_crashed_before_each_call_restart_is_consistent_move() {
    action_crashed_before_each_call_restart_is_consistent(Move);
}

#[test]
fn action_crashed_before_each_call_restart_is_consistent_copy() {
    action_crashed_before_each_call_restart_is_consistent(Copy);
}

#[test]
fn action_crashed_before_each_call_restart_is_consistent_recycle() {
    action_crashed_before_each_call_restart_is_consistent(Recycle);
}

fn later_entry(id: &str) -> JournalEntry {
    let epoch = JournalDateTime::utc(utc("1970-01-01T00:00:00Z"));
    JournalEntry {
        id: id.to_owned(),
        op_type: Copy,
        state: JournalState::Prepared,
        source: r"C:\x.jpg".to_owned(),
        destination: Some(r"C:\y\x.jpg".to_owned()),
        size: 1,
        last_write_utc: epoch,
        timestamp_utc: epoch,
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

// C#: CrashMatrixTests.TornJournalAppend_RestartIsConsistent (Theory: Move, Copy, Recycle)
fn torn_journal_append_restart_is_consistent(operation: FileOperationType) {
    let request = cm_request(operation);
    let probe = crash_rig(true);
    let _ = probe.service.execute(&request, &CancelToken::none());
    let append_positions = probe.fs.mutations();

    for torn in 1..=append_positions {
        let rig = crash_rig(true);
        rig.fs.torn_at.store(torn, Ordering::SeqCst);
        let _ = rig.service.execute(&request, &CancelToken::none());
        // The same process keeps running after a failed append (disk full, then space freed): its next append must
        // start on a fresh line, otherwise two records are lost.
        rig.journal
            .append(&later_entry("later"))
            .expect("the next append works");
        assert!(
            latest_by_id(&rig.disk).contains_key("later"),
            "{operation:?} torn append #{torn}: the next append was glued to the torn line"
        );
        assert_restart_invariants(&rig.disk, operation, &format!("{operation:?} torn append #{torn}"));
    }
}

#[test]
fn torn_journal_append_restart_is_consistent_move() {
    torn_journal_append_restart_is_consistent(Move);
}

#[test]
fn torn_journal_append_restart_is_consistent_copy() {
    torn_journal_append_restart_is_consistent(Copy);
}

#[test]
fn torn_journal_append_restart_is_consistent_recycle() {
    torn_journal_append_restart_is_consistent(Recycle);
}

// C#: CrashMatrixTests.TornAppendAfterSuccessfulAppends_NextAppendStartsOnNewLine
#[test]
fn torn_append_after_successful_appends_next_append_starts_on_new_line() {
    let rig = crash_rig(false);
    rig.journal.append(&later_entry("first")).expect("healthy append");
    rig.fs.torn_at.store(rig.fs.mutations() + 1, Ordering::SeqCst); // the first append from now on (mkdir counts too)
    assert!(rig.journal.append(&later_entry("torn")).is_err());
    rig.journal.append(&later_entry("third")).expect("healthy append");
    assert_eq!(pending_ids(&rig.journal), ["first", "third"]);
}

// C#: CrashMatrixTests.UndoCrashedBeforeEachCall_RestartIsConsistent
#[test]
fn undo_crashed_before_each_call_restart_is_consistent() {
    let mut undo_mutations: Option<usize> = None;
    let mut crash = 0;
    loop {
        let rig = crash_rig(true);
        let moved = rig.service.execute(&cm_request(Move), &CancelToken::none());
        assert!(moved.succeeded, "{:?}", moved.error);
        let bin = Arc::new(DeletingBin {
            fs: Arc::clone(&rig.fs),
        });
        let mut undo = UndoService::new(
            Arc::clone(&rig.journal),
            rig.fs.clone(),
            bin,
            Arc::new(EnglishUndoTexts),
        )
        .with_gate(rig.service.clone())
        .with_clock(rig.clock.clone());
        undo.register(&moved);

        let before = rig.fs.mutations();
        let Some(total) = undo_mutations else {
            let probe = undo.undo_move();
            assert!(probe.succeeded, "{:?}", probe.error_message);
            let total = rig.fs.mutations() - before;
            assert!(total >= 3, "undo must journal Prepared, move and Committed");
            undo_mutations = Some(total);
            continue;
        };
        crash += 1;
        if crash > total {
            break;
        }

        rig.fs.crash_before.store(before + crash, Ordering::SeqCst);
        let _ = undo.undo_move();

        let journal = new_journal(rig.disk.clone(), cm_clock());
        let _ = journal.reconcile_pending_operations(None);
        assert!(pending_ids(&journal).is_empty(), "undo crash #{crash}: Prepared left");
        let at_source = rig.disk.file_exists(Path::new(CM_SOURCE));
        let at_destination = rig.disk.file_exists(Path::new(CM_DESTINATION));
        assert!(
            at_source ^ at_destination,
            "undo crash #{crash}: photo must be in exactly one place"
        );
        let history = consistent_committed_moves(&journal, &rig.disk);
        assert_eq!(history.len(), usize::from(at_destination), "undo crash #{crash}");
        assert!(!history.iter().any(|h| h.undo == Some(true)), "undo crash #{crash}");
    }
}

// C#: CrashMatrixTests.RetryCrashedBeforeEachCall_RestartIsConsistent
#[test]
fn retry_crashed_before_each_call_restart_is_consistent() {
    let mut total_mutations: Option<usize> = None;
    let mut crash = 0;
    loop {
        let rig = crash_rig(true);
        let clock = rig.clock.clone();
        let failed = JournalEntry {
            id: "retry1".to_owned(),
            op_type: Move,
            state: JournalState::Failed,
            source: CM_SOURCE.to_owned(),
            destination: Some(CM_DESTINATION.to_owned()),
            size: CM_CONTENT.len() as i64,
            last_write_utc: JournalDateTime::utc(cm_stamp()),
            timestamp_utc: JournalDateTime::utc(clock.utc_now()),
            error: Some("boom".to_owned()),
            ..later_entry("retry1")
        };
        rig.journal.append(&failed).expect("healthy append");
        let retry = RecoveryRetryService::new(
            Arc::clone(&rig.journal),
            rig.fs.clone(),
            clock,
            Arc::new(EnglishRecoveryRetryTexts),
        );

        let before = rig.fs.mutations();
        let Some(total) = total_mutations else {
            let probe = retry
                .retry_move_or_copy(&failed, false, &CancelToken::none())
                .expect("not canceled");
            assert!(probe.succeeded, "{}", probe.message);
            total_mutations = Some(rig.fs.mutations() - before);
            continue;
        };
        crash += 1;
        if crash > total {
            break;
        }

        rig.fs.crash_before.store(before + crash, Ordering::SeqCst);
        let _ = retry.retry_move_or_copy(&failed, false, &CancelToken::none());
        assert_restart_invariants(&rig.disk, Move, &format!("retry crash #{crash}/{total}"));
    }
}

// C#: RenameFaultInjectionTests.GroupCaseOnlyRename_Matrix. Reuses the full W3 safety and follow-up checker.
#[test]
fn group_case_only_rename_matrix() {
    matrix("RenameGroupCase", &group_rig, &|rig| {
        let result = rig.service.rename_group(&capture(true), "A", &rig.token());
        rig.outcomes.push(Outcome::Group(result));
        Ok(())
    });
}

// C#: RenameFaultInjectionTests.CaseOnlyRename_ReconcileInterrupted, including every nested reconcile fault.
#[test]
fn case_only_rename_reconcile_interrupted() {
    assert_reconcile_interrupted("RenameCase", &single_rig, &|rig| {
        let result = rig.service.rename(
            &RenameRequest {
                source: A.to_owned(),
                new_file_name: "A.jpg".to_owned(),
            },
            &rig.token(),
        );
        rig.outcomes.push(Outcome::Action(result));
        Ok(())
    });
}

// C#: FaultInjectionFileSystem.Move at 7eaee2a2 classifies same-folder moves as atomic rename.
#[test]
fn fault_injector_same_folder_rename_excludes_cross_volume_faults() {
    let rig = single_rig();
    rig.fs.arm(None);
    rig.fs
        .move_file(Path::new(A), Path::new(&join(PHOTOS, "A.jpg")))
        .expect("case rename");
    assert_eq!(rig.fs.kinds(), ["rename"]);
    for kind in [
        FaultKind::CrashPartial,
        FaultKind::FailPartial,
        FaultKind::CrashMoveCopiedNotDeleted,
    ] {
        assert!(!kind.applies("rename"), "{kind:?}");
    }
    for kind in [
        FaultKind::CrashBefore,
        FaultKind::CrashAfter,
        FaultKind::FailOnce,
        FaultKind::FailUnauthorized,
    ] {
        assert!(kind.applies("rename"), "{kind:?}");
    }
}
