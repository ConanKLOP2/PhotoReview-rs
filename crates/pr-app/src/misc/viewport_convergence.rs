//! Port of `PhotoReview.App.Shared/MainWindowConvergence.cs` (C# `ViewportConvergence` and `ValidationResult`, baseline d92d6aae).
//! Unit W8-04e. Contract: `contracts/W8-04e.md`.
//!
//! Pure module: no `unsafe`, no `windows` crate. `is_stable_viewport` is passed as a `fn` pointer (`StabilityRule`) by
//! `coordinators::fit` (W8-01d, decision D-01d-4), so its signature must stay exactly `fn(&ViewportSnapshot, &ViewportSnapshot) -> bool`.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

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
        todo!()
    }

    /// C# `ValidationResult.InvalidDimensions(reason)`: `(false, reason)`. The name is kept although it also serves
    /// the offset, max-image and actual-image groups (C# does the same).
    pub fn invalid_dimensions(reason: &'static str) -> Self {
        todo!()
    }
}

/// C# `ViewportConvergence.IsStableViewport(before, after)`: true when the two snapshots agree within [`EPSILON`] on the
/// viewport and extent sizes and have the same scroll bar visibility. Only `after` is validated (C# `ValidateMeasurements(after)`
/// first, and an invalid `after` returns false); `before` is not, so a NaN `before` value simply fails the `<` comparison.
/// Zoom, stretch, offsets and actual/max image sizes are NOT compared (C# does not compare them either).
pub fn is_stable_viewport(before: &ViewportSnapshot, after: &ViewportSnapshot) -> bool {
    todo!()
}

/// C# `ViewportConvergence.ValidateMeasurements(snapshot)`: checks the four groups in this order and reports the first invalid one:
/// 1. viewport width and height positive and finite -> `"Viewport not ready"`;
/// 2. horizontal and vertical offset non-negative and finite -> `"Offsets invalid"`;
/// 3. max image width and height non-negative and finite -> `"MaxImage not set"`;
/// 4. actual image width and height non-negative and finite -> `"ActualImage invalid"`.
///
/// A zero image size and a zero offset are valid (C# `IsNonNegativeFinite`); a zero viewport is not.
pub fn validate_measurements(snapshot: &ViewportSnapshot) -> ValidationResult {
    todo!()
}

/// C# `IsPositiveFinite`: `value > 0 && double.IsFinite(value)`. NaN is false.
fn is_positive_finite(value: f64) -> bool {
    todo!()
}

/// C# `IsNonNegativeFinite`: `value >= 0 && double.IsFinite(value)`. NaN is false.
fn is_non_negative_finite(value: f64) -> bool {
    todo!()
}
