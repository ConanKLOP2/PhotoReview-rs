//! Shared helper of the native host tests (W8-13c; 13d and 13e reuse it): a real UI thread that builds a dispatcher and a `MessageLoop` and runs
//! it until `dispose`. Port of `tests/PhotoReview.Shell.Tests/Hosting/UiThread.cs` (baseline `d92d6aae`), minus what needs `Win32UiDispatcher`.
//!
//! Generic over the dispatcher: [`UiThread::start_with`] takes a builder that runs ON the UI thread and returns the `LoopDispatcher` the loop uses
//! plus a `post` function (callable from any thread) that queues a task on that dispatcher. [`UiThread::start`] uses the small [`TestDispatcher`] of
//! this file (a queue + `PostThreadMessage` wake); W8-13d passes the real `Win32UiDispatcher` the same way. Every wait is bounded ([`BOUND`]), no
//! fixed sleeps. Tasks reach the loop through [`ui_loop`] (a thread-local), so no `!Send` value crosses threads.
#![allow(dead_code)] // shared by several test crates, each of which uses a different part

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::Duration;

use pr_platform_win::host::error::HostError;
use pr_platform_win::host::message_loop::{LoopDispatcher, MessageLoop};
use pr_platform_win::win32::structs::Msg;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{PM_NOREMOVE, PeekMessageW, PostThreadMessageW};

/// Hang guard, not a speed budget (a cold first UI thread on a busy 2 vCPU runner can be slow).
pub const BOUND: Duration = Duration::from_secs(60);

/// `WM_APP + 1`: the wake message of the test dispatcher (a thread message; `DispatchMessage` ignores it).
const WAKE: u32 = 0x8000 + 1;

/// A task that runs on the UI thread.
pub type Task = Box<dyn FnOnce() + Send>;

/// Queues a task on the dispatcher of the UI thread; callable from any thread.
pub type Poster = Arc<dyn Fn(Task) + Send + Sync>;

/// What the builder of [`UiThread::start_with`] returns (built on the UI thread).
pub struct UiParts {
    pub dispatcher: Rc<dyn LoopDispatcher>,
    pub post: Poster,
}

thread_local! {
    static LOOP: RefCell<Option<Rc<MessageLoop>>> = const { RefCell::new(None) };
}

/// The `MessageLoop` of the calling UI thread (call it from a task).
pub fn ui_loop() -> Rc<MessageLoop> {
    LOOP.with(|slot| slot.borrow().clone())
        .expect("ui_loop() must be called on a UiThread task")
}

/// A running UI thread.
pub struct UiThread {
    post: Poster,
    thread_id: ThreadId,
    exit: Receiver<Result<i32, HostError>>,
    join: Option<JoinHandle<()>>,
}

impl UiThread {
    /// A UI thread with the built-in [`TestDispatcher`].
    pub fn start() -> Self {
        Self::start_with(|| {
            let dispatcher = Rc::new(TestDispatcher::new());
            let post = dispatcher.poster();
            UiParts {
                dispatcher: dispatcher as Rc<dyn LoopDispatcher>,
                post,
            }
        })
    }

    /// A UI thread with a dispatcher built on that thread.
    pub fn start_with(build: impl FnOnce() -> UiParts + Send + 'static) -> Self {
        let (ready_tx, ready_rx) = channel::<Result<Poster, HostError>>();
        let (exit_tx, exit_rx) = channel::<Result<i32, HostError>>();
        let join = thread::Builder::new()
            .name("PhotoReviewRs test UI thread".to_owned())
            .spawn(move || {
                // Create this thread's message queue before anybody can `PostThreadMessage` to it.
                let mut scratch = Msg::default();
                // SAFETY: `scratch` is a live MSG; PM_NOREMOVE leaves the queue untouched (this call only creates the queue).
                let _ = unsafe { PeekMessageW(&mut scratch, None, 0, 0, PM_NOREMOVE) };
                let parts = build();
                let message_loop = match MessageLoop::new(parts.dispatcher) {
                    Ok(message_loop) => Rc::new(message_loop),
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.clone()));
                        let _ = exit_tx.send(Err(error));
                        return;
                    }
                };
                LOOP.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&message_loop)));
                let _ = ready_tx.send(Ok(parts.post));
                let result = message_loop.run();
                LOOP.with(|slot| *slot.borrow_mut() = None);
                let _ = exit_tx.send(result);
            })
            .expect("spawn the UI thread");
        let post = ready_rx
            .recv_timeout(BOUND)
            .expect("the UI thread did not start in time")
            .expect("the message loop could not be created");
        Self {
            post,
            thread_id: join.thread().id(),
            exit: exit_rx,
            join: Some(join),
        }
    }

    /// `std` id of the UI thread (to compare with `thread::current().id()` inside handlers).
    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    /// Runs `task` on the UI thread and returns its result (C# `InvokeAsync`); bounded.
    pub fn invoke<R: Send + 'static>(&self, task: impl FnOnce() -> R + Send + 'static) -> R {
        let (tx, rx) = channel();
        (self.post)(Box::new(move || {
            let _ = tx.send(task());
        }));
        rx.recv_timeout(BOUND)
            .expect("the UI thread did not run the task in time")
    }

    /// Ends the loop with exit code `0` and joins the thread. Returns the loop's result (`Err` when the loop failed).
    pub fn dispose(mut self) -> Result<i32, HostError> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> Result<i32, HostError> {
        let Some(join) = self.join.take() else {
            return Ok(0);
        };
        (self.post)(Box::new(|| MessageLoop::quit(0)));
        let result = self
            .exit
            .recv_timeout(BOUND)
            .expect("the UI thread did not exit in time");
        join.join().expect("the UI thread panicked");
        result
    }
}

impl Drop for UiThread {
    fn drop(&mut self) {
        if self.join.is_some() && !thread::panicking() {
            let _ = self.shutdown();
        }
    }
}

/// Queue + `PostThreadMessage` wake: the smallest `LoopDispatcher` that tasks can be posted to from another thread.
pub struct TestDispatcher {
    owner: u32,
    queue: Arc<Mutex<VecDeque<Task>>>,
}

impl TestDispatcher {
    /// Must be created on the UI thread.
    pub fn new() -> Self {
        Self {
            // SAFETY: no arguments.
            owner: unsafe { GetCurrentThreadId() },
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// The cross-thread half.
    pub fn poster(&self) -> Poster {
        let queue = Arc::clone(&self.queue);
        let owner = self.owner;
        Arc::new(move |task| {
            queue.lock().expect("task queue").push_back(task);
            // SAFETY: plain integer arguments; the target thread created its queue before the poster was handed out.
            let _ = unsafe { PostThreadMessageW(owner, WAKE, Default::default(), Default::default()) };
        })
    }
}

impl LoopDispatcher for TestDispatcher {
    fn verify_access(&self) -> Result<(), HostError> {
        // SAFETY: no arguments.
        if unsafe { GetCurrentThreadId() } == self.owner {
            Ok(())
        } else {
            Err(HostError::InvalidOperation {
                message: "The calling thread cannot access this object because a different thread owns it.",
            })
        }
    }

    fn has_high_priority_work(&self) -> bool {
        !self.queue.lock().expect("task queue").is_empty()
    }

    fn run_high_priority_batch(&self) {
        loop {
            let task = self.queue.lock().expect("task queue").pop_front();
            match task {
                Some(task) => task(),
                None => break,
            }
        }
    }

    fn has_background_work(&self) -> bool {
        false
    }

    fn try_run_background(&self) -> bool {
        false
    }
}
