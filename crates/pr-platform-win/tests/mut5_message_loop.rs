//! MUT5 (2026-11-06): bounded killers for `host/message_loop.rs` (`MessageLoop` over fakes). P3 left 31 mutants of this file NOT RUN and caught the
//! two `add_frame_clock` mutants (240, 241) only by a runaway 25 GB allocation ABORT of `host_message_loop`.
//!
//! Every fake here counts its calls against one shared budget ([`BUDGET`]): a mutated loop that no longer ends (or no longer blocks) runs into
//! `budget exceeded` after a few hundred passes, which is an ordinary assertion failure with a tiny log, never an unbounded allocation or a timeout.
//! The expected values are hand-written from `MessageLoop.cs` (baseline `d92d6aae`) and the pass order documented in `message_loop.rs`.
#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

use pr_platform_win::host::contracts::WindowMessage;
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::base::{FrameHandler, FrameHandlerId, HostFrameClock, qpc_frequency};
use pr_platform_win::host::message_loop::{
    LoopDispatcher, LoopOs, MAX_WAIT_HANDLES, MessageFilter, MessageLoop, WaitOutcome,
};
use pr_platform_win::win32::structs::Msg;
use windows::Win32::Foundation::WPARAM;

const WM_QUIT: u32 = 0x0012;
/// Total number of fake calls one test may make before the fake panics. The longest legitimate test below needs under 200.
const BUDGET: u32 = 600;

type Log = Rc<RefCell<Vec<String>>>;

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

#[derive(Default)]
struct Budget(Cell<u32>);

impl Budget {
    fn tick(&self, what: &str) {
        let used = self.0.get() + 1;
        self.0.set(used);
        assert!(
            used <= BUDGET,
            "budget exceeded in {what}: the mutated loop does not end"
        );
    }
}

struct FakeDispatcher {
    log: Log,
    budget: Rc<Budget>,
    background: Cell<u32>,
    on_background: RefCell<Option<Box<dyn FnMut()>>>,
}

impl LoopDispatcher for FakeDispatcher {
    fn verify_access(&self) -> Result<(), HostError> {
        Ok(())
    }
    fn has_high_priority_work(&self) -> bool {
        self.budget.tick("has_high_priority_work");
        false
    }
    fn run_high_priority_batch(&self) {
        self.log.borrow_mut().push("hp-batch".to_owned());
    }
    fn has_background_work(&self) -> bool {
        self.background.get() > 0
    }
    fn try_run_background(&self) -> bool {
        if self.background.get() == 0 {
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

struct FakeOs {
    log: Log,
    budget: Rc<Budget>,
    queue: RefCell<VecDeque<Msg>>,
    signaled: RefCell<HashSet<isize>>,
    waits: RefCell<Vec<(Vec<isize>, u32)>>,
    on_wait: RefCell<Option<WaitHook>>,
    dispatched: RefCell<Vec<u32>>,
    quits: RefCell<Vec<i32>>,
}

impl FakeOs {
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
        self.budget.tick("peek_message");
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
        self.budget.tick("is_message_pending");
        !self.queue.borrow().is_empty()
    }
    fn translate_and_dispatch(&self, message: &Msg) {
        self.log.borrow_mut().push(format!("dispatch:{}", message.message));
        self.dispatched.borrow_mut().push(message.message);
    }
    fn poll_handle(&self, handle: isize) -> bool {
        self.budget.tick("poll_handle");
        self.log.borrow_mut().push(format!("poll:{handle}"));
        self.signaled.borrow_mut().remove(&handle)
    }
    fn wait(&self, handles: &[isize], timeout_ms: u32) -> WaitOutcome {
        self.budget.tick("wait");
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
    name: String,
    log: Log,
    armed: Cell<bool>,
    handle: Cell<isize>,
    until: Cell<i64>,
    fail: Cell<bool>,
}

impl FakeClock {
    fn new(name: &str, log: &Log, handle: isize, until: i64) -> Rc<Self> {
        Rc::new(Self {
            name: name.to_owned(),
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

/// Handles a message when its number is `handle_on`; records every message it saw.
struct FakeFilter {
    handle_on: u32,
    seen: RefCell<Vec<u32>>,
}

impl MessageFilter for FakeFilter {
    fn pre_filter_message(&self, message: &WindowMessage) -> bool {
        self.seen.borrow_mut().push(message.msg);
        message.msg == self.handle_on
    }
}

struct Rig {
    log: Log,
    dispatcher: Rc<FakeDispatcher>,
    os: Rc<FakeOs>,
    message_loop: Rc<MessageLoop>,
}

fn rig() -> Rig {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let budget = Rc::new(Budget::default());
    let dispatcher = Rc::new(FakeDispatcher {
        log: Rc::clone(&log),
        budget: Rc::clone(&budget),
        background: Cell::new(0),
        on_background: RefCell::new(None),
    });
    let os = Rc::new(FakeOs {
        log: Rc::clone(&log),
        budget,
        queue: RefCell::new(VecDeque::new()),
        signaled: RefCell::new(HashSet::new()),
        waits: RefCell::new(Vec::new()),
        on_wait: RefCell::new(None),
        dispatched: RefCell::new(Vec::new()),
        quits: RefCell::new(Vec::new()),
    });
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

/// The next blocking wait posts WM_QUIT(code) and returns `outcome`.
fn quit_on_wait(rig: &Rig, code: usize, outcome: WaitOutcome) {
    let os = Rc::clone(&rig.os);
    *rig.os.on_wait.borrow_mut() = Some(Box::new(move |_handles, _timeout| {
        os.push(quit_msg(code));
        outcome
    }));
}

// ---- add_frame_clock (message_loop.rs 240, 241; same_clock 478) ----

// Killer of 240 (`add_frame_clock -> ()`) and 241 (`delete !`): with no clock registered a signaled failing clock is never polled, the idle loop
// blocks once, the hook posts WM_QUIT and `run` returns `Ok(9)` instead of the clock's error. The assertion fails; the budget bounds the loop.
#[test]
fn a_registered_failing_clock_ends_run_with_its_error_and_an_unregistered_one_cannot() {
    let rig = rig();
    let clock = FakeClock::new("A", &rig.log, 5, i64::MAX);
    clock.fail.set(true);
    rig.os.signaled.borrow_mut().insert(5);
    rig.message_loop.add_frame_clock(as_clock(&clock));
    rig.message_loop.add_frame_clock(as_clock(&clock)); // the same Rc twice: still one registration
    quit_on_wait(&rig, 9, WaitOutcome::Messages);

    let result = rig.message_loop.run();

    assert_eq!(
        result,
        Err(HostError::InvalidOperation {
            message: "clock failed"
        })
    );
    assert_eq!(entries(&rig.log), strs(&["poll:5", "clock:A:signaled"]));
    assert!(
        rig.os.waits.borrow().is_empty(),
        "the error ended the run before any wait"
    );
}

// Killer of same_clock 478 (`-> true`, `-> false`): identity decides who is registered.
#[test]
fn clocks_are_registered_once_per_identity() {
    let rig = rig();
    let a = FakeClock::new("A", &rig.log, 5, i64::MAX);
    let b = FakeClock::new("B", &rig.log, 6, i64::MAX);
    rig.os.signaled.borrow_mut().extend([5, 6]);
    rig.message_loop.add_frame_clock(as_clock(&a));
    rig.message_loop.add_frame_clock(as_clock(&a)); // `-> false` would register A twice
    rig.message_loop.add_frame_clock(as_clock(&b)); // `-> true` would drop B
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(
        entries(&rig.log),
        strs(&["poll:5", "clock:A:signaled", "poll:6", "clock:B:signaled"])
    );
}

// ---- run_nested (327, 328) ----

// Killer of 327 (`-> Ok(true)`, `-> Ok(false)`) and 328 (`delete !` of `while !is_done()`): the body runs while `is_done` is false.
#[test]
fn run_nested_pumps_until_is_done_and_returns_true() {
    let rig = rig();
    rig.os.push_all([1, 2, 3]);
    let os = Rc::clone(&rig.os);
    let mut calls = 0u32;
    let mut is_done = move || {
        calls += 1;
        assert!(calls <= 50, "is_done called too often");
        os.dispatched.borrow().len() >= 2
    };

    let result = rig.message_loop.run_nested(&mut is_done, None);

    assert_eq!(result, Ok(true));
    assert_eq!(*rig.os.dispatched.borrow(), vec![1, 2, 3]); // the whole pump runs; is_done is checked between passes
    assert_eq!(rig.message_loop.pass_count(), 1);
    assert!(rig.os.quits.borrow().is_empty());
}

// Killer of 328: an already-done nested loop runs no pass at all (`while is_done()` would enter the body once).
#[test]
fn run_nested_with_is_done_already_true_runs_no_pass() {
    let rig = rig();
    rig.os.push(msg(1));
    let mut is_done = || true;

    assert_eq!(rig.message_loop.run_nested(&mut is_done, None), Ok(true));

    assert_eq!(rig.message_loop.pass_count(), 0);
    assert!(rig.os.dispatched.borrow().is_empty());
    assert_eq!(rig.os.queue.borrow().len(), 1, "nothing was taken from the queue");
}

// Killer of 327 `-> Ok(true)`: WM_QUIT ends a nested loop with Ok(false) and is posted again for the outer loop.
#[test]
fn run_nested_meets_wm_quit_returns_false_and_reposts_it() {
    let rig = rig();
    rig.os.push(msg(1));
    rig.os.push(quit_msg(3));
    let mut calls = 0u32;
    let mut is_done = move || {
        calls += 1;
        assert!(calls <= 50, "is_done called too often");
        false
    };

    let result = rig.message_loop.run_nested(&mut is_done, None);

    assert_eq!(result, Ok(false));
    assert_eq!(*rig.os.quits.borrow(), vec![3]);
    assert_eq!(*rig.os.dispatched.borrow(), vec![1]);
}

// ---- after_pump (349, 353, 358) ----

// Killer of 349 (`after_pump -> Ok(())`) and 353 (`delete !` of `!is_message_pending()` before the Background item): with an empty queue the
// Background item runs; its hook posts WM_QUIT.
#[test]
fn background_item_runs_when_the_queue_is_empty() {
    let rig = rig();
    rig.dispatcher.background.set(1);
    let os = Rc::clone(&rig.os);
    *rig.dispatcher.on_background.borrow_mut() = Some(Box::new(move || os.push(quit_msg(4))));

    assert_eq!(rig.message_loop.run().unwrap(), 4);

    assert_eq!(entries(&rig.log), strs(&["bg"]));
    assert_eq!(rig.message_loop.wait_count(), 0);
}

// Killer of 353: with a message still pending after a full pump (33 queued, 32 per pass) the Background item waits for the pump to empty the queue.
#[test]
fn background_item_waits_for_the_pending_message() {
    let rig = rig();
    rig.dispatcher.background.set(1);
    rig.os.push_all(1001..=1033);
    let os = Rc::clone(&rig.os);
    *rig.dispatcher.on_background.borrow_mut() = Some(Box::new(move || os.push(quit_msg(0))));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    let log = entries(&rig.log);
    let last_message = log.iter().position(|entry| entry == "dispatch:1033").unwrap();
    let background = log.iter().position(|entry| entry == "bg").unwrap();
    assert!(last_message < background, "{log:?}");
    assert_eq!(log.iter().filter(|entry| *entry == "bg").count(), 1);
}

// Killer of 358 (`delete !` of `!is_message_pending()` before the wait): a pending message skips the wait, an empty queue blocks exactly once.
#[test]
fn wait_is_skipped_with_a_pending_message_and_taken_with_an_empty_queue() {
    let rig = rig();
    rig.os.push_all(1001..=1033); // one message is left pending after the first pump
    rig.os.push(quit_msg(0));
    assert_eq!(rig.message_loop.run().unwrap(), 0);
    assert_eq!(rig.message_loop.wait_count(), 0);
    assert!(rig.os.waits.borrow().is_empty());

    let rig = rig_with_quit_on_wait();
    assert_eq!(rig.message_loop.run().unwrap(), 6);
    assert_eq!(rig.message_loop.wait_count(), 1);
    assert_eq!(*rig.os.waits.borrow(), vec![(Vec::<isize>::new(), 0xFFFF_FFFF)]);
}

fn rig_with_quit_on_wait() -> Rig {
    let rig = rig();
    quit_on_wait(&rig, 6, WaitOutcome::Timeout);
    rig
}

// ---- pump_messages (368, 370, 373, 383, 397) ----

// Killer of 368 (`-> None`, `Some(0)`, `Some(1)`, `Some(-1)`), 370 (`delete !` of `!peek_message`) and 373 (`== -> !=` of WM_QUIT): messages are
// dispatched in order, then WM_QUIT's wParam (5, none of 0, 1, -1) is the exit code.
#[test]
fn pump_dispatches_in_order_and_returns_the_quit_code() {
    let rig = rig();
    rig.os.push(msg(1));
    rig.os.push(msg(2));
    rig.os.push(quit_msg(5));
    rig.os.push(msg(3)); // behind WM_QUIT: not reached

    assert_eq!(rig.message_loop.run().unwrap(), 5);

    assert_eq!(*rig.os.dispatched.borrow(), vec![1, 2]);
    assert_eq!(entries(&rig.log), strs(&["dispatch:1", "dispatch:2"]));
}

// Killer of 383 (`delete !` of `!self.filtered(..)`) and 397 (`filtered -> true`, `-> false`): with no filter every message is dispatched.
#[test]
fn without_filters_every_message_is_dispatched() {
    let rig = rig();
    rig.os.push_all([1, 2, 3]);
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(*rig.os.dispatched.borrow(), vec![1, 2, 3]);
    assert_eq!(rig.message_loop.message_filter_count(), 0);
}

// Killer of 383 and 397: a filter that handles one message keeps exactly that one from the window procedure.
#[test]
fn a_filter_that_handles_a_message_stops_its_dispatch() {
    let rig = rig();
    let filter = Rc::new(FakeFilter {
        handle_on: 2,
        seen: RefCell::new(Vec::new()),
    });
    rig.message_loop
        .add_message_filter(Rc::clone(&filter) as Rc<dyn MessageFilter>);
    rig.os.push_all([1, 2, 3]);
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(*rig.os.dispatched.borrow(), vec![1, 3]);
    assert_eq!(*filter.seen.borrow(), vec![1, 2, 3]);
}

// ---- poll_clocks (426, 428, 432, 434) ----

// Killer of 426 (`poll_clocks -> Ok(())`): a signaled armed clock gets `on_signaled` (and not `on_deadline`) before the pump.
#[test]
fn a_signaled_clock_raises_its_frame_and_not_its_deadline() {
    let rig = rig();
    let clock = FakeClock::new("A", &rig.log, 5, 0);
    rig.os.signaled.borrow_mut().insert(5);
    rig.message_loop.add_frame_clock(as_clock(&clock));
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(entries(&rig.log), strs(&["poll:5", "clock:A:signaled"]));
}

// Killer of 428 (`delete !` of `!clock.is_armed()`): an unarmed clock is not touched at all; the armed one is.
#[test]
fn an_unarmed_clock_is_skipped_and_an_armed_one_is_polled() {
    let rig = rig();
    let unarmed = FakeClock::new("U", &rig.log, 7, 0);
    unarmed.armed.set(false);
    rig.os.signaled.borrow_mut().insert(7);
    let armed = FakeClock::new("A", &rig.log, 5, i64::MAX);
    rig.message_loop.add_frame_clock(as_clock(&unarmed));
    rig.message_loop.add_frame_clock(as_clock(&armed));
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(entries(&rig.log), strs(&["poll:5"]));
}

// Killer of 432:23 (`!= -> ==`) and 434 (`<= -> >`): handle 0 means "no handle": it is never polled, and a deadline of 0 ticks raises `on_deadline`.
#[test]
fn a_clock_without_handle_is_never_polled_and_a_passed_deadline_raises_the_deadline() {
    let rig = rig();
    let clock = FakeClock::new("Z", &rig.log, 0, 0);
    rig.message_loop.add_frame_clock(as_clock(&clock));
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(entries(&rig.log), strs(&["clock:Z:deadline"]));
}

// Killer of 432:28 (`&& -> ||`) and 434: a handle that is not signaled and a deadline in the future raises nothing; a negative deadline does.
#[test]
fn an_unsignaled_clock_with_a_future_deadline_raises_nothing() {
    let rig = rig();
    let waiting = FakeClock::new("W", &rig.log, 5, 1); // 1 tick left: not passed
    let late = FakeClock::new("L", &rig.log, 6, -1); // passed, handle not signaled
    rig.message_loop.add_frame_clock(as_clock(&waiting));
    rig.message_loop.add_frame_clock(as_clock(&late));
    rig.os.push(quit_msg(0));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    assert_eq!(entries(&rig.log), strs(&["poll:5", "poll:6", "clock:L:deadline"]));
}

// ---- wait (444, 449, 454) ----

// Killer of 444 (`wait -> Ok(())`), 449 (`delete !` of `!clock.is_armed()`), 454:23 (`!= -> ==`) and 454:28 (`&& -> ||`): the wait gets the handles of the
// ARMED clocks with a handle, in order, and the nearest deadline of the armed clocks (2 s = 2000 ms), and `Handle(0)` signals its owner.
#[test]
fn wait_gets_the_handles_and_the_nearest_deadline_of_the_armed_clocks() {
    let rig = rig();
    let frequency = qpc_frequency();
    let unarmed = FakeClock::new("U", &rig.log, 7, 1); // would make the timeout 1 ms and the list [7]
    unarmed.armed.set(false);
    let armed = FakeClock::new("A", &rig.log, 5, frequency * 2);
    let no_handle = FakeClock::new("Z", &rig.log, 0, frequency * 10); // armed, no handle: not in the list
    rig.message_loop.add_frame_clock(as_clock(&unarmed));
    rig.message_loop.add_frame_clock(as_clock(&armed));
    rig.message_loop.add_frame_clock(as_clock(&no_handle));
    quit_on_wait(&rig, 3, WaitOutcome::Handle(0));

    assert_eq!(rig.message_loop.run().unwrap(), 3);

    assert_eq!(*rig.os.waits.borrow(), vec![(vec![5isize], 2000u32)]);
    assert_eq!(rig.message_loop.wait_count(), 1);
    assert_eq!(
        entries(&rig.log),
        strs(&["poll:5", "wait", "clock:A:signaled", "poll:5"])
    );
}

// Killer of 454:45 (`< -> ==`, `>`, `<=`) and of the owner mapping: with 70 armed clocks the list holds the first MAX_WAIT_HANDLES (63) handles,
// and `Handle(62)` signals the 63rd clock.
#[test]
fn wait_takes_at_most_63_handles_in_registration_order() {
    let rig = rig();
    assert_eq!(MAX_WAIT_HANDLES, 63);
    for number in 1..=70isize {
        let clock = FakeClock::new(&format!("c{number}"), &rig.log, number, i64::MAX);
        rig.message_loop.add_frame_clock(as_clock(&clock));
    }
    quit_on_wait(&rig, 0, WaitOutcome::Handle(62));

    assert_eq!(rig.message_loop.run().unwrap(), 0);

    let waits = rig.os.waits.borrow();
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].0, (1..=63isize).collect::<Vec<_>>());
    assert_eq!(waits[0].1, 0xFFFF_FFFF);
    assert!(entries(&rig.log).iter().any(|entry| entry == "clock:c63:signaled"));
    assert!(!entries(&rig.log).iter().any(|entry| entry == "clock:c64:signaled"));
}
