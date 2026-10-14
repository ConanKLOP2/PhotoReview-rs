//! Cancellation (C# `CancellationToken`), per docs/adr/R0003-concurrency.md.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cancellation flag shared by clones. Cancelling is one-way. Long-running work checks
/// [`is_canceled`](Self::is_canceled) between steps (per file, per batch, per decode stage); there is no callback
/// registration.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    canceled: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// A token nobody can cancel (C# `CancellationToken.None`).
    pub fn none() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::Acquire)
    }

    /// `Err(Canceled)` once canceled: `token.check()?` at each step.
    pub fn check(&self) -> Result<(), Canceled> {
        if self.is_canceled() { Err(Canceled) } else { Ok(()) }
    }
}

/// The operation was canceled (C# `OperationCanceledException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Canceled;

impl std::fmt::Display for Canceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation canceled")
    }
}

impl std::error::Error for Canceled {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_is_seen_by_every_clone() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert_eq!(clone.check(), Ok(()));
        token.cancel();
        assert!(clone.is_canceled());
        assert_eq!(clone.check(), Err(Canceled));
    }
}
