//! Red tests of W5-16a `FullDecodeGate` (contracts/W5-16a.md): port of the managed single-slot gate tests of the C# baseline
//! d92d6aae (`tests/PhotoReview.Imaging.Tests/Raw/LibRawFullDecodeGateTests.cs`, `LibRawGateAndGuardMutationGapTests.cs`
//! FullDecodeGate half, `RawErrorHandlingReviewTests.cs`), the new Rust-only tests of the contract (`// pins:`), and the golden
//! driver over `tests/golden/full-decode-gate.json` (30 cases, inputs in `tests/golden-inputs/full-decode-gate.json`).
//!
//! Red tests (W5-16a): they compile against the stub and fail through `todo!()` until the implementation lands.
//! Deterministic: no sleeps, no polling, no elapsed-time asserts. Every wait is a permit or a channel event with a 30 s bound.
//! Needs neither libraw.dll nor the corpus.

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use pr_core::abstractions::SourceReadPriority;
use pr_core::abstractions::SourceReadPriority::{Preload, Viewer};
use pr_core::sync::{CancelToken, Canceled};
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::fallback::FallbackImageDecoder;
use pr_libraw::full_decode_gate::{FullDecodeGate, FullDecodeLease, GateError, PRELOAD_QUEUE_BUSY_MESSAGE};
use pr_test_support::golden;
use serde_json::{Value, json};

const BOUND: Duration = Duration::from_secs(30);
const GOLDEN: &str = "full-decode-gate";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// C# `SemaphoreSlim`: `release` adds a permit, `acquire` takes one within [`BOUND`]. No sleeps, no polling.
#[derive(Default)]
struct Permits {
    count: Mutex<u32>,
    changed: Condvar,
}

impl Permits {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn release(&self) {
        *lock(&self.count) += 1;
        self.changed.notify_all();
    }

    fn acquire(&self) -> bool {
        let guard = lock(&self.count);
        let (mut guard, _) = self
            .changed
            .wait_timeout_while(guard, BOUND, |count| *count == 0)
            .unwrap_or_else(PoisonError::into_inner);
        if *guard == 0 {
            return false;
        }
        *guard -= 1;
        true
    }
}

/// A waiter thread of the [`Harness`] (C# `Harness.Start`): it enters, records its name, then holds the slot until `release`.
struct Started {
    handle: JoinHandle<Result<(), GateError>>,
    release: Arc<Permits>,
    /// Signalled after the name is recorded: the waiter now owns the slot.
    granted: Arc<Permits>,
}

/// Joins a waiter thread. A panicking thread (a failed assertion inside it, or a `todo!()`) fails the test here.
fn join(handle: JoinHandle<Result<(), GateError>>) -> Result<(), GateError> {
    handle.join().unwrap_or_else(|payload| panic::resume_unwind(payload))
}

/// Releases every waiter (C# `Release.Release()` for each) and joins them, in the order given.
fn finish_all(started: Vec<Started>) -> Vec<Result<(), GateError>> {
    for waiter in &started {
        waiter.release.release();
    }
    started.into_iter().map(|waiter| join(waiter.handle)).collect()
}

/// C# `LibRawFullDecodeGateTests` harness: a gate whose hook counts queued waiters and a shared record of grant order.
struct Harness {
    gate: FullDecodeGate,
    queued: Arc<Permits>,
    hook_calls: Arc<AtomicUsize>,
    order: Arc<Mutex<Vec<String>>>,
}

impl Harness {
    fn new(max_queued_preloads: i32) -> Self {
        let queued = Permits::new();
        let hook_calls = Arc::new(AtomicUsize::new(0));
        let (hook_queued, hook_count) = (Arc::clone(&queued), Arc::clone(&hook_calls));
        let gate = FullDecodeGate::with_waiter_queued(
            max_queued_preloads,
            Arc::new(move |_priority| {
                hook_count.fetch_add(1, Ordering::SeqCst);
                hook_queued.release();
                Ok(())
            }),
        );
        Self {
            gate,
            queued,
            hook_calls,
            order: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// C# `AwaitQueued(1)`: one waiter has reached the queue.
    fn await_queued(&self) {
        assert!(self.queued.acquire(), "waiter never queued");
    }

    fn order(&self) -> Vec<String> {
        lock(&self.order).clone()
    }

    /// C# `Harness.Start`: the waiter enters with `token`, marks itself worked when asked, records its name, and holds the
    /// slot until its own `release` permit is released.
    fn start(&self, name: &str, priority: SourceReadPriority, worked: bool, token: CancelToken) -> Started {
        let gate = self.gate.clone();
        let order = Arc::clone(&self.order);
        let release = Permits::new();
        let granted = Permits::new();
        let (release_wait, granted_signal) = (Arc::clone(&release), Arc::clone(&granted));
        let name = name.to_owned();
        let handle = thread::spawn(move || -> Result<(), GateError> {
            let mut lease = gate.enter(priority, &token)?;
            if worked {
                lease.mark_worked();
            }
            lock(&order).push(name);
            granted_signal.release();
            assert!(release_wait.acquire(), "release never signalled");
            lease.release();
            Ok(())
        });
        Started {
            handle,
            release,
            granted,
        }
    }
}

// ---- LibRawFullDecodeGateTests (C#, 14 facts) ------------------------------------------------------------------------------

#[test]
fn enter_viewer_arrives_after_three_queued_preloads_is_served_before_them() {
    // C#: LibRawFullDecodeGateTests.Enter_ViewerArrivesAfterThreeQueuedPreloads_IsServedBeforeThem
    let h = Harness::new(3);
    let running = h
        .gate
        .enter(Preload, &CancelToken::none())
        .expect("the free slot is granted");
    let p1 = h.start("p1", Preload, true, CancelToken::new());
    h.await_queued();
    let p2 = h.start("p2", Preload, true, CancelToken::new());
    h.await_queued();
    let p3 = h.start("p3", Preload, true, CancelToken::new());
    h.await_queued();
    let viewer = h.start("viewer", Viewer, true, CancelToken::new());
    h.await_queued();

    drop(running);
    for result in finish_all(vec![viewer, p1, p2, p3]) {
        result.expect("every waiter is granted and released");
    }

    assert_eq!(h.order(), ["viewer", "p1", "p2", "p3"]);
}

#[test]
fn enter_viewer_waiters_are_served_in_arrival_order() {
    // C#: LibRawFullDecodeGateTests.Enter_ViewerWaitersAreServedInArrivalOrder
    let h = Harness::new(2);
    let running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let v1 = h.start("v1", Viewer, true, CancelToken::new());
    h.await_queued();
    let v2 = h.start("v2", Viewer, true, CancelToken::new());
    h.await_queued();

    drop(running);
    for result in finish_all(vec![v1, v2]) {
        result.expect("every waiter is granted and released");
    }

    assert_eq!(h.order(), ["v1", "v2"]);
}

#[test]
fn enter_while_a_decode_runs_the_running_decode_is_never_interrupted_and_the_slot_stays_taken() {
    // C#: LibRawFullDecodeGateTests.Enter_WhileADecodeRuns_TheRunningDecodeIsNeverInterruptedAndTheSlotStaysTaken
    let h = Harness::new(2);
    let running_token = CancelToken::new();
    let running = h.gate.enter(Preload, &running_token).expect("the free slot is granted");
    let viewer = h.start("viewer", Viewer, true, CancelToken::new());
    h.await_queued();

    // A waiting viewer neither steals the slot nor cancels the holder (its token stays untouched).
    assert_eq!(h.gate.slots_available(), 0);
    assert!(!running_token.is_canceled());
    assert!(h.order().is_empty());

    drop(running);
    viewer.release.release();
    join(viewer.handle).expect("the viewer is granted after the holder releases");
    assert_eq!(h.gate.slots_available(), 1);
}

#[test]
fn enter_queued_waiter_cancelled_releases_it_without_touching_the_slot() {
    // C#: LibRawFullDecodeGateTests.Enter_QueuedWaiterCancelled_ReleasesItWithoutTouchingTheSlot
    let h = Harness::new(2);
    let running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let cancel = CancelToken::new();
    let waiter = h.start("cancelled", Viewer, true, cancel.clone());
    h.await_queued();

    cancel.cancel();
    assert_eq!(join(waiter.handle), Err(GateError::Canceled));

    assert_eq!(h.gate.slots_available(), 0); // the holder still owns the slot
    assert_eq!(h.gate.queued_viewers(), 0); // and the cancelled waiter left the queue
    drop(running);
    assert_eq!(h.gate.slots_available(), 1);
    assert!(h.order().is_empty());
}

#[test]
fn enter_cancelled_preload_waiter_frees_its_queue_spot_for_another_preload() {
    // C#: LibRawFullDecodeGateTests.Enter_CancelledPreloadWaiter_FreesItsQueueSpotForAnotherPreload
    let h = Harness::new(1);
    let running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let cancel = CancelToken::new();
    let first = h.start("first", Preload, true, cancel.clone());
    h.await_queued();
    cancel.cancel();
    assert_eq!(join(first.handle), Err(GateError::Canceled));

    // Would be refused if the cancelled waiter still counted toward the bound.
    let second = h.start("second", Preload, true, CancelToken::new());
    h.await_queued();
    drop(running);
    second.release.release();

    join(second.handle).expect("the second preload is granted");
    assert_eq!(h.order(), ["second"]);
}

#[test]
fn enter_already_cancelled_token_throws_before_taking_the_slot() {
    // C#: LibRawFullDecodeGateTests.Enter_AlreadyCancelledToken_ThrowsBeforeTakingTheSlot
    let gate = FullDecodeGate::new(2);
    let cancel = CancelToken::new();
    cancel.cancel();

    let outcome = gate.enter(Viewer, &cancel).map(|_lease| ());
    assert_eq!(outcome, Err(GateError::Canceled));

    assert_eq!(gate.slots_available(), 1);
}

#[test]
fn enter_more_than_the_maximum_queued_preloads_fails_fast_without_queueing() {
    // C#: LibRawFullDecodeGateTests.Enter_MoreThanTheMaximumQueuedPreloads_FailFastWithoutQueueing
    let h = Harness::new(2);
    let running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let p1 = h.start("p1", Preload, true, CancelToken::new());
    h.await_queued();
    let p2 = h.start("p2", Preload, true, CancelToken::new());
    h.await_queued();

    match h.gate.enter(Preload, &CancelToken::new()) {
        Err(GateError::Busy { message }) => assert_eq!(message, PRELOAD_QUEUE_BUSY_MESSAGE),
        other => panic!("a third preload must be refused as Busy, got {other:?}"),
    }
    assert_eq!(h.gate.queued_preloads(), 2);

    // A viewer is never refused because preloads are queued.
    let viewer = h.start("viewer", Viewer, true, CancelToken::new());
    h.await_queued();
    drop(running);
    for result in finish_all(vec![viewer, p1, p2]) {
        result.expect("every waiter is granted and released");
    }
    assert_eq!(h.order(), ["viewer", "p1", "p2"]);
}

#[test]
fn enter_preload_waiter_that_saw_three_decodes_finish_is_promoted_ahead_of_newer_viewers() {
    // C#: LibRawFullDecodeGateTests.Enter_PreloadWaiterThatSawThreeDecodesFinish_IsPromotedAheadOfNewerViewers
    let h = Harness::new(2);
    let running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let preload = h.start("p", Preload, true, CancelToken::new());
    h.await_queued();
    let v1 = h.start("v1", Viewer, true, CancelToken::new());
    h.await_queued();
    let v2 = h.start("v2", Viewer, true, CancelToken::new());
    h.await_queued();
    let v3 = h.start("v3", Viewer, true, CancelToken::new());
    h.await_queued();
    let v4 = h.start("v4", Viewer, true, CancelToken::new());
    h.await_queued();

    running.mark_worked();
    drop(running); // completion 1: the viewer lane keeps priority while the preload is young
    for result in finish_all(vec![v1, v2, preload, v3, v4]) {
        result.expect("every waiter is granted and released");
    }

    // v1 and v2 finish (completions 2 and 3): the preload has now aged out and goes before v3/v4.
    assert_eq!(h.order(), ["v1", "v2", "p", "v3", "v4"]);
}

#[test]
fn enter_preload_waiter_below_the_aging_bound_still_yields_to_viewers() {
    // C#: LibRawFullDecodeGateTests.Enter_PreloadWaiterBelowTheAgingBound_StillYieldsToViewers
    let h = Harness::new(2);
    let mut running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let preload = h.start("p", Preload, true, CancelToken::new());
    h.await_queued();
    let v1 = h.start("v1", Viewer, true, CancelToken::new());
    h.await_queued();
    let v2 = h.start("v2", Viewer, true, CancelToken::new());
    h.await_queued();

    running.mark_worked();
    running.release();
    for result in finish_all(vec![v1, v2, preload]) {
        result.expect("every waiter is granted and released");
    }

    // Only 2 completions before the preload's turn: not aged yet.
    assert_eq!(h.order(), ["v1", "v2", "p"]);
}

#[test]
fn enter_failed_leases_that_did_no_work_do_not_age_a_queued_preload() {
    // C#: LibRawFullDecodeGateTests.Enter_FailedLeasesThatDidNoWork_DoNotAgeAQueuedPreload
    let h = Harness::new(2);
    // Never marked worked: e.g. a memory-guard refusal.
    let running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let preload = h.start("p", Preload, false, CancelToken::new());
    h.await_queued();
    let mut viewers = Vec::new();
    for name in ["v1", "v2", "v3", "v4"] {
        viewers.push(h.start(name, Viewer, false, CancelToken::new()));
        h.await_queued();
    }

    drop(running);
    viewers.push(preload);
    for result in finish_all(viewers) {
        result.expect("every waiter is granted and released");
    }

    // Four instant failures are not "three completions": no promotion.
    assert_eq!(h.order(), ["v1", "v2", "v3", "v4", "p"]);
}

#[test]
fn enter_two_overdue_slow_preloads_are_promoted_one_at_a_time_with_a_viewer_served_between() {
    // C#: LibRawFullDecodeGateTests.Enter_TwoOverdueSlowPreloads_ArePromotedOneAtATimeWithAViewerServedBetween
    let h = Harness::new(2);
    let mut running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let p1 = h.start("p1", Preload, true, CancelToken::new());
    h.await_queued();
    let p2 = h.start("p2", Preload, true, CancelToken::new());
    h.await_queued();
    let mut waiters = vec![p1, p2];
    for name in ["v1", "v2", "v3", "v4"] {
        waiters.push(h.start(name, Viewer, true, CancelToken::new()));
        h.await_queued();
    }

    running.mark_worked();
    running.release();
    for result in finish_all(waiters) {
        result.expect("every waiter is granted and released");
    }

    // Both preloads were stamped at completion 0. p1 ages out after three worked completions; p2 must then age again
    // (a promotion moves the baseline) instead of following right behind, so v3/v4 are served first.
    assert_eq!(h.order(), ["v1", "v2", "p1", "v3", "v4", "p2"]);
}

#[test]
fn lease_marked_worked_but_disposed_twice_counts_one_completion() {
    // C#: LibRawFullDecodeGateTests.Lease_MarkedWorkedButDisposedTwice_CountsOneCompletion
    let h = Harness::new(2);
    let mut running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let p = h.start("p", Preload, true, CancelToken::new());
    h.await_queued();
    let v1 = h.start("v1", Viewer, true, CancelToken::new());
    h.await_queued();
    let v2 = h.start("v2", Viewer, true, CancelToken::new());
    h.await_queued();

    running.mark_worked();
    running.release();
    // A second disposal must not advance the clock a second time.
    running.release();
    for result in finish_all(vec![v1, v2, p]) {
        result.expect("every waiter is granted and released");
    }

    // Completions 1 (running), 2 (v1): the preload is not yet aged at v2's turn.
    assert_eq!(h.order(), ["v1", "v2", "p"]);
}

#[test]
fn lease_disposed_twice_releases_the_slot_only_once() {
    // C#: LibRawFullDecodeGateTests.Lease_DisposedTwice_ReleasesTheSlotOnlyOnce
    let h = Harness::new(2);
    let mut first = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let waiter = h.start("waiter", Viewer, true, CancelToken::new());
    h.await_queued();

    first.release();
    assert_eq!(h.gate.slots_available(), 0); // handed to the waiter
    first.release(); // must not release the waiter's slot
    assert_eq!(h.gate.slots_available(), 0);

    waiter.release.release();
    join(waiter.handle).expect("the waiter is granted and released");
    assert_eq!(h.gate.slots_available(), 1);
}

#[test]
fn enter_every_path_leaves_the_gate_free_at_the_end() {
    // C#: LibRawFullDecodeGateTests.Enter_EveryPath_LeavesTheGateFreeAtTheEnd
    let h = Harness::new(1);
    let running = h
        .gate
        .enter(Preload, &CancelToken::none())
        .expect("the free slot is granted");
    let queued = h.start("p", Preload, true, CancelToken::new());
    h.await_queued();
    assert!(matches!(
        h.gate.enter(Preload, &CancelToken::new()),
        Err(GateError::Busy { .. })
    ));
    queued.release.release();
    drop(running);

    join(queued.handle).expect("the queued preload is granted and released");
    assert_eq!(h.gate.slots_available(), 1);
    assert_eq!(h.gate.queued_preloads(), 0);
    assert_eq!(h.gate.queued_viewers(), 0);
}

// ---- LibRawGateAndGuardMutationGapTests (FullDecodeGate half) and RawErrorHandlingReviewTests (C#) -------------------------

#[test]
fn queued_viewers_a_viewer_waits_behind_the_running_decode_is_counted_in_its_own_lane() {
    // C#: LibRawGateAndGuardMutationGapTests.QueuedViewers_AViewerWaitsBehindTheRunningDecode_IsCountedInItsOwnLane
    let h = Harness::new(2);
    let running = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let viewer = h.start("viewer", Viewer, true, CancelToken::new());
    h.await_queued();

    assert_eq!((h.gate.queued_viewers(), h.gate.queued_preloads()), (1, 0));

    drop(running);
    viewer.release.release();
    join(viewer.handle).expect("the viewer is granted and released");
    assert_eq!((h.gate.queued_viewers(), h.gate.queued_preloads()), (0, 0));
}

#[test]
fn enter_slot_granted_in_the_same_instant_as_a_failure_while_queuing_passes_the_slot_on_instead_of_losing_it() {
    // C#: LibRawGateAndGuardMutationGapTests.Enter_SlotGrantedInTheSameInstantAsAFailureWhileQueuing_PassesTheSlotOnInsteadOfLosingIt
    // The queued seam hands the slot to this very waiter (releasing the holder) and then fails: the "granted and cancelled at
    // once" race. The grant must not be lost: the slot has to end up free. Taking the lease out of the slot breaks the
    // gate -> hook -> lease -> gate `Arc` cycle.
    let holder_slot: Arc<Mutex<Option<FullDecodeLease>>> = Arc::new(Mutex::new(None));
    let hook_slot = Arc::clone(&holder_slot);
    let gate = FullDecodeGate::with_waiter_queued(
        2,
        Arc::new(move |_priority| -> Result<(), String> {
            let holder = lock(&hook_slot).take();
            drop(holder);
            Err("seam failed after the grant".to_owned())
        }),
    );
    *lock(&holder_slot) = Some(
        gate.enter(Viewer, &CancelToken::none())
            .expect("the free slot is granted"),
    );

    let outcome = gate.enter(Viewer, &CancelToken::none());
    match outcome {
        Err(GateError::WaiterHook { message }) => assert_eq!(message, "seam failed after the grant"),
        other => panic!("the seam failure must surface as WaiterHook, got {other:?}"),
    }

    assert_eq!(gate.slots_available(), 1);
    assert_eq!(gate.queued_viewers(), 0);
    let next = gate
        .enter(Viewer, &CancelToken::none())
        .expect("the slot is really usable again");
    assert_eq!(gate.slots_available(), 0);
    drop(next);
}

#[test]
fn full_decode_gate_waiter_queued_seam_throws_does_not_orphan_waiter_or_stick_busy() {
    // C#: RawErrorHandlingReviewTests.FullDecodeGate_WaiterQueuedSeamThrows_DoesNotOrphanWaiterOrStickBusy
    let gate = FullDecodeGate::with_waiter_queued(
        2,
        Arc::new(|_priority| -> Result<(), String> { Err("seam".to_owned()) }),
    );
    let holder = gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");

    assert!(matches!(
        gate.enter(Viewer, &CancelToken::none()),
        Err(GateError::WaiterHook { .. })
    ));
    assert_eq!(gate.queued_viewers(), 0);

    drop(holder);
    assert_eq!(gate.slots_available(), 1);
}

// ---- New Rust tests (no C# twin): each pins one C# fact that no fact or golden case reaches --------------------------------

#[test]
fn hook_panics_cleans_up_then_resumes() {
    // pins: C# `catch (Exception)` covers every throw; a panicking hook leaves no queued waiter and the slot free after the
    // holder is released, and the panic reaches the caller unchanged.
    let armed = Arc::new(AtomicBool::new(true));
    let armed_hook = Arc::clone(&armed);
    let gate = FullDecodeGate::with_waiter_queued(
        2,
        Arc::new(move |_priority| -> Result<(), String> {
            if armed_hook.swap(false, Ordering::SeqCst) {
                panic!("seam panic");
            }
            Ok(())
        }),
    );
    let holder = gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");

    let outcome = panic::catch_unwind(AssertUnwindSafe(|| gate.enter(Viewer, &CancelToken::none())));
    let payload = outcome.expect_err("the hook panic must reach the caller");
    assert_eq!(payload.downcast_ref::<&str>().copied(), Some("seam panic"));
    assert_eq!(gate.queued_viewers(), 0);

    drop(holder);
    assert_eq!(gate.slots_available(), 1);
}

#[test]
fn hook_not_called_for_free_slot_or_refused_preload() {
    // pins: C# `_waiterQueued` is invoked only after `AddLast`: never for an immediate grant, never for a refused preload.
    let h = Harness::new(1);
    let free = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    assert_eq!(h.hook_calls.load(Ordering::SeqCst), 0);
    drop(free);

    let holder = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    assert_eq!(h.hook_calls.load(Ordering::SeqCst), 0);

    let queued = h.start("p1", Preload, true, CancelToken::new());
    h.await_queued();
    assert_eq!(h.hook_calls.load(Ordering::SeqCst), 1);

    // The bound (1) is full: this preload is refused and the hook is not called for it.
    assert!(matches!(
        h.gate.enter(Preload, &CancelToken::new()),
        Err(GateError::Busy { .. })
    ));
    assert_eq!(h.hook_calls.load(Ordering::SeqCst), 1);

    drop(holder);
    queued.release.release();
    join(queued.handle).expect("the queued preload is granted and released");
}

#[test]
fn free_slot_ignores_preload_bound() {
    // pins: C# `if (!_busy)` grants before the preload bound is consulted, for a bound of 0 and of -1.
    for max in [0, -1] {
        let gate = FullDecodeGate::new(max);
        let granted = gate
            .enter(Preload, &CancelToken::none())
            .expect("a free slot ignores the preload bound");
        assert_eq!(gate.slots_available(), 0);

        match gate.enter(Preload, &CancelToken::none()) {
            Err(GateError::Busy { message }) => assert_eq!(message, PRELOAD_QUEUE_BUSY_MESSAGE),
            other => panic!("bound {max}: the second preload must be refused as Busy, got {other:?}"),
        }

        drop(granted);
        assert_eq!(gate.slots_available(), 1);
    }
}

#[test]
fn lease_drop_releases() {
    // pins: C# `IDisposable` -> Rust `Drop`: dropping the only lease frees the slot and `is_held` reports it.
    let gate = FullDecodeGate::new(2);
    let lease = gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    assert!(lease.is_held());
    assert_eq!(gate.slots_available(), 0);

    drop(lease);
    assert_eq!(gate.slots_available(), 1);
}

#[test]
fn release_then_drop_releases_once() {
    // pins: `release` then the final `Drop` free the slot once: the waiter granted by `release` keeps it through the drop,
    // and `is_held` is false for the released lease.
    let h = Harness::new(2);
    let mut holder = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let waiter = h.start("waiter", Viewer, true, CancelToken::new());
    h.await_queued();

    holder.release();
    assert!(!holder.is_held());
    assert_eq!(h.gate.slots_available(), 0); // handed to the waiter

    drop(holder); // the final Drop must not release the waiter's slot
    assert_eq!(h.gate.slots_available(), 0);

    waiter.release.release();
    join(waiter.handle).expect("the waiter is granted and released");
    assert_eq!(h.gate.slots_available(), 1);
}

#[test]
fn gate_error_into_decode_error() {
    // pins: `From<GateError> for DecodeError` (Canceled -> Canceled; Busy keeps the message and is never fallbackable (INV-12);
    // WaiterHook -> InvalidOperation), `From<Canceled>`, and the `Display` texts.
    assert!(matches!(DecodeError::from(GateError::Canceled), DecodeError::Canceled));

    let busy = DecodeError::from(GateError::Busy {
        message: PRELOAD_QUEUE_BUSY_MESSAGE.to_owned(),
    });
    match &busy {
        DecodeError::Busy { message } => assert_eq!(message, PRELOAD_QUEUE_BUSY_MESSAGE),
        other => panic!("Busy must map to DecodeError::Busy, got {other:?}"),
    }
    assert!(!FallbackImageDecoder::is_fallbackable(&busy));

    match DecodeError::from(GateError::WaiterHook {
        message: "seam".to_owned(),
    }) {
        DecodeError::InvalidOperation { message } => assert_eq!(message, "seam"),
        other => panic!("WaiterHook must map to InvalidOperation, got {other:?}"),
    }

    assert_eq!(GateError::from(Canceled), GateError::Canceled);
    assert_eq!(GateError::Canceled.to_string(), "operation canceled");
    assert_eq!(
        GateError::Busy {
            message: PRELOAD_QUEUE_BUSY_MESSAGE.to_owned()
        }
        .to_string(),
        PRELOAD_QUEUE_BUSY_MESSAGE
    );
    assert_eq!(
        GateError::WaiterHook {
            message: "seam".to_owned()
        }
        .to_string(),
        "seam"
    );
}

#[test]
fn unworked_release_still_promotes_an_overdue_preload() {
    // pins: the overdue check runs on EVERY release, worked or not. Reachable form: the promoted preload `p` (stamped 0)
    // is granted by the third worked completion (v2); its own lease did no work, and its release still hands the slot on
    // to v3 (no slot lost, no second promotion at completion 3). Expected order v1 v2 p v3 v4.
    let h = Harness::new(2);
    let mut holder = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let preload = h.start("p", Preload, false, CancelToken::new());
    h.await_queued();
    let mut waiters = vec![preload];
    for name in ["v1", "v2", "v3", "v4"] {
        waiters.push(h.start(name, Viewer, true, CancelToken::new()));
        h.await_queued();
    }

    holder.mark_worked();
    holder.release();
    for result in finish_all(waiters) {
        result.expect("every waiter is granted and released");
    }

    assert_eq!(h.order(), ["v1", "v2", "p", "v3", "v4"]);
}

#[test]
fn cancel_is_seen_within_poll_interval() {
    // pins: a queued waiter cancelled from another thread returns within the bound (the 10 ms poll interval is documented,
    // not asserted: no elapsed-time check).
    let h = Harness::new(2);
    let holder = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let cancel = CancelToken::new();
    let (results, received) = mpsc::channel();
    let (gate, token) = (h.gate.clone(), cancel.clone());
    thread::spawn(move || {
        let _ = results.send(gate.enter(Viewer, &token).map(|_lease| ()));
    });
    h.await_queued();

    cancel.cancel();
    let outcome = received
        .recv_timeout(BOUND)
        .expect("a cancelled waiter must return within the bound");
    assert_eq!(outcome, Err(GateError::Canceled));
    assert_eq!(h.gate.queued_viewers(), 0);

    drop(holder);
    assert_eq!(h.gate.slots_available(), 1);
}

#[test]
fn gate_and_lease_and_error_are_send_and_sync() {
    // pins: W5-16c shares one process-wide gate across threads (`static`), so the gate, its leases and its errors cross threads.
    fn assert_send_sync<T: Send + Sync>(_value: &T) {}

    let gate = FullDecodeGate::new(2);
    assert_send_sync(&gate);
    let lease = gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    assert_send_sync(&lease);
    assert_send_sync(&GateError::Canceled);
}

#[test]
fn preload_queued_after_completions_ages_from_its_stamp() {
    // pins: the `max(enqueued_at_completion, last_promotion_completion)` of C# `ReleaseLocked`. No C# test and no golden case
    // stamps a preload above 0. Here p is stamped 1, so it is promoted by the 4th completion (after v3), not by the 3rd
    // (after v2, which `last_promotion` alone would give). Expected v1 v2 v3 p v4.
    let h = Harness::new(2);
    let mut holder = h
        .gate
        .enter(Viewer, &CancelToken::none())
        .expect("the free slot is granted");
    let v1 = h.start("v1", Viewer, true, CancelToken::new());
    h.await_queued();

    holder.mark_worked();
    holder.release(); // completion 1: v1 is granted and holds the slot
    assert!(v1.granted.acquire(), "v1 never took the slot");

    let p = h.start("p", Preload, true, CancelToken::new()); // stamped at completion 1
    h.await_queued();
    let v2 = h.start("v2", Viewer, true, CancelToken::new());
    h.await_queued();
    let v3 = h.start("v3", Viewer, true, CancelToken::new());
    h.await_queued();
    let v4 = h.start("v4", Viewer, true, CancelToken::new());
    h.await_queued();

    for result in finish_all(vec![v1, p, v2, v3, v4]) {
        result.expect("every waiter is granted and released");
    }

    assert_eq!(h.order(), ["v1", "v2", "v3", "p", "v4"]);
}

// ---- Golden driver: tests/golden/full-decode-gate.json (30 cases) ----------------------------------------------------------

/// What the driver reports to the main thread: a hook call (a waiter queued) or the end of a waiter thread.
enum Event {
    Queued,
    Finished(String),
}

/// Sends `Finished(name)` when the waiter thread ends, also when it panics, so the driver never waits on a dead thread.
struct FinishGuard {
    name: String,
    events: mpsc::Sender<Event>,
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        let _ = self.events.send(Event::Finished(self.name.clone()));
    }
}

fn priority_of(text: &str) -> SourceReadPriority {
    match text {
        "Viewer" => Viewer,
        "Preload" => Preload,
        other => panic!("unknown priority {other}"),
    }
}

fn next_event(events: &mpsc::Receiver<Event>) -> Event {
    events
        .recv_timeout(BOUND)
        .expect("the driver waited 30 s for the gate: neither a queued waiter nor a finished one")
}

fn snapshot(gate: &FullDecodeGate) -> Value {
    json!({
        "slots": gate.slots_available(),
        "viewers": gate.queued_viewers(),
        "preloads": gate.queued_preloads(),
    })
}

/// One waiter thread of a golden case: enters, marks worked when asked, records its name, releases at once.
fn spawn_waiter(
    gate: FullDecodeGate,
    name: String,
    priority: SourceReadPriority,
    worked: bool,
    token: CancelToken,
    events: mpsc::Sender<Event>,
    order: Arc<Mutex<Vec<String>>>,
) -> JoinHandle<Result<(), GateError>> {
    thread::spawn(move || -> Result<(), GateError> {
        let _finished = FinishGuard {
            name: name.clone(),
            events,
        };
        let mut lease = gate.enter(priority, &token)?;
        if worked {
            lease.mark_worked();
        }
        lock(&order).push(name);
        lease.release();
        Ok(())
    })
}

/// Drives one golden input case (C# `oracle-cs full-decode-gate` protocol) and returns the observed result object.
fn run_case(input: &Value) -> Value {
    let name = golden::str_of(input, "name");
    let max = i32::try_from(golden::i64_of(input, "max_queued_preloads")).expect("max_queued_preloads fits i32");

    let (events, queue_events) = mpsc::channel::<Event>();
    let hook_events = Mutex::new(events.clone());
    let gate = FullDecodeGate::with_waiter_queued(
        max,
        Arc::new(move |_priority| -> Result<(), String> {
            let _ = lock(&hook_events).send(Event::Queued);
            Ok(())
        }),
    );

    let mut holder: Option<FullDecodeLease> = None;
    let mut release_twice = false;
    if !input["holder"].is_null() {
        let spec = &input["holder"];
        let lease = gate
            .enter(priority_of(golden::str_of(spec, "priority")), &CancelToken::none())
            .expect("the holder takes the free slot");
        if golden::bool_of(spec, "worked") {
            lease.mark_worked();
        }
        release_twice = spec.get("release_twice").and_then(Value::as_bool).unwrap_or(false);
        holder = Some(lease);
    }

    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut tokens: BTreeMap<String, CancelToken> = BTreeMap::new();
    let mut waiters: Vec<(String, JoinHandle<Result<(), GateError>>)> = Vec::new();
    let mut snapshots = Vec::new();
    for step in input["steps"].as_array().expect("steps is an array") {
        match golden::str_of(step, "op") {
            op @ ("enter" | "enter_canceled") => {
                let waiter = golden::str_of(step, "name").to_owned();
                let priority = priority_of(golden::str_of(step, "priority"));
                let worked = step.get("worked").and_then(Value::as_bool).unwrap_or(false);
                let token = CancelToken::new();
                if op == "enter_canceled" {
                    token.cancel();
                }
                tokens.insert(waiter.clone(), token.clone());
                let handle = spawn_waiter(
                    gate.clone(),
                    waiter.clone(),
                    priority,
                    worked,
                    token,
                    events.clone(),
                    Arc::clone(&order),
                );
                waiters.push((waiter.clone(), handle));
                // The step settles when the hook reports the queued waiter, or the waiter itself has ended.
                match next_event(&queue_events) {
                    Event::Queued => {}
                    Event::Finished(done) => assert_eq!(done, waiter, "case {name}: step {op} ended the wrong waiter"),
                }
            }
            "cancel" => {
                let target = golden::str_of(step, "name");
                tokens
                    .get(target)
                    .unwrap_or_else(|| panic!("case {name}: cancel of an unknown waiter {target}"))
                    .cancel();
                match next_event(&queue_events) {
                    Event::Finished(done) => assert_eq!(done, target, "case {name}: cancel ended the wrong waiter"),
                    Event::Queued => panic!("case {name}: a waiter queued during a cancel"),
                }
            }
            "counters" => snapshots.push(snapshot(&gate)),
            other => panic!("case {name}: unknown op {other}"),
        }
    }

    let before_release = snapshot(&gate);
    if let Some(mut lease) = holder.take() {
        lease.release();
        if release_twice {
            lease.release();
        }
    }

    let mut outcomes = serde_json::Map::new();
    let mut busy_messages = Vec::new();
    for (waiter, handle) in waiters {
        let result = join(handle);
        let outcome = match result {
            Ok(()) => "granted".to_owned(),
            Err(GateError::Canceled) => "canceled".to_owned(),
            Err(GateError::Busy { message }) => {
                busy_messages.push(message);
                "busy".to_owned()
            }
            Err(other) => format!("error: {other}"),
        };
        outcomes.insert(waiter, Value::String(outcome));
    }

    let order = lock(&order).clone();
    json!({
        "name": name,
        "outcomes": Value::Object(outcomes),
        "busy_messages": busy_messages,
        "order": order,
        "snapshots": snapshots,
        "before_release": before_release,
        "final": snapshot(&gate),
    })
}

#[test]
fn golden_full_decode_gate() {
    // C#: tests/golden/full-decode-gate.json (oracle-cs full-decode-gate, C# baseline d92d6aae), 30 cases; inputs from
    // tests/golden-inputs/full-decode-gate.json, same order.
    let expected = golden::load(GOLDEN);
    assert_eq!(30, expected.cases.len());

    let input_path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("full-decode-gate.json");
    let text = std::fs::read_to_string(&input_path).unwrap_or_else(|e| panic!("read {}: {e}", input_path.display()));
    let document: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", input_path.display()));
    let inputs = document["cases"].as_array().expect("golden-inputs has a cases array");
    assert_eq!(expected.cases.len(), inputs.len());

    for (input, want) in inputs.iter().zip(&expected.cases) {
        assert_eq!(run_case(input), *want, "golden case {}", golden::str_of(input, "name"));
    }
}

// Repair round 1: kill M21 (grant taken before cancel) and M15 (aging looks at the back preload).
/// Kills M21 (grant checked before cancel): C# `SemaphoreSlim.Wait(token)` throws on an already-cancelled token even when
/// the grant is already there, and the catch passes the slot on. Hook drops the holder (grant to this waiter) and cancels
/// this waiter's token; `enter` must return `Canceled` and the slot must end free.
#[test]
fn granted_and_cancelled_before_wait_returns_canceled_and_frees_slot() {
    let holder: Arc<Mutex<Option<FullDecodeLease>>> = Arc::new(Mutex::new(None));
    let token = CancelToken::new();
    let (h2, t2) = (Arc::clone(&holder), token.clone());
    let gate = FullDecodeGate::with_waiter_queued(
        2,
        Arc::new(move |_p| {
            drop(h2.lock().unwrap().take());
            t2.cancel();
            Ok(())
        }),
    );
    *holder.lock().unwrap() = Some(gate.enter(Viewer, &CancelToken::none()).unwrap());
    let result = gate.enter(Viewer, &token);
    assert_eq!(result.err(), Some(GateError::Canceled));
    assert_eq!(gate.slots_available(), 1);
}

/// Kills M15 (aging looks at the back preload): only the FRONT preload is considered (C# `_preloadQueue.First`).
#[test]
fn only_the_front_preload_is_aged() {
    let (queued_tx, queued_rx) = mpsc::channel::<()>();
    let queued_tx = Mutex::new(queued_tx);
    let gate = FullDecodeGate::with_waiter_queued(
        3,
        Arc::new(move |_p| {
            queued_tx.lock().unwrap().send(()).unwrap();
            Ok(())
        }),
    );
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let (granted_tx, granted_rx) = mpsc::channel::<String>();

    let spawn = |name: &str, pri: SourceReadPriority, hold: Option<mpsc::Receiver<()>>| {
        let (g, o, gt, n) = (gate.clone(), Arc::clone(&order), granted_tx.clone(), name.to_owned());
        let h = thread::spawn(move || {
            let lease = g.enter(pri, &CancelToken::none()).unwrap();
            lease.mark_worked();
            o.lock().unwrap().push(n.clone());
            gt.send(n).unwrap();
            if let Some(rx) = hold {
                rx.recv_timeout(BOUND).unwrap();
            }
            drop(lease);
        });
        queued_rx.recv_timeout(BOUND).expect("queued");
        h
    };

    let holder = gate.enter(Viewer, &CancelToken::none()).unwrap();
    let mut hs = vec![spawn("p1", Preload, None)]; // stamp 0
    let (v1_tx, v1_rx) = mpsc::channel();
    hs.push(spawn("v1", Viewer, Some(v1_rx)));
    holder.mark_worked();
    drop(holder); // completions 1, p1 not overdue -> v1
    assert_eq!(granted_rx.recv_timeout(BOUND).unwrap(), "v1");
    hs.push(spawn("p2", Preload, None)); // stamp 1
    hs.push(spawn("v2", Viewer, None));
    hs.push(spawn("v3", Viewer, None));
    v1_tx.send(()).unwrap();
    for h in hs {
        h.join().unwrap();
    }
    // c=2: p1 2-0 -> v2; c=3: p1 overdue -> p1; c=4: p2 max(1,3)=3 -> 1 -> v3; then p2.
    assert_eq!(*order.lock().unwrap(), ["v1", "v2", "p1", "v3", "p2"]);
    assert_eq!(gate.slots_available(), 1);
}
