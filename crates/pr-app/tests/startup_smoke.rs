//! W8-17e smoke run, pure half. Contract: `contracts/W8-17e.md`.
use pr_app::startup::smoke::*;
use std::sync::Arc;

#[test]
fn smoke_trigger_follows_the_initial_path() {
    // new: C# `ArmSmokeExit` lines 487-498.
    assert_eq!(smoke_trigger(None), SmokeTrigger::FirstPresented);
    assert_eq!(smoke_trigger(Some("x.jpg")), SmokeTrigger::FirstImagePresented);
    assert_eq!(smoke_trigger(Some("")), SmokeTrigger::FirstImagePresented);
}

#[test]
fn smoke_latch_fires_once() {
    // new: C# `Interlocked.Exchange(ref closing, 1) != 0`.
    let latch = SmokeLatch::new();
    assert!(latch.try_close());
    assert!(!latch.try_close());
    assert!(!latch.try_close());
}

#[test]
fn smoke_latch_fires_once_across_threads() {
    // new: the trigger and the timeout race on different threads.
    let latch = Arc::new(SmokeLatch::new());
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let latch = Arc::clone(&latch);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                latch.try_close()
            })
        })
        .collect();
    let winners = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
}
