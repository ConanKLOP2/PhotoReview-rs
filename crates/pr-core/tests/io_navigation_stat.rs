//! Tests for `pr_core::io::navigation_stat` (C# `NavigationStatWorker`), plus the golden replay of
//! `tests/golden/navigation-stat-worker.json`. Every wait is bounded by 20 s.
//!
//! C# `NavigationStatWorkerTests.RunAsync_ContinuationDoesNotRunOnTheWorkerThread` is not ported: Rust has no
//! continuations (`StatTask` is a result slot), so the property is trivially true.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
use std::time::Duration;

use pr_core::io::navigation_stat::{NavigationStatWorker, StatError, StatTask};
use pr_core::sync::CancelToken;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};

const TIMEOUT: Duration = Duration::from_secs(20);

/// Occupies the worker thread until `release` is used; `busy` returns once the worker runs the item.
struct Gate {
    release: mpsc::Sender<()>,
    task: StatTask<i32>,
}

fn busy(worker: &NavigationStatWorker) -> Gate {
    let (release, rx) = mpsc::channel::<()>();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let task = worker.run(
        move || {
            let _ = started_tx.send(());
            let _ = rx.recv_timeout(TIMEOUT);
            1
        },
        &CancelToken::none(),
    );
    assert!(
        started_rx.recv_timeout(TIMEOUT).is_ok(),
        "the worker never started the item"
    );
    Gate { release, task }
}

// C#: NavigationStatWorkerTests.RunAsync_RunsOffTheCallerAndFaultsWithTheWorkException
#[test]
fn run_runs_off_the_caller_and_faults_with_the_work_panic() {
    let worker = NavigationStatWorker::new();
    let caller = thread::current().id();

    let task = worker.run(|| thread::current().id(), &CancelToken::none());
    assert!(task.wait_timeout(TIMEOUT));
    let ran_on = task.wait();
    let error = worker.run(|| -> i32 { panic!("boom") }, &CancelToken::none()).wait();

    assert_ne!(Ok(caller), ran_on);
    assert!(ran_on.is_ok());
    match error {
        Err(StatError::Panicked(message)) => assert!(message.contains("boom"), "{message}"),
        other => panic!("expected Panicked, got {other:?}"),
    }
}

#[test]
fn run_string_payload_panic_keeps_its_message() {
    let worker = NavigationStatWorker::new();
    let result = worker
        .run(
            || -> i32 { std::panic::panic_any(String::from("boom-string")) },
            &CancelToken::none(),
        )
        .wait();
    assert_eq!(result, Err(StatError::Panicked("boom-string".to_owned())));
}

// C#: NavigationStatWorkerTests.RunAsync_CancelledToken_DoesNotRunTheWork
#[test]
fn run_cancelled_token_does_not_run_the_work() {
    let worker = NavigationStatWorker::new();
    let token = CancelToken::new();
    token.cancel();
    let ran = Arc::new(Mutex::new(false));
    let ran_in_work = Arc::clone(&ran);

    let task = worker.run(
        move || {
            *ran_in_work.lock().unwrap() = true;
            true
        },
        &token,
    );

    assert_eq!(task.wait(), Err(StatError::Canceled));
    assert!(!*ran.lock().unwrap());
}

// C#: NavigationStatWorkerTests.RunAsync_CancelledToken_CompletesWithoutWaitingForTheBusyWorker
#[test]
fn run_cancelled_token_completes_without_waiting_for_the_busy_worker() {
    let worker = NavigationStatWorker::new();
    let gate = busy(&worker);
    let token = CancelToken::new();
    token.cancel();

    let task = worker.run(|| 2, &token);
    let canceled_at_return = task.is_canceled();
    let _ = gate.release.send(());

    assert!(canceled_at_return, "must not queue behind the busy item");
    assert_eq!(gate.task.wait(), Ok(1));
}

// C#: InstanceAndWorkerMutationGapTests.StatWorker_ItemCancelledWhileQueued_IsCancelledWithoutRunningItsWork
#[test]
fn item_cancelled_while_queued_is_cancelled_without_running_its_work() {
    let worker = NavigationStatWorker::new();
    let gate = busy(&worker);
    let token = CancelToken::new();
    let ran = Arc::new(Mutex::new(0));
    let ran_in_work = Arc::clone(&ran);

    let second = worker.run(
        move || {
            *ran_in_work.lock().unwrap() += 1;
            1
        },
        &token,
    );
    token.cancel();
    let _ = gate.release.send(());

    assert_eq!(gate.task.wait(), Ok(1));
    assert!(second.wait_timeout(TIMEOUT));
    assert!(second.is_canceled());
    assert_eq!(second.wait(), Err(StatError::Canceled));
    assert_eq!(*ran.lock().unwrap(), 0);
}

// C#: InstanceAndWorkerMutationGapTests.StatWorker_WorkThrows_FaultsOnlyThatTask
#[test]
fn work_panics_faults_only_that_task() {
    let worker = NavigationStatWorker::new();

    let bad = worker.run(|| -> i32 { panic!("stat failed") }, &CancelToken::none());
    let good = worker.run(|| 7, &CancelToken::none());

    // Wait for completion without consuming, then check the flags.
    assert!(bad.wait_timeout(Duration::from_secs(20)));
    assert!(good.wait_timeout(Duration::from_secs(20)));
    // A faulted task is completed but not canceled.
    assert!(bad.is_completed());
    assert!(!bad.is_canceled());
    assert!(good.is_completed());
    assert!(!good.is_canceled());
    assert!(matches!(bad.wait(), Err(StatError::Panicked(m)) if m.contains("stat failed")));
    assert_eq!(good.wait(), Ok(7));
}

#[test]
fn token_cancelled_after_the_work_started_has_no_effect() {
    let worker = NavigationStatWorker::new();
    let token = CancelToken::new();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release, rx) = mpsc::channel::<()>();
    let task = worker.run(
        move || {
            let _ = started_tx.send(());
            let _ = rx.recv_timeout(TIMEOUT);
            5
        },
        &token,
    );
    assert!(started_rx.recv_timeout(TIMEOUT).is_ok());
    token.cancel();
    let _ = release.send(());
    assert_eq!(task.wait(), Ok(5));
}

#[test]
fn wait_timeout_returns_false_and_leaves_the_result_for_wait() {
    let worker = NavigationStatWorker::new();
    let gate = busy(&worker);
    assert!(!gate.task.wait_timeout(Duration::from_millis(30)));
    assert!(!gate.task.is_completed());
    let _ = gate.release.send(());
    assert!(gate.task.wait_timeout(TIMEOUT));
    assert!(gate.task.is_completed());
    assert!(!gate.task.is_canceled());
    assert_eq!(gate.task.wait(), Ok(1));
}

// C#: NavigationStatWorker (golden oracle `oracle-cs navigation-stat-worker`).
// DIVERGENCE: `exception_type`/`message` are C#-specific; `faulted` maps to `Panicked` whose text contains `boom-<id>`.
#[test]
fn navigation_stat_worker_golden_replays_every_case() {
    let g = golden::load("navigation-stat-worker");
    for case in &g.cases {
        let name = str_of(case, "name");
        let worker = NavigationStatWorker::new();
        let gate = bool_of(case, "busy_first").then(|| busy(&worker));
        let ran_order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let threads: Arc<Mutex<Vec<ThreadId>>> = Arc::new(Mutex::new(Vec::new()));

        let outcomes = case["outcomes"].as_array().expect("outcomes");
        let mut tasks = Vec::new();
        let mut late_tokens = Vec::new();
        for outcome in outcomes {
            let id = str_of(outcome, "id").to_owned();
            let status = str_of(outcome, "status");
            let token = CancelToken::new();
            let at_return = bool_of(outcome, "canceled_at_return");
            if status == "canceled" {
                if at_return {
                    token.cancel();
                } else {
                    late_tokens.push(token.clone());
                }
            }
            let throws = status == "faulted";
            let value = if status == "value" { i64_of(outcome, "value") } else { 0 };
            let (order, thread_log, work_id) = (Arc::clone(&ran_order), Arc::clone(&threads), id.clone());
            let task = worker.run(
                move || {
                    order.lock().unwrap().push(work_id.clone());
                    thread_log.lock().unwrap().push(thread::current().id());
                    if throws {
                        panic!("boom-{work_id}");
                    }
                    value
                },
                &token,
            );
            assert_eq!(task.is_canceled(), at_return, "{name}/{id}: canceled_at_return");
            tasks.push((id, status.to_owned(), outcome, task));
        }
        for token in &late_tokens {
            token.cancel();
        }
        if let Some(gate) = gate {
            let _ = gate.release.send(());
            assert_eq!(gate.task.wait(), Ok(1), "{name}: gate item");
        }

        for (id, status, outcome, task) in tasks {
            assert!(task.wait_timeout(TIMEOUT), "{name}/{id}: never completed");
            match (status.as_str(), task.wait()) {
                ("value", Ok(v)) => assert_eq!(v, i64_of(outcome, "value"), "{name}/{id}"),
                ("canceled", Err(StatError::Canceled)) => {}
                ("faulted", Err(StatError::Panicked(m))) => {
                    assert!(m.contains(str_of(outcome, "message")), "{name}/{id}: {m}");
                }
                (s, r) => panic!("{name}/{id}: expected {s}, got {r:?}"),
            }
        }

        let expected_order: Vec<String> = case["ran_order"]
            .as_array()
            .expect("ran_order")
            .iter()
            .map(|v| v.as_str().expect("id").to_owned())
            .collect();
        assert_eq!(*ran_order.lock().unwrap(), expected_order, "{name}: ran_order");
        let ids = threads.lock().unwrap();
        let single = ids.windows(2).all(|w| w[0] == w[1]) && ids.iter().all(|t| *t != thread::current().id());
        assert_eq!(
            single,
            bool_of(case, "single_worker_thread"),
            "{name}: single_worker_thread"
        );
    }
}
