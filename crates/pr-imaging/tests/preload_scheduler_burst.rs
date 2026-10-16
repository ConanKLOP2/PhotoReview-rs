//! Port of the scheduler half of `tests/PhotoReview.Imaging.Tests/BurstPreloadTests.cs` (C# baseline d92d6aae, 384 lines):
//! direction, burst lead, wake-up, superseded skip and viewer priority under navigation. The direction-order and
//! `NavigationPace` tests of the same C# file are W4-05b / W4-06a (see the contract), not here.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Test notes: C# `GatedTarget.NextStartAsync` (an ordered start queue with a 10 s guard) is [`BurstTarget::next_start`].

mod common;

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use common::preload_fakes::*;
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::navigation_pace::NavigationPace;
use pr_imaging::preload::scheduler::{PreloadScheduler, RunHandle};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload_order::PreloadWindow;
use pr_test_support::FakeFileSystem;

/// Fake target of the burst tests (C# `GatedTarget`): every decode may be held by a gate, starts are queued in order.
struct BurstTarget {
    fs: Arc<FakeFileSystem>,
    cached: Mutex<HashSet<String>>,
    gates: Mutex<Vec<(String, Arc<Gate>)>>,
    starts: Mutex<VecDeque<String>>,
    starts_cv: Condvar,
    started_all: Mutex<Vec<String>>,
    block_all: bool,
    ignore_cancellation: bool,
    release_all: AtomicBool,
    active_viewer: AtomicI32,
}

impl BurstTarget {
    fn new(fs: &Arc<FakeFileSystem>, block_all: bool, ignore_cancellation: bool) -> Arc<Self> {
        Arc::new(Self {
            fs: Arc::clone(fs),
            cached: Mutex::new(HashSet::new()),
            gates: Mutex::new(Vec::new()),
            starts: Mutex::new(VecDeque::new()),
            starts_cv: Condvar::new(),
            started_all: Mutex::new(Vec::new()),
            block_all,
            ignore_cancellation,
            release_all: AtomicBool::new(false),
            active_viewer: AtomicI32::new(0),
        })
    }

    fn gate(&self, path: &str) -> Arc<Gate> {
        let mut gates = lock(&self.gates);
        if let Some((_, gate)) = gates.iter().find(|(p, _)| p.eq_ignore_ascii_case(path)) {
            return Arc::clone(gate);
        }
        let gate = Gate::new();
        gates.push((path.to_owned(), Arc::clone(&gate)));
        gate
    }

    fn mark_cached(&self, path: &str) {
        lock(&self.cached).insert(path.to_ascii_uppercase());
    }

    fn release(&self, path: &str) {
        self.gate(path).open();
    }

    fn is_released(&self, path: &str) -> bool {
        self.gate(path).is_open()
    }

    fn release_all(&self) {
        self.release_all.store(true, Ordering::SeqCst);
        for (_, gate) in lock(&self.gates).iter() {
            gate.open();
        }
    }

    /// C# `NextStartAsync`: the next path in start order (guarded).
    fn next_start(&self) -> String {
        let start = std::time::Instant::now();
        let mut starts = lock(&self.starts);
        loop {
            if let Some(path) = starts.pop_front() {
                return path;
            }
            assert!(start.elapsed() < GUARD, "Timed out waiting for a preload start.");
            let (guard, _) = self
                .starts_cv
                .wait_timeout(starts, Duration::from_millis(5))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            starts = guard;
        }
    }

    fn started_count(&self) -> usize {
        lock(&self.started_all).len()
    }

    fn started_contains(&self, path: &str) -> bool {
        lock(&self.started_all).iter().any(|p| p.eq_ignore_ascii_case(path))
    }

    fn set_active_viewer(&self, decodes: i32) {
        self.active_viewer.store(decodes, Ordering::SeqCst);
    }
}

impl PreloadTarget for BurstTarget {
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        lock(&self.cached).contains(&path.to_ascii_uppercase())
    }

    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        lock(&self.cached).contains(key.path())
    }

    fn preload(&self, path: &str, cancel: &CancelToken) -> Result<(), DecodeError> {
        lock(&self.started_all).push(path.to_owned());
        lock(&self.starts).push_back(path.to_owned());
        self.starts_cv.notify_all();
        if self.block_all && !self.release_all.load(Ordering::SeqCst) {
            let gate = self.gate(path);
            let released = if self.ignore_cancellation {
                gate.wait(None)
            } else {
                gate.wait(Some(cancel))
            };
            if !released {
                return Err(DecodeError::Canceled);
            }
        }
        lock(&self.cached).insert(path.to_ascii_uppercase());
        Ok(())
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        Ok(ImageCacheKey::create(
            &*self.fs,
            path,
            false,
            DecodeBox::new(100, 100),
            KeyOptions::default(),
        )?)
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        self.get_current_cache_key_for_path(entry.path())
    }

    fn cache_count(&self) -> i32 {
        i32::try_from(lock(&self.cached).len()).unwrap_or(i32::MAX)
    }

    fn cache_bytes(&self) -> i64 {
        0
    }

    fn active_viewer_decodes(&self) -> i32 {
        self.active_viewer.load(Ordering::SeqCst)
    }
}

/// Test fixture: 100 files `image-000.jpg .. image-099.jpg`, a shared metrics object and a settable clock.
struct Burst {
    fs: Arc<FakeFileSystem>,
    entries: Arc<[CatalogEntry]>,
    files: Vec<String>,
    metrics: Arc<ReviewMetrics>,
    clock: FakeClock,
    pace: Arc<NavigationPace>,
}

impl Burst {
    fn new() -> Self {
        let fs = fake_fs();
        let entries = entries(&fs, 100, "image");
        let files = entries.iter().map(|e| e.path().to_owned()).collect();
        let clock = FakeClock::new();
        let pace = clock.pace();
        Self {
            fs,
            entries,
            files,
            metrics: Arc::new(ReviewMetrics::new()),
            clock,
            pace,
        }
    }

    fn advance(&self, ms: f64) {
        self.clock.advance_ms(ms);
    }

    /// C# `Create(GatedTarget, workers)`: windowed (not whole-folder) preload with the shared pace and metrics.
    fn scheduler(&self, target: &Arc<BurstTarget>, workers: i32) -> PreloadScheduler {
        create(
            target,
            &self.entries,
            Setup::new(workers, PreloadWindow::DEFAULT)
                .full_folder_threshold(0)
                .metrics(Arc::clone(&self.metrics))
                .pace(Arc::clone(&self.pace)),
        )
    }

    fn index_of(&self, path: &str) -> usize {
        self.files
            .iter()
            .position(|f| f.eq_ignore_ascii_case(path))
            .unwrap_or_else(|| panic!("{path} is not a fixture file"))
    }
}

#[test]
fn after_prev_preloads_backward_first() {
    // C#: BurstPreloadSchedulerTests.AfterPrev_PreloadsBackwardFirst
    let burst = Burst::new();
    let target = BurstTarget::new(&burst.fs, true, false);
    let scheduler = burst.scheduler(&target, 1);
    scheduler.notify_navigation(50);
    burst.advance(600.0);
    scheduler.notify_navigation(49);
    let _run: RunHandle = scheduler.preload_around(49);
    assert_eq!(48, burst.index_of(&target.next_start()));
    target.release_all();
}

#[test]
fn burst_starts_at_lead_offset() {
    // C#: BurstPreloadSchedulerTests.Burst_StartsAtLeadOffset
    let burst = Burst::new();
    burst.metrics.record_source_read(1, 350, true); // decode EWMA = 350 ms
    let target = BurstTarget::new(&burst.fs, true, false);
    let scheduler = burst.scheduler(&target, 1);
    scheduler.notify_navigation(10);
    burst.advance(33.0);
    scheduler.notify_navigation(11);
    burst.advance(33.0);
    scheduler.notify_navigation(12); // burst established: lead = ceil(350/33) = 11, scheduler self-starts
    assert_eq!(12 + 11 + 1, burst.index_of(&target.next_start()));
    target.release_all();
}

#[test]
fn burst_skips_passed_images() {
    // C#: BurstPreloadSchedulerTests.Burst_SkipsPassedImages
    let burst = Burst::new();
    burst.metrics.record_source_read(1, 350, true);
    let target = BurstTarget::new(&burst.fs, true, false);
    let scheduler = burst.scheduler(&target, 2);
    scheduler.notify_navigation(10);
    for i in 11..=12 {
        burst.advance(33.0);
        scheduler.notify_navigation(i);
    }
    let first = burst.index_of(&target.next_start());
    let second = burst.index_of(&target.next_start());
    assert_eq!([24, 25], [first, second]);
    for i in 13..=30 {
        burst.advance(33.0);
        scheduler.notify_navigation(i);
    }
    target.release(&burst.files[24]);
    // Center 30 + lead 11: the next start is 42, not 26..41 (passed before a decode could finish).
    assert_eq!(42, burst.index_of(&target.next_start()));
    target.release_all();
}

#[test]
fn navigation_wakes_waiting_scheduler() {
    // C#: BurstPreloadSchedulerTests.Navigation_WakesWaitingScheduler
    let burst = Burst::new();
    let target = BurstTarget::new(&burst.fs, true, false);
    for i in 2..=32 {
        target.mark_cached(&burst.files[i]);
    }
    let scheduler = burst.scheduler(&target, 2);
    let _run = scheduler.preload_around(0);
    assert_eq!(1, burst.index_of(&target.next_start())); // the only uncached image; 1 worker idle
    scheduler.notify_navigation(60); // jump: new neighbours need decoding while image 1 is still decoding
    assert_eq!(61, burst.index_of(&target.next_start()));
    assert!(!target.is_released(&burst.files[1]));
    target.release_all();
}

#[test]
fn queued_item_reached_by_user_is_dropped() {
    // C#: BurstPreloadSchedulerTests.QueuedItemReachedByUser_IsDropped
    let burst = Burst::new();
    let target = BurstTarget::new(&burst.fs, true, true);
    let scheduler = burst.scheduler(&target, 1);
    let first = scheduler.preload_around(0);
    assert_eq!(1, burst.index_of(&target.next_start())); // holds the only slot, ignores cancellation
    scheduler.cancel();
    // New lifetime: its first worker (image 6) waits for the held slot.
    let _second = scheduler.preload_around(5);
    scheduler.notify_navigation(6); // the user is now ON image 6 (the viewer decodes it)
    target.release(&burst.files[1]);
    assert!(finished(&first));
    assert_eq!(7, burst.index_of(&target.next_start()));
    assert!(!target.started_contains(&burst.files[6]));
    target.release_all();
}

#[test]
fn viewer_decoding_caps_preload_starts() {
    // C#: BurstPreloadSchedulerTests.ViewerDecoding_CapsPreloadStarts
    let burst = Burst::new();
    let cpus = std::thread::available_parallelism().map_or(1, |n| i32::try_from(n.get()).unwrap_or(i32::MAX));
    let cap = 8.min(2.max(cpus / 3));
    let workers = cap + 2; // strictly above the cap, so a broken cap cannot hide behind the worker count
    let target = BurstTarget::new(&burst.fs, true, false);
    target.set_active_viewer(1);
    let scheduler = burst.scheduler(&target, workers);
    let _run = scheduler.preload_around(0);
    let mut first = String::new();
    for i in 0..cap {
        let started = target.next_start();
        if i == 0 {
            first = started;
        }
    }
    // Deterministic "nothing more starts": finish one worker. That wakes the scheduler loop, which re-evaluates its
    // limit and refills exactly the freed slot.
    target.release(&first);
    target.next_start();
    assert_eq!((cap + 1) as usize, target.started_count());
    target.set_active_viewer(0);
    scheduler.notify_navigation(0); // wakes the loop; the viewer is idle again
    for _ in cap..workers {
        target.next_start();
    }
    assert_eq!((workers + 1) as usize, target.started_count()); // cap + 1 so far, plus the workers - cap that the wake ramped up
    target.release_all();
}
