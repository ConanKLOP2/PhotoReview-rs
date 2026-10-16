//! W4-14b part 1 of 2 (port of `PreviewImageService.cs`, C# baseline d92d6aae): the result handle of a preview /
//! original request, the shared in-flight load cell (C# `Lazy<Task<IDecodedImage>>`) and the cancellable counting gate
//! (C# "channel pre-filled with tokens" `_viewerSlots` / `_originalDecodeGate`). Contract: `contracts/W4-14b.md`.
//!
//! Target path: `crates/pr-imaging/src/service/handle.rs`.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::sync::{CancelToken, Canceled};

use super::deps::ImageCacheKey;
use super::{PreviewResult, ServiceInner, lock};

/// Longest a cancellable wait (start delay, viewer slot, original gate) sleeps before re-checking its token.
/// `CancelToken` has no wake-up registration (R0003), so cancellation is noticed within this slice; a slot release wakes
/// the waiter at once (condvar). See the contract, open question 2 (a waitable token would remove the slice).
pub const CANCEL_POLL_SLICE: Duration = Duration::from_millis(10);

/// One shared load: written once by the worker, read by every waiter (C# `Lazy<Task<IDecodedImage>>` with
/// `ExecutionAndPublication`; the task's outcome is replayed to each awaiter).
pub(crate) struct LoadCell {
    result: Mutex<Option<PreviewResult>>,
    done: Condvar,
}

impl fmt::Debug for LoadCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadCell").finish_non_exhaustive()
    }
}

impl LoadCell {
    pub(crate) fn new() -> Self {
        Self {
            result: Mutex::new(None),
            done: Condvar::new(),
        }
    }

    /// Stores the outcome (first write wins; a second write is a bug and is ignored) and wakes every waiter.
    pub(crate) fn complete(&self, result: PreviewResult) {
        {
            let mut slot = lock(&self.result);
            if slot.is_none() {
                *slot = Some(result);
            }
        }
        self.done.notify_all();
    }

    /// Blocks until [`Self::complete`] ran; returns a clone of the outcome.
    pub(crate) fn wait(&self) -> PreviewResult {
        let mut slot = lock(&self.result);
        loop {
            if let Some(result) = slot.as_ref() {
                return result.clone();
            }
            slot = self.done.wait(slot).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// As [`Self::wait`] but gives up after `timeout` (`None`).
    pub(crate) fn wait_timeout(&self, timeout: Duration) -> Option<PreviewResult> {
        // A deadline past `Instant`'s range is "no deadline".
        let deadline = Instant::now().checked_add(timeout);
        let mut slot = lock(&self.result);
        loop {
            if let Some(result) = slot.as_ref() {
                return Some(result.clone());
            }
            let remaining = match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return None;
                    }
                    deadline - now
                }
                None => timeout,
            };
            slot = self
                .done
                .wait_timeout(slot, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Non-blocking peek.
    pub(crate) fn try_result(&self) -> Option<PreviewResult> {
        lock(&self.result).clone()
    }

    /// Whether the outcome is stored (no clone).
    fn is_complete(&self) -> bool {
        lock(&self.result).is_some()
    }
}

/// A cancellable counting gate (C# `Channel<byte>` pre-filled with `count` tokens: read = acquire, write = release).
/// Owns nothing disposable. Waiters are woken by a release; cancellation is seen within [`CANCEL_POLL_SLICE`].
pub(crate) struct SlotGate {
    available: Mutex<i32>,
    released: Condvar,
}

impl fmt::Debug for SlotGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlotGate").finish_non_exhaustive()
    }
}

impl SlotGate {
    /// C# `CreateSlotTokens(count)`.
    pub(crate) fn new(count: i32) -> Self {
        Self {
            available: Mutex::new(count),
            released: Condvar::new(),
        }
    }

    /// C# `Reader.ReadAsync(token)`: `Err(Canceled)` when `token` is (or becomes) cancelled before a token is free;
    /// a token already cancelled on entry never takes a slot even if one is free (C# `ReadAsync` checks first).
    pub(crate) fn acquire(&self, token: &CancelToken) -> Result<SlotPermit<'_>, Canceled> {
        let mut available = lock(&self.available);
        loop {
            // Checked before every take: a superseded request never consumes a free slot.
            token.check()?;
            if *available > 0 {
                *available -= 1;
                return Ok(SlotPermit { gate: self });
            }
            available = self
                .released
                .wait_timeout(available, CANCEL_POLL_SLICE)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Free tokens right now (tests).
    pub(crate) fn available(&self) -> i32 {
        *lock(&self.available)
    }
}

/// A taken slot; dropping it returns the token (C# `Writer.TryWrite(0)` in `finally`).
pub(crate) struct SlotPermit<'a> {
    gate: &'a SlotGate,
}

impl fmt::Debug for SlotPermit<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlotPermit").finish_non_exhaustive()
    }
}

impl Drop for SlotPermit<'_> {
    fn drop(&mut self) {
        *lock(&self.gate.available) += 1;
        // Every waiter re-checks: one woken waiter may be cancelled and leave the token to another.
        self.gate.released.notify_all();
    }
}

/// C# `Task.Delay(delay, token)` for a positive delay: blocks until `delay` elapsed (`Ok`) or `token` is cancelled
/// (`Err`, seen within [`CANCEL_POLL_SLICE`]). `Duration::ZERO` returns at once without looking at the token.
/// DIVERGENCE: C# `Task.Delay` throws `ArgumentOutOfRangeException` for a delay above `uint.MaxValue - 1` ms (~49.7
/// days); here such a delay simply waits (until cancelled). No caller passes more than a key-repeat burst delay.
pub(crate) fn wait_cancellable(delay: Duration, token: &CancelToken) -> Result<(), Canceled> {
    if delay.is_zero() {
        return Ok(());
    }
    let deadline = Instant::now().checked_add(delay);
    loop {
        token.check()?;
        let slice = match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return Ok(());
                }
                (deadline - now).min(CANCEL_POLL_SLICE)
            }
            None => CANCEL_POLL_SLICE,
        };
        std::thread::sleep(slice);
    }
}

/// Everything a joiner needs to retry (C# locals of `GetPreviewCoreAsync` / `GetPreviewOnceAsync`).
pub(crate) struct RetryContext {
    pub(crate) service: Arc<ServiceInner>,
    pub(crate) path: String,
    pub(crate) key: ImageCacheKey,
    pub(crate) viewer: bool,
    pub(crate) start_delay: Duration,
    pub(crate) token: CancelToken,
    /// `ReferenceEquals(lazy, candidate)`: this caller created the load (it never retries).
    pub(crate) is_creator: bool,
    /// D04 perf: `(nav, path_id, start timestamp)` of a joiner when perf tracing was on at join time; `JoinEnd` is
    /// emitted when the wait returns.
    pub(crate) perf_join: Option<(i64, String, i64)>,
}

impl fmt::Debug for RetryContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryContext")
            .field("path", &self.path)
            .field("viewer", &self.viewer)
            .field("is_creator", &self.is_creator)
            .finish_non_exhaustive()
    }
}

enum HandleState {
    /// C# a completed task: cache hit, immediate failure (cancelled token, bad key).
    Ready(PreviewResult),
    /// A shared load. `retry` is `None` for `decode_original` (never joined, never retried).
    Load {
        cell: Arc<LoadCell>,
        retry: Option<Box<RetryContext>>,
    },
}

/// What one finished attempt means for the caller.
enum Step {
    Done(PreviewResult),
    Retry(PreviewHandle),
}

/// The joiner rules of C# `GetPreviewCoreAsync` / `GetPreviewOnceAsync` (W4-14b semantics 4), applied to the outcome
/// of one attempt.
fn finish_attempt(result: PreviewResult, retry: Option<Box<RetryContext>>) -> Step {
    let Some(context) = retry else {
        return Step::Done(result);
    };
    // C# `finally`: JoinEnd once per joined attempt, whatever the outcome.
    if let Some((nav, path_id, start)) = &context.perf_join {
        PhotoReviewPerf::log().join_end(*nav, path_id, perf::ms(*start));
    }
    let error = match result {
        Ok(image) => return Step::Done(Ok(image)),
        Err(error) => error,
    };
    if context.is_creator {
        // The creator surfaces Busy and every cancellation (the PreloadScheduler handles Busy).
        return Step::Done(Err(error));
    }
    // `catch (OperationCanceledException) when (!creator && !token.IsCancellationRequested)` and
    // `catch (DecoderBusyException) when (!creator)`: start over (C# `while (true)`).
    let retry = (error.is_canceled() && !context.token.is_canceled()) || error.is_busy();
    if !retry {
        return Step::Done(Err(error));
    }
    Step::Retry(context.service.get_preview_attempt(
        &context.path,
        &context.key,
        context.viewer,
        context.start_delay,
        &context.token,
    ))
}

/// The outcome of a preview / original request (C# `Task<IDecodedImage>`). Waiting is blocking; never wait on the UI
/// thread (R0003). Dropping the handle without waiting abandons only THIS caller's interest: the shared load still
/// completes, publishes and is removed from the in-flight table (C# a not-awaited task still runs to completion).
pub struct PreviewHandle {
    state: HandleState,
}

impl fmt::Debug for PreviewHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = match &self.state {
            HandleState::Ready(_) => "ready",
            HandleState::Load { .. } => "load",
        };
        f.debug_struct("PreviewHandle").field("state", &state).finish()
    }
}

impl PreviewHandle {
    /// A finished request.
    pub(crate) fn ready(result: PreviewResult) -> Self {
        Self {
            state: HandleState::Ready(result),
        }
    }

    /// A pending shared load (`retry` = `None`: no joiner rules, `decode_original`).
    pub(crate) fn load(cell: Arc<LoadCell>, retry: Option<Box<RetryContext>>) -> Self {
        Self {
            state: HandleState::Load { cell, retry },
        }
    }

    /// True when [`Self::wait`] would return without blocking FOR THE CURRENT ATTEMPT (a joiner that must retry may still
    /// block afterwards).
    pub fn is_ready(&self) -> bool {
        match &self.state {
            HandleState::Ready(_) => true,
            HandleState::Load { cell, .. } => cell.is_complete(),
        }
    }

    /// C# `await task` including the joiner rules of `GetPreviewCoreAsync` (W4-14b semantics 4): waits for the shared
    /// load; when this caller is NOT the creator and the load failed with a cancellation while this caller's own token
    /// is not cancelled, or failed with `Busy`, starts over (`ServiceInner::get_preview_attempt`) and waits again; emits
    /// the joiner's `JoinEnd` perf event.
    pub fn wait(self) -> PreviewResult {
        let mut handle = self;
        loop {
            let (cell, retry) = match handle.state {
                HandleState::Ready(result) => return result,
                HandleState::Load { cell, retry } => (cell, retry),
            };
            let result = cell.wait();
            match finish_attempt(result, retry) {
                Step::Done(result) => return result,
                Step::Retry(next) => handle = next,
            }
        }
    }

    /// [`Self::wait`] with a hang guard: `Err(self)` (the handle, still usable) when the current attempt did not finish
    /// within `timeout`. A retry started inside the call gets the remaining time.
    pub fn wait_timeout(self, timeout: Duration) -> Result<PreviewResult, PreviewHandle> {
        let deadline = Instant::now().checked_add(timeout);
        let mut handle = self;
        loop {
            let (cell, retry) = match handle.state {
                HandleState::Ready(result) => return Ok(result),
                HandleState::Load { cell, retry } => (cell, retry),
            };
            let remaining = match deadline {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => timeout,
            };
            let Some(result) = cell.wait_timeout(remaining) else {
                return Err(PreviewHandle::load(cell, retry));
            };
            match finish_attempt(result, retry) {
                Step::Done(result) => return Ok(result),
                Step::Retry(next) => handle = next,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::PreviewError;

    #[test]
    fn slot_gate_cancelled_token_never_takes_a_free_slot() {
        let gate = SlotGate::new(1);
        let token = CancelToken::new();
        token.cancel();
        assert!(gate.acquire(&token).is_err());
        assert_eq!(gate.available(), 1);
    }

    #[test]
    fn slot_gate_full_gate_waits_instead_of_overdrawing() {
        let gate = Arc::new(SlotGate::new(1));
        let _held = gate.acquire(&CancelToken::new()).expect("slot");
        let token = CancelToken::new();
        let canceller = token.clone();
        // Whenever the cancel lands, a full gate must never hand out a second token.
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            canceller.cancel();
        });
        assert!(gate.acquire(&token).is_err());
        cancel_thread.join().expect("canceller");
        assert_eq!(gate.available(), 0);
    }

    #[test]
    fn debug_output_names_the_types() {
        assert_eq!(format!("{:?}", LoadCell::new()), "LoadCell { .. }");
        let gate = SlotGate::new(1);
        assert_eq!(format!("{gate:?}"), "SlotGate { .. }");
        let permit = gate.acquire(&CancelToken::new()).expect("slot");
        assert_eq!(format!("{permit:?}"), "SlotPermit { .. }");
    }

    #[test]
    fn slot_gate_permit_drop_returns_the_token() {
        let gate = SlotGate::new(2);
        let token = CancelToken::new();
        let first = gate.acquire(&token).expect("first slot");
        let second = gate.acquire(&token).expect("second slot");
        assert_eq!(gate.available(), 0);
        drop(first);
        assert_eq!(gate.available(), 1);
        drop(second);
        assert_eq!(gate.available(), 2);
    }

    #[test]
    fn slot_gate_release_wakes_a_waiter() {
        let gate = Arc::new(SlotGate::new(1));
        let token = CancelToken::new();
        let permit = gate.acquire(&token).expect("slot");
        let (sender, receiver) = std::sync::mpsc::channel();
        let waiter_gate = Arc::clone(&gate);
        let waiter = std::thread::spawn(move || {
            let acquired = waiter_gate.acquire(&CancelToken::new()).is_ok();
            sender.send(acquired).expect("send");
        });
        assert!(
            receiver.try_recv().is_err(),
            "the waiter must block while no slot is free"
        );
        drop(permit);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(10)), Ok(true));
        waiter.join().expect("waiter thread");
        assert_eq!(gate.available(), 1);
    }

    #[test]
    fn slot_gate_cancel_while_waiting_returns_canceled_within_the_slice() {
        let gate = Arc::new(SlotGate::new(1));
        let _held = gate.acquire(&CancelToken::new()).expect("slot");
        let token = CancelToken::new();
        let (sender, receiver) = std::sync::mpsc::channel();
        let waiter_gate = Arc::clone(&gate);
        let waiter_token = token.clone();
        let waiter = std::thread::spawn(move || {
            sender.send(waiter_gate.acquire(&waiter_token).map(drop)).expect("send");
        });
        token.cancel();
        assert_eq!(receiver.recv_timeout(Duration::from_secs(10)), Ok(Err(Canceled)));
        waiter.join().expect("waiter thread");
        assert_eq!(gate.available(), 0, "the cancelled waiter took nothing");
    }

    #[test]
    fn wait_cancellable_zero_ignores_a_cancelled_token() {
        let token = CancelToken::new();
        token.cancel();
        assert_eq!(wait_cancellable(Duration::ZERO, &token), Ok(()));
        assert_eq!(wait_cancellable(Duration::from_nanos(1), &token), Err(Canceled));
    }

    #[test]
    fn wait_cancellable_elapses_and_returns_ok() {
        let token = CancelToken::new();
        let start = Instant::now();
        assert_eq!(wait_cancellable(Duration::from_millis(30), &token), Ok(()));
        assert!(start.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn wait_cancellable_huge_delay_is_cancelled_by_the_token() {
        let token = CancelToken::new();
        let canceller = token.clone();
        let waiter = std::thread::spawn(move || wait_cancellable(Duration::MAX, &token));
        canceller.cancel();
        assert_eq!(waiter.join().expect("waiter thread"), Err(Canceled));
    }

    #[test]
    fn load_cell_first_write_wins_and_wakes_waiters() {
        let cell = Arc::new(LoadCell::new());
        assert!(cell.try_result().is_none());
        assert!(cell.wait_timeout(Duration::from_millis(1)).is_none());
        let waiter_cell = Arc::clone(&cell);
        let waiter = std::thread::spawn(move || waiter_cell.wait());
        cell.complete(Err(PreviewError::Canceled));
        cell.complete(Err(PreviewError::RawFullDecodeKey));
        assert!(matches!(waiter.join().expect("waiter"), Err(PreviewError::Canceled)));
        assert!(matches!(cell.try_result(), Some(Err(PreviewError::Canceled))));
        assert!(matches!(
            cell.wait_timeout(Duration::ZERO),
            Some(Err(PreviewError::Canceled))
        ));
    }

    #[test]
    fn wait_timeout_returns_the_handle_when_pending() {
        let cell = Arc::new(LoadCell::new());
        let handle = PreviewHandle::load(Arc::clone(&cell), None);
        assert!(!handle.is_ready());
        let handle = match handle.wait_timeout(Duration::from_millis(5)) {
            Ok(_) => panic!("a pending load must not finish"),
            Err(handle) => handle,
        };
        cell.complete(Err(PreviewError::Canceled));
        assert!(handle.is_ready());
        assert!(matches!(
            handle.wait_timeout(Duration::from_secs(10)),
            Ok(Err(PreviewError::Canceled))
        ));
    }
}
