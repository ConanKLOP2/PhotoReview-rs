//! Mutation killers of lane cl-host3 (2026-10-31): `host/ui_dispatcher.rs` lines 246+ (batch, background, wake protocol, thread checks,
//! trait impls). Everything runs on the TEST thread (no message loop, no helper thread that a broken mutant could hang): the dispatcher is driven
//! by hand (`run_high_priority_batch`, `try_run_background`, `on_wake_message`) and the wake message is taken out of the queue with
//! `PeekMessageW(PM_REMOVE)` WITHOUT dispatching it. Message-only window, never shown. No sleeps; the only wait is a busy spin on `Instant`.
#![cfg(windows)]

use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pr_core::abstractions::{Log, NullLog, UiCompletion, UiDispatcher, UiPriority, UiScheduler};
use pr_platform_win::host::message_loop::LoopDispatcher;
use pr_platform_win::host::ui_dispatcher::{BATCH_SLICE_MS, WAKE_MESSAGE, Win32UiDispatcher};
use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};

#[derive(Default)]
struct RecordingLog {
    errors: Mutex<Vec<(String, Option<String>)>>,
}

impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        self.errors
            .lock()
            .unwrap()
            .push((message.to_owned(), error.map(ToString::to_string)));
    }
}

fn new_dispatcher() -> Win32UiDispatcher {
    Win32UiDispatcher::new(Arc::new(NullLog)).expect("dispatcher")
}

/// Takes the pending wake message(s) out of this thread's queue without dispatching them (so the dispatcher's window procedure does not run).
fn discard_wake_messages() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a live MSG; the filter selects only `WAKE_MESSAGE`, PM_REMOVE removes it from the queue, nothing is dispatched.
    while unsafe { PeekMessageW(&mut msg, None, WAKE_MESSAGE, WAKE_MESSAGE, PM_REMOVE) }.as_bool() {}
}

fn counter() -> (Arc<AtomicUsize>, impl Fn() -> Box<dyn FnOnce() + Send>) {
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    (count, move || {
        let c = Arc::clone(&c);
        Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })
    })
}

fn spin_past_the_slice() {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(BATCH_SLICE_MS + 5) {
        std::hint::spin_loop();
    }
}

/// 284 `run_high_priority_batch -> ()`, 370 `dequeue_high -> None`, 415 `execute -> ()`, 467/483 `post*`/`post_with_priority -> ()`, 375 `try_enqueue`:
/// a posted Normal item runs when the batch is called on the UI thread, and Send/Normal/Render run in level order.
#[test]
#[ignore = "native: needs desktop"]
fn batch_runs_posted_items_in_priority_order_on_the_ui_thread() {
    let dispatcher = new_dispatcher();
    let order = Arc::new(Mutex::new(Vec::new()));
    for (name, priority) in [
        ("render", UiPriority::Render),
        ("normal", UiPriority::Normal),
        ("send", UiPriority::Send),
    ] {
        let order = Arc::clone(&order);
        dispatcher.post_with_priority(Box::new(move || order.lock().unwrap().push(name)), priority);
    }
    let order2 = Arc::clone(&order);
    dispatcher.post(Box::new(move || order2.lock().unwrap().push("post")));
    assert!(dispatcher.has_high_priority_work());
    dispatcher.run_high_priority_batch();
    assert_eq!(*order.lock().unwrap(), ["send", "normal", "post", "render"]);
    assert!(!dispatcher.has_high_priority_work());
    dispatcher.shutdown();
}

/// 293 `>= -> <` in the slice check: an item that uses the whole slice ends the batch, the next item waits for the next wake.
#[test]
#[ignore = "native: needs desktop"]
fn batch_stops_after_the_item_that_used_up_the_fifty_millisecond_slice() {
    let dispatcher = new_dispatcher();
    let (ran, make) = counter();
    dispatcher.post(Box::new(spin_past_the_slice));
    dispatcher.post(make());
    dispatcher.run_high_priority_batch();
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "the second item must wait for the next batch"
    );
    assert!(dispatcher.has_high_priority_work());
    dispatcher.run_high_priority_batch();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    dispatcher.shutdown();
}

/// And the other direction: fast items all run in one batch.
#[test]
#[ignore = "native: needs desktop"]
fn batch_runs_many_fast_items_in_one_pass() {
    let dispatcher = new_dispatcher();
    let (ran, make) = counter();
    for _ in 0..5 {
        dispatcher.post(make());
    }
    dispatcher.run_high_priority_batch();
    assert_eq!(ran.load(Ordering::SeqCst), 5);
    dispatcher.shutdown();
}

/// 301 `try_run_background -> true/false`: one Background item per call, `true` only when one ran; not while high-priority work waits.
#[test]
#[ignore = "native: needs desktop"]
fn try_run_background_runs_one_item_and_reports_whether_it_ran() {
    let dispatcher = new_dispatcher();
    let (ran, make) = counter();
    assert!(!dispatcher.try_run_background(), "nothing queued");
    dispatcher.post_with_priority(make(), UiPriority::Background);
    dispatcher.post_with_priority(make(), UiPriority::Background);
    dispatcher.post(make());
    assert!(
        !dispatcher.try_run_background(),
        "a Normal item waits: Background must not run"
    );
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    dispatcher.run_high_priority_batch();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert!(dispatcher.try_run_background());
    assert_eq!(ran.load(Ordering::SeqCst), 2);
    assert!(dispatcher.try_run_background());
    assert_eq!(ran.load(Ordering::SeqCst), 3);
    assert!(!dispatcher.try_run_background());
    dispatcher.shutdown();
}

/// 317 `on_wake_message -> ()`, 319 (`delete !` x2, `&& -> ||` at col 73), 438 `is_message_pending -> true/false`: with no message pending the wake runs the
/// high-priority batch AND one Background item; the remaining background item makes it ask for a new wake.
#[test]
#[ignore = "native: needs desktop"]
fn wake_message_runs_high_work_then_one_background_item_when_no_message_is_pending() {
    let dispatcher = new_dispatcher();
    let (ran, make) = counter();
    dispatcher.post(make());
    dispatcher.post_with_priority(make(), UiPriority::Background);
    dispatcher.post_with_priority(make(), UiPriority::Background);
    discard_wake_messages();
    let posted_before = dispatcher.wake_messages_posted();
    dispatcher.on_wake_message();
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "the Normal item and exactly one Background item"
    );
    assert!(dispatcher.has_background_work());
    // 322:61 `ran_background && has_background_work`: a Background item ran and another waits -> a new wake is posted.
    assert_eq!(dispatcher.wake_messages_posted(), posted_before + 1);
    discard_wake_messages();
    dispatcher.shutdown();
}

/// 319 / 438 (the other side) and 322:61 `&& -> ||`: with a message waiting in the thread queue Background must not run, and no wake is requested
/// for it (nothing ran).
#[test]
#[ignore = "native: needs desktop"]
fn wake_message_leaves_background_alone_while_a_message_is_pending() {
    let dispatcher = new_dispatcher();
    let (ran, make) = counter();
    dispatcher.post_with_priority(make(), UiPriority::Background);
    // The wake posted by `post_with_priority` is still in the queue: a message is pending. Pretend the flag was consumed by clearing it through a batch.
    dispatcher.run_high_priority_batch();
    let posted_before = dispatcher.wake_messages_posted();
    dispatcher.on_wake_message();
    assert_eq!(ran.load(Ordering::SeqCst), 0, "a pending message holds Background back");
    assert!(dispatcher.has_background_work());
    assert_eq!(
        dispatcher.wake_messages_posted(),
        posted_before,
        "nothing ran, no high work: no new wake"
    );
    discard_wake_messages();
    dispatcher.shutdown();
}

/// 322:42 `|| -> &&`: high-priority work left after the batch (slice used up) requests a new wake.
#[test]
#[ignore = "native: needs desktop"]
fn wake_message_requests_another_wake_while_high_priority_work_remains() {
    let dispatcher = new_dispatcher();
    let (ran, make) = counter();
    dispatcher.post(Box::new(spin_past_the_slice));
    dispatcher.post(make());
    discard_wake_messages();
    let posted_before = dispatcher.wake_messages_posted();
    dispatcher.on_wake_message();
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    assert!(dispatcher.has_high_priority_work());
    assert_eq!(dispatcher.wake_messages_posted(), posted_before + 1);
    discard_wake_messages();
    dispatcher.shutdown();
}

/// 390 `request_wake -> ()` / `delete !`: the first enqueue posts exactly one wake, later ones while the flag is set post none.
#[test]
#[ignore = "native: needs desktop"]
fn request_wake_posts_one_message_until_the_batch_clears_the_flag() {
    let dispatcher = new_dispatcher();
    let (_ran, make) = counter();
    assert_eq!(dispatcher.wake_messages_posted(), 0);
    dispatcher.post(make());
    assert_eq!(dispatcher.wake_messages_posted(), 1);
    dispatcher.post(make());
    dispatcher.post_with_priority(make(), UiPriority::Background);
    assert_eq!(dispatcher.wake_messages_posted(), 1, "the flag is still set");
    dispatcher.run_high_priority_batch();
    dispatcher.post(make());
    assert_eq!(dispatcher.wake_messages_posted(), 2, "the batch cleared the flag");
    discard_wake_messages();
    dispatcher.shutdown();
}

/// 472/493 `invoke*`, 415 `execute`: an invoked item resolves its completion on the batch; a panic faults it with the message and is not logged;
/// a panicking `post` is logged and the batch continues.
#[test]
#[ignore = "native: needs desktop"]
fn invoke_completes_after_the_batch_and_posted_panics_are_logged() {
    let log = Arc::new(RecordingLog::default());
    let dispatcher = Win32UiDispatcher::new(log.clone()).expect("dispatcher");
    let (ran, make) = counter();
    let ok: UiCompletion = dispatcher.invoke(make());
    let boom: UiCompletion = dispatcher.invoke_with_priority(Box::new(|| panic!("boom-invoke")), UiPriority::Render);
    assert!(
        ok.try_result().is_none() && boom.try_result().is_none(),
        "nothing ran yet"
    );
    dispatcher.post(Box::new(|| panic!("boom-post")));
    dispatcher.post(make());
    dispatcher.run_high_priority_batch();
    assert_eq!(ran.load(Ordering::SeqCst), 2, "the loop keeps going after a panic");
    assert!(matches!(ok.try_result(), Some(Ok(()))));
    let faulted = boom.try_result().expect("resolved").expect_err("faulted");
    assert!(faulted.message.contains("boom-invoke"), "{}", faulted.message);
    let errors = log.errors.lock().unwrap().clone();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        errors[0].0,
        "Win32UiDispatcher: a posted UI action threw; the message loop keeps running"
    );
    assert!(errors[0].1.as_deref().unwrap_or("").contains("boom-post"), "{errors:?}");
    dispatcher.shutdown();
}

/// 479 `check_access -> true/false`, `== -> !=`; 360 `reject_wrong_thread -> false`; 271/508 `verify_access`; 331 `shutdown -> ()`: from another thread
/// the access check is false, `verify_access` is the exact error, batch / background / shutdown do nothing and log it.
#[test]
#[ignore = "native: needs desktop"]
fn calls_from_another_thread_are_rejected_and_logged() {
    let log = Arc::new(RecordingLog::default());
    let dispatcher = Win32UiDispatcher::new(log.clone()).expect("dispatcher");
    let (ran, make) = counter();
    assert!(dispatcher.check_access());
    assert!(dispatcher.verify_access().is_ok());
    dispatcher.post(make());
    dispatcher.post_with_priority(make(), UiPriority::Background);
    let d = dispatcher.clone();
    thread::spawn(move || {
        assert!(!d.check_access());
        let message = d.verify_access().expect_err("off thread").to_string();
        assert!(message.contains("must be called on its UI thread"), "{message}");
        d.run_high_priority_batch();
        assert!(!d.try_run_background());
        d.shutdown();
    })
    .join()
    .expect("helper thread");
    assert_eq!(ran.load(Ordering::SeqCst), 0, "nothing ran off the UI thread");
    assert!(!dispatcher.is_shut_down());
    assert_eq!(
        log.errors.lock().unwrap().len(),
        3,
        "batch, background, shutdown (verify_access itself does not log)"
    );
    dispatcher.run_high_priority_batch();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    dispatcher.shutdown();
    assert!(dispatcher.is_shut_down());
}

/// 508-520 the `LoopDispatcher` impl forwards to the inherent methods (each `-> true/false/Ok(())/()` mutant).
#[test]
#[ignore = "native: needs desktop"]
fn loop_dispatcher_seam_forwards_to_the_dispatcher() {
    let dispatcher = new_dispatcher();
    let seam: &dyn LoopDispatcher = &dispatcher;
    let (ran, make) = counter();
    assert!(seam.verify_access().is_ok());
    assert!(!seam.has_high_priority_work());
    assert!(!seam.has_background_work());
    assert!(!seam.try_run_background());
    dispatcher.post(make());
    dispatcher.post_with_priority(make(), UiPriority::Background);
    assert!(seam.has_high_priority_work());
    assert!(seam.has_background_work());
    seam.run_high_priority_batch();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert!(!seam.has_high_priority_work());
    assert!(seam.try_run_background());
    assert_eq!(ran.load(Ordering::SeqCst), 2);
    assert!(!seam.has_background_work());
    let d = dispatcher.clone();
    thread::spawn(move || {
        let seam: &dyn LoopDispatcher = &d;
        assert!(seam.verify_access().is_err());
    })
    .join()
    .expect("helper thread");
    discard_wake_messages();
    dispatcher.shutdown();
}

/// 331 `shutdown -> ()` / 444 `ensure_class_registered -> Ok(())`: after `shutdown` posts are dropped, an invoke resolves as dropped, a second
/// dispatcher can be created on the same thread again.
#[test]
#[ignore = "native: needs desktop"]
fn shutdown_drops_pending_and_later_work_and_frees_the_thread() {
    let dispatcher = new_dispatcher();
    let (ran, make) = counter();
    let pending = dispatcher.invoke(make());
    dispatcher.shutdown();
    assert!(dispatcher.is_shut_down());
    assert!(pending.wait().is_err(), "pending invoke resolves as dropped");
    dispatcher.post(make());
    assert!(!dispatcher.has_high_priority_work());
    assert!(dispatcher.invoke(make()).wait().is_err());
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    let again = new_dispatcher();
    assert!(Win32UiDispatcher::from_current_thread().is_some());
    again.shutdown();
}
