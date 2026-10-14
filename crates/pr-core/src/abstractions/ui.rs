//! Port of `IUiScheduler.cs` + `ImmediateUiScheduler.cs`. C# `YieldAsync` has no equivalent: Rust UI code does not
//! await on the UI thread (docs/adr/R0003-concurrency.md).

use std::any::Any;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;

/// Work to run on the UI thread.
pub type UiAction = Box<dyn FnOnce() + Send + 'static>;

/// The action given to [`UiScheduler::invoke`] panicked (C#: the returned task faulted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiActionPanicked {
    pub message: String,
}

impl fmt::Display for UiActionPanicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UI action panicked: {}", self.message)
    }
}

impl std::error::Error for UiActionPanicked {}

/// Completion of an [`UiScheduler::invoke`] (C#: the returned `Task`).
#[derive(Debug)]
pub struct UiCompletion {
    receiver: mpsc::Receiver<Result<(), UiActionPanicked>>,
}

impl UiCompletion {
    /// A completion and the sender the scheduler resolves it with once the action ran.
    pub fn channel() -> (mpsc::Sender<Result<(), UiActionPanicked>>, Self) {
        let (sender, receiver) = mpsc::channel();
        (sender, Self { receiver })
    }

    /// Blocks until the action ran. NEVER call this on the UI thread itself (deadlock).
    /// A scheduler dropped before running the action reports it as panicked.
    pub fn wait(self) -> Result<(), UiActionPanicked> {
        self.receiver.recv().unwrap_or_else(|_| {
            Err(UiActionPanicked {
                message: "UI scheduler dropped the action".to_owned(),
            })
        })
    }

    /// Non-blocking check: `None` while the action has not run yet.
    pub fn try_result(&self) -> Option<Result<(), UiActionPanicked>> {
        self.receiver.try_recv().ok()
    }
}

/// Runs work on the UI thread.
pub trait UiScheduler: Send + Sync {
    /// Queues `action` on the UI thread and returns immediately.
    fn post(&self, action: UiAction);
    /// Queues `action`; the completion resolves once it ran. A panicking action resolves the completion with
    /// [`UiActionPanicked`] instead of unwinding into the caller (C# RV-S07: the task faults, the call does not throw).
    fn invoke(&self, action: UiAction) -> UiCompletion;
}

/// Port of `IUiDispatcher.cs` `UiPriority` (C# d92d6aae, C-04): WPF `DispatcherPriority` names; lower value = sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UiPriority {
    Send = 0,
    Normal = 1,
    Render = 2,
    Background = 3,
}

/// Port of `IUiDispatcher`: a [`UiScheduler`] with priorities. `post`/`invoke` mean [`UiPriority::Normal`].
/// (C# `YieldAsync(priority)` has no equivalent: the UI thread never awaits, R0003.)
pub trait UiDispatcher: UiScheduler {
    /// True on the UI thread.
    fn check_access(&self) -> bool;
    fn post_with_priority(&self, action: UiAction, priority: UiPriority);
    fn invoke_with_priority(&self, action: UiAction, priority: UiPriority) -> UiCompletion;
}

/// Runs everything inline on the calling thread (tests, CLI).
#[derive(Debug, Default, Clone, Copy)]
pub struct ImmediateUiScheduler;

impl UiScheduler for ImmediateUiScheduler {
    fn post(&self, action: UiAction) {
        action();
    }

    fn invoke(&self, action: UiAction) -> UiCompletion {
        let (sender, completion) = UiCompletion::channel();
        let result = panic::catch_unwind(AssertUnwindSafe(action)).map_err(|payload| UiActionPanicked {
            message: panic_message(payload.as_ref()),
        });
        // The receiver is alive (owned by `completion`), so this cannot fail.
        let _ = sender.send(result);
        completion
    }
}

/// Text of a panic payload (`&str` or `String`; anything else is reported generically).
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn immediate_invoke_runs_inline_and_completes() {
        let ran = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&ran);
        let completion = ImmediateUiScheduler.invoke(Box::new(move || {
            r.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(ran.load(Ordering::SeqCst), 1);
        assert_eq!(completion.wait(), Ok(()));
    }

    #[test]
    fn immediate_invoke_turns_a_panic_into_a_failed_completion() {
        let completion = ImmediateUiScheduler.invoke(Box::new(|| panic!("boom")));
        assert_eq!(
            completion.wait(),
            Err(UiActionPanicked {
                message: "boom".to_owned()
            })
        );
    }
}
