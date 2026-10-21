//! Tests of W8-13c (F): the pass structure of `host::message_loop::MessageLoop` driven through a fake dispatcher, a fake `LoopOs` and fake clocks
//! (D-W8B3H-7), plus `wait_timeout_ms` and `host::pending_panic`. Nothing here touches the OS message queue; the real loop is in
//! `host_message_loop_native.rs`. The C# had no way to test the pass order without a thread; the expectations below are hand-written from
//! `MessageLoop.cs` (baseline `d92d6aae`) and the traps T1 to T10 of the contract.
#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use pr_platform_win::host::contracts::WindowMessage;
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::base::{FrameHandler, FrameHandlerId, HostFrameClock, qpc_frequency};
use pr_platform_win::host::message_loop::{
    LoopDispatcher, LoopOs, MAX_MESSAGES_PER_PASS, MAX_WAIT_HANDLES, MessageFilter, MessageLoop, RawMessageFilter,
    WaitOutcome, wait_timeout_ms,
};
use pr_platform_win::host::pending_panic;
use pr_platform_win::win32::structs::{Msg, Win32Error};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

const WM_QUIT: u32 = 0x0012;
const INFINITE: u32 = 0xFFFF_FFFF;

type Log = Rc<RefCell<Vec<String>>>;

fn new_log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn entries(log: &Log) -> Vec<String> {
    log.borrow().clone()
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn msg(number: u32) -> Msg {
    Msg {
        message: number,
        ..Msg::default()
    }
}

fn quit_msg(code: usize) -> Msg {
    Msg {
        message: WM_QUIT,
        wParam: WPARAM(code),
        ..Msg::default()
    }
}

// ---- fakes ----

#[derive(Default)]
struct FakeDispatcher {
    log: Log,
    high: Cell<u32>,
    background: Cell<u32>,
    deny: Cell<bool>,
    on_batch: RefCell<Option<Box<dyn FnMut()>>>,
    on_background: RefCell<Option<Box<dyn FnMut()>>>,
}

impl FakeDispatcher {
    fn new(log: &Log) -> Rc<Self> {
        Rc::new(Self {
            log: Rc::clone(log),
            ..Self::default()
        })
    }
}

impl LoopDispatcher for FakeDispatcher {
    fn verify_access(&self) -> Result<(), HostError> {
        if self.deny.get() {
            Err(HostError::InvalidOperation {
                message: "wrong thread",
            })
        } else {
            Ok(())
        }
    }
    fn has_high_priority_work(&self) -> bool {
        self.high.get() > 0
    }
    fn run_high_priority_batch(&self) {
        self.log.borrow_mut().push("hp-batch".to_owned());
        self.high.set(0);
        let hook = self.on_batch.borrow_mut().take();
        if let Some(mut hook) = hook {
            hook();
            *self.on_batch.borrow_mut() = Some(hook);
        }
    }
    fn has_background_work(&self) -> bool {
        self.background.get() > 0
    }
    fn try_run_background(&self) -> bool {
        if self.background.get() == 0 || self.high.get() > 0 {
            return false;
        }
        self.background.set(self.background.get() - 1);
        self.log.borrow_mut().push("bg".to_owned());
        let hook = self.on_background.borrow_mut().take();
        if let Some(mut hook) = hook {
            hook();
            *self.on_background.borrow_mut() = Some(hook);
        }
        true
    }
}

type WaitHook = Box<dyn FnMut(&[isize], u32) -> WaitOutcome>;
type DispatchHook = Box<dyn FnMut(&Msg)>;

#[derive(Default)]
struct FakeOs {
    log: Log,
    queue: RefCell<VecDeque<Msg>>,
    signaled: RefCell<HashSet<isize>>,
    waits: RefCell<Vec<(Vec<isize>, u32)>>,
    on_wait: RefCell<Option<WaitHook>>,
    on_dispatch: RefCell<Option<DispatchHook>>,
    dispatched: RefCell<Vec<u32>>,
    quits: RefCell<Vec<i32>>,
}

impl FakeOs {
    fn new(log: &Log) -> Rc<Self> {
        Rc::new(Self {
            log: Rc::clone(log),
            ..Self::default()
        })
    }
    fn push(&self, message: Msg) {
        self.queue.borrow_mut().push_back(message);
    }
    fn push_all(&self, numbers: impl IntoIterator<Item = u32>) {
        for number in numbers {
            self.push(msg(number));
        }
    }
}

impl LoopOs for FakeOs {
    fn peek_message(&self, out: &mut Msg, remove: bool) -> bool {
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
    fn translate_and_dispatch(&self, message: &Msg) {
        self.log.borrow_mut().push(format!("dispatch:{}", message.message));
        self.dispatched.borrow_mut().push(message.message);
        let hook = self.on_dispatch.borrow_mut().take();
        if let Some(mut hook) = hook {
            hook(message);
            *self.on_dispatch.borrow_mut() = Some(hook);
        }
    }
    fn poll_handle(&self, handle: isize) -> bool {
        self.log.borrow_mut().push(format!("poll:{handle}"));
        self.signaled.borrow_mut().remove(&handle)
    }
    fn wait(&self, handles: &[isize], timeout_ms: u32) -> WaitOutcome {
        self.log.borrow_mut().push("wait".to_owned());
        self.waits.borrow_mut().push((handles.to_vec(), timeout_ms));
        let hook = self.on_wait.borrow_mut().take();
        let Some(mut hook) = hook else {
            return WaitOutcome::Timeout;
        };
        let outcome = hook(handles, timeout_ms);
        *self.on_wait.borrow_mut() = Some(hook);
        outcome
    }
    fn post_quit(&self, exit_code: i32) {
        self.log.borrow_mut().push(format!("quit:{exit_code}"));
        self.quits.borrow_mut().push(exit_code);
    }
}

struct FakeClock {
    name: &'static str,
    log: Log,
    armed: Cell<bool>,
    handle: Cell<isize>,
    until: Cell<i64>,
    fail: Cell<bool>,
}

impl FakeClock {
    fn new(name: &'static str, log: &Log, handle: isize, until: i64) -> Rc<Self> {
        Rc::new(Self {
            name,
            log: Rc::clone(log),
            armed: Cell::new(true),
            handle: Cell::new(handle),
            until: Cell::new(until),
            fail: Cell::new(false),
        })
    }
    fn record(&self, what: &str) -> Result<bool, HostError> {
        self.log.borrow_mut().push(format!("clock:{}:{what}", self.name));
        if self.fail.get() {
            Err(HostError::InvalidOperation {
                message: "clock failed",
            })
        } else {
            Ok(true)
        }
    }
}

impl HostFrameClock for FakeClock {
    fn subscribe(&self, _handler: FrameHandler) -> FrameHandlerId {
        0
    }
    fn unsubscribe(&self, _id: FrameHandlerId) {}
    fn request_frame(&self) {}
    fn is_armed(&self) -> bool {
        self.armed.get()
    }
    fn tick_count(&self) -> i64 {
        0
    }
    fn wait_handle(&self) -> isize {
        self.handle.get()
    }
    fn deadline_timestamp(&self) -> i64 {
        i64::MAX
    }
    fn ticks_until_deadline(&self) -> i64 {
        self.until.get()
    }
    fn on_signaled(&self) -> Result<bool, HostError> {
        self.record("signaled")
    }
    fn on_deadline(&self) -> Result<bool, HostError> {
        self.record("deadline")
    }
    fn dispose(&self) {}
}

fn as_clock(clock: &Rc<FakeClock>) -> Rc<dyn HostFrameClock> {
    Rc::clone(clock) as Rc<dyn HostFrameClock>
}

struct Rig {
    log: Log,
    dispatcher: Rc<FakeDispatcher>,
    os: Rc<FakeOs>,
    message_loop: Rc<MessageLoop>,
}

fn rig() -> Rig {
    let log = new_log();
    let dispatcher = FakeDispatcher::new(&log);
    let os = FakeOs::new(&log);
    let message_loop = Rc::new(
        MessageLoop::with_os(
            Rc::clone(&dispatcher) as Rc<dyn LoopDispatcher>,
            Rc::clone(&os) as Rc<dyn LoopOs>,
        )
        .unwrap(),
    );
    Rig {
        log,
        dispatcher,
        os,
        message_loop,
    }
}

// ---- pass structure ----

// new (T6): high-priority drain, then the clocks, then the pump
#[test]
fn pass_order_is_dispatcher_then_clocks_then_messages() {
    let rig = rig();
    rig.dispatcher.high.set(1);
    let clock = FakeClock::new("A", &rig.log, 5, i64::MAX);
    rig.os.signaled.borrow_mut().insert(5);
    rig.message_loop.add_frame_clock(as_clock(&clock));
    rig.os.push(msg(1));
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(
        entries(&rig.log),
        strs(&["hp-batch", "poll:5", "clock:A:signaled", "dispatch:1"])
    );
    assert_eq!(rig.message_loop.pass_count(), 1);
}

// new (T6): Normal work that a message posts runs before the next input message
#[test]
fn high_priority_work_posted_by_a_message_runs_before_the_next_message() {
    let rig = rig();
    rig.os.push_all([1, 2, 3]);
    rig.os.push(quit_msg(0));
    let dispatcher = Rc::clone(&rig.dispatcher);
    *rig.os.on_dispatch.borrow_mut() = Some(Box::new(move |message| {
        if message.message == 1 {
            dispatcher.high.set(1);
        }
    }));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(
        entries(&rig.log),
        strs(&["dispatch:1", "hp-batch", "dispatch:2", "dispatch:3"])
    );
}

// new: a message flood cannot starve the clocks (T6, `>=` 32)
#[test]
fn pump_stops_after_32_messages_and_polls_the_clocks_again() {
    let rig = rig();
    assert_eq!(MAX_MESSAGES_PER_PASS, 32);
    let clock = FakeClock::new("A", &rig.log, 5, i64::MAX);
    rig.message_loop.add_frame_clock(as_clock(&clock));
    rig.os.push_all(1001..=1100); // not 1..=100: message 18 is WM_QUIT
    rig.os.push(quit_msg(7));

    assert_eq!(rig.message_loop.run().unwrap(), 7);

    let mut expected = Vec::new();
    for chunk in (1001..=1100u32).collect::<Vec<_>>().chunks(32) {
        expected.push("poll:5".to_owned());
        expected.extend(chunk.iter().map(|number| format!("dispatch:{number}")));
    }
    assert_eq!(entries(&rig.log), expected);
    assert_eq!(rig.message_loop.pass_count(), 4);
    assert_eq!(rig.message_loop.wait_count(), 0);
}

// new (T5, C# mutant M7): Background runs only with an empty queue, and a message posted during it is pumped before the next item
#[test]
fn background_runs_only_when_no_message_is_pending() {
    let rig = rig();
    rig.dispatcher.background.set(2);
    rig.os.push_all(1001..=1040); // 8 are still pending after the first pump
    let os = Rc::clone(&rig.os);
    let mut first = true;
    *rig.dispatcher.on_background.borrow_mut() = Some(Box::new(move || {
        if first {
            first = false;
            os.push(msg(99)); // posted during the Background item
        } else {
            os.push(quit_msg(0));
        }
    }));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    let log = entries(&rig.log);
    let position = |what: &str| log.iter().position(|entry| entry == what).unwrap();
    assert!(position("dispatch:1040") < position("bg"), "{log:?}");
    assert!(position("bg") < position("dispatch:99"), "{log:?}");
    let second_bg = log.iter().rposition(|entry| entry == "bg").unwrap();
    assert!(position("dispatch:99") < second_bg, "{log:?}");
    assert_eq!(log.iter().filter(|entry| *entry == "bg").count(), 2);
    assert_eq!(rig.message_loop.wait_count(), 0);
}

// new: one Background item per pass
#[test]
fn background_runs_one_item_per_pass() {
    let rig = rig();
    rig.dispatcher.background.set(3);
    *rig.dispatcher.on_background.borrow_mut() = Some(Box::new({
        let os = Rc::clone(&rig.os);
        let dispatcher = Rc::clone(&rig.dispatcher);
        move || {
            if dispatcher.background.get() == 0 {
                os.push(quit_msg(0));
            }
        }
    }));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(entries(&rig.log), strs(&["bg", "bg", "bg"]));
    assert_eq!(rig.message_loop.pass_count(), 4); // three items, then the pass that sees WM_QUIT
}

// new (T4): a pending message means the pass repeats instead of blocking
#[test]
fn wait_is_skipped_when_a_message_is_pending() {
    let rig = rig();
    rig.os.push_all(1001..=1033); // the pump leaves the last message pending
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(rig.message_loop.wait_count(), 0);
    assert!(rig.os.waits.borrow().is_empty());
    assert_eq!(rig.message_loop.pass_count(), 2);
}

// new: the idle loop sleeps forever and keeps the pass count tiny (T8)
#[test]
fn idle_loop_waits_infinitely_without_armed_clocks() {
    let rig = rig();
    let unarmed = FakeClock::new("U", &rig.log, 12, 1);
    unarmed.armed.set(false);
    rig.message_loop.add_frame_clock(as_clock(&unarmed));
    let os = Rc::clone(&rig.os);
    *rig.os.on_wait.borrow_mut() = Some(Box::new(move |_, _| {
        os.push(quit_msg(3));
        WaitOutcome::Messages
    }));

    assert_eq!(rig.message_loop.run().unwrap(), 3);

    assert_eq!(*rig.os.waits.borrow(), vec![(Vec::<isize>::new(), INFINITE)]);
    assert_eq!(rig.message_loop.wait_count(), 1);
    assert_eq!(rig.message_loop.pass_count(), 2);
}

// new: the nearest deadline of the armed clocks (also those without a handle), the handles of the armed ones only
#[test]
fn wait_uses_the_nearest_deadline_and_only_armed_handles() {
    let rig = rig();
    let freq = qpc_frequency();
    let a = FakeClock::new("A", &rig.log, 10, freq); // 1000 ms
    let b = FakeClock::new("B", &rig.log, 0, freq / 2); // 500 ms, no handle
    let c = FakeClock::new("C", &rig.log, 12, 1);
    c.armed.set(false);
    let d = FakeClock::new("D", &rig.log, 11, i64::MAX);
    for clock in [&a, &b, &c, &d] {
        rig.message_loop.add_frame_clock(as_clock(clock));
    }
    let os = Rc::clone(&rig.os);
    *rig.os.on_wait.borrow_mut() = Some(Box::new(move |_, _| {
        os.push(quit_msg(0));
        WaitOutcome::Timeout
    }));

    rig.message_loop.run().unwrap();

    assert_eq!(*rig.os.waits.borrow(), vec![(vec![10, 11], 500)]);
}

// new (T1): the 64th armed clock counts for the deadline but is not waited on
#[test]
fn wait_collects_at_most_63_handles() {
    let rig = rig();
    let freq = qpc_frequency();
    let mut clocks = Vec::new();
    for handle in 1..=70isize {
        let until = if handle == 70 { freq / 1000 } else { freq * 10 };
        let clock = FakeClock::new("N", &rig.log, handle, until);
        rig.message_loop.add_frame_clock(as_clock(&clock));
        clocks.push(clock);
    }
    let os = Rc::clone(&rig.os);
    *rig.os.on_wait.borrow_mut() = Some(Box::new(move |_, _| {
        os.push(quit_msg(0));
        WaitOutcome::Timeout
    }));

    rig.message_loop.run().unwrap();

    let waits = rig.os.waits.borrow();
    assert_eq!(waits.len(), 1);
    assert_eq!(MAX_WAIT_HANDLES, 63);
    assert_eq!(waits[0].0, (1..=63).collect::<Vec<isize>>());
    assert_eq!(waits[0].1, 1); // the 70th clock (not waited on) still sets the deadline
}

// new: `Handle(i)` signals the owner i
#[test]
fn wait_handle_result_signals_its_owner() {
    let rig = rig();
    let a = FakeClock::new("A", &rig.log, 1, i64::MAX);
    let b = FakeClock::new("B", &rig.log, 2, i64::MAX);
    rig.message_loop.add_frame_clock(as_clock(&a));
    rig.message_loop.add_frame_clock(as_clock(&b));
    let os = Rc::clone(&rig.os);
    *rig.os.on_wait.borrow_mut() = Some(Box::new(move |_, _| {
        os.push(quit_msg(0));
        WaitOutcome::Handle(1)
    }));

    rig.message_loop.run().unwrap();

    let log = entries(&rig.log);
    assert_eq!(
        log.iter().filter(|entry| entry.ends_with(":signaled")).count(),
        1,
        "{log:?}"
    );
    assert!(log.contains(&"clock:B:signaled".to_owned()));
}

// new
#[test]
fn wait_failure_is_an_error() {
    let rig = rig();
    *rig.os.on_wait.borrow_mut() = Some(Box::new(|_, _| WaitOutcome::Failed(Win32Error(6))));

    let error = rig.message_loop.run().unwrap_err();

    assert_eq!(
        error,
        HostError::Win32 {
            operation: "MsgWaitForMultipleObjectsEx failed",
            error: Win32Error(6),
        }
    );
    // `running` was cleared: the loop can run again.
    rig.os.on_wait.borrow_mut().take();
    rig.os.push(quit_msg(2));
    assert_eq!(rig.message_loop.run().unwrap(), 2);
}

// new: a clock error ends the run
#[test]
fn a_failing_clock_ends_run_with_its_error() {
    let rig = rig();
    let clock = FakeClock::new("A", &rig.log, 5, i64::MAX);
    clock.fail.set(true);
    rig.os.signaled.borrow_mut().insert(5);
    rig.message_loop.add_frame_clock(as_clock(&clock));

    assert_eq!(
        rig.message_loop.run().unwrap_err(),
        HostError::InvalidOperation {
            message: "clock failed"
        }
    );
}

// new (T3)
#[test]
fn quit_message_ends_run_with_wparam_as_exit_code() {
    for (wparam, expected) in [
        (0usize, 0),
        (5, 5),
        ((-3i32) as isize as usize, -3),
        (0x1_0000_0005, 5), // `(int)message.WParam` truncates
    ] {
        let rig = rig();
        rig.os.push(msg(1));
        rig.os.push(quit_msg(wparam));
        rig.os.push(msg(2)); // behind WM_QUIT: not reached
        assert_eq!(rig.message_loop.run().unwrap(), expected, "wParam {wparam:#x}");
        assert_eq!(*rig.os.dispatched.borrow(), vec![1]);
    }
}

// new (T2, T9): a second `run` from inside a callback is an error, and `running` is cleared after an `Ok`, an `Err` and a resumed panic
#[test]
fn run_is_not_reentrant() {
    let rig = rig();
    let nested: Rc<RefCell<Option<Result<i32, HostError>>>> = Rc::new(RefCell::new(None));
    let weak: Weak<MessageLoop> = Rc::downgrade(&rig.message_loop);
    *rig.dispatcher.on_batch.borrow_mut() = Some(Box::new({
        let nested = Rc::clone(&nested);
        move || {
            if let Some(message_loop) = weak.upgrade() {
                *nested.borrow_mut() = Some(message_loop.run());
            }
        }
    }));
    rig.dispatcher.high.set(1);
    rig.os.push(quit_msg(4));
    assert_eq!(rig.message_loop.run().unwrap(), 4);
    assert_eq!(
        nested.borrow_mut().take().unwrap(),
        Err(HostError::InvalidOperation {
            message: "MessageLoop.Run is not re-entrant."
        })
    );

    // After a normal end the loop runs again; after a resumed panic too.
    rig.os.push(msg(1));
    rig.os.push(quit_msg(1));
    *rig.os.on_dispatch.borrow_mut() = Some(Box::new(|_| pending_panic::capture(Box::new("boom"))));
    let outcome = catch_unwind(AssertUnwindSafe(|| rig.message_loop.run()));
    let payload = outcome.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"boom"));
    rig.os.on_dispatch.borrow_mut().take();
    rig.os.queue.borrow_mut().clear();
    rig.os.push(quit_msg(9));
    assert_eq!(rig.message_loop.run().unwrap(), 9);
    assert!(!pending_panic::has_pending());
}

// new: the access check of the dispatcher guards `new`/`run`/`run_nested`
#[test]
fn a_dispatcher_that_denies_access_fails_new() {
    let log = new_log();
    let dispatcher = FakeDispatcher::new(&log);
    dispatcher.deny.set(true);
    let os = FakeOs::new(&log);
    let result = MessageLoop::with_os(dispatcher as Rc<dyn LoopDispatcher>, os as Rc<dyn LoopOs>);
    assert_eq!(
        result.unwrap_err(),
        HostError::InvalidOperation {
            message: "wrong thread"
        }
    );
}

// new (T3, lesson #486)
#[test]
fn run_nested_ends_when_done_and_reposts_quit() {
    let rig = rig();
    rig.os.push(msg(1));
    rig.os.push(quit_msg(9));
    let seen_by_claim = Cell::new(0u32);
    let mut claim = |_: &mut Msg| {
        seen_by_claim.set(seen_by_claim.get() + 1);
        false
    };

    let finished = rig.message_loop.run_nested(&mut || false, Some(&mut claim)).unwrap();

    assert!(!finished);
    assert_eq!(*rig.os.quits.borrow(), vec![9]);
    assert_eq!(seen_by_claim.get(), 1, "WM_QUIT never reaches the claim");
    assert_eq!(*rig.os.dispatched.borrow(), vec![1]);
    assert!(
        rig.os.queue.borrow().is_empty(),
        "WM_QUIT was consumed by the nested pump and posted again"
    );
}

// new
#[test]
fn run_nested_done_returns_true() {
    let rig = rig();
    // Already done: no pass at all.
    assert!(rig.message_loop.run_nested(&mut || true, None).unwrap());
    assert_eq!(rig.message_loop.pass_count(), 0);

    // Done after the first pump: the rest of the pass is skipped (no Background item, no wait).
    rig.os.push_all([1, 2]);
    rig.dispatcher.background.set(1);
    let calls = Cell::new(0);
    let mut is_done = || {
        calls.set(calls.get() + 1);
        calls.get() >= 2
    };
    assert!(rig.message_loop.run_nested(&mut is_done, None).unwrap());
    assert_eq!(*rig.os.dispatched.borrow(), vec![1, 2]);
    assert_eq!(rig.message_loop.wait_count(), 0);
    assert_eq!(rig.dispatcher.background.get(), 1);
    assert!(rig.os.quits.borrow().is_empty());

    // Nested with the outer loop running is allowed: `run_nested` has no re-entrancy check.
    let weak: Weak<MessageLoop> = Rc::downgrade(&rig.message_loop);
    let nested_result = Rc::new(Cell::new(None));
    *rig.dispatcher.on_batch.borrow_mut() = Some(Box::new({
        let nested_result = Rc::clone(&nested_result);
        move || {
            if let Some(message_loop) = weak.upgrade() {
                nested_result.set(message_loop.run_nested(&mut || true, None).ok());
            }
        }
    }));
    rig.dispatcher.high.set(1);
    rig.os.push(quit_msg(0));
    rig.message_loop.run().unwrap();
    assert_eq!(nested_result.get(), Some(true));
}

// new
#[test]
fn nested_claim_sees_the_message_before_the_filters_and_blocks_dispatch() {
    let rig = rig();
    let filter = Rc::new(RecordingFilter::new(&rig.log, "F", false));
    rig.message_loop.add_message_filter(filter as Rc<dyn MessageFilter>);
    rig.os.push_all([1, 2, 3]);
    rig.os.push(quit_msg(0));
    let log = Rc::clone(&rig.log);
    let mut claim = move |message: &mut Msg| {
        log.borrow_mut().push(format!("claim:{}", message.message));
        message.message == 2
    };

    assert!(!rig.message_loop.run_nested(&mut || false, Some(&mut claim)).unwrap());

    assert_eq!(
        entries(&rig.log),
        strs(&[
            "claim:1",
            "F:plain:1",
            "dispatch:1",
            "claim:2",
            "claim:3",
            "F:plain:3",
            "dispatch:3",
            "quit:0",
        ])
    );
}

// new (T9): a panic captured while the claim handled the message is resumed
#[test]
fn nested_claim_resumes_a_captured_panic() {
    let rig = rig();
    rig.os.push_all([1, 2]);
    let mut claim = |_: &mut Msg| {
        pending_panic::capture(Box::new(String::from("from claim")));
        true
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        rig.message_loop.run_nested(&mut || false, Some(&mut claim))
    }));
    let payload = outcome.unwrap_err();
    assert_eq!(payload.downcast_ref::<String>().map(String::as_str), Some("from claim"));
    assert!(rig.os.dispatched.borrow().is_empty());
    assert!(!pending_panic::has_pending());
}

struct RecordingFilter {
    name: &'static str,
    log: Log,
    result: Cell<bool>,
    seen: RefCell<Vec<WindowMessage>>,
}

impl RecordingFilter {
    fn new(log: &Log, name: &'static str, result: bool) -> Self {
        Self {
            name,
            log: Rc::clone(log),
            result: Cell::new(result),
            seen: RefCell::new(Vec::new()),
        }
    }
}

impl MessageFilter for RecordingFilter {
    fn pre_filter_message(&self, message: &WindowMessage) -> bool {
        self.log
            .borrow_mut()
            .push(format!("{}:plain:{}", self.name, message.msg));
        self.seen.borrow_mut().push(*message);
        self.result.get()
    }
}

/// A filter that implements both callbacks: the loop must call only the raw one.
struct RawFilter {
    log: Log,
    result: bool,
    plain_calls: Cell<u32>,
    raw: RefCell<Vec<Msg>>,
}

impl MessageFilter for RawFilter {
    fn pre_filter_message(&self, _message: &WindowMessage) -> bool {
        self.plain_calls.set(self.plain_calls.get() + 1);
        false
    }
    fn as_raw(&self) -> Option<&dyn RawMessageFilter> {
        Some(self)
    }
}

impl RawMessageFilter for RawFilter {
    fn pre_filter_raw_message(&self, message: &Msg) -> bool {
        self.log.borrow_mut().push(format!("raw:{}", message.message));
        self.raw.borrow_mut().push(*message);
        self.result
    }
}

/// Removes itself from the loop when called (C# filters do that on dispose).
struct SelfRemoving {
    message_loop: Weak<MessageLoop>,
    id: Cell<u64>,
    log: Log,
}

impl MessageFilter for SelfRemoving {
    fn pre_filter_message(&self, message: &WindowMessage) -> bool {
        self.log.borrow_mut().push(format!("self:{}", message.msg));
        if let Some(message_loop) = self.message_loop.upgrade() {
            message_loop.remove_message_filter(self.id.get());
        }
        false
    }
}

// new: C# MessageLoopFilterTests semantics without a window
#[test]
fn filters_raw_preferred_over_plain_first_true_wins_and_remove_works() {
    let rig = rig();
    let first = Rc::new(RecordingFilter::new(&rig.log, "P1", false));
    let raw = Rc::new(RawFilter {
        log: Rc::clone(&rig.log),
        result: true,
        plain_calls: Cell::new(0),
        raw: RefCell::new(Vec::new()),
    });
    let last = Rc::new(RecordingFilter::new(&rig.log, "P3", false));
    let id1 = rig
        .message_loop
        .add_message_filter(Rc::clone(&first) as Rc<dyn MessageFilter>);
    let id2 = rig
        .message_loop
        .add_message_filter(Rc::clone(&raw) as Rc<dyn MessageFilter>);
    let id3 = rig
        .message_loop
        .add_message_filter(Rc::clone(&last) as Rc<dyn MessageFilter>);
    assert!(id1 != id2 && id2 != id3 && id1 != id3);
    assert_eq!(rig.message_loop.message_filter_count(), 3);

    rig.os.push(Msg {
        hwnd: HWND(7 as *mut _),
        message: 0x0401,
        wParam: WPARAM(8),
        lParam: LPARAM(-9),
        time: 1234,
        ..Msg::default()
    });
    rig.os.push(quit_msg(0));
    rig.message_loop.run().unwrap();

    // Registration order, the raw filter instead of its plain callback, the first `true` wins (P3 never sees it), nothing is dispatched.
    assert_eq!(entries(&rig.log), strs(&["P1:plain:1025", "raw:1025"]));
    assert_eq!(raw.plain_calls.get(), 0);
    assert!(rig.os.dispatched.borrow().is_empty());
    assert_eq!(
        first.seen.borrow()[0],
        WindowMessage {
            hwnd: 7,
            msg: 0x0401,
            wparam: 8,
            lparam: -9
        }
    );
    assert_eq!(raw.raw.borrow()[0].time, 1234, "the raw filter gets the whole MSG");

    // Removing the raw filter: the next filter sees the message and the message is dispatched.
    rig.log.borrow_mut().clear();
    rig.message_loop.remove_message_filter(id2);
    rig.message_loop.remove_message_filter(id2); // unknown id: no-op
    assert_eq!(rig.message_loop.message_filter_count(), 2);
    rig.os.push(msg(0x0402));
    rig.os.push(quit_msg(0));
    rig.message_loop.run().unwrap();
    assert_eq!(
        entries(&rig.log),
        strs(&["P1:plain:1026", "P3:plain:1026", "dispatch:1026"])
    );
    rig.message_loop.remove_message_filter(id1);
    rig.message_loop.remove_message_filter(id3);
    assert_eq!(rig.message_loop.message_filter_count(), 0);
}

// new (T2): a filter may remove itself while the list is being walked; the snapshot still reaches the filters behind it
#[test]
fn a_filter_removing_itself_does_not_disturb_the_walk() {
    let rig = rig();
    let remover = Rc::new(SelfRemoving {
        message_loop: Rc::downgrade(&rig.message_loop),
        id: Cell::new(0),
        log: Rc::clone(&rig.log),
    });
    let tail = Rc::new(RecordingFilter::new(&rig.log, "T", false));
    remover.id.set(
        rig.message_loop
            .add_message_filter(Rc::clone(&remover) as Rc<dyn MessageFilter>),
    );
    rig.message_loop.add_message_filter(tail as Rc<dyn MessageFilter>);
    rig.os.push_all([1, 2]);
    rig.os.push(quit_msg(0));

    rig.message_loop.run().unwrap();

    assert_eq!(
        entries(&rig.log),
        strs(&["self:1", "T:plain:1", "dispatch:1", "T:plain:2", "dispatch:2"])
    );
    assert_eq!(rig.message_loop.message_filter_count(), 1);
}

// new
#[test]
fn clocks_add_is_idempotent_remove_works_and_unarmed_clocks_are_skipped() {
    let rig = rig();
    let a = FakeClock::new("A", &rig.log, 5, i64::MAX);
    let sleeper = FakeClock::new("S", &rig.log, 6, 0);
    sleeper.armed.set(false); // would fire its deadline if it were polled
    let a_dyn = as_clock(&a);
    rig.message_loop.add_frame_clock(Rc::clone(&a_dyn));
    rig.message_loop.add_frame_clock(Rc::clone(&a_dyn)); // same Rc again: ignored
    rig.message_loop.add_frame_clock(as_clock(&sleeper));
    rig.os.push(quit_msg(0));
    rig.message_loop.run().unwrap();
    assert_eq!(entries(&rig.log), strs(&["poll:5"]));

    rig.log.borrow_mut().clear();
    rig.message_loop.remove_frame_clock(&a_dyn);
    rig.message_loop.remove_frame_clock(&a_dyn); // removing twice is fine
    rig.os.push(quit_msg(0));
    rig.message_loop.run().unwrap();
    assert!(entries(&rig.log).is_empty());
}

// new (C# PollClocks `else if`)
#[test]
fn poll_clocks_signaled_does_not_also_fire_the_deadline() {
    let rig = rig();
    let clock = FakeClock::new("A", &rig.log, 5, 0); // deadline already passed AND signaled
    rig.os.signaled.borrow_mut().insert(5);
    rig.message_loop.add_frame_clock(as_clock(&clock));
    rig.os.push(quit_msg(0));

    rig.message_loop.run().unwrap();

    assert_eq!(entries(&rig.log), strs(&["poll:5", "clock:A:signaled"]));
}

// new (T7)
#[test]
fn poll_clocks_deadline_fires_at_exactly_zero() {
    for (until, fires) in [(-1i64, true), (0, true), (1, false)] {
        let rig = rig();
        let clock = FakeClock::new("A", &rig.log, 0, until); // no handle: deadline only
        rig.message_loop.add_frame_clock(as_clock(&clock));
        rig.os.push(quit_msg(0));
        rig.message_loop.run().unwrap();
        assert_eq!(
            entries(&rig.log),
            if fires { strs(&["clock:A:deadline"]) } else { Vec::new() },
            "until {until}"
        );
    }
}

// new (T9): a panic captured in a window procedure is resumed right after dispatch; the loop stops there
#[test]
fn a_dispatched_panic_is_resumed_after_dispatch() {
    let rig = rig();
    rig.os.push_all([1, 2]);
    rig.os.push(quit_msg(0));
    *rig.os.on_dispatch.borrow_mut() = Some(Box::new(|message| {
        if message.message == 1 {
            pending_panic::capture(Box::new("wndproc panicked"));
        }
    }));
    let filter = Rc::new(RecordingFilter::new(&rig.log, "F", false));
    rig.message_loop
        .add_message_filter(Rc::clone(&filter) as Rc<dyn MessageFilter>);

    let outcome = catch_unwind(AssertUnwindSafe(|| rig.message_loop.run()));

    let payload = outcome.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"wndproc panicked"));
    assert_eq!(*rig.os.dispatched.borrow(), vec![1], "message 2 was not dispatched");
    assert_eq!(filter.seen.borrow().len(), 1);
    assert!(!pending_panic::has_pending());
    assert_eq!(rig.os.queue.borrow().len(), 2, "message 2 and WM_QUIT are still queued");
    // The lists are intact and the loop is usable again.
    assert_eq!(rig.message_loop.message_filter_count(), 1);
    rig.os.on_dispatch.borrow_mut().take();
    assert_eq!(rig.message_loop.run().unwrap(), 0);
    assert_eq!(*rig.os.dispatched.borrow(), vec![1, 2]);
}

// new: quit goes through the loop's OS
#[test]
fn quit_with_posts_through_the_loop_os() {
    let rig = rig();
    rig.message_loop.quit_with(-4);
    assert_eq!(*rig.os.quits.borrow(), vec![-4]);
}

// ---- pure helpers ----

// new (T7): INFINITE, 0, ceil to whole milliseconds, capped below INFINITE, `i128` arithmetic
#[test]
fn wait_timeout_ms_table() {
    let freq = 10_000_000;
    assert_eq!(wait_timeout_ms(i64::MAX, freq), INFINITE);
    assert_eq!(wait_timeout_ms(0, freq), 0);
    assert_eq!(wait_timeout_ms(-5, freq), 0);
    assert_eq!(wait_timeout_ms(i64::MIN, freq), 0);
    assert_eq!(wait_timeout_ms(1, freq), 1); // 0.0001 ms rounds up
    assert_eq!(wait_timeout_ms(9_999, freq), 1);
    assert_eq!(wait_timeout_ms(10_000, freq), 1); // exactly one ms
    assert_eq!(wait_timeout_ms(10_001, freq), 2); // one tick past
    assert_eq!(wait_timeout_ms(20_000, freq), 2);
    assert_eq!(wait_timeout_ms(10_000_000, freq), 1_000);
    // The cap: never INFINITE by accident.
    assert_eq!(wait_timeout_ms(i64::MAX - 1, freq), 0xFFFF_FFFE);
    assert_eq!(wait_timeout_ms(10_000 * 0xFFFF_FFFE, freq), 0xFFFF_FFFE);
    assert_eq!(wait_timeout_ms(10_000 * 0xFFFF_FFFE + 1, freq), 0xFFFF_FFFE);
    assert_eq!(wait_timeout_ms(10_000 * 0xFFFF_FFFD + 1, freq), 0xFFFF_FFFE);
    // `until * 1000` would overflow `i64` here (C# `long`): the i128 product does not.
    assert_eq!(wait_timeout_ms(i64::MAX / 2, 1_000_000_000), 0xFFFF_FFFE);
    // A different frequency.
    assert_eq!(wait_timeout_ms(3, 3), 1_000);
    assert_eq!(wait_timeout_ms(1, 3), 334);
}

// new (T9)
#[test]
fn pending_panic_first_wins_take_clears_and_guard_returns_default() {
    // Nothing pending: a no-op.
    assert!(!pending_panic::has_pending());
    pending_panic::rethrow_pending();

    // First wins (C# `??=`).
    pending_panic::capture(Box::new("first"));
    pending_panic::capture(Box::new("second"));
    assert!(pending_panic::has_pending());
    let payload = catch_unwind(pending_panic::rethrow_pending).unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"first"));
    // Taken: the slot is empty afterwards.
    assert!(!pending_panic::has_pending());
    pending_panic::rethrow_pending();

    // `guarded` returns the body's value when it does not panic ...
    assert_eq!(pending_panic::guarded(-1, || 5), 5);
    assert!(!pending_panic::has_pending());
    // ... and the default plus a captured payload when it does (silenced hook: the panic message is not the point).
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let value = pending_panic::guarded(-1, || -> i32 { std::panic::panic_any(String::from("inside wndproc")) });
    std::panic::set_hook(previous);
    assert_eq!(value, -1);
    assert!(pending_panic::has_pending());
    let payload = catch_unwind(pending_panic::rethrow_pending).unwrap_err();
    assert_eq!(
        payload.downcast_ref::<String>().map(String::as_str),
        Some("inside wndproc")
    );
}

// new: the slot is per thread
#[test]
fn pending_panic_is_thread_local() {
    pending_panic::capture(Box::new("mine"));
    let other = std::thread::spawn(|| {
        let had = pending_panic::has_pending();
        pending_panic::rethrow_pending(); // no-op on this thread
        had
    })
    .join()
    .unwrap();
    assert!(!other);
    assert!(pending_panic::has_pending());
    let _ = catch_unwind(pending_panic::rethrow_pending);
}

// The loop is `!Send` (D-W8B3H-4, T10). Compile-time proof: if `MessageLoop` were `Send` this path would be ambiguous.
trait AmbiguousIfSend<A> {
    fn some_item() {}
}
impl<T: ?Sized> AmbiguousIfSend<()> for T {}
impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

#[test]
fn message_loop_is_not_send() {
    let _ = <MessageLoop as AmbiguousIfSend<_>>::some_item;
}
