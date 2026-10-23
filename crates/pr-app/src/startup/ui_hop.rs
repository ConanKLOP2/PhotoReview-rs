//! New (no C# twin): the replacement of the `await` points of `ShellStartup.StartupCoreAsync` (D-W8-2, R0003: no async, nothing waits on the UI thread). Unit W8-17d.
//! Contract: `contracts/W8-17d.md`.
//!
//! The C# start-up awaits a pool task (the settings load, the second-instance hand-off) and CONTINUES ON THE UI THREAD. Rust: the worker thread owns a
//! [`UiHopSender`] (`Send`), the UI thread registers the continuation (not `Send`: it captures `Rc` state) with [`ui_hop`]. `send` posts a `Send` job to the
//! [`UiScheduler`]; the job runs on the UI thread, finds the continuation in a `thread_local!` registry (the same idea as `ForwardedOpenRouter`) and calls it.
//! A sender dropped without `send` (the worker panicked or its thread could not be started) delivers `Err(HopLost)`: a start-up never hangs silently.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pr_core::abstractions::UiScheduler;

type Delivery = Box<dyn FnOnce(Box<dyn Any + Send>)>;
thread_local! {
    static CONTINUATIONS: RefCell<HashMap<u64, Delivery>> = RefCell::new(HashMap::new());
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn deliver<T: Send + 'static>(id: u64, value: Result<T, HopLost>) {
    let continuation = CONTINUATIONS.with(|registry| registry.borrow_mut().remove(&id));
    if let Some(continuation) = continuation {
        continuation(Box::new(value));
    }
}

/// The sender was dropped before it sent a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopLost;

impl fmt::Display for HopLost {
    /// `"The background step ended without a result."`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The background step ended without a result.")
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
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    CONTINUATIONS.with(|registry| {
        registry.borrow_mut().insert(
            id,
            Box::new(move |value| {
                if let Ok(value) = value.downcast::<Result<T, HopLost>>() {
                    continuation(*value);
                }
            }),
        );
    });
    UiHopSender {
        id,
        ui,
        sent: false,
        _value: PhantomData,
    }
}

impl<T: Send + 'static> UiHopSender<T> {
    /// Posts the value to the UI thread (never runs the continuation inline).
    pub fn send(mut self, value: T) {
        self.sent = true;
        let id = self.id;
        self.ui.post(Box::new(move || deliver(id, Ok(value))));
    }
}

impl<T: Send + 'static> Drop for UiHopSender<T> {
    /// Unsent: posts the `Err(HopLost)` delivery. Sent: nothing.
    fn drop(&mut self) {
        if !self.sent {
            let id = self.id;
            self.ui.post(Box::new(move || deliver::<T>(id, Err(HopLost))));
        }
    }
}
