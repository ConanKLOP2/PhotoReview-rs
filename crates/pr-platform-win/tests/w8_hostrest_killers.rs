//! CX-HOSTREST: bounded fake passes on the test thread; no sleeps or elapsed-time assertions.
//! Expectations from C# Hosting/MessageLoop.cs and ShellWindow.cs at baseline 7eaee2a2.
#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::base::{FrameHandler, FrameHandlerId, HostFrameClock};
use pr_platform_win::host::message_loop::{LoopDispatcher, LoopOs, MessageLoop, WaitOutcome};
use pr_platform_win::win32::structs::Msg;
use windows::Win32::Foundation::WPARAM;

#[derive(Default)]
struct Budget(Cell<usize>);
impl Budget {
    fn step(&self) {
        let calls = self.0.get() + 1;
        self.0.set(calls);
        assert!(calls <= 128, "mutated loop exceeded its deterministic callback budget");
    }
}

#[derive(Default)]
struct Dispatcher {
    budget: Budget,
    background: Cell<bool>,
    background_runs: Cell<usize>,
}
impl LoopDispatcher for Dispatcher {
    fn verify_access(&self) -> Result<(), HostError> {
        Ok(())
    }
    fn has_high_priority_work(&self) -> bool {
        self.budget.step();
        false
    }
    fn run_high_priority_batch(&self) {
        panic!("no high priority work queued");
    }
    fn has_background_work(&self) -> bool {
        self.background.get()
    }
    fn try_run_background(&self) -> bool {
        self.background.set(false);
        self.background_runs.set(self.background_runs.get() + 1);
        true
    }
}

#[derive(Default)]
struct Os {
    budget: Budget,
    queue: RefCell<VecDeque<Msg>>,
    dispatches: Cell<usize>,
    waits: RefCell<Vec<Vec<isize>>>,
    signaled: Cell<isize>,
}
fn quit() -> Msg {
    Msg {
        message: 0x12,
        wParam: WPARAM(37),
        ..Msg::default()
    }
}
impl LoopOs for Os {
    fn peek_message(&self, out: &mut Msg, remove: bool) -> bool {
        self.budget.step();
        let mut queue = self.queue.borrow_mut();
        let Some(front) = queue.front().copied() else {
            return false;
        };
        *out = front;
        if remove {
            queue.pop_front();
        }
        true
    }
    fn is_message_pending(&self) -> bool {
        !self.queue.borrow().is_empty()
    }
    fn translate_and_dispatch(&self, _: &Msg) {
        self.dispatches.set(self.dispatches.get() + 1);
    }
    fn poll_handle(&self, handle: isize) -> bool {
        handle != 0 && self.signaled.get() == handle
    }
    fn wait(&self, handles: &[isize], _: u32) -> WaitOutcome {
        self.budget.step();
        self.waits.borrow_mut().push(handles.to_vec());
        self.queue.borrow_mut().push_back(quit());
        WaitOutcome::Timeout
    }
    fn post_quit(&self, code: i32) {
        assert_eq!(code, 37);
        self.queue.borrow_mut().push_back(quit());
    }
}

struct Clock {
    armed: bool,
    handle: isize,
    until: i64,
    signals: Cell<usize>,
    deadlines: Cell<usize>,
}
impl Clock {
    fn new(armed: bool, handle: isize, until: i64) -> Rc<Self> {
        Rc::new(Self {
            armed,
            handle,
            until,
            signals: Cell::new(0),
            deadlines: Cell::new(0),
        })
    }
}
impl HostFrameClock for Clock {
    fn subscribe(&self, _: FrameHandler) -> FrameHandlerId {
        0
    }
    fn unsubscribe(&self, _: FrameHandlerId) {}
    fn request_frame(&self) {}
    fn is_armed(&self) -> bool {
        self.armed
    }
    fn tick_count(&self) -> i64 {
        0
    }
    fn wait_handle(&self) -> isize {
        self.handle
    }
    fn deadline_timestamp(&self) -> i64 {
        self.until
    }
    fn ticks_until_deadline(&self) -> i64 {
        self.until
    }
    fn on_signaled(&self) -> Result<bool, HostError> {
        self.signals.set(self.signals.get() + 1);
        Ok(true)
    }
    fn on_deadline(&self) -> Result<bool, HostError> {
        self.deadlines.set(self.deadlines.get() + 1);
        Ok(true)
    }
    fn dispose(&self) {}
}

// message_loop.rs:347/351/356 after_pump -> Ok(()) / delete !; :366/368 pump_messages -> None / delete !.
// C# MessageLoop pass: an idle loop waits once, then returns the exact WM_QUIT code.
#[test]
fn idle_and_background_passes_wait_then_return_quit_without_spinning() {
    for background in [false, true] {
        let dispatcher = Rc::new(Dispatcher::default());
        dispatcher.background.set(background);
        let os = Rc::new(Os::default());
        let message_loop = MessageLoop::with_os(dispatcher.clone(), os.clone()).unwrap();
        assert_eq!(message_loop.run().unwrap(), 37);
        assert_eq!(os.waits.borrow().len(), 1);
        assert_eq!(message_loop.wait_count(), 1);
        assert_eq!(dispatcher.background_runs.get(), usize::from(background));
        assert_eq!(message_loop.pass_count(), if background { 3 } else { 2 });
        // message_loop.rs:176 RunningGuard::drop -> (): a completed run must release the guard.
        os.queue.borrow_mut().push_back(quit());
        assert_eq!(message_loop.run().unwrap(), 37);
    }
}

// message_loop.rs:238/239 add_frame_clock -> () / delete !; :424/426/430/432 poll_clocks operators.
// C# MessageLoop.PollClocks: armed signals precede deadlines, disarmed clocks are ignored, same instance is unique.
#[test]
fn clocks_are_polled_once_by_identity_and_only_when_armed() {
    let os = Rc::new(Os::default());
    os.signaled.set(9);
    os.queue.borrow_mut().push_back(quit());
    let message_loop = MessageLoop::with_os(Rc::new(Dispatcher::default()), os).unwrap();
    let signaled = Clock::new(true, 9, 0);
    let deadline = Clock::new(true, 0, 0);
    let future = Clock::new(true, 8, 1);
    let disarmed = Clock::new(false, 9, 0);
    for clock in [&signaled, &signaled, &deadline, &future, &disarmed] {
        message_loop.add_frame_clock(clock.clone());
    }
    assert_eq!(message_loop.run().unwrap(), 37);
    assert_eq!((signaled.signals.get(), signaled.deadlines.get()), (1, 0));
    assert_eq!((deadline.signals.get(), deadline.deadlines.get()), (0, 1));
    assert_eq!((future.signals.get(), future.deadlines.get()), (0, 0));
    assert_eq!((disarmed.signals.get(), disarmed.deadlines.get()), (0, 0));
}

// message_loop.rs:442/447/452 wait -> Ok(()) / delete ! / handle and cap operators.
// C# MessageLoop.Wait: only armed nonzero handles, at most 63; no blocking OS calls in this test.
#[test]
fn wait_handles_exclude_disarmed_and_zero_and_stop_at_sixty_three() {
    let os = Rc::new(Os::default());
    let message_loop = MessageLoop::with_os(Rc::new(Dispatcher::default()), os.clone()).unwrap();
    message_loop.add_frame_clock(Clock::new(false, 999, 1));
    message_loop.add_frame_clock(Clock::new(true, 0, 1));
    for handle in 1..=65 {
        message_loop.add_frame_clock(Clock::new(true, handle, 1));
    }
    assert_eq!(message_loop.run().unwrap(), 37);
    assert_eq!(*os.waits.borrow(), vec![(1..=63).collect::<Vec<isize>>()]);
}

// message_loop.rs:366/368/371 pump_messages return replacements / delete ! / == -> !=.
// C# MessageLoop.PumpMessages: dispatch normal input, consume WM_QUIT before dispatching it.
#[test]
fn ordinary_input_is_dispatched_before_quit_and_quit_is_never_dispatched() {
    let os = Rc::new(Os::default());
    os.queue.borrow_mut().extend([
        Msg {
            message: 0x8001,
            ..Msg::default()
        },
        quit(),
    ]);
    let message_loop = MessageLoop::with_os(Rc::new(Dispatcher::default()), os.clone()).unwrap();
    assert_eq!(message_loop.run().unwrap(), 37);
    assert_eq!(os.dispatches.get(), 1);
    assert!(os.waits.borrow().is_empty());
}

// messages.rs:55/130 WM_ACTIVATE arm / != -> == / update_active -> () / == -> !=; :154 raise_event -> ().
// C# ShellWindowTests activation semantics, exercised synchronously on the test thread with a hidden window.
#[test]
#[ignore = "native: hidden shell window, no desktop interaction"]
fn activation_transitions_update_state_and_raise_each_event_once() {
    use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
    use pr_platform_win::host::shell_window::ShellWindow;
    let window = ShellWindow::create(Default::default(), Clock::new(false, 0, 1)).unwrap();
    let activated = Rc::new(Cell::new(0));
    let deactivated = Rc::new(Cell::new(0));
    let count = activated.clone();
    window.subscribe(
        ShellWindowEvent::Activated,
        Box::new(move || count.set(count.get() + 1)),
    );
    let count = deactivated.clone();
    window.subscribe(
        ShellWindowEvent::Deactivated,
        Box::new(move || count.set(count.get() + 1)),
    );
    send(window.hwnd(), 6, 0, 0);
    activated.set(0);
    deactivated.set(0);
    for state in [1, 1, 2, 0, 0] {
        send(window.hwnd(), 6, state, 0);
        assert_eq!(window.is_active(), state != 0);
    }
    assert_eq!((activated.get(), deactivated.get()), (1, 1));
    window.dispose();
}

fn send(hwnd: isize, message: u32, wparam: usize, lparam: isize) -> isize {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
    // SAFETY: a live hidden window owned by this test thread; messages have scalar payloads only.
    let result = unsafe {
        SendMessageW(
            HWND(hwnd as *mut _),
            message,
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        )
    };
    pr_platform_win::host::pending_panic::rethrow_pending();
    result.0
}

// messages.rs:45/111/119/120 WM_SIZE arm / update_client_size -> () / size event removal / == -> !=.
// C# ShellWindowTests: size is stored before the change event, repeated identical sizes do not raise another event.
#[test]
#[ignore = "native: hidden shell window, no desktop interaction"]
fn size_changes_store_pixels_and_suppress_duplicate_events() {
    use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
    use pr_platform_win::host::shell_window::ShellWindow;
    let window = Rc::new(ShellWindow::create(Default::default(), Clock::new(false, 0, 1)).unwrap());
    let sizes = Rc::new(RefCell::new(Vec::new()));
    let seen = sizes.clone();
    let weak = Rc::downgrade(&window);
    window.subscribe(
        ShellWindowEvent::ClientSizeChanged,
        Box::new(move || {
            seen.borrow_mut().push(weak.upgrade().unwrap().client_size_pixels());
        }),
    );
    send(window.hwnd(), 5, 0, (321 << 16) | 654);
    send(window.hwnd(), 5, 0, (321 << 16) | 654);
    send(window.hwnd(), 5, 0, (123 << 16) | 456);
    assert_eq!(*sizes.borrow(), vec![(654, 321), (456, 123)]);
    assert_eq!(window.client_size_pixels(), (456, 123));
    window.dispose();
}

// messages.rs:89/144/147 WM_CLOSE arm / raise_closing -> () / delete !.
// C# ShellWindowTests.Close_ClosingCanceled_KeepsTheWindowThenClosesWhenAllowed.
#[test]
#[ignore = "native: hidden shell window, no desktop interaction"]
fn close_cancellation_is_fresh_and_every_handler_runs() {
    use pr_platform_win::host::contracts::ShellWindowApi;
    use pr_platform_win::host::shell_window::ShellWindow;
    let window = ShellWindow::create(Default::default(), Clock::new(false, 0, 1)).unwrap();
    let cancel = Rc::new(Cell::new(true));
    let toggle = cancel.clone();
    window.subscribe_closing(Box::new(move |args| args.cancel = toggle.get()));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let copy = seen.clone();
    window.subscribe_closing(Box::new(move |args| copy.borrow_mut().push(args.cancel)));
    send(window.hwnd(), 0x10, 0, 0);
    assert!(!window.is_destroyed());
    cancel.set(false);
    send(window.hwnd(), 0x10, 0, 0);
    assert!(window.is_destroyed());
    assert_eq!(*seen.borrow(), vec![true, false]);
}
