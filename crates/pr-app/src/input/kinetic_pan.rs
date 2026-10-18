//! Port of `PhotoReview.App.Shared/Input/KineticPan.cs` (baseline `d92d6aae`, 377 lines): the pan velocity tracker,
//! the inertial glide (`KineticScroller`), the arrow-key pan step (`KeyboardPan`) and the glide frame clock
//! (`GlideFrameClock`). Pure and time-driven: no clock is read here. The caller passes the elapsed time, the QPC
//! time (`now_ticks`) and the display timing, as the C# callers do. No `unsafe`, no `windows` crate (D-W8-3).
//!
//! Decisions of the W8-05f contract (`contracts/W8-05f.md`):
//! - D-W8-5f-1: an undefined `KineticGlideSmoothing` value is not representable in Rust, so the C# test
//!   `UndefinedMode_BehavesLikeOff` is not ported (the settings loader already maps unknown values to `Predict`).
//! - D-W8-5f-2: .NET `Math.Max`, `Math.Min` and `Math.Clamp` are copied as private helpers (`dotnet_max`,
//!   `dotnet_min`, `dotnet_clamp`). They propagate NaN, and never panic. `f64::max`, `f64::min` and `f64::clamp`
//!   do not do that (`clamp` panics on a NaN bound).
//! - D-W8-5f-3: `Math.Round(double)` is ties-to-even, so the port uses `f64::round_ties_even`, not `f64::round`.
//!
//! Baseline behaviour notes (the golden decides, see the contract): arrow keys use the EQUAL-STEP rule
//! (`Math.Min(width, height) * fraction`) at `d92d6aae`; the older per-axis rule mentioned in `ArrowPanRule.cs` is not in
//! this file.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use pr_core::abstractions::DisplayTiming;
use pr_core::model::enums::KineticGlideSmoothing;

/// `PanVelocityTracker.Capacity`: ring buffer size (samples).
pub const PAN_TRACKER_CAPACITY: usize = 32;
/// `PanVelocityTracker.WindowMs`: velocity window before release (ms).
pub const PAN_WINDOW_MS: f64 = 80.0;
/// `PanVelocityTracker.MinSpanMs`: a span shorter than this (ms) gives no velocity.
pub const PAN_MIN_SPAN_MS: f64 = 8.0;

/// `KineticScroller.TimeConstantMs`: friction time constant (ms).
pub const KINETIC_TIME_CONSTANT_MS: f64 = 325.0;
/// `KineticScroller.StartVelocity` (DIP/ms).
pub const KINETIC_START_VELOCITY: f64 = 0.1;
/// `KineticScroller.StopVelocity` (DIP/ms).
pub const KINETIC_STOP_VELOCITY: f64 = 0.02;
/// `KineticScroller.MaxVelocity` (DIP/ms): per-axis clamp in `start` and `add_impulse`.
pub const KINETIC_MAX_VELOCITY: f64 = 8.0;
/// `KineticScroller.PointerReleaseSpeedFactor` (Q-R40): damps the POINTER release only, never `add_impulse`.
pub const KINETIC_POINTER_RELEASE_SPEED_FACTOR: f64 = 0.65;
/// `KineticScroller.MaxFrameMs`: a longer frame gap is integrated as this much (ms).
pub const KINETIC_MAX_FRAME_MS: f64 = 100.0;

/// `KeyboardPan.StepFraction`: default arrow step, fraction of the shorter viewport side.
pub const KEYBOARD_STEP_FRACTION: f64 = 0.1;

/// `GlideFrameClock.PresentLeadMs`: time from the render callback to the compositor (ms).
pub const GLIDE_PRESENT_LEAD_MS: f64 = 1.0;
/// `GlideFrameClock.PeriodChangeTolerance`: a relative refresh-period change beyond this re-anchors the grid.
pub const GLIDE_PERIOD_CHANGE_TOLERANCE: f64 = 0.02;

/// .NET `Math.Max(double, double)`: NaN if either side is NaN; `+0.0` beats `-0.0`.
fn dotnet_max(a: f64, b: f64) -> f64 {
    todo!()
}

/// .NET `Math.Min(double, double)`: NaN if either side is NaN; `-0.0` beats `+0.0`.
fn dotnet_min(a: f64, b: f64) -> f64 {
    todo!()
}

/// .NET `Math.Clamp(double, min, max)` for `min <= max` (C# throws otherwise, and no caller here can pass that):
/// `value < min` gives `min`, `value > max` gives `max`, otherwise `value` (a NaN value stays NaN).
fn dotnet_clamp(value: f64, min: f64, max: f64) -> f64 {
    todo!()
}

/// C# `Finite(v)`: a non-finite value is treated as `0.0`.
fn finite(value: f64) -> f64 {
    todo!()
}

/// Ring buffer of pointer samples for the release velocity (`PanVelocityTracker`). Samples are `(time_ms, x, y)` in
/// DIP and ms. No allocation per sample; the oldest sample is overwritten after `PAN_TRACKER_CAPACITY` adds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PanVelocityTracker {
    time_ms: [f64; PAN_TRACKER_CAPACITY],
    x: [f64; PAN_TRACKER_CAPACITY],
    y: [f64; PAN_TRACKER_CAPACITY],
    count: usize,
    next: usize,
}

impl PanVelocityTracker {
    /// An empty tracker (`Count == 0`).
    pub fn new() -> Self {
        Self::default()
    }

    /// `Reset()`: forgets all samples. Count becomes 0.
    pub fn reset(&mut self) {
        todo!()
    }

    /// `Add(timeMs, x, y)`: argument order is time, then x, then y (the C# tests depend on it).
    pub fn add(&mut self, time_ms: f64, x: f64, y: f64) {
        todo!()
    }

    /// `Count`: samples held, saturates at `PAN_TRACKER_CAPACITY`.
    pub fn count(&self) -> usize {
        todo!()
    }

    /// `GetVelocity()`: `(vx, vy)` in DIP/ms over the samples within `PAN_WINDOW_MS` of the newest one. `(0.0, 0.0)`
    /// when fewer than two samples are held, when the span is below `PAN_MIN_SPAN_MS`, or when only the newest is in
    /// the window. Zero values are returned as `+0.0`.
    pub fn velocity(&self) -> (f64, f64) {
        todo!()
    }
}

/// Scroll state of the viewer, as the kinetic step reads it (`ScrollBounds`, a C# record struct).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBounds {
    pub extent_width: f64,
    pub extent_height: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
}

impl ScrollBounds {
    /// Positional constructor, same order as the C# record: extent width, extent height, viewport width, viewport height.
    pub const fn new(extent_width: f64, extent_height: f64, viewport_width: f64, viewport_height: f64) -> Self {
        Self {
            extent_width,
            extent_height,
            viewport_width,
            viewport_height,
        }
    }

    /// `MaxHorizontal` = `Math.Max(0, ExtentWidth - ViewportWidth)` (NaN propagates).
    pub fn max_horizontal(&self) -> f64 {
        todo!()
    }

    /// `MaxVertical` = `Math.Max(0, ExtentHeight - ViewportHeight)` (NaN propagates).
    pub fn max_vertical(&self) -> f64 {
        todo!()
    }
}

/// Inertial glide after a drag-pan (`KineticScroller`). Velocities are SCROLL-OFFSET velocities in DIP/ms: the
/// opposite sign of the pointer's (dragging right scrolls left). Friction is exponential,
/// `v(t) = v0 * e^(-t / KINETIC_TIME_CONSTANT_MS)`, integrated exactly per step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KineticScroller {
    velocity_x: f64,
    velocity_y: f64,
    is_active: bool,
}

impl KineticScroller {
    /// `VelocityX`.
    pub fn velocity_x(&self) -> f64 {
        self.velocity_x
    }

    /// `VelocityY`.
    pub fn velocity_y(&self) -> f64 {
        self.velocity_y
    }

    /// `IsActive`.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// `Start(pointerVelocityX, pointerVelocityY)`: damps the POINTER velocity by `KINETIC_POINTER_RELEASE_SPEED_FACTOR`,
    /// negates it, clamps each axis to `±KINETIC_MAX_VELOCITY`. Returns `false` and calls `stop` when the Euclidean
    /// speed is below `KINETIC_START_VELOCITY`; otherwise stores the velocities, becomes active and returns `true`.
    pub fn start(&mut self, pointer_velocity_x: f64, pointer_velocity_y: f64) -> bool {
        todo!()
    }

    /// `Stop()`: velocities become `+0.0`, inactive.
    pub fn stop(&mut self) {
        todo!()
    }

    /// `AddImpulse(scrollVelocityX, scrollVelocityY)`: adds a SCROLL-offset velocity (same sign as an offset delta)
    /// to each axis, clamped to `±KINETIC_MAX_VELOCITY` per axis; a non-finite component counts as `0.0`. Becomes
    /// active even when both components are zero. Never damped.
    pub fn add_impulse(&mut self, scroll_velocity_x: f64, scroll_velocity_y: f64) {
        todo!()
    }

    /// `Step(elapsedMs, horizontalOffset, verticalOffset, bounds)`: advances the glide and returns the new offsets.
    /// Inactive: returns the offsets unchanged. `elapsed_ms` is treated as `0.0` when non-finite or negative, and capped
    /// at `KINETIC_MAX_FRAME_MS`. An axis that reaches an edge stops (its velocity becomes `+0.0`). Both velocities below
    /// `KINETIC_STOP_VELOCITY` become `+0.0`; when both are zero the glide becomes inactive.
    pub fn step(
        &mut self,
        elapsed_ms: f64,
        horizontal_offset: f64,
        vertical_offset: f64,
        bounds: ScrollBounds,
    ) -> (f64, f64) {
        todo!()
    }
}

/// Result of one arrow press on a zoomed image (`KeyboardPanResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardPanResult {
    /// The image does not scroll on the pressed axis (e.g. Fit): the key keeps its normal meaning.
    NotScrollable,
    /// The view moved.
    Panned,
    /// Scrollable, but already at the edge in that direction.
    AtEdge,
}

/// `KeyboardPan.StepPixels`: `Math.Min(viewport_width, viewport_height) * step_fraction` (the SAME pixels on both axes,
/// ARROW-PAN-EQUAL-STEP). NaN on either side propagates.
pub fn keyboard_pan_step_pixels(viewport_width: f64, viewport_height: f64, step_fraction: f64) -> f64 {
    todo!()
}

/// `KeyboardPan.Step(dx, dy, horizontal, vertical, bounds, stepFraction)`. The axis is the horizontal one when `dx != 0`
/// (even when `dy` is also set), else the vertical one. `max <= 0.5` (or NaN, which is false here: see the golden)
/// gives `NotScrollable`; NaN does not (the comparison is false, so the step continues: golden case). A move smaller than
/// `0.5` DIP gives `AtEdge`; otherwise `Panned` with the target clamped to `[0, max]` on that axis. The step is `keyboard_pan_step_pixels(...) * (dx + dy)`, with `dx + dy` in wrapping
/// `i32` arithmetic.
pub fn keyboard_pan_step(
    dx: i32,
    dy: i32,
    horizontal: f64,
    vertical: f64,
    bounds: ScrollBounds,
    step_fraction: f64,
) -> (KeyboardPanResult, f64, f64) {
    todo!()
}

/// `KeyboardPan.IsZoomed`: true when either axis scrolls by more than `0.5` DIP.
pub fn keyboard_pan_is_zoomed(bounds: ScrollBounds) -> bool {
    todo!()
}

/// `KeyboardPan.ImpulseVelocity(stepX, stepY)`: the `add_impulse` velocities that make a glide travel the step.
/// Per axis: `0.0` for a zero step, else `copysign(|step| / KINETIC_TIME_CONSTANT_MS + KINETIC_STOP_VELOCITY, step)`.
pub fn keyboard_pan_impulse_velocity(step_x: f64, step_y: f64) -> (f64, f64) {
    todo!()
}

/// `KeyboardPan.ConsumesKey(result, isRepeat, navigatesAtEdge, bounds)`: whether the arrow key is consumed (`true`) or
/// falls through to Left/Right navigation (`false`). Default (`navigates_at_edge == false`): consumed while zoomed,
/// so only at Fit does it fall through. With `navigates_at_edge`: consumed on `Panned`, and on `AtEdge` only for an
/// auto-repeat.
pub fn keyboard_pan_consumes_key(
    result: KeyboardPanResult,
    is_repeat: bool,
    navigates_at_edge: bool,
    bounds: ScrollBounds,
) -> bool {
    todo!()
}

/// Turns the render callbacks of a glide into the time it advances by (`GlideFrameClock`), per the mode.
/// `Off` steps by rendering-time deltas. `Predict` steps to the refresh (vblank) of the window's monitor that the frame
/// is expected on, on that monitor's own grid. Until timing is known, and for one frame after the period changes,
/// it steps like `Off`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlideFrameClock {
    mode: KineticGlideSmoothing,
    ticks_per_ms: f64,
    started: bool,
    last_rendering_ms: f64,
    anchored: bool,
    anchor_vblank: i64,
    anchor_period: i64,
    last_index: i64,
}

impl GlideFrameClock {
    /// A clock in `Off` mode with no timing (C# `default(GlideFrameClock)`), not yet started.
    pub fn new() -> Self {
        Self {
            mode: KineticGlideSmoothing::Off,
            ticks_per_ms: 0.0,
            started: false,
            last_rendering_ms: 0.0,
            anchored: false,
            anchor_vblank: 0,
            anchor_period: 0,
            last_index: 0,
        }
    }

    /// `NeedsDisplayTiming`: true when the mode is not `Off` (the caller may skip the timing query otherwise).
    pub fn needs_display_timing(&self) -> bool {
        todo!()
    }

    /// `Start(mode, ticksPerMs)`: `ticks_per_ms` is the QPC frequency per millisecond. Mode becomes `Off` when
    /// `ticks_per_ms` is not `> 0` (NaN included). Resets only `started` and `anchored`: the rendering-time anchor and
    /// the grid index keep their old values, as in C#.
    pub fn start(&mut self, mode: KineticGlideSmoothing, ticks_per_ms: f64) {
        todo!()
    }

    /// `Advance(renderingTimeMs, nowTicks, timing)`: milliseconds the glide advances in this render callback. `0.0` means
    /// do not step (first callback, repeated rendering time, or a callback aimed at an already stepped refresh).
    pub fn advance(&mut self, rendering_time_ms: f64, now_ticks: i64, timing: Option<DisplayTiming>) -> f64 {
        todo!()
    }

    /// `Anchor(nowTicks, timing)`: starts the refresh grid at the next vblank; `last_index` becomes 0.
    fn anchor(&mut self, now_ticks: i64, timing: DisplayTiming) {
        todo!()
    }

    /// `NextVBlank(nowTicks, timing)`: the first vblank at least `GLIDE_PRESENT_LEAD_MS` after `now_ticks` on the
    /// timing's grid. Ticks use wrapping `i64` arithmetic, as C# `long` does.
    fn next_vblank(&self, now_ticks: i64, timing: DisplayTiming) -> i64 {
        todo!()
    }
}

impl Default for GlideFrameClock {
    fn default() -> Self {
        Self::new()
    }
}
