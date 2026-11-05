//! MUT3-HOST (2026-11-06): lifecycle messages never reach the handler chain. `ShellWindowInner::process_message` answers `WM_NCCREATE`, `WM_CREATE`,
//! `WM_DESTROY` and `WM_NCDESTROY` before the handlers run (step 1 of its doc); deleting the `WM_NCDESTROY` arm (`messages.rs:52:13`) survived because no
//! test looked at what the handlers saw while the window was destroyed. `on_destroy` does not clear the handler list, so a recording handler would see
//! `WM_NCDESTROY` under that mutant. Runs on the test thread with a hidden window; no message loop is needed (everything is a synchronous send).
#![cfg(windows)]

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_platform_win::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::win32::dialog_native::user32 as dialog_user32;
use pr_platform_win::win32::messages::WM_APP;
use pr_platform_win::win32::structs::Lresult;

const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_NCCREATE: u32 = 0x0081;
const WM_NCDESTROY: u32 = 0x0082;

struct Recorder(Arc<Mutex<Vec<u32>>>);

impl WindowMessageHandler for Recorder {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        self.0.lock().unwrap().push(message.msg);
        None
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn destroying_a_window_never_shows_the_lifecycle_messages_to_the_handlers() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let clock: Rc<dyn HostFrameClock> = Rc::new(TimerFrameClock::create(None, 0).unwrap());
    let window = ShellWindow::create(ShellWindowOptions::default(), clock).unwrap();
    window.add_message_handler(Rc::new(Recorder(Arc::clone(&seen))));
    let hwnd = window.hwnd();

    // Positive control: an ordinary message does reach the handler.
    // SAFETY: integer parameters only; the window procedure runs synchronously on this thread.
    unsafe { dialog_user32::send_message(hwnd, WM_APP + 61, 0, 0) };
    assert_eq!(*seen.lock().unwrap(), vec![WM_APP + 61], "the recorder works");

    window.dispose(); // DestroyWindow: WM_DESTROY, WM_NCDESTROY
    let seen = seen.lock().unwrap().clone();
    for lifecycle in [WM_NCCREATE, WM_CREATE, WM_DESTROY, WM_NCDESTROY] {
        assert!(
            !seen.contains(&lifecycle),
            "handlers must not see lifecycle message {lifecycle:#06x}; saw {seen:x?}"
        );
    }
}
