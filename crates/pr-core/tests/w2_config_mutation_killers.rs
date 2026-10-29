//! W2 config mutation regressions; C# SettingsStore/SettingsNormalizer at 7eaee2a2,
//! plus the Rust-only read-only first-run import contract R0002.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{AppendStream, FileStat, FileSystem, ReadSeek};
use pr_core::app_paths::AppPaths;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::normalizer::normalize;
use pr_core::settings::perf_options;
use pr_core::settings::store::SettingsStore;
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, RecordingLog, TempRoot};

#[test]
fn parse_text_salvaged_and_normalized_repairs_preserve_order_and_are_not_repeated() {
    // store.rs:467:16 and :467:45 delete !; :467:42 && -> || is equivalent:
    // salvaged names start in reset_settings and normalize emits distinct names.
    // C#: SettingsStore.ParseText uses salvaged.Concat(normalized.Except(salvaged)).
    let parsed = SettingsStore::parse_text(
        r#"{"ConfigVersion":4,"ExternalEditors":false,"DefaultExternalEditorId":"missing","ClickZoomPercent":99999}"#,
        &EnglishSettingsTexts,
    )
    .unwrap();
    assert_eq!(parsed.salvaged, ["ExternalEditors"]);
    assert!(parsed.salvage_cause.is_some());
    assert_eq!(parsed.reset_settings, ["ExternalEditors", "ClickZoomPercent"]);
    assert_eq!(parsed.repairs(), parsed.reset_settings);
    assert!(parsed.settings.external_editors.is_empty());
    assert!(parsed.settings.default_external_editor_id.is_empty());
    let mut repaired = parsed.settings;
    assert!(normalize(&mut repaired).is_empty());
}

#[test]
fn normalize_zero_reserve_is_valid_but_negative_reserve_is_repaired_once() {
    // normalizer.rs:83:38 < -> <=, ==, >; C#: SettingsNormalizer.Normalize.
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.memory_reserve_bytes = 0;
    assert!(normalize(&mut settings).is_empty());
    assert_eq!(settings.memory_reserve_bytes, 0);
    settings.memory_reserve_bytes = -1;
    assert_eq!(normalize(&mut settings), ["MemoryReserveBytes"]);
    assert_eq!(settings.memory_reserve_bytes, perf_options::MEMORY_RESERVE_BYTES);
    assert!(normalize(&mut settings).is_empty());
}

#[test]
fn parse_text_version_boundaries_keep_language_and_report_only_post_migration_repairs() {
    // store.rs:828:9 replace migrate_core with (); :828:36 < -> ==, >, <=;
    // :828:40 && -> ||; :828:43 delete !; :831:36 and :834:36 < -> ==, >, <=.
    // C#: SettingsStore.Migrate; legacy editor reconciliation is silent before v4.
    for version in [2, 3, 4, 99] {
        let parsed = SettingsStore::parse_text(
            &format!(r#"{{"ConfigVersion":{version},"DefaultExternalEditorId":"orphan"}}"#),
            &EnglishSettingsTexts,
        )
        .unwrap();
        assert_eq!(parsed.settings.config_version, version.max(4));
        assert_eq!(parsed.settings.ui_language, if version < 3 { "vi" } else { "auto" });
        assert!(parsed.settings.hidden_context_menu_items.is_some());
        assert!(parsed.settings.default_external_editor_id.is_empty());
        assert_eq!(
            parsed.reset_settings,
            if version < 4 { vec![] } else { vec!["ExternalEditors"] }
        );
    }
    let declared =
        SettingsStore::parse_text(r#"{"ConfigVersion":2,"UiLanguage":"en"}"#, &EnglishSettingsTexts).unwrap();
    assert_eq!(declared.settings.ui_language, "en");
    assert!(declared.reset_settings.is_empty());
}

#[test]
fn first_run_import_repairs_once_preserves_source_and_prefers_the_saved_rust_config() {
    // store.rs:544:9 whole-return replacement, :544:70 || -> &&, :544:73 delete !,
    // :558:22 match guard -> true/false. Rust R0002; C# repair persistence contract.
    let root = TempRoot::new("w2-config-import-killers");
    let paths = AppPaths::new(root.path(), None, false);
    let source = root.join("csharp-source/config.json");
    let original = r#"{"ConfigVersion":4,"UiLanguage":"en","ClickZoomPercent":99999}"#;
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_file(&source, original.as_bytes());
    let store = SettingsStore::new(
        paths.clone(),
        disk.clone(),
        Arc::new(RecordingLog::new()),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    )
    .with_read_retry_sleeper(Arc::new(|_| panic!("this fixture has no read failures")));
    let changed = Arc::new(AtomicUsize::new(0));
    let event_count = changed.clone();
    store.subscribe_changed(Box::new(move |_| {
        event_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));

    let imported = store.load_first_run_with_import(&source).unwrap();
    assert_eq!(imported.ui_language, "en");
    assert_eq!(store.last_load_repairs(), ["ClickZoomPercent"]);
    assert_eq!(changed.load(Ordering::SeqCst), 1);
    assert_eq!(disk.count(FsOp::WriteAtomic), 1);
    assert_eq!(disk.count(FsOp::Copy), 0);
    assert_eq!(disk.file_content(&source).unwrap(), original.as_bytes());
    assert!(disk.file_content(&paths.config_file).is_some());
    assert_eq!(disk.all_files().len(), 2);

    // Own config wins even when a different, usable source could be imported.
    let replacement = br#"{"ConfigVersion":4,"UiLanguage":"vi","ClickZoomPercent":50}"#;
    disk.add_file(&source, replacement);
    let reloaded = store.load_first_run_with_import(&source).unwrap();
    assert_eq!(reloaded.click_zoom_percent, imported.click_zoom_percent);
    assert_eq!(reloaded.ui_language, "en");
    assert!(store.last_load_repairs().is_empty());
    assert_eq!(changed.load(Ordering::SeqCst), 2);
    assert_eq!(disk.count(FsOp::WriteAtomic), 1);
    assert_eq!(disk.count(FsOp::Copy), 0);
    assert_eq!(disk.file_content(&source).unwrap(), replacement);
    assert_eq!(disk.all_files().len(), 2);
}

#[test]
fn first_run_json_null_uses_defaults_without_recording_an_import() {
    // store.rs:558:22 match guard p.is_json_null -> false; Rust R0002.
    let root = TempRoot::new("w2-config-null-killer");
    let paths = AppPaths::new(root.path(), None, false);
    let source = root.join("csharp-source/config.json");
    let disk = Arc::new(FakeFileSystem::new());
    let log = Arc::new(RecordingLog::new());
    disk.add_file(&source, b"null");
    let store = SettingsStore::new(
        paths,
        disk.clone(),
        log.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    );
    let loaded = store.load_first_run_with_import(&source).unwrap();
    assert_eq!(
        loaded.to_json().unwrap(),
        AppSettings::new(&EnglishSettingsTexts).to_json().unwrap()
    );
    assert!(store.last_load_repairs().is_empty());
    assert_eq!(disk.file_content(&source).unwrap(), b"null");
    assert_eq!(disk.count(FsOp::WriteAtomic), 1);
    assert_eq!(disk.count(FsOp::Copy), 0);
    assert_eq!(disk.all_files().len(), 2);
    assert!(
        !log.entries()
            .iter()
            .any(|(_, message)| message.starts_with("Imported "))
    );
}

#[test]
fn corrupt_backup_collision_limit_preserves_all_existing_files_and_the_unbacked_source() {
    // store.rs:772:9 whole-return replacement; :776:17 < -> ==, >, <=;
    // :776:23 && -> ||; :778:15 += -> -=, *=.
    // C#: SettingsStore.UniqueBackupPath stops below 100; finite fake entries
    // exercise exhaustion without the always-true exists hook that can hang a mutant.
    for occupied in [0, 1, 2, 98, 99] {
        let root = TempRoot::new("w2-config-backup-limit");
        let paths = AppPaths::new(root.path(), None, false);
        let disk = Arc::new(FakeFileSystem::new());
        let stem = format!("{}.corrupt-20260101000000", paths.config_file.display());
        disk.add_file(&paths.config_file, b"broken config");
        let backup_path = |index| {
            if index == 1 {
                stem.clone()
            } else {
                format!("{stem}-{index}")
            }
        };
        for index in 1..=occupied {
            disk.add_file(backup_path(index), format!("existing {index}").as_bytes());
        }
        let bounded_disk = Arc::new(BackupProbeGuard {
            disk: disk.clone(),
            visited: Mutex::new(HashSet::new()),
        });
        let store = SettingsStore::new(
            paths.clone(),
            bounded_disk,
            Arc::new(RecordingLog::new()),
            Arc::new(FakeClock::new()),
            Arc::new(EnglishSettingsTexts),
        )
        .with_read_retry_sleeper(Arc::new(|_| panic!("this fixture has no read failures")));
        let loaded = store.load(None).unwrap();
        assert_eq!(
            loaded.to_json().unwrap(),
            AppSettings::new(&EnglishSettingsTexts).to_json().unwrap()
        );
        for index in 1..=occupied {
            assert_eq!(
                disk.file_content(backup_path(index)).unwrap(),
                format!("existing {index}").as_bytes()
            );
        }
        if occupied == 99 {
            assert_eq!(disk.file_content(&paths.config_file).unwrap(), b"broken config");
            assert_eq!(disk.count(FsOp::WriteAtomic), 0);
            store.save((*loaded).clone()).unwrap();
            assert_eq!(disk.count(FsOp::WriteAtomic), 0);
            assert_eq!(disk.all_files().len(), 100);
        } else {
            assert_eq!(disk.file_content(backup_path(occupied + 1)).unwrap(), b"broken config");
            assert_eq!(disk.count(FsOp::WriteAtomic), 1);
            assert_eq!(disk.all_files().len(), occupied + 2);
        }
    }
}

/// A backup search must advance, reading each candidate once. A repeated probe
/// fails synchronously, so the *= 1 mutant fails without a sleep or timeout.
#[derive(Debug)]
struct BackupProbeGuard {
    disk: Arc<FakeFileSystem>,
    visited: Mutex<HashSet<PathBuf>>,
}

macro_rules! forward_disk {
    ($(fn $name:ident($($arg:ident: $arg_type:ty),*) -> $result:ty;)*) => {
        $(fn $name(&self, $($arg: $arg_type),*) -> $result {
            self.disk.$name($($arg),*)
        })*
    };
}

impl FileSystem for BackupProbeGuard {
    fn file_exists(&self, path: &Path) -> bool {
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("config.json.corrupt-"))
        {
            assert!(
                self.visited.lock().unwrap().insert(path.to_owned()),
                "backup search revisited {path:?}"
            );
        }
        self.disk.file_exists(path)
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
        fn read_all_text(path: &Path) -> io::Result<String>;
        fn enumerate_files(directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>>;
        fn enumerate_directories(directory: &Path) -> io::Result<Vec<PathBuf>>;
        fn create_directory(path: &Path) -> io::Result<()>;
    }
}
