//! Port of `PhotoReview.App.Shared/Coordinators/FileActionGate.cs` (152 lines, baseline `d92d6aae`): the single mutual-exclusion
//! gate for file actions (Move / Copy / Recycle / run-action) and for Undo and duplicate cleanup (OC14, Q-T1, R7-7, RV-A01;
//! INV-4 caller side). Contract: `contracts/W8-06e.md`. Data-safety critical: read the contract before touching it.
//!
//! Two ways to hold the gate, mutually exclusive with each other:
//! - [`FileActionGate::run_exclusive`] / [`FileActionGate::try_enter`] (Undo, duplicate cleanup): refused (the work is NOT run,
//!   `false`) while anything holds the gate, queued actions included.
//! - [`FileActionGate::run_queued`] (Move / Delete / Copy review actions): FIFO. Every accepted action runs exactly once, one at
//!   a time, in submission order, and is never dropped; refused only while an EXCLUSIVE holder exists.
//!
//! D-W8-2 (no futures): the C# `Func<Task> work` becomes `FnOnce(GateTurn)`. The gate is held from the moment the work is
//! accepted until its [`GateTurn`] is finished (or dropped). The work keeps the turn for as long as the action lasts, usually
//! moving it into the completion callback that runs on the UI thread after the worker finished (INV-4: the gate must cover the
//! catalog/undo update that follows the file operation, not just the worker call). `GateTurn` is `Send`.
//! "Never inline on the finishing stack" (C# `RunContinuationsAsynchronously`) is kept by posting the successor and every
//! released-waiter through the injected [`UiScheduler`]; user work never runs under the internal lock (RV-A01).
//!
//! Pure module: std + `pr_core::abstractions::{UiScheduler, UiAction}`; no `unsafe`, no `windows` crate.
//! NOT the `pr_core::file_actions::undo::FileActionGate` trait (the service-level CAS gate `try_begin/end/is_busy`
//! that `FileActionService` implements): that is a different gate, owned by pr-core.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{UiAction, UiScheduler};

/// A queued action that has not started yet (C# `Func<Task> work` waiting behind `_tail`).
type QueuedJob = Box<dyn FnOnce(GateTurn) + Send + 'static>;

/// How `run_queued` disposed of an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedOutcome {
    /// C# `Task.FromResult(false)`: an exclusive holder owns the gate. The work was NOT run and nothing was queued.
    Rejected,
    /// The gate was idle: the work started synchronously, inside this call, on the caller's thread (so a confirmation
    /// dialog in its prefix appears without a dispatcher hop). Its turn may still be unfinished.
    StartedNow,
    /// Accepted behind earlier actions (or submitted re-entrantly from a running action's prefix): the work will run later,
    /// posted through the scheduler when its predecessor finishes. It is never dropped.
    Queued,
}

impl QueuedOutcome {
    /// True for `StartedNow` and `Queued` (the C# task would complete with `true`).
    pub fn accepted(self) -> bool {
        self != Self::Rejected
    }
}

/// What a [`GateTurn`] releases when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnKind {
    /// An exclusive hold: ending it is `exit()`.
    Exclusive,
    /// A queued action's slot: ending it starts the next queued action (posted), or wakes the released-waiters.
    Queued,
}

/// The right to run one action under the gate. Ending it (call [`GateTurn::finish`], or just drop it) is the C# `finally`
/// block of `RunExclusiveAsync` / `RunAfterAsync`: it releases the gate on EVERY path, including a panic of the work
/// (unwinding drops the turn) and a work that forgets to finish (a leak would block every later action forever, so Drop
/// releases). Finishing twice is impossible (`finish` consumes the turn). `Send`: it may be finished from a worker thread
/// or from a UI completion.
#[must_use = "dropping the turn releases the gate at once; keep it until the action has fully completed"]
pub struct GateTurn {
    inner: Arc<GateInner>,
    kind: TurnKind,
    finished: bool,
}

impl GateTurn {
    /// Ends the turn now (same effect as dropping it).
    pub fn finish(mut self) {
        self.release();
    }

    fn release(&mut self) {
        if !self.finished {
            self.finished = true;
            self.inner.release(self.kind);
        }
    }
}

impl Drop for GateTurn {
    /// Releases the gate if `finish` was not called (idempotent with it).
    fn drop(&mut self) {
        self.release();
    }
}

impl fmt::Debug for GateTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GateTurn")
            .field("kind", &self.kind)
            .field("finished", &self.finished)
            .finish()
    }
}

/// The lock-protected state (C# `_held`, `_queueLength`, `_tail`, `_released`).
struct GateState {
    /// C# `_held != 0`: an exclusive holder exists (`try_enter` / `run_exclusive`).
    held: bool,
    /// C# `_queueLength`: accepted queued actions that have not finished (the running one and the waiting ones).
    queue_length: usize,
    /// True while the head queued action has started and has not finished. A new `run_queued` starts inline only when this
    /// is false AND `pending` is empty; otherwise it waits (RV-A01: a re-entrant submission from a running prefix sees the slot).
    running: bool,
    /// Accepted actions waiting for their turn, in submission order (the Rust form of the `_tail` chain).
    pending: VecDeque<QueuedJob>,
    /// C# `_released`: callbacks of `when_released` that registered while the gate was held.
    released_waiters: Vec<UiAction>,
}

struct GateInner {
    state: Mutex<GateState>,
    scheduler: Arc<dyn UiScheduler>,
}

impl GateInner {
    fn post_waiters(&self, waiters: Vec<UiAction>) {
        for waiter in waiters {
            self.scheduler.post(waiter);
        }
    }

    fn release(self: &Arc<Self>, kind: TurnKind) {
        let (next, waiters) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let next = match kind {
                TurnKind::Exclusive => {
                    state.held = false;
                    None
                }
                TurnKind::Queued => {
                    state.queue_length -= 1;
                    state.running = false;
                    let next = state.pending.pop_front();
                    if next.is_some() {
                        state.running = true;
                    }
                    next
                }
            };
            let waiters = if !state.held && state.queue_length == 0 {
                std::mem::take(&mut state.released_waiters)
            } else {
                Vec::new()
            };
            (next, waiters)
        };

        if let Some(job) = next {
            let inner = Arc::clone(self);
            self.scheduler.post(Box::new(move || {
                job(GateTurn {
                    inner,
                    kind: TurnKind::Queued,
                    finished: false,
                });
            }));
        }
        self.post_waiters(waiters);
    }
}

/// C# `FileActionGate`. Cheap to clone (all clones share one gate); `Send + Sync`; every method may be called from any thread
/// and never blocks behind user work.
#[derive(Clone)]
pub struct FileActionGate {
    inner: Arc<GateInner>,
}

impl FileActionGate {
    /// C# `new FileActionGate()`, plus the scheduler that runs deferred continuations (the UI dispatcher in the app, a
    /// recording fake in tests). The scheduler must run every posted action eventually, or queued actions stall.
    pub fn new(scheduler: Arc<dyn UiScheduler>) -> Self {
        Self {
            inner: Arc::new(GateInner {
                state: Mutex::new(GateState {
                    held: false,
                    queue_length: 0,
                    running: false,
                    pending: VecDeque::new(),
                    released_waiters: Vec::new(),
                }),
                scheduler,
            }),
        }
    }

    /// C# `IsHeld`: an exclusive holder exists, or a queued action is pending or running. Never blocks behind user work.
    pub fn is_held(&self) -> bool {
        let state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
        state.held || state.queue_length != 0
    }

    /// C# `TryEnter()`: takes the gate exclusively. `false` when it is held by either mechanism (exclusive holder, or any
    /// queued action pending or running). Exactly one of N simultaneous callers wins.
    pub fn try_enter(&self) -> bool {
        let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.held || state.queue_length != 0 {
            false
        } else {
            state.held = true;
            true
        }
    }

    /// C# `Exit()`: releases the exclusive hold (sets it to free, unconditionally; harmless when nothing is held) and, if the
    /// gate is now completely free (no exclusive holder, empty queue), posts every `when_released` waiter through the scheduler.
    pub fn exit(&self) {
        let waiters = {
            let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
            state.held = false;
            if state.queue_length == 0 {
                std::mem::take(&mut state.released_waiters)
            } else {
                Vec::new()
            }
        };
        self.inner.post_waiters(waiters);
    }

    /// C# `WhenReleasedAsync()` (R7-7). When the gate is completely free (`!held && queue_length == 0`) the waiter runs NOW,
    /// inline in this call (the C# task is already complete) and the result is `true`. Otherwise the waiter is stored and the
    /// result is `false`; it is posted through the scheduler (never inline on the releasing stack) the next time the gate
    /// becomes completely free. A waiter that registers while the holder exits is never lost: registration and the free
    /// check happen under the same lock as the release. A caller that needs the C# `while (IsHeld) await WhenReleasedAsync()`
    /// loop re-checks `is_held` in the waiter.
    pub fn when_released(&self, waiter: UiAction) -> bool {
        {
            let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.held || state.queue_length != 0 {
                state.released_waiters.push(waiter);
                return false;
            }
        }
        waiter();
        true
    }

    /// C# `RunExclusiveAsync(work)` (Undo, duplicate cleanup). `false` and `work` NOT called when the gate is held by either
    /// mechanism. Otherwise takes the gate, calls `work(turn)` synchronously on this thread, and returns `true` ("started").
    /// The gate stays held until the turn is finished; a panic in `work` unwinds through the turn's Drop (released) and then
    /// propagates to the caller.
    pub fn run_exclusive<F>(&self, work: F) -> bool
    where
        F: FnOnce(GateTurn),
    {
        if !self.try_enter() {
            return false;
        }
        work(GateTurn {
            inner: Arc::clone(&self.inner),
            kind: TurnKind::Exclusive,
            finished: false,
        });
        true
    }

    /// C# `RunQueuedAsync(work)` (Q-T1, RV-A01).
    /// - an exclusive holder exists: `Rejected`, `work` not run, nothing queued;
    /// - else the action takes a FIFO slot under the lock (`queue_length` +1) BEFORE any user code runs;
    /// - the gate idle (no running action, nothing pending): `StartedNow`, `work(turn)` is called synchronously, OUTSIDE the lock;
    /// - otherwise `Queued`: stored in `pending`; when the predecessor's turn ends, the next job is POSTED through the
    ///   scheduler (never started inline on the finishing stack) and then receives its own turn.
    ///
    /// A failure of one action never stops the queue: its turn ends (also on panic), the next is posted. The work reports its
    /// own result through its own channel; the gate does not carry results.
    pub fn run_queued<F>(&self, work: F) -> QueuedOutcome
    where
        F: FnOnce(GateTurn) + Send + 'static,
    {
        {
            let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.held {
                return QueuedOutcome::Rejected;
            }
            state.queue_length += 1;
            if state.running || !state.pending.is_empty() {
                state.pending.push_back(Box::new(work));
                return QueuedOutcome::Queued;
            }
            state.running = true;
        }

        work(GateTurn {
            inner: Arc::clone(&self.inner),
            kind: TurnKind::Queued,
            finished: false,
        });
        QueuedOutcome::StartedNow
    }
}

impl fmt::Debug for FileActionGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileActionGate").finish_non_exhaustive()
    }
}
