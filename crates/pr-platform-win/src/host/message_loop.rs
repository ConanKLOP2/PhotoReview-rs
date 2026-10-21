//! Port of `PhotoReview.Shell.Win32/Hosting/MessageLoop.cs` (354 lines, baseline `d92d6aae`). Unit W8-13c.
//!
//! The UI thread's message loop. Each pass, in this order:
//! 1. drain the dispatcher's Send/Normal/Render work (before the next input message and before drawing a frame);
//! 2. poll the armed frame clocks: a clock whose handle is signaled, or whose deadline has passed, raises its frame;
//! 3. pump messages (`PeekMessage`), stopping at once when new high-priority work appears (Normal runs before the next input) or after
//!    [`MAX_MESSAGES_PER_PASS`] messages, so a message flood cannot starve the frame clocks;
//! 4. with the queue empty, run ONE Background item and loop;
//! 5. nothing left: block in `MsgWaitForMultipleObjectsEx` on (messages, the handles of armed clocks, the nearest deadline). An idle window with no
//!    armed clock waits forever: 0 % CPU.
//!
//! A panic that escaped a window procedure (captured by `pending_panic`) is resumed right after `DispatchMessage` returns.
//!
//! Rust shape (D-W8B3H-7): the loop talks to the dispatcher through [`LoopDispatcher`] (13d implements it for `Win32UiDispatcher`) and to the OS through
//! [`LoopOs`] ([`Win32LoopOs`] is the real one), so the pass order is testable with fakes. Clocks are `Rc<dyn HostFrameClock>`; filters are `Rc<dyn MessageFilter>`
//! with `u64` ids; `&self` everywhere (the loop is re-entered by `run_nested`). `!Send`.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use windows::Win32::Foundation::HANDLE;

use crate::gfx::extras::wait_for_handle;
use crate::host::contracts::WindowMessage;
use crate::host::error::HostError;
use crate::host::frame_clock::base::{HostFrameClock, qpc_frequency};
use crate::host::pending_panic;
use crate::win32::api::user32;
use crate::win32::messages::{MWMO_INPUTAVAILABLE, PM_NOREMOVE, PM_REMOVE, QS_ALLINPUT, WAIT_FAILED, WM_QUIT};
use crate::win32::structs::{Msg, Win32Error};

/// C# `MaxMessagesPerPass`.
pub const MAX_MESSAGES_PER_PASS: u32 = 32;
/// C# `MaxWaitHandles`: `MAXIMUM_WAIT_OBJECTS` (64) minus one slot for the message queue.
pub const MAX_WAIT_HANDLES: usize = 63;

/// Identifies one message filter registration.
pub type MessageFilterId = u64;

/// C# `IMessageFilter`: a hook that sees every message before `TranslateMessage`/`DispatchMessage` (the WPF `ComponentDispatcher` bridge of the C#
/// app; in Rust it serves modeless native windows and tests). `true` = handled, skip translate/dispatch.
pub trait MessageFilter {
    /// C# `PreFilterMessage(in WindowMessage)`.
    fn pre_filter_message(&self, message: &WindowMessage) -> bool;

    /// C# `filter is IRawMessageFilter raw`: a filter that needs the whole `MSG` (`time`, `pt`) returns itself here and the loop calls it INSTEAD of
    /// `pre_filter_message`. Default: not raw.
    fn as_raw(&self) -> Option<&dyn RawMessageFilter> {
        None
    }
}

/// C# `IRawMessageFilter`.
pub trait RawMessageFilter {
    /// C# `PreFilterRawMessage(in Msg)`.
    fn pre_filter_raw_message(&self, message: &Msg) -> bool;
}

/// C# `NestedMessageClaim` (`delegate bool (ref Msg)`): a modal dialog gets the first look at each message of a nested loop (`IsDialogMessage`);
/// `true` = handled.
pub type NestedMessageClaim<'a> = &'a mut dyn FnMut(&mut Msg) -> bool;

/// What the loop needs from the UI dispatcher (C# `Win32UiDispatcher` members `VerifyAccess`, `HasHighPriorityWork`, `RunHighPriorityBatch`,
/// `HasBackgroundWork`, `TryRunBackground`). Implemented by `ui_dispatcher::Win32UiDispatcher` (13d) and by a fake in the loop tests.
pub trait LoopDispatcher {
    /// C# `VerifyAccess()`: `Err(InvalidOperation)` when called off the dispatcher's thread (the loop is `!Send`, so this only guards a wrong dispatcher).
    fn verify_access(&self) -> Result<(), HostError>;
    fn has_high_priority_work(&self) -> bool;
    /// Drains Send, Normal, Render by priority until empty or the 50 ms slice is used.
    fn run_high_priority_batch(&self);
    fn has_background_work(&self) -> bool;
    /// Runs one Background item if any (and no high-priority work); `true` if one ran.
    fn try_run_background(&self) -> bool;
}

/// What a blocking wait returned (C# `MsgWaitForMultipleObjectsEx` result decoded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// `WAIT_OBJECT_0 + index` with `index < handles.len()`: the handle at `index` signaled (and the wait consumed it if it is an auto-reset object).
    Handle(usize),
    /// `WAIT_OBJECT_0 + handles.len()`: input is available in the message queue.
    Messages,
    /// `WAIT_TIMEOUT` (or `WAIT_IO_COMPLETION`, `WAIT_ABANDONED_0 + n`): nothing to do, loop again.
    Timeout,
    /// `WAIT_FAILED` with the error read straight after the call.
    Failed(Win32Error),
}

/// The OS calls of the loop.
pub trait LoopOs {
    /// C# `PeekMessage(&msg, 0, 0, 0, PM_REMOVE or PM_NOREMOVE)`.
    fn peek_message(&self, msg: &mut Msg, remove: bool) -> bool;
    /// C# `IsMessagePending`: `PeekMessage(.., PM_NOREMOVE)` without keeping the message.
    fn is_message_pending(&self) -> bool;
    /// C# `TranslateMessage` then `DispatchMessage`.
    fn translate_and_dispatch(&self, msg: &Msg);
    /// C# `WaitForSingleObject(handle, 0) == WAIT_OBJECT_0`.
    fn poll_handle(&self, handle: isize) -> bool;
    /// C# `MsgWaitForMultipleObjectsEx(count, handles, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)`.
    fn wait(&self, handles: &[isize], timeout_ms: u32) -> WaitOutcome;
    /// C# `PostQuitMessage(exit_code)`.
    fn post_quit(&self, exit_code: i32);
}

/// The real [`LoopOs`] over `win32::api::user32` and `gfx::extras::wait_for_handle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Win32LoopOs;

impl LoopOs for Win32LoopOs {
    fn peek_message(&self, msg: &mut Msg, remove: bool) -> bool {
        // hwnd 0 and range (0, 0): every message of this thread, window and thread messages alike (T10).
        user32::peek_message(msg, 0, 0, 0, if remove { PM_REMOVE } else { PM_NOREMOVE })
    }

    fn is_message_pending(&self) -> bool {
        let mut scratch = Msg::default();
        user32::peek_message(&mut scratch, 0, 0, 0, PM_NOREMOVE)
    }

    fn translate_and_dispatch(&self, msg: &Msg) {
        user32::translate_message(msg);
        // SAFETY: `msg` was filled by PeekMessage on this thread, so `wparam`/`lparam` match what the target window procedure expects.
        // The procedure itself is panic-guarded (`pending_panic::guarded`).
        let _result = unsafe { user32::dispatch_message(msg) };
    }

    fn poll_handle(&self, handle: isize) -> bool {
        // A stale handle value only makes the wait fail (`Failed`, not signaled).
        wait_for_handle(HANDLE(handle as *mut c_void), 0, false).is_signaled()
    }

    fn wait(&self, handles: &[isize], timeout_ms: u32) -> WaitOutcome {
        let result = user32::msg_wait_for_multiple_objects_ex(handles, timeout_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        if result == WAIT_FAILED {
            // Read straight after the call, before anything else can change it.
            return WaitOutcome::Failed(Win32Error::last());
        }
        let count = u32::try_from(handles.len()).unwrap_or(u32::MAX);
        if result < count {
            WaitOutcome::Handle(result as usize)
        } else if result == count {
            WaitOutcome::Messages
        } else {
            WaitOutcome::Timeout
        }
    }

    fn post_quit(&self, exit_code: i32) {
        user32::post_quit_message(exit_code);
    }
}

/// The `MsgWaitForMultipleObjectsEx` timeout for a nearest deadline (C# `Wait`, lines 327-332): `until_deadline == i64::MAX` is `INFINITE`
/// (`0xFFFF_FFFF`); `<= 0` is `0`; else `ceil(until_deadline * 1000 / frequency)` capped at `0xFFFF_FFFE` (never accidentally INFINITE). Computed in `i128`.
pub fn wait_timeout_ms(until_deadline: i64, frequency: i64) -> u32 {
    const INFINITE: u32 = 0xFFFF_FFFF;
    if until_deadline == i64::MAX {
        return INFINITE;
    }
    if until_deadline <= 0 {
        return 0;
    }
    let frequency = i128::from(frequency.max(1));
    let ms = (i128::from(until_deadline) * 1000 + frequency - 1) / frequency;
    u32::try_from(ms.min(i128::from(INFINITE - 1))).unwrap_or(INFINITE - 1)
}

/// Clears `running` on every exit of `run` (an `Err`, or a resumed panic).
struct RunningGuard<'a>(&'a Cell<bool>);

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// C# `MessageLoop`.
pub struct MessageLoop {
    dispatcher: Rc<dyn LoopDispatcher>,
    os: Rc<dyn LoopOs>,
    clocks: RefCell<Vec<Rc<dyn HostFrameClock>>>,
    filters: RefCell<Vec<(MessageFilterId, Rc<dyn MessageFilter>)>>,
    next_filter_id: Cell<MessageFilterId>,
    running: Cell<bool>,
    wait_count: Cell<i64>,
    pass_count: Cell<i64>,
    _not_send: PhantomData<*const ()>,
}

impl fmt::Debug for MessageLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageLoop").finish_non_exhaustive()
    }
}

impl MessageLoop {
    /// C# constructor: real OS calls; `dispatcher.verify_access()` must pass.
    pub fn new(dispatcher: Rc<dyn LoopDispatcher>) -> Result<Self, HostError> {
        Self::with_os(dispatcher, Rc::new(Win32LoopOs))
    }

    /// Constructor with the OS seam (tests).
    pub fn with_os(dispatcher: Rc<dyn LoopDispatcher>, os: Rc<dyn LoopOs>) -> Result<Self, HostError> {
        dispatcher.verify_access()?;
        Ok(Self {
            dispatcher,
            os,
            clocks: RefCell::new(Vec::new()),
            filters: RefCell::new(Vec::new()),
            next_filter_id: Cell::new(1),
            running: Cell::new(false),
            wait_count: Cell::new(0),
            pass_count: Cell::new(0),
            _not_send: PhantomData,
        })
    }

    /// C# `WaitCount`: how many times the loop blocked (idle diagnostics/tests).
    pub fn wait_count(&self) -> i64 {
        self.wait_count.get()
    }

    /// C# `PassCount`.
    pub fn pass_count(&self) -> i64 {
        self.pass_count.get()
    }

    /// C# `MessageFilterCount`.
    pub fn message_filter_count(&self) -> usize {
        self.filters.borrow().len()
    }

    /// C# `AddFrameClock`: ignored when the same `Rc` (pointer equality of the data pointer) is already registered.
    pub fn add_frame_clock(&self, clock: Rc<dyn HostFrameClock>) {
        let mut clocks = self.clocks.borrow_mut();
        if !clocks.iter().any(|known| same_clock(known, &clock)) {
            clocks.push(clock);
        }
    }

    /// C# `RemoveFrameClock`.
    pub fn remove_frame_clock(&self, clock: &Rc<dyn HostFrameClock>) {
        let removed: Vec<Rc<dyn HostFrameClock>> = {
            let mut clocks = self.clocks.borrow_mut();
            let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut *clocks)
                .into_iter()
                .partition(|known| same_clock(known, clock));
            *clocks = kept;
            gone
        };
        drop(removed);
    }

    /// C# `AddMessageFilter`.
    pub fn add_message_filter(&self, filter: Rc<dyn MessageFilter>) -> MessageFilterId {
        let id = self.next_filter_id.get();
        self.next_filter_id.set(id.wrapping_add(1));
        self.filters.borrow_mut().push((id, filter));
        id
    }

    /// C# `RemoveMessageFilter`; an unknown id is a no-op.
    pub fn remove_message_filter(&self, id: MessageFilterId) {
        let removed = {
            let mut filters = self.filters.borrow_mut();
            filters
                .iter()
                .position(|(known, _)| *known == id)
                .map(|index| filters.remove(index))
        };
        // Dropped outside the borrow: the filter's `Drop` may touch the loop.
        drop(removed);
    }

    /// C# static `Quit(exitCode)`: `PostQuitMessage` for the current thread's loop (the real OS call; use [`MessageLoop::quit_with`] with a fake).
    pub fn quit(exit_code: i32) {
        Win32LoopOs.post_quit(exit_code);
    }

    /// Same through this loop's [`LoopOs`].
    pub fn quit_with(&self, exit_code: i32) {
        self.os.post_quit(exit_code);
    }

    /// C# static `IsMessagePending()` (real OS).
    pub fn is_message_pending() -> bool {
        Win32LoopOs.is_message_pending()
    }

    /// C# `Run()`: runs to `WM_QUIT`, returns its exit code. Not re-entrant: a second `run` on a running loop is
    /// `Err(InvalidOperation { message: "MessageLoop.Run is not re-entrant." })`. `Err(Win32 "MsgWaitForMultipleObjectsEx failed")` when the wait fails.
    /// A panic captured from a window procedure is resumed from inside this call (D-W8B3H-4).
    pub fn run(&self) -> Result<i32, HostError> {
        self.dispatcher.verify_access()?;
        if self.running.get() {
            return Err(HostError::InvalidOperation {
                message: "MessageLoop.Run is not re-entrant.",
            });
        }
        self.running.set(true);
        let _running = RunningGuard(&self.running);
        loop {
            self.pass_count.set(self.pass_count.get().wrapping_add(1));
            if self.dispatcher.has_high_priority_work() {
                self.dispatcher.run_high_priority_batch();
            }
            self.poll_clocks()?;
            if let Some(exit_code) = self.pump_messages(None) {
                return Ok(exit_code);
            }
            self.after_pump()?;
        }
    }

    /// C# `RunNested(isDone, claim)`: the loop for a modal Win32 dialog (same pass structure as `run`). `Ok(true)` when it ended because `is_done()`;
    /// `Ok(false)` when it met `WM_QUIT`, in which case `WM_QUIT` is POSTED AGAIN for the outer loop (lesson #486: a nested loop never swallows it).
    pub fn run_nested(
        &self,
        is_done: &mut dyn FnMut() -> bool,
        mut claim: Option<NestedMessageClaim<'_>>,
    ) -> Result<bool, HostError> {
        self.dispatcher.verify_access()?;
        while !is_done() {
            self.pass_count.set(self.pass_count.get().wrapping_add(1));
            if self.dispatcher.has_high_priority_work() {
                self.dispatcher.run_high_priority_batch();
            }
            self.poll_clocks()?;
            if let Some(exit_code) = self.pump_messages(claim.as_deref_mut()) {
                self.os.post_quit(exit_code);
                return Ok(false);
            }
            if is_done() {
                break;
            }
            self.after_pump()?;
        }
        Ok(true)
    }

    /// Steps 5 to 7 of a pass: high-priority work -> next pass; else one Background item (only with an empty queue) -> next pass; else block in `wait`
    /// (only with an empty queue). Every branch ends the pass (the C# `continue`s).
    fn after_pump(&self) -> Result<(), HostError> {
        if self.dispatcher.has_high_priority_work() {
            return Ok(());
        }
        if self.dispatcher.has_background_work() {
            if !self.os.is_message_pending() {
                self.dispatcher.try_run_background();
            }
            return Ok(());
        }
        if !self.os.is_message_pending() {
            self.wait()?;
        }
        Ok(())
    }

    /// C# private `PumpMessages(out exitCode, claim)`: `Some(exit_code)` on `WM_QUIT`. At most `MAX_MESSAGES_PER_PASS` messages; per message: `WM_QUIT`
    /// ends the pump; `claim` (nested) then the filters; if nothing claimed it, translate + dispatch, then `pending_panic::rethrow_pending()`; stop when
    /// `has_high_priority_work()` becomes true.
    fn pump_messages<'a, 'b>(&self, mut claim: Option<&'a mut (dyn FnMut(&mut Msg) -> bool + 'b)>) -> Option<i32> {
        let mut msg = Msg::default();
        for _ in 0..MAX_MESSAGES_PER_PASS {
            if !self.os.peek_message(&mut msg, true) {
                return None;
            }
            if msg.message == WM_QUIT {
                // `(int)message.WParam`: the cast truncates like C#.
                return Some(msg.wParam.0 as i32);
            }
            let claimed = match claim.as_deref_mut() {
                Some(claim) => claim(&mut msg),
                None => false,
            };
            if claimed {
                pending_panic::rethrow_pending();
            } else if !self.filtered(&msg) {
                self.os.translate_and_dispatch(&msg);
                pending_panic::rethrow_pending();
            }
            if self.dispatcher.has_high_priority_work() {
                return None;
            }
        }
        None
    }

    /// C# private `Filtered(in Msg)`: snapshot of the filter list; raw filters get the `Msg`, plain ones the `WindowMessage`.
    fn filtered(&self, msg: &Msg) -> bool {
        // A snapshot: a filter may add or remove filters (or itself) from inside its callback (T2).
        let filters: Vec<Rc<dyn MessageFilter>> = self
            .filters
            .borrow()
            .iter()
            .map(|(_, filter)| Rc::clone(filter))
            .collect();
        if filters.is_empty() {
            return false;
        }
        let window_message = WindowMessage {
            hwnd: msg.hwnd.0 as isize,
            msg: msg.message,
            wparam: msg.wParam.0,
            lparam: msg.lParam.0,
        };
        for filter in filters {
            let handled = match filter.as_raw() {
                Some(raw) => raw.pre_filter_raw_message(msg),
                None => filter.pre_filter_message(&window_message),
            };
            if handled {
                return true;
            }
        }
        false
    }

    /// C# private `PollClocks()`: for each armed clock: handle != 0 and `poll_handle` -> `on_signaled`; else `ticks_until_deadline() <= 0` -> `on_deadline`.
    fn poll_clocks(&self) -> Result<(), HostError> {
        let clocks: Vec<Rc<dyn HostFrameClock>> = self.clocks.borrow().clone();
        for clock in clocks {
            if !clock.is_armed() {
                continue;
            }
            let handle = clock.wait_handle();
            if handle != 0 && self.os.poll_handle(handle) {
                clock.on_signaled()?;
            } else if clock.ticks_until_deadline() <= 0 {
                clock.on_deadline()?;
            }
        }
        Ok(())
    }

    /// C# private `Wait()`: build the handle array of the armed clocks (at most `MAX_WAIT_HANDLES`), the nearest deadline, block once;
    /// `Handle(i)` -> `owners[i].on_signaled()`; `Failed(e)` -> `Err(Win32 "MsgWaitForMultipleObjectsEx failed")`.
    fn wait(&self) -> Result<(), HostError> {
        let clocks: Vec<Rc<dyn HostFrameClock>> = self.clocks.borrow().clone();
        let mut handles: Vec<isize> = Vec::new();
        let mut owners: Vec<Rc<dyn HostFrameClock>> = Vec::new();
        let mut until_deadline = i64::MAX;
        for clock in clocks {
            if !clock.is_armed() {
                continue;
            }
            until_deadline = until_deadline.min(clock.ticks_until_deadline());
            let handle = clock.wait_handle();
            if handle != 0 && handles.len() < MAX_WAIT_HANDLES {
                handles.push(handle);
                owners.push(clock);
            }
        }
        let timeout = wait_timeout_ms(until_deadline, qpc_frequency());
        self.wait_count.set(self.wait_count.get().wrapping_add(1));
        match self.os.wait(&handles, timeout) {
            WaitOutcome::Failed(error) => Err(HostError::Win32 {
                operation: "MsgWaitForMultipleObjectsEx failed",
                error,
            }),
            // The wait consumed the signal (auto-reset object or synchronization timer).
            WaitOutcome::Handle(index) => match owners.get(index) {
                Some(owner) => owner.on_signaled().map(|_| ()),
                None => Ok(()),
            },
            WaitOutcome::Messages | WaitOutcome::Timeout => Ok(()),
        }
    }
}

/// Identity of two clocks: the data pointers (the vtable pointers of one object may differ between codegen units).
fn same_clock(a: &Rc<dyn HostFrameClock>, b: &Rc<dyn HostFrameClock>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}
