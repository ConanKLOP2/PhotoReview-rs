//! Port of `PhotoReview.Shell.Win32/Hosting/Win32UiDispatcher.cs` (389 lines, baseline `d92d6aae`). Unit W8-13d.
//! `Win32UiSynchronizationContext.cs` (80 lines) is RETIRED: Rust has no `SynchronizationContext` and no `await` on the UI thread (R0003, D-W8-2); what
//! it did (continuations back on the UI thread) is `UiScheduler::post`, and `Win32UiDispatcher` here is the one queue behind it.
//!
//! C-04 on Win32: priority queues owned by ONE UI thread (the thread that created the dispatcher), woken by `PostMessage(message-only window,
//! WM_APP + 1)`. Send > Normal > Render run by priority (FIFO inside a level) in a drain pass of `MessageLoop` before the next message and before a
//! frame; work posted during the drain runs in the same drain (drain to empty, like WPF's priority above Input) but a drain stops after
//! [`BATCH_SLICE_MS`] so input is not starved. Background runs one item at a time, only when the thread's message queue is empty and nothing of
//! higher priority waits. In a modal loop that is not ours (resize drag, menu, `MessageBox`) the wake message is still dispatched to the
//! message-only window, so the queue keeps running ([`Win32UiDispatcher::on_wake_message`]).
//!
//! Rust shape (D-W8B3H-8): [`Win32UiDispatcher`] is `Clone + Send + Sync` (an `Arc` of the shared state) and implements
//! `pr_core::abstractions::{UiScheduler, UiDispatcher}` (the traits C# `IUiDispatcher` became); the UI-thread-only members check the thread id at run time.
//! A wake-protocol flag (`AtomicBool`) keeps at most one `WM_APP + 1` in the thread's queue (a 10 000-entry limit) however many items are posted.
//! There is no `Drop` magic: the composition root calls [`Win32UiDispatcher::shutdown`] (C# `Dispose`) on the UI thread.
//!
//! Dropped C# members: `InvokeAsync`/`YieldAsync` (`Task`), `TryPost(SendOrPostCallback..)`; `Post` with a null action or an out-of-range
//! `UiPriority` cannot be written in Rust (the enum is closed).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use pr_core::abstractions::{
    Log, UiAction, UiActionPanicked, UiCompletion, UiDispatcher, UiPriority, UiScheduler, panic_message,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use crate::host::error::HostError;
use crate::host::message_loop::LoopDispatcher;
use crate::host::pending_panic;
use crate::win32::api::{kernel32, user32};
use crate::win32::messages::PM_NOREMOVE;
use crate::win32::structs::{Msg, new_wnd_class_ex, to_utf16z};

/// C# `WakeMessage` = `WM_APP + 1`.
pub const WAKE_MESSAGE: u32 = crate::win32::messages::WM_APP + 1;
/// C# `BatchSliceMs`.
pub const BATCH_SLICE_MS: u64 = 50;
/// Window class of the message-only window. C# `"PhotoReview.Shell.Dispatcher"`; the Rust app uses its own prefix (R0002, D-W8B3H-9).
pub const CLASS_NAME: &str = "PhotoReviewRs.Shell.Dispatcher";

/// `HWND_MESSAGE` (`(HWND)-3`): the parent of a message-only window.
const HWND_MESSAGE: isize = -3;

/// The four priority queues (C# `Queue<WorkItem>[4]` + `_highCount`), pure so the ordering rules are tested without a window (the F half of 13d).
/// Index = `UiPriority as usize`: Send 0, Normal 1, Render 2 (the "high" levels) and Background 3.
#[derive(Debug)]
pub struct UiQueues<T> {
    queues: [VecDeque<T>; 4],
    high_count: usize,
}

impl<T> UiQueues<T> {
    /// Empty queues.
    pub fn new() -> Self {
        Self {
            queues: [VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new()],
            high_count: 0,
        }
    }

    /// C# `TryEnqueue` under the gate: FIFO inside a level; a non-Background item raises the high count.
    pub fn enqueue(&mut self, priority: UiPriority, item: T) {
        // T9: `UiPriority` values are Send 0 .. Background 3, so `as usize` indexes `queues`.
        if let Some(queue) = self.queues.get_mut(priority as usize) {
            queue.push_back(item);
            if priority != UiPriority::Background {
                self.high_count += 1;
            }
        }
    }

    /// C# `TryDequeueHigh`: the first item of the lowest-numbered non-empty level among Send, Normal, Render; lowers the high count.
    pub fn dequeue_high(&mut self) -> Option<T> {
        for queue in self.queues.iter_mut().take(UiPriority::Background as usize) {
            if let Some(item) = queue.pop_front() {
                self.high_count = self.high_count.saturating_sub(1);
                return Some(item);
            }
        }
        None
    }

    /// C# `TryRunBackground`'s dequeue: a Background item only when `high_count == 0`.
    pub fn dequeue_background(&mut self) -> Option<T> {
        if self.high_count > 0 {
            return None;
        }
        self.queues.get_mut(UiPriority::Background as usize)?.pop_front()
    }

    /// C# `_highCount > 0`.
    pub fn has_high(&self) -> bool {
        self.high_count > 0
    }

    /// C# `_queues[Background].Count > 0`.
    pub fn has_background(&self) -> bool {
        self.queues
            .get(UiPriority::Background as usize)
            .is_some_and(|queue| !queue.is_empty())
    }

    /// C# `Dispose`: removes everything (all levels, in level order then FIFO) and zeroes the high count.
    pub fn drain_all(&mut self) -> Vec<T> {
        let mut all = Vec::new();
        for queue in &mut self.queues {
            all.extend(queue.drain(..));
        }
        self.high_count = 0;
        all
    }
}

impl<T> Default for UiQueues<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// The wake-up flag protocol (C# `_wakePending` with `Interlocked.Exchange`): the poster sets the flag and posts `WM_APP + 1` only if it was clear;
/// the drain clears it BEFORE it reads the queues. Pure and thread-safe so both halves are tested without a window.
#[derive(Debug, Default)]
pub struct WakeFlag(AtomicBool);

impl WakeFlag {
    /// `true` when the caller must post the wake message (the flag was clear and is now set).
    pub fn try_set(&self) -> bool {
        !self.0.swap(true, Ordering::AcqRel)
    }

    /// Drain side: clear before reading the queues.
    pub fn clear(&self) {
        self.0.store(false, Ordering::Release);
    }

    /// Whether a wake message is on its way.
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// One queued item: the action and, for `invoke`, the sender that resolves the completion.
struct WorkItem {
    action: UiAction,
    completion: Option<Sender<Result<(), UiActionPanicked>>>,
}

/// What the mutex protects (C# `_gate`): the queues and the shut-down flag.
struct State {
    queues: UiQueues<WorkItem>,
    shut_down: bool,
}

/// State shared between the UI thread and the posting threads (C# fields under `_gate` plus the interlocked flags).
struct Shared {
    thread: ThreadId,
    hwnd: isize,
    log: Arc<dyn Log>,
    state: Mutex<State>,
    wake_pending: WakeFlag,
    wake_messages_posted: AtomicI64,
}

impl Shared {
    /// Locks the state; a poisoned lock is still usable (the data are plain queues, no invariant spans a panic).
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

thread_local! {
    /// C# `[ThreadStatic] current`.
    static CURRENT: RefCell<Option<Win32UiDispatcher>> = const { RefCell::new(None) };
}

/// Process-wide "window class registered" flag (C# `ClassGate` + `classRegistered`); only a success is remembered.
static CLASS_REGISTERED: Mutex<bool> = Mutex::new(false);

/// C# `Win32UiDispatcher`.
#[derive(Clone)]
pub struct Win32UiDispatcher {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Win32UiDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32UiDispatcher").finish_non_exhaustive()
    }
}

impl Win32UiDispatcher {
    /// C# constructor: creates the dispatcher for the CURRENT thread (which must pump messages: `MessageLoop`). Registers the window class once per
    /// process and creates the message-only window (`HWND_MESSAGE` parent). Errors: a second dispatcher on this thread is
    /// `InvalidOperation { message: "This thread already has a Win32UiDispatcher." }`; `Win32 "RegisterClassEx (dispatcher window) failed"`;
    /// `Win32 "CreateWindowEx (message-only dispatcher window) failed"`. Registers itself as this thread's current dispatcher.
    pub fn new(log: Arc<dyn Log>) -> Result<Self, HostError> {
        if CURRENT.with(|current| current.borrow().is_some()) {
            return Err(HostError::InvalidOperation {
                message: "This thread already has a Win32UiDispatcher.",
            });
        }

        ensure_class_registered()?;
        let instance = kernel32::get_module_handle().unwrap_or(0);
        // SAFETY: the parent is `HWND_MESSAGE`; `param` is 0 and `dispatcher_wnd_proc` expects no creation data.
        let created =
            unsafe { user32::create_window_ex(0, CLASS_NAME, "", 0, 0, 0, 0, 0, HWND_MESSAGE, 0, instance, 0) };
        let hwnd = created.map_err(|error| HostError::Win32 {
            operation: "CreateWindowEx (message-only dispatcher window) failed",
            error,
        })?;

        let dispatcher = Self {
            shared: Arc::new(Shared {
                thread: std::thread::current().id(),
                hwnd,
                log,
                state: Mutex::new(State {
                    queues: UiQueues::new(),
                    shut_down: false,
                }),
                wake_pending: WakeFlag::default(),
                wake_messages_posted: AtomicI64::new(0),
            }),
        };
        CURRENT.with(|current| *current.borrow_mut() = Some(dispatcher.clone()));
        Ok(dispatcher)
    }

    /// C# static `FromCurrentThread`: the dispatcher created on this thread, if it is not shut down.
    pub fn from_current_thread() -> Option<Win32UiDispatcher> {
        CURRENT
            .with(|current| current.borrow().clone())
            .filter(|dispatcher| !dispatcher.is_shut_down())
    }

    /// C# `MessageWindow`.
    pub fn message_window(&self) -> isize {
        self.shared.hwnd
    }

    /// C# `WakeMessagesPosted`: how many `WM_APP + 1` were posted (tests: a flood of posts must not flood the queue).
    pub fn wake_messages_posted(&self) -> i64 {
        self.shared.wake_messages_posted.load(Ordering::Acquire)
    }

    /// C# `IsShutDown`.
    pub fn is_shut_down(&self) -> bool {
        self.shared.lock().shut_down
    }

    /// C# `HasHighPriorityWork`: Send + Normal + Render items waiting. Thread-safe.
    pub fn has_high_priority_work(&self) -> bool {
        self.shared.lock().queues.has_high()
    }

    /// C# `HasBackgroundWork`. Thread-safe.
    pub fn has_background_work(&self) -> bool {
        self.shared.lock().queues.has_background()
    }

    /// C# `VerifyAccess()`: `Err(InvalidOperation { message: "This Win32UiDispatcher member must be called on its UI thread." })` off the UI thread.
    pub fn verify_access(&self) -> Result<(), HostError> {
        if self.check_access() {
            Ok(())
        } else {
            Err(HostError::InvalidOperation {
                message: "This Win32UiDispatcher member must be called on its UI thread.",
            })
        }
    }

    /// C# `RunHighPriorityBatch()` (UI thread): clears the wake flag BEFORE reading the queues, then runs Send, Normal, Render items by priority until
    /// none is left or `BATCH_SLICE_MS` elapsed (checked after each item).
    /// Called off the UI thread it does nothing (and logs the error): the C# throws, Rust never panics here (contract open question 5).
    pub fn run_high_priority_batch(&self) {
        if self.reject_wrong_thread() {
            return;
        }
        // T4: clear BEFORE reading the queues, so an item enqueued during the drain either is seen by it or posts a fresh wake.
        self.shared.wake_pending.clear();
        let started = Instant::now();
        let slice = Duration::from_millis(BATCH_SLICE_MS);
        while let Some(item) = self.dequeue_high() {
            self.execute(item);
            if started.elapsed() >= slice {
                break;
            }
        }
    }

    /// C# `TryRunBackground()` (UI thread): one Background item if no high-priority work waits; `false` when nothing ran.
    pub fn try_run_background(&self) -> bool {
        if self.reject_wrong_thread() {
            return false;
        }
        let item = self.shared.lock().queues.dequeue_background();
        match item {
            Some(item) => {
                self.execute(item);
                true
            }
            None => false,
        }
    }

    /// C# `OnWakeMessage()` (the dispatcher window's procedure calls it for `WM_APP + 1`, also inside a foreign modal loop): drain high priority;
    /// run one Background item if the thread's message queue is empty; wake again if more runnable work remains.
    pub fn on_wake_message(&self) {
        self.run_high_priority_batch();
        let mut ran_background = false;
        if !self.has_high_priority_work() && self.has_background_work() && !is_message_pending() {
            ran_background = self.try_run_background();
        }
        if self.has_high_priority_work() || (ran_background && self.has_background_work()) {
            self.request_wake();
        }
    }

    /// C# `Dispose()` (UI thread): stops accepting work; pending `post`s are dropped, pending `invoke` completions resolve as dropped
    /// (`UiActionPanicked { message: "UI scheduler dropped the action" }`, the `UiCompletion` rule); destroys the message-only window; clears the
    /// thread's current dispatcher. Idempotent.
    pub fn shutdown(&self) {
        if self.reject_wrong_thread() {
            return;
        }
        let abandoned = {
            let mut state = self.shared.lock();
            if state.shut_down {
                return;
            }
            state.shut_down = true;
            state.queues.drain_all()
        };
        // T7: the items are dropped outside the mutex (their destructors may post).
        drop(abandoned);

        // A failure (the window is already gone) is ignored, like the C# call.
        let _ = user32::destroy_window(self.shared.hwnd);
        CURRENT.with(|current| {
            let mut current = current.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|dispatcher| Arc::ptr_eq(&dispatcher.shared, &self.shared))
            {
                *current = None;
            }
        });
    }

    /// `true` (after logging) when the caller is not on the UI thread.
    fn reject_wrong_thread(&self) -> bool {
        match self.verify_access() {
            Ok(()) => false,
            Err(error) => {
                self.shared.log.error(&error.to_string(), Some(&error));
                true
            }
        }
    }

    fn dequeue_high(&self) -> Option<WorkItem> {
        self.shared.lock().queues.dequeue_high()
    }

    /// C# private `TryEnqueue`: `false` after shutdown. Increments the high count for non-Background items, then `request_wake`.
    fn try_enqueue(&self, item: WorkItem, priority: UiPriority) -> bool {
        let mut state = self.shared.lock();
        if state.shut_down {
            drop(state);
            drop(item); // resolves an `invoke` completion as dropped
            return false;
        }
        state.queues.enqueue(priority, item);
        drop(state);
        self.request_wake();
        true
    }

    /// C# private `RequestWake()`: the flag protocol. Swap the flag to true; if it was false, `PostMessage(hwnd, WAKE_MESSAGE, 0, 0)`; on failure clear
    /// the flag again (the next post retries) and log a warning with the error code; on success count it.
    fn request_wake(&self) {
        if !self.shared.wake_pending.try_set() {
            return;
        }
        // SAFETY: `WAKE_MESSAGE` with `wparam = lparam = 0` carries no pointer, so nothing has to outlive the call.
        let posted = unsafe { user32::post_message(self.shared.hwnd, WAKE_MESSAGE, 0, 0) };
        match posted {
            Ok(()) => {
                self.shared.wake_messages_posted.fetch_add(1, Ordering::AcqRel);
            }
            Err(error) => {
                // Queue full or window destroyed: the next post retries; our loop still sees the work through `has_high_priority_work`.
                self.shared.wake_pending.clear();
                self.shared.log.warn(&format!(
                    "Win32UiDispatcher: PostMessage(WM_APP+1) failed (error {})",
                    error.0
                ));
            }
        }
    }

    /// C# private `Execute(in WorkItem)`: runs the action under `catch_unwind`. With a completion: send `Ok(())` or `Err(UiActionPanicked { message })`
    /// (no log). Without one (`post`): log `"Win32UiDispatcher: a posted UI action threw; the message loop keeps running"` plus the panic text via
    /// `Log::error` and continue (D-W8B3H-10).
    fn execute(&self, item: WorkItem) {
        // T3: no lock is held here; the action may post or shut down.
        let WorkItem { action, completion } = item;
        let outcome = catch_unwind(AssertUnwindSafe(action)).map_err(|payload| UiActionPanicked {
            message: panic_message(payload.as_ref()),
        });
        match (completion, outcome) {
            (Some(sender), outcome) => {
                // Nobody waits any more: ignored.
                let _ = sender.send(outcome);
            }
            (None, Ok(())) => {}
            (None, Err(panicked)) => {
                self.shared.log.error(
                    "Win32UiDispatcher: a posted UI action threw; the message loop keeps running",
                    Some(&panicked),
                );
            }
        }
    }
}

/// C# `MessageLoop.IsMessagePending()`: `PeekMessage(.., PM_NOREMOVE)` without keeping the message. (Same call as the loop's; kept local so the
/// dispatcher does not depend on the loop's implementation, only on its `LoopDispatcher` seam.)
fn is_message_pending() -> bool {
    let mut msg = Msg::default();
    user32::peek_message(&mut msg, 0, 0, 0, PM_NOREMOVE)
}

/// C# `EnsureClassRegistered`: once per process; only a success is remembered.
fn ensure_class_registered() -> Result<(), HostError> {
    let mut registered = CLASS_REGISTERED.lock().unwrap_or_else(PoisonError::into_inner);
    if *registered {
        return Ok(());
    }
    let class_name = to_utf16z(CLASS_NAME);
    let mut class = new_wnd_class_ex();
    class.lpfnWndProc = Some(dispatcher_wnd_proc);
    class.hInstance =
        windows::Win32::Foundation::HINSTANCE(kernel32::get_module_handle().unwrap_or(0) as *mut core::ffi::c_void);
    class.lpszClassName = windows::core::PCWSTR(class_name.as_ptr());
    // SAFETY: `class_name` (NUL-terminated UTF-16) outlives the call and the OS copies it; `lpfnWndProc` is our `dispatcher_wnd_proc`, valid for the
    // life of the process.
    unsafe { user32::register_class_ex(&class) }.map_err(|error| HostError::Win32 {
        operation: "RegisterClassEx (dispatcher window) failed",
        error,
    })?;
    *registered = true;
    Ok(())
}

impl UiScheduler for Win32UiDispatcher {
    /// C# `Post(action)` = `Post(action, Normal)`; thread-safe; dropped silently after shutdown.
    fn post(&self, action: UiAction) {
        self.post_with_priority(action, UiPriority::Normal);
    }

    /// C# `InvokeAsync(action)` = Normal. The completion resolves after the action ran on the UI thread; after shutdown it resolves as dropped.
    fn invoke(&self, action: UiAction) -> UiCompletion {
        self.invoke_with_priority(action, UiPriority::Normal)
    }
}

impl UiDispatcher for Win32UiDispatcher {
    /// C# `CheckAccess()`: the calling thread is the UI thread.
    fn check_access(&self) -> bool {
        std::thread::current().id() == self.shared.thread
    }

    fn post_with_priority(&self, action: UiAction, priority: UiPriority) {
        let _ = self.try_enqueue(
            WorkItem {
                action,
                completion: None,
            },
            priority,
        );
    }

    fn invoke_with_priority(&self, action: UiAction, priority: UiPriority) -> UiCompletion {
        let (sender, completion) = UiCompletion::channel();
        // After shutdown the item (and so the sender) is dropped: the completion resolves as dropped.
        let _ = self.try_enqueue(
            WorkItem {
                action,
                completion: Some(sender),
            },
            priority,
        );
        completion
    }
}

impl LoopDispatcher for Win32UiDispatcher {
    fn verify_access(&self) -> Result<(), HostError> {
        Win32UiDispatcher::verify_access(self)
    }
    fn has_high_priority_work(&self) -> bool {
        Win32UiDispatcher::has_high_priority_work(self)
    }
    fn run_high_priority_batch(&self) {
        Win32UiDispatcher::run_high_priority_batch(self);
    }
    fn has_background_work(&self) -> bool {
        Win32UiDispatcher::has_background_work(self)
    }
    fn try_run_background(&self) -> bool {
        Win32UiDispatcher::try_run_background(self)
    }
}

/// C# `WndProcThunk.DispatcherWndProc` (relocated here from `wndproc.rs` so 13d does not depend on 13e, D-W8B3H-11): the window procedure of the
/// message-only window. `WAKE_MESSAGE` -> `from_current_thread()` -> `on_wake_message()`, result 0; anything else `DefWindowProcW`. Runs under
/// `pending_panic::guarded(LRESULT(0), ..)`: it never unwinds into the OS. Declared with the `windows` types so it can be stored in
/// `WNDCLASSEXW::lpfnWndProc` without a cast.
///
/// # Safety
/// Only the OS calls it, as a `WNDPROC`, with the arguments of one message on the thread that created the window.
pub unsafe extern "system" fn dispatcher_wnd_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    pending_panic::guarded(
        LRESULT(0),
        AssertUnwindSafe(|| {
            if message == WAKE_MESSAGE {
                if let Some(dispatcher) = Win32UiDispatcher::from_current_thread() {
                    dispatcher.on_wake_message();
                }
                LRESULT(0)
            } else {
                // SAFETY: the parameters are forwarded unchanged from the OS call that invoked this procedure.
                LRESULT(unsafe { user32::def_window_proc(hwnd.0 as isize, message, wparam.0, lparam.0) })
            }
        }),
    )
}
