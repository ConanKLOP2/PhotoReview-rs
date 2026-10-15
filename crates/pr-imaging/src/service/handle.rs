//! W4-14b part 1 of 2 (port of `PreviewImageService.cs`, C# baseline d92d6aae): the result handle of a preview /
//! original request, the shared in-flight load cell (C# `Lazy<Task<IDecodedImage>>`) and the cancellable counting gate
//! (C# "channel pre-filled with tokens" `_viewerSlots` / `_originalDecodeGate`). Contract: `contracts/W4-14b.md`.
//!
//! Target path: `crates/pr-imaging/src/service/handle.rs`.

#![allow(unused_variables, unused_imports, dead_code)] // D11: remove with the last `todo!()`.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use pr_core::sync::{CancelToken, Canceled};

use super::deps::ImageCacheKey;
use super::{PreviewError, PreviewImage, PreviewResult, ServiceInner};

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
        todo!("W4-14b")
    }

    /// Blocks until [`Self::complete`] ran; returns a clone of the outcome.
    pub(crate) fn wait(&self) -> PreviewResult {
        todo!("W4-14b")
    }

    /// As [`Self::wait`] but gives up after `timeout` (`None`).
    pub(crate) fn wait_timeout(&self, timeout: Duration) -> Option<PreviewResult> {
        todo!("W4-14b")
    }

    /// Non-blocking peek.
    pub(crate) fn try_result(&self) -> Option<PreviewResult> {
        todo!("W4-14b")
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
        todo!("W4-14b")
    }

    /// Free tokens right now (tests).
    pub(crate) fn available(&self) -> i32 {
        todo!("W4-14b")
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
        todo!("W4-14b: give the token back and wake one waiter")
    }
}

/// C# `Task.Delay(delay, token)` for a positive delay: blocks until `delay` elapsed (`Ok`) or `token` is cancelled
/// (`Err`, seen within [`CANCEL_POLL_SLICE`]). `Duration::ZERO` returns at once without looking at the token.
pub(crate) fn wait_cancellable(delay: Duration, token: &CancelToken) -> Result<(), Canceled> {
    todo!("W4-14b")
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
        todo!("W4-14b")
    }

    /// C# `await task` including the joiner rules of `GetPreviewCoreAsync` (W4-14b semantics 4): waits for the shared
    /// load; when this caller is NOT the creator and the load failed with a cancellation while this caller's own token
    /// is not cancelled, or failed with `Busy`, starts over (`ServiceInner::get_preview_attempt`) and waits again; emits
    /// the joiner's `JoinEnd` perf event.
    pub fn wait(self) -> PreviewResult {
        todo!("W4-14b")
    }

    /// [`Self::wait`] with a hang guard: `Err(self)` (the handle, still usable) when the current attempt did not finish
    /// within `timeout`. A retry started inside the call gets the remaining time.
    pub fn wait_timeout(self, timeout: Duration) -> Result<PreviewResult, PreviewHandle> {
        todo!("W4-14b")
    }
}
