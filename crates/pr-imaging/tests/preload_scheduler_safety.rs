//! Port of `tests/PhotoReview.Imaging.Tests/PreloadSafetyTests.cs` (C# baseline d92d6aae, 204 lines): memory headroom gates
//! every decode start, cancellation drains the active workers before dispose returns, and a restart never reuses a
//! cancelled lifetime.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Test notes: C# `Task.Delay(10)` decodes become instant decodes (the assertions do not depend on the delay); the
//! C# `Task.Delay(Infinite, ct)` decode becomes a gate that only the token releases (see `blocks_until_cancelled`).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::preload_fakes::*;
use pr_imaging::preload_order::PreloadWindow;

/// Counters of a decode that blocks until its token is cancelled (C# `RecordingTarget(blockUntilCancellation: true)`).
#[derive(Default)]
struct Counters {
    cancelled: AtomicUsize,
    completed: AtomicUsize,
}

/// Makes every decode of `target` block until its lifetime is cancelled, counting cancellations and completions.
fn block_until_cancelled(target: &Arc<FakeTarget>, counters: &Arc<Counters>) {
    let never = Gate::new();
    let counters = Arc::clone(counters);
    target.set_on_preload(move |_path, token| {
        let result = if never.wait(Some(token)) {
            Ok(())
        } else {
            counters.cancelled.fetch_add(1, Ordering::SeqCst);
            Err(pr_imaging::decoder_traits::DecodeError::Canceled)
        };
        counters.completed.fetch_add(1, Ordering::SeqCst);
        result
    });
}

/// C# `Create(target, probe, workerCount)` over the 8 fixture files `image-0 .. image-7`.
fn fixture(
    target: &Arc<FakeTarget>,
    entries: &Arc<[pr_core::catalog::types::CatalogEntry]>,
    probe: Arc<dyn pr_core::abstractions::MemoryProbe>,
    workers: i32,
) -> pr_imaging::preload::scheduler::PreloadScheduler {
    create(
        target,
        entries,
        Setup::new(workers, PreloadWindow::DEFAULT)
            .default_threshold()
            .probe(probe),
    )
}

#[test]
fn memory_pressure_does_not_start_decode() {
    // C#: PreloadSafetyTests.MemoryPressure_DoesNotStartDecode
    let fs = fake_fs();
    let entries = entries(&fs, 8, "image");
    let target = FakeTarget::new(&fs);
    let scheduler = fixture(&target, &entries, FakeProbe::answering(false), 2);
    assert!(finished(&scheduler.preload_around(0)));
    assert!(target.started().is_empty());
}

#[test]
fn available_memory_continues_across_multiple_batches() {
    // C#: PreloadSafetyTests.AvailableMemory_ContinuesAcrossMultipleBatches
    let fs = fake_fs();
    let entries = entries(&fs, 8, "image");
    let target = FakeTarget::new(&fs);
    let scheduler = fixture(&target, &entries, FakeProbe::answering(true), 2);
    assert!(finished(&scheduler.preload_around(0)));
    let started = target.started().len();
    assert!(
        started > 2,
        "Expected more than one worker batch, got {started} decodes."
    );
}

#[test]
fn headroom_lost_mid_batch_stops_new_starts() {
    // C#: PreloadSafetyTests.HeadroomLost_MidBatch_StopsNewStarts (IMG-03: a per-batch check would start up to 7 more)
    let fs = fake_fs();
    let entries = entries(&fs, 8, "image");
    let target = FakeTarget::new(&fs);
    // DIVERGENCE: "started" = dispatched (see `count_dispatches`): the decode itself runs on a pool thread.
    let dispatched = Arc::new(AtomicUsize::new(0));
    let probe_dispatched = Arc::clone(&dispatched);
    let probe = FakeProbe::new(move || probe_dispatched.load(Ordering::SeqCst) == 0);
    let scheduler = fixture(&target, &entries, probe, 8);
    count_dispatches(&scheduler, &dispatched);
    assert!(scheduler.preload_around(0).wait_timeout(Duration::from_secs(5)));
    let started = target.started().len();
    assert!((1..=2).contains(&started), "started {started}");
}

#[test]
fn dispose_cancels_and_drains_active_workers_before_returning() {
    // C#: PreloadSafetyTests.Dispose_CancelsAndDrainsActiveWorkersBeforeReturning
    let fs = fake_fs();
    let entries = entries(&fs, 8, "image");
    let target = FakeTarget::new(&fs);
    let counters = Arc::new(Counters::default());
    block_until_cancelled(&target, &counters);
    let scheduler = fixture(&target, &entries, FakeProbe::answering(true), 2);
    let preload = scheduler.preload_around(0);
    assert!(target.wait_started_count(2));
    scheduler.dispose();
    assert!(preload.wait_timeout(Duration::from_secs(5)));
    assert_eq!(2, counters.cancelled.load(Ordering::SeqCst));
    assert_eq!(target.started().len(), counters.completed.load(Ordering::SeqCst));
    assert!(scheduler.preload_around(0).is_complete());
    scheduler.dispose(); // idempotent
}

#[test]
fn dispose_worker_ignores_cancellation_returns_within_drain_timeout() {
    // C#: PreloadSafetyTests.Dispose_WorkerIgnoresCancellation_ReturnsWithinDrainTimeout
    let fs = fake_fs();
    let entries = entries(&fs, 8, "image");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    target.set_on_preload(gated_ignoring_cancel(&gate));
    let scheduler = fixture(&target, &entries, FakeProbe::answering(true), 2);
    scheduler.set_dispose_drain_timeout(Duration::from_millis(200));
    let _run = scheduler.preload_around(0);
    assert!(target.wait_started_count(1));
    let stopwatch = Instant::now();
    scheduler.dispose();
    let elapsed = stopwatch.elapsed();
    assert!(elapsed < Duration::from_secs(3), "Dispose blocked for {elapsed:?}.");
    gate.open(); // let the abandoned workers finish
}

#[test]
fn cancel_then_restart_uses_fresh_lifetime_and_dispose_stops_it() {
    // C#: PreloadSafetyTests.CancelThenRestart_UsesFreshLifetime_AndDisposeStopsIt
    let fs = fake_fs();
    let entries = entries(&fs, 8, "image");
    let target = FakeTarget::new(&fs);
    let counters = Arc::new(Counters::default());
    block_until_cancelled(&target, &counters);
    let scheduler = fixture(&target, &entries, FakeProbe::answering(true), 1);
    let first = scheduler.preload_around(0);
    assert!(target.wait_started_count(1));
    scheduler.cancel();
    assert!(first.wait_timeout(Duration::from_secs(5)));
    let second = scheduler.preload_around(1);
    assert!(target.wait_started_count(2));
    scheduler.dispose();
    assert!(second.wait_timeout(Duration::from_secs(5)));
    assert_eq!(2, counters.cancelled.load(Ordering::SeqCst));
    assert_eq!(target.started().len(), counters.completed.load(Ordering::SeqCst));
}

#[test]
fn cancel_then_restart_before_drain_dispose_waits_for_both_lifetimes() {
    // C#: PreloadSafetyTests.CancelThenRestartBeforeDrain_DisposeWaitsForBothLifetimes
    let fs = fake_fs();
    let entries = entries(&fs, 8, "image");
    let target = FakeTarget::new(&fs);
    let counters = Arc::new(Counters::default());
    block_until_cancelled(&target, &counters);
    let scheduler = fixture(&target, &entries, FakeProbe::answering(true), 1);
    let first = scheduler.preload_around(0);
    assert!(target.wait_started_count(1));
    scheduler.cancel();
    // Start the replacement lifetime before the cancelled worker has necessarily unwound. Dispose must retain
    // ownership of both scheduler runs.
    let second = scheduler.preload_around(1);
    assert!(target.wait_started_count(2));
    scheduler.dispose();
    assert!(first.wait_timeout(Duration::from_secs(5)) && second.wait_timeout(Duration::from_secs(5)));
    assert_eq!(2, counters.cancelled.load(Ordering::SeqCst));
    assert_eq!(target.started().len(), counters.completed.load(Ordering::SeqCst));
}
