//! Port of `PhotoReview.Imaging.LibRaw/FullDecodeGate.cs` (C# baseline d92d6aae, 156 lines).
//!
//! Single-slot gate serialising LibRaw full decodes (each holds hundreds of MB), with two lanes: waiting
//! [`SourceReadPriority::Viewer`] decodes are served before waiting [`SourceReadPriority::Preload`] decodes, except that the
//! OLDEST preload waiter that has seen [`MAX_PRELOAD_AGE_COMPLETIONS`] worked decodes finish since it queued (and since the
//! last aged promotion) is promoted ahead of newer viewer waiters. Only leases marked with
//! [`FullDecodeLease::mark_worked`] advance that clock (no wall time). A running decode is never interrupted, the number of
//! queued preload waiters is bounded (extra preload requests fail fast with [`GateError::Busy`]), and a queued waiter is
//! cancellable without disturbing the slot. Every acquisition returns a lease that releases exactly once.
//!
//! Thread model (docs/adr/R0003): blocking, no async. One `Mutex<GateState>` replaces the C# `lock (_sync)`; one `Condvar`
//! replaces the per-waiter `SemaphoreSlim`. [`CancelToken`] is only a flag (contracts/README.md A7/A10 not applicable yet:
//! `CancelToken::wait_timeout` does not exist in `pr-core`), so a queued waiter re-checks it every [`CANCEL_POLL_INTERVAL`].
//! DIVERGENCE: C# wakes a cancelled waiter at once; here it notices within 10 ms. Grants are never delayed (condvar).
//! The waiter-queued hook and every lease release run OUTSIDE the lock's critical section of the waiter that calls them;
//! nothing here does file I/O.
//!
//! No `unsafe`, no FFI: this file is plain Rust although it lives in the LibRaw decoder crate.

use std::collections::VecDeque;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use pr_core::abstractions::SourceReadPriority;
use pr_core::sync::{CancelToken, Canceled};
use pr_imaging::decoder_traits::DecodeError;

/// C# `private const int MaxPreloadAgeCompletions = 3`: worked completions a queued preload waits before it is promoted
/// ahead of newer viewer waiters (and the spacing between two promotions). `i64` because it is compared with the `long`
/// completion clock.
pub const MAX_PRELOAD_AGE_COMPLETIONS: i64 = 3;

/// Message of the `DecoderBusyException` thrown when the preload queue is full (C# literal, verbatim).
pub const PRELOAD_QUEUE_BUSY_MESSAGE: &str = "The RAW full-decode queue is busy; this background decode was skipped.";

/// How often a queued waiter re-checks its [`CancelToken`] while it waits for the slot (C# wakes on cancellation at once;
/// a flag cannot wake a condvar). A grant always wakes the waiter immediately through the condvar.
pub const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// C# `Action<SourceReadPriority>? waiterQueued`: test seam invoked once per QUEUED waiter (never for an immediate grant,
/// never for a refused preload), after the waiter is in its lane and outside the gate lock. `Err(message)` is the C#
/// "seam throws": the waiter leaves the queue (or passes on a racing grant) exactly like a cancellation and
/// [`FullDecodeGate::enter`] returns [`GateError::WaiterHook`]. A panic in the hook is cleaned up the same way, then resumed.
pub type WaiterQueuedHook = Arc<dyn Fn(SourceReadPriority) -> Result<(), String> + Send + Sync>;

/// Why [`FullDecodeGate::enter`] did not grant the slot. In every case the gate is left exactly as if the call never
/// happened (slot owner unchanged, the caller is in no queue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    /// C# `OperationCanceledException`: the token was already canceled on entry (checked before the slot is looked at),
    /// or was canceled while queued.
    Canceled,
    /// C# `DecoderBusyException(PRELOAD_QUEUE_BUSY_MESSAGE)`: a preload found the slot taken and
    /// `queued_preloads >= max_queued_preloads`. Viewers are never refused.
    Busy { message: String },
    /// The [`WaiterQueuedHook`] returned `Err(message)` (C#: the seam's exception is rethrown unchanged).
    WaiterHook { message: String },
}

impl fmt::Display for GateError {
    /// `Canceled` => the `Canceled` text ("operation canceled"); `Busy`/`WaiterHook` => `message`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canceled => fmt::Display::fmt(&Canceled, f),
            Self::Busy { message } | Self::WaiterHook { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for GateError {}

impl From<Canceled> for GateError {
    fn from(_: Canceled) -> Self {
        Self::Canceled
    }
}

impl From<GateError> for DecodeError {
    /// `Canceled` => `DecodeError::Canceled`; `Busy { message }` => `DecodeError::busy_with(message)` (never fallbackable:
    /// W4-12b `is_fallbackable`, INV-12); `WaiterHook { message }` => `DecodeError::InvalidOperation { message }` (test seam only).
    fn from(error: GateError) -> Self {
        match error {
            GateError::Canceled => DecodeError::Canceled,
            GateError::Busy { message } => DecodeError::busy_with(message),
            GateError::WaiterHook { message } => DecodeError::InvalidOperation { message },
        }
    }
}

/// C# `internal sealed class FullDecodeGate`. Cloning shares the same gate (like passing the C# reference); the process-wide
/// instance of `LibRawDecoder` (W5-16c) is a `static LazyLock<FullDecodeGate>` built with `new(MAX_QUEUED_PRELOAD_DECODES)`.
#[derive(Clone)]
pub struct FullDecodeGate {
    inner: Arc<GateInner>,
}

/// Shared by the gate handle and every lease it hands out.
struct GateInner {
    state: Mutex<GateState>,
    /// Notified (`notify_all`) whenever `pending_grant` is set; each waiter checks whether the grant is its own.
    granted: Condvar,
    /// C# `_maxQueuedPreloads` (`int`, not validated: `0` or negative refuses every preload that would have to queue).
    max_queued_preloads: i32,
    /// C# `_waiterQueued`.
    waiter_queued: Option<WaiterQueuedHook>,
}

/// Everything C# guards with `lock (_sync)`.
#[derive(Debug, Default)]
struct GateState {
    /// C# `_viewerQueue` (FIFO).
    viewer_queue: VecDeque<Waiter>,
    /// C# `_preloadQueue` (FIFO; its length is the bounded `QueuedPreloads`).
    preload_queue: VecDeque<Waiter>,
    /// C# `_completions`: worked lease releases; the aging clock.
    completions: i64,
    /// C# `_lastPromotionCompletion`: `completions` at the last aged preload promotion.
    last_promotion_completion: i64,
    /// C# `_busy`: a lease (or a grant not yet picked up) owns the slot.
    busy: bool,
    /// Source of [`Waiter::id`] (monotonic, wrapping is unreachable).
    next_waiter_id: u64,
    /// The waiter the slot was handed to and that has not picked it up yet (C# `Waiter.Granted` + `Signal.Release()`).
    /// At most one: a grant happens only when the single owner releases, and nobody else can release before the grantee
    /// takes over. While `Some`, `busy` stays true.
    pending_grant: Option<u64>,
}

/// C# `private sealed class Waiter` without the semaphore (the condvar + `pending_grant` replace it) and without the
/// list node (removal by `id`; the lanes are short).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Waiter {
    id: u64,
    /// C# `EnqueuedAtCompletion`: `completions` when this waiter queued.
    enqueued_at_completion: i64,
}

/// Lane length as the C# `int` count (saturating; a lane can never be that long in practice).
fn count(lane: &VecDeque<Waiter>) -> i32 {
    i32::try_from(lane.len()).unwrap_or(i32::MAX)
}

impl fmt::Debug for FullDecodeGate {
    /// Shows `max_queued_preloads`, `busy`, both lane lengths and `completions` (the hook is not `Debug`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.inner.lock();
        f.debug_struct("FullDecodeGate")
            .field("max_queued_preloads", &self.inner.max_queued_preloads)
            .field("busy", &state.busy)
            .field("queued_viewers", &state.viewer_queue.len())
            .field("queued_preloads", &state.preload_queue.len())
            .field("completions", &state.completions)
            .finish()
    }
}

impl FullDecodeGate {
    /// C# `new FullDecodeGate(maxQueuedPreloads)`.
    pub fn new(max_queued_preloads: i32) -> Self {
        Self::build(max_queued_preloads, None)
    }

    /// C# `new FullDecodeGate(maxQueuedPreloads, waiterQueued)` (test seam).
    pub fn with_waiter_queued(max_queued_preloads: i32, waiter_queued: WaiterQueuedHook) -> Self {
        Self::build(max_queued_preloads, Some(waiter_queued))
    }

    fn build(max_queued_preloads: i32, waiter_queued: Option<WaiterQueuedHook>) -> Self {
        Self {
            inner: Arc::new(GateInner {
                state: Mutex::new(GateState::default()),
                granted: Condvar::new(),
                max_queued_preloads,
                waiter_queued,
            }),
        }
    }

    /// C# `SlotsAvailable`: 1 when the slot is free, 0 while a lease (or a grant not yet picked up) holds it.
    pub fn slots_available(&self) -> i32 {
        if self.inner.lock().busy { 0 } else { 1 }
    }

    /// C# `QueuedViewers`.
    pub fn queued_viewers(&self) -> i32 {
        count(&self.inner.lock().viewer_queue)
    }

    /// C# `QueuedPreloads`.
    pub fn queued_preloads(&self) -> i32 {
        count(&self.inner.lock().preload_queue)
    }

    /// C# `Enter(priority, cancellationToken)`: blocks until the slot is granted.
    ///
    /// Order: `cancel.check()` first (`Err(Canceled)`, slot untouched); under the lock, a free slot is taken at once (no
    /// queue, no hook, the preload bound is not consulted); otherwise a preload with `queued_preloads >= max` is refused
    /// with `Busy`; otherwise the waiter is appended to its lane stamped with `completions`. Then, outside the lock, the hook
    /// runs and the waiter waits for its grant, re-checking `cancel` every [`CANCEL_POLL_INTERVAL`]. On a hook error/panic or
    /// a cancellation: if the slot was already granted to this waiter it is passed on (`release_locked`), otherwise the
    /// waiter is removed from its lane at once (so it no longer counts toward the preload bound).
    pub fn enter(&self, priority: SourceReadPriority, cancel: &CancelToken) -> Result<FullDecodeLease, GateError> {
        cancel.check()?;
        let id = {
            let mut state = self.inner.lock();
            if !state.busy {
                state.busy = true;
                return Ok(self.lease());
            }

            let preload = priority == SourceReadPriority::Preload;
            if preload
                && i64::try_from(state.preload_queue.len()).unwrap_or(i64::MAX)
                    >= i64::from(self.inner.max_queued_preloads)
            {
                return Err(GateError::Busy {
                    message: PRELOAD_QUEUE_BUSY_MESSAGE.to_owned(),
                });
            }
            let id = state.next_waiter_id;
            state.next_waiter_id = state.next_waiter_id.wrapping_add(1);
            let waiter = Waiter {
                id,
                enqueued_at_completion: state.completions,
            };
            state.lane_mut(priority).push_back(waiter);
            id
        };

        if let Some(hook) = &self.inner.waiter_queued {
            match panic::catch_unwind(AssertUnwindSafe(|| hook(priority))) {
                Ok(Ok(())) => {}
                Ok(Err(message)) => {
                    self.inner.abandon(id, priority);
                    return Err(GateError::WaiterHook { message });
                }
                Err(payload) => {
                    self.inner.abandon(id, priority);
                    panic::resume_unwind(payload);
                }
            }
        }

        self.inner.wait_for_grant(id, priority, cancel)?;
        Ok(self.lease())
    }

    fn lease(&self) -> FullDecodeLease {
        FullDecodeLease {
            gate: Some(Arc::clone(&self.inner)),
            worked: AtomicBool::new(false),
        }
    }
}

impl GateInner {
    /// The gate lock; a poisoned lock is recovered (R0003): every critical section leaves the state consistent.
    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// C# `Release(worked)`: under the lock, `completions += 1` when `worked`, then `release_locked`; notifies the condvar
    /// when a waiter was granted.
    fn release(&self, worked: bool) {
        let granted = {
            let mut state = self.lock();
            if worked {
                state.completions += 1;
            }
            state.release_locked()
        };
        if granted {
            self.granted.notify_all();
        }
    }

    /// Blocks the calling waiter until `pending_grant == Some(id)` (takes it: `pending_grant = None`, returns `Ok`) or
    /// `cancel` is seen canceled (`Err(Canceled)`, after the cleanup described on [`FullDecodeGate::enter`]). Each iteration
    /// checks `cancel` BEFORE the grant (C# `SemaphoreSlim.Wait(token)` throws on an already-canceled token even when the
    /// count is available); a grant made to a canceled waiter is passed on, never lost.
    fn wait_for_grant(&self, id: u64, priority: SourceReadPriority, cancel: &CancelToken) -> Result<(), Canceled> {
        let mut state = self.lock();
        loop {
            if cancel.is_canceled() {
                let granted = Self::abandon_locked(&mut state, id, priority);
                drop(state);
                if granted {
                    self.granted.notify_all();
                }
                return Err(Canceled);
            }
            if state.pending_grant == Some(id) {
                state.pending_grant = None;
                return Ok(());
            }
            state = self
                .granted
                .wait_timeout(state, CANCEL_POLL_INTERVAL)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// [`abandon_locked`](Self::abandon_locked) under a fresh lock, then notifies when the slot was passed on.
    fn abandon(&self, id: u64, priority: SourceReadPriority) {
        let granted = {
            let mut state = self.lock();
            Self::abandon_locked(&mut state, id, priority)
        };
        if granted {
            self.granted.notify_all();
        }
    }

    /// Cleanup of a queued waiter that gives up (cancellation or hook failure), under the lock: pass on a racing grant,
    /// else remove `id` from the lane of `priority`. Returns whether a new grant was made (caller notifies).
    fn abandon_locked(state: &mut GateState, id: u64, priority: SourceReadPriority) -> bool {
        if state.pending_grant == Some(id) {
            // The slot was handed to this waiter in the same instant as the failure: pass it on so it is not lost.
            state.pending_grant = None;
            return state.release_locked();
        }
        let lane = state.lane_mut(priority);
        if let Some(index) = lane.iter().position(|waiter| waiter.id == id) {
            lane.remove(index);
        }
        false
    }
}

impl GateState {
    /// C# `ReleaseLocked`. `overdue = front of preload_queue exists && completions - max(front.enqueued_at_completion,
    /// last_promotion_completion) >= MAX_PRELOAD_AGE_COMPLETIONS`; when overdue, `last_promotion_completion = completions`
    /// and the preload front is granted; otherwise the viewer front, else the preload front. Nobody waiting =>
    /// `busy = false`. A grant keeps `busy == true` and sets `pending_grant`. Returns whether a grant was made.
    fn release_locked(&mut self) -> bool {
        let overdue = self.preload_queue.front().is_some_and(|oldest| {
            self.completions - oldest.enqueued_at_completion.max(self.last_promotion_completion)
                >= MAX_PRELOAD_AGE_COMPLETIONS
        });
        let next = if overdue {
            self.last_promotion_completion = self.completions;
            self.preload_queue.pop_front()
        } else {
            self.viewer_queue.pop_front().or_else(|| self.preload_queue.pop_front())
        };
        match next {
            None => {
                self.busy = false;
                false
            }
            Some(waiter) => {
                // The slot stays busy: ownership moves straight to the waiter.
                self.pending_grant = Some(waiter.id);
                true
            }
        }
    }

    /// The lane a priority queues in (C#: `priority == Preload ? _preloadQueue : _viewerQueue`).
    fn lane_mut(&mut self, priority: SourceReadPriority) -> &mut VecDeque<Waiter> {
        if priority == SourceReadPriority::Preload {
            &mut self.preload_queue
        } else {
            &mut self.viewer_queue
        }
    }
}

/// C# `FullDecodeGate.Lease`: holds the slot; releasing more than once releases only once. `Drop` releases.
pub struct FullDecodeLease {
    /// `None` once released (C# `Interlocked.Exchange(ref _gate, null)`).
    gate: Option<Arc<GateInner>>,
    /// C# `_worked` (`Volatile` int): set from the decode thread by `mark_worked`, read at release.
    worked: AtomicBool,
}

impl fmt::Debug for FullDecodeLease {
    /// Shows `released` (`gate.is_none()`) and `worked`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FullDecodeLease")
            .field("released", &self.gate.is_none())
            .field("worked", &self.is_worked())
            .finish()
    }
}

impl FullDecodeLease {
    /// C# `MarkWorked()`: this lease really ran a decode (past the memory guard, unpack started), so its release advances
    /// the aging clock. Idempotent; `SeqCst`.
    pub fn mark_worked(&self) {
        self.worked.store(true, Ordering::SeqCst);
    }

    /// C# `IsWorked` (test seam).
    pub fn is_worked(&self) -> bool {
        self.worked.load(Ordering::SeqCst)
    }

    /// Whether this lease still holds the slot (no C# twin; `false` after [`release`](Self::release)).
    pub fn is_held(&self) -> bool {
        self.gate.is_some()
    }

    /// C# `Dispose()`: releases the slot once (with the `worked` flag read now); later calls and the final `Drop` do
    /// nothing.
    pub fn release(&mut self) {
        if let Some(gate) = self.gate.take() {
            gate.release(self.is_worked());
        }
    }
}

impl Drop for FullDecodeLease {
    fn drop(&mut self) {
        self.release();
    }
}
