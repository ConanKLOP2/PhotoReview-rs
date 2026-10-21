//! Tests of W8-13c (N): the real `MessageLoop` on a real UI thread with the real OS (`Win32LoopOs`), real waitable timers, a semaphore and a message-only
//! test window. Ports of `MessageLoopFrameClockTests` (5) and `MessageLoopFilterTests` (4) of the C# repo (baseline `d92d6aae`). All `#[ignore]`d
//! ("native: needs desktop"): they need a message queue and timers, but only hidden/message-only windows are created. Every wait is bounded.
//!
//! The UI thread comes from `common/ui_thread.rs` with its own tiny dispatcher until W8-13d provides `Win32UiDispatcher`.
#![cfg(windows)]

#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, Once};
use std::thread;

use pr_platform_win::host::contracts::WindowMessage;
use pr_platform_win::host::frame_clock::base::{HostFrameClock, qpc_frequency};
use pr_platform_win::host::frame_clock::timer::{TimerFrameClock, early_tolerance};
use pr_platform_win::host::message_loop::{
    LoopOs, MessageFilter, MessageLoop, RawMessageFilter, WaitOutcome, Win32LoopOs,
};
use pr_platform_win::host::native::HWND_MESSAGE;
use pr_platform_win::host::swap_chain_clock::SwapChainFrameClock;
use pr_platform_win::win32::structs::Msg;
use ui_thread::{BOUND, UiThread, ui_loop};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateSemaphoreW, GetCurrentThreadId, ReleaseSemaphore, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostMessageW, PostThreadMessageW, RegisterClassExW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
};
use windows::core::w;

/// 10 ms, as in the C# tests.
fn period() -> i64 {
    qpc_frequency() / 100
}

const WM_APP: u32 = 0x8000;
const PROBE: u32 = WM_APP + 71;

// ---- frame clocks in the loop (C# MessageLoopFrameClockTests) ----

thread_local! {
    /// Clocks and ids of the test running on this UI thread (each test has its own UI thread).
    static TIMER: RefCell<Option<Rc<TimerFrameClock>>> = const { RefCell::new(None) };
    static SWAP: RefCell<Option<Rc<SwapChainFrameClock>>> = const { RefCell::new(None) };
    static HANDLER_ID: Cell<u64> = const { Cell::new(0) };
}

fn timer_clock() -> Rc<TimerFrameClock> {
    TIMER.with(|slot| slot.borrow().clone().expect("timer clock"))
}

/// C# `WaitForWitnessTicksAsync`: waits (bounded) until another clock on the same loop ticked `ticks` times, then removes it.
fn wait_for_witness_ticks(ui: &UiThread, ticks: u32) {
    let (done_tx, done_rx) = channel::<()>();
    ui.invoke(move || {
        let witness = Rc::new(TimerFrameClock::with_seams(None, 0, None, Some(period())).unwrap());
        let witness_dyn: Rc<dyn HostFrameClock> = Rc::clone(&witness) as Rc<dyn HostFrameClock>;
        ui_loop().add_frame_clock(Rc::clone(&witness_dyn));
        let count = Cell::new(0u32);
        let id = Rc::new(Cell::new(0u64));
        let handler_id = Rc::clone(&id);
        let handler_clock = Rc::clone(&witness);
        id.set(witness.subscribe(Box::new(move |_| {
            count.set(count.get() + 1);
            if count.get() == ticks {
                handler_clock.unsubscribe(handler_id.get());
                ui_loop().remove_frame_clock(&witness_dyn);
                handler_clock.dispose();
                let _ = done_tx.send(());
            }
        })));
    });
    done_rx
        .recv_timeout(BOUND)
        .expect("the witness clock did not tick in time");
}

// C#: MessageLoopFrameClockTests.TimerClock_InTheLoop_TicksOnTheUiThreadAtMostOncePerPeriod
#[test]
#[ignore = "native: needs desktop"]
fn timer_clock_in_the_loop_ticks_on_the_ui_thread_at_most_once_per_period() {
    let ui = UiThread::start();
    let ui_thread = ui.thread_id();
    let ticks: Arc<Mutex<Vec<(i64, i64, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let (enough_tx, enough_rx) = channel::<()>();
    let started = Arc::new(Mutex::new(0i64));

    {
        let ticks = Arc::clone(&ticks);
        let started = Arc::clone(&started);
        ui.invoke(move || {
            let clock = Rc::new(TimerFrameClock::with_seams(None, 0, None, Some(period())).unwrap());
            ui_loop().add_frame_clock(Rc::clone(&clock) as Rc<dyn HostFrameClock>);
            *started.lock().unwrap() = pr_platform_win::host::frame_clock::base::qpc_now();
            clock.subscribe(Box::new(move |tick| {
                let mut ticks = ticks.lock().unwrap();
                ticks.push((
                    tick.timestamp,
                    pr_platform_win::host::frame_clock::base::qpc_now(),
                    thread::current().id() == ui_thread,
                ));
                if ticks.len() == 20 {
                    let _ = enough_tx.send(());
                }
            }));
            TIMER.with(|slot| *slot.borrow_mut() = Some(clock));
        });
    }
    enough_rx.recv_timeout(BOUND).expect("20 ticks");

    let snapshot = ticks.lock().unwrap().clone();
    let elapsed = snapshot.last().unwrap().1 - *started.lock().unwrap();
    assert!(snapshot.iter().all(|tick| tick.2), "every tick on the UI thread");
    for pair in snapshot.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(
            gap >= period() - early_tolerance(),
            "frames {gap} ticks apart (period {})",
            period()
        );
    }
    // No burst: not more ticks than elapsed periods (+1 for the immediate first one, +1 slack as in C#).
    assert!(
        snapshot.len() as i64 <= elapsed / period() + 2,
        "{} ticks in {elapsed} ticks of time (period {})",
        snapshot.len(),
        period()
    );
    ui.dispose().unwrap();
}

// C#: MessageLoopFrameClockTests.TimerClock_LastSubscriberRemoved_StopsTickingWhileAWitnessClockKeepsGoing
#[test]
#[ignore = "native: needs desktop"]
fn timer_clock_last_subscriber_removed_stops_ticking_while_a_witness_clock_keeps_going() {
    let ui = UiThread::start();
    let (done_tx, done_rx) = channel::<()>();
    let after_unsubscribe = Arc::new(Mutex::new(-1i64));

    {
        let after_unsubscribe = Arc::clone(&after_unsubscribe);
        ui.invoke(move || {
            let subject = Rc::new(TimerFrameClock::with_seams(None, 0, None, Some(period())).unwrap());
            let witness = Rc::new(TimerFrameClock::with_seams(None, 0, None, Some(period())).unwrap());
            ui_loop().add_frame_clock(Rc::clone(&subject) as Rc<dyn HostFrameClock>);
            ui_loop().add_frame_clock(Rc::clone(&witness) as Rc<dyn HostFrameClock>);

            let own_id = Rc::new(Cell::new(0u64));
            let handler_subject = Rc::clone(&subject);
            let handler_id = Rc::clone(&own_id);
            let witness_ticks = Rc::new(Cell::new(0u32));
            let handler_witness = Rc::clone(&witness);
            let removed = Cell::new(false);
            own_id.set(subject.subscribe(Box::new(move |_| {
                if handler_subject.tick_count() >= 3 && !removed.get() {
                    removed.set(true);
                    handler_subject.unsubscribe(handler_id.get());
                    *after_unsubscribe.lock().unwrap() = handler_subject.tick_count();
                    let done_tx = done_tx.clone();
                    let witness_ticks = Rc::clone(&witness_ticks);
                    handler_witness.subscribe(Box::new(move |_| {
                        witness_ticks.set(witness_ticks.get() + 1);
                        if witness_ticks.get() == 5 {
                            let _ = done_tx.send(()); // at least 4 periods since the unsubscribe
                        }
                    }));
                }
            })));
            TIMER.with(|slot| *slot.borrow_mut() = Some(subject));
        });
    }
    done_rx.recv_timeout(BOUND).expect("witness ticks");

    let (ticks, armed, scheduled) = ui.invoke(|| {
        let subject = timer_clock();
        (subject.tick_count(), subject.is_armed(), subject.scheduled_frame())
    });
    assert_eq!(ticks, *after_unsubscribe.lock().unwrap());
    assert!(!armed);
    assert_eq!(scheduled, None);
    ui.dispose().unwrap();
}

// C#: MessageLoopFrameClockTests.SwapChainClock_InTheLoop_TicksWhenItsHandleIsSignaled_AndADisarmedHandleIsNotConsumed
#[test]
#[ignore = "native: needs desktop"]
fn swap_chain_clock_in_the_loop_ticks_when_its_handle_is_signaled_and_a_disarmed_handle_is_not_consumed() {
    let ui = UiThread::start();
    // SAFETY: plain arguments (initial 0, maximum 16, no name, default security).
    let semaphore = unsafe { CreateSemaphoreW(None, 0, 16, None) }.unwrap();
    let handle = semaphore.0 as isize;
    // A long period: the two-period fallback deadline (20 s) cannot be the source of a tick in this test.
    let long_period = qpc_frequency() * 10;
    let (first_tx, first_rx) = channel::<()>();

    ui.invoke(move || {
        let clock = Rc::new(SwapChainFrameClock::with_seams(handle, None, 0, None, Some(long_period)).unwrap());
        ui_loop().add_frame_clock(Rc::clone(&clock) as Rc<dyn HostFrameClock>);
        let id = clock.subscribe(Box::new(move |_| {
            let _ = first_tx.send(());
        }));
        HANDLER_ID.with(|slot| slot.set(id));
        SWAP.with(|slot| *slot.borrow_mut() = Some(clock));
    });
    // SAFETY: `semaphore` is a valid handle owned by this test.
    unsafe { ReleaseSemaphore(semaphore, 1, None) }.unwrap();
    first_rx.recv_timeout(BOUND).expect("first tick");
    // SAFETY: valid handle; a zero timeout only polls.
    let polled = unsafe { WaitForSingleObject(semaphore, 0) };
    assert_eq!(polled, WAIT_TIMEOUT, "the loop consumed the signal");

    ui.invoke(|| {
        let clock = SWAP.with(|slot| slot.borrow().clone()).unwrap();
        clock.unsubscribe(HANDLER_ID.with(Cell::get));
    });
    // SAFETY: valid handle.
    unsafe { ReleaseSemaphore(semaphore, 1, None) }.unwrap();
    wait_for_witness_ticks(&ui, 3);

    assert_eq!(
        ui.invoke(|| SWAP.with(|slot| slot.borrow().clone()).unwrap().tick_count()),
        1
    );
    // SAFETY: valid handle; the disarmed clock's handle was not waited on or consumed.
    let polled = unsafe { WaitForSingleObject(semaphore, 0) };
    assert_eq!(polled, WAIT_OBJECT_0);
    ui.dispose().unwrap();
    // SAFETY: the test owns the handle and the UI thread is gone.
    unsafe { CloseHandle(HANDLE(handle as *mut _)) }.unwrap();
}

// C#: MessageLoopFrameClockTests.RequestFrame_InTheLoop_ProducesExactlyOneTick
#[test]
#[ignore = "native: needs desktop"]
fn request_frame_in_the_loop_produces_exactly_one_tick() {
    let ui = UiThread::start();
    ui.invoke(|| {
        let clock = Rc::new(TimerFrameClock::with_seams(None, 0, None, Some(period())).unwrap());
        ui_loop().add_frame_clock(Rc::clone(&clock) as Rc<dyn HostFrameClock>);
        clock.request_frame();
        clock.request_frame();
        TIMER.with(|slot| *slot.borrow_mut() = Some(clock));
    });
    wait_for_witness_ticks(&ui, 3);

    assert_eq!(ui.invoke(|| timer_clock().tick_count()), 1);
    assert!(!ui.invoke(|| timer_clock().is_armed()));
    ui.dispose().unwrap();
}

// C#: MessageLoopFrameClockTests.IdleLoop_BlocksInMsgWaitInsteadOfSpinning
#[test]
#[ignore = "native: needs desktop"]
fn idle_loop_blocks_in_msg_wait_instead_of_spinning() {
    let ui = UiThread::start();
    let before = ui.invoke(|| (ui_loop().pass_count(), ui_loop().wait_count()));

    wait_for_witness_ticks(&ui, 10); // at least 9 periods of 10 ms elapsed

    let after = ui.invoke(|| (ui_loop().pass_count(), ui_loop().wait_count()));
    // Each tick wakes the loop for a few passes (the tick plus the test's messages); a spinning loop would have millions.
    let passes = after.0 - before.0;
    assert!((1..=100).contains(&passes), "{passes} passes");
    assert!(
        after.1 - before.1 >= 9,
        "loop blocked only {} times",
        after.1 - before.1
    );
    ui.dispose().unwrap();
}

// ---- message filters with a real window (C# MessageLoopFilterTests, test window class instead of ShellWindow) ----

static CLASS: Once = Once::new();

/// Per window: how many probe messages reached the window procedure, and who to tell.
static PROBES: Mutex<Option<HashMap<isize, Sender<()>>>> = Mutex::new(None);
static STRAY: AtomicU32 = AtomicU32::new(0);

extern "system" fn test_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == PROBE {
        let guard = PROBES.lock().unwrap();
        match guard.as_ref().and_then(|map| map.get(&(hwnd.0 as isize))) {
            Some(sender) => {
                let _ = sender.send(());
            }
            None => {
                STRAY.fetch_add(1, Ordering::SeqCst);
            }
        }
        return LRESULT(0);
    }
    // SAFETY: forwards the parameters this procedure received to the default procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Creates a message-only test window on the calling (UI) thread; the receiver gets one `()` per probe message the window procedure handles.
fn create_window() -> (isize, Receiver<()>) {
    // SAFETY: a null module name selects the running executable.
    let instance = unsafe { GetModuleHandleW(None) }.unwrap();
    CLASS.call_once(|| {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(test_proc),
            hInstance: instance.into(),
            lpszClassName: w!("PhotoReviewRsLoopTest"),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and outlives the call; the class name is a static wide string.
        assert_ne!(unsafe { RegisterClassExW(&class) }, 0);
    });
    // SAFETY: the class was registered above; the parent is HWND_MESSAGE, so the window is message-only (never visible); created on this thread.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("PhotoReviewRsLoopTest"),
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND(HWND_MESSAGE as *mut _)),
            None,
            Some(instance.into()),
            None,
        )
    }
    .unwrap();
    let (tx, rx) = channel();
    PROBES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(hwnd.0 as isize, tx);
    (hwnd.0 as isize, rx)
}

fn destroy_window(hwnd: isize) {
    // SAFETY: the window was created by `create_window` on this (the calling) thread.
    let _ = unsafe { DestroyWindow(HWND(hwnd as *mut _)) };
    if let Some(map) = PROBES.lock().unwrap().as_mut() {
        map.remove(&hwnd);
    }
}

fn post_probe(hwnd: isize, wparam: usize, lparam: isize) {
    // SAFETY: plain integer parameters; the window belongs to the UI thread, which is running its loop.
    unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), PROBE, WPARAM(wparam), LPARAM(lparam)) }.expect("PostMessage");
}

/// What a filter saw of one probe message (the `MSG` fields as plain integers: `Msg` itself holds a pointer and is not `Send`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    hwnd: isize,
    wparam: usize,
    lparam: isize,
    time: u32,
}

/// C# `Recorder : IRawMessageFilter`.
struct RawRecorder {
    seen: Arc<Mutex<Option<Seen>>>,
    raw_calls: Arc<AtomicU32>,
    plain_calls: Arc<AtomicU32>,
    handle_probe: bool,
    tx: Sender<()>,
}

impl MessageFilter for RawRecorder {
    fn pre_filter_message(&self, message: &WindowMessage) -> bool {
        if message.msg == PROBE {
            self.plain_calls.fetch_add(1, Ordering::SeqCst);
        }
        false
    }
    fn as_raw(&self) -> Option<&dyn RawMessageFilter> {
        Some(self)
    }
}

impl RawMessageFilter for RawRecorder {
    fn pre_filter_raw_message(&self, message: &Msg) -> bool {
        if message.message != PROBE {
            return false;
        }
        *self.seen.lock().unwrap() = Some(Seen {
            hwnd: message.hwnd.0 as isize,
            wparam: message.wParam.0,
            lparam: message.lParam.0,
            time: message.time,
        });
        self.raw_calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(());
        self.handle_probe
    }
}

/// C# `PlainRecorder : IMessageFilter`.
struct PlainRecorder {
    calls: Arc<AtomicU32>,
    last: Arc<Mutex<Option<WindowMessage>>>,
    tx: Sender<()>,
}

impl MessageFilter for PlainRecorder {
    fn pre_filter_message(&self, message: &WindowMessage) -> bool {
        if message.msg == PROBE {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some(*message);
            let _ = self.tx.send(());
        }
        false
    }
}

// C#: MessageLoopFilterTests.RawFilter_ReceivesTheFullMsg_AndIsPreferredOverThePlainCallback
#[test]
#[ignore = "native: needs desktop"]
fn raw_filter_receives_the_full_msg_and_is_preferred_over_the_plain_callback() {
    let ui = UiThread::start();
    let (hwnd, _probes) = ui.invoke(create_window);
    let (tx, seen_rx) = channel();
    let seen = Arc::new(Mutex::new(None));
    let raw_calls = Arc::new(AtomicU32::new(0));
    let plain_calls = Arc::new(AtomicU32::new(0));
    {
        let (seen, raw_calls, plain_calls) = (Arc::clone(&seen), Arc::clone(&raw_calls), Arc::clone(&plain_calls));
        ui.invoke(move || {
            let filter = Rc::new(RawRecorder {
                seen,
                raw_calls,
                plain_calls,
                handle_probe: false,
                tx,
            });
            ui_loop().add_message_filter(filter);
        });
    }

    post_probe(hwnd, 11, 22);
    seen_rx.recv_timeout(BOUND).expect("the raw filter saw the probe");

    let seen = seen.lock().unwrap().unwrap();
    assert_eq!(seen.hwnd, hwnd);
    assert_eq!(seen.wparam, 11);
    assert_eq!(seen.lparam, 22);
    assert_ne!(seen.time, 0, "MSG.time, which WindowMessage does not carry");
    assert_eq!(raw_calls.load(Ordering::SeqCst), 1);
    assert_eq!(plain_calls.load(Ordering::SeqCst), 0);
    ui.invoke(move || destroy_window(hwnd));
    ui.dispose().unwrap();
}

// C#: MessageLoopFilterTests.PlainFilter_StillReceivesWindowMessage
#[test]
#[ignore = "native: needs desktop"]
fn plain_filter_still_receives_window_message() {
    let ui = UiThread::start();
    let (hwnd, _probes) = ui.invoke(create_window);
    let (tx, seen_rx) = channel();
    let calls = Arc::new(AtomicU32::new(0));
    let last = Arc::new(Mutex::new(None));
    {
        let (calls, last) = (Arc::clone(&calls), Arc::clone(&last));
        ui.invoke(move || {
            ui_loop().add_message_filter(Rc::new(PlainRecorder { calls, last, tx }));
        });
    }

    post_probe(hwnd, 5, 6);
    seen_rx.recv_timeout(BOUND).expect("the plain filter saw the probe");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        last.lock().unwrap().unwrap(),
        WindowMessage {
            hwnd,
            msg: PROBE,
            wparam: 5,
            lparam: 6
        }
    );
    ui.invoke(move || destroy_window(hwnd));
    ui.dispose().unwrap();
}

// C#: MessageLoopFilterTests.RawFilter_ReturningTrue_StopsTranslateAndDispatch
#[test]
#[ignore = "native: needs desktop"]
fn raw_filter_returning_true_stops_translate_and_dispatch() {
    let ui = UiThread::start();
    let (hwnd, probes) = ui.invoke(create_window);
    let (tx, seen_rx) = channel();
    let id = {
        let (seen, raw_calls, plain_calls) = (
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicU32::new(0)),
            Arc::new(AtomicU32::new(0)),
        );
        ui.invoke(move || {
            ui_loop().add_message_filter(Rc::new(RawRecorder {
                seen,
                raw_calls,
                plain_calls,
                handle_probe: true,
                tx,
            }))
        })
    };

    post_probe(hwnd, 0, 0);
    seen_rx
        .recv_timeout(BOUND)
        .expect("the swallowing filter saw the probe");
    ui.invoke(|| {}); // one more dispatcher turn: the message is fully pumped by now
    assert!(
        probes.try_recv().is_err(),
        "the window procedure never saw the swallowed probe"
    );

    ui.invoke(move || ui_loop().remove_message_filter(id));
    post_probe(hwnd, 0, 0);
    probes
        .recv_timeout(BOUND)
        .expect("the message is dispatched once the filter is removed");
    ui.invoke(move || destroy_window(hwnd));
    ui.dispose().unwrap();
}

// C#: MessageLoopFilterTests.RemoveMessageFilter_StopsCalls_AndCountTracksRegistrations
#[test]
#[ignore = "native: needs desktop"]
fn remove_message_filter_stops_calls_and_count_tracks_registrations() {
    let ui = UiThread::start();
    let (hwnd, probes) = ui.invoke(create_window);
    let (tx, seen_rx) = channel();
    let raw_calls = Arc::new(AtomicU32::new(0));

    let before = ui.invoke(|| ui_loop().message_filter_count());
    let id = {
        let raw_calls = Arc::clone(&raw_calls);
        ui.invoke(move || {
            ui_loop().add_message_filter(Rc::new(RawRecorder {
                seen: Arc::new(Mutex::new(None)),
                raw_calls,
                plain_calls: Arc::new(AtomicU32::new(0)),
                handle_probe: false,
                tx,
            }))
        })
    };
    let added = ui.invoke(|| ui_loop().message_filter_count());
    ui.invoke(move || ui_loop().remove_message_filter(id));
    let removed = ui.invoke(|| ui_loop().message_filter_count());

    assert_eq!(added, before + 1);
    assert_eq!(removed, before);
    // Removed filters are not called any more: probe, then wait until the window procedure got it (so the pump has passed it).
    post_probe(hwnd, 0, 0);
    probes.recv_timeout(BOUND).expect("dispatched");
    assert_eq!(raw_calls.load(Ordering::SeqCst), 0);
    assert!(seen_rx.try_recv().is_err());
    ui.invoke(move || destroy_window(hwnd));
    ui.dispose().unwrap();
}

// ---- Win32LoopOs against the real OS ----

// new: peek, post, remove, `PostQuitMessage`, wait outcomes (runs on the test's own thread: thread messages only)
#[test]
#[ignore = "native: needs desktop"]
fn win32_loop_os_peek_post_quit_round_trip() {
    let os = Win32LoopOs;
    let mut message = Msg::default();
    // The first peek also creates this thread's message queue.
    assert!(!os.peek_message(&mut message, false));
    assert!(!os.is_message_pending());
    assert!(!MessageLoop::is_message_pending());

    // SAFETY: no arguments.
    let thread_id = unsafe { GetCurrentThreadId() };
    // SAFETY: plain integer parameters; the target thread is this one and has a queue.
    unsafe { PostThreadMessageW(thread_id, PROBE, WPARAM(11), LPARAM(22)) }.unwrap();
    assert!(os.is_message_pending());
    assert!(MessageLoop::is_message_pending());
    assert!(os.peek_message(&mut message, false), "PM_NOREMOVE keeps the message");
    assert_eq!((message.message, message.wParam.0, message.lParam.0), (PROBE, 11, 22));
    assert!(os.is_message_pending());
    let mut removed = Msg::default();
    assert!(os.peek_message(&mut removed, true));
    assert_eq!(removed.message, PROBE);
    assert!(!os.is_message_pending());
    os.translate_and_dispatch(&removed); // a thread message has no window: nothing to call, nothing to crash

    // WM_QUIT with its code.
    MessageLoop::quit(5);
    assert!(os.is_message_pending());
    assert!(os.peek_message(&mut removed, true));
    assert_eq!((removed.message, removed.wParam.0), (0x0012, 5));
    assert!(!os.is_message_pending());

    // Wait outcomes: a signaled handle, a timeout, a message, a bad handle.
    // SAFETY: plain arguments (initial 1, maximum 2, no name, default security).
    let semaphore = unsafe { CreateSemaphoreW(None, 1, 2, None) }.unwrap();
    let handle = semaphore.0 as isize;
    assert!(os.poll_handle(handle), "poll consumes the single count");
    assert!(!os.poll_handle(handle));
    // SAFETY: valid handle owned by this test.
    unsafe { ReleaseSemaphore(semaphore, 1, None) }.unwrap();
    assert_eq!(os.wait(&[handle], 5_000), WaitOutcome::Handle(0));
    assert_eq!(os.wait(&[handle], 10), WaitOutcome::Timeout);
    // SAFETY: as above.
    unsafe { PostThreadMessageW(thread_id, PROBE, WPARAM(0), LPARAM(0)) }.unwrap();
    assert_eq!(os.wait(&[handle], 5_000), WaitOutcome::Messages);
    assert!(os.peek_message(&mut removed, true));
    match os.wait(&[0x7777_7777], 10) {
        WaitOutcome::Failed(error) => assert_ne!(error.0, 0),
        other => panic!("expected a failed wait, got {other:?}"),
    }
    // SAFETY: the test owns the handle.
    unsafe { CloseHandle(semaphore) }.unwrap();
    assert_eq!(STRAY.load(Ordering::SeqCst), 0);
}
