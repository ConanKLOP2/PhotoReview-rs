//! Shared fixture for the W3-04 `FileActionService` red tests (`file_actions_service*.rs`).
//!
//! Port of the C# test fakes used by `tests/PhotoReview.Core.Tests/FileActions/` (baseline d92d6aae):
//!
//! * the in-memory disk: [`pr_test_support::FakeFileSystem`] wrapped in [`pr_test_support::HookFileSystem`] (the C#
//!   `InMemoryFileSystem` hooks; decision P-14 put them in `pr-test-support`, so this module does not copy them again);
//! * the recycle bin: [`ScenarioBin`], a [`RecycleBin`] that can leave the file behind (C# `FakeRecycleBin`,
//!   `RemoveFile` / `RecycleHook`, and the golden `W304Bin`), so no test ever touches the real Recycle Bin;
//! * the fault injector and step clock of the C# `FaultInjectionRig`: [`pr_test_support::FaultInjectionFileSystem`]
//!   and [`pr_test_support::FakeClock`] (only the parts the four `FileActionFaultInjectionGapTests` use).
//!
//! Layout matches the C# fixture: journal at `C:\data\operations.jsonl`, clock at 2026-10-01T00:00:00Z, source files
//! stamped 2026-09-19T09:00:00Z. Helpers here compile against the stubs; the bodies they call are the ones under test.

#![allow(dead_code)] // shared helpers: each test binary uses only part of this module

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{AppendStream, Clock, CopyCreationProof, FileStat, FileSystem, ReadSeek, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::errors::{self, DESTINATION_OUTSIDE_SOURCE};
use pr_core::file_actions::journal::{EnglishJournalTexts, JournalTexts, OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::JournalEntry;
use pr_core::file_actions::service::{EnglishFileActionTexts, FileActionService};
use pr_core::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, FileActionRequest, FileActionResult,
};
use pr_core::file_actions::undo::MoveOverride;
use pr_core::model::{FileOperationType, JournalDurability};
use pr_core::sync::{CancelToken, Canceled};
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::{BinHook, FakeClock, FakeFileSystem, HookFileSystem};

/// The C# fixture's photo folder.
pub const PHOTOS: &str = r"C:\photos";
/// `2026-09-19T09:00:00Z` in .NET ticks: the last write of every source file the scenarios create.
pub const LAST_WRITE_TICKS: i64 = 639_254_052_000_000_000;
/// `2026-10-01T00:00:00Z` in .NET ticks: the fixed service clock (golden `CLOCK`).
pub const CLOCK_TICKS: i64 = 639_264_096_000_000_000;

/// A `DateTimeUtc` from .NET ticks (the fixture's stamps are all in range).
pub fn stamp(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("fixture stamp is in range")
}

/// `LAST_WRITE_TICKS` plus `seconds`.
pub fn last_write_plus(seconds: i64) -> DateTimeUtc {
    stamp(LAST_WRITE_TICKS + seconds * TICKS_PER_SECOND)
}

/// The journal's paths: data root `C:\data`, journal at [`JOURNAL_PATH`] (C# `FakeAppPaths`).
pub fn fixture_paths() -> AppPaths {
    AppPaths::new(Path::new(r"C:\data"), None, false)
}

/// The journal file of [`fixture_paths`] as text (`AppPaths::journal_file`; C# `FakeAppPaths.JournalFile`).
pub fn journal_path() -> String {
    fixture_paths().journal_file.to_string_lossy().into_owned()
}

/// Deterministic content of `len` bytes: the golden fixture uses `'a' + i % 26`.
pub fn pattern_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| b'a' + (i % 26) as u8).collect()
}

/// `io::Error` of the C# `OperationCanceledException` a fake or override throws (contract: `io::Error::other(Canceled)`).
pub fn canceled_error() -> io::Error {
    io::Error::other(Canceled)
}

/// The English text of a journal error code, as the result's `error` shows it for the English UI (C# `Tr.*`).
pub fn english_code_text(code: &str) -> String {
    errors::text(code, &|key| EnglishJournalTexts.localized(key)).expect("known journal code")
}

/// C# `Tr.CoreFileActionDestinationOutsideSource`: the coded error text of `DESTINATION_OUTSIDE_SOURCE`.
pub fn outside_text() -> String {
    english_code_text(DESTINATION_OUTSIDE_SOURCE)
}

/// Keys and paths compare OrdinalIgnoreCase (C# default for the in-memory fake).
fn key(path: &str) -> String {
    path.to_ascii_lowercase()
}

/// `true` when `path` is `root` or lies under it (case-insensitive, separator-aware enough for `C:\` roots).
fn under_root(path: &str, root: &str) -> bool {
    let p = key(path);
    let r = key(root);
    p.starts_with(&r)
}

// ---------------------------------------------------------------------------------------------------------------
// ScenarioBin: a RecycleBin that never touches the real bin
// ---------------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct BinState {
    /// Roots (`C:\`) that have no Recycle Bin: `can_recycle` is `false` under them (golden `no_bin_roots`).
    no_bin_roots: Vec<String>,
    /// Per-root capacity in bytes; a root without an entry always fits (golden `bin_capacity`).
    capacity: Vec<(String, u64)>,
    /// Paths whose `send_to_recycle_bin` fails with the given message (golden `recycle_fail`).
    send_fail: HashMap<String, String>,
    /// Paths whose `delete_permanently` fails with the given message (golden `permanent_fail`).
    permanent_fail: HashMap<String, String>,
    /// Paths that a successful send leaves on disk ("the shell says yes but the file is still there", golden `recycle_leaves`).
    send_leaves: HashSet<String>,
    /// Paths that a successful permanent delete leaves on disk (golden `permanent_leaves`).
    permanent_leaves: HashSet<String>,
    /// Optional hook run before every send (C# `RecycleHook`); `Some(err)` fails the send.
    send_hook: Option<BinHook>,
    /// Paths `send_to_recycle_bin` completed for, in call order (C# `RecycledPaths`).
    recycled: Vec<String>,
    /// Paths `delete_permanently` completed for, in call order.
    permanently_deleted: Vec<String>,
    /// Every `fits_in_recycle_bin` question, in order (golden `fits_calls`).
    fits_calls: Vec<String>,
}

/// The recycle bin fake: fails and leaves files behind on request, records every call, and deletes the file from the
/// wrapped disk like the real shell does (C# `FakeRecycleBin.RemoveFile`, the B-06 post-check).
pub struct ScenarioBin {
    disk: Arc<FakeFileSystem>,
    state: Mutex<BinState>,
}

impl ScenarioBin {
    pub fn new(disk: Arc<FakeFileSystem>) -> Self {
        Self {
            disk,
            state: Mutex::new(BinState::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BinState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_no_bin_roots(&self, roots: &[&str]) {
        self.lock().no_bin_roots = roots.iter().map(|r| (*r).to_owned()).collect();
    }

    pub fn set_capacity(&self, root: &str, bytes: u64) {
        self.lock().capacity.push((root.to_owned(), bytes));
    }

    pub fn fail_send(&self, path: &str, message: &str) {
        self.lock().send_fail.insert(key(path), message.to_owned());
    }

    pub fn fail_permanent(&self, path: &str, message: &str) {
        self.lock().permanent_fail.insert(key(path), message.to_owned());
    }

    pub fn leave_on_send(&self, path: &str) {
        self.lock().send_leaves.insert(key(path));
    }

    pub fn leave_on_permanent(&self, path: &str) {
        self.lock().permanent_leaves.insert(key(path));
    }

    pub fn set_send_hook(&self, hook: Option<BinHook>) {
        self.lock().send_hook = hook;
    }

    pub fn recycled(&self) -> Vec<String> {
        self.lock().recycled.clone()
    }

    pub fn permanently_deleted(&self) -> Vec<String> {
        self.lock().permanently_deleted.clone()
    }

    pub fn fits_calls(&self) -> Vec<String> {
        self.lock().fits_calls.clone()
    }

    fn root_of(path: &Path) -> String {
        let text = path.to_string_lossy();
        match text.find(':') {
            Some(i) => text[..=i].to_owned() + r"\",
            None => text.into_owned(),
        }
    }
}

impl RecycleBin for ScenarioBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        let text = path.to_string_lossy().into_owned();
        let hook = self.lock().send_hook.clone();
        if let Some(hook) = hook {
            if let Some(err) = hook(path) {
                return Err(err);
            }
        }
        if let Some(message) = self.lock().send_fail.get(&key(&text)).cloned() {
            return Err(io::Error::other(message));
        }
        let leaves = self.lock().send_leaves.contains(&key(&text));
        if !leaves {
            self.disk.delete(path)?;
        }
        self.lock().recycled.push(text);
        Ok(())
    }

    fn can_recycle(&self, path: &Path) -> bool {
        let text = path.to_string_lossy().into_owned();
        !self.lock().no_bin_roots.iter().any(|root| under_root(&text, root))
    }

    fn delete_permanently(&self, path: &Path) -> io::Result<()> {
        let text = path.to_string_lossy().into_owned();
        if let Some(message) = self.lock().permanent_fail.get(&key(&text)).cloned() {
            return Err(io::Error::other(message));
        }
        let leaves = self.lock().permanent_leaves.contains(&key(&text));
        if !leaves {
            self.disk.delete(path)?;
        }
        self.lock().permanently_deleted.push(text);
        Ok(())
    }

    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        // Undo is not exercised through this fake in the W3-04 tests; the real restore lives in the undo tests (W3-05).
        let _ = (original_path, expected_size, expected_last_write_utc);
        Ok(true)
    }

    fn fits_in_recycle_bin(&self, path: &Path, file_size: u64) -> bool {
        let text = path.to_string_lossy().into_owned();
        let root = Self::root_of(path);
        let mut state = self.lock();
        state.fits_calls.push(text);
        match state.capacity.iter().find(|(r, _)| key(r) == key(&root)) {
            Some((_, cap)) => file_size <= *cap,
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// FaultableFs: the C# `FaultableFileSystem` of the mutation-gap tests
// ---------------------------------------------------------------------------------------------------------------

/// C# `FileExistsHook`: `Some(answer)` overrides `file_exists`. C# throws from `FileExists`; the Rust `file_exists` cannot
/// fail, so a test that makes it throw answers `true` instead ("cannot confirm the file is gone" = still counted there).
pub type ExistsHook = Arc<dyn Fn(&Path) -> Option<bool> + Send + Sync>;
/// C# `StatFilter`: rewrites what a stat returns (`None` = the file looks missing).
pub type StatFilter = Arc<dyn Fn(&Path, Option<FileStat>) -> Option<FileStat> + Send + Sync>;
/// C# `AfterMove`: runs after a successful move.
pub type AfterMove = Arc<dyn Fn(&Path, &Path) + Send + Sync>;

#[derive(Default)]
struct FaultableHooks {
    exists: Option<ExistsHook>,
    stat_filter: Option<StatFilter>,
    after_move: Option<AfterMove>,
}

/// A [`FileSystem`] over the fixture's [`HookFileSystem`] adding the three hooks of C# `FaultableFileSystem`; every other
/// call is forwarded unchanged, so it is transparent while no hook is set.
pub struct FaultableFs {
    inner: Arc<HookFileSystem>,
    hooks: Mutex<FaultableHooks>,
}

impl FaultableFs {
    pub fn new(inner: Arc<HookFileSystem>) -> Self {
        Self {
            inner,
            hooks: Mutex::new(FaultableHooks::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FaultableHooks> {
        self.hooks.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_exists_hook(&self, hook: Option<ExistsHook>) {
        self.lock().exists = hook;
    }

    pub fn set_stat_filter(&self, filter: Option<StatFilter>) {
        self.lock().stat_filter = filter;
    }

    pub fn set_after_move(&self, hook: Option<AfterMove>) {
        self.lock().after_move = hook;
    }

    fn filter(&self, path: &Path, stat: Option<FileStat>) -> Option<FileStat> {
        let filter = self.lock().stat_filter.clone();
        match filter {
            Some(filter) => filter(path, stat),
            None => stat,
        }
    }
}

impl FileSystem for FaultableFs {
    fn file_exists(&self, path: &Path) -> bool {
        let hook = self.lock().exists.clone();
        hook.and_then(|hook| hook(path))
            .unwrap_or_else(|| self.inner.file_exists(path))
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        let stat = self.inner.get_file_stat(path);
        self.filter(path, stat)
    }
    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        let stat = self.inner.try_get_file_stat(path)?;
        Ok(self.filter(path, stat))
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)?;
        let hook = self.lock().after_move.clone();
        if let Some(hook) = hook {
            hook(source, destination);
        }
        Ok(())
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }
    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        self.inner.try_copy_new_with_proof(source, destination, proof)
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
    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        self.inner.try_probe_readable(path)
    }
    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.inner.create_directory(path)
    }
    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        self.inner.try_delete_empty_directory(path)
    }
    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.resolve_real_path(path)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------------------------------------------

/// One test's world: disk, hooks, clock, bin, journal and the service under test (C# test constructor).
pub struct Fixture {
    pub disk: Arc<FakeFileSystem>,
    pub hooks: Arc<HookFileSystem>,
    /// The file system the journal and the service see: [`FaultableFs`] over `hooks` (transparent until a hook is set).
    pub faulty: Arc<FaultableFs>,
    pub clock: Arc<FakeClock>,
    pub bin: Arc<ScenarioBin>,
    pub journal: Arc<OperationJournal>,
    pub service: FileActionService,
}

impl Fixture {
    /// The C# constructor: no move override, English texts, durable journal.
    pub fn new() -> Self {
        Self::build(None, OperationJournalOptions::default())
    }

    /// The C# constructor with a `moveOverride` built from the disk it acts on (the W3-05 test seam; the same
    /// override is handed to undo in its tests). `make` receives the fixture's own disk, so the override can move or
    /// rewrite files the way the C# lambda does.
    pub fn with_move_override<F>(make: F) -> Self
    where
        F: FnOnce(Arc<FakeFileSystem>) -> MoveOverride + 'static,
    {
        Self::build(Some(Box::new(make)), OperationJournalOptions::default())
    }

    /// The journal with explicit options (durability, append-failure hooks, live-operation registry).
    pub fn with_journal_options(options: OperationJournalOptions) -> Self {
        Self::build(None, options)
    }

    /// A move override and explicit journal options together (the Vietnamese-journal test needs both).
    pub fn with_move_override_and_journal_options<F>(make: F, options: OperationJournalOptions) -> Self
    where
        F: FnOnce(Arc<FakeFileSystem>) -> MoveOverride + 'static,
    {
        Self::build(Some(Box::new(make)), options)
    }

    fn build(
        make_override: Option<Box<dyn FnOnce(Arc<FakeFileSystem>) -> MoveOverride>>,
        options: OperationJournalOptions,
    ) -> Self {
        let clock = Arc::new(FakeClock::at(stamp(CLOCK_TICKS)));
        let disk = Arc::new(FakeFileSystem::with_clock(clock.clone()));
        let hooks = Arc::new(HookFileSystem::new(disk.clone()));
        let bin = Arc::new(ScenarioBin::new(disk.clone()));
        let faulty = Arc::new(FaultableFs::new(hooks.clone()));
        let fs: Arc<dyn FileSystem> = faulty.clone();
        let clock_dyn: Arc<dyn Clock> = clock.clone();
        let journal = Arc::new(OperationJournal::with_options(
            &fixture_paths(),
            fs.clone(),
            clock_dyn.clone(),
            options,
        ));
        let recycle: Arc<dyn RecycleBin> = bin.clone();
        let mut service = FileActionService::new(journal.clone(), fs, clock_dyn, recycle);
        if let Some(make) = make_override {
            service = service.with_move_override(make(disk.clone()));
        }
        Self {
            disk,
            hooks,
            faulty,
            clock,
            bin,
            journal,
            service,
        }
    }

    /// `true` when the disk holds a file at `path` (the C# `_fs.FileExists`).
    pub fn file_exists(&self, path: &str) -> bool {
        self.disk.file_exists(Path::new(path))
    }

    /// `true` when the disk holds a directory at `path` (the C# `_fs.DirectoryExists`).
    pub fn dir_exists(&self, path: &str) -> bool {
        self.disk.directory_exists(Path::new(path))
    }

    /// Adds a source file of `len` bytes stamped [`LAST_WRITE_TICKS`] (C# `_fs.WriteAllTextAtomic` + stamp).
    pub fn add_file(&self, path: &str, len: usize) {
        self.disk
            .add_file_at(Path::new(path), &pattern_bytes(len), stamp(LAST_WRITE_TICKS));
    }

    /// Adds a file with explicit content and stamp.
    pub fn add_file_bytes(&self, path: &str, bytes: &[u8], last_write: DateTimeUtc) {
        self.disk.add_file_at(Path::new(path), bytes, last_write);
    }

    /// Adds a directory (C# `_fs.CreateDirectory`).
    pub fn add_dir(&self, path: &str) {
        self.disk.add_dir(Path::new(path));
    }

    /// Current content of a file on the disk, if any.
    pub fn content(&self, path: &str) -> Option<Vec<u8>> {
        self.disk.file_content(Path::new(path))
    }

    /// Current stat of a file on the disk, if any.
    pub fn stat(&self, path: &str) -> Option<pr_core::abstractions::FileStat> {
        self.disk.peek_stat(Path::new(path))
    }

    /// The journal file's lines, split on CRLF (C# `File.ReadAllLines` of `operations.jsonl`); empty when no journal.
    pub fn journal_lines(&self) -> Vec<String> {
        match self.disk.file_content(Path::new(&journal_path())) {
            None => Vec::new(),
            Some(bytes) => String::from_utf8(bytes)
                .expect("journal is UTF-8")
                .split("\r\n")
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    /// Every journal line parsed back (W1-14 `read_entries`), in file order.
    pub fn journal_entries(&self) -> Vec<JournalEntry> {
        let mut out = Vec::new();
        self.journal
            .read_entries(&mut |entry| out.push(entry))
            .expect("journal reads back");
        out
    }

    /// The `(kind, path)` pairs of the hook trace whose kind is one of `kinds` (C# `_fs.Events` filtered).
    pub fn events_of(&self, kinds: &[&str]) -> Vec<(String, PathBuf)> {
        self.hooks
            .events()
            .into_iter()
            .filter(|e| kinds.contains(&e.kind))
            .map(|e| (e.kind.to_owned(), e.path))
            .collect()
    }

    /// `FileActionService::execute` with a fresh, un-cancelled token.
    pub fn run(&self, request: &FileActionRequest) -> FileActionResult {
        self.service.execute(request, &CancelToken::none())
    }

    /// `FileActionService::execute` with the given token.
    pub fn run_with(&self, request: &FileActionRequest, cancel: &CancelToken) -> FileActionResult {
        self.service.execute(request, cancel)
    }

    /// `FileActionService::execute_group` with a fresh, un-cancelled token.
    pub fn run_group(&self, request: &CaptureGroupActionRequest) -> CaptureGroupActionResult {
        self.service.execute_group(request, &CancelToken::none())
    }

    /// `FileActionService::execute_group` with the given token.
    pub fn run_group_with(
        &self,
        request: &CaptureGroupActionRequest,
        cancel: &CancelToken,
    ) -> CaptureGroupActionResult {
        self.service.execute_group(request, cancel)
    }

    /// Journal durability of this fixture's journal (C# `OperationJournal.Durability`).
    pub fn durability(&self) -> JournalDurability {
        self.journal.durability()
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

/// Makes the `n`-th (1-based) `open_append` on the journal fail with `message` (golden `append_fail`: the Prepared line
/// is append 1, the Committed line append 2). The counter is per fixture.
pub fn fail_journal_append_nth(fx: &Fixture, n: usize, message: &'static str) {
    let counter = Arc::new(Mutex::new(0usize));
    let journal = journal_path();
    fx.hooks.set_open_append_hook(Some(Arc::new(move |path: &Path| {
        if !path.to_string_lossy().eq_ignore_ascii_case(&journal) {
            return None;
        }
        let mut count = counter.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *count += 1;
        if *count == n {
            Some(io::Error::other(message))
        } else {
            None
        }
    })));
}

/// Request for one single-file action (C# `new FileActionRequest(source, op, destination)`).
pub fn single(source: &str, operation: FileOperationType, destination: Option<&str>) -> FileActionRequest {
    let mut request = FileActionRequest::new(source, operation);
    request.destination = destination.map(str::to_owned);
    request
}

/// Request for a capture group made of JPEG, RAW and optional XMP (C# `CaptureGroupActionRequest`).
pub fn group(
    jpeg: &str,
    raw: &str,
    xmp: Option<&str>,
    operation: FileOperationType,
    destination: Option<&str>,
) -> CaptureGroupActionRequest {
    let capture = CaptureGroup::new(jpeg, raw, xmp.map(str::to_owned)).expect("valid capture group");
    let mut request = CaptureGroupActionRequest::new(capture, operation);
    request.destination = destination.map(str::to_owned);
    request
}

/// The English catalog the service falls back to (`EnglishFileActionTexts`), for message checks.
pub fn english_texts() -> EnglishFileActionTexts {
    EnglishFileActionTexts
}
