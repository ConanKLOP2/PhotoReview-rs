//! Port of `PhotoReview.App.Shared/Coordinators/ImageTransitionDecision.cs` (29 lines, feat/image-crossfade).
//!
//! Pure, side-effect-free decision: should a bitmap assignment play the optional image-change fade?

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
        if is_compare_visible {
            return false;
        }
        let Some(new_path) = new_path else {
            return false;
        };
        let Some(previous_path) = previous_path else {
            return false;
        };
        !pr_core::text::eq_ignore_case(previous_path, new_path)
    }
}
