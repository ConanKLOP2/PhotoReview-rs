//! Port of `PhotoReview.Imaging/Caching/DiskCacheStore.cs` (C# baseline d92d6aae; PARITY I-08, I-09, C-05, INV-2).
//!
//! The on-disk cache directory primitive shared by the preview cache (`*.pv4`, W4-14c) and the thumbnail cache
//! (`*.png`, W4-10b): quota prune in least-recently-used order (last access, then creation time), an access touch on
//! cache hits (F-IMG-3, at most once per [`ACCESS_TOUCH_INTERVAL`] per entry), incremental size tracking so a pass
//! under quota does not re-list the directory (IMG-10, a full scan at least every [`FULL_SCAN_EVERY`]th pass),
//! coalesced background prune passes that never wedge, clear, and the sweep of temp files a killed writer left
//! behind ([`TEMP_FILE_PATTERN`], older than [`STALE_TEMP_FILE_AGE`]).
//!
//! The C# class uses `File`/`Directory`/`DirectoryInfo` directly (no `IFileSystem`), so this module does too:
//! `std::fs` for listings and times, [`pr_core::io::physical`] for the .NET `File.Exists`/`File.Delete` semantics and
//! the DOS wildcard matcher. No new dependency; Windows-only calls stay behind `cfg(windows)` with a portable
//! fallback. The cache file FORMAT is not here (W4-08b `preview_file`, W4-11e PNG): this module only lists, stats,
//! touches and deletes files.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, Metadata};
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pr_core::abstractions::{Clock, FileSystem, Log, NullLog, SystemClock, panic_message};
use pr_core::io::physical::{PhysicalFileSystem, matches_win32_expression};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{self, PathError};
use pr_core::text::path_key;
use pr_core::time::DateTimeUtc;

use crate::cache::atomic_file::{AtomicWriteError, write_atomic};
use crate::decode_types::DecodedImageView; // W4-01b (`IDecodedImage`)
use crate::decoder_traits::{DecodeError, PlatformImageCodec}; // W4-01c
use crate::pixels::PixelBuffer; // W4-16a

/// C# `AccessTouchInterval` (10 minutes): a cache hit stamps the entry's last-access time at most this often.
pub const ACCESS_TOUCH_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// C# private `MaxTouchEntries`: the throttle table is cleared (all entries forgotten) when a NEW entry would be
/// added while it already holds this many.
pub const MAX_TOUCH_ENTRIES: usize = 8192;

/// C# private `FullScanEvery`: under quota, the 256th pass since the last full scan scans anyway.
pub const FULL_SCAN_EVERY: i32 = 256;

/// C# `TempFilePattern`: temp files of the atomic cache writes (`X.png.<guidN>.tmp`, `X.pv4.<guidN>.tmp`, D-05). No
/// store pattern matches them; [`DiskCacheStore::clear_directory`] and [`delete_stale_temp_files`] remove them.
pub const TEMP_FILE_PATTERN: &str = "*.tmp";

/// C# `StaleTempFileAge` (10 minutes): a live writer's temp file exists for milliseconds; older ones were left by a
/// killed process.
pub const STALE_TEMP_FILE_AGE: Duration = Duration::from_secs(10 * 60);

/// [`ACCESS_TOUCH_INTERVAL`] in .NET ticks (100 ns), the unit [`DateTimeUtc`] differences are compared in.
pub(crate) const ACCESS_TOUCH_INTERVAL_TICKS: i64 = 600 * pr_core::time::TICKS_PER_SECOND;

/// [`STALE_TEMP_FILE_AGE`] in .NET ticks.
pub(crate) const STALE_TEMP_FILE_AGE_TICKS: i64 = 600 * pr_core::time::TICKS_PER_SECOND;

/// Name of the background prune thread.
const PRUNE_THREAD_NAME: &str = "pr-disk-cache-prune";

/// Win32 `FILE_ATTRIBUTE_DIRECTORY`.
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

/// .NET ticks of 1970-01-01T00:00:00Z.
const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;

/// .NET text of `ArgumentException.ThrowIfNullOrWhiteSpace`, without the `(Parameter '...')` suffix.
const BLANK_ARGUMENT_TEXT: &str = "The value cannot be an empty string or composed entirely of whitespace.";

/// C# `ArgumentException` from `ArgumentException.ThrowIfNullOrWhiteSpace` in the constructor: `param_name` is
/// `"directory"` or `"searchPattern"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidStoreArgument {
    pub param_name: &'static str,
}

impl fmt::Display for InvalidStoreArgument {
    /// .NET text: `The value cannot be an empty string or composed entirely of whitespace. (Parameter '{param_name}')`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{BLANK_ARGUMENT_TEXT} (Parameter '{}')", self.param_name)
    }
}

impl std::error::Error for InvalidStoreArgument {}

/// The encoder [`write_pixels_atomically`] uses (C# `WicImageEncoder.EncodePng`: the WIC PNG encoder with WPF's
/// `PngBitmapEncoder` default options). W4-11e implements it in `pr_platform_win::wic::cache_image`; tests use a fake.
pub trait PngEncoder: Send + Sync {
    /// Writes `pixels` as one PNG to `output`. `pixels` is only read.
    fn encode_png(&self, pixels: &PixelBuffer, output: &mut dyn Write) -> Result<(), DecodeError>;
}

/// C# fields `_trackedBytes`, `_notedDuringScan`, `_skippedPasses`, guarded by `_sizeGate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SizeState {
    /// Bytes of matching files believed to be in the directory; `-1` = unknown (forces a full scan).
    tracked_bytes: i64,
    /// Bytes reported by [`DiskCacheStore::note_written`] since the current/last full scan started.
    noted_during_scan: i64,
    /// Passes skipped since the last full scan.
    skipped_passes: i32,
}

/// The test hook of [`DiskCacheStore::set_before_clear_prune_scheduled_for_tests`].
type PruneHook = Arc<dyn Fn() + Send + Sync>;

/// Shared state; the background prune thread holds an `Arc` of it.
struct StoreInner {
    directory: PathBuf,
    search_pattern: String,
    /// `max(0, maxBytes)`.
    max_bytes: i64,
    log: Arc<dyn Log>,
    clock: Arc<dyn Clock>,
    /// C# `ConcurrentDictionary<string, DateTime>(OrdinalIgnoreCase)`: key = `pr_core::text::path_key(path)`.
    last_touched: Mutex<HashMap<String, DateTimeUtc>>,
    /// C# `_pruneScheduled` (0/1).
    prune_scheduled: AtomicBool,
    /// C# `_prunePending` (0/1).
    prune_pending: AtomicBool,
    size: Mutex<SizeState>,
    /// C# `_fullScanCount`.
    full_scan_count: AtomicI32,
    /// C# `BeforeClearPruneScheduledForTests` (`None` in production).
    before_clear_prune_scheduled: Mutex<Option<PruneHook>>,
    /// Signalled whenever the prune loop clears `prune_scheduled` (replaces the C# 20 ms polling of
    /// `WaitForPruneAsync`). The waiter reads the flags and the notifier calls `notify_all` while holding
    /// `idle_lock` (no lost wake-up).
    idle_lock: Mutex<()>,
    idle: Condvar,
}

/// C# `DiskCacheStore`: one cache directory, its file pattern, quota and prune-coalescing state.
///
/// A handle over shared state: `schedule_prune` runs the pass on a background thread that keeps the state alive.
/// `Send + Sync`; every method takes `&self`.
pub struct DiskCacheStore {
    inner: Arc<StoreInner>,
}

impl fmt::Debug for DiskCacheStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiskCacheStore")
            .field("directory", &self.inner.directory)
            .field("search_pattern", &self.inner.search_pattern)
            .field("max_bytes", &self.inner.max_bytes)
            .finish_non_exhaustive()
    }
}

/// .NET `string.IsNullOrWhiteSpace` (`char.IsWhiteSpace` is the same set as `char::is_whitespace`).
fn is_blank_text(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn is_blank_path(path: &Path) -> bool {
    is_blank_text(&path.as_os_str().to_string_lossy())
}

impl DiskCacheStore {
    /// C# default arguments: pattern `"*.png"`, quota 0, `NullLog`, system clock.
    pub const DEFAULT_SEARCH_PATTERN: &'static str = "*.png";

    /// C# constructor `DiskCacheStore(directory, searchPattern = "*.png", maxBytes = 0, log = null, utcNow = null)`.
    /// A blank `directory` (empty or only `char::is_whitespace`) or `search_pattern` is `Err` (checked in that order);
    /// `max_bytes` below 0 becomes 0; `None` log = `NullLog`; `None` clock = `SystemClock`. Creates nothing on disk.
    pub fn new(
        directory: impl Into<PathBuf>,
        search_pattern: &str,
        max_bytes: i64,
        log: Option<Arc<dyn Log>>,
        clock: Option<Arc<dyn Clock>>,
    ) -> Result<Self, InvalidStoreArgument> {
        let directory = directory.into();
        if is_blank_path(&directory) {
            return Err(InvalidStoreArgument {
                param_name: "directory",
            });
        }
        if is_blank_text(search_pattern) {
            return Err(InvalidStoreArgument {
                param_name: "searchPattern",
            });
        }
        let inner = StoreInner {
            directory,
            search_pattern: search_pattern.to_owned(),
            max_bytes: max_bytes.max(0),
            log: log.unwrap_or_else(|| Arc::new(NullLog)),
            clock: clock.unwrap_or_else(|| Arc::new(SystemClock)),
            last_touched: Mutex::new(HashMap::new()),
            prune_scheduled: AtomicBool::new(false),
            prune_pending: AtomicBool::new(false),
            size: Mutex::new(SizeState {
                tracked_bytes: -1,
                noted_during_scan: 0,
                skipped_passes: 0,
            }),
            full_scan_count: AtomicI32::new(0),
            before_clear_prune_scheduled: Mutex::new(None),
            idle_lock: Mutex::new(()),
            idle: Condvar::new(),
        };
        Ok(Self { inner: Arc::new(inner) })
    }

    pub fn directory(&self) -> &Path {
        &self.inner.directory
    }

    pub fn search_pattern(&self) -> &str {
        &self.inner.search_pattern
    }

    /// The effective quota (`max(0, maxBytes)`).
    pub fn max_bytes(&self) -> i64 {
        self.inner.max_bytes
    }

    /// C# internal `FullScanCount`: full directory enumerations performed so far (test/diagnostic).
    pub fn full_scan_count(&self) -> i32 {
        self.inner.full_scan_count.load(Ordering::SeqCst)
    }

    /// C# `NoteAccessed(path)`: a cache hit on `path`. Skipped when the same entry (OrdinalIgnoreCase) was stamped less
    /// than [`ACCESS_TOUCH_INTERVAL`] ago by the injected clock (`now - last < interval`, ticks; a clock that went back
    /// also skips). Otherwise: clear the whole table if it holds [`MAX_TOUCH_ENTRIES`] or more, record `now` for the
    /// entry (ALSO when the file is missing), then set the file's last-access time to `now`. Best effort, never fails.
    pub fn note_accessed(&self, path: &Path) {
        let now = self.inner.clock.utc_now();
        let key = path_key(path);
        {
            let mut table = self.inner.last_touched.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(last) = table.get(&key)
                && now.ticks() - last.ticks() < ACCESS_TOUCH_INTERVAL_TICKS
            {
                return;
            }
            if table.len() >= MAX_TOUCH_ENTRIES {
                table.clear();
            }
            table.insert(key, now);
        }
        // DIVERGENCE: C# catches only IOException/UnauthorizedAccessException, so `""`, a NUL in the path or a time
        // before 1601 throw ArgumentException out of NoteAccessed (after the entry was recorded); here every failure is
        // swallowed. Unreachable from the callers (a cache path they just built, a real clock).
        let _ = set_last_access_time(path, now);
    }

    /// C# `NoteWritten(path)`: a file was just persisted into the directory. Reads its length (no follow of links, a
    /// directory or a missing/unreadable path is ignored), then under the size lock adds it to `noted_during_scan` and,
    /// when the size is known (`tracked_bytes >= 0`), to `tracked_bytes` (wrapping add, C# `long` unchecked).
    pub fn note_written(&self, path: &Path) {
        // DIVERGENCE: `new FileInfo("")` / a NUL throws ArgumentException out of C# NoteWritten; here it returns quietly
        // (unreachable from the callers, which pass a path they just wrote).
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if is_directory(&metadata) {
            return; // C# `FileInfo.Length` throws FileNotFoundException for a directory.
        }
        let length = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
        let mut size = self.inner.size.lock().unwrap_or_else(PoisonError::into_inner);
        size.noted_during_scan = size.noted_during_scan.wrapping_add(length);
        if size.tracked_bytes >= 0 {
            size.tracked_bytes = size.tracked_bytes.wrapping_add(length);
        }
    }

    /// C# `SchedulePrune()`: coalesced background prune. If a loop is already scheduled/running, only marks one pending
    /// request (however many calls) and returns; else starts the loop on a new named thread (see the contract,
    /// semantics 6 for the loop, the exit window and the spawn-failure rule). Never blocks on file I/O.
    pub fn schedule_prune(&self) {
        if self.inner.prune_scheduled.swap(true, Ordering::SeqCst) {
            self.inner.prune_pending.store(true, Ordering::SeqCst);
            return;
        }
        start_prune_loop(&self.inner);
    }

    /// C# `WaitForPruneAsync(timeout)` (blocking; R0003): waits until neither a loop is scheduled nor a request is
    /// pending, or `timeout` elapses; returns whether no loop is scheduled at the end. Waits on a condvar, no polling.
    pub fn wait_for_prune(&self, timeout: Duration) -> bool {
        let inner = &self.inner;
        let deadline = Instant::now().checked_add(timeout);
        let mut guard = inner.idle_lock.lock().unwrap_or_else(PoisonError::into_inner);
        while inner.prune_scheduled.load(Ordering::SeqCst) || inner.prune_pending.load(Ordering::SeqCst) {
            guard = match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    inner
                        .idle
                        .wait_timeout(guard, deadline - now)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => inner.idle.wait(guard).unwrap_or_else(PoisonError::into_inner),
            };
        }
        drop(guard);
        !inner.prune_scheduled.load(Ordering::SeqCst)
    }

    /// C# private `RunPrunePass()`, public as a test seam (the C# tests call it through reflection so no test depends
    /// on background timing). Skips (no listing) when the size is known, at or under quota, and this is not the
    /// [`FULL_SCAN_EVERY`]th pass; otherwise counts a full scan, runs [`prune_directory`] and stores
    /// `remaining + noted_during_scan` (or `-1` when the directory is missing). `Err` = the listing failed (the loop
    /// logs it as `Prune failed: {directory}`); the size state is then left as the start of the pass set it.
    #[doc(hidden)]
    pub fn run_prune_pass(&self) -> io::Result<()> {
        self.inner.run_prune_pass()
    }

    /// C# `ClearDirectory()`: deletes every file matching the store pattern (errors logged per file), then every
    /// [`TEMP_FILE_PATTERN`] file WITHOUT logging (a live writer still holds its temp file and removes it itself), then
    /// forgets the tracked size (`tracked_bytes = -1`, `noted_during_scan = 0`). A listing error stops at once and is
    /// returned (the tracked size is then NOT reset, like the C# exception path).
    pub fn clear_directory(&self) -> io::Result<()> {
        let inner = &self.inner;
        clear_directory_matching(&inner.directory, &inner.search_pattern, Some(inner.log.as_ref()))?;
        // No log: a temp file a writer still holds open fails to delete, and that writer removes it itself.
        clear_directory_matching(&inner.directory, TEMP_FILE_PATTERN, None)?;
        let mut size = inner.size.lock().unwrap_or_else(PoisonError::into_inner);
        size.tracked_bytes = -1;
        size.noted_during_scan = 0;
        Ok(())
    }

    /// C# `BeforeClearPruneScheduledForTests`: runs on the prune thread after the loop's last pending check, right
    /// before the scheduled flag is cleared. `None` in production.
    #[doc(hidden)]
    pub fn set_before_clear_prune_scheduled_for_tests(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self
            .inner
            .before_clear_prune_scheduled
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = hook;
    }
}

impl StoreInner {
    /// C# private `RunPrunePass()` (contract semantics 4; never holds the size lock across the scan).
    fn run_prune_pass(&self) -> io::Result<()> {
        {
            let mut size = self.size.lock().unwrap_or_else(PoisonError::into_inner);
            if size.tracked_bytes >= 0 && size.tracked_bytes <= self.max_bytes {
                size.skipped_passes += 1;
                if size.skipped_passes < FULL_SCAN_EVERY {
                    return Ok(());
                }
            }
            size.skipped_passes = 0;
            size.noted_during_scan = 0;
        }

        // Counted before the scan, so a failing scan still counts.
        self.full_scan_count.fetch_add(1, Ordering::SeqCst);
        let remaining = prune_directory(
            &self.directory,
            &self.search_pattern,
            self.max_bytes,
            Some(self.log.as_ref()),
        )?;
        let mut size = self.size.lock().unwrap_or_else(PoisonError::into_inner);
        // Writes noted while the scan ran may or may not have been listed; counting them again only over-estimates
        // (safe), never under-estimates.
        size.tracked_bytes = if remaining < 0 {
            -1
        } else {
            remaining.wrapping_add(size.noted_during_scan)
        };
        Ok(())
    }

    /// Logs a failed pass as C# `_log.Error($"Prune failed: {_directory}", ex)`.
    fn log_prune_failure(&self, error: &io::Error) {
        self.log
            .error(&format!("Prune failed: {}", self.directory.display()), Some(error));
    }
}

/// C# `RunPruneLoop()`: starts the loop on a new named thread. The caller has set `prune_scheduled`.
fn start_prune_loop(inner: &Arc<StoreInner>) {
    let for_thread = Arc::clone(inner);
    let spawned = thread::Builder::new()
        .name(PRUNE_THREAD_NAME.to_owned())
        .spawn(move || run_prune_loop(&for_thread));
    if spawned.is_err() {
        // DIVERGENCE: C# `Task.Run` cannot fail. When no thread can be created, the loop runs inline on the caller's
        // thread rather than leave `prune_scheduled` stuck at true (the store must never wedge, INV-2).
        run_prune_loop(inner);
    }
}

/// The body of the C# `Task.Run` lambda in `RunPruneLoop` (contract semantics 6).
fn run_prune_loop(inner: &Arc<StoreInner>) {
    loop {
        inner.prune_pending.store(false, Ordering::SeqCst);
        // Any failure (I/O, access, or a panic) costs only this pass: one escaping the loop would leave
        // `prune_scheduled` true forever and the cache would never be pruned again.
        match catch_unwind(AssertUnwindSafe(|| inner.run_prune_pass())) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => inner.log_prune_failure(&error),
            Err(payload) => inner.log_prune_failure(&io::Error::other(panic_message(payload.as_ref()))),
        }
        if !inner.prune_pending.load(Ordering::SeqCst) {
            break;
        }
    }

    // C# `finally`: the test hook, then clear the scheduled flag. No lock is held while the hook runs.
    let hook = inner
        .before_clear_prune_scheduled
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let Some(hook) = hook {
        // A panicking hook must not skip the flag reset below.
        let _ = catch_unwind(AssertUnwindSafe(|| hook()));
    }
    inner.prune_scheduled.store(false, Ordering::SeqCst);
    {
        let _guard = inner.idle_lock.lock().unwrap_or_else(PoisonError::into_inner);
        inner.idle.notify_all();
    }

    // A prune requested right before or during the clearing of the flag is serviced by a new loop.
    if inner.prune_pending.load(Ordering::SeqCst) && !inner.prune_scheduled.swap(true, Ordering::SeqCst) {
        start_prune_loop(inner);
    }
}

/// C# static `PruneDirectory(directory, searchPattern, maxBytes, log)`: deletes matching files, least recently used
/// first (last-access time, then creation time, stable on the listing order), until the total length is at or under
/// `max_bytes`. A file that cannot be deleted stays counted. Returns the bytes remaining, `Ok(-1)` when `directory`
/// is not an existing directory (nothing is created). Lengths and times come from the listing itself (no per-file
/// stat). Only files: directories (including directory links/junctions) never match. Each file is addressed (and a
/// failed delete logged) by its C# `FileInfo.FullName`: `dotnet_path::get_full_path(directory)` joined with the name.
/// `Err` = the listing failed or the pattern is invalid (C# `ArgumentException` for a NUL: `InvalidInput`), or a
/// matching file has a last-access or creation FILETIME past `DateTime.MaxValue` (C# `FileInfo.LastAccessTimeUtc` /
/// `CreationTimeUtc` throws `ArgumentOutOfRangeException` from the `OrderBy`/`ThenBy` key: `InvalidData`, nothing deleted).
pub fn prune_directory(
    directory: &Path,
    search_pattern: &str,
    max_bytes: i64,
    log: Option<&dyn Log>,
) -> io::Result<i64> {
    if !PhysicalFileSystem.directory_exists(directory) {
        return Ok(-1);
    }
    // DIVERGENCE: .NET splits a pattern with a separator into sub-folder + pattern; it is rejected here as in W2-14
    // (every store pattern is a constant `*.ext`).
    validate_search_pattern(search_pattern)?;
    let root = full_listing_root(directory)?;

    struct Candidate {
        full_name: PathBuf,
        length: i64,
        key: (u64, u64),
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if is_directory(&metadata) {
            continue;
        }
        let name = entry.file_name();
        if !matches_win32_expression(search_pattern, &name.to_string_lossy()) {
            continue;
        }
        files.push(Candidate {
            full_name: join_full(&root, &name),
            length: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
            key: lru_key(&metadata)?,
        });
    }
    // DIVERGENCE: .NET converts each FILETIME to LOCAL time (`DateTimeOffset.FromFileTime`) before taking `.UtcDateTime`,
    // so on a machine east of UTC a time in the last hours before `DateTime.MaxValue` also throws (ArgumentException);
    // only a time past `DateTime.MaxValue` itself fails here (machine-independent). Only a foreign process setting bogus
    // times in the cache folder reaches either.
    // Stable: equal keys keep the listing order (C# `OrderBy(...).ThenBy(...)` is a stable sort).
    files.sort_by_key(|candidate| candidate.key);

    // DIVERGENCE: LINQ `Sum` is checked and throws OverflowException past i64::MAX; this saturates (unreachable on a
    // real disk).
    let mut total = files
        .iter()
        .fold(0i64, |sum, candidate| sum.saturating_add(candidate.length));
    for candidate in &files {
        if total <= max_bytes {
            break;
        }
        if try_delete(&candidate.full_name, log) {
            total -= candidate.length;
        }
    }
    Ok(total)
}

/// C# static `ClearDirectory(directory, searchPattern, log)`: [`try_delete`] every matching file, listed like C#
/// `Directory.EnumerateFiles` (paths spelled from `directory` as given). A missing directory is `Ok(())`; a listing
/// error or an invalid pattern is returned.
pub fn clear_directory_matching(directory: &Path, search_pattern: &str, log: Option<&dyn Log>) -> io::Result<()> {
    let fs = PhysicalFileSystem;
    if !fs.directory_exists(directory) {
        return Ok(());
    }
    // The W2-14 call lists eagerly; C# deletes during a lazy listing: same effects.
    for path in fs.enumerate_files(directory, search_pattern)? {
        try_delete(&path, log);
    }
    Ok(())
}

/// C# static `DeleteStaleTempFiles(directory, utcNow, maxFiles, log)`: looks at the first `max_files` (in listing
/// order; `<= 0` looks at none) [`TEMP_FILE_PATTERN`] files and deletes those last written strictly more than
/// [`STALE_TEMP_FILE_AGE`] before `utc_now`. Returns how many were deleted. Never fails: a file whose write time cannot
/// be read is skipped; a listing failure, or a write time past `DateTime.MaxValue` (C# `ArgumentOutOfRangeException`),
/// stops the sweep, is logged as `Stale temp file cleanup failed: {directory}` and the count so far is returned.
pub fn delete_stale_temp_files(directory: &Path, utc_now: DateTimeUtc, max_files: i32, log: Option<&dyn Log>) -> i32 {
    if !PhysicalFileSystem.directory_exists(directory) {
        return 0;
    }
    let mut removed = 0;
    if let Err(error) = sweep_stale_temp_files(directory, utc_now, max_files, log, &mut removed)
        && let Some(log) = log
    {
        log.error(
            &format!("Stale temp file cleanup failed: {}", directory.display()),
            Some(&error),
        );
    }
    removed
}

/// The lazy listing of [`delete_stale_temp_files`]: an `Err` is a listing failure; `removed` keeps the count so far.
fn sweep_stale_temp_files(
    directory: &Path,
    utc_now: DateTimeUtc,
    max_files: i32,
    log: Option<&dyn Log>,
    removed: &mut i32,
) -> io::Result<()> {
    // .NET opens the listing when `Directory.EnumerateFiles` is called, before `Take` looks at the count.
    let root = full_listing_root(directory)?;
    let listing = fs::read_dir(&root)?;
    let mut remaining = usize::try_from(max_files).unwrap_or(0);
    for entry in listing {
        if remaining == 0 {
            break;
        }
        let entry = entry?;
        let metadata = entry.metadata()?;
        if is_directory(&metadata) {
            continue;
        }
        let name = entry.file_name();
        if !matches_win32_expression(TEMP_FILE_PATTERN, &name.to_string_lossy()) {
            continue;
        }
        // C# `Take` counts the entries looked at, fresh ones included.
        remaining -= 1;
        let path = join_as_spelled(directory, &name);
        // C# catches only IOException/UnauthorizedAccessException around `File.GetLastWriteTimeUtc`; the
        // ArgumentOutOfRangeException of a time past `DateTime.MaxValue` reaches the outer catch: the sweep stops.
        let Some(written) = last_write_time_utc(&path)? else {
            continue;
        };
        if utc_now.ticks() - written.ticks() > STALE_TEMP_FILE_AGE_TICKS && try_delete(&path, log) {
            *removed += 1;
        }
    }
    Ok(())
}

/// C# `File.GetLastWriteTimeUtc(path)` (the entry itself, links not followed): a missing file is 1601-01-01 (.NET
/// `FromFileTimeUtc(0)`); any other stat failure is `Ok(None)` (the sweep skips the file). A write time past
/// `DateTime.MaxValue` is `Err(InvalidData)` (.NET `ArgumentOutOfRangeException`, which the sweep does not catch per
/// file).
// DIVERGENCE: .NET converts through LOCAL time (`DateTimeOffset.FromFileTime`), so east of UTC a write time in the last
// hours before `DateTime.MaxValue` also throws (ArgumentException); here only a time past `DateTime.MaxValue` does.
fn last_write_time_utc(path: &Path) -> io::Result<Option<DateTimeUtc>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => last_write_of(&metadata).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DateTimeUtc::from_filetime(0)),
        Err(_) => Ok(None),
    }
}

/// C# static `TryDelete(path, log)`: `false` (nothing logged) when `path` is not an existing file; else .NET
/// `File.Delete` (a read-only file fails) and returns whether the file is gone afterwards. A delete error is logged as
/// `Delete failed: {path}` with the error and gives `false`.
pub fn try_delete(path: &Path, log: Option<&dyn Log>) -> bool {
    let fs = PhysicalFileSystem;
    if !fs.file_exists(path) {
        return false;
    }
    // W2-14 `delete` is `DeleteFileW` (.NET `File.Delete`): unlike std `remove_file` it does not clear Read-only.
    match fs.delete(path) {
        Ok(()) => !fs.file_exists(path),
        Err(error) => {
            if let Some(log) = log {
                log.error(&format!("Delete failed: {}", path.display()), Some(&error));
            }
            false
        }
    }
}

/// C# internal `IsCacheEntryFailure(ex)`: whether a failure reading/decoding a disk-cache entry means "damaged,
/// unreadable or foreign entry" (delete it, decode from source) rather than a failure that must propagate
/// (cancellation, out of memory, busy, programming errors). Mapping per variant: contract, semantics 9.
pub fn is_cache_entry_failure(error: &DecodeError) -> bool {
    // One exhaustive match with no `_` arm: a new variant forces a decision.
    match error {
        DecodeError::Io(_)
        | DecodeError::FileNotFound { .. }
        | DecodeError::DirectoryNotFound { .. }
        | DecodeError::MemoryAdmission { .. }
        | DecodeError::NotSupported { .. }
        | DecodeError::MissingCodec { .. }
        | DecodeError::Format { .. }
        | DecodeError::InvalidData { .. }
        | DecodeError::InvalidArgument { .. }
        | DecodeError::InvalidOperation { .. }
        | DecodeError::Disposed { .. }
        | DecodeError::Overflow { .. }
        | DecodeError::InvalidCast { .. }
        | DecodeError::Com { .. } => true,
        DecodeError::Busy { .. }
        | DecodeError::Canceled
        | DecodeError::OutOfMemory { .. }
        | DecodeError::DllNotFound { .. }
        | DecodeError::BadImageFormat { .. }
        | DecodeError::EntryPointNotFound { .. }
        | DecodeError::Other { .. } => false,
        // Transparent wrapper (A2): the C# exception is the wrapped one.
        DecodeError::WithSourceBytes { error, .. } => is_cache_entry_failure(error),
    }
}

/// C# static `WriteAtomicallyAsync(IDecodedImage, IPlatformImageCodec, cachePath, log, ct)` (blocking, R0003): takes
/// the pixels through `codec.to_pixels(image.platform_image())` (a foreign platform image is the codec's
/// `InvalidArgument`, before any disk access), then [`write_pixels_atomically`]. A blank `cache_path` is
/// `InvalidArgument` (param `cachePath`), checked before the codec.
pub fn write_atomically(
    image: &dyn DecodedImageView,
    codec: &dyn PlatformImageCodec,
    encoder: &dyn PngEncoder,
    cache_path: &Path,
    log: Option<&dyn Log>,
    token: &CancelToken,
) -> Result<(), DecodeError> {
    check_cache_path(cache_path)?;
    let lease = codec.to_pixels(image.platform_image())?;
    // C# `lease.Pixels` of a default lease is null: the pixel overload throws ArgumentNullException.
    let pixels = lease.pixels().ok_or_else(|| DecodeError::InvalidArgument {
        message: "Value cannot be null. (Parameter 'pixels')".to_owned(),
    })?;
    write_pixels_atomically(pixels, encoder, cache_path, log, token)
}

/// C# internal static `WriteAtomicallyAsync(PixelBuffer, cachePath, log, ct)`: `crate::cache::atomic_file` (W4-08a)
/// with `encoder.encode_png` as the payload. `pixels` is only read.
pub fn write_pixels_atomically(
    pixels: &PixelBuffer,
    encoder: &dyn PngEncoder,
    cache_path: &Path,
    log: Option<&dyn Log>,
    token: &CancelToken,
) -> Result<(), DecodeError> {
    check_cache_path(cache_path)?;
    let mut encode_error: Option<DecodeError> = None;
    let result = write_atomic(
        cache_path,
        |output| match encoder.encode_png(pixels, output) {
            Ok(()) => Ok(()),
            Err(error) => {
                encode_error = Some(error);
                Err(io::Error::other("PNG encode failed"))
            }
        },
        log,
        token,
    );
    // C# rethrows the encoder's own exception.
    if let Some(error) = encode_error {
        return Err(error);
    }
    match result {
        Ok(()) => Ok(()),
        Err(AtomicWriteError::Canceled) => Err(DecodeError::Canceled),
        Err(AtomicWriteError::Io(error)) => Err(DecodeError::Io(error)),
    }
}

/// `ArgumentException.ThrowIfNullOrWhiteSpace(cachePath)`.
fn check_cache_path(cache_path: &Path) -> Result<(), DecodeError> {
    if is_blank_path(cache_path) {
        return Err(DecodeError::InvalidArgument {
            message: format!("{BLANK_ARGUMENT_TEXT} (Parameter 'cachePath')"),
        });
    }
    Ok(())
}

// ---- listing helpers ---------------------------------------------------------------------------------------------

/// The pattern checks of W2-14 `PhysicalFileSystem::enumerate_files`: a NUL, a separator or a drive-rooted pattern is
/// `InvalidInput` (C# `ArgumentException` for NUL/rooted).
fn validate_search_pattern(pattern: &str) -> io::Result<()> {
    let mut chars = pattern.chars();
    let rooted = matches!((chars.next(), chars.next()), (Some(d), Some(':')) if d.is_ascii_alphabetic());
    if pattern.contains(['\\', '/']) || pattern.contains('\0') || rooted {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("The search pattern '{pattern}' is not supported."),
        ));
    }
    Ok(())
}

/// The folder .NET lists: `TrimEndingDirectorySeparator(Path.GetFullPath(directory))` (never `std::path::absolute`
/// directly: §7 item 7).
fn full_listing_root(directory: &Path) -> io::Result<String> {
    let text = directory
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The directory path is not valid Unicode."))?;
    let full = dotnet_path::get_full_path(text).map_err(path_error)?;
    Ok(dotnet_path::trim_ending_directory_separator(&full).to_owned())
}

fn path_error(error: PathError) -> io::Error {
    match error {
        PathError::Argument => io::Error::new(io::ErrorKind::InvalidInput, error.to_string()),
        // ERROR_FILENAME_EXCED_RANGE (206), .NET PathTooLongException.
        PathError::TooLong => io::Error::from_raw_os_error(206),
    }
}

/// C# `FileInfo.FullName` of a listed entry: the full root (a drive root keeps its separator) joined with the name.
fn join_full(root: &str, name: &OsStr) -> PathBuf {
    let mut joined = OsString::from(root);
    if !root.ends_with(['\\', '/']) {
        joined.push("\\");
    }
    joined.push(name);
    PathBuf::from(joined)
}

/// `Path.Join(directory, name)` as `Directory.EnumerateFiles` spells a result (W2-14 `join_as_spelled`): one `\`
/// unless `directory` already ends with a separator or is a bare drive spec `X:`.
fn join_as_spelled(directory: &Path, name: &OsStr) -> PathBuf {
    let spelled = directory.as_os_str().to_string_lossy();
    let ends_with_sep = spelled.ends_with(['\\', '/']);
    let bare_drive = {
        let b = spelled.as_bytes();
        b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    };
    let mut joined: OsString = directory.as_os_str().to_owned();
    if !spelled.is_empty() && !ends_with_sep && !bare_drive {
        joined.push("\\");
    }
    joined.push(name);
    PathBuf::from(joined)
}

// ---- metadata helpers --------------------------------------------------------------------------------------------

/// The DIRECTORY attribute of the entry itself (a directory link/junction counts as a directory, a file link does not,
/// like `DirectoryInfo.EnumerateFiles`).
#[cfg(windows)]
fn is_directory(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
}

#[cfg(not(windows))]
fn is_directory(metadata: &Metadata) -> bool {
    let _ = FILE_ATTRIBUTE_DIRECTORY;
    metadata.is_dir()
}

/// .NET `DateTime.FromFileTimeUtc` range check: a FILETIME past `DateTime.MaxValue` (or negative as a `long`) throws
/// `ArgumentOutOfRangeException("Not a valid Win32 FileTime.")`, here `InvalidData`.
#[cfg(windows)]
fn checked_filetime(filetime: u64) -> io::Result<DateTimeUtc> {
    DateTimeUtc::from_filetime(filetime)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Not a valid Win32 FileTime."))
}

/// Sort key of the LRU order: raw FILETIMEs of (last access, creation), the same order as the C# `DateTime`s. `Err`
/// when either is not a valid .NET `DateTime` (C# computes every key before sorting, so any such file fails the call).
#[cfg(windows)]
fn lru_key(metadata: &Metadata) -> io::Result<(u64, u64)> {
    use std::os::windows::fs::MetadataExt;
    let accessed = metadata.last_access_time();
    let created = metadata.creation_time();
    checked_filetime(accessed)?;
    checked_filetime(created)?;
    Ok((accessed, created))
}

#[cfg(not(windows))]
fn lru_key(metadata: &Metadata) -> io::Result<(u64, u64)> {
    let ticks = |t: io::Result<SystemTime>| {
        t.ok()
            .and_then(DateTimeUtc::from_system_time)
            .map_or(0, |d| u64::try_from(d.ticks()).unwrap_or(0))
    };
    Ok((ticks(metadata.accessed()), ticks(metadata.created())))
}

#[cfg(windows)]
fn last_write_of(metadata: &Metadata) -> io::Result<DateTimeUtc> {
    use std::os::windows::fs::MetadataExt;
    checked_filetime(metadata.last_write_time())
}

#[cfg(not(windows))]
fn last_write_of(metadata: &Metadata) -> io::Result<DateTimeUtc> {
    Ok(metadata
        .modified()
        .ok()
        .and_then(DateTimeUtc::from_system_time)
        .unwrap_or(DateTimeUtc::MIN))
}

/// `DateTimeUtc` as a `SystemTime` (`UNIX_EPOCH` +/- the tick difference); `None` when not representable.
fn to_system_time(time: DateTimeUtc) -> Option<SystemTime> {
    let ticks = time.ticks() - UNIX_EPOCH_TICKS;
    let magnitude = ticks.unsigned_abs();
    let per_second = pr_core::time::TICKS_PER_SECOND.unsigned_abs();
    let nanos = u32::try_from(magnitude % per_second).ok()? * 100;
    let offset = Duration::new(magnitude / per_second, nanos);
    if ticks >= 0 {
        UNIX_EPOCH.checked_add(offset)
    } else {
        UNIX_EPOCH.checked_sub(offset)
    }
}

/// .NET `File.SetLastAccessTimeUtc(path, time)`: opens with only `FILE_WRITE_ATTRIBUTES`, sharing Read|Write|Delete,
/// without `FILE_FLAG_BACKUP_SEMANTICS` (a directory fails the open, like .NET `File.*`), and sets the access time.
fn set_last_access_time(path: &Path, time: DateTimeUtc) -> io::Result<()> {
    let accessed = to_system_time(time).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let file = open_for_attributes(path)?;
    file.set_times(fs::FileTimes::new().set_accessed(accessed))
}

#[cfg(windows)]
fn open_for_attributes(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
    const FILE_SHARE_READ_WRITE_DELETE: u32 = 0x1 | 0x2 | 0x4;
    fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ_WRITE_DELETE)
        .open(path)
}

#[cfg(not(windows))]
fn open_for_attributes(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new().write(true).open(path)
}
