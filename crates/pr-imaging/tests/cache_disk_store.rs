//! Red tests of W4-09 `cache::disk_store` (port of `PhotoReview.Imaging/Caching/DiskCacheStore.cs`, C# baseline
//! d92d6aae). Every test names the C# test it ports in a `// C#:` comment. Until the porter lands the bodies, each
//! test panics at the stub's `todo!()` (D21, test-first).
//!
//! Conventions: real temp folders ([`TempRoot`]) and real file times. Tests that lock a file (share none), set a
//! Read-only attribute or set a creation time are `#[cfg(windows)]` (CI is `windows-latest`); they are not ignored.
//! Logs are local recorders that keep `(message, io::ErrorKind)` so error classes are asserted, not only text
//! (`RecordingLog` keeps only the text). The crate targets `x86_64-pc-windows-msvc` only (rust-toolchain.toml).

use std::fs::{self, File, FileTimes, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, OnceLock, PoisonError, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pr_core::abstractions::{Clock, Log};
use pr_core::sync::CancelToken;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_imaging::cache::disk_store::{
    ACCESS_TOUCH_INTERVAL, DiskCacheStore, InvalidStoreArgument, PngEncoder, STALE_TEMP_FILE_AGE,
    clear_directory_matching, delete_stale_temp_files, prune_directory, try_delete, write_atomically,
    write_pixels_atomically,
};
use pr_imaging::decode_types::{DecodedImageView, PixelLease, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, PlatformImageCodec};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_test_support::{FakeClock, TempRoot};

/// Upper bound for every wait in these tests (never a fixed sleep).
const WAIT: Duration = Duration::from_secs(10);

/// .NET ticks of 1970-01-01T00:00:00Z (C# `DateTime` ticks at the Unix epoch).
const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;

// ---- time helpers ------------------------------------------------------------------------------------------------

fn utc(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text).expect("valid literal").0
}

fn duration_ticks(d: Duration) -> i64 {
    i64::try_from(d.as_nanos() / 100).expect("duration fits in ticks")
}

fn plus(t: DateTimeUtc, d: Duration) -> DateTimeUtc {
    t.checked_add_ticks(duration_ticks(d)).expect("in range")
}

fn minus(t: DateTimeUtc, d: Duration) -> DateTimeUtc {
    t.checked_add_ticks(-duration_ticks(d)).expect("in range")
}

/// `UtcNow - age` (C# `DateTime.UtcNow - accessedAgo`).
fn ago(age: Duration) -> DateTimeUtc {
    DateTimeUtc::now()
        .checked_add_ticks(-duration_ticks(age))
        .expect("in range")
}

const fn mins(m: u64) -> Duration {
    Duration::from_secs(m * 60)
}

const fn hours(h: u64) -> Duration {
    Duration::from_secs(h * 3600)
}

/// `DateTimeUtc` as a `SystemTime` for `FileTimes` (ticks since 0001 less the Unix epoch's ticks).
#[cfg(windows)]
fn to_system_time(t: DateTimeUtc) -> SystemTime {
    let ticks = t.ticks() - UNIX_EPOCH_TICKS;
    let magnitude = ticks.unsigned_abs();
    let tick_unit = TICKS_PER_SECOND.unsigned_abs();
    let d = Duration::new(
        magnitude / tick_unit,
        u32::try_from(magnitude % tick_unit).expect("< 1e7") * 100,
    );
    if ticks >= 0 { UNIX_EPOCH + d } else { UNIX_EPOCH - d }
}

/// C# `Math.Abs((File.GetLastAccessTimeUtc(path) - expected).TotalSeconds) < 5`.
fn touched(path: &Path, expected: DateTimeUtc) -> bool {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.accessed().ok())
        .and_then(DateTimeUtc::from_system_time)
        .is_some_and(|got| (got.ticks() - expected.ticks()).abs() < 5 * TICKS_PER_SECOND)
}

// ---- file helpers ------------------------------------------------------------------------------------------------

/// Sets any of the three file times (C# `SetLastAccessTimeUtc` / `SetCreationTimeUtc` / `SetLastWriteTimeUtc`).
#[cfg(windows)]
fn stamp(path: &Path, accessed: Option<DateTimeUtc>, created: Option<DateTimeUtc>, modified: Option<DateTimeUtc>) {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("open {} for times: {e}", path.display()));
    let mut times = FileTimes::new();
    if let Some(t) = accessed {
        times = times.set_accessed(to_system_time(t));
    }
    if let Some(t) = created {
        times = times.set_created(to_system_time(t));
    }
    if let Some(t) = modified {
        times = times.set_modified(to_system_time(t));
    }
    file.set_times(times)
        .unwrap_or_else(|e| panic!("set times on {}: {e}", path.display()));
}

/// Writes `bytes` zero bytes to `dir\name` with access and creation time `accessed_ago` before now.
#[cfg(windows)]
fn write_file(dir: &Path, name: &str, bytes: usize, accessed_ago: Duration) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, vec![0u8; bytes]).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    let stamp_at = ago(accessed_ago);
    stamp(&path, Some(stamp_at), Some(stamp_at), None);
    path
}

/// A folder under `root` (created, unlike C# `TempRoot.Combine`).
fn dir_of(root: &TempRoot, name: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).expect("create test folder");
    dir
}

/// Opens `path` so no other handle can open, delete or rename it (C# `FileShare.None`).
#[cfg(windows)]
fn lock_exclusive(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap_or_else(|e| panic!("lock {}: {e}", path.display()))
}

/// Names of the regular files directly in `dir`, sorted ordinally (the golden `survivors` shape).
fn survivors(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list folder")
        .map(|entry| entry.expect("entry"))
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// ---- clock and log fakes -----------------------------------------------------------------------------------------

fn clock_at(t: DateTimeUtc) -> Arc<FakeClock> {
    Arc::new(FakeClock::at(t))
}

fn as_clock(clock: &Arc<FakeClock>) -> Option<Arc<dyn Clock>> {
    Some(Arc::clone(clock) as Arc<dyn Clock>)
}

/// Records `(message, error kind)` for every `error` call; `info`/`warn` are dropped.
#[derive(Debug, Default)]
struct KindLog {
    entries: Mutex<Vec<(String, Option<io::ErrorKind>)>>,
}

impl KindLog {
    fn entries(&self) -> Vec<(String, Option<io::ErrorKind>)> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn messages(&self) -> Vec<String> {
        self.entries().into_iter().map(|(message, _)| message).collect()
    }
}

impl Log for KindLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, _message: &str) {}

    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        let kind = error.and_then(|e| e.downcast_ref::<io::Error>()).map(io::Error::kind);
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((message.to_owned(), kind));
    }
}

/// Runs `on_error` for every `error` call (after recording the message), so a test can act from inside a pass.
struct HookLog<F: Fn(&str) + Send + Sync> {
    messages: Mutex<Vec<String>>,
    on_error: F,
}

impl<F: Fn(&str) + Send + Sync> HookLog<F> {
    fn new(on_error: F) -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
            on_error,
        }
    }

    fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl<F: Fn(&str) + Send + Sync> Log for HookLog<F> {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, _message: &str) {}

    fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message.to_owned());
        (self.on_error)(message);
    }
}

/// C# `BlockingLog`: the first `Delete failed` error signals `entered`, then waits (at most 10 s) for `release`.
struct BlockingLog {
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
    /// Latched once the first wait ended (C# `_release` is a manual-reset event).
    released: Mutex<bool>,
}

impl Log for BlockingLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, _message: &str) {}

    fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
        if !message.starts_with("Delete failed") {
            return;
        }
        let _ = self.entered.lock().unwrap_or_else(PoisonError::into_inner).send(());
        // C# `ManualResetEventSlim`: once released, it stays released, so the extra pass's failed delete does not
        // block again (a one-shot channel message would make that pass wait the full 10 s bound).
        let mut released = self.released.lock().unwrap_or_else(PoisonError::into_inner);
        if !*released {
            let _ = self
                .release
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .recv_timeout(WAIT);
            *released = true;
        }
    }
}

// ---- fakes for the atomic write ----------------------------------------------------------------------------------

/// A decoded image whose platform image is not pixels (C# `NonBitmapImage`: a foreign platform object).
struct ForeignImage {
    platform: PlatformImage,
}

impl ForeignImage {
    fn new() -> Self {
        Self {
            platform: PlatformImage::Opaque(Arc::new(0u8)),
        }
    }
}

impl DecodedImageView for ForeignImage {
    fn pixel_width(&self) -> i32 {
        1
    }

    fn pixel_height(&self) -> i32 {
        1
    }

    fn downscaled(&self) -> bool {
        false
    }

    fn orientation(&self) -> i32 {
        1
    }

    fn estimated_bytes(&self) -> i64 {
        4
    }

    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
}

/// A codec that refuses every platform image as foreign (`ArgumentException` in C#).
struct RejectingCodec;

impl PlatformImageCodec for RejectingCodec {
    fn name(&self) -> &'static str {
        "rejecting"
    }

    fn image_from_pixels(&self, _pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        Err(DecodeError::InvalidArgument {
            message: "rejecting codec".to_owned(),
        })
    }

    fn to_pixels<'a>(&self, _platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        Err(DecodeError::InvalidArgument {
            message: "foreign platform image".to_owned(),
        })
    }
}

/// A PNG encoder that writes fixed bytes, so the final file's content is known (the real WIC encoder is W4-11e's).
struct FixedEncoder {
    bytes: Vec<u8>,
}

impl PngEncoder for FixedEncoder {
    fn encode_png(&self, _pixels: &PixelBuffer, output: &mut dyn Write) -> Result<(), DecodeError> {
        output.write_all(&self.bytes).map_err(DecodeError::Io)
    }
}

fn test_pixels(width: i32, height: i32) -> PixelBuffer {
    PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).expect("allocate test pixels")
}

// ---- DiskCacheStoreTests.cs --------------------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn prune_directory_evicts_least_recently_used_first() {
    // C#: DiskCacheStoreTests.PruneDirectoryEvictsLeastRecentlyUsedFirst
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "prune");
    let oldest = write_file(&dir, "oldest.png", 100, mins(30));
    let middle = write_file(&dir, "middle.png", 100, mins(20));
    let newest = write_file(&dir, "newest.png", 100, mins(10));

    let _ = prune_directory(&dir, "*.png", 150, None);

    assert!(!oldest.exists() && !middle.exists() && newest.exists());
}

#[cfg(windows)]
#[test]
fn prune_directory_leaves_files_untouched_within_quota() {
    // C#: DiskCacheStoreTests.PruneDirectoryLeavesFilesUntouchedWithinQuota
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "within-quota");
    let a = write_file(&dir, "a.png", 50, mins(5));
    let b = write_file(&dir, "b.png", 50, mins(1));

    let _ = prune_directory(&dir, "*.png", 1_000, None);

    assert!(a.exists() && b.exists());
}

#[cfg(windows)]
#[test]
fn prune_directory_tolerates_missing_directory() {
    // C#: DiskCacheStoreTests.PruneDirectoryToleratesMissingDirectory (also: pruning does not create the folder)
    let root = TempRoot::new("disk-cache-store");
    let missing = root.join("does-not-exist");

    let result = prune_directory(&missing, "*.png", 10, None);

    assert!(result.is_ok(), "a missing folder is not an error: {result:?}");
    assert!(
        !missing.exists(),
        "pruning must not create the directory it was asked to trim"
    );
}

#[cfg(windows)]
#[test]
#[ignore = "needs W4-08a: cache::atomic_file::write_atomic is still todo!() on integration/wave-4"]
fn write_atomically_produces_only_the_final_file() {
    // C#: DiskCacheStoreTests.WriteAtomicallyAsyncProducesOnlyTheFinalFile
    // The C# test decodes a real PNG; here a fake encoder writes fixed bytes and the pixel overload is used. The real
    // WIC twin is W4-11e's.
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "write-atomic");
    let cache_path = dir.join("out.png");
    let pixels = test_pixels(4, 4);
    let encoder = FixedEncoder {
        bytes: vec![1, 2, 3, 4],
    };

    let result = write_pixels_atomically(&pixels, &encoder, &cache_path, None, &CancelToken::new());

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(survivors(&dir), vec!["out.png".to_owned()]);
    assert!(fs::metadata(&cache_path).expect("final file").len() > 0);
}

#[cfg(windows)]
#[test]
fn clear_directory_removes_all_matching_files() {
    // C#: DiskCacheStoreTests.ClearDirectoryRemovesAllMatchingFiles (static ClearDirectory)
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "clear");
    let a = write_file(&dir, "a.png", 10, Duration::ZERO);
    let b = write_file(&dir, "b.png", 10, Duration::ZERO);

    let _ = clear_directory_matching(&dir, "*.png", None);

    assert!(!a.exists() && !b.exists());
}

#[cfg(windows)]
#[test]
fn instance_schedule_prune_and_wait_for_prune_complete() {
    // C#: DiskCacheStoreTests.InstanceSchedulePruneAndWaitForPruneAsync
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "instance-prune");
    let oldest = write_file(&dir, "oldest.png", 100, mins(30));
    let newest = write_file(&dir, "newest.png", 100, mins(10));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 150, None, None).expect("store");

    store.schedule_prune();
    let finished = store.wait_for_prune(WAIT);

    assert!(finished);
    assert!(!oldest.exists() && newest.exists());
}

#[cfg(windows)]
#[test]
fn prune_pass_under_quota_skips_full_scan() {
    // C#: DiskCacheStoreTests.PrunePassUnderQuotaSkipsFullScan (IMG-10)
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "incremental-under");
    write_file(&dir, "a.png", 100, mins(30));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, None).expect("store");

    store.schedule_prune(); // size unknown: one full scan establishes the tracked total
    assert!(store.wait_for_prune(WAIT));
    assert_eq!(store.full_scan_count(), 1);

    for i in 0..5 {
        let path = write_file(&dir, &format!("n{i}.png"), 100, Duration::ZERO);
        store.note_written(&path);
        store.schedule_prune();
        assert!(store.wait_for_prune(WAIT));
    }

    assert_eq!(
        store.full_scan_count(),
        1,
        "600 bytes tracked <= 1000: no re-enumeration"
    );
}

#[cfg(windows)]
#[test]
fn noted_writes_over_quota_evict_oldest() {
    // C#: DiskCacheStoreTests.NotedWritesOverQuotaEvictOldest (IMG-10)
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "incremental-over");
    let oldest = write_file(&dir, "oldest.png", 100, mins(30));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 250, None, None).expect("store");
    store.schedule_prune();
    assert!(store.wait_for_prune(WAIT));
    assert!(oldest.exists());

    let mid = write_file(&dir, "mid.png", 100, mins(20));
    store.note_written(&mid);
    let newest = write_file(&dir, "newest.png", 100, mins(10));
    store.note_written(&newest); // 300 tracked > 250
    store.schedule_prune();
    assert!(store.wait_for_prune(WAIT));

    assert!(!oldest.exists() && mid.exists() && newest.exists());
    assert_eq!(store.full_scan_count(), 2);
}

#[cfg(windows)]
#[test]
fn instance_clear_directory_removes_files() {
    // C#: DiskCacheStoreTests.InstanceClearDirectoryRemovesFiles
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "instance-clear");
    let a = write_file(&dir, "a.png", 10, Duration::ZERO);
    let b = write_file(&dir, "b.png", 10, Duration::ZERO);
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, None).expect("store");

    let result = store.clear_directory();

    assert!(result.is_ok(), "{result:?}");
    assert!(!a.exists() && !b.exists());
}

#[cfg(windows)]
#[test]
fn note_accessed_protects_recently_read_entry_from_prune() {
    // C#: DiskCacheStoreTests.NoteAccessedProtectsRecentlyReadEntryFromPrune (LRU, not FIFO)
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "lru-touch");
    let clock = clock_at(DateTimeUtc::now());
    let oldest = write_file(&dir, "oldest.png", 100, mins(30));
    let middle = write_file(&dir, "middle.png", 100, mins(20));
    let newest = write_file(&dir, "newest.png", 100, mins(10));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 200, None, as_clock(&clock)).expect("store");

    store.note_accessed(&oldest); // a cache hit on the oldest entry
    let _ = prune_directory(&dir, "*.png", 200, None);

    assert!(oldest.exists() && !middle.exists() && newest.exists());
}

#[cfg(windows)]
#[test]
fn note_accessed_is_throttled_per_entry() {
    // C#: DiskCacheStoreTests.NoteAccessedIsThrottledPerEntry
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "lru-throttle");
    let start = DateTimeUtc::now();
    let clock = clock_at(start);
    let path = write_file(&dir, "a.png", 10, hours(1));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, as_clock(&clock)).expect("store");

    store.note_accessed(&path);
    assert!(touched(&path, start), "first hit stamps the clock time");
    let backdated = minus(start, hours(2));
    stamp(&path, Some(backdated), None, None);

    clock.set(plus(start, ACCESS_TOUCH_INTERVAL - Duration::from_secs(1)));
    store.note_accessed(&path); // still inside the interval: no write
    assert!(
        touched(&path, backdated),
        "a repeat inside the interval must not touch the file"
    );

    let after = plus(start, ACCESS_TOUCH_INTERVAL + Duration::from_secs(1));
    clock.set(after);
    store.note_accessed(&path); // interval elapsed: touched again
    assert!(touched(&path, after), "touched again once the interval has elapsed");
}

#[test]
fn note_accessed_tolerates_missing_file() {
    // C#: DiskCacheStoreTests.NoteAccessedToleratesMissingFile
    let root = TempRoot::new("disk-cache-store");
    let dir = dir_of(&root, "lru-missing");
    let store = DiskCacheStore::new(dir.clone(), "*.png", 10, None, None).expect("store");

    store.note_accessed(&dir.join("gone.png"));
}

// ---- DiskCacheStoreGapTests.cs -----------------------------------------------------------------------------------

#[cfg(windows)]
fn temp_file(dir: &Path, name: &str, age: Duration) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, [1u8, 2, 3]).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    stamp(&path, None, None, Some(ago(age)));
    path
}

#[cfg(windows)]
#[test]
fn delete_stale_temp_files_deletes_only_old_temp_files() {
    // C#: DiskCacheStoreGapTests.DeleteStaleTempFiles_DeletesOnlyOldTempFiles
    let root = TempRoot::new("DiskCacheGap");
    let dir = dir_of(&root, "temp");
    let old = temp_file(&dir, "a.png.1.tmp", STALE_TEMP_FILE_AGE + mins(5));
    let fresh = temp_file(&dir, "b.png.2.tmp", mins(1));
    let old_not_temp = temp_file(&dir, "c.pv4", STALE_TEMP_FILE_AGE + hours(1));

    let removed = delete_stale_temp_files(&dir, DateTimeUtc::now(), 100, None);

    assert_eq!(removed, 1);
    assert!(!old.exists());
    assert!(fresh.exists());
    assert!(old_not_temp.exists());
}

#[cfg(windows)]
#[test]
fn delete_stale_temp_files_honours_max_files() {
    // C#: DiskCacheStoreGapTests.DeleteStaleTempFiles_HonoursMaxFiles
    let root = TempRoot::new("DiskCacheGap");
    let dir = dir_of(&root, "cap");
    for i in 0..5 {
        temp_file(&dir, &format!("f{i}.tmp"), hours(1));
    }

    let removed = delete_stale_temp_files(&dir, DateTimeUtc::now(), 2, None);

    assert_eq!(removed, 2);
    assert_eq!(survivors(&dir).len(), 3);
}

#[cfg(windows)]
#[test]
fn delete_stale_temp_files_uses_the_supplied_now() {
    // C#: DiskCacheStoreGapTests.DeleteStaleTempFiles_UsesTheSuppliedNow
    let root = TempRoot::new("DiskCacheGap");
    let dir = dir_of(&root, "clock");
    let path = temp_file(&dir, "a.tmp", mins(1));

    assert_eq!(delete_stale_temp_files(&dir, DateTimeUtc::now(), 10, None), 0);
    assert!(path.exists());
    let later = plus(DateTimeUtc::now(), STALE_TEMP_FILE_AGE + mins(2));
    assert_eq!(delete_stale_temp_files(&dir, later, 10, None), 1);
    assert!(!path.exists());
}

#[test]
fn delete_stale_temp_files_missing_directory_returns_zero() {
    // C#: DiskCacheStoreGapTests.DeleteStaleTempFiles_MissingDirectory_ReturnsZero
    let root = TempRoot::new("DiskCacheGap");

    assert_eq!(
        delete_stale_temp_files(&root.join("nope"), DateTimeUtc::now(), 10, None),
        0
    );
}

#[cfg(windows)]
#[test]
fn schedule_prune_during_running_pass_runs_exactly_one_extra_pass() {
    // C#: DiskCacheStoreGapTests.SchedulePrune_DuringRunningPass_RunsExactlyOneExtraPass
    // The over-quota file cannot be deleted while open exclusively, so its failed delete logs from inside the pass,
    // where BlockingLog blocks until released.
    let root = TempRoot::new("DiskCacheGap");
    let dir = dir_of(&root, "coalesce");
    let locked = dir.join("locked.pv4");
    fs::write(&locked, vec![0u8; 100]).expect("write locked file");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let log = Arc::new(BlockingLog {
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release_rx),
        released: Mutex::new(false),
    });
    let store = DiskCacheStore::new(dir.clone(), "*.pv4", 10, Some(log.clone() as Arc<dyn Log>), None).expect("store");
    let _exclusive = lock_exclusive(&locked);

    store.schedule_prune();
    entered_rx
        .recv_timeout(WAIT)
        .expect("the first prune pass never started");
    for _ in 0..5 {
        store.schedule_prune(); // all coalesce into one pending request
    }
    release_tx.send(()).expect("release the blocked pass");

    assert!(store.wait_for_prune(WAIT));
    assert_eq!(store.full_scan_count(), 2);
}

// ---- DiskCacheStoreMutationTests.cs ------------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn note_accessed_uses_injected_clock() {
    // C#: DiskCacheStoreMutationTests.NoteAccessed_UsesInjectedClock
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "clock");
    let path = write_file(&dir, "a.png", 10, hours(5));
    let t0 = utc("2020-01-02T03:04:05Z");
    let clock = clock_at(t0);
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, as_clock(&clock)).expect("store");

    store.note_accessed(&path);

    assert!(touched(&path, t0), "the entry was not stamped with the injected clock");
}

#[cfg(windows)]
#[test]
fn note_accessed_throttle_boundaries() {
    // C#: DiskCacheStoreMutationTests.NoteAccessed_ThrottleBoundaries (inside the interval skipped, exactly one interval touched)
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "throttle");
    let path = write_file(&dir, "a.png", 10, hours(5));
    let t0 = utc("2020-01-02T03:04:05Z");
    let sentinel = utc("2010-06-07T08:09:10Z");
    let clock = clock_at(t0);
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, as_clock(&clock)).expect("store");

    store.note_accessed(&path);
    assert!(touched(&path, t0));

    stamp(&path, Some(sentinel), None, None);
    clock.set(plus(t0, ACCESS_TOUCH_INTERVAL - Duration::from_secs(1)));
    store.note_accessed(&path);
    assert!(touched(&path, sentinel), "touched again inside the throttle interval");

    let at_interval = plus(t0, ACCESS_TOUCH_INTERVAL); // exactly one interval after the last touch
    clock.set(at_interval);
    store.note_accessed(&path);
    assert!(
        touched(&path, at_interval),
        "not touched once a full interval has elapsed"
    );
}

#[cfg(windows)]
#[test]
fn note_accessed_throttle_table_is_cleared_exactly_at_the_cap() {
    // C#: DiskCacheStoreMutationTests.NoteAccessed_ThrottleTableIsClearedExactlyAtTheCap
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "cap");
    let a = write_file(&dir, "a.png", 10, hours(5));
    let t0 = utc("2020-01-02T03:04:05Z");
    let sentinel = utc("2010-06-07T08:09:10Z");
    let clock = clock_at(t0);
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, as_clock(&clock)).expect("store");

    store.note_accessed(&a);
    stamp(&a, Some(sentinel), None, None);
    store.note_accessed(&a);
    assert!(
        touched(&a, sentinel),
        "a throttled repeat must not clear the table or touch the file"
    );
    store.note_accessed(&dir.join("other.png")); // a second entry well below the cap must not make the table forget "a"
    store.note_accessed(&a);
    assert!(
        touched(&a, sentinel),
        "the table was cleared long before reaching its cap"
    );

    // 8191 further distinct entries (missing files: the touch fails quietly, the entry is still tracked): exactly 8192.
    for i in 0..8191 {
        store.note_accessed(&dir.join(format!("f{i}.png")));
    }
    store.note_accessed(&dir.join("overflow.png")); // table full: cleared before this one is added

    store.note_accessed(&a);
    assert!(touched(&a, t0), "the table was not cleared when it reached its cap");
}

#[cfg(windows)]
#[test]
fn note_written_after_empty_scan_is_tracked() {
    // C#: DiskCacheStoreMutationTests.NoteWritten_AfterEmptyScan_IsTracked
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "empty-then-write");
    let store = DiskCacheStore::new(dir.clone(), "*.png", 50, None, None).expect("store");
    store.run_prune_pass().expect("empty pass");
    assert_eq!(store.full_scan_count(), 1);

    let big = write_file(&dir, "big.png", 100, Duration::ZERO);
    store.note_written(&big);
    store.run_prune_pass().expect("pass after the write");

    assert_eq!(store.full_scan_count(), 2);
    assert!(!big.exists());
}

#[cfg(windows)]
#[test]
fn note_written_when_size_unknown_still_forces_a_full_scan() {
    // C#: DiskCacheStoreMutationTests.NoteWritten_WhenSizeUnknown_StillForcesAFullScan
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "unknown-size");
    write_file(&dir, "a.png", 100, mins(30));
    write_file(&dir, "b.png", 100, mins(20));
    write_file(&dir, "c.png", 100, mins(10));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 150, None, None).expect("store");
    let d = write_file(&dir, "d.png", 10, Duration::ZERO);
    store.note_written(&d);

    store.run_prune_pass().expect("pass");

    assert_eq!(store.full_scan_count(), 1);
    let total: u64 = fs::read_dir(&dir)
        .expect("list")
        .map(|e| e.expect("entry").metadata().expect("metadata").len())
        .sum();
    assert!(total <= 150, "{total}");
}

#[cfg(windows)]
#[test]
fn run_prune_pass_empty_directory_next_pass_skipped() {
    // C#: DiskCacheStoreMutationTests.RunPrunePass_EmptyDirectory_NextPassSkipped
    let root = TempRoot::new("DiskCacheMut");
    let store = DiskCacheStore::new(dir_of(&root, "empty"), "*.png", 100, None, None).expect("store");

    store.run_prune_pass().expect("pass");
    store.run_prune_pass().expect("pass");

    assert_eq!(store.full_scan_count(), 1);
}

#[cfg(windows)]
#[test]
fn run_prune_pass_exactly_at_quota_next_pass_skipped() {
    // C#: DiskCacheStoreMutationTests.RunPrunePass_ExactlyAtQuota_NextPassSkipped
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "at-quota");
    let file = write_file(&dir, "a.png", 100, mins(5));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 100, None, None).expect("store");

    store.run_prune_pass().expect("pass");
    store.run_prune_pass().expect("pass");

    assert_eq!(store.full_scan_count(), 1);
    assert!(file.exists());
}

#[cfg(windows)]
#[test]
fn run_prune_pass_missing_directory_every_pass_scans() {
    // C#: DiskCacheStoreMutationTests.RunPrunePass_MissingDirectory_EveryPassScans
    let root = TempRoot::new("DiskCacheMut");
    let store = DiskCacheStore::new(root.join("not-there"), "*.png", 100, None, None).expect("store");

    let _ = store.run_prune_pass();
    let _ = store.run_prune_pass();
    let _ = store.run_prune_pass();

    assert_eq!(store.full_scan_count(), 3);
}

#[cfg(windows)]
#[test]
fn run_prune_pass_forces_a_full_scan_every_256th_pass() {
    // C#: DiskCacheStoreMutationTests.RunPrunePass_ForcesAFullScanEvery256thPass
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "forced");
    write_file(&dir, "a.png", 10, mins(5));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, None).expect("store");
    store.run_prune_pass().expect("pass");
    assert_eq!(store.full_scan_count(), 1);

    for _ in 0..255 {
        store.run_prune_pass().expect("pass");
    }
    assert_eq!(store.full_scan_count(), 1);

    store.run_prune_pass().expect("pass"); // the 256th pass since the scan
    assert_eq!(store.full_scan_count(), 2);

    for _ in 0..255 {
        store.run_prune_pass().expect("pass");
    }
    assert_eq!(store.full_scan_count(), 2);
    store.run_prune_pass().expect("pass");
    assert_eq!(store.full_scan_count(), 3);
}

#[cfg(windows)]
#[test]
fn run_prune_pass_writes_noted_during_scan_are_added_to_the_tracked_size() {
    // C#: DiskCacheStoreMutationTests.RunPrunePass_WritesNotedDuringScan_AreAddedToTheTrackedSize
    // The log calls note_written from inside the scan (when the locked delete fails): the store is reached through a
    // OnceLock filled after construction.
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "noted-during");
    let extra_dir = dir_of(&root, "extra");
    let locked = write_file(&dir, "a-locked.png", 60, mins(30)); // oldest: pruned first, but cannot be deleted
    write_file(&dir, "b.png", 60, mins(20));
    let extra = write_file(&extra_dir, "x.png", 50, Duration::ZERO);
    let slot: Arc<OnceLock<Arc<DiskCacheStore>>> = Arc::new(OnceLock::new());
    let hook_slot = Arc::clone(&slot);
    let extra_for_hook = extra.clone();
    let log = Arc::new(HookLog::new(move |message: &str| {
        if message.starts_with("Delete failed") {
            if let Some(store) = hook_slot.get() {
                store.note_written(&extra_for_hook);
            }
        }
    }));
    let store = Arc::new(
        DiskCacheStore::new(dir.clone(), "*.png", 100, Some(log.clone() as Arc<dyn Log>), None).expect("store"),
    );
    slot.set(Arc::clone(&store)).expect("store set once");

    {
        let _exclusive = lock_exclusive(&locked);
        store.run_prune_pass().expect("pass");
    }

    // scan left 60 (b deleted, a locked) + 50 noted during it = 110 > 100: the next pass must scan again.
    assert!(log.messages().iter().any(|m| m.starts_with("Delete failed")));
    assert_eq!(store.full_scan_count(), 1);
    store.run_prune_pass().expect("second pass");
    assert_eq!(store.full_scan_count(), 2);
}

#[cfg(windows)]
#[test]
fn clear_directory_forgets_tracked_size() {
    // C#: DiskCacheStoreMutationTests.ClearDirectory_ForgetsTrackedSize
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "clear-forget");
    write_file(&dir, "a.png", 100, mins(5));
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, None).expect("store");
    store.run_prune_pass().expect("pass");
    store.run_prune_pass().expect("pass");
    assert_eq!(store.full_scan_count(), 1);

    store.clear_directory().expect("clear");
    store.run_prune_pass().expect("pass");

    assert_eq!(store.full_scan_count(), 2);
}

#[cfg(windows)]
#[test]
fn prune_directory_total_equals_quota_deletes_nothing() {
    // C#: DiskCacheStoreMutationTests.PruneDirectory_TotalEqualsQuota_DeletesNothing
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "equal");
    let a = write_file(&dir, "a.png", 100, mins(20));
    let b = write_file(&dir, "b.png", 100, mins(10));

    let remaining = prune_directory(&dir, "*.png", 200, None).expect("prune");

    assert_eq!(remaining, 200);
    assert!(a.exists() && b.exists());
}

#[cfg(windows)]
#[test]
fn prune_directory_returns_remaining_bytes() {
    // C#: DiskCacheStoreMutationTests.PruneDirectory_ReturnsRemainingBytes (a file that cannot be deleted stays counted)
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "remaining");
    let oldest = write_file(&dir, "oldest.png", 100, mins(30));
    write_file(&dir, "middle.png", 100, mins(20));
    write_file(&dir, "newest.png", 100, mins(10));

    assert_eq!(prune_directory(&dir, "*.png", 150, None).expect("prune"), 100);

    let dir2 = dir_of(&root, "remaining-locked");
    let locked = write_file(&dir2, "locked.png", 100, mins(30));
    write_file(&dir2, "other.png", 100, mins(10));
    {
        let _exclusive = lock_exclusive(&locked);
        assert_eq!(prune_directory(&dir2, "*.png", 150, None).expect("prune"), 100);
        assert_eq!(prune_directory(&dir2, "*.png", 10, None).expect("prune"), 100); // the locked one stays, counted
    }
    assert!(!oldest.exists());
}

#[cfg(windows)]
#[test]
fn try_delete_locked_file_returns_false_and_logs() {
    // C#: DiskCacheStoreMutationTests.TryDelete_LockedFile_ReturnsFalseAndLogs
    let root = TempRoot::new("DiskCacheMut");
    let path = write_file(&dir_of(&root, "trydel"), "a.png", 10, Duration::ZERO);
    let log = KindLog::default();

    {
        let _exclusive = lock_exclusive(&path);
        assert!(!try_delete(&path, Some(&log)));
    }
    let entries = log.entries();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert!(entries[0].0.starts_with("Delete failed"), "{entries:?}");
    assert!(path.exists());

    assert!(try_delete(&path, Some(&log)));
    assert!(!path.exists());
    assert!(!try_delete(&path, Some(&log)), "already gone: false, no new error");
    assert_eq!(log.entries().len(), 1);
}

#[cfg(windows)]
#[test]
fn try_delete_read_only_file_returns_false_and_logs() {
    // C#: DiskCacheStoreMutationTests.TryDelete_ReadOnlyFile_ReturnsFalseAndLogs (the C# exception type is asserted here
    // as the logged error's kind: PermissionDenied)
    let root = TempRoot::new("DiskCacheMut");
    let path = write_file(&dir_of(&root, "readonly"), "a.png", 10, Duration::ZERO);
    let mut perms = fs::metadata(&path).expect("metadata").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms).expect("set read-only");
    let log = KindLog::default();

    let deleted = try_delete(&path, Some(&log));

    // Undo the attribute before any assert can panic, so the temp folder cleans up (Windows-only folder: the Unix
    // lint about world-writable files does not apply).
    let mut perms = fs::metadata(&path).expect("metadata").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).expect("clear read-only");

    assert!(!deleted);
    let entries = log.entries();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].1, Some(io::ErrorKind::PermissionDenied), "{entries:?}");
    assert!(path.exists());
}

/// Restores the icacls Deny ACE on drop, so a panicking test cannot leave the temp folder denied.
struct DenyListGuard {
    dir: PathBuf,
    sid: String,
}

#[cfg(windows)]
impl Drop for DenyListGuard {
    fn drop(&mut self) {
        let _ = Command::new("icacls")
            .arg(&self.dir)
            .arg("/remove:d")
            .arg(format!("*{}", self.sid))
            .status();
    }
}

#[cfg(windows)]
fn current_user_sid() -> String {
    let output = Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .expect("whoami");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    // CSV row: "DOMAIN\\user","S-1-5-21-..."
    text.split(',')
        .nth(1)
        .map(|field| field.trim().trim_matches('"').to_owned())
        .expect("SID column in whoami output")
}

#[cfg(windows)]
#[test]
#[ignore = "native: denies ListDirectory on a temp folder with icacls"]
fn delete_stale_temp_files_unlistable_directory_logs_and_returns_zero() {
    // C#: DiskCacheStoreMutationTests.DeleteStaleTempFiles_UnlistableDirectory_LogsAndReturnsZero
    // Changes a real ACL (Deny ListDirectory for the CURRENT user, via icacls); the drop guard restores it.
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "denied");
    let sid = current_user_sid();
    let deny = Command::new("icacls")
        .arg(&dir)
        .arg("/deny")
        .arg(format!("*{sid}:(RD)"))
        .status()
        .expect("icacls deny");
    assert!(deny.success(), "icacls /deny failed");
    let _restore = DenyListGuard {
        dir: dir.clone(),
        sid: sid.clone(),
    };
    let log = KindLog::default();

    let removed = delete_stale_temp_files(&dir, DateTimeUtc::now(), 10, Some(&log));

    assert_eq!(removed, 0);
    assert!(
        log.messages()
            .iter()
            .any(|m| m.contains("Stale temp file cleanup failed")),
        "{:?}",
        log.messages()
    );
}

#[cfg(windows)]
#[test]
fn write_atomically_invalid_image_throws() {
    // C#: DiskCacheStoreMutationTests.WriteAtomicallyAsync_InvalidImage_Throws (the null-image half has no Rust twin)
    let root = TempRoot::new("DiskCacheMut");
    let path = root.join("x").join("out.png");
    let encoder = FixedEncoder { bytes: vec![1] };

    let result = write_atomically(
        &ForeignImage::new(),
        &RejectingCodec,
        &encoder,
        &path,
        None,
        &CancelToken::new(),
    );

    assert!(matches!(result, Err(DecodeError::InvalidArgument { .. })), "{result:?}");
    assert!(!path.exists());
    assert!(
        !root.join("x").exists(),
        "nothing is created before the codec accepts the image"
    );
}

#[cfg(windows)]
#[test]
#[ignore = "needs W4-08a: cache::atomic_file::write_atomic is still todo!() on integration/wave-4"]
fn write_atomically_writes_a_decodable_png() {
    // C#: DiskCacheStoreMutationTests.WriteAtomicallyAsync_WritesADecodablePng
    // The decode-back half (PngBitmapDecoder) is W4-11e's; here the fake encoder's bytes must be the file's content
    // and the only file left.
    let root = TempRoot::new("DiskCacheMut");
    let dir = dir_of(&root, "png");
    let path = dir.join("out.png");
    let bytes = vec![0x89, b'P', b'N', b'G', 0, 1, 2, 3];
    let encoder = FixedEncoder { bytes: bytes.clone() };

    let result = write_pixels_atomically(&test_pixels(5, 3), &encoder, &path, None, &CancelToken::new());

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(fs::read(&path).expect("final file"), bytes);
    assert_eq!(survivors(&dir), vec!["out.png".to_owned()]);
}

// ---- DiskCacheStorePruneRaceTests.cs -----------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn schedule_prune_requested_inside_exit_window_is_serviced() {
    // C#: DiskCacheStorePruneRaceTests.SchedulePrune_RequestedInsideExitWindow_IsServiced
    // The hook (after the loop's last pending check, before the scheduled flag clears) writes an over-quota file, notes
    // it and requests a prune. The hook captures a clone of the Arc<DiskCacheStore>.
    let root = TempRoot::new("DiskCachePruneRace");
    let dir = dir_of(&root, "race");
    let late = dir.join("late.pv4");
    let store = Arc::new(DiskCacheStore::new(dir.clone(), "*.pv4", 10, None, None).expect("store"));
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let hook_store = Arc::clone(&store);
    let hook_late = late.clone();
    let hook_counter = Arc::clone(&hook_calls);
    let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if hook_counter.fetch_add(1, Ordering::SeqCst) != 0 {
            return;
        }
        // Over quota and noted, so the second pass cannot take the tracked-bytes shortcut.
        fs::write(&hook_late, vec![0u8; 100]).expect("write late file");
        hook_store.note_written(&hook_late);
        hook_store.schedule_prune();
    });
    store.set_before_clear_prune_scheduled_for_tests(Some(hook));

    store.schedule_prune();
    let finished = store.wait_for_prune(WAIT);
    store.set_before_clear_prune_scheduled_for_tests(None); // break the store <-> hook cycle

    assert!(
        finished,
        "the scheduled flag never cleared (request made in the exit window was dropped)"
    );
    assert_eq!(store.full_scan_count(), 2);
    assert!(!late.exists());
}

// ---- DiskCacheStoreRobustnessTests.cs ----------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn unexpected_exception_in_prune_pass_does_not_wedge_the_store() {
    // C#: DiskCacheStoreRobustnessTests.UnexpectedExceptionInPrunePass_DoesNotWedgeTheStore
    // A NUL in the pattern makes the listing fail with InvalidInput (C# ArgumentException, not an I/O error).
    let root = TempRoot::new("DiskCacheRobust");
    let dir = dir_of(&root, "wedge");
    let log = Arc::new(KindLog::default());
    let store = DiskCacheStore::new(dir.clone(), "a\0b", 1, Some(log.clone() as Arc<dyn Log>), None).expect("store");

    store.schedule_prune();
    let finished = store.wait_for_prune(Duration::from_secs(3));

    assert!(
        finished,
        "the prune machinery stayed 'scheduled' forever after one unexpected exception"
    );
    assert!(
        log.entries()
            .iter()
            .any(|(m, k)| m.starts_with("Prune failed") && *k == Some(io::ErrorKind::InvalidInput)),
        "{:?}",
        log.entries()
    );

    // A second request must start a fresh pass (failing the same, logged way), not be swallowed as "pending".
    store.schedule_prune();
    assert!(store.wait_for_prune(Duration::from_secs(3)));
    assert!(log.messages().iter().filter(|m| m.starts_with("Prune failed")).count() >= 2);
}

#[cfg(windows)]
#[test]
fn missing_directory_finishes_quietly() {
    // C#: DiskCacheStoreRobustnessTests.MissingDirectory_FinishesQuietly
    let root = TempRoot::new("DiskCacheRobust");
    let log = Arc::new(KindLog::default());
    let store = DiskCacheStore::new(
        root.join("does-not-exist"),
        "*.pv4",
        1,
        Some(log.clone() as Arc<dyn Log>),
        None,
    )
    .expect("store");

    store.schedule_prune();

    assert!(store.wait_for_prune(Duration::from_secs(3)));
    assert!(log.entries().is_empty(), "{:?}", log.entries());
}

#[cfg(windows)]
#[test]
fn concurrent_schedule_prune_burst_ends_idle_and_within_quota() {
    // C#: DiskCacheStoreRobustnessTests.ConcurrentSchedulePrune_BurstEndsIdleAndWithinQuota
    let root = TempRoot::new("DiskCacheRobust");
    let dir = dir_of(&root, "burst");
    for i in 0..20 {
        let path = dir.join(format!("f{i:02}.pv4"));
        fs::write(&path, vec![0u8; 100]).expect("write");
        let age = if i >= 60 { Duration::ZERO } else { mins(60 - i) };
        stamp(&path, Some(ago(age)), None, None);
    }
    let store = DiskCacheStore::new(dir.clone(), "*.pv4", 500, None, None).expect("store");
    let start = Barrier::new(8);

    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                start.wait();
                for _ in 0..50 {
                    store.schedule_prune();
                }
            });
        }
    });

    assert!(store.wait_for_prune(WAIT));
    let total: u64 = fs::read_dir(&dir)
        .expect("list")
        .map(|e| e.expect("entry").metadata().expect("metadata").len())
        .sum();
    assert!(total <= 500, "{total}");
}

// ---- DiskCacheStoreTouchCapGapTests.cs ---------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn note_accessed_clears_when_the_table_holds_exactly_the_cap() {
    // C#: DiskCacheStoreTouchCapGapTests.NoteAccessed_ClearsWhenTheTableHoldsExactlyTheCap
    let root = TempRoot::new("DiskCacheTouchCap");
    let dir = dir_of(&root, "cap-exact");
    let a = dir.join("a.png");
    fs::write(&a, [0u8; 10]).expect("write");
    let t0 = utc("2020-01-02T03:04:05Z");
    let sentinel = utc("2010-06-07T08:09:10Z");
    let clock = clock_at(t0);
    let store = DiskCacheStore::new(dir.clone(), "*.png", 1_000, None, as_clock(&clock)).expect("store");

    store.note_accessed(&a); // tracked: 1
    for i in 0..8191 {
        store.note_accessed(&dir.join(format!("f{i}.png"))); // missing files: tracked: 8192
    }
    stamp(&a, Some(sentinel), None, None);
    store.note_accessed(&a); // still throttled (count 8192, no new entry)
    assert!(touched(&a, sentinel), "a throttled repeat must not touch the file");

    store.note_accessed(&dir.join("overflow.png")); // the table holds exactly its cap: cleared before this one is added
    store.note_accessed(&a);

    assert!(
        touched(&a, t0),
        "the table was not cleared when it held exactly its cap"
    );
}

// ---- WicCacheImageIoTests.cs / Wp06BridgeSplitTests.cs (IsCacheEntryFailure lines only) --------------------------

#[test]
fn empty_or_garbage_payload_is_entry_failure() {
    // C#: WicCacheImageIoTests.EmptyOrGarbagePayload_IsEntryFailure (the WIC decode halves are W4-11e's; here the
    // classification of the two failures it expects: InvalidDataException and a COM error)
    let empty = DecodeError::InvalidData {
        message: "empty payload".to_owned(),
    };
    let garbage = DecodeError::Com {
        hresult: 0x88982F60_u32 as i32,
        message: "garbage payload".to_owned(),
    };

    assert!(pr_imaging::cache::disk_store::is_cache_entry_failure(&empty));
    assert!(pr_imaging::cache::disk_store::is_cache_entry_failure(&garbage));
}

#[test]
fn is_out_of_memory_only_for_e_outofmemory() {
    // C#: WicCacheImageIoTests.IsOutOfMemory_OnlyForEOutOfMemory (the `InsufficientMemoryException => false` line only)
    let insufficient = DecodeError::OutOfMemory {
        message: "insufficient memory".to_owned(),
    };

    assert!(!pr_imaging::cache::disk_store::is_cache_entry_failure(&insufficient));
}

#[test]
fn format_exception_is_a_cache_entry_failure() {
    // C#: Wp06BridgeSplitTests.FormatException_IsFallbackable_AndACacheEntryFailure (the IsCacheEntryFailure lines only;
    // the IsFallbackable lines are W4-12b's)
    let format = DecodeError::Format {
        message: "bad entry".to_owned(),
    };

    assert!(pr_imaging::cache::disk_store::is_cache_entry_failure(&format));
}

/// Contract semantics note 1 (no C# test of its own: the C# constructor's `ArgumentException.ThrowIfNullOrWhiteSpace`).
/// A blank directory or pattern is `Err` naming the parameter, checked directory first.
#[test]
fn new_rejects_blank_directory_and_pattern() {
    let blank = DiskCacheStore::new("   ", "*.png", 0, None, None);
    let blank_pattern = DiskCacheStore::new("C:\\cache", "", 0, None, None);

    assert_eq!(
        blank.err(),
        Some(InvalidStoreArgument {
            param_name: "directory"
        })
    );
    assert_eq!(
        blank_pattern.err(),
        Some(InvalidStoreArgument {
            param_name: "searchPattern"
        })
    );
}
