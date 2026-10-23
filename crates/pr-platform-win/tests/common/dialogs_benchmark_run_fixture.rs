//! Shared extensions for W8-18s native benchmark tests.
//! C#: BenchmarkDialogTests fixture extensions for cancellable sessions and UI pumping.

#[path = "dialogs_benchmark_fixture.rs"]
pub mod dialogs_benchmark_fixture;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// A test-controlled blocking gate for a background benchmark operation.
pub struct FakeSessionGate {
    release: Sender<()>,
    canceled: ArcFlag,
}

#[derive(Clone, Default)]
pub struct ArcFlag(std::sync::Arc<AtomicBool>);

impl ArcFlag {
    pub fn cancel(&self) { self.0.store(true, Ordering::Release); }
    pub fn is_canceled(&self) -> bool { self.0.load(Ordering::Acquire) }
}

impl FakeSessionGate {
    pub fn new() -> (Self, Receiver<()>) {
        let (release, receiver) = mpsc::channel();
        (Self { release, canceled: ArcFlag::default() }, receiver)
    }

    pub fn token(&self) -> ArcFlag { self.canceled.clone() }
    pub fn release(&self) { let _ = self.release.send(()); }

    pub fn wait(&self, receiver: &Receiver<()>) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            if self.canceled.is_canceled() { return false; }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() { return false; }
            match receiver.recv_timeout(remaining.min(Duration::from_millis(25))) {
                Ok(()) => return true,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
    }
}
