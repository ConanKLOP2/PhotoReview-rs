//! Port of the disk-cache tests of `PhotoReview.Imaging.Tests` for `PreviewImageService` (C# baseline d92d6aae; PARITY
//! I-08, D-06, C-05, INV-2), plus the golden check against `tests/golden/preview-disk-cache-path.json` (36 cases).
//!
//! Port of `PhotoReview.Imaging/Caching/PreviewImageService.cs` (persist queue and workers, startup cleanup, Clear
//! Disk, disk-cache path): `contracts/W4-14c.md`.
//!
//! Layout:
//! - the top level holds the tests that need only W4-14a + W4-14c (keys, constants, startup cleanup, Clear Disk, the
//!   probe, the worker lifecycle, the golden): green now;
//! - `decode_path` holds the C# twins that go through `GetPreviewAsync` (`get_preview_path` / `get_preview`, W4-14b):
//!   they are RED (they panic in the W4-14b `todo!()` stubs) until W4-14b merges (MULTI-AGENT-PLAN lesson: blocked
//!   units leave red tests, no `#[ignore]` that would hide them afterwards). Their persist-side halves also run green
//!   now as white-box tests in `src/service/disk.rs` (`queue_persist` hand-off).
//!
//! The C# tests decode real PNG/JPEG files through WPF/WIC; here the decoder is fake and transparency comes from the
//! pixel buffer. C# polling helpers (`WaitForCacheFilesAsync`) become `shutdown_persist_workers_timeout` (every queued
//! write has finished when it returns) or `wait_for_prune`.

mod support {
    pub mod preview_service;
}

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{ImageCacheKey, ImageSourceKind, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::service::disk::{
    DISK_CACHE_EXTENSION, DISK_CACHE_IDENTITY_PREFIX, DISK_CACHE_SEARCH_PATTERN, LEGACY_CLEANUP_MAX_FILES,
    LEGACY_PATTERNS, PERSIST_QUEUE_CAPACITY, PERSIST_WORKER_COUNT, disk_cache_file_name, disk_cache_identity,
    disk_cache_path,
};
use pr_imaging::service::{
    DecodeThreadKind, DecodeThreadSpawner, PreviewImageService, PreviewServiceBuildError, PreviewServiceConfig,
    StdThreadSpawner,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, LogLevel, RecordingLog, TempRoot};
use serde_json::Value;

use support::preview_service::{FakeDecoder, NeverDecoder, TIMEOUT, config, lock};

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// A service over `dir` with the disk cache ON (C# `disableDiskCacheOverride: false` or a positive capacity).
fn disk_service(dir: &Path, log: Option<Arc<RecordingLog>>) -> PreviewImageService {
    let mut config = config(dir, Arc::new(NeverDecoder));
    config.disable_disk_cache_override = Some(false);
    if let Some(log) = log {
        config.log = Some(log);
    }
    PreviewImageService::new(config).expect("the service builds")
}

/// Shuts the workers down and waits for the prune (C# `DisposeAsync` of every test class).
fn retire(service: &PreviewImageService) {
    assert!(
        service.shutdown_persist_workers_timeout(TIMEOUT),
        "the persist workers exit"
    );
    assert!(service.wait_for_prune(TIMEOUT), "the prune finishes");
}

/// A key with length and write time set (no disk access).
fn entry_key(path: &str, original: bool, target: DecodeBox, options: KeyOptions) -> ImageCacheKey {
    let mut entry = CatalogEntry::new(path).expect("path is not blank");
    entry.length = Some(1234);
    entry.last_write_utc = DateTimeUtc::from_ticks(638_000_000_000_000_000);
    ImageCacheKey::create_for_entry(&FakeFileSystem::new(), &entry, original, target, options).expect("key")
}

/// C# `Temp(directory, name, age)`: `{name}.{guid}.tmp` with its write time `age` before now.
fn temp_file(directory: &Path, name: &str, age: Duration, serial: usize) -> PathBuf {
    let path = directory.join(format!("{name}.0123456789abcdef0123456789abcd{serial:02}.tmp"));
    std::fs::write(&path, [1u8, 2, 3]).expect("write temp file");
    set_write_time(&path, SystemTime::now() - age);
    path
}

fn set_write_time(path: &Path, time: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(time))
        .expect("set the write time");
}

fn write_file(path: &Path, bytes: &[u8]) -> PathBuf {
    std::fs::write(path, bytes).expect("write file");
    path.to_path_buf()
}

/// `Directory.GetFiles(dir, "*.pv4")` (a missing directory = none).
fn entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pv4")))
        .collect();
    files.sort();
    files
}

fn errors(log: &RecordingLog) -> Vec<String> {
    log.entries()
        .into_iter()
        .filter(|(level, _)| *level == LogLevel::Error)
        .map(|(_, text)| text)
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------------------------------

// C#: PreviewImagePersistQueueBoundTests.PersistQueueBound_IsPinnedAt18
#[test]
fn persist_queue_bound_is_pinned_at_18() {
    assert_eq!(PERSIST_QUEUE_CAPACITY, 16);
    assert_eq!(PERSIST_WORKER_COUNT, 2);
    assert_eq!(PERSIST_QUEUE_CAPACITY + PERSIST_WORKER_COUNT, 18);
}

// Rust-only: the persisted names (D-06) and the cleanup bounds are the C# literals.
#[test]
fn disk_cache_constants_are_the_csharp_literals() {
    assert_eq!(DISK_CACHE_IDENTITY_PREFIX, "preview-v5-box");
    assert_eq!(DISK_CACHE_EXTENSION, ".pv4");
    assert_eq!(DISK_CACHE_SEARCH_PATTERN, "*.pv4");
    assert_eq!(LEGACY_CLEANUP_MAX_FILES, 5000);
    assert_eq!(LEGACY_PATTERNS, ["*.png", "*.png.meta"]);
}

// ---------------------------------------------------------------------------------------------------------------------
// Identity, file name, path (D-06) and the golden
// ---------------------------------------------------------------------------------------------------------------------

// Rust-only: the exact identity text of a standard and of a RAW-preview key (contract example strings).
#[test]
fn disk_cache_identity_text_examples() {
    let standard = entry_key(
        "C:\\Pics\\A.jpg",
        false,
        DecodeBox::new(2400, 1350),
        KeyOptions::default(),
    );
    assert_eq!(
        disk_cache_identity(&standard),
        "preview-v5-box|C:\\PICS\\A.JPG|1234|638000000000000000|False|2400x1350|True|Wpf"
    );
    let raw_options = KeyOptions {
        source_kind: ImageSourceKind::RAW_PREVIEW,
        ..KeyOptions::default()
    };
    let raw = entry_key("C:\\Pics\\A.jpg", false, DecodeBox::new(2400, 1350), raw_options);
    assert_eq!(
        disk_cache_identity(&raw),
        "preview-v5-box|C:\\PICS\\A.JPG|1234|638000000000000000|False|2400x1350|True|Wpf|kind1"
    );
    // The file name is the hash of exactly that text: the golden pins it.
    assert_eq!(
        disk_cache_file_name(&standard),
        "CC7D79274BE0DCCFDBDF0EF50E7FAF9D7B9C25F1E90068F203E3CAC2A1E0C524.pv4"
    );
    assert_ne!(disk_cache_file_name(&standard), disk_cache_file_name(&raw));
}

fn parse_backend(name: &str) -> DecoderBackend {
    match name {
        "Wpf" => DecoderBackend::Wpf,
        "WicDirect" => DecoderBackend::WicDirect,
        "TurboJpeg" => DecoderBackend::TurboJpeg,
        "LibRaw" => DecoderBackend::LibRaw,
        other => panic!("unknown decoder backend '{other}' in the golden file"),
    }
}

fn i32_of(value: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(value, key)).unwrap_or_else(|_| panic!("'{key}' does not fit an i32: {value}"))
}

// C#: golden `preview-disk-cache-path` (oracle W4-14c: the private `GetDiskCachePath` through reflection; 36 cases)
#[test]
fn golden_preview_disk_cache_path_matches_csharp() {
    let golden = golden::load("preview-disk-cache-path");
    assert_eq!(golden.cases.len(), 36, "cases in the golden file");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = &case["input"];
        let spec = &input["key"];
        let mut entry = CatalogEntry::new(str_of(spec, "path")).expect("path is not blank");
        entry.length = Some(i64_of(spec, "length"));
        entry.last_write_utc =
            Some(DateTimeUtc::from_ticks(i64_of(spec, "ticks")).unwrap_or_else(|| panic!("{name}: ticks in range")));
        let options = KeyOptions {
            orientation_applied: spec.get("orient").and_then(Value::as_bool).unwrap_or(true),
            backend: parse_backend(spec.get("backend").and_then(Value::as_str).unwrap_or("Wpf")),
            source_kind: u8::try_from(spec.get("source_kind").and_then(Value::as_i64).unwrap_or(0)).expect("u8 kind"),
        };
        let key = ImageCacheKey::create_for_entry(
            &FakeFileSystem::new(),
            &entry,
            bool_of(spec, "original"),
            DecodeBox::new(i32_of(spec, "w"), i32_of(spec, "h")),
            options,
        )
        .unwrap_or_else(|e| panic!("{name}: key builds: {e:?}"));

        let expected_key = &case["key"];
        assert_eq!(key.path(), str_of(expected_key, "path"), "{name}: key path");
        assert_eq!(key.length(), i64_of(expected_key, "length"), "{name}: length");
        assert_eq!(
            key.last_write_utc_ticks(),
            i64_of(expected_key, "ticks"),
            "{name}: ticks"
        );
        assert_eq!(
            key.is_original(),
            bool_of(expected_key, "is_original"),
            "{name}: original"
        );
        assert_eq!(key.target_width(), i32_of(expected_key, "width"), "{name}: width");
        assert_eq!(key.target_height(), i32_of(expected_key, "height"), "{name}: height");
        assert_eq!(
            key.orientation_applied(),
            bool_of(expected_key, "orientation_applied"),
            "{name}: orient"
        );
        assert_eq!(
            key.backend(),
            parse_backend(str_of(expected_key, "backend")),
            "{name}: backend"
        );
        assert_eq!(
            i64::from(key.source_kind()),
            i64_of(expected_key, "source_kind"),
            "{name}: kind"
        );

        assert_eq!(
            disk_cache_file_name(&key),
            str_of(case, "file_name"),
            "{name}: file name"
        );
        let path = disk_cache_path(Path::new(str_of(input, "directory")), &key);
        assert_eq!(path.to_str(), Some(str_of(case, "path")), "{name}: combined path");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Startup cleanup and Clear Disk (no decode needed)
// ---------------------------------------------------------------------------------------------------------------------

// C#: CacheTempFileAndDiskProbeTests.PreviewStartup_DeletesStaleTempFiles_KeepsFreshAndEntries
#[test]
fn preview_startup_deletes_stale_temp_files_keeps_fresh_and_entries() {
    let root = TempRoot::new("w414c-startup-temp");
    let dir = root.join("preview");
    std::fs::create_dir_all(&dir).expect("dir");
    let stale = temp_file(&dir, "a.pv4", Duration::from_secs(2 * 3600), 1);
    let fresh = temp_file(&dir, "b.pv4", Duration::ZERO, 2);
    let entry = write_file(&dir.join("c.pv4"), &[1]);
    set_write_time(&entry, SystemTime::now() - Duration::from_secs(2 * 3600));
    // The sweep compares with the injected clock (C# `DateTime.UtcNow`).
    let mut config = config(&dir, Arc::new(NeverDecoder));
    config.disable_disk_cache_override = Some(false);
    config.clock = Some(Arc::new(FakeClock::at(DateTimeUtc::now())));
    let service = PreviewImageService::new(config).expect("service");
    assert!(service.wait_startup_cleanup(TIMEOUT));
    assert!(!stale.exists());
    assert!(fresh.exists(), "may belong to a write in flight");
    assert!(entry.exists());
    retire(&service);
}

// Rust-only: the startup sweep reads the injected clock, not the wall clock (a clock 1 h behind keeps the 2 h old one
// only when it is less than 10 min older than that clock).
#[test]
fn startup_temp_sweep_uses_the_injected_clock() {
    let root = TempRoot::new("w414c-startup-clock");
    let dir = root.join("preview");
    std::fs::create_dir_all(&dir).expect("dir");
    let old = temp_file(&dir, "a.pv4", Duration::from_secs(2 * 3600), 1);
    let behind = DateTimeUtc::now()
        .checked_add_ticks(-2 * 3600 * 10_000_000)
        .expect("in range");
    let mut config = config(&dir, Arc::new(NeverDecoder));
    config.disable_disk_cache_override = Some(false);
    config.clock = Some(Arc::new(FakeClock::at(behind)));
    let service = PreviewImageService::new(config).expect("service");
    assert!(service.wait_startup_cleanup(TIMEOUT));
    assert!(old.exists(), "not stale for a clock two hours behind");
    retire(&service);
}

// C#: CacheTempFileAndDiskProbeTests.ClearDisk_DeletesTempFiles
#[test]
fn clear_disk_deletes_temp_files() {
    let root = TempRoot::new("w414c-clear-temp");
    let dir = root.join("clear");
    std::fs::create_dir_all(&dir).expect("dir");
    let service = disk_service(&dir, None);
    assert!(service.wait_startup_cleanup(TIMEOUT));
    let fresh = temp_file(&dir, "a.pv4", Duration::ZERO, 1);
    let thumb_fresh = temp_file(&dir, "b.png", Duration::ZERO, 2);
    service.clear_disk();
    assert!(!fresh.exists());
    assert!(!thumb_fresh.exists());
    retire(&service);
}

// C#: PreviewImageServiceMutationTests.StartupCleanup_RemovesLegacyFilesOnly
#[test]
fn startup_cleanup_removes_legacy_files_only() {
    let root = TempRoot::new("w414c-legacy");
    let dir = root.join("legacy");
    std::fs::create_dir_all(&dir).expect("dir");
    let png = write_file(&dir.join("a.png"), &[1]);
    let meta = write_file(&dir.join("b.png.meta"), &[1]);
    let pv4 = write_file(&dir.join("c.pv4"), &[1]);
    let other = write_file(&dir.join("d.txt"), &[1]);
    let sub = dir.join("e.png");
    std::fs::create_dir_all(&sub).expect("a directory named like a legacy file");
    let service = disk_service(&dir, None);
    assert!(service.wait_startup_cleanup(TIMEOUT));
    assert!(!png.exists());
    assert!(!meta.exists());
    assert!(pv4.exists());
    assert!(other.exists());
    assert!(sub.is_dir(), "directories are never touched");
    retire(&service);
}

// Rust-only: the legacy patterns follow `Directory.EnumerateFiles` (`*.png` does not match `x.png.meta` nor `x.pv4`;
// the match is case-insensitive) and the startup pass logs nothing on success.
#[test]
fn legacy_patterns_follow_enumerate_files_semantics() {
    let root = TempRoot::new("w414c-legacy-patterns");
    let dir = root.join("legacy");
    std::fs::create_dir_all(&dir).expect("dir");
    let upper = write_file(&dir.join("A.PNG"), &[1]);
    let png_tmp = write_file(&dir.join("x.png.tmp"), &[1]);
    let pngx = write_file(&dir.join("x.pngx"), &[1]);
    let meta_upper = write_file(&dir.join("y.PNG.META"), &[1]);
    let log = Arc::new(RecordingLog::new());
    let service = disk_service(&dir, Some(log.clone()));
    assert!(service.wait_startup_cleanup(TIMEOUT));
    assert!(!upper.exists());
    assert!(!meta_upper.exists());
    assert!(png_tmp.exists(), "a fresh temp file is kept");
    assert!(pngx.exists());
    assert!(errors(&log).is_empty(), "{:?}", errors(&log));
    retire(&service);
}

// C#: PreviewImageServiceMutationTests.ClearDisk_RemovesEntriesAndLegacyFiles
#[test]
fn clear_disk_removes_entries_and_legacy_files() {
    let root = TempRoot::new("w414c-clear-legacy");
    let dir = root.join("clear");
    std::fs::create_dir_all(&dir).expect("dir");
    let service = disk_service(&dir, None);
    assert!(service.wait_startup_cleanup(TIMEOUT));
    let png = write_file(&dir.join("a.png"), &[1]);
    let meta = write_file(&dir.join("b.png.meta"), &[1]);
    let pv4 = write_file(&dir.join("c.pv4"), &[1]);
    let other = write_file(&dir.join("d.txt"), &[1]);
    service.clear_disk();
    assert!(!png.exists());
    assert!(!meta.exists());
    assert!(!pv4.exists());
    assert!(other.exists());
    retire(&service);
}

// Rust-only: Clear Disk on a missing directory creates nothing and logs nothing; with the disk cache disabled it still
// clears (C# does not look at the switch there).
#[test]
fn clear_disk_missing_directory_and_disabled_cache() {
    let root = TempRoot::new("w414c-clear-missing");
    let missing = root.join("never-created");
    let log = Arc::new(RecordingLog::new());
    let service = disk_service(&missing, Some(log.clone()));
    service.clear_disk();
    assert!(!missing.exists());
    assert!(errors(&log).is_empty(), "{:?}", errors(&log));
    retire(&service);

    let dir = root.join("disabled");
    std::fs::create_dir_all(&dir).expect("dir");
    let pv4 = write_file(&dir.join("c.pv4"), &[1]);
    let png = write_file(&dir.join("a.png"), &[1]);
    let mut config = config(&dir, Arc::new(NeverDecoder));
    config.disable_disk_cache_override = Some(true);
    let disabled = PreviewImageService::new(config).expect("service");
    assert!(png.exists(), "no startup cleanup while disabled");
    disabled.clear_disk();
    assert!(!pv4.exists());
    assert!(!png.exists());
    retire(&disabled);
}

/// Denies `ListDirectory` on a directory for the current user with `icacls` and restores it on drop (C# `DenyListing`).
struct DenyListing {
    directory: PathBuf,
    sid: String,
}

impl DenyListing {
    fn new(directory: &Path) -> Self {
        let output = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("whoami runs");
        let text = String::from_utf8_lossy(&output.stdout);
        let sid = text
            .trim()
            .rsplit(',')
            .next()
            .map(|s| s.trim_matches('"').to_owned())
            .filter(|s| s.starts_with("S-"))
            .expect("the current user's SID");
        let status = Command::new("icacls")
            .arg(directory)
            .arg("/deny")
            .arg(format!("*{sid}:(RD)"))
            .output()
            .expect("icacls runs");
        assert!(status.status.success(), "icacls /deny: {status:?}");
        Self {
            directory: directory.to_path_buf(),
            sid,
        }
    }
}

impl Drop for DenyListing {
    fn drop(&mut self) {
        let _ = Command::new("icacls")
            .arg(&self.directory)
            .arg("/remove:d")
            .arg(format!("*{}", self.sid))
            .output();
    }
}

// C#: PreviewImageServiceMutationTests.StartupCleanup_UnlistableDirectory_LogsAndCompletes
#[test]
#[ignore = "native: changes a directory ACL"]
fn startup_cleanup_unlistable_directory_logs_and_completes() {
    let root = TempRoot::new("w414c-denied-startup");
    let dir = root.join("denied-startup");
    std::fs::create_dir_all(&dir).expect("dir");
    let log = Arc::new(RecordingLog::new());
    let service;
    {
        let _deny = DenyListing::new(&dir);
        service = disk_service(&dir, Some(log.clone()));
        assert!(service.wait_startup_cleanup(TIMEOUT), "must complete");
    }
    assert!(
        errors(&log)
            .iter()
            .any(|e| e.contains("Legacy preview cache cleanup failed")),
        "{:?}",
        errors(&log)
    );
    retire(&service);
}

// C#: PreviewImageServiceMutationTests.ClearDisk_UnlistableDirectory_LogsAndDoesNotThrow
#[test]
#[ignore = "native: changes a directory ACL"]
fn clear_disk_unlistable_directory_logs_and_does_not_throw() {
    let root = TempRoot::new("w414c-denied-clear");
    let dir = root.join("denied-clear");
    std::fs::create_dir_all(&dir).expect("dir");
    let log = Arc::new(RecordingLog::new());
    let service = disk_service(&dir, Some(log.clone()));
    assert!(service.wait_startup_cleanup(TIMEOUT));
    {
        let _deny = DenyListing::new(&dir);
        service.clear_disk(); // must not panic
    }
    let errors = errors(&log);
    assert!(
        errors.iter().any(|e| e.contains("Preview disk cache clear failed")),
        "{errors:?}"
    );
    assert!(
        !errors.iter().any(|e| e.contains("Legacy preview cache cleanup failed")),
        "the legacy sweep is skipped after a failed clear: {errors:?}"
    );
    retire(&service);
}

// ---------------------------------------------------------------------------------------------------------------------
// Probe and on/off switch (no decode needed: the entry file is placed at the key's path)
// ---------------------------------------------------------------------------------------------------------------------

// C#: PreviewImageServiceMutationTests.Ctor_NonPositiveDiskCapacity_DisablesDiskCache (theory 0, -1). The C# seeds the
// entry through a decode; `HasDiskCachedPreview` only probes the key's path, so the entry is placed there directly
// (the decode-path seeding is pinned by `decode_path::ctor_non_positive_disk_capacity_disables_disk_cache`).
#[test]
fn ctor_non_positive_disk_capacity_disables_disk_cache() {
    for capacity in [0i64, -1] {
        let root = TempRoot::new("w414c-capacity");
        let dir = root.join("disk");
        std::fs::create_dir_all(&dir).expect("dir");
        let source = write_file(&root.join("a.jpg"), &[1, 2, 3, 4]);
        let mut seeding_config = config(&dir, Arc::new(NeverDecoder));
        seeding_config.disable_disk_cache_override = None;
        seeding_config.diag = Some(Arc::new(Default::default()));
        let seeding = PreviewImageService::new(seeding_config).expect("service");
        let key = seeding
            .get_current_cache_key(source.to_str().expect("utf-8"))
            .expect("key");
        write_file(&disk_cache_path(&dir, &key), &[1]);
        assert!(seeding.has_disk_cached_preview(&key));
        let mut disabled_config = config(&dir, Arc::new(NeverDecoder));
        disabled_config.disable_disk_cache_override = None;
        disabled_config.diag = Some(Arc::new(Default::default()));
        disabled_config.disk_cache_capacity_bytes = capacity;
        let disabled = PreviewImageService::new(disabled_config).expect("service");
        assert!(!disabled.has_disk_cached_preview(&key), "capacity {capacity}");
        // A disabled cache schedules no startup cleanup.
        assert!(disabled.wait_startup_cleanup(Duration::ZERO));
        retire(&seeding);
        retire(&disabled);
    }
}

// Rust-only: the probe is a regular-file check; a directory at the entry path is no entry.
#[test]
fn has_disk_cached_preview_is_a_file_check() {
    let root = TempRoot::new("w414c-probe-dir");
    let dir = root.join("disk");
    let service = disk_service(&dir, None);
    let key = entry_key("C:\\Pics\\A.jpg", false, DecodeBox::new(32, 32), KeyOptions::default());
    assert!(!service.has_disk_cached_preview(&key), "missing directory");
    std::fs::create_dir_all(disk_cache_path(&dir, &key)).expect("a directory at the entry path");
    assert!(!service.has_disk_cached_preview(&key));
    retire(&service);
}

// ---------------------------------------------------------------------------------------------------------------------
// Worker lifecycle (no decode needed)
// ---------------------------------------------------------------------------------------------------------------------

// Rust-only: shutdown twice is harmless and returns at once the second time.
#[test]
fn shutdown_twice_is_harmless() {
    let root = TempRoot::new("w414c-shutdown-twice");
    let service = disk_service(&root.join("disk"), None);
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(service.shutdown_persist_workers_timeout(Duration::ZERO));
    service.shutdown_persist_workers();
    service.shutdown_persist_workers();
    assert!(service.wait_for_prune(TIMEOUT));
}

/// Records which thread kinds finished their job (delegates the spawning to [`StdThreadSpawner`]); can refuse one kind.
#[derive(Default)]
struct TrackingSpawner {
    started: Mutex<Vec<DecodeThreadKind>>,
    finished: Mutex<Vec<DecodeThreadKind>>,
    changed: Condvar,
    /// Refuse the n-th (1-based) spawn of this kind.
    refuse: Option<(DecodeThreadKind, usize)>,
}

impl TrackingSpawner {
    fn finished(&self, kind: DecodeThreadKind) -> usize {
        lock(&self.finished).iter().filter(|k| **k == kind).count()
    }

    /// Waits until `count` jobs of `kind` finished; `false` after [`TIMEOUT`].
    fn wait_finished(&self, kind: DecodeThreadKind, count: usize) -> bool {
        let guard = lock(&self.finished);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |done| {
                done.iter().filter(|k| **k == kind).count() < count
            })
            .unwrap_or_else(PoisonError::into_inner);
        guard.iter().filter(|k| **k == kind).count() >= count
    }
}

/// The `Arc` handed to the service (the trait takes `&self`; finishing needs the shared state).
struct SharedSpawner(Arc<TrackingSpawner>);

impl DecodeThreadSpawner for SharedSpawner {
    fn spawn(&self, kind: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()> {
        let tracker = self.0.clone();
        let nth = {
            let mut started = lock(&tracker.started);
            started.push(kind);
            started.iter().filter(|k| **k == kind).count()
        };
        if tracker.refuse == Some((kind, nth)) {
            return Err(std::io::Error::other("scripted spawn failure"));
        }
        let finisher = tracker.clone();
        StdThreadSpawner.spawn(
            kind,
            Box::new(move || {
                job();
                lock(&finisher.finished).push(kind);
                finisher.changed.notify_all();
            }),
        )
    }
}

fn tracked_service(
    dir: &Path,
    tracker: &Arc<TrackingSpawner>,
) -> Result<PreviewImageService, PreviewServiceBuildError> {
    let mut config = config(dir, Arc::new(NeverDecoder));
    config.disable_disk_cache_override = Some(false);
    config.spawner = Some(Arc::new(SharedSpawner(tracker.clone())));
    PreviewImageService::new(config)
}

// Rust-only: two persist workers and one startup cleanup are started; dropping the service completes the queue and the
// workers exit on their own (no C# twin: the C# singleton is never disposed).
#[test]
fn drop_of_the_service_completes_the_queue() {
    let root = TempRoot::new("w414c-drop");
    let tracker = Arc::new(TrackingSpawner::default());
    let service = tracked_service(&root.join("disk"), &tracker).expect("service");
    assert!(service.wait_startup_cleanup(TIMEOUT));
    assert_eq!(
        lock(&tracker.started).clone(),
        vec![
            DecodeThreadKind::PersistWorker,
            DecodeThreadKind::PersistWorker,
            DecodeThreadKind::StartupCleanup
        ]
    );
    assert!(tracker.wait_finished(DecodeThreadKind::StartupCleanup, 1));
    assert_eq!(
        tracker.finished(DecodeThreadKind::PersistWorker),
        0,
        "the workers wait for work"
    );
    drop(service);
    assert!(tracker.wait_finished(DecodeThreadKind::PersistWorker, PERSIST_WORKER_COUNT));
}

// Rust-only: a worker that cannot be started fails the constructor; the worker already started exits (the queue was
// completed), nothing hangs.
#[test]
fn persist_worker_spawn_failure_fails_the_constructor() {
    let root = TempRoot::new("w414c-spawn-fail");
    let tracker = Arc::new(TrackingSpawner {
        refuse: Some((DecodeThreadKind::PersistWorker, 2)),
        ..TrackingSpawner::default()
    });
    let result = tracked_service(&root.join("disk"), &tracker);
    assert!(matches!(result, Err(PreviewServiceBuildError::Spawn(_))), "{result:?}");
    assert!(
        tracker.wait_finished(DecodeThreadKind::PersistWorker, 1),
        "the started worker exits"
    );
    assert!(
        !lock(&tracker.started).contains(&DecodeThreadKind::StartupCleanup),
        "nothing else is started after the failure"
    );

    let tracker = Arc::new(TrackingSpawner {
        refuse: Some((DecodeThreadKind::StartupCleanup, 1)),
        ..TrackingSpawner::default()
    });
    let result = tracked_service(&root.join("disk2"), &tracker);
    assert!(matches!(result, Err(PreviewServiceBuildError::Spawn(_))), "{result:?}");
    assert!(tracker.wait_finished(DecodeThreadKind::PersistWorker, PERSIST_WORKER_COUNT));
}

// Rust-only (semantics 2): with the disk cache disabled the workers are still started, no startup cleanup is.
#[test]
fn disabled_disk_cache_starts_the_workers_but_no_cleanup() {
    let root = TempRoot::new("w414c-disabled-threads");
    let tracker = Arc::new(TrackingSpawner::default());
    let mut config = config(&root.join("disk"), Arc::new(NeverDecoder));
    config.disable_disk_cache_override = Some(true);
    config.spawner = Some(Arc::new(SharedSpawner(tracker.clone())));
    let service = PreviewImageService::new(config).expect("service");
    assert!(service.wait_startup_cleanup(Duration::ZERO));
    assert_eq!(
        lock(&tracker.started).clone(),
        vec![DecodeThreadKind::PersistWorker, DecodeThreadKind::PersistWorker]
    );
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(tracker.wait_finished(DecodeThreadKind::PersistWorker, PERSIST_WORKER_COUNT));
}

// ---------------------------------------------------------------------------------------------------------------------
// Decode-path twins (RED until W4-14b merges: they go through `get_preview_path` / `get_preview`)
// ---------------------------------------------------------------------------------------------------------------------

mod decode_path {
    use super::*;
    use pr_imaging::decode_types::{
        DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
    };
    use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, ImageDecoderFactory};
    use pr_imaging::exif_summary::{ExifRational, ExifSummary};
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_imaging::service::PreviewImage;
    use support::preview_service::{Script, box_source};

    /// A scripted decoder producing a pixel buffer of a given layout and alpha pattern (C# test PNG shapes and
    /// `PixelDecoder`), counting its decodes. Downscaled whenever the request has a box (C# `TargetWidth > 0`).
    pub(super) struct ShapeDecoder {
        pub layout: PixelLayout,
        pub width: i32,
        pub height: i32,
        pub alpha: fn(i32, i32) -> u8,
        pub backend: DecoderBackend,
        pub exif: Option<ExifSummary>,
        pub calls: AtomicUsize,
    }

    impl ShapeDecoder {
        pub fn new(layout: PixelLayout, alpha: fn(i32, i32) -> u8) -> Arc<Self> {
            Arc::new(Self {
                layout,
                width: 120,
                height: 80,
                alpha,
                backend: DecoderBackend::TurboJpeg,
                exif: None,
                calls: AtomicUsize::new(0),
            })
        }

        pub fn opaque() -> Arc<Self> {
            Self::new(PixelLayout::Bgr32, |_, _| 255)
        }

        pub fn with_backend(backend: DecoderBackend) -> Arc<Self> {
            Arc::new(Self {
                backend,
                ..Self::fields(PixelLayout::Bgr32)
            })
        }

        pub fn with_exif(exif: Option<ExifSummary>) -> Arc<Self> {
            Arc::new(Self {
                exif,
                ..Self::fields(PixelLayout::Bgr32)
            })
        }

        fn fields(layout: PixelLayout) -> Self {
            Self {
                layout,
                width: 120,
                height: 80,
                alpha: |_, _| 255,
                backend: DecoderBackend::TurboJpeg,
                exif: None,
                calls: AtomicUsize::new(0),
            }
        }

        pub fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl ImageDecoder for ShapeDecoder {
        fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut pixels = PixelBuffer::allocate(self.width, self.height, self.layout)
                .map_err(|e| DecodeError::InvalidArgument { message: e.to_string() })?;
            for y in 0..self.height {
                let row = pixels
                    .get_row_mut(y)
                    .map_err(|e| DecodeError::InvalidArgument { message: e.to_string() })?;
                for x in 0..self.width {
                    let i = usize::try_from(x * 4).unwrap_or(0);
                    row[i] = 90;
                    row[i + 1] = 91;
                    row[i + 2] = 92;
                    row[i + 3] = (self.alpha)(x, y);
                }
            }
            let options = DecodedImageOptions {
                downscaled: request.target_width > 0 || request.target_height > 0,
                orientation: 6,
                actual_backend: self.backend,
                original_width: 6000,
                original_height: 4000,
                exif: self.exif.clone(),
                is_degraded_fallback: false,
            };
            let estimated = pixels.byte_count();
            let image = DecodedImage::new(
                PlatformImage::Pixels(Arc::new(pixels)),
                self.width,
                self.height,
                estimated,
                options,
            )
            .map_err(|e| DecodeError::InvalidArgument { message: e.to_string() })?;
            Ok(Arc::new(image))
        }

        fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
            Ok(ImageInfo::new(6000, 4000))
        }
    }

    /// A factory returning one decoder for every backend, recording the backends asked for.
    pub(super) struct OneDecoderFactory(pub Arc<dyn ImageDecoder>);

    impl ImageDecoderFactory for OneDecoderFactory {
        fn create(&self, _backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
            self.0.clone()
        }

        fn is_registered(&self, _backend: DecoderBackend) -> bool {
            true
        }
    }

    pub(super) struct Setup {
        pub root: TempRoot,
        pub source: String,
    }

    impl Setup {
        pub fn new(label: &str, file: &str) -> Self {
            let root = TempRoot::new(label);
            let source = write_file(&root.join(file), &[1, 2, 3, 4]);
            Self {
                root,
                source: source.to_str().expect("utf-8").to_owned(),
            }
        }

        pub fn dir(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }
    }

    /// A service over `dir` (disk cache ON unless `disabled`), the C# test box 32 x 32.
    pub(super) fn service(
        dir: &Path,
        decoder: Arc<dyn ImageDecoder>,
        metrics: Option<Arc<ReviewMetrics>>,
    ) -> PreviewImageService {
        service_with(dir, decoder, metrics, |_| {})
    }

    pub(super) fn service_with(
        dir: &Path,
        decoder: Arc<dyn ImageDecoder>,
        metrics: Option<Arc<ReviewMetrics>>,
        adjust: impl FnOnce(&mut PreviewServiceConfig),
    ) -> PreviewImageService {
        let mut config = config(dir, decoder);
        config.disable_disk_cache_override = Some(false);
        config.target_decode_box = Arc::new(|| DecodeBox::new(32, 32));
        if let Some(metrics) = metrics {
            config.metrics = metrics;
        }
        adjust(&mut config);
        PreviewImageService::new(config).expect("the service builds")
    }

    pub(super) fn load(service: &PreviewImageService, path: &str) -> PreviewImage {
        service
            .get_preview_path(path)
            .wait_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("the decode of {path} hung"))
            .unwrap_or_else(|e| panic!("the decode of {path} failed: {e}"))
    }

    pub(super) fn load_key(service: &PreviewImageService, path: &str, key: &ImageCacheKey) -> PreviewImage {
        service
            .get_preview(path, key)
            .wait_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("the decode of {path} hung"))
            .unwrap_or_else(|e| panic!("the decode of {path} failed: {e}"))
    }

    /// Decodes once with a fresh service so its worker writes the entry; returns the entry path and bytes (C#
    /// `WriteEntryAsync`).
    pub(super) fn write_entry(setup: &Setup, dir: &Path) -> (PathBuf, Vec<u8>) {
        let writer = service(dir, ShapeDecoder::opaque(), None);
        load(&writer, &setup.source);
        retire(&writer);
        let files = entries(dir);
        assert_eq!(files.len(), 1, "one entry written");
        let bytes = std::fs::read(&files[0]).expect("entry bytes");
        (files[0].clone(), bytes)
    }

    fn snapshot(metrics: &ReviewMetrics) -> (i64, i64) {
        let s = metrics.snapshot();
        (s.source_reads, s.disk_cache_hits)
    }

    // ---- PreviewImageServiceDiskCacheTests ------------------------------------------------------------------------

    // C#: PreviewImageServiceDiskCacheTests.DownscaledPreviewIsPersistedToDiskCache
    #[test]
    fn downscaled_preview_is_persisted_to_disk_cache() {
        let setup = Setup::new("w414c-e2e-write", "preview-a.png");
        let dir = setup.dir("write");
        let service = service(&dir, ShapeDecoder::opaque(), None);
        load(&service, &setup.source);
        retire(&service);
        let files = entries(&dir);
        assert_eq!(files.len(), 1);
        assert!(std::fs::metadata(&files[0]).expect("entry").len() > 0);
    }

    /// C# `DecodePngAsync`: decode one shape into a fresh directory, drain the workers, return the entries and image.
    fn decode_shape(setup: &Setup, name: &str, decoder: Arc<ShapeDecoder>) -> (Vec<PathBuf>, PreviewImage) {
        let dir = setup.dir(&format!("out-{name}"));
        let service = service(&dir, decoder, None);
        let image = load(&service, &setup.source);
        retire(&service);
        (entries(&dir), image)
    }

    fn has_alpha(image: &PreviewImage) -> bool {
        matches!(image.platform_image(), PlatformImage::Pixels(p) if p.layout() == PixelLayout::Pbgra32)
    }

    // C#: PreviewImageServiceDiskCacheTests.TransparentPng_IsNotPersistedToDiskCache
    #[test]
    fn transparent_png_is_not_persisted_to_disk_cache() {
        let setup = Setup::new("w414c-e2e-alpha", "transparent.png");
        let dir = setup.dir("alpha-no-write");
        let service = service(
            &dir,
            ShapeDecoder::new(PixelLayout::Pbgra32, |x, _| (x % 2) as u8 * 255),
            None,
        );
        let first = load(&service, &setup.source);
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        let written = std::fs::read_dir(&dir).map(Iterator::count).unwrap_or(0);
        service.clear_cache();
        let second = load(&service, &setup.source);
        assert!(first.downscaled() && second.downscaled());
        assert!(has_alpha(&second), "the returned image keeps its alpha");
        assert_eq!(written, 0, "nothing at all was written");
        assert!(service.wait_for_prune(TIMEOUT));
    }

    // C#: PreviewImageServiceDiskCacheTests.OpaquePng_IsStillPersisted
    #[test]
    fn opaque_png_is_still_persisted() {
        let setup = Setup::new("w414c-e2e-opaque", "preview-a.png");
        let (files, image) = decode_shape(&setup, "opaque", ShapeDecoder::opaque());
        assert!(image.downscaled());
        assert_eq!(files.len(), 1);
    }

    // C#: PreviewImageServiceDiskCacheTests.OpaqueRgbaPng_IsPersisted
    #[test]
    fn opaque_rgba_png_is_persisted() {
        let setup = Setup::new("w414c-e2e-rgba", "rgba.png");
        let (files, image) = decode_shape(
            &setup,
            "rgba-opaque",
            ShapeDecoder::new(PixelLayout::Pbgra32, |_, _| 255),
        );
        assert!(image.downscaled());
        assert!(
            has_alpha(&image),
            "the alpha scan path (not the opaque-format shortcut) was exercised"
        );
        assert_eq!(files.len(), 1);
    }

    // C#: PreviewImageServiceDiskCacheTests.HalfTransparentRgbaPng_IsNotPersisted
    #[test]
    fn half_transparent_rgba_png_is_not_persisted() {
        let setup = Setup::new("w414c-e2e-half", "rgba.png");
        let (files, image) = decode_shape(&setup, "rgba-half", ShapeDecoder::new(PixelLayout::Pbgra32, |_, _| 128));
        assert!(has_alpha(&image));
        assert!(files.is_empty());
    }

    // C#: PreviewImageServiceDiskCacheTests.PartiallyTransparentRgbaPng_IsNotPersisted
    #[test]
    fn partially_transparent_rgba_png_is_not_persisted() {
        let setup = Setup::new("w414c-e2e-corner", "rgba.png");
        let decoder = ShapeDecoder::new(PixelLayout::Pbgra32, |x, y| if x >= 60 && y >= 40 { 0 } else { 255 });
        let (files, _) = decode_shape(&setup, "rgba-corner", decoder);
        assert!(files.is_empty());
    }

    // C#: PreviewImageServiceDiskCacheTests.IndexedPng_PersistedOnlyWhenPaletteOpaque (a palette is a Pbgra32 buffer
    // after decode: an opaque palette = all 255, a translucent entry = a few 0 alphas)
    #[test]
    fn indexed_png_persisted_only_when_palette_opaque() {
        let setup = Setup::new("w414c-e2e-indexed", "idx.png");
        let (opaque, _) = decode_shape(
            &setup,
            "idx-opaque",
            ShapeDecoder::new(PixelLayout::Pbgra32, |_, _| 255),
        );
        let translucent = ShapeDecoder::new(PixelLayout::Pbgra32, |x, y| if (x + y) % 17 == 0 { 0 } else { 255 });
        let (translucent, _) = decode_shape(&setup, "idx-trns", translucent);
        assert_eq!(opaque.len(), 1);
        assert!(translucent.is_empty());
    }

    // C#: PreviewImageServiceDiskCacheTests.OriginalModeDoesNotWriteToDiskCache
    #[test]
    fn original_mode_does_not_write_to_disk_cache() {
        let setup = Setup::new("w414c-e2e-original", "preview-a.png");
        let dir = setup.dir("original-no-write");
        let decoder = FakeDecoder::new();
        decoder.script_default(Script::default());
        let service = service_with(&dir, decoder, None, |config| {
            config.is_original_loading_mode = Arc::new(|| true);
            config.target_decode_box = Arc::new(|| DecodeBox::new(0, 0));
        });
        load(&service, &setup.source);
        retire(&service);
        assert!(entries(&dir).is_empty());
    }

    // C#: PreviewImageServiceDiskCacheTests.DiskCacheHitAvoidsSourceRead
    #[test]
    fn disk_cache_hit_avoids_source_read() {
        let setup = Setup::new("w414c-e2e-hit", "preview-a.png");
        let dir = setup.dir("hit");
        write_entry(&setup, &dir);
        let metrics = Arc::new(ReviewMetrics::new());
        let decoder = ShapeDecoder::opaque();
        let reader = service(&dir, decoder.clone(), Some(metrics.clone()));
        let image = load(&reader, &setup.source);
        assert!(image.pixel_width() > 0);
        assert_eq!(snapshot(&metrics), (0, 1));
        assert_eq!(decoder.calls(), 0);
        retire(&reader);
    }

    // C#: PreviewImageServiceDiskCacheTests.CorruptDiskCacheEntryFallsBackToSource
    #[test]
    fn corrupt_disk_cache_entry_falls_back_to_source() {
        let setup = Setup::new("w414c-e2e-corrupt", "preview-a.png");
        let dir = setup.dir("corrupt");
        let (entry, _) = write_entry(&setup, &dir);
        write_file(&entry, &[1, 2, 3, 4]);
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = service(&dir, ShapeDecoder::opaque(), Some(metrics.clone()));
        let image = load(&reader, &setup.source);
        assert!(image.pixel_width() > 0);
        assert_eq!(snapshot(&metrics), (1, 0));
        retire(&reader);
    }

    // C#: PreviewImageServiceDiskCacheTests.DiskCacheEntryWithMismatchedVersionIsMiss
    #[test]
    fn disk_cache_entry_with_mismatched_version_is_miss() {
        let setup = Setup::new("w414c-e2e-version", "preview-a.png");
        let dir = setup.dir("cache-version");
        let (entry, bytes) = write_entry(&setup, &dir);
        const HEADER_SIZE: usize = 24;
        assert!(
            bytes[HEADER_SIZE] == 0 && bytes[HEADER_SIZE + 1] == 0,
            "fixture entry has no EXIF block"
        );
        // The previous (no-EXIF-length) layout, stamped with a future version: only the version check makes it a miss.
        let mut legacy: Vec<u8> = bytes[..HEADER_SIZE].to_vec();
        legacy.extend_from_slice(&bytes[HEADER_SIZE + 2..]);
        legacy[4] = legacy[4].wrapping_add(1);
        write_file(&entry, &legacy);
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = service(&dir, ShapeDecoder::opaque(), Some(metrics.clone()));
        let image = load(&reader, &setup.source);
        assert!(image.pixel_width() > 0);
        assert_eq!(snapshot(&metrics), (1, 0));
        retire(&reader);
    }

    // C#: PreviewImageServiceDiskCacheTests.ZeroDiskCacheCapacityDisablesDiskCache
    #[test]
    fn zero_disk_cache_capacity_disables_disk_cache() {
        let setup = Setup::new("w414c-e2e-cap0", "preview-a.png");
        let dir = setup.dir("cap-zero");
        let (existing, _) = write_entry(&setup, &dir);
        let metrics = Arc::new(ReviewMetrics::new());
        let off = service_with(&dir, ShapeDecoder::opaque(), Some(metrics.clone()), |config| {
            config.disable_disk_cache_override = None;
            config.diag = Some(Arc::new(Default::default()));
            config.disk_cache_capacity_bytes = 0;
        });
        let image = load(&off, &setup.source);
        retire(&off);
        assert!(image.pixel_width() > 0);
        assert_eq!(snapshot(&metrics), (1, 0), "the existing entry must not be read");
        assert!(existing.exists(), "a disabled cache must not prune existing entries");
    }

    // C#: PreviewImageServiceDiskCacheTests.ZeroDiskCacheCapacityDoesNotWrite
    #[test]
    fn zero_disk_cache_capacity_does_not_write() {
        let setup = Setup::new("w414c-e2e-cap0-write", "preview-a.png");
        let dir = setup.dir("cap-zero-write");
        let off = service_with(&dir, ShapeDecoder::opaque(), None, |config| {
            config.disable_disk_cache_override = None;
            config.diag = Some(Arc::new(Default::default()));
            config.disk_cache_capacity_bytes = 0;
        });
        load(&off, &setup.source);
        retire(&off);
        assert!(entries(&dir).is_empty());
    }

    // C#: PreviewImageServiceDiskCacheTests.DiskCacheIsPrunedToConfiguredQuota
    #[test]
    fn disk_cache_is_pruned_to_configured_quota() {
        let setup = Setup::new("w414c-e2e-quota", "preview-a.png");
        let dir = setup.dir("quota");
        const QUOTA_BYTES: i64 = 1;
        for width in [100, 200, 300, 400, 500] {
            let target = Arc::new(Mutex::new(DecodeBox::new(width, 0)));
            let service = service_with(&dir, ShapeDecoder::opaque(), None, |config| {
                config.target_decode_box = box_source(target.clone());
                config.disk_cache_capacity_bytes = QUOTA_BYTES;
            });
            load(&service, &setup.source);
            assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
            assert!(service.wait_for_prune(TIMEOUT), "prune for width={width}");
        }
        let remaining: u64 = entries(&dir)
            .iter()
            .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
            .sum();
        assert!(remaining <= 1, "the 1-byte quota is enforced: {remaining} bytes remain");
    }

    // C#: PreviewImageServiceTests.EvictionInDownscaledModeForcesFreshSourceReadWhenDiskCacheCleared
    #[test]
    fn eviction_in_downscaled_mode_forces_fresh_source_read_when_disk_cache_cleared() {
        let setup = Setup::new("w414c-e2e-evict", "preview-a.png");
        let dir = setup.dir("disk-cache-eviction-downscaled");
        let metrics = Arc::new(ReviewMetrics::new());
        let service = service_with(&dir, ShapeDecoder::opaque(), Some(metrics.clone()), |config| {
            config.target_decode_box = Arc::new(|| DecodeBox::new(512, 0));
        });
        load(&service, &setup.source);
        // The background persistence has written the entry once the workers are down.
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(service.wait_for_prune(TIMEOUT));
        service.disk_store().clear_directory().expect("clear the directory");
        service.evict_cached_path(&setup.source, None).expect("evict");
        load(&service, &setup.source);
        assert_eq!(metrics.snapshot().source_reads, 2);
        assert_eq!(service.cache_count(), 1);
    }

    // ---- PreviewImageServicePersistStaleTests / TwoWorkerTests ----------------------------------------------------

    // C#: PreviewImageServicePersistStaleTests.Persist_ClearCacheDuringWrite_DeletesStaleFile
    #[test]
    fn persist_clear_cache_during_write_deletes_stale_file() {
        let setup = Setup::new("w414c-e2e-stale", "opaque.png");
        let dir = setup.dir("cache");
        let service = Arc::new(service(&dir, ShapeDecoder::opaque(), None));
        let calls = Arc::new(AtomicUsize::new(0));
        let weak = Arc::downgrade(&service);
        let hook_calls = calls.clone();
        service.set_after_persist_write_for_tests(Some(Arc::new(move || {
            if hook_calls.fetch_add(1, Ordering::SeqCst) == 0
                && let Some(service) = weak.upgrade()
            {
                service.clear_cache(); // bumps the epoch; does not touch disk files
            }
        })));
        let key = service.get_current_cache_key(&setup.source).expect("key");
        load_key(&service, &setup.source, &key);
        retire(&service);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!service.has_disk_cached_preview(&key));
        assert!(entries(&dir).is_empty());
    }

    // C#: PreviewImageServicePersistStaleTests.Persist_NoClearCache_KeepsFile
    #[test]
    fn persist_no_clear_cache_keeps_file() {
        let setup = Setup::new("w414c-e2e-keep", "opaque.png");
        let dir = setup.dir("cache");
        let service = service(&dir, ShapeDecoder::opaque(), None);
        let calls = Arc::new(AtomicUsize::new(0));
        let hook_calls = calls.clone();
        service.set_after_persist_write_for_tests(Some(Arc::new(move || {
            hook_calls.fetch_add(1, Ordering::SeqCst);
        })));
        let key = service.get_current_cache_key(&setup.source).expect("key");
        load_key(&service, &setup.source, &key);
        retire(&service);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(service.has_disk_cached_preview(&key));
    }

    /// A one-shot gate (bounded by [`TIMEOUT`]).
    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        changed: Condvar,
    }

    impl Gate {
        fn open(&self) {
            *lock(&self.open) = true;
            self.changed.notify_all();
        }

        fn wait(&self) -> bool {
            let guard = lock(&self.open);
            let (guard, _) = self
                .changed
                .wait_timeout_while(guard, Duration::from_secs(30), |open| !*open)
                .unwrap_or_else(PoisonError::into_inner);
            *guard
        }
    }

    // C#: PreviewImageServicePersistTwoWorkerTests.StaleWorkerCleanup_AfterNewerWorkerWroteSamePath_KeepsNewerFile
    #[test]
    fn stale_worker_cleanup_after_newer_worker_wrote_same_path_keeps_newer_file() {
        let setup = Setup::new("w414c-e2e-two", "opaque.png");
        let dir = setup.dir("cache");
        let service = service(&dir, ShapeDecoder::opaque(), None);
        let first_done = Arc::new(Gate::default());
        let release_first = Arc::new(Gate::default());
        let second_done = Arc::new(Gate::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let timed_out = Arc::new(AtomicUsize::new(0));
        {
            let (first_done, release_first, second_done, calls, timed_out) = (
                first_done.clone(),
                release_first.clone(),
                second_done.clone(),
                calls.clone(),
                timed_out.clone(),
            );
            service.set_after_persist_write_for_tests(Some(Arc::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    first_done.open();
                    if !release_first.wait() {
                        timed_out.fetch_add(1, Ordering::SeqCst);
                    }
                } else {
                    second_done.open();
                }
            })));
        }
        let key = service.get_current_cache_key(&setup.source).expect("key");
        load_key(&service, &setup.source, &key);
        assert!(first_done.wait());
        service.clear_cache(); // drops the RAM copy so the next load decodes again
        service.clear_disk(); // newer epoch; removes worker 1's file so the next load misses disk and persists again
        load_key(&service, &setup.source, &key);
        assert!(second_done.wait());
        assert!(service.has_disk_cached_preview(&key));
        release_first.open();
        retire(&service);
        assert_eq!(timed_out.load(Ordering::SeqCst), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(service.has_disk_cached_preview(&key));
    }

    // ---- PreviewImageServicePixelCodecTests ------------------------------------------------------------------------

    // C#: PreviewImageServicePixelCodecTests.PixelBufferPreview_PersistsAndReadsBack_WithEitherCodec (only the pixel-buffer
    // codec: the WPF codec and `LegacyWpfPreviewCache` halves have no Rust twin, there is no WPF in the port)
    #[test]
    fn pixel_buffer_preview_persists_and_reads_back() {
        let setup = Setup::new("w414c-e2e-pixels", "photo.jpg");
        let dir = setup.dir("disk");
        let writer = service_with(&dir, ShapeDecoder::opaque(), None, |c| {
            c.target_decode_box = Arc::new(|| DecodeBox::new(4, 0));
        });
        let produced = load(&writer, &setup.source);
        assert!(matches!(produced.platform_image(), PlatformImage::Pixels(_)));
        retire(&writer);
        assert_eq!(entries(&dir).len(), 1);
        let reader_decoder = ShapeDecoder::opaque();
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = service_with(&dir, reader_decoder.clone(), Some(metrics.clone()), |c| {
            c.target_decode_box = Arc::new(|| DecodeBox::new(4, 0));
        });
        let from_disk = load(&reader, &setup.source);
        assert_eq!(reader_decoder.calls(), 0);
        assert_eq!(metrics.snapshot().disk_cache_hits, 1);
        let PlatformImage::Pixels(pixels) = from_disk.platform_image() else {
            panic!("served as a pixel buffer");
        };
        assert_eq!(
            (pixels.width(), pixels.height(), pixels.layout()),
            (120, 80, PixelLayout::Bgr32)
        );
        assert_eq!(
            (
                from_disk.orientation(),
                from_disk.actual_backend(),
                from_disk.original_width(),
                from_disk.original_height()
            ),
            (6, DecoderBackend::TurboJpeg, 6000, 4000)
        );
        assert!(from_disk.downscaled());
        retire(&reader);
    }

    // C#: PreviewImageServicePixelCodecTests.Pbgra32Preview_PersistsOnlyWhenOpaque (theory: true -> 1, false -> 0)
    #[test]
    fn pbgra32_preview_persists_only_when_opaque() {
        let setup = Setup::new("w414c-e2e-pbgra", "photo.jpg");
        let opaque = ShapeDecoder::new(PixelLayout::Pbgra32, |_, _| 255);
        let one_transparent = ShapeDecoder::new(PixelLayout::Pbgra32, |x, y| if x == 0 && y == 40 { 0 } else { 255 });
        for (decoder, expected, name) in [(opaque, 1usize, "disk-true"), (one_transparent, 0, "disk-false")] {
            let dir = setup.dir(name);
            let writer = service(&dir, decoder, None);
            load(&writer, &setup.source);
            retire(&writer);
            assert_eq!(entries(&dir).len(), expected, "{name}");
        }
    }

    /// A decoder whose image carries a platform image the codec cannot convert (C# `ForeignDecoder`).
    struct ForeignDecoder;

    impl ImageDecoder for ForeignDecoder {
        fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
            let options = DecodedImageOptions {
                downscaled: true,
                orientation: 1,
                actual_backend: DecoderBackend::Wpf,
                original_width: 0,
                original_height: 0,
                exif: None,
                is_degraded_fallback: false,
            };
            let foreign: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
            DecodedImage::new(PlatformImage::Opaque(foreign), 8, 8, 256, options)
                .map(|image| Arc::new(image) as Arc<dyn DecodedImageView>)
                .map_err(|e| DecodeError::InvalidArgument { message: e.to_string() })
        }

        fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
            Ok(ImageInfo::new(8, 8))
        }
    }

    // C#: PreviewImageServicePixelCodecTests.UnconvertiblePlatformImage_IsNotPersisted_NoError
    #[test]
    fn unconvertible_platform_image_is_not_persisted_no_error() {
        let setup = Setup::new("w414c-e2e-foreign", "photo.jpg");
        let dir = setup.dir("disk-foreign");
        let log = Arc::new(RecordingLog::new());
        let service = service_with(&dir, Arc::new(ForeignDecoder), None, |c| {
            c.log = Some(log.clone());
            c.capacity_bytes = 1024 * 1024;
        });
        load(&service, &setup.source);
        retire(&service);
        assert!(entries(&dir).is_empty());
        assert!(errors(&log).is_empty(), "{:?}", errors(&log));
    }

    // ---- CacheTempFileAndDiskProbeTests ---------------------------------------------------------------------------

    // C#: CacheTempFileAndDiskProbeTests.HasDiskCachedPreview_ReflectsPersistedEntry
    #[test]
    fn has_disk_cached_preview_reflects_persisted_entry() {
        let setup = Setup::new("w414c-e2e-probe", "opaque.png");
        let dir = setup.dir("probe");
        let service = service(&dir, ShapeDecoder::opaque(), None);
        let disabled = service_with(&dir, ShapeDecoder::opaque(), None, |c| {
            c.disable_disk_cache_override = Some(true);
        });
        let key = service.get_current_cache_key(&setup.source).expect("key");
        assert!(!service.has_disk_cached_preview(&key));
        load_key(&service, &setup.source, &key);
        retire(&service);
        assert!(service.has_disk_cached_preview(&key));
        let disabled_key = disabled.get_current_cache_key(&setup.source).expect("key");
        assert!(!disabled.has_disk_cached_preview(&disabled_key));
        retire(&disabled);
    }

    // ---- PreviewImageServiceMutationTests ---------------------------------------------------------------------------

    // C#: PreviewImageServiceMutationTests.Ctor_NonPositiveDiskCapacity_DisablesDiskCache (theory 0, -1; seeded through
    // a real decode)
    #[test]
    fn ctor_non_positive_disk_capacity_disables_disk_cache() {
        for capacity in [0i64, -1] {
            let setup = Setup::new("w414c-e2e-capacity", "a.jpg");
            let dir = setup.dir("disk");
            let seeding = service(&dir, ShapeDecoder::opaque(), None);
            let key = seeding.get_current_cache_key(&setup.source).expect("key");
            load_key(&seeding, &setup.source, &key);
            retire(&seeding);
            assert_eq!(entries(&dir).len(), 1);
            assert!(seeding.has_disk_cached_preview(&key));
            let disabled = service_with(&dir, ShapeDecoder::opaque(), None, |c| {
                c.disable_disk_cache_override = None;
                c.diag = Some(Arc::new(Default::default()));
                c.disk_cache_capacity_bytes = capacity;
            });
            assert!(!disabled.has_disk_cached_preview(&key), "capacity {capacity}");
            retire(&disabled);
        }
    }

    // C#: PreviewImageServiceMutationTests.DiskCacheDisabled_DecodeIsNotPersisted
    #[test]
    fn disk_cache_disabled_decode_is_not_persisted() {
        let setup = Setup::new("w414c-e2e-disabled", "a.jpg");
        let dir = setup.dir("never-created");
        let log = Arc::new(RecordingLog::new());
        let decoder = ShapeDecoder::opaque();
        let service = service_with(&dir, decoder.clone(), None, |c| {
            c.disable_disk_cache_override = Some(true);
            c.log = Some(log.clone());
        });
        load(&service, &setup.source);
        retire(&service);
        assert_eq!(decoder.calls(), 1);
        assert!(errors(&log).is_empty());
        assert!(!dir.exists(), "the directory is never created");
    }

    // C#: PreviewImageServiceMutationTests.DiskCacheHit_DoesNotRewriteTheEntry
    #[test]
    fn disk_cache_hit_does_not_rewrite_the_entry() {
        let setup = Setup::new("w414c-e2e-hit-no-write", "a.jpg");
        let dir = setup.dir("disk-hit");
        let (entry, _) = write_entry(&setup, &dir);
        let sentinel = std::time::UNIX_EPOCH + Duration::from_secs(981_173_106); // 2001-02-03 04:05:06 UTC
        set_write_time(&entry, sentinel);
        let decoder = ShapeDecoder::opaque();
        let second = service(&dir, decoder.clone(), None);
        load(&second, &setup.source);
        retire(&second);
        assert_eq!(decoder.calls(), 0, "served from disk");
        assert_eq!(
            std::fs::metadata(&entry).and_then(|m| m.modified()).expect("mtime"),
            sentinel
        );
        assert_eq!(entries(&dir).len(), 1);
    }

    // C#: PreviewImageServiceMutationTests.ClearDisk_DuringInflightDecode_ResultNotCachedOrPersisted
    #[test]
    fn clear_disk_during_inflight_decode_result_not_cached_or_persisted() {
        let setup = Setup::new("w414c-e2e-inflight", "a.jpg");
        let dir = setup.dir("disk-clear-inflight");
        let decoder = FakeDecoder::new();
        decoder.script_default(Script {
            downscaled: true,
            ..Script::default()
        });
        decoder.block();
        let service = service(&dir, decoder.clone(), None);
        assert!(service.wait_startup_cleanup(TIMEOUT));
        let key = service.get_current_cache_key(&setup.source).expect("key");
        let pending = service.get_preview(&setup.source, &key);
        assert!(decoder.wait_started("a.jpg"));
        service.clear_disk(); // the cache generation changes while the decode runs
        decoder.release();
        let image = pending
            .wait_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("hung"))
            .expect("the caller still gets the image");
        retire(&service);
        assert!(image.pixel_width() > 0);
        assert!(service.try_get_cached_preview(&key).is_none());
        assert!(entries(&dir).is_empty());
    }

    // ---- PreviewImageServiceFaultTests ------------------------------------------------------------------------------

    /// C# `Break(kind, valid)`.
    fn break_entry(kind: &str, valid: &[u8]) -> Vec<u8> {
        let mut bytes = valid.to_vec();
        let len = bytes.len();
        match kind {
            "zero-bytes" => return Vec::new(),
            "one-byte" => return vec![b'P'],
            "header-only" => return bytes[..24].to_vec(),
            "cut-mid-payload" => return bytes[..len - (len / 3).max(4)].to_vec(),
            "bad-magic" => bytes[0] = b'X',
            "future-version" => bytes[4] = bytes[4].wrapping_add(1),
            "orientation-0" => bytes[6] = 0,
            "backend-255" => bytes[5] = 255,
            "width-negative" => bytes[11] = 0x80,
            "exif-length-past-eof" => {
                bytes[24] = 0xFF;
                bytes[25] = 0x03;
            }
            "garbage-payload" => {
                for (i, byte) in bytes.iter_mut().enumerate().skip(26) {
                    *byte = (i * 31 % 256) as u8;
                }
            }
            "garbage-with-eoi" => {
                for (i, byte) in bytes.iter_mut().enumerate().skip(26) {
                    *byte = ((i * 31) | 1) as u8;
                }
                bytes[len - 2] = 0xFF;
                bytes[len - 1] = 0xD9;
            }
            "jpeg-soi-garbage-eoi" => {
                for (i, byte) in bytes.iter_mut().enumerate().skip(26) {
                    *byte = ((i * 31) | 1) as u8;
                }
                bytes[26..30].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);
                bytes[len - 2] = 0xFF;
                bytes[len - 1] = 0xD9;
            }
            other => panic!("unknown broken entry kind {other}"),
        }
        bytes
    }

    fn check_broken_entry(kind: &str) {
        let setup = Setup::new("w414c-e2e-broken", "a.png");
        let dir = setup.dir(&format!("disk-{kind}"));
        let (entry, valid) = write_entry(&setup, &dir);
        let broken = break_entry(kind, &valid);
        write_file(&entry, &broken);
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = service(&dir, ShapeDecoder::opaque(), Some(metrics.clone()));
        let image = load(&reader, &setup.source);
        retire(&reader);
        assert!(image.pixel_width() > 0 && image.pixel_height() > 0, "{kind}");
        assert_eq!(snapshot(&metrics), (1, 0), "{kind}: one source read, no disk hit");
        if entry.exists() {
            assert_ne!(
                std::fs::read(&entry).expect("entry"),
                broken,
                "{kind}: the broken entry was left in place"
            );
        }
    }

    // C#: PreviewImageServiceFaultTests.BrokenEntry_IsAMiss (theory over the 14 BrokenEntries shapes; the payload rows
    // end in InvalidData through FakePayloadCodec). The 14th row, `jpeg-header-mangled-eoi`, is NOT here: the fake payload
    // is 13 bytes, so zeroing from offset 40 changes nothing and no fake can reproduce WIC's verdict on a real JPEG. Its
    // twin runs against the real WIC payload codec in crates/pr-platform-win/tests/wic_preview_service_broken_entry.rs.
    #[test]
    fn broken_entry_is_a_miss() {
        for kind in [
            "zero-bytes",
            "one-byte",
            "header-only",
            "cut-mid-payload",
            "bad-magic",
            "future-version",
            "orientation-0",
            "backend-255",
            "width-negative",
            "exif-length-past-eof",
            "garbage-payload",
            "garbage-with-eoi",
            "jpeg-soi-garbage-eoi",
        ] {
            check_broken_entry(kind);
        }
    }

    /// C# `ExifDecoder` value.
    fn exif() -> ExifSummary {
        ExifSummary {
            date_taken: DateTimeUtc::from_ticks(638_501_690_020_000_000),
            camera_make: Some("Canon".to_owned()),
            camera_model: Some("Canon EOS R5".to_owned()),
            lens_model: Some("RF24-70mm F2.8 L IS USM".to_owned()),
            iso: Some(400),
            focal_length: Some(ExifRational::new(50, 1)),
            f_number: Some(ExifRational::new(28, 10)),
            exposure_time: Some(ExifRational::new(1, 250)),
        }
    }

    // C#: PreviewImageServiceFaultTests.DamagedExifSegment_PixelsIntact_IsAHit. The C# damages the APP1 segment inside
    // the JPEG payload (a WIC concern, W4-11e); the Rust twin damages the entry's own EXIF block (same length, garbage
    // bytes): the pixels are intact, so it is a disk hit with the clean entry's pixels and no EXIF.
    #[test]
    fn damaged_exif_segment_pixels_intact_is_a_hit() {
        let setup = Setup::new("w414c-e2e-damaged-exif", "a.png");
        let dir = setup.dir("disk-damaged-exif");
        let writer = service(&dir, ShapeDecoder::with_exif(Some(exif())), None);
        load(&writer, &setup.source);
        retire(&writer);
        let entry = entries(&dir).remove(0);
        let mut bytes = std::fs::read(&entry).expect("entry");
        let exif_len = usize::from(u16::from_le_bytes([bytes[24], bytes[25]]));
        assert!(exif_len > 0, "the entry carries an EXIF block");
        for byte in &mut bytes[26..26 + exif_len] {
            *byte = 0xEE;
        }
        write_file(&entry, &bytes);
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = service(&dir, ShapeDecoder::opaque(), Some(metrics.clone()));
        let image = load(&reader, &setup.source);
        retire(&reader);
        assert_eq!(snapshot(&metrics), (0, 1));
        assert_eq!((image.pixel_width(), image.pixel_height()), (120, 80));
    }

    // C#: PreviewImageServiceFaultTests.LockedEntry_FallsBackToSource
    #[cfg(windows)]
    #[test]
    fn locked_entry_falls_back_to_source() {
        use std::os::windows::fs::OpenOptionsExt;
        let setup = Setup::new("w414c-e2e-locked", "a.png");
        let dir = setup.dir("disk-locked");
        let (entry, _) = write_entry(&setup, &dir);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&entry)
            .expect("hold the entry exclusively");
        let metrics = Arc::new(ReviewMetrics::new());
        let log = Arc::new(RecordingLog::new());
        let reader = service_with(&dir, ShapeDecoder::opaque(), Some(metrics.clone()), |c| {
            c.log = Some(log.clone());
        });
        let image = load(&reader, &setup.source);
        retire(&reader);
        drop(held);
        assert!(image.pixel_width() > 0);
        assert_eq!(metrics.snapshot().source_reads, 1);
        assert!(
            errors(&log).iter().any(|e| e.contains("Delete failed")),
            "{:?}",
            errors(&log)
        );
    }

    // ---- PreviewCacheExifTests --------------------------------------------------------------------------------------

    // C#: PreviewCacheExifTests.DiskHitCarriesExifWithoutSourceRead
    #[test]
    fn disk_hit_carries_exif_without_source_read() {
        let setup = Setup::new("w414c-e2e-exif", "a.jpg");
        let dir = setup.dir("service");
        let writer = service_with(&dir, ShapeDecoder::with_exif(Some(exif())), None, |c| {
            c.target_decode_box = Arc::new(|| DecodeBox::new(4, 0));
        });
        let produced = load(&writer, &setup.source);
        retire(&writer);
        assert_eq!(produced.exif(), Some(&exif()));
        assert_eq!(entries(&dir).len(), 1);
        let reader_decoder = ShapeDecoder::with_exif(None);
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = service_with(&dir, reader_decoder.clone(), Some(metrics.clone()), |c| {
            c.target_decode_box = Arc::new(|| DecodeBox::new(4, 0));
        });
        let from_disk = load(&reader, &setup.source);
        assert_eq!(from_disk.exif(), Some(&exif()));
        assert_eq!(reader_decoder.calls(), 0);
        assert_eq!(snapshot(&metrics), (0, 1));
        // The RAM hit that follows is the same object, EXIF included.
        assert!(Arc::ptr_eq(&from_disk, &load(&reader, &setup.source)));
        retire(&reader);
    }

    // ---- PreviewBackendIdentityTests --------------------------------------------------------------------------------

    fn backend_service(
        dir: &Path,
        current: DecoderBackend,
        decoder: Arc<ShapeDecoder>,
        metrics: Option<Arc<ReviewMetrics>>,
    ) -> PreviewImageService {
        service_with(dir, ShapeDecoder::opaque(), metrics, move |c| {
            c.current_backend = Some(Arc::new(move || current));
            c.decoder = None;
            c.decoder_factory = Some(Arc::new(OneDecoderFactory(decoder)));
            c.capacity_bytes = 1024 * 1024;
            c.target_decode_box = Arc::new(|| DecodeBox::new(2, 0));
        })
    }

    // C#: PreviewBackendIdentityTests.FallbackDecodedPreviewIsPersistedAndDiskHitReportsTheActualBackend
    #[test]
    fn fallback_decoded_preview_is_persisted_and_disk_hit_reports_the_actual_backend() {
        let setup = Setup::new("w414c-e2e-fallback", "source.bin");
        let dir = setup.dir("fallback");
        let decoder = ShapeDecoder::with_backend(DecoderBackend::Wpf);
        let writer = backend_service(&dir, DecoderBackend::WicDirect, decoder.clone(), None);
        let first = load(&writer, &setup.source);
        let second = load(&writer, &setup.source);
        retire(&writer);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.actual_backend(), DecoderBackend::Wpf);
        assert_eq!(decoder.calls(), 1);
        assert_eq!(entries(&dir).len(), 1);
        let reader_decoder = ShapeDecoder::with_backend(DecoderBackend::Wpf);
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = backend_service(
            &dir,
            DecoderBackend::WicDirect,
            reader_decoder.clone(),
            Some(metrics.clone()),
        );
        let from_disk = load(&reader, &setup.source);
        retire(&reader);
        assert_eq!(from_disk.actual_backend(), DecoderBackend::Wpf);
        assert_eq!(reader_decoder.calls(), 0);
        assert_eq!(snapshot(&metrics), (0, 1));
    }

    // C#: PreviewBackendIdentityTests.VersionedDiskHitPreservesProducingBackendAndAvoidsSourceDecode
    #[test]
    fn versioned_disk_hit_preserves_producing_backend_and_avoids_source_decode() {
        let setup = Setup::new("w414c-e2e-versioned", "source.bin");
        let dir = setup.dir("disk-hit");
        let writer_decoder = ShapeDecoder::with_backend(DecoderBackend::WicDirect);
        let writer = backend_service(&dir, DecoderBackend::WicDirect, writer_decoder, None);
        let produced = load(&writer, &setup.source);
        retire(&writer);
        assert_eq!(produced.actual_backend(), DecoderBackend::WicDirect);
        assert_eq!(entries(&dir).len(), 1);
        let reader_decoder = ShapeDecoder::with_backend(DecoderBackend::WicDirect);
        let metrics = Arc::new(ReviewMetrics::new());
        let reader = backend_service(
            &dir,
            DecoderBackend::WicDirect,
            reader_decoder.clone(),
            Some(metrics.clone()),
        );
        let from_disk = load(&reader, &setup.source);
        retire(&reader);
        assert_eq!(from_disk.actual_backend(), DecoderBackend::WicDirect);
        assert_eq!(reader_decoder.calls(), 0);
        assert_eq!(snapshot(&metrics), (0, 1));
    }

    // C#: PreviewBackendIdentityTests.LegacyDiskEntryWithoutBackendProvenanceIsAMiss (the legacy `.png` entry is named by
    // the pre-v4 identity hash; it never matches a `.pv4` name)
    #[test]
    fn legacy_disk_entry_without_backend_provenance_is_a_miss() {
        let setup = Setup::new("w414c-e2e-legacy", "source.bin");
        let dir = setup.dir("legacy");
        std::fs::create_dir_all(&dir).expect("dir");
        let decoder = ShapeDecoder::with_backend(DecoderBackend::WicDirect);
        let metrics = Arc::new(ReviewMetrics::new());
        let service = backend_service(&dir, DecoderBackend::WicDirect, decoder.clone(), Some(metrics.clone()));
        let key = service.get_current_cache_key(&setup.source).expect("key");
        let legacy_identity = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            key.path(),
            key.length(),
            key.last_write_utc_ticks(),
            if key.is_original() { "True" } else { "False" },
            key.target_width(),
            if key.orientation_applied() { "True" } else { "False" },
            "WicDirect"
        );
        let legacy_name = disk_cache_file_name(&key).replace(".pv4", ".png");
        write_file(&dir.join(legacy_name), legacy_identity.as_bytes());
        let image = load_key(&service, &setup.source, &key);
        retire(&service);
        assert_eq!(image.actual_backend(), DecoderBackend::WicDirect);
        assert_eq!(decoder.calls(), 1);
        assert_eq!(snapshot(&metrics), (1, 0));
    }

    // ---- PreviewImageServiceRawKeyTests -----------------------------------------------------------------------------

    // C#: PreviewImageServiceRawKeyTests.DiskCacheIdentity_DistinguishesTheRawPreviewKindFromAStandardKeyOfTheSamePath
    #[test]
    fn disk_cache_identity_distinguishes_the_raw_preview_kind_from_a_standard_key_of_the_same_path() {
        let setup = Setup::new("w414c-e2e-raw-kind", "kind.cr2");
        let dir = setup.dir("disk");
        let service = service(&dir, ShapeDecoder::opaque(), None);
        let raw_key = service.get_current_cache_key(&setup.source).expect("key");
        assert_eq!(raw_key.source_kind(), ImageSourceKind::RAW_PREVIEW);
        load_key(&service, &setup.source, &raw_key);
        retire(&service);
        assert!(service.has_disk_cached_preview(&raw_key));
        let options = KeyOptions {
            orientation_applied: raw_key.orientation_applied(),
            backend: raw_key.backend(),
            source_kind: ImageSourceKind::STANDARD,
        };
        let standard_key = ImageCacheKey::create(
            &pr_core::io::physical::PhysicalFileSystem,
            &setup.source,
            false,
            raw_key.target_box(),
            options,
        )
        .expect("standard key");
        assert!(!service.has_disk_cached_preview(&standard_key));
    }

    // C#: PreviewImageServiceRawKeyTests.DiskCacheIdentity_OfAStandardKey_IsUnchangedByTheSourceKindField
    #[test]
    fn disk_cache_identity_of_a_standard_key_is_unchanged_by_the_source_kind_field() {
        let setup = Setup::new("w414c-e2e-raw-standard", "plain.png");
        let dir = setup.dir("disk-standard");
        let service = service(&dir, ShapeDecoder::opaque(), None);
        let key = service.get_current_cache_key(&setup.source).expect("key");
        load_key(&service, &setup.source, &key);
        retire(&service);
        let files = entries(&dir);
        assert_eq!(files.len(), 1);
        let identity = format!(
            "preview-v5-box|{}|{}|{}|{}|{}x{}|{}|{}",
            key.path(),
            key.length(),
            key.last_write_utc_ticks(),
            if key.is_original() { "True" } else { "False" },
            key.target_width(),
            key.target_height(),
            if key.orientation_applied() { "True" } else { "False" },
            "Wpf"
        );
        assert_eq!(disk_cache_identity(&key), identity);
        let expected = files[0].file_name().and_then(|n| n.to_str()).expect("name");
        assert_eq!(disk_cache_file_name(&key), expected);
    }

    // ---- PreviewDecodeBoxTests --------------------------------------------------------------------------------------

    // C#: PreviewDecodeBoxTests.DiskCacheKey_IncludesBoxHeight
    #[test]
    fn disk_cache_key_includes_box_height() {
        let setup = Setup::new("w414c-e2e-box", "a.jpg");
        let dir = setup.dir("disk");
        let with_box = |b: DecodeBox, decoder: Arc<ShapeDecoder>, metrics: Option<Arc<ReviewMetrics>>| {
            service_with(&dir, decoder, metrics, move |c| {
                c.target_decode_box = Arc::new(move || b);
            })
        };
        let writer = with_box(DecodeBox::new(2304, 1280), ShapeDecoder::opaque(), None);
        load(&writer, &setup.source);
        retire(&writer);
        assert_eq!(entries(&dir).len(), 1);
        // Same width, different height: not served the 1280-high entry.
        let taller_decoder = ShapeDecoder::opaque();
        let taller_metrics = Arc::new(ReviewMetrics::new());
        let taller = with_box(
            DecodeBox::new(2304, 1408),
            taller_decoder.clone(),
            Some(taller_metrics.clone()),
        );
        load(&taller, &setup.source);
        retire(&taller);
        assert_eq!(taller_decoder.calls(), 1);
        assert_eq!(taller_metrics.snapshot().disk_cache_hits, 0);
        // Same box: served from disk without decoding the source.
        let same_decoder = ShapeDecoder::opaque();
        let same_metrics = Arc::new(ReviewMetrics::new());
        let same = with_box(
            DecodeBox::new(2304, 1280),
            same_decoder.clone(),
            Some(same_metrics.clone()),
        );
        load(&same, &setup.source);
        retire(&same);
        assert_eq!(same_decoder.calls(), 0);
        assert_eq!(same_metrics.snapshot().disk_cache_hits, 1);
    }
}
