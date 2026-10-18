//! Port of `PhotoReview.App.Shared/Coordinators/ImageTransitionDecision.cs` (29 lines, feat/image-crossfade).
//!
//! Pure, side-effect-free decision: should a bitmap assignment play the optional image-change fade?
//! Stub: body is `todo!()` (contract W8-06b).

#![allow(unused_variables, dead_code)] // D11: removed in the same commit that removes the last todo!()

/// C# `public static class ImageTransitionDecision`. Unit struct, associated functions only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageTransitionDecision;

impl ImageTransitionDecision {
    /// C# `ShouldTransition(string? previousPath, string? newPath, bool isCompareVisible)`.
    ///
    /// Returns `true` only when all hold, in this order:
    /// 1. `is_compare_visible` is `false` (Compare on screen never fades);
    /// 2. `new_path` is `Some` (clearing the display is never a target);
    /// 3. `previous_path` is `Some` (the first image after a folder open has nothing to fade from);
    /// 4. the two paths differ under `OrdinalIgnoreCase`, i.e. `!pr_core::text::eq_ignore_case(prev, new)`.
    ///    `Some("")` is a path (not `None`); `""` against `"a"` differs.
    pub fn should_transition(previous_path: Option<&str>, new_path: Option<&str>, is_compare_visible: bool) -> bool {
        todo!()
    }
}
