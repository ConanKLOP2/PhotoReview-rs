//! Port of `PhotoReview.App.Shared/MainWindowConvergence.cs` (C# `ViewportConvergence` and `ValidationResult`, baseline d92d6aae).
//! Unit W8-04e. Contract: `contracts/W8-04e.md`.
//!
//! Pure module: no `unsafe`, no `windows` crate. `is_stable_viewport` is passed as a `fn` pointer (`StabilityRule`) by
//! `coordinators::fit` (W8-01d, decision D-01d-4), so its signature must stay exactly `fn(&ViewportSnapshot, &ViewportSnapshot) -> bool`.

use crate::viewport::types::ViewportSnapshot;

/// C# `private const double Epsilon = 0.5;`: DIP tolerance for subpixel and DPI noise (strict `<` in the comparison).
pub const EPSILON: f64 = 0.5;

/// C# `ValidationResult(bool IsValid, string? Reason)`, `internal record`.
///
/// `reason` is an internal English diagnostic, not a UI sentence: C# does not localize it either, so it stays a
/// `&'static str` and is not routed through `pr_l10n` (D-W8-4 is about UI text; these strings are never shown to the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub reason: Option<&'static str>,
}

impl ValidationResult {
    /// C# `ValidationResult.Valid()`: `(true, null)`.
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            reason: None,
        }
    }

    /// C# `ValidationResult.InvalidDimensions(reason)`: `(false, reason)`. The name is kept although it also serves
    /// the offset, max-image and actual-image groups (C# does the same).
    pub fn invalid_dimensions(reason: &'static str) -> Self {
        Self {
            is_valid: false,
            reason: Some(reason),
        }
    }
}

/// C# `ViewportConvergence.IsStableViewport(before, after)`: true when the two snapshots agree within [`EPSILON`] on the
/// viewport and extent sizes and have the same scroll bar visibility. Only `after` is validated (C# `ValidateMeasurements(after)`
/// first, and an invalid `after` returns false); `before` is not, so a NaN `before` value simply fails the `<` comparison.
/// Zoom, stretch, offsets and actual/max image sizes are NOT compared (C# does not compare them either).
pub fn is_stable_viewport(before: &ViewportSnapshot, after: &ViewportSnapshot) -> bool {
    if !validate_measurements(after).is_valid {
        return false;
    }
    (before.viewport_width - after.viewport_width).abs() < EPSILON
        && (before.viewport_height - after.viewport_height).abs() < EPSILON
        && (before.extent_width - after.extent_width).abs() < EPSILON
        && (before.extent_height - after.extent_height).abs() < EPSILON
        && before.horizontal_scroll_bar_visible == after.horizontal_scroll_bar_visible
        && before.vertical_scroll_bar_visible == after.vertical_scroll_bar_visible
}

/// C# `ViewportConvergence.ValidateMeasurements(snapshot)`: checks the four groups in this order and reports the first invalid one:
/// 1. viewport width and height positive and finite -> `"Viewport not ready"`;
/// 2. horizontal and vertical offset non-negative and finite -> `"Offsets invalid"`;
/// 3. max image width and height non-negative and finite -> `"MaxImage not set"`;
/// 4. actual image width and height non-negative and finite -> `"ActualImage invalid"`.
///
/// A zero image size and a zero offset are valid (C# `IsNonNegativeFinite`); a zero viewport is not.
pub fn validate_measurements(snapshot: &ViewportSnapshot) -> ValidationResult {
    if !is_positive_finite(snapshot.viewport_width) || !is_positive_finite(snapshot.viewport_height) {
        return ValidationResult::invalid_dimensions("Viewport not ready");
    }
    if !is_non_negative_finite(snapshot.horizontal_offset) || !is_non_negative_finite(snapshot.vertical_offset) {
        return ValidationResult::invalid_dimensions("Offsets invalid");
    }
    if !is_non_negative_finite(snapshot.max_image_width) || !is_non_negative_finite(snapshot.max_image_height) {
        return ValidationResult::invalid_dimensions("MaxImage not set");
    }
    if !is_non_negative_finite(snapshot.actual_image_width) || !is_non_negative_finite(snapshot.actual_image_height) {
        return ValidationResult::invalid_dimensions("ActualImage invalid");
    }
    ValidationResult::valid()
}

/// C# `IsPositiveFinite`: `value > 0 && double.IsFinite(value)`. NaN is false.
fn is_positive_finite(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}

/// C# `IsNonNegativeFinite`: `value >= 0 && double.IsFinite(value)`. NaN is false.
fn is_non_negative_finite(value: f64) -> bool {
    value >= 0.0 && value.is_finite()
}
