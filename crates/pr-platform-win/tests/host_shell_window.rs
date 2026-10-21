//! Tests of W8-13f (N): `ShellWindow` message processing and DPI on a real UI thread with real (hidden, never shown or activated) windows. Ports of
//! 11 tests of `ShellWindowTests` and of `ShellWindowLifecycleLeakTests` (baseline `d92d6aae`), the `WM_SETCURSOR` half of
//! `SetCursor_IsAppliedForClientHitTests`, and the `new:` tests of the contract. Messages are sent straight to the window (no foreground APIs).
//! The native tests are `#[ignore]`d ("native: needs desktop"); the pure helpers (`low_word`, `high_word`, `min_track_size`) are not.
//! Every wait is bounded; no sleeps.
#![cfg(windows)]

#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use pr_platform_win::host::contracts::{
    PointD, ShellCursor, ShellWindowApi, ShellWindowEvent, SizeD, WindowMessage, WindowMessageHandler, new_rect,
};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
use pr_platform_win::host::icons;
use pr_platform_win::host::message_loop::MessageLoop;
use pr_platform_win::host::native::{HTCLIENT, ICON_BIG, WM_SETICON};
use pr_platform_win::host::pending_panic::rethrow_pending;
use pr_platform_win::host::shell_window::dpi::{high_word, low_word, min_track_size};
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::dialog_native::{
    GR_GDIOBJECTS, GR_USEROBJECTS, kernel32 as dialog_kernel32, user32 as dialog_user32,
};
use pr_platform_win::win32::messages::{
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, WM_ACTIVATE, WM_CLOSE, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO,
    WM_PAINT, WM_SETCURSOR,
};
use pr_platform_win::win32::structs::{Hwnd, Lresult, Point, Rect};
use shell_window_fixture::{
    Handler, SizeProbe, TEST_MESSAGE, ThrowingHandler, call_window_proc, create_window, dispose_window,
    expected_frame_size, log_snapshot, new_log, payload_text, send, send_dpi_changed, send_get_min_max_info,
    wait_until_idle, window, window_rect,
};
use ui_thread::{BOUND, TestDispatcher, UiThread};
use windows::Win32::UI::WindowsAndMessaging::GetCursor;

// ---- pure ----

// new: C# LowWord = (int)((ulong)v & 0xFFFF), HighWord = (int)(((ulong)v >> 16) & 0xFFFF)
#[test]
fn low_word_and_high_word_table() {
    let table: [(isize, i32, i32); 9] = [
        (0, 0, 0),
        (0xFFFF, 0xFFFF, 0),
        (0x1_0000, 0, 1),
        (-1, 0xFFFF, 0xFFFF),
        (isize::MIN, 0, 0),
        (isize::MAX, 0xFFFF, 0xFFFF),
        (0x1234_5678_9ABC_DEF0, 0xDEF0, 0x9ABC),
        (0x0001_0000_0000_0005, 5, 0),
        (-65536, 0, 0xFFFF),
    ];
    for (value, low, high) in table {
        assert_eq!(low_word(value), low, "low_word({value:#x})");
        assert_eq!(high_word(value), high, "high_word({value:#x})");
    }
}

// new: 400 x 300 DIP at 1.25 is 500 x 375 px; the frame adds 16 x 39
#[test]
fn min_track_size_grows_by_the_frame_and_scales_the_minimum() {
    let frame = |client: Rect| {
        Some(Rect {
            left: client.left,
            top: client.top,
            right: client.right + 16,
            bottom: client.bottom + 39,
        })
    };
    assert_eq!(min_track_size(400.0, 300.0, 1.25, &frame), Some((516, 414)));
}

// new
#[test]
fn min_track_size_returns_none_when_the_grow_call_fails() {
    assert_eq!(min_track_size(400.0, 300.0, 1.25, &|_| None), None);
}

// new: ToPixels is applied to the minimum BEFORE the frame is added
#[test]
fn min_track_size_rounds_ties_to_even_before_growing() {
    let identity = |rect: Rect| Some(rect);
    assert_eq!(min_track_size(0.5, 2.5, 1.0, &identity), Some((0, 2)));
    assert_eq!(min_track_size(1.5, 3.5, 1.0, &identity), Some((2, 4)));
    let seen = RefCell::new(None);
    let record = |rect: Rect| {
        *seen.borrow_mut() = Some(rect);
        Some(rect)
    };
    min_track_size(3.0, 5.0, 0.5, &record);
    let rect = seen.borrow().unwrap();
    assert_eq!(
        (rect.left, rect.top, rect.right, rect.bottom),
        (0, 0, 2, 2),
        "1.5 -> 2 and 2.5 -> 2"
    );
}

// ---- native ----

fn default_options() -> ShellWindowOptions {
    ShellWindowOptions::default()
}

fn start_window(options: ShellWindowOptions) -> (UiThread, Hwnd) {
    let ui = UiThread::start();
    let hwnd = ui.invoke(move || create_window(options).hwnd());
    (ui, hwnd)
}

fn finish(ui: &UiThread) {
    ui.invoke(dispose_window);
}

// C#: ShellWindowTests.Close_ClosingCanceled_KeepsTheWindow_ThenClosesWhenAllowed
#[test]
#[ignore = "native: needs desktop"]
fn close_closing_canceled_keeps_the_window_then_closes_when_allowed() {
    let ui = UiThread::start();
    let events = new_log();
    let cancel = Arc::new(AtomicBool::new(true));
    let hwnd = {
        let (events, cancel) = (Arc::clone(&events), Arc::clone(&cancel));
        ui.invoke(move || {
            let w = create_window(default_options());
            let closing_log = Arc::clone(&events);
            w.subscribe_closing(Box::new(move |args| {
                closing_log.lock().unwrap().push("closing".to_owned());
                args.cancel = cancel.load(Ordering::SeqCst);
            }));
            w.subscribe(
                ShellWindowEvent::Closed,
                Box::new(move || events.lock().unwrap().push("closed".to_owned())),
            );
            w.hwnd()
        })
    };

    ui.invoke(|| window().close());
    let still_open = ui.invoke(move || (window().is_loaded(), dialog_user32::is_window(hwnd)));
    cancel.store(false, Ordering::SeqCst);
    ui.invoke(|| window().close());
    let after_close = ui.invoke(move || (window().is_loaded(), dialog_user32::is_window(hwnd)));

    assert_eq!(still_open, (true, true));
    assert_eq!(after_close, (false, false));
    assert_eq!(log_snapshot(&events), vec!["closing", "closing", "closed"]);
    finish(&ui);
}

// C#: ShellWindowTests.PostedWmClose_LikeTheCloseButton_RaisesClosingThenClosed
#[test]
#[ignore = "native: needs desktop"]
fn posted_wm_close_like_the_close_button_raises_closing_then_closed() {
    let ui = UiThread::start();
    let events = new_log();
    let (closed_tx, closed_rx) = channel::<()>();
    let hwnd = {
        let events = Arc::clone(&events);
        ui.invoke(move || {
            let w = create_window(default_options());
            let closing_log = Arc::clone(&events);
            w.subscribe_closing(Box::new(move |_| {
                closing_log.lock().unwrap().push("closing".to_owned())
            }));
            w.subscribe(
                ShellWindowEvent::Closed,
                Box::new(move || {
                    events.lock().unwrap().push("closed".to_owned());
                    let _ = closed_tx.send(());
                }),
            );
            w.hwnd()
        })
    };

    // SAFETY: WM_CLOSE carries no pointers; posted from the test thread.
    unsafe { user32::post_message(hwnd, WM_CLOSE, 0, 0) }.expect("PostMessage");
    closed_rx.recv_timeout(BOUND).expect("Closed");

    assert_eq!(log_snapshot(&events), vec!["closing", "closed"]);
    finish(&ui);
}

// C#: ShellWindowTests.DpiChanged_SimulatedMessage_AppliesTheSuggestedRectBeforeRaisingTheEvent
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_simulated_message_applies_the_suggested_rect_before_raising_the_event() {
    let (ui, hwnd) = start_window(default_options());
    let observed: Arc<Mutex<Vec<(f64, Rect)>>> = Arc::new(Mutex::new(Vec::new()));
    let size_changes = Arc::new(AtomicU32::new(0));
    {
        let (observed, size_changes) = (Arc::clone(&observed), Arc::clone(&size_changes));
        ui.invoke(move || {
            let w = window();
            w.subscribe(
                ShellWindowEvent::DpiChanged,
                Box::new(move || {
                    let w = window();
                    observed.lock().unwrap().push((w.dpi_scale(), window_rect(w.hwnd())));
                }),
            );
            w.subscribe(
                ShellWindowEvent::ClientSizeChanged,
                Box::new(move || {
                    size_changes.fetch_add(1, Ordering::SeqCst);
                }),
            );
        });
    }
    let suggested = new_rect(40, 50, 40 + 800, 50 + 600);

    let (scale, client, client_pixels) = ui.invoke(move || {
        send_dpi_changed(hwnd, 144, suggested);
        let w = window();
        (w.dpi_scale(), w.client_size_dip(), w.client_size_pixels())
    });

    assert_eq!(scale, 1.5);
    let observed = observed.lock().unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].0, 1.5);
    assert_eq!(
        observed[0].1, suggested,
        "the suggested rect was applied before DpiChanged"
    );
    assert_eq!(
        client,
        SizeD {
            width: f64::from(client_pixels.0) / 1.5,
            height: f64::from(client_pixels.1) / 1.5
        }
    );
    assert!(size_changes.load(Ordering::SeqCst) >= 1);
    finish(&ui);
}

// C#: ShellWindowTests.DpiChanged_SamePixelSize_StillRaisesClientSizeChangedBecauseDipsChanged
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_same_pixel_size_still_raises_client_size_changed_because_dips_changed() {
    let (ui, hwnd) = start_window(default_options());
    let size_changes = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&size_changes);
    let (before, rect) = ui.invoke(move || {
        let w = window();
        w.subscribe(
            ShellWindowEvent::ClientSizeChanged,
            Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        (w.client_size_dip(), window_rect(hwnd))
    });

    let after = ui.invoke(move || {
        send_dpi_changed(hwnd, 192, rect); // the same rect: the pixel size does not change
        window().client_size_dip()
    });

    assert_eq!(size_changes.load(Ordering::SeqCst), 1);
    assert!((before.width / 2.0 - after.width).abs() < 1e-6);
    assert!((before.height / 2.0 - after.height).abs() < 1e-6);
    finish(&ui);
}

// C#: ShellWindowTests.GetMinMaxInfo_ReportsTheMinimumSizeScaledByDpi
#[test]
#[ignore = "native: needs desktop"]
fn get_min_max_info_reports_the_minimum_size_scaled_by_dpi() {
    let (ui, hwnd) = start_window(ShellWindowOptions {
        min_width_dip: 400.0,
        min_height_dip: 300.0,
        ..default_options()
    });

    let (min_track, expected) = ui.invoke(move || {
        send_dpi_changed(hwnd, 120, window_rect(hwnd));
        (send_get_min_max_info(hwnd), expected_frame_size(500, 375, 120)) // 400 x 300 DIP x 1.25
    });

    assert_eq!(min_track, expected);
    finish(&ui);
}

// C#: ShellWindowTests.Activate_Messages_RaiseActivatedAndDeactivatedOncePerChange
#[test]
#[ignore = "native: needs desktop"]
fn activate_messages_raise_activated_and_deactivated_once_per_change() {
    let (ui, hwnd) = start_window(default_options());
    let events = new_log();
    let log = Arc::clone(&events);
    let states = ui.invoke(move || {
        let w = window();
        let activated = Arc::clone(&log);
        w.subscribe(
            ShellWindowEvent::Activated,
            Box::new(move || activated.lock().unwrap().push("activated".to_owned())),
        );
        w.subscribe(
            ShellWindowEvent::Deactivated,
            Box::new(move || log.lock().unwrap().push("deactivated".to_owned())),
        );
        let mut states = Vec::new();
        send(hwnd, WM_ACTIVATE, 1, 0); // WA_ACTIVE
        states.push(w.is_active());
        send(hwnd, WM_ACTIVATE, 2, 0); // WA_CLICKACTIVE: still active
        send(hwnd, WM_ACTIVATE, 0, 0); // WA_INACTIVE
        states.push(w.is_active());
        states
    });

    assert_eq!(states, vec![true, false]);
    assert_eq!(log_snapshot(&events), vec!["activated", "deactivated"]);
    finish(&ui);
}

// C#: ShellWindowTests.MessageHandlers_RunInRegistrationOrder_AndTheFirstThatHandlesWins
#[test]
#[ignore = "native: needs desktop"]
fn message_handlers_run_in_registration_order_and_the_first_that_handles_wins() {
    let (ui, hwnd) = start_window(default_options());
    let calls = new_log();
    let log = Arc::clone(&calls);
    let result = ui.invoke(move || {
        let w = window();
        w.add_message_handler(Rc::new(Handler {
            name: "first",
            calls: Arc::clone(&log),
            handles: false,
            result: 0,
        }));
        w.add_message_handler(Rc::new(Handler {
            name: "second",
            calls: Arc::clone(&log),
            handles: true,
            result: 42,
        }));
        w.add_message_handler(Rc::new(Handler {
            name: "third",
            calls: log,
            handles: true,
            result: 7,
        }));
        send(hwnd, TEST_MESSAGE, 0, 0)
    });

    assert_eq!(result, 42);
    assert_eq!(log_snapshot(&calls), vec!["first", "second"]);
    finish(&ui);
}

// C#: ShellWindowTests.Handlers_SeeResizeAfterTheWindowUpdatedItsClientSize
#[test]
#[ignore = "native: needs desktop"]
fn handlers_see_resize_after_the_window_updated_its_client_size() {
    let (ui, hwnd) = start_window(default_options());
    let seen = Arc::new(Mutex::new(Vec::<SizeD>::new()));
    let seen_pixels = Arc::new(Mutex::new(Vec::new()));
    let raised = Arc::new(AtomicU32::new(0));
    {
        let (seen, seen_pixels, raised) = (Arc::clone(&seen), Arc::clone(&seen_pixels), Arc::clone(&raised));
        ui.invoke(move || {
            let w = window();
            w.subscribe(
                ShellWindowEvent::ClientSizeChanged,
                Box::new(move || {
                    raised.fetch_add(1, Ordering::SeqCst);
                }),
            );
            w.add_message_handler(Rc::new(SizeProbe {
                window: Rc::downgrade(&w),
                seen,
                seen_pixels,
            }));
            user32::set_window_pos(hwnd, 0, 0, 0, 900, 700, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
        });
    }

    let client = ui.invoke(|| window().client_size_dip());
    let seen = seen.lock().unwrap();
    assert_eq!(*seen.last().unwrap(), client);
    assert_eq!(raised.load(Ordering::SeqCst), 1);
    // new (wm_size_updates_the_client_size_before_handlers_run): the pixels the handler read are the ones in the message
    let pixels = seen_pixels.lock().unwrap();
    assert!(!pixels.is_empty());
    assert!(
        pixels
            .iter()
            .all(|(window_pixels, message_pixels)| window_pixels == message_pixels)
    );
    finish(&ui);
}

// C#: ShellWindowTests.ScreenToClientDip_SubtractsTheClientOriginAndDividesByDpi
#[test]
#[ignore = "native: needs desktop"]
fn screen_to_client_dip_subtracts_the_client_origin_and_divides_by_dpi() {
    let (ui, hwnd) = start_window(default_options());

    let (dip, origin) = ui.invoke(move || {
        send_dpi_changed(hwnd, 192, window_rect(hwnd));
        let origin = shell_window_fixture::client_origin(hwnd);
        (
            window().screen_to_client_dip(PointD {
                x: f64::from(origin.x + 200),
                y: f64::from(origin.y + 100),
            }),
            origin,
        )
    });

    assert_eq!(dip, PointD { x: 100.0, y: 50.0 });
    assert_ne!(origin, Point::default());
    finish(&ui);
}

// C#: ShellWindowTests.SetCursor_IsAppliedForClientHitTests (WM_SETCURSOR half)
#[test]
#[ignore = "native: needs desktop"]
fn set_cursor_is_applied_for_client_hit_tests() {
    let (ui, hwnd) = start_window(default_options());

    let (client, cursor, applied) = ui.invoke(move || {
        let w = window();
        let arrow = user32::load_system_cursor(user32::IDC_ARROW).unwrap();
        let size_all = user32::load_system_cursor(pr_platform_win::host::native::IDC_SIZEALL).unwrap();
        user32::set_cursor(arrow);
        w.set_cursor(ShellCursor::SizeAll);
        let in_client = send(hwnd, WM_SETCURSOR, hwnd as usize, HTCLIENT as isize);
        // SAFETY: no arguments; reads the calling thread's current cursor.
        let current = unsafe { GetCursor() }.0 as isize;
        (in_client, w.cursor(), current == size_all)
    });

    assert_eq!(client, 1); // the cursor was set and the default processing stopped
    assert_eq!(cursor, ShellCursor::SizeAll);
    assert!(applied, "the OS cursor is the SizeAll cursor");
    finish(&ui);
}

// new (T6): only HTCLIENT is "processed"; the frame keeps its resize cursors
#[test]
#[ignore = "native: needs desktop"]
fn wm_setcursor_outside_the_client_area_falls_through() {
    let (ui, hwnd) = start_window(default_options());

    let (result, direct, wait_applied) = ui.invoke(move || {
        let w = window();
        let arrow = user32::load_system_cursor(user32::IDC_ARROW).unwrap();
        let wait = user32::load_system_cursor(pr_platform_win::host::native::IDC_WAIT).unwrap();
        user32::set_cursor(arrow);
        w.set_cursor(ShellCursor::Wait);
        let hit_caption = 2isize; // HTCAPTION
        let result = send(hwnd, WM_SETCURSOR, hwnd as usize, hit_caption);
        // SAFETY: the parameters are the ones a WM_SETCURSOR carries; direct default processing for comparison.
        let direct = unsafe { user32::def_window_proc(hwnd, WM_SETCURSOR, hwnd as usize, hit_caption) };
        // SAFETY: no arguments; reads the calling thread's current cursor.
        let current = unsafe { GetCursor() }.0 as isize;
        (result, direct, current == wait)
    });

    assert_ne!(result, 1, "not the HTCLIENT answer");
    assert_eq!(result, direct, "the default result of DefWindowProc");
    assert!(!wait_applied, "our cursor was not applied outside the client area");
    finish(&ui);
}

// C#: ShellWindowTests.HandlerException_IsRethrownFromTheMessageLoop
#[test]
#[ignore = "native: needs desktop"]
fn handler_exception_is_rethrown_from_the_message_loop() {
    // The UI thread itself panics out of `run`, so this test owns its loop thread (the `UiThread` helper has no way to report a panicking loop).
    let (hwnd_tx, hwnd_rx) = channel::<Hwnd>();
    let (result_tx, result_rx) = channel::<Result<i32, String>>();
    let join = std::thread::spawn(move || {
        let mut scratch = pr_platform_win::win32::structs::Msg::default();
        // SAFETY: `scratch` is a live MSG; PM_NOREMOVE leaves the queue untouched (this call only creates the queue).
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                &mut scratch,
                None,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_NOREMOVE,
            )
        };
        let dispatcher = Rc::new(TestDispatcher::new());
        let message_loop = MessageLoop::new(dispatcher).unwrap();
        let clock: Rc<dyn HostFrameClock> = Rc::new(TimerFrameClock::create(None, 0).unwrap());
        message_loop.add_frame_clock(Rc::clone(&clock));
        let window = ShellWindow::create(default_options(), clock).unwrap();
        window.add_message_handler(Rc::new(ThrowingHandler));
        hwnd_tx.send(window.hwnd()).unwrap();
        let outcome = catch_unwind(AssertUnwindSafe(|| message_loop.run()));
        drop(window);
        let _ = result_tx.send(match outcome {
            Ok(result) => Ok(result.map_err(|error| error.to_string()).unwrap_or(-1)),
            Err(payload) => Err(payload_text(payload.as_ref())),
        });
    });
    let hwnd = hwnd_rx.recv_timeout(BOUND).expect("window");

    // SAFETY: the message carries no pointers; posted from the test thread.
    unsafe { user32::post_message(hwnd, TEST_MESSAGE, 0, 0) }.expect("PostMessage");

    let outcome = result_rx.recv_timeout(BOUND).expect("the loop ended");
    join.join().expect("loop thread");
    assert_eq!(outcome, Err("from WndProc".to_owned()));
}

// C#: ShellWindowTests.HandlerException_DuringSynchronousClose_IsRethrownToTheCaller
#[test]
#[ignore = "native: needs desktop"]
fn handler_exception_during_synchronous_close_is_rethrown_to_the_caller() {
    let (ui, hwnd) = start_window(default_options());
    let message = ui.invoke(|| {
        let w = window();
        w.subscribe_closing(Box::new(|_| panic!("closing failed")));
        catch_unwind(AssertUnwindSafe(|| w.close()))
            .err()
            .map(|payload| payload_text(payload.as_ref()))
    });

    assert_eq!(message.as_deref(), Some("closing failed"));
    assert!(ui.invoke(|| window().is_loaded()), "not destroyed half-way");
    assert!(ui.invoke(move || dialog_user32::is_window(hwnd)));
    finish(&ui);
}

// new (T4): a panicking Closing handler leaves the chain without default processing; the next close works
#[test]
#[ignore = "native: needs desktop"]
fn wm_close_with_a_panicking_closing_handler_keeps_the_window_and_resumes_the_panic() {
    let (ui, hwnd) = start_window(default_options());
    let first_time = Arc::new(AtomicBool::new(true));
    let second_calls = Arc::new(AtomicU32::new(0));
    {
        let (first_time, second_calls) = (Arc::clone(&first_time), Arc::clone(&second_calls));
        ui.invoke(move || {
            let w = window();
            w.subscribe_closing(Box::new(move |_| {
                if first_time.swap(false, Ordering::SeqCst) {
                    panic!("closing failed once");
                }
            }));
            w.subscribe_closing(Box::new(move |_| {
                second_calls.fetch_add(1, Ordering::SeqCst);
            }));
        });
    }

    let (message, exists, second_calls_after_panic) = {
        let second_calls = Arc::clone(&second_calls);
        ui.invoke(move || {
            let result = send(hwnd, WM_CLOSE, 0, 0);
            assert_eq!(result, 0, "the OS got 0 from the guard");
            let message = catch_unwind(AssertUnwindSafe(rethrow_pending))
                .err()
                .map(|payload| payload_text(payload.as_ref()));
            (
                message,
                dialog_user32::is_window(hwnd),
                second_calls.load(Ordering::SeqCst),
            )
        })
    };
    assert_eq!(message.as_deref(), Some("closing failed once"));
    assert!(
        exists,
        "no DefWindowProc(WM_CLOSE) and no DestroyWindow after the panic"
    );
    assert_eq!(
        second_calls_after_panic, 0,
        "a panic ends the multicast like a C# exception"
    );

    ui.invoke(|| window().close());
    assert_eq!(second_calls.load(Ordering::SeqCst), 1);
    assert!(!ui.invoke(move || dialog_user32::is_window(hwnd)));
    finish(&ui);
}

// new (T3): a handler added while a message is processed is seen from the next message
#[test]
#[ignore = "native: needs desktop"]
fn a_handler_added_during_a_message_runs_from_the_next_message() {
    struct Adder {
        window: std::rc::Weak<ShellWindow>,
        added: Cell<bool>,
        calls: shell_window_fixture::Log,
    }
    impl WindowMessageHandler for Adder {
        fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
            if message.msg == TEST_MESSAGE && !self.added.replace(true) {
                if let Some(window) = self.window.upgrade() {
                    window.add_message_handler(Rc::new(Handler {
                        name: "late",
                        calls: Arc::clone(&self.calls),
                        handles: true,
                        result: 5,
                    }));
                }
            }
            None
        }
    }

    let (ui, hwnd) = start_window(default_options());
    let calls = new_log();
    let log = Arc::clone(&calls);
    let (first, second) = ui.invoke(move || {
        let w = window();
        w.add_message_handler(Rc::new(Adder {
            window: Rc::downgrade(&w),
            added: Cell::new(false),
            calls: log,
        }));
        (send(hwnd, TEST_MESSAGE, 0, 0), send(hwnd, TEST_MESSAGE, 0, 0))
    });

    assert_eq!(first, 0, "the late handler did not see the first message");
    assert_eq!(second, 5);
    assert_eq!(log_snapshot(&calls), vec!["late"]);
    finish(&ui);
}

// new (T3): a handler may call back into the window while a message is processed
#[test]
#[ignore = "native: needs desktop"]
fn a_handler_may_set_the_title_and_close_from_inside() {
    struct Reentrant {
        window: std::rc::Weak<ShellWindow>,
    }
    impl WindowMessageHandler for Reentrant {
        fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
            if message.msg != TEST_MESSAGE {
                return None;
            }
            let window = self.window.upgrade()?;
            window.set_title("from inside").unwrap();
            window.invalidate();
            window.close();
            Some(1)
        }
    }

    let (ui, hwnd) = start_window(default_options());
    let (result, destroyed, exists) = ui.invoke(move || {
        let w = window();
        w.add_message_handler(Rc::new(Reentrant {
            window: Rc::downgrade(&w),
        }));
        let result = send(hwnd, TEST_MESSAGE, 0, 0);
        (result, w.is_destroyed(), dialog_user32::is_window(hwnd))
    });

    assert_eq!(result, 1);
    assert!(destroyed);
    assert!(!exists);
    finish(&ui);
}

// new (T1): wParam = 0 keeps the old dpi (the suggested rect is still applied)
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_with_zero_dpi_keeps_the_old_dpi() {
    let (ui, hwnd) = start_window(default_options());
    let (before, after) = ui.invoke(move || {
        let w = window();
        let before = w.dpi_scale();
        send_dpi_changed(hwnd, 0, window_rect(hwnd));
        (before, w.dpi_scale())
    });
    assert_eq!(before, after);
    finish(&ui);
}

// new (T1, D-W8B3H-19): a null lParam is "no suggested rect"
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_with_a_null_rect_skips_set_window_pos() {
    let (ui, hwnd) = start_window(default_options());
    let (scale, rect_before, rect_after) = ui.invoke(move || {
        let before = window_rect(hwnd);
        let result = call_window_proc(hwnd, WM_DPICHANGED, (144 << 16) | 144, 0);
        rethrow_pending();
        assert_eq!(result, 0);
        // a misaligned pointer is ignored as well (D-W8B3H-19)
        assert_eq!(call_window_proc(hwnd, WM_DPICHANGED, (144 << 16) | 144, 1), 0);
        rethrow_pending();
        (window().dpi_scale(), before, window_rect(hwnd))
    });
    assert_eq!(scale, 1.5, "the dpi is taken from wParam");
    assert_eq!(rect_before, rect_after, "no SetWindowPos without a rect");
    finish(&ui);
}

// new (D-W8B3H-19)
#[test]
#[ignore = "native: needs desktop"]
fn get_min_max_info_with_a_null_pointer_is_ignored() {
    let (ui, hwnd) = start_window(default_options());
    let result = ui.invoke(move || {
        let result = call_window_proc(hwnd, WM_GETMINMAXINFO, 0, 0);
        rethrow_pending();
        assert_eq!(call_window_proc(hwnd, WM_GETMINMAXINFO, 0, 3), 0, "misaligned");
        rethrow_pending();
        result
    });
    assert_eq!(result, 0);
    finish(&ui);
}

/// Handles `WM_ACTIVATE` (result 0) so that the default processing does not run.
struct SwallowActivate;

impl WindowMessageHandler for SwallowActivate {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        (message.msg == WM_ACTIVATE).then_some(0)
    }
}

// new (T8)
#[test]
#[ignore = "native: needs desktop"]
fn wm_activate_click_activate_counts_as_active() {
    let (ui, hwnd) = start_window(default_options());
    let activated = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&activated);
    let active = ui.invoke(move || {
        let w = window();
        // The handler answers WM_ACTIVATE itself, so DefWindowProc (which moves the focus and can send a nested WA_ACTIVE) does not mask the result.
        w.add_message_handler(Rc::new(SwallowActivate));
        w.subscribe(
            ShellWindowEvent::Activated,
            Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        send(hwnd, WM_ACTIVATE, 2, 0); // WA_CLICKACTIVE
        w.is_active()
    });
    assert!(active);
    assert_eq!(activated.load(Ordering::SeqCst), 1);
    finish(&ui);
}

// new (T7): the erase is swallowed and a paint request re-arms the frame clock. (A hidden window has an empty update region, so the validation
// itself cannot be observed with GetUpdateRect.)
#[test]
#[ignore = "native: needs desktop"]
fn wm_erasebkgnd_returns_one_and_wm_paint_requests_a_frame() {
    let (ui, hwnd) = start_window(default_options());
    wait_until_idle(&ui); // the creation frame is drawn
    let (erase, paint, requested) = ui.invoke(move || {
        let erase = send(hwnd, WM_ERASEBKGND, 0, 0);
        let paint = send(hwnd, WM_PAINT, 0, 0);
        (erase, paint, window().is_render_requested())
    });
    assert_eq!(erase, 1);
    assert_eq!(paint, 0);
    assert!(requested);
    finish(&ui);
}

// new
#[test]
#[ignore = "native: needs desktop"]
fn insert_message_handler_first_sees_messages_before_earlier_handlers() {
    let (ui, hwnd) = start_window(default_options());
    let calls = new_log();
    let log = Arc::clone(&calls);
    ui.invoke(move || {
        let w = window();
        w.add_message_handler(Rc::new(Handler {
            name: "added",
            calls: Arc::clone(&log),
            handles: false,
            result: 0,
        }));
        w.insert_message_handler_first(Rc::new(Handler {
            name: "first",
            calls: log,
            handles: false,
            result: 0,
        }));
        send(hwnd, TEST_MESSAGE, 0, 0);
    });
    assert_eq!(log_snapshot(&calls), vec!["first", "added"]);
    finish(&ui);
}

// new (amendment A7): the trait method `ShellWindowApi::insert_message_handler_first` puts the handler at the head, through a trait object
#[test]
#[ignore = "native: needs desktop"]
fn trait_insert_message_handler_first_sees_messages_before_earlier_handlers() {
    let (ui, hwnd) = start_window(default_options());
    let calls = new_log();
    let log = Arc::clone(&calls);
    ui.invoke(move || {
        let concrete = window();
        let w: Rc<dyn ShellWindowApi> = concrete;
        w.add_message_handler(Rc::new(Handler {
            name: "added",
            calls: Arc::clone(&log),
            handles: false,
            result: 0,
        }));
        w.insert_message_handler_first(Rc::new(Handler {
            name: "first",
            calls: Arc::clone(&log),
            handles: true,
            result: 7,
        }));
        w.insert_message_handler_first(Rc::new(Handler {
            name: "newest",
            calls: log,
            handles: false,
            result: 0,
        }));
        send(hwnd, TEST_MESSAGE, 0, 0);
    });
    assert_eq!(
        log_snapshot(&calls),
        vec!["newest", "first"],
        "the later insert is in front; a handled message stops the chain before 'added'"
    );
    finish(&ui);
}

// new (amendment A7): `ShellWindowApi::is_destroyed` is false while the window lives and true after `close` / `dispose`
#[test]
#[ignore = "native: needs desktop"]
fn trait_is_destroyed_turns_true_after_close() {
    let (ui, _hwnd) = start_window(default_options());
    let (before, after) = ui.invoke(move || {
        let w: Rc<dyn ShellWindowApi> = window();
        let before = w.is_destroyed();
        w.close();
        (before, w.is_destroyed())
    });
    assert!(!before);
    assert!(after);
    finish(&ui);
}

// new (Q3 of W8-13b): the test executable has no icon resource, so the re-application can only be asserted where one exists
#[test]
#[ignore = "native: needs desktop"]
fn icons_are_reapplied_on_dpi_change() {
    if icons::load_big(0) == 0 {
        eprintln!("skipped: this executable has no RT_GROUP_ICON resource (W8-20b smoke test covers the exe path)");
        return;
    }
    let (ui, hwnd) = start_window(default_options());
    let icon = ui.invoke(move || {
        send_dpi_changed(hwnd, 144, window_rect(hwnd));
        send(hwnd, 0x007F, ICON_BIG, 0) // WM_GETICON
    });
    assert_ne!(icon, 0);
    let _ = WM_SETICON;
    finish(&ui);
}

// C#: ShellWindowLifecycleLeakTests.OpenRunClose_FiftyTimesOnOneThread_DoesNotLeakUserOrGdiHandles
// (the C# `Show(activate: false)` is not ported: a visible window would flash on the desktop of whoever runs the tests)
#[test]
#[ignore = "native: needs desktop"]
fn open_run_close_fifty_times_on_one_thread_does_not_leak_user_or_gdi_handles() {
    const CYCLES: u32 = 50;
    let (result_tx, result_rx) = channel::<Result<((u32, u32), (u32, u32), i64), String>>();
    let join = std::thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let mut renders = cycle() + cycle(); // warm-up: window class registration, system cursors
            let before = counts();
            for _ in 0..CYCLES {
                renders += cycle();
            }
            (before, counts(), renders)
        }));
        let _ = result_tx.send(outcome.map_err(|payload| payload_text(payload.as_ref())));
    });
    let (before, after, renders) = result_rx
        .recv_timeout(BOUND)
        .expect("open/close cycles did not finish")
        .expect("a cycle failed");
    join.join().expect("cycle thread");

    assert!(renders >= i64::from(CYCLES) + 2, "only {renders} frames rendered");
    assert!(
        after.0 <= before.0 + 2,
        "USER objects grew from {} to {} over {CYCLES} cycles",
        before.0,
        after.0
    );
    assert!(
        after.1 <= before.1 + 2,
        "GDI objects grew from {} to {} over {CYCLES} cycles",
        before.1,
        after.1
    );
}

/// One full cycle on the calling thread: dispatcher, loop, frame clock, window; the first rendered frame closes the window and the `Closed` event
/// ends the loop. Returns the number of rendered frames.
fn cycle() -> i64 {
    let mut scratch = pr_platform_win::win32::structs::Msg::default();
    // SAFETY: `scratch` is a live MSG; PM_NOREMOVE leaves the queue untouched (this call only creates the queue).
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
            &mut scratch,
            None,
            0,
            0,
            windows::Win32::UI::WindowsAndMessaging::PM_NOREMOVE,
        )
    };
    let dispatcher = Rc::new(TestDispatcher::new());
    let message_loop = MessageLoop::new(dispatcher).unwrap();
    let clock = Rc::new(TimerFrameClock::create(None, 0).unwrap());
    message_loop.add_frame_clock(Rc::clone(&clock) as Rc<dyn HostFrameClock>);
    let window = ShellWindow::create(default_options(), Rc::clone(&clock) as Rc<dyn HostFrameClock>).unwrap();
    let hwnd = window.hwnd();
    window.subscribe(ShellWindowEvent::Closed, Box::new(|| MessageLoop::quit(0)));
    window.subscribe_render(Box::new(move |_| {
        send(hwnd, WM_CLOSE, 0, 0);
    }));
    window.invalidate();

    assert_eq!(message_loop.run().unwrap(), 0);
    assert!(window.is_destroyed());
    let renders = window.render_count();
    clock.dispose();
    renders
}

/// (USER objects, GDI objects) of this process.
fn counts() -> (u32, u32) {
    let process = dialog_kernel32::get_current_process();
    (
        dialog_user32::get_gui_resources(process, GR_USEROBJECTS as u32),
        dialog_user32::get_gui_resources(process, GR_GDIOBJECTS as u32),
    )
}
