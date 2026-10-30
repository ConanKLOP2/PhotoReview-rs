//! Mutation killers of lane cl-hostrerun2 (2026-10-31): `host/shell_window/dpi.rs` `apply_dpi_change` (`SWP_NOZORDER | SWP_NOACTIVATE`).
//! A hidden window on a private UI thread, never shown or activated. No sleeps, no wall clock.
#![cfg(windows)]

#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_platform_win::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler, new_rect};
use pr_platform_win::win32::messages::{SWP_NOACTIVATE, SWP_NOZORDER, WM_WINDOWPOSCHANGING};
use pr_platform_win::win32::structs::Lresult;
use shell_window_fixture::{create_window, dispose_window, send_dpi_changed, window};
use ui_thread::UiThread;

/// Records the `flags` member of the `WINDOWPOS` of every `WM_WINDOWPOSCHANGING`; handles nothing.
struct PosFlagsProbe {
    flags: Arc<Mutex<Vec<u32>>>,
}

impl WindowMessageHandler for PosFlagsProbe {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if message.msg == WM_WINDOWPOSCHANGING && message.lparam != 0 {
            // SAFETY: for WM_WINDOWPOSCHANGING `lParam` points to a WINDOWPOS (hwnd, hwndInsertAfter, x, y, cx, cy: 8+8+4*4 bytes, then `flags: u32` at
            // offset 32) that is valid for the duration of the synchronous message.
            let flags = unsafe { *((message.lparam as *const u8).add(32) as *const u32) };
            self.flags.lock().unwrap().push(flags);
        }
        None
    }
}

/// dpi.rs:58:30 `| -> &` (flags 0 would re-order and activate the window; `| -> ^` is equivalent because the two flags are disjoint bits):
/// the `SetWindowPos` of a suggested DPI rect carries both `SWP_NOZORDER` and `SWP_NOACTIVATE`.
#[test]
#[ignore = "native: needs desktop"]
fn dpi_change_set_window_pos_keeps_the_z_order_and_does_not_activate() {
    let ui = UiThread::start();
    let hwnd = ui.invoke(|| create_window(Default::default()).hwnd());
    let flags = Arc::new(Mutex::new(Vec::new()));
    {
        let flags = Arc::clone(&flags);
        ui.invoke(move || window().add_message_handler(Rc::new(PosFlagsProbe { flags })));
    }
    ui.invoke(move || send_dpi_changed(hwnd, 144, new_rect(40, 50, 40 + 800, 50 + 600)));
    let seen = flags.lock().unwrap().clone();
    assert!(!seen.is_empty(), "SetWindowPos sent WM_WINDOWPOSCHANGING");
    let wanted = SWP_NOZORDER | SWP_NOACTIVATE;
    assert!(seen.iter().any(|f| f & wanted == wanted), "flags seen: {seen:x?}");
    ui.invoke(dispose_window);
}

/// ui_dispatcher.rs:256/261/266 `is_shut_down`, `has_high_priority_work`, `has_background_work` replaced by constants (the `-> true` ones only timed out
/// in the old suite, `-> false` survived): the three flags follow the queues on the test thread itself, with no loop running and no helper thread.
#[test]
#[ignore = "native: needs desktop"]
fn dispatcher_flags_follow_the_queues_and_the_shutdown() {
    use pr_core::abstractions::{NullLog, UiDispatcher, UiPriority};
    use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;

    let dispatcher = Win32UiDispatcher::new(Arc::new(NullLog)).expect("dispatcher");
    assert!(!dispatcher.is_shut_down());
    assert!(!dispatcher.has_high_priority_work());
    assert!(!dispatcher.has_background_work());
    dispatcher.post_with_priority(Box::new(|| {}), UiPriority::Background);
    assert!(dispatcher.has_background_work());
    assert!(!dispatcher.has_high_priority_work(), "only a background item is queued");
    dispatcher.post_with_priority(Box::new(|| {}), UiPriority::Normal);
    assert!(dispatcher.has_high_priority_work());
    dispatcher.shutdown();
    assert!(dispatcher.is_shut_down());
}
