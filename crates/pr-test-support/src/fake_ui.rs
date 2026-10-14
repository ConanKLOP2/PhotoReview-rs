//! A UI scheduler whose queue the test drains explicitly, to observe ordering between posted work and the caller.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};

use pr_core::abstractions::{UiAction, UiActionPanicked, UiCompletion, UiScheduler};

type Pending = (UiAction, Option<Sender<Result<(), UiActionPanicked>>>);

/// Queues every action until [`run_pending`](Self::run_pending) is called (on the test thread, which plays the UI
/// thread).
#[derive(Default)]
pub struct QueuedUiScheduler {
    queue: Mutex<Vec<Pending>>,
}

impl std::fmt::Debug for QueuedUiScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueuedUiScheduler")
            .field("pending", &self.pending())
            .finish()
    }
}

impl QueuedUiScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending(&self) -> usize {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// Runs queued actions in order, including ones they queue, until the queue is empty. Returns how many ran.
    /// A panic in a `post`ed action propagates (as it would crash the UI thread); a panic in an `invoke`d one
    /// resolves its completion instead.
    pub fn run_pending(&self) -> usize {
        let mut ran = 0;
        loop {
            let next = {
                let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
                if queue.is_empty() { None } else { Some(queue.remove(0)) }
            };
            let Some((action, completion)) = next else { return ran };
            ran += 1;
            match completion {
                None => action(),
                Some(sender) => {
                    let result = panic::catch_unwind(AssertUnwindSafe(action)).map_err(|payload| UiActionPanicked {
                        message: pr_core::abstractions::panic_message(payload.as_ref()),
                    });
                    let _ = sender.send(result);
                }
            }
        }
    }
}

impl UiScheduler for QueuedUiScheduler {
    fn post(&self, action: UiAction) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((action, None));
    }

    fn invoke(&self, action: UiAction) -> UiCompletion {
        let (sender, completion) = UiCompletion::channel();
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((action, Some(sender)));
        completion
    }
}
