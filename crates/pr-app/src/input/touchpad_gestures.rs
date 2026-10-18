//! Port of `PhotoReview.App.Shared/Input/TouchpadGestures.cs` (baseline `d92d6aae`, 172 lines), feature Q-TOUCHPAD-REFRESH:
//! tells a precision-touchpad two-finger swipe apart from a notched mouse wheel, turns a swipe at Fit into image changes by
//! distance, and routes a touchpad message (pan when zoomed, navigate at Fit, ignore sideways at Fit).
//!
//! Contents: `WheelDeviceHint`, `WheelInput` (C# `record struct`), `TouchpadWheelClassifier`, `TouchpadSwipeNavigator`,
//! `TouchpadWheelAction`, and the static rules `TouchpadGestureRules` (free functions `route` and `pan_offset_delta`).
//!
//! Depends on `mouse_gestures` for `WheelOutcomeKind` and `NOTCH_DELTA` (C# `WheelGestureInterpreter.NotchDelta`).
//! NOT here: `WheelMessageSource.Delta/ScreenPoint` (W8-05a `input::wheel_message`) and the OS source query `GetCurrentInputMessageSource`
//! (`pr_platform_win`, W8-14a, which maps it to `WheelDeviceHint`).
//!
//! Lead-owned: `input/mod.rs` gets `pub mod touchpad_gestures;` (W8-05e merge).

#![allow(unused_variables, unused_imports, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::mouse_gestures::{NOTCH_DELTA, WheelOutcomeKind};

/// C# `TouchpadWheelClassifier.GestureGapMs`: messages closer than this belong to one gesture (a touchpad sends one every
/// ~8 to 16 ms while moving). "Within the gap" means `0 <= now - last <= GESTURE_GAP_MS` (wrap-safe, see `is_within_gap`).
pub const GESTURE_GAP_MS: i32 = 250;

/// C# `TouchpadSwipeNavigator.FirstStepFraction`: the first image of a swipe changes after this fraction of the distance.
pub const FIRST_STEP_FRACTION: f64 = 0.5;

/// C# `TouchpadGestureRules.PanDipPerDeltaUnit`: DIP scrolled per wheel-delta unit of a touchpad pan (120 units = 120 DIP).
pub const PAN_DIP_PER_DELTA_UNIT: f64 = 1.0;

/// C# `WheelDeviceHint` (`internal enum`). `Unknown = 0` (default: decide from the deltas), `Touchpad = 1` (the OS said so).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum WheelDeviceHint {
    /// Not reported: decide from the deltas.
    #[default]
    Unknown = 0,
    /// `GetCurrentInputMessageSource` reported a touchpad as the source.
    Touchpad = 1,
}

/// C# `WheelInput` (`internal readonly record struct`, positional `(int Delta, bool Horizontal, bool Ctrl, int Timestamp,
/// WheelDeviceHint Hint = Unknown)`). One wheel message over the main image.
///
/// `delta`: WHEEL_DELTA units (120 per notch); vertical positive = up, horizontal (WM_MOUSEHWHEEL) positive = right.
/// `horizontal`: true for WM_MOUSEHWHEEL. `ctrl`: Ctrl held (a touchpad pinch arrives as Ctrl+wheel).
/// `timestamp`: message time in ms on the Environment.TickCount clock (it wraps; compare with wrapping arithmetic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WheelInput {
    pub delta: i32,
    pub horizontal: bool,
    pub ctrl: bool,
    pub timestamp: i32,
    pub hint: WheelDeviceHint,
}

impl WheelInput {
    /// C# `new WheelInput(delta, horizontal, ctrl, timestamp)` (the hint defaults to `Unknown`).
    pub const fn new(delta: i32, horizontal: bool, ctrl: bool, timestamp: i32) -> Self {
        Self {
            delta,
            horizontal,
            ctrl,
            timestamp,
            hint: WheelDeviceHint::Unknown,
        }
    }

    /// C# `new WheelInput(..., hint)`: the same message with the OS device hint set.
    pub const fn with_hint(self, hint: WheelDeviceHint) -> Self {
        Self { hint, ..self }
    }
}

/// C# `TouchpadWheelClassifier` (`internal sealed class`): a message is a touchpad one when the OS hint says so, OR its delta is
/// not a whole number of notches (`delta % NOTCH_DELTA != 0`), OR it follows a touchpad message within `GESTURE_GAP_MS`
/// (`in_gesture`). Otherwise it is a mouse wheel. Each call updates the state (`in_gesture = result`, and `last_timestamp` is set
/// only when the result is touchpad).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TouchpadWheelClassifier {
    /// C# `_inGesture`: the previous message was classified as touchpad.
    in_gesture: bool,
    /// C# `_lastTimestamp`: timestamp of the last touchpad message.
    last_timestamp: i32,
}

impl TouchpadWheelClassifier {
    /// C# `new TouchpadWheelClassifier()`: no gesture in progress.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `IsTouchpad(int delta, int timestamp, WheelDeviceHint hint)`: true when the message is a touchpad message.
    pub fn is_touchpad(&mut self, delta: i32, timestamp: i32, hint: WheelDeviceHint) -> bool {
        todo!()
    }

    /// C# `Reset()`: the next message starts a new decision (no gesture in progress).
    pub fn reset(&mut self) {
        todo!()
    }
}

/// C# `TouchpadWheelClassifier.IsWithinGap(int last, int now)` (`internal static`): true when `now` is at most `GESTURE_GAP_MS`
/// after `last`. Elapsed time is `now.wrapping_sub(last)` (C# `unchecked`), so a TickCount wrap counts as the true elapsed time;
/// a negative elapsed time (timestamp went backwards) is false.
pub fn is_within_gap(last: i32, now: i32) -> bool {
    todo!()
}

/// C# `TouchpadSwipeNavigator` (`internal sealed class`): turns the vertical deltas of a touchpad swipe at Fit into image changes
/// BY DISTANCE. The first image of a swipe changes after `round_half_even(distance * FIRST_STEP_FRACTION)` (min 1), each later
/// image after a further full `distance`. At most one image per message; a message larger than a step leaves no queued remainder
/// when `|remainder| >= distance` (the remainder is then cleared). A reversal of direction, or a message more than `GESTURE_GAP_MS`
/// after the previous one, starts a fresh swipe.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TouchpadSwipeNavigator {
    /// C# `_active`: a swipe message has been seen since the last `reset`.
    active: bool,
    /// C# `_stepped`: the first image of the current swipe has already changed.
    stepped: bool,
    /// C# `_accumulated`: delta of the current swipe since its last image (or the fresh-swipe start).
    accumulated: i32,
    /// C# `_lastTimestamp`: timestamp of the last message handled (any delta, zero included).
    last_timestamp: i32,
    /// C# `GestureTotal` (get, private set): total delta of the current swipe (diagnostics).
    gesture_total: i32,
    /// C# `GestureImages` (get, private set): images changed by the current swipe.
    gesture_images: i32,
}

impl TouchpadSwipeNavigator {
    /// C# `new TouchpadSwipeNavigator()`: no swipe in progress.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Handle(int delta, int timestamp, int distancePerImage)`.
    ///
    /// Order of operations (must match C#): `distance = max(1, distance_per_image)`; `fresh = !active || !is_within_gap(last,
    /// timestamp)`; `active = true`; `last_timestamp = timestamp`; a zero delta returns `None`; when `fresh` or the sum is non-zero
    /// with the opposite sign, clear `accumulated`, `stepped`, `gesture_total`, `gesture_images`; then add `delta` to `accumulated`
    /// and `gesture_total` (wrapping); `required = if stepped { distance } else { max(1, round_half_even(distance * 0.5)) }`;
    /// `|accumulated| < required` returns `None`; otherwise `down = accumulated < 0`, `remainder = accumulated - sign * required`,
    /// `accumulated = if |remainder| >= distance { 0 } else { remainder }`, `stepped = true`, `gesture_images += 1`, and return
    /// `Next` (down) or `Previous`.
    pub fn handle(&mut self, delta: i32, timestamp: i32, distance_per_image: i32) -> WheelOutcomeKind {
        todo!()
    }

    /// C# `Reset()`: forget the swipe in progress (`active`, `accumulated`, `stepped` cleared; the totals are kept until the
    /// next fresh swipe, as in C#).
    pub fn reset(&mut self) {
        todo!()
    }

    /// C# `GestureTotal`: total delta of the swipe in progress.
    pub fn gesture_total(&self) -> i32 {
        todo!()
    }

    /// C# `GestureImages`: images changed by the swipe in progress.
    pub fn gesture_images(&self) -> i32 {
        todo!()
    }
}

/// C# `TouchpadWheelAction` (public enum): what one touchpad wheel message does. Values: `MouseWheel = 0`, `Ignore = 1`,
/// `Pan = 2`, `Navigate = 3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TouchpadWheelAction {
    /// Not a touchpad swipe: the mouse-wheel path (`WheelGestureInterpreter`) handles it.
    MouseWheel = 0,
    /// Ignored (a sideways swipe at Fit, or a sideways message with the feature off or Ctrl held).
    Ignore = 1,
    /// Scroll the zoomed image (`pan_offset_delta`).
    Pan = 2,
    /// Feed the vertical delta to `TouchpadSwipeNavigator`.
    Navigate = 3,
}

/// C# `TouchpadGestureRules.Route(WheelInput input, bool enabled, bool isTouchpad, bool canPan)` (`internal static class`,
/// the pure routing rule for a touchpad wheel message).
///
/// Rule, in order: if `input.ctrl || !enabled || !is_touchpad`, return `Ignore` for a horizontal message and `MouseWheel`
/// otherwise (Ctrl pinch always keeps the mouse/zoom path). Else if `can_pan`, return `Pan`. Else `Ignore` for horizontal,
/// `Navigate` for vertical.
pub fn route(input: WheelInput, enabled: bool, is_touchpad: bool, can_pan: bool) -> TouchpadWheelAction {
    todo!()
}

/// C# `TouchpadGestureRules.PanOffsetDelta(WheelInput input)`: the scroll-offset change `(horizontal, vertical)` for a touchpad pan.
///
/// Horizontal message: `(delta * PAN_DIP_PER_DELTA_UNIT, 0)`. Vertical message: `(0, -delta * PAN_DIP_PER_DELTA_UNIT)`, so an
/// upward swipe (`delta > 0`) scrolls towards the top (negative offset change). The vertical negation is done on the integer
/// (`delta.wrapping_neg()`, C# unchecked), then converted, so a zero delta gives `+0.0`, never `-0.0`.
pub fn pan_offset_delta(input: WheelInput) -> (f64, f64) {
    todo!()
}
