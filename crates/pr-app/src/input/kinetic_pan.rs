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

/// `KeyboardPan.Epsilon`: sub-pixel slack (DIP).
const KEYBOARD_EPSILON: f64 = 0.5;

/// .NET `Math.Max(double, double)`: NaN if either side is NaN; `+0.0` beats `-0.0`.
fn dotnet_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && !a.is_sign_negative()) {
        a
    } else {
        b
    }
}

/// .NET `Math.Min(double, double)`: NaN if either side is NaN; `-0.0` beats `+0.0`.
fn dotnet_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}

/// .NET `Math.Clamp(double, min, max)` for `min <= max` (C# throws otherwise, and no caller here can pass that):
/// `value < min` gives `min`, `value > max` gives `max`, otherwise `value` (a NaN value stays NaN).
fn dotnet_clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// C# `Finite(v)`: a non-finite value is treated as `0.0`.
fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
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
        self.count = 0;
        self.next = 0;
    }

    /// `Add(timeMs, x, y)`: argument order is time, then x, then y (the C# tests depend on it).
    pub fn add(&mut self, time_ms: f64, x: f64, y: f64) {
        self.time_ms[self.next] = time_ms;
        self.x[self.next] = x;
        self.y[self.next] = y;
        self.next = (self.next + 1) % PAN_TRACKER_CAPACITY;
        if self.count < PAN_TRACKER_CAPACITY {
            self.count += 1;
        }
    }

    /// `Count`: samples held, saturates at `PAN_TRACKER_CAPACITY`.
    pub fn count(&self) -> usize {
        self.count
    }

    /// `GetVelocity()`: `(vx, vy)` in DIP/ms over the samples within `PAN_WINDOW_MS` of the newest one. `(0.0, 0.0)`
    /// when fewer than two samples are held, when the span is below `PAN_MIN_SPAN_MS`, or when only the newest is in
    /// the window. Zero values are returned as `+0.0`.
    pub fn velocity(&self) -> (f64, f64) {
        if self.count < 2 {
            return (0.0, 0.0);
        }
        let newest = (self.next + PAN_TRACKER_CAPACITY - 1) % PAN_TRACKER_CAPACITY;
        let newest_time = self.time_ms[newest];
        let mut oldest = newest;
        for i in 1..self.count {
            let index = (newest + PAN_TRACKER_CAPACITY - i) % PAN_TRACKER_CAPACITY;
            if newest_time - self.time_ms[index] > PAN_WINDOW_MS {
                break;
            }
            oldest = index;
        }
        let span = newest_time - self.time_ms[oldest];
        if oldest == newest || span < PAN_MIN_SPAN_MS {
            return (0.0, 0.0);
        }
        (
            (self.x[newest] - self.x[oldest]) / span,
            (self.y[newest] - self.y[oldest]) / span,
        )
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
        dotnet_max(0.0, self.extent_width - self.viewport_width)
    }

    /// `MaxVertical` = `Math.Max(0, ExtentHeight - ViewportHeight)` (NaN propagates).
    pub fn max_vertical(&self) -> f64 {
        dotnet_max(0.0, self.extent_height - self.viewport_height)
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
        let vx = dotnet_clamp(
            -finite(pointer_velocity_x) * KINETIC_POINTER_RELEASE_SPEED_FACTOR,
            -KINETIC_MAX_VELOCITY,
            KINETIC_MAX_VELOCITY,
        );
        let vy = dotnet_clamp(
            -finite(pointer_velocity_y) * KINETIC_POINTER_RELEASE_SPEED_FACTOR,
            -KINETIC_MAX_VELOCITY,
            KINETIC_MAX_VELOCITY,
        );
        if (vx * vx + vy * vy).sqrt() < KINETIC_START_VELOCITY {
            self.stop();
            return false;
        }
        self.velocity_x = vx;
        self.velocity_y = vy;
        self.is_active = true;
        true
    }

    /// `Stop()`: velocities become `+0.0`, inactive.
    pub fn stop(&mut self) {
        self.velocity_x = 0.0;
        self.velocity_y = 0.0;
        self.is_active = false;
    }

    /// `AddImpulse(scrollVelocityX, scrollVelocityY)`: adds a SCROLL-offset velocity (same sign as an offset delta)
    /// to each axis, clamped to `±KINETIC_MAX_VELOCITY` per axis; a non-finite component counts as `0.0`. Becomes
    /// active even when both components are zero. Never damped.
    pub fn add_impulse(&mut self, scroll_velocity_x: f64, scroll_velocity_y: f64) {
        self.velocity_x = dotnet_clamp(
            self.velocity_x + finite(scroll_velocity_x),
            -KINETIC_MAX_VELOCITY,
            KINETIC_MAX_VELOCITY,
        );
        self.velocity_y = dotnet_clamp(
            self.velocity_y + finite(scroll_velocity_y),
            -KINETIC_MAX_VELOCITY,
            KINETIC_MAX_VELOCITY,
        );
        self.is_active = true;
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
        if !self.is_active {
            return (horizontal_offset, vertical_offset);
        }
        let dt = dotnet_clamp(finite(elapsed_ms), 0.0, KINETIC_MAX_FRAME_MS);
        let decay = (-dt / KINETIC_TIME_CONSTANT_MS).exp();
        let travel = KINETIC_TIME_CONSTANT_MS * (1.0 - decay);

        let (horizontal, hit_x) = advance_axis(horizontal_offset, self.velocity_x, travel, bounds.max_horizontal());
        let (vertical, hit_y) = advance_axis(vertical_offset, self.velocity_y, travel, bounds.max_vertical());
        self.velocity_x = if hit_x { 0.0 } else { self.velocity_x * decay };
        self.velocity_y = if hit_y { 0.0 } else { self.velocity_y * decay };
        if self.velocity_x.abs() < KINETIC_STOP_VELOCITY {
            self.velocity_x = 0.0;
        }
        if self.velocity_y.abs() < KINETIC_STOP_VELOCITY {
            self.velocity_y = 0.0;
        }
        if self.velocity_x == 0.0 && self.velocity_y == 0.0 {
            self.is_active = false;
        }
        (horizontal, vertical)
    }
}

/// C# `KineticScroller.Advance`: the clamped new offset and whether the axis hit an edge.
fn advance_axis(offset: f64, velocity: f64, travel: f64, max: f64) -> (f64, bool) {
    let target = finite(offset) + velocity * travel;
    let clamped = dotnet_clamp(target, 0.0, max);
    // A NaN target compares unequal to itself, exactly as in C# (`clamped != target`).
    let hit = velocity != 0.0 && clamped != target;
    (clamped, hit)
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
    dotnet_min(viewport_width, viewport_height) * step_fraction
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
    let max = if dx != 0 {
        bounds.max_horizontal()
    } else {
        bounds.max_vertical()
    };
    if max <= KEYBOARD_EPSILON {
        return (KeyboardPanResult::NotScrollable, horizontal, vertical);
    }
    let current = if dx != 0 { horizontal } else { vertical };
    let step = keyboard_pan_step_pixels(bounds.viewport_width, bounds.viewport_height, step_fraction)
        * f64::from(dx.wrapping_add(dy));
    let target = dotnet_clamp(current + step, 0.0, max);
    if (target - current).abs() < KEYBOARD_EPSILON {
        return (KeyboardPanResult::AtEdge, horizontal, vertical);
    }
    if dx != 0 {
        (KeyboardPanResult::Panned, target, vertical)
    } else {
        (KeyboardPanResult::Panned, horizontal, target)
    }
}

/// `KeyboardPan.IsZoomed`: true when either axis scrolls by more than `0.5` DIP.
pub fn keyboard_pan_is_zoomed(bounds: ScrollBounds) -> bool {
    bounds.max_horizontal() > KEYBOARD_EPSILON || bounds.max_vertical() > KEYBOARD_EPSILON
}

fn impulse(step: f64) -> f64 {
    if step == 0.0 {
        return 0.0;
    }
    let magnitude = step.abs() / KINETIC_TIME_CONSTANT_MS + KINETIC_STOP_VELOCITY;
    magnitude.copysign(step)
}

/// `KeyboardPan.ImpulseVelocity(stepX, stepY)`: the `add_impulse` velocities that make a glide travel the step.
/// Per axis: `0.0` for a zero step, else `copysign(|step| / KINETIC_TIME_CONSTANT_MS + KINETIC_STOP_VELOCITY, step)`.
pub fn keyboard_pan_impulse_velocity(step_x: f64, step_y: f64) -> (f64, f64) {
    (impulse(step_x), impulse(step_y))
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
    if result == KeyboardPanResult::Panned {
        return true;
    }
    if !navigates_at_edge {
        return keyboard_pan_is_zoomed(bounds);
    }
    result == KeyboardPanResult::AtEdge && is_repeat
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
        self.mode != KineticGlideSmoothing::Off
    }

    /// `Start(mode, ticksPerMs)`: `ticks_per_ms` is the QPC frequency per millisecond. Mode becomes `Off` when
    /// `ticks_per_ms` is not `> 0` (NaN included). Resets only `started` and `anchored`: the rendering-time anchor and
    /// the grid index keep their old values, as in C#.
    pub fn start(&mut self, mode: KineticGlideSmoothing, ticks_per_ms: f64) {
        self.mode = if ticks_per_ms > 0.0 {
            mode
        } else {
            KineticGlideSmoothing::Off
        };
        self.ticks_per_ms = ticks_per_ms;
        self.started = false;
        self.anchored = false;
    }

    /// `Advance(renderingTimeMs, nowTicks, timing)`: milliseconds the glide advances in this render callback. `0.0` means
    /// do not step (first callback, repeated rendering time, or a callback aimed at an already stepped refresh).
    pub fn advance(&mut self, rendering_time_ms: f64, now_ticks: i64, timing: Option<DisplayTiming>) -> f64 {
        let rendering_elapsed = rendering_time_ms - self.last_rendering_ms;
        let usable = timing.filter(|t| t.refresh_period > 0);
        if !self.started {
            self.started = true;
            self.last_rendering_ms = rendering_time_ms;
            if self.mode != KineticGlideSmoothing::Off
                && let Some(first) = usable
            {
                self.anchor(now_ticks, first);
            }
            return 0.0;
        }

        if self.mode != KineticGlideSmoothing::Off
            && let Some(current) = usable
        {
            let period_diff = current.refresh_period.wrapping_sub(self.anchor_period).unsigned_abs() as f64;
            if self.anchored && period_diff <= GLIDE_PERIOD_CHANGE_TOLERANCE * self.anchor_period as f64 {
                let delta = self.next_vblank(now_ticks, current).wrapping_sub(self.anchor_vblank);
                let index = (delta as f64 / current.refresh_period as f64).round_ties_even() as i64;
                if rendering_elapsed > 0.0 {
                    self.last_rendering_ms = rendering_time_ms;
                }
                if index <= self.last_index {
                    return 0.0;
                }
                let advanced =
                    index.wrapping_sub(self.last_index) as f64 * current.refresh_period as f64 / self.ticks_per_ms;
                self.last_index = index;
                return advanced;
            }
            // Timing just became available, or the monitor/refresh rate changed: this frame steps by the rendering
            // time and the refresh grid starts here.
            self.anchor(now_ticks, current);
        } else if self.mode != KineticGlideSmoothing::Off {
            // Timing lost: this frame steps by the rendering time and the grid is re-anchored when timing returns.
            self.anchored = false;
        }

        if rendering_elapsed.is_nan() || rendering_elapsed <= 0.0 {
            return 0.0; // Rendering can fire more than once per frame with the same RenderingTime.
        }
        self.last_rendering_ms = rendering_time_ms;
        rendering_elapsed
    }

    /// `Anchor(nowTicks, timing)`: starts the refresh grid at the next vblank; `last_index` becomes 0.
    fn anchor(&mut self, now_ticks: i64, timing: DisplayTiming) {
        self.anchored = true;
        self.anchor_vblank = self.next_vblank(now_ticks, timing);
        self.anchor_period = timing.refresh_period;
        self.last_index = 0;
    }

    /// `NextVBlank(nowTicks, timing)`: the first vblank at least `GLIDE_PRESENT_LEAD_MS` after `now_ticks` on the
    /// timing's grid. Ticks use wrapping `i64` arithmetic, as C# `long` does.
    fn next_vblank(&self, now_ticks: i64, timing: DisplayTiming) -> i64 {
        let target = now_ticks.wrapping_add((GLIDE_PRESENT_LEAD_MS * self.ticks_per_ms) as i64);
        let periods = (target.wrapping_sub(timing.last_vblank) as f64 / timing.refresh_period as f64).ceil();
        timing
            .last_vblank
            .wrapping_add((dotnet_max(0.0, periods) * timing.refresh_period as f64) as i64)
    }
}

impl Default for GlideFrameClock {
    fn default() -> Self {
        Self::new()
    }
}
