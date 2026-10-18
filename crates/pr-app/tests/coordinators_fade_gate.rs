//! Tests of `crates/pr-app/src/coordinators/fade_gate.rs` (contract `contracts/W8-06b.md`, sub-part (b)): the C# twins of
//! `FadeTargetGateTests`. Port of `PhotoReview.App.Shared/Coordinators/FadeTargetGate.cs` (18 lines).
//!
//! The NaN, `-0` and infinity rules (traps T1) are covered by the golden `fade-target-gate` in `golden_info_overlay.rs`.
//! Every test here fails through `todo!()` until the body lands.

use pr_app::coordinators::fade_gate::FadeTargetGate;

// C#: FadeTargetGateTests.TryChange_SameTargetRepeated_OnlyFirstStarts
#[test]
fn try_change_same_target_repeated_only_first_starts() {
    let mut gate = FadeTargetGate::new();

    assert!(gate.try_change(1.0));
    assert!(!gate.try_change(1.0));
    assert!(!gate.try_change(1.0));
}

// C#: FadeTargetGateTests.TryChange_DifferentTarget_Starts
#[test]
fn try_change_different_target_starts() {
    let mut gate = FadeTargetGate::new();
    assert!(gate.try_change(1.0));

    assert!(gate.try_change(0.0));
    assert!(!gate.try_change(0.0));
    assert!(gate.try_change(0.6));
    assert!(gate.try_change(0.0));
}
