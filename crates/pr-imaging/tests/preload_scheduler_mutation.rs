//! Port of `tests/PhotoReview.Imaging.Tests/PreloadSchedulerMutationTests.cs` (C# baseline d92d6aae, 705 lines): worker
//! limits while the viewer decodes, constructor wiring, Cancel/IsIdle/NotifyNavigation, lifetime bookkeeping, the
//! caller-thread window, the whole-folder fill limit, the headroom cache and the failure paths of one candidate.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Test notes: the C# synchronous prefix (`Started.Count` right after `PreloadAroundAsync`) becomes a guarded wait for
//! the number of starts the test expects (`wait_started_count`); the C# `ArgumentNullException` for a null probe has no
//! Rust twin (the type forbids it, see the constructor doc); `StartedCount`/`KeyThread`/`PreloadThread` read the fake.

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};
use std::time::Duration;

use common::preload_fakes::*;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::policy::{WHOLE_FOLDER_CACHE_FILL_LIMIT, format_preload_failed};
use pr_imaging::preload::scheduler::{PreloadScheduler, RunHandle, ThresholdOptions};
use pr_imaging::preload::target::{DelegateMemoryProbe, KeyError};
use pr_imaging::preload_order::PreloadWindow;

/// C# `Target.PreloadThread` seam for the caller-thread tests: `set_dispatched_for_tests` records the dispatching thread.
type DispatchLog = Arc<Mutex<HashMap<String, ThreadId>>>;

fn recording_dispatch(scheduler: &PreloadScheduler) -> DispatchLog {
    let dispatched: DispatchLog = Arc::new(Mutex::new(HashMap::new()));
    let sink = Arc::clone(&dispatched);
    scheduler.set_dispatched_for_tests(Some(Arc::new(move |path: &str| {
        lock(&sink).insert(path.to_ascii_uppercase(), thread::current().id());
    })));
    dispatched
}

fn dispatched_on(log: &DispatchLog, path: &str) -> Option<ThreadId> {
    lock(log).get(&path.to_ascii_uppercase()).copied()
}

/// C# `RunFromDedicatedThread`: kicks from a brand-new thread that ends after its synchronous part, and returns that
/// thread's id together with the run (the caller waits for the run itself).
fn kick_from_dedicated_thread(scheduler: &PreloadScheduler, center: i32) -> (ThreadId, RunHandle) {
    thread::scope(|scope| {
        scope
            .spawn(move || (thread::current().id(), scheduler.preload_around(center)))
            .join()
            .expect("kick thread")
    })
}

/// C# `StartsOnFirstPass`: how many decodes the first pass starts while the (gated) decodes are still running. The
/// C# synchronous prefix becomes a guarded wait for the policy's worker limit (no further start can happen while every
/// slot is held by a gated decode).
fn starts_on_first_pass(workers: i32, viewer_decodes: i32, decode_ewma_ms: i64) -> usize {
    let fs = fake_fs();
    let entries = entries(&fs, 40, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    target.set_active_viewer(viewer_decodes);
    target.set_on_preload(gated(&gate));
    let metrics = Arc::new(ReviewMetrics::new());
    if decode_ewma_ms > 0 {
        metrics.record_source_read(1, decode_ewma_ms, true);
    }
    let scheduler = create(
        &target,
        &entries,
        Setup::new(workers, PreloadWindow::new(30, 0)).metrics(metrics),
    );

    let run = scheduler.preload_around(0);
    let limit = if viewer_decodes == 0 {
        workers
    } else if decode_ewma_ms > 200 {
        1
    } else {
        viewer_busy_limit(workers)
    };
    assert!(
        target.wait_started_count(limit as usize),
        "the first pass reaches its worker limit"
    );
    let started = target.started().len();
    gate.open();
    assert!(finished(&run));
    started
}

// ---- viewer-busy worker limits ------------------------------------------------------------------------------------------

#[test]
fn viewer_decode_active_limits_preload_starts() {
    // C#: PreloadSchedulerMutationTests.ViewerDecodeActive_LimitsPreloadStarts
    assert_eq!(viewer_busy_limit(16) as usize, starts_on_first_pass(16, 1, 0));
}

#[test]
fn viewer_decode_active_slow_link_starts_one() {
    // C#: PreloadSchedulerMutationTests.ViewerDecodeActive_SlowLink_StartsOne
    assert_eq!(1, starts_on_first_pass(16, 1, 201));
}

#[test]
fn viewer_decode_active_link_at_threshold_is_not_slow() {
    // C#: PreloadSchedulerMutationTests.ViewerDecodeActive_LinkAtThreshold_IsNotSlow
    assert_eq!(viewer_busy_limit(16) as usize, starts_on_first_pass(16, 1, 200));
}

#[test]
fn no_viewer_decode_starts_every_worker() {
    // C#: PreloadSchedulerMutationTests.NoViewerDecode_StartsEveryWorker
    assert_eq!(16, starts_on_first_pass(16, 0, 0));
    assert_eq!(16, starts_on_first_pass(16, 0, 500));
}

// ---- constructor wiring -------------------------------------------------------------------------------------------------

#[test]
fn injected_pace_drives_the_order_direction() {
    // C#: PreloadSchedulerMutationTests.InjectedPace_DrivesTheOrderDirection
    let fs = fake_fs();
    let entries = entries(&fs, 20, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    target.set_on_preload(gated(&gate));
    let clock = FakeClock::new();
    let pace = clock.pace();
    pace.record(10);
    pace.record(9); // one step towards lower indices
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(2, 2)).pace(pace));

    let run = scheduler.preload_around(9);

    assert!(target.wait_started_count(1));
    assert_eq!(target.started(), vec![entries[8].path().to_owned()]);
    gate.open();
    assert!(finished(&run));
    assert_eq!(
        target.started(),
        vec![
            entries[8].path(),
            entries[7].path(),
            entries[10].path(),
            entries[11].path()
        ]
    );
}

#[test]
fn convenience_ctor_honours_worker_override_and_window() {
    // C#: PreloadSchedulerMutationTests.ConvenienceCtor_HonoursWorkerOverrideAndWindow
    let fs = fake_fs();
    let entries = entries(&fs, 20, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    target.set_on_preload(gated(&gate));
    let scheduler = PreloadScheduler::with_thresholds(
        Arc::clone(&target) as Arc<dyn pr_imaging::preload::target::PreloadTarget>,
        Arc::new(ReviewMetrics::new()),
        snapshot_of(Arc::clone(&entries)),
        FakeProbe::answering(true),
        ThresholdOptions {
            full_folder_ram_threshold_bytes: -1,
            memory_load_limit: 0.9,
            worker_count_override: Some(3),
            window: Some(PreloadWindow::new(5, 0)),
        },
        pr_imaging::preload::scheduler::PreloadSchedulerDeps {
            env_lookup: Some(Arc::new(|_: &str| None)),
            ..Default::default()
        },
    );

    let run = scheduler.preload_around(0);
    assert!(target.wait_started_count(3));
    assert_eq!(target.started().len(), 3);
    gate.open();
    assert!(finished(&run));

    let mut started = target.started();
    started.sort();
    let mut expected: Vec<String> = (1..=5).map(|i| entries[i].path().to_owned()).collect();
    expected.sort();
    assert_eq!(started, expected, "exactly the 5-image window");
}

#[test]
fn convenience_ctor_memory_probe_or_delegate() {
    // C#: PreloadSchedulerMutationTests.ConvenienceCtor_MemoryProbeOrDelegate
    // DIVERGENCE: the C# "null probe throws" half has no Rust twin (`Arc<dyn MemoryProbe>` cannot be null).
    let fs = fake_fs();
    let entries = entries(&fs, 6, "img");
    let thresholds = |workers: i32| ThresholdOptions {
        full_folder_ram_threshold_bytes: -1,
        memory_load_limit: 0.9,
        worker_count_override: Some(workers),
        window: Some(PreloadWindow::new(3, 0)),
    };
    let env_none = || pr_imaging::preload::scheduler::PreloadSchedulerDeps {
        env_lookup: Some(Arc::new(|_: &str| None)),
        ..Default::default()
    };

    let probe_target = FakeTarget::new(&fs);
    let denied = PreloadScheduler::with_thresholds(
        Arc::clone(&probe_target) as Arc<dyn pr_imaging::preload::target::PreloadTarget>,
        Arc::new(ReviewMetrics::new()),
        snapshot_of(Arc::clone(&entries)),
        FakeProbe::answering(false),
        thresholds(2),
        env_none(),
    );
    assert!(finished(&denied.preload_around(0)));
    assert!(probe_target.started().is_empty(), "no headroom: nothing preloaded");

    let delegate_target = FakeTarget::new(&fs);
    let by_delegate = PreloadScheduler::with_thresholds(
        Arc::clone(&delegate_target) as Arc<dyn pr_imaging::preload::target::PreloadTarget>,
        Arc::new(ReviewMetrics::new()),
        snapshot_of(Arc::clone(&entries)),
        Arc::new(DelegateMemoryProbe::new(|_| true)),
        thresholds(2),
        env_none(),
    );
    assert!(finished(&by_delegate.preload_around(0)));
    assert_eq!(delegate_target.started().len(), 3);
}

// ---- Cancel / IsIdle / NotifyNavigation ---------------------------------------------------------------------------------

#[test]
fn cancel_cancels_running_work() {
    // C#: PreloadSchedulerMutationTests.Cancel_CancelsRunningWork
    let fs = fake_fs();
    let entries = entries(&fs, 6, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    target.set_on_preload(gated(&gate));
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(3, 0)));
    let run = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));

    scheduler.cancel();

    assert!(finished(&run), "the gated decode only ends through the cancelled token");
    assert_eq!(target.started(), vec![entries[1].path().to_owned()]);
    scheduler.dispose();
    scheduler.cancel(); // after Dispose: a quiet no-op
}

#[test]
fn is_idle_tracks_the_running_lifetime() {
    // C#: PreloadSchedulerMutationTests.IsIdle_TracksTheRunningLifetime
    let fs = fake_fs();
    let entries = entries(&fs, 4, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    target.set_on_preload(gated(&gate));
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(2, 0)));
    assert!(scheduler.is_idle());

    let run = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));
    assert!(!scheduler.is_idle());

    gate.open();
    assert!(finished(&run));
    assert!(scheduler.is_idle());
}

#[test]
fn remove_preloaded_keys_for_path_removes_only_that_path() {
    // C#: PreloadSchedulerMutationTests.RemovePreloadedKeysForPath_RemovesOnlyThatPath
    let fs = fake_fs();
    let entries = entries(&fs, 5, "img");
    let target = FakeTarget::new(&fs);
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(3, 0)));
    assert!(finished(&scheduler.preload_around(0)));
    let key1 = target.key(entries[1].path());
    let key2 = target.key(entries[2].path());
    assert_eq!(3, scheduler.preloaded_key_count_for_tests());

    scheduler.remove_preloaded_keys_for_path(key1.path());

    assert!(!scheduler.try_consume_preloaded_key(&key1));
    assert!(scheduler.try_consume_preloaded_key(&key2));
    assert_eq!(1, scheduler.preloaded_key_count_for_tests());
}

#[test]
fn notify_navigation_records_pace_only_with_workers() {
    // C#: PreloadSchedulerMutationTests.NotifyNavigation_RecordsPaceOnlyWithWorkers
    let fs = fake_fs();
    let clock = FakeClock::new();
    let enabled_pace = clock.pace();
    let disabled_pace = clock.pace();
    let enabled_entries = entries(&fs, 8, "on");
    let disabled_entries = entries(&fs, 8, "off");
    let enabled = create(
        &FakeTarget::new(&fs),
        &enabled_entries,
        Setup::new(2, PreloadWindow::new(1, 0)).pace(Arc::clone(&enabled_pace)),
    );
    let disabled = create(
        &FakeTarget::new(&fs),
        &disabled_entries,
        Setup::new(0, PreloadWindow::new(1, 0)).pace(Arc::clone(&disabled_pace)),
    );

    enabled.notify_navigation(5);
    enabled.notify_navigation(4);
    disabled.notify_navigation(5);
    disabled.notify_navigation(4);

    assert_eq!(-1, enabled_pace.direction());
    assert_eq!(1, disabled_pace.direction(), "never recorded");
}

#[test]
fn notify_navigation_reprioritizes_a_running_scheduler() {
    // C#: PreloadSchedulerMutationTests.NotifyNavigation_ReprioritizesARunningScheduler
    let fs = fake_fs();
    let entries = entries(&fs, 20, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    let first_path = entries[1].path().to_owned();
    let gate_for_first = Arc::clone(&gate);
    target.set_on_preload(move |path, token| {
        if path.eq_ignore_ascii_case(&first_path) {
            gated(&gate_for_first)(path, token)
        } else {
            Ok(())
        }
    });
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(2, 0)));
    let run = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));

    scheduler.notify_navigation(10);
    gate.open();
    assert!(finished(&run));

    let started = target.started();
    assert!(started.iter().any(|p| p == entries[11].path()));
    assert!(started.iter().any(|p| p == entries[12].path()));
    assert!(
        !started.iter().any(|p| p == entries[2].path()),
        "the old centre's next image was dropped"
    );
}

#[test]
fn notify_navigation_starts_the_scheduler_only_during_a_burst() {
    // C#: PreloadSchedulerMutationTests.NotifyNavigation_StartsTheSchedulerOnlyDuringABurst
    let fs = fake_fs();
    let entries = entries(&fs, 20, "img");
    let quiet = FakeTarget::new(&fs);
    let quiet_scheduler = create(&quiet, &entries, Setup::new(2, PreloadWindow::new(3, 0)));
    quiet_scheduler.notify_navigation(5);
    assert!(quiet_scheduler.is_idle());
    assert!(quiet.started().is_empty());

    let clock = FakeClock::new();
    let pace = clock.pace();
    let metrics = Arc::new(ReviewMetrics::new());
    metrics.record_source_read(1, 200, true); // a 200 ms decode ...
    pace.record(0);
    clock.advance_ms(50.0);
    pace.record(1);
    clock.advance_ms(50.0);
    pace.record(2); // ... against 50 ms key intervals: a burst
    let burst = FakeTarget::new(&fs);
    let burst_scheduler = create(
        &burst,
        &entries,
        Setup::new(2, PreloadWindow::new(3, 0)).metrics(metrics).pace(pace),
    );
    clock.advance_ms(50.0);

    burst_scheduler.notify_navigation(3);

    assert!(burst.wait_started_count(1), "a burst starts the scheduler at once");
    assert!(finished(&burst_scheduler.preload_around(3)));
}

#[test]
fn get_viewer_decode_delay_follows_the_pace_only_when_enabled() {
    // C#: PreloadSchedulerMutationTests.GetViewerDecodeDelay_FollowsThePaceOnlyWhenEnabled
    let fs = fake_fs();
    let clock = FakeClock::new();
    let pace = clock.pace();
    let metrics = Arc::new(ReviewMetrics::new());
    metrics.record_source_read(1, 200, true);
    pace.record(0);
    clock.advance_ms(50.0);
    pace.record(1);
    clock.advance_ms(50.0);
    pace.record(2);
    let enabled = create(
        &FakeTarget::new(&fs),
        &entries(&fs, 4, "on"),
        Setup::new(2, PreloadWindow::new(1, 0))
            .metrics(Arc::clone(&metrics))
            .pace(Arc::clone(&pace)),
    );
    let disabled = create(
        &FakeTarget::new(&fs),
        &entries(&fs, 4, "off"),
        Setup::new(0, PreloadWindow::new(1, 0)).metrics(metrics).pace(pace),
    );

    assert_eq!(Duration::from_millis(75), enabled.get_viewer_decode_delay());
    assert_eq!(Duration::ZERO, disabled.get_viewer_decode_delay());
}

// ---- lifetimes ----------------------------------------------------------------------------------------------------------

#[test]
fn lifetimes_are_bounded_including_the_first() {
    // C#: PreloadSchedulerMutationTests.Lifetimes_AreBoundedIncludingTheFirst
    let fs = fake_fs();
    let entries = entries(&fs, 4, "img");
    let scheduler = create(&FakeTarget::new(&fs), &entries, Setup::new(1, PreloadWindow::new(1, 0)));

    assert!(finished(&scheduler.preload_around(0)));
    scheduler.cancel();
    assert!(finished(&scheduler.preload_around(0)));
    scheduler.cancel();
    assert!(finished(&scheduler.preload_around(0)));

    assert_eq!(2, scheduler.tracked_lifetime_counts().0);
}

// ---- scheduler pass decisions -------------------------------------------------------------------------------------------

#[test]
fn scheduler_failure_marks_run_exiting() {
    // C#: PreloadSchedulerMutationTests.SchedulerFailure_MarksRunExiting
    let fs = fake_fs();
    let entries = entries(&fs, 10, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    target.set_on_preload(gated(&gate));
    let log = CaptureLog::new();
    // Headroom is fine until the first decode started; the next probe then panics: an unexpected failure in the loop.
    let probe_target = Arc::clone(&target);
    let probe = FakeProbe::new(move || {
        if probe_target.started().is_empty() {
            true
        } else {
            panic!("probe failed")
        }
    });
    let scheduler = create(
        &target,
        &entries,
        Setup::new(2, PreloadWindow::new(5, 0)).log(&log).probe(probe),
    );

    let first = scheduler.preload_around(0); // fails after starting one decode, then drains it (gated)
    assert!(!first.is_complete());
    assert!(log.wait_error("Preload scheduler failed"), "the failure is logged");
    let second = scheduler.preload_around(5);

    assert!(!first.same_run(&second));
    gate.open();
    assert!(finished(&first) && finished(&second));
}

#[test]
fn order_is_rebuilt_only_on_change() {
    // C#: PreloadSchedulerMutationTests.Order_IsRebuiltOnlyOnChange
    let fs = fake_fs();
    let entries = entries(&fs, 10, "img");
    let target = FakeTarget::new(&fs);
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(4, 0)));

    assert!(finished(&scheduler.preload_around(0)));

    assert_eq!(4, target.started().len());
    // The centre entry is only ever keyed when the order (and its decode box) is rebuilt.
    assert_eq!(1, target.key_calls(entries[0].path()));
}

#[test]
fn paused_run_with_pending_navigation_exits_instead_of_repeating() {
    // C#: PreloadSchedulerMutationTests.PausedRun_WithPendingNavigation_ExitsInsteadOfRepeating
    let fs = fake_fs();
    let entries = entries(&fs, 10, "img");
    let target = FakeTarget::new(&fs);
    let slot: Arc<OnceLock<PreloadScheduler>> = Arc::new(OnceLock::new());
    let fired = Arc::new(AtomicBool::new(false));
    let probe_slot = Arc::clone(&slot);
    let probe_fired = Arc::clone(&fired);
    let probe = FakeProbe::new(move || {
        if !probe_fired.swap(true, Ordering::SeqCst) {
            // lands after this pass read its version
            probe_slot.get().expect("scheduler set").notify_navigation(7);
        }
        false
    });
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(3, 0)).probe(probe));
    assert!(slot.set(scheduler).is_ok());

    assert!(finished(&slot.get().expect("scheduler set").preload_around(0)));

    assert!(
        target.key_calls(entries[7].path()) == 0,
        "no second pass rebuilt the order around the new centre"
    );
    assert!(target.started().is_empty());
}

#[test]
fn headroom_is_rechecked_after_every_queued_decode() {
    // C#: PreloadSchedulerMutationTests.Headroom_IsRecheckedAfterEveryQueuedDecode
    let fs = fake_fs();
    let entries = entries(&fs, 10, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    target.set_on_preload(gated(&gate));
    let probe_target = Arc::clone(&target);
    let probe = FakeProbe::new(move || probe_target.started().is_empty());
    let scheduler = create(&target, &entries, Setup::new(4, PreloadWindow::new(8, 0)).probe(probe));

    let run = scheduler.preload_around(0);
    assert!(target.wait_started_count(1));
    gate.open();
    assert!(finished(&run));

    assert_eq!(
        1,
        target.started().len(),
        "paused for memory after the first, never resumed in this run"
    );
}

#[test]
fn headroom_is_cached_while_nothing_is_queued() {
    // C#: PreloadSchedulerMutationTests.Headroom_IsCachedWhileNothingIsQueued
    let fs = fake_fs();
    let entries = entries(&fs, 14, "img");
    let target = FakeTarget::new(&fs);
    for entry in entries.iter() {
        target.mark_cached(entry.path());
    }
    let probe = FakeProbe::answering(true);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(2, PreloadWindow::new(13, 0))
            .probe(Arc::clone(&probe) as Arc<dyn pr_core::abstractions::MemoryProbe>),
    );

    assert!(finished(&scheduler.preload_around(0)));

    assert!(target.started().is_empty());
    assert!(
        (1..=5).contains(&probe.calls()),
        "one real probe, not one per image: {}",
        probe.calls()
    );
}

// ---- caller-thread window -----------------------------------------------------------------------------------------------

#[test]
fn only_the_window_runs_on_the_callers_thread() {
    // C#: PreloadSchedulerMutationTests.OnlyTheWindowRunsOnTheCallerThread
    // DIVERGENCE: decodes never run on the caller thread (R0003); the window's ITEMS are dispatched on the caller.
    for direction in [1i32, -1] {
        let fs = fake_fs();
        let entries = entries(&fs, 30, "img");
        let target = FakeTarget::new(&fs);
        let clock = FakeClock::new();
        let pace = clock.pace();
        if direction < 0 {
            pace.record(11);
            pace.record(10);
        }
        let scheduler = create(
            &target,
            &entries,
            Setup::new(16, PreloadWindow::new(3, 2)).whole_folder().pace(pace),
        );
        let dispatched = recording_dispatch(&scheduler);

        let (caller, run) = kick_from_dedicated_thread(&scheduler, 10);
        assert!(finished(&run));

        let on_caller = [
            10 + direction,
            10 + 2 * direction,
            10 + 3 * direction,
            10 - direction,
            10 - 2 * direction,
        ];
        for (i, entry) in entries.iter().enumerate() {
            if i == 10 {
                continue;
            }
            assert!(
                target.key_thread(entry.path()).is_some(),
                "index {i} was never examined (whole folder expected)"
            );
            let expected = on_caller.contains(&(i as i32));
            assert_eq!(
                expected,
                target.key_thread(entry.path()) == Some(caller),
                "index {i} examined on the wrong thread (direction {direction})"
            );
        }
        for (i, entry) in entries.iter().enumerate() {
            if i != 10 && on_caller.contains(&(i as i32)) {
                assert_eq!(
                    Some(caller),
                    dispatched_on(&dispatched, entry.path()),
                    "index {i} dispatched on the caller"
                );
                assert_ne!(
                    Some(caller),
                    target.preload_thread(entry.path()),
                    "decodes never run on the caller"
                );
            }
        }
    }
}

#[test]
fn first_workers_start_on_the_callers_thread() {
    // C#: PreloadSchedulerMutationTests.FirstWorkersStartOnTheCallerThread
    let fs = fake_fs();
    let entries = entries(&fs, 12, "img");
    let target = FakeTarget::new(&fs);
    let scheduler = create(&target, &entries, Setup::new(3, PreloadWindow::new(8, 0)));
    let dispatched = recording_dispatch(&scheduler);

    let (caller, run) = kick_from_dedicated_thread(&scheduler, 0);
    assert!(finished(&run));

    for i in [1usize, 2, 3] {
        assert_eq!(
            Some(caller),
            dispatched_on(&dispatched, entries[i].path()),
            "index {i} dispatched on the caller"
        );
    }
    // DIVERGENCE: decodes never run on the caller thread (R0003).
    assert_ne!(Some(caller), target.preload_thread(entries[1].path()));
    assert_ne!(
        Some(caller),
        target.preload_thread(entries[8].path()),
        "started after the batch yield"
    );
}

// ---- whole-folder fill limit --------------------------------------------------------------------------------------------

#[test]
fn whole_folder_stops_at_the_fill_limit() {
    // C#: PreloadSchedulerMutationTests.WholeFolder_StopsAtTheFillLimit (cache 900_000_000: stops; 899_999_999: reaches on)
    for (cache_bytes, reaches_the_rest) in [(900_000_000i64, false), (899_999_999i64, true)] {
        assert_eq!(900_000_000.0, 1_000_000_000_f64 * WHOLE_FOLDER_CACHE_FILL_LIMIT);
        let fs = fake_fs();
        let entries = entries(&fs, 20, "img");
        let target = FakeTarget::new(&fs);
        target.set_bytes(cache_bytes);
        let scheduler = create(
            &target,
            &entries,
            Setup::new(4, PreloadWindow::new(2, 1)).whole_folder(),
        );

        assert!(finished(&scheduler.preload_around(5)));

        let started: std::collections::HashSet<String> =
            target.started().into_iter().map(|p| p.to_ascii_uppercase()).collect();
        let up = |i: usize| entries[i].path().to_ascii_uppercase();
        assert!(started.contains(&up(6)));
        assert!(started.contains(&up(7)));
        assert!(started.contains(&up(4)));
        assert_eq!(reaches_the_rest, started.contains(&up(15)), "cache {cache_bytes}");
        assert_eq!(
            if reaches_the_rest { 19 } else { 3 },
            started.len(),
            "cache {cache_bytes}"
        );
    }
}

// ---- one candidate failing ----------------------------------------------------------------------------------------------

#[test]
fn candidate_key_failure_skips_only_that_candidate() {
    // C#: PreloadSchedulerMutationTests.CandidateKeyFailure_SkipsOnlyThatCandidate (theory: io, access)
    for kind in ["io", "access"] {
        let fs = fake_fs();
        let entries = entries(&fs, 8, "img");
        let target = FakeTarget::new(&fs);
        let failing = entries[2].path().to_owned();
        let kind = kind.to_owned();
        target.set_key_failure(move |path| {
            if path == failing {
                Some(if kind == "io" {
                    KeyError::Io(io_error(std::io::ErrorKind::NotFound, "gone"))
                } else {
                    KeyError::Io(io_error(std::io::ErrorKind::PermissionDenied, "denied"))
                })
            } else {
                None
            }
        });
        let log = CaptureLog::new();
        let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(6, 0)).log(&log));

        assert!(finished(&scheduler.preload_around(0)));

        assert_eq!(0, target.started_count(entries[2].path()));
        for i in [1usize, 3, 4, 5, 6] {
            assert!(target.started_count(entries[i].path()) >= 1, "index {i} preloaded");
        }
        assert!(log.warnings().iter().any(|w| w.contains(entries[2].path())));
        assert!(log.errors().is_empty());
    }
}

#[test]
fn centre_key_failure_falls_back_to_unbounded_box() {
    // C#: PreloadSchedulerMutationTests.CentreKeyFailure_FallsBackToUnboundedBox
    let fs = fake_fs();
    let entries = entries(&fs, 6, "img");
    let target = FakeTarget::new(&fs);
    let centre = entries[0].path().to_owned();
    target.set_key_failure(move |path| {
        (path == centre).then(|| KeyError::Io(io_error(std::io::ErrorKind::NotFound, "gone")))
    });
    let log = CaptureLog::new();
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(3, 0)).log(&log));

    assert!(finished(&scheduler.preload_around(0)));

    assert_eq!(3, target.started().len());
    assert!(log.errors().is_empty());
}

#[test]
fn centre_out_of_range_is_a_no_op_not_an_error() {
    // C#: PreloadSchedulerMutationTests.CentreOutOfRange_IsANoOpNotAnError (theory: -1, 6, 100)
    for center in [-1i32, 6, 100] {
        let fs = fake_fs();
        let entries = entries(&fs, 6, "img");
        let target = FakeTarget::new(&fs);
        let log = CaptureLog::new();
        let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(3, 0)).log(&log));

        assert!(finished(&scheduler.preload_around(center)));

        assert!(target.started().is_empty());
        assert!(log.errors().is_empty());
    }
}

#[test]
fn unexpected_decode_exception_skips_only_that_image() {
    // C#: PreloadSchedulerMutationTests.UnexpectedDecodeException_SkipsOnlyThatImage
    let fs = fake_fs();
    let entries = entries(&fs, 8, "img");
    let target = FakeTarget::new(&fs);
    let odd = entries[2].path().to_owned();
    target.set_on_preload(move |path, _| {
        if path == odd {
            Err(DecodeError::Other {
                message: "odd format".to_owned(),
            })
        } else {
            Ok(())
        }
    });
    let log = CaptureLog::new();
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(6, 0)).log(&log));

    assert!(finished(&scheduler.preload_around(0)));

    for i in [3usize, 4, 5, 6] {
        assert!(target.started_count(entries[i].path()) >= 1, "index {i} preloaded");
    }
    assert!(
        log.errors()
            .iter()
            .any(|e| e == &format_preload_failed(entries[2].path()) || e.starts_with("Preload failed"))
    );
    assert!(!log.errors().iter().any(|e| e.starts_with("Preload scheduler failed")));
}

#[test]
fn missing_codec_skips_those_images_quietly_and_keeps_preloading_the_others() {
    // C#: PreloadSchedulerMutationTests.MissingCodec_SkipsThoseImagesQuietly_AndKeepsPreloadingTheOthers (Q-FMT-WEBP-HEIC)
    let fs = fake_fs();
    let entries = entries(&fs, 8, "img");
    let target = FakeTarget::new(&fs);
    let heic_a = entries[2].path().to_owned();
    let heic_b = entries[3].path().to_owned();
    target.set_on_preload(move |path, _| {
        if path == heic_a || path == heic_b {
            Err(DecodeError::missing_codec_with("HEIC needs HEVC"))
        } else {
            Ok(())
        }
    });
    let log = CaptureLog::new();
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(6, 0)).log(&log));

    assert!(finished(&scheduler.preload_around(0)));

    for i in [4usize, 5, 6] {
        assert!(target.started_count(entries[i].path()) >= 1, "index {i} preloaded");
    }
    assert!(log.errors().is_empty());
}

// ---- items waiting for a worker slot ------------------------------------------------------------------------------------

#[test]
fn queued_item_that_became_the_current_image_is_dropped() {
    // C#: PreloadSchedulerMutationTests.QueuedItem_ThatBecameTheCurrentImage_IsDropped
    let fs = fake_fs();
    let entries = entries(&fs, 8, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    let held = entries[3].path().to_owned();
    let gate_for_held = Arc::clone(&gate);
    // image 3 starts and holds the only slot (it ignores cancellation)
    target.set_on_preload(move |path, token| {
        if path == held {
            gated_ignoring_cancel(&gate_for_held)(path, token)
        } else {
            Ok(())
        }
    });
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(2, 0)));

    let first = scheduler.preload_around(2);
    assert!(target.wait_started(entries[3].path()));
    scheduler.cancel();
    let second = scheduler.preload_around(0); // new lifetime: image 1 waits for that slot
    scheduler.notify_navigation(1); // the user lands on image 1 while it waits
    assert!(!second.is_complete());
    gate.open();
    assert!(finished(&first) && finished(&second));

    assert_eq!(0, target.started_count(entries[1].path()));
}

#[test]
fn queued_item_already_cached_meanwhile_is_not_decoded_again() {
    // C#: PreloadSchedulerMutationTests.QueuedItem_AlreadyCachedMeanwhile_IsNotDecodedAgain
    let fs = fake_fs();
    let entries = entries(&fs, 8, "img");
    let gate = Gate::new();
    let target = FakeTarget::new(&fs);
    let held = entries[1].path().to_owned();
    let gate_for_held = Arc::clone(&gate);
    target.set_on_preload(move |path, token| {
        if path == held {
            gated_ignoring_cancel(&gate_for_held)(path, token)
        } else {
            Ok(())
        }
    });
    let scheduler = create(&target, &entries, Setup::new(1, PreloadWindow::new(2, 0)));

    let first = scheduler.preload_around(0); // image 1 starts and holds the only slot
    assert!(target.wait_started(entries[1].path()));
    scheduler.cancel();
    let second = scheduler.preload_around(0); // new lifetime: image 1 queued again, waiting for the slot
    assert!(!second.is_complete());
    gate.open();
    assert!(finished(&first) && finished(&second));

    assert_eq!(1, target.started_count(entries[1].path()));
}
