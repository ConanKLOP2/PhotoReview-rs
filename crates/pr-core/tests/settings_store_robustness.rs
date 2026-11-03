//! Port of two `SettingsRobustnessTests` of `PhotoReview.Tests` (C# baseline 7eaee2a2) that need the real file system:
//! `ConcurrentSaveAndLoad_NeverTorn` and `Load_HugeFile_DoesNotThrow` (test-strengthening-plan tp-pr-core T5 and T6, P-373).
//! Pure file system (`PhysicalFileSystem` over a `TempRoot`), no sleeps: the reader thread stops when the writers have joined.
//! The rest of `SettingsRobustnessTests` is covered by the `settings-parse-text` golden (see `settings_store.rs`).

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pr_core::app_paths::AppPaths;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts, SettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_test_support::{FakeClock, RecordingLog, TempRoot};

fn texts() -> Arc<dyn SettingsTexts> {
    Arc::new(EnglishSettingsTexts)
}

/// A store on the real file system whose `config.json` lives in `root/PhotoReviewRs`.
fn store_in(root: &TempRoot) -> (SettingsStore, AppPaths) {
    let paths = AppPaths::new(root.path(), None, false);
    let store = SettingsStore::new(
        paths.clone(),
        Arc::new(PhysicalFileSystem),
        Arc::new(RecordingLog::new()),
        Arc::new(FakeClock::new()),
        texts(),
    );
    (store, paths)
}

fn config_dir(paths: &AppPaths) -> &Path {
    paths.config_file.parent().expect("config dir")
}

fn files_matching(dir: &Path, needle: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains(needle))
                .collect()
        })
        .unwrap_or_default()
}

/// What the rest of the app relies on after a load (the numeric part of the C# `AssertUsable`).
fn assert_usable(s: &AppSettings) {
    assert!(s.click_zoom_percent > 0);
    assert!(
        (1..=100).contains(&s.image_cache_ram_percent),
        "{}",
        s.image_cache_ram_percent
    );
    assert!(s.preload_worker_count >= 1);
    assert!(!s.ui_language.trim().is_empty());
}

// C#: SettingsRobustnessTests.ConcurrentSaveAndLoad_NeverTorn
#[test]
fn concurrent_save_and_load_never_torn() {
    let root = TempRoot::new("g17-store-concurrent");
    let (writer, paths) = store_in(&root);
    writer
        .save(AppSettings::new(&EnglishSettingsTexts))
        .expect("first save");
    let stop = AtomicBool::new(false);

    let (failures, reads) = std::thread::scope(|scope| {
        let savers: Vec<_> = (0..4)
            .map(|t| {
                let paths = paths.clone();
                scope.spawn(move || {
                    let store = SettingsStore::new(
                        paths,
                        Arc::new(PhysicalFileSystem),
                        Arc::new(RecordingLog::new()),
                        Arc::new(FakeClock::new()),
                        texts(),
                    );
                    for i in 0..60 {
                        let mut s = AppSettings::new(&EnglishSettingsTexts);
                        s.click_zoom_percent = 10 + t * 60 + i;
                        s.last_move_to_folder = Some("\u{e9}".repeat(200 + i as usize));
                        // A rename racing another rename may fail with an IO error; the file must still be whole.
                        let _ = store.save(s);
                    }
                })
            })
            .collect();
        let reader = scope.spawn(|| {
            let mut failures: Vec<String> = Vec::new();
            let mut reads = 0usize;
            while !stop.load(Ordering::SeqCst) {
                let bytes = match std::fs::read(&paths.config_file) {
                    Ok(b) => b,
                    Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied) => continue,
                    Err(e) => {
                        failures.push(format!("read: {e}"));
                        continue;
                    }
                };
                reads += 1;
                if let Err(e) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    failures.push(e.to_string());
                }
            }
            (failures, reads)
        });
        for s in savers {
            s.join().expect("saver thread");
        }
        stop.store(true, Ordering::SeqCst);
        reader.join().expect("reader thread")
    });

    assert!(
        failures.is_empty(),
        "torn config.json observed ({reads} reads): {failures:?}"
    );
    assert!(reads > 0, "the reader never saw the file, the test proved nothing");
    assert_eq!(files_matching(config_dir(&paths), ".tmp"), Vec::<String>::new());
    let (fresh, _) = store_in(&root);
    assert_usable(&fresh.load(None).expect("final load"));
}

// C#: SettingsRobustnessTests.Load_HugeFile_DoesNotThrow
#[test]
fn load_huge_file_does_not_throw_and_makes_no_backup() {
    let root = TempRoot::new("g17-store-huge");
    let (store, paths) = store_in(&root);
    std::fs::create_dir_all(config_dir(&paths)).expect("config dir");
    // 4 MiB is far beyond any real config yet keeps the run light (the C# test had 50 MB).
    let mut text = String::from("{\"ConfigVersion\":3,\"ClickZoomPercent\":250,\"Junk\":\"");
    text.push_str(&"x".repeat(4 * 1024 * 1024));
    text.push_str("\"}");
    std::fs::write(&paths.config_file, text).expect("write huge config");

    let loaded = store.load(None).expect("load");

    assert_usable(&loaded);
    assert_eq!(loaded.click_zoom_percent, 250);
    assert_eq!(files_matching(config_dir(&paths), ".corrupt-"), Vec::<String>::new());
}
