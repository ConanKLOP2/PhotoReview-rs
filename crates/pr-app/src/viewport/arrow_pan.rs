//! Port of `PhotoReview.App.Shared/Viewport/ArrowPanRule.cs` (WP-16, `internal static class` in C#).
//!
//! The step rule for an arrow key while zoomed. The C# file states the NEW rule (requested by the project owner, 2026-10-10):
//! step = `step_fraction` x `min(viewport_width, viewport_height)` in DIP, the same on both axes. The baseline app still runs
//! the OLD per-axis rule (`KeyboardPan.Step` in `App/Input/KineticPan.cs`, unit W8-05f). This port follows the NEW rule
//! (decision D-W8-01a-3); PARITY K-27 is `Different` until an ADR confirms it.
//! Baseline `d92d6aae`. Threshold 0.5 DIP for "not scrollable" and "at edge", as in `KeyboardPan`.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::viewport::types::ViewportLayout;

/// `DefaultStepFraction` = 0.1 (`AppSettings.DefaultArrowPanStepPercent` = 10 percent).
pub const DEFAULT_STEP_FRACTION: f64 = 0.1;

/// C# `Epsilon` = 0.5 DIP.
const EPSILON: f64 = 0.5;

/// C# `ArrowPanOutcome` (internal enum; same meaning as `KeyboardPanResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowPanOutcome {
    NotScrollable = 0,
    AtEdge = 1,
    Panned = 2,
}

/// C# `StepLength(viewportWidth, viewportHeight, stepFraction)`: `shorter = min(w, h)` (NaN propagates, as C# `Math.Min`
/// does; Rust `f64::min` would not), `length = shorter * step_fraction`; returns `length` when it is finite and > 0, else 0.
pub fn step_length(viewport_width: f64, viewport_height: f64, step_fraction: f64) -> f64 {
    todo!()
}

/// C# `Step(dx, dy, horizontal, vertical, in layout, stepFraction)`.
///
/// One press along `(dx, dy)`; the normal use is exactly one of them is `-1` or `1`. The horizontal axis is used when
/// `dx != 0`, otherwise the vertical axis. The step is `step_length(...) * (dx + dy)` (so `(1, 1)` moves twice as far and
/// `(1, -1)` does not move). The axis max is `layout.max_horizontal_offset` or `layout.max_vertical_offset`.
///
/// Returns `(outcome, horizontal, vertical)`:
/// - `max <= 0.5` (NaN max is NOT caught: the comparison is false): `NotScrollable`, the inputs unchanged.
/// - `target = clamp(current + step, 0, max)` with the C# `Math.Clamp` branch order (see contract trap T5), and
///   `|target - current| < 0.5`: `AtEdge`, inputs unchanged.
/// - otherwise `Panned`, the target on the moved axis, the other axis unchanged.
pub fn step(
    dx: i32,
    dy: i32,
    horizontal: f64,
    vertical: f64,
    layout: &ViewportLayout,
    step_fraction: f64,
) -> (ArrowPanOutcome, f64, f64) {
    todo!()
}
