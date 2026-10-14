//! Tests for `pr_core::file_actions::journal` (port of `PhotoReview.Core/FileActions/OperationJournal.cs`, C# baseline
//! d92d6aae). Red until the W3-01 bodies land: every test that reaches an unimplemented method panics with `todo!()`.
//!
//! Layout: C# test file -> sections below, one `// C#: <Class>.<Method>` comment above every test. Native tests run real
//! files under a [`TempRoot`] and are `#[ignore]`d: run them with `cargo test -- --include-ignored` on Windows. Nothing here
//! touches the Recycle Bin.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap};
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_core::abstractions::{
    AppendStream, Clock, FileStat, FileSystem, InProcessLiveOperationRegistry, LiveOperationRegistry, ReadSeek,
    SystemClock,
};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{
    APPEND_ATTEMPTS, DismissOutcome, FULL_SCAN_THRESHOLD_BYTES, OperationJournal, OperationJournalOptions,
    decode_stream_reader_lines,
};
use pr_core::file_actions::journal_line::{self, JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::journal_rules::STARTUP_COMMITTED_MOVE_LIMIT;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, JournalDurability, JournalState};
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::golden::{self, i64_of, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, TempRoot};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

/// C# `Stamp` of the in-memory tests (`2026-10-01T08:00:00Z` and `2026-09-25T00:00:00Z` both occur; this is the base).
fn base() -> DateTimeUtc {
    DateTimeUtc::parse_json("2026-09-25T00:00:00Z")
        .expect("base stamp parses")
        .0
}

/// `base()` plus `seconds`.
fn at(seconds: i64) -> DateTimeUtc {
    base()
        .checked_add_ticks(seconds * TICKS_PER_SECOND)
        .expect("stamp in range")
}

fn stamp(seconds: i64) -> JournalDateTime {
    JournalDateTime::utc(at(seconds))
}

/// `AppPaths` for `C:\Users\test\AppData\Local` (the journal lives under its `Data` folder).
fn fake_paths() -> AppPaths {
    AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false)
}

fn fake_journal_path() -> PathBuf {
    fake_paths().journal_file
}

/// A journal line with size 1 and both stamps at `base()`. Use struct update syntax for the other fields.
fn entry(
    id: &str,
    op: FileOperationType,
    state: JournalState,
    source: &str,
    destination: Option<&str>,
) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: op,
        state,
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size: 1,
        last_write_utc: stamp(0),
        timestamp_utc: stamp(0),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

fn parse_entry(line: &str) -> JournalEntry {
    journal_line::try_parse(line).unwrap_or_else(|| panic!("not an accepted journal line: {line}"))
}

fn hash_of(entry: &JournalEntry) -> u64 {
    let mut hasher = DefaultHasher::new();
    entry.hash(&mut hasher);
    hasher.finish()
}

/// Lines of a fake file (CRLF or LF; blank lines dropped).
fn fake_lines(fs: &FakeFileSystem, path: &Path) -> Vec<String> {
    let bytes = fs.file_content(path).unwrap_or_default();
    String::from_utf8(bytes)
        .expect("journal is UTF-8 in these tests")
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A fake journal (`FakeClock` at `base()`), append retries without delay.
fn quiet_journal(fs: &Arc<FakeFileSystem>, clock: &Arc<FakeClock>) -> OperationJournal {
    OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(|_| {})),
            ..OperationJournalOptions::default()
        },
    )
}

fn journal_on(fs: &Arc<FakeFileSystem>, clock: &Arc<FakeClock>) -> OperationJournal {
    OperationJournal::new(&fake_paths(), fs.clone(), clock.clone())
}

fn new_fake() -> (Arc<FakeClock>, Arc<FakeFileSystem>) {
    let clock = Arc::new(FakeClock::at(base()));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    (clock, fs)
}

/// Pending (Prepared) records of `read_pending_and_failed_operations` (the C# `ReadPendingOperations` helper).
fn read_pending(journal: &OperationJournal) -> Vec<JournalEntry> {
    read_by_state(journal, JournalState::Prepared)
}

/// Failed records (the C# `ReadFailedOperations` helper).
fn read_failed(journal: &OperationJournal) -> Vec<JournalEntry> {
    read_by_state(journal, JournalState::Failed)
}

fn read_by_state(journal: &OperationJournal, state: JournalState) -> Vec<JournalEntry> {
    journal
        .read_pending_and_failed_operations()
        .expect("read pending and failed")
        .into_iter()
        .filter(|e| e.state == state)
        .collect()
}

/// Committed Moves in file order (`read_committed_moves`, unwrapped).
fn committed(journal: &OperationJournal) -> Vec<JournalEntry> {
    journal.read_committed_moves().expect("read committed moves")
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex byte"))
        .collect()
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// FNV-1a 64 as lowercase hex (the golden fingerprints).
fn fnv64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A hand-written byte spec of a golden: `{"text": s, "eol": e}` (default CRLF) or `{"hex": h}`.
fn spec_bytes(spec: &Value) -> Vec<u8> {
    if let Some(hex) = spec.get("hex").and_then(Value::as_str) {
        return unhex(hex);
    }
    let eol = spec.get("eol").and_then(Value::as_str).unwrap_or("\r\n");
    let text = spec.get("text").and_then(Value::as_str).expect("text spec");
    format!("{text}{eol}").into_bytes()
}

/// A golden list of byte specs, concatenated.
fn specs_bytes(specs: &Value) -> Vec<u8> {
    specs
        .as_array()
        .map(|list| list.iter().flat_map(spec_bytes).collect())
        .unwrap_or_default()
}

/// `W301Build` of the oracle (`tools/oracle-cs/Commands/W3-01.cs`): the generated journal of `journal-committed-moves` and
/// `journal-compact`. Segments are concatenated in order, then `pad_to` prepends a malformed `x...\r\n` line.
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
                    let si = i64::try_from(si).expect("state index");
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
                        last_write_utc: JournalDateTime::utc(at(0)),
                        timestamp_utc: JournalDateTime::utc(at(i * 10 + si)),
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
                last_write_utc: JournalDateTime::utc(at(0)),
                timestamp_utc: JournalDateTime::utc(at(0)),
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

/// Counts the bytes actually read through every stream it opens (C# `BoundedReadFileSystem`), and the newline bytes seen.
#[derive(Clone)]
struct CountingFs {
    inner: Arc<dyn FileSystem>,
    bytes: Arc<AtomicU64>,
    lines: Arc<AtomicU64>,
}

impl CountingFs {
    fn new(inner: Arc<dyn FileSystem>) -> Self {
        Self {
            inner,
            bytes: Arc::new(AtomicU64::new(0)),
            lines: Arc::new(AtomicU64::new(0)),
        }
    }

    fn total_bytes_read(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    fn approx_lines_scanned(&self) -> u64 {
        self.lines.load(Ordering::Relaxed)
    }
}

struct CountingReader {
    inner: Box<dyn ReadSeek>,
    owner: CountingFs,
}

impl Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.owner.bytes.fetch_add(read as u64, Ordering::Relaxed);
        let newlines = buf[..read].iter().filter(|b| **b == b'\n').count();
        self.owner.lines.fetch_add(newlines as u64, Ordering::Relaxed);
        Ok(read)
    }
}

impl Seek for CountingReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl FileSystem for CountingFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
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
        let inner = self.inner.open_read_shared(path, buffer_size)?;
        Ok(Box::new(CountingReader {
            inner,
            owner: self.clone(),
        }))
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

/// Counts `flush` calls on every append stream it opens (C# `FlushSpyStream`).
struct FlushSpyFs {
    inner: Arc<FakeFileSystem>,
    flushes: Arc<AtomicUsize>,
}

struct FlushSpyStream {
    inner: Box<dyn AppendStream>,
    flushes: Arc<AtomicUsize>,
}

impl Write for FlushSpyStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes.fetch_add(1, Ordering::Relaxed);
        self.inner.flush()
    }
}

impl AppendStream for FlushSpyStream {}

impl FileSystem for FlushSpyFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
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
        let inner = self.inner.open_append(path, durable)?;
        Ok(Box::new(FlushSpyStream {
            inner,
            flushes: self.flushes.clone(),
        }))
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

/// A native data root under a [`TempRoot`]: the real `PhysicalFileSystem` and `SystemClock`.
fn native_paths(root: &TempRoot) -> AppPaths {
    AppPaths::new(root.path(), None, false)
}

fn physical_journal(paths: &AppPaths) -> OperationJournal {
    OperationJournal::new(paths, Arc::new(PhysicalFileSystem), Arc::new(SystemClock))
}

/// The C# `OperationJournalTests` constructor: source file, Prepared, a real move, Committed.
struct NativeFixture {
    paths: AppPaths,
    journal: OperationJournal,
    source: PathBuf,
    destination: PathBuf,
    operation_id: String,
    size: i64,
}

fn native_fixture(root: &TempRoot) -> NativeFixture {
    let paths = native_paths(root);
    let source = root.join("source.jpg");
    let destination = root.join("dest").join("source.jpg");
    std::fs::create_dir_all(destination.parent().expect("parent")).expect("create dest");
    std::fs::write(&source, [1u8, 2, 3, 4]).expect("write source");
    let size = 4;
    let journal = physical_journal(&paths);
    let operation_id = "native-op-1".to_owned();
    let prepared = JournalEntry {
        size,
        ..entry(
            &operation_id,
            FileOperationType::Move,
            JournalState::Prepared,
            &source.to_string_lossy(),
            Some(&destination.to_string_lossy()),
        )
    };
    journal.append(&prepared).expect("append prepared");
    std::fs::rename(&source, &destination).expect("move source");
    let committed = JournalEntry {
        state: JournalState::Committed,
        ..prepared
    };
    journal.append(&committed).expect("append committed");
    NativeFixture {
        paths,
        journal,
        source,
        destination,
        operation_id,
        size,
    }
}

/// `operations.jsonl` of a native fixture.
fn native_journal_file(paths: &AppPaths) -> PathBuf {
    paths.journal_file.clone()
}

/// The "other PhotoReview process" of the sharing-violation tests: an append handle with `FILE_SHARE_READ` only.
#[cfg(windows)]
fn other_writer(path: &Path) -> File {
    OpenOptions::new()
        .create(true)
        .append(true)
        .share_mode(1) // FILE_SHARE_READ, the mode PhysicalFileSystem::open_append uses
        .open(path)
        .expect("other writer opens the journal")
}

/// A seeded xorshift generator (no `rand` crate in the test tree; the sequences differ from .NET `Random` by design).
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish in `0..n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

// ---------------------------------------------------------------------------------------------------------------
// OperationJournalTests: native, the real data root under a TempRoot.
// ---------------------------------------------------------------------------------------------------------------

// C#: OperationJournalTests.JournalCommittedMove
#[test]
#[ignore = "native: real files under TempRoot"]
fn journal_committed_move() {
    let root = TempRoot::new("journal-committed-move");
    let fixture = native_fixture(&root);
    let moves = committed(&fixture.journal);
    assert!(
        moves.iter().any(|x| x.source == fixture.source.to_string_lossy()
            && x.destination.as_deref() == Some(&*fixture.destination.to_string_lossy())),
        "committed move missing: {moves:?}"
    );
}

// C#: OperationJournalTests.JournalHasNoPendingCommittedMove
#[test]
#[ignore = "native: real files under TempRoot"]
fn journal_has_no_pending_committed_move() {
    let root = TempRoot::new("journal-no-pending");
    let fixture = native_fixture(&root);
    assert!(read_pending(&fixture.journal).is_empty());
}

// C#: OperationJournalTests.UnknownTypeOrState_IsNotReadAsPendingMove
#[test]
fn unknown_type_or_state_is_not_read_as_pending_move() {
    let (clock, fs) = new_fake();
    let journal = quiet_journal(&fs, &clock);
    let path = fake_journal_path();
    fs.add_dir(path.parent().expect("parent"));
    let bogus = [
        r#"{"Id":"bogus-1","Type":"Teleport","State":"Prepared","Source":"C:/x.jpg","Destination":"C:/y.jpg","Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
        r#"{"Id":"bogus-2","Type":"Move","State":"Exploded","Source":"C:/x.jpg","Destination":"C:/y.jpg","Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
        r#"{"Id":"bogus-3","Source":"C:/x.jpg","Destination":"C:/y.jpg","Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ];
    let text: String = bogus.iter().map(|l| format!("{l}\r\n")).collect();
    fs.add_file(&path, text.as_bytes());

    assert!(read_pending(&journal).is_empty());
}

// C#: OperationJournalTests.Append_AfterTornLastLine_KeepsTheNewEntry
#[test]
#[ignore = "native: real files under TempRoot"]
fn append_after_torn_last_line_keeps_the_new_entry() {
    let root = TempRoot::new("journal-torn-last");
    let fixture = native_fixture(&root);
    {
        let mut file = OpenOptions::new()
            .append(true)
            .open(native_journal_file(&fixture.paths))
            .expect("open journal");
        file.write_all(b"{\"Id\":\"torn\",\"Type\":\"Mov").expect("torn write"); // power loss mid-record: no trailing newline
    }
    let fresh = physical_journal(&fixture.paths); // a new process: has not checked the tail yet
    let id = "after-torn";
    fresh
        .append(&entry(
            id,
            FileOperationType::Move,
            JournalState::Prepared,
            &fixture.source.to_string_lossy(),
            Some(&fixture.destination.to_string_lossy()),
        ))
        .expect("append after torn tail");

    assert!(read_pending(&fresh).iter().any(|e| e.id == id));
}

// C#: OperationJournalTests.Append_AfterOtherWriterLeftTornTail_StartsOnNewLine
#[test]
#[ignore = "native: real files under TempRoot"]
#[cfg(windows)]
fn append_after_other_writer_left_torn_tail_starts_on_new_line() {
    let root = TempRoot::new("journal-other-torn");
    let fixture = native_fixture(&root);
    let path = native_journal_file(&fixture.paths);
    {
        // Another PhotoReview process (InstanceMode.PerFolder) dies mid-append and leaves a line without a newline.
        let mut other = other_writer(&path);
        other.write_all(b"{\"partial").expect("torn write by other writer");
    }
    let id = "after-other-torn";
    fixture
        .journal
        .append(&entry(
            id,
            FileOperationType::Move,
            JournalState::Prepared,
            &fixture.source.to_string_lossy(),
            Some(&fixture.destination.to_string_lossy()),
        ))
        .expect("append");

    let text = std::fs::read_to_string(&path).expect("read journal");
    assert!(
        text.lines().any(|line| line == "{\"partial"),
        "the fragment is isolated on its own line"
    );
    assert!(read_pending(&fixture.journal).iter().any(|e| e.id == id));
    assert!(committed(&fixture.journal).iter().any(|e| e.id == fixture.operation_id));
}

// C#: OperationJournalTests.JournalEntriesAreDurablyWrittenAsJsonl
#[test]
#[ignore = "native: real files under TempRoot"]
fn journal_entries_are_durably_written_as_jsonl() {
    let root = TempRoot::new("journal-jsonl");
    let fixture = native_fixture(&root);
    let text = std::fs::read_to_string(native_journal_file(&fixture.paths)).expect("read journal");
    assert!(!text.is_empty() && text.lines().all(|line| line.starts_with('{')));
}

// C#: OperationJournalTests.JournalConcurrentAppendReadRemainsLineConsistent
#[test]
#[ignore = "native: real files under TempRoot"]
fn journal_concurrent_append_read_remains_line_consistent() {
    let root = TempRoot::new("journal-concurrent-append");
    let fixture = native_fixture(&root);
    std::thread::scope(|scope| {
        for i in 0..8 {
            let journal = &fixture.journal;
            let source = fixture.source.to_string_lossy().into_owned();
            let destination = fixture.destination.to_string_lossy().into_owned();
            scope.spawn(move || {
                let record = JournalEntry {
                    size: 4,
                    ..entry(
                        &format!("parallel-{i}"),
                        FileOperationType::Move,
                        JournalState::Committed,
                        &source,
                        Some(&destination),
                    )
                };
                journal.append(&record).expect("parallel append");
            });
        }
    });
    let count = committed(&fixture.journal)
        .iter()
        .filter(|x| x.id.starts_with("parallel-"))
        .count();
    assert_eq!(count, 8);
}

// C#: OperationJournalTests.MovePreservesBytes
#[test]
#[ignore = "native: real files under TempRoot"]
fn move_preserves_bytes() {
    let root = TempRoot::new("journal-move-bytes");
    let fixture = native_fixture(&root);
    assert_eq!(
        std::fs::read(&fixture.destination).expect("read destination"),
        vec![1u8, 2, 3, 4]
    );
    assert_eq!(fixture.size, 4);
}

// C#: OperationJournalTests.LegacyOperationsFixtureDeserializesToEnums
#[test]
fn legacy_operations_fixture_deserializes_to_enums() {
    let path = golden::repo_root()
        .join("tests")
        .join("fixtures")
        .join("operations-legacy.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()));
    // C# `File.ReadAllLines` strips the UTF-8 byte-order mark of the first line; the fixture starts with one.
    let text = text.trim_start_matches('\u{feff}');
    let entries: Vec<JournalEntry> = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| journal_line::try_parse(line).unwrap_or_else(|| panic!("legacy line does not parse: {line}")))
        .collect();
    assert_eq!(entries.len(), 4);

    assert_eq!(entries[0].id, "op-1");
    assert_eq!(entries[0].op_type, FileOperationType::Move);
    assert_eq!(entries[0].state, JournalState::Prepared);

    assert_eq!(entries[1].id, "op-1");
    assert_eq!(entries[1].op_type, FileOperationType::Move);
    assert_eq!(entries[1].state, JournalState::Committed);

    assert_eq!(entries[2].id, "op-2");
    assert_eq!(entries[2].op_type, FileOperationType::Recycle);
    assert_eq!(entries[2].state, JournalState::Prepared);

    assert_eq!(entries[3].id, "op-3");
    assert_eq!(entries[3].op_type, FileOperationType::Copy);
    assert_eq!(entries[3].state, JournalState::Failed);
}

// C#: OperationJournalUnitTests.Constructor_NullValidation (not portable: no null references in Rust; the constructor
// takes references and Arcs, so there is nothing to refuse at run time)

// ---------------------------------------------------------------------------------------------------------------
// OperationJournalUnitTests: in memory.
// ---------------------------------------------------------------------------------------------------------------

// C#: OperationJournalUnitTests.Append_CreatesDirectoryAndWritesReadableEntries
#[test]
fn append_creates_directory_and_writes_readable_entries() {
    let (clock, fs) = new_fake();
    let journal = quiet_journal(&fs, &clock);
    let record = JournalEntry {
        size: 1024,
        ..entry(
            "op-1",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\b.jpg"),
        )
    };

    journal.append(&record).expect("append");

    assert!(fs.file_exists(&fake_journal_path()));
    let moves = committed(&journal);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].id, "op-1");
    assert_eq!(moves[0].source, r"C:\photos\a.jpg");
    assert_eq!(moves[0].destination.as_deref(), Some(r"C:\photos\b.jpg"));
}

// C#: OperationJournalUnitTests.ReadOperations_ReflectsLatestState
#[test]
fn read_operations_reflects_latest_state() {
    let (clock, fs) = new_fake();
    let journal = quiet_journal(&fs, &clock);
    let id = "op-retry";
    let base_entry = JournalEntry {
        size: 1024,
        ..entry(
            id,
            FileOperationType::Move,
            JournalState::Failed,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\b.jpg"),
        )
    };

    journal
        .append(&JournalEntry {
            error: Some("Locked".into()),
            ..base_entry.clone()
        })
        .expect("append failed");
    assert_eq!(read_failed(&journal).len(), 1);
    assert!(read_pending(&journal).is_empty());

    journal
        .append(&JournalEntry {
            state: JournalState::Prepared,
            error: None,
            ..base_entry.clone()
        })
        .expect("append prepared");
    assert!(read_failed(&journal).is_empty());
    let pending = read_pending(&journal);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id);

    journal
        .append(&JournalEntry {
            state: JournalState::Committed,
            error: None,
            ..base_entry
        })
        .expect("append committed");
    assert!(read_failed(&journal).is_empty());
    assert!(read_pending(&journal).is_empty());
    assert_eq!(committed(&journal).len(), 1);
}

// C#: OperationJournalUnitTests.Dismiss_FailedAndPending_RemovedFromRecoveryLists
#[test]
fn dismiss_failed_and_pending_removed_from_recovery_lists() {
    let (clock, fs) = new_fake();
    let journal = quiet_journal(&fs, &clock);
    journal
        .append(&JournalEntry {
            error: Some("Locked".into()),
            ..entry(
                "failed",
                FileOperationType::Move,
                JournalState::Failed,
                r"C:\photos\a.jpg",
                Some(r"C:\photos\b.jpg"),
            )
        })
        .expect("append failed");
    journal
        .append(&entry(
            "pending",
            FileOperationType::Copy,
            JournalState::Prepared,
            r"C:\photos\c.jpg",
            Some(r"C:\photos\d.jpg"),
        ))
        .expect("append pending");
    journal
        .append(&JournalEntry {
            error: Some("Locked".into()),
            ..entry(
                "kept",
                FileOperationType::Recycle,
                JournalState::Failed,
                r"C:\photos\e.jpg",
                None,
            )
        })
        .expect("append kept");

    let failed_snapshot = read_failed(&journal)
        .into_iter()
        .find(|x| x.id == "failed")
        .expect("failed snapshot");
    let pending_snapshot = read_pending(&journal).into_iter().next().expect("pending snapshot");
    let outcome = journal.dismiss(&[failed_snapshot, pending_snapshot]).expect("dismiss");

    assert!(outcome.dismissed.iter().all(|x| x.state == JournalState::Dismissed));
    assert!(outcome.skipped.is_empty());
    assert!(read_pending(&journal).is_empty());
    let failed = read_failed(&journal);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id, "kept");
    assert!(
        journal
            .reconcile_pending_operations(None)
            .expect("reconcile")
            .is_empty()
    );
    assert!(journal.dismiss(&[]).expect("dismiss nothing").dismissed.is_empty());
}

// C#: OperationJournalUnitTests.Dismiss_StaleSnapshotRacedByAConcurrentRetry_SkipsInsteadOfOverwritingTheNewerEntry
#[test]
fn dismiss_stale_snapshot_raced_by_a_concurrent_retry_skips_instead_of_overwriting_the_newer_entry() {
    let (clock, fs) = new_fake();
    // Two journal instances on the same file: the Recovery window and a concurrent retry (or two PhotoReview processes).
    let owner = journal_on(&fs, &clock);
    let retrier = journal_on(&fs, &clock);

    let snapshot = JournalEntry {
        size: 1024,
        ..entry(
            "op-race",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\race.jpg",
            Some(r"C:\photos\sel\race.jpg"),
        )
    };
    owner.append(&snapshot).expect("owner appends the snapshot");
    // The Recovery window would have read `snapshot` here via read_pending_and_failed_operations.

    // The retry completes for the SAME Id before the user clicks Dismiss.
    let completed = JournalEntry {
        state: JournalState::Committed,
        timestamp_utc: stamp(1),
        ..snapshot.clone()
    };
    retrier.append(&completed).expect("retrier appends the outcome");

    let outcome = owner
        .dismiss(std::slice::from_ref(&snapshot))
        .expect("dismiss against the stale snapshot");

    assert!(outcome.dismissed.is_empty());
    assert_eq!(outcome.skipped, vec![snapshot]);
    assert!(read_pending(&owner).is_empty());
    assert!(read_failed(&owner).is_empty());
    assert!(
        committed(&owner).iter().any(|x| x.id == "op-race"),
        "the retry's outcome was not hidden"
    );
}

// C#: OperationJournalUnitTests.Dismiss_NoConcurrentChange_StillDismissesNormally
#[test]
fn dismiss_no_concurrent_change_still_dismisses_normally() {
    let (clock, fs) = new_fake();
    let journal = quiet_journal(&fs, &clock);
    let failed = JournalEntry {
        size: 1024,
        error: Some("boom".into()),
        ..entry(
            "op-normal",
            FileOperationType::Move,
            JournalState::Failed,
            r"C:\photos\normal.jpg",
            Some(r"C:\photos\sel\normal.jpg"),
        )
    };
    journal.append(&failed).expect("append");

    let outcome = journal.dismiss(std::slice::from_ref(&failed)).expect("dismiss");

    assert!(outcome.skipped.is_empty());
    assert_eq!(outcome.dismissed.len(), 1);
    assert_eq!(outcome.dismissed[0].state, JournalState::Dismissed);
    assert!(read_failed(&journal).is_empty());
}

// C#: OperationJournalUnitTests.ToleratesCorruptedLines
#[test]
fn tolerates_corrupted_lines() {
    let (clock, fs) = new_fake();
    let journal = quiet_journal(&fs, &clock);
    let valid = JournalEntry {
        size: 500,
        ..entry(
            "op-good",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\1.jpg",
            Some(r"C:\photos\2.jpg"),
        )
    };
    journal.append(&valid).expect("append valid");

    {
        let mut stream = fs.open_append(&fake_journal_path(), true).expect("open append");
        stream
            .write_all(b"{not json}\r\n\r\n{ incomplete json \r\n")
            .expect("write corrupt lines");
        stream.flush().expect("flush");
    }

    let valid2 = JournalEntry {
        size: 600,
        ..entry(
            "op-good-2",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\3.jpg",
            Some(r"C:\photos\4.jpg"),
        )
    };
    journal.append(&valid2).expect("append valid 2");

    let moves = committed(&journal);
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0].id, "op-good");
    assert_eq!(moves[1].id, "op-good-2");
}

// C#: OperationJournalUnitTests.ReconcileRecycle, ReconcileMove, ReconcileCopy: W3-03 (file_actions::reconcile), not here.

// C#: OperationJournalUnitTests.LargeJournal_ReadCommittedMoves_IsBoundedAndFast (native: 100 000 lines on a real file)
#[test]
#[ignore = "native: slow, 100 000 lines on a real file"]
fn large_journal_read_committed_moves_is_bounded_and_fast() {
    let root = TempRoot::new("journal-large-bounded");
    let paths = native_paths(&root);
    let total = 100_000u32;
    let ts = "2026-09-25T00:00:00.0000000Z";
    let mut text = String::new();
    for i in 0..total {
        text.push_str(&format!(
            "{{\"Id\":\"old-{i}\",\"Type\":\"Move\",\"State\":\"Committed\",\"Source\":\"C:\\\\photos\\\\source-{i}.jpg\",\"Destination\":\"C:\\\\photos\\\\dest-{i}.jpg\",\"Size\":{i},\"LastWriteUtc\":\"{ts}\",\"TimestampUtc\":\"{ts}\",\"Error\":null}}\n"
        ));
    }
    std::fs::create_dir_all(paths.journal_file.parent().expect("journal folder")).expect("create data root");
    std::fs::write(&paths.journal_file, text).expect("write large journal");
    let counting = CountingFs::new(Arc::new(PhysicalFileSystem));
    let journal = OperationJournal::new(&paths, Arc::new(counting.clone()), Arc::new(SystemClock));

    let entries = committed(&journal);

    assert_eq!(entries.len(), 200);
    assert_eq!(entries[0].id, "old-99800");
    assert_eq!(entries[entries.len() - 1].id, "old-99999");
    // Each line is ~140 bytes: the tail is ~28 KB; a full scan would read the whole {file_length}-byte file.
    assert!(
        counting.total_bytes_read() <= 1024 * 1024,
        "read {} bytes",
        counting.total_bytes_read()
    );
    assert!(
        counting.approx_lines_scanned() <= 3_000,
        "scanned ~{} lines",
        counting.approx_lines_scanned()
    );
}

/// A journal of `total` lines, every `move_period`-th one a committed Move, the rest Recycle (`Destination` null).
fn move_sparse_text(total: usize, move_period: usize, ts: &str) -> (String, Vec<String>) {
    let mut text = String::new();
    let mut moves = Vec::new();
    for i in 0..total {
        let id = format!("e-{i}");
        let is_move = i % move_period == 0;
        if is_move {
            moves.push(id.clone());
        }
        let ty = if is_move { "Move" } else { "Recycle" };
        let destination = if is_move {
            format!("\"C:\\\\photos\\\\dest-{i}.jpg\"")
        } else {
            "null".to_owned()
        };
        text.push_str(&format!(
            "{{\"Id\":\"{id}\",\"Type\":\"{ty}\",\"State\":\"Committed\",\"Source\":\"C:\\\\photos\\\\s-{id}.jpg\",\"Destination\":{destination},\"Size\":1,\"LastWriteUtc\":\"{ts}\",\"TimestampUtc\":\"{ts}\",\"Error\":null}}\n"
        ));
    }
    (text, moves)
}

// C#: OperationJournalUnitTests.LargeJournal_ReadCommittedMoves_ReturnsLastMovesAcrossWindows (in memory, 37 / 400 / 5000)
#[test]
fn large_journal_read_committed_moves_returns_last_moves_across_windows_dense_37() {
    assert_windows_case(37);
}

// C#: OperationJournalUnitTests.LargeJournal_ReadCommittedMoves_ReturnsLastMovesAcrossWindows (in memory, 400)
#[test]
fn large_journal_read_committed_moves_returns_last_moves_across_windows_sparse_400() {
    assert_windows_case(400);
}

// C#: OperationJournalUnitTests.LargeJournal_ReadCommittedMoves_ReturnsLastMovesAcrossWindows (in memory, 5000)
#[test]
fn large_journal_read_committed_moves_returns_last_moves_across_windows_very_sparse_5000() {
    assert_windows_case(5000);
}

fn assert_windows_case(move_period: usize) {
    let (clock, fs) = new_fake();
    let ts = "2026-09-25T00:00:00.0000000Z";
    let (text, moves) = move_sparse_text(20_000, move_period, ts); // ~4.5 MB: over the 1 MB full-scan threshold
    fs.add_file(fake_journal_path(), text.as_bytes());
    let journal = journal_on(&fs, &clock);
    let expected: Vec<String> = moves.iter().rev().take(200).rev().cloned().collect();
    let got: Vec<String> = committed(&journal).into_iter().map(|e| e.id).collect();
    assert_eq!(got, expected);
}

// C#: OperationJournalUnitTests.LargeJournal_ReadCommittedMoves_ReturnsLastMovesAcrossWindows (native twin, 37 / 400 / 5000)
#[test]
#[ignore = "native: 20 000 lines on a real file"]
fn large_journal_read_committed_moves_returns_last_moves_across_windows_native() {
    let root = TempRoot::new("journal-windows-native");
    let path = root.join("operations.jsonl");
    for period in [37usize, 400, 5000] {
        let (text, moves) = move_sparse_text(20_000, period, "2026-09-25T00:00:00.0000000Z");
        std::fs::write(&path, text).expect("write native journal");
        let paths = AppPaths {
            journal_file: path.clone(),
            ..native_paths(&root)
        };
        let journal = OperationJournal::new(&paths, Arc::new(PhysicalFileSystem), Arc::new(SystemClock));
        let expected: Vec<String> = moves.iter().rev().take(200).rev().cloned().collect();
        let got: Vec<String> = committed(&journal).into_iter().map(|e| e.id).collect();
        assert_eq!(got, expected, "period {period}");
    }
}

// C#: OperationJournalUnitTests.LargeMoveSparseJournal_ReadCommittedMoves_ParsesEachByteOnce (native, 100 000 lines)
#[test]
#[ignore = "slow"]
fn large_move_sparse_journal_read_committed_moves_parses_each_byte_once() {
    let root = TempRoot::new("journal-sparse-once");
    let path = root.join("operations.jsonl");
    // Move paths embed the skip token (JSON-escaped): must not be skipped.
    let ts = "2026-09-25T00:00:00.0000000Z";
    let mut text = String::new();
    for i in 0..100_000 {
        let is_move = i % 1000 == 0;
        let ty = if is_move { "Move" } else { "Recycle" };
        let skip = if is_move { "\\\"Type\\\":\\\"Recycle\\\"-" } else { "" };
        let destination = if is_move {
            format!("\"C:\\\\photos\\\\dest-{i}.jpg\"")
        } else {
            "null".to_owned()
        };
        text.push_str(&format!(
            "{{\"Id\":\"e-{i}\",\"Type\":\"{ty}\",\"State\":\"Committed\",\"Source\":\"C:\\\\photos\\\\source-{skip}{i}.jpg\",\"Destination\":{destination},\"Size\":{i},\"LastWriteUtc\":\"{ts}\",\"TimestampUtc\":\"{ts}\",\"Error\":null}}\n"
        ));
    }
    std::fs::write(&path, &text).expect("write sparse journal");
    let file_length = std::fs::metadata(&path).expect("stat").len();
    let paths = AppPaths {
        journal_file: path.clone(),
        ..native_paths(&root)
    };
    let counting = CountingFs::new(Arc::new(PhysicalFileSystem));
    let journal = OperationJournal::new(&paths, Arc::new(counting.clone()), Arc::new(SystemClock));

    let entries = committed(&journal);

    assert_eq!(entries.len(), 100);
    assert_eq!(entries[0].id, "e-0");
    assert_eq!(entries[entries.len() - 1].id, "e-99000");
    assert!(
        counting.total_bytes_read() <= file_length + 8 * 1024,
        "read {} of {file_length} bytes",
        counting.total_bytes_read()
    );
}

// C#: OperationJournalUnitTests.SmallMoveSparseJournal_ReadCommittedMoves_ReadsEachByteOnce (in memory, ~1.3 MB)
#[test]
fn small_move_sparse_journal_read_committed_moves_reads_each_byte_once() {
    let (clock, fs) = new_fake();
    let (text, _) = move_sparse_text(9_500, 1000, "2026-09-25T00:00:00.0000000Z");
    fs.add_file(fake_journal_path(), text.as_bytes());
    let file_length = fs.get_file_stat(&fake_journal_path()).expect("stat").length;
    assert!(
        file_length > 1024 * 1024,
        "fixture must exceed the full-scan threshold (is {file_length} bytes)"
    );
    let counting = CountingFs::new(fs.clone());
    let journal = OperationJournal::new(&fake_paths(), Arc::new(counting.clone()), clock.clone());

    let moves = committed(&journal);

    assert_eq!(moves.len(), 10);
    assert_eq!(moves[0].id, "e-0");
    assert_eq!(moves[moves.len() - 1].id, "e-9000");
    assert!(counting.total_bytes_read() > 0);
    assert!(
        counting.total_bytes_read() as f64 <= file_length as f64 * 1.05,
        "read {} of {file_length} bytes",
        counting.total_bytes_read()
    );
}

// C#: OperationJournalUnitTests.LargeMoveSparseJournal_ReadCommittedMoves_TimingReport (manual: timing report, no assertion)
#[test]
#[ignore = "manual: timing report, no assertion"]
fn large_move_sparse_journal_read_committed_moves_timing_report() {
    let root = TempRoot::new("journal-timing-report");
    let path = root.join("operations.jsonl");
    let (text, _) = move_sparse_text(100_000, 1000, "2026-09-25T00:00:00.0000000Z");
    std::fs::write(&path, text).expect("write timing journal");
    let paths = AppPaths {
        journal_file: path,
        ..native_paths(&root)
    };
    let journal = OperationJournal::new(&paths, Arc::new(PhysicalFileSystem), Arc::new(SystemClock));
    let _warm_up = committed(&journal);
    let mut samples = Vec::new();
    for _ in 0..5 {
        let watch = std::time::Instant::now();
        let _ = committed(&journal);
        samples.push(watch.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "Move-sparse 100k-line tail read: min {:.1} ms, median {:.1} ms, max {:.1} ms (ADR 0003 budget: 100 ms).",
        samples[0], samples[2], samples[4]
    );
}

// C#: OperationJournalUnitTests.ReadPendingAndFailedOperations_ParsesJournalOnce (R2-F-17)
#[test]
fn read_pending_and_failed_operations_parses_journal_once() {
    let (clock, fs) = new_fake();
    let seed = quiet_journal(&fs, &clock);
    let entry_of = |id: &str, state: JournalState| JournalEntry {
        size: 4,
        ..entry(
            id,
            FileOperationType::Move,
            state,
            &format!(r"C:\a\{id}.jpg"),
            Some(&format!(r"C:\b\{id}.jpg")),
        )
    };
    seed.append(&entry_of("done", JournalState::Prepared)).expect("append");
    seed.append(&entry_of("done", JournalState::Committed)).expect("append");
    seed.append(&entry_of("stuck", JournalState::Prepared)).expect("append");
    seed.append(&entry_of("broken", JournalState::Prepared))
        .expect("append");
    seed.append(&entry_of("broken", JournalState::Failed)).expect("append");
    let file_length = fs.get_file_stat(&fake_journal_path()).expect("stat").length;

    let counting = CountingFs::new(fs.clone());
    let journal = OperationJournal::new(&fake_paths(), Arc::new(counting.clone()), clock.clone());
    let list = journal.read_pending_and_failed_operations().expect("read");

    let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["stuck", "broken"]);
    assert!(
        counting.total_bytes_read() <= file_length,
        "read {} of {file_length} bytes",
        counting.total_bytes_read()
    );
}

// ---------------------------------------------------------------------------------------------------------------
// JournalAppendRobustnessTests
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalAppendRobustnessTests.FailedFirstAppend_NextAppendStillRepairsPartialTail
#[test]
fn failed_first_append_next_append_still_repairs_partial_tail() {
    let (clock, fs) = new_fake();
    let path = fake_journal_path();
    fs.add_dir(path.parent().expect("parent"));
    // A crash left a partial last line without a newline.
    fs.add_file(&path, b"{\"Id\":\"torn\",\"Ty");
    let journal = quiet_journal(&fs, &clock);

    fs.fail_times(FsOp::OpenAppend, &path, io::ErrorKind::Other, 1); // "disk full" on the first open
    assert!(
        journal
            .append(&entry(
                "first",
                FileOperationType::Move,
                JournalState::Prepared,
                r"C:\photos\first.jpg",
                Some(r"C:\photos\sel\first.jpg")
            ))
            .is_err()
    );
    fs.clear_faults();

    journal
        .append(&entry(
            "second",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\second.jpg",
            Some(r"C:\photos\sel\second.jpg"),
        ))
        .expect("second append");

    let pending = read_pending(&journal);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "second");
}

// C#: JournalAppendRobustnessTests.Append_SharingViolation_RetriesUntilOtherWriterReleases (native, share-mode handle)
#[test]
#[ignore = "native: real files under TempRoot"]
#[cfg(windows)]
fn append_sharing_violation_retries_until_other_writer_releases() {
    let root = TempRoot::new("journal-sharing-retry");
    let paths = native_paths(&root);
    std::fs::create_dir_all(paths.journal_file.parent().expect("parent")).expect("create data folder");
    let other = Arc::new(Mutex::new(Some(other_writer(&paths.journal_file))));
    let delays: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
    let (other_hook, delays_hook) = (other.clone(), delays.clone());
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(SystemClock),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(move |delay| {
                let mut recorded = delays_hook.lock().expect("delays");
                recorded.push(delay);
                if recorded.len() == 2 {
                    other_hook.lock().expect("other").take(); // the other process finishes its write
                }
            })),
            ..OperationJournalOptions::default()
        },
    );

    let result = journal.append(&entry(
        "a",
        FileOperationType::Move,
        JournalState::Prepared,
        r"C:\photos\a.jpg",
        Some(r"C:\photos\sel\a.jpg"),
    ));
    other.lock().expect("other").take();

    result.expect("append succeeds once the other writer is gone");
    let recorded = delays.lock().expect("delays").clone();
    assert_eq!(recorded.len(), 2);
    assert!(recorded[1] > recorded[0]);
    assert_eq!(read_pending(&journal)[0].id, "a");
}

// C#: JournalAppendRobustnessTests.Append_OtherWriterTearsTailWhileThisAppendWaits_StartsOnNewLine (native)
#[test]
#[ignore = "native: real files under TempRoot"]
#[cfg(windows)]
fn append_other_writer_tears_tail_while_this_append_waits_starts_on_new_line() {
    let root = TempRoot::new("journal-tears-while-waiting");
    let paths = native_paths(&root);
    std::fs::create_dir_all(paths.journal_file.parent().expect("parent")).expect("create data folder");
    std::fs::write(&paths.journal_file, "{\"Id\":\"clean\"}\r\n").expect("clean tail");
    // Another PhotoReview process holds the journal mid-append ...
    let other = Arc::new(Mutex::new(Some(other_writer(&paths.journal_file))));
    let other_hook = other.clone();
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(SystemClock),
        OperationJournalOptions {
            // ... and dies half-way through its record while this append is backing off.
            append_retry_delay: Some(Arc::new(move |_| {
                if let Some(mut handle) = other_hook.lock().expect("other").take() {
                    handle.write_all(b"{\"partial").expect("torn write");
                }
            })),
            ..OperationJournalOptions::default()
        },
    );

    let result = journal.append(&entry(
        "a",
        FileOperationType::Move,
        JournalState::Prepared,
        r"C:\photos\a.jpg",
        Some(r"C:\photos\sel\a.jpg"),
    ));
    other.lock().expect("other").take();

    result.expect("append");
    let text = std::fs::read_to_string(&paths.journal_file).expect("read journal");
    assert!(text.lines().any(|line| line == "{\"partial"));
    assert_eq!(read_pending(&journal)[0].id, "a");
}

// C#: JournalAppendRobustnessTests.Append_SharingViolationNeverReleased_ThrowsAfterBoundedAttempts (native)
#[test]
#[ignore = "native: real files under TempRoot"]
#[cfg(windows)]
fn append_sharing_violation_never_released_throws_after_bounded_attempts_native() {
    let root = TempRoot::new("journal-never-released");
    let paths = native_paths(&root);
    std::fs::create_dir_all(paths.journal_file.parent().expect("parent")).expect("create data folder");
    let _other = other_writer(&paths.journal_file);
    let delays = Arc::new(AtomicUsize::new(0));
    let counter = delays.clone();
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(SystemClock),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            })),
            ..OperationJournalOptions::default()
        },
    );

    let result = journal.append(&entry(
        "a",
        FileOperationType::Move,
        JournalState::Prepared,
        r"C:\photos\a.jpg",
        Some(r"C:\photos\sel\a.jpg"),
    ));

    assert!(result.is_err());
    assert_eq!(delays.load(Ordering::Relaxed), (APPEND_ATTEMPTS - 1) as usize);
}

// C#: JournalAppendRobustnessTests.Append_SharingViolationNeverReleased_ThrowsAfterBoundedAttempts (fake twin: 4 delays
// 10/20/30/40 ms, 5 opens)
#[test]
fn append_sharing_violation_never_released_throws_after_bounded_attempts() {
    let (clock, fs) = new_fake();
    let path = fake_journal_path();
    fs.add_dir(path.parent().expect("parent"));
    fs.fail_locked(FsOp::OpenAppend, &path);
    let delays: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = delays.clone();
    let journal = OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(move |delay| recorder.lock().expect("delays").push(delay))),
            ..OperationJournalOptions::default()
        },
    );

    let result = journal.append(&entry(
        "a",
        FileOperationType::Move,
        JournalState::Prepared,
        r"C:\photos\a.jpg",
        Some(r"C:\photos\sel\a.jpg"),
    ));

    assert!(result.is_err());
    assert_eq!(
        *delays.lock().expect("delays"),
        [10, 20, 30, 40].map(Duration::from_millis).to_vec(),
        "4 delays, 10/20/30/40 ms"
    );
    assert_eq!(fs.count(FsOp::OpenAppend), 5);
}

// C#: JournalAppendRobustnessTests.Append_OtherIoError_IsNotRetried
#[test]
fn append_other_io_error_is_not_retried() {
    let (clock, fs) = new_fake();
    let path = fake_journal_path();
    fs.add_dir(path.parent().expect("parent"));
    let delays = Arc::new(AtomicUsize::new(0));
    let counter = delays.clone();
    let journal = OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            })),
            ..OperationJournalOptions::default()
        },
    );
    fs.fail_times(FsOp::OpenAppend, &path, io::ErrorKind::Other, 1); // "disk full": not a sharing violation

    assert!(
        journal
            .append(&entry(
                "a",
                FileOperationType::Move,
                JournalState::Prepared,
                r"C:\photos\a.jpg",
                Some(r"C:\photos\sel\a.jpg")
            ))
            .is_err()
    );
    assert_eq!(delays.load(Ordering::Relaxed), 0);
}

// C#: JournalAppendRobustnessTests.Reconcile_AppendFailsForOneEntry_ReturnsTheOthers: W3-03 (reconcile), not here.

// C#: JournalAppendRobustnessTests.Append_PathWithUnpairedSurrogate_ThrowsCodedErrorAndWritesNothing
// DIVERGENCE: not portable. A Rust `String` cannot hold an unpaired UTF-16 surrogate (trap T19), so a path with one never
// reaches a `JournalEntry`; the refusal belongs where OS paths are converted (W3-04 / the catalog, contract open question 2).

// C#: JournalAppendRobustnessTests.Append_PathWithValidSurrogatePair_RoundTrips
#[test]
fn append_path_with_valid_surrogate_pair_round_trips() {
    let (clock, fs) = new_fake();
    let journal = quiet_journal(&fs, &clock);
    let source = format!(r"C:\photos\a{}.jpg", '\u{1F600}');

    journal
        .append(&entry(
            "ok",
            FileOperationType::Move,
            JournalState::Prepared,
            &source,
            Some(r"C:\photos\sel\a.jpg"),
        ))
        .expect("append");

    assert_eq!(read_pending(&journal)[0].source, source);
}

// ---------------------------------------------------------------------------------------------------------------
// JournalDurabilityTests (journal half). The AppSettings half is W2-01's and already ported.
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalDurabilityTests.DefaultsToFast_WhenNoProviderGiven
#[test]
fn defaults_to_fast_when_no_provider_given() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    assert_eq!(journal.durability(), JournalDurability::Fast);
}

// C#: JournalDurabilityTests.ReadSkipsCorruptTrailingLine (Fast)
#[test]
fn read_skips_corrupt_trailing_line_fast() {
    read_skips_corrupt_trailing_line(JournalDurability::Fast);
}

// C#: JournalDurabilityTests.ReadSkipsCorruptTrailingLine (PowerLossSafe)
#[test]
fn read_skips_corrupt_trailing_line_power_loss_safe() {
    read_skips_corrupt_trailing_line(JournalDurability::PowerLossSafe);
}

fn read_skips_corrupt_trailing_line(mode: JournalDurability) {
    let (clock, fs) = new_fake();
    let journal = OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            durability: Some(Arc::new(move || mode)),
            append_retry_delay: Some(Arc::new(|_| {})),
            ..OperationJournalOptions::default()
        },
    );
    let ok = JournalEntry {
        size: 1,
        ..entry(
            "ok",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\a.jpg",
            Some(r"C:\b\a.jpg"),
        )
    };
    journal.append(&ok).expect("append");
    {
        let mut stream = fs
            .open_append(&fake_journal_path(), mode == JournalDurability::PowerLossSafe)
            .expect("open append");
        stream.write_all(b"{\"Id\":\"torn\",\"Type\":\"Mo").expect("torn write"); // power loss mid-line, no newline
        stream.flush().expect("flush");
    }

    let moves = committed(&journal);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].id, "ok");
    assert!(read_pending(&journal).is_empty());
}

// C#: JournalDurabilityTests.PhysicalFileSystem_OpenAppend_AppendsInBothModes (native, durable false and true)
#[test]
#[ignore = "native: real files under TempRoot"]
fn physical_file_system_open_append_appends_in_both_modes() {
    for durable in [false, true] {
        let root = TempRoot::new("journal-open-append");
        let path = root.join("j.jsonl");
        let fs = PhysicalFileSystem;
        for i in 0..2 {
            let mut stream = fs.open_append(&path, durable).expect("open append");
            stream.write_all(format!("line{i}\n").as_bytes()).expect("write");
            stream.flush().expect("flush");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "line0\nline1\n",
            "durable {durable}"
        );
    }
}

// C#: JournalDurabilityTests.ReadPending_SkipsLinesWithoutIdOrSource_AndKeepsValidOnes
#[test]
fn read_pending_skips_lines_without_id_or_source_and_keeps_valid_ones() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    fs.add_dir(fake_journal_path().parent().expect("parent")); // the folder exists, as after any earlier append
    let no_id = r#"{"Type":"Move","State":"Prepared","Source":"C:\p\b.jpg","Size":1}"#;
    let no_source = r#"{"Id":"noSource","Type":"Move","State":"Prepared","Size":1}"#;
    {
        let mut stream = fs.open_append(&fake_journal_path(), false).expect("open append");
        stream
            .write_all(format!("{no_id}\n{no_source}\n").as_bytes())
            .expect("write");
        stream.flush().expect("flush");
    }
    journal
        .append(&entry(
            "ok",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\a.jpg",
            Some(r"C:\b\a.jpg"),
        ))
        .expect("append");

    let pending = read_pending(&journal);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "ok");
}

// ---------------------------------------------------------------------------------------------------------------
// OperationJournalDurableFlushTests: the flush-to-disk seam (decision P-11 / contract open question 1).
// ---------------------------------------------------------------------------------------------------------------

// C#: OperationJournalDurableFlushTests.Append_PowerLossSafeOnARealFile_FlushesToDiskOncePerAppendAfterTheBytesWereWritten
#[test]
#[ignore = "native: real files under TempRoot"]
fn append_power_loss_safe_on_a_real_file_flushes_to_disk_once_per_append_after_the_bytes_were_written() {
    let root = TempRoot::new("journal-flush");
    let paths = native_paths(&root);
    let flushed: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = flushed.clone();
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(SystemClock),
        OperationJournalOptions {
            durability: Some(Arc::new(|| JournalDurability::PowerLossSafe)),
            flush_to_disk: Some(Arc::new(move |file: &mut File| {
                recorder.lock().expect("flushed").push(file.metadata()?.len());
                Ok(())
            })),
            ..OperationJournalOptions::default()
        },
    );
    let record = |id: &str| JournalEntry {
        state: JournalState::Committed,
        size: 1,
        ..entry(
            id,
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\b.jpg"),
        )
    };

    journal.append(&record("op-1")).expect("append 1");
    journal.append(&record("op-2")).expect("append 2");

    let sizes = flushed.lock().expect("flushed").clone();
    assert_eq!(sizes.len(), 2);
    assert!(sizes[0] > 0, "the flush must come after the line was written");
    assert!(sizes[1] > sizes[0]);
    assert_eq!(
        std::fs::read_to_string(&paths.journal_file)
            .expect("read")
            .lines()
            .count(),
        2
    );
}

// C#: OperationJournalDurableFlushTests.Append_FastMode_NeverFlushesToDisk
#[test]
#[ignore = "native: real files under TempRoot"]
fn append_fast_mode_never_flushes_to_disk() {
    let root = TempRoot::new("journal-fast-no-flush");
    let paths = native_paths(&root);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let journal = OperationJournal::with_options(
        &paths,
        Arc::new(PhysicalFileSystem),
        Arc::new(SystemClock),
        OperationJournalOptions {
            flush_to_disk: Some(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })),
            ..OperationJournalOptions::default()
        },
    );

    journal
        .append(&entry(
            "op-1",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\b.jpg"),
        ))
        .expect("append");

    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

// C#: OperationJournalDurableFlushTests.Append_PowerLossSafeOnAStreamThatIsNotAFileStream_FallsBackToAPlainFlush
#[test]
fn append_power_loss_safe_on_a_stream_that_is_not_a_file_stream_falls_back_to_a_plain_flush() {
    let (clock, fs) = new_fake();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let journal = OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            durability: Some(Arc::new(|| JournalDurability::PowerLossSafe)),
            flush_to_disk: Some(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })),
            ..OperationJournalOptions::default()
        },
    );

    journal
        .append(&entry(
            "op-1",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\b.jpg"),
        ))
        .expect("append");

    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(fake_lines(&fs, &fake_journal_path()).len(), 1);
}

// ---------------------------------------------------------------------------------------------------------------
// OperationJournalGapTests
// ---------------------------------------------------------------------------------------------------------------

/// `count` committed Moves `m000000..`, each about 300 bytes, ~1.3 MiB in total with `pad` bytes of error text.
fn small_move_lines(count: usize) -> String {
    (0..count)
        .map(|i| {
            let line = JournalEntry {
                size: 10 + i as i64,
                error: Some("p".repeat(200)),
                ..entry(
                    &format!("m{i:06}"),
                    FileOperationType::Move,
                    JournalState::Committed,
                    &format!(r"C:\photos\s{i:06}.jpg"),
                    Some(&format!(r"C:\photos\sel\s{i:06}.jpg")),
                )
            };
            format!("{}\n", line.to_line())
        })
        .collect()
}

fn move_id(i: usize) -> String {
    format!("m{i:06}")
}

fn gap_journal(fs: &Arc<FakeFileSystem>, clock: &Arc<FakeClock>, content: &str) -> OperationJournal {
    fs.add_file(fake_journal_path(), content.as_bytes());
    assert!(
        fs.get_file_stat(&fake_journal_path()).expect("stat").length >= 1024 * 1024,
        "fixture must exceed the full-scan threshold"
    );
    journal_on(fs, clock)
}

// C#: OperationJournalGapTests.ReadCommittedMoves_LastLineLongerThanTheWindow_WidensAndReturnsNewest200InOrder
#[test]
fn read_committed_moves_last_line_longer_than_the_window_widens_and_returns_newest_200_in_order() {
    let (clock, fs) = new_fake();
    let small = 5000;
    let giant = JournalEntry {
        error: Some("g".repeat(256 * 1024 + 50_000)), // one committed Move line longer than a window
        ..entry(
            &move_id(small),
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\giant.jpg",
            Some(r"C:\photos\sel\giant.jpg"),
        )
    };
    let content = format!("{}{}\n", small_move_lines(small), giant.to_line());
    let journal = gap_journal(&fs, &clock, &content);

    let ids: Vec<String> = committed(&journal).into_iter().map(|e| e.id).collect();

    let expected: Vec<String> = (small - 199..=small).map(move_id).collect();
    assert_eq!(ids, expected);
}

// C#: OperationJournalGapTests.ReadCommittedMoves_TornGiantLastLineWithoutNewline_IsSkippedAndEarlierMovesKept
#[test]
fn read_committed_moves_torn_giant_last_line_without_newline_is_skipped_and_earlier_moves_kept() {
    let (clock, fs) = new_fake();
    let small = 5000;
    let torn = format!(
        "{{\"Id\":\"torn\",\"Type\":\"Move\",\"State\":\"Committed\",\"Source\":\"{}",
        "t".repeat(256 * 1024 + 50_000)
    ); // no closing, no newline
    let journal = gap_journal(&fs, &clock, &format!("{}{torn}", small_move_lines(small)));

    let ids: Vec<String> = committed(&journal).into_iter().map(|e| e.id).collect();

    let expected: Vec<String> = (small - 200..small).map(move_id).collect();
    assert_eq!(ids, expected);
}

// C#: OperationJournalGapTests.ReadCommittedMoves_TornShortLastLine_IsIgnoredAndEarlierLinesKept
#[test]
fn read_committed_moves_torn_short_last_line_is_ignored_and_earlier_lines_kept() {
    let (clock, fs) = new_fake();
    let small = 5000;
    let tail_line = JournalEntry {
        size: 1,
        ..entry(
            &move_id(small),
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\x.jpg",
            Some(r"C:\photos\sel\x.jpg"),
        )
    }
    .to_line();
    let torn_tail: String = tail_line.chars().take(40).collect(); // half a line, no newline: a writer died mid-append
    let journal = gap_journal(&fs, &clock, &format!("{}{torn_tail}", small_move_lines(small)));

    let ids: Vec<String> = committed(&journal).into_iter().map(|e| e.id).collect();

    assert_eq!(ids.len(), 200);
    assert_eq!(ids.last().map(String::as_str), Some(move_id(small - 1).as_str()));
    assert!(!ids.contains(&move_id(small)));
}

// C#: OperationJournalGapTests.Dismiss_PreparedSnapshotMarkedLiveByAnotherProcess_IsDismissedAndTheOwnersLaterOutcomeStillWins
#[test]
fn dismiss_prepared_snapshot_marked_live_by_another_process_is_dismissed_and_the_owners_later_outcome_still_wins() {
    let (clock, fs) = new_fake();
    let registry: Arc<dyn LiveOperationRegistry> = Arc::new(InProcessLiveOperationRegistry::new());
    let options = || OperationJournalOptions {
        live_operations: Some(registry.clone()),
        append_retry_delay: Some(Arc::new(|_| {})),
        ..OperationJournalOptions::default()
    };
    let recovery = OperationJournal::with_options(&fake_paths(), fs.clone(), clock.clone(), options());
    let owner = OperationJournal::with_options(&fake_paths(), fs.clone(), clock.clone(), options());
    let prepared = JournalEntry {
        size: 10,
        ..entry(
            "op-live",
            FileOperationType::Move,
            JournalState::Prepared,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\sel\a.jpg"),
        )
    };
    let _marker = registry.begin(&prepared.id);
    owner.append(&prepared).expect("owner appends");
    assert!(registry.is_live(&prepared.id));

    let outcome = recovery.dismiss(std::slice::from_ref(&prepared)).expect("dismiss");

    assert!(outcome.skipped.is_empty());
    assert_eq!(outcome.dismissed.len(), 1);
    assert_eq!(outcome.dismissed[0].state, JournalState::Dismissed);
    assert!(read_pending(&recovery).is_empty());

    owner
        .append(&JournalEntry {
            state: JournalState::Committed,
            ..prepared
        })
        .expect("owner's outcome");
    assert!(committed(&recovery).iter().any(|e| e.id == "op-live"));
    assert!(read_failed(&recovery).is_empty());
    assert!(read_pending(&recovery).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// OperationJournalMutationTests (the non-reconcile half: value equality, Dismiss, the guards, flush, the tail reader).
// ---------------------------------------------------------------------------------------------------------------

const JPG: &str = r"C:\photos\a.jpg";
const OUT_JPG: &str = r"C:\photos\out\a.jpg";

fn full_entry() -> JournalEntry {
    JournalEntry {
        id: "id".into(),
        op_type: FileOperationType::Move,
        state: JournalState::Failed,
        source: r"C:\a.jpg".into(),
        destination: Some(r"C:\b\a.jpg".into()),
        size: 10,
        last_write_utc: stamp(0),
        timestamp_utc: stamp(1),
        error: Some("e".into()),
        error_code: Some("c".into()),
        permanent: Some(false),
        undo: Some(false),
        group_id: Some("g".into()),
        group_members: Some(vec![member(r"C:\a.jpg", r"C:\b\a.jpg", 10)]),
    }
}

fn member(source: &str, destination: &str, size: i64) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: Some(destination.to_owned()),
        size,
        last_write_utc: stamp(0),
        permanent: false,
    }
}

fn variant(e: &JournalEntry, field: usize) -> JournalEntry {
    let mut out = e.clone();
    match field {
        0 => out.id = "other".into(),
        1 => out.op_type = FileOperationType::Copy,
        2 => out.state = JournalState::Committed,
        3 => out.source = r"C:\other.jpg".into(),
        4 => out.destination = Some(r"C:\b\other.jpg".into()),
        5 => out.size = 11,
        6 => out.last_write_utc = stamp(1),
        7 => out.timestamp_utc = stamp(2),
        8 => out.error = Some("other".into()),
        9 => out.error_code = Some("other".into()),
        10 => out.permanent = Some(true),
        11 => out.undo = Some(true),
        12 => out.group_id = Some("other".into()),
        _ => panic!("field {field} out of range"),
    }
    out
}

// C#: OperationJournalMutationTests.Equals_EntriesDifferingInOnlyOneField_AreNotEqual (13 fields, one loop)
#[test]
fn equals_entries_differing_in_only_one_field_are_not_equal() {
    let left = full_entry();
    for field in 0..13 {
        let right = variant(&left, field);
        assert_ne!(left, right, "field {field}");
        assert_ne!(right, left, "field {field} (reversed)");
    }
}

// C#: OperationJournalMutationTests.Equals_EntriesDifferingOnlyInTheMembers_AreNotEqual
#[test]
fn equals_entries_differing_only_in_the_members_are_not_equal() {
    let left = full_entry();
    let right = JournalEntry {
        group_members: Some(vec![member(r"C:\a.jpg", r"C:\b\a.jpg", 99)]),
        ..left.clone()
    };
    assert_ne!(left, right);
}

// C#: OperationJournalMutationTests.Equals_Null_IsFalse
#[test]
fn equals_null_is_false() {
    assert_ne!(Some(full_entry()), None::<JournalEntry>);
}

// C#: OperationJournalMutationTests.Equals_SameValuesWithFreshMemberLists_AreEqualWithTheSameHash
#[test]
fn equals_same_values_with_fresh_member_lists_are_equal_with_the_same_hash() {
    let left = full_entry();
    let right = JournalEntry {
        group_members: Some(vec![member(r"C:\a.jpg", r"C:\b\a.jpg", 10)]),
        ..full_entry()
    };
    assert_eq!(left, right);
    assert_eq!(hash_of(&left), hash_of(&right));
}

// C#: OperationJournalMutationTests.GetHashCode_EntryWithoutMembers_DoesNotThrow
#[test]
fn get_hash_code_entry_without_members_does_not_throw() {
    let entry_without = JournalEntry {
        group_id: None,
        group_members: None,
        ..full_entry()
    };
    assert_eq!(hash_of(&entry_without), hash_of(&entry_without.clone()));
}

// C#: OperationJournalMutationTests.GetHashCode_EntriesDifferingOnlyInTheMembers_DifferInTheirHash
#[test]
fn get_hash_code_entries_differing_only_in_the_members_differ_in_their_hash() {
    let baseline = full_entry();
    let mut hashes = BTreeSet::new();
    hashes.insert(hash_of(&baseline));
    for i in 1..=16 {
        let variant = JournalEntry {
            group_members: Some(vec![
                member(r"C:\a.jpg", r"C:\b\a.jpg", 99 + i),
                member(r"C:\photos\a.cr2", r"C:\photos\out\a.cr2", 8),
            ]),
            ..full_entry()
        };
        hashes.insert(hash_of(&variant));
    }
    assert!(hashes.len() > 1, "the hash ignores GroupMembers");
}

// C#: OperationJournalMutationTests.Dismiss_OnlyStaleSnapshots_AppendsNothingAndTouchesNoFile
#[test]
fn dismiss_only_stale_snapshots_appends_nothing_and_touches_no_file() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let group = group_move(JournalState::Failed);

    let outcome: DismissOutcome = journal.dismiss(&[group]).expect("dismiss");

    assert!(outcome.dismissed.is_empty());
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(fs.count(FsOp::OpenAppend), 0, "no append was opened");
    assert_eq!(fs.count(FsOp::CreateDirectory), 0, "no folder was created");
    assert!(!fs.file_exists(&fake_journal_path()));
}

fn group_move(state: JournalState) -> JournalEntry {
    JournalEntry {
        state,
        group_id: Some("capture".into()),
        group_members: Some(vec![
            member(JPG, OUT_JPG, 4),
            member(r"C:\photos\a.cr2", r"C:\photos\out\a.cr2", 8),
        ]),
        size: 4,
        ..entry("grp", FileOperationType::Move, state, JPG, Some(OUT_JPG))
    }
}

fn line_at(state: JournalState, tick: i64) -> JournalEntry {
    JournalEntry {
        size: 4,
        timestamp_utc: stamp(tick),
        ..entry("op", FileOperationType::Move, state, JPG, Some(OUT_JPG))
    }
}

// C#: OperationJournalMutationTests.AppendIfUnchangedSince_TailIsExactlyTheOwnEntriesInOrder_Appends
#[test]
fn append_if_unchanged_since_tail_is_exactly_the_own_entries_in_order_appends() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let anchor = line_at(JournalState::Failed, 0);
    let own1 = line_at(JournalState::Prepared, 1);
    let own2 = line_at(JournalState::Prepared, 2);
    journal.append(&anchor).expect("anchor");
    journal.append(&own1).expect("own 1");
    journal.append(&own2).expect("own 2");

    let appended = journal
        .append_if_unchanged_since(&anchor, &[own1, own2], &line_at(JournalState::Committed, 3))
        .expect("guard");

    assert!(appended);
    assert_eq!(fake_lines(&fs, &fake_journal_path()).len(), 4);
}

// C#: OperationJournalMutationTests.AppendIfUnchangedSince_SecondEntryAfterTheAnchorIsSomebodyElses_DoesNotAppend
#[test]
fn append_if_unchanged_since_second_entry_after_the_anchor_is_somebody_elses_does_not_append() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let anchor = line_at(JournalState::Failed, 0);
    let own1 = line_at(JournalState::Prepared, 1);
    let own2 = line_at(JournalState::Prepared, 2);
    journal.append(&anchor).expect("anchor");
    journal.append(&own1).expect("own 1");
    journal
        .append(&line_at(JournalState::Committed, 7))
        .expect("somebody else's line");

    let appended = journal
        .append_if_unchanged_since(&anchor, &[own1, own2], &line_at(JournalState::Failed, 8))
        .expect("guard");

    assert!(!appended);
    assert_eq!(fake_lines(&fs, &fake_journal_path()).len(), 3);
}

// C#: OperationJournalMutationTests.Append_FlushesTheAppendStream (both modes)
#[test]
fn append_flushes_the_append_stream_fast() {
    append_flushes_the_append_stream(JournalDurability::Fast);
}

// C#: OperationJournalMutationTests.Append_FlushesTheAppendStream (both modes)
#[test]
fn append_flushes_the_append_stream_power_loss_safe() {
    append_flushes_the_append_stream(JournalDurability::PowerLossSafe);
}

fn append_flushes_the_append_stream(mode: JournalDurability) {
    let (clock, fs) = new_fake();
    let spy = Arc::new(FlushSpyFs {
        inner: fs.clone(),
        flushes: Arc::new(AtomicUsize::new(0)),
    });
    let journal = OperationJournal::with_options(
        &fake_paths(),
        spy.clone(),
        clock.clone(),
        OperationJournalOptions {
            durability: Some(Arc::new(move || mode)),
            append_retry_delay: Some(Arc::new(|_| {})),
            ..OperationJournalOptions::default()
        },
    );

    journal.append(&line_at(JournalState::Committed, 0)).expect("append");

    assert!(spy.flushes.load(Ordering::Relaxed) >= 1);
    assert_eq!(fake_lines(&fs, &fake_journal_path()).len(), 1);
}

/// Padded journal: a malformed `x...` line (ending in CRLF) first, so that `rest` ends the file at exactly `total` bytes.
fn padded_journal(total: usize, rest: &[u8]) -> Vec<u8> {
    let pad_length = total - rest.len();
    assert!(pad_length >= 3, "pad too small");
    let mut out = vec![b'x'; pad_length];
    out[pad_length - 2] = b'\r';
    out[pad_length - 1] = b'\n';
    out.extend_from_slice(rest);
    out
}

fn move_line_bytes(i: usize) -> Vec<u8> {
    let line = JournalEntry {
        size: 4,
        ..entry(
            &format!("m{i:04}"),
            FileOperationType::Move,
            JournalState::Committed,
            &format!(r"C:\photos\{i}.jpg"),
            Some(&format!(r"C:\sel\{i}.jpg")),
        )
    };
    format!("{}\r\n", line.to_line()).into_bytes()
}

const ONE_MIB: usize = 1024 * 1024;

// C#: OperationJournalMutationTests.ReadCommittedMoves_JournalOfExactlyOneMiB_UsesTheTailWindowOf200Moves
#[test]
fn read_committed_moves_journal_of_exactly_one_mib_uses_the_tail_window_of_200_moves() {
    let (clock, fs) = new_fake();
    let rest: Vec<u8> = (0..300).flat_map(move_line_bytes).collect();
    fs.add_file(fake_journal_path(), &padded_journal(ONE_MIB, &rest));

    let moves = committed(&journal_on(&fs, &clock));

    assert_eq!(moves.len(), STARTUP_COMMITTED_MOVE_LIMIT);
    assert_eq!(moves[0].id, "m0100");
    assert_eq!(moves[moves.len() - 1].id, "m0299");
    assert_eq!(FULL_SCAN_THRESHOLD_BYTES as usize, ONE_MIB);
}

// C#: OperationJournalMutationTests.ReadCommittedMoves_JournalOneByteUnderOneMiB_ReadsEveryMove
#[test]
fn read_committed_moves_journal_one_byte_under_one_mib_reads_every_move() {
    let (clock, fs) = new_fake();
    let rest: Vec<u8> = (0..300).flat_map(move_line_bytes).collect();
    fs.add_file(fake_journal_path(), &padded_journal(ONE_MIB - 1, &rest));

    let moves = committed(&journal_on(&fs, &clock));

    assert_eq!(moves.len(), 300);
}

// C#: OperationJournalMutationTests.ReadCommittedMoves_BareNewlinesBetweenLinesInABigJournal_NeitherLosesNorDuplicatesAMove
#[test]
fn read_committed_moves_bare_newlines_between_lines_in_a_big_journal_neither_loses_nor_duplicates_a_move() {
    let (clock, fs) = new_fake();
    // A bare "\n" is what the torn-tail repair leaves between records.
    let mut rest: Vec<u8> = (0..50).flat_map(move_line_bytes).collect();
    rest.push(b'\n');
    rest.extend((50..99).flat_map(move_line_bytes));
    rest.push(b'\n');
    rest.extend(move_line_bytes(99));
    fs.add_file(fake_journal_path(), &padded_journal(ONE_MIB + 5000, &rest));

    let ids: Vec<String> = committed(&journal_on(&fs, &clock)).into_iter().map(|e| e.id).collect();

    let expected: Vec<String> = (0..100).map(|i| format!("m{i:04}")).collect();
    assert_eq!(ids, expected);
}

// C#: OperationJournalMutationTests.ReadCommittedMoves_LastLineLongerThanTheFirstWindowAndWithoutATrailingNewline_IsStillRead
#[test]
fn read_committed_moves_last_line_longer_than_the_first_window_and_without_a_trailing_newline_is_still_read() {
    let (clock, fs) = new_fake();
    // 300 KB of path in one Move line: the first 256 KB window holds no line break, so the reader must widen it.
    let long_move = JournalEntry {
        size: 4,
        ..entry(
            "big",
            FileOperationType::Move,
            JournalState::Committed,
            &format!(r"C:\photos\{}.jpg", "a".repeat(300_000)),
            Some(r"C:\sel\big.jpg"),
        )
    }
    .to_line();
    let mut rest = move_line_bytes(1);
    rest.extend_from_slice(long_move.as_bytes());
    fs.add_file(fake_journal_path(), &padded_journal(ONE_MIB, &rest));

    let ids: Vec<String> = committed(&journal_on(&fs, &clock)).into_iter().map(|e| e.id).collect();

    assert_eq!(ids, ["m0001", "big"]);
}

// C#: OperationJournalMutationTests.Reconcile_* (11 tests): W3-03 (file_actions::reconcile), not here.

// ---------------------------------------------------------------------------------------------------------------
// JournalGroupReconcileTests (non-reconcile half). The six Reconcile_* tests are W3-03; TryParse_* is W1-14.
// ---------------------------------------------------------------------------------------------------------------

fn group_prepared(members: Vec<JournalGroupMember>) -> JournalEntry {
    let first = members[0].clone();
    JournalEntry {
        size: first.size,
        last_write_utc: first.last_write_utc,
        group_id: Some("capture-a".into()),
        group_members: Some(members.clone()),
        ..entry(
            "group-op",
            FileOperationType::Move,
            JournalState::Prepared,
            &first.source,
            first.destination.as_deref(),
        )
    }
}

// C#: JournalGroupReconcileTests.Dismiss_GroupEntryReReadFromJournal_IsDismissedNotSkipped
#[test]
fn dismiss_group_entry_re_read_from_journal_is_dismissed_not_skipped() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let prepared = group_prepared(vec![
        member(r"C:\photos\a.jpg", r"C:\selected\a.jpg", 10),
        member(r"C:\photos\a.cr2", r"C:\selected\a.cr2", 100),
    ]);
    journal
        .append(&JournalEntry {
            state: JournalState::Failed,
            ..prepared
        })
        .expect("append");
    let snapshot = read_failed(&journal).into_iter().next().expect("one failed group");

    let outcome = journal.dismiss(std::slice::from_ref(&snapshot)).expect("dismiss");

    assert!(outcome.skipped.is_empty());
    assert_eq!(outcome.dismissed.len(), 1);
    assert_eq!(outcome.dismissed[0].id, snapshot.id);
    assert!(read_failed(&journal).is_empty());
}

// C#: JournalGroupReconcileTests.Dismiss_MixedGroupAndSingleEntries_ClearsAllOfThem
#[test]
fn dismiss_mixed_group_and_single_entries_clears_all_of_them() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let prepared = group_prepared(vec![
        member(r"C:\photos\a.jpg", r"C:\selected\a.jpg", 10),
        member(r"C:\photos\a.cr2", r"C:\selected\a.cr2", 100),
    ]);
    journal
        .append(&JournalEntry {
            state: JournalState::Failed,
            ..prepared
        })
        .expect("group");
    journal
        .append(&JournalEntry {
            state: JournalState::Failed,
            size: 5,
            ..entry(
                "single",
                FileOperationType::Move,
                JournalState::Failed,
                r"C:\photos\b.jpg",
                Some(r"C:\selected\b.jpg"),
            )
        })
        .expect("single");

    let outcome = journal.dismiss(&read_failed(&journal)).expect("dismiss");

    assert!(outcome.skipped.is_empty());
    assert_eq!(outcome.dismissed.len(), 2);
    assert!(read_failed(&journal).is_empty());
}

// C#: JournalGroupReconcileTests.JournalEntry_GroupEntriesWithEqualMembers_AreValueEqual
#[test]
fn journal_entry_group_entries_with_equal_members_are_value_equal() {
    let left = group_prepared(vec![member(r"C:\photos\a.jpg", r"C:\selected\a.jpg", 10)]);
    let right = group_prepared(vec![member(r"C:\photos\a.jpg", r"C:\selected\a.jpg", 10)]);
    let different = group_prepared(vec![member(r"C:\photos\a.jpg", r"C:\selected\a.jpg", 11)]);

    assert_eq!(left, right);
    assert_eq!(hash_of(&left), hash_of(&right));
    assert_ne!(left, different);
    assert_ne!(
        left,
        JournalEntry {
            group_members: None,
            ..left.clone()
        }
    );
}

// ---------------------------------------------------------------------------------------------------------------
// JournalErrorCodeTests, JournalStartupRecoveryTests, JournalLineParserTests (journal-level twins)
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalErrorCodeTests.EntryWithoutError_KeepsPreviousJsonShape
#[test]
fn entry_without_error_keeps_previous_json_shape() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    journal
        .append(&entry(
            "ok",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\b.jpg"),
        ))
        .expect("append");

    let lines = fake_lines(&fs, &fake_journal_path());
    assert_eq!(lines.len(), 1);
    assert!(lines[0].ends_with(",\"Error\":null}"), "{}", lines[0]);
    assert!(!lines[0].contains("ErrorCode"));
}

// C#: JournalStartupRecoveryTests.Dismiss_ClearsErrorCode
#[test]
fn dismiss_clears_error_code() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let failed = JournalEntry {
        state: JournalState::Failed,
        error: Some("boom".into()),
        error_code: Some(pr_core::file_actions::errors::DESTINATION_OUTSIDE_SOURCE.to_owned()),
        size: 5,
        ..entry(
            "f1",
            FileOperationType::Move,
            JournalState::Failed,
            r"C:\photos\a.jpg",
            Some(r"C:\photos\sel\a.jpg"),
        )
    };
    journal.append(&failed).expect("append");

    let dismissed = journal.dismiss(std::slice::from_ref(&failed)).expect("dismiss");

    assert_eq!(dismissed.dismissed.len(), 1);
    assert_eq!(dismissed.dismissed[0].error_code, None);
    let lines = fake_lines(&fs, &fake_journal_path());
    assert!(!lines.last().expect("a line").contains("ErrorCode"));
}

/// Lines `journal_line::try_parse` rejects (the skip cases of the C# corpus in `JournalLineParserTests`).
const SKIP_CASES: &[(&str, &str)] = &[
    (
        "TypeIsObject",
        r#"{"Id":"id1","Type":{},"State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "TypeIsArray",
        r#"{"Id":"id1","Type":[],"State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "MissingType",
        r#"{"Id":"id1","State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "MissingState",
        r#"{"Id":"id1","Type":"Move","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "LowerCasePropertyNames",
        r#"{"id":"id1","type":"Move","state":"Prepared","source":"C:\\a.jpg","destination":null,"size":1,"lastWriteUtc":"2026-01-01T00:00:00Z","timestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "MissingId",
        r#"{"Type":"Move","State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "EmptyId",
        r#"{"Id":"","Type":"Move","State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "NullId",
        r#"{"Id":null,"Type":"Move","State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "MissingSource",
        r#"{"Id":"id1","Type":"Move","State":"Prepared","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "NullSource",
        r#"{"Id":"id1","Type":"Move","State":"Prepared","Source":null,"Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    ("MalformedTruncated", r#"{"Id":"id1","Type":"Move""#),
    ("MalformedSingleBrace", "{"),
    ("NonObjectArray", "[]"),
    ("NonObjectString", r#""x""#),
    ("NonObjectNumber", "1"),
    ("NonObjectNull", "null"),
    ("NonObjectEmptyString", ""),
    ("NonObjectWhitespace", "   "),
    (
        "CommaListTypes",
        r#"{"Id":"id1","Type":"Move, Copy","State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "UnknownType",
        r#"{"Id":"id1","Type":"Rename","State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "EmptyType",
        r#"{"Id":"id1","Type":"","State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "TypeJsonNull",
        r#"{"Id":"id1","Type":null,"State":"Prepared","Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "StateJsonNull",
        r#"{"Id":"id1","Type":"Move","State":null,"Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
    (
        "NumericStateOutOfRange",
        r#"{"Id":"id1","Type":"Move","State":7,"Source":"C:\\a.jpg","Destination":null,"Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#,
    ),
];

// C#: JournalLineParserTests.ReadPendingOperations_BadLineBesideGoodLine_OnlyGoodLineReturned (one case per skip line)
#[test]
fn read_pending_operations_bad_line_beside_good_line_only_good_line_returned() {
    let good = r#"{"Id":"good","Type":"Move","State":"Prepared","Source":"C:\\photos\\good.jpg","Destination":"C:\\photos\\sel\\good.jpg","Size":1,"LastWriteUtc":"2026-01-01T00:00:00Z","TimestampUtc":"2026-01-01T00:00:00Z"}"#;
    for &(name, bad) in SKIP_CASES {
        let (clock, fs) = new_fake();
        let journal = journal_on(&fs, &clock);
        fs.add_file(fake_journal_path(), format!("{bad}\n{good}\n").as_bytes());

        let pending = read_pending(&journal);

        assert_eq!(pending.len(), 1, "case {name}");
        assert_eq!(pending[0].id, "good", "case {name}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// JournalConcurrencyTests
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalConcurrencyTests.ManyWritersOneFile_NoTornOrLostAcknowledgedRecords (native, six journals on one file)
#[test]
#[ignore = "native: slow, six writers on a real file"]
fn many_writers_one_file_no_torn_or_lost_acknowledged_records() {
    let root = TempRoot::new("journal-many-writers");
    let paths = native_paths(&root);
    let writers = 6;
    let per_writer = 40;
    let acknowledged = Arc::new(Mutex::new(Vec::<String>::new()));
    let refusals = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for w in 0..writers {
            let paths = paths.clone();
            let acknowledged = acknowledged.clone();
            let refusals = refusals.clone();
            scope.spawn(move || {
                // One instance per "process": its gate does not serialize the others, only the file sharing does.
                let journal = physical_journal(&paths);
                for i in 0..per_writer {
                    let id = format!("w{w}-{i}");
                    let record = entry(
                        &id,
                        FileOperationType::Copy,
                        JournalState::Prepared,
                        r"C:\photos\a.jpg",
                        Some(r"C:\photos\sel\a.jpg"),
                    );
                    // Like the production caller: retry a refused append, every refusal leaves no half-written record.
                    loop {
                        match journal.append(&record) {
                            Ok(()) => {
                                acknowledged.lock().expect("acknowledged").push(id);
                                break;
                            }
                            Err(_) => {
                                refusals.fetch_add(1, Ordering::Relaxed);
                                std::thread::yield_now();
                            }
                        }
                    }
                }
            });
        }
    });

    let text = std::fs::read_to_string(&paths.journal_file).expect("read journal");
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let mut ids = BTreeSet::new();
    for line in &lines {
        let entry = parse_entry(line); // a glued/torn line fails here
        assert!(ids.insert(entry.id.clone()), "record {} was written twice", entry.id);
    }
    let acknowledged = acknowledged.lock().expect("acknowledged").clone();
    for id in &acknowledged {
        assert!(ids.contains(id), "acknowledged {id} is missing");
    }
    assert_eq!(ids.len(), lines.len());
    assert_eq!(
        acknowledged.len(),
        writers * per_writer,
        "{} refused attempts",
        refusals.load(Ordering::Relaxed)
    );
}

// C#: JournalConcurrencyTests.ReconcileWhileOtherProcessCommits_LatestStateStaysCommitted: W3-03 (reconcile), not here.
// C#: JournalConcurrencyTests.ManySimultaneousActions_ExactlyOneRuns: W3-04 (FileActionService), not here.

// ---------------------------------------------------------------------------------------------------------------
// JournalModelTests: a seeded random history against a reference model. Red until W3-03 (reconcile is in the history).
// ---------------------------------------------------------------------------------------------------------------

/// The reference model: the latest record per Id, the committed Moves in order, and the first-insertion order of Ids.
#[derive(Default)]
struct Model {
    latest: HashMap<String, JournalEntry>,
    committed_moves: Vec<JournalEntry>,
    order: Vec<String>,
}

impl Model {
    fn record(&mut self, journal: &OperationJournal, record: JournalEntry) {
        journal.append(&record).expect("append in the model history");
        self.apply(record);
    }

    /// Updates the model without writing (after a dismiss or a reconcile verdict that the journal has written).
    fn apply(&mut self, record: JournalEntry) {
        if !self.order.contains(&record.id) {
            self.order.push(record.id.clone());
        }
        if record.op_type == FileOperationType::Move && record.state == JournalState::Committed {
            self.committed_moves.push(record.clone());
        }
        self.latest.insert(record.id.clone(), record);
    }

    /// `ReadPendingAndFailedOperations` in the model: Prepared then Failed, each in first-insertion order.
    fn pending_and_failed(&self) -> Vec<(String, JournalState)> {
        let mut rows: Vec<(u8, usize, String, JournalState)> = self
            .latest
            .values()
            .filter(|e| matches!(e.state, JournalState::Prepared | JournalState::Failed))
            .map(|e| {
                let rank = if e.state == JournalState::Prepared { 0 } else { 1 };
                let index = self.order.iter().position(|id| *id == e.id).expect("ordered id");
                (rank, index, e.id.clone(), e.state)
            })
            .collect();
        rows.sort_by_key(|row| (row.0, row.1));
        rows.into_iter().map(|(_, _, id, state)| (id, state)).collect()
    }
}

fn random_history(seed: u64, pad_bytes: usize) {
    let mut rng = XorShift::new(seed);
    let (clock, fs) = new_fake();
    let path = fake_journal_path();
    fs.add_dir(path.parent().expect("parent"));
    let mut journal = quiet_journal(&fs, &clock);
    let mut model = Model::default();
    let mut next_id = 0usize;
    if pad_bytes > 0 {
        // A history already past the 1 MB threshold (mixed Move/Copy so the tail reader has to skip lines).
        let mut text = String::new();
        for i in 0..400usize {
            let ty = if i % 3 == 0 {
                FileOperationType::Copy
            } else {
                FileOperationType::Move
            };
            let record = JournalEntry {
                error: Some("p".repeat(pad_bytes)),
                size: 10,
                last_write_utc: stamp(0),
                timestamp_utc: stamp(0),
                ..entry(
                    &format!("pre{i}"),
                    ty,
                    JournalState::Committed,
                    &format!(r"C:\old\{i}.jpg"),
                    Some(&format!(r"C:\old\sel\{i}.jpg")),
                )
            };
            text.push_str(&record.to_line());
            text.push_str("\r\n");
            model.apply(record);
        }
        fs.add_file(&path, text.as_bytes());
    }
    let steps = 120;

    for step in 0..steps {
        clock.set(clock_plus(&clock, 1));
        let action = rng.below(10);
        if action <= 2 || model.order.is_empty() {
            let fresh = fresh_record(&mut rng, &fs, &clock, next_id, pad_bytes);
            next_id += 1;
            model.record(&journal, fresh);
        } else if action <= 4 {
            // A later state (or a retry) under an existing Id.
            let previous = model.latest[&model.order[rng.below(model.order.len() as u64) as usize]].clone();
            let state = match rng.below(4) {
                0 => JournalState::Prepared,
                1 => JournalState::Committed,
                2 => JournalState::Failed,
                _ => JournalState::Dismissed,
            };
            let record = JournalEntry {
                state,
                timestamp_utc: JournalDateTime::utc(clock.utc_now()),
                error: if state == JournalState::Failed {
                    Some("boom".into())
                } else {
                    None
                },
                error_code: None,
                ..previous
            };
            model.record(&journal, record);
        } else if action == 5 {
            // Dismiss from a possibly stale copy of the entry.
            let previous = model.latest[&model.order[rng.below(model.order.len() as u64) as usize]].clone();
            journal.dismiss(std::slice::from_ref(&previous)).expect("dismiss");
            model.apply(JournalEntry {
                state: JournalState::Dismissed,
                ..previous
            });
        } else if action == 6 {
            // Crash mid-append: a torn last line, then a restarted process.
            let mut text = fs.file_content(&path).unwrap_or_default();
            text.extend_from_slice(b"{\"Id\":\"torn\",\"Ty");
            fs.add_file(&path, &text);
            journal = quiet_journal(&fs, &clock);
        } else if action == 7 {
            journal = quiet_journal(&fs, &clock); // restart without a crash
        } else {
            // Reconcile: verdicts from the documented file rules, applied to the model.
            let mut expected: HashMap<String, JournalState> = HashMap::new();
            for pending in model.latest.values().filter(|e| e.state == JournalState::Prepared) {
                let source_exists = fs.file_exists(Path::new(&pending.source));
                let destination_ok = pending
                    .destination
                    .as_deref()
                    .and_then(|d| fs.get_file_stat(Path::new(d)))
                    .is_some_and(|stat| stat.length as i64 == pending.size);
                let verdict = match pending.op_type {
                    FileOperationType::Recycle => {
                        if source_exists {
                            JournalState::Failed
                        } else {
                            JournalState::Committed
                        }
                    }
                    FileOperationType::Move => {
                        if !source_exists && destination_ok {
                            JournalState::Committed
                        } else {
                            JournalState::Failed
                        }
                    }
                    FileOperationType::Copy => {
                        if destination_ok {
                            JournalState::Committed
                        } else {
                            JournalState::Failed
                        }
                    }
                };
                expected.insert(pending.id.clone(), verdict);
            }

            let reconciled = journal.reconcile_pending_operations(None).expect("reconcile");

            assert_eq!(expected.len(), reconciled.len(), "seed {seed} step {step}");
            for verdict in &reconciled {
                assert_eq!(expected[&verdict.id], verdict.state, "seed {seed} step {step}");
                model.apply(verdict.clone());
            }
            assert!(read_pending(&journal).is_empty(), "seed {seed} step {step}");
        }

        let view: Vec<(String, JournalState)> = journal
            .read_pending_and_failed_operations()
            .expect("read")
            .into_iter()
            .map(|e| (e.id, e.state))
            .collect();
        assert_eq!(
            model.pending_and_failed(),
            view,
            "seed {seed} step {step}: pending and failed differ"
        );

        let file_len = fs.file_content(&path).map_or(0, |bytes| bytes.len());
        let reverse_reader = file_len as u64 >= FULL_SCAN_THRESHOLD_BYTES;
        let expected_moves: Vec<String> = if reverse_reader {
            let skip = model.committed_moves.len().saturating_sub(STARTUP_COMMITTED_MOVE_LIMIT);
            model.committed_moves[skip..].iter().map(|e| e.id.clone()).collect()
        } else {
            model.committed_moves.iter().map(|e| e.id.clone()).collect()
        };
        let got: Vec<String> = committed(&journal).into_iter().map(|e| e.id).collect();
        assert_eq!(
            got, expected_moves,
            "seed {seed} step {step}: committed moves differ (reverse reader: {reverse_reader})"
        );
    }
}

fn clock_plus(clock: &FakeClock, seconds: i64) -> DateTimeUtc {
    clock
        .utc_now()
        .checked_add_ticks(seconds * TICKS_PER_SECOND)
        .expect("in range")
}

fn fresh_record(
    rng: &mut XorShift,
    fs: &FakeFileSystem,
    clock: &FakeClock,
    next_id: usize,
    pad_bytes: usize,
) -> JournalEntry {
    let id = format!("op{next_id}");
    let ty = match rng.below(3) {
        0 => FileOperationType::Move,
        1 => FileOperationType::Copy,
        _ => FileOperationType::Recycle,
    };
    let source = format!(r"C:\photos\{id}.jpg");
    let destination = if ty == FileOperationType::Recycle {
        None
    } else {
        Some(format!(r"C:\photos\sel\{id}.jpg"))
    };
    if rng.chance(2) {
        fs.add_file(&source, b"xxxxxxxxxx");
    }
    if let Some(dest) = &destination {
        if rng.chance(2) {
            let len = if rng.chance(4) { 11 } else { 10 };
            fs.add_file(dest, &vec![b'x'; len]);
        }
    }
    let error = if pad_bytes > 0 {
        Some("p".repeat(pad_bytes))
    } else {
        None
    };
    JournalEntry {
        size: 10,
        last_write_utc: stamp(0),
        timestamp_utc: JournalDateTime::utc(clock.utc_now()),
        error,
        ..entry(&id, ty, JournalState::Prepared, &source, destination.as_deref())
    }
}

// C#: JournalModelTests.RandomHistory_MatchesReferenceModel (seeds 1..=25, no padding; one loop)
#[test]
fn random_history_matches_reference_model_seeds_1_to_25() {
    for seed in 1..=25 {
        random_history(seed, 0);
    }
}

// C#: JournalModelTests.RandomHistory_MatchesReferenceModel (seed 100, padded past 1 MB: the reverse tail reader)
#[test]
fn random_history_matches_reference_model_padded_seed_100() {
    random_history(100, 3500);
}

// ---------------------------------------------------------------------------------------------------------------
// JournalRecordRoundTripPropertyTests: a deterministic generator (no proptest crate).
// ---------------------------------------------------------------------------------------------------------------

const PROPERTY_CHARS: &[char] = &[
    'a',
    'b',
    'c',
    'X',
    'Y',
    'Z',
    '0',
    '1',
    '9',
    ' ',
    '_',
    '-',
    '.',
    ':',
    '\\',
    '/',
    '"',
    '\'',
    '\t',
    '\r',
    '\n',
    '\u{e9}',
    '\u{65e5}',
    '\u{0e01}',
    '{',
    '}',
    '[',
    ']',
    '\u{1F600}',
];

fn property_text(rng: &mut XorShift, max: u64) -> String {
    let len = rng.below(max + 1);
    let mut text: String = (0..len)
        .map(|_| PROPERTY_CHARS[rng.below(PROPERTY_CHARS.len() as u64) as usize])
        .collect();
    if rng.chance(6) {
        text.push('\u{1F600}');
    }
    text
}

fn property_path(rng: &mut XorShift) -> String {
    format!(r"C:\p\{}x", property_text(rng, 12))
}

fn property_opt(rng: &mut XorShift, make: &mut dyn FnMut(&mut XorShift) -> String) -> Option<String> {
    if rng.below(3) == 0 { None } else { Some(make(rng)) }
}

fn property_stamp(rng: &mut XorShift) -> JournalDateTime {
    loop {
        let ticks = rng.next_u64() % (DateTimeUtc::MAX.ticks() as u64 + 1);
        if let Some(value) = DateTimeUtc::from_ticks(ticks as i64) {
            return JournalDateTime::utc(value);
        }
    }
}

fn random_entry(rng: &mut XorShift) -> JournalEntry {
    let members = if rng.below(3) == 0 {
        None
    } else {
        let count = rng.below(4);
        Some(
            (0..count)
                .map(|_| JournalGroupMember {
                    source: property_path(rng),
                    destination: property_opt(rng, &mut property_path),
                    size: (rng.next_u64() >> 1) as i64,
                    last_write_utc: property_stamp(rng),
                    permanent: rng.chance(2),
                })
                .collect(),
        )
    };
    let flag = |rng: &mut XorShift| match rng.below(3) {
        0 => None,
        1 => Some(true),
        _ => Some(false),
    };
    JournalEntry {
        id: format!("{}{}", uuid_like(rng), property_text(rng, 3)),
        op_type: match rng.below(3) {
            0 => FileOperationType::Move,
            1 => FileOperationType::Copy,
            _ => FileOperationType::Recycle,
        },
        state: match rng.below(4) {
            0 => JournalState::Prepared,
            1 => JournalState::Committed,
            2 => JournalState::Failed,
            _ => JournalState::Dismissed,
        },
        source: property_path(rng),
        destination: property_opt(rng, &mut property_path),
        size: match rng.below(4) {
            0 => 0,
            1 => i64::MAX,
            2 => -1,
            _ => rng.next_u64() as i64,
        },
        last_write_utc: property_stamp(rng),
        timestamp_utc: property_stamp(rng),
        error: property_opt(rng, &mut |r| property_text(r, 30)),
        error_code: property_opt(rng, &mut |r| property_text(r, 8)),
        permanent: flag(rng),
        undo: flag(rng),
        group_id: property_opt(rng, &mut |r| property_text(r, 8)),
        group_members: members,
    }
}

fn uuid_like(rng: &mut XorShift) -> String {
    format!("{:016x}{:016x}", rng.next_u64(), rng.next_u64())
}

// C#: JournalRecordRoundTripPropertyTests.SerializedEntry_ReadsBackEqual (400 iterations)
#[test]
fn serialized_entry_reads_back_equal() {
    let mut rng = XorShift::new(0x5EED);
    for iteration in 0..400 {
        let record = random_entry(&mut rng);
        let line = record.to_line();
        assert!(
            !line.contains('\n'),
            "iteration {iteration}: one journal record == one line"
        );

        let parsed = journal_line::try_parse(&line).unwrap_or_else(|| panic!("iteration {iteration}: {line}"));
        assert_eq!(record, parsed, "iteration {iteration}");
        assert_eq!(hash_of(&record), hash_of(&parsed), "iteration {iteration}");
        assert_eq!(
            Some(record.clone()),
            journal_line::try_parse_utf8(line.as_bytes()),
            "iteration {iteration}"
        );
        assert_eq!(
            line,
            parsed.to_line(),
            "iteration {iteration}: re-serialising is byte-stable"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Json/JsonFormatCompatTests: the three journal tests (the session and config tests are W2-12).
// ---------------------------------------------------------------------------------------------------------------

// C#: JsonFormatCompatTests.JournalLine_Bytes_MatchPreviousBuild
#[test]
fn journal_line_bytes_match_previous_build() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let record = JournalEntry {
        size: 10,
        permanent: Some(true),
        last_write_utc: JournalDateTime::utc(base()),
        timestamp_utc: JournalDateTime::utc(base()),
        ..entry(
            "id-1",
            FileOperationType::Move,
            JournalState::Committed,
            r"C:\old\a.jpg",
            Some(r"C:\sel\a.jpg"),
        )
    };

    journal.append(&record).expect("append");

    let expected = concat!(
        r#"{"Id":"id-1","Type":"Move","State":"Committed","Source":"C:\\old\\a.jpg","#,
        r#""Destination":"C:\\sel\\a.jpg","Size":10,"LastWriteUtc":"2026-09-25T00:00:00Z","#,
        r#""TimestampUtc":"2026-09-25T00:00:00Z","Error":null,"Permanent":true}"#,
        "\r\n"
    );
    let bytes = fs.file_content(fake_journal_path()).expect("journal bytes");
    assert_eq!(String::from_utf8(bytes).expect("utf-8"), expected);
}

// C#: JsonFormatCompatTests.JournalLine_LegacyGroupLine_Parses
#[test]
fn journal_line_legacy_group_line_parses() {
    let legacy = r#"{"Id":"g-1","Type":"Copy","State":"Prepared","Source":"C:\\a\\x.jpg","Destination":null,"Size":5,"LastWriteUtc":"2026-09-01T00:00:00Z","TimestampUtc":"2026-09-01T00:00:00Z","GroupId":"grp","GroupMembers":[{"Source":"C:\\a\\x.jpg","Destination":"C:\\b\\x.jpg","Size":5,"LastWriteUtc":"2026-09-01T00:00:00Z","Permanent":false}]}"#;

    let parsed = journal_line::try_parse(legacy).expect("legacy group line parses");

    assert_eq!(parsed.id, "g-1");
    assert_eq!(parsed.op_type, FileOperationType::Copy);
    assert_eq!(parsed.state, JournalState::Prepared);
    assert_eq!(parsed.group_id.as_deref(), Some("grp"));
    let members = parsed.group_members.expect("members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].destination.as_deref(), Some(r"C:\b\x.jpg"));
    assert_eq!(members[0].size, 5);
}

// C#: JsonFormatCompatTests.JournalLine_AppendThenParse_RoundTrips
#[test]
fn journal_line_append_then_parse_round_trips() {
    let (clock, fs) = new_fake();
    let journal = journal_on(&fs, &clock);
    let written = JournalEntry {
        size: 7,
        undo: Some(true),
        group_id: Some("g".into()),
        ..entry(
            "rt-1",
            FileOperationType::Move,
            JournalState::Prepared,
            "C:\\old\\\u{e7}.jpg",
            None,
        )
    };

    journal.append(&written).expect("append");
    let text = String::from_utf8(fs.file_content(fake_journal_path()).expect("bytes")).expect("utf-8");
    let line = text.trim_end_matches(['\r', '\n']);

    assert_eq!(journal_line::try_parse(line), Some(written));
}

// ---------------------------------------------------------------------------------------------------------------
// Goldens: journal-stream-lines, journal-ops, journal-committed-moves (the W3-01 oracle, tests/golden/*.json).
// ---------------------------------------------------------------------------------------------------------------

// Golden: journal-stream-lines (the StreamReader line rules of ReadEntries, UTF-16 code units per line)
#[test]
fn golden_journal_stream_lines() {
    let golden = golden::load("journal-stream-lines");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let lines = decode_stream_reader_lines(&unhex(str_of(case, "hex")));
        let got: Vec<String> = lines
            .iter()
            .map(|line| {
                line.encode_utf16()
                    .map(|unit| format!("{unit:04X}"))
                    .collect::<String>()
            })
            .collect();
        let want: Vec<String> = case["lines_utf16"]
            .as_array()
            .expect("lines_utf16")
            .iter()
            .map(|v| v.as_str().expect("utf16 text").to_owned())
            .collect();
        assert_eq!(got, want, "case {name}");
    }
}

// Golden: journal-ops (one OperationJournal, scripted calls on a fake file; append opens, reads, dismissals, guards)
#[test]
fn golden_journal_ops() {
    let golden = golden::load("journal-ops");
    for case in &golden.cases {
        replay_ops_case(case);
    }
}

fn replay_ops_case(case: &Value) {
    let name = str_of(case, "name");
    let (clock, fs) = new_fake();
    let paths = fake_paths();
    let path = paths.journal_file.clone();
    fs.add_dir(path.parent().expect("parent"));
    if case["initial"].is_array() {
        fs.add_file(&path, &specs_bytes(&case["initial"]));
    }
    let journal = quiet_journal(&fs, &clock);

    for (op, step) in case["ops"]
        .as_array()
        .expect("ops")
        .iter()
        .zip(case["steps"].as_array().expect("steps"))
    {
        if let Some(now) = op.get("now").and_then(Value::as_i64) {
            clock.set(at(now));
        }
        let fail_append = op.get("fail_append").and_then(Value::as_u64).unwrap_or(0);
        if fail_append > 0 {
            fs.fail_times(
                FsOp::OpenAppend,
                &path,
                io::ErrorKind::Other,
                usize::try_from(fail_append).expect("count"),
            );
        }
        let opens_before = fs.count(FsOp::OpenAppend);
        let kind = str_of(op, "op");
        let outcome: Result<Value, io::Error> = match kind {
            "append" => journal
                .append(&parse_entry(str_of(op, "entry")))
                .map(|()| serde_json::json!({})),
            "append_raw" => {
                let mut bytes = fs.file_content(&path).unwrap_or_default();
                bytes.extend_from_slice(&specs_bytes(&op["bytes"]));
                fs.add_file(&path, &bytes);
                Ok(serde_json::json!({}))
            }
            "dismiss" => {
                let entries: Vec<JournalEntry> = op["entries"]
                    .as_array()
                    .expect("entries")
                    .iter()
                    .map(|v| parse_entry(v.as_str().expect("line")))
                    .collect();
                journal.dismiss(&entries).map(|o: DismissOutcome| {
                    serde_json::json!({
                        "dismissed": o.dismissed.iter().map(JournalEntry::to_line).collect::<Vec<_>>(),
                        "skipped": o.skipped.iter().map(JournalEntry::to_line).collect::<Vec<_>>(),
                    })
                })
            }
            "read_committed_moves" => committed_lines(journal.read_committed_moves()),
            "read_pending_and_failed" => committed_lines(journal.read_pending_and_failed_operations()),
            "read_latest" => journal
                .read_latest_entry(str_of(op, "id"))
                .map(|latest| serde_json::json!({ "entry": latest.map(|e| e.to_line()) })),
            "append_if_unchanged_since" => {
                let own: Vec<JournalEntry> = op["own_since"]
                    .as_array()
                    .expect("own_since")
                    .iter()
                    .map(|v| parse_entry(v.as_str().expect("line")))
                    .collect();
                journal
                    .append_if_unchanged_since(
                        &parse_entry(str_of(op, "anchor")),
                        &own,
                        &parse_entry(str_of(op, "entry")),
                    )
                    .map(|appended| serde_json::json!({ "appended": appended }))
            }
            other => panic!("case {name}: unknown op {other}"),
        };
        fs.clear_faults();
        let appends = fs.count(FsOp::OpenAppend) - opens_before;

        assert_eq!(
            step["append_opens"].as_u64(),
            Some(appends as u64),
            "case {name}: append_opens of {kind}"
        );
        if step.get("throws").is_some() {
            assert!(outcome.is_err(), "case {name}: {kind} must throw");
        } else {
            let got = outcome.unwrap_or_else(|e| panic!("case {name}: {kind} failed: {e}"));
            for key in ["entries", "entry", "dismissed", "skipped", "appended"] {
                if let Some(want) = step.get(key) {
                    assert_eq!(got.get(key), Some(want), "case {name}: {kind} {key}");
                }
            }
        }
    }

    let exists = fs.file_exists(&path);
    assert_eq!(
        case["final_exists"].as_bool(),
        Some(exists),
        "case {name}: final_exists"
    );
    if exists {
        let bytes = fs.file_content(&path).expect("final bytes");
        assert_eq!(
            case["final_hex"].as_str(),
            Some(hex_upper(&bytes).as_str()),
            "case {name}: final_hex"
        );
    }
}

/// The lines of a read result (`{"entries": [line]}`).
fn committed_lines(result: io::Result<Vec<JournalEntry>>) -> Result<Value, io::Error> {
    result.map(|list| serde_json::json!({ "entries": list.iter().map(JournalEntry::to_line).collect::<Vec<_>>() }))
}

// Golden: journal-committed-moves (ReadCommittedMoves over generated journals; the full and the tail reader)
#[test]
fn golden_journal_committed_moves() {
    let golden = golden::load("journal-committed-moves");
    for case in &golden.cases {
        replay_committed_moves_case(case);
    }
}

/// The biggest case of the golden on a real file under a `TempRoot` (native twin of `golden_journal_committed_moves`).
#[test]
#[ignore = "native: a 4 MB generated journal on a real file"]
fn golden_journal_committed_moves_native_biggest_case() {
    let golden = golden::load("journal-committed-moves");
    let case = golden
        .cases
        .iter()
        .find(|c| str_of(c, "name") == "dense-moves")
        .expect("dense-moves case");
    let root = TempRoot::new("journal-committed-golden");
    let bytes = build_generated(case);
    std::fs::write(root.join("operations.jsonl"), &bytes).expect("write native journal");
    let paths = AppPaths {
        journal_file: root.join("operations.jsonl"),
        ..native_paths(&root)
    };
    let journal = OperationJournal::new(&paths, Arc::new(PhysicalFileSystem), Arc::new(SystemClock));
    check_committed_moves(case, &journal, &bytes);
}

fn replay_committed_moves_case(case: &Value) {
    let (clock, fs) = new_fake();
    let bytes = build_generated(case);
    let path = fake_journal_path();
    fs.add_dir(path.parent().expect("parent"));
    fs.add_file(&path, &bytes);
    let journal = OperationJournal::new(&fake_paths(), fs.clone(), clock.clone());
    check_committed_moves(case, &journal, &bytes);
}

/// The golden checks in the order the oracle reports them: the generator's length and fingerprint first (so a generator
/// mismatch is not mistaken for a reader bug), then the committed Moves a fresh journal reads back.
fn check_committed_moves(case: &Value, journal: &OperationJournal, bytes: &[u8]) {
    let name = str_of(case, "name");
    assert_eq!(
        bytes.len() as i64,
        i64_of(case, "length"),
        "case {name}: generated length"
    );
    assert_eq!(
        fnv64(bytes),
        str_of(case, "fnv64"),
        "case {name}: generated fingerprint"
    );

    let moves = committed(journal);
    assert_eq!(moves.len() as i64, i64_of(case, "count"), "case {name}: count");
    let ids: Vec<String> = moves.iter().map(|e| e.id.clone()).collect();
    let want_ids: Vec<String> = case["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().expect("id").to_owned())
        .collect();
    assert_eq!(ids, want_ids, "case {name}: ids");
    // The oracle's `last_line` is `JsonSerializer.Serialize(moves[^1])` (null without moves): the LAST MOVE READ, not
    // the file's last line (they differ when the file ends in a torn or non-Move line).
    let last_line = moves.last().map(JournalEntry::to_line);
    assert_eq!(
        last_line.as_deref(),
        case["last_line"].as_str(),
        "case {name}: last_line"
    );
}
