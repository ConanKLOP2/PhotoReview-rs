//! Port of `PhotoReview.App.Shared/Coordinators/FadeTargetGate.cs` (18 lines).
//!
//! Remembers the opacity an element was last faded to, so a caller that re-evaluates its state on every mouse
//! move starts an animation only when the target changes.

/// C# `public sealed class FadeTargetGate` with `private double? _target`.
#[derive(Debug, Clone, Default)]
pub struct FadeTargetGate {
    /// `None` until the first accepted target (C# `null`).
    target: Option<f64>,
}

impl FadeTargetGate {
    /// Fresh gate: no target yet, so the first `try_change` always returns `true`.
    pub fn new() -> Self {
        Self { target: None }
    }

    /// C# `TryChange(double target)`: `false` when `target` equals the last accepted one (then nothing changes);
    /// otherwise records it and returns `true`.
    ///
    /// Equality is C# `==` on `double?` against `double`, which is IEEE `==`: `0.0 == -0.0` is equal, and `NaN`
    /// is never equal to anything, so a `NaN` target returns `true` on every call. Do NOT use `f64::to_bits`
    /// or `PartialEq` on a wrapper with NaN-aware semantics here.
    pub fn try_change(&mut self, target: f64) -> bool {
        // `Option<f64> == Some(t)` uses IEEE equality on the inner value (contract trap T1).
        if self.target == Some(target) {
            return false;
        }
        self.target = Some(target);
        true
    }
}
