//! Port of `PhotoReview.App.Shared/Input/FrameClock.cs` (C-05): the frame tick record and the frame-clock contract.
//!
//! Only the pure part. The Win32 clocks (swap-chain waitable object, timer) are W8-13a/13b in `pr_platform_win::host`.
//! Baseline `d92d6aae`. The C# `FrameTickEventArgs` wrapper is not ported: a subscriber receives `&FrameTick` directly.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use pr_core::abstractions::DisplayTiming;

/// `FrameTick` (C# `readonly record struct`, positional `(long Timestamp, double RenderingTimeMs, DisplayTiming? Timing)`).
/// `timestamp` is in Stopwatch (QPC) ticks; `rendering_time_ms` keeps the C# name in snake_case.
#[derive(Debug, Clone, Copy)]
pub struct FrameTick {
    pub timestamp: i64,
    pub rendering_time_ms: f64,
    /// `DisplayTiming?`: `None` when the clock has no vblank timing for this frame.
    pub timing: Option<DisplayTiming>,
}

impl PartialEq for FrameTick {
    /// C# record equality: `EqualityComparer<double>.Default` for `rendering_time_ms` (`primitives::f64_same`), field by field otherwise.
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

/// Identifies one subscription made with [`FrameClock::subscribe`]. Replaces C# delegate identity for `-=`.
pub type FrameHandlerId = u64;

/// A `Frame` subscriber. Runs on the UI thread (no `Send` bound).
pub type FrameHandler = Box<dyn FnMut(&FrameTick)>;

/// `IFrameClock` (C-05). Rust name without the `I` prefix (precedent: `pr_core::abstractions::DisplayClock`).
///
/// Contract: the `Frame` event is raised once per frame that is about to be presented, on the UI thread, while there is at
/// least one subscriber or a `request_frame` is pending (C# doc of `Frame`). The raising logic is the clock's, not the trait's.
pub trait FrameClock {
    /// C# `Frame += handler`. Returns the id needed to unsubscribe.
    fn subscribe(&mut self, handler: FrameHandler) -> FrameHandlerId;

    /// C# `Frame -= handler`. An unknown id is a no-op.
    fn unsubscribe(&mut self, id: FrameHandlerId);

    /// C# `RequestFrame()`: asks for one more frame even without a subscriber.
    fn request_frame(&mut self);
}
