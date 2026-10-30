//! Mutation killers of lane cl-hostrerun (2026-10-31): `host/ui_dispatcher.rs` wire constants that the existing suites only use symbolically.
//! Pure except one message-only window that is never shown. No sleeps, no wall clock.
#![cfg(windows)]

use std::sync::Arc;

use pr_core::abstractions::NullLog;
use pr_platform_win::host::ui_dispatcher::{BATCH_SLICE_MS, WAKE_MESSAGE, Win32UiDispatcher};

/// C# `WakeMessage = WM_APP + 1` with `WM_APP = 0x8000`. The tests post and compare the constant itself, so `+ -> -` / `+ -> *` in its
/// definition (0x7FFF / 0x8000) survived; the literal pins the wire value (0x8000 would also collide with `WM_APP`).
#[test]
fn wake_message_is_wm_app_plus_one() {
    assert_eq!(WAKE_MESSAGE, 0x8001);
    assert_eq!(WAKE_MESSAGE, pr_platform_win::win32::messages::WM_APP + 1);
}

/// C# `BatchSliceMs = 50`.
#[test]
fn batch_slice_is_fifty_milliseconds() {
    assert_eq!(BATCH_SLICE_MS, 50);
}

/// `Debug for Win32UiDispatcher`: `fmt -> Ok(Default::default())` printed nothing. Message-only window on this thread, never shown, no pump needed.
#[test]
#[ignore = "native: needs desktop"]
fn debug_output_names_the_dispatcher_type() {
    let dispatcher = Win32UiDispatcher::new(Arc::new(NullLog)).expect("dispatcher");
    assert_eq!(format!("{dispatcher:?}"), "Win32UiDispatcher { .. }");
    dispatcher.shutdown();
}

/// `from_current_thread -> None` and `delete !` only timed out (300 s) in the old suite, because their helper thread hangs when its assertion panics.
/// Here the asserts run on the test thread itself, so both fail fast: Some while running, None after `shutdown`.
#[test]
#[ignore = "native: needs desktop"]
fn from_current_thread_follows_the_dispatcher_life_cycle_on_the_test_thread() {
    assert!(Win32UiDispatcher::from_current_thread().is_none());
    let dispatcher = Win32UiDispatcher::new(Arc::new(NullLog)).expect("dispatcher");
    let found = Win32UiDispatcher::from_current_thread().expect("registered while running");
    assert_eq!(found.message_window(), dispatcher.message_window());
    dispatcher.shutdown();
    assert!(Win32UiDispatcher::from_current_thread().is_none());
}
