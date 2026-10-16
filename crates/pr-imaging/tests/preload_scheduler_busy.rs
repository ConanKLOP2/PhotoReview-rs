//! Port of `tests/PhotoReview.Imaging.Tests/PreloadDecoderBusyTests.cs` (C# baseline d92d6aae, 239 lines): a decoder
//! refusing background work because its queue is full is a transient skip: no error, and the path is retried by a later
//! pass (cooldown of `COOLDOWN_PASSES` passes, at most `MAX_RETRIES` retries).
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Test notes: "order passes" are counted through the marker image (its `get_current_cache_key(entry)` runs once per
//! examined candidate and order pass, as in C#; cached from the start, see `BusyTarget::new`); `BusyNoted` is the
//! `set_busy_noted_for_tests` seam.

mod common;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use common::preload_fakes::*;
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::scheduler::{PreloadScheduler, RunHandle};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload_backoff::{COOLDOWN_PASSES, MAX_RETRIES};
use pr_imaging::preload_order::FORWARD_LOOKAHEAD;
use pr_test_support::FakeFileSystem;

const IMAGE_COUNT: usize = 40;
const BLOCKED_INDEX: usize = FORWARD_LOOKAHEAD as usize;
const BUSY_INDEX: usize = 2;
/// Examined by the scheduler loop right after the busy index, once per order pass.
const MARKER_INDEX: usize = BUSY_INDEX + 1;

/// C# `BusyTarget`: index `BUSY_INDEX` is refused as busy `busy_attempts` times; `BLOCKED_INDEX` blocks until released.
struct BusyTarget {
    fs: Arc<FakeFileSystem>,
    entries: Arc<[CatalogEntry]>,
    busy_attempts: usize,
    cached: Mutex<HashSet<String>>,
    preloads: Mutex<HashMap<usize, usize>>,
    index_by_path: HashMap<String, usize>,
    entered: Arc<Gate>,
    release: Arc<Gate>,
    passes: Mutex<usize>,
    busy_recorded: Mutex<usize>,
    signal: Condvar,
    blocked_released: AtomicBool,
}

impl BusyTarget {
    fn new(fs: &Arc<FakeFileSystem>, busy_attempts: usize) -> Arc<Self> {
        let entries = entries(fs, IMAGE_COUNT, "busy");
        let index_by_path = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path().to_ascii_uppercase(), i))
            .collect();
        // DIVERGENCE: the marker starts cached, so every order pass examines it (key lookup) and none dispatches it. In
        // C# its preload completes synchronously inside the pass; a Rust decode runs on a pool thread, and a marker
        // still in flight when a navigation rebuilds the order would be skipped as already queued (no pass counted).
        let marker = entries[MARKER_INDEX].path().to_ascii_uppercase();
        Arc::new(Self {
            fs: Arc::clone(fs),
            entries,
            busy_attempts,
            cached: Mutex::new(HashSet::from([marker])),
            preloads: Mutex::new(HashMap::new()),
            index_by_path,
            entered: Gate::new(),
            release: Gate::new(),
            passes: Mutex::new(0),
            busy_recorded: Mutex::new(0),
            signal: Condvar::new(),
            blocked_released: AtomicBool::new(false),
        })
    }

    fn path(&self, index: usize) -> &str {
        self.entries[index].path()
    }

    fn preload_count(&self, index: usize) -> usize {
        lock(&self.preloads).get(&index).copied().unwrap_or(0)
    }

    fn is_cached(&self, index: usize) -> bool {
        lock(&self.cached).contains(&self.path(index).to_ascii_uppercase())
    }

    fn release_blocked(&self) {
        self.blocked_released.store(true, Ordering::SeqCst);
        self.release.open();
    }

    fn set_cached(&self, index: usize, cached: bool) {
        let key = self.path(index).to_ascii_uppercase();
        if cached {
            lock(&self.cached).insert(key);
        } else {
            lock(&self.cached).remove(&key);
        }
    }

    /// C# `BusyNoted`: called by the scheduler right AFTER a busy outcome was recorded in the back-off.
    fn busy_was_recorded(&self) {
        *lock(&self.busy_recorded) += 1;
        self.signal.notify_all();
    }

    /// Waits (guarded) until the marker was examined in `pass` order passes.
    fn wait_for_passes(&self, pass: usize) {
        self.wait_counter(&self.passes, pass, "order passes");
    }

    /// Waits (guarded) until `count` busy refusals were recorded by the back-off.
    fn busy_recorded_at_least(&self, count: usize) {
        self.wait_counter(&self.busy_recorded, count, "busy records");
    }

    fn wait_counter(&self, counter: &Mutex<usize>, threshold: usize, what: &str) {
        let start = std::time::Instant::now();
        let mut value = lock(counter);
        while *value < threshold {
            assert!(start.elapsed() < GUARD, "timed out waiting for {threshold} {what}");
            let (guard, _) = self
                .signal
                .wait_timeout(value, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner);
            value = guard;
        }
    }
}

impl PreloadTarget for BusyTarget {
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        lock(&self.cached).contains(&path.to_ascii_uppercase())
    }

    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        lock(&self.cached).contains(key.path())
    }

    fn preload(&self, path: &str, _cancel: &CancelToken) -> Result<(), DecodeError> {
        let index = self.index_by_path[&path.to_ascii_uppercase()];
        let attempt = {
            let mut preloads = lock(&self.preloads);
            let count = preloads.entry(index).or_insert(0);
            *count += 1;
            *count
        };
        if index == BUSY_INDEX && attempt <= self.busy_attempts {
            return Err(DecodeError::busy());
        }
        if index == BLOCKED_INDEX {
            self.entered.open();
            self.release.wait(None);
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
        // Only the scheduler loop stats a CatalogEntry, once per examined candidate and order pass: the marker counts passes.
        if entry.path().eq_ignore_ascii_case(self.path(MARKER_INDEX)) {
            *lock(&self.passes) += 1;
            self.signal.notify_all();
        }
        self.get_current_cache_key_for_path(entry.path())
    }

    fn cache_count(&self) -> i32 {
        i32::try_from(lock(&self.cached).len()).unwrap_or(i32::MAX)
    }

    fn cache_bytes(&self) -> i64 {
        0
    }
}

/// Builds the scheduler of the busy tests (4 workers, whole-folder threshold 1, fake memory probe, BusyNoted seam).
fn busy_scheduler(target: &Arc<BusyTarget>, log: &Arc<CaptureLog>) -> PreloadScheduler {
    let scheduler = create(
        target,
        &target.entries,
        Setup::new(4, pr_imaging::preload_order::PreloadWindow::DEFAULT)
            .full_folder_threshold(1)
            .log(log),
    );
    let busy_target = Arc::clone(target);
    scheduler.set_busy_noted_for_tests(Some(Arc::new(move |_path: &str| busy_target.busy_was_recorded())));
    scheduler
}

fn lines_containing(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .filter(|m| m.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()))
        .count()
}

#[test]
fn busy_preload_is_skipped_without_error_and_retried_on_the_next_pass() {
    // C#: PreloadDecoderBusyTests.BusyPreload_IsSkippedWithoutErrorAndRetriedOnTheNextPass
    let fs = fake_fs();
    let log = CaptureLog::with_enabled(true);
    let target = BusyTarget::new(&fs, 1);
    let scheduler = busy_scheduler(&target, &log);

    let lifetime: RunHandle = scheduler.preload_around(0);
    assert!(target.entered.wait(None), "the blocked decode was entered");
    target.busy_recorded_at_least(1); // the backoff stamped pass 1 before the next pass begins
    assert_eq!(1, target.preload_count(BUSY_INDEX));
    assert!(!target.is_cached(BUSY_INDEX));

    // Navigations: same lifetime, new order passes. The busy path is cooling down for COOLDOWN_PASSES of them.
    for pass in 2..=(1 + COOLDOWN_PASSES as usize + 1) {
        assert_eq!(1, target.preload_count(BUSY_INDEX));
        let _ = scheduler.preload_around(0);
        target.wait_for_passes(pass);
    }
    target.release_blocked();
    assert!(finished(&lifetime));
    assert_eq!(
        2,
        target.preload_count(BUSY_INDEX),
        "a Failed outcome would never be retried in this lifetime"
    );
    assert!(target.is_cached(BUSY_INDEX));
    assert!(log.errors().is_empty());
    assert!(lines_containing(&log.infos(), "busy") >= 1);
}

#[test]
fn busy_path_is_not_retried_on_every_order_rebuild_but_after_the_cooldown() {
    // C#: PreloadDecoderBusyTests.BusyPath_IsNotRetriedOnEveryOrderRebuild_ButAfterTheCooldown
    let fs = fake_fs();
    let log = CaptureLog::with_enabled(true);
    let target = BusyTarget::new(&fs, usize::MAX);
    let scheduler = busy_scheduler(&target, &log);
    let _ = scheduler.preload_around(0);
    target.wait_for_passes(1);
    target.busy_recorded_at_least(1);
    let mut attempts_after_pass = vec![target.preload_count(BUSY_INDEX)];
    for pass in 2..=5 {
        let _ = scheduler.preload_around(0);
        target.wait_for_passes(pass);
        if pass == 5 {
            target.busy_recorded_at_least(2);
        }
        attempts_after_pass.push(target.preload_count(BUSY_INDEX));
    }
    target.release_blocked();
    // Passes 2..4 are inside the cooldown (no attempt); pass 5 retries.
    assert_eq!(vec![1, 1, 1, 1, 2], attempts_after_pass);
}

#[test]
fn busy_path_stops_being_retried_after_the_retry_cap_and_only_the_first_busy_is_logged_at_info() {
    // C#: PreloadDecoderBusyTests.BusyPath_StopsBeingRetriedAfterTheRetryCap_AndOnlyTheFirstBusyIsLoggedAtInfo
    let fs = fake_fs();
    let log = CaptureLog::with_enabled(true);
    let target = BusyTarget::new(&fs, usize::MAX);
    let scheduler = busy_scheduler(&target, &log);
    let _ = scheduler.preload_around(0);
    target.wait_for_passes(1);
    let mut expected_busy_attempts = 1usize;
    target.busy_recorded_at_least(expected_busy_attempts);
    let passes = 1 + (MAX_RETRIES as usize + 3) * (COOLDOWN_PASSES as usize + 1);
    for pass in 2..=passes {
        let _ = scheduler.preload_around(0);
        target.wait_for_passes(pass);
        // A retry happens exactly every COOLDOWN_PASSES + 1 passes until the cap (1 attempt + MAX_RETRIES retries) is used up.
        if (pass - 1) % (COOLDOWN_PASSES as usize + 1) == 0 && expected_busy_attempts < MAX_RETRIES as usize + 1 {
            expected_busy_attempts += 1;
            target.busy_recorded_at_least(expected_busy_attempts);
        }
    }
    target.release_blocked();
    assert_eq!(MAX_RETRIES as usize + 1, target.preload_count(BUSY_INDEX));
    assert_eq!(1, lines_containing(&log.infos(), "decoder busy"));
}

#[test]
fn busy_path_at_the_retry_cap_forgets_its_history_when_it_is_seen_cached() {
    // C#: PreloadDecoderBusyTests.BusyPathAtTheRetryCap_ForgetsItsHistoryWhenItIsSeenCached
    let fs = fake_fs();
    let log = CaptureLog::with_enabled(true);
    let target = BusyTarget::new(&fs, usize::MAX);
    let scheduler = busy_scheduler(&target, &log);
    let _ = scheduler.preload_around(0);
    target.wait_for_passes(1);
    let mut busy_attempts = 1usize;
    target.busy_recorded_at_least(busy_attempts);
    let mut pass = 1usize;
    while busy_attempts < MAX_RETRIES as usize + 1 {
        // exhaust the retry cap exactly like the test above
        pass += 1;
        let _ = scheduler.preload_around(0);
        target.wait_for_passes(pass);
        if (pass - 1) % (COOLDOWN_PASSES as usize + 1) == 0 {
            busy_attempts += 1;
            target.busy_recorded_at_least(busy_attempts);
        }
    }
    target.set_cached(BUSY_INDEX, true); // the viewer decoded it by another route
    let _ = scheduler.preload_around(0);
    pass += 1;
    target.wait_for_passes(pass);
    target.set_cached(BUSY_INDEX, false); // ... and the entry was evicted later
    let _ = scheduler.preload_around(0);
    pass += 1;
    target.wait_for_passes(pass);
    target.busy_recorded_at_least(busy_attempts + 1);
    target.release_blocked();
    assert_eq!(
        MAX_RETRIES as usize + 2,
        target.preload_count(BUSY_INDEX),
        "capped forever before the fix"
    );
}
