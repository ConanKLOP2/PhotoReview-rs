//! Regression tests of lane `fix-shellwindow` (ST-WIN32 M1 and M2) on a real UI thread with a hidden, never shown window. The native tests are
//! `#[ignore]`d ("native: needs desktop"). Every wait is bounded; no sleeps.
//!
//! * M1: the owner `ShellWindow` is dropped from a `Closed` subscriber, i.e. between `WM_DESTROY` and `WM_NCDESTROY`. `GWLP_USERDATA` still points at the
//!   cookie then, so the cookie must stay alive until `WM_NCDESTROY` frees it. Observed without reading freed memory: a `WH_CALLWNDPROC` hook sees
//!   `WM_NCDESTROY` BEFORE the window procedure and counts the strong references of the frame clock, which only the window object (the cookie's target)
//!   holds. When the target is already gone (the bug) the hook clears `GWLP_USERDATA` itself so the test never dereferences the dangling cookie.
//! * M2: a `WM_CLOSE` that arrives while a `Closing` handler still runs must not destroy the window under that handler.
#![cfg(windows)]

#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::dialog_native::user32 as dialog_user32;
use pr_platform_win::win32::messages::{GWLP_USERDATA, WM_CLOSE, WM_NCDESTROY};
use pr_platform_win::win32::structs::Hwnd;
use shell_window_fixture::{create_window, dispose_window, send, window};
use ui_thread::{UiThread, ui_loop};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CWPSTRUCT, CallNextHookEx, HHOOK, SetWindowsHookExW, UnhookWindowsHookEx, WH_CALLWNDPROC,
};

const UNSET: usize = usize::MAX;

static PROBE_HWND: AtomicUsize = AtomicUsize::new(0);
/// Strong references of the frame clock when the hook saw `WM_NCDESTROY` of the probed window.
static COUNT_AT_NCDESTROY: AtomicUsize = AtomicUsize::new(UNSET);
/// The same count measured in the `Closed` subscriber right after the owner was dropped (the window procedure still holds its own clone then).
static COUNT_AT_CLOSED: AtomicUsize = AtomicUsize::new(UNSET);

thread_local! {
    static PROBE_CLOCK: RefCell<Option<std::rc::Weak<dyn HostFrameClock>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn nc_destroy_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam.0 != 0 {
        // SAFETY: for WH_CALLWNDPROC `lParam` points to a CWPSTRUCT that is valid for the duration of the hook call.
        let message = unsafe { &*(lparam.0 as *const CWPSTRUCT) };
        let probed = PROBE_HWND.load(Ordering::SeqCst);
        if probed != 0 && message.message == WM_NCDESTROY && message.hwnd.0 as usize == probed {
            let count = PROBE_CLOCK.with(|slot| slot.borrow().as_ref().map_or(UNSET, std::rc::Weak::strong_count));
            COUNT_AT_NCDESTROY.store(count, Ordering::SeqCst);
            if count != COUNT_AT_CLOSED.load(Ordering::SeqCst) {
                // The window object is already gone: make the window procedure see "no target" instead of a dangling cookie.
                // SAFETY: clearing GWLP_USERDATA (0) is the state the window procedure handles ("after WM_NCDESTROY").
                let _previous = unsafe { user32::set_window_long_ptr(probed as Hwnd, GWLP_USERDATA, 0) };
            }
        }
    }
    // SAFETY: forwards the hook call unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

// ST-WIN32 M1: dropping the owner between WM_DESTROY and WM_NCDESTROY must not free the cookie the window still points at.
#[test]
#[ignore = "native: needs desktop"]
fn owner_dropped_from_closed_keeps_the_cookie_alive_until_wm_ncdestroy() {
    let ui = UiThread::start();
    let (at_closed, at_ncdestroy, after, exists) = ui.invoke(|| {
        let clock = Rc::new(TimerFrameClock::create(None, 0).expect("timer clock"));
        ui_loop().add_frame_clock(Rc::clone(&clock) as Rc<dyn HostFrameClock>);
        let shared: Rc<dyn HostFrameClock> = clock;
        let weak_clock = Rc::downgrade(&shared);
        let owner = ShellWindow::create(ShellWindowOptions::default(), shared).expect("shell window");
        let hwnd = owner.hwnd();
        let slot = Rc::new(RefCell::new(Some(owner)));

        let at_closed = Rc::new(Cell::new(UNSET));
        {
            let (inner_slot, at_closed, weak_clock) = (Rc::clone(&slot), Rc::clone(&at_closed), weak_clock.clone());
            slot.borrow().as_ref().expect("owner").subscribe(
                ShellWindowEvent::Closed,
                Box::new(move || {
                    let owner = inner_slot.borrow_mut().take();
                    drop(owner); // WM_DESTROY has run; WM_NCDESTROY has not.
                    at_closed.set(weak_clock.strong_count());
                    COUNT_AT_CLOSED.store(weak_clock.strong_count(), Ordering::SeqCst);
                }),
            );
        }
        PROBE_CLOCK.with(|probe| *probe.borrow_mut() = Some(weak_clock.clone()));
        PROBE_HWND.store(hwnd as usize, Ordering::SeqCst);
        COUNT_AT_NCDESTROY.store(UNSET, Ordering::SeqCst);
        COUNT_AT_CLOSED.store(UNSET, Ordering::SeqCst);
        // SAFETY: `nc_destroy_hook` is a valid HOOKPROC that only reads the CWPSTRUCT of the call and clears GWLP_USERDATA of the probed window; the hook is
        // removed below on this same thread.
        let hook: HHOOK = unsafe {
            SetWindowsHookExW(WH_CALLWNDPROC, Some(nc_destroy_hook), None, GetCurrentThreadId()).expect("hook")
        };
        // Destroy: use the raw API so the owner stays inside `slot` until `Closed` drops it.
        let _ = user32::destroy_window(hwnd);
        let observed = COUNT_AT_NCDESTROY.load(Ordering::SeqCst);
        // SAFETY: `hook` came from SetWindowsHookExW above on this thread.
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        PROBE_HWND.store(0, Ordering::SeqCst);
        (
            at_closed.get(),
            observed,
            weak_clock.strong_count(),
            dialog_user32::is_window(hwnd),
        )
    });
    assert!(!exists, "the window is destroyed");
    assert_ne!(at_closed, UNSET, "Closed ran and dropped the owner");
    assert_eq!(
        at_ncdestroy, at_closed,
        "the window object must still be alive when WM_NCDESTROY reaches the procedure (cookie not freed by the owner's drop)"
    );
    assert_eq!(
        after,
        at_ncdestroy - 1,
        "WM_NCDESTROY frees the cookie (and with it the window object): no leak"
    );
    ui.invoke(|| ());
}

// ST-WIN32 M2: a WM_CLOSE sent while a Closing handler still runs is ignored; the next close works again.
#[test]
#[ignore = "native: needs desktop"]
fn nested_wm_close_during_a_closing_handler_does_not_destroy_the_window() {
    let ui = UiThread::start();
    let hwnd = ui.invoke(|| create_window(ShellWindowOptions::default()).hwnd());
    let calls = Arc::new(AtomicU32::new(0));
    let alive_after_nested = Arc::new(AtomicBool::new(false));
    let nested_result = Arc::new(AtomicUsize::new(UNSET));
    {
        let (calls, alive_after_nested, nested_result) = (
            Arc::clone(&calls),
            Arc::clone(&alive_after_nested),
            Arc::clone(&nested_result),
        );
        ui.invoke(move || {
            window().subscribe_closing(Box::new(move |args| {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    // The confirmation box of the real handler pumps messages; a WM_CLOSE from elsewhere arrives meanwhile.
                    let result = send(hwnd, WM_CLOSE, 0, 0);
                    nested_result.store(result as usize, Ordering::SeqCst);
                    alive_after_nested.store(dialog_user32::is_window(hwnd), Ordering::SeqCst);
                    args.cancel = true; // the user said "no"
                }
            }));
        });
    }

    ui.invoke(|| window().close());
    assert_eq!(
        nested_result.load(Ordering::SeqCst),
        0,
        "the nested WM_CLOSE is answered with 0"
    );
    assert!(
        alive_after_nested.load(Ordering::SeqCst),
        "the nested WM_CLOSE must not destroy the window while the outer Closing handler runs"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the running handler is not re-entered");
    assert!(
        ui.invoke(move || dialog_user32::is_window(hwnd)),
        "the outer handler cancelled"
    );

    ui.invoke(|| window().close());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the flag is cleared: the next close reaches the handler again"
    );
    assert!(!ui.invoke(move || dialog_user32::is_window(hwnd)));
    ui.invoke(dispose_window);
}
