//! Port of `PhotoReview.Integration.Tests/BenchmarkImageExecutorTeardownTests.cs`, the five executor / profile-scope tests of
//! `BenchmarkTrustTests.cs`, and `BenchmarkWorkloadRunnerTests.WarmNextWorkloadRecordsPreloadHitAfterWarming` (C# baseline
//! d92d6aae), plus the golden twins of `BenchmarkProfileScope.ApplyLogging` and `WindowedPreloadTarget` and the Rust-only gap
//! tests named in `contracts/W7-03b.md`. Unit W7-03b, red stage: the stub bodies are `todo!()`, so every test here fails
//! until the porter implements them.
//!
//! The C# tests decode real PNGs through WPF; these use the FAKE decoder and codecs of `pr-imaging`'s `support/preview_service.rs`
//! over tiny files in a `TempRoot`, never WIC. Every executor is disposed by a guard, and every wait has a timeout (hang guard).
//!
//! Documented skips (C# tests that belong to other units): `DecoderBench_*` (W7-03c), `WaitIdle_*` (`PerfSession`, not a
//! Benchmarking type), `Ranking_*` and `RunProfileAsync_StampsImagesPerSample` (W7-01 / W7-03c), and the rest of
//! `BenchmarkWorkloadRunnerTests.cs` / `BenchmarkEngineTimingTests.cs` (they only use the executor).

#[path = "../../pr-imaging/tests/support/preview_service.rs"]
mod preview_service;

use std::cell::Cell;
use std::collections::HashSet;
use std::fs;
use std::ops::Deref;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use pr_bench::image_executor::{
    BenchmarkExecutorOptions, BenchmarkImageExecutor, BenchmarkProfile, BenchmarkProfileScope, WindowedPreloadTarget,
    new_scratch_directory_name,
};
use pr_bench::models::BenchmarkWorkload;
use pr_core::abstractions::Log;
use pr_core::catalog::types::CatalogEntry;
use pr_core::model::enums::LoadingMode;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::service::PreviewError;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog, TempRoot};
use preview_service::{FakeDecoder, FakePayloadCodec, GIB, PHYSICAL_32_GIB, PixelsCodec, Script};
use serde_json::Value;

/// Hang guard for every blocking wait in this file (AGENTS.md "Tests").
const WAIT: Duration = Duration::from_secs(30);

/// The C# `Task` of a teardown that the test waits on (the executor's background pass).
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// `ticks` of `new DateTime(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc)` (the oracle's fixed write time).
const WRITTEN_TICKS: i64 = 638_769_024_000_000_000;

// ---- fixtures -----------------------------------------------------------------------------------------------------

/// A profile with the fields the executor reads (the rest is filled with neutral values).
/// Values mirror `BenchmarkProfiles.cs` for the ids the tests name (`fast-balanced`, `fast-sequential`, ...).
fn make_profile(
    id: &str,
    loading_mode: LoadingMode,
    workers: i32,
    next_window: i32,
    previous_window: i32,
    full_folder: bool,
) -> BenchmarkProfile {
    BenchmarkProfile {
        id: id.to_owned(),
        loading_mode,
        workers,
        next_window,
        previous_window,
        full_folder,
        memory_reserve_bytes: 2 * GIB,
        disk_cache: false,
        detailed_logging: false,
        name: id.to_owned(),
        description: String::new(),
        workload: BenchmarkWorkload::Sequential,
        warmup_count: 1,
        iterations: 1,
        correctness_only: false,
    }
}

fn fast_balanced() -> BenchmarkProfile {
    make_profile("fast-balanced", LoadingMode::Fast, 4, 16, 8, false)
}

fn fast_sequential() -> BenchmarkProfile {
    make_profile("fast-sequential", LoadingMode::Fast, 8, 32, 8, false)
}

fn no_preload_baseline() -> BenchmarkProfile {
    make_profile("no-preload-baseline", LoadingMode::Preview, 1, 0, 0, false)
}

fn full_folder_warm() -> BenchmarkProfile {
    make_profile("full-folder-warm", LoadingMode::Preview, 8, 32, 8, true)
}

/// `n` tiny files named `img-000.png`, ... in the root. Their content is irrelevant: the decoder is the fake.
fn write_files(root: &TempRoot, count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            let path = root.join(format!("img-{i:03}.png"));
            fs::write(&path, b"fake preview bytes").expect("write test file");
            path.to_string_lossy().into_owned()
        })
        .collect()
}

/// The executor options every test starts from: the fake decoder, a fixed "headroom" answer, a scratch parent inside the
/// test's own temp root, and 32 GiB of RAM. Tests change single fields with struct update.
fn default_options(root: &TempRoot, decoder: &Arc<FakeDecoder>) -> BenchmarkExecutorOptions {
    let dyn_decoder: Arc<dyn ImageDecoder> = decoder.clone();
    BenchmarkExecutorOptions {
        has_headroom: Some(Arc::new(|_: f64| true)),
        decoder: Some(dyn_decoder),
        scratch_parent: Some(root.join("scratch")),
        physical_memory_bytes: PHYSICAL_32_GIB,
        ..Default::default()
    }
}

/// Owns an executor for the length of a test. Dropping it disposes the executor and waits (bounded) for its teardown. A
/// panic from the executor during an unwinding assertion is caught here, so it cannot abort the test binary.
struct Executor {
    inner: Arc<BenchmarkImageExecutor>,
}

impl Deref for Executor {
    type Target = BenchmarkImageExecutor;

    fn deref(&self) -> &BenchmarkImageExecutor {
        &self.inner
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        let inner = Arc::clone(&self.inner);
        let _ = panic::catch_unwind(AssertUnwindSafe(move || {
            inner.dispose();
            inner.teardown_background_task().wait_timeout(TEARDOWN_TIMEOUT);
        }));
    }
}

fn build(profile: &BenchmarkProfile, files: &[String], options: BenchmarkExecutorOptions) -> Executor {
    let inner = BenchmarkImageExecutor::new(
        profile,
        files,
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
        options,
    )
    .unwrap_or_else(|_| panic!("executor construction"));
    Executor { inner: Arc::new(inner) }
}

/// `IsPreviewCached(path)` of the C# test seam, for a file that exists.
fn cached(executor: &BenchmarkImageExecutor, path: &str) -> bool {
    executor
        .is_preview_cached(path)
        .unwrap_or_else(|_| panic!("cache key of a test file"))
}

fn decodes(executor: &BenchmarkImageExecutor, path: &str) -> bool {
    executor.decode(path, &CancelToken::none()).is_ok()
}

/// Number of entries in a directory; 0 when it does not exist.
fn entries(dir: &Path) -> usize {
    fs::read_dir(dir).map(|items| items.count()).unwrap_or(0)
}

fn written() -> DateTimeUtc {
    DateTimeUtc::from_ticks(WRITTEN_TICKS).expect("valid ticks")
}

/// Key of a path built the way the oracle builds it: the entry carries its own length and write time (no file access).
fn key_for_probe(path: &str) -> Option<ImageCacheKey> {
    let entry = CatalogEntry::new(path)
        .ok()
        .map(|entry| entry.with_metadata(1000, written(), None, None))?;
    ImageCacheKey::create_for_entry(
        &FakeFileSystem::new(),
        &entry,
        false,
        DecodeBox::new(100, 100),
        KeyOptions::default(),
    )
    .ok()
}

/// A one-shot gate: `open` releases every `wait_open`. Used to hold a prune pass in flight.
#[derive(Clone)]
struct Latch(Arc<(Mutex<bool>, Condvar)>);

impl Latch {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }

    fn open(&self) {
        let (flag, changed) = &*self.0;
        *lock(flag) = true;
        changed.notify_all();
    }

    /// `true` when the latch opened within `timeout`.
    fn wait_open(&self, timeout: Duration) -> bool {
        let (flag, changed) = &*self.0;
        let (open, _) = changed
            .wait_timeout_while(lock(flag), timeout, |open| !*open)
            .unwrap_or_else(PoisonError::into_inner);
        *open
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- fakes for the windowed target --------------------------------------------------------------------------------

/// A preload target that counts its calls and answers every member with a value the wrapper must forward. The three members
/// with trait defaults (`active_viewer_decodes`, `cached_preview_bytes`, `has_disk_cached_preview`) answer NON-default values.
struct Inner {
    cached: bool,
    disk: bool,
    path_calls: AtomicUsize,
    key_calls: AtomicUsize,
    key_for_path_calls: AtomicUsize,
    preloads: AtomicUsize,
}

impl Inner {
    fn new(cached: bool) -> Self {
        Self {
            cached,
            disk: true,
            path_calls: AtomicUsize::new(0),
            key_calls: AtomicUsize::new(0),
            key_for_path_calls: AtomicUsize::new(0),
            preloads: AtomicUsize::new(0),
        }
    }
}

impl PreloadTarget for Inner {
    fn try_get_cached_preview_for_path(&self, _path: &str) -> bool {
        self.path_calls.fetch_add(1, Ordering::SeqCst);
        self.cached
    }

    fn try_get_cached_preview(&self, _key: &ImageCacheKey) -> bool {
        self.key_calls.fetch_add(1, Ordering::SeqCst);
        self.cached
    }

    fn preload(&self, _path: &str, _cancel: &CancelToken) -> Result<(), DecodeError> {
        self.preloads.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        self.key_for_path_calls.fetch_add(1, Ordering::SeqCst);
        key_for_probe(path).ok_or_else(|| KeyError::Other("no key for the test path".into()))
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        ImageCacheKey::create_for_entry(
            &FakeFileSystem::new(),
            entry,
            false,
            DecodeBox::new(100, 100),
            KeyOptions::default(),
        )
        .map_err(KeyError::from)
    }

    fn cache_count(&self) -> i32 {
        7
    }

    fn cache_bytes(&self) -> i64 {
        9000
    }

    fn active_viewer_decodes(&self) -> i32 {
        3
    }

    fn cached_preview_bytes(&self, _key: &ImageCacheKey) -> Option<i64> {
        Some(4096)
    }

    fn has_disk_cached_preview(&self, _key: &ImageCacheKey) -> bool {
        self.disk
    }
}

fn windowed(inner: &Arc<Inner>, files: &[String], profile: &BenchmarkProfile) -> WindowedPreloadTarget {
    let dyn_inner: Arc<dyn PreloadTarget> = inner.clone();
    WindowedPreloadTarget::new(dyn_inner, files, profile).unwrap_or_else(|_| panic!("windowed target over test files"))
}

// ---- BenchmarkImageExecutorTeardownTests ---------------------------------------------------------------------------

#[test]
fn dispose_async_does_not_block_on_prune_but_still_cleans_up_eventually() {
    // C#: BenchmarkImageExecutorTeardownTests.DisposeAsync_DoesNotBlockOnPrune_ButStillCleansUpEventually
    let root = TempRoot::new("executor-teardown");
    let files = write_files(&root, 5);
    let decoder = FakeDecoder::new();
    let profile = BenchmarkProfile {
        disk_cache: true,
        ..fast_sequential()
    };
    let executor = build(&profile, &files, default_options(&root, &decoder));
    let cache_dir = executor.disk_cache_directory().to_path_buf();

    for file in &files {
        assert!(decodes(&executor, file), "decode of {file}");
    }
    // The RAM cache is far bigger than five tiny previews, so nothing was persisted: seed the scratch directory directly so the
    // test really checks "was the directory removed".
    fs::create_dir_all(&cache_dir).expect("create scratch directory");
    fs::write(cache_dir.join("seed.pv4"), b"seed").expect("seed the scratch directory");

    // Hold a real prune pass in flight (a barrier inside the prune loop).
    let entered = Latch::new();
    let release = Latch::new();
    let (enter_hook, release_hook) = (entered.clone(), release.clone());
    let disk_store = executor.preview_service_for_tests().disk_store();
    disk_store.set_before_clear_prune_scheduled_for_tests(Some(Arc::new(move || {
        enter_hook.open();
        release_hook.wait_open(Duration::from_secs(60));
    })));
    disk_store.schedule_prune();
    assert!(entered.wait_open(WAIT), "the prune pass never reached the barrier");

    // Dispose on a helper thread: with the pass held open, a dispose that waits for it cannot finish, so the bounded wait below
    // turns "blocks on prune" into a failure instead of a hang.
    let disposing = Arc::clone(&executor.inner);
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        disposing.dispose();
        let _ = done_tx.send(());
    });
    let returned = done_rx.recv_timeout(Duration::from_secs(10)).is_ok();
    assert!(returned, "dispose blocked on the held prune pass");

    // The prune is still running: the scratch directory must survive it, and the teardown is still pending. The negative check is
    // a bounded wait: it can only finish early when the teardown did not wait for the prune.
    assert!(
        !executor.teardown_background_task().wait_timeout(Duration::from_secs(1)),
        "the teardown finished while the prune pass was still held"
    );
    assert!(
        cache_dir.exists(),
        "the scratch directory was removed while the prune pass was still running"
    );

    release.open();
    assert!(
        executor.teardown_background_task().wait_timeout(WAIT),
        "the teardown did not finish after the prune settled"
    );
    assert!(
        !cache_dir.exists(),
        "scratch disk-cache directory was not cleaned up: {}",
        cache_dir.display()
    );
}

// ---- BenchmarkTrustTests (PERF-01 / DetailedLogging) ---------------------------------------------------------------

#[test]
fn executor_honors_next_and_previous_window() {
    // C#: BenchmarkTrustTests.Executor_HonorsNextAndPreviousWindow
    let root = TempRoot::new("executor-window");
    let files = write_files(&root, 9);
    let decoder = FakeDecoder::new();
    let profile = BenchmarkProfile {
        next_window: 2,
        previous_window: 1,
        full_folder: false,
        ..fast_balanced()
    };
    let executor = build(&profile, &files, default_options(&root, &decoder));

    executor.warm_preload_around(4);
    executor.when_preload_settled();

    assert!(cached(&executor, &files[5]));
    assert!(cached(&executor, &files[6]));
    assert!(cached(&executor, &files[3]));
    assert!(!cached(&executor, &files[7]));
    assert!(!cached(&executor, &files[2]));
    assert!(!cached(&executor, &files[0]));
    assert!(!cached(&executor, &files[8]));
}

#[test]
fn executor_zero_window_warms_no_neighbours() {
    // C#: BenchmarkTrustTests.Executor_ZeroWindowWarmsNoNeighbours
    let root = TempRoot::new("executor-zero-window");
    let files = write_files(&root, 5);
    let decoder = FakeDecoder::new();
    let executor = build(&no_preload_baseline(), &files, default_options(&root, &decoder));

    executor.warm_preload_around(2);
    executor.when_preload_settled();

    assert!(!cached(&executor, &files[3]));
    assert!(!cached(&executor, &files[1]));
}

#[test]
fn executor_full_folder_controls_whole_folder_warming() {
    // C#: BenchmarkTrustTests.Executor_FullFolderControlsWholeFolderWarming
    // 45 files: the scheduler's own forward window (32) leaves the last file out unless it goes whole-folder.
    let root = TempRoot::new("executor-full-folder");
    let files = write_files(&root, 45);
    let decoder = FakeDecoder::new();

    let full_profile = BenchmarkProfile {
        next_window: 100,
        previous_window: 100,
        full_folder: true,
        ..full_folder_warm()
    };
    let full = build(&full_profile, &files, default_options(&root, &decoder));
    full.warm_preload_around(0);
    full.when_preload_settled();
    assert!(
        cached(&full, &files[44]),
        "a FullFolder profile must warm the whole folder"
    );

    let windowed_profile = BenchmarkProfile {
        next_window: 100,
        previous_window: 100,
        full_folder: false,
        ..fast_balanced()
    };
    let windowed_executor = build(&windowed_profile, &files, default_options(&root, &decoder));
    windowed_executor.warm_preload_around(0);
    windowed_executor.when_preload_settled();
    assert!(
        !cached(&windowed_executor, &files[44]),
        "a non-FullFolder profile must never warm the whole folder"
    );
}

#[test]
fn profile_scope_applies_and_restores_logging() {
    // C#: BenchmarkTrustTests.ProfileScope_AppliesAndRestoresLogging
    let enabled = Cell::new(false);
    let set = |value: bool| enabled.set(value);
    let set_ref = &set;
    let on = BenchmarkProfile {
        detailed_logging: true,
        ..make_profile("logging-on", LoadingMode::Preview, 4, 16, 8, false)
    };
    let off = make_profile("logging-off", LoadingMode::Preview, 4, 16, 8, false);

    {
        let _scope = BenchmarkProfileScope::apply_logging(&on, || enabled.get(), set_ref, None);
        assert!(enabled.get(), "detailed logging must be on for the run");
    }
    assert!(!enabled.get(), "the previous state (off) must be restored");

    enabled.set(true);
    {
        let _scope = BenchmarkProfileScope::apply_logging(&off, || enabled.get(), set_ref, None);
        assert!(!enabled.get(), "detailed logging must be off for the run");
    }
    assert!(enabled.get(), "the previous state (on) must be restored");
}

#[test]
fn profile_scope_logging_changed_during_run_is_not_overwritten() {
    // C#: BenchmarkTrustTests.ProfileScope_LoggingChangedDuringRun_IsNotOverwritten
    let enabled = Cell::new(true);
    let writes = Cell::new(0_i32);
    let set = |value: bool| {
        enabled.set(value);
        writes.set(writes.get() + 1);
    };
    let set_ref = &set;
    let off = make_profile("logging-off", LoadingMode::Preview, 4, 16, 8, false);

    {
        let _scope =
            BenchmarkProfileScope::apply_logging(&off, || enabled.get(), set_ref, Some(Box::new(|| writes.get())));
        assert!(!enabled.get());
        set(false); // the user switches logging off in Settings while the profile runs
    }
    assert!(
        !enabled.get(),
        "a change made during the run must not be restored to the stale value"
    );

    enabled.set(true);
    {
        let _scope =
            BenchmarkProfileScope::apply_logging(&off, || enabled.get(), set_ref, Some(Box::new(|| writes.get())));
        assert!(!enabled.get());
    }
    assert!(
        enabled.get(),
        "untouched during the run: the previous state is restored"
    );
}

// ---- BenchmarkWorkloadRunnerTests (executor part only) -------------------------------------------------------------

#[test]
fn warm_next_workload_records_preload_hit_after_warming() {
    // C#: BenchmarkWorkloadRunnerTests.WarmNextWorkloadRecordsPreloadHitAfterWarming
    let root = TempRoot::new("warm-next");
    let files = write_files(&root, 2);
    let decoder = FakeDecoder::new();
    // Headroom is fixed to "always available" so the test checks the preload-hit bookkeeping, not the machine's free RAM.
    let executor = build(&fast_balanced(), &files, default_options(&root, &decoder));

    assert!(decodes(&executor, &files[0]));
    executor.warm_preload_around(0);
    // The preload pass is fire-and-forget: wait for it before the foreground decode of files[1].
    executor.when_preload_settled();

    assert_eq!(executor.metrics().preload_hits, 0);
    assert!(decodes(&executor, &files[1]));
    assert_eq!(executor.metrics().preload_hits, 1);
}

// ---- Rust-only gap tests -------------------------------------------------------------------------------------------

#[test]
fn decode_counts_a_preload_hit_only_when_warmed_and_consumed() {
    // Rust-only gap test: a hit needs the RAM copy AND the scheduler's preloaded key, and the key is consumed by the hit.
    let root = TempRoot::new("gap-preload-hit");
    let files = write_files(&root, 3);
    let decoder = FakeDecoder::new();
    let executor = build(&fast_balanced(), &files, default_options(&root, &decoder));

    assert!(decodes(&executor, &files[0]), "cold decode");
    assert_eq!(executor.metrics().preload_hits, 0, "a cold decode is never a hit");

    executor.warm_preload_around(0);
    executor.when_preload_settled();
    assert!(decodes(&executor, &files[1]), "decode of a warmed neighbour");
    assert_eq!(
        executor.metrics().preload_hits,
        1,
        "a warmed and RAM-cached neighbour is one hit"
    );

    assert!(decodes(&executor, &files[1]), "second decode of the same file");
    assert_eq!(
        executor.metrics().preload_hits,
        1,
        "the key was consumed by the first hit"
    );

    // A RAM-cached image that the scheduler never warmed: decoded twice, no hit.
    let other = build(&fast_balanced(), &files, default_options(&root, &decoder));
    assert!(decodes(&other, &files[2]));
    assert!(decodes(&other, &files[2]));
    assert_eq!(
        other.metrics().preload_hits,
        0,
        "an unwarmed RAM copy is not a preload hit"
    );
}

#[test]
fn decode_with_a_cancelled_token_fails_before_any_work() {
    // Rust-only gap test: cancellation is checked before the key lookup, so a missing file is `Canceled`, not a `Key` error.
    let root = TempRoot::new("gap-cancel");
    let decoder = FakeDecoder::new();
    let executor = build(&fast_balanced(), &[], default_options(&root, &decoder));
    let missing = root.join("missing.png").to_string_lossy().into_owned();

    let token = CancelToken::none();
    token.cancel();
    let result = executor.decode(&missing, &token);

    assert!(
        matches!(result, Err(PreviewError::Canceled)),
        "a cancelled decode must fail as Canceled"
    );
    assert_eq!(decoder.calls(), 0, "no decode may start");
    assert_eq!(executor.metrics().preload_hits, 0);
}

#[test]
fn decode_of_a_missing_file_is_a_key_error() {
    // Rust-only gap test: the key of a missing file is `PreviewError::Key` (the C# synchronous FileNotFoundException).
    let root = TempRoot::new("gap-missing");
    let decoder = FakeDecoder::new();
    let executor = build(&fast_balanced(), &[], default_options(&root, &decoder));
    let missing = root.join("missing.png").to_string_lossy().into_owned();

    let result = executor.decode(&missing, &CancelToken::none());

    assert!(
        matches!(result, Err(PreviewError::Key(_))),
        "a missing file must be a key error"
    );
}

#[test]
fn evict_for_cold_decode_clears_ram_disk_and_the_preloaded_key() {
    // Rust-only gap test: eviction drops the RAM copy, the disk copy and the scheduler's preloaded key of that path.
    let root = TempRoot::new("gap-evict");
    let files = write_files(&root, 3);
    let decoder = FakeDecoder::new();
    let profile = BenchmarkProfile {
        disk_cache: true,
        ..fast_balanced()
    };
    let executor = build(&profile, &files, default_options(&root, &decoder));

    executor.warm_preload_around(0);
    executor.when_preload_settled();
    assert!(
        cached(&executor, &files[1]),
        "the neighbour must be warm before the eviction"
    );

    let dir = executor.disk_cache_directory().to_path_buf();
    fs::create_dir_all(&dir).expect("create scratch directory");
    fs::write(dir.join("seed.pv4"), b"seed").expect("seed the disk cache");

    assert!(
        executor.evict_for_cold_decode(&files[1]).is_ok(),
        "eviction of a valid path"
    );
    assert!(!cached(&executor, &files[1]), "the RAM copy must be gone");
    assert_eq!(
        entries(&dir),
        0,
        "the scratch directory must hold no entry after the eviction"
    );

    assert!(decodes(&executor, &files[1]), "the cold decode after the eviction");
    assert_eq!(
        executor.metrics().preload_hits,
        0,
        "a warm -> evict -> decode sequence records no preload hit"
    );
}

#[test]
fn evict_for_cold_decode_with_an_invalid_path_does_not_clear_the_disk() {
    // Rust-only gap test: a path `get_full_path` refuses fails BEFORE the disk is cleared (C# threw before `ClearDisk`).
    let root = TempRoot::new("gap-evict-invalid");
    let decoder = FakeDecoder::new();
    let profile = BenchmarkProfile {
        disk_cache: true,
        ..fast_balanced()
    };
    let executor = build(&profile, &[], default_options(&root, &decoder));
    let dir = executor.disk_cache_directory().to_path_buf();
    fs::create_dir_all(&dir).expect("create scratch directory");
    fs::write(dir.join("seed.pv4"), b"seed").expect("seed the disk cache");

    let result = executor.evict_for_cold_decode("");

    assert!(
        matches!(result, Err(PreviewError::InvalidPath(_))),
        "an empty path must be refused"
    );
    assert!(
        dir.join("seed.pv4").exists(),
        "the disk cache must survive a refused path"
    );
}

#[test]
fn scratch_directory_is_a_fresh_32_hex_child_of_the_parent() {
    // Rust-only gap test: each executor gets its own child of the scratch parent, named by 32 lowercase hex digits.
    let root = TempRoot::new("gap-scratch-name");
    let decoder = FakeDecoder::new();
    let parent = root.join("scratch");
    let first = build(&fast_balanced(), &[], default_options(&root, &decoder));
    let second = build(&fast_balanced(), &[], default_options(&root, &decoder));

    for executor in [&first, &second] {
        let dir = executor.disk_cache_directory();
        assert_eq!(
            dir.parent(),
            Some(parent.as_path()),
            "the scratch directory is a child of the scratch parent"
        );
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert_eq!(name.len(), 32, "name {name:?} must be 32 characters");
        assert!(
            name.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "name {name:?} must be lowercase hex"
        );
    }
    assert_ne!(
        first.disk_cache_directory(),
        second.disk_cache_directory(),
        "two executors never share a directory"
    );

    let names: HashSet<String> = (0..1000).map(|_| new_scratch_directory_name()).collect();
    assert_eq!(names.len(), 1000, "scratch directory names must not repeat");
    for name in &names {
        assert_eq!(name.len(), 32);
        assert!(
            name.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{name:?}"
        );
    }
}

#[test]
fn service_is_configured_from_the_profile() {
    // Rust-only gap test: the preview service takes the loading mode, the 16 GiB budget, the disk directory and the profile's
    // target width (the key carries it) from the profile, not from defaults.
    let root = TempRoot::new("gap-configured");
    let files = write_files(&root, 1);
    let decoder = FakeDecoder::new();
    let options = BenchmarkExecutorOptions {
        physical_memory_bytes: 64 * GIB,
        ..default_options(&root, &decoder)
    };
    let preview = make_profile("preview-light", LoadingMode::Preview, 4, 16, 4, false);
    let executor = build(&preview, &files, options);
    let service = executor.preview_service_for_tests();

    assert!(
        !service.is_original_loading_mode(),
        "a Preview profile is not the Original mode"
    );
    assert_eq!(
        service.capacity_bytes(),
        16 * GIB,
        "the image cache budget is 16 GiB on a 64 GiB machine"
    );
    assert_eq!(service.disk_directory(), executor.disk_cache_directory());
    let key = service
        .get_current_cache_key(&files[0])
        .unwrap_or_else(|_| panic!("key of a test file"));
    assert_eq!(key.target_width(), 960, "preview-light decodes at 960 px wide");
    assert!(!key.is_original());

    let original = make_profile("original-correctness", LoadingMode::Original, 1, 0, 0, false);
    let original_options = BenchmarkExecutorOptions {
        physical_memory_bytes: 64 * GIB,
        ..default_options(&root, &decoder)
    };
    let original_executor = build(&original, &files, original_options);
    let original_service = original_executor.preview_service_for_tests();
    assert!(
        original_service.is_original_loading_mode(),
        "an Original profile selects the original mode"
    );
    let original_key = original_service
        .get_current_cache_key(&files[0])
        .unwrap_or_else(|_| panic!("key of a test file"));
    assert!(original_key.is_original());
    assert_eq!(
        original_key.target_width(),
        0,
        "original-correctness decodes at its own size (width 0)"
    );
}

#[test]
fn disk_cache_off_profile_never_writes_the_scratch_directory() {
    // Rust-only gap test: `disk_cache = false` must reach the service as `disable_disk_cache_override = Some(true)`.
    let root = TempRoot::new("gap-disk-off");
    let files = write_files(&root, 3);
    let decoder = FakeDecoder::new();
    let executor = build(&fast_balanced(), &files, default_options(&root, &decoder));

    for file in &files {
        assert!(decodes(&executor, file));
    }
    executor.warm_preload_around(0);
    executor.when_preload_settled();

    assert!(
        !executor.disk_cache_directory().exists(),
        "a profile without the disk cache must never create the scratch directory"
    );
}

#[test]
fn disk_cache_on_profile_probes_the_disk() {
    // Rust-only gap test: with `disk_cache = true` a persisted entry is found by the disk probe (`!profile.disk_cache` mutant).
    let root = TempRoot::new("gap-disk-on");
    let files = write_files(&root, 1);
    let decoder = FakeDecoder::new();
    // Only a preview that really decoded downscaled is persisted, so the fake reports a smaller decode than the original.
    decoder.script_default(Script {
        downscaled: true,
        original: Some((400, 300)),
        ..Script::default()
    });
    let profile = BenchmarkProfile {
        disk_cache: true,
        ..fast_balanced()
    };
    let executor = build(&profile, &files, default_options(&root, &decoder));
    let service = executor.preview_service_for_tests();

    assert!(decodes(&executor, &files[0]));
    service.shutdown_persist_workers(); // flush the persist of the decode above
    assert!(
        service.evict_cached_path(&files[0], None).is_ok(),
        "RAM eviction of a valid path"
    );
    let key = service
        .get_current_cache_key(&files[0])
        .unwrap_or_else(|_| panic!("key of a test file"));

    assert!(
        pr_imaging::preload::target::PreloadTarget::has_disk_cached_preview(&**service, &key),
        "a persisted entry of a disk-cache profile must be found by the disk probe"
    );
}

#[test]
fn dispose_twice_is_a_no_op() {
    // Rust-only gap test: a second dispose returns at once and does not tear down again or fail.
    let root = TempRoot::new("gap-dispose-twice");
    let decoder = FakeDecoder::new();
    let executor = build(&fast_balanced(), &[], default_options(&root, &decoder));
    let dir = executor.disk_cache_directory().to_path_buf();
    fs::create_dir_all(&dir).expect("create scratch directory");
    fs::write(dir.join("seed.pv4"), b"seed").expect("seed the scratch directory");

    executor.dispose();
    executor.dispose();

    assert!(
        executor.teardown_background_task().wait_timeout(TEARDOWN_TIMEOUT),
        "the teardown of the first dispose must still complete"
    );
    assert!(!dir.exists(), "the scratch directory is removed once");
}

#[test]
fn teardown_handle_is_complete_before_dispose() {
    // Rust-only gap test: before `dispose` the teardown handle is `Task.CompletedTask`; after it, it completes.
    let root = TempRoot::new("gap-teardown-handle");
    let decoder = FakeDecoder::new();
    let executor = build(&fast_balanced(), &[], default_options(&root, &decoder));

    assert!(
        executor.teardown_background_task().is_complete(),
        "nothing is pending before dispose"
    );

    executor.dispose();
    assert!(
        executor.teardown_background_task().wait_timeout(TEARDOWN_TIMEOUT),
        "the teardown completes"
    );
    assert!(executor.teardown_background_task().is_complete());
}

#[test]
fn dispose_leaves_the_directory_when_the_prune_does_not_settle() {
    // Rust-only gap test: a prune that does not settle within `prune_wait_timeout` leaves the directory and logs the error.
    let root = TempRoot::new("gap-prune-timeout");
    let decoder = FakeDecoder::new();
    let log = Arc::new(RecordingLog::new());
    let dyn_log: Arc<dyn Log> = log.clone();
    let profile = BenchmarkProfile {
        disk_cache: true,
        ..fast_balanced()
    };
    let options = BenchmarkExecutorOptions {
        log: Some(dyn_log),
        prune_wait_timeout: Duration::from_millis(100),
        ..default_options(&root, &decoder)
    };
    let executor = build(&profile, &[], options);
    let dir = executor.disk_cache_directory().to_path_buf();
    fs::create_dir_all(&dir).expect("create scratch directory");
    fs::write(dir.join("seed.pv4"), b"seed").expect("seed the scratch directory");

    let entered = Latch::new();
    let release = Latch::new();
    let (enter_hook, release_hook) = (entered.clone(), release.clone());
    let disk_store = executor.preview_service_for_tests().disk_store();
    disk_store.set_before_clear_prune_scheduled_for_tests(Some(Arc::new(move || {
        enter_hook.open();
        release_hook.wait_open(Duration::from_secs(60));
    })));
    disk_store.schedule_prune();
    assert!(entered.wait_open(WAIT), "the prune pass never reached the barrier");

    executor.dispose();
    let finished = executor.teardown_background_task().wait_timeout(WAIT);
    release.open();

    assert!(
        finished,
        "the teardown must complete even though the prune did not settle"
    );
    assert!(
        dir.exists(),
        "the scratch directory is left while the prune is unsettled"
    );
    assert!(
        log.contains(
            LogLevel::Error,
            &format!("leaving scratch directory: {}", dir.display())
        ),
        "the error must name the directory that was left"
    );
}

#[cfg(windows)]
#[test]
fn dispose_logs_and_swallows_a_directory_that_cannot_be_removed() {
    // Rust-only gap test: a removal failure is logged and swallowed. Deterministic on Windows: a handle opened with share mode 0
    // on a file inside the directory makes the delete fail with a sharing violation. The file is ours (a fake seed).
    use std::os::windows::fs::OpenOptionsExt;

    let root = TempRoot::new("gap-remove-fails");
    let decoder = FakeDecoder::new();
    let log = Arc::new(RecordingLog::new());
    let dyn_log: Arc<dyn Log> = log.clone();
    let options = BenchmarkExecutorOptions {
        log: Some(dyn_log),
        ..default_options(&root, &decoder)
    };
    let executor = build(&fast_balanced(), &[], options);
    let dir = executor.disk_cache_directory().to_path_buf();
    fs::create_dir_all(&dir).expect("create scratch directory");
    let held_path = dir.join("held.pv4");
    fs::write(&held_path, b"held").expect("seed the scratch directory");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held_path)
        .expect("exclusive handle on the seed");

    executor.dispose();
    let finished = executor.teardown_background_task().wait_timeout(WAIT);
    drop(held);

    assert!(finished, "a failed removal must still complete the teardown");
    assert!(
        log.contains(
            LogLevel::Error,
            &format!("scratch directory could not be removed: {}", dir.display())
        ),
        "the removal failure must be logged with the directory"
    );
    assert!(dir.exists(), "the directory that could not be removed is still there");
}

// ---- WindowedPreloadTarget (BenchmarkTrustTests window cases and the gap tests) --------------------------------------

#[test]
fn windowed_target_asks_the_inner_only_for_in_window_paths() {
    // Rust-only gap test: an out-of-window path is answered `true` without asking the inner target (short-circuit); an in-window
    // path is the inner's answer.
    let root = TempRoot::new("gap-window-short-circuit");
    let files = write_files(&root, 5);
    let inner = Arc::new(Inner::new(false));
    let profile = BenchmarkProfile {
        next_window: 1,
        previous_window: 1,
        full_folder: false,
        ..fast_balanced()
    };
    let target = windowed(&inner, &files, &profile);
    target.set_center(2);

    assert!(
        target.try_get_cached_preview_for_path(&files[0]),
        "files[0] is outside the window: reported cached"
    );
    assert_eq!(
        inner.path_calls.load(Ordering::SeqCst),
        0,
        "the inner is not asked for an out-of-window path"
    );
    assert!(
        !target.try_get_cached_preview_for_path(&files[2]),
        "the centre is in the window: the inner answers false"
    );
    assert_eq!(inner.path_calls.load(Ordering::SeqCst), 1);

    let outside_key = key_for_probe(&files[0]).expect("key");
    assert!(target.try_get_cached_preview(&outside_key));
    assert_eq!(
        inner.key_calls.load(Ordering::SeqCst),
        0,
        "the inner is not asked for an out-of-window key"
    );
    let inside_key = key_for_probe(&files[2]).expect("key");
    assert!(!target.try_get_cached_preview(&inside_key));
    assert_eq!(inner.key_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn windowed_target_forwards_every_member() {
    // Rust-only gap test: every member reaches the inner target, including the three with trait defaults (a missing override would
    // answer 0 / None / false here, where the inner answers 3 / Some(4096) / true).
    let root = TempRoot::new("gap-window-forward");
    let files = write_files(&root, 3);
    let inner = Arc::new(Inner::new(false));
    // full_folder: no window restriction, so every answer comes from the inner target.
    let profile = BenchmarkProfile {
        full_folder: true,
        ..fast_balanced()
    };
    let target = windowed(&inner, &files, &profile);
    let key = key_for_probe(&files[0]).expect("key");

    assert_eq!(target.active_viewer_decodes(), 3, "active_viewer_decodes is forwarded");
    assert_eq!(
        target.cached_preview_bytes(&key),
        Some(4096),
        "cached_preview_bytes is forwarded"
    );
    assert!(
        target.has_disk_cached_preview(&key),
        "has_disk_cached_preview is forwarded"
    );
    assert_eq!(target.cache_count(), 7, "cache_count is forwarded");
    assert_eq!(target.cache_bytes(), 9000, "cache_bytes is forwarded");
    assert!(
        target.get_current_cache_key_for_path(&files[1]).is_ok(),
        "the path key builder is forwarded"
    );
    assert_eq!(inner.key_for_path_calls.load(Ordering::SeqCst), 1);
    let entry = CatalogEntry::new(&files[1])
        .ok()
        .map(|entry| entry.with_metadata(1000, written(), None, None))
        .expect("entry of a test file");
    assert!(
        target.get_current_cache_key(&entry).is_ok(),
        "the entry key builder is forwarded"
    );
    assert!(
        target.preload(&files[0], &CancelToken::none()).is_ok(),
        "preload is forwarded"
    );
    assert_eq!(inner.preloads.load(Ordering::SeqCst), 1);
    assert!(
        !target.try_get_cached_preview_for_path(&files[0]),
        "the verdict is the inner's answer when not restricted"
    );
}

#[test]
fn windowed_target_forwards_a_false_disk_cache_answer() {
    // Kills has_disk_cached_preview -> true: the wrapper must relay the inner's alse, not invent 	rue.
    let root = TempRoot::new("gap-window-disk-false");
    let files = write_files(&root, 2);
    let mut raw = Inner::new(false);
    raw.disk = false;
    let inner = Arc::new(raw);
    let profile = BenchmarkProfile {
        full_folder: true,
        ..fast_balanced()
    };
    let target = windowed(&inner, &files, &profile);
    let key = key_for_probe(&files[0]).expect("key");
    assert!(!target.has_disk_cached_preview(&key));
}

#[test]
fn executor_debug_names_the_type_and_directory() {
    // Kills Debug for BenchmarkImageExecutor -> Ok(Default::default()).
    let root = TempRoot::new("gap-debug");
    let decoder = FakeDecoder::new();
    let executor = build(&fast_balanced(), &[], default_options(&root, &decoder));
    let text = format!("{:?}", *executor);
    assert!(text.contains("BenchmarkImageExecutor"), "{text}");
    assert!(text.contains("disk_cache_directory"), "{text}");
}

#[test]
fn warm_preload_around_sets_the_centre_before_kicking() {
    // Rust-only gap test: the centre is set BEFORE the kick. A swapped order judges the first candidates with the old centre, holds
    // the neighbours back and nothing is warmed around the new centre.
    let root = TempRoot::new("gap-centre-order");
    let files = write_files(&root, 9);
    let decoder = FakeDecoder::new();
    let profile = BenchmarkProfile {
        next_window: 2,
        previous_window: 1,
        full_folder: false,
        ..fast_balanced()
    };
    let executor = build(&profile, &files, default_options(&root, &decoder));

    executor.warm_preload_around(4);
    executor.when_preload_settled();

    assert!(
        cached(&executor, &files[3]),
        "the previous neighbour is warmed around the new centre"
    );
    assert!(
        cached(&executor, &files[5]),
        "the next neighbour is warmed around the new centre"
    );
}

// ---- golden twins --------------------------------------------------------------------------------------------------

/// The oracle's INPUT cases (`tests/golden-inputs/<name>.json`, read-only). The expected output is `tests/golden/<name>.json`.
fn golden_inputs(name: &str) -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

#[test]
fn bench_profile_scope_matches_golden() {
    // Golden twin of `BenchmarkProfileScope.ApplyLogging` (oracle `bench-profile-scope`, 17 cases). Each case replays the oracle's
    // script: apply, external writes through the same setter, first dispose, optionally a flip and a second dispose.
    let expected = golden::load("bench-profile-scope");
    let inputs = golden_inputs("bench-profile-scope");
    assert_eq!(expected.cases.len(), inputs.len(), "one input per expected case");

    for (case, input) in expected.cases.iter().zip(&inputs) {
        let name = str_of(case, "name");
        assert_eq!(
            str_of(input, "name"),
            name,
            "inputs and expectations are in the same order"
        );

        let initial = bool_of(input, "initial");
        let detailed = bool_of(input, "detailed_logging");
        let use_counter = bool_of(input, "use_write_count");
        let dispose_twice = bool_of(input, "dispose_twice");
        let externals: Vec<bool> = input["external"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: 'external' must be an array"))
            .iter()
            .map(|value| {
                value
                    .as_bool()
                    .unwrap_or_else(|| panic!("{name}: external values are bools"))
            })
            .collect();

        let enabled = Cell::new(initial);
        let writes = Cell::new(0_i32);
        let set = |value: bool| {
            enabled.set(value);
            writes.set(writes.get() + 1);
        };
        let set_ref = &set;
        let profile = BenchmarkProfile {
            detailed_logging: detailed,
            ..make_profile("w703b", LoadingMode::Preview, 1, 0, 0, false)
        };
        let counter: Option<Box<dyn Fn() -> i32 + '_>> = if use_counter {
            Some(Box::new(|| writes.get()))
        } else {
            None
        };

        let mut scope = BenchmarkProfileScope::apply_logging(&profile, || enabled.get(), set_ref, counter);
        let during = enabled.get();
        for value in externals {
            set(value);
        }
        let after_externals = enabled.get();
        scope.dispose();
        let after = enabled.get();
        let after_second_dispose = if dispose_twice {
            enabled.set(!enabled.get()); // a flip that is not a counted write
            scope.dispose();
            Some(enabled.get())
        } else {
            None
        };

        assert_eq!(during, bool_of(case, "during"), "{name}: flag right after ApplyLogging");
        assert_eq!(
            after_externals,
            bool_of(case, "after_externals"),
            "{name}: flag after the external writes"
        );
        assert_eq!(after, bool_of(case, "after"), "{name}: flag after the first dispose");
        assert_eq!(
            after_second_dispose,
            case["after_second_dispose"].as_bool(),
            "{name}: flag after the second dispose"
        );
        assert_eq!(
            i64::from(writes.get()),
            i64_of(case, "writes"),
            "{name}: counted setter calls"
        );
    }
}

/// A JSON array of path strings (the `files` of a golden case).
fn string_array(value: &Value, name: &str) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{name}: expected an array"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("{name}: expected strings"))
                .to_owned()
        })
        .collect()
}

#[test]
fn bench_windowed_target_matches_golden() {
    // Golden twin of `WindowedPreloadTarget` (oracle `bench-windowed-target`, 21 cases): for each op the centre is set, then each
    // probe is asked by path and by key. A case with `throws` expects the constructor to refuse the file list.
    let golden = golden::load("bench-windowed-target");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let full_folder = bool_of(case, "full_folder");
        let next = i32::try_from(i64_of(case, "next")).expect("next fits i32");
        let previous = i32::try_from(i64_of(case, "previous")).expect("previous fits i32");
        let files = string_array(&case["files"], name);
        let inner_cached = bool_of(case, "inner_cached");
        let profile = BenchmarkProfile {
            next_window: next,
            previous_window: previous,
            ..make_profile("w703b", LoadingMode::Preview, 1, next, previous, full_folder)
        };
        let inner = Arc::new(Inner::new(inner_cached));
        let dyn_inner: Arc<dyn PreloadTarget> = inner.clone();
        let built = WindowedPreloadTarget::new(dyn_inner, &files, &profile);

        if case.get("throws").is_some() {
            assert_eq!(
                str_of(case, "throws"),
                "ArgumentException",
                "{name}: the golden names the C# exception"
            );
            assert!(
                matches!(built, Err(PathError::Argument)),
                "{name}: an empty path must be refused like C# ArgumentException"
            );
            continue;
        }
        let target = built.unwrap_or_else(|_| panic!("the constructor accepts this file list"));

        for op in case["ops"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: missing 'ops'"))
        {
            let center = i32::try_from(i64_of(op, "center")).expect("center fits i32");
            target.set_center(center);
            for probe in op["probes"]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: missing 'probes'"))
            {
                let path = str_of(probe, "path");
                assert_eq!(
                    target.try_get_cached_preview_for_path(path),
                    bool_of(probe, "by_path"),
                    "{name}: centre {center}, by path {path}"
                );
                match (probe["by_key"].as_bool(), key_for_probe(path)) {
                    (None, None) => {}
                    (Some(expected), Some(key)) => assert_eq!(
                        target.try_get_cached_preview(&key),
                        expected,
                        "{name}: centre {center}, by key {path}"
                    ),
                    (expected, key) => panic!(
                        "{name}: by_key expected {expected:?} but the key {} built for {path}",
                        if key.is_some() { "was" } else { "was not" }
                    ),
                }
            }
        }
    }
}
