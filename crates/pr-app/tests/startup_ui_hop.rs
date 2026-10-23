//! W8-17d: the UI continuation is retained on the registering thread only.
use pr_app::startup::ui_hop::{HopLost, ui_hop};
use pr_test_support::QueuedUiScheduler;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[test]
fn send_posts_once_and_releases_the_continuation() {
    // New Rust await replacement (C# ShellStartup.StartupCoreAsync await settingsLoad).
    let ui = Arc::new(QueuedUiScheduler::new());
    let values = Rc::new(RefCell::new(Vec::new()));
    let weak = Rc::downgrade(&values);
    let sink = values.clone();
    let sender = ui_hop(ui.clone(), move |value| sink.borrow_mut().push(value));
    std::thread::spawn(move || sender.send(42)).join().unwrap();
    assert!(values.borrow().is_empty());
    assert_eq!(ui.pending(), 1);
    assert_eq!(ui.run_pending(), 1);
    assert_eq!(*values.borrow(), vec![Ok(42)]);
    assert_eq!(ui.run_pending(), 0);
    drop(values);
    assert!(weak.upgrade().is_none());
}

#[test]
fn dropped_sender_posts_hop_lost_once() {
    // New: a panicked or unstarted worker cannot silently lose its UI continuation.
    let ui = Arc::new(QueuedUiScheduler::new());
    let values = Rc::new(RefCell::new(Vec::new()));
    let sink = values.clone();
    drop(ui_hop::<i32>(ui.clone(), move |v| sink.borrow_mut().push(v)));
    assert!(values.borrow().is_empty());
    assert_eq!(ui.run_pending(), 1);
    assert_eq!(*values.borrow(), vec![Err(HopLost)]);
}

#[test]
fn a_continuation_can_register_and_deliver_another_hop() {
    // New: no registry borrow survives the callback.
    let ui = Arc::new(QueuedUiScheduler::new());
    let values = Rc::new(RefCell::new(Vec::new()));
    let sink = values.clone();
    let nested_ui = ui.clone();
    ui_hop(ui.clone(), move |first| {
        sink.borrow_mut().push(first.unwrap());
        ui_hop(nested_ui, move |second| sink.borrow_mut().push(second.unwrap())).send(2);
    })
    .send(1);
    assert_eq!(ui.run_pending(), 2);
    assert_eq!(*values.borrow(), vec![1, 2]);
}

#[test]
fn wrong_thread_delivery_does_not_run_the_registered_continuation() {
    // New: Send actions carry only an id; the non-Send continuation remains on its owning thread.
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;
    let ui = Arc::new(QueuedUiScheduler::new());
    let ran = Arc::new(AtomicBool::new(false));
    let sink = ran.clone();
    let scheduler = ui.clone();
    let (ready, started) = mpsc::channel();
    let (finish, stop) = mpsc::channel();
    let owner = std::thread::spawn(move || {
        ui_hop(scheduler, move |_: Result<i32, HopLost>| {
            sink.store(true, Ordering::SeqCst);
        })
        .send(1);
        ready.send(()).unwrap();
        stop.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(ui.run_pending(), 1);
    assert!(!ran.load(Ordering::SeqCst));
    finish.send(()).unwrap();
    owner.join().unwrap();
}
