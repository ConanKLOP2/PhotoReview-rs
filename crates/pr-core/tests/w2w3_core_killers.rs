//! cl-core lane (2026-10-30) mutant killers for `pr_core::settings::store` (PARITY S-64, S-65, INV-11, C-28), found by
//! `cargo mutants` on the guards of `load` / `apply_parsed` / `read_all_text_with_retry` / newer-backup naming and on the
//! small public accessors. C# oracle: `SettingsStore` at 7eaee2a2. Evidence: `docs/reviews/CORE-LANE-2026-10-30.md`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{AppendStream, FileStat, FileSystem, ReadSeek};
use pr_core::app_paths::AppPaths;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::store::{SettingsSaveError, SettingsStore};
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, RecordingLog, TempRoot};

macro_rules! forward_disk {
    ($(fn $name:ident($($arg:ident: $arg_type:ty),*) -> $result:ty;)*) => {
        $(fn $name(&self, $($arg: $arg_type),*) -> $result {
            self.disk.$name($($arg),*)
        })*
    };
}

/// Every read of `config.json` fails with a sharing violation; all other calls forward.
#[derive(Debug)]
struct AlwaysLockedConfig {
    disk: Arc<FakeFileSystem>,
}

impl FileSystem for AlwaysLockedConfig {
    fn file_exists(&self, path: &Path) -> bool {
        self.disk.file_exists(path)
    }

    fn read_all_text(&self, _path: &Path) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "locked by another process",
        ))
    }

    forward_disk! {
        fn directory_exists(path: &Path) -> bool;
        fn get_file_stat(path: &Path) -> Option<FileStat>;
        fn move_file(source: &Path, destination: &Path) -> io::Result<()>;
        fn copy_file(source: &Path, destination: &Path) -> io::Result<()>;
        fn delete(path: &Path) -> io::Result<()>;
        fn open_read_shared(path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>>;
        fn open_append(path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>>;
        fn write_all_text_atomic(path: &Path, text: &str, durable: bool) -> io::Result<()>;
        fn enumerate_files(directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>>;
        fn enumerate_directories(directory: &Path) -> io::Result<Vec<PathBuf>>;
        fn create_directory(path: &Path) -> io::Result<()>;
    }
}

#[test]
fn locked_config_read_retries_exactly_four_times_then_gives_in_memory_defaults() {
    // store.rs:616:29 `attempt += 1` -> `*= 1` (the counter never advances: an endless
    // retry loop that only a timeout caught) and `-= 1`. The sleeper panics on a 5th
    // sleep so the mutant fails synchronously. C#: SettingsStore.ReadAllTextWithRetry
    // (delays 50/100/200/400 ms), INV-11 (a locked config is never overwritten).
    let root = TempRoot::new("w2-config-locked-retry");
    let paths = AppPaths::new(root.path(), None, false);
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_file(&paths.config_file, b"{\"ConfigVersion\":4}");
    let sleeps = Arc::new(Mutex::new(Vec::new()));
    let seen = sleeps.clone();
    let store = SettingsStore::new(
        paths.clone(),
        Arc::new(AlwaysLockedConfig { disk: disk.clone() }),
        Arc::new(RecordingLog::new()),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    )
    .with_read_retry_sleeper(Arc::new(move |d| {
        let mut all = seen.lock().unwrap();
        assert!(all.len() < 4, "retry loop did not stop after four sleeps");
        all.push(d.as_millis());
    }));
    let loaded = store.load(None).unwrap();
    assert_eq!(*sleeps.lock().unwrap(), [50, 100, 200, 400]);
    assert_eq!(
        loaded.to_json().unwrap(),
        AppSettings::new(&EnglishSettingsTexts).to_json().unwrap()
    );
    assert_eq!(disk.file_content(&paths.config_file).unwrap(), b"{\"ConfigVersion\":4}");
    assert_eq!(disk.count(FsOp::WriteAtomic), 0);
}

#[test]
fn changed_handler_count_follows_subscribe_and_unsubscribe() {
    // store.rs:255:9 `changed_handler_count` -> 0 / 1. C#: `Changed.GetInvocationList().Length`.
    let root = TempRoot::new("w2-config-handler-count");
    let store = SettingsStore::new(
        AppPaths::new(root.path(), None, false),
        Arc::new(FakeFileSystem::new()),
        Arc::new(RecordingLog::new()),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    );
    assert_eq!(store.changed_handler_count(), 0);
    let first = store.subscribe_changed(Box::new(|_| Ok(())));
    assert_eq!(store.changed_handler_count(), 1);
    let second = store.subscribe_changed(Box::new(|_| Ok(())));
    assert_eq!(store.changed_handler_count(), 2);
    assert!(store.unsubscribe_changed(first));
    assert_eq!(store.changed_handler_count(), 1);
    assert!(store.unsubscribe_changed(second));
    assert_eq!(store.changed_handler_count(), 0);
}

#[test]
fn validate_shortcuts_default_settings_have_no_conflict() {
    // store.rs:268:9 `validate_shortcuts` -> Some("") / Some("xyzzy") (the `None` mutant was
    // already caught). C#: SettingsStore.ValidateShortcuts => SettingsValidator.ValidateShortcuts.
    let root = TempRoot::new("w2-config-validate-shortcuts");
    let store = SettingsStore::new(
        AppPaths::new(root.path(), None, false),
        Arc::new(FakeFileSystem::new()),
        Arc::new(RecordingLog::new()),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    );
    let defaults = AppSettings::new(&EnglishSettingsTexts);
    assert_eq!(store.validate_shortcuts(&defaults), None);
}

#[test]
fn migrate_public_keeps_a_declared_language_and_sets_vi_only_when_not_chosen_below_version_3() {
    // store.rs:497:13 `delete !` in `migrate` (`declared = !is_blank(..) && ..`): a declared "en"
    // at version 2 must stay "en". C#: SettingsStore.Migrate(settings), version < 3 rule.
    for (language, expected) in [("en", "en"), ("fr", "fr"), ("", "vi"), ("auto", "vi"), ("AUTO", "vi")] {
        let mut settings = AppSettings::new(&EnglishSettingsTexts);
        settings.config_version = 2;
        settings.ui_language = language.to_owned();
        SettingsStore::migrate(&mut settings);
        assert_eq!(settings.ui_language, expected, "language {language:?}");
        assert_eq!(settings.config_version, 4, "language {language:?}");
    }
}

#[test]
fn save_error_display_and_store_debug_are_not_empty() {
    // store.rs:65:9 `<SettingsSaveError as Display>::fmt` and :114:9 `<SettingsStore as Debug>::fmt`
    // replaced by `Ok(Default::default())` (they would print nothing).
    let root = TempRoot::new("w2-config-fmt");
    let error = SettingsSaveError::Io(io::Error::other("disk is full"));
    assert_eq!(error.to_string(), "disk is full");
    let store = SettingsStore::new(
        AppPaths::new(root.path(), None, false),
        Arc::new(FakeFileSystem::new()),
        Arc::new(RecordingLog::new()),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    );
    assert!(format!("{store:?}").starts_with("SettingsStore"));
}
