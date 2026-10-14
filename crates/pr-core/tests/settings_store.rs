//! W2-03: port of `PhotoReview.Core/Settings/SettingsStore.cs` tests (baseline d92d6aae) plus the Rust-only first-run
//! import (docs/adr/R0002). Goldens: `tests/golden/settings-parse-text.json` (66 cases), `tests/golden/settings-store.json`
//! (43 scenarios).
//!
//! C# test classes ported here: `SettingsStoreTests`, `SettingsStoreFailureTests`,
//! `SettingsStoreLegacyAndWriteFailureTests`, `SettingsStoreLoadRepairPersistenceTests` (the four `Build_*` tests
//! exercise `SettingsLoadRepairText` and belong to W2-02), `SettingsStoreMutationGapTests`,
//! `SettingsStoreSaveAfterNewerVersionLoadTests` and `SessionWritePolicyTests.SettingsSave_UsesDurableAtomicWrite`.
//!
//! Skipped on purpose:
//! * `SettingsStoreFailureTests.Save_Null_Throws`: Rust has no null; `save` takes the value.
//! * `SettingsStoreTests.Normalize_WhenEnumIsOutOfRange_ResetsToDefault`: it tests `SettingsNormalizer` (W2-02).
//! * `Save_SourceGeneratedJson_MatchesReflectionFormat` / `Load_SourceGeneratedJson_MatchesReflectionRead` compare two
//!   C# serializer back ends; Rust has one, so they are ported as a `to_json` / `parse_text` round trip.
//! * `SettingsParseTextTests`, `SettingsRobustnessTests`, `SettingsParsePropertyTests` are not claimed (contract
//!   "Open questions"); the golden `settings-parse-text` covers the same pipeline.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{Clock, FileStat, FileSystem, KeyNameValidator, ReadSeek};
use pr_core::app_paths::AppPaths;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::enums::{FileOperationType, ImageSortMode, InitialViewMode, LoadingMode};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts, SettingsJsonError, SettingsTexts};
use pr_core::settings::normalizer::build_repair_text;
use pr_core::settings::store::{
    CONFIG_FILE_NAME, SettingsSaveError, SettingsStore, SimpleKeyNameValidator, csharp_config_path,
};
use pr_test_support::golden::{self, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, RecordingLog, TempRoot};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

const LOCAL_APP_DATA: &str = r"C:\Users\test\AppData\Local";

fn english() -> Arc<dyn SettingsTexts> {
    Arc::new(EnglishSettingsTexts)
}

fn defaults() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

/// The .NET exception type name the golden reports for an error.
fn dotnet_type_name(e: &(dyn Error + 'static)) -> &'static str {
    if e.is::<SettingsJsonError>() {
        "JsonException"
    } else if let Some(io) = e.downcast_ref::<io::Error>() {
        if io.kind() == io::ErrorKind::PermissionDenied {
            "UnauthorizedAccessException"
        } else {
            "IOException"
        }
    } else {
        "Unknown"
    }
}

/// Splits a written document into `(property name, segment)` pairs the way the oracle does.
fn segments(text: &str) -> BTreeMap<String, String> {
    let lines: Vec<&str> = text.split("\r\n").collect();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in &lines[1..lines.len() - 1] {
        if let Some(rest) = line.strip_prefix("  \"") {
            if !rest.starts_with(' ') {
                let (name, value) = rest.split_once("\": ").expect("property line");
                out.push((name.to_owned(), vec![value.to_owned()]));
                continue;
            }
        }
        out.last_mut().expect("continuation").1.push((*line).to_owned());
    }
    out.into_iter()
        .map(|(name, parts)| {
            let joined = parts.join("\r\n");
            let trimmed = joined.strip_suffix(',').unwrap_or(&joined).to_owned();
            (name, trimmed)
        })
        .collect()
}

/// The top-level JSON segments of `settings` that differ from `AppSettings::new`.
fn changed_vs_defaults(settings: &AppSettings) -> BTreeMap<String, String> {
    let base = segments(&defaults().to_json().expect("defaults serialize"));
    segments(&settings.to_json().expect("settings serialize"))
        .into_iter()
        .filter(|(k, v)| base.get(k) != Some(v))
        .collect()
}

fn segment_of(settings: &AppSettings, name: &str) -> String {
    segments(&settings.to_json().expect("serialize"))
        .remove(name)
        .unwrap_or_else(|| panic!("no segment {name}"))
}

fn bytes_to_text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

fn b64_decode(text: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    out
}

/// A `FileSystem` over a `FakeFileSystem` that can lie about `file_exists`, swallow copies and records what the
/// store does (the twin of the C# `MutationFileSystemProxy` / `RecordingFileSystem`).
type ExistsHook = Box<dyn Fn(&Path) -> Option<bool> + Send + Sync>;

struct ProxyFs {
    inner: Arc<FakeFileSystem>,
    exists_hook: Mutex<Option<ExistsHook>>,
    swallow_copies: AtomicBool,
    copies: Mutex<Vec<PathBuf>>,
    calls: Mutex<Vec<&'static str>>,
    atomic_writes: Mutex<Vec<(PathBuf, bool)>>,
}

impl ProxyFs {
    fn new(inner: Arc<FakeFileSystem>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            exists_hook: Mutex::new(None),
            swallow_copies: AtomicBool::new(false),
            copies: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            atomic_writes: Mutex::new(Vec::new()),
        })
    }

    fn note(&self, call: &'static str) {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).push(call);
    }
}

impl FileSystem for ProxyFs {
    fn file_exists(&self, path: &Path) -> bool {
        let hooked = self
            .exists_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|hook| hook(path));
        hooked.unwrap_or_else(|| self.inner.file_exists(path))
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
        self.note("Copy");
        self.copies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(destination.to_path_buf());
        if self.swallow_copies.load(Ordering::SeqCst) {
            Ok(())
        } else {
            self.inner.copy_file(source, destination)
        }
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
        self.note("WriteAllTextAtomic");
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
        self.note("CreateDirectory");
        self.inner.create_directory(path)
    }
}

type StartupLog = Arc<Mutex<Vec<(String, &'static str)>>>;

/// One test's world: a fake file system, log, clock and the default `AppPaths` under `LOCAL_APP_DATA`.
struct Rig {
    fs: Arc<FakeFileSystem>,
    log: Arc<RecordingLog>,
    clock: Arc<FakeClock>,
    paths: AppPaths,
    startup: StartupLog,
    sleeps: Arc<Mutex<Vec<Duration>>>,
}

impl Rig {
    fn new() -> Self {
        let clock = Arc::new(FakeClock::new());
        Self {
            fs: Arc::new(FakeFileSystem::with_clock(Arc::clone(&clock))),
            log: Arc::new(RecordingLog::new()),
            clock,
            paths: AppPaths::new(Path::new(LOCAL_APP_DATA), None, false),
            startup: Arc::new(Mutex::new(Vec::new())),
            sleeps: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn cfg(&self) -> PathBuf {
        self.paths.config_file.clone()
    }

    fn dir(&self) -> PathBuf {
        self.cfg().parent().expect("config dir").to_path_buf()
    }

    fn put(&self, text: &str) {
        self.fs.add_file(self.cfg(), text.as_bytes());
    }

    fn read(&self) -> String {
        bytes_to_text(&self.fs.file_content(self.cfg()).expect("config.json exists"))
    }

    fn store_on(&self, fs: Arc<dyn FileSystem>) -> SettingsStore {
        let startup = Arc::clone(&self.startup);
        let sleeps = Arc::clone(&self.sleeps);
        SettingsStore::new(
            self.paths.clone(),
            fs,
            Arc::clone(&self.log) as Arc<dyn pr_core::abstractions::Log>,
            Arc::clone(&self.clock) as Arc<dyn Clock>,
            english(),
        )
        .with_startup_error_callback(Box::new(move |message, error| {
            startup
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((message.to_owned(), dotnet_type_name(error)));
        }))
        .with_read_retry_sleeper(Arc::new(move |d| {
            sleeps.lock().unwrap_or_else(PoisonError::into_inner).push(d);
        }))
    }

    fn store(&self) -> SettingsStore {
        self.store_on(Arc::clone(&self.fs) as Arc<dyn FileSystem>)
    }

    fn startup_errors(&self) -> Vec<(String, &'static str)> {
        self.startup.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn startup_messages(&self) -> Vec<String> {
        self.startup_errors().into_iter().map(|(m, _)| m).collect()
    }

    fn sleeps_ms(&self) -> Vec<u128> {
        self.sleeps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(Duration::as_millis)
            .collect()
    }

    /// Files next to config.json matching a .NET pattern.
    fn listing(&self, pattern: &str) -> Vec<PathBuf> {
        self.fs.enumerate_files(&self.dir(), pattern).expect("listing")
    }

    fn siblings(&self) -> Vec<PathBuf> {
        let cfg = self.cfg();
        self.listing("*")
            .into_iter()
            .filter(|f| !f.as_os_str().eq_ignore_ascii_case(cfg.as_os_str()))
            .collect()
    }

    fn newer_backups(&self) -> Vec<PathBuf> {
        self.siblings()
            .into_iter()
            .filter(|f| name_of(f).starts_with("config.json.newer-"))
            .collect()
    }

    fn text_of(&self, path: &Path) -> String {
        bytes_to_text(&self.fs.file_content(path).expect("file exists"))
    }

    fn log_lines(&self, level: Lv) -> Vec<String> {
        self.log
            .entries()
            .into_iter()
            .filter(|(l, _)| Lv::of(l) == level)
            .map(|(_, m)| m)
            .collect()
    }

    fn has(&self, level: Lv, needle: &str) -> bool {
        self.log_lines(level).iter().any(|m| m.contains(needle))
    }
}

/// `pr_test_support::LogLevel` is not re-exported; mirror it through its `Debug` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lv {
    Info,
    Warn,
    Error,
}

impl Lv {
    fn of(level: &impl std::fmt::Debug) -> Lv {
        match format!("{level:?}").as_str() {
            "Info" => Lv::Info,
            "Warn" => Lv::Warn,
            _ => Lv::Error,
        }
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn counter() -> (Arc<AtomicUsize>, impl Fn() -> pr_core::settings::store::ChangedHandler) {
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    (count, move || {
        let c = Arc::clone(&c);
        Box::new(move |_s: &Arc<AppSettings>| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    })
}

fn handler_bug() -> io::Error {
    io::Error::other("handler bug")
}

const CONFLICTING_CONFIG: &str = r#"{"ConfigVersion":3,"Actions":[{"Name":"Mine","Shortcut":"K"}]}"#;
const OUT_OF_RANGE_CONFIG: &str = r#"{"ConfigVersion":3,"PreloadWorkerCount":99999}"#;
const MISTYPED_VALUE_CONFIG: &str = r#"{"ConfigVersion":3,"ClickZoomPercent":"abc"}"#;
const NEWER: &str = r#"{"ConfigVersion":99,"FutureField":123,"UiLanguage":"en","ClickZoomPercent":250}"#;

fn fixture_text(name: &str) -> String {
    let path = golden::repo_root().join("tests").join("fixtures").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    // File.ReadAllText drops a UTF-8 BOM.
    text.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(text)
}

// ---------------------------------------------------------------------------------------------------------------
// SettingsStoreTests
// ---------------------------------------------------------------------------------------------------------------

// C#: SettingsStoreTests.Load_WhenFileDoesNotExist_CreatesAndSavesDefault
#[test]
fn load_when_file_does_not_exist_creates_and_saves_default() {
    let r = Rig::new();
    let store = r.store();
    assert!(!r.fs.file_exists(&r.cfg()));

    let loaded = store.load(None).expect("load");

    assert_eq!(loaded.config_version, 3);
    assert_eq!(loaded.loading_mode, Some(LoadingMode::Preview));
    assert!(r.fs.file_exists(&r.cfg()));
    assert!(r.read().contains("\"ConfigVersion\": 3"));
}

// C#: SettingsStoreTests.Load_WhenActionUsesLegacyDeleteOperation_KeepsItAsRecycle
#[test]
fn load_when_action_uses_legacy_delete_operation_keeps_it_as_recycle() {
    let r = Rig::new();
    r.put(
        r#"{
            "ConfigVersion": 3,
            "Actions": [
                { "Name": "Trash it", "Shortcut": "F7", "Operation": "Delete", "Destination": "", "Confirm": true },
                { "Name": "Keep", "Shortcut": "F8", "Operation": "Move", "Destination": "Keepers" }
            ]
        }"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert_eq!(loaded.actions.len(), 2);
    assert_eq!(loaded.actions[0].operation, FileOperationType::Recycle);
    assert_eq!(loaded.actions[0].name, "Trash it");
    assert_eq!(loaded.actions[0].shortcut, "F7");
    assert!(loaded.actions[0].confirm);
    assert_eq!(loaded.actions[1].operation, FileOperationType::Move);
    assert!(store.last_load_repairs().is_empty());
}

// C#: SettingsStoreTests.Load_WhenLegacyFolder2NameIsPresent_IgnoresItAndKeepsOtherValues
#[test]
fn load_when_legacy_folder2_name_is_present_ignores_it_and_keeps_other_values() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion": 2, "Folder2Name": "Old-Target", "LoadingMode": "Fast", "LoggingEnabled": true}"#);

    let loaded = r.store().load(None).expect("load");

    assert_eq!(loaded.loading_mode, Some(LoadingMode::Fast));
    assert!(loaded.logging_enabled);
}

// C#: SettingsStoreTests.Load_WhenValidConfigExists_ParsesProperly
#[test]
fn load_when_valid_config_exists_parses_properly() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion": 2, "LoadingMode": "Fast", "ImageSortMode": "Size", "LoggingEnabled": true}"#);
    let store = r.store();
    let seen = Arc::new(Mutex::new(None));
    let s = Arc::clone(&seen);
    store.subscribe_changed(Box::new(move |settings| {
        *s.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(settings));
        Ok(())
    }));

    let loaded = store.load(None).expect("load");

    let raised = seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .expect("Changed fired");
    assert_eq!(raised.loading_mode, Some(LoadingMode::Fast));
    assert_eq!(raised.image_sort_mode, Some(ImageSortMode::SizeDescending));
    assert!(raised.logging_enabled);
    assert_eq!(loaded.loading_mode, Some(LoadingMode::Fast));
    assert_eq!(loaded.image_sort_mode, Some(ImageSortMode::SizeDescending));
    assert!(r.startup_errors().is_empty());
}

// C#: SettingsStoreTests.Load_Inv11_WhenCorruptConfig_BacksUpAndResetsToDefault
#[test]
fn load_inv11_when_corrupt_config_backs_up_and_resets_to_default() {
    let r = Rig::new();
    let corrupt = "{ this is not valid json, corrupt content }";
    r.put(corrupt);

    let loaded = r.store().load(None).expect("load");

    assert_eq!(loaded.loading_mode, Some(LoadingMode::Preview));
    assert_eq!(loaded.config_version, 3);
    let errors = r.startup_errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].0.contains("Corrupt config.json detected"));
    assert_eq!(errors[0].1, "JsonException");
    let backups = r.listing("config.json.corrupt-*");
    assert_eq!(backups.len(), 1);
    assert_eq!(r.text_of(&backups[0]), corrupt);
}

// C#: SettingsStoreTests.Load_CorruptConfigBackupFails_LaterSaveKeepsTheCorruptFile
#[test]
fn load_corrupt_config_backup_fails_later_save_keeps_the_corrupt_file() {
    let r = Rig::new();
    let corrupt = "{ this is not valid json }";
    r.put(corrupt);
    r.fs.fail(FsOp::Copy, r.cfg(), io::ErrorKind::PermissionDenied);
    let store = r.store();

    let loaded = store.load(None).expect("load");
    store.save((*loaded).clone()).expect("save");

    assert_eq!(r.read(), corrupt);
    assert_eq!(*store.current(), *loaded);
    assert!(r.has(
        Lv::Warn,
        "Settings kept in memory only: config.json is preserved untouched"
    ));
}

// C#: SettingsStoreTests.Load_ConfigLocked_LaterSaveKeepsTheValidFile
#[test]
fn load_config_locked_later_save_keeps_the_valid_file() {
    let r = Rig::new();
    let valid = r#"{ "ConfigVersion": 3, "LoggingEnabled": true }"#;
    r.put(valid);
    r.fs.fail(FsOp::ReadText, r.cfg(), io::ErrorKind::Other);
    let store = r.store();

    let loaded = store.load(None).expect("load");
    r.fs.clear_faults();
    store.save((*loaded).clone()).expect("save");

    assert_eq!(r.read(), valid);
}

// C#: SettingsStoreTests.Load_Inv11_WhenFileLocked_UsesInMemoryDefaultsAndLeavesFileIntact
#[test]
fn load_inv11_when_file_locked_uses_in_memory_defaults_and_leaves_file_intact() {
    let r = Rig::new();
    let valid = r#"{"ConfigVersion": 2, "LoadingMode": "Original"}"#;
    r.put(valid);
    r.fs.fail_locked(FsOp::ReadText, r.cfg());

    let loaded = r.store().load(None).expect("load");

    assert_eq!(loaded.loading_mode, Some(LoadingMode::Preview));
    let errors = r.startup_errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].0.contains("Config.json inaccessible"));
    assert_eq!(errors[0].1, "IOException");
    // The four retries slept 50/100/200/400 ms.
    assert_eq!(r.sleeps_ms(), vec![50, 100, 200, 400]);
    r.fs.clear_faults();
    assert_eq!(r.read(), valid);
}

// C#: SettingsStoreTests.Load_TransientReadFailure_IsRetried (unauthorized = true / false)
#[test]
fn load_transient_read_failure_is_retried() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
        let r = Rig::new();
        r.put(r#"{ "ConfigVersion": 3, "ClickZoomPercent": 275 }"#);
        r.fs.fail_times(FsOp::ReadText, r.cfg(), kind, 2);

        let loaded = r.store().load(None).expect("load");

        assert_eq!(loaded.click_zoom_percent, 275, "{kind:?}");
        assert!(r.startup_errors().is_empty());
        assert_eq!(r.sleeps_ms(), vec![50, 100]);
        let infos = r.log_lines(Lv::Info);
        let expected = if kind == io::ErrorKind::PermissionDenied {
            "UnauthorizedAccessException"
        } else {
            "IOException"
        };
        assert_eq!(
            infos
                .iter()
                .filter(|m| m.starts_with(&format!("config.json read failed ({expected}), retrying: ")))
                .count(),
            2,
            "{infos:?}"
        );
    }
}

// C#: SettingsStoreTests.Save_WritesAtomicAndRaisesChanged
#[test]
fn save_writes_atomic_and_raises_changed() {
    let r = Rig::new();
    let store = r.store();
    let mut settings = defaults();
    settings.loading_mode = Some(LoadingMode::Original);
    settings.logging_enabled = true;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = Arc::clone(&seen);
    store.subscribe_changed(Box::new(move |settings| {
        s.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(settings.loading_mode);
        Ok(())
    }));

    let saved = store.save(settings).expect("save");

    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        vec![Some(LoadingMode::Original)]
    );
    assert_eq!(*store.current(), *saved);
    let disk = r.read();
    assert!(disk.contains("\"LoggingEnabled\": true"));
    assert!(!disk.contains("Folder2Name"));
    assert!(disk.contains("\"LoadingMode\": \"Original\""));
}

// C#: SettingsStoreTests.Migrate_UpgradesV1ToCurrent
#[test]
fn migrate_upgrades_v1_to_current() {
    let mut v1 = defaults();
    v1.config_version = 1;
    v1.actions.clear(); // C# sets Actions = null; Rust has no null, Migrate leaves a list alone

    SettingsStore::migrate(&mut v1);

    assert_eq!(v1.config_version, 3);
    assert!(v1.hidden_context_menu_items.is_some());
}

// C#: SettingsStoreTests.Save_SourceGeneratedJson_MatchesReflectionFormat (one serializer in Rust: round trip)
#[test]
fn save_json_round_trips_through_parse_text() {
    let r = Rig::new();
    let store = r.store();
    let mut settings = defaults();
    settings.logging_enabled = true;
    settings.image_sort_mode = Some(ImageSortMode::SizeDescending);
    settings.preload_memory_load_limit = 0.75;
    settings.actions = vec![pr_core::settings::shortcuts::ReviewAction {
        name: "Loại 2".to_owned(),
        shortcut: "Enter".to_owned(),
        operation: FileOperationType::Move,
        destination: r"C:\Xiuren\[[WALLPAPER]".to_owned(),
        confirm: false,
    }];

    let saved = store.save(settings).expect("save");

    let parsed = SettingsStore::parse_text(&r.read(), &EnglishSettingsTexts).expect("parse");
    assert_eq!(parsed.settings.actions, saved.actions);
    assert_eq!(parsed.settings.preload_memory_load_limit, 0.75);
    assert_eq!(parsed.settings.image_sort_mode, Some(ImageSortMode::SizeDescending));
    assert_eq!(r.read(), saved.to_json().expect("json"));
}

// C#: SettingsStoreTests.Load_SourceGeneratedJson_MatchesReflectionRead (round trip through to_json)
#[test]
fn load_reads_what_to_json_wrote_including_lenient_enums() {
    let r = Rig::new();
    r.put(
        r#"{
            "ConfigVersion": 2,
            "LoadingMode": "NoSuchMode",
            "DecoderBackend": "WicDirect",
            "ImageSortMode": 3,
            "ImageCacheCapacityBytes": 17179869184,
            "PreloadWorkerCount": 8,
            "Shortcuts": { "Next": "D", "Previous": "A" },
            "Actions": [ { "Name": "Keep", "Shortcut": "L", "Operation": "Copy", "Destination": "Keep" } ]
        }"#,
    );

    let loaded = r.store().load(None).expect("load");

    assert_eq!(loaded.shortcuts.next, "D");
    assert_eq!(loaded.actions.len(), 1);
    assert_eq!(loaded.actions[0].operation, FileOperationType::Copy);
    // The writer's output reads back to the same settings (the only serializer there is).
    let again = SettingsStore::parse_text(&loaded.to_json().expect("json"), &EnglishSettingsTexts).expect("parse");
    assert_eq!(again.settings.to_json().expect("json"), loaded.to_json().expect("json"));
}

// C#: SettingsStoreTests.Load_WhenConfigHoldsUnusableValues_ResetsThemAndReportsRepairs
#[test]
fn load_when_config_holds_unusable_values_resets_them_and_reports_repairs() {
    let r = Rig::new();
    r.put(
        r#"{
            "ImageCacheCapacityBytes": 0,
            "SourceBytesCapacityBytes": -5,
            "PreloadWorkerCount": -1,
            "PreloadMemoryLoadLimit": 7.5,
            "Actions": [null, { "Name": "Keep", "Shortcut": "F9", "Operation": "Move", "Destination": "K" }]
        }"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    use pr_core::settings::perf_options as perf;
    assert_eq!(loaded.image_cache_capacity_bytes, perf::IMAGE_CACHE_CAPACITY_BYTES);
    assert_eq!(loaded.source_bytes_capacity_bytes, perf::SOURCE_BYTES_CAPACITY_BYTES);
    assert_eq!(loaded.preload_worker_count, perf::PRELOAD_WORKER_COUNT);
    assert_eq!(loaded.preload_memory_load_limit, perf::PRELOAD_MEMORY_LOAD_LIMIT);
    assert_eq!(loaded.actions.len(), 1);
    assert_eq!(loaded.actions[0].name, "Keep");
    let mut repairs = store.last_load_repairs();
    repairs.sort();
    let mut expected: Vec<String> = [
        "ImageCacheCapacityBytes",
        "SourceBytesCapacityBytes",
        "PreloadWorkerCount",
        "PreloadMemoryLoadLimit",
        "Actions",
    ]
    .map(str::to_owned)
    .to_vec();
    expected.sort();
    assert_eq!(repairs, expected);
}

// C#: SettingsStoreTests.Load_WhenAnActionUsesAnOptionalShortcutKey_ReportsTheDisabledShortcutAsARepair
#[test]
fn load_when_an_action_uses_an_optional_shortcut_key_reports_the_disabled_shortcut_as_a_repair() {
    let r = Rig::new();
    r.put(
        r#"{
            "Actions": [
                { "Name": "Keep", "Shortcut": "O", "Operation": "Copy", "Destination": "Keep" },
                { "Name": "Zoomed", "Shortcut": "D3", "Operation": "Copy", "Destination": "Zoomed" }
            ]
        }"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert_eq!(loaded.shortcuts.open_folder, "");
    assert_eq!(loaded.shortcuts.custom_zoom, "");
    let mut repairs = store.last_load_repairs();
    repairs.sort();
    assert_eq!(
        repairs,
        vec!["Shortcuts.CustomZoom".to_owned(), "Shortcuts.OpenFolder".to_owned()]
    );
}

// C#: SettingsStoreTests.Load_WhenConfigIsValid_ReportsNoRepairs
#[test]
fn load_when_config_is_valid_reports_no_repairs() {
    let r = Rig::new();
    r.put(r#"{ "LoggingEnabled": true }"#);
    let store = r.store();

    store.load(None).expect("load");

    assert!(store.last_load_repairs().is_empty());
}

// C#: SettingsStoreTests.Parse_V1Fixture_MigratesProperly
#[test]
fn parse_v1_fixture_migrates_properly() {
    let settings = SettingsStore::parse(&fixture_text("config-v1.json"), &EnglishSettingsTexts).expect("parse");

    assert_eq!(settings.loading_mode, Some(LoadingMode::Preview));
    assert_eq!(settings.image_sort_mode, Some(ImageSortMode::SizeDescending));
    assert_eq!(settings.initial_view_mode, InitialViewMode::Fit);
    assert_eq!(settings.actions.len(), 1);
    assert_eq!(settings.actions[0].operation, FileOperationType::Move);
}

// C#: SettingsStoreTests.Parse_V2Fixture_ParsesCorrectly
#[test]
fn parse_v2_fixture_parses_correctly() {
    let settings = SettingsStore::parse(&fixture_text("config-v2.json"), &EnglishSettingsTexts).expect("parse");

    assert_eq!(settings.config_version, 3);
    assert_eq!(settings.ui_language, "vi"); // Q-L1: pre-i18n configs keep Vietnamese
    assert_eq!(settings.loading_mode, Some(LoadingMode::Original));
    assert_eq!(settings.image_sort_mode, Some(ImageSortMode::SizeAscending));
    assert_eq!(settings.initial_view_mode, InitialViewMode::Percent200);
    assert_eq!(settings.actions.len(), 1);
    assert_eq!(settings.actions[0].operation, FileOperationType::Recycle);
}

// ---------------------------------------------------------------------------------------------------------------
// SettingsStoreFailureTests
// ---------------------------------------------------------------------------------------------------------------

// C#: SettingsStoreFailureTests.Save_WriteFails_KeepsPreviousFileAndState (IOException and UnauthorizedAccessException)
#[test]
fn save_write_fails_keeps_previous_file_and_state() {
    for kind in [io::ErrorKind::Other, io::ErrorKind::PermissionDenied] {
        let r = Rig::new();
        let store = r.store();
        let mut first = defaults();
        first.click_zoom_percent = 150;
        store.save(first).expect("first save");
        let before = r.read();
        let current = store.current();
        let (changed, handler) = counter();
        store.subscribe_changed(handler());
        r.fs.fail(FsOp::WriteAtomic, r.cfg(), kind);

        let mut second = defaults();
        second.click_zoom_percent = 400;
        let err = store.save(second.clone()).expect_err("write fails");

        assert!(matches!(err, SettingsSaveError::Io(ref e) if e.kind() == kind));
        assert_eq!(r.read(), before);
        assert!(Arc::ptr_eq(&current, &store.current()));
        assert_eq!(changed.load(Ordering::SeqCst), 0);
        r.fs.clear_faults();
        store.save(second).expect("save after recovery");
        assert_eq!(r.store().load(None).expect("load").click_zoom_percent, 400);
        assert_eq!(changed.load(Ordering::SeqCst), 1);
    }
}

// C#: SettingsStoreFailureTests.Migrate_Idempotent
#[test]
fn migrate_is_idempotent_for_every_historic_version() {
    for version in [i32::MIN, -1, 0, 1, 2, 3, 4, i32::MAX] {
        let mut settings = defaults();
        settings.config_version = version;
        settings.ui_language = String::new(); // C# null

        SettingsStore::migrate(&mut settings);
        let once = settings.to_json().expect("json");
        SettingsStore::migrate(&mut settings);

        assert_eq!(settings.to_json().expect("json"), once, "version {version}");
        assert!(!settings.ui_language.trim().is_empty());
        assert!(settings.config_version >= 3);
    }
}

// C#: SettingsStoreFailureTests.Migrate_LanguageDefaults
#[test]
fn migrate_language_defaults() {
    let mut old = defaults();
    old.config_version = 2;
    old.ui_language = "auto".to_owned();
    let mut current = defaults();
    current.config_version = 3;
    current.ui_language = "de".to_owned();

    SettingsStore::migrate(&mut old);
    SettingsStore::migrate(&mut current);

    assert_eq!(old.ui_language, "vi");
    assert_eq!(current.ui_language, "de");
}

// C#: SettingsStoreFailureTests.Load_TwoCorruptLoadsInOneSecond_KeepBothOrFirstBackup
#[test]
fn load_two_corrupt_loads_in_one_second_keep_both_backups_under_unique_names() {
    let r = Rig::new();
    r.put("{ broken 1");
    r.store().load(None).expect("first");
    let backups = r.listing("config.json.corrupt-*");
    assert_eq!(backups.len(), 1);
    assert_eq!(r.text_of(&backups[0]), "{ broken 1");

    r.put("{ broken 2"); // same clock second
    r.store().load(None).expect("second");

    let after: Vec<String> = r
        .listing("config.json.corrupt-*")
        .iter()
        .map(|f| r.text_of(f))
        .collect();
    assert!(after.contains(&"{ broken 1".to_owned()));
    assert!(after.contains(&"{ broken 2".to_owned()));
    assert_eq!(after.len(), 2);
}

// C#: SettingsStoreFailureTests.Load_MistypedValue_SalvagesAndReports
#[test]
fn load_mistyped_value_salvages_and_reports() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3,"ClickZoomPercent":"abc","LoggingEnabled":true,"LoadingMode":"Original"}"#);
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert!(loaded.logging_enabled);
    assert_eq!(loaded.loading_mode, Some(LoadingMode::Original));
    assert_eq!(loaded.click_zoom_percent, defaults().click_zoom_percent);
    assert!(store.last_load_repairs().contains(&"ClickZoomPercent".to_owned()));
    assert_eq!(r.listing("config.json.corrupt-*").len(), 1);
}

// C#: SettingsStoreFailureTests.Load_SalvageBackupFails_ConfigNeverOverwritten
#[test]
fn load_salvage_backup_fails_config_never_overwritten() {
    let r = Rig::new();
    let original = r#"{"ConfigVersion":3,"ClickZoomPercent":"abc","LoggingEnabled":true}"#;
    r.put(original);
    r.fs.fail(FsOp::Copy, r.cfg(), io::ErrorKind::Other);
    let store = r.store();

    let loaded = store.load(None).expect("load");
    store.save((*loaded).clone()).expect("save");

    assert!(loaded.logging_enabled);
    assert_eq!(r.read(), original);
    assert!(
        r.startup_messages()
            .iter()
            .any(|m| m == "Could not back up config.json with unreadable values")
    );
}

// C#: SettingsStoreFailureTests.Load_MistypedValue_KeepsNestedCollections
#[test]
fn load_mistyped_value_keeps_nested_collections() {
    let r = Rig::new();
    r.put(
        r#"{"ConfigVersion":3,"ClickZoomPercent":"abc",
             "Actions":[{"Name":"Keep","Shortcut":"K","Operation":"Copy","Destination":"Keepers","Confirm":true}],
             "Shortcuts":{"Next":"N","Previous":"P"}}"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    let repairs = store.last_load_repairs();
    assert!(repairs.contains(&"ClickZoomPercent".to_owned()));
    assert!(!repairs.contains(&"Actions".to_owned()));
    assert_eq!(loaded.actions.len(), 1);
    assert_eq!(loaded.actions[0].name, "Keep");
    assert_eq!(loaded.actions[0].destination, "Keepers");
    assert_eq!(loaded.actions[0].operation, FileOperationType::Copy);
    assert!(loaded.actions[0].confirm);
    assert_eq!(loaded.shortcuts.next, "N");
    assert_eq!(loaded.shortcuts.previous, "P");
}

// C#: SettingsStoreFailureTests.Load_BadElementInsideActions_ResetsOnlyActions
#[test]
fn load_bad_element_inside_actions_resets_only_actions() {
    let r = Rig::new();
    r.put(
        r#"{"ConfigVersion":3,"LoggingEnabled":true,
             "Actions":[{"Name":"Ok","Shortcut":"K"},{"Name":5,"Shortcut":{"x":1}}],
             "Shortcuts":{"Next":"N"}}"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert!(loaded.logging_enabled);
    assert_eq!(loaded.shortcuts.next, "N");
    assert!(store.last_load_repairs().contains(&"Actions".to_owned()));
    assert!(!loaded.actions.is_empty());
}

// C#: SettingsStoreFailureTests.Load_BadValueInsideShortcuts_ResetsOnlyShortcuts
#[test]
fn load_bad_value_inside_shortcuts_resets_only_shortcuts() {
    let r = Rig::new();
    r.put(
        r#"{"ConfigVersion":3,
             "Actions":[{"Name":"Keep","Shortcut":"K"}],
             "Shortcuts":{"Next":["not","a","string"]}}"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert!(store.last_load_repairs().contains(&"Shortcuts".to_owned()));
    assert_eq!(loaded.shortcuts.next, defaults().shortcuts.next);
    assert_eq!(loaded.actions.len(), 1);
    assert_eq!(loaded.actions[0].name, "Keep");
}

// C#: SettingsStoreFailureTests.Load_BlankShortcutWhoseDefaultIsTaken_ActionShortcutIsClearedAndReported
#[test]
fn load_blank_shortcut_whose_default_is_taken_action_shortcut_is_cleared_and_reported() {
    let r = Rig::new();
    r.put(
        r#"{"ConfigVersion":3,"Shortcuts":{"Previous":""},
             "Actions":[{"Name":"Mine","Shortcut":"Left"},{"Name":"Other","Shortcut":"Z"}]}"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert_eq!(loaded.shortcuts.previous, defaults().shortcuts.previous);
    assert_eq!(loaded.actions[0].shortcut, "");
    assert_eq!(loaded.actions[1].shortcut, "Z");
    let repairs = store.last_load_repairs();
    assert!(repairs.contains(&"Shortcuts".to_owned()));
    assert!(repairs.contains(&"Actions[0].Shortcut".to_owned()));
    assert!(!repairs.contains(&"Actions[1].Shortcut".to_owned()));
}

// C#: SettingsStoreFailureTests.Load_BlankOrDuplicateActions_LoadAndValidatorFlagsThem
#[test]
fn load_blank_or_duplicate_actions_load_and_validator_flags_them() {
    let r = Rig::new();
    r.put(
        r#"{"ConfigVersion":3,
             "Actions":[{"Name":"","Shortcut":"Q"},{"Name":"B","Shortcut":""},{"Name":"C","Shortcut":"W"},{"Name":"D","Shortcut":"W"}]}"#,
    );
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert_eq!(loaded.actions.len(), 4);
    assert!(store.validate_shortcuts(&loaded).is_some());
}

// C#: SettingsStoreFailureTests.Load_ChangedHandlerThrowsIOException_KeepsLoadedValues
#[test]
fn load_changed_handler_returns_io_error_keeps_loaded_values() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3,"ClickZoomPercent":150}"#);
    let store = r.store();
    let id = store.subscribe_changed(Box::new(|_| Err(handler_bug())));

    let err = store.load(None).expect_err("handler error surfaces");

    assert_eq!(err.to_string(), "handler bug");
    assert_eq!(store.current().click_zoom_percent, 150); // not the defaults
    assert!(store.unsubscribe_changed(id));
    assert!(!store.unsubscribe_changed(id));
    let mut next = defaults();
    next.click_zoom_percent = 200;
    store.save(next).expect("save still writes the file");
    assert_eq!(r.store().load(None).expect("load").click_zoom_percent, 200);
}

// C#: SettingsStoreFailureTests.Load_UnparsableEnum_ResetsToDefaultAndReports (8 properties x 5 literals)
#[test]
fn load_unparsable_enum_resets_to_default_and_reports() {
    let properties = [
        "LoadingMode",
        "ImageSortMode",
        "ScalingQuality",
        "DecoderBackend",
        "KeyboardZoomAnchor",
        "KineticGlideSmoothing",
        "FitWidthAnchor2",
        "MiddleClickAction",
    ];
    let base = defaults();
    for property in properties {
        for literal in ["\"garbage\"", "\"\"", "null", "42", "{}"] {
            let r = Rig::new();
            r.put(&format!("{{\"ConfigVersion\":3,\"{property}\":{literal}}}"));
            let store = r.store();

            let loaded = store.load(None).expect("load");

            assert_eq!(
                segment_of(&loaded, property),
                segment_of(&base, property),
                "{property} = {literal}"
            );
            assert!(
                store.last_load_repairs().contains(&property.to_owned()),
                "{property} = {literal}: {:?}",
                store.last_load_repairs()
            );
        }
    }
}

// C#: SettingsStoreFailureTests.Load_UnparsableEnum_IsPersistedAsName
#[test]
fn load_unparsable_enum_is_persisted_as_name() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3,"LoadingMode":"garbage"}"#);

    r.store().load(None).expect("load");

    assert!(r.read().contains("\"LoadingMode\": \"Preview\""));
    let second = r.store();
    second.load(None).expect("second load");
    assert!(second.last_load_repairs().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// SettingsStoreLegacyAndWriteFailureTests
// ---------------------------------------------------------------------------------------------------------------

// C#: SettingsStoreLegacyAndWriteFailureTests.Load_FirstRun_DefaultWriteFails_ReturnsInMemoryDefaults (IO, Unauthorized)
#[test]
fn load_first_run_default_write_fails_returns_in_memory_defaults() {
    for kind in [io::ErrorKind::Other, io::ErrorKind::PermissionDenied] {
        let r = Rig::new();
        r.fs.fail(FsOp::WriteAtomic, r.cfg(), kind);
        let store = r.store();
        let seen: Arc<Mutex<Vec<Arc<AppSettings>>>> = Arc::new(Mutex::new(Vec::new()));
        let s = Arc::clone(&seen);
        store.subscribe_changed(Box::new(move |settings| {
            s.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Arc::clone(settings));
            Ok(())
        }));

        let loaded = store.load(None).expect("load");

        assert!(Arc::ptr_eq(&loaded, &store.current()));
        let seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(seen.len(), 1);
        assert!(Arc::ptr_eq(&loaded, &seen[0]));
        assert_eq!(r.startup_errors().len(), 1);
    }
}

// C#: SettingsStoreLegacyAndWriteFailureTests.Load_CorruptConfig_DefaultWriteFails_ReturnsInMemoryDefaults
#[test]
fn load_corrupt_config_default_write_fails_returns_in_memory_defaults() {
    let r = Rig::new();
    r.put("{ not json");
    r.fs.fail(FsOp::WriteAtomic, r.cfg(), io::ErrorKind::Other);

    let loaded = r.store().load(None).expect("load");

    assert_eq!(loaded.config_version, 3);
    assert!(r.startup_messages().iter().any(|m| m.contains("default config.json")));
}

// C#: SettingsStoreLegacyAndWriteFailureTests.Load_ConfigWithoutVersionKey_KeepsVietnamese
#[test]
fn load_config_without_version_key_keeps_vietnamese() {
    let r = Rig::new();
    r.put(r#"{ "LoadingMode": "Fast" }"#);

    let loaded = r.store().load(None).expect("load");

    assert_eq!(loaded.ui_language, "vi");
    assert_eq!(loaded.config_version, 3);
}

// C#: SettingsStoreLegacyAndWriteFailureTests.Load_UnversionedSalvagedConfig_KeepsVietnamese
#[test]
fn load_unversioned_salvaged_config_keeps_vietnamese() {
    let r = Rig::new();
    r.put(r#"{ "ClickZoomPercent": "abc", "LoadingMode": "Fast" }"#);

    assert_eq!(r.store().load(None).expect("load").ui_language, "vi");
}

// C#: SettingsStoreLegacyAndWriteFailureTests.Load_CurrentVersionConfig_KeepsItsLanguage
#[test]
fn load_current_version_config_keeps_its_language() {
    let r = Rig::new();
    r.put(r#"{ "ConfigVersion": 3, "UiLanguage": "auto" }"#);

    assert_eq!(r.store().load(None).expect("load").ui_language, "auto");
}

// C#: SettingsStoreLegacyAndWriteFailureTests.Parse_V1Fixture_UiLanguageIsVietnamese
#[test]
fn parse_v1_fixture_ui_language_is_vietnamese() {
    let settings = SettingsStore::parse(&fixture_text("config-v1.json"), &EnglishSettingsTexts).expect("parse");

    assert_eq!(settings.ui_language, "vi");
}

// ---------------------------------------------------------------------------------------------------------------
// SettingsStoreLoadRepairPersistenceTests
// ---------------------------------------------------------------------------------------------------------------

// C#: SettingsStoreLoadRepairPersistenceTests.Load_DisabledShortcutConflict_IsPersistedSoTheSecondLoadReportsNothing
#[test]
fn load_disabled_shortcut_conflict_is_persisted_so_the_second_load_reports_nothing() {
    let r = Rig::new();
    r.put(CONFLICTING_CONFIG);
    let first = r.store();

    let loaded = first.load(None).expect("load");

    assert!(
        first
            .last_load_repairs()
            .contains(&"Shortcuts.ToggleKeepZoom".to_owned())
    );
    assert_eq!(loaded.shortcuts.toggle_keep_zoom, "");
    let second = r.store();
    let reloaded = second.load(None).expect("reload");
    assert!(second.last_load_repairs().is_empty());
    assert_eq!(reloaded.shortcuts.toggle_keep_zoom, "");
    assert_eq!(reloaded.actions.len(), 1);
    assert_eq!(reloaded.actions[0].shortcut, "K"); // the user's action binding is untouched
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_RepairThatCannotBePersisted_StillLoadsAndReportsTheRepair
#[test]
fn load_repair_that_cannot_be_persisted_still_loads_and_reports_the_repair() {
    let r = Rig::new();
    r.put(CONFLICTING_CONFIG);
    r.fs.fail(FsOp::WriteAtomic, r.cfg(), io::ErrorKind::Other);
    let store = r.store();
    let (changed, handler) = counter();
    store.subscribe_changed(handler());

    let loaded = store.load(None).expect("load");

    assert!(
        store
            .last_load_repairs()
            .contains(&"Shortcuts.ToggleKeepZoom".to_owned())
    );
    assert_eq!(loaded.shortcuts.toggle_keep_zoom, "");
    assert!(Arc::ptr_eq(&loaded, &store.current()));
    assert_eq!(changed.load(Ordering::SeqCst), 1);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_RepairedConfig_ThrowingChangedHandlerIsInvokedOnce
#[test]
fn load_repaired_config_throwing_changed_handler_is_invoked_once() {
    let r = Rig::new();
    r.put(CONFLICTING_CONFIG);
    let store = r.store();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    store.subscribe_changed(Box::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        Err(handler_bug())
    }));

    assert!(store.load(None).is_err());

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(r.store().load(None).expect("load").shortcuts.toggle_keep_zoom, ""); // the repair was written
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_FirstRun_ThrowingChangedHandlerIsInvokedOnce
#[test]
fn load_first_run_throwing_changed_handler_is_invoked_once() {
    let r = Rig::new();
    let store = r.store();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    store.subscribe_changed(Box::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        Err(handler_bug())
    }));

    assert!(store.load(None).is_err());

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(r.fs.file_exists(&r.cfg()));
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_RepairIsPersistedExactlyOnce_AndChangedIsRaisedOnce
#[test]
fn load_repair_is_persisted_exactly_once_and_changed_is_raised_once() {
    let r = Rig::new();
    r.put(CONFLICTING_CONFIG);
    let store = r.store();
    let (changed, handler) = counter();
    store.subscribe_changed(handler());

    store.load(None).expect("load");

    assert_eq!(r.fs.count(FsOp::WriteAtomic), 1);
    assert_eq!(changed.load(Ordering::SeqCst), 1);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_ConfigFromNewerBuildWithRepair_IsNotRewrittenButRepairedInMemory
#[test]
fn load_config_from_newer_build_with_repair_is_not_rewritten_but_repaired_in_memory() {
    let r = Rig::new();
    let newer = r#"{"ConfigVersion":99,"FutureField":"keep me","Actions":[{"Name":"Mine","Shortcut":"K"}]}"#;
    r.put(newer);
    let store = r.store();
    let (changed, handler) = counter();
    store.subscribe_changed(handler());

    let loaded = store.load(None).expect("load");

    assert_eq!(r.fs.count(FsOp::WriteAtomic), 0);
    assert_eq!(r.read(), newer);
    assert!(
        store
            .last_load_repairs()
            .contains(&"Shortcuts.ToggleKeepZoom".to_owned())
    );
    assert_eq!(loaded.shortcuts.toggle_keep_zoom, "");
    assert_eq!(changed.load(Ordering::SeqCst), 1);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_ValidConfigWithoutRepairs_DoesNotRewriteTheFile
#[test]
fn load_valid_config_without_repairs_does_not_rewrite_the_file() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3}"#);
    let store = r.store();

    store.load(None).expect("load");

    assert!(store.last_load_repairs().is_empty());
    assert_eq!(r.fs.count(FsOp::WriteAtomic), 0);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_ExplicitPath_DoesNotWriteTheDefaultConfigFile
#[test]
fn load_explicit_path_does_not_write_the_default_config_file() {
    let r = Rig::new();
    let other = PathBuf::from(r"C:\elsewhere\config.json");
    r.fs.add_file(&other, CONFLICTING_CONFIG.as_bytes());
    let store = r.store();

    let loaded = store.load(Some(&other)).expect("load");

    // Not vacuous: the explicit file really was read and repaired.
    assert!(
        store
            .last_load_repairs()
            .contains(&"Shortcuts.ToggleKeepZoom".to_owned())
    );
    assert_eq!(loaded.shortcuts.toggle_keep_zoom, "");
    assert_eq!(loaded.actions[0].shortcut, "K");
    assert_eq!(r.fs.count(FsOp::WriteAtomic), 0);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_OutOfRangeValue_RewritesConfigAndKeepsOriginalBackup
#[test]
fn load_out_of_range_value_rewrites_config_and_keeps_original_backup() {
    let r = Rig::new();
    r.put(OUT_OF_RANGE_CONFIG);
    let store = r.store();

    let loaded = store.load(None).expect("load");

    assert!(store.last_load_repairs().contains(&"PreloadWorkerCount".to_owned()));
    assert_ne!(loaded.preload_worker_count, 99999);
    assert_ne!(r.read(), OUT_OF_RANGE_CONFIG);
    let backups = r.listing("config.json.repaired-*");
    assert_eq!(backups.len(), 1);
    assert_eq!(r.text_of(&backups[0]), OUT_OF_RANGE_CONFIG);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_DisabledShortcutConflict_KeepsOriginalBackup
#[test]
fn load_disabled_shortcut_conflict_keeps_original_backup() {
    let r = Rig::new();
    r.put(CONFLICTING_CONFIG);

    r.store().load(None).expect("load");

    let backups = r.listing("config.json.repaired-*");
    assert_eq!(backups.len(), 1);
    assert_eq!(r.text_of(&backups[0]), CONFLICTING_CONFIG);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_Salvage_MakesExactlyOneBackup
#[test]
fn load_salvage_makes_exactly_one_backup() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3,"ClickZoomPercent":"abc"}"#);

    r.store().load(None).expect("load");

    assert_eq!(r.listing("config.json.corrupt-*").len(), 1);
    assert!(r.listing("config.json.repaired-*").is_empty());
    assert_eq!(r.listing("config.json.*").len(), 1);
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_CleanFile_MakesNoBackup
#[test]
fn load_clean_file_makes_no_backup() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3}"#);
    let store = r.store();

    store.load(None).expect("load");

    assert!(store.last_load_repairs().is_empty());
    assert!(r.listing("config.json.*").is_empty());
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_NewerVersionWithRepair_NoBackup
#[test]
fn load_newer_version_with_repair_makes_no_backup() {
    let r = Rig::new();
    let newer = r#"{"ConfigVersion":99,"PreloadWorkerCount":99999}"#;
    r.put(newer);
    let store = r.store();

    store.load(None).expect("load");

    assert!(!store.last_load_repairs().is_empty());
    assert_eq!(r.read(), newer);
    assert!(r.listing("config.json.*").is_empty());
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_RepairBackupFails_ConfigUntouchedAndRepairedInMemory
#[test]
fn load_repair_backup_fails_config_untouched_and_repaired_in_memory() {
    let r = Rig::new();
    r.put(OUT_OF_RANGE_CONFIG);
    r.fs.fail(FsOp::Copy, r.cfg(), io::ErrorKind::Other);
    let store = r.store();

    let loaded = store.load(None).expect("load");
    store.save((*loaded).clone()).expect("save"); // a later Save must honour the kept-file guard too

    assert_ne!(loaded.preload_worker_count, 99999);
    assert_eq!(*store.current(), *loaded);
    assert_eq!(r.read(), OUT_OF_RANGE_CONFIG);
    assert!(!r.startup_errors().is_empty());
    assert!(
        r.startup_messages()
            .iter()
            .any(|m| m == "Could not back up config.json before writing repairs")
    );
    assert!(r.listing("config.json.*").is_empty());
}

// C#: SettingsStoreLoadRepairPersistenceTests.Load_RepairBackup_NameIsDistinctFromConfig
#[test]
fn load_repair_backup_name_is_distinct_from_config() {
    let r = Rig::new();
    r.put(OUT_OF_RANGE_CONFIG);

    r.store().load(None).expect("load");

    let backups = r.listing("config.json.repaired-*");
    assert_eq!(backups.len(), 1);
    assert_ne!(backups[0], r.cfg());
    assert!(
        backups[0]
            .to_string_lossy()
            .starts_with(&format!("{}.repaired-", r.cfg().display()))
    );
}

// ---------------------------------------------------------------------------------------------------------------
// SettingsStoreMutationGapTests
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct RejectAllKeyNames;

impl KeyNameValidator for RejectAllKeyNames {
    fn is_valid_key_name(&self, _key_name: &str) -> bool {
        false
    }
}

// C#: SettingsStoreMutationGapTests.Constructor_CustomKeyNameValidator_IsExposedAndUsedForShortcutValidation
#[test]
fn constructor_custom_key_name_validator_is_exposed_and_used_for_shortcut_validation() {
    let r = Rig::new();
    let validator: Arc<dyn KeyNameValidator> = Arc::new(RejectAllKeyNames);
    let store = r.store().with_key_name_validator(Arc::clone(&validator));

    assert!(Arc::ptr_eq(&store.key_names(), &validator));
    assert!(store.validate_shortcuts(&defaults()).is_some());
}

// C#: SettingsStoreMutationGapTests.Constructor_NoValidator_FallsBackToAnyNonBlankName
#[test]
fn constructor_no_validator_falls_back_to_any_non_blank_name() {
    let r = Rig::new();
    let store = r.store();

    assert!(store.key_names().is_valid_key_name("F13"));
    assert!(!store.key_names().is_valid_key_name("  "));
    assert!(SimpleKeyNameValidator.is_valid_key_name("A"));
}

// C#: SettingsStoreMutationGapTests.Load_TransientReadFailure_IsRetriedAndLogged
#[test]
fn load_transient_read_failure_is_retried_and_logged() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3}"#);
    r.fs.fail_times(FsOp::ReadText, r.cfg(), io::ErrorKind::Other, 1);

    r.store().load(None).expect("load");

    assert!(r.startup_errors().is_empty());
    let retrying: Vec<String> = r
        .log_lines(Lv::Info)
        .into_iter()
        .filter(|m| m.contains("retrying"))
        .collect();
    assert_eq!(retrying.len(), 1);
    assert!(retrying[0].contains("IOException"));
}

// C#: SettingsStoreMutationGapTests.Load_CleanFile_ReportsNoRepairsAndLogsNothingAboutRepairsOrShortcuts
#[test]
fn load_clean_file_reports_no_repairs_and_logs_nothing_about_repairs_or_shortcuts() {
    let r = Rig::new();
    let store = r.store();
    store.save(defaults()).expect("save");

    store.load(None).expect("load");

    assert!(store.last_load_repairs().is_empty());
    assert!(r.log_lines(Lv::Warn).is_empty());
    assert!(!r.log_lines(Lv::Info).iter().any(|m| m.contains("Optional shortcuts")));
}

// C#: SettingsStoreMutationGapTests.Load_UnusableValue_LogsAWarningNamingTheResetSetting
#[test]
fn load_unusable_value_logs_a_warning_naming_the_reset_setting() {
    let r = Rig::new();
    r.put(MISTYPED_VALUE_CONFIG);
    let store = r.store();

    store.load(None).expect("load");

    assert!(store.last_load_repairs().contains(&"ClickZoomPercent".to_owned()));
    let warnings = r.log_lines(Lv::Warn);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("ClickZoomPercent"));
}

// C#: SettingsStoreMutationGapTests.Load_RepairThatCannotBePersisted_ReportsTheWriteFailureAndKeepsTheRepairInMemory
#[test]
fn load_repair_that_cannot_be_persisted_reports_the_write_failure_and_keeps_the_repair_in_memory() {
    let r = Rig::new();
    r.put(MISTYPED_VALUE_CONFIG);
    r.fs.fail(FsOp::WriteAtomic, r.cfg(), io::ErrorKind::Other);
    let store = r.store();
    let (changed, handler) = counter();
    store.subscribe_changed(handler());

    let loaded = store.load(None).expect("load");

    assert!(r.startup_messages().iter().any(|m| m.contains("repaired config.json")));
    assert!(Arc::ptr_eq(&loaded, &store.current()));
    assert!(store.last_load_repairs().contains(&"ClickZoomPercent".to_owned()));
    assert_eq!(changed.load(Ordering::SeqCst), 1);
}

// C#: SettingsStoreMutationGapTests.Load_ExplicitPathThatDoesNotExist_ReturnsDefaultsMakesThemCurrentAndRaisesChanged
#[test]
fn load_explicit_path_that_does_not_exist_returns_defaults_makes_them_current_and_raises_changed() {
    let r = Rig::new();
    let store = r.store();
    let mut custom = defaults();
    custom.click_zoom_percent = 321;
    store.save(custom).expect("save");
    let events: Arc<Mutex<Vec<Arc<AppSettings>>>> = Arc::new(Mutex::new(Vec::new()));
    let e = Arc::clone(&events);
    store.subscribe_changed(Box::new(move |s| {
        e.lock().unwrap_or_else(PoisonError::into_inner).push(Arc::clone(s));
        Ok(())
    }));
    let missing = PathBuf::from(r"C:\elsewhere\missing.json");
    let writes_before = r.fs.count(FsOp::WriteAtomic);

    let loaded = store.load(Some(&missing)).expect("load");

    assert_eq!(loaded.click_zoom_percent, defaults().click_zoom_percent);
    assert!(Arc::ptr_eq(&loaded, &store.current()));
    let events = events.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(events.len(), 1);
    assert!(Arc::ptr_eq(&loaded, &events[0]));
    assert!(!r.fs.file_exists(&missing)); // an explicit path is never written
    assert_eq!(r.fs.count(FsOp::WriteAtomic), writes_before);
}

// C#: SettingsStoreMutationGapTests.Save_CreatesTheConfigDirectoryBeforeWriting
#[test]
fn save_creates_the_config_directory_before_writing() {
    let r = Rig::new();
    let proxy = ProxyFs::new(Arc::clone(&r.fs));
    let store = r.store_on(Arc::clone(&proxy) as Arc<dyn FileSystem>);

    store.save(defaults()).expect("save");

    let calls = proxy.calls.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let create = calls
        .iter()
        .position(|c| *c == "CreateDirectory")
        .expect("CreateDirectory was called");
    let write = calls
        .iter()
        .position(|c| *c == "WriteAllTextAtomic")
        .expect("write was called");
    assert!(
        write > create,
        "the directory must exist before the atomic write: {calls:?}"
    );
}

// C#: SettingsStoreMutationGapTests.Load_CorruptConfigWithEveryBackupNameTaken_UsesTheLastNumberedNameBelowOneHundred
#[test]
fn load_corrupt_config_with_every_backup_name_taken_uses_the_last_numbered_name_below_one_hundred() {
    let r = Rig::new();
    r.put("{ not json");
    let proxy = ProxyFs::new(Arc::clone(&r.fs));
    proxy.swallow_copies.store(true, Ordering::SeqCst);
    // Taken: the plain name and -2 .. -99; free: -100 and up.
    *proxy.exists_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(|path| {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let rest = name.strip_prefix("config.json.corrupt-")?;
        let counter = rest.split_once('-').map(|(_, n)| n.parse::<i32>().unwrap_or(0));
        Some(counter.is_none_or(|n| n < 100))
    }));
    let store = r.store_on(Arc::clone(&proxy) as Arc<dyn FileSystem>);

    store.load(None).expect("load");

    let copies = proxy.copies.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let first = copies.first().expect("a backup copy was attempted");
    assert!(first.to_string_lossy().ends_with("-99"), "{}", first.display());
}

// C#: SettingsStoreMutationGapTests.Load_CorruptConfigWithFirstBackupNameTaken_UsesTheSecondName
#[test]
fn load_corrupt_config_with_first_backup_name_taken_uses_the_second_name() {
    let r = Rig::new();
    r.put("{ not json");
    let proxy = ProxyFs::new(Arc::clone(&r.fs));
    proxy.swallow_copies.store(true, Ordering::SeqCst);
    *proxy.exists_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(|path| {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let rest = name.strip_prefix("config.json.corrupt-")?;
        Some(!rest.contains('-')) // only the plain name is taken
    }));
    let store = r.store_on(Arc::clone(&proxy) as Arc<dyn FileSystem>);

    store.load(None).expect("load");

    let copies = proxy.copies.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let first = copies.first().expect("a backup copy was attempted");
    assert!(first.to_string_lossy().ends_with("-2"), "{}", first.display());
}

// ---------------------------------------------------------------------------------------------------------------
// SettingsStoreSaveAfterNewerVersionLoadTests
// ---------------------------------------------------------------------------------------------------------------

fn load_and_save(r: &Rig, file_text: &str) {
    r.put(file_text);
    let store = r.store();
    let loaded = store.load(None).expect("load");
    store.save((*loaded).clone()).expect("save");
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_AfterLoadingNewerVersion_WritesCurrentVersionWithoutUnknownFields
#[test]
fn save_after_loading_newer_version_writes_current_version_without_unknown_fields() {
    let r = Rig::new();
    r.put(NEWER);
    let store = r.store();
    let loaded = store.load(None).expect("load");
    assert_eq!(loaded.config_version, 99); // Load keeps the newer stamp in memory...
    assert_eq!(r.read(), NEWER); // ...and does not touch the file
    assert!(r.siblings().is_empty()); // Load itself makes no backup

    let mut edited = (*loaded).clone();
    edited.click_zoom_percent = 300;
    store.save(edited).expect("save");

    let written = r.read();
    assert!(written.contains("\"ConfigVersion\": 3"));
    assert!(!written.contains("FutureField"));
    assert!(written.contains("\"ClickZoomPercent\": 300"));
    assert_eq!(store.current().config_version, 3);
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_AfterLoadingNewerVersion_KeepsVerbatimBackupOfTheOriginal
#[test]
fn save_after_loading_newer_version_keeps_verbatim_backup_of_the_original() {
    let r = Rig::new();
    r.put(NEWER);
    let store = r.store();
    let loaded = store.load(None).expect("load");
    assert!(store.last_load_repairs().is_empty());

    store.save((*loaded).clone()).expect("save");

    let backups = r.newer_backups();
    assert_eq!(backups.len(), 1);
    let name = name_of(&backups[0]);
    let stamp = name.strip_prefix("config.json.newer-").expect("prefix");
    assert_eq!(stamp.len(), 14);
    assert!(stamp.bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(backups[0].parent(), r.cfg().parent());
    assert_eq!(r.text_of(&backups[0]), NEWER);
    assert!(r.startup_errors().is_empty());
    assert!(store.last_load_repairs().is_empty());
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_Twice_BacksUpOnlyTheNewerOriginal
#[test]
fn save_twice_backs_up_only_the_newer_original() {
    let r = Rig::new();
    r.put(NEWER);
    let store = r.store();
    let loaded = store.load(None).expect("load");
    store.save((*loaded).clone()).expect("save 1");
    store.save((*loaded).clone()).expect("save 2");

    let backups = r.newer_backups();
    assert_eq!(backups.len(), 1);
    assert_eq!(r.text_of(&backups[0]), NEWER);
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_FileNotNewer_MakesNoBackup (3, 2, 1)
#[test]
fn save_file_not_newer_makes_no_backup() {
    for version in [3, 2, 1] {
        let r = Rig::new();
        load_and_save(&r, &format!("{{\"ConfigVersion\":{version},\"ClickZoomPercent\":250}}"));

        assert!(r.newer_backups().is_empty(), "version {version}");
    }
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_TwiceInSameSecond_UsesUniqueNames
#[test]
fn save_twice_in_same_second_uses_unique_names() {
    let r = Rig::new();
    let first = NEWER.replace("123", "1");
    let second = NEWER.replace("123", "2");
    load_and_save(&r, &first);
    load_and_save(&r, &second);

    let mut contents: Vec<String> = r.newer_backups().iter().map(|f| r.text_of(f)).collect();
    contents.sort();
    assert_eq!(contents, vec![first, second]);
    let mut names: Vec<String> = r.newer_backups().iter().map(|f| name_of(f).to_lowercase()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 2);
    // The second one is the bare stamp plus "-2" (a bare stamp counts as 1).
    assert!(names.iter().any(|n| n.ends_with("-2")));
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_ManyTimes_KeepsAtMostThreeNewestBackups
#[test]
fn save_many_times_keeps_at_most_three_newest_backups() {
    let r = Rig::new();
    let corrupt = PathBuf::from(format!("{}.corrupt-20200101000000", r.cfg().display()));
    r.fs.add_file(&corrupt, b"keep me");
    for i in 1..=5 {
        load_and_save(&r, &NEWER.replace("123", &i.to_string()));
    }

    let mut contents: Vec<String> = r.newer_backups().iter().map(|f| r.text_of(f)).collect();
    contents.sort();
    let mut expected: Vec<String> = ["3", "4", "5"].iter().map(|n| NEWER.replace("123", n)).collect();
    expected.sort();
    assert_eq!(contents, expected);
    assert!(r.fs.file_exists(&corrupt));
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_WhenBackupFails_StillWritesTheConfig
#[test]
fn save_when_backup_fails_still_writes_the_config() {
    let r = Rig::new();
    r.put(NEWER);
    let store = r.store();
    let loaded = store.load(None).expect("load");
    let mut edited = (*loaded).clone();
    edited.click_zoom_percent = 310;
    r.fs.fail(FsOp::Copy, r.cfg(), io::ErrorKind::Other);

    store.save(edited).expect("save is not blocked");

    assert!(r.newer_backups().is_empty());
    let written = r.read();
    assert!(written.contains("\"ConfigVersion\": 3"));
    assert!(written.contains("\"ClickZoomPercent\": 310"));
    assert!(r.has(
        Lv::Warn,
        "Could not back up the newer-build config.json before overwriting it: "
    ));
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Load_AfterSaveOfNewerVersionFile_ReadsBackCurrentVersion
#[test]
fn load_after_save_of_newer_version_file_reads_back_current_version() {
    let r = Rig::new();
    r.put(NEWER);
    let first = r.store();
    let loaded = first.load(None).expect("load");
    first.save((*loaded).clone()).expect("save");

    let second = r.store();
    let reloaded = second.load(None).expect("reload");

    assert_eq!(reloaded.config_version, 3);
    assert_eq!(reloaded.click_zoom_percent, 250);
    assert!(second.last_load_repairs().is_empty());
}

// C#: SettingsStoreSaveAfterNewerVersionLoadTests.Save_AlwaysStampsCurrentConfigVersion
#[test]
fn save_always_stamps_current_config_version() {
    let r = Rig::new();
    let store = r.store();
    let mut settings = defaults();
    settings.config_version = 99;

    store.save(settings).expect("save");

    assert!(r.read().contains("\"ConfigVersion\": 3"));
}

// ---- newer-backup prune paths the C# tests only reach through the proxy (contract note 8) ----

// C#: (contract note 8) a copy failure prunes nothing; a delete failure warns; a listing failure warns
#[test]
fn newer_backup_prune_failures_are_logged_and_never_block_the_save() {
    // Delete failure: four existing backups, one more is made, the oldest is not deletable.
    let r = Rig::new();
    for stamp in ["20200101000001", "20200101000002", "20200101000003", "20200101000004"] {
        r.fs.add_file(format!("{}.newer-{stamp}", r.cfg().display()), b"old");
    }
    let oldest = PathBuf::from(format!("{}.newer-20200101000001", r.cfg().display()));
    r.fs.fail(FsOp::Delete, &oldest, io::ErrorKind::PermissionDenied);
    r.put(NEWER);
    let store = r.store();
    let loaded = store.load(None).expect("load");

    store.save((*loaded).clone()).expect("save");

    assert!(r.has(Lv::Warn, "Could not delete an old newer-build config backup: "));
    assert!(r.fs.file_exists(&oldest));
    assert!(r.read().contains("\"ConfigVersion\": 3"));

    // Listing failure: the name picker's listing fails too, so the copy branch takes the warning and prunes nothing.
    let r = Rig::new();
    r.put(NEWER);
    let store = r.store();
    let loaded = store.load(None).expect("load");
    r.fs.fail(FsOp::EnumerateFiles, r.dir(), io::ErrorKind::Other);

    store.save((*loaded).clone()).expect("save");

    assert!(r.has(
        Lv::Warn,
        "Could not back up the newer-build config.json before overwriting it: "
    ));
    assert!(r.read().contains("\"ConfigVersion\": 3"));
}

// ---------------------------------------------------------------------------------------------------------------
// SessionWritePolicyTests.SettingsSave_UsesDurableAtomicWrite
// ---------------------------------------------------------------------------------------------------------------

// C#: SessionWritePolicyTests.SettingsSave_UsesDurableAtomicWrite
#[test]
fn settings_save_uses_durable_atomic_write() {
    let r = Rig::new();
    let proxy = ProxyFs::new(Arc::clone(&r.fs));
    let store = r.store_on(Arc::clone(&proxy) as Arc<dyn FileSystem>);

    store.save(defaults()).expect("save");

    let writes = proxy
        .atomic_writes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, r.cfg());
    assert!(writes[0].1, "settings are written durably");
    let bytes = r.fs.file_content(r.cfg()).expect("written");
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "no BOM");
    assert!(bytes_to_text(&bytes).contains("\r\n"), "CRLF line endings");
}

// ---------------------------------------------------------------------------------------------------------------
// Store-level behaviours that the goldens do not pin
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn startup_error_goes_to_log_error_without_a_callback() {
    let r = Rig::new();
    r.put("{ nope");
    let store = SettingsStore::new(
        r.paths.clone(),
        Arc::clone(&r.fs) as Arc<dyn FileSystem>,
        Arc::clone(&r.log) as Arc<dyn pr_core::abstractions::Log>,
        Arc::clone(&r.clock) as Arc<dyn Clock>,
        english(),
    );

    store.load(None).expect("load");

    assert!(r.has(Lv::Error, "Corrupt config.json detected, resetting to defaults"));
}

#[test]
fn last_load_repairs_is_reset_at_the_start_of_every_load() {
    let r = Rig::new();
    r.put(OUT_OF_RANGE_CONFIG);
    let store = r.store();
    store.load(None).expect("load");
    assert!(!store.last_load_repairs().is_empty());

    store
        .load(Some(Path::new(r"C:\elsewhere\missing.json")))
        .expect("explicit load");

    assert!(store.last_load_repairs().is_empty());
}

#[test]
fn backup_stamp_is_the_utc_time_of_the_injected_clock() {
    let r = Rig::new();
    r.clock.set(
        pr_core::time::DateTimeUtc::parse_json("2026-10-14T09:08:07Z")
            .expect("literal")
            .0,
    );
    r.put("{ nope");

    r.store().load(None).expect("load");

    let backups = r.listing("config.json.corrupt-*");
    assert_eq!(backups.len(), 1);
    assert_eq!(name_of(&backups[0]), "config.json.corrupt-20261014090807");
}

#[test]
fn changed_handlers_run_in_subscription_order_and_may_subscribe_during_a_raise() {
    let r = Rig::new();
    let store = Arc::new(r.store());
    let order = Arc::new(Mutex::new(Vec::new()));
    let o1 = Arc::clone(&order);
    let s = Arc::clone(&store);
    store.subscribe_changed(Box::new(move |_| {
        o1.lock().unwrap_or_else(PoisonError::into_inner).push(1);
        // A handler may call back into the store without a deadlock.
        let _ = s.current();
        s.subscribe_changed(Box::new(|_| Ok(())));
        Ok(())
    }));
    let o2 = Arc::clone(&order);
    store.subscribe_changed(Box::new(move |_| {
        o2.lock().unwrap_or_else(PoisonError::into_inner).push(2);
        Ok(())
    }));

    store.save(defaults()).expect("save");

    assert_eq!(*order.lock().unwrap_or_else(PoisonError::into_inner), vec![1, 2]);
}

#[test]
fn parse_text_null_is_defaults_and_not_an_error() {
    let parsed = SettingsStore::parse_text("null", &EnglishSettingsTexts).expect("null parses");
    assert!(parsed.is_json_null);
    assert!(parsed.salvaged.is_empty());
    assert!(SettingsStore::parse_text("[1]", &EnglishSettingsTexts).is_err());
    assert!(SettingsStore::parse_text("", &EnglishSettingsTexts).is_err());
    assert!(SettingsStore::parse("[1]", &EnglishSettingsTexts).is_err());
    assert_eq!(SettingsStore::parse("null", &EnglishSettingsTexts).expect("null"), {
        let mut d = defaults();
        SettingsStore::migrate(&mut d);
        d
    });
}

#[test]
fn migrate_treats_auto_ignoring_ascii_case_as_not_chosen() {
    let mut s = defaults();
    s.config_version = 2;
    s.ui_language = "AUTO".to_owned();
    SettingsStore::migrate(&mut s);
    assert_eq!(s.ui_language, "vi");

    let mut s = defaults();
    s.config_version = 4;
    s.ui_language = "AUTO".to_owned();
    SettingsStore::migrate(&mut s);
    assert_eq!(s.ui_language, "AUTO");
    assert_eq!(s.config_version, 4); // a newer version is never lowered
}

// ---------------------------------------------------------------------------------------------------------------
// R0002: first-run import of the C# config.json (Rust-only, no C# twin)
// ---------------------------------------------------------------------------------------------------------------

fn csharp_cfg() -> PathBuf {
    csharp_config_path(Path::new(LOCAL_APP_DATA))
}

fn assert_no_writes_under_csharp_folder(r: &Rig) {
    for file in r.fs.all_files() {
        let in_csharp = file.starts_with(csharp_cfg().parent().expect("dir"));
        assert!(
            !in_csharp || file == csharp_cfg(),
            "unexpected file in the C# folder: {}",
            file.display()
        );
    }
}

#[test]
fn csharp_config_path_is_under_the_csharp_app_folder() {
    assert_eq!(
        csharp_cfg(),
        PathBuf::from(r"C:\Users\test\AppData\Local\PhotoReview\config.json")
    );
    assert_eq!(CONFIG_FILE_NAME, "config.json");
}

#[test]
fn import_first_run_with_csharp_config_imports_it_read_only() {
    let r = Rig::new();
    let original = r#"{"ConfigVersion":3,"UiLanguage":"en","ClickZoomPercent":250,"LoggingEnabled":true}"#;
    r.fs.add_file(csharp_cfg(), original.as_bytes());
    let store = r.store();
    let (changed, handler) = counter();
    store.subscribe_changed(handler());
    r.fs.reset_counts();

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert_eq!(loaded.click_zoom_percent, 250);
    assert_eq!(loaded.ui_language, "en");
    assert!(loaded.logging_enabled);
    assert!(Arc::ptr_eq(&loaded, &store.current()));
    assert_eq!(changed.load(Ordering::SeqCst), 1);
    let own = r.read();
    assert!(own.contains("\"ClickZoomPercent\": 250"));
    assert!(own.contains("\"ConfigVersion\": 3"));
    // Nothing is ever written into the C# folder.
    assert_eq!(r.fs.count(FsOp::Copy), 0);
    assert_eq!(r.fs.count(FsOp::Delete), 0);
    assert_eq!(r.fs.count(FsOp::Move), 0);
    assert_eq!(r.fs.count(FsOp::WriteAtomic), 1);
    assert_eq!(r.text_of(&csharp_cfg()), original);
    assert_no_writes_under_csharp_folder(&r);
    assert!(r.has(Lv::Info, "Imported the settings of the C# app"));
}

#[test]
fn import_when_own_config_exists_is_exactly_load() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3,"ClickZoomPercent":175}"#);
    r.fs.add_file(csharp_cfg(), br#"{"ConfigVersion":3,"ClickZoomPercent":250}"#);
    let store = r.store();
    r.fs.reset_counts();

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert_eq!(loaded.click_zoom_percent, 175);
    assert_eq!(r.fs.count(FsOp::ReadText), 1); // only the own file was read
    assert_eq!(r.fs.count(FsOp::WriteAtomic), 0);
}

#[test]
fn import_when_csharp_config_is_missing_writes_defaults() {
    let r = Rig::new();
    let store = r.store();

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert_eq!(*loaded, {
        let mut d = defaults();
        d.config_version = 3;
        d
    });
    assert!(r.fs.file_exists(&r.cfg()));
}

#[test]
fn import_unreadable_csharp_config_warns_and_writes_defaults() {
    let r = Rig::new();
    r.fs.add_file(csharp_cfg(), br#"{"ConfigVersion":3,"ClickZoomPercent":250}"#);
    r.fs.fail(FsOp::ReadText, csharp_cfg(), io::ErrorKind::PermissionDenied);
    let store = r.store();

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert_eq!(loaded.click_zoom_percent, defaults().click_zoom_percent);
    assert!(!r.log_lines(Lv::Warn).is_empty());
    assert!(r.fs.file_exists(&r.cfg()));
    assert_eq!(r.sleeps_ms(), vec![50, 100, 200, 400]);
    assert_no_writes_under_csharp_folder(&r);
}

#[test]
fn import_csharp_config_with_garbage_leaves_no_backup_and_no_write_in_the_csharp_folder() {
    let r = Rig::new();
    r.fs.add_file(csharp_cfg(), b"{ this is not json");
    let store = r.store();
    r.fs.reset_counts();

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert_eq!(loaded.click_zoom_percent, defaults().click_zoom_percent);
    assert!(!r.log_lines(Lv::Warn).is_empty());
    assert_eq!(r.fs.count(FsOp::Copy), 0);
    assert_eq!(r.fs.count(FsOp::Delete), 0);
    assert_eq!(r.fs.count(FsOp::Move), 0);
    assert_eq!(r.fs.count(FsOp::WriteAtomic), 1); // the defaults, to the own config
    assert!(r.startup_errors().is_empty());
    assert_no_writes_under_csharp_folder(&r);
    assert_eq!(r.text_of(&csharp_cfg()), "{ this is not json");
}

#[test]
fn import_of_a_newer_csharp_config_makes_no_newer_backup() {
    let r = Rig::new();
    r.fs.add_file(csharp_cfg(), NEWER.as_bytes());
    let store = r.store();

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert_eq!(loaded.config_version, 3); // written as the current version of the OWN file
    assert!(r.newer_backups().is_empty());
    assert!(r.siblings().is_empty());
    assert_eq!(r.fs.count(FsOp::Copy), 0);
    store.save((*loaded).clone()).expect("later save");
    assert!(r.newer_backups().is_empty());
}

#[test]
fn import_with_repairs_sets_last_load_repairs_and_write_failure_keeps_the_import_in_memory() {
    let r = Rig::new();
    r.fs.add_file(csharp_cfg(), OUT_OF_RANGE_CONFIG.as_bytes());
    r.fs.fail(FsOp::WriteAtomic, r.cfg(), io::ErrorKind::Other);
    let store = r.store();

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert!(store.last_load_repairs().contains(&"PreloadWorkerCount".to_owned()));
    assert_ne!(loaded.preload_worker_count, 99999);
    assert!(Arc::ptr_eq(&loaded, &store.current()));
    assert!(
        r.startup_messages()
            .iter()
            .any(|m| m.contains("Could not write default config.json"))
    );
    assert!(!r.fs.file_exists(&r.cfg()));
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: settings-parse-text
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn golden_settings_parse_text() {
    let g = golden::load("settings-parse-text");
    assert!(g.csharp_commit.starts_with("d92d6aae"), "baseline {}", g.csharp_commit);
    assert_eq!(g.cases.len(), 66);
    // Every case runs; failures are collected so one bad case does not hide the others.
    let mut failures: Vec<String> = Vec::new();
    for case in &g.cases {
        let outcome = std::panic::catch_unwind(|| check_parse_text_case(case));
        if outcome.is_err() {
            failures.push(str_of(case, "name").to_owned());
        }
    }
    assert!(failures.is_empty(), "failing cases: {failures:#?}");
}

fn check_parse_text_case(case: &Value) {
    let texts = EnglishSettingsTexts;
    {
        let name = str_of(case, "name");
        let text = str_of(case, "text");
        let result = SettingsStore::parse_text(text, &texts);
        if str_of(case, "outcome") == "error" {
            assert!(result.is_err(), "{name}: expected an error");
            assert_eq!(str_of(case, "error_type"), "JsonException", "{name}");
            return;
        }
        let parsed = result.unwrap_or_else(|e| panic!("{name}: unexpected error {e}"));
        let strings = |key: &str| -> Vec<String> {
            case[key]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: no {key}"))
                .iter()
                .map(|v| v.as_str().expect("string").to_owned())
                .collect()
        };
        assert_eq!(
            case["is_json_null"].as_bool(),
            Some(parsed.is_json_null),
            "{name}: is_json_null"
        );
        assert_eq!(strings("salvaged"), parsed.salvaged, "{name}: salvaged");
        assert_eq!(
            strings("reset_settings"),
            parsed.reset_settings,
            "{name}: reset_settings"
        );
        assert_eq!(
            strings("disabled_shortcuts"),
            parsed.disabled_shortcuts,
            "{name}: disabled_shortcuts"
        );
        assert_eq!(strings("repairs"), parsed.repairs(), "{name}: repairs");
        match &case["salvage_cause"] {
            Value::Null => assert!(parsed.salvage_cause.is_none(), "{name}: salvage_cause"),
            Value::String(kind) => {
                assert_eq!(kind, "JsonException", "{name}");
                assert!(parsed.salvage_cause.is_some(), "{name}: salvage_cause");
            }
            other => panic!("{name}: salvage_cause {other}"),
        }
        assert_eq!(
            case["config_version"].as_i64(),
            Some(i64::from(parsed.settings.config_version)),
            "{name}: config_version"
        );
        assert_eq!(
            str_of(case, "repair_text"),
            build_repair_text(&parsed.repairs(), false, &texts),
            "{name}: repair_text"
        );
        let expected: BTreeMap<String, String> = case["changed"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: no changed"))
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().expect("segment").to_owned()))
            .collect();
        assert_eq!(expected, changed_vs_defaults(&parsed.settings), "{name}: changed");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: settings-store (real PhysicalFileSystem over a TempRoot: the three encoding scenarios need it)
// ---------------------------------------------------------------------------------------------------------------

/// Replaces every run of exactly 14 digits that follows a `-` with `<STAMP>`.
fn replace_stamps(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '-' {
            let run = chars[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if run == 14 {
                out.push_str("-<STAMP>");
                i += 15;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn golden_inputs(name: &str) -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn read_dir_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                let bytes = std::fs::read(entry.path()).expect("read file");
                out.push((entry.file_name().to_string_lossy().into_owned(), bytes));
            }
        }
    }
    out
}

#[test]
fn golden_settings_store() {
    let expected_all = golden::load("settings-store");
    assert!(expected_all.csharp_commit.starts_with("d92d6aae"));
    let inputs = golden_inputs("settings-store");
    let scenarios = inputs["scenarios"].as_array().expect("scenarios");
    assert_eq!(scenarios.len(), 43);
    assert_eq!(expected_all.cases.len(), 43);

    // Every scenario runs; failures are collected so one bad scenario does not hide the others.
    let mut failures: Vec<String> = Vec::new();
    for (input, expected) in scenarios.iter().zip(&expected_all.cases) {
        let name = str_of(input, "name");
        assert_eq!(name, str_of(expected, "name"));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_store_scenario(input, expected)));
        if outcome.is_err() {
            failures.push(name.to_owned());
        }
    }
    assert!(failures.is_empty(), "failing scenarios: {failures:#?}");
}

fn run_store_scenario(input: &Value, expected: &Value) {
    {
        let name = str_of(input, "name");
        let root = TempRoot::new("w2-03-store");
        let config_dir = root.join("PhotoReview");
        let other_dir = root.join("other");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        std::fs::create_dir_all(&other_dir).expect("other dir");
        for file in input["files"].as_array().expect("files") {
            let dir = if str_of(file, "dir") == "config" {
                &config_dir
            } else {
                &other_dir
            };
            let bytes = match file.get("b64").and_then(Value::as_str) {
                Some(b64) => b64_decode(b64),
                None => str_of(file, "text").as_bytes().to_vec(),
            };
            std::fs::write(dir.join(str_of(file, "name")), bytes).expect("write input");
        }

        let mut paths = AppPaths::new(root.path(), None, false);
        paths.config_file = config_dir.join("config.json");
        let log = Arc::new(RecordingLog::new());
        let startup: StartupLog = Arc::new(Mutex::new(Vec::new()));
        let st = Arc::clone(&startup);
        let store = SettingsStore::new(
            paths,
            Arc::new(PhysicalFileSystem),
            Arc::clone(&log) as Arc<dyn pr_core::abstractions::Log>,
            Arc::new(FakeClock::new()),
            english(),
        )
        .with_startup_error_callback(Box::new(move |m, e| {
            st.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((m.to_owned(), dotnet_type_name(e)));
        }));
        let (events, handler) = counter();
        store.subscribe_changed(handler());

        let expected_steps = expected["steps"].as_array().expect("steps");
        for (i, (step, want)) in input["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .zip(expected_steps)
            .enumerate()
        {
            let before = events.load(Ordering::SeqCst);
            let mut thrown: Option<&str> = None;
            match str_of(step, "op") {
                "load" => {
                    store
                        .load(None)
                        .unwrap_or_else(|e| panic!("{name}[{i}]: load failed: {e}"));
                }
                "load_explicit" => {
                    let file = other_dir.join(str_of(step, "file"));
                    store
                        .load(Some(&file))
                        .unwrap_or_else(|e| panic!("{name}[{i}]: load failed: {e}"));
                }
                "save" => {
                    let settings = AppSettings::from_json(str_of(step, "settings"), &EnglishSettingsTexts)
                        .unwrap_or_else(|e| panic!("{name}[{i}]: settings input: {e}"))
                        .unwrap_or_else(defaults);
                    match store.save(settings) {
                        Ok(_) => {}
                        Err(SettingsSaveError::Json(_)) => thrown = Some("ArgumentException"),
                        Err(SettingsSaveError::Io(e)) => thrown = Some(dotnet_io(&e)),
                    }
                }
                other => panic!("{name}[{i}]: unknown op {other}"),
            }
            assert_eq!(
                want.get("thrown").and_then(Value::as_str),
                thrown,
                "{name}[{i}]: thrown"
            );
            let repairs: Vec<String> = want["repairs"]
                .as_array()
                .expect("repairs")
                .iter()
                .map(|v| v.as_str().expect("string").to_owned())
                .collect();
            assert_eq!(repairs, store.last_load_repairs(), "{name}[{i}]: repairs");
            let current = store.current();
            assert_eq!(
                want["config_version"].as_i64(),
                Some(i64::from(current.config_version)),
                "{name}[{i}]: config_version"
            );
            let want_changed: BTreeMap<String, String> = want["changed"]
                .as_object()
                .expect("changed")
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().expect("segment").to_owned()))
                .collect();
            assert_eq!(want_changed, changed_vs_defaults(&current), "{name}[{i}]: changed");
            assert_eq!(
                want["changed_events"].as_u64(),
                Some((events.load(Ordering::SeqCst) - before) as u64),
                "{name}[{i}]: changed_events"
            );
        }

        // Files left behind (names with stamps replaced, bytes exact).
        let mut want_files: Vec<(String, String, Vec<u8>)> = expected["files_after"]
            .as_array()
            .expect("files_after")
            .iter()
            .map(|f| {
                (
                    str_of(f, "dir").to_owned(),
                    str_of(f, "name").to_owned(),
                    b64_decode(str_of(f, "b64")),
                )
            })
            .collect();
        let mut got_files: Vec<(String, String, Vec<u8>)> = Vec::new();
        for (label, dir) in [("config", &config_dir), ("other", &other_dir)] {
            for (file_name, bytes) in read_dir_files(dir) {
                got_files.push((label.to_owned(), replace_stamps(&file_name), bytes));
            }
        }
        want_files.sort();
        got_files.sort();
        assert_eq!(want_files, got_files, "{name}: files_after");

        // Startup errors: "<message> [<.NET type>]".
        let want_errors: Vec<String> = expected["startup_errors"]
            .as_array()
            .expect("startup_errors")
            .iter()
            .map(|v| v.as_str().expect("string").to_owned())
            .collect();
        let got_errors: Vec<String> = startup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(m, t)| format!("{m} [{t}]"))
            .collect();
        assert_eq!(want_errors, got_errors, "{name}: startup_errors");

        // Log lines with <ROOT> and <STAMP> placeholders.
        let root_text = root.path().to_string_lossy().into_owned();
        let want_log: Vec<String> = expected["log"]
            .as_array()
            .expect("log")
            .iter()
            .map(|v| v.as_str().expect("string").to_owned())
            .collect();
        let got_log: Vec<String> = log
            .entries()
            .into_iter()
            .map(|(level, message)| {
                let prefix = match Lv::of(&level) {
                    Lv::Info => "info",
                    Lv::Warn => "warn",
                    Lv::Error => "error",
                };
                replace_stamps(&format!("{prefix}: {message}").replace(&root_text, "<ROOT>"))
            })
            .collect();
        assert_eq!(want_log, got_log, "{name}: log");
    }
}

fn dotnet_io(e: &io::Error) -> &'static str {
    if e.kind() == io::ErrorKind::PermissionDenied {
        "UnauthorizedAccessException"
    } else {
        "IOException"
    }
}

// Golden-less sanity: the golden helpers themselves.
#[test]
fn replace_stamps_only_touches_fourteen_digit_runs() {
    assert_eq!(
        replace_stamps("config.json.corrupt-20261014090807"),
        "config.json.corrupt-<STAMP>"
    );
    assert_eq!(replace_stamps("a-123-b"), "a-123-b");
    assert_eq!(replace_stamps("x.newer-20261014090807-2"), "x.newer-<STAMP>-2");
    assert_eq!(b64_decode("TWFu"), b"Man");
}

// ---- repair round 1: salvage enum fallback, counter wrap, flag-guard mutations ----

// C#: TrySalvage deserialises with the property type (type-level LenientEnumConverter): an unparsable value of one of
// the eight SettingsEnumConverter properties becomes default(T) and is NOT reported as unusable.
#[test]
fn salvage_enum_uses_type_level_default_and_is_not_reported() {
    let root = pr_core::settings::app_settings::json::parse(
        r#"{"ConfigVersion":3,"ClickZoomPercent":"abc","LoadingMode":"x","ImageSortMode":"nope","ScalingQuality":"?","DecoderBackend":"?","KineticGlideSmoothing":"?","FitWidthAnchor2":"?","MiddleClickAction":"?","KeyboardZoomAnchor":"?"}"#,
    )
    .expect("parse");
    let (settings, unusable) = SettingsStore::try_salvage(&root, &EnglishSettingsTexts).expect("object");
    assert_eq!(unusable, vec!["ClickZoomPercent".to_owned()]);
    assert_eq!(settings.loading_mode, Some(LoadingMode::Fast));
    assert!(settings.image_sort_mode.is_some());
    assert!(settings.scaling_quality.is_some());
    assert!(settings.decoder_backend.is_some());
    assert!(settings.kinetic_glide_smoothing.is_some());
    assert!(settings.fit_width_anchor2.is_some());
    assert!(settings.middle_click_action.is_some());
    assert!(settings.keyboard_zoom_anchor.is_some());
}

// C#: NewerBackupPath `(max + 1).ToString()` is unchecked int arithmetic: 2147483647 wraps to -2147483648.
#[test]
fn newer_backup_counter_overflow_wraps_like_csharp() {
    let r = Rig::new();
    load_and_save(&r, NEWER);
    let first = r.newer_backups().remove(0);
    let stamp = name_of(&first)
        .strip_prefix("config.json.newer-")
        .expect("prefix")
        .to_owned();
    r.fs.delete(&first).expect("delete");
    r.fs.add_file(format!("{}.newer-{stamp}-2147483647", r.cfg().display()), b"old");

    load_and_save(&r, NEWER);

    assert!(
        r.newer_backups()
            .iter()
            .any(|f| name_of(f).ends_with(&format!("{stamp}--2147483648"))),
        "{:?}",
        r.newer_backups()
    );
}

// Guard: a newer EXPLICIT file is not the default file, so a later save makes no newer-build backup.
#[test]
fn explicit_newer_file_then_save_makes_no_newer_backup() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3}"#);
    let explicit = PathBuf::from(r"C:\other\explicit.json");
    r.fs.add_file(&explicit, NEWER.as_bytes());
    let store = r.store();
    let loaded = store.load(Some(&explicit)).expect("load");
    assert_eq!(loaded.config_version, 99);

    store.save((*loaded).clone()).expect("save");

    assert!(r.newer_backups().is_empty());
}

// Guard: a failed salvage backup of an EXPLICIT file must not block the later default-file save.
#[test]
fn explicit_salvage_backup_failure_does_not_block_default_save() {
    let r = Rig::new();
    let explicit = PathBuf::from(r"C:\other\explicit.json");
    r.fs.add_file(&explicit, br#"{"ConfigVersion":3,"ClickZoomPercent":"abc"}"#);
    r.fs.fail(FsOp::Copy, &explicit, io::ErrorKind::Other);
    let store = r.store();
    let loaded = store.load(Some(&explicit)).expect("load");

    store.save((*loaded).clone()).expect("save");

    assert!(r.fs.file_exists(&r.cfg()));
    assert!(r.read().contains("\"ConfigVersion\": 3"));
}

// Guard: the import resets the keep-corrupt flag set by an earlier locked default load.
#[test]
fn import_after_locked_default_load_resets_keep_flag() {
    let r = Rig::new();
    r.put(r#"{"ConfigVersion":3}"#);
    r.fs.fail_locked(FsOp::ReadText, r.cfg());
    let store = r.store();
    store.load(None).expect("locked load");
    r.fs.clear_faults();
    r.fs.delete(&r.cfg()).expect("delete");
    r.fs.add_file(csharp_cfg(), br#"{"ConfigVersion":3,"ClickZoomPercent":250}"#);

    let loaded = store.load_first_run_with_import(&csharp_cfg()).expect("import");

    assert_eq!(loaded.click_zoom_percent, 250);
    assert!(r.read().contains("\"ClickZoomPercent\": 250"));
}
