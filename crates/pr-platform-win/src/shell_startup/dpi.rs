//! Port of `PhotoReview.Shell.Win32/Startup/DpiAwarenessBootstrap.cs` (50 lines, baseline `d92d6aae`): `IDpiAwarenessNative` and `NativeDpiAwareness` (lines 6-23),
//! `DpiAwarenessBootstrap` (lines 30-50). Unit W8-17a. Contract: `contracts/W8-17a.md`.
//!
//! PerMonitorV2 (NO-WPF-EXEC-PLAN section 3, NE-2): the exe manifest declares it at load; this is the safety net for a process created by a host (a test host, an odd
//! loader) without the exe manifest. It must run BEFORE the first window (changing the awareness after a window exists has no effect). Never fails.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use crate::win32::startup::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;

/// `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2` (-4), the context both calls of the bootstrap use.
pub const TARGET_CONTEXT: isize = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;

/// The C# `EntryPointNotFoundException` / `DllNotFoundException` of an OS without the API. `windows-rs` links the functions statically, so the production
/// [`NativeDpiAwareness`] never returns it; the seam keeps the C# branch testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpiApiMissing;

/// C# `IDpiAwarenessNative`.
pub trait DpiAwarenessNative {
    /// The current THREAD is in `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2` (`GetThreadDpiAwarenessContext` equals `-4` by `AreDpiAwarenessContextsEqual`).
    fn is_per_monitor_v2(&self) -> Result<bool, DpiApiMissing>;
    /// `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`; `false` when refused (already set, or an old OS).
    fn try_set_per_monitor_v2(&self) -> Result<bool, DpiApiMissing>;
}

/// C# `NativeDpiAwareness` (production = user32 through `win32::startup` and `win32::api::user32`).
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeDpiAwareness;

impl DpiAwarenessNative for NativeDpiAwareness {
    /// `are_dpi_awareness_contexts_equal(user32::get_thread_dpi_awareness_context(), DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`.
    fn is_per_monitor_v2(&self) -> Result<bool, DpiApiMissing> {
        todo!()
    }

    /// `set_process_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok()`.
    fn try_set_per_monitor_v2(&self) -> Result<bool, DpiApiMissing> {
        todo!()
    }
}

/// C# `DpiAwarenessBootstrap.EnsurePerMonitorV2(native)`: `true` when PerMonitorV2 is in effect afterwards. `None` = [`NativeDpiAwareness`].
pub fn ensure_per_monitor_v2(native: Option<&dyn DpiAwarenessNative>) -> bool {
    todo!()
}
