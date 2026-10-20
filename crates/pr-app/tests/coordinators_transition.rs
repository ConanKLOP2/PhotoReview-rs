//! Tests of `crates/pr-app/src/coordinators/transition.rs` (contract `contracts/W8-06b.md`, sub-part (c)): the C# twins of
//! `ImageTransitionDecisionTests` (`Trait HotPath`). Port of `PhotoReview.App.Shared/Coordinators/ImageTransitionDecision.cs`
//! (29 lines, feat/image-crossfade). Case-folding pairs and `null` / `""` rows are in the golden `image-transition`
//! (`golden_info_overlay.rs`).
//!
//! Every test here fails through `todo!()` until the body lands.

use pr_app::coordinators::transition::ImageTransitionDecision;

// C#: ImageTransitionDecisionTests.ShouldTransition_DifferentFile_ReturnsTrue
#[test]
fn should_transition_different_file_returns_true() {
    let result = ImageTransitionDecision::should_transition(Some(r"C:\photos\a.jpg"), Some(r"C:\photos\b.jpg"), false);

    assert!(result);
}

// C#: ImageTransitionDecisionTests.ShouldTransition_SameFileUpgrade_ReturnsFalse
// thumbnail -> preview -> original within the same navigation: identical path, must stay instant.
#[test]
fn should_transition_same_file_upgrade_returns_false() {
    let result = ImageTransitionDecision::should_transition(Some(r"C:\photos\a.jpg"), Some(r"C:\photos\a.jpg"), false);

    assert!(!result);
}

// C#: ImageTransitionDecisionTests.ShouldTransition_SameFileDifferentCasing_ReturnsFalse
// Windows paths are case-insensitive (OrdinalIgnoreCase); a re-cased entry is still the same file.
#[test]
fn should_transition_same_file_different_casing_returns_false() {
    let result = ImageTransitionDecision::should_transition(Some(r"C:\photos\A.JPG"), Some(r"C:\photos\a.jpg"), false);

    assert!(!result);
}

// C#: ImageTransitionDecisionTests.ShouldTransition_FirstImageAfterFolderOpen_ReturnsFalse
// No previously presented file: nothing to fade from.
#[test]
fn should_transition_first_image_after_folder_open_returns_false() {
    let result = ImageTransitionDecision::should_transition(None, Some(r"C:\photos\a.jpg"), false);

    assert!(!result);
}

// C#: ImageTransitionDecisionTests.ShouldTransition_ClearingTheDisplay_ReturnsFalse
// A `None` new path means the display is being cleared (error or empty state), never a fade target.
#[test]
fn should_transition_clearing_the_display_returns_false() {
    let result = ImageTransitionDecision::should_transition(Some(r"C:\photos\a.jpg"), None, false);

    assert!(!result);
}

// C#: ImageTransitionDecisionTests.ShouldTransition_CompareVisible_ReturnsFalseEvenForADifferentFile
#[test]
fn should_transition_compare_visible_returns_false_even_for_a_different_file() {
    let result = ImageTransitionDecision::should_transition(Some(r"C:\photos\a.jpg"), Some(r"C:\photos\b.jpg"), true);

    assert!(!result);
}
