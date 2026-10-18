//! Port of `Services/RenderFrameTrace.cs` (43 lines). Contract: `contracts/W8-04c.md`, rules R-RF-*.
//!
//! perf(render-metric): reports "first frame after the assign" and "frame that contains the new image" for a presented image,
//! from the frame-tick source (`CompositionTarget.Rendering` in C#; the Win32 frame clock in Rust, W8-13a).
//!
//! Rust shape (decision D-04c-6): C# passes callbacks and subscribes a delegate. Here the trace is a pure state machine that
//! returns the ordered steps the host must perform: subscribe or unsubscribe a handler id on its tick source, and report a
//! first or second frame. No callback re-enters the trace, so the reentrancy of C# (a first-frame callback that calls `Start`)
//! becomes "the host performs the steps, then calls `start` again".

#![allow(unused_variables, dead_code)] // D11: stub bodies are todo!(); removed with the last todo!() of this file.

/// Identity of one subscription on the host's frame-tick source (C# delegate identity). Never reused within one trace.
pub type HandlerId = u64;

/// One step for the host, in the order the C# code performs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameTraceStep {
    /// Register `HandlerId` on the tick source; the host must route that handler's ticks to `on_frame_tick`.
    Subscribe(HandlerId),
    /// Remove `HandlerId` from the tick source; no further ticks for it.
    Unsubscribe(HandlerId),
    /// C# `onFirstFrame(token, kind, assignedTimestamp)`: the first tick after the start.
    FirstFrame {
        token: i64,
        kind: String,
        assigned_timestamp: i64,
    },
    /// C# `onSecondFrame(token, assignedTimestamp)`: the second tick; the trace is finished.
    SecondFrame { token: i64, assigned_timestamp: i64 },
}

/// The one pending trace (C# `_pending` handler and its captured state).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingTrace {
    id: HandlerId,
    token: i64,
    kind: String,
    assigned_timestamp: i64,
    /// Ticks seen so far (0 after `start`, 1 after the first frame).
    ticks: u32,
}

/// C# `RenderFrameTrace` (43 lines). At most one pending trace (RV-A17).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderFrameTrace {
    /// Next handler id to hand out (C# object identity replaced by a counter; wraps like the C# reference identity never does, not
    /// reachable in practice: R-RF-6).
    next_id: u64,
    pending: Option<PendingTrace>,
}

impl RenderFrameTrace {
    /// No pending trace, id counter at 0.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Start(token, kind, assignedTimestamp)`. Cancels any pending trace first (its `Unsubscribe` comes first), then creates a
    /// new pending trace and `Subscribe`s it. Result: `[Unsubscribe(old)?, Subscribe(new)]` (R-RF-1, R-RF-2).
    pub fn start(&mut self, token: i64, kind: &str, assigned_timestamp: i64) -> Vec<FrameTraceStep> {
        todo!()
    }

    /// C# `Cancel()`: drops the pending trace without reporting it. `[Unsubscribe(pending)]`, or empty when nothing is pending (R-RF-3).
    pub fn cancel(&mut self) -> Vec<FrameTraceStep> {
        todo!()
    }

    /// The host calls this for every tick it delivers to handler `id`. An id that is not the pending one (superseded or cancelled)
    /// yields nothing (R-RF-4). Tick 1: `[FirstFrame]`, the trace stays pending. Tick 2: `[Unsubscribe(id), SecondFrame]`, pending cleared
    /// (C# calls `Cancel()` before `onSecondFrame`, R-RF-5).
    pub fn on_frame_tick(&mut self, id: HandlerId) -> Vec<FrameTraceStep> {
        todo!()
    }
}
