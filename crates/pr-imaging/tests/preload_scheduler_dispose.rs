//! Port of `tests/PhotoReview.Imaging.Tests/PreloadDisposeDrainGapTests.cs` (C# baseline d92d6aae, 90 lines) and
//! `PreloadLateDisposalTests.cs` (96 lines): dispose past `DisposeDrainTimeout` returns with a warning; the late
//! disposal completes only after the uncancellable worker has finished.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Documented skips:
//! - `Dispose_WorkerOutlivesDrainTimeout_WarnsAndWorkerFinishesCleanly` checks that no `ObjectDisposedException` escapes
//!   the worker through a first-chance hook (`AppDomain.FirstChanceException`). Rust has no such hook and the slots are
//!   owned by `Arc`, so a slot cannot be disposed under a running worker: the claim is covered by the absence of
//!   error log lines and by the run completing without a fault (`RunHandle` has no faulted state).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::preload_fakes::*;
use pr_core::catalog::types::CatalogEntry;
use pr_imaging::preload::scheduler::PreloadScheduler;
use pr_imaging::preload_order::PreloadWindow;

/// C# `Dispose_WorkerOutlivesDrainTimeout_WarnsAndWorkerFinishesCleanly` (uncancellable decode, 100 ms drain timeout).
#[test]
fn dispose_worker_outlives_drain_timeout_warns_and_worker_finishes_cleanly() {
    // C#: PreloadDisposeDrainGapTests.Dispose_WorkerOutlivesDrainTimeout_WarnsAndWorkerFinishesCleanly
    let fs = fake_fs();
    let entries = entries(&fs, 3, "i");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    target.set_on_preload(gated_ignoring_cancel(&gate));
    let log = CaptureLog::with_enabled(true);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::DEFAULT).default_threshold().log(&log),
    );
    scheduler.set_dispose_drain_timeout(Duration::from_millis(100));

    let run = scheduler.preload_around(0);
    let started = first_started(&target);
    scheduler.dispose(); // returns although the decode is still running
    assert!(!target.is_cached(&started), "the decode has not finished yet");
    assert!(log.warnings().iter().any(|w| w.contains("did not drain")));
    gate.open();
    assert!(finished(&run), "the abandoned run unwinds");
    assert!(target.is_cached(&started));
    assert!(log.errors().is_empty());
}

/// C# `target.Started` (the first decode entered, whichever image it is). The centre itself is never preloaded (the
/// viewer decodes it), so the first decode is a neighbour of entry 0.
fn first_started(target: &FakeTarget) -> String {
    assert!(target.wait_started_count(1), "a decode started");
    target.started()[0].clone()
}

/// C# `PreloadLateDisposalTests` fixture: a decode that ignores cancellation, a 100 ms drain timeout, a null log.
fn blocked_scheduler() -> (Arc<FakeTarget>, Arc<Gate>, PreloadScheduler, Arc<[CatalogEntry]>) {
    let fs = fake_fs();
    let entries = entries(&fs, 3, "i");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    target.set_on_preload(gated_ignoring_cancel(&gate));
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::DEFAULT).default_threshold(),
    );
    scheduler.set_dispose_drain_timeout(Duration::from_millis(100));
    (target, gate, scheduler, entries)
}

/// C# `Dispose_DrainsInTime_NoLateDisposal`: a decode that finishes in time leaves no late disposal behind.
#[test]
fn dispose_drains_in_time_no_late_disposal() {
    // C#: PreloadLateDisposalTests.Dispose_DrainsInTime_NoLateDisposal
    let (target, gate, scheduler, _entries) = blocked_scheduler();
    let run = scheduler.preload_around(0);
    first_started(&target);
    gate.open();
    assert!(finished(&run));
    scheduler.dispose();
    assert!(scheduler.late_disposal_for_tests().is_none());
}

/// C# `Dispose_WorkerOutlivesDrainTimeout_LateDisposalCompletesAfterRelease`: the late disposal completes only after the
/// blocked decode finished, and then successfully.
#[test]
fn dispose_worker_outlives_drain_timeout_late_disposal_completes_after_release() {
    // C#: PreloadLateDisposalTests.Dispose_WorkerOutlivesDrainTimeout_LateDisposalCompletesAfterRelease
    let (target, gate, scheduler, _entries) = blocked_scheduler();
    let run = scheduler.preload_around(0);
    first_started(&target);
    scheduler.dispose();
    let late = scheduler
        .late_disposal_for_tests()
        .expect("the dispose timed out: a late disposal");
    assert!(
        !late.is_complete(),
        "slots/CTS were released while the decode was still running"
    );
    gate.open();
    assert!(finished(&run));
    assert!(
        late.wait_timeout(GUARD),
        "the late disposal completes after the last worker"
    );
    assert!(late.is_complete());
}
