//! Q10 mutant killers for `pr_libraw::full_decode_gate` (`FullDecodeGate`, `FullDecodeLease`).
//!
//! The 2026-11-04 run of `tools/dev/mutants.sh` over `full_decode_gate.rs` (54 mutants) left 2 MISSED (the two `Debug` impls) and
//! 11 TIMEOUT mutants (a mutant that makes `enter` wait for a grant that never comes hangs the 30 s-bound tests of
//! `full_decode_gate.rs`; a timeout is not a catch). Every test here fails fast instead of hanging: a blocking call runs on a
//! thread and is awaited for at most `BOUND`; a thread left blocked by a mutant is released through its cancel token.
//! No libraw.dll, no corpus, no sleeps (the one absence check uses `recv_timeout`, see `waiter_is_not_granted_while_the_slot_is_held`).
//!
//! No C# twin: these pin the Rust gate against mutation; the C# behaviour they check is `LibRawFullDecodeGateTests`.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use pr_core::abstractions::SourceReadPriority::{self, Preload, Viewer};
use pr_core::sync::CancelToken;
use pr_libraw::full_decode_gate::{FullDecodeGate, FullDecodeLease, GateError};

/// Upper bound for a call that must return at once (a mutant that blocks it fails the test after this).
const BOUND: Duration = Duration::from_secs(5);

/// Runs `enter` on a thread and returns its outcome, or `None` when it did not return within `BOUND`
/// (the thread is then released through `cancel`).
fn bounded_enter(
    gate: &FullDecodeGate,
    priority: SourceReadPriority,
    cancel: &CancelToken,
) -> Option<Result<FullDecodeLease, GateError>> {
    let (tx, rx) = mpsc::channel();
    let (gate, token) = (gate.clone(), cancel.clone());
    thread::spawn(move || {
        let _ = tx.send(gate.enter(priority, &token));
    });
    match rx.recv_timeout(BOUND) {
        Ok(result) => Some(result),
        Err(_) => {
            cancel.cancel();
            None
        }
    }
}

/// Takes the free slot of `gate` on a bounded thread: a mutant that makes a free slot wait fails the test instead of hanging it.
fn take_free_slot(gate: &FullDecodeGate) -> FullDecodeLease {
    bounded_enter(gate, Viewer, &CancelToken::new())
        .expect("a free slot must be granted at once")
        .expect("the free slot is granted")
}

/// A gate whose queued-hook forwards the priority of every queued waiter to the returned receiver.
fn gate_with_queue_events(max_queued_preloads: i32) -> (FullDecodeGate, Receiver<SourceReadPriority>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let gate = FullDecodeGate::with_waiter_queued(
        max_queued_preloads,
        Arc::new(move |priority| {
            let _ = tx.lock().unwrap_or_else(PoisonError::into_inner).send(priority);
            Ok(())
        }),
    );
    (gate, rx)
}

/// Starts a waiter thread that enters with `priority` and reports its outcome (granted lease or error) on the channel.
fn spawn_waiter(
    gate: &FullDecodeGate,
    priority: SourceReadPriority,
    cancel: &CancelToken,
) -> Receiver<Result<FullDecodeLease, GateError>> {
    let (tx, rx) = mpsc::channel();
    let (gate, token) = (gate.clone(), cancel.clone());
    thread::spawn(move || {
        let _ = tx.send(gate.enter(priority, &token));
    });
    rx
}

#[test]
fn debug_of_the_gate_shows_its_state() {
    // Kills 149:9 (`Debug for FullDecodeGate::fmt -> Ok(Default::default())`, which prints nothing).
    let gate = FullDecodeGate::new(3);
    let text = format!("{gate:?}");
    assert!(text.contains("FullDecodeGate"), "{text}");
    assert!(text.contains("max_queued_preloads: 3"), "{text}");
    assert!(text.contains("busy: false"), "{text}");
    assert!(text.contains("queued_viewers: 0"), "{text}");
    assert!(text.contains("queued_preloads: 0"), "{text}");
    assert!(text.contains("completions: 0"), "{text}");

    let lease = take_free_slot(&gate);
    assert!(format!("{gate:?}").contains("busy: true"));
    drop(lease);
}

#[test]
fn debug_of_the_lease_shows_released_and_worked() {
    // Kills 384:9 (`Debug for FullDecodeLease::fmt -> Ok(Default::default())`).
    let gate = FullDecodeGate::new(1);
    let mut lease = take_free_slot(&gate);
    let text = format!("{lease:?}");
    assert!(text.contains("FullDecodeLease"), "{text}");
    assert!(text.contains("released: false"), "{text}");
    assert!(text.contains("worked: false"), "{text}");

    lease.mark_worked();
    assert!(format!("{lease:?}").contains("worked: true"));
    lease.release();
    assert!(format!("{lease:?}").contains("released: true"));
}

#[test]
fn free_slot_is_taken_at_once_without_queueing() {
    // Kills 209:16 (`delete !` in `if !state.busy`): a free slot would queue and wait for a grant that never comes.
    let (gate, queued) = gate_with_queue_events(1);
    let cancel = CancelToken::new();
    let lease = bounded_enter(&gate, Viewer, &cancel)
        .expect("a free slot must be granted at once")
        .expect("the free slot is granted");
    assert_eq!(gate.slots_available(), 0);
    assert_eq!(gate.queued_viewers(), 0);
    assert!(queued.try_recv().is_err(), "a free slot never reaches the queued hook");
    drop(lease);
    assert_eq!(gate.slots_available(), 1);
}

#[test]
fn releasing_a_lease_frees_the_slot_when_nobody_waits() {
    // Kills 268:9 (`GateInner::release -> ()`), 340:9 (`release_locked -> true` / `-> false`: `busy` is never cleared)
    // and 411:9 (`FullDecodeLease::release -> ()`).
    let gate = FullDecodeGate::new(1);
    let mut lease = take_free_slot(&gate);
    assert_eq!(gate.slots_available(), 0);
    lease.release();
    assert!(!lease.is_held());
    assert_eq!(gate.slots_available(), 1, "release frees the slot");

    // the freed slot can be taken again at once
    let again = bounded_enter(&gate, Viewer, &CancelToken::new())
        .expect("the freed slot is granted at once")
        .expect("the freed slot is granted");
    drop(again);
}

#[test]
fn dropping_a_lease_frees_the_slot() {
    // Kills 419:9 (`Drop for FullDecodeLease::drop -> ()`).
    let gate = FullDecodeGate::new(1);
    {
        let _lease = take_free_slot(&gate);
        assert_eq!(gate.slots_available(), 0);
    }
    assert_eq!(gate.slots_available(), 1);
}

#[test]
fn release_hands_the_slot_to_the_queued_waiter_and_keeps_it_busy() {
    // Kills 340:9 (`release_locked -> true` / `-> false`), 268:9 and 411:9 on the grant path: the waiter is never granted.
    let (gate, queued) = gate_with_queue_events(1);
    let mut first = take_free_slot(&gate);
    let cancel = CancelToken::new();
    let outcome = spawn_waiter(&gate, Viewer, &cancel);
    assert_eq!(queued.recv_timeout(BOUND), Ok(Viewer), "the waiter queued");

    first.release();
    let granted = match outcome.recv_timeout(BOUND) {
        Ok(result) => result.expect("the queued waiter is granted"),
        Err(_) => {
            cancel.cancel();
            panic!("the queued waiter was not granted after the release");
        }
    };
    assert_eq!(gate.slots_available(), 0, "the slot went straight to the waiter");
    assert_eq!(gate.queued_viewers(), 0);
    drop(granted);
    assert_eq!(gate.slots_available(), 1);
}

#[test]
fn waiter_is_not_granted_while_the_slot_is_held() {
    // Kills 295:36 (`pending_grant == Some(id)` -> `!=`): the waiter would return at once without owning the slot.
    // `recv_timeout` is a bounded absence check: it never fails for the real gate (nothing can grant before the release).
    let (gate, queued) = gate_with_queue_events(1);
    let first = take_free_slot(&gate);
    let cancel = CancelToken::new();
    let outcome = spawn_waiter(&gate, Viewer, &cancel);
    assert_eq!(queued.recv_timeout(BOUND), Ok(Viewer), "the waiter queued");
    assert!(
        matches!(
            outcome.recv_timeout(Duration::from_millis(300)),
            Err(RecvTimeoutError::Timeout)
        ),
        "the waiter must keep waiting while the first lease holds the slot"
    );
    assert_eq!(gate.queued_viewers(), 1);
    assert_eq!(gate.slots_available(), 0);

    cancel.cancel();
    let result = outcome.recv_timeout(BOUND).expect("the cancelled waiter returns");
    assert!(
        matches!(result, Err(GateError::Canceled)),
        "cancelled waiter: {result:?}"
    );
    drop(first);
    assert_eq!(gate.slots_available(), 1);
}

#[test]
fn preload_over_the_bound_is_refused_and_a_viewer_is_not() {
    // Kills 214:36 (`priority == Preload` -> `!=`): with a bound of 0 a queued Preload is refused with Busy and a queued
    // Viewer is not (the bound is not consulted for viewers).
    let (gate, queued) = gate_with_queue_events(0);
    let first = take_free_slot(&gate);

    let refused = bounded_enter(&gate, Preload, &CancelToken::new()).expect("a refused preload returns at once");
    assert!(matches!(refused, Err(GateError::Busy { .. })), "preload: {refused:?}");
    assert_eq!(gate.queued_preloads(), 0);
    assert!(queued.try_recv().is_err(), "a refused preload never reaches the hook");

    let cancel = CancelToken::new();
    let viewer = spawn_waiter(&gate, Viewer, &cancel);
    assert_eq!(
        queued.recv_timeout(BOUND),
        Ok(Viewer),
        "the viewer queues although the bound is 0"
    );
    assert_eq!(gate.queued_viewers(), 1);

    cancel.cancel();
    assert!(matches!(
        viewer.recv_timeout(BOUND).expect("the cancelled viewer returns"),
        Err(GateError::Canceled)
    ));
    drop(first);
}

#[test]
fn preload_queues_while_below_the_bound_and_is_refused_at_the_bound() {
    // Kills 217:21 (`>=` -> `<`) and the lane choice of 365:9 / 365:21 (`lane_mut`): each lane counts its own waiters.
    let (gate, queued) = gate_with_queue_events(1);
    let first = take_free_slot(&gate);

    let cancel_preload = CancelToken::new();
    let preload = spawn_waiter(&gate, Preload, &cancel_preload);
    assert_eq!(
        queued.recv_timeout(BOUND),
        Ok(Preload),
        "the first preload queues below the bound"
    );
    assert_eq!((gate.queued_viewers(), gate.queued_preloads()), (0, 1));

    let second = bounded_enter(&gate, Preload, &CancelToken::new()).expect("a refused preload returns at once");
    assert!(
        matches!(second, Err(GateError::Busy { .. })),
        "second preload at the bound: {second:?}"
    );
    assert_eq!(gate.queued_preloads(), 1);

    let cancel_viewer = CancelToken::new();
    let viewer = spawn_waiter(&gate, Viewer, &cancel_viewer);
    assert_eq!(
        queued.recv_timeout(BOUND),
        Ok(Viewer),
        "the viewer queues in its own lane"
    );
    assert_eq!((gate.queued_viewers(), gate.queued_preloads()), (1, 1));

    cancel_preload.cancel();
    cancel_viewer.cancel();
    assert!(matches!(
        preload.recv_timeout(BOUND).expect("the cancelled preload returns"),
        Err(GateError::Canceled)
    ));
    assert!(matches!(
        viewer.recv_timeout(BOUND).expect("the cancelled viewer returns"),
        Err(GateError::Canceled)
    ));
    assert_eq!((gate.queued_viewers(), gate.queued_preloads()), (0, 0));
    drop(first);
    assert_eq!(gate.slots_available(), 1);
}
