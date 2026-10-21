//! Rust form of the exception handling of `PhotoReview.Shell.Win32/Hosting/WndProcThunk.cs` (`Capture`/`RethrowPending`, lines 16-38, baseline `d92d6aae`).
//! Unit W8-13c (the message loop is the first caller; 13d and 13e use it).
//!
//! A panic must never unwind out of an `extern "system"` window procedure (Rust aborts the process: "panic in a function that cannot unwind", no
//! message pump of ours to report it). C# solved the same problem by capturing the exception in the thunk, returning 0 to the OS and rethrowing it
//! on the same thread right after the native call returned (`MessageLoop` after `DispatchMessage`, `ShellWindow` after `CreateWindowEx`/`SendMessage`/
//! `DestroyWindow`). This module is that mechanism: a `thread_local!` slot holding the FIRST payload; later panics on the thread while one is
//! pending are dropped (C# `pendingException ??=`). `rethrow_pending` uses `std::panic::resume_unwind`, so the panic reaches the top of the UI thread
//! with its original payload (R0003: the UI thread itself does not catch panics; this only carries one across the OS frames).

use std::any::Any;
use std::cell::RefCell;
use std::panic::{UnwindSafe, catch_unwind, resume_unwind};

thread_local! {
    /// C# `[ThreadStatic] pendingException`.
    static PENDING: RefCell<Option<Box<dyn Any + Send>>> = const { RefCell::new(None) };
}

/// C# `Capture(exception)`: stores `payload` unless one is already pending on this thread (first wins).
pub fn capture(payload: Box<dyn Any + Send>) {
    // `try_with`: during thread-local teardown the slot is gone; the payload is simply dropped then.
    let rejected = PENDING
        .try_with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                *slot = Some(payload);
                None
            } else {
                Some(payload)
            }
        })
        .unwrap_or(None);
    // A dropped payload may run arbitrary `Drop` code: outside the borrow.
    drop(rejected);
}

/// `true` when this thread holds a captured panic.
pub fn has_pending() -> bool {
    PENDING.try_with(|slot| slot.borrow().is_some()).unwrap_or(false)
}

/// C# `RethrowPending()`: takes the payload (the slot is empty afterwards) and `resume_unwind`s it; returns normally when nothing is pending.
///
/// `resume_unwind` does not call the panic hook again: the hook already ran where the panic started (inside the window procedure), so the message
/// is printed once.
pub fn rethrow_pending() {
    let payload = PENDING.try_with(|slot| slot.borrow_mut().take()).unwrap_or(None);
    if let Some(payload) = payload {
        resume_unwind(payload);
    }
}

/// The panic guard of every window procedure: runs `body` under `catch_unwind`; on a panic it `capture`s the payload and returns `on_panic`
/// (C# `catch (Exception) { Capture(ex); return 0; }`). Never unwinds.
pub fn guarded<R>(on_panic: R, body: impl FnOnce() -> R + UnwindSafe) -> R {
    match catch_unwind(body) {
        Ok(value) => value,
        Err(payload) => {
            capture(payload);
            on_panic
        }
    }
}
