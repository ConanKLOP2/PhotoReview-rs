//! Scheduler-shell behaviour pinned after the W4-15b verification (repair round 1): rules W3 and W4 of the contract
//! against C# `PreloadOneAsync` (baseline d92d6aae, `Preload/PreloadScheduler.cs`), lifetime pruning, the convenience
//! constructor's options, `Drop`, `RunHandle::wait`, and runs that must end without hanging the caller.
//!
//! No C# twin for most tests here: they pin C# behaviour the ported suite left open (marked `// C# behaviour:`).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use common::preload_fakes::*;
use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_imaging::navigation_pace::NavigationPace;
use pr_imaging::preload::policy::MSG_SCHEDULER_FAILED;
use pr_imaging::preload::scheduler::{PreloadScheduler, PreloadSchedulerDeps, RunHandle, ThresholdOptions};
use pr_imaging::preload::target::PreloadTarget;
use pr_imaging::preload_order::PreloadWindow;

fn file_name(path: &str) -> String {
    path.rsplit('\\').next().unwrap_or(path).to_ascii_lowercase()
}

fn started_names(target: &FakeTarget) -> Vec<String> {
    let mut names: Vec<String> = target.started().iter().map(|p| file_name(p)).collect();
    names.sort();
    names
}

/// Runs `body` on a helper thread and returns its result, or `None` when it did not return within the guard (a run
/// that loops forever on the caller's thread must fail the test, not hang it).
fn on_helper<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(body());
    });
    rx.recv_timeout(GUARD).ok()
}

#[test]
fn item_dispatched_after_its_lifetime_was_cancelled_never_starts() {
    // C# behaviour: PreloadOneAsync starts with `await _preloadSlots.WaitAsync(ct)`, which is cancelled at once for a
    // cancelled token even with a free slot, so the item never reaches the re-check, the prefetch or PreloadAsync (W3).
    let fs = fake_fs();
    let entries = entries(&fs, 6, "w3");
    let target = FakeTarget::new(&fs);
    let prefetched = Arc::new(Mutex::new(Vec::<String>::new()));
    let prefetch_log = Arc::clone(&prefetched);
    let scheduler = Arc::new(create(
        &target,
        &entries,
        Setup::new(8, PreloadWindow::new(4, 0)).prefetch(move |path, _token| {
            lock(&prefetch_log).push(file_name(path));
            Ok(())
        }),
    ));
    let weak: Weak<PreloadScheduler> = Arc::downgrade(&scheduler);
    // The cancel lands while the loop builds img-2's key: after the loop's own cancellation check for img-2.
    target.set_key_failure(move |path| {
        if file_name(path) == "w3-2.jpg" {
            if let Some(scheduler) = weak.upgrade() {
                scheduler.cancel();
            }
        }
        None
    });
    let run = scheduler.preload_around(0);
    assert!(finished(&run));
    let started = started_names(&target);
    assert!(
        !started.contains(&"w3-2.jpg".to_owned()),
        "img-2 started although its lifetime was cancelled: {started:?}"
    );
    assert!(
        started.iter().all(|name| name == "w3-1.jpg"),
        "nothing after the cancel starts: {started:?}"
    );
    assert!(
        !lock(&prefetched).contains(&"w3-2.jpg".to_owned()),
        "no prefetch either"
    );
}

#[test]
fn faulted_inline_recheck_does_not_stop_the_rest_of_the_pass() {
    // C# behaviour: a throwing re-check (after the slot was taken) faults img-1's task; the loop puts it in `running` and
    // keeps dispatching (img-2 and img-3 reach PreloadAsync); only the later `await finished` logs the failure.
    let fs = fake_fs();
    let entries = entries(&fs, 4, "fault");
    let target = FakeTarget::new(&fs);
    let lookups = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&lookups);
    target.set_on_cached_lookup(move |path| {
        // The loop's own lookup is the first; PreloadOneAsync's re-check is the second.
        if file_name(path) == "fault-1.jpg" && counter.fetch_add(1, Ordering::SeqCst) == 1 {
            panic!("re-check exploded");
        }
    });
    let log = CaptureLog::new();
    let scheduler = create(&target, &entries, Setup::new(8, PreloadWindow::new(3, 0)).log(&log));
    let run = scheduler.preload_around(0);
    assert!(finished(&run));
    assert_eq!(started_names(&target), ["fault-2.jpg", "fault-3.jpg"]);
    assert_eq!(
        log.errors(),
        [format!("{MSG_SCHEDULER_FAILED} :: re-check exploded")],
        "the run fails once, with the re-check's panic text"
    );
    assert!(scheduler.is_idle());
}

#[test]
fn own_cancellation_ends_the_item_without_a_log_line() {
    // C# behaviour: `catch (OperationCanceledException) when (!ct.IsCancellationRequested)` (RV-I14) does not match an
    // item cancelled by its own lifetime: the task is cancelled, nothing is logged, the run drains.
    let fs = fake_fs();
    let entries = entries(&fs, 3, "own");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    target.set_on_preload(gated(&gate));
    let log = CaptureLog::new();
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(1, 0)).log(&log));
    let run = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));
    scheduler.cancel();
    assert!(finished(&run));
    assert!(log.warnings().is_empty(), "not a foreign cancel: {:?}", log.warnings());
    assert!(log.errors().is_empty(), "not a failure: {:?}", log.errors());
    gate.open();
}

#[test]
fn finished_lifetimes_are_pruned_and_live_ones_kept() {
    // C# PruneFinishedLifetimes: completed runs leave the list; a lifetime stays while it is current or one of its runs
    // is still unfinished.
    let fs = fake_fs();
    let entries = entries(&fs, 3, "life");
    let target = FakeTarget::new(&fs);
    // The k-th decode waits on gates[k] (ignoring cancellation); later decodes pass.
    let gates: Arc<Vec<Arc<Gate>>> = Arc::new((0..3).map(|_| Gate::new()).collect());
    let calls = Arc::new(AtomicUsize::new(0));
    let (hook_gates, hook_calls) = (Arc::clone(&gates), Arc::clone(&calls));
    target.set_on_preload(
        move |path, token| match hook_gates.get(hook_calls.fetch_add(1, Ordering::SeqCst)) {
            Some(gate) => gated_ignoring_cancel(gate)(path, token),
            None => Ok(()),
        },
    );
    let scheduler = create(&target, &entries, Setup::new(4, PreloadWindow::new(1, 0)));
    let mut runs: Vec<RunHandle> = Vec::new();
    for round in 1..=3 {
        if round > 1 {
            scheduler.cancel();
        }
        runs.push(scheduler.preload_around(0));
        assert!(target.wait_started_count(round), "round {round} decode started");
    }
    assert_eq!(
        scheduler.tracked_lifetime_counts(),
        (3, 3),
        "every lifetime with an unfinished run is still tracked"
    );
    // The first two runs finish; the third still decodes.
    gates[0].open();
    gates[1].open();
    assert!(finished(&runs[0]));
    assert!(finished(&runs[1]));
    scheduler.cancel();
    let fourth = scheduler.preload_around(0);
    assert_eq!(
        scheduler.tracked_lifetime_counts(),
        (2, 2),
        "finished lifetimes are dropped; the third (unfinished run) and the new one stay"
    );
    assert!(finished(&fourth));
    gates[2].open();
    assert!(finished(&runs[2]));
    scheduler.cancel();
    let last = scheduler.preload_around(0);
    assert_eq!(
        scheduler.tracked_lifetime_counts(),
        (2, 1),
        "only the cancelled current lifetime and the new one remain"
    );
    assert!(finished(&last));
}
/// Records the limits `has_headroom` is asked with.
struct LimitProbe {
    limits: Mutex<Vec<f64>>,
}

impl MemoryProbe for LimitProbe {
    fn has_headroom(&self, maximum_load: f64, _reserve_bytes: i64) -> bool {
        lock(&self.limits).push(maximum_load);
        true
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        None
    }
}

#[test]
fn convenience_constructor_uses_its_worker_override_and_load_limit_and_ignores_the_pace() {
    // C#: the convenience constructor builds its own PreloadOptions (worker override, memory load limit) and passes
    // `pace: null`.
    let fs = fake_fs();
    let entries = entries(&fs, 4, "conv");
    let target = FakeTarget::new(&fs);
    let probe = Arc::new(LimitProbe {
        limits: Mutex::new(Vec::new()),
    });
    let pace = Arc::new(NavigationPace::new());
    let scheduler = PreloadScheduler::with_thresholds(
        Arc::clone(&target) as Arc<dyn PreloadTarget>,
        Arc::new(ReviewMetrics::new()),
        snapshot_of(Arc::clone(&entries)),
        Arc::clone(&probe) as Arc<dyn MemoryProbe>,
        ThresholdOptions {
            full_folder_ram_threshold_bytes: -1,
            memory_load_limit: 0.625,
            worker_count_override: Some(5),
            window: Some(PreloadWindow::new(2, 0)),
        },
        PreloadSchedulerDeps {
            pace: Some(Arc::clone(&pace)),
            env_lookup: Some(Arc::new(|_: &str| None)),
            ..PreloadSchedulerDeps::default()
        },
    );
    assert_eq!(scheduler.worker_count(), 5);
    scheduler.notify_navigation(2);
    scheduler.notify_navigation(1);
    assert_eq!(pace.direction(), 1, "the injected pace is not used");
    assert!(finished(&scheduler.preload_around(1)));
    let limits = lock(&probe.limits).clone();
    assert!(!limits.is_empty(), "the headroom was probed");
    assert!(limits.iter().all(|limit| (*limit - 0.625).abs() < 1e-12), "{limits:?}");
}

#[test]
fn dropping_the_scheduler_disposes_it() {
    // C#: owners call Dispose; Rust's Drop does it (contract semantics 9): the running lifetime is cancelled.
    let fs = fake_fs();
    let entries = entries(&fs, 3, "drop");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    target.set_on_preload(gated(&gate));
    let log = CaptureLog::new();
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(1, 0)).log(&log));
    let run = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));
    drop(scheduler);
    assert!(
        run.wait_timeout(Duration::from_secs(5)),
        "the dropped scheduler cancelled and drained its run"
    );
    assert!(log.warnings().is_empty(), "{:?}", log.warnings());
    gate.open();
}

#[test]
fn run_handle_wait_returns_only_once_the_run_completed() {
    let fs = fake_fs();
    let entries = entries(&fs, 3, "wait");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    target.set_on_preload(gated(&gate));
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(1, 0)));
    let run = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));
    let waited = run.clone();
    let waiter = thread::spawn(move || {
        waited.wait();
        waited.is_complete()
    });
    // A correct wait cannot return while the decode is gated; give a wrong one time to return early.
    let deadline = Instant::now() + Duration::from_millis(200);
    while !waiter.is_finished() && Instant::now() < deadline {
        thread::yield_now();
    }
    gate.open();
    assert_eq!(waiter.join().ok(), Some(true));
    assert!(run.is_complete());
}

#[test]
fn a_run_with_nothing_to_do_completes_on_the_caller() {
    // C#: a run that never awaits completes synchronously (the returned task is already complete).
    let fs = fake_fs();
    let entries = entries(&fs, 3, "cached");
    let target = FakeTarget::new(&fs);
    for entry in entries.iter() {
        target.mark_cached(entry.path());
    }
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(2, 2)));
    let run = scheduler.preload_around(1);
    assert!(
        run.is_complete(),
        "nothing to decode: the run ended before preload_around returned"
    );
    assert!(target.started().is_empty());
}

#[test]
fn a_kick_returns_and_its_run_completes_within_the_guard() {
    let fs = fake_fs();
    let entries = entries(&fs, 8, "guard");
    let target = FakeTarget::new(&fs);
    let scheduler = Arc::new(create(&target, &entries, Setup::new(2, PreloadWindow::new(4, 2))));
    let kicker = Arc::clone(&scheduler);
    let run = on_helper(move || kicker.preload_around(3)).expect("preload_around returned");
    assert!(finished(&run));
    assert_eq!(
        started_names(&target),
        [
            "guard-1.jpg",
            "guard-2.jpg",
            "guard-4.jpg",
            "guard-5.jpg",
            "guard-6.jpg",
            "guard-7.jpg"
        ]
    );
    // A second kick with everything cached also ends.
    let kicker = Arc::clone(&scheduler);
    let again = on_helper(move || kicker.preload_around(3)).expect("preload_around returned");
    assert!(finished(&again));
    assert_eq!(target.started().len(), 6);
}

#[test]
fn a_failing_pass_logs_once_and_completes_within_the_guard() {
    // C#: `catch (Exception ex)`: MarkRunExiting, Error "Preload scheduler failed", drain.
    let fs = fake_fs();
    let entries = entries(&fs, 6, "probe");
    let target = FakeTarget::new(&fs);
    let log = CaptureLog::new();
    let scheduler = Arc::new(create(
        &target,
        &entries,
        Setup::new(2, PreloadWindow::new(3, 0))
            .log(&log)
            .probe(FakeProbe::new(|| panic!("probe exploded"))),
    ));
    let kicker = Arc::clone(&scheduler);
    let run = on_helper(move || kicker.preload_around(0)).expect("preload_around returned");
    assert!(finished(&run));
    assert_eq!(log.errors(), [format!("{MSG_SCHEDULER_FAILED} :: probe exploded")]);
    assert!(target.started().is_empty());
    assert!(scheduler.is_idle(), "the failed run is not joined by the next kick");
}
