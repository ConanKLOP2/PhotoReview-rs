//! Port of `PhotoReview.App.Shared/Input/MouseGestures.cs` (baseline `d92d6aae`, 161 lines): the mouse wheel,
//! click / drag / double-click and middle-click decisions for the main image. Pure: no window, no clock, no `windows` crate.
//!
//! Contents (C# names in the doc comments): `WheelOutcomeKind` (`WheelOutcomeKind`), `WheelGestureInterpreter`,
//! `PointerReleaseAction`, `ClickZoomTarget`, `PointerGestures` (free functions here), `MiddleClickResolver` (`resolve_middle_click`).
//!
//! NOT here: `MainWindowHelpers.CalculateZoomToPointOffsets` (the 4 `ZoomToPoint_*` tests in `MouseGesturesTests.cs`) lives in
//! `misc::window_helpers` (W8-04e). The `WheelMessageSource` decoding test lives in W8-05a. Units W8-05g/05h consume this module.
//!
//! Lead-owned: `input/mod.rs` gets `pub mod mouse_gestures;` (W8-05e merge).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use pr_core::model::enums::{KeyboardZoomAnchor, MiddleClickAction, MouseWheelAction};

use crate::input::primitives::PointD;
use crate::input::review_command::ReviewCommand;

/// C# `WheelGestureInterpreter.NotchDelta` (`WHEEL_DELTA`): one wheel notch. Navigation moves one image per notch.
pub const NOTCH_DELTA: i32 = 120;

/// C# `PointerGestures.SameZoomTolerance`: zooms closer than this (0.1 %) count as "already at the click zoom".
/// The comparison is strict (`<`): a difference of exactly the tolerance is NOT the same zoom.
pub const SAME_ZOOM_TOLERANCE: f64 = 0.001;

/// C# `WheelOutcomeKind` (public enum). Explicit values match C# declaration order: `None = 0`, `Zoom = 1`, `Next = 2`,
/// `Previous = 3`. `None` here is `WheelOutcomeKind::None`, never `Option::None` (always qualified).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WheelOutcomeKind {
    /// Nothing yet (a high-resolution wheel has not reached a full notch), or a zero delta.
    None = 0,
    /// One zoom step at the cursor; the sign of the event's delta gives the direction.
    Zoom = 1,
    /// Go to the next image (wheel down, negative delta in WPF).
    Next = 2,
    /// Go to the previous image (wheel up).
    Previous = 3,
}

/// C# `WheelGestureInterpreter` (`internal sealed class`): decides what a wheel event does from `MouseWheelAction` and Ctrl,
/// and turns high-resolution deltas into whole notches for navigation.
///
/// Port notes (contract "Semantics" 1..4): the accumulator is `i32` with `wrapping_add` (C# `+=` is unchecked); the sign
/// comparison uses `Math.Sign` semantics; `Math.Abs(int.MinValue)` throws in C# and is unreachable here (open question 3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WheelGestureInterpreter {
    /// C# `_accumulated`: partial navigation delta; `0` when no partial notch is pending.
    accumulated: i32,
}

impl WheelGestureInterpreter {
    /// C# `new WheelGestureInterpreter()`: no partial notch.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Handle(int delta, bool ctrlPressed, MouseWheelAction mode)`.
    ///
    /// Ctrl, or any mode other than `Navigate` (`Zoom`), clears the partial sum and returns `None` for a zero delta, else `Zoom`.
    /// In `Navigate`: a zero delta returns `None` without touching the sum; a sign change drops the partial sum; then the
    /// delta is added and, once `|sum| >= NOTCH_DELTA`, the sum is cleared and `Next` (sum < 0, wheel down) or `Previous` is
    /// returned. At most ONE image per event: the excess is dropped (no queued burst).
    pub fn handle(&mut self, delta: i32, ctrl_pressed: bool, mode: MouseWheelAction) -> WheelOutcomeKind {
        todo!()
    }

    /// C# `Reset()`: forget a partial notch (navigation by other means, mode change).
    pub fn reset(&mut self) {
        todo!()
    }
}

/// C# `PointerReleaseAction` (public enum). Values: `None = 0`, `ClickZoom = 1`, `StartKinetic = 2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PointerReleaseAction {
    /// Nothing happens on release.
    None = 0,
    /// A click: toggle between Fit and the click zoom at the cursor.
    ClickZoom = 1,
    /// The end of a drag-pan: glide on with the release velocity.
    StartKinetic = 2,
}

/// C# `ClickZoomTarget` (public enum). Values: `ClickZoom = 0`, `Fit = 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ClickZoomTarget {
    /// Zoom to the click zoom percent, keeping the point under the cursor.
    ClickZoom = 0,
    /// Back to Fit.
    Fit = 1,
}

/// C# `PointerGestures.IsFitDoubleClick(int clickCount)`: a second (or later) press of a multi-click always means Fit (DF03).
/// True exactly when `click_count >= 2`.
pub fn is_fit_double_click(click_count: i32) -> bool {
    todo!()
}

/// C# `PointerGestures.ClassifyRelease(dragged, panned, pressWasConsumed, clickToZoomEnabled, kineticPanEnabled)`.
///
/// `dragged`: the pointer crossed the system drag threshold during the press. `panned`: the press scrolled the image.
/// `press_was_consumed`: the press only stopped a running glide, or was the second press of a double-click.
/// Rule: if `dragged`, `StartKinetic` only when `panned && kinetic_pan_enabled`, else `None`. Otherwise `None` when
/// `press_was_consumed || !click_to_zoom_enabled`, else `ClickZoom`.
pub fn classify_release(
    dragged: bool,
    panned: bool,
    press_was_consumed: bool,
    click_to_zoom_enabled: bool,
    kinetic_pan_enabled: bool,
) -> PointerReleaseAction {
    todo!()
}

/// C# `PointerGestures.DecideClickZoom(bool isFit, double currentZoom, double clickZoom)`.
///
/// `Fit` only when `!is_fit && |current_zoom - click_zoom| < SAME_ZOOM_TOLERANCE` (strict); otherwise `ClickZoom`.
/// NaN in either zoom makes the comparison false, so the result is `ClickZoom` (as C# `Math.Abs(NaN) < tol` is false).
pub fn decide_click_zoom(is_fit: bool, current_zoom: f64, click_zoom: f64) -> ClickZoomTarget {
    todo!()
}

/// C# `PointerGestures.ClickZoomFactor(int percent)`: the `ClickZoomPercent` setting as an original-relative factor (ADR 0008).
///
/// `percent.clamp(MIN_CLICK_ZOOM_PERCENT, MAX_CLICK_ZOOM_PERCENT) as f64 / 100.0`, the clamp in the integer domain first
/// (C# `Math.Clamp(int, int, int)` then `/ 100.0`). Bounds: `pr_core::settings::app_settings::{MIN,MAX}_CLICK_ZOOM_PERCENT` (10, 800).
pub fn click_zoom_factor(percent: i32) -> f64 {
    todo!()
}

/// C# `PointerGestures.ResolveKeyboardZoomAnchor(KeyboardZoomAnchor mode, PointD? pointer, double viewportWidth, double viewportHeight)`
/// (feat/zoom-key-anchor).
///
/// `Pointer` mode with `Some(p)` (pointer over the viewport) returns `p`. Otherwise, including `ViewportCentre` mode for every
/// pointer value, returns the viewport centre `(viewport_width / 2, viewport_height / 2)`. The point is in ImageScroll coordinates.
pub fn resolve_keyboard_zoom_anchor(
    mode: KeyboardZoomAnchor,
    pointer: Option<PointD>,
    viewport_width: f64,
    viewport_height: f64,
) -> PointD {
    todo!()
}

/// C# `MiddleClickResolver.Resolve(MiddleClickAction action)` (`internal static class`): maps the middle-click action to the
/// existing `ReviewCommand` that the same-named keyboard shortcut runs, so the window executes it through one switch.
///
/// Mapping: `ClickZoom -> CLICK_ZOOM`, `ActualSize -> ZOOM_ACTUAL_SIZE`, `Fit -> TOGGLE_FIT`, `FitWidth -> FIT_WIDTH`,
/// `FitWidth2 -> FIT_WIDTH2`, `PreviousImage -> PREVIOUS`, `NextImage -> NEXT`, `PreviousFolder -> PREVIOUS_FOLDER`,
/// `NextFolder -> NEXT_FOLDER`, `OpenFolder -> OPEN_FOLDER`. `None` maps to `None`. (C# also maps undefined values to `null`:
/// a Rust enum is closed, so that case is not representable here; see contract "Traps".)
pub fn resolve_middle_click(action: MiddleClickAction) -> Option<ReviewCommand> {
    todo!()
}
