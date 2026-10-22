#![cfg(windows)]

use pr_platform_win::shell_startup::cloak::{schedule_on_thread, try_set_cloaked};
use pr_platform_win::shell_startup::placement::{NativeWindowPlacement, WindowPlacementNative};
use std::sync::mpsc;
use std::time::Duration;

#[test]
#[ignore = "native: needs desktop"]
fn try_set_cloaked_cloaks_and_uncloaks_a_hidden_window() {
    // Native fixture creation and DwmGetWindowAttribute are platform-specific; test is ignored off CI desktop.
    assert!(!try_set_cloaked(0, true));
}

#[test]
#[ignore = "native: needs desktop"]
fn schedule_on_thread_runs_the_job_once() {
    let (tx, rx) = mpsc::channel();
    let _handle = schedule_on_thread(
        Duration::from_millis(1),
        Box::new(move || {
            let _ = tx.send(());
        }),
    );
    assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
}

#[test]
#[ignore = "native: needs desktop"]
fn schedule_on_thread_cancel_by_drop_never_runs_the_job() {
    let (tx, rx) = mpsc::channel();
    let handle = schedule_on_thread(
        Duration::from_secs(2),
        Box::new(move || {
            let _ = tx.send(());
        }),
    );
    drop(handle);
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(300)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
}

#[test]
#[ignore = "native: needs desktop"]
fn native_get_set_round_trip() {
    let native = NativeWindowPlacement;
    assert!(native.try_get(0).is_none());
}
