//! Shared fixture of the W3-05 `UndoService` tests (`file_actions_undo.rs`, `file_actions_undo_golden.rs`).
//!
//! Port of the C# test fakes of `tests/PhotoReview.Core.Tests/FileActions/` (baseline d92d6aae) and of the scripted fakes of
//! the golden oracle `tools/oracle-cs/Commands/W3-05.cs` (`W305Fs`, `W305Bin`):
//!
//! * [`ScriptedFs`]: a [`FileSystem`] over the in-memory [`FakeFileSystem`] with the per-step hooks of the oracle (`move_fail`,
//!   `move_leaves_source`, `after_move`, `on_append`, `append_fail_from`) and the C# test hooks (`MoveHook`, `OpenAppendHook`,
//!   `StampOnMove`). Hooks fire only on calls made by the code under test; scripted "by hand" actions go straight to the disk.
//!   Hook closures run with the hook mutex released, so a hook may read the journal (which calls back into this file system).
//! * [`ScriptedBin`]: a [`RecycleBin`] over the same disk with the six modes of the oracle plus `Throw` (a bin that fails with an
//!   error) and `RestoreThenThrow`. It never touches the real Recycle Bin.
//! * [`TestGate`]: [`FileActionGate`] over an `AtomicBool` (stands in for `FileActionService`, which is W3-04's).
//! * [`Fixture`] wires the journal (`C:\data\operations.jsonl`, like the oracle), the clock, and the fakes together.
//!
//! The helpers here compile against the W3-05 stub; the bodies they call (`UndoService`, the journal) are the ones under test.
#![allow(dead_code)] // shared fixture: each test binary uses only part of this module

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{AppendStream, Clock, FileStat, FileSystem, LiveOperationRegistry, ReadSeek, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::types::{CaptureGroupActionResult, CaptureGroupMemberResult, FileActionResult};
use pr_core::file_actions::undo::{EnglishUndoTexts, FileActionGate, MoveOverride, UndoService};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::text::ignore_case_key;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::{FakeClock, FakeFileSystem};

// ---------------------------------------------------------------------------------------------------------------
// Paths and time
// ---------------------------------------------------------------------------------------------------------------

/// The journal file of the fixture: `C:\data\operations.jsonl` (the oracle's `W305JournalPath`).
pub const JOURNAL: &str = r"C:\data\operations.jsonl";

/// `AppPaths` whose journal is [`JOURNAL`] (`AppPaths::new(local, Some(data root), false)`).
pub fn paths() -> AppPaths {
    AppPaths::new(
        std::path::Path::new(r"C:\Users\test\AppData\Local"),
        Some(r"C:\data"),
        false,
    )
}

/// .NET ticks of `2026-09-19T09:00:00Z`: the `writeTime` most C# tests use (the oracle's `LAST_WRITE`).
pub const BASE_TICKS: i64 = 639_254_052_000_000_000;

/// `DateTime(y, mo, d, h, mi, s, Utc)` as a [`DateTimeUtc`] (proleptic Gregorian; exact for every date the tests use).
pub fn utc(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> DateTimeUtc {
    // Days since 1970-01-01 with the civil-calendar algorithm (Howard Hinnant), then .NET ticks of the Unix epoch.
    const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_unix_epoch = era * 146_097 + doe - 719_468;
    let seconds = days_since_unix_epoch * 86_400 + hour * 3_600 + minute * 60 + second;
    DateTimeUtc::from_ticks(UNIX_EPOCH_TICKS + seconds * TICKS_PER_SECOND).expect("fixture stamp is in range")
}

/// `t` plus `seconds`.
pub fn plus_secs(t: DateTimeUtc, seconds: i64) -> DateTimeUtc {
    plus_ticks(t, seconds * TICKS_PER_SECOND)
}

/// `t` plus `hours`.
pub fn plus_hours(t: DateTimeUtc, hours: i64) -> DateTimeUtc {
    plus_secs(t, hours * 3_600)
}

/// `t` plus `millis` milliseconds.
pub fn plus_ms(t: DateTimeUtc, millis: i64) -> DateTimeUtc {
    plus_ticks(t, millis * 10_000)
}

/// `t` plus `ticks` (100 ns units).
pub fn plus_ticks(t: DateTimeUtc, ticks: i64) -> DateTimeUtc {
    t.checked_add_ticks(ticks).expect("fixture stamp stays in range")
}

/// `2026-09-19T09:00:00Z` (`writeTime`).
pub fn write_time() -> DateTimeUtc {
    DateTimeUtc::from_ticks(BASE_TICKS).expect("fixture stamp is in range")
}

/// `2026-09-19T10:00:00Z`: the C# `_clock.UtcNow` of the journal and the undo clock.
pub fn clock_now() -> DateTimeUtc {
    utc(2026, 9, 19, 10, 0, 0)
}

/// Rounds down to a 2 s boundary: what a FAT destination stores (C# `RoundToFat`, RV-C01).
pub fn round_to_fat(t: DateTimeUtc) -> DateTimeUtc {
    let two_seconds = 2 * TICKS_PER_SECOND;
    DateTimeUtc::from_ticks(t.ticks() - t.ticks() % two_seconds).expect("fixture stamp is in range")
}

// ---------------------------------------------------------------------------------------------------------------
// Journal entries and results (C# object initialisers, by hand)
// ---------------------------------------------------------------------------------------------------------------

/// A journal line with `timestamp == last_write == stamp`; use struct update syntax for `undo`, `group_*`, `error`, ...
pub fn entry(
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
        last_write_utc: JournalDateTime::utc(stamp),
        timestamp_utc: JournalDateTime::utc(stamp),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// A group member: `permanent` is written on disk, `destination` may be `None`.
pub fn member(
    source: &str,
    destination: Option<&str>,
    size: i64,
    stamp: DateTimeUtc,
    permanent: bool,
) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size,
        last_write_utc: JournalDateTime::utc(stamp),
        permanent,
    }
}

/// A completed member result (`completed`, no conflict, no error).
pub fn completed(member: &JournalGroupMember) -> CaptureGroupMemberResult {
    CaptureGroupMemberResult::new(member.clone(), true, false)
}

/// A single Move/Recycle/Copy result, as `FileActionService` reports it.
pub fn single_result(
    succeeded: bool,
    operation: FileOperationType,
    source: &str,
    destination: Option<&str>,
    size: i64,
    stamp: DateTimeUtc,
) -> FileActionResult {
    FileActionResult::new(
        succeeded,
        operation,
        source,
        destination.map(str::to_owned),
        size,
        stamp,
        None,
    )
}

/// A group result whose entry is the `Committed` line `entry_id` of the first member (C# `RegisterGroup` test helper).
pub fn group_result(
    operation: FileOperationType,
    id: &str,
    members: &[JournalGroupMember],
) -> CaptureGroupActionResult {
    let first = &members[0];
    let mut line = entry(
        id,
        operation,
        JournalState::Committed,
        &first.source,
        first.destination.as_deref(),
        first.size,
        first.last_write_utc.value,
    );
    line.group_id = Some(id.to_owned());
    line.group_members = Some(members.to_vec());
    CaptureGroupActionResult::new(
        true,
        false,
        operation,
        id,
        Some(Box::new(line)),
        members.iter().map(completed).collect(),
        None,
    )
}

// ---------------------------------------------------------------------------------------------------------------
// ScriptedFs
// ---------------------------------------------------------------------------------------------------------------

/// C# `MoveHook` / `Func<string,string,Task>`: returns the error the move fails with, before anything moves.
pub type MoveHook = Arc<dyn Fn(&Path, &Path) -> Option<io::Error> + Send + Sync>;
/// C# `OpenAppendHook`: returns the error `open_append` fails with, before the stream exists.
pub type AppendHook = Arc<dyn Fn(&Path) -> Option<io::Error> + Send + Sync>;
/// C# `StampOnMove`: the write time a moved file keeps at its destination (FAT rounding in the tests).
pub type StampOnMove = Arc<dyn Fn(&Path, DateTimeUtc) -> DateTimeUtc + Send + Sync>;
/// An arbitrary scripted action (test-only; the golden uses the data actions).
pub type RunFn = Arc<dyn Fn(&FakeFileSystem) + Send + Sync>;

/// A scripted "by hand" action: runs directly on the disk, never fires a hook (C# `Run(JsonArray)` of the oracle).
#[derive(Clone)]
pub enum Action {
    /// `{add, size, ticks}`: a file of `size` bytes `x` at `path` with the given write time (parents created).
    Add {
        path: String,
        size: usize,
        stamp: DateTimeUtc,
    },
    /// `{delete}`: removes the file (no error when it is missing).
    Delete(String),
    /// `{move, to}`: moves the file keeping its stamp, no hook.
    MoveTo { from: String, to: String },
    /// `{append_line}`: the raw line plus CRLF, appended to the journal file.
    AppendLine(String),
    /// Test-only action with arbitrary effects on the disk.
    Run(RunFn),
}

/// Per-step hooks and counters, reset by [`ScriptedFs::reset_step`] (C# `ResetHooks`).
#[derive(Default)]
struct Hooks {
    /// `move_fail`: a move whose SOURCE (OrdinalIgnoreCase) is a key fails with the message, before anything moves.
    move_fail: HashMap<String, String>,
    /// `move_leaves_source`: the move copies instead (the source stays), and still counts as a successful move.
    move_leaves_source: bool,
    /// `after_move`: actions run right after the nth successful move of the step (1-based).
    after_move: Vec<(usize, Vec<Action>)>,
    /// `on_append`: actions run at the nth `open_append` of the step (1-based), before the fail check.
    on_append: Vec<(usize, Vec<Action>)>,
    /// `append_fail_from`: `open_append` calls numbered >= n fail with `append_fail_message`.
    append_fail_from: Option<usize>,
    /// `append_fail_message` (default `"simulated journal failure"`).
    append_fail_message: Option<String>,
    /// C# `MoveHook`: consulted for every move (after `move_fail`).
    move_hook: Option<MoveHook>,
    /// C# `OpenAppendHook`: consulted for every `open_append` (after `on_append`).
    append_hook: Option<AppendHook>,
    /// C# `StampOnMove`.
    stamp_on_move: Option<StampOnMove>,
    /// Successful move calls of the step, in order (the source path of each), the oracle's `Moves` counter.
    moved_sources: Vec<String>,
    /// `open_append` calls of the step (the oracle's `Appends`).
    appends: usize,
    /// `file_exists` panics while set (a file system that fails inside the undo, C# `CrashBeforeMutation = 0`).
    probe_panics: bool,
}

/// A [`FileSystem`] over the in-memory disk with the undo tests' hooks. See the module docs.
pub struct ScriptedFs {
    disk: Arc<FakeFileSystem>,
    hooks: Mutex<Hooks>,
}

impl ScriptedFs {
    pub fn new(disk: Arc<FakeFileSystem>) -> Self {
        Self {
            disk,
            hooks: Mutex::new(Hooks::default()),
        }
    }

    /// The in-memory disk under the hooks (for "by hand" setup and assertions).
    pub fn disk(&self) -> &Arc<FakeFileSystem> {
        &self.disk
    }

    fn hooks(&self) -> MutexGuard<'_, Hooks> {
        self.hooks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Clears every hook, counter and the move record (C# `ResetHooks`, called before each golden step).
    pub fn reset_step(&self) {
        *self.hooks() = Hooks::default();
    }

    /// `move_fail`: moves whose source is `path` fail with `message` (OrdinalIgnoreCase).
    pub fn fail_move_of(&self, path: &str, message: &str) {
        self.hooks().move_fail.insert(ignore_case_key(path), message.to_owned());
    }

    /// `move_leaves_source`: moves copy instead of moving.
    pub fn set_move_leaves_source(&self, value: bool) {
        self.hooks().move_leaves_source = value;
    }

    /// `after_move`: `actions` run right after the nth successful move of the step.
    pub fn after_move(&self, nth: usize, actions: Vec<Action>) {
        self.hooks().after_move.push((nth, actions));
    }

    /// `on_append`: `actions` run at the nth `open_append` of the step.
    pub fn on_append(&self, nth: usize, actions: Vec<Action>) {
        self.hooks().on_append.push((nth, actions));
    }

    /// Runs `action` at the NEXT `open_append` after this call (C# `PlantAtNextJournalAppend`, the Prepared line of the
    /// action under test).
    pub fn plant_at_next_append(&self, action: Action) {
        let mut hooks = self.hooks();
        let next = hooks.appends + 1;
        hooks.on_append.push((next, vec![action]));
    }

    /// `append_fail_from` with `message`: `open_append` calls numbered >= `from` fail with `io::Error::other(message)`.
    pub fn fail_appends_from(&self, from: usize, message: &str) {
        let mut hooks = self.hooks();
        hooks.append_fail_from = Some(from);
        hooks.append_fail_message = Some(message.to_owned());
    }

    /// C# `MoveHook`: consulted before every move; `Some(error)` fails it.
    pub fn on_move(&self, hook: impl Fn(&Path, &Path) -> Option<io::Error> + Send + Sync + 'static) {
        self.hooks().move_hook = Some(Arc::new(hook));
    }

    /// C# `OpenAppendHook`: consulted before every `open_append`; `Some(error)` fails it.
    pub fn on_open_append(&self, hook: impl Fn(&Path) -> Option<io::Error> + Send + Sync + 'static) {
        self.hooks().append_hook = Some(Arc::new(hook));
    }

    /// C# `StampOnMove`: the write time a file keeps at its destination after a move.
    pub fn stamp_on_move(&self, hook: impl Fn(&Path, DateTimeUtc) -> DateTimeUtc + Send + Sync + 'static) {
        self.hooks().stamp_on_move = Some(Arc::new(hook));
    }

    /// Sources of the successful moves of the step, in order (C# `Events` of kind "move").
    pub fn moved_sources(&self) -> Vec<String> {
        self.hooks().moved_sources.clone()
    }

    /// Number of `open_append` calls of the step.
    pub fn append_calls(&self) -> usize {
        self.hooks().appends
    }

    /// While `value` is set, every `file_exists` call panics with "simulated probe failure".
    pub fn set_probe_panics(&self, value: bool) {
        self.hooks().probe_panics = value;
    }

    fn run_actions(&self, actions: &[Action]) {
        for action in actions {
            run_action(&self.disk, action);
        }
    }
}

/// Runs one scripted action directly on `disk` (no hook fires).
pub fn run_action(disk: &FakeFileSystem, action: &Action) {
    match action {
        Action::Add { path, size, stamp } => disk.add_file_at(path, &vec![b'x'; *size], *stamp),
        Action::Delete(path) => {
            let _ = FileSystem::delete(disk, Path::new(path));
        }
        Action::MoveTo { from, to } => {
            FileSystem::move_file(disk, Path::new(from), Path::new(to)).expect("scripted by-hand move");
        }
        Action::AppendLine(line) => {
            let path = Path::new(JOURNAL);
            disk.add_dir(path.parent().expect("journal has a folder"));
            let mut stream = FileSystem::open_append(disk, path, false).expect("scripted journal append");
            stream
                .write_all(format!("{line}\r\n").as_bytes())
                .expect("scripted journal write");
        }
        Action::Run(run) => run(disk),
    }
}

impl FileSystem for ScriptedFs {
    fn file_exists(&self, path: &Path) -> bool {
        if self.hooks().probe_panics {
            panic!("simulated probe failure");
        }
        self.disk.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.disk.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.disk.get_file_stat(path)
    }

    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        self.disk.try_get_file_stat(path)
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        // Decide under the lock, call the hooks and the disk without it (a hook may read the journal).
        let (fail, hook, leaves_source) = {
            let hooks = self.hooks();
            (
                hooks
                    .move_fail
                    .get(&ignore_case_key(&source.to_string_lossy()))
                    .cloned(),
                hooks.move_hook.clone(),
                hooks.move_leaves_source,
            )
        };
        if let Some(message) = fail {
            return Err(io::Error::other(message));
        }
        if let Some(hook) = hook {
            if let Some(error) = hook(source, destination) {
                return Err(error);
            }
        }
        if leaves_source {
            FileSystem::copy_file(&*self.disk, source, destination)?;
        } else {
            FileSystem::move_file(&*self.disk, source, destination)?;
        }
        let stamp_hook = self.hooks().stamp_on_move.clone();
        if let (Some(stamp_hook), Some(stat)) = (stamp_hook, self.disk.peek_stat(destination)) {
            self.disk
                .set_last_write(destination, stamp_hook(destination, stat.last_write_utc));
        }
        let due = {
            let mut hooks = self.hooks();
            hooks.moved_sources.push(source.to_string_lossy().into_owned());
            let nth = hooks.moved_sources.len();
            hooks
                .after_move
                .iter()
                .filter(|(at, _)| *at == nth)
                .flat_map(|(_, actions)| actions.clone())
                .collect::<Vec<_>>()
        };
        self.run_actions(&due);
        Ok(())
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        FileSystem::copy_file(&*self.disk, source, destination)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        FileSystem::delete(&*self.disk, path)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        FileSystem::open_read_shared(&*self.disk, path, buffer_size)
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        let (appends, due, hook, fail_from, message) = {
            let mut hooks = self.hooks();
            hooks.appends += 1;
            let nth = hooks.appends;
            let due: Vec<Action> = hooks
                .on_append
                .iter()
                .filter(|(at, _)| *at == nth)
                .flat_map(|(_, actions)| actions.clone())
                .collect();
            (
                nth,
                due,
                hooks.append_hook.clone(),
                hooks.append_fail_from,
                hooks.append_fail_message.clone(),
            )
        };
        self.run_actions(&due);
        if let Some(hook) = hook {
            if let Some(error) = hook(path) {
                return Err(error);
            }
        }
        if let Some(from) = fail_from {
            if appends >= from {
                return Err(io::Error::other(
                    message.unwrap_or_else(|| "simulated journal failure".to_owned()),
                ));
            }
        }
        FileSystem::open_append(&*self.disk, path, durable)
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        FileSystem::write_all_text_atomic(&*self.disk, path, text, durable)
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        FileSystem::read_all_text(&*self.disk, path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        FileSystem::enumerate_files(&*self.disk, directory, pattern)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        FileSystem::enumerate_directories(&*self.disk, directory)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        FileSystem::create_directory(&*self.disk, path)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ScriptedBin
// ---------------------------------------------------------------------------------------------------------------

/// What a scripted bin does on `try_restore` (the oracle's modes, plus two error modes for the test-only throws).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BinMode {
    /// Adds the file (`size` bytes `x`, the expected write time), returns `Ok(true)`.
    Restore,
    /// Touches nothing, returns `Ok(false)` (the item is not in the bin).
    Fail,
    /// Adds the file, returns `Ok(false)` (the bin lies about a successful restore, the C5 case).
    RestoreReportFalse,
    /// Touches nothing, returns `Ok(true)` (the bin claims success without a file).
    ReportTrueOnly,
    /// Adds the file with the expected write time + 1 h, returns `Ok(true)`.
    RestoreOtherStamp,
    /// Adds a 3-byte file with the expected write time, returns `Ok(true)`.
    RestoreWrongSize,
    /// Touches nothing, returns `Err` with this message (a bin whose shell call fails).
    Throw(String),
    /// Adds the file, then returns `Err` with this message (a bin that restored and then threw).
    RestoreThenThrow(String),
}

/// `PlantInsteadOfRestore`: leaves something else at the path instead of restoring (returns `Ok(true)`).
pub type PlantFn = Arc<dyn Fn(&Path, u64, DateTimeUtc) + Send + Sync>;

#[derive(Default)]
struct BinState {
    default: Option<BinMode>,
    by_path: HashMap<String, BinMode>,
    calls: Vec<String>,
    plant: Option<PlantFn>,
}

/// A scripted [`RecycleBin`] over the in-memory disk (never the real bin). See the module docs.
pub struct ScriptedBin {
    disk: Arc<FakeFileSystem>,
    state: Mutex<BinState>,
}

impl ScriptedBin {
    pub fn new(disk: Arc<FakeFileSystem>) -> Self {
        Self {
            disk,
            state: Mutex::new(BinState::default()),
        }
    }

    fn state(&self) -> MutexGuard<'_, BinState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The mode of every path without its own mode (default `Restore`).
    pub fn set_default(&self, mode: BinMode) {
        self.state().default = Some(mode);
    }

    /// The mode of one path (OrdinalIgnoreCase).
    pub fn set_mode_for(&self, path: &str, mode: BinMode) {
        self.state().by_path.insert(ignore_case_key(path), mode);
    }

    /// Forgets every per-path mode (C# `set_bin` replaces the whole spec).
    pub fn clear_modes(&self) {
        let mut state = self.state();
        state.default = None;
        state.by_path.clear();
    }

    /// Replaces the restore with `plant` (C# `PlantInsteadOfRestore`); `None` restores normally again.
    pub fn plant_instead_of_restore(&self, plant: Option<PlantFn>) {
        self.state().plant = plant;
    }

    /// Every `try_restore` call of the step, in order (the original paths as the undo passed them).
    pub fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }

    /// Number of `try_restore` calls (C# `RestoreCalls`).
    pub fn restore_calls(&self) -> usize {
        self.state().calls.len()
    }

    /// Forgets the recorded calls (golden: `bin_calls` is per step).
    pub fn clear_calls(&self) {
        self.state().calls.clear();
    }
}

impl RecycleBin for ScriptedBin {
    fn send_to_recycle_bin(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "the undo never recycles"))
    }

    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        let (mode, plant) = {
            let mut state = self.state();
            state.calls.push(original_path.to_string_lossy().into_owned());
            let key = ignore_case_key(&original_path.to_string_lossy());
            let mode = state
                .by_path
                .get(&key)
                .cloned()
                .or_else(|| state.default.clone())
                .unwrap_or(BinMode::Restore);
            (mode, state.plant.clone())
        };
        if let Some(plant) = plant {
            plant(original_path, expected_size, expected_last_write_utc);
            return Ok(true);
        }
        let restore_with = |stamp: DateTimeUtc, size: usize| {
            self.disk.add_file_at(original_path, &vec![b'x'; size], stamp);
        };
        let size = usize::try_from(expected_size).expect("fixture size fits in usize");
        match mode {
            BinMode::Restore => {
                restore_with(expected_last_write_utc, size);
                Ok(true)
            }
            BinMode::Fail => Ok(false),
            BinMode::RestoreReportFalse => {
                restore_with(expected_last_write_utc, size);
                Ok(false)
            }
            BinMode::ReportTrueOnly => Ok(true),
            BinMode::RestoreOtherStamp => {
                restore_with(plus_hours(expected_last_write_utc, 1), size);
                Ok(true)
            }
            BinMode::RestoreWrongSize => {
                restore_with(expected_last_write_utc, 3);
                Ok(true)
            }
            BinMode::Throw(message) => Err(io::Error::other(message)),
            BinMode::RestoreThenThrow(message) => {
                restore_with(expected_last_write_utc, size);
                Err(io::Error::other(message))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// TestGate
// ---------------------------------------------------------------------------------------------------------------

/// The shared file-action gate of the tests (C# `FileActionService`'s `TryBegin`/`End`/`IsBusy`, over an `AtomicBool`).
#[derive(Default)]
pub struct TestGate {
    busy: AtomicBool,
}

impl TestGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }
}

impl FileActionGate for TestGate {
    fn try_begin(&self) -> bool {
        self.busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn end(&self) {
        self.busy.store(false, Ordering::SeqCst);
    }

    fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------------------------------------------

/// The C# `UndoServiceTests` fixture: an in-memory disk, a [`ScriptedFs`] and [`ScriptedBin`] over it, a journal at
/// [`JOURNAL`] with the clock at `now` (default `2026-09-19T10:00:00Z`), and the English texts.
pub struct Fixture {
    pub disk: Arc<FakeFileSystem>,
    pub fs: Arc<ScriptedFs>,
    pub bin: Arc<ScriptedBin>,
    pub clock: Arc<FakeClock>,
    pub journal: Arc<OperationJournal>,
}

impl Fixture {
    pub fn new() -> Self {
        Self::at(clock_now())
    }

    pub fn at(now: DateTimeUtc) -> Self {
        Self::build(now, None)
    }

    /// Like [`Fixture::at`], with the journal's live-operation registry set to `live` (C# `liveOperations:`).
    pub fn with_live_registry(now: DateTimeUtc, live: Arc<dyn LiveOperationRegistry>) -> Self {
        Self::build(now, Some(live))
    }

    fn build(now: DateTimeUtc, live: Option<Arc<dyn LiveOperationRegistry>>) -> Self {
        let clock = Arc::new(FakeClock::at(now));
        let disk = Arc::new(FakeFileSystem::with_clock(clock.clone()));
        let fs = Arc::new(ScriptedFs::new(disk.clone()));
        let bin = Arc::new(ScriptedBin::new(disk.clone()));
        let journal = Arc::new(OperationJournal::with_options(
            &paths(),
            fs.clone(),
            clock.clone(),
            OperationJournalOptions {
                append_retry_delay: Some(Arc::new(|_| {})),
                live_operations: live,
                ..OperationJournalOptions::default()
            },
        ));
        Self {
            disk,
            fs,
            bin,
            clock,
            journal,
        }
    }

    /// A fresh `UndoService` over this fixture (own gate, no move override, the fixture's clock is NOT injected: the
    /// service stamps with `SystemClock`, so tests that check timestamps call [`Fixture::undo_with_clock`]).
    pub fn undo(&self) -> UndoService {
        UndoService::new(
            self.journal.clone(),
            self.fs.clone(),
            self.bin.clone(),
            Arc::new(EnglishUndoTexts),
        )
    }

    /// Like [`Fixture::undo`] with the undo clock set to `clock` (C# `clock:` argument).
    pub fn undo_with_clock(&self, clock: Arc<dyn Clock>) -> UndoService {
        self.undo().with_clock(clock)
    }

    /// A move override that moves through the scripted fs (C# `moveOverride`), recording every `(from, to)` in `calls`.
    pub fn recording_override(&self, calls: Arc<Mutex<Vec<(String, String)>>>) -> MoveOverride {
        let fs = self.fs.clone();
        Arc::new(move |from: &Path, to: &Path| {
            calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((from.to_string_lossy().into_owned(), to.to_string_lossy().into_owned()));
            FileSystem::move_file(&*fs, from, to)
        })
    }

    /// A file of `size` bytes `x` at `path` with the given write time, by hand (no hook fires).
    pub fn add(&self, path: &str, size: usize, stamp: DateTimeUtc) {
        self.disk.add_file_at(path, &vec![b'x'; size], stamp);
    }

    /// A file with the given content and write time, by hand.
    pub fn add_bytes(&self, path: &str, content: &[u8], stamp: DateTimeUtc) {
        self.disk.add_file_at(path, content, stamp);
    }

    pub fn exists(&self, path: &str) -> bool {
        self.disk.file_exists(Path::new(path))
    }

    pub fn size_of(&self, path: &str) -> u64 {
        self.disk.peek_stat(path).expect("file exists").length
    }

    pub fn stamp_of(&self, path: &str) -> DateTimeUtc {
        self.disk.peek_stat(path).expect("file exists").last_write_utc
    }

    /// The journal text as the disk holds it (empty when nothing was written).
    pub fn journal_text(&self) -> String {
        String::from_utf8(self.disk.file_content(JOURNAL).unwrap_or_default()).expect("journal is UTF-8")
    }

    /// Appends a committed Move line by hand (C# `_journal.Append`).
    pub fn append(&self, line: &JournalEntry) {
        self.journal.append(line).expect("journal append");
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Journal readers (C# `OperationJournalTestExtensions`)
// ---------------------------------------------------------------------------------------------------------------

/// C# `ReadPendingOperations`: the Prepared lines of the journal.
pub fn read_pending_operations(journal: &OperationJournal) -> Vec<JournalEntry> {
    read_by_state(journal, JournalState::Prepared)
}

/// C# `ReadFailedOperations`: the Failed lines of the journal.
pub fn read_failed_operations(journal: &OperationJournal) -> Vec<JournalEntry> {
    read_by_state(journal, JournalState::Failed)
}

/// C# `ReadPendingAndFailedOperations`.
pub fn read_pending_and_failed_operations(journal: &OperationJournal) -> Vec<JournalEntry> {
    journal.read_pending_and_failed_operations().expect("journal readable")
}

/// C# `ReadCommittedMoves`.
pub fn read_committed_moves(journal: &OperationJournal) -> Vec<JournalEntry> {
    journal.read_committed_moves().expect("journal readable")
}

fn read_by_state(journal: &OperationJournal, state: JournalState) -> Vec<JournalEntry> {
    read_pending_and_failed_operations(journal)
        .into_iter()
        .filter(|line| line.state == state)
        .collect()
}
