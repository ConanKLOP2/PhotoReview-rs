//! Shared fixture for the W3-06 `RecoveryRetryService` red tests (`recovery_retry.rs`, `recovery_retry_golden.rs`).
//!
//! Port of the C# test doubles of `tests/PhotoReview.Core.Tests/FileActions/` (baseline d92d6aae) and of the scripted
//! world of the golden oracle `tools/oracle-cs/Commands/W3-06.cs` (`W306Fs`, `W306Bin`, `W306World`):
//!
//! * [`ScriptedFs`]: an in-memory disk with Windows rules (case-insensitive keys, no overwrite, parent folder required).
//!   It logs every call outside the journal folder exactly as the oracle does (`exists p`, `dir p`, `stat p`,
//!   `mkdir p`, `move s > d`, `copy s > d`, `delete p`, `open-read p`, `open-append p`, `write p`, `read-text p`), and
//!   runs hook rules at the START of a call, before the real operation. Copy and move use the trait defaults for
//!   `try_copy_new` (C# default interface method), so `exists` then `copy` is what the log shows.
//! * hook rules: [`Rule`] runs a closure ([`HookFn`]) or the data actions of the golden DSL ([`Op`], see
//!   [`data_rule`]). Rules are counted per (`on`, path) from the start of the retry (seed appends are not counted).
//!   The hook gets the [`ScriptedFs`] handle, so it may change the disk (C# `_fs.AddFile(...)` inside a hook). The
//!   world lock is NOT held while a hook runs: a hook may call into another service over the same disk (the race
//!   tests do).
//! * [`ScriptedBin`]: the scripted Recycle Bin (`W306Bin`): logs `bin.recycle p`, `bin.delete-permanently p`,
//!   `bin.restore p`, `bin.fits p n`. It never touches the real Recycle Bin (AGENTS.md).
//! * [`TestGate`]: the `FileActionGate` double (`AtomicBool`, C# `FileActionService.TryBegin/End/IsBusy`).
//! * [`Rig`]: journal + disk + two fixed clocks (journal and service), as the C# test constructor builds them.
//!
//! The journal lives at `C:\data\operations.jsonl` (C# `FakeAppPaths`). Its appends and reads are never logged.

#![allow(dead_code)] // shared helpers: each test binary uses only part of this module

use std::collections::{HashMap, HashSet};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{AppendStream, Clock, FileStat, FileSystem, ReadSeek, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_retry::{
    AllowPermanentDelete, EnglishRecoveryRetryTexts, RecoveryRetryService, RecoveryRetryTexts,
};
pub use pr_core::file_actions::undo::FileActionGate;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::FakeClock;

/// The journal folder of every scripted world (C# `FakeAppPaths` under `C:\data`).
pub const JOURNAL_DIR: &str = r"C:\data";
/// The app paths of every scripted world: the journal is exactly [`JOURNAL_FILE`] (C# `FakeAppPaths`, golden `W306Paths`).
pub fn journal_paths() -> AppPaths {
    AppPaths {
        journal_file: PathBuf::from(JOURNAL_FILE),
        ..AppPaths::new(Path::new(JOURNAL_DIR), None, false)
    }
}

/// The journal file (C# `FakeAppPaths.JournalFile`).
pub const JOURNAL_FILE: &str = r"C:\data\operations.jsonl";
/// `2026-09-18T12:00:00Z` in .NET ticks: the C# test clock (`FakeClock(new DateTime(2026, 9, 18, 12, 0, 0))`).
pub const CSHARP_CLOCK_TICKS: i64 = 639_253_296_000_000_000;

// ---------------------------------------------------------------------------------------------------------------
// Small builders
// ---------------------------------------------------------------------------------------------------------------

/// A `DateTimeUtc` from .NET ticks (the fixtures are all in range).
pub fn ts(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("fixture ticks are in range")
}

/// The English catalog of the recovery texts (`EnglishRecoveryRetryTexts`).
pub fn english() -> Arc<dyn RecoveryRetryTexts> {
    Arc::new(EnglishRecoveryRetryTexts)
}

/// A journal entry in the C# constructor's positional order. Optional members start as C# defaults (`null`); set
/// them with struct update syntax: `JournalEntry { undo: Some(true), ..entry(..) }`.
#[allow(clippy::too_many_arguments)]
pub fn entry(
    id: &str,
    op_type: FileOperationType,
    state: JournalState,
    source: &str,
    destination: Option<&str>,
    size: i64,
    last_write: DateTimeUtc,
    timestamp: DateTimeUtc,
    error: Option<&str>,
) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type,
        state,
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size,
        last_write_utc: JournalDateTime::utc(last_write),
        timestamp_utc: JournalDateTime::utc(timestamp),
        error: error.map(str::to_owned),
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// A Failed Move/Copy entry with no error text, stamped at `timestamp`.
pub fn failed_entry(
    id: &str,
    op_type: FileOperationType,
    source: &str,
    destination: &str,
    size: i64,
    last_write: DateTimeUtc,
    timestamp: DateTimeUtc,
) -> JournalEntry {
    entry(
        id,
        op_type,
        JournalState::Failed,
        source,
        Some(destination),
        size,
        last_write,
        timestamp,
        Some("Previous error"),
    )
}

/// A group member (C# `JournalGroupMember`, `Permanent` defaults to false).
pub fn member(source: &str, destination: Option<&str>, size: i64, last_write: DateTimeUtc) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size,
        last_write_utc: JournalDateTime::utc(last_write),
        permanent: false,
    }
}

/// `text` as the bytes of a seeded file.
pub fn bytes_of(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// Deterministic zero-filled content of `len` bytes (the golden `files`/`binned` items).
pub fn zeros(len: usize) -> Vec<u8> {
    vec![0; len]
}

/// Lower-cased key of a path (Windows paths compare OrdinalIgnoreCase).
fn key(path: &str) -> String {
    path.to_ascii_lowercase()
}

fn is_journal_path(path: &str) -> bool {
    key(path) == key(JOURNAL_DIR) || key(path).starts_with(&format!("{}\\", key(JOURNAL_DIR)))
}

/// The parent folder of a Windows path (`C:\photos\a.jpg` -> `C:\photos`; `C:\` -> none).
fn parent_of(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('\\');
    let index = trimmed.rfind('\\')?;
    if index == 0 {
        return None;
    }
    let parent = &trimmed[..index];
    // `C:` alone is the drive: its folder is `C:\`.
    if parent.ends_with(':') {
        Some(format!("{parent}\\"))
    } else {
        Some(parent.to_owned())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The scripted world
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct FileRec {
    path: String,
    data: Vec<u8>,
    last_write: DateTimeUtc,
}

/// What a hook returns when it does not fail: the special actions the oracle performs in place of the operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Run the real operation.
    Proceed,
    /// Move only: copy to the destination and keep the source (`{leave_source: true}`).
    LeaveSource,
    /// Copy only: create the destination with the first `bytes` bytes, then fail with `message` (`{partial: n}`).
    Partial { bytes: usize, message: String },
}

/// The call a hook is about to see.
pub struct Call<'a> {
    /// The first path of the call (the source of a move/copy, the path of a stat/restore/recycle). `None` for appends.
    pub path: Option<&'a Path>,
    /// The destination of a move/copy.
    pub to: Option<&'a Path>,
    /// 1-based count of calls of this (`on`, path) so far, this call included.
    pub nth: usize,
}

/// A hook body: may change the disk through the handle, and fails the call with `Err`.
pub type HookFn = Arc<dyn Fn(&ScriptedFs, &Call<'_>) -> io::Result<Next> + Send + Sync>;

/// One hook rule: runs `f` for the `nth` call (`None` = every call) of `on` (`stat`, `move`, `copy`, `append`,
/// `delete`, `restore`, `recycle`) on `path` (`None` = any path; matched case-insensitively).
#[derive(Clone)]
pub struct Rule {
    pub on: &'static str,
    pub path: Option<String>,
    pub nth: Option<usize>,
    pub f: HookFn,
}

/// A data action of the golden DSL (`tests/golden/recovery-retry.json`, `hooks[].do[]`).
#[derive(Debug, Clone)]
pub enum Op {
    /// `{delete: path}`: remove the file silently (no log, no hook).
    Delete(String),
    /// `{write: {path, len, ticks}}`: add a zero-filled file with that last write.
    Write { path: String, len: usize, ticks: i64 },
    /// `{cancel: true}`: cancel the token the test passed to the retry.
    Cancel,
    /// `{throw: message, exception?}`: fail the call with this error kind (`IOException` = `Other`).
    Throw { kind: io::ErrorKind, message: String },
    /// `{leave_source: true}`: see [`Next::LeaveSource`].
    LeaveSource,
    /// `{partial: n, message}`: see [`Next::Partial`].
    Partial { bytes: usize, message: String },
}

/// The data rule of one golden hook: `{on, path?, nth, do: [...]}` (`nth` defaults to 1).
pub fn data_rule(on: &'static str, path: Option<&str>, nth: usize, ops: Vec<Op>) -> Rule {
    let f: HookFn = Arc::new(move |fs: &ScriptedFs, _call: &Call<'_>| {
        for op in &ops {
            match op {
                Op::Delete(p) => {
                    fs.inner.world().files.remove(&key(p));
                }
                Op::Write { path, len, ticks } => fs.add_file_at_ticks(path, zeros(*len), *ticks),
                Op::Cancel => fs.inner.cancel.cancel(),
                Op::Throw { kind, message } => return Err(io::Error::new(*kind, message.clone())),
                Op::LeaveSource => return Ok(Next::LeaveSource),
                Op::Partial { bytes, message } => {
                    return Ok(Next::Partial {
                        bytes: *bytes,
                        message: message.clone(),
                    });
                }
            }
        }
        Ok(Next::Proceed)
    });
    Rule {
        on,
        path: path.map(str::to_owned),
        nth: Some(nth),
        f,
    }
}

/// Mutable state of the world (one lock; never held across a hook call).
#[derive(Default)]
struct World {
    files: HashMap<String, FileRec>,
    dirs: HashSet<String>,
    ops: Vec<String>,
    binned: Vec<(String, Vec<u8>, DateTimeUtc)>,
    counts: HashMap<String, usize>,
    counting: bool,
    rules: Vec<Rule>,
    /// C# `MoveLeavesSource`: every move copies and keeps the source.
    move_leaves_source: bool,
    /// C# `CopyFailsAfterBytes`: every copy creates the destination with this many bytes, then fails with
    /// `Simulated disk full during copy.`
    copy_fails_after: Option<usize>,
}

struct Inner {
    world: Mutex<World>,
    cancel: CancelToken,
}

impl Inner {
    fn world(&self) -> MutexGuard<'_, World> {
        self.world.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The scripted disk: a cheap handle (clones share one world), implementing [`FileSystem`].
#[derive(Clone)]
pub struct ScriptedFs {
    inner: Arc<Inner>,
}

/// Message of the C# `CopyFailsAfterBytes` failure.
pub const DISK_FULL_DURING_COPY: &str = "Simulated disk full during copy.";

impl ScriptedFs {
    /// An empty disk with the journal folder `C:\data` present (C# `FakeAppPaths` folder), cancelled through `cancel`.
    pub fn new(cancel: CancelToken) -> Self {
        let fs = Self {
            inner: Arc::new(Inner {
                world: Mutex::new(World::default()),
                cancel,
            }),
        };
        fs.add_dirs(JOURNAL_DIR);
        // Hook rules count from the start: the seed appends of `Rig::append` switch counting off around themselves.
        fs.inner.world().counting = true;
        fs
    }

    /// Turns the hook counting on or off; returns the previous state.
    pub fn set_counting(&self, value: bool) -> bool {
        std::mem::replace(&mut self.inner.world().counting, value)
    }

    fn add_dirs(&self, path: &str) {
        let mut dir = Some(path.to_owned());
        let mut world = self.inner.world();
        while let Some(current) = dir {
            world.dirs.insert(key(&current));
            dir = parent_of(&current);
        }
    }

    /// Adds a file with `data` and `last_write` without logging (C# `_fs.WriteAllTextAtomic` in the test setup).
    pub fn add_file(&self, path: &str, data: Vec<u8>, last_write: DateTimeUtc) {
        self.add_dirs_of(path);
        self.inner.world().files.insert(
            key(path),
            FileRec {
                path: path.to_owned(),
                data,
                last_write,
            },
        );
    }

    /// As [`add_file`](Self::add_file), last write from .NET ticks (golden `write` actions).
    pub fn add_file_at_ticks(&self, path: &str, data: Vec<u8>, ticks: i64) {
        self.add_file(path, data, ts(ticks));
    }

    /// Seeds a text file stamped `last_write` (C# `WriteAllTextAtomic(path, text)`), no log.
    pub fn seed_text(&self, path: &str, text: &str, last_write: DateTimeUtc) {
        self.add_file(path, bytes_of(text), last_write);
    }

    /// Removes a file without logging (C# `_fs.Delete(path)` inside a hook).
    pub fn remove(&self, path: &str) {
        self.inner.world().files.remove(&key(path));
    }

    /// Adds a folder (and its parents) without logging (C# `CreateDirectory` in the setup).
    pub fn seed_dir(&self, path: &str) {
        self.add_dirs(path);
    }

    fn add_dirs_of(&self, path: &str) {
        if let Some(parent) = parent_of(path) {
            self.add_dirs(&parent);
        }
    }

    /// The bytes of a file, without logging.
    pub fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.inner.world().files.get(&key(path)).map(|f| f.data.clone())
    }

    /// The text of a file, without logging (C# `ReadAllText`). Panics when the file is missing.
    pub fn text(&self, path: &str) -> String {
        let bytes = self.bytes(path).unwrap_or_else(|| panic!("no file {path}"));
        String::from_utf8(bytes).expect("fixture text is UTF-8")
    }

    /// Whether a file exists, without logging (C# `_fs.FileExists`).
    pub fn has_file(&self, path: &str) -> bool {
        self.inner.world().files.contains_key(&key(path))
    }

    /// Whether a folder exists, without logging.
    pub fn has_dir(&self, path: &str) -> bool {
        self.inner.world().dirs.contains(&key(path))
    }

    /// Stat of a file, without logging or hooks (C# `_fs.GetFileStat`).
    pub fn stat(&self, path: &str) -> Option<FileStat> {
        self.inner.world().files.get(&key(path)).map(|f| FileStat {
            length: f.data.len() as u64,
            last_write_utc: f.last_write,
        })
    }

    /// Every file outside the journal folder, sorted by path: `(path, len, last write)` (golden `files_after`).
    pub fn files_after(&self) -> Vec<(String, usize, DateTimeUtc)> {
        let world = self.inner.world();
        let mut out: Vec<_> = world
            .files
            .values()
            .filter(|f| !is_journal_path(&f.path))
            .map(|f| (f.path.clone(), f.data.len(), f.last_write))
            .collect();
        out.sort_by_key(|(path, _, _)| key(path));
        out
    }

    /// The journal file's text, or `None` when there is no journal (golden `journal_text`).
    pub fn journal_text(&self) -> Option<String> {
        self.bytes(JOURNAL_FILE)
            .map(|b| String::from_utf8(b).expect("journal is UTF-8"))
    }

    /// The journal file's lines (split on CRLF), empty when there is no journal.
    pub fn journal_lines(&self) -> Vec<String> {
        self.journal_text()
            .map(|t| t.split("\r\n").filter(|l| !l.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// The logged calls, in call order (golden `ops`; C# `Events`).
    pub fn ops(&self) -> Vec<String> {
        self.inner.world().ops.clone()
    }

    /// How many logged calls start with `prefix` (`"stat "` = stat count; C# `_fs.StatCount`).
    pub fn count_ops(&self, prefix: &str) -> usize {
        self.inner.world().ops.iter().filter(|o| o.starts_with(prefix)).count()
    }

    /// Forgets the logged calls (after the seeding, before the call under test).
    pub fn clear_ops(&self) {
        self.inner.world().ops.clear();
    }

    /// Adds a hook rule (see [`Rule`]).
    pub fn add_rule(&self, rule: Rule) {
        self.inner.world().rules.push(rule);
    }

    /// Removes every hook rule (C# `_fs.OpenAppendHook = null` and the like).
    pub fn clear_rules(&self) {
        self.inner.world().rules.clear();
    }

    /// Adds a closure hook that runs on every call of `on` (any path).
    pub fn on_every(
        &self,
        on: &'static str,
        f: impl Fn(&ScriptedFs, &Call<'_>) -> io::Result<Next> + Send + Sync + 'static,
    ) {
        self.add_rule(Rule {
            on,
            path: None,
            nth: None,
            f: Arc::new(f),
        });
    }

    /// C# `MoveLeavesSource = true`: every move copies and keeps the source.
    pub fn set_move_leaves_source(&self, value: bool) {
        self.inner.world().move_leaves_source = value;
    }

    /// C# `CopyFailsAfterBytes = n` (`None` = off): every copy leaves `n` bytes and fails.
    pub fn set_copy_fails_after(&self, bytes: Option<usize>) {
        self.inner.world().copy_fails_after = bytes;
    }

    /// The recycle-bin items, in bin order: `(path, len, last write)` (golden `binned_after`).
    pub fn binned(&self) -> Vec<(String, usize, DateTimeUtc)> {
        self.inner
            .world()
            .binned
            .iter()
            .map(|(p, d, w)| (p.clone(), d.len(), *w))
            .collect()
    }

    /// Adds an item to the recycle bin (golden `binned`, restorable by `try_restore`).
    pub fn add_binned(&self, path: &str, len: usize, last_write: DateTimeUtc) {
        self.inner
            .world()
            .binned
            .push((path.to_owned(), zeros(len), last_write));
    }

    fn log(&self, text: String, path: &str) {
        if !is_journal_path(path) {
            self.inner.world().ops.push(text);
        }
    }

    /// Runs the hook rules for one call: counts it, then runs every matching rule in order. The first rule that
    /// answers something other than `Proceed` ends the walk. The world lock is not held while a rule runs.
    fn fire(&self, on: &'static str, path: Option<&Path>, to: Option<&Path>) -> io::Result<Next> {
        let matches: Vec<(HookFn, usize)> = {
            let mut world = self.inner.world();
            if !world.counting {
                return Ok(Next::Proceed);
            }
            let call_key = format!(
                "{on}|{}",
                path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
            );
            let count = {
                let entry = world.counts.entry(call_key).or_insert(0);
                *entry += 1;
                *entry
            };
            world
                .rules
                .iter()
                .filter(|r| r.on == on)
                .filter(|r| match (&r.path, path) {
                    (None, _) => true,
                    (Some(rp), Some(p)) => rp.eq_ignore_ascii_case(&p.to_string_lossy()),
                    (Some(_), None) => false,
                })
                .filter(|r| r.nth.is_none_or(|n| n == count))
                .map(|r| (r.f.clone(), count))
                .collect()
        };
        for (f, nth) in matches {
            let call = Call { path, to, nth };
            match f(self, &call)? {
                Next::Proceed => {}
                other => return Ok(other),
            }
        }
        Ok(Next::Proceed)
    }

    fn require_parent(&self, path: &str) -> io::Result<()> {
        match parent_of(path) {
            Some(dir) if !self.inner.world().dirs.contains(&key(&dir)) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing folder: {dir}"),
            )),
            _ => Ok(()),
        }
    }
}

fn not_found(path: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("not found: {path}"))
}

/// The C# `IOException` of an existing destination.
fn exists_error(path: &str) -> io::Error {
    io::Error::other(format!("exists: {path}"))
}

impl FileSystem for ScriptedFs {
    fn file_exists(&self, path: &Path) -> bool {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("exists {text}"), &text);
        self.inner.world().files.contains_key(&key(&text))
    }

    fn directory_exists(&self, path: &Path) -> bool {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("dir {text}"), &text);
        self.inner.world().dirs.contains(&key(&text))
    }

    /// C# `GetFileStat` without its exceptions: a failing hook reads as "no file".
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.try_get_file_stat(path).ok().flatten()
    }

    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("stat {text}"), &text);
        self.fire("stat", Some(path), None)?;
        Ok(self.stat(&text))
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        let from = source.to_string_lossy().into_owned();
        let to = destination.to_string_lossy().into_owned();
        self.log(format!("move {from} > {to}"), &from);
        let special = self.fire("move", Some(source), Some(destination))?;
        let keep_source = special == Next::LeaveSource || self.inner.world().move_leaves_source;
        let world = self.inner.world();
        let Some(file) = world.files.get(&key(&from)).cloned() else {
            return Err(not_found(&from));
        };
        if world.files.contains_key(&key(&to)) {
            return Err(exists_error(&to));
        }
        drop(world);
        self.require_parent(&to)?;
        let mut world = self.inner.world();
        world.files.insert(
            key(&to),
            FileRec {
                path: to.clone(),
                data: file.data,
                last_write: file.last_write,
            },
        );
        if !keep_source {
            world.files.remove(&key(&from));
        }
        Ok(())
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        let from = source.to_string_lossy().into_owned();
        let to = destination.to_string_lossy().into_owned();
        self.log(format!("copy {from} > {to}"), &from);
        let special = self.fire("copy", Some(source), Some(destination))?;
        let (file, exists) = {
            let world = self.inner.world();
            (
                world.files.get(&key(&from)).cloned(),
                world.files.contains_key(&key(&to)),
            )
        };
        let Some(file) = file else {
            return Err(not_found(&from));
        };
        if exists {
            return Err(exists_error(&to));
        }
        self.require_parent(&to)?;
        let partial = match special {
            Next::Partial { bytes, message } => Some((bytes, message)),
            _ => self
                .inner
                .world()
                .copy_fails_after
                .map(|bytes| (bytes, DISK_FULL_DURING_COPY.to_owned())),
        };
        if let Some((bytes, message)) = partial {
            let data: Vec<u8> = file.data.iter().take(bytes).copied().collect();
            self.inner.world().files.insert(
                key(&to),
                FileRec {
                    path: to.clone(),
                    data,
                    last_write: file.last_write,
                },
            );
            return Err(io::Error::other(message));
        }
        self.inner.world().files.insert(
            key(&to),
            FileRec {
                path: to.clone(),
                data: file.data,
                last_write: file.last_write,
            },
        );
        // The copy completed; a hook may still fail the call (C# `AfterCopy` throwing after the proof was raised).
        self.fire("copy-after", Some(source), Some(destination))?;
        Ok(())
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("delete {text}"), &text);
        self.fire("delete", Some(path), None)?;
        self.inner.world().files.remove(&key(&text));
        Ok(())
    }

    fn open_read_shared(&self, path: &Path, _buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("open-read {text}"), &text);
        let bytes = self.bytes(&text).ok_or_else(|| not_found(&text))?;
        Ok(Box::new(Cursor::new(bytes)))
    }

    fn open_append(&self, path: &Path, _durable: bool) -> io::Result<Box<dyn AppendStream>> {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("open-append {text}"), &text);
        self.fire("append", None, None)?;
        self.require_parent(&text)?;
        Ok(Box::new(Appender {
            fs: self.clone(),
            path: text,
        }))
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, _durable: bool) -> io::Result<()> {
        let target = path.to_string_lossy().into_owned();
        self.log(format!("write {target}"), &target);
        self.require_parent(&target)?;
        self.add_file(&target, bytes_of(text), DateTimeUtc::MIN);
        Ok(())
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("read-text {text}"), &text);
        let bytes = self.bytes(&text).ok_or_else(|| not_found(&text))?;
        String::from_utf8(bytes).map_err(io::Error::other)
    }

    fn enumerate_files(&self, _directory: &Path, _pattern: &str) -> io::Result<Vec<PathBuf>> {
        Err(io::Error::other("RecoveryRetry must not list files"))
    }

    fn enumerate_directories(&self, _directory: &Path) -> io::Result<Vec<PathBuf>> {
        Err(io::Error::other("RecoveryRetry must not list folders"))
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        let text = path.to_string_lossy().into_owned();
        self.log(format!("mkdir {text}"), &text);
        self.add_dirs(&text);
        Ok(())
    }
}

/// The journal's append stream: writes into the world's file at `path` (creating it with the C# `MinValue` stamp).
struct Appender {
    fs: ScriptedFs,
    path: String,
}

impl Write for Appender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut world = self.fs.inner.world();
        let entry = world.files.entry(key(&self.path)).or_insert_with(|| FileRec {
            path: self.path.clone(),
            data: Vec::new(),
            last_write: DateTimeUtc::MIN,
        });
        entry.data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AppendStream for Appender {}

// ---------------------------------------------------------------------------------------------------------------
// The scripted Recycle Bin
// ---------------------------------------------------------------------------------------------------------------

/// What `try_restore` does (golden `bin.restore`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    /// Restores the matching item (size and last write) and answers `true`; `false` when nothing matches.
    Ok,
    /// Answers `false` and leaves a foreign file at the original path (golden `refuse-leave-file`).
    RefuseLeaveFile,
    /// Answers `true` without putting anything back (golden `ok-no-file`).
    OkNoFile,
    /// The C# test bin: when `recreate`, puts a zero-filled file of the expected size and last write at the original
    /// path (C# `RestoreRecreatesFile`); then answers `answer` (C# `RestoreResult`). No binned item is needed.
    Synthetic { recreate: bool, answer: bool },
}

/// The scripted bin's configuration (golden `bin` object).
#[derive(Debug, Clone)]
pub struct BinConfig {
    /// Paths for which `can_recycle` is false (exact, case-insensitive).
    pub no_bin: Vec<String>,
    /// `fits_in_recycle_bin` answer when `capacity` is `None`.
    pub fits: bool,
    /// Capacity in bytes: `fits_in_recycle_bin` answers `size <= capacity` (C# `Capacity`). `None` = use `fits`.
    pub capacity: Option<u64>,
    /// `try_restore` behaviour.
    pub restore: RestoreMode,
    /// `send_to_recycle_bin` succeeds without removing the file.
    pub recycle_noop: bool,
}

impl Default for BinConfig {
    fn default() -> Self {
        Self {
            no_bin: Vec::new(),
            fits: true,
            capacity: None,
            restore: RestoreMode::Ok,
            recycle_noop: false,
        }
    }
}

/// The recycle bin double over [`ScriptedFs`]: never the real bin (AGENTS.md "Recycle Bin").
pub struct ScriptedBin {
    fs: ScriptedFs,
    config: Mutex<BinConfig>,
}

impl ScriptedBin {
    pub fn new(fs: ScriptedFs, config: BinConfig) -> Self {
        Self {
            fs,
            config: Mutex::new(config),
        }
    }

    /// Changes the configuration while a test runs (C# `bin.NoBin.Clear()` inside a hook: the drive gains a bin).
    pub fn update(&self, change: impl FnOnce(&mut BinConfig)) {
        change(&mut self.config.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn config(&self) -> BinConfig {
        self.config.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl RecycleBin for ScriptedBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        let text = path.to_string_lossy().into_owned();
        self.fs.log(format!("bin.recycle {text}"), &text);
        self.fs.fire("recycle", Some(path), None)?;
        if self.config().recycle_noop {
            return Ok(());
        }
        let removed = self.fs.inner.world().files.remove(&key(&text));
        let Some(file) = removed else {
            return Err(not_found(&text));
        };
        self.fs
            .inner
            .world()
            .binned
            .push((file.path, file.data, file.last_write));
        Ok(())
    }

    fn can_recycle(&self, path: &Path) -> bool {
        let text = path.to_string_lossy();
        !self.config().no_bin.iter().any(|p| p.eq_ignore_ascii_case(&text))
    }

    fn delete_permanently(&self, path: &Path) -> io::Result<()> {
        let text = path.to_string_lossy().into_owned();
        self.fs.log(format!("bin.delete-permanently {text}"), &text);
        self.fs.inner.world().files.remove(&key(&text));
        Ok(())
    }

    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        let text = original_path.to_string_lossy().into_owned();
        self.fs.log(format!("bin.restore {text}"), &text);
        self.fs.fire("restore", Some(original_path), None)?;
        match self.config().restore {
            RestoreMode::OkNoFile => Ok(true),
            RestoreMode::Synthetic { recreate, answer } => {
                if recreate {
                    self.fs
                        .add_file(&text, zeros(expected_size as usize), expected_last_write_utc);
                }
                Ok(answer)
            }
            RestoreMode::RefuseLeaveFile => {
                let mut world = self.fs.inner.world();
                world.files.entry(key(&text)).or_insert_with(|| FileRec {
                    path: text.clone(),
                    data: zeros(expected_size as usize),
                    last_write: expected_last_write_utc,
                });
                Ok(false)
            }
            RestoreMode::Ok => {
                let mut world = self.fs.inner.world();
                if world.files.contains_key(&key(&text)) {
                    return Ok(false);
                }
                let index = world.binned.iter().rposition(|(p, d, w)| {
                    p.eq_ignore_ascii_case(&text) && d.len() as u64 == expected_size && *w == expected_last_write_utc
                });
                let Some(index) = index else {
                    return Ok(false);
                };
                let (path, data, last_write) = world.binned.remove(index);
                world.files.insert(key(&text), FileRec { path, data, last_write });
                Ok(true)
            }
        }
    }

    fn fits_in_recycle_bin(&self, path: &Path, file_size: u64) -> bool {
        let text = path.to_string_lossy().into_owned();
        self.fs.log(format!("bin.fits {text} {file_size}"), &text);
        let config = self.config();
        match config.capacity {
            Some(capacity) => file_size <= capacity,
            None => config.fits,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------------------------------------------

/// The `FileActionGate` double: `try_begin` is a compare-exchange 0 -> 1, `end` stores 0 (C# `TryBegin/End/IsBusy`).
#[derive(Default)]
pub struct TestGate {
    busy: AtomicBool,
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
// The rig: journal + disk + clocks, as the C# test constructor builds them
// ---------------------------------------------------------------------------------------------------------------

/// One test's world: the scripted disk, the journal over it, the two fixed clocks and the cancellation token.
pub struct Rig {
    pub cancel: CancelToken,
    pub fs: ScriptedFs,
    pub journal: Arc<OperationJournal>,
    /// C# `_clock`: the clock the service and its transaction read (`service_clock_ticks` in the golden).
    pub service_clock: Arc<FakeClock>,
    /// The journal's own clock (`journal_clock_ticks` in the golden).
    pub journal_clock: Arc<FakeClock>,
}

impl Rig {
    /// Both clocks at the C# test time (2026-09-18 12:00 UTC).
    pub fn new() -> Self {
        Self::with_clocks(CSHARP_CLOCK_TICKS, CSHARP_CLOCK_TICKS)
    }

    /// Two fixed clocks (golden: `journal_clock_ticks`, `service_clock_ticks`).
    pub fn with_clocks(journal_ticks: i64, service_ticks: i64) -> Self {
        let cancel = CancelToken::new();
        let fs = ScriptedFs::new(cancel.clone());
        let paths = journal_paths();
        let journal_clock = Arc::new(FakeClock::at(ts(journal_ticks)));
        let service_clock = Arc::new(FakeClock::at(ts(service_ticks)));
        let fs_dyn: Arc<dyn FileSystem> = Arc::new(fs.clone());
        let journal_clock_dyn: Arc<dyn Clock> = journal_clock.clone();
        let journal = Arc::new(OperationJournal::with_options(
            &paths,
            fs_dyn,
            journal_clock_dyn,
            OperationJournalOptions::default(),
        ));
        Self {
            cancel,
            fs,
            journal,
            service_clock,
            journal_clock,
        }
    }

    /// The service over this rig with the English texts and no optional parts (C# `new RecoveryRetryService(...)`).
    pub fn service(&self) -> RecoveryRetryService {
        let clock_dyn: Arc<dyn Clock> = self.service_clock.clone();
        RecoveryRetryService::new(self.journal.clone(), self.fs_dyn(), clock_dyn, english())
    }

    /// A scripted bin over this rig's disk.
    pub fn bin(&self, config: BinConfig) -> Arc<ScriptedBin> {
        Arc::new(ScriptedBin::new(self.fs.clone(), config))
    }

    /// `allowPermanentDelete` answering `value`.
    pub fn allow(value: bool) -> AllowPermanentDelete {
        Arc::new(move || value)
    }

    /// The disk as the `Arc<dyn FileSystem>` the production types take.
    pub fn fs_dyn(&self) -> Arc<dyn FileSystem> {
        Arc::new(self.fs.clone())
    }

    /// Appends a journal line as the test setup does (C# `_journal.Append`): not counted by the hook rules.
    pub fn append(&self, entry: &JournalEntry) {
        let was_counting = self.fs.set_counting(false);
        let result = self.journal.append(entry);
        self.fs.set_counting(was_counting);
        result.expect("seed append succeeds");
    }

    /// Every journal entry in file order (C# `ReadAll`).
    pub fn entries(&self) -> Vec<JournalEntry> {
        let mut out = Vec::new();
        self.journal
            .read_entries(&mut |entry| out.push(entry))
            .expect("journal reads back");
        out
    }

    /// The latest Failed entry per Id (C# `ReadFailedOperations`).
    pub fn failed(&self) -> Vec<JournalEntry> {
        self.journal
            .read_pending_and_failed_operations()
            .expect("journal reads back")
            .into_iter()
            .filter(|e| e.state == JournalState::Failed)
            .collect()
    }

    /// The latest Prepared entry per Id (C# `ReadPendingOperations`).
    pub fn pending(&self) -> Vec<JournalEntry> {
        self.journal
            .read_pending_and_failed_operations()
            .expect("journal reads back")
            .into_iter()
            .filter(|e| e.state == JournalState::Prepared)
            .collect()
    }

    /// The committed moves (C# `ReadCommittedMoves`).
    pub fn committed(&self) -> Vec<JournalEntry> {
        self.journal.read_committed_moves().expect("journal reads back")
    }

    /// Whether the journal holds an entry for `id` in any state other than the seed.
    pub fn is_live(&self, id: &str) -> bool {
        self.journal.live_operations().is_live(id)
    }
}

impl Default for Rig {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of the stat of a seeded path (C# `_fs.GetFileStat(path)!`).
pub fn stat_of(fs: &ScriptedFs, path: &str) -> FileStat {
    fs.stat(path).unwrap_or_else(|| panic!("no file {path}"))
}

/// A fresh [`TestGate`] (the test keeps one handle and passes a clone to the service).
pub fn new_gate() -> Arc<TestGate> {
    Arc::new(TestGate::default())
}
