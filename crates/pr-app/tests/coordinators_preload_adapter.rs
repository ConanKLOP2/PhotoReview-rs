//! Port of `tests/PhotoReview.App.Tests/Coordinators/PreloadControllerAdapterTests.cs` (C# baseline d92d6aae, `[Trait("Category",
//! "HotPath")]`). Port of `PhotoReview.App.Shared/Coordinators/PreloadControllerAdapter.cs` (32 lines).
//! Contract: `contracts/W8-06d.md`. Target: `crates/pr-app/src/coordinators/traits.rs` (`coordinators::traits`).
//!
//! Ported: 7 of the 8 C# tests. N/A, type system: `Constructor_NullFactory_Throws` (the C# null check has no Rust form, a
//! closure is always present), so no test is written for it.
//!
//! Red tests (W8-06d): they compile against the stub and fail through `todo!()` until the adapter is implemented.
//! The harness is test-local (contract "Test plan"): a `RecordingTarget` (C# `RecordingTarget`, a `PreloadTarget` fake
//! with an optional blocking `preload`), a `FakeMemoryProbe` (always headroom), and a real `PreloadScheduler`.
//! No sleeps: the blocking test is released by a latch, and every wait is bounded by `GUARD` (a hang guard only).

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use pr_app::coordinators::traits::{IPreloadController, PreloadControllerAdapter};
use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::scheduler::{CallbackError, PreloadScheduler, PreloadSchedulerDeps};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload_order::PreloadOptions;
use pr_test_support::FakeFileSystem;

/// Hang guard for every bounded wait (C# `WaitAsync(TimeSpan.FromSeconds(30))`). Never a timing assertion.
const GUARD: Duration = Duration::from_secs(30);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A one-shot latch: `open` releases every waiter; `wait` is bounded by `timeout` and reports whether it opened.
#[derive(Default)]
struct Latch {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Latch {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn open(&self) {
        *lock(&self.open) = true;
        self.changed.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        let guard = lock(&self.open);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |open| !*open)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

/// C# `FakeMemoryProbe`: headroom is always available.
struct FakeMemoryProbe;

impl MemoryProbe for FakeMemoryProbe {
    fn has_headroom(&self, _maximum_load: f64, _reserve_bytes: i64) -> bool {
        true
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        Some(MemorySnapshot {
            load_percent: 50,
            available_bytes: 16 << 30,
        })
    }
}

/// C# `MakeEntries(count)`: `C:\adapter-test\img-000.jpg`, ... with a fixed length and write time.
fn make_entries(count: usize) -> Arc<[CatalogEntry]> {
    (0..count)
        .map(|i| {
            CatalogEntry::new(entry_path(i))
                .expect("a valid test path")
                .with_metadata(1, DateTimeUtc::default(), None, None)
        })
        .collect::<Vec<_>>()
        .into()
}

fn entry_path(i: usize) -> String {
    format!(r"C:\adapter-test\img-{i:03}.jpg")
}

/// C# `RecordingTarget : IPreloadTarget`: records every preloaded path and keeps the warmed keys. With `block`, `preload`
/// announces itself on `started` and holds until `release` opens. The last token it was handed is kept for the test.
struct RecordingTarget {
    entries: Arc<[CatalogEntry]>,
    warmed: Mutex<Vec<(String, ImageCacheKey)>>,
    preloaded: Mutex<Vec<String>>,
    block: bool,
    started: Arc<Latch>,
    release: Arc<Latch>,
    last_token: Mutex<Option<CancelToken>>,
}

impl RecordingTarget {
    fn new(count: usize) -> Arc<Self> {
        Self::build(count, false)
    }

    fn blocking(count: usize) -> Arc<Self> {
        Self::build(count, true)
    }

    fn build(count: usize, block: bool) -> Arc<Self> {
        Arc::new(Self {
            entries: make_entries(count),
            warmed: Mutex::new(Vec::new()),
            preloaded: Mutex::new(Vec::new()),
            block,
            started: Latch::new(),
            release: Latch::new(),
            last_token: Mutex::new(None),
        })
    }

    fn preloaded(&self) -> Vec<String> {
        lock(&self.preloaded).clone()
    }

    /// The cache key of catalog entry `i` (C# `GetCurrentCacheKey(entry)`).
    fn key_of(&self, i: usize) -> ImageCacheKey {
        self.get_current_cache_key(&self.entries[i])
            .expect("a key for a catalog entry")
    }
}

impl PreloadTarget for RecordingTarget {
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        lock(&self.warmed).iter().any(|(warmed, _)| warmed == path)
    }

    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        lock(&self.warmed).iter().any(|(_, warmed)| warmed == key)
    }

    fn preload(&self, path: &str, cancel: &CancelToken) -> Result<(), DecodeError> {
        lock(&self.preloaded).push(path.to_owned());
        *lock(&self.last_token) = Some(cancel.clone());
        if self.block {
            self.started.open();
            self.release.wait(GUARD);
        } else {
            let key = self
                .get_current_cache_key_for_path(path)
                .map_err(|error| DecodeError::Other {
                    message: error.to_string(),
                })?;
            lock(&self.warmed).push((path.to_owned(), key));
        }
        Ok(())
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.path().eq_ignore_ascii_case(path))
            .ok_or_else(|| KeyError::Io(io::Error::new(io::ErrorKind::NotFound, path.to_owned())))?;
        self.get_current_cache_key(entry)
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        let fs = FakeFileSystem::new();
        ImageCacheKey::create_for_entry(
            &fs,
            entry,
            false,
            DecodeBox {
                width: 100,
                height: 100,
            },
            KeyOptions::default(),
        )
        .map_err(KeyError::from)
    }

    fn cache_count(&self) -> i32 {
        i32::try_from(lock(&self.warmed).len()).unwrap_or(i32::MAX)
    }

    fn cache_bytes(&self) -> i64 {
        0
    }
}

/// C# `NewScheduler(target)`: two workers, a zero full-folder threshold, the always-headroom memory probe.
fn new_scheduler(target: &Arc<RecordingTarget>) -> Arc<PreloadScheduler> {
    let snapshot = Arc::clone(&target.entries);
    let deps = PreloadSchedulerDeps {
        options: Some(PreloadOptions {
            worker_count: 2,
            full_folder_threshold_bytes: 0,
            ..PreloadOptions::default()
        }),
        processor_count: Some(2),
        ..PreloadSchedulerDeps::default()
    };
    Arc::new(PreloadScheduler::new(
        Arc::clone(target) as Arc<dyn PreloadTarget>,
        Arc::new(ReviewMetrics::new()),
        Arc::new(move || Ok::<_, CallbackError>(Arc::clone(&snapshot))),
        Arc::new(FakeMemoryProbe),
        deps,
    ))
}

#[test]
fn every_member_reads_the_scheduler_from_the_factory_on_each_call() {
    // C#: PreloadControllerAdapterTests.EveryMember_ReadsTheSchedulerFromTheLazyFactoryOnEachCall
    let target = RecordingTarget::new(2);
    let scheduler = new_scheduler(&target);
    let reads = Arc::new(AtomicUsize::new(0));
    let factory_reads = Arc::clone(&reads);
    let factory_scheduler = Arc::clone(&scheduler);
    let adapter = PreloadControllerAdapter::new(move || {
        factory_reads.fetch_add(1, Ordering::SeqCst);
        Arc::clone(&factory_scheduler)
    });
    // Lazy: construction does not touch the scheduler.
    assert_eq!(0, reads.load(Ordering::SeqCst));
    let key = target.key_of(0);

    adapter.cancel();
    adapter.clear_preloaded_keys();
    adapter.remove_preloaded_keys_for_path(&entry_path(0));
    let _ = adapter.try_consume_preloaded_key(&key);
    let _ = adapter.is_idle();
    let _ = adapter.get_viewer_decode_delay();
    adapter.notify_navigation(0);

    assert_eq!(7, reads.load(Ordering::SeqCst));
}

#[test]
fn preload_around_delegates_to_the_scheduler_and_warms_the_neighbour() {
    // C#: PreloadControllerAdapterTests.PreloadAroundAsync_DelegatesToTheSchedulerAndWarmsTheNeighbour
    let target = RecordingTarget::new(2);
    let scheduler = new_scheduler(&target);
    let shared = Arc::clone(&scheduler);
    let adapter = PreloadControllerAdapter::new(move || Arc::clone(&shared));

    assert!(adapter.preload_around(0).wait_timeout(GUARD));

    assert!(target.preloaded().contains(&entry_path(1)));
    assert!(adapter.is_idle());
}

#[test]
fn try_consume_preloaded_key_after_a_warmed_preload_consumes_the_key_exactly_once() {
    // C#: PreloadControllerAdapterTests.TryConsumePreloadedKey_AfterAWarmedPreload_ConsumesTheKeyExactlyOnce
    let target = RecordingTarget::new(2);
    let scheduler = new_scheduler(&target);
    let shared = Arc::clone(&scheduler);
    let adapter = PreloadControllerAdapter::new(move || Arc::clone(&shared));
    assert!(adapter.preload_around(0).wait_timeout(GUARD));
    let key = target.key_of(1);

    assert!(adapter.try_consume_preloaded_key(&key));
    assert!(!adapter.try_consume_preloaded_key(&key));
}

#[test]
fn remove_preloaded_keys_for_path_drops_the_warmed_key_of_that_path_only() {
    // C#: PreloadControllerAdapterTests.RemovePreloadedKeysForPath_DropsTheWarmedKeyOfThatPathOnly
    let target = RecordingTarget::new(3);
    let scheduler = new_scheduler(&target);
    let shared = Arc::clone(&scheduler);
    let adapter = PreloadControllerAdapter::new(move || Arc::clone(&shared));
    assert!(adapter.preload_around(0).wait_timeout(GUARD));
    let removed = target.key_of(1);
    let kept = target.key_of(2);

    // C# passes `removed.Path`: the key's own normalised (upper-invariant) path, not the raw catalog path.
    adapter.remove_preloaded_keys_for_path(removed.path());

    assert!(!adapter.try_consume_preloaded_key(&removed));
    assert!(adapter.try_consume_preloaded_key(&kept));
}

#[test]
fn clear_preloaded_keys_drops_every_warmed_key() {
    // C#: PreloadControllerAdapterTests.ClearPreloadedKeys_DropsEveryWarmedKey
    let target = RecordingTarget::new(3);
    let scheduler = new_scheduler(&target);
    let shared = Arc::clone(&scheduler);
    let adapter = PreloadControllerAdapter::new(move || Arc::clone(&shared));
    assert!(adapter.preload_around(0).wait_timeout(GUARD));

    adapter.clear_preloaded_keys();

    assert!(!adapter.try_consume_preloaded_key(&target.key_of(1)));
    assert!(!adapter.try_consume_preloaded_key(&target.key_of(2)));
}

#[test]
fn cancel_and_is_idle_reflect_the_schedulers_in_flight_preload() {
    // C#: PreloadControllerAdapterTests.CancelAndIsIdle_ReflectTheSchedulersInFlightPreload
    let target = RecordingTarget::blocking(2);
    let scheduler = new_scheduler(&target);
    let shared = Arc::clone(&scheduler);
    let adapter = PreloadControllerAdapter::new(move || Arc::clone(&shared));
    assert!(adapter.is_idle(), "a fresh scheduler is idle");

    let lifetime = adapter.preload_around(0);
    assert!(target.started.wait(GUARD), "the blocking preload started");
    assert!(!adapter.is_idle());
    let token = lock(&target.last_token)
        .clone()
        .expect("a token was handed to the preload");
    assert!(!token.is_canceled());

    adapter.cancel();

    assert!(token.is_canceled());
    target.release.open();
    assert!(lifetime.wait_timeout(GUARD));
    assert!(adapter.is_idle());
}

#[test]
fn dispose_disposes_the_scheduler_so_a_later_preload_starts_nothing() {
    // C#: PreloadControllerAdapterTests.Dispose_DisposesTheSchedulerSoALaterPreloadStartsNothing
    let target = RecordingTarget::new(2);
    let scheduler = new_scheduler(&target);
    let shared = Arc::clone(&scheduler);
    let adapter = PreloadControllerAdapter::new(move || Arc::clone(&shared));

    adapter.dispose();
    assert!(adapter.preload_around(0).wait_timeout(GUARD));

    assert!(target.preloaded().is_empty());
}
