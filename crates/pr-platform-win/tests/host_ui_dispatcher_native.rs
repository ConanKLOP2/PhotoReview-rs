//! W8-13d, the native half (kind N): the real `Win32UiDispatcher` on a real UI thread (message-only windows, nothing is shown).
//! 14 ports of `Win32UiDispatcherTests` plus the new tests of the contract. All `#[ignore = "native: needs desktop"]`.
//!
//! The UI thread runs `PumpLoop`, a test-local twin of the `MessageLoop` pass order (drain high priority, pump at most 32 messages and stop when new
//! high-priority work appears, with the queue empty run one Background item, else block). It talks to the dispatcher only through the public
//! `LoopDispatcher` seam, the same one `MessageLoop` uses. The real `MessageLoop` (W8-13c) is not used here, so the one place where the C# test goes
//! through it (`Quit`: `MessageLoop.Quit`) uses `PostQuitMessage` directly. `tests/common/ui_thread.rs` is created by 13c; this file carries its own
//! small helper so the unit does not depend on it.
#![cfg(windows)]

use std::cell::RefCell;
use std::error::Error;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, Once};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::Duration;

use pr_core::abstractions::{Log, NullLog, UiActionPanicked, UiCompletion, UiDispatcher, UiPriority, UiScheduler};
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::message_loop::MAX_MESSAGES_PER_PASS;
use pr_platform_win::host::ui_dispatcher::{CLASS_NAME, WAKE_MESSAGE, Win32UiDispatcher, dispatcher_wnd_proc};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::MsgWaitForMultipleObjectsEx;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, HWND_MESSAGE, MSG, MWMO_INPUTAVAILABLE,
    PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage, QS_ALLINPUT, RegisterClassExW, SendMessageW,
    TranslateMessage, WM_APP, WM_QUIT, WNDCLASSEXW,
};
use windows::core::PCWSTR;

const BOUND: Duration = Duration::from_secs(10);
const TEST_MESSAGE: u32 = WM_APP + 50;

// ---------------------------------------------------------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// C# `RecordingLog`: stores `warn` and `error` calls (the error text is the `Display` of the attached error).
#[derive(Default)]
struct RecordingLog {
    warns: Mutex<Vec<String>>,
    errors: Mutex<Vec<(String, Option<String>)>>,
}

impl RecordingLog {
    fn warns(&self) -> Vec<String> {
        self.warns.lock().unwrap().clone()
    }
    fn errors(&self) -> Vec<(String, Option<String>)> {
        self.errors.lock().unwrap().clone()
    }
}

impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _message: &str) {}
    fn warn(&self, message: &str) {
        self.warns.lock().unwrap().push(message.to_owned());
    }
    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        self.errors
            .lock()
            .unwrap()
            .push((message.to_owned(), error.map(ToString::to_string)));
    }
}

/// `completion.wait()` with a bound, so a regression fails instead of hanging the run.
fn wait_bounded(completion: UiCompletion) -> Result<(), UiActionPanicked> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(completion.wait());
    });
    receiver
        .recv_timeout(BOUND)
        .expect("the completion did not resolve in time")
}

fn recv_bounded<T>(receiver: &Receiver<T>) -> T {
    receiver.recv_timeout(BOUND).expect("nothing arrived in time")
}

/// The UI-thread loop: the pass order of `MessageLoop` (see the file header). Returns the `WM_QUIT` exit code.
fn pump_loop(dispatcher: &Win32UiDispatcher) -> i32 {
    loop {
        if dispatcher.has_high_priority_work() {
            dispatcher.run_high_priority_batch();
        }

        let mut pumped = 0;
        while pumped < MAX_MESSAGES_PER_PASS {
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live MSG; filter 0/0 takes any message of this thread.
            if !unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if msg.message == WM_QUIT {
                return msg.wParam.0 as i32;
            }
            // SAFETY: `msg` was filled by PeekMessageW on this thread.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            pumped += 1;
            if dispatcher.has_high_priority_work() {
                break;
            }
        }
        if dispatcher.has_high_priority_work() {
            continue;
        }
        if !message_pending() && dispatcher.try_run_background() {
            continue;
        }
        if message_pending() || dispatcher.has_high_priority_work() || dispatcher.has_background_work() {
            continue;
        }
        // SAFETY: no handles, an infinite wait for input; the wake message and every posted message end it.
        unsafe {
            MsgWaitForMultipleObjectsEx(None, u32::MAX, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
}

fn message_pending() -> bool {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a live MSG; PM_NOREMOVE leaves the message in the queue.
    unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) }.as_bool()
}

/// C# `UiThread`: a thread that owns a `Win32UiDispatcher` and runs `pump_loop`.
struct UiThread {
    dispatcher: Win32UiDispatcher,
    thread_id: ThreadId,
    exit: Receiver<i32>,
    join: Option<JoinHandle<()>>,
}

impl UiThread {
    fn start() -> Self {
        Self::start_with(Arc::new(NullLog))
    }

    fn start_with(log: Arc<dyn Log>) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (exit_tx, exit_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let dispatcher = Win32UiDispatcher::new(log).expect("dispatcher");
            ready_tx.send((dispatcher.clone(), thread::current().id())).unwrap();
            let code = pump_loop(&dispatcher);
            dispatcher.shutdown();
            let _ = exit_tx.send(code);
        });
        let (dispatcher, thread_id) = recv_bounded(&ready_rx);
        Self {
            dispatcher,
            thread_id,
            exit: exit_rx,
            join: Some(join),
        }
    }

    /// C# `ui.InvokeAsync(..)` + await: runs `action` on the UI thread and waits for it.
    fn run(&self, action: impl FnOnce() + Send + 'static) {
        wait_bounded(self.dispatcher.invoke(Box::new(action))).expect("the UI action panicked");
    }

    /// Runs `compute` on the UI thread and returns its result.
    fn run_with<T: Send + 'static>(&self, compute: impl FnOnce() -> T + Send + 'static) -> T {
        let (sender, receiver) = mpsc::channel();
        self.run(move || {
            let _ = sender.send(compute());
        });
        recv_bounded(&receiver)
    }

    fn finish(&mut self) {
        if let Some(join) = self.join.take() {
            // Harmless when the loop already ended (the dispatcher drops the post).
            // SAFETY: runs on the UI thread, where it posts WM_QUIT to that thread's own queue.
            self.dispatcher.post(Box::new(|| unsafe { PostQuitMessage(0) }));
            join.join().expect("the UI thread panicked");
        }
    }
}

impl Drop for UiThread {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Runs `body` on a fresh thread that owns a dispatcher but runs NO loop; returns what `body` returns. The dispatcher is shut down afterwards when
/// `body` did not already do it.
fn on_loopless_thread<T: Send + 'static>(
    log: Arc<dyn Log>,
    body: impl FnOnce(&Win32UiDispatcher) -> T + Send + 'static,
) -> T {
    thread::spawn(move || {
        let dispatcher = Win32UiDispatcher::new(log).expect("dispatcher");
        let result = body(&dispatcher);
        dispatcher.shutdown();
        result
    })
    .join()
    .expect("the loopless thread panicked")
}

/// xorshift64* (the role of C# `new Random(seed)`).
fn interleaved_priorities(count: usize, seed: u64) -> Vec<UiPriority> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..count)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            match (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % 4 {
                0 => UiPriority::Send,
                1 => UiPriority::Normal,
                2 => UiPriority::Render,
                _ => UiPriority::Background,
            }
        })
        .collect()
}

fn sorted_expected(posted: &[UiPriority]) -> Vec<(usize, usize)> {
    let mut expected: Vec<(usize, usize)> = posted.iter().enumerate().map(|(i, p)| (*p as usize, i)).collect();
    expected.sort_unstable();
    expected
}

// The recording test window (C# `CreateRecordingWindow`): its own message-only window class on the UI thread.
struct Recorder {
    order: Arc<Mutex<Vec<String>>>,
    post_normal_on_first: bool,
}

thread_local! {
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

const RECORDER_CLASS: &str = "PhotoReviewRs.Tests.DispatcherRecorder";

unsafe extern "system" fn recorder_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == TEST_MESSAGE {
        RECORDER.with(|recorder| {
            if let Some(recorder) = recorder.borrow().as_ref() {
                recorder.order.lock().unwrap().push(format!("M{}", wparam.0));
                if recorder.post_normal_on_first && wparam.0 == 1 {
                    let order = Arc::clone(&recorder.order);
                    Win32UiDispatcher::from_current_thread()
                        .expect("dispatcher")
                        .post(Box::new(move || order.lock().unwrap().push("N-from-M1".to_owned())));
                }
            }
        });
        return LRESULT(0);
    }
    // SAFETY: parameters forwarded unchanged from the OS call.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Creates the recording window ON THE CALLING (UI) thread; returns its handle value.
fn create_recording_window(order: Arc<Mutex<Vec<String>>>, post_normal_on_first: bool) -> isize {
    static REGISTER: Once = Once::new();
    let class_name = wide(RECORDER_CLASS);
    REGISTER.call_once(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(recorder_proc),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `class_name` outlives the call (the OS copies it); `recorder_proc` is a valid window procedure.
        assert_ne!(unsafe { RegisterClassExW(&class) }, 0);
    });
    RECORDER.with(|recorder| {
        *recorder.borrow_mut() = Some(Recorder {
            order,
            post_normal_on_first,
        })
    });
    // SAFETY: a registered class, a message-only parent, no creation data.
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(wide("").as_ptr()),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
    }
    .expect("recording window");
    hwnd.0 as isize
}

fn hwnd_of(value: isize) -> HWND {
    HWND(value as *mut core::ffi::c_void)
}

fn post_test_message(window: isize, wparam: usize) {
    // SAFETY: a plain posted message without pointers.
    unsafe { PostMessageW(Some(hwnd_of(window)), TEST_MESSAGE, WPARAM(wparam), LPARAM(0)) }.expect("PostMessage");
}

fn destroy_recording_window(ui: &UiThread, window: isize) {
    ui.run(move || {
        // SAFETY: the window was created on this (UI) thread.
        let _ = unsafe { DestroyWindow(hwnd_of(window)) };
    });
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// the 14 ports
// ---------------------------------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn priorities_thousand_interleaved_posts_from_ui_thread_run_send_normal_render_background_each_in_fifo_order() {
    // C#: Win32UiDispatcherTests.Priorities_ThousandInterleavedPostsFromUiThread_RunSendNormalRenderBackgroundEachInFifoOrder
    let ui = UiThread::start();
    let order = Arc::new(Mutex::new(Vec::<(usize, usize)>::new()));
    let posted = interleaved_priorities(1000, 7);
    let (last_tx, last_rx) = mpsc::channel();

    let dispatcher = ui.dispatcher.clone();
    let (order_in, posted_in) = (Arc::clone(&order), posted.clone());
    ui.run(move || {
        for (index, priority) in posted_in.iter().enumerate() {
            let (order, item) = (Arc::clone(&order_in), (*priority as usize, index));
            dispatcher.post_with_priority(Box::new(move || order.lock().unwrap().push(item)), *priority);
        }
        last_tx
            .send(dispatcher.invoke_with_priority(Box::new(|| {}), UiPriority::Background))
            .unwrap();
    });
    wait_bounded(recv_bounded(&last_rx)).unwrap();

    let order = order.lock().unwrap().clone();
    assert_eq!(order.len(), posted.len());
    assert_eq!(order, sorted_expected(&posted));
    let distinct: std::collections::BTreeSet<usize> = order.iter().map(|(p, _)| *p).collect();
    assert_eq!(distinct.len(), 4);
}

#[test]
#[ignore = "native: needs desktop"]
fn priorities_thousand_interleaved_posts_from_pool_thread_while_ui_busy_run_in_priority_order_with_one_wake_message() {
    // C#: Win32UiDispatcherTests.Priorities_ThousandInterleavedPostsFromPoolThreadWhileUiBusy_RunInPriorityOrderWithOneWakeMessage
    let ui = UiThread::start();
    let order = Arc::new(Mutex::new(Vec::<(usize, usize)>::new()));
    let posted = interleaved_priorities(1000, 11);
    let (busy_tx, busy_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();

    ui.dispatcher.post_with_priority(
        Box::new(move || {
            busy_tx.send(()).unwrap();
            // keep the UI thread inside one item until the worker has posted everything
            release_rx.recv_timeout(BOUND).expect("released");
        }),
        UiPriority::Normal,
    );
    recv_bounded(&busy_rx);

    let wakes_before = ui.dispatcher.wake_messages_posted();
    let (dispatcher, order_in, posted_in) = (ui.dispatcher.clone(), Arc::clone(&order), posted.clone());
    let last = thread::spawn(move || {
        for (index, priority) in posted_in.iter().enumerate() {
            let (order, item) = (Arc::clone(&order_in), (*priority as usize, index));
            dispatcher.post_with_priority(Box::new(move || order.lock().unwrap().push(item)), *priority);
        }
        dispatcher.invoke_with_priority(Box::new(|| {}), UiPriority::Background)
    })
    .join()
    .unwrap();
    let wakes_during_posting = ui.dispatcher.wake_messages_posted() - wakes_before;
    release_tx.send(()).unwrap();
    wait_bounded(last).unwrap();

    assert_eq!(*order.lock().unwrap(), sorted_expected(&posted));
    // The flag protocol: 1001 posts while the UI is busy push at most one WM_APP+1.
    assert!(
        (0..=1).contains(&wakes_during_posting),
        "wake messages: {wakes_during_posting}"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn background_runs_only_after_queued_messages_while_normal_runs_before_them() {
    // C#: Win32UiDispatcherTests.Background_RunsOnlyAfterQueuedMessages_WhileNormalRunsBeforeThem
    let ui = UiThread::start();
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let order_in = Arc::clone(&order);
    let window = ui.run_with(move || create_recording_window(order_in, false));

    let (background_tx, background_rx) = mpsc::channel();
    let (dispatcher, order_in) = (ui.dispatcher.clone(), Arc::clone(&order));
    ui.run(move || {
        let order_bg = Arc::clone(&order_in);
        background_tx
            .send(dispatcher.invoke_with_priority(
                Box::new(move || order_bg.lock().unwrap().push("B".to_owned())),
                UiPriority::Background,
            ))
            .unwrap();
        for i in 1..=3 {
            post_test_message(window, i);
        }
        let order_n = Arc::clone(&order_in);
        dispatcher.post_with_priority(
            Box::new(move || order_n.lock().unwrap().push("N".to_owned())),
            UiPriority::Normal,
        );
    });
    wait_bounded(recv_bounded(&background_rx)).unwrap();

    assert_eq!(*order.lock().unwrap(), ["N", "M1", "M2", "M3", "B"]);
    destroy_recording_window(&ui, window);
}

#[test]
#[ignore = "native: needs desktop"]
fn normal_posted_while_handling_a_message_runs_before_the_next_queued_message() {
    // C#: Win32UiDispatcherTests.Normal_PostedWhileHandlingAMessage_RunsBeforeTheNextQueuedMessage
    let ui = UiThread::start();
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let order_in = Arc::clone(&order);
    let window = ui.run_with(move || create_recording_window(order_in, true));

    let (drained_tx, drained_rx) = mpsc::channel();
    let dispatcher = ui.dispatcher.clone();
    ui.run(move || {
        post_test_message(window, 1);
        post_test_message(window, 2);
        dispatcher.post_with_priority(
            Box::new(move || {
                let _ = drained_tx.send(());
            }),
            UiPriority::Background,
        );
    });
    recv_bounded(&drained_rx);

    assert_eq!(*order.lock().unwrap(), ["M1", "N-from-M1", "M2"]);
    destroy_recording_window(&ui, window);
}

#[test]
#[ignore = "native: needs desktop"]
fn invoke_from_pool_thread_runs_on_ui_thread_and_completes() {
    // C#: Win32UiDispatcherTests.InvokeAsync_FromPoolThread_RunsOnUiThreadAndCompletes
    let ui = UiThread::start();
    let dispatcher = ui.dispatcher.clone();

    let ran_on = thread::spawn(move || {
        let (sender, receiver) = mpsc::channel();
        wait_bounded(dispatcher.invoke_with_priority(
            Box::new(move || {
                let _ = sender.send(thread::current().id());
            }),
            UiPriority::Normal,
        ))
        .unwrap();
        recv_bounded(&receiver)
    })
    .join()
    .unwrap();

    assert_eq!(ran_on, ui.thread_id);
}

#[test]
#[ignore = "native: needs desktop"]
fn post_from_pool_thread_runs_on_ui_thread() {
    // C#: Win32UiDispatcherTests.Post_FromPoolThread_RunsOnUiThread
    let ui = UiThread::start();
    let (sender, receiver) = mpsc::channel();
    let dispatcher = ui.dispatcher.clone();

    thread::spawn(move || {
        dispatcher.post(Box::new(move || {
            let _ = sender.send(thread::current().id());
        }));
    })
    .join()
    .unwrap();

    assert_eq!(recv_bounded(&receiver), ui.thread_id);
}

#[test]
#[ignore = "native: needs desktop"]
fn check_access_is_true_only_on_the_ui_thread() {
    // C#: Win32UiDispatcherTests.CheckAccess_IsTrueOnlyOnTheUiThread
    let log = Arc::new(RecordingLog::default());
    let ui = UiThread::start_with(log.clone());

    assert!(!ui.dispatcher.check_access());
    let on_ui = ui.dispatcher.clone();
    assert!(ui.run_with(move || on_ui.check_access()));
    let on_worker = ui.dispatcher.clone();
    assert!(!thread::spawn(move || on_worker.check_access()).join().unwrap());

    // C#: Assert.Throws<InvalidOperationException>(RunHighPriorityBatch). Rust never panics there: the call is a no-op that reports the error.
    assert!(matches!(
        ui.dispatcher.verify_access(),
        Err(HostError::InvalidOperation {
            message: "This Win32UiDispatcher member must be called on its UI thread."
        })
    ));
    ui.dispatcher.run_high_priority_batch();
    assert!(!ui.dispatcher.try_run_background());
    let errors = log.errors();
    assert_eq!(errors.len(), 2);
    assert!(
        errors
            .iter()
            .all(|(m, _)| m == "This Win32UiDispatcher member must be called on its UI thread.")
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn post_action_throws_is_logged_and_the_loop_keeps_running() {
    // C#: Win32UiDispatcherTests.Post_ActionThrows_IsLoggedAndTheLoopKeepsRunning
    let log = Arc::new(RecordingLog::default());
    let ui = UiThread::start_with(log.clone());

    ui.dispatcher
        .post_with_priority(Box::new(|| panic!("boom")), UiPriority::Normal);
    ui.dispatcher
        .post_with_priority(Box::new(|| panic!("boom")), UiPriority::Background);
    wait_bounded(
        ui.dispatcher
            .invoke_with_priority(Box::new(|| {}), UiPriority::Background),
    )
    .unwrap();

    let errors = log.errors();
    assert_eq!(errors.len(), 2);
    for (message, error) in &errors {
        assert_eq!(
            message,
            "Win32UiDispatcher: a posted UI action threw; the message loop keeps running"
        );
        assert_eq!(error.as_deref(), Some("UI action panicked: boom"));
    }
    assert!(ui.exit.try_recv().is_err(), "the loop is still running");
    let id = ui.run_with(|| thread::current().id());
    assert_eq!(id, ui.thread_id);
}

#[test]
#[ignore = "native: needs desktop"]
fn invoke_action_throws_faults_the_completion_without_logging() {
    // C#: Win32UiDispatcherTests.InvokeAsync_ActionThrows_FaultsTheTaskWithoutLogging
    let log = Arc::new(RecordingLog::default());
    let ui = UiThread::start_with(log.clone());

    let completion = ui
        .dispatcher
        .invoke_with_priority(Box::new(|| panic!("bad")), UiPriority::Render);

    assert_eq!(
        wait_bounded(completion),
        Err(UiActionPanicked {
            message: "bad".to_owned()
        })
    );
    assert!(log.errors().is_empty());
}

#[test]
#[ignore = "native: needs desktop"]
fn background_with_more_queued_messages_than_one_pump_pass_yields_to_every_queued_message_first() {
    // C#: Win32UiDispatcherTests.Background_WithMoreQueuedMessagesThanOnePumpPass_YieldsToEveryQueuedMessageFirst (mutant M7)
    let ui = UiThread::start();
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let order_in = Arc::clone(&order);
    let window = ui.run_with(move || create_recording_window(order_in, false));
    let message_count = MAX_MESSAGES_PER_PASS as usize * 3;

    let (background_tx, background_rx) = mpsc::channel();
    let (dispatcher, order_in) = (ui.dispatcher.clone(), Arc::clone(&order));
    ui.run(move || {
        background_tx
            .send(dispatcher.invoke_with_priority(
                Box::new(move || order_in.lock().unwrap().push("B".to_owned())),
                UiPriority::Background,
            ))
            .unwrap();
        for i in 1..=message_count {
            post_test_message(window, i);
        }
    });
    wait_bounded(recv_bounded(&background_rx)).unwrap();

    let mut expected: Vec<String> = (1..=message_count).map(|i| format!("M{i}")).collect();
    expected.push("B".to_owned());
    assert_eq!(*order.lock().unwrap(), expected);
    destroy_recording_window(&ui, window);
}

#[test]
#[ignore = "native: needs desktop"]
fn quit_ends_the_loop_with_its_exit_code() {
    // C#: Win32UiDispatcherTests.Quit_EndsTheLoopWithItsExitCode (PostQuitMessage directly: MessageLoop::quit is W8-13c)
    let mut ui = UiThread::start();

    // SAFETY: runs on the UI thread, where it posts WM_QUIT to that thread's own queue.
    ui.run(|| unsafe { PostQuitMessage(3) });

    assert_eq!(recv_bounded(&ui.exit), 3);
    ui.finish();
    assert!(ui.dispatcher.is_shut_down());
}

#[test]
#[ignore = "native: needs desktop"]
fn shutdown_cancels_pending_invokes_and_drops_posts_and_later_calls_do_not_hang() {
    // C#: Win32UiDispatcherTests.Dispose_CancelsPendingInvokesAndDropsPosts_AndLaterCallsDoNotHang
    let ran = Arc::new(Mutex::new(0));
    let (ran_post, ran_invoke) = (Arc::clone(&ran), Arc::clone(&ran));
    let (dispatcher, pending_invoke) = on_loopless_thread(Arc::new(NullLog), move |dispatcher| {
        dispatcher.post_with_priority(Box::new(move || *ran_post.lock().unwrap() += 1), UiPriority::Normal);
        let pending = dispatcher.invoke_with_priority(
            Box::new(move || *ran_invoke.lock().unwrap() += 1),
            UiPriority::Background,
        );
        dispatcher.shutdown(); // no loop is running: everything is still pending
        (dispatcher.clone(), pending)
    });

    let dropped = Err(UiActionPanicked {
        message: "UI scheduler dropped the action".to_owned(),
    });
    assert_eq!(wait_bounded(pending_invoke), dropped);
    assert!(dispatcher.is_shut_down());
    let ran_later = Arc::clone(&ran);
    assert_eq!(
        wait_bounded(
            dispatcher.invoke_with_priority(Box::new(move || *ran_later.lock().unwrap() += 1), UiPriority::Normal)
        ),
        dropped
    );
    let ran_post_after = Arc::clone(&ran);
    dispatcher.post_with_priority(
        Box::new(move || *ran_post_after.lock().unwrap() += 1),
        UiPriority::Normal,
    );
    assert_eq!(*ran.lock().unwrap(), 0);
    assert!(!dispatcher.has_high_priority_work());
    assert!(!dispatcher.has_background_work());
}

#[test]
#[ignore = "native: needs desktop"]
fn constructor_second_dispatcher_on_the_same_thread_throws() {
    // C#: Win32UiDispatcherTests.Constructor_SecondDispatcherOnTheSameThread_Throws
    let ui = UiThread::start();

    let error = ui.run_with(|| Win32UiDispatcher::new(Arc::new(NullLog)).err());

    assert_eq!(
        error,
        Some(HostError::InvalidOperation {
            message: "This thread already has a Win32UiDispatcher."
        })
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn wake_message_dispatched_by_a_foreign_modal_loop_still_runs_queued_work() {
    // C#: Win32UiDispatcherTests.WakeMessage_DispatchedByAForeignModalLoop_StillRunsQueuedWork
    let ui = UiThread::start();
    let dispatcher = ui.dispatcher.clone();

    let ran_inside_foreign_loop = ui.run_with(move || {
        let ran = Arc::new(Mutex::new(false));
        let ran_in = Arc::clone(&ran);
        dispatcher.post_with_priority(Box::new(move || *ran_in.lock().unwrap() = true), UiPriority::Normal);
        // A "modal" loop that is not ours (resize drag, TrackPopupMenu): Peek + Dispatch only.
        let deadline = std::time::Instant::now() + BOUND;
        while !*ran.lock().unwrap() && std::time::Instant::now() < deadline {
            let mut msg = MSG::default();
            // SAFETY: a live MSG; messages of this thread only.
            unsafe {
                if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                } else {
                    MsgWaitForMultipleObjectsEx(None, 100, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
                }
            }
        }
        *ran.lock().unwrap()
    });

    assert!(ran_inside_foreign_loop);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// new tests of the contract
// ---------------------------------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn post_after_shutdown_is_dropped_and_invoke_resolves_as_dropped() {
    let ran = Arc::new(Mutex::new(0));
    let (dispatcher, posted_before) = on_loopless_thread(Arc::new(NullLog), |dispatcher| {
        dispatcher.shutdown();
        (dispatcher.clone(), dispatcher.wake_messages_posted())
    });
    let ran_post = Arc::clone(&ran);
    dispatcher.post(Box::new(move || *ran_post.lock().unwrap() += 1));
    let ran_invoke = Arc::clone(&ran);
    let completion = dispatcher.invoke(Box::new(move || *ran_invoke.lock().unwrap() += 1));

    assert_eq!(
        wait_bounded(completion),
        Err(UiActionPanicked {
            message: "UI scheduler dropped the action".to_owned()
        })
    );
    assert_eq!(*ran.lock().unwrap(), 0);
    assert_eq!(
        dispatcher.wake_messages_posted(),
        posted_before,
        "no wake message after shutdown"
    );
    // shutdown is idempotent (called from a thread that is not the UI thread it is a reported no-op, not a hang)
    dispatcher.shutdown();
}

#[test]
#[ignore = "native: needs desktop"]
fn wake_post_failure_clears_the_flag_and_logs() {
    let log = Arc::new(RecordingLog::default());
    let log_in = Arc::clone(&log);
    let (warns, posted, high) = on_loopless_thread(log.clone(), move |dispatcher| {
        // Destroy the window behind the dispatcher's back: PostMessage now fails with ERROR_INVALID_WINDOW_HANDLE (1400).
        // SAFETY: the window was created on this thread.
        unsafe { DestroyWindow(hwnd_of(dispatcher.message_window())) }.expect("DestroyWindow");
        dispatcher.post(Box::new(|| {}));
        let after_first = log_in.warns().len();
        dispatcher.post(Box::new(|| {})); // the flag was cleared: this post retries (and fails again)
        (
            (after_first, log_in.warns()),
            dispatcher.wake_messages_posted(),
            dispatcher.has_high_priority_work(),
        )
    });

    assert_eq!(warns.0, 1);
    assert_eq!(warns.1.len(), 2, "the second post retried the wake");
    assert!(
        warns
            .1
            .iter()
            .all(|w| w == "Win32UiDispatcher: PostMessage(WM_APP+1) failed (error 1400)"),
        "{:?}",
        warns.1
    );
    assert_eq!(posted, 0);
    assert!(high, "the work is still queued and visible to the loop");
}

#[test]
#[ignore = "native: needs desktop"]
fn from_current_thread_is_none_after_shutdown() {
    assert!(Win32UiDispatcher::from_current_thread().is_none());
    let (before, after) = on_loopless_thread(Arc::new(NullLog), |dispatcher| {
        let before = Win32UiDispatcher::from_current_thread().map(|d| d.message_window());
        assert_eq!(before, Some(dispatcher.message_window()));
        dispatcher.shutdown();
        (before.is_some(), Win32UiDispatcher::from_current_thread().is_none())
    });
    assert!(before && after);
    // a thread that is shut down can create a new dispatcher
    thread::spawn(|| {
        let first = Win32UiDispatcher::new(Arc::new(NullLog)).unwrap();
        first.shutdown();
        let second = Win32UiDispatcher::new(Arc::new(NullLog)).unwrap();
        assert!(!second.is_shut_down());
        second.shutdown();
    })
    .join()
    .unwrap();
}

#[test]
#[ignore = "native: needs desktop"]
fn batch_slice_stops_a_drain_after_50_ms_and_the_rest_runs_on_the_next_wake() {
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let order_in = Arc::clone(&order);
    let result = on_loopless_thread(Arc::new(NullLog), move |dispatcher| {
        let slow = Arc::clone(&order_in);
        dispatcher.post(Box::new(move || {
            thread::sleep(Duration::from_millis(60)); // crosses the 50 ms slice
            slow.lock().unwrap().push("slow");
        }));
        let fast = Arc::clone(&order_in);
        dispatcher.post(Box::new(move || fast.lock().unwrap().push("fast")));
        let wakes_before = dispatcher.wake_messages_posted();

        dispatcher.on_wake_message(); // the first wake: the slow item crosses the slice, the item behind it waits
        let after_first = order_in.lock().unwrap().clone();
        let still_high = dispatcher.has_high_priority_work();
        let wakes_after = dispatcher.wake_messages_posted() - wakes_before;

        dispatcher.on_wake_message(); // the next wake runs the rest
        (
            after_first,
            still_high,
            wakes_after,
            order_in.lock().unwrap().clone(),
            dispatcher.has_high_priority_work(),
        )
    });

    assert_eq!(result.0, ["slow"]);
    assert!(result.1);
    assert_eq!(result.2, 1, "the unfinished drain asked for another wake");
    assert_eq!(result.3, ["slow", "fast"]);
    assert!(!result.4);
}

#[test]
#[ignore = "native: needs desktop"]
fn an_action_may_post_and_shutdown_from_inside() {
    let ran = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let ran_in = Arc::clone(&ran);
    let pending = on_loopless_thread(Arc::new(NullLog), move |dispatcher| {
        let (inner, ran_a) = (dispatcher.clone(), Arc::clone(&ran_in));
        dispatcher.post(Box::new(move || {
            ran_a.lock().unwrap().push("a");
            let ran_b = Arc::clone(&ran_a);
            inner.post(Box::new(move || ran_b.lock().unwrap().push("posted-inside"))); // T3: re-locks the queues
            inner.shutdown(); // T3/T7: drops the queued items while running one
        }));
        let ran_c = Arc::clone(&ran_in);
        let behind = dispatcher.invoke(Box::new(move || ran_c.lock().unwrap().push("behind")));
        dispatcher.run_high_priority_batch();
        behind
    });

    assert_eq!(
        wait_bounded(pending),
        Err(UiActionPanicked {
            message: "UI scheduler dropped the action".to_owned()
        })
    );
    assert_eq!(*ran.lock().unwrap(), ["a"]);
}

#[test]
#[ignore = "native: needs desktop"]
fn wake_message_runs_queued_work_through_the_window_procedure() {
    let ran = Arc::new(Mutex::new(false));
    let ran_in = Arc::clone(&ran);
    let result = on_loopless_thread(Arc::new(NullLog), move |dispatcher| {
        dispatcher.post(Box::new(move || *ran_in.lock().unwrap() = true));
        // SAFETY: the window belongs to this thread, so the message is handled synchronously by `dispatcher_wnd_proc`.
        unsafe {
            SendMessageW(
                hwnd_of(dispatcher.message_window()),
                WAKE_MESSAGE,
                Some(WPARAM(0)),
                Some(LPARAM(0)),
            )
        }
        .0
    });

    assert_eq!(result, 0);
    assert!(*ran.lock().unwrap());
}

#[test]
#[ignore = "native: needs desktop"]
fn dispatcher_wnd_proc_default_arm_calls_def_window_proc() {
    let result = on_loopless_thread(Arc::new(NullLog), |dispatcher| {
        let window = hwnd_of(dispatcher.message_window());
        // SAFETY: the window belongs to this thread; an unknown message goes to DefWindowProc, which returns 0 for it.
        let unknown = unsafe { SendMessageW(window, WM_APP + 99, Some(WPARAM(1)), Some(LPARAM(2))) }.0;
        // WM_GETTEXTLENGTH (0x000E) is answered by DefWindowProc: the window has no text, so 0 as well.
        // SAFETY: as above.
        let text_length = unsafe { SendMessageW(window, 0x000E, Some(WPARAM(0)), Some(LPARAM(0))) }.0;
        // The procedure is the registered one of the class.
        let proc_is_ours = {
            let direct = dispatcher_wnd_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
            // SAFETY: called on the owning thread with an unknown message.
            unsafe { direct(window, WM_APP + 98, WPARAM(0), LPARAM(0)) }.0
        };
        (unknown, text_length, proc_is_ours, CLASS_NAME)
    });

    assert_eq!((result.0, result.1, result.2), (0, 0, 0));
    assert_eq!(result.3, "PhotoReviewRs.Shell.Dispatcher");
}

#[test]
#[ignore = "native: needs desktop"]
fn check_access_is_false_from_a_worker() {
    let (dispatcher, here) = on_loopless_thread(Arc::new(NullLog), |dispatcher| {
        (dispatcher.clone(), dispatcher.check_access())
    });

    assert!(here);
    assert!(!dispatcher.check_access());
    assert!(!thread::spawn(move || dispatcher.check_access()).join().unwrap());
}
