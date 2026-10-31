//! Fail-fast killers for `session/writer.rs` hang-style mutants (lane cl-mut-perfcom).
//!
//! Every wait is a bounded `recv_timeout`; nothing sleeps or reads a clock, so a mutant that makes `Completion::wait`
//! block forever fails an assertion instead of hanging the test binary.
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pr_core::session::writer::Completion;

const BOUND: Duration = Duration::from_secs(5);

/// Runs `Completion::wait` on a helper thread; the receiver gets `is_complete()` as seen right after `wait` returned.
fn wait_returns(completion: &Completion) -> mpsc::Receiver<bool> {
    let (tx, rx) = mpsc::channel();
    let waiting = completion.clone();
    thread::spawn(move || {
        waiting.wait();
        let _ = tx.send(waiting.is_complete());
    });
    rx
}

#[test]
fn wait_on_a_completed_completion_returns_at_once() {
    let completion = Completion::completed();
    let rx = wait_returns(&completion);
    assert!(
        rx.recv_timeout(BOUND).is_ok(),
        "wait() must not block when the completion is already done"
    );
}

#[test]
fn wait_blocks_until_complete_and_then_returns() {
    let completion = Completion::new();
    let rx = wait_returns(&completion);
    // Not complete yet: the waiter must still be blocked (a `wait -> ()` mutant would already have answered `false`).
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "wait() returned before complete() ran"
    );
    completion.complete();
    // wait() may only return once the flag is set, so the waiter always observes a completed state.
    assert_eq!(
        rx.recv_timeout(BOUND),
        Ok(true),
        "wait() must return after complete(), never before"
    );
}
