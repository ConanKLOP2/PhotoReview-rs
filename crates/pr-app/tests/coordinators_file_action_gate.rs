use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use pr_app::coordinators::file_action_gate::{FileActionGate, GateTurn, QueuedOutcome};
use pr_core::abstractions::{UiAction, UiActionPanicked, UiCompletion, UiScheduler};

#[derive(Default)]
struct ManualScheduler(Mutex<VecDeque<UiAction>>);

impl UiScheduler for ManualScheduler {
    fn post(&self, action: UiAction) {
        self.0.lock().unwrap().push_back(action);
    }

    fn invoke(&self, action: UiAction) -> UiCompletion {
        let (sender, completion) = UiCompletion::channel();
        let result = catch_unwind(AssertUnwindSafe(action)).map_err(|_| UiActionPanicked {
            message: "manual scheduler action panicked".to_owned(),
        });
        sender.send(result).unwrap();
        completion
    }
}

impl ManualScheduler {
    fn pump_one(&self) -> bool {
        let action = self.0.lock().unwrap().pop_front();
        if let Some(action) = action {
            action();
            true
        } else {
            false
        }
    }

    fn pump(&self) {
        while self.pump_one() {}
    }

    fn pending(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

fn gate() -> (FileActionGate, Arc<ManualScheduler>) {
    let scheduler = Arc::new(ManualScheduler::default());
    (FileActionGate::new(scheduler.clone()), scheduler)
}

type TurnSlot = Arc<Mutex<Option<GateTurn>>>;

fn slot() -> TurnSlot {
    Arc::new(Mutex::new(None))
}

fn finish(slot: &TurnSlot) {
    slot.lock().unwrap().take().unwrap().finish();
}

// C#: FileActionGateTests.TryEnter_IsExclusive_UntilExit
#[test]
fn try_enter_is_exclusive_until_exit() {
    let (gate, _) = gate();
    assert!(gate.try_enter());
    assert!(gate.is_held());
    assert!(!gate.try_enter());
    gate.exit();
    assert!(!gate.is_held());
    assert!(gate.try_enter());
    gate.exit();
}

// C#: FileActionGateTests.RunExclusiveAsync_SecondCallWhileHeld_IsNoOp
#[test]
fn run_exclusive_second_call_while_held_is_no_op() {
    let (gate, _) = gate();
    let runs = Arc::new(AtomicUsize::new(0));
    let held = slot();
    let held_work = held.clone();
    let first_runs = runs.clone();
    assert!(gate.run_exclusive(move |turn| {
        first_runs.fetch_add(1, Ordering::SeqCst);
        *held_work.lock().unwrap() = Some(turn);
    }));
    let second_runs = runs.clone();
    assert!(!gate.run_exclusive(move |_| {
        second_runs.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    finish(&held);
}

// C#: FileActionGateTests.RunQueuedAsync_SecondCallWhileRunning_IsQueuedNotDropped
#[test]
fn run_queued_second_call_while_running_is_queued_not_dropped() {
    let (gate, scheduler) = gate();
    let order = Arc::new(Mutex::new(Vec::new()));
    let held = slot();
    let (first_order, first_held) = (order.clone(), held.clone());
    assert_eq!(
        gate.run_queued(move |turn| {
            first_order.lock().unwrap().push(1);
            *first_held.lock().unwrap() = Some(turn);
        }),
        QueuedOutcome::StartedNow
    );
    let second_order = order.clone();
    assert_eq!(
        gate.run_queued(move |_| second_order.lock().unwrap().push(2)),
        QueuedOutcome::Queued
    );
    assert_eq!(*order.lock().unwrap(), [1]);
    finish(&held);
    assert_eq!(*order.lock().unwrap(), [1]);
    scheduler.pump();
    assert_eq!(*order.lock().unwrap(), [1, 2]);
    assert!(!gate.is_held());
}

// C#: FileActionGateTests.RunQueuedAsync_WhileExclusivelyHeld_IsNoOp
#[test]
fn run_queued_while_exclusively_held_is_rejected() {
    let (gate, scheduler) = gate();
    assert!(gate.try_enter());
    let runs = Arc::new(AtomicUsize::new(0));
    let work_runs = runs.clone();
    assert_eq!(
        gate.run_queued(move |_| {
            work_runs.fetch_add(1, Ordering::SeqCst);
        }),
        QueuedOutcome::Rejected
    );
    gate.exit();
    scheduler.pump();
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

// C#: FileActionGateTests.RunExclusiveAsync_WhileQueuedActionPending_IsNoOp
#[test]
fn run_exclusive_while_queued_action_pending_is_no_op() {
    let (gate, scheduler) = gate();
    let held = slot();
    let first_held = held.clone();
    gate.run_queued(move |turn| *first_held.lock().unwrap() = Some(turn));
    assert_eq!(gate.run_queued(|_| {}), QueuedOutcome::Queued);
    finish(&held);
    assert!(gate.is_held());
    assert!(!gate.run_exclusive(|_| panic!("exclusive work must not run")));
    scheduler.pump();
}

// C#: FileActionGateTests.WhenReleasedAsync_CompletesOnExit
#[test]
fn when_released_completes_on_exit() {
    let (gate, scheduler) = gate();
    let calls = Arc::new(AtomicUsize::new(0));
    let free_calls = calls.clone();
    assert!(gate.when_released(Box::new(move || {
        free_calls.fetch_add(1, Ordering::SeqCst);
    })));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(gate.try_enter());
    let held_calls = calls.clone();
    assert!(!gate.when_released(Box::new(move || {
        held_calls.fetch_add(1, Ordering::SeqCst);
    })));
    gate.exit();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    scheduler.pump();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

// C#: FileActionGateTests.RunExclusiveAsync_ReleasesGate_WhenWorkThrows
#[test]
fn run_exclusive_releases_gate_when_work_panics() {
    let (gate, _) = gate();
    assert!(catch_unwind(AssertUnwindSafe(|| gate.run_exclusive(|_| panic!("work failed")))).is_err());
    assert!(!gate.is_held());
    assert!(gate.run_exclusive(|_| {}));
}

// C#: FileActionGateTests.RunExclusiveAsync_ManyConcurrentCallers_OnlyOneRuns
#[test]
fn run_exclusive_many_concurrent_callers_only_one_runs() {
    let (gate, _) = gate();
    let start = Arc::new(Barrier::new(33));
    let attempted = Arc::new(Barrier::new(33));
    let runs = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();
    for _ in 0..32 {
        let (gate, start, attempted, runs) = (gate.clone(), start.clone(), attempted.clone(), runs.clone());
        threads.push(thread::spawn(move || {
            start.wait();
            let held = slot();
            let work_held = held.clone();
            let won = gate.run_exclusive(move |turn| {
                runs.fetch_add(1, Ordering::SeqCst);
                *work_held.lock().unwrap() = Some(turn);
            });
            attempted.wait();
            if won {
                finish(&held);
            }
            won
        }));
    }
    start.wait();
    attempted.wait();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|won| *won)
            .count(),
        1
    );
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

// C#: FileActionGateTests.RunQueuedAsync_WorkReentersSynchronously_SecondActionWaitsForFirst
#[test]
fn run_queued_work_reenters_synchronously_second_action_waits_for_first() {
    let (gate, scheduler) = gate();
    let order = Arc::new(Mutex::new(Vec::new()));
    let nested_gate = gate.clone();
    let outer_order = order.clone();
    gate.run_queued(move |turn| {
        outer_order.lock().unwrap().push("A-start");
        let inner_order = outer_order.clone();
        assert_eq!(
            nested_gate.run_queued(move |_| inner_order.lock().unwrap().push("B-start")),
            QueuedOutcome::Queued
        );
        outer_order.lock().unwrap().push("A-end");
        drop(turn);
    });
    assert_eq!(*order.lock().unwrap(), ["A-start", "A-end"]);
    scheduler.pump();
    assert_eq!(*order.lock().unwrap(), ["A-start", "A-end", "B-start"]);
}

// C#: FileActionGateTests.RunQueuedAsync_NestedReentrantSubmissions_RunInSubmissionOrder
#[test]
fn run_queued_nested_reentrant_submissions_run_in_submission_order() {
    let (gate, scheduler) = gate();
    let order = Arc::new(Mutex::new(Vec::new()));
    let running = Arc::new(AtomicUsize::new(0));
    let max_running = Arc::new(AtomicUsize::new(0));
    let held = slot();
    let (nested_gate, first_order, first_running, first_max, first_held) = (
        gate.clone(),
        order.clone(),
        running.clone(),
        max_running.clone(),
        held.clone(),
    );
    gate.run_queued(move |turn| {
        let current = first_running.fetch_add(1, Ordering::SeqCst) + 1;
        first_max.fetch_max(current, Ordering::SeqCst);
        first_order.lock().unwrap().push('A');
        for letter in ['B', 'C'] {
            let (order, running, max) = (first_order.clone(), first_running.clone(), first_max.clone());
            assert_eq!(
                nested_gate.run_queued(move |_| {
                    let current = running.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(current, Ordering::SeqCst);
                    order.lock().unwrap().push(letter);
                    running.fetch_sub(1, Ordering::SeqCst);
                }),
                QueuedOutcome::Queued
            );
        }
        *first_held.lock().unwrap() = Some(turn);
    });
    let (last_order, last_running, last_max) = (order.clone(), running.clone(), max_running.clone());
    assert_eq!(
        gate.run_queued(move |_| {
            let current = last_running.fetch_add(1, Ordering::SeqCst) + 1;
            last_max.fetch_max(current, Ordering::SeqCst);
            last_order.lock().unwrap().push('D');
            last_running.fetch_sub(1, Ordering::SeqCst);
        }),
        QueuedOutcome::Queued
    );
    running.fetch_sub(1, Ordering::SeqCst);
    finish(&held);
    scheduler.pump();
    assert_eq!(*order.lock().unwrap(), ['A', 'B', 'C', 'D']);
    assert_eq!(max_running.load(Ordering::SeqCst), 1);
}

// C#: FileActionGateTests.IsHeld_FromOtherThreadWhileQueuedWorkInSyncPrefix_DoesNotBlock
#[test]
fn is_held_from_other_thread_while_queued_work_in_sync_prefix_does_not_block() {
    let (gate, scheduler) = gate();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_gate = gate.clone();
    let worker = thread::spawn(move || {
        worker_gate.run_queued(move |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (probe_tx, probe_rx) = mpsc::channel();
    let probe_gate = gate.clone();
    let probe = thread::spawn(move || {
        let held = probe_gate.is_held();
        let entered = probe_gate.try_enter();
        let waiter_inline = probe_gate.when_released(Box::new(|| {}));
        probe_tx.send((held, entered, waiter_inline)).unwrap();
    });
    let observed = probe_rx.recv_timeout(Duration::from_secs(5));
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    probe.join().unwrap();
    assert_eq!(observed.unwrap(), (true, false, false));
    scheduler.pump();
}

// C#: FileActionGateTests.RunQueuedAsync_PreviousActionCompletes_NextActionDoesNotRunInlineOnItsStack
#[test]
fn run_queued_previous_action_completes_next_action_does_not_run_inline_on_its_stack() {
    let (gate, scheduler) = gate();
    let held = slot();
    let first_held = held.clone();
    gate.run_queued(move |turn| *first_held.lock().unwrap() = Some(turn));
    let order = Arc::new(Mutex::new(Vec::new()));
    let next_order = order.clone();
    gate.run_queued(move |_| next_order.lock().unwrap().push("next"));
    finish(&held);
    assert!(order.lock().unwrap().is_empty());
    assert_eq!(scheduler.pending(), 1);
    scheduler.pump();
    assert_eq!(*order.lock().unwrap(), ["next"]);
}

// C#: FileActionGateTests.RunQueuedAsync_FirstActionThrows_OnlyItsTaskFaultsAndNextRuns
#[test]
fn run_queued_first_action_fails_only_its_own_result_and_next_runs() {
    let (gate, scheduler) = gate();
    let held = slot();
    let first_held = held.clone();
    let results = Arc::new(Mutex::new(Vec::<Result<(), &'static str>>::new()));
    gate.run_queued(move |turn| *first_held.lock().unwrap() = Some(turn));
    let second_results = results.clone();
    gate.run_queued(move |_| second_results.lock().unwrap().push(Ok(())));
    results.lock().unwrap().push(Err("first failed"));
    finish(&held);
    scheduler.pump();
    assert_eq!(*results.lock().unwrap(), [Err("first failed"), Ok(())]);
    assert!(!gate.is_held());
}

// C#: FileActionGateTests.RunQueuedAsync_WorkThrowsSynchronously_TaskFaultsAndQueueContinues
#[test]
fn run_queued_work_panics_synchronously_gate_released_and_queue_continues() {
    let (gate, scheduler) = gate();
    assert!(catch_unwind(AssertUnwindSafe(|| gate.run_queued(|_| panic!("first")))).is_err());
    assert!(!gate.is_held());
    assert_eq!(gate.run_queued(|_| {}), QueuedOutcome::StartedNow);
    let held = slot();
    let first_held = held.clone();
    gate.run_queued(move |turn| *first_held.lock().unwrap() = Some(turn));
    gate.run_queued(|_| panic!("posted successor"));
    let ran = Arc::new(AtomicUsize::new(0));
    let last_ran = ran.clone();
    gate.run_queued(move |_| {
        last_ran.fetch_add(1, Ordering::SeqCst);
    });
    finish(&held);
    assert!(catch_unwind(AssertUnwindSafe(|| scheduler.pump_one())).is_err());
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    scheduler.pump();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert!(!gate.is_held());
}

// C#: FileActionGateTests.RunQueuedAsync_WorkCancelled_TaskIsCanceledAndGateReleased
#[test]
fn run_queued_work_cancelled_gate_released() {
    let (gate, _) = gate();
    assert_eq!(gate.run_queued(|_| {}), QueuedOutcome::StartedNow);
    assert!(!gate.is_held());
    assert!(gate.run_exclusive(|_| {}));
}

// C#: FileActionGateTests.WhenReleasedAsync_WithQueuedActions_CompletesWhenQueueDrains
#[test]
fn when_released_with_queued_actions_waits_for_the_queue_to_drain() {
    let (gate, scheduler) = gate();
    let held = slot();
    let next_held = slot();
    let first_held = held.clone();
    let second_held = next_held.clone();
    gate.run_queued(move |turn| *first_held.lock().unwrap() = Some(turn));
    gate.run_queued(move |turn| *second_held.lock().unwrap() = Some(turn));
    let called = Arc::new(AtomicUsize::new(0));
    let waiter_called = called.clone();
    assert!(!gate.when_released(Box::new(move || {
        waiter_called.fetch_add(1, Ordering::SeqCst);
    })));
    finish(&held);
    assert_eq!(called.load(Ordering::SeqCst), 0);
    assert!(scheduler.pump_one());
    assert_eq!(called.load(Ordering::SeqCst), 0);
    finish(&next_held);
    assert_eq!(called.load(Ordering::SeqCst), 0);
    scheduler.pump();
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

// C#: FileActionGateTests.RunQueuedAsync_IdleGate_StartsWorkSynchronously
#[test]
fn run_queued_idle_gate_starts_work_synchronously() {
    let (gate, scheduler) = gate();
    let inside_gate = gate.clone();
    let observed = Arc::new(AtomicUsize::new(0));
    let work_observed = observed.clone();
    assert_eq!(
        gate.run_queued(move |_| {
            assert!(inside_gate.is_held());
            work_observed.fetch_add(1, Ordering::SeqCst);
        }),
        QueuedOutcome::StartedNow
    );
    assert_eq!(observed.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.pending(), 0);
    assert!(!gate.is_held());
}

// C#: FileActionGateStressTests.TryEnter_SimultaneousCallers_ExactlyOneWins
#[test]
fn try_enter_simultaneous_callers_exactly_one_wins() {
    for _ in 0..200 {
        let (gate, _) = gate();
        let start = Arc::new(Barrier::new(9));
        let attempted = Arc::new(Barrier::new(9));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let (gate, start, attempted) = (gate.clone(), start.clone(), attempted.clone());
            threads.push(thread::spawn(move || {
                start.wait();
                let won = gate.try_enter();
                attempted.wait();
                won
            }));
        }
        start.wait();
        attempted.wait();
        assert_eq!(
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
        gate.exit();
    }
}

// C#: FileActionGateStressTests.WhenReleasedAsync_RacingWithExit_NeverLosesTheWakeup
#[test]
fn when_released_racing_with_exit_never_loses_the_wakeup() {
    for _ in 0..100 {
        let (gate, scheduler) = gate();
        assert!(gate.try_enter());
        let start = Arc::new(Barrier::new(9));
        let called = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let (gate, start, called) = (gate.clone(), start.clone(), called.clone());
            threads.push(thread::spawn(move || {
                start.wait();
                gate.when_released(Box::new(move || {
                    called.fetch_add(1, Ordering::SeqCst);
                }));
            }));
        }
        start.wait();
        gate.exit();
        for thread in threads {
            thread.join().unwrap();
        }
        scheduler.pump();
        assert_eq!(called.load(Ordering::SeqCst), 8);
    }
}

// C#: FileActionGateStressTests.Exit_WithoutHolder_IsHarmless
#[test]
fn exit_without_holder_is_harmless() {
    let (gate, _) = gate();
    gate.exit();
    gate.exit();
    assert!(!gate.is_held());
    assert!(gate.try_enter());
    gate.exit();
}

// C#: FileActionGateStressTests.RunExclusiveAsync_CancelledOrFaultedWork_ReleasesAndWakesWaiters
#[test]
fn run_exclusive_cancelled_or_faulted_work_releases_and_wakes_waiters() {
    for fail in [false, true] {
        let (gate, scheduler) = gate();
        let called = Arc::new(AtomicUsize::new(0));
        let work_gate = gate.clone();
        let work_called = called.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            gate.run_exclusive(move |_| {
                let waiter_called = work_called.clone();
                assert!(!work_gate.when_released(Box::new(move || {
                    waiter_called.fetch_add(1, Ordering::SeqCst);
                })));
                if fail {
                    panic!("fault");
                }
            })
        }));
        assert_eq!(result.is_err(), fail);
        assert!(!gate.is_held());
        assert_eq!(called.load(Ordering::SeqCst), 0);
        scheduler.pump();
        assert_eq!(called.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn queued_turn_dropped_without_finish_releases_the_gate() {
    let (gate, _) = gate();
    let held = slot();
    let work_held = held.clone();
    gate.run_queued(move |turn| *work_held.lock().unwrap() = Some(turn));
    assert!(gate.is_held());
    drop(held.lock().unwrap().take());
    assert!(!gate.is_held());
}

#[test]
fn try_enter_fails_while_a_posted_successor_has_not_started() {
    let (gate, scheduler) = gate();
    let held = slot();
    let work_held = held.clone();
    gate.run_queued(move |turn| *work_held.lock().unwrap() = Some(turn));
    gate.run_queued(|_| {});
    finish(&held);
    assert_eq!(scheduler.pending(), 1);
    assert!(!gate.try_enter());
    scheduler.pump();
    assert!(gate.try_enter());
    gate.exit();
}

#[test]
fn run_queued_rejects_only_on_exclusive_hold() {
    let (gate, scheduler) = gate();
    assert!(gate.try_enter());
    assert_eq!(gate.run_queued(|_| {}), QueuedOutcome::Rejected);
    gate.exit();
    let held = slot();
    let work_held = held.clone();
    assert_eq!(
        gate.run_queued(move |turn| *work_held.lock().unwrap() = Some(turn)),
        QueuedOutcome::StartedNow
    );
    assert_eq!(gate.run_queued(|_| {}), QueuedOutcome::Queued);
    finish(&held);
    scheduler.pump();
}

#[test]
fn run_exclusive_rejects_on_exclusive_or_queued() {
    let (gate, scheduler) = gate();
    assert!(gate.try_enter());
    assert!(!gate.run_exclusive(|_| panic!("must not run")));
    gate.exit();
    let held = slot();
    let work_held = held.clone();
    gate.run_queued(move |turn| *work_held.lock().unwrap() = Some(turn));
    assert!(!gate.run_exclusive(|_| panic!("must not run")));
    gate.run_queued(|_| {});
    finish(&held);
    assert!(!gate.run_exclusive(|_| panic!("must not run")));
    scheduler.pump();
    assert!(gate.run_exclusive(|_| {}));
}

#[test]
fn when_released_waiters_are_posted_not_run_inline_on_release() {
    let (gate, scheduler) = gate();
    assert!(gate.try_enter());
    let called = Arc::new(AtomicUsize::new(0));
    for _ in 0..3 {
        let called = called.clone();
        assert!(!gate.when_released(Box::new(move || {
            called.fetch_add(1, Ordering::SeqCst);
        })));
    }
    gate.exit();
    assert_eq!(called.load(Ordering::SeqCst), 0);
    assert_eq!(scheduler.pending(), 3);
    scheduler.pump();
    assert_eq!(called.load(Ordering::SeqCst), 3);
}

#[test]
fn turn_finished_from_another_thread_behaves_like_on_the_ui_thread() {
    let (gate, scheduler) = gate();
    let held = slot();
    let work_held = held.clone();
    gate.run_queued(move |turn| *work_held.lock().unwrap() = Some(turn));
    let ran = Arc::new(AtomicUsize::new(0));
    let next_ran = ran.clone();
    gate.run_queued(move |_| {
        next_ran.fetch_add(1, Ordering::SeqCst);
    });
    let turn = held.lock().unwrap().take().unwrap();
    thread::spawn(move || turn.finish()).join().unwrap();
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    assert!(gate.is_held());
    scheduler.pump();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert!(!gate.is_held());
}

#[test]
fn successor_gets_its_own_turn_and_gate_stays_held_between_actions() {
    let (gate, scheduler) = gate();
    let first = slot();
    let second = slot();
    let work_first = first.clone();
    let work_second = second.clone();
    gate.run_queued(move |turn| *work_first.lock().unwrap() = Some(turn));
    gate.run_queued(move |turn| *work_second.lock().unwrap() = Some(turn));
    finish(&first);
    assert!(gate.is_held());
    assert!(second.lock().unwrap().is_none());
    assert!(scheduler.pump_one());
    assert!(gate.is_held());
    assert!(second.lock().unwrap().is_some());
    finish(&second);
    assert!(!gate.is_held());
}

#[test]
fn queued_outcome_accepted_matches_each_variant() {
    assert!(!QueuedOutcome::Rejected.accepted());
    assert!(QueuedOutcome::StartedNow.accepted());
    assert!(QueuedOutcome::Queued.accepted());
}

#[test]
fn finishing_exclusive_turn_releases_and_posts_waiter() {
    let (gate, scheduler) = gate();
    let held = slot();
    let work_held = held.clone();
    assert!(gate.run_exclusive(move |turn| {
        *work_held.lock().unwrap() = Some(turn);
    }));
    let called = Arc::new(AtomicUsize::new(0));
    let waiter_called = called.clone();
    assert!(!gate.when_released(Box::new(move || {
        waiter_called.fetch_add(1, Ordering::SeqCst);
    })));
    assert!(gate.is_held());
    assert!(!gate.try_enter());

    finish(&held);

    assert!(!gate.is_held());
    assert_eq!(called.load(Ordering::SeqCst), 0);
    assert_eq!(scheduler.pending(), 1);
    scheduler.pump();
    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert!(gate.try_enter());
    gate.exit();
}

#[test]
fn dropping_queued_turn_releases_and_starts_successor() {
    let (gate, scheduler) = gate();
    let held = slot();
    let work_held = held.clone();
    assert_eq!(
        gate.run_queued(move |turn| *work_held.lock().unwrap() = Some(turn)),
        QueuedOutcome::StartedNow
    );
    let runs = Arc::new(AtomicUsize::new(0));
    let successor_runs = runs.clone();
    assert_eq!(
        gate.run_queued(move |_| {
            successor_runs.fetch_add(1, Ordering::SeqCst);
        }),
        QueuedOutcome::Queued
    );
    assert!(gate.is_held());
    assert_eq!(runs.load(Ordering::SeqCst), 0);

    drop(held.lock().unwrap().take());

    assert!(gate.is_held());
    assert_eq!(scheduler.pending(), 1);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    scheduler.pump();
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(!gate.is_held());
}
