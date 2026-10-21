//! Tests of W8-13e (N): the `WndProcThunk` trampoline with a recording fake `WindowTarget` (a window of a test class whose procedure is
//! `shell_window_proc`; message-only windows, never shown), and `ShellWindow` creation, lifecycle and public API on a real UI thread. `WndProcThunk`
//! has no C# test of its own (every shell-window test covers it); the `ShellWindow` ports are the 4 tests of `ShellWindowTests` that need no message
//! arm of 13f. The native tests are `#[ignore]`d ("native: needs desktop"): they create real (hidden) windows. Every wait is bounded.
//! `WindowMembers_OffTheUiThread_Throw` has no twin (`ShellWindow` is `!Send`; `shell_window_is_not_send` below is the compile-time proof).
#![cfg(windows)]

#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;
use std::sync::mpsc::channel;
use std::sync::{Arc, Once};

use pr_platform_win::host::contracts::{PointD, ShellCursor, ShellWindowApi, ShellWindowEvent};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
use pr_platform_win::host::pending_panic::{has_pending, rethrow_pending};
use pr_platform_win::host::shell_window::create::{CLASS_NAME, DEFAULT_DPI, EventList, to_pixels};
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::host::wndproc::{WindowTarget, free_cookie, make_cookie, shell_window_proc};
use pr_platform_win::win32::api::{kernel32, user32};
use pr_platform_win::win32::dialog_native::user32 as dialog_user32;
use pr_platform_win::win32::messages::{
    GWLP_USERDATA, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_USER,
};
use pr_platform_win::win32::structs::{Hwnd, Lparam, Lresult, Win32Error, Wparam};
use shell_window_fixture::{
    create_window, dispose_window, log_snapshot, new_log, send, wait_until_idle, window, window_rect, window_text,
};
use ui_thread::{BOUND, UiThread, ui_loop};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, RegisterClassExW, WNDCLASSEXW};
use windows::core::w;

// ---- the trampoline with a recording target ----

/// What the trampoline tests need from a target: a log of the messages, the hooks, a panic switch.
struct Recorder {
    log: RefCell<Vec<u32>>,
    hwnd: Cell<Hwnd>,
    cookie: Cell<isize>,
    native_destroyed: Cell<u32>,
    slot_when_notified: Cell<Option<isize>>,
    free_cookie_when_notified: bool,
    /// A message that makes `process_message` panic with its `lparam` as payload.
    panic_on: Cell<Option<u32>>,
}

impl Recorder {
    fn new(free_cookie_when_notified: bool) -> Rc<Self> {
        Rc::new(Self {
            log: RefCell::new(Vec::new()),
            hwnd: Cell::new(0),
            cookie: Cell::new(0),
            native_destroyed: Cell::new(0),
            slot_when_notified: Cell::new(None),
            free_cookie_when_notified,
            panic_on: Cell::new(None),
        })
    }
}

impl WindowTarget for Recorder {
    fn process_message(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        self.hwnd.set(hwnd);
        self.log.borrow_mut().push(message);
        if self.panic_on.get() == Some(message) {
            panic_any(format!("panic {lparam}"));
        }
        // SAFETY: the parameters are the ones the window procedure received.
        unsafe { user32::def_window_proc(hwnd, message, wparam, lparam) }
    }

    fn on_native_destroyed(&self) {
        self.native_destroyed.set(self.native_destroyed.get() + 1);
        self.slot_when_notified
            .set(Some(user32::get_window_long_ptr(self.hwnd.get(), GWLP_USERDATA)));
        if self.free_cookie_when_notified {
            // SAFETY: the cookie came from `make_cookie` and is freed here once; the trampoline holds its own clone of the `Rc`.
            unsafe { free_cookie(self.cookie.replace(0)) };
        }
    }
}

/// `WM_GETTEXTLENGTH`.
const WM_GETTEXTLENGTH: u32 = 0x000E;
const TRAMPOLINE_CLASS: &str = "PhotoReviewRs.Test.Trampoline";
const PLAIN_CLASS: &str = "PhotoReviewRs.Test.Plain";

unsafe extern "system" fn plain_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: forwards the parameters this procedure received.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn register_test_classes() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let instance = HINSTANCE(kernel32::get_module_handle().unwrap() as *mut core::ffi::c_void);
        for (name, procedure) in [
            (
                w!("PhotoReviewRs.Test.Trampoline"),
                shell_window_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
            ),
            (w!("PhotoReviewRs.Test.Plain"), plain_proc),
        ] {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: name,
                ..Default::default()
            };
            // SAFETY: `class` is a live, correctly sized WNDCLASSEXW; the class name is a static literal; the procedures are valid for the process.
            assert_ne!(unsafe { RegisterClassExW(&class) }, 0);
        }
    });
}

/// A message-only window of `class` with `cookie` as `lpCreateParams`.
fn create_message_only(class: &str, title: &str, cookie: isize) -> Result<Hwnd, Win32Error> {
    register_test_classes();
    let instance = kernel32::get_module_handle().unwrap();
    // SAFETY: `class` is a test class whose procedure is `shell_window_proc` (expects a cookie from `make_cookie` or 0) or `DefWindowProc`.
    unsafe { user32::create_window_ex(0, class, title, 0, 0, 0, 100, 100, -3, 0, instance, cookie) }
}

fn create_with_target(target: &Rc<Recorder>) -> Hwnd {
    let cookie = make_cookie(Rc::clone(target) as Rc<dyn WindowTarget>);
    target.cookie.set(cookie);
    create_message_only(TRAMPOLINE_CLASS, "trampoline", cookie).expect("CreateWindowEx")
}

// C#: no twin (WndProcThunk is covered by every shell-window test)
#[test]
#[ignore = "native: needs desktop"]
fn nc_create_binds_the_cookie_and_every_message_reaches_the_target_in_order() {
    let target = Recorder::new(true);
    let hwnd = create_with_target(&target);
    assert_ne!(
        user32::get_window_long_ptr(hwnd, GWLP_USERDATA),
        0,
        "WM_NCCREATE stored the cookie"
    );
    {
        let log = target.log.borrow();
        assert_eq!(log[0], WM_NCCREATE, "log: {log:?}");
        let create = log
            .iter()
            .position(|&message| message == WM_CREATE)
            .expect("WM_CREATE reached the target");
        assert!(
            create > 0,
            "WM_CREATE comes after WM_NCCREATE (the OS sends WM_NCCALCSIZE in between): {log:?}"
        );
    }
    user32::destroy_window(hwnd).unwrap();
    let log = target.log.borrow();
    let tail: Vec<u32> = log.iter().rev().take(2).rev().copied().collect();
    assert_eq!(tail, vec![WM_DESTROY, WM_NCDESTROY], "log: {log:?}");
    assert_eq!(target.native_destroyed.get(), 1);
}

#[test]
#[ignore = "native: needs desktop"]
fn zero_cookie_cancels_creation() {
    let result = create_message_only(TRAMPOLINE_CLASS, "no owner", 0);
    assert!(result.is_err(), "WM_NCCREATE returned 0 so CreateWindowEx failed");
    assert!(!has_pending(), "a cancelled creation is not a panic");
}

#[test]
#[ignore = "native: needs desktop"]
fn messages_before_nc_create_go_to_def_window_proc() {
    // A window of another class has no cookie in its slot: the trampoline must hand the message to DefWindowProc (WM_GETTEXTLENGTH answers the title).
    let hwnd = create_message_only(PLAIN_CLASS, "abcdef", 0).expect("plain window");
    assert_eq!(user32::get_window_long_ptr(hwnd, GWLP_USERDATA), 0);
    let result = unsafe_call_proc(hwnd, WM_GETTEXTLENGTH);
    assert_eq!(result, 6);
    user32::destroy_window(hwnd).unwrap();
}

fn unsafe_call_proc(hwnd: Hwnd, message: u32) -> isize {
    // SAFETY: called on the thread that created `hwnd`; WM_GETTEXTLENGTH carries no pointers.
    unsafe { shell_window_proc(HWND(hwnd as *mut core::ffi::c_void), message, WPARAM(0), LPARAM(0)) }.0
}

#[test]
#[ignore = "native: needs desktop"]
fn nc_destroy_clears_the_slot_and_notifies_once() {
    let target = Recorder::new(true);
    let hwnd = create_with_target(&target);
    user32::destroy_window(hwnd).unwrap();
    assert_eq!(target.native_destroyed.get(), 1);
    assert_eq!(
        target.slot_when_notified.get(),
        Some(0),
        "the slot is cleared BEFORE the target is notified"
    );
    let calls = target.log.borrow().len();
    assert_eq!(send(hwnd, WM_USER, 0, 0), 0, "a destroyed handle is the OS's 0");
    assert_eq!(
        target.log.borrow().len(),
        calls,
        "no call reaches the target after WM_NCDESTROY"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn target_may_free_its_own_cookie_in_on_native_destroyed() {
    let target = Recorder::new(true);
    let hwnd = create_with_target(&target);
    assert_eq!(Rc::strong_count(&target), 2, "the cookie holds one reference");
    user32::destroy_window(hwnd).unwrap();
    assert_eq!(target.native_destroyed.get(), 1);
    assert_eq!(
        Rc::strong_count(&target),
        1,
        "the cookie was freed exactly once and the clone taken by the trampoline is gone"
    );
    assert_eq!(target.cookie.get(), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_the_target_returns_zero_and_resumes_at_the_owner() {
    let target = Recorder::new(true);
    let hwnd = create_with_target(&target);
    target.panic_on.set(Some(WM_CLOSE));
    let result = send(hwnd, WM_CLOSE, 0, 7);
    assert_eq!(result, 0, "the OS got 0");
    assert!(has_pending());
    let payload = catch_unwind(AssertUnwindSafe(rethrow_pending)).expect_err("the panic resumes at the owner");
    assert_eq!(shell_window_fixture::payload_text(payload.as_ref()), "panic 7");
    assert!(!has_pending(), "the slot is empty after the rethrow");
    assert!(
        dialog_user32::is_window(hwnd),
        "no DefWindowProc(WM_CLOSE) ran, so the window still exists"
    );
    target.panic_on.set(None);
    user32::destroy_window(hwnd).unwrap();
    assert_eq!(target.native_destroyed.get(), 1);
}

#[test]
#[ignore = "native: needs desktop"]
fn first_panic_wins_when_two_messages_panic() {
    let target = Recorder::new(true);
    let hwnd = create_with_target(&target);
    target.panic_on.set(Some(WM_USER));
    assert_eq!(send(hwnd, WM_USER, 0, 1), 0);
    assert_eq!(send(hwnd, WM_USER, 0, 2), 0);
    let payload = catch_unwind(AssertUnwindSafe(rethrow_pending)).expect_err("pending panic");
    assert_eq!(shell_window_fixture::payload_text(payload.as_ref()), "panic 1");
    assert!(!has_pending(), "the second payload was dropped");
    target.panic_on.set(None);
    user32::destroy_window(hwnd).unwrap();
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_during_nc_destroy_still_clears_the_slot_and_notifies_the_target() {
    let target = Recorder::new(true);
    let hwnd = create_with_target(&target);
    target.panic_on.set(Some(WM_NCDESTROY));
    user32::destroy_window(hwnd).unwrap();
    assert_eq!(target.native_destroyed.get(), 1);
    assert_eq!(target.slot_when_notified.get(), Some(0));
    let payload = catch_unwind(AssertUnwindSafe(rethrow_pending)).expect_err("pending panic");
    assert!(shell_window_fixture::payload_text(payload.as_ref()).starts_with("panic"));
}

#[test]
fn make_cookie_and_free_cookie_round_trip() {
    // new: the cookie is a thin non-zero pointer and `free_cookie` releases exactly the reference it holds
    let target = Recorder::new(false);
    let cookie = make_cookie(Rc::clone(&target) as Rc<dyn WindowTarget>);
    assert_ne!(cookie, 0);
    assert_eq!(Rc::strong_count(&target), 2);
    // SAFETY: the cookie was just made and is freed once.
    unsafe { free_cookie(cookie) };
    assert_eq!(Rc::strong_count(&target), 1);
    // SAFETY: 0 is "no cookie" and ignored.
    unsafe { free_cookie(0) };
}

// ---- ShellWindow part 1 (native) ----

// C#: new (ApplyInitialSize, WP-20 SizeIsOuter)
#[test]
#[ignore = "native: needs desktop"]
fn create_makes_a_window_with_the_requested_client_size() {
    let ui = UiThread::start();
    let (client, expected, scale) = ui.invoke(|| {
        let w = create_window(ShellWindowOptions {
            width_dip: 640.0,
            height_dip: 480.0,
            ..ShellWindowOptions::default()
        });
        let scale = w.dpi_scale();
        (
            w.client_size_pixels(),
            (to_pixels(640.0, scale), to_pixels(480.0, scale)),
            scale,
        )
    });
    assert_eq!(client, expected, "client size at scale {scale}");
    ui.invoke(dispose_window);

    let outer = ui.invoke(|| {
        let w = create_window(ShellWindowOptions {
            width_dip: 640.0,
            height_dip: 480.0,
            size_is_outer: true,
            ..ShellWindowOptions::default()
        });
        let rect = window_rect(w.hwnd());
        let scale = w.dpi_scale();
        (
            (rect.right - rect.left, rect.bottom - rect.top),
            (to_pixels(640.0, scale), to_pixels(480.0, scale)),
        )
    });
    assert_eq!(outer.0, outer.1, "size_is_outer: the window rect is the requested size");
    ui.invoke(dispose_window);
}

// new: DpiScale = dpi / 96 with the dpi of the window
#[test]
#[ignore = "native: needs desktop"]
fn dpi_scale_is_the_window_dpi_over_96() {
    let ui = UiThread::start();
    let (scale, dpi) = ui.invoke(|| {
        let w = create_window(ShellWindowOptions::default());
        (w.dpi_scale(), user32::get_dpi_for_window(w.hwnd()))
    });
    assert_eq!(scale, f64::from(dpi) / f64::from(DEFAULT_DPI));
    assert!(dpi >= DEFAULT_DPI || dpi == 0);
    ui.invoke(dispose_window);
}

// C#: ShellWindowTests.SetTitle_ChangesTheWindowText
#[test]
#[ignore = "native: needs desktop"]
fn set_title_changes_the_window_text() {
    let ui = UiThread::start();
    let text = ui.invoke(|| {
        let w = create_window(ShellWindowOptions::default());
        w.set_title("ảnh 1/20 - PhotoReview").unwrap();
        window_text(w.hwnd())
    });
    assert_eq!(text, "ảnh 1/20 - PhotoReview");
    ui.invoke(dispose_window);
}

// C#: ShellWindowTests.PointerCapture_IsTakenAndReleased
#[test]
#[ignore = "native: needs desktop"]
fn pointer_capture_is_taken_and_released() {
    let ui = UiThread::start();
    let states = ui.invoke(|| {
        let w = create_window(ShellWindowOptions::default());
        w.capture_pointer();
        let captured = w.has_pointer_capture();
        w.release_pointer();
        (captured, w.has_pointer_capture())
    });
    assert_eq!(states, (true, false));
    ui.invoke(dispose_window);
}

// C#: ShellWindowTests.Invalidate_ManyTimes_CoalescesIntoOneRenderAndReleasesTheFrameClock
#[test]
#[ignore = "native: needs desktop"]
fn invalidate_many_times_coalesces_into_one_render_and_releases_the_frame_clock() {
    let ui = UiThread::start();
    ui.invoke(|| {
        create_window(ShellWindowOptions::default());
    });
    wait_until_idle(&ui); // the frame of the creation (WM_SIZE) is drawn

    let (rendered_tx, rendered_rx) = channel::<()>();
    let before = ui.invoke(move || {
        let w = window();
        let count = w.render_count();
        w.subscribe_render(Box::new(move |_| {
            let _ = rendered_tx.send(());
        }));
        w.invalidate();
        w.invalidate();
        w.invalidate();
        count
    });
    rendered_rx.recv_timeout(BOUND).expect("a Render");
    wait_until_idle(&ui);

    let (after, armed) = ui.invoke(|| (window().render_count(), shell_window_fixture::clock().is_armed()));
    assert_eq!(after, before + 1);
    assert!(!armed);
    assert!(rendered_rx.try_recv().is_err(), "exactly one Render for the burst");
    ui.invoke(dispose_window);
}

// C#: ShellWindowTests.SetCursor_IsAppliedForClientHitTests (API half; the WM_SETCURSOR half is in host_shell_window.rs)
#[test]
#[ignore = "native: needs desktop"]
fn set_cursor_stores_the_cursor_and_applies_it_only_over_the_client_area() {
    let ui = UiThread::start();
    let cursor = ui.invoke(|| {
        let w = create_window(ShellWindowOptions::default());
        assert_eq!(w.cursor(), ShellCursor::Arrow);
        w.set_cursor(ShellCursor::SizeAll);
        w.cursor()
    });
    assert_eq!(cursor, ShellCursor::SizeAll);
    ui.invoke(dispose_window);
}

// new: Dispose destroys the window, raises Closed once and frees the cookie (the clock reference of the inner state is released with it)
#[test]
#[ignore = "native: needs desktop"]
fn dispose_destroys_the_window_and_frees_the_cookie() {
    let ui = UiThread::start();
    let closed = new_log();
    let log = Arc::clone(&closed);
    let outcome = ui.invoke(move || {
        let clock: Rc<dyn HostFrameClock> = Rc::new(TimerFrameClock::create(None, 0).unwrap());
        ui_loop().add_frame_clock(Rc::clone(&clock));
        let base = Rc::strong_count(&clock);
        let w = ShellWindow::create(ShellWindowOptions::default(), Rc::clone(&clock)).unwrap();
        w.subscribe(
            ShellWindowEvent::Closed,
            Box::new(move || log.lock().unwrap().push("closed".to_owned())),
        );
        let hwnd = w.hwnd();
        assert!(dialog_user32::is_window(hwnd));
        w.dispose();
        w.dispose(); // idempotent
        let after_dispose = (
            w.is_destroyed(),
            dialog_user32::is_window(hwnd),
            clock.is_armed(),
            Rc::strong_count(&clock) - base,
        );
        drop(w);
        (after_dispose, Rc::strong_count(&clock) - base)
    });
    assert_eq!(
        outcome.0,
        (true, false, false, 1),
        "destroyed, no frame pending, window state still alive"
    );
    assert_eq!(
        outcome.1, 0,
        "after the drop nothing of the window (cookie included) is left"
    );
    assert_eq!(log_snapshot(&closed), vec!["closed"], "Closed once");
}

// new: Drop is Dispose
#[test]
#[ignore = "native: needs desktop"]
fn drop_is_dispose() {
    let ui = UiThread::start();
    let closed = new_log();
    let log = Arc::clone(&closed);
    let (hwnd_alive, clock_refs) = ui.invoke(move || {
        let clock: Rc<dyn HostFrameClock> = Rc::new(TimerFrameClock::create(None, 0).unwrap());
        ui_loop().add_frame_clock(Rc::clone(&clock));
        let base = Rc::strong_count(&clock);
        let w = ShellWindow::create(ShellWindowOptions::default(), Rc::clone(&clock)).unwrap();
        w.subscribe(
            ShellWindowEvent::Closed,
            Box::new(move || log.lock().unwrap().push("closed".to_owned())),
        );
        let hwnd = w.hwnd();
        drop(w);
        (dialog_user32::is_window(hwnd), Rc::strong_count(&clock) - base)
    });
    assert!(!hwnd_alive);
    assert_eq!(clock_refs, 0);
    assert_eq!(log_snapshot(&closed), vec!["closed"]);
}

// new: ScreenToClientDip of a destroyed window only rounds and divides by the scale; Close/SetTitle/Invalidate on a destroyed window are no-ops
#[test]
#[ignore = "native: needs desktop"]
fn members_of_a_destroyed_window_are_harmless() {
    let ui = UiThread::start();
    let (title, dip, scale) = ui.invoke(|| {
        let w = create_window(ShellWindowOptions::default());
        let scale = w.dpi_scale();
        w.dispose();
        w.close();
        w.capture_pointer();
        w.invalidate();
        (
            w.set_title("x"),
            w.screen_to_client_dip(PointD { x: 10.4, y: 20.5 }),
            scale,
        )
    });
    assert_eq!(title, Ok(()));
    assert_eq!(
        dip,
        PointD {
            x: 10.0 / scale,
            y: 20.0 / scale
        }
    );
    ui.invoke(dispose_window);
}

// ---- pure ----

// new: ties-to-even like C# Math.Round, saturating cast
#[test]
fn to_pixels_table() {
    assert_eq!(to_pixels(0.5, 1.0), 0);
    assert_eq!(to_pixels(1.5, 1.0), 2);
    assert_eq!(to_pixels(2.5, 1.0), 2);
    assert_eq!(to_pixels(-0.5, 1.0), 0);
    assert_eq!(to_pixels(1024.0, 1.25), 1280);
    assert_eq!(to_pixels(1.0, 1.5), 2);
    assert_eq!(to_pixels(f64::NAN, 1.0), 0);
    assert_eq!(to_pixels(1e20, 1.0), i32::MAX);
    assert_eq!(to_pixels(-1e20, 1.0), i32::MIN);
    assert_eq!(to_pixels(400.0, 1.25), 500);
}

// new: C# `ShellWindowOptions` defaults
#[test]
fn shell_window_options_default_matches_csharp() {
    let options = ShellWindowOptions::default();
    assert_eq!(options.title, "PhotoReview");
    assert_eq!(options.width_dip, 1024.0);
    assert_eq!(options.height_dip, 720.0);
    assert_eq!(options.min_width_dip, 320.0);
    assert_eq!(options.min_height_dip, 240.0);
    assert!(!options.size_is_outer);
}

// new: R0002 / D-W8B3H-9
#[test]
fn class_name_has_the_rust_prefix() {
    assert_eq!(CLASS_NAME, "PhotoReviewRs.Shell.Window");
    assert_eq!(DEFAULT_DPI, 96);
}

// new: the `EventList` semantics of D-W8B3H-12
#[test]
fn event_list_snapshot_and_removal_semantics() {
    type Handler = Box<dyn FnMut()>;
    let list: Rc<EventList<Handler>> = Rc::new(EventList::new());
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let event = ShellWindowEvent::Closed;

    let push = |name: &'static str| -> Handler {
        let log = Rc::clone(&log);
        Box::new(move || log.borrow_mut().push(name.to_owned()))
    };
    let first = list.add(event, push("a"));
    assert_eq!(first, 1, "ids start at 1");
    let second_id = Rc::new(Cell::new(0));
    // b removes itself and c, and adds d while the raise is running
    let second = {
        let list = Rc::clone(&list);
        let log = Rc::clone(&log);
        let ids = Rc::clone(&second_id);
        let late = Rc::new(RefCell::new(Some(push("d"))));
        list.add(
            event,
            Box::new({
                let list = Rc::clone(&list);
                move || {
                    log.borrow_mut().push("b".to_owned());
                    assert!(list.remove(ids.get()), "a running handler can remove itself");
                    assert!(list.remove(3), "and a later one");
                    if let Some(handler) = late.borrow_mut().take() {
                        list.add(ShellWindowEvent::Closed, handler);
                    }
                }
            }),
        )
    };
    second_id.set(second);
    let third = list.add(event, push("c"));
    assert_eq!((second, third), (2, 3));
    list.add(ShellWindowEvent::Activated, push("other event"));

    list.raise(event, &mut |handler| handler());
    assert_eq!(
        *log.borrow(),
        vec!["a", "b"],
        "c was removed before its turn, d was added during the raise"
    );
    log.borrow_mut().clear();

    list.raise(event, &mut |handler| handler());
    assert_eq!(
        *log.borrow(),
        vec!["a", "d"],
        "the next raise sees d, not the removed b and c"
    );
    assert!(!list.remove(999), "unknown id");
    assert!(!list.remove(second), "already removed");

    // a panicking handler stays subscribed and the panic propagates
    let panicker = list.add(event, Box::new(|| panic!("boom")));
    log.borrow_mut().clear();
    let outcome = catch_unwind(AssertUnwindSafe(|| list.raise(event, &mut |handler| handler())));
    assert!(outcome.is_err());
    assert_eq!(*log.borrow(), vec!["a", "d"]);
    let again = catch_unwind(AssertUnwindSafe(|| list.raise(event, &mut |handler| handler())));
    assert!(again.is_err(), "still subscribed");
    assert!(list.remove(panicker));
    list.raise(event, &mut |handler| handler());
}

// new: Rc-based state is UI-thread only (C# VerifyAccess has no Rust twin)
#[test]
fn shell_window_is_not_send() {
    trait AmbiguousIfSend<A> {
        fn probe() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    // Compiles only while `ShellWindow` is not `Send` (otherwise the inference below is ambiguous).
    <ShellWindow as AmbiguousIfSend<_>>::probe();
}
