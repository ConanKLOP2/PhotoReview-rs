//! Port of the `IDisplayClock` part of `IDisplayClock.cs`. `VBlankEstimator` (pure math in the same C# file) is a
//! separate work unit (contracts/W1-22).

/// Refresh timing of a display in QPC ticks: the latest vertical blank and the refresh period. Later vblanks are
/// `last_vblank + n * refresh_period`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayTiming {
    pub last_vblank: i64,
    pub refresh_period: i64,
}

/// Reads the refresh timing of the monitor a window is on.
pub trait DisplayClock: Send + Sync {
    /// Timing of the monitor showing `window` (an HWND). Non-blocking; `None` until the clock has observed a few
    /// vblanks after the window moved to another monitor (or after a pause).
    fn get_timing(&self, window: isize) -> Option<DisplayTiming>;
}
