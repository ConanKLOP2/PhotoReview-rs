//! Tests for `pr_core::session::store` (port of `PhotoReview.Core/Session/SessionStore.cs`, C# baseline d92d6aae).
//! The goldens `session-key`, `session-json-write` and `session-json-read` come from the C# oracle
//! (`tools/oracle-cs/Commands/W2-12.cs`) and are checked case by case.
//!
//! C# tests with NO Rust twin (reasons):
//! * `SessionStoreTests.Constructor_NullValidation`: `&AppPaths` / `Arc<dyn FileSystem>` cannot be null.
//! * `SessionStoreTests.Save_UnexpectedFileSystemFailure_DoesNotThrow` rows `NotSupportedException` and
//!   `InvalidOperationException`: no Rust error type; only the `ArgumentException` image (`InvalidInput`) is ported.
//! * `SessionStoreLogTests.*LoggerThrows*` / `*LoggerRunsOutOfMemory*`: a `Log` method that unwinds is a bug (R0003).
//! * `SessionStoreSweepPerfTests` `< 50 ms` asserts: wall-clock timing, replaced by the deterministic gate test
//!   `save_and_load_while_startup_sweep_still_running_are_unaffected`.
//! * `SessionWritePolicyTests.SettingsSave_UsesDurableAtomicWrite` (W2-03), `PhysicalNonDurableWrite_*` (W2-14) and
//!   `Measure_SessionSaveLatency_*` (Manual).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{FileStat, FileSystem, Log, ReadSeek};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::io::physical::{PhysicalFileSystem, decode_text_like_dotnet};
use pr_core::session::store::{
    BlankFolder, STALE_TEMP_AGE_TICKS, SessionPathError, SessionState, SessionStore, SessionWriteCounter,
};
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_test_support::{FakeFileSystem, FsOp, LogLevel, RecordingLog, TempRoot};
use serde_json::Value;

const SESSIONS_DIR: &str = r"C:\data\Sessions";
const FOLDER: &str = r"C:\photos\shoot";
const DAY_TICKS: i64 = 864_000_000_000;

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

fn paths() -> AppPaths {
    AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), Some(r"C:\data"), false)
}

fn new_store(fs: &Arc<FakeFileSystem>) -> SessionStore {
    let fs: Arc<dyn FileSystem> = fs.clone();
    SessionStore::new(&paths(), fs, None)
}

fn sessions_dir() -> PathBuf {
    PathBuf::from(SESSIONS_DIR)
}

fn ticks(value: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(value).expect("ticks in range")
}

fn now_minus_days(days: i64) -> DateTimeUtc {
    DateTimeUtc::now()
        .checked_add_ticks(-days * DAY_TICKS)
        .expect("in range")
}

/// C# `new DateTime(2026, 9, 18, 10, 30, 0, DateTimeKind.Utc)`.
fn stamp() -> JournalDateTime {
    JournalDateTime::parse_json("2026-09-18T10:30:00Z").expect("stamp parses")
}

fn warnings(log: &RecordingLog) -> Vec<String> {
    log.entries()
        .into_iter()
        .filter(|(level, _)| *level == LogLevel::Warn)
        .map(|(_, message)| message)
        .collect()
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex"))
        .collect()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Test-local decorator: records the `durable` argument of every atomic write and can hold the first `get_file_stat`
/// (the sweep's) until the test opens the gate.
struct Decorator {
    inner: Arc<FakeFileSystem>,
    atomic_writes: Mutex<Vec<(PathBuf, bool)>>,
    gate: Option<Arc<(Mutex<bool>, Condvar)>>,
}

impl Decorator {
    fn recording(inner: Arc<FakeFileSystem>) -> Self {
        Self {
            inner,
            atomic_writes: Mutex::new(Vec::new()),
            gate: None,
        }
    }

    fn gated(inner: Arc<FakeFileSystem>, gate: Arc<(Mutex<bool>, Condvar)>) -> Self {
        Self {
            inner,
            atomic_writes: Mutex::new(Vec::new()),
            gate: Some(gate),
        }
    }
}

impl FileSystem for Decorator {
    fn file_exists(&self, path: &Path) -> bool {
        self.inner.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        if let Some(gate) = &self.gate {
            let (open, changed) = &**gate;
            let guard = open.lock().unwrap_or_else(PoisonError::into_inner);
            // Bounded: a test that forgets to open the gate must not hang the sweep thread for ever.
            let _ = changed
                .wait_timeout_while(guard, Duration::from_secs(10), |opened| !*opened)
                .unwrap_or_else(PoisonError::into_inner);
        }
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
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn io::Write + Send>> {
        self.inner.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.atomic_writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((path.to_path_buf(), durable));
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

// ---------------------------------------------------------------------------------------------------------------
// SessionStoreTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn save_and_load_round_trips() {
    // C#: SessionStoreTests.SaveAndLoad_RoundTrips
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\vacation";
    let state = SessionState {
        folder: folder.to_owned(),
        current_path: Some(r"C:\photos\vacation\img1.jpg".to_owned()),
        skipped: vec![
            r"C:\photos\vacation\skip1.jpg".to_owned(),
            r"C:\photos\vacation\skip2.jpg".to_owned(),
        ],
        updated_utc: stamp(),
    };

    assert_eq!(store.save(&state), Ok(()));
    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded.folder, folder);
    assert_eq!(loaded.current_path, state.current_path);
    assert_eq!(loaded.skipped.len(), 2);
    assert!(loaded.skipped.contains(&r"C:\photos\vacation\skip1.jpg".to_owned()));
    assert!(loaded.skipped.contains(&r"C:\photos\vacation\skip2.jpg".to_owned()));
    assert_eq!(loaded, state);
}

#[test]
fn load_missing_file_returns_default() {
    // C#: SessionStoreTests.Load_MissingFile_ReturnsDefault
    let store = new_store(&Arc::new(FakeFileSystem::new()));
    let folder = r"C:\photos\nonexistent";

    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded.folder, folder);
    assert_eq!(loaded.current_path, None);
    assert!(loaded.skipped.is_empty());
}

#[test]
fn load_folder_with_embedded_nul_returns_no_session() {
    // C#: SessionStoreTests.Load_FolderWithEmbeddedNul_ReturnsNoSession
    let store = new_store(&Arc::new(FakeFileSystem::new()));
    let folder = "C:\\photos\\bad\0name";

    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded, SessionState::new(folder));
    assert_eq!(store.get_path(folder), Err(SessionPathError::Invalid));
}

#[test]
fn load_overlong_path_returns_no_session() {
    // C#: SessionStoreTests.Load_OverlongPath_ReturnsNoSession
    let store = new_store(&Arc::new(FakeFileSystem::new()));
    let folder = format!(r"C:\{}", "a".repeat(40_000));

    let loaded = store.load(&folder).expect("load");

    assert_eq!(loaded.folder, folder);
    assert_eq!(loaded.current_path, None);
    assert_eq!(store.get_path(&folder), Err(SessionPathError::TooLong));
}

#[test]
fn load_corrupt_json_returns_default() {
    // C#: SessionStoreTests.Load_CorruptJson_ReturnsDefault
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\corrupt";
    fs.add_file(store.get_path(folder).expect("path"), b"{ invalid json content");

    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded, SessionState::new(folder));
}

#[test]
fn load_json_without_folder_assigns_folder() {
    // C#: SessionStoreTests.Load_JsonWithoutFolder_AssignsFolder
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\nofolder";
    let json = "{\n    \"CurrentPath\": \"C:\\\\photos\\\\nofolder\\\\test.jpg\",\n    \"Skipped\": [\"C:\\\\photos\\\\nofolder\\\\skip.jpg\"],\n    \"UpdatedUtc\": \"2026-09-18T00:00:00Z\"\n}";
    fs.add_file(store.get_path(folder).expect("path"), json.as_bytes());

    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded.folder, folder);
    assert_eq!(loaded.current_path.as_deref(), Some(r"C:\photos\nofolder\test.jpg"));
    assert_eq!(loaded.skipped.len(), 1);
}

#[test]
fn save_creates_sessions_directory() {
    // C#: SessionStoreTests.Save_CreatesSessionsDirectory
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    store.wait_for_startup_sweep();
    assert!(!fs.directory_exists(&sessions_dir()));

    store.save(&SessionState::new(r"C:\photos\newdir")).expect("save");

    assert!(fs.directory_exists(&sessions_dir()));
}

#[test]
fn get_path_canonicalization() {
    // C#: SessionStoreTests.GetPath_Canonicalization
    let store = new_store(&Arc::new(FakeFileSystem::new()));

    let path1 = store.get_path(r"C:\Photos\Album").expect("path");
    let path2 = store.get_path(r"c:\photos\album\").expect("path");
    let path3 = store.get_path(r"C:\photos\album").expect("path");

    assert_eq!(path1, path2);
    assert_eq!(path1, path3);
}

#[test]
fn blank_folder_is_the_only_error_of_load_and_save() {
    // C#: ArgumentException.ThrowIfNullOrWhiteSpace in Load/Save/GetPath
    let store = new_store(&Arc::new(FakeFileSystem::new()));

    assert_eq!(store.load(""), Err(BlankFolder));
    assert_eq!(store.load("   "), Err(BlankFolder));
    assert_eq!(store.save(&SessionState::new("")), Err(BlankFolder));
    assert_eq!(store.save(&SessionState::new(" \t ")), Err(BlankFolder));
    assert_eq!(store.get_path(""), Err(SessionPathError::Blank));
    assert_eq!(SessionStore::canonical_folder("  "), Err(SessionPathError::Blank));
}

#[test]
fn constructor_sweeps_stale_temp_files() {
    // C#: SessionStoreTests.Constructor_SweepsStaleTempFiles
    let fs = Arc::new(FakeFileSystem::new());
    let old = now_minus_days(3);
    fs.add_file_at(format!(r"{SESSIONS_DIR}\abc.json.1111.tmp"), b"x", old);
    fs.add_file_at(format!(r"{SESSIONS_DIR}\abc.json.2222.tmp"), b"x", DateTimeUtc::now());
    fs.add_file_at(format!(r"{SESSIONS_DIR}\abc.json"), b"{}", old);

    let store = new_store(&fs);
    store.wait_for_startup_sweep();

    assert!(!fs.file_exists(Path::new(&format!(r"{SESSIONS_DIR}\abc.json.1111.tmp"))));
    assert!(fs.file_exists(Path::new(&format!(r"{SESSIONS_DIR}\abc.json.2222.tmp"))));
    assert!(fs.file_exists(Path::new(&format!(r"{SESSIONS_DIR}\abc.json"))));
}

#[test]
fn constructor_sweep_continues_after_one_delete_fails() {
    // C#: SessionStoreTests.Constructor_SweepContinuesAfterOneDeleteFails
    let fs = Arc::new(FakeFileSystem::new());
    let old = now_minus_days(3);
    let a = format!(r"{SESSIONS_DIR}\a.json.1111.tmp");
    let b = format!(r"{SESSIONS_DIR}\b.json.2222.tmp");
    fs.add_file_at(&a, b"x", old);
    fs.add_file_at(&b, b"x", old);
    fs.fail(FsOp::Delete, &a, io::ErrorKind::Other);

    let store = new_store(&fs);
    store.wait_for_startup_sweep();

    assert!(fs.file_exists(Path::new(&a)));
    assert!(!fs.file_exists(Path::new(&b)));
}

#[test]
fn load_null_skipped_becomes_empty_list() {
    // C#: SessionStoreTests.Load_NullSkipped_BecomesEmptyList
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\nullskip";
    let json = r#"{"Folder":"C:\\photos\\nullskip","CurrentPath":"C:\\photos\\nullskip\\a.jpg","Skipped":null}"#;
    fs.add_file(store.get_path(folder).expect("path"), json.as_bytes());

    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded.current_path.as_deref(), Some(r"C:\photos\nullskip\a.jpg"));
    assert!(loaded.skipped.is_empty());
}

#[test]
fn save_and_load_while_startup_sweep_still_running_are_unaffected() {
    // C#: SessionStoreTests.SaveAndLoad_WhileStartupSweepStillRunning_AreUnaffected
    let fake = Arc::new(FakeFileSystem::new());
    let old = now_minus_days(3);
    let stale1 = format!(r"{SESSIONS_DIR}\stale1.json.1111.tmp");
    let stale2 = format!(r"{SESSIONS_DIR}\stale2.json.2222.tmp");
    fake.add_file_at(&stale1, b"x", old);
    fake.add_file_at(&stale2, b"x", old);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let decorated: Arc<dyn FileSystem> = Arc::new(Decorator::gated(Arc::clone(&fake), Arc::clone(&gate)));
    let store = Arc::new(SessionStore::new(&paths(), decorated, None));
    let folder = r"C:\photos\concurrent";
    let state = SessionState {
        folder: folder.to_owned(),
        current_path: Some(r"C:\photos\concurrent\a.jpg".to_owned()),
        ..SessionState::default()
    };

    // Save and load run while the sweep sits on its first stat; a regression fails by timeout instead of hanging.
    let (tx, rx) = mpsc::channel();
    let worker_store = Arc::clone(&store);
    let worker_state = state.clone();
    std::thread::spawn(move || {
        let saved = worker_store.save(&worker_state);
        let loaded = worker_store.load(&worker_state.folder);
        let _ = tx.send((saved, loaded));
    });
    let (saved, loaded) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("save/load must not wait for the startup sweep");
    assert_eq!(saved, Ok(()));
    assert_eq!(
        loaded.expect("load").current_path.as_deref(),
        Some(r"C:\photos\concurrent\a.jpg")
    );

    {
        let (open, changed) = &*gate;
        *open.lock().unwrap_or_else(PoisonError::into_inner) = true;
        changed.notify_all();
    }
    store.wait_for_startup_sweep();

    assert!(!fake.file_exists(Path::new(&stale1)));
    assert!(!fake.file_exists(Path::new(&stale2)));
    assert!(fake.file_exists(&store.get_path(folder).expect("path")));
}

#[test]
fn get_path_keeps_the_persisted_file_name_scheme() {
    // C#: SessionStoreTests.GetPath_KeepsThePersistedFileNameScheme (3 rows)
    let store = new_store(&Arc::new(FakeFileSystem::new()));
    // SHA-256 of UTF-8 `C:\PHOTOS\VACATION` (the same key as golden session-key case 1).
    let expected_key = "38AC4A4FD7B17B4BB90E42A1D8509D9ADED80C1505F4CD2663E3F60B3BA39D6A";
    for spelling in [r"C:\photos\vacation", r"C:\photos\vacation\", r"c:\PHOTOS\Vacation/"] {
        assert_eq!(
            store.get_path(spelling).expect("path"),
            sessions_dir().join(format!("{expected_key}.json")),
            "{spelling}"
        );
    }
}

#[test]
fn save_unexpected_file_system_failure_does_not_throw() {
    // C#: SessionStoreTests.Save_UnexpectedFileSystemFailure_DoesNotThrow (ArgumentException row; the other two rows
    // have no Rust error type).
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\vacation";
    fs.fail(
        FsOp::WriteAtomic,
        store.get_path(folder).expect("path"),
        io::ErrorKind::InvalidInput,
    );

    assert_eq!(store.save(&SessionState::new(folder)), Ok(()));

    fs.clear_faults();
    assert_eq!(store.load(folder).expect("load").current_path, None);
}

// ---------------------------------------------------------------------------------------------------------------
// SessionStoreAccessDeniedTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn load_read_fails_returns_an_empty_session_for_the_folder() {
    // C#: SessionStoreAccessDeniedTests.Load_ReadFails_ReturnsAnEmptySessionForTheFolder (2 rows)
    for kind in [io::ErrorKind::Other, io::ErrorKind::PermissionDenied] {
        let fs = Arc::new(FakeFileSystem::new());
        let store = new_store(&fs);
        store
            .save(&SessionState {
                folder: FOLDER.to_owned(),
                current_path: Some(format!(r"{FOLDER}\a.jpg")),
                skipped: vec![format!(r"{FOLDER}\b.jpg")],
                updated_utc: JournalDateTime::default(),
            })
            .expect("save");
        assert_eq!(
            store.load(FOLDER).expect("load").current_path,
            Some(format!(r"{FOLDER}\a.jpg")),
            "precondition: the session is on disk"
        );
        fs.fail(FsOp::ReadText, store.get_path(FOLDER).expect("path"), kind);

        let loaded = store.load(FOLDER).expect("load");

        assert_eq!(loaded.folder, FOLDER, "{kind:?}");
        assert_eq!(loaded.current_path, None);
        assert!(loaded.skipped.is_empty());
    }
}

#[test]
fn sweep_listing_denied_returns_zero() {
    // C#: SessionStoreAccessDeniedTests.Sweep_ListingDenied_ReturnsZero
    let fs = Arc::new(FakeFileSystem::new());
    fs.fail(FsOp::EnumerateFiles, SESSIONS_DIR, io::ErrorKind::PermissionDenied);
    let store = new_store(&fs);
    store.wait_for_startup_sweep(); // the constructor sweep itself must not fault
    let old_tmp = format!(r"{SESSIONS_DIR}\old.tmp");
    fs.add_file_at(&old_tmp, b"x", now_minus_days(3));

    let removed = store.sweep_stale_temp_files(DateTimeUtc::now());

    assert_eq!(removed, 0);
    assert!(fs.file_exists(Path::new(&old_tmp)));
}

#[test]
fn sweep_delete_denied_for_one_file_others_still_removed() {
    // C#: SessionStoreAccessDeniedTests.Sweep_DeleteDeniedForOneFile_OthersStillRemoved
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    store.wait_for_startup_sweep();
    let old = now_minus_days(3);
    let a = format!(r"{SESSIONS_DIR}\a.tmp");
    let b = format!(r"{SESSIONS_DIR}\b.tmp");
    fs.add_file_at(&a, b"x", old);
    fs.add_file_at(&b, b"x", old);
    fs.fail(FsOp::Delete, &a, io::ErrorKind::PermissionDenied);

    let removed = store.sweep_stale_temp_files(DateTimeUtc::now());

    assert_eq!(removed, 1);
    assert!(fs.file_exists(Path::new(&a)));
    assert!(!fs.file_exists(Path::new(&b)));
}

// ---------------------------------------------------------------------------------------------------------------
// SessionStoreEnvironmentRootTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: real disk under the temp folder"]
fn session_save_and_load_over_a_data_root() {
    // C#: SessionStoreEnvironmentRootTests.SessionSaveAndLoad (the data root is passed in; the environment is not set)
    let root = TempRoot::new("session-store");
    let root_text = root.path().to_string_lossy().into_owned();
    let store = SessionStore::new(
        &AppPaths::new(root.path(), Some(&root_text), true),
        Arc::new(PhysicalFileSystem),
        None,
    );
    let state = SessionState {
        folder: root_text.clone(),
        current_path: Some(root.join("one.jpg").to_string_lossy().into_owned()),
        skipped: vec![root.join("skip.jpg").to_string_lossy().into_owned()],
        updated_utc: JournalDateTime::default(),
    };

    store.save(&state).expect("save");
    let loaded = store.load(&root_text).expect("load");

    assert_eq!(loaded.current_path, state.current_path);
    assert_eq!(loaded.skipped.len(), 1);
    assert!(
        store
            .get_path(&root_text)
            .expect("path")
            .starts_with(root.path().join("Sessions"))
    );
}

#[test]
#[ignore = "native: reads the real environment"]
fn from_environment_constructs_and_names_a_session_file() {
    // C#: SessionStoreEnvironmentRootTests (default constructor); only constructs and calls get_path.
    let store = SessionStore::from_environment();
    store.wait_for_startup_sweep();

    let path = store.get_path(r"C:\photos\vacation").expect("path");

    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("38AC4A4FD7B17B4BB90E42A1D8509D9ADED80C1505F4CD2663E3F60B3BA39D6A.json")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// SessionStoreLogTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn save_write_fails_does_not_throw_and_warns_with_folder_and_reason() {
    // C#: SessionStoreLogTests.Save_WriteFails_DoesNotThrowAndWarnsWithFolderAndReason
    let fs = Arc::new(FakeFileSystem::new());
    let log = Arc::new(RecordingLog::new());
    let store = new_store(&fs).with_log(log.clone());
    fs.fail(
        FsOp::WriteAtomic,
        store.get_path(FOLDER).expect("path"),
        io::ErrorKind::Other,
    );

    assert_eq!(store.save(&SessionState::new(FOLDER)), Ok(()));

    let all = warnings(&log);
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(all[0].contains(FOLDER), "{}", all[0]);
    assert!(all[0].contains("injected WriteAtomic fault"), "{}", all[0]);
    assert!(all[0].starts_with(&format!("Session write failed for '{FOLDER}': ")));
}

#[test]
fn load_corrupt_file_returns_empty_session_and_warns_with_the_path() {
    // C#: SessionStoreLogTests.Load_CorruptFile_ReturnsEmptySessionAndWarnsWithThePath
    let fs = Arc::new(FakeFileSystem::new());
    let log = Arc::new(RecordingLog::new());
    let store = new_store(&fs).with_log(log.clone());
    let path = store.get_path(FOLDER).expect("path");
    fs.add_file(&path, b"{ not json");

    let state = store.load(FOLDER).expect("load");

    assert_eq!(state, SessionState::new(FOLDER));
    let all = warnings(&log);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(
        all[0],
        format!(
            "Session file '{}' is empty or corrupt; starting without a session.",
            path.display()
        )
    );
}

// ---------------------------------------------------------------------------------------------------------------
// SessionStoreMetricsTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn save_is_counted_load_is_not() {
    // C#: SessionStoreMetricsTests.SaveIsCountedLoadIsNot
    let writes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&writes);
    let metrics: Arc<dyn SessionWriteCounter> = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let fs: Arc<dyn FileSystem> = Arc::new(FakeFileSystem::new());
    let store = SessionStore::new(&paths(), fs, Some(metrics));
    let state = SessionState {
        folder: r"C:\photos".to_owned(),
        current_path: Some(r"C:\photos\a.jpg".to_owned()),
        ..SessionState::default()
    };

    store.save(&state).expect("save");
    store.save(&state).expect("save");
    store.load(r"C:\photos").expect("load");

    assert_eq!(writes.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_save_is_not_counted() {
    // C#: SessionStore.Save calls RecordSessionWrite only after WriteAllTextAtomic returned (mutation: metrics first).
    let fake = Arc::new(FakeFileSystem::new());
    let writes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&writes);
    let metrics: Arc<dyn SessionWriteCounter> = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let fs: Arc<dyn FileSystem> = fake.clone();
    let store = SessionStore::new(&paths(), fs, Some(metrics));
    fake.fail(
        FsOp::WriteAtomic,
        store.get_path(FOLDER).expect("path"),
        io::ErrorKind::Other,
    );

    store.save(&SessionState::new(FOLDER)).expect("save");

    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[test]
fn save_without_metrics_still_works() {
    // C#: SessionStoreMetricsTests.SaveWithoutMetricsStillWorks
    let store = new_store(&Arc::new(FakeFileSystem::new()));

    store.save(&SessionState::new(r"C:\photos")).expect("save");

    assert_eq!(store.load(r"C:\photos").expect("load").folder, r"C:\photos");
}

// ---------------------------------------------------------------------------------------------------------------
// SessionStoreMutationGapTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn sweep_stale_temp_files_temp_exactly_one_day_old_is_kept_and_older_one_is_removed() {
    // C#: SessionStoreMutationGapTests.SweepStaleTempFiles_TempExactlyOneDayOld_IsKeptAndOlderOneIsRemoved
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    store.wait_for_startup_sweep(); // the real-clock startup sweep must be over before the fixture is added
    let now = DateTimeUtc::from_ticks(639_040_000_000_000_000).expect("range");
    let exactly = format!(r"{SESSIONS_DIR}\exactly-a-day.tmp");
    let older = format!(r"{SESSIONS_DIR}\older.tmp");
    fs.add_file_at(
        &exactly,
        b"x",
        now.checked_add_ticks(-STALE_TEMP_AGE_TICKS).expect("range"),
    );
    fs.add_file_at(
        &older,
        b"x",
        now.checked_add_ticks(-STALE_TEMP_AGE_TICKS - 1).expect("range"),
    );

    let removed = store.sweep_stale_temp_files(now);

    assert_eq!(removed, 1);
    assert!(fs.file_exists(Path::new(&exactly)));
    assert!(!fs.file_exists(Path::new(&older)));
}

#[test]
fn sweep_never_removes_a_future_temp_file_or_a_json_file() {
    // C#: SweepStaleTempFiles: `utcNow - LastWriteUtc > StaleTempAge` is false for a future write time; only `*.tmp`.
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    store.wait_for_startup_sweep();
    let now = DateTimeUtc::from_ticks(639_040_000_000_000_000).expect("range");
    let future = format!(r"{SESSIONS_DIR}\future.tmp");
    let json = format!(r"{SESSIONS_DIR}\old.json");
    fs.add_file_at(&future, b"x", now.checked_add_ticks(5 * DAY_TICKS).expect("range"));
    fs.add_file_at(&json, b"{}", now.checked_add_ticks(-9 * DAY_TICKS).expect("range"));

    assert_eq!(store.sweep_stale_temp_files(now), 0);
    assert!(fs.file_exists(Path::new(&future)));
    assert!(fs.file_exists(Path::new(&json)));
}

#[test]
fn canonical_folder_trims_trailing_separators_except_on_a_drive_root() {
    // C#: SessionStoreMutationGapTests.CanonicalFolder_TrimsTrailingSeparatorsExceptOnADriveRoot (4 rows)
    for (folder, expected) in [
        (r"C:\", r"C:\"),
        (r"C:\photos", r"C:\photos"),
        (r"C:\photos\", r"C:\photos"),
        (r"C:\photos\\", r"C:\photos"),
    ] {
        assert_eq!(
            SessionStore::canonical_folder(folder).as_deref(),
            Ok(expected),
            "{folder}"
        );
    }
}

#[test]
fn get_path_folder_with_and_without_trailing_separator_is_the_same_file_but_drive_root_differs_from_its_child() {
    // C#: SessionStoreMutationGapTests.GetPath_FolderWithAndWithoutTrailingSeparator_IsTheSameFileButDriveRootDiffersFromItsChild
    let store = new_store(&Arc::new(FakeFileSystem::new()));

    assert_eq!(
        store.get_path(r"C:\photos").expect("path"),
        store.get_path(r"C:\photos\").expect("path")
    );
    assert_ne!(
        store.get_path(r"C:\").expect("path"),
        store.get_path(r"C:\photos").expect("path")
    );
}

#[test]
fn to_upper_invariant_differs_from_ordinal_upper_only_for_the_long_s() {
    // C#: string.ToUpperInvariant (contract section 2): U+017F -> S, sharp s stays, no multi-char mappings.
    assert_eq!(SessionStore::to_upper_invariant("a\u{017F}ß/ǆ"), "ASß/Ǆ");
    assert_eq!(SessionStore::to_upper_invariant(r"c:\photos"), r"C:\PHOTOS");
}

// ---------------------------------------------------------------------------------------------------------------
// SessionStoreSweepPerfTests (correctness half only)
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: real disk, 3000 files"]
fn constructor_with_many_stale_real_temp_files_leaves_only_fresh_ones() {
    // C#: SessionStoreSweepPerfTests.Constructor_WithManyStaleRealTempFiles_DoesNotBlockOnTheSweep (correctness half;
    // the timing asserts are replaced by the gate test above)
    let root = TempRoot::new("session-sweep");
    let old = std::time::SystemTime::now() - Duration::from_secs(3 * 86_400);
    for i in 0..3000 {
        let path = root.join(format!("abc{i}.json.tmp"));
        std::fs::write(&path, "x").expect("write");
        let file = std::fs::File::options().write(true).open(&path).expect("open");
        file.set_modified(old).expect("set_modified");
    }
    for i in 0..5 {
        std::fs::write(root.join(format!("fresh{i}.json.tmp")), "x").expect("write");
    }
    std::fs::write(root.join("real-session.json"), "{}").expect("write");
    let root_text = root.path().to_string_lossy().into_owned();
    let mut app_paths = AppPaths::new(root.path(), None, false);
    app_paths.sessions_dir = PathBuf::from(&root_text);

    let store = SessionStore::new(&app_paths, Arc::new(PhysicalFileSystem), None);
    store.wait_for_startup_sweep();

    let remaining_tmp = std::fs::read_dir(root.path())
        .expect("read_dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
        .count();
    assert_eq!(remaining_tmp, 5);
    assert!(root.join("real-session.json").exists());
}

// ---------------------------------------------------------------------------------------------------------------
// SessionWritePolicyTests / JsonFormatCompatTests (session part)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn session_save_uses_non_durable_atomic_write() {
    // C#: SessionWritePolicyTests.SessionSave_UsesNonDurableAtomicWrite
    let decorator = Arc::new(Decorator::recording(Arc::new(FakeFileSystem::new())));
    let fs: Arc<dyn FileSystem> = decorator.clone();
    let store = SessionStore::new(&paths(), fs, None);

    store.save(&SessionState::new(r"C:\photos")).expect("save");

    let writes = decorator.atomic_writes.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(writes.len(), 1);
    assert!(!writes[0].1, "session writes must not be durable (ADR 0007 section 2)");
}

#[test]
fn session_load_empty_or_partial_file_returns_no_session() {
    // C#: SessionWritePolicyTests.SessionLoad_EmptyOrPartialFile_ReturnsNoSession (5 rows)
    for content in [
        "",
        "   ",
        "{\"Folder\": \"C:\\\\photos\", \"CurrentPa",
        "\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}",
        "null",
    ] {
        let fs = Arc::new(FakeFileSystem::new());
        let store = new_store(&fs);
        fs.add_file(store.get_path(r"C:\photos").expect("path"), content.as_bytes());

        let loaded = store.load(r"C:\photos").expect("load");

        assert_eq!(loaded, SessionState::new(r"C:\photos"), "{content:?}");
    }
}

#[test]
fn session_file_bytes_match_previous_build() {
    // C#: JsonFormatCompatTests.SessionFile_Bytes_MatchPreviousBuild (CRLF = Environment.NewLine on Windows)
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\vacation";
    store
        .save(&SessionState {
            folder: folder.to_owned(),
            current_path: Some("C:\\photos\\vacation\\\u{111}\u{1EA1}i.jpg".to_owned()),
            skipped: vec![r"C:\photos\vacation\skip1.jpg".to_owned()],
            updated_utc: stamp(),
        })
        .expect("save");

    let expected = "{\r\n  \"Folder\": \"C:\\\\photos\\\\vacation\",\r\n  \"CurrentPath\": \"C:\\\\photos\\\\vacation\\\\\\u0111\\u1EA1i.jpg\",\r\n  \"Skipped\": [\r\n    \"C:\\\\photos\\\\vacation\\\\skip1.jpg\"\r\n  ],\r\n  \"UpdatedUtc\": \"2026-09-18T10:30:00Z\"\r\n}";
    let written = fs.file_content(store.get_path(folder).expect("path")).expect("file");
    assert_eq!(String::from_utf8(written).expect("utf8"), expected);

    let golden = golden::load("session-json-write");
    let case = golden
        .cases
        .iter()
        .find(|c| str_of(c, "name") == "from test SessionFile_Bytes_MatchPreviousBuild")
        .expect("golden case");
    assert_eq!(str_of(case, "text"), expected);
}

#[test]
fn session_file_legacy_text_loads() {
    // C#: JsonFormatCompatTests.SessionFile_LegacyText_Loads (LF line breaks, as an older build wrote them)
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\vacation";
    let legacy = "{\n  \"Folder\": \"C:\\\\photos\\\\vacation\",\n  \"CurrentPath\": \"C:\\\\photos\\\\vacation\\\\img2.jpg\",\n  \"Skipped\": [\n    \"C:\\\\photos\\\\vacation\\\\s.jpg\"\n  ],\n  \"UpdatedUtc\": \"2026-09-18T10:30:00Z\"\n}";
    fs.add_file(store.get_path(folder).expect("path"), legacy.as_bytes());

    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded.folder, folder);
    assert_eq!(loaded.current_path.as_deref(), Some(r"C:\photos\vacation\img2.jpg"));
    assert_eq!(loaded.skipped, vec![r"C:\photos\vacation\s.jpg".to_owned()]);
    assert_eq!(loaded.updated_utc, stamp());
    assert!(!loaded.updated_utc.unspecified);
}

#[test]
fn session_file_missing_optional_keys_loads_with_defaults() {
    // C#: JsonFormatCompatTests.SessionFile_MissingOptionalKeys_LoadsWithDefaults
    let fs = Arc::new(FakeFileSystem::new());
    let store = new_store(&fs);
    let folder = r"C:\photos\other";
    fs.add_file(
        store.get_path(folder).expect("path"),
        br#"{"CurrentPath":"C:\\photos\\other\\a.jpg"}"#,
    );

    let loaded = store.load(folder).expect("load");

    assert_eq!(loaded.folder, folder);
    assert_eq!(loaded.current_path.as_deref(), Some(r"C:\photos\other\a.jpg"));
    assert!(loaded.skipped.is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// Golden-driven tests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn golden_session_key_cases() {
    // Golden `session-key`: canonical form, upper-cased form, key, file location, exception class.
    let golden = golden::load("session-key");
    let store = new_store(&Arc::new(FakeFileSystem::new()));
    for case in &golden.cases {
        let folder = str_of(case, "folder");
        if let Some(exception) = case.get("exception").and_then(Value::as_str) {
            let expected_canonical = str_of(case, "canonical_exception");
            assert_eq!(exception, expected_canonical, "{folder:?}");
            let got = SessionStore::canonical_folder(folder);
            match exception {
                "PathTooLongException" => assert_eq!(got, Err(SessionPathError::TooLong), "{folder:?}"),
                "ArgumentException" => assert!(
                    matches!(got, Err(SessionPathError::Blank | SessionPathError::Invalid)),
                    "{folder:?}: {got:?}"
                ),
                other => panic!("unexpected exception class {other}"),
            }
            assert!(store.get_path(folder).is_err(), "{folder:?}");
            continue;
        }
        let canonical = str_of(case, "canonical");
        // DIVERGENCE: .NET expands 8.3 short names of existing paths; std does not (no golden input has `~`).
        assert_eq!(
            SessionStore::canonical_folder(folder).as_deref(),
            Ok(canonical),
            "{folder:?}"
        );
        assert_eq!(
            SessionStore::to_upper_invariant(canonical),
            str_of(case, "upper"),
            "{folder:?}"
        );
        assert_eq!(SessionStore::session_key(canonical), str_of(case, "key"), "{folder:?}");
        let path = store.get_path(folder).expect("path");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(format!("{}.json", str_of(case, "key")).as_str()),
            "{folder:?}"
        );
        assert_eq!(
            path.parent() == Some(sessions_dir().as_path()),
            bool_of(case, "dir_ok"),
            "{folder:?}"
        );
    }
}

#[test]
fn golden_session_json_write_cases() {
    // Golden `session-json-write`: exact text (CRLF included), bytes, file name; and the text loads back.
    let golden = golden::load("session-json-write");
    let store = new_store(&Arc::new(FakeFileSystem::new()));
    for case in &golden.cases {
        let name = str_of(case, "name");
        let value = ticks(str_of(case, "ticks").parse::<i64>().expect("ticks"));
        let updated_utc = match str_of(case, "kind") {
            "utc" => JournalDateTime::utc(value),
            "unspecified" => JournalDateTime::unspecified(value),
            other => panic!("unknown kind {other}"),
        };
        let state = SessionState {
            folder: str_of(case, "folder").to_owned(),
            current_path: case["currentPath"].as_str().map(str::to_owned),
            skipped: case["skipped"]
                .as_array()
                .expect("skipped")
                .iter()
                .map(|v| v.as_str().expect("string").to_owned())
                .collect(),
            updated_utc,
        };

        let text = SessionStore::serialize_state(&state);

        assert_eq!(text, str_of(case, "text"), "{name}");
        assert_eq!(bytes_to_hex(text.as_bytes()), str_of(case, "bytes_hex"), "{name}");
        let path = store.get_path(&state.folder).expect("path");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(str_of(case, "file_name")),
            "{name}"
        );
        let back = SessionStore::deserialize_state(&text).expect("valid").expect("object");
        assert_eq!(back, state, "{name}");
        assert_eq!(back.updated_utc.unspecified, state.updated_utc.unspecified, "{name}");
        assert_eq!(SessionStore::serialize_state(&back), text, "{name}");
    }
}

#[test]
fn golden_session_json_read_cases() {
    // Golden `session-json-read`: outcome, warning flag and values of 130 file texts through `load`.
    let golden = golden::load("session-json-read");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let folder = str_of(case, "folder");
        let log = Arc::new(RecordingLog::new());
        let fs = Arc::new(FakeFileSystem::new());
        let store = new_store(&fs).with_log(log.clone());
        let path = store.get_path(folder).expect("path");

        let loaded = if case.get("missing").and_then(Value::as_bool) == Some(true) {
            store.load(folder).expect("load")
        } else if let Some(hex) = case.get("hex").and_then(Value::as_str) {
            // The file system's decoding (`File.ReadAllText`) is `decode_text_like_dotnet`; the decoded text goes
            // straight into the deserializer and the `load` rules are applied by hand.
            let decoded = decode_text_like_dotnet(&hex_to_bytes(hex));
            match SessionStore::deserialize_state(&decoded) {
                Ok(Some(mut state)) => {
                    if state.folder.trim().is_empty() {
                        state.folder = folder.to_owned();
                    }
                    state
                }
                Ok(None) => SessionState::new(folder),
                Err(_) => {
                    log.warn(&format!(
                        "Session file '{}' is empty or corrupt; starting without a session.",
                        path.display()
                    ));
                    SessionState::new(folder)
                }
            }
        } else {
            fs.add_file(&path, str_of(case, "text").as_bytes());
            store.load(folder).expect("load")
        };

        assert_eq!(loaded.folder, str_of(case, "folder"), "{name}");
        assert_eq!(loaded.current_path.as_deref(), case["currentPath"].as_str(), "{name}");
        // DIVERGENCE: C# keeps a `null` element of Skipped; Vec<String> cannot, so null elements are dropped.
        let expected_skipped: Vec<&str> = case["skipped"]
            .as_array()
            .expect("skipped")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let got_skipped: Vec<&str> = loaded.skipped.iter().map(String::as_str).collect();
        assert_eq!(got_skipped, expected_skipped, "{name}");
        assert_eq!(
            loaded.updated_utc.value.ticks(),
            i64_of(case, "updated_utc_ticks"),
            "{name}"
        );
        match str_of(case, "updated_kind") {
            "Utc" => assert!(!loaded.updated_utc.unspecified, "{name}"),
            "Unspecified" => assert!(loaded.updated_utc.unspecified, "{name}"),
            // DIVERGENCE (README D5): C# makes an offset text a Local DateTime; Rust converts to UTC (ticks compared).
            "Local" => {}
            other => panic!("unknown kind {other}"),
        }
        let warned = warnings(&log);
        assert_eq!(!warned.is_empty(), bool_of(case, "warned"), "{name}: {warned:?}");
        if bool_of(case, "warning_mentions_path") {
            assert!(
                warned.iter().any(|w| w.contains(&path.display().to_string())),
                "{name}: {warned:?}"
            );
        }
    }
}

#[test]
fn deserialize_state_is_strict_about_duplicates_names_and_depth() {
    // Contract section 6 rules not worth a golden row each: last duplicate wins, a malformed duplicate rejects the
    // file, member names are case-sensitive, 64 nested containers are accepted and 65 are not.
    let ok = |json: &str| SessionStore::deserialize_state(json).expect("valid").expect("object");
    assert_eq!(ok(r#"{"Folder":"a","Folder":"b"}"#).folder, "b");
    assert_eq!(ok(r#"{"Skipped":["a"],"Skipped":["b","c"]}"#).skipped, vec!["b", "c"]);
    assert!(SessionStore::deserialize_state(r#"{"Folder":5,"Folder":"x"}"#).is_err());
    assert_eq!(ok(r#"{"folder":"a","Curr\u0065ntPath":"p"}"#).folder, "");
    assert_eq!(
        ok(r#"{"folder":"a","Curr\u0065ntPath":"p"}"#).current_path.as_deref(),
        Some("p")
    );
    assert_eq!(SessionStore::deserialize_state("null"), Ok(None));
    assert!(SessionStore::deserialize_state("[]").is_err());
    assert!(SessionStore::deserialize_state(r#"{"UpdatedUtc":null}"#).is_err());

    let nested = |arrays: usize| format!(r#"{{"X":{}{}}}"#, "[".repeat(arrays), "]".repeat(arrays));
    assert!(SessionStore::deserialize_state(&nested(63)).is_ok());
    assert!(SessionStore::deserialize_state(&nested(64)).is_err());
}

#[test]
fn depth_guard_ignores_brackets_after_an_escaped_quote_inside_a_string() {
    // Kills mutant M20 (nests_too_deep escape handling): `\"` must not end the string, so the 70 '[' stay literal text.
    let json = format!(r#"{{"Folder":"a\"{}"}}"#, "[".repeat(70));
    let state = SessionStore::deserialize_state(&json).expect("valid").expect("object");
    assert_eq!(state.folder, format!("a\"{}", "[".repeat(70)));
    // `\` is an escaped backslash: the following quote DOES end the string, so a real 64-deep array is rejected.
    let deep = format!(r#"{{"Folder":"\\","X":{}{}}}"#, "[".repeat(64), "]".repeat(64));
    assert!(SessionStore::deserialize_state(&deep).is_err());
    let ok = format!(r#"{{"Folder":"\\","X":{}{}}}"#, "[".repeat(63), "]".repeat(63));
    assert!(SessionStore::deserialize_state(&ok).is_ok());
}

// ---------------------------------------------------------------------------------------------------------------
// Repair round 1: .NET quirks and mutation gaps
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn nt_namespace_prefix_is_kept() {
    // .NET Path.GetFullPath keeps `\??\` paths as typed (verified against .NET 10).
    assert_eq!(SessionStore::canonical_folder(r"\??\C:\a").expect("c"), r"\??\C:\a");
    assert_eq!(SessionStore::canonical_folder(r"\??\C:\a\").expect("c"), r"\??\C:\a");
    assert_eq!(
        SessionStore::canonical_folder(r"\??\UNC\s\h\x").expect("c"),
        r"\??\UNC\s\h\x"
    );
    assert_eq!(SessionStore::canonical_folder(r"\??\C:").expect("c"), r"\??\C:");
}

#[test]
fn verbatim_trailing_forward_slash_is_trimmed() {
    // Kills the mutant that trims only '\' (C# TrimEnd('\', '/')).
    assert_eq!(SessionStore::canonical_folder(r"\\?\C:\a/").expect("c"), r"\\?\C:\a");
    assert_eq!(SessionStore::canonical_folder(r"\\?\C:\a\/").expect("c"), r"\\?\C:\a");
    assert_eq!(
        SessionStore::canonical_folder(r"\\?\C:/a/b//").expect("c"),
        r"\\?\C:/a/b"
    );
}

#[test]
fn save_with_unnameable_folder_still_creates_sessions_dir_and_warns() {
    // C# creates the Sessions directory BEFORE GetPath throws (contract 4.2.a).
    let fs = Arc::new(FakeFileSystem::new());
    let log = Arc::new(RecordingLog::new());
    let store = new_store(&fs).with_log(log.clone());
    store.wait_for_startup_sweep();
    assert_eq!(store.save(&SessionState::new("C:\\a\0b")), Ok(()));
    assert!(fs.directory_exists(&sessions_dir()));
    assert_eq!(warnings(&log).len(), 1);
}

#[test]
fn load_read_error_is_silent() {
    // C# swallows the read failure without logging (contract 3.4).
    let fs = Arc::new(FakeFileSystem::new());
    let log = Arc::new(RecordingLog::new());
    let store = new_store(&fs).with_log(log.clone());
    store.save(&SessionState::new(FOLDER)).expect("save");
    fs.fail(
        FsOp::ReadText,
        store.get_path(FOLDER).expect("path"),
        io::ErrorKind::Other,
    );
    let _ = store.load(FOLDER).expect("load");
    assert!(warnings(&log).is_empty());
}
