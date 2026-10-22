//! New (no C# twin): the replacement of the `await` points of `ShellStartup.StartupCoreAsync` (D-W8-2, R0003: no async, nothing waits on the UI thread). Unit W8-17d.
//! Contract: `contracts/W8-17d.md`.
//!
//! The C# start-up awaits a pool task (the settings load, the second-instance hand-off) and CONTINUES ON THE UI THREAD. Rust: the worker thread owns a
//! [`UiHopSender`] (`Send`), the UI thread registers the continuation (not `Send`: it captures `Rc` state) with [`ui_hop`]. `send` posts a `Send` job to the
//! [`UiScheduler`]; the job runs on the UI thread, finds the continuation in a `thread_local!` registry (the same idea as `ForwardedOpenRouter`) and calls it.
//! A sender dropped without `send` (the worker panicked or its thread could not be started) delivers `Err(HopLost)`: a start-up never hangs silently.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use pr_core::abstractions::UiScheduler;

/// The sender was dropped before it sent a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopLost;

impl fmt::Display for HopLost {
    /// `"The background step ended without a result."`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for HopLost {}

/// The worker half. `Send` for any `T: Send`. Consumed by [`send`](UiHopSender::send); dropping it unsent delivers `Err(HopLost)`.
pub struct UiHopSender<T: Send + 'static> {
    id: u64,
    ui: Arc<dyn UiScheduler>,
    sent: bool,
    _value: PhantomData<fn(T)>,
}

impl<T: Send + 'static> fmt::Debug for UiHopSender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiHopSender")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Registers `continuation` on the CALLING (UI) thread and returns the sender for the worker. The id comes from a process-wide counter. The continuation runs
/// exactly once, on the UI thread, from a job posted to `ui`; if that job runs on a thread other than the registering one it finds nothing and does nothing.
pub fn ui_hop<T: Send + 'static>(
    ui: Arc<dyn UiScheduler>,
    continuation: impl FnOnce(Result<T, HopLost>) + 'static,
) -> UiHopSender<T> {
    todo!()
}

impl<T: Send + 'static> UiHopSender<T> {
    /// Posts the value to the UI thread (never runs the continuation inline).
    pub fn send(self, value: T) {
        todo!()
    }
}

impl<T: Send + 'static> Drop for UiHopSender<T> {
    /// Unsent: posts the `Err(HopLost)` delivery. Sent: nothing.
    fn drop(&mut self) {
        todo!()
    }
}
