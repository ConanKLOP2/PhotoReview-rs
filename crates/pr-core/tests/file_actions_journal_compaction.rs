//! Tests for `OperationJournal::try_compact` (F-16, D-14; port of `PhotoReview.Core/FileActions/OperationJournal.cs` TryCompact
//! and the C# tests `FileActions/JournalCompactionTests.cs`, `OperationJournalCompactionBoundaryTests.cs`,
//! `JournalCompactionFaultInjectionTests.cs`, `JournalGroupDowngradeTests.cs` (the round trip only) and the `journal-compact`
//! golden). Red until the W3-01 bodies land. Native tests use real files under a [`TempRoot`] with
//! `PhysicalJournalCompactionFiles` and are `#[ignore]`d: run them with `cargo test -- --include-ignored` on Windows.
//! Nothing here touches the Recycle Bin.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use pr_core::abstractions::{Clock, FileSystem, JournalCompactionFiles, ReadSeek, StagedReplacement, SystemClock};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{
    COMPACTION_FILE_SUFFIX, COMPACTION_THRESHOLD_BYTES, JournalCompactionOutcome, JournalCompactionResult,
    OperationJournal, OperationJournalOptions, STALE_COMPACTION_FILE_AGE_TICKS,
};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::io::journal_compaction_files::PhysicalJournalCompactionFiles;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::golden::{self, i64_of, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, FaultInjectionFileSystem, FaultKind, FaultPlan, FsOp, TempRoot};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

/// C# `BaseTime` of the compaction tests: `2026-09-25T00:00:00Z`.
fn base() -> DateTimeUtc {
    DateTimeUtc::parse_json("2026-09-25T00:00:00Z")
        .expect("base stamp parses")
        .0
}

fn at(seconds: i64) -> DateTimeUtc {
    base()
        .checked_add_ticks(seconds * TICKS_PER_SECOND)
        .expect("stamp in range")
}

/// C# `PathPadding = new('x', 80)`.
fn path_padding() -> String {
    "x".repeat(80)
}

fn id_of(i: usize) -> String {
    format!("j{i:06}")
}

/// C# `PreparedEntry(i, timestamp)`.
fn prepared_entry(i: usize, timestamp: DateTimeUtc) -> JournalEntry {
    let id = id_of(i);
    let padding = path_padding();
    JournalEntry {
        id: id.clone(),
        op_type: FileOperationType::Move,
        state: JournalState::Prepared,
        source: format!(r"C:\photos\folder-{}\{padding}\{id}.jpg", i % 50),
        destination: Some(format!(r"C:\photos\sel\folder-{}\{padding}\{id}.jpg", i % 50)),
        size: 12345,
        last_write_utc: JournalDateTime::utc(timestamp),
        timestamp_utc: JournalDateTime::utc(timestamp),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// C# `LineBytes(entry)`: `JsonSerializer.Serialize(entry) + "\r\n"`.
fn line_bytes(entry: &JournalEntry) -> Vec<u8> {
    format!("{}\r\n", entry.to_line()).into_bytes()
}

/// C# `BuildCompactibleJournal(count)`: Prepared then Committed pairs (the Prepared half is droppable).
fn build_compactible(count: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..count {
        let prepared = prepared_entry(i, at(i as i64 * 2));
        let committed = JournalEntry {
            state: JournalState::Committed,
            timestamp_utc: JournalDateTime::utc(at(i as i64 * 2 + 1)),
            ..prepared.clone()
        };
        out.extend(line_bytes(&prepared));
        out.extend(line_bytes(&committed));
    }
    out
}

/// C# `BuildUncompactibleJournal(count)`: Prepared-only lines, nothing droppable.
fn build_uncompactible(count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|i| line_bytes(&prepared_entry(i, at(i as i64))))
        .collect()
}

fn fnv64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex byte"))
        .collect()
}

fn spec_bytes(spec: &Value) -> Vec<u8> {
    if let Some(hex) = spec.get("hex").and_then(Value::as_str) {
        return unhex(hex);
    }
    let eol = spec.get("eol").and_then(Value::as_str).unwrap_or("\r\n");
    let text = spec.get("text").and_then(Value::as_str).expect("text spec");
    format!("{text}{eol}").into_bytes()
}

fn specs_bytes(specs: &Value) -> Vec<u8> {
    specs
        .as_array()
        .map(|list| list.iter().flat_map(spec_bytes).collect())
        .unwrap_or_default()
}

/// `W301Build` of the oracle (generated journal: `segments`, then `pad_to`). See `file_actions_journal.rs` for the spec.
fn build_generated(case: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    for seg in case["segments"].as_array().expect("segments") {
        if let Some(range) = seg.get("entries").and_then(Value::as_array) {
            let from = range[0].as_i64().expect("from");
            let to = range[1].as_i64().expect("to");
            let every = seg.get("move_every").and_then(Value::as_i64);
            let fixed_type = match seg.get("type").and_then(Value::as_str).unwrap_or("Move") {
                "Copy" => FileOperationType::Copy,
                "Recycle" => FileOperationType::Recycle,
                _ => FileOperationType::Move,
            };
            let states: Vec<JournalState> = match seg.get("states").and_then(Value::as_array) {
                Some(list) => list.iter().map(|s| parse_state(s.as_str().expect("state"))).collect(),
                None => vec![JournalState::Committed],
            };
            let pad = seg.get("pad").and_then(Value::as_i64).unwrap_or(0);
            let eol = seg.get("eol").and_then(Value::as_str).unwrap_or("\r\n");
            let folder = if pad > 0 {
                format!("{}\\", "x".repeat(usize::try_from(pad).expect("pad")))
            } else {
                String::new()
            };
            for i in from..to {
                let op_type = match every {
                    Some(k) if i % k == 0 => FileOperationType::Move,
                    Some(_) => FileOperationType::Recycle,
                    None => fixed_type,
                };
                let id = format!("e{i:06}");
                for (si, state) in states.iter().enumerate() {
                    let destination = if op_type == FileOperationType::Recycle {
                        None
                    } else {
                        Some(format!(r"C:\photos\sel\{id}.jpg"))
                    };
                    let line = JournalEntry {
                        id: id.clone(),
                        op_type,
                        state: *state,
                        source: format!(r"C:\photos\{folder}{id}.jpg"),
                        destination,
                        size: 1,
                        last_write_utc: JournalDateTime::utc(base()),
                        timestamp_utc: JournalDateTime::utc(at(i * 10 + si as i64)),
                        error: None,
                        error_code: None,
                        permanent: None,
                        undo: None,
                        group_id: None,
                        group_members: None,
                    };
                    out.extend_from_slice(format!("{}{eol}", line.to_line()).as_bytes());
                }
            }
        } else if let Some(repeat) = seg.get("repeat_text").and_then(Value::as_str) {
            let count = usize::try_from(i64_of(seg, "count")).expect("count");
            out.extend_from_slice(repeat.repeat(count).as_bytes());
            out.extend_from_slice(seg.get("eol").and_then(Value::as_str).unwrap_or("").as_bytes());
        } else if let Some(long) = seg.get("long_move").and_then(Value::as_i64) {
            let id = seg.get("id").and_then(Value::as_str).unwrap_or("big");
            let line = JournalEntry {
                id: id.to_owned(),
                op_type: FileOperationType::Move,
                state: JournalState::Committed,
                source: format!(r"C:\photos\{}.jpg", "a".repeat(usize::try_from(long).expect("long"))),
                destination: Some(format!(r"C:\sel\{id}.jpg")),
                size: 4,
                last_write_utc: JournalDateTime::utc(base()),
                timestamp_utc: JournalDateTime::utc(base()),
                error: None,
                error_code: None,
                permanent: None,
                undo: None,
                group_id: None,
                group_members: None,
            };
            out.extend_from_slice(
                format!(
                    "{}{}",
                    line.to_line(),
                    seg.get("eol").and_then(Value::as_str).unwrap_or("")
                )
                .as_bytes(),
            );
        } else {
            out.extend_from_slice(&spec_bytes(seg));
        }
    }
    match case.get("pad_to").and_then(Value::as_i64) {
        None => out,
        Some(pad_to) => {
            let total = usize::try_from(pad_to).expect("pad_to");
            assert!(total >= out.len() + 3, "pad_to is too small for the segments");
            let pad = total - out.len();
            let mut all = vec![b'x'; pad];
            all[pad - 2] = b'\r';
            all[pad - 1] = b'\n';
            all.extend_from_slice(&out);
            all
        }
    }
}

fn parse_state(text: &str) -> JournalState {
    match text {
        "Prepared" => JournalState::Prepared,
        "Committed" => JournalState::Committed,
        "Failed" => JournalState::Failed,
        "Dismissed" => JournalState::Dismissed,
        other => panic!("unknown state {other}"),
    }
}

/// Lines of the journal `bytes` that `journal_line::try_parse` accepts (for committed-Move reads through a fresh journal).
fn moves_of(journal: &OperationJournal) -> Vec<JournalEntry> {
    journal.read_committed_moves().expect("read committed moves")
}

fn fresh_journal(paths: &AppPaths, fs: Arc<dyn FileSystem>, clock: Arc<dyn Clock>) -> OperationJournal {
    OperationJournal::new(paths, fs, clock)
}

fn physical() -> Arc<dyn FileSystem> {
    Arc::new(PhysicalFileSystem)
}

fn system_clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

/// The `journal_file` of the native compaction tests: `<TempRoot>\PhotoReviewRs\Data\operations.jsonl`.
fn native_paths(root: &TempRoot) -> AppPaths {
    AppPaths::new(root.path(), None, false)
}

/// Creates the data folder and writes `bytes` as the journal.
fn write_journal(paths: &AppPaths, bytes: &[u8]) {
    std::fs::create_dir_all(paths.journal_file.parent().expect("parent")).expect("create data folder");
    std::fs::write(&paths.journal_file, bytes).expect("write journal");
}

/// Count of `*.compact.tmp` files in the journal folder.
fn leftover_count(paths: &AppPaths) -> usize {
    let dir = paths.journal_file.parent().expect("parent");
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(COMPACTION_FILE_SUFFIX))
                .count()
        })
        .unwrap_or(0)
}

/// Hooks at the two moments the compaction protocol documents (C# `HookedCompactionFiles`).
#[derive(Default)]
struct Hooks {
    /// Runs at the very start of `open_read_deny_writers`, before delegating.
    before_lock: Option<Box<dyn Fn() + Send + Sync>>,
    /// Runs just before the staged replacement is renamed over the journal.
    before_replace: Option<Box<dyn Fn() + Send + Sync>>,
    /// When true, `replace_atomically` fails instead of delegating (a simulated crash).
    throw_instead_of_replace: bool,
}

/// C# `HookedCompactionFiles`: decorates any [`JournalCompactionFiles`] with the [`Hooks`].
struct HookedCompactionFiles {
    inner: Arc<dyn JournalCompactionFiles>,
    hooks: Arc<Hooks>,
}

impl HookedCompactionFiles {
    fn wrap(inner: Arc<dyn JournalCompactionFiles>, hooks: Hooks) -> Arc<dyn JournalCompactionFiles> {
        Arc::new(Self {
            inner,
            hooks: Arc::new(hooks),
        })
    }
}

impl JournalCompactionFiles for HookedCompactionFiles {
    fn open_read_deny_writers(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        if let Some(hook) = &self.hooks.before_lock {
            hook();
        }
        self.inner.open_read_deny_writers(path)
    }

    fn create_staged_replacement(&self, temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>> {
        Ok(Box::new(HookedStaged {
            inner: self.inner.create_staged_replacement(temp_path)?,
            hooks: self.hooks.clone(),
        }))
    }
}

struct HookedStaged {
    inner: Box<dyn StagedReplacement>,
    hooks: Arc<Hooks>,
}

impl StagedReplacement for HookedStaged {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write(bytes)
    }

    fn flush_to_disk(&mut self) -> io::Result<()> {
        self.inner.flush_to_disk()
    }

    fn replace_atomically(&mut self, destination: &Path) -> io::Result<()> {
        if let Some(hook) = &self.hooks.before_replace {
            hook();
        }
        if self.hooks.throw_instead_of_replace {
            return Err(io::Error::other("simulated crash before replace"));
        }
        self.inner.replace_atomically(destination)
    }
}

/// The in-memory compaction primitives over a [`FakeFileSystem`] (C# `MemoryCompactionFiles`).
struct MemoryCompactionFiles {
    fs: Arc<FakeFileSystem>,
}

impl JournalCompactionFiles for MemoryCompactionFiles {
    fn open_read_deny_writers(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        self.fs.open_read_shared(path, 65536)
    }

    fn create_staged_replacement(&self, _temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>> {
        Ok(Box::new(MemoryStaged {
            fs: self.fs.clone(),
            bytes: Vec::new(),
        }))
    }
}

struct MemoryStaged {
    fs: Arc<FakeFileSystem>,
    bytes: Vec<u8>,
}

impl StagedReplacement for MemoryStaged {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn flush_to_disk(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn replace_atomically(&mut self, destination: &Path) -> io::Result<()> {
        self.fs.add_file(destination, &self.bytes);
        Ok(())
    }
}

/// A `FileSystem` view of the fake that the boundary tests build their journal on.
fn memory_fs() -> (Arc<FakeFileSystem>, Arc<FakeClock>) {
    let clock = Arc::new(FakeClock::at(at(86_400)));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    (fs, clock)
}

fn fake_paths() -> AppPaths {
    AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false)
}

/// Boundary-test entry: a Copy with stamps at `base()` and size 12345.
fn boundary_entry(i: usize, state: JournalState) -> JournalEntry {
    JournalEntry {
        id: format!("j{i:06}"),
        op_type: FileOperationType::Copy,
        state,
        source: format!(r"C:\photos\{i}.jpg"),
        destination: Some(format!(r"C:\sel\{i}.jpg")),
        size: 12345,
        last_write_utc: JournalDateTime::utc(base()),
        timestamp_utc: JournalDateTime::utc(base()),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// `count` Prepared+Committed pairs, then one blank line: exactly `total` bytes (C# `CompactibleJournal`).
fn compactible_exact(total: usize) -> (Vec<u8>, usize) {
    let mut parts: Vec<u8> = Vec::new();
    let mut pairs = 0;
    loop {
        let mut pair = line_bytes(&boundary_entry(pairs, JournalState::Prepared));
        pair.extend(line_bytes(&boundary_entry(pairs, JournalState::Committed)));
        if parts.len() + pair.len() + 2 > total {
            break;
        }
        parts.extend(pair);
        pairs += 1;
    }
    let rest = total - parts.len();
    parts.extend(std::iter::repeat_n(b' ', rest - 2));
    parts.extend_from_slice(b"\r\n");
    (parts, pairs)
}

const ONE_MIB: usize = 1024 * 1024;

fn journal_bytes(fs: &FakeFileSystem) -> Vec<u8> {
    fs.file_content(fake_paths().journal_file).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------
// JournalCompactionTests (native, PhysicalFileSystem + PhysicalJournalCompactionFiles + the hooking decorator).
// ---------------------------------------------------------------------------------------------------------------

const COMPACTIBLE: usize = 9000;
const UNCOMPACTIBLE: usize = 6000;

// C#: JournalCompactionTests.TryCompact_NoCompactionFiles_NotSupported
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_no_compaction_files_not_supported() {
    let root = TempRoot::new("compact-no-files");
    let paths = native_paths(&root);
    let bytes = build_compactible(COMPACTIBLE);
    write_journal(&paths, &bytes);
    let journal = fresh_journal(&paths, physical(), system_clock());

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::NotSupported);
    assert_eq!(std::fs::read(&paths.journal_file).expect("read"), bytes);
}

// C#: JournalCompactionTests.TryCompact_SmallJournal_BelowThreshold
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_small_journal_below_threshold() {
    let root = TempRoot::new("compact-small");
    let paths = native_paths(&root);
    let bytes = build_compactible(5);
    write_journal(&paths, &bytes);
    let journal = OperationJournal::with_options(
        &paths,
        physical(),
        system_clock(),
        OperationJournalOptions {
            compaction_files: Some(HookedCompactionFiles::wrap(
                Arc::new(PhysicalJournalCompactionFiles),
                Hooks::default(),
            )),
            ..OperationJournalOptions::default()
        },
    );

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::BelowThreshold);
    assert_eq!(std::fs::read(&paths.journal_file).expect("read"), bytes);
}

// C#: JournalCompactionTests.TryCompact_LowDroppableShare_NotWorthIt
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_low_droppable_share_not_worth_it() {
    let root = TempRoot::new("compact-not-worth");
    let paths = native_paths(&root);
    let bytes = build_uncompactible(UNCOMPACTIBLE);
    assert!(bytes.len() as u64 >= COMPACTION_THRESHOLD_BYTES);
    write_journal(&paths, &bytes);
    let journal = OperationJournal::with_options(
        &paths,
        physical(),
        system_clock(),
        OperationJournalOptions {
            compaction_files: Some(HookedCompactionFiles::wrap(
                Arc::new(PhysicalJournalCompactionFiles),
                Hooks::default(),
            )),
            ..OperationJournalOptions::default()
        },
    );

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::NotWorthIt);
    assert_eq!(std::fs::read(&paths.journal_file).expect("read"), bytes);
}

/// A native journal with the hooking decorator over the real primitives.
fn native_journal(paths: &AppPaths, hooks: Hooks) -> OperationJournal {
    OperationJournal::with_options(
        paths,
        physical(),
        system_clock(),
        OperationJournalOptions {
            compaction_files: Some(HookedCompactionFiles::wrap(
                Arc::new(PhysicalJournalCompactionFiles),
                hooks,
            )),
            ..OperationJournalOptions::default()
        },
    )
}

// C#: JournalCompactionTests.TryCompact_BigJournal_Compacted
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_big_journal_compacted() {
    let root = TempRoot::new("compact-big");
    let paths = native_paths(&root);
    let bytes = build_compactible(COMPACTIBLE);
    assert!(bytes.len() as u64 >= COMPACTION_THRESHOLD_BYTES);
    write_journal(&paths, &bytes);
    let before = fresh_journal(&paths, physical(), system_clock());
    let before_pending = before.read_pending_and_failed_operations().expect("read");
    let before_moves = moves_of(&before);

    let journal = native_journal(&paths, Hooks::default());
    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    let new_bytes = std::fs::read(&paths.journal_file).expect("read");
    assert!((new_bytes.len() as i64) < bytes.len() as i64);
    assert!(
        new_bytes.len() as u64 >= COMPACTION_THRESHOLD_BYTES,
        "the compacted file must stay >= 1 MiB for the tail-window check"
    );
    assert_eq!(result.bytes_after, new_bytes.len() as i64);
    assert_eq!(result.bytes_before, bytes.len() as i64);

    let after = fresh_journal(&paths, physical(), system_clock());
    assert_eq!(
        after.read_pending_and_failed_operations().expect("read"),
        before_pending
    );
    assert_eq!(moves_of(&after), before_moves);
    assert_eq!(leftover_count(&paths), 0);
}

// C#: JournalCompactionTests.TryCompact_ConcurrentAppendBetweenSnapshotAndLock_SurvivesOnce
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_concurrent_append_between_snapshot_and_lock_survives_once() {
    let root = TempRoot::new("compact-concurrent");
    let paths = native_paths(&root);
    write_journal(&paths, &build_compactible(COMPACTIBLE));
    let other = Arc::new(fresh_journal(&paths, physical(), system_clock())); // "another process" on the same path
    let late = prepared_entry(999_999, at(86_400));
    let hooks = Hooks {
        before_lock: Some(Box::new({
            let other = other.clone();
            let late = late.clone();
            move || other.append(&late).expect("late append")
        })),
        ..Hooks::default()
    };
    let journal = native_journal(&paths, hooks);

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    let after = fresh_journal(&paths, physical(), system_clock());
    let pending: Vec<JournalEntry> = after
        .read_pending_and_failed_operations()
        .expect("read")
        .into_iter()
        .filter(|e| e.id == late.id)
        .collect();
    assert_eq!(pending, vec![late.clone()]);
    let text = std::fs::read_to_string(&paths.journal_file).expect("read");
    let lines_with_id = text
        .lines()
        .filter(|l| l.contains(&format!("\"Id\":\"{}\"", late.id)))
        .count();
    assert_eq!(lines_with_id, 1);
}

// C#: JournalCompactionTests.TryCompact_TornTailInDelta_RepairedAndNotGlued
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_torn_tail_in_delta_repaired_and_not_glued() {
    let root = TempRoot::new("compact-torn-delta");
    let paths = native_paths(&root);
    write_journal(&paths, &build_compactible(COMPACTIBLE));
    let torn_path = paths.journal_file.clone();
    let hooks = Hooks {
        before_lock: Some(Box::new(move || {
            // A torn record (no trailing newline) appended by another writer right before the lock.
            let mut stream = PhysicalFileSystem.open_append(&torn_path, false).expect("open append");
            let torn = prepared_entry(888_888, at(86_400 + 1)).to_line();
            stream.write_all(torn.as_bytes()).expect("torn write");
            stream.flush().expect("flush");
        })),
        ..Hooks::default()
    };
    let journal = native_journal(&paths, hooks);

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    let bytes = std::fs::read(&paths.journal_file).expect("read");
    assert_eq!(bytes.last(), Some(&b'\n'));

    let next = fresh_journal(&paths, physical(), system_clock());
    let new_entry = prepared_entry(777_777, at(86_400 + 2));
    next.append(&new_entry).expect("append");

    // The torn line's Id and the new Id are each readable on their own line; neither glued onto the other.
    let text = std::fs::read_to_string(&paths.journal_file).expect("read");
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let torn_line = lines
        .iter()
        .find(|l| l.contains(&id_of(888_888)))
        .expect("torn line present");
    assert!(!torn_line.contains(&id_of(777_777)));
    let new_line = lines
        .iter()
        .find(|l| l.contains(&id_of(777_777)))
        .expect("new line present");
    assert!(!new_line.contains(&id_of(888_888)));
    let pending = next.read_pending_and_failed_operations().expect("read");
    assert!(pending.iter().any(|e| e.id == new_entry.id));
    assert!(pending.iter().any(|e| e.id == id_of(888_888)));
}

// C#: JournalCompactionTests.TryCompact_WritersExcludedDuringCriticalSection_AllowedAfter
#[test]
#[ignore = "native: real files under TempRoot"]
#[cfg(windows)]
fn try_compact_writers_excluded_during_critical_section_allowed_after() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = TempRoot::new("compact-excluded");
    let paths = native_paths(&root);
    write_journal(&paths, &build_compactible(COMPACTIBLE));
    let observed: Arc<Mutex<Option<io::Error>>> = Arc::new(Mutex::new(None));
    let path = paths.journal_file.clone();
    let slot = observed.clone();
    let hooks = Hooks {
        before_replace: Some(Box::new(move || {
            // The deny-writers stream is still open: an append must fail with a sharing violation (0x80070020).
            if let Err(err) = std::fs::OpenOptions::new().append(true).share_mode(1).open(&path) {
                *slot.lock().expect("slot") = Some(err);
            }
        })),
        ..Hooks::default()
    };
    let journal = native_journal(&paths, hooks);

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    let err = observed
        .lock()
        .expect("slot")
        .take()
        .expect("the append was refused while compacting");
    assert_eq!(
        err.raw_os_error(),
        Some(32),
        "ERROR_SHARING_VIOLATION (HRESULT 0x80070020)"
    );
    // Afterwards writers are allowed again.
    let _again = std::fs::OpenOptions::new()
        .append(true)
        .share_mode(1)
        .open(&paths.journal_file)
        .expect("writable again");
}

// C#: JournalCompactionTests.TryCompact_ReaderHoldingJournalOpen_NotBlockedSeesOldBytes
#[test]
#[ignore = "native: real files under TempRoot"]
#[cfg(windows)]
fn try_compact_reader_holding_journal_open_not_blocked_sees_old_bytes() {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    let root = TempRoot::new("compact-reader");
    let paths = native_paths(&root);
    let original = build_compactible(COMPACTIBLE);
    write_journal(&paths, &original);
    // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
    let mut reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(&paths.journal_file)
        .expect("reader");
    let mut before = vec![0u8; original.len()];
    reader.read_exact(&mut before).expect("read before");

    let journal = native_journal(&paths, Hooks::default());
    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    let mut after = Vec::new();
    std::io::Seek::seek(&mut reader, std::io::SeekFrom::Start(0)).expect("seek");
    reader.read_to_end(&mut after).expect("read after");
    assert_eq!(after, original);
    assert_eq!(before, original);
}

// C#: JournalCompactionTests.TryCompact_CrashBeforeReplace_JournalUnchangedNoTempLeft
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_crash_before_replace_journal_unchanged_no_temp_left() {
    let root = TempRoot::new("compact-crash");
    let paths = native_paths(&root);
    let bytes = build_compactible(COMPACTIBLE);
    write_journal(&paths, &bytes);
    let journal = native_journal(
        &paths,
        Hooks {
            throw_instead_of_replace: true,
            ..Hooks::default()
        },
    );

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Failed);
    assert_eq!(std::fs::read(&paths.journal_file).expect("read"), bytes);
    assert_eq!(leftover_count(&paths), 0);
}

// C#: JournalCompactionTests.TryCompact_StaleLeftoverRemoved_FreshLeftoverKept
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_stale_leftover_removed_fresh_leftover_kept() {
    let root = TempRoot::new("compact-stale");
    let paths = native_paths(&root);
    write_journal(&paths, &build_compactible(COMPACTIBLE));
    let dir = paths.journal_file.parent().expect("parent").to_path_buf();
    let stale = dir.join(format!("operations.jsonl.{}{COMPACTION_FILE_SUFFIX}", "1".repeat(32)));
    let fresh = dir.join(format!("operations.jsonl.{}{COMPACTION_FILE_SUFFIX}", "2".repeat(32)));
    std::fs::write(&stale, "leftover").expect("stale");
    std::fs::write(&fresh, "leftover").expect("fresh");
    let now = SystemTime::now();
    let stale_age =
        Duration::from_secs((STALE_COMPACTION_FILE_AGE_TICKS / TICKS_PER_SECOND) as u64) + Duration::from_secs(5 * 60);
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .expect("open stale")
        .set_modified(now - stale_age)
        .expect("age stale");
    std::fs::File::options()
        .write(true)
        .open(&fresh)
        .expect("open fresh")
        .set_modified(now)
        .expect("fresh time");

    let journal = native_journal(&paths, Hooks::default());
    let _ = journal.try_compact();

    assert!(!stale.exists());
    assert!(fresh.exists());
    std::fs::remove_file(&fresh).expect("cleanup");
}

// C#: JournalCompactionTests.TryCompact_ChangedBetweenSnapshotAndLock_ReportsExactContent
#[test]
#[ignore = "native: real files under TempRoot"]
fn try_compact_changed_between_snapshot_and_lock_reports_exact_content() {
    let root = TempRoot::new("compact-changed");
    let paths = native_paths(&root);
    write_journal(&paths, &build_compactible(COMPACTIBLE));
    let rewritten = build_compactible(10); // smaller: below the threshold, as a real compaction would leave it
    let path = paths.journal_file.clone();
    let bytes_for_hook = rewritten.clone();
    let journal = native_journal(
        &paths,
        Hooks {
            before_lock: Some(Box::new(move || {
                std::fs::write(&path, &bytes_for_hook).expect("rewrite")
            })),
            ..Hooks::default()
        },
    );

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Changed);
    assert_eq!(std::fs::read(&paths.journal_file).expect("read"), rewritten);
    assert_eq!(result.bytes_after, rewritten.len() as i64);
}

// C#: JournalCompactionTests.TryCompact_WriterHoldsJournalWhenLocking_Busy
#[test]
#[ignore = "native: real files under TempRoot"]
#[cfg(windows)]
fn try_compact_writer_holds_journal_when_locking_busy() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = TempRoot::new("compact-busy");
    let paths = native_paths(&root);
    let bytes = build_compactible(COMPACTIBLE);
    write_journal(&paths, &bytes);
    let holder: Arc<Mutex<Option<std::fs::File>>> = Arc::new(Mutex::new(None));
    let path = paths.journal_file.clone();
    let slot = holder.clone();
    let journal = native_journal(
        &paths,
        Hooks {
            before_lock: Some(Box::new(move || {
                let handle = std::fs::OpenOptions::new()
                    .append(true)
                    .share_mode(1)
                    .open(&path)
                    .expect("writer handle");
                *slot.lock().expect("holder") = Some(handle);
            })),
            ..Hooks::default()
        },
    );

    let result = journal.try_compact();
    holder.lock().expect("holder").take();

    assert_eq!(result.outcome, JournalCompactionOutcome::Busy);
    assert_eq!(std::fs::read(&paths.journal_file).expect("read"), bytes);
}

// C#: JournalCompactionTests.TryCompact_StressWithConcurrentWriters_NoTornOrLostRecords (slow: 4 writers, a compactor)
#[test]
#[ignore = "slow"]
fn try_compact_stress_with_concurrent_writers_no_torn_or_lost_records() {
    let root = TempRoot::new("compact-stress");
    let paths = native_paths(&root);
    write_journal(&paths, &build_compactible(COMPACTIBLE));
    let writers = 4usize;
    let per_thread = 60usize;
    let acknowledged: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let stop = std::sync::atomic::AtomicBool::new(false);
    let compactions = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let compactor_paths = paths.clone();
        let stop = &stop;
        let compactions = &compactions;
        scope.spawn(move || {
            let journal = native_journal(&compactor_paths, Hooks::default());
            while !stop.load(Ordering::Relaxed) {
                let _ = journal.try_compact();
                compactions.fetch_add(1, Ordering::Relaxed);
            }
        });
        let mut handles = Vec::new();
        for w in 0..writers {
            let paths = paths.clone();
            let acknowledged = &acknowledged;
            handles.push(scope.spawn(move || {
                let journal = fresh_journal(&paths, physical(), system_clock());
                for i in 0..per_thread {
                    let id = format!("stress-{w}-{i}");
                    let record = JournalEntry {
                        id: id.clone(),
                        op_type: FileOperationType::Copy,
                        state: JournalState::Committed,
                        source: format!(r"C:\photos\{id}.jpg"),
                        destination: Some(format!(r"C:\photos\sel\{id}.jpg")),
                        size: 1,
                        last_write_utc: JournalDateTime::utc(base()),
                        timestamp_utc: JournalDateTime::utc(at(i as i64)),
                        error: None,
                        error_code: None,
                        permanent: None,
                        undo: None,
                        group_id: None,
                        group_members: None,
                    };
                    // Bounded retries exhausted under contention with compaction are allowed: not acknowledged then.
                    if journal.append(&record).is_ok() {
                        acknowledged.lock().expect("acknowledged").push(id);
                    }
                }
            }));
        }
        for handle in handles {
            handle.join().expect("writer thread");
        }
        stop.store(true, Ordering::Relaxed);
    });

    let text = std::fs::read_to_string(&paths.journal_file).expect("read");
    let mut latest: std::collections::HashMap<String, JournalState> = std::collections::HashMap::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let entry = journal_line_parse(line); // a glued or torn line fails here
        latest.insert(entry.id, entry.state);
    }
    let fresh = fresh_journal(&paths, physical(), system_clock());
    let pending: Vec<String> = fresh
        .read_pending_and_failed_operations()
        .expect("read")
        .into_iter()
        .map(|e| e.id)
        .collect();
    let acknowledged = acknowledged.lock().expect("acknowledged").clone();
    for id in &acknowledged {
        assert_eq!(
            latest.get(id),
            Some(&JournalState::Committed),
            "{id} was acknowledged but is missing or not Committed"
        );
        assert!(
            !pending.contains(id),
            "{id} unexpectedly still pending or failed after compaction"
        );
    }
    assert!(
        !acknowledged.is_empty(),
        "guards the loop above against passing vacuously"
    );
}

fn journal_line_parse(line: &str) -> JournalEntry {
    pr_core::file_actions::journal_line::try_parse(line)
        .unwrap_or_else(|| panic!("not an accepted journal line: {line}"))
}

// ---------------------------------------------------------------------------------------------------------------
// OperationJournalCompactionBoundaryTests (in memory, the test-local MemoryCompactionFiles over the fake).
// ---------------------------------------------------------------------------------------------------------------

fn boundary_journal(fs: &Arc<FakeFileSystem>, clock: &Arc<FakeClock>, hooks: Hooks) -> OperationJournal {
    let files: Arc<dyn JournalCompactionFiles> =
        HookedCompactionFiles::wrap(Arc::new(MemoryCompactionFiles { fs: fs.clone() }), hooks);
    OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            compaction_files: Some(files),
            ..OperationJournalOptions::default()
        },
    )
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_JournalOfExactlyTheThreshold_IsCompacted
#[test]
fn try_compact_journal_of_exactly_the_threshold_is_compacted() {
    let (fs, clock) = memory_fs();
    let (bytes, pairs) = compactible_exact(ONE_MIB);
    fs.add_file(fake_paths().journal_file, &bytes);
    let journal = boundary_journal(&fs, &clock, Hooks::default());

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    assert_eq!(result.bytes_before, ONE_MIB as i64);
    let after = journal_bytes(&fs);
    assert_eq!(result.bytes_after, after.len() as i64);
    let pending_and_failed = journal.read_pending_and_failed_operations().expect("read").len();
    let lines_after = after.split(|b| *b == b'\n').filter(|l| !l.is_empty()).count();
    assert_eq!(pairs, pending_and_failed + lines_after);
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_JournalOneByteUnderTheThreshold_IsLeftAlone
#[test]
fn try_compact_journal_one_byte_under_the_threshold_is_left_alone() {
    let (fs, clock) = memory_fs();
    let (bytes, _) = compactible_exact(ONE_MIB - 1);
    fs.add_file(fake_paths().journal_file, &bytes);
    let journal = boundary_journal(&fs, &clock, Hooks::default());

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::BelowThreshold);
    assert_eq!(result.bytes_before, (ONE_MIB - 1) as i64);
    assert_eq!(journal_bytes(&fs), bytes);
}

/// 1 MiB journal: `droppable` bytes of blank line, Prepared-only lines, then one malformed line that is kept.
fn journal_with_droppable(droppable: usize) -> Vec<u8> {
    let mut parts: Vec<u8> = Vec::new();
    parts.extend(std::iter::repeat_n(b' ', droppable - 2));
    parts.extend_from_slice(b"\r\n");
    let kept_target = ONE_MIB - droppable;
    let mut size = 0;
    let mut i = 0;
    loop {
        let line = line_bytes(&boundary_entry(i, JournalState::Prepared));
        if size + line.len() + 3 > kept_target {
            break;
        }
        size += line.len();
        parts.extend(line);
        i += 1;
    }
    let garbage = kept_target - size;
    parts.extend(std::iter::repeat_n(b'x', garbage - 2));
    parts.extend_from_slice(b"\r\n");
    assert_eq!(parts.len(), ONE_MIB);
    parts
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_ExactlyAQuarterDroppable_IsCompacted
#[test]
fn try_compact_exactly_a_quarter_droppable_is_compacted() {
    let (fs, clock) = memory_fs();
    fs.add_file(fake_paths().journal_file, &journal_with_droppable(ONE_MIB / 4));
    let journal = boundary_journal(&fs, &clock, Hooks::default());

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    assert_eq!(result.bytes_after, (ONE_MIB - ONE_MIB / 4) as i64);
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_OneByteLessThanAQuarterDroppable_IsNotWorthIt
#[test]
fn try_compact_one_byte_less_than_a_quarter_droppable_is_not_worth_it() {
    let (fs, clock) = memory_fs();
    let bytes = journal_with_droppable(ONE_MIB / 4 - 1);
    fs.add_file(fake_paths().journal_file, &bytes);
    let journal = boundary_journal(&fs, &clock, Hooks::default());

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::NotWorthIt);
    assert_eq!(journal_bytes(&fs), bytes);
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_LinesAppendedMeanwhile_AreKeptAndBytesAfterCountsThem
#[test]
fn try_compact_lines_appended_meanwhile_are_kept_and_bytes_after_counts_them() {
    let (fs, clock) = memory_fs();
    let (bytes, _) = compactible_exact(ONE_MIB);
    fs.add_file(fake_paths().journal_file, &bytes);
    let other = Arc::new(OperationJournal::new(&fake_paths(), fs.clone(), clock.clone()));
    let late = boundary_entry(900_001, JournalState::Prepared);
    let hooks = Hooks {
        before_lock: Some(Box::new({
            let other = other.clone();
            let late = late.clone();
            move || other.append(&late).expect("append meanwhile")
        })),
        ..Hooks::default()
    };
    let journal = boundary_journal(&fs, &clock, hooks);

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    let after = journal_bytes(&fs);
    assert_eq!(result.bytes_after, after.len() as i64);
    assert_eq!(result.bytes_before, (bytes.len() + line_bytes(&late).len()) as i64);
    let pending = journal.read_pending_and_failed_operations().expect("read");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "j900001");
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_TornLineAppendedMeanwhile_GetsANewlineAndBytesAfterCountsIt
#[test]
fn try_compact_torn_line_appended_meanwhile_gets_a_newline_and_bytes_after_counts_it() {
    const TORN_DELTA: &str = "{\"Id\":\"torn\",\"Ty";
    let (fs, clock) = memory_fs();
    let (bytes, _) = compactible_exact(ONE_MIB);
    fs.add_file(fake_paths().journal_file, &bytes);
    let writer_fs = fs.clone();
    let hooks = Hooks {
        before_lock: Some(Box::new(move || {
            let mut stream = writer_fs
                .open_append(&fake_paths().journal_file, false)
                .expect("open append");
            stream.write_all(TORN_DELTA.as_bytes()).expect("torn write");
            stream.flush().expect("flush");
        })),
        ..Hooks::default()
    };
    let journal = boundary_journal(&fs, &clock, hooks);

    let result = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);
    let after = journal_bytes(&fs);
    assert_eq!(result.bytes_after, after.len() as i64);
    assert_eq!(after.last(), Some(&b'\n'));
    // The delta bytes survive, then the terminator (a newline on the end alone is not enough).
    let tail = &after[after.len() - (TORN_DELTA.len() + 2)..];
    assert_eq!(tail, format!("{TORN_DELTA}\r\n").as_bytes());
}

fn leftover_path(name: &str) -> PathBuf {
    fake_paths()
        .journal_file
        .with_extension(format!("jsonl.{name}{COMPACTION_FILE_SUFFIX}"))
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_LeftoverExactlyAsOldAsTheStaleAge_IsRemoved
#[test]
fn try_compact_leftover_exactly_as_old_as_the_stale_age_is_removed() {
    let (fs, clock) = memory_fs();
    let now = clock.utc_now();
    let stale_age_seconds = STALE_COMPACTION_FILE_AGE_TICKS / TICKS_PER_SECOND;
    fs.add_file_at(
        leftover_path("old"),
        b"partial",
        now.checked_add_ticks(-stale_age_seconds * TICKS_PER_SECOND)
            .expect("in range"),
    );
    fs.add_file_at(
        leftover_path("young"),
        b"partial",
        now.checked_add_ticks(-stale_age_seconds * TICKS_PER_SECOND + TICKS_PER_SECOND)
            .expect("in range"),
    );
    let journal = boundary_journal(&fs, &clock, Hooks::default());

    let _ = journal.try_compact();

    assert!(!fs.file_exists(&leftover_path("old")));
    assert!(fs.file_exists(&leftover_path("young")));
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_LeftoverCannotBeDeleted_IsIgnoredAndTheRunContinues (both exception kinds)
#[test]
fn try_compact_leftover_cannot_be_deleted_is_ignored_and_the_run_continues_io_error() {
    try_compact_leftover_cannot_be_deleted(io::ErrorKind::Other);
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_LeftoverCannotBeDeleted_IsIgnoredAndTheRunContinues (both exception kinds)
#[test]
fn try_compact_leftover_cannot_be_deleted_is_ignored_and_the_run_continues_unauthorized() {
    try_compact_leftover_cannot_be_deleted(io::ErrorKind::PermissionDenied);
}

fn try_compact_leftover_cannot_be_deleted(kind: io::ErrorKind) {
    let (fs, clock) = memory_fs();
    let now = clock.utc_now();
    fs.add_file_at(
        leftover_path("old"),
        b"partial",
        now.checked_add_ticks(-2 * 24 * 3600 * TICKS_PER_SECOND)
            .expect("in range"),
    );
    fs.fail(FsOp::Delete, leftover_path("old"), kind);
    let journal = boundary_journal(&fs, &clock, Hooks::default());

    let result = journal.try_compact();

    assert_eq!(
        result.outcome,
        JournalCompactionOutcome::BelowThreshold,
        "no journal yet: nothing to compact, but not Failed"
    );
}

// C#: OperationJournalCompactionBoundaryTests.TryCompact_JournalFolderDoesNotExist_ReportsBelowThresholdNotFailure
#[test]
fn try_compact_journal_folder_does_not_exist_reports_below_threshold_not_failure() {
    let (fs, clock) = memory_fs();
    let journal = boundary_journal(&fs, &clock, Hooks::default());

    let result: JournalCompactionResult = journal.try_compact();

    assert_eq!(result.outcome, JournalCompactionOutcome::BelowThreshold);
}

// ---------------------------------------------------------------------------------------------------------------
// JournalCompactionFaultInjectionTests: the compaction interrupted at every file operation (pr-test-support's
// FaultInjectionFileSystem; the contract's open question 3 is answered: the injector exists, so this is not ignored).
// ---------------------------------------------------------------------------------------------------------------

/// C# `FaultCompactionFiles`: staging, the fsync and the atomic rename are numbered mutations of the injector.
struct FaultCompactionFiles {
    fs: Arc<FaultInjectionFileSystem>,
}

impl JournalCompactionFiles for FaultCompactionFiles {
    fn open_read_deny_writers(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        self.fs.open_read_shared(path, 65536)
    }

    fn create_staged_replacement(&self, temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>> {
        let disk = self.fs.disk().clone();
        let temp = temp_path.to_path_buf();
        self.fs.mutate(
            "stage-create",
            &mut || {
                disk.add_file(&temp, &[]);
                Ok(())
            },
            None,
            None,
        )?;
        Ok(Box::new(FaultStaged {
            fs: self.fs.clone(),
            temp: temp_path.to_path_buf(),
            bytes: Vec::new(),
            replaced: false,
        }))
    }
}

struct FaultStaged {
    fs: Arc<FaultInjectionFileSystem>,
    temp: PathBuf,
    bytes: Vec<u8>,
    replaced: bool,
}

impl StagedReplacement for FaultStaged {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn flush_to_disk(&mut self) -> io::Result<()> {
        let disk = self.fs.disk().clone();
        let temp = self.temp.clone();
        let bytes = self.bytes.clone();
        self.fs.mutate(
            "stage-flush",
            &mut || {
                disk.add_file(&temp, &bytes);
                Ok(())
            },
            None,
            None,
        )
    }

    fn replace_atomically(&mut self, destination: &Path) -> io::Result<()> {
        let disk = self.fs.disk().clone();
        let temp = self.temp.clone();
        let bytes = self.bytes.clone();
        let destination = destination.to_path_buf();
        let replaced = &mut self.replaced;
        self.fs.mutate(
            "replace",
            &mut || {
                disk.add_file(&destination, &bytes);
                disk.delete(&temp)?;
                *replaced = true;
                Ok(())
            },
            None,
            None,
        )
    }
}

impl Drop for FaultStaged {
    /// A dropped, non-replaced replacement deletes its temp file (a dead process cleans nothing up).
    fn drop(&mut self) {
        if !self.replaced && !self.fs.dead() {
            let _ = self.fs.disk().delete(&self.temp);
        }
    }
}

const FAULT_STAMP_SECONDS: i64 = 0;

/// `FaultRig.Paths` and a journal of 4 500 Prepared+Committed pairs (over 1 MB), then a Prepared and a Failed line that must
/// survive untouched.
fn fault_journal_file() -> (Arc<FakeFileSystem>, Arc<FaultInjectionFileSystem>) {
    let clock = Arc::new(FakeClock::at(at(FAULT_STAMP_SECONDS)));
    let disk = Arc::new(FakeFileSystem::with_clock(clock));
    let mut text = String::new();
    for i in 0..4500 {
        let id = format!("op{i:05}");
        let source = format!(r"C:\photos\2026\summer-holiday\IMG_{i:05}.jpg");
        let destination = format!(r"C:\photos\2026\summer-holiday\selected\IMG_{i:05}.jpg");
        text.push_str(&fault_line(&id, JournalState::Prepared, &source, Some(&destination)));
        text.push_str(&fault_line(&id, JournalState::Committed, &source, Some(&destination)));
    }
    text.push_str(&fault_line(
        "pending",
        JournalState::Prepared,
        r"C:\photos\p.jpg",
        Some(r"C:\photos\sel\p.jpg"),
    ));
    text.push_str(&fault_line(
        "failed",
        JournalState::Failed,
        r"C:\photos\f.jpg",
        Some(r"C:\photos\sel\f.jpg"),
    ));
    disk.add_file(fake_paths().journal_file, text.as_bytes());
    let fs = Arc::new(FaultInjectionFileSystem::new(disk.clone()));
    (disk, fs)
}

fn fault_line(id: &str, state: JournalState, source: &str, destination: Option<&str>) -> String {
    let entry = JournalEntry {
        id: id.to_owned(),
        op_type: FileOperationType::Move,
        state,
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size: 1234,
        last_write_utc: JournalDateTime::utc(at(FAULT_STAMP_SECONDS)),
        timestamp_utc: JournalDateTime::utc(at(FAULT_STAMP_SECONDS + 1)),
        error: if state == JournalState::Failed {
            Some("boom".to_owned())
        } else {
            None
        },
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    };
    format!("{}\n", entry.to_line())
}

/// The latest verdict of every Id (`state|source|destination`) and the number of lines that do not parse.
fn latest_verdicts(disk: &FakeFileSystem) -> (std::collections::BTreeMap<String, String>, usize) {
    let mut latest = std::collections::BTreeMap::new();
    let mut unparsable = 0;
    let bytes = disk.file_content(fake_paths().journal_file).unwrap_or_default();
    for raw in String::from_utf8_lossy(&bytes).split('\n').filter(|l| !l.is_empty()) {
        match pr_core::file_actions::journal_line::try_parse(raw.trim_end_matches('\r')) {
            Some(entry) => {
                latest.insert(
                    entry.id,
                    format!("{:?}|{}|{:?}", entry.state, entry.source, entry.destination),
                );
            }
            None => unparsable += 1,
        }
    }
    (latest, unparsable)
}

fn fault_journal(fs: &Arc<FaultInjectionFileSystem>) -> OperationJournal {
    let clock = Arc::new(FakeClock::at(at(FAULT_STAMP_SECONDS)));
    OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock,
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(|_| {})),
            compaction_files: Some(Arc::new(FaultCompactionFiles { fs: fs.clone() })),
            ..OperationJournalOptions::default()
        },
    )
}

// C#: JournalCompactionFaultInjectionTests.Compaction_FaultAtEveryStep_KeepsEveryVerdict
#[test]
fn compaction_fault_at_every_step_keeps_every_verdict() {
    // The fault matrix: every kind of the injector except CancelAfter (not one of the matrix's default kinds).
    let kinds: Vec<FaultKind> = FaultKind::ALL
        .iter()
        .copied()
        .filter(|k| *k != FaultKind::CancelAfter)
        .collect();
    let (probe_disk, probe_fs) = fault_journal_file();
    let (before, _) = latest_verdicts(&probe_disk);
    probe_fs.arm(None);
    let probe = fault_journal(&probe_fs).try_compact();
    assert_eq!(probe.outcome, JournalCompactionOutcome::Compacted);
    let calls = probe_fs.kinds();
    assert!(calls.len() >= 3, "compaction must stage, flush and rename");
    let (healthy, healthy_bad) = latest_verdicts(&probe_disk);
    assert_eq!(healthy, before, "the healthy run is lossless");
    assert_eq!(healthy_bad, 0);
    assert!(
        probe.bytes_after < probe.bytes_before,
        "the fixture must really shrink the journal"
    );

    let mut violations: Vec<String> = Vec::new();
    let mut runs = 0usize;
    for point in 1..=calls.len() {
        for kind in &kinds {
            if !kind.applies(&calls[point - 1]) {
                continue;
            }
            runs += 1;
            let ctx = format!(
                "compaction [{kind:?} at call #{point}/{} = {}]",
                calls.len(),
                calls[point - 1]
            );
            let (disk, fs) = fault_journal_file();
            fs.arm(Some(FaultPlan { at: point, kind: *kind }));
            let result = fault_journal(&fs).try_compact(); // never throws for I/O
            if matches!(kind, FaultKind::CrashBefore | FaultKind::FailOnce)
                && result.outcome == JournalCompactionOutcome::Compacted
            {
                violations.push(format!("{ctx}: reported Compacted although its own step failed"));
            }

            // Restart: healthy file system, new journal object.
            fs.arm(None);
            let (after, bad) = latest_verdicts(&disk);
            if bad != 0 {
                violations.push(format!("{ctx}: {bad} journal line(s) no longer parse"));
            }
            for (id, verdict) in &before {
                match after.get(id) {
                    None => violations.push(format!("{ctx}: the verdict of {id} VANISHED")),
                    Some(now) if now != verdict => violations.push(format!("{ctx}: the verdict of {id} changed")),
                    Some(_) => {}
                }
            }

            let again = fault_journal(&fs).try_compact();
            if again.outcome == JournalCompactionOutcome::Failed {
                violations.push(format!(
                    "{ctx}: a later compaction on the surviving files FAILED: {:?}",
                    again.error
                ));
            }
            let (final_verdicts, final_bad) = latest_verdicts(&disk);
            if final_bad != 0 || before.iter().any(|(id, v)| final_verdicts.get(id) != Some(v)) {
                violations.push(format!(
                    "{ctx}: after the second compaction a verdict is lost or a line is unparsable"
                ));
            }
            let mut pending: Vec<String> = fault_journal(&fs)
                .read_pending_and_failed_operations()
                .map(|list| list.into_iter().map(|e| e.id).collect())
                .unwrap_or_default();
            pending.sort();
            if pending != ["failed", "pending"] {
                violations.push(format!(
                    "{ctx}: the Recovery items are {pending:?} instead of failed,pending"
                ));
            }
        }
    }

    assert!(runs >= calls.len(), "only {runs} compaction fault runs");
    assert!(
        violations.is_empty(),
        "{} violation(s) over {runs} compaction fault runs:\n{}",
        violations.len(),
        violations.iter().take(25).cloned().collect::<Vec<_>>().join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// JournalGroupDowngradeTests: the round trip only (the seven Reconcile_* tests are W3-03).
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalGroupDowngradeTests.GroupLineWrittenByThisBuild_SurvivesCompactionAndTailReaderUnchanged
#[test]
#[ignore = "native: real files under TempRoot"]
fn group_line_written_by_this_build_survives_compaction_and_tail_reader_unchanged() {
    let root = TempRoot::new("compact-group-roundtrip");
    let paths = native_paths(&root);
    std::fs::create_dir_all(paths.journal_file.parent().expect("parent")).expect("create data folder");
    let members = vec![
        pr_core::file_actions::journal_line::JournalGroupMember {
            source: r"C:\photos\a.jpg".into(),
            destination: Some(r"C:\selected\a.jpg".into()),
            size: 10,
            last_write_utc: JournalDateTime::utc(base()),
            permanent: false,
        },
        pr_core::file_actions::journal_line::JournalGroupMember {
            source: r"C:\photos\a.cr2".into(),
            destination: Some(r"C:\selected\a.cr2".into()),
            size: 100,
            last_write_utc: JournalDateTime::utc(base()),
            permanent: false,
        },
    ];
    let group = JournalEntry {
        id: "group-op".into(),
        op_type: FileOperationType::Move,
        state: JournalState::Prepared,
        source: r"C:\photos\a.jpg".into(),
        destination: Some(r"C:\selected\a.jpg".into()),
        size: 10,
        last_write_utc: JournalDateTime::utc(base()),
        timestamp_utc: JournalDateTime::utc(base()),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: Some("capture-a".into()),
        group_members: Some(members),
    };
    let mut bytes = Vec::new();
    for i in 0..9000 {
        let filler = JournalEntry {
            id: format!("f{i:06}"),
            source: format!(r"C:\photos\{}\f{i}.jpg", "x".repeat(80)),
            destination: Some(format!(r"C:\sel\{}\f{i}.jpg", "x".repeat(80))),
            size: 1,
            timestamp_utc: JournalDateTime::utc(at(i as i64)),
            ..group.clone()
        };
        bytes.extend(line_bytes(&filler));
        bytes.extend(line_bytes(&JournalEntry {
            state: JournalState::Committed,
            ..filler
        }));
    }
    // Last, so it sits inside the tail reader's window of the newest committed moves.
    bytes.extend(line_bytes(&group));
    let committed_group = JournalEntry {
        state: JournalState::Committed,
        timestamp_utc: JournalDateTime::utc(at(1)),
        ..group
    };
    bytes.extend(line_bytes(&committed_group));
    std::fs::write(&paths.journal_file, &bytes).expect("write journal");
    assert!(bytes.len() as u64 >= COMPACTION_THRESHOLD_BYTES);

    assert!(moves_of(&fresh_journal(&paths, physical(), system_clock())).contains(&committed_group));

    let result = native_journal(&paths, Hooks::default()).try_compact();
    assert_eq!(result.outcome, JournalCompactionOutcome::Compacted);

    assert!(moves_of(&fresh_journal(&paths, physical(), system_clock())).contains(&committed_group));
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: journal-compact (outcome, byte counts, the final file, leftovers, the moves a fresh journal reads).
// ---------------------------------------------------------------------------------------------------------------

// Golden: journal-compact (fake file system: the in-memory compaction primitives)
#[test]
fn golden_journal_compact() {
    let golden = golden::load("journal-compact");
    for case in &golden.cases {
        let clock = Arc::new(FakeClock::at(at(86_400)));
        let disk = Arc::new(FakeFileSystem::with_clock(clock.clone()));
        let paths = fake_paths();
        let fs: Arc<dyn FileSystem> = disk.clone();
        let hooks = before_lock_hook(&case["case"], fs.clone(), paths.journal_file.clone());
        let files = HookedCompactionFiles::wrap(Arc::new(MemoryCompactionFiles { fs: disk.clone() }), hooks);
        let files = (!no_compaction_files(&case["case"])).then_some(files);
        replay_compact_case(case, &paths, fs, clock, files);
    }
}

// Golden: journal-compact (native twin: PhysicalFileSystem + PhysicalJournalCompactionFiles under a TempRoot)
#[test]
#[ignore = "native: real files under TempRoot"]
fn golden_journal_compact_native() {
    let golden = golden::load("journal-compact");
    for case in &golden.cases {
        let root = TempRoot::new("compact-golden");
        let paths = native_paths(&root);
        let fs: Arc<dyn FileSystem> = physical();
        let hooks = before_lock_hook(&case["case"], fs.clone(), paths.journal_file.clone());
        let files = HookedCompactionFiles::wrap(Arc::new(PhysicalJournalCompactionFiles), hooks);
        let files = (!no_compaction_files(&case["case"])).then_some(files);
        replay_compact_case(case, &paths, fs, system_clock(), files);
    }
}

fn no_compaction_files(input: &Value) -> bool {
    input.get("no_compaction_files").and_then(Value::as_bool) == Some(true)
}

/// The before-lock action of a compaction case (C# `before_lock_append` / `before_lock_rewrite`).
fn before_lock_hook(input: &Value, fs: Arc<dyn FileSystem>, path: PathBuf) -> Hooks {
    let mut hooks = Hooks::default();
    if input.get("before_lock_append").is_some_and(Value::is_array) {
        let extra = specs_bytes(&input["before_lock_append"]);
        hooks.before_lock = Some(Box::new(move || {
            let mut stream = fs.open_append(&path, false).expect("open append");
            stream.write_all(&extra).expect("write");
            stream.flush().expect("flush");
        }));
    } else if input.get("before_lock_rewrite").is_some_and(Value::is_object) {
        let replacement = String::from_utf8(build_generated(&input["before_lock_rewrite"])).expect("text journal");
        hooks.before_lock = Some(Box::new(move || {
            fs.write_all_text_atomic(&path, &replacement, true).expect("rewrite")
        }));
    }
    hooks
}

/// The file's bytes through the file system's own read path (empty when the file is missing).
fn read_bytes(fs: &dyn FileSystem, path: &Path) -> Vec<u8> {
    use std::io::Read;
    if !fs.file_exists(path) {
        return Vec::new();
    }
    let mut reader = fs.open_read_shared(path, 65536).expect("open journal");
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).expect("read journal");
    bytes
}

fn replay_compact_case(
    case: &Value,
    paths: &AppPaths,
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    files: Option<Arc<dyn JournalCompactionFiles>>,
) {
    let name = str_of(case, "name");
    let input = &case["case"];
    let path = &paths.journal_file;
    let dir = path.parent().expect("parent").to_path_buf();
    if input.get("missing_dir").and_then(Value::as_bool) != Some(true) {
        let bytes = build_generated(input);
        assert_eq!(
            Some(bytes.len() as i64),
            case["initial_length"].as_i64(),
            "case {name}: initial length"
        );
        assert_eq!(
            Some(fnv64(&bytes).as_str()),
            case["initial_fnv64"].as_str(),
            "case {name}: initial fingerprint"
        );
        fs.create_directory(&dir).expect("create journal folder");
        fs.write_all_text_atomic(path, &String::from_utf8(bytes).expect("text journal"), true)
            .expect("write journal");
    } else {
        assert!(
            case["initial_length"].is_null(),
            "case {name}: a missing folder has no initial file"
        );
    }
    let journal = OperationJournal::with_options(
        paths,
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            compaction_files: files,
            ..OperationJournalOptions::default()
        },
    );
    let result = journal.try_compact();
    compare_compact_result(case, &result, paths, &fs);
}

fn compare_compact_result(case: &Value, result: &JournalCompactionResult, paths: &AppPaths, fs: &Arc<dyn FileSystem>) {
    let name = str_of(case, "name");
    let path = &paths.journal_file;
    assert_eq!(result.outcome.as_str(), str_of(case, "outcome"), "case {name}: outcome");
    assert_eq!(
        result.bytes_before,
        i64_of(case, "bytes_before"),
        "case {name}: bytes_before"
    );
    assert_eq!(
        result.bytes_after,
        i64_of(case, "bytes_after"),
        "case {name}: bytes_after"
    );
    assert_eq!(
        result.error.is_some(),
        case["error_set"].as_bool() == Some(true),
        "case {name}: error_set"
    );
    assert_eq!(
        Some(fs.file_exists(path)),
        case["final_exists"].as_bool(),
        "case {name}: final_exists"
    );
    let final_bytes = read_bytes(fs.as_ref(), path);
    assert_eq!(
        final_bytes.len() as i64,
        i64_of(case, "final_length"),
        "case {name}: final_length"
    );
    assert_eq!(
        fnv64(&final_bytes),
        str_of(case, "final_fnv64"),
        "case {name}: final_fnv64"
    );
    let newlines = final_bytes.iter().filter(|b| **b == b'\n').count();
    assert_eq!(
        newlines as i64,
        i64_of(case, "final_newlines"),
        "case {name}: final_newlines"
    );
    let dir = path.parent().expect("parent");
    let leftovers = fs
        .enumerate_files(dir, &format!("*{COMPACTION_FILE_SUFFIX}"))
        .map_or(0, |v| v.len());
    assert_eq!(
        leftovers as i64,
        i64_of(case, "leftover_temp_files"),
        "case {name}: leftover_temp_files"
    );
    let after = OperationJournal::new(paths, fs.clone(), system_clock())
        .read_committed_moves()
        .expect("read committed moves");
    assert_eq!(
        after.len() as i64,
        i64_of(case, "moves_after"),
        "case {name}: moves_after"
    );
    assert_eq!(
        after.last().map(|e| e.id.clone()),
        case["last_move_after"].as_str().map(str::to_owned),
        "case {name}: last_move_after"
    );
}
