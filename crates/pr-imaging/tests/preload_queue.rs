//! Queue-level twins of the C# preload tests (C# baseline d92d6aae), driving `RunQueue::refresh` / `next` / `on_finished`
//! directly with a fake `PreloadTarget`, a `PreviewSizeSampler`, a `PreloadBusyBackoff` and a manual clock. No thread,
//! no worker, no sleep: the caller thread of the C# run is a flag the driver passes to `next`.
//!
//! Sources: `PreloadSchedulerGapTests.cs` (pause log), `PreloadSchedulerMutationTests.cs` (order, headroom, caller-thread
//! window, whole-folder fill limit, candidate failures, centre handling, injected shape), `PreloadEstimateCalibrationTests.cs`,
//! `PreloadDecoderBusyTests.cs` (cooldown), `PreloadRequeueTests.cs`. Each twin is named `<csharp_name>_queue` and carries
//! a `// C#:` comment. Port of `PhotoReview.Imaging/Preload/PreloadScheduler.cs` lines 475-688 (the queue half).
//!
//! Red tests (W4-15a): `crates/pr-imaging/src/preload/queue.rs` is `todo!()`, so these tests compile against the stub and
//! FAIL until the porter implements the queue.
//!
//! Documented skips and substitutions:
//! - The C# end-to-end tests (`PreloadScheduler` with threads, workers, `PreloadAroundAsync`) are W4-15b. Here the caller
//!   thread is the `on_caller_thread` argument of `next`; `LeaveCallerThread` plays the role of the C# yield.
//! - The C# `Thread`-identity assertions become "which candidates were examined with `on_caller_thread = true`".
//! - The C# worker gates and in-flight counts do not exist here: every dispatched candidate is finished at once, by
//!   `Cached` (`finish_cached`), `Busy` or `Failed` as the test needs.
//! - `DiagWorkerEnvironmentVariable_OverridesTheOption`, the `Preload*` tests of other units and the calibration
//!   `Sampler_NeedsMinimumSamples_AndRestartsOnBoxChange` are not here (W4-15b, W4-06b).
//! - Only the pause-line perf event is not observable; the snapshot-cost rule is pinned by counting snapshot calls.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::policy::{PreloadOutcome, Shape};
use pr_imaging::preload::queue::{Candidate, PassInputs, QueueStep, RunQueue};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload_backoff::{PreloadBusyBackoff, PreviewSizeSampler};
use pr_imaging::preload_order::{PreloadOptions, PreloadWindow};
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog};

// ---- fakes ----------------------------------------------------------------------------------------------------------

/// How the fake memory probe answers `has_headroom` (the C# `Probe`, `LoadProbe` and `FakeMemoryProbe` fakes).
enum Headroom {
    /// Always the same answer (C# `FakeMemoryProbe(bool)`).
    Fixed(bool),
    /// `load < maximum_load` (C# `LoadProbe`).
    Load(f64),
    /// Headroom until the first decode was queued (C# `Probe(() => target.Started.IsEmpty)`).
    UntilDecodeQueued,
}

struct FakeProbe {
    policy: Headroom,
    snapshot: Option<MemorySnapshot>,
    /// Decodes the driver finished as dispatched (drives `Headroom::UntilDecodeQueued`).
    dispatches: Arc<AtomicUsize>,
    headroom_calls: AtomicUsize,
    snapshot_calls: AtomicUsize,
    limits: Mutex<Vec<(f64, i64)>>,
}

impl FakeProbe {
    /// Answers with `policy`; the snapshot is C# `FakeMemoryProbe`'s `(50, 16 GiB)` unless overridden.
    fn new(policy: Headroom) -> Self {
        Self {
            policy,
            snapshot: Some(MemorySnapshot {
                load_percent: 50,
                available_bytes: 16 * 1024 * 1024 * 1024,
            }),
            dispatches: Arc::new(AtomicUsize::new(0)),
            headroom_calls: AtomicUsize::new(0),
            snapshot_calls: AtomicUsize::new(0),
            limits: Mutex::new(Vec::new()),
        }
    }

    fn with_snapshot(mut self, snapshot: MemorySnapshot) -> Self {
        self.snapshot = Some(snapshot);
        self
    }

    fn headroom_calls(&self) -> usize {
        self.headroom_calls.load(Ordering::SeqCst)
    }

    fn snapshot_calls(&self) -> usize {
        self.snapshot_calls.load(Ordering::SeqCst)
    }

    fn reset_snapshot_calls(&self) {
        self.snapshot_calls.store(0, Ordering::SeqCst);
    }
}

impl MemoryProbe for FakeProbe {
    fn has_headroom(&self, maximum_load: f64, reserve_bytes: i64) -> bool {
        self.headroom_calls.fetch_add(1, Ordering::SeqCst);
        self.limits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((maximum_load, reserve_bytes));
        match &self.policy {
            Headroom::Fixed(answer) => *answer,
            Headroom::Load(load) => *load < maximum_load,
            Headroom::UntilDecodeQueued => self.dispatches.load(Ordering::SeqCst) == 0,
        }
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
        self.snapshot
    }
}

#[derive(Debug, Clone, Copy)]
enum KeyFailure {
    /// C# `IOException` / `UnauthorizedAccessException`: `KeyError::Io` with this kind.
    Io(io::ErrorKind),
    /// C# any other exception: `KeyError::Other`.
    Other,
}

/// File-free preview cache over a catalog snapshot. Keys come from the entries' metadata (no file system access).
struct FakeTarget {
    fs: FakeFileSystem,
    entries: Arc<[CatalogEntry]>,
    key_box: DecodeBox,
    /// Indices whose preview is in the RAM cache.
    cached: Mutex<HashSet<usize>>,
    /// Bytes per cached preview; `cache_bytes` = `fixed_cache_bytes + cached * per_image_bytes`.
    per_image_bytes: i64,
    fixed_cache_bytes: i64,
    key_failures: HashMap<usize, KeyFailure>,
    key_calls: Mutex<HashMap<usize, usize>>,
}

impl FakeTarget {
    fn new(entries: Arc<[CatalogEntry]>, key_box: DecodeBox) -> Self {
        Self {
            fs: FakeFileSystem::new(),
            entries,
            key_box,
            cached: Mutex::new(HashSet::new()),
            per_image_bytes: 0,
            fixed_cache_bytes: 0,
            key_failures: HashMap::new(),
            key_calls: Mutex::new(HashMap::new()),
        }
    }

    fn index_of(&self, path: &str) -> usize {
        self.entries
            .iter()
            .position(|entry| entry.path().eq_ignore_ascii_case(path))
            .unwrap_or_else(|| panic!("{path} is not in the catalog"))
    }

    fn mark_cached(&self, index: usize) {
        self.cached.lock().unwrap_or_else(PoisonError::into_inner).insert(index);
    }

    fn evict(&self, index: usize) {
        self.cached
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&index);
    }

    fn is_cached(&self, index: usize) -> bool {
        self.cached
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&index)
    }

    fn cached_count(&self) -> usize {
        self.cached.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// How often the queue asked for the key of entry `index` (C# `KeyCalls`).
    fn key_calls(&self, index: usize) -> usize {
        self.key_calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&index)
            .copied()
            .unwrap_or(0)
    }
}

impl PreloadTarget for FakeTarget {
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        self.is_cached(self.index_of(path))
    }

    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        self.is_cached(self.index_of(key.path()))
    }

    fn preload(&self, _path: &str, _cancel: &CancelToken) -> Result<(), DecodeError> {
        unreachable!("the queue never decodes: workers are W4-15b")
    }

    fn get_current_cache_key_for_path(&self, _path: &str) -> Result<ImageCacheKey, KeyError> {
        unreachable!("the queue asks for keys by entry only")
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        let index = self.index_of(entry.path());
        *self
            .key_calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(index)
            .or_insert(0) += 1;
        match self.key_failures.get(&index) {
            Some(KeyFailure::Io(kind)) => Err(KeyError::Io(io::Error::new(*kind, "test failure"))),
            Some(KeyFailure::Other) => Err(KeyError::Other("malformed path".into())),
            None => Ok(
                ImageCacheKey::create_for_entry(&self.fs, entry, false, self.key_box, KeyOptions::default())
                    .expect("entries carry their metadata, so no file is touched"),
            ),
        }
    }

    fn cache_count(&self) -> i32 {
        self.cached_count() as i32
    }

    fn cache_bytes(&self) -> i64 {
        self.fixed_cache_bytes + self.cached_count() as i64 * self.per_image_bytes
    }
}

// ---- harness --------------------------------------------------------------------------------------------------------

/// Key box of the default fixture; the calibration tests use their own.
const KEY_BOX: DecodeBox = DecodeBox::new(100, 100);
/// C# `PreloadEstimateCalibrationTests.Budget`: 100 MiB.
const BUDGET: i64 = 100 * 1024 * 1024;

fn path_of(index: usize) -> String {
    format!(r"C:\queue-test\img-{index:03}.jpg")
}

fn catalog(count: usize) -> Arc<[CatalogEntry]> {
    let written = DateTimeUtc::from_ticks(638_000_000_000_000_000).expect("valid ticks");
    (0..count)
        .map(|index| {
            CatalogEntry::new(path_of(index)).expect("valid path").with_metadata(
                1000 + index as i64,
                written,
                None,
                None,
            )
        })
        .collect::<Vec<_>>()
        .into()
}

fn options(window: PreloadWindow, whole_folder: bool, workers: i32) -> PreloadOptions {
    PreloadOptions {
        worker_count: workers,
        full_folder_threshold_bytes: if whole_folder { 1_000_000_000 } else { -1 },
        window,
        ..PreloadOptions::default()
    }
}

/// Budgeted whole-folder options (C# `FullFolderThresholdBytes: Budget`).
fn budget_options(window: PreloadWindow, workers: i32) -> PreloadOptions {
    PreloadOptions {
        worker_count: workers,
        full_folder_threshold_bytes: BUDGET,
        window,
        ..PreloadOptions::default()
    }
}

fn inputs(version: i64, center: i32) -> PassInputs {
    PassInputs {
        version,
        center,
        shape: Shape::INITIAL,
    }
}

/// One dispatched candidate as the driver saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    index: i32,
    on_caller: bool,
}

struct Harness {
    target: FakeTarget,
    probe: FakeProbe,
    sizes: PreviewSizeSampler,
    backoff: PreloadBusyBackoff,
    log: RecordingLog,
    options: PreloadOptions,
    clock: Arc<AtomicI64>,
    clock_fn: Box<dyn Fn() -> i64 + Sync>,
    perf_enabled: bool,
    /// Measured decoded size of every preview that lands (C# `CachedPreviewBytes`); `None` = nothing measured.
    preview_bytes: Option<i64>,
    entries: Arc<[CatalogEntry]>,
}

impl Harness {
    fn new(count: usize, options: PreloadOptions, probe: FakeProbe, key_box: DecodeBox) -> Self {
        let entries = catalog(count);
        let clock = Arc::new(AtomicI64::new(1_000_000));
        let clock_source = Arc::clone(&clock);
        Self {
            target: FakeTarget::new(Arc::clone(&entries), key_box),
            probe,
            sizes: PreviewSizeSampler::new(),
            backoff: PreloadBusyBackoff::new(),
            log: RecordingLog::new(),
            options,
            clock,
            clock_fn: Box::new(move || clock_source.load(Ordering::SeqCst)),
            perf_enabled: false,
            preview_bytes: None,
            entries,
        }
    }

    fn ctx(&self) -> pr_imaging::preload::queue::QueueContext<'_> {
        pr_imaging::preload::queue::QueueContext {
            target: &self.target,
            memory_probe: &self.probe,
            sizes: &self.sizes,
            busy_backoff: &self.backoff,
            log: &self.log,
            options: &self.options,
            clock_ms: &*self.clock_fn,
            perf_enabled: self.perf_enabled,
        }
    }

    fn queue(&self, workers: i32) -> RunQueue {
        RunQueue::new(Arc::clone(&self.entries), workers)
    }

    /// The worker's job for a successful decode: the preview is cached, its size is measured, the queue is told.
    fn finish_cached(&self, queue: &mut RunQueue, candidate: &Candidate) {
        self.target.mark_cached(candidate.index as usize);
        self.probe.dispatches.fetch_add(1, Ordering::SeqCst);
        if let Some(bytes) = self.preview_bytes {
            self.sizes.record(self.target.key_box, bytes);
        }
        queue.on_finished(&candidate.path, PreloadOutcome::Cached);
    }

    /// Runs one pass to its end the way the run loop does: dispatched candidates finish as `Cached`, a `LeaveCallerThread`
    /// moves the rest of the pass off the caller thread. Returns the dispatches and the step that ended the pass.
    fn drain(&self, queue: &mut RunQueue, on_caller_start: bool) -> (Vec<Seen>, QueueStep) {
        let cancel = CancelToken::new();
        let mut on_caller = on_caller_start;
        let mut seen = Vec::new();
        loop {
            let step = queue
                .next(&self.ctx(), on_caller, &cancel)
                .expect("no key error in this scenario");
            match step {
                QueueStep::Dispatch(candidate) => {
                    seen.push(Seen {
                        index: candidate.index,
                        on_caller,
                    });
                    self.finish_cached(queue, &candidate);
                }
                QueueStep::Skipped => {}
                QueueStep::LeaveCallerThread => on_caller = false,
                end => return (seen, end),
            }
        }
    }

    /// Log messages of one level, in order.
    fn messages(&self, level: LogLevel) -> Vec<String> {
        self.log
            .entries()
            .into_iter()
            .filter(|(entry_level, _)| *entry_level == level)
            .map(|(_, message)| message)
            .collect()
    }
}

fn next_candidate(harness: &Harness, queue: &mut RunQueue) -> Candidate {
    match queue
        .next(&harness.ctx(), true, &CancelToken::new())
        .expect("no key error in this scenario")
    {
        QueueStep::Dispatch(candidate) => candidate,
        other => panic!("expected a dispatch, got {other:?}"),
    }
}

fn indices(seen: &[Seen]) -> Vec<i32> {
    seen.iter().map(|s| s.index).collect()
}

fn unique(seen: &[Seen]) -> HashSet<i32> {
    seen.iter().map(|s| s.index).collect()
}

// ---- pause log (PreloadSchedulerGapTests.cs) -------------------------------------------------------------------------

// C#: PreloadSchedulerGapTests.PausedForMemory_LogsTheSnapshot (queue-level twin)
#[test]
fn paused_for_memory_logs_the_snapshot_queue() {
    let probe = FakeProbe::new(Headroom::Fixed(false)).with_snapshot(MemorySnapshot {
        load_percent: 77,
        available_bytes: 123_456_789,
    });
    let harness = Harness::new(5, options(PreloadWindow::DEFAULT, false, 2), probe, KEY_BOX);
    let mut queue = harness.queue(2);
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));

    let step = queue.next(&harness.ctx(), true, &CancelToken::new()).expect("next");

    assert_eq!(step, QueueStep::Paused);
    assert!(queue.is_paused());
    let paused: Vec<String> = harness
        .messages(LogLevel::Info)
        .into_iter()
        .filter(|message| message.starts_with("Preload paused for memory"))
        .collect();
    assert_eq!(paused.len(), 1, "exactly one pause line");
    assert!(paused[0].contains("availableBytes=123456789"), "{}", paused[0]);
    assert!(paused[0].contains("loadPercent=77"), "{}", paused[0]);
}

// ---- order, headroom and the caller thread (PreloadSchedulerMutationTests.cs) -----------------------------------------

// C#: PreloadSchedulerMutationTests.Order_IsRebuiltOnlyOnChange (queue-level twin)
#[test]
fn order_is_rebuilt_only_on_change_queue() {
    let harness = Harness::new(
        10,
        options(PreloadWindow::new(4, 0), false, 1),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(1);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (seen, _) = harness.drain(&mut queue, true);

    assert_eq!(indices(&seen), vec![1, 2, 3, 4]);
    // The centre entry is only ever keyed when the order (and its decode box) is rebuilt.
    assert_eq!(harness.target.key_calls(0), 1);
    // Same inputs: no rebuild. A new shape (direction or lead) does rebuild.
    assert!(!queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let reversed = PassInputs {
        version: 0,
        center: 0,
        shape: Shape { direction: -1, lead: 0 },
    };
    assert!(queue.refresh(&harness.ctx(), reversed).expect("refresh"));
}

// C#: PreloadSchedulerMutationTests.Headroom_IsRecheckedAfterEveryQueuedDecode (queue-level twin)
#[test]
fn headroom_is_rechecked_after_every_queued_decode_queue() {
    let harness = Harness::new(
        10,
        options(PreloadWindow::new(8, 0), false, 4),
        FakeProbe::new(Headroom::UntilDecodeQueued),
        KEY_BOX,
    );
    let mut queue = harness.queue(4);
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));

    let first = next_candidate(&harness, &mut queue);
    harness.finish_cached(&mut queue, &first);
    let second = queue.next(&harness.ctx(), true, &CancelToken::new()).expect("next");

    // The first decode was queued: the answer is re-probed, it is now "no headroom", the run pauses.
    assert_eq!(second, QueueStep::Paused);
    assert!(queue.is_paused());
    assert_eq!(harness.probe.dispatches.load(Ordering::SeqCst), 1, "one decode started");
}

// C#: PreloadSchedulerMutationTests.Headroom_IsCachedWhileNothingIsQueued (queue-level twin, with the manual clock)
#[test]
fn headroom_is_cached_while_nothing_is_queued_queue() {
    let harness = Harness::new(
        14,
        options(PreloadWindow::new(13, 0), false, 2),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    for index in 0..14 {
        harness.target.mark_cached(index);
    }
    let mut queue = harness.queue(2);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let before = harness.probe.headroom_calls();
    let (seen, _) = harness.drain(&mut queue, true);
    assert!(seen.is_empty(), "everything is cached: nothing is dispatched");
    let probes = harness.probe.headroom_calls() - before;
    assert!((1..=5).contains(&probes), "one real probe, not one per image: {probes}");

    // Exactly 50 ms after that probe the cached answer still stands (the re-probe needs more than 50 ms).
    harness.clock.store(1_000_050, Ordering::SeqCst);
    assert!(queue.refresh(&harness.ctx(), inputs(1, 0)).expect("refresh"));
    let before = harness.probe.headroom_calls();
    harness.drain(&mut queue, true);
    assert_eq!(harness.probe.headroom_calls(), before, "no probe at 50 ms");

    // 51 ms: one probe for the whole pass.
    harness.clock.store(1_000_051, Ordering::SeqCst);
    assert!(queue.refresh(&harness.ctx(), inputs(2, 0)).expect("refresh"));
    let before = harness.probe.headroom_calls();
    harness.drain(&mut queue, true);
    assert_eq!(harness.probe.headroom_calls(), before + 1, "one probe at 51 ms");
}

// C#: PreloadSchedulerMutationTests.OnlyTheWindowRunsOnTheCallerThread (queue-level twin, both directions)
#[test]
fn only_the_window_runs_on_the_callers_thread_queue() {
    for direction in [1, -1] {
        let harness = Harness::new(
            30,
            options(PreloadWindow::new(3, 2), true, 16),
            FakeProbe::new(Headroom::Fixed(true)),
            KEY_BOX,
        );
        let mut queue = harness.queue(16);
        let shape = Shape { direction, lead: 0 };
        assert!(
            queue
                .refresh(
                    &harness.ctx(),
                    PassInputs {
                        version: 0,
                        center: 10,
                        shape,
                    },
                )
                .expect("refresh")
        );

        let (seen, end) = harness.drain(&mut queue, true);

        assert_eq!(end, QueueStep::Exhausted);
        let on_caller: HashSet<i32> = seen.iter().filter(|s| s.on_caller).map(|s| s.index).collect();
        let expected: HashSet<i32> = [
            10 + direction,
            10 + 2 * direction,
            10 + 3 * direction,
            10 - direction,
            10 - 2 * direction,
        ]
        .into_iter()
        .collect();
        assert_eq!(on_caller, expected, "direction {direction}: the window only");
        // Whole folder: every other image is examined, and the ones beyond the window run off the caller thread.
        assert_eq!(unique(&seen).len(), 29, "direction {direction}: the whole folder");
    }
}

// C#: PreloadSchedulerMutationTests.InjectedPace_DrivesTheOrderDirection (queue-level twin: shape {-1, 0})
#[test]
fn injected_pace_drives_the_order_direction_queue() {
    let harness = Harness::new(
        20,
        options(PreloadWindow::new(2, 2), false, 1),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(1);
    assert!(
        queue
            .refresh(
                &harness.ctx(),
                PassInputs {
                    version: 0,
                    center: 9,
                    shape: Shape { direction: -1, lead: 0 },
                },
            )
            .expect("refresh")
    );

    let (seen, _) = harness.drain(&mut queue, true);

    assert_eq!(indices(&seen), vec![8, 7, 10, 11]);
}

// ---- whole-folder fill limit and candidate failures ------------------------------------------------------------------

// C#: PreloadSchedulerMutationTests.WholeFolder_StopsAtTheFillLimit (queue-level twin, both rows)
#[test]
fn whole_folder_stops_at_the_fill_limit_queue() {
    // (cache bytes, the pass reaches the rest of the folder)
    for (cache_bytes, reaches_the_rest) in [(900_000_000_i64, false), (899_999_999, true)] {
        let mut harness = Harness::new(
            20,
            options(PreloadWindow::new(2, 1), true, 4),
            FakeProbe::new(Headroom::Fixed(true)),
            KEY_BOX,
        );
        harness.target.fixed_cache_bytes = cache_bytes;
        let mut queue = harness.queue(4);
        assert!(queue.refresh(&harness.ctx(), inputs(0, 5)).expect("refresh"));

        let (seen, end) = harness.drain(&mut queue, true);

        let started = unique(&seen);
        assert!(started.contains(&6) && started.contains(&7) && started.contains(&4));
        assert_eq!(started.contains(&15), reaches_the_rest);
        assert_eq!(started.len(), if reaches_the_rest { 19 } else { 3 });
        let expected_end = if reaches_the_rest {
            QueueStep::Exhausted
        } else {
            QueueStep::FillLimitStop
        };
        assert_eq!(end, expected_end, "cache {cache_bytes}");
    }
}

// C#: PreloadSchedulerMutationTests.CandidateKeyFailure_SkipsOnlyThatCandidate (queue-level twin, io and access)
#[test]
fn candidate_key_failure_skips_only_that_candidate_queue() {
    for kind in [io::ErrorKind::NotFound, io::ErrorKind::PermissionDenied] {
        let mut harness = Harness::new(
            8,
            options(PreloadWindow::new(6, 0), false, 2),
            FakeProbe::new(Headroom::Fixed(true)),
            KEY_BOX,
        );
        harness.target.key_failures.insert(2, KeyFailure::Io(kind));
        let mut queue = harness.queue(2);
        assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));

        let (seen, end) = harness.drain(&mut queue, true);

        assert_eq!(end, QueueStep::Exhausted, "{kind:?}");
        assert_eq!(unique(&seen), HashSet::from([1, 3, 4, 5, 6]), "{kind:?}");
        let warnings = harness.messages(LogLevel::Warn);
        assert!(
            warnings.iter().any(|w| w.contains(&path_of(2))),
            "{kind:?}: {warnings:?}"
        );
        assert!(harness.messages(LogLevel::Error).is_empty(), "{kind:?}: no error line");
    }
}

// C#: PreloadSchedulerMutationTests.CentreKeyFailure_FallsBackToUnboundedBox (queue-level twin)
#[test]
fn centre_key_failure_falls_back_to_unbounded_box_queue() {
    let mut harness = Harness::new(
        6,
        options(PreloadWindow::new(3, 0), false, 2),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    harness
        .target
        .key_failures
        .insert(0, KeyFailure::Io(io::ErrorKind::NotFound));
    let mut queue = harness.queue(2);

    assert!(
        queue
            .refresh(&harness.ctx(), inputs(0, 0))
            .expect("an I/O failure at the centre is not an error")
    );
    let (seen, _) = harness.drain(&mut queue, true);

    assert_eq!(indices(&seen), vec![1, 2, 3]);
    assert!(harness.messages(LogLevel::Error).is_empty());
}

// C#: PreloadSchedulerMutationTests.CentreOutOfRange_IsANoOpNotAnError (queue-level twin, -1 / len / 100)
#[test]
fn centre_out_of_range_is_a_no_op_not_an_error_queue() {
    for center in [-1, 6, 100] {
        let harness = Harness::new(
            6,
            options(PreloadWindow::new(3, 0), false, 2),
            FakeProbe::new(Headroom::Fixed(true)),
            KEY_BOX,
        );
        let mut queue = harness.queue(2);
        assert!(
            queue.refresh(&harness.ctx(), inputs(0, center)).expect("refresh"),
            "centre {center}"
        );

        let (seen, end) = harness.drain(&mut queue, true);

        assert!(seen.is_empty(), "centre {center}");
        assert_eq!(end, QueueStep::Exhausted, "centre {center}");
        assert!(harness.messages(LogLevel::Error).is_empty(), "centre {center}");
    }
}

// Contract semantics 10 (Rust-only): an error other than I/O at the centre ends the run (`refresh` fails).
#[test]
fn centre_key_other_error_ends_the_run_queue() {
    let mut harness = Harness::new(
        6,
        options(PreloadWindow::new(3, 0), false, 2),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    harness.target.key_failures.insert(0, KeyFailure::Other);
    let mut queue = harness.queue(2);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).is_err());
}

// Contract semantics 10 (Rust-only): an error other than I/O at a candidate is a queue error, not a skip.
#[test]
fn candidate_key_other_error_is_a_queue_error() {
    let mut harness = Harness::new(
        6,
        options(PreloadWindow::new(3, 0), false, 2),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    harness.target.key_failures.insert(2, KeyFailure::Other);
    let mut queue = harness.queue(2);
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    assert_eq!(next_candidate(&harness, &mut queue).index, 1);

    assert!(queue.next(&harness.ctx(), true, &CancelToken::new()).is_err());
}

// ---- calibration (PreloadEstimateCalibrationTests.cs) ----------------------------------------------------------------

/// The C# calibration fixture: 200 images, a 1000 x 1000 box bound (4 MB per image, 800 MB for the folder).
const CALIBRATION_BOX: DecodeBox = DecodeBox::new(1000, 1000);
const CALIBRATION_IMAGES: usize = 200;

// C#: PreloadEstimateCalibrationTests.SmallMeasuredPreviews_UnlockWholeFolder (queue-level twin)
#[test]
fn small_measured_previews_unlock_whole_folder_queue() {
    let mut harness = Harness::new(
        CALIBRATION_IMAGES,
        budget_options(PreloadWindow::DEFAULT, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        CALIBRATION_BOX,
    );
    harness.preview_bytes = Some(100_000); // 200 x 100 KB x 1.25 = 25 MB < budget
    let mut queue = harness.queue(4);

    // Box bound over budget: the window only.
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (first, end) = harness.drain(&mut queue, true);
    assert_eq!(end, QueueStep::Exhausted);
    assert_eq!(unique(&first).len(), 32, "the 32-ahead window");

    // The measured previews unlock the whole folder: the shell takes one more pass and the order is rebuilt.
    assert!(queue.calibration_changed(&harness.sizes));
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (second, _) = harness.drain(&mut queue, true);

    let mut everything = unique(&first);
    everything.extend(unique(&second));
    assert_eq!(everything.len(), CALIBRATION_IMAGES - 1, "the whole folder");
}

// C#: PreloadEstimateCalibrationTests.LargeMeasuredPreviews_KeepWindow (queue-level twin)
#[test]
fn large_measured_previews_keep_window_queue() {
    let mut harness = Harness::new(
        CALIBRATION_IMAGES,
        budget_options(PreloadWindow::DEFAULT, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        CALIBRATION_BOX,
    );
    harness.preview_bytes = Some(3_000_000); // 200 x 3 MB x 1.25 = 750 MB > budget
    let mut queue = harness.queue(4);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (first, _) = harness.drain(&mut queue, true);
    assert!(queue.calibration_changed(&harness.sizes));
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (second, end) = harness.drain(&mut queue, true);

    assert!(second.is_empty(), "still the window: nothing new beyond it");
    assert_eq!(end, QueueStep::Exhausted);
    assert_eq!(unique(&first).len(), 32, "PreloadOrderService.ForwardLookahead");
}

// C#: PreloadEstimateCalibrationTests.NoMeasurement_UsesBoxBound (queue-level twin)
#[test]
fn no_measurement_uses_box_bound_queue() {
    let harness = Harness::new(
        CALIBRATION_IMAGES,
        budget_options(PreloadWindow::DEFAULT, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        CALIBRATION_BOX,
    );
    let mut queue = harness.queue(4);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (seen, _) = harness.drain(&mut queue, true);

    assert!(!queue.calibration_changed(&harness.sizes), "nothing was measured");
    assert_eq!(unique(&seen).len(), 32);
}

// C#: PreloadEstimateCalibrationTests.BoxBoundWithinBudget_WholeFolderFromStart (queue-level twin)
#[test]
fn box_bound_within_budget_whole_folder_from_start_queue() {
    let harness = Harness::new(
        CALIBRATION_IMAGES,
        budget_options(PreloadWindow::DEFAULT, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        DecodeBox::new(100, 100), // 200 x 40 KB = 8 MB
    );
    let mut queue = harness.queue(4);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (seen, _) = harness.drain(&mut queue, true);

    assert_eq!(unique(&seen).len(), CALIBRATION_IMAGES - 1);
}

// C#: PreloadEstimateCalibrationTests.WholeFolderDecision_UsesConfiguredMemoryLoadLimit (queue-level twin)
#[test]
fn whole_folder_decision_uses_configured_memory_load_limit_queue() {
    // Load 0.85: above the 0.80 default, below the configured 0.95.
    let harness = Harness::new(
        CALIBRATION_IMAGES,
        PreloadOptions {
            memory_load_limit: 0.95,
            ..budget_options(PreloadWindow::DEFAULT, 4)
        },
        FakeProbe::new(Headroom::Load(0.85)),
        DecodeBox::new(100, 100),
    );
    let mut queue = harness.queue(4);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (seen, _) = harness.drain(&mut queue, true);

    assert_eq!(unique(&seen).len(), CALIBRATION_IMAGES - 1);
}

// C#: PreloadEstimateCalibrationTests.WholeFolder_StopsAtCacheFillLimit (queue-level twin)
#[test]
fn whole_folder_stops_at_cache_fill_limit_queue() {
    // The estimate fits (8 MB), but each preview really holds 1 MiB: past ~90 images a far decode would evict a near one.
    let mut harness = Harness::new(
        CALIBRATION_IMAGES,
        budget_options(PreloadWindow::DEFAULT, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        DecodeBox::new(100, 100),
    );
    harness.target.per_image_bytes = 1024 * 1024;
    let mut queue = harness.queue(4);

    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let (seen, _) = harness.drain(&mut queue, true);

    let limit = (BUDGET as f64 * 0.9 / (1024.0 * 1024.0)).ceil() as usize;
    let preloaded = unique(&seen).len();
    assert!(
        (32..=limit + 4).contains(&preloaded),
        "{preloaded} previews, expected between the window and the fill limit (+4 in flight)"
    );
}

// ---- decoder busy (PreloadDecoderBusyTests.cs) -------------------------------------------------------------------------

// C#: PreloadDecoderBusyTests.BusyPath_IsNotRetriedOnEveryOrderRebuild_ButAfterTheCooldown (queue-level twin)
#[test]
fn busy_path_is_not_retried_on_every_order_rebuild_but_after_the_cooldown_queue() {
    // Window of one image: the order is [1] on every pass; image 1 is "busy" each time it is attempted.
    let harness = Harness::new(
        4,
        options(PreloadWindow::new(1, 0), false, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(4);
    let cancel = CancelToken::new();
    let mut attempts_after_pass = Vec::new();
    let mut attempts = 0;

    for pass in 1..=5 {
        assert!(queue.refresh(&harness.ctx(), inputs(pass, 0)).expect("refresh"));
        assert_eq!(
            harness.backoff.pass_for_tests(),
            pass as i32,
            "one begin_pass per rebuild"
        );
        loop {
            match queue.next(&harness.ctx(), true, &cancel).expect("next") {
                QueueStep::Dispatch(candidate) => {
                    attempts += 1;
                    let _busy_count = harness.backoff.note_busy(&candidate.path, &candidate.key);
                    queue.on_finished(&candidate.path, PreloadOutcome::Busy);
                }
                QueueStep::Skipped => {}
                QueueStep::Exhausted => break,
                other => panic!("unexpected step {other:?}"),
            }
        }
        attempts_after_pass.push(attempts);
    }

    // Passes 2..4 are inside the cooldown (no attempt); pass 5 retries.
    assert_eq!(attempts_after_pass, vec![1, 1, 1, 1, 2]);
}

// ---- requeue (PreloadRequeueTests.cs) -------------------------------------------------------------------------------

// C#: PreloadRequeueTests.EvictedPreview_IsRequeued_WithinSameLifetime (queue-level twin)
#[test]
fn evicted_preview_is_requeued_within_same_lifetime_queue() {
    let harness = Harness::new(
        10,
        options(PreloadWindow::new(3, 0), false, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(4);
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));

    let first = next_candidate(&harness, &mut queue);
    assert_eq!(first.index, 1);
    harness.finish_cached(&mut queue, &first);

    harness.target.evict(1); // e.g. a compare/zoom decode pushed it out of the LRU
    assert!(queue.refresh(&harness.ctx(), inputs(1, 0)).expect("refresh")); // navigation: same lifetime, new order pass
    let again = next_candidate(&harness, &mut queue);

    assert_eq!(again.index, 1, "the evicted preview is queued again");
    harness.finish_cached(&mut queue, &again);
    assert!(harness.target.is_cached(1));
}

// C#: PreloadRequeueTests.FailedPreload_IsNotRetried_WithinSameLifetime (queue-level twin)
#[test]
fn failed_preload_is_not_retried_within_same_lifetime_queue() {
    let harness = Harness::new(
        10,
        options(PreloadWindow::new(3, 0), false, 4),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(4);
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));

    let first = next_candidate(&harness, &mut queue);
    harness.finish_cached(&mut queue, &first);
    let failing = next_candidate(&harness, &mut queue);
    assert_eq!(failing.index, 2);
    queue.on_finished(&failing.path, PreloadOutcome::Failed); // unreadable: stays queued for this lifetime

    assert!(queue.refresh(&harness.ctx(), inputs(1, 0)).expect("refresh"));
    let (seen, end) = harness.drain(&mut queue, true);

    assert_eq!(end, QueueStep::Exhausted);
    assert_eq!(
        indices(&seen),
        vec![3],
        "image 1 is cached, image 2 is not retried, image 3 is new"
    );
}

// ---- Rust-only rules (contract semantics 9, 10, 11, 13) ---------------------------------------------------------------

#[test]
fn refresh_records_what_the_order_was_built_for() {
    let harness = Harness::new(
        10,
        options(PreloadWindow::new(2, 0), false, 2),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(2);
    assert_eq!(queue.seen_version(), -1, "nothing built yet");

    assert!(queue.refresh(&harness.ctx(), inputs(3, 7)).expect("refresh"));

    assert_eq!(queue.seen_version(), 3);
    assert_eq!(queue.order_center(), 7);
    assert_eq!(queue.seen_shape(), Shape::INITIAL);
}

// Contract semantics 10 (Rust-only): cancellation is checked after the candidate is consumed.
#[test]
fn canceled_pass_consumes_the_candidate_it_stops_on() {
    let harness = Harness::new(
        5,
        options(PreloadWindow::new(3, 0), false, 2),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(2);
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    let canceled = CancelToken::new();
    canceled.cancel();

    assert_eq!(
        queue.next(&harness.ctx(), true, &canceled).expect("next"),
        QueueStep::Canceled
    );
    // With a live token the next candidate is image 2: image 1 was consumed by the cancelled step.
    assert_eq!(next_candidate(&harness, &mut queue).index, 2);
}

// Contract semantics 11 (Rust-only): the memory snapshot is taken only when the pause line or the perf trace uses it.
#[test]
fn pause_line_takes_a_snapshot_only_when_logged_or_traced() {
    // (log enabled, perf enabled, snapshot calls expected)
    for (log_enabled, perf_enabled, expected) in [(false, false, 0), (true, false, 1), (false, true, 1)] {
        let mut harness = Harness::new(
            5,
            options(PreloadWindow::new(3, 0), false, 2),
            FakeProbe::new(Headroom::Fixed(false)),
            KEY_BOX,
        );
        harness.log = RecordingLog::with_enabled(log_enabled);
        harness.perf_enabled = perf_enabled;
        let mut queue = harness.queue(2);
        assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
        harness.probe.reset_snapshot_calls();

        let step = queue.next(&harness.ctx(), true, &CancelToken::new()).expect("next");

        assert_eq!(step, QueueStep::Paused, "log {log_enabled}, perf {perf_enabled}");
        assert_eq!(
            harness.probe.snapshot_calls(),
            expected,
            "log {log_enabled}, perf {perf_enabled}"
        );
    }
}

// Contract semantics 9 (Rust-only): the policy line is written only when the log is enabled.
#[test]
fn policy_line_is_written_only_when_the_log_is_enabled() {
    for log_enabled in [false, true] {
        let mut harness = Harness::new(
            10,
            options(PreloadWindow::new(3, 0), false, 2),
            FakeProbe::new(Headroom::Fixed(true)),
            KEY_BOX,
        );
        harness.log = RecordingLog::with_enabled(log_enabled);
        let mut queue = harness.queue(2);

        assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));

        let policy_lines: Vec<String> = harness
            .messages(LogLevel::Info)
            .into_iter()
            .filter(|message| message.starts_with("Preload policy:"))
            .collect();
        if log_enabled {
            assert_eq!(policy_lines.len(), 1, "one line per rebuild");
            assert!(
                policy_lines[0].starts_with("Preload policy: images=10 box=100x100 measuredMeanBytes=none "),
                "{}",
                policy_lines[0]
            );
            assert!(
                policy_lines[0].ends_with(" center=0 direction=1 lead=0"),
                "{}",
                policy_lines[0]
            );
        } else {
            assert!(policy_lines.is_empty(), "{policy_lines:?}");
        }
    }
}

// Contract semantics 9 (Rust-only): a pending caller-thread candidate is dropped when the order is rebuilt.
#[test]
fn pending_caller_candidate_is_dropped_when_the_order_is_rebuilt_queue() {
    let harness = Harness::new(
        10,
        options(PreloadWindow::new(1, 0), true, 2),
        FakeProbe::new(Headroom::Fixed(true)),
        KEY_BOX,
    );
    let mut queue = harness.queue(2);
    let cancel = CancelToken::new();
    assert!(queue.refresh(&harness.ctx(), inputs(0, 0)).expect("refresh"));
    assert_eq!(next_candidate(&harness, &mut queue).index, 1);

    // Image 2 is outside the window: held back for the worker thread.
    assert_eq!(
        queue.next(&harness.ctx(), true, &cancel).expect("next"),
        QueueStep::LeaveCallerThread
    );

    // A navigation rebuilds the order around image 5: the held-back image 2 is gone with the old order.
    assert!(queue.refresh(&harness.ctx(), inputs(1, 5)).expect("refresh"));
    assert_eq!(next_candidate(&harness, &mut queue).index, 6);
}
