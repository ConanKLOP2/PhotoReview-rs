//! Error type of the window host (owned by W8-13a, used by 13a to 13f). Replaces the exceptions of `PhotoReview.Shell.Win32/Hosting`:
//! `Win32Exception` (a failed `CreateWindowEx`, `RegisterClassEx`, `SetWaitableTimer`, `SetWindowText`, `MsgWaitForMultipleObjectsEx`),
//! `InvalidOperationException` (re-entrant `MessageLoop.Run`, a second dispatcher on one thread), `ArgumentException` and
//! `ArgumentOutOfRangeException` (zero wait handle, non-positive fixed period). Baseline `d92d6aae`.
//!
//! Not an error: a handler or window procedure that panics (see `pending_panic`), and the C# thread-affinity `InvalidOperationException`
//! (the host types are `!Send`, so a call from another thread does not compile; D-W8B3H-4).

#![allow(unused_variables, dead_code)] // W8-13a stub: the porter removes this line when the last `todo!()` is gone.

use std::fmt;

use crate::gfx::com::GfxError;
use crate::win32::structs::Win32Error;

/// Everything a host call can fail with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// A Win32 call failed. `operation` is the C# exception message, for example `"CreateWindowEx (shell window) failed"`.
    Win32 { operation: &'static str, error: Win32Error },
    /// A `gfx` helper failed (the waitable timer of `TimerFrameClock`).
    Gfx(GfxError),
    /// C# `InvalidOperationException`; `message` is the C# text.
    InvalidOperation { message: &'static str },
    /// C# `ArgumentException`.
    InvalidArgument { param: &'static str, message: &'static str },
    /// C# `ArgumentOutOfRangeException`.
    ArgumentOutOfRange { param: &'static str },
}

impl fmt::Display for HostError {
    /// `Win32`: `"{operation} (Win32 error {code})"`; `Gfx`: `"{operation} failed with HRESULT 0x{hresult:08X}"`; `InvalidOperation`: the message;
    /// `InvalidArgument`: `"{message} (Parameter '{param}')"`; `ArgumentOutOfRange`: `"Specified argument was out of the range of valid values. (Parameter '{param}')"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-13a")
    }
}

impl std::error::Error for HostError {}

impl From<GfxError> for HostError {
    fn from(error: GfxError) -> Self {
        todo!("W8-13a")
    }
}
