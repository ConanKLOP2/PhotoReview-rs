//! Port of `PhotoReview.Shell.Win32/Viewing/FramePacer.cs` (108 lines, WP-21, baseline d92d6aae): the present policy, so that
//! `Present` never blocks the UI thread for hundreds of ms.
//!
//! Policy: a windowed (not fullscreen) frame uses `Immediate` (`Present(0)`, DWM composes the latest frame, no tearing); only a
//! fullscreen frame uses `VSync` (`Present(1)`), and the first image is always `Immediate` (C# NOWPF-WP20). A busy swap chain
//! defers a frame (at most `MAX_DEFERRED_FRAMES` in a row). `VSync` that still blocks for `BLOCK_THRESHOLD_MS` in
//! `BLOCKED_FRAMES_TO_DEGRADE` consecutive frames degrades the session to `Immediate`, once and for good.
//!
//! Single-thread: the C# class is used on the UI thread only.

/// C# `PresentMode`: `Immediate = 0` is `Present(0)`, `VSync = 1` is `Present(1)` (same numbers as the C# enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentMode {
    Immediate = 0,
    VSync = 1,
}

/// C# `FramePacer.BlockThresholdMs`: a present at or above this many ms (strictly: `presentMs < 40` is "fast") counts as blocked.
pub const BLOCK_THRESHOLD_MS: f64 = 40.0;

/// C# `BlockedFramesToDegrade`: consecutive blocked `VSync` frames that degrade the session.
pub const BLOCKED_FRAMES_TO_DEGRADE: i32 = 2;

/// C# `MaxDeferredFrames`: consecutive deferrals allowed before a frame is drawn anyway.
pub const MAX_DEFERRED_FRAMES: i32 = 3;

/// C# `FramePacer` (`internal sealed class`, UI thread only).
#[derive(Debug, Default)]
pub struct FramePacer {
    consecutive_blocked: i32,
    deferred: i32,
    degraded: bool,
    deferred_total: i64,
}

impl FramePacer {
    /// `new FramePacer()`: not degraded, no deferrals.
    pub fn new() -> Self {
        Self::default()
    }

    /// C# `IsDegraded`: VSync was degraded to Immediate for the rest of the session.
    pub fn is_degraded(&self) -> bool {
        self.degraded
    }

    /// C# `DeferredTotal`: frames deferred since construction (diagnostics and tests).
    pub fn deferred_total(&self) -> i64 {
        self.deferred_total
    }

    /// C# `ModeFor(fullscreen, awaitingFirstImage)`: `Immediate` when degraded, when awaiting the first image, or when not fullscreen;
    /// otherwise `VSync`.
    pub fn mode_for(&self, fullscreen: bool, awaiting_first_image: bool) -> PresentMode {
        if self.degraded || awaiting_first_image || !fullscreen {
            PresentMode::Immediate
        } else {
            PresentMode::VSync
        }
    }

    /// C# `ShouldDefer(swapChainReady)`: `false` and reset the streak when ready; when not ready and the streak is below
    /// `MAX_DEFERRED_FRAMES`, count it (`deferred` + 1, `deferred_total` + 1) and return `true`; at the limit reset the streak
    /// and return `false` (the frame is drawn, `deferred_total` unchanged).
    pub fn should_defer(&mut self, swap_chain_ready: bool) -> bool {
        if swap_chain_ready {
            self.deferred = 0;
            return false;
        }
        if self.deferred >= MAX_DEFERRED_FRAMES {
            self.deferred = 0;
            return false;
        }
        self.deferred += 1;
        self.deferred_total = self.deferred_total.saturating_add(1);
        true
    }

    /// C# `ObservePresent(mode, presentMs)`. `Immediate` clears the blocked streak. `VSync` with `present_ms < 40` clears it;
    /// otherwise the streak grows (it keeps growing once degraded, which changes nothing). Returns `true` only on the call that
    /// sets `is_degraded` (the C# `Degraded` event fires once).
    pub fn observe_present(&mut self, mode: PresentMode, present_ms: f64) -> bool {
        if mode != PresentMode::VSync {
            self.consecutive_blocked = 0;
            return false;
        }
        // Keep the C# `<` form: a NaN present is not "fast", so it counts as blocked (trap T4).
        if present_ms < BLOCK_THRESHOLD_MS {
            self.consecutive_blocked = 0;
            return false;
        }
        self.consecutive_blocked = self.consecutive_blocked.saturating_add(1);
        if self.consecutive_blocked >= BLOCKED_FRAMES_TO_DEGRADE && !self.degraded {
            self.degraded = true;
            return true;
        }
        false
    }
}
