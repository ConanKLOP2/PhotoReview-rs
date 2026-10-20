//! Tests of `crates/pr-app/src/coordinators/autohide.rs` (contract `contracts/W8-06b.md`, sub-part (a)): the C# twins of
//! `InfoOverlayAutoHidePolicyTests` (Q-R34) and `ToolbarAutoHidePolicyTests` (feat/ui-dark-chrome-toolbar). Port of
//! `PhotoReview.App.Shared/Coordinators/InfoOverlayAutoHidePolicy.cs` (33 lines) and `ToolbarAutoHidePolicy.cs` (38 lines).
//!
//! Theory rows are looped inside one test per C# method, and each row's failure message names the row. The golden
//! `info-overlay-autohide` and `toolbar-autohide` cases are in `golden_info_overlay.rs`.
//!
//! Every test here fails through `todo!()` until the bodies land.

use pr_app::coordinators::autohide::{InfoOverlayAutoHidePolicy, ToolbarAutoHidePolicy};
use pr_core::settings::app_settings::AppSettings;

/// C# `Eval(enabled, folder, attention, compare, active, idle)`: returns `(opacity, is_hit_test_visible)`.
fn eval_info(enabled: bool, folder: bool, attention: bool, compare: bool, active: bool, idle: bool) -> (f64, bool) {
    let outcome = InfoOverlayAutoHidePolicy::evaluate(enabled, folder, attention, compare, active, idle);
    (outcome.opacity, outcome.is_hit_test_visible)
}

// C#: InfoOverlayAutoHidePolicyTests.Enabled_IdleElapsed_NothingForcesVisibility_HidesAndIsClickThrough
#[test]
fn enabled_idle_elapsed_nothing_forces_visibility_hides_and_is_click_through() {
    let (opacity, hit_test_visible) = eval_info(true, true, false, false, true, true);

    assert_eq!(opacity, 0.0);
    assert!(!hit_test_visible);
}

// C#: InfoOverlayAutoHidePolicyTests.Enabled_IdleNotElapsed_StaysVisibleAndHitTestable
#[test]
fn enabled_idle_not_elapsed_stays_visible_and_hit_testable() {
    let (opacity, hit_test_visible) = eval_info(true, true, false, false, true, false);

    assert_eq!(opacity, 1.0);
    assert!(hit_test_visible);
}

// C#: InfoOverlayAutoHidePolicyTests.Disabled_NeverHides_EvenWhenIdle
#[test]
fn disabled_never_hides_even_when_idle() {
    let (opacity, hit_test_visible) = eval_info(false, true, false, false, true, true);

    assert_eq!(opacity, 1.0);
    assert!(hit_test_visible);
}

// C#: InfoOverlayAutoHidePolicyTests.IdleElapsed_ButAStayVisibleRuleApplies_StaysVisible (4 InlineData rows)
#[test]
fn idle_elapsed_but_a_stay_visible_rule_applies_stays_visible() {
    let rows = [
        (false, false, false, true), // no folder open
        (true, true, false, true),   // loading / error / warning / event message showing
        (true, false, true, true),   // Compare open
        (true, false, false, false), // window inactive (dialog, Settings, other app)
    ];
    for (folder, attention, compare, active) in rows {
        let (opacity, hit_test_visible) = eval_info(true, folder, attention, compare, active, true);
        let row = format!("folder={folder} attention={attention} compare={compare} active={active}");
        assert_eq!(opacity, 1.0, "{row}: opacity");
        assert!(hit_test_visible, "{row}: hit test");
        assert!(
            InfoOverlayAutoHidePolicy::must_stay_visible(true, folder, attention, compare, active),
            "{row}: must_stay_visible"
        );
    }
}

// C#: InfoOverlayAutoHidePolicyTests.Matrix_HiddenOnlyWhenEnabledAndIdleAndNoStayVisibleRule (4 InlineData rows)
#[test]
fn matrix_hidden_only_when_enabled_and_idle_and_no_stay_visible_rule() {
    for (enabled, idle) in [(true, true), (true, false), (false, true), (false, false)] {
        let (opacity, _) = eval_info(enabled, true, false, false, true, idle);
        let hidden = opacity == 0.0;
        assert_eq!(hidden, enabled && idle, "enabled={enabled} idle={idle}");
    }
}

// C#: InfoOverlayAutoHidePolicyTests.DelayMs_UsesOnlyTheInfoDelay_IndependentOfTheToolbarDelay
#[test]
fn delay_ms_uses_only_the_info_delay_independent_of_the_toolbar_delay() {
    let mut settings = AppSettings {
        toolbar_auto_hide_delay_ms: 9000,
        info_overlay_auto_hide_delay_ms: 3000,
        ..AppSettings::default()
    };
    assert_eq!(InfoOverlayAutoHidePolicy::delay_ms(&settings), 3000);

    settings.toolbar_auto_hide_delay_ms = 1;
    assert_eq!(InfoOverlayAutoHidePolicy::delay_ms(&settings), 3000);

    settings.info_overlay_auto_hide_delay_ms = 4200;
    assert_eq!(InfoOverlayAutoHidePolicy::delay_ms(&settings), 4200);
}

// C#: InfoOverlayAutoHidePolicyTests.DelayMs_IsClampedToTheAllowedRange (2 InlineData rows)
#[test]
fn delay_ms_is_clamped_to_the_allowed_range() {
    for (stored, expected) in [(-5, 0), (99_999, 10_000)] {
        let settings = AppSettings {
            info_overlay_auto_hide_delay_ms: stored,
            ..AppSettings::default()
        };
        assert_eq!(
            InfoOverlayAutoHidePolicy::delay_ms(&settings),
            expected,
            "stored {stored}"
        );
    }
}

// C#: InfoOverlayAutoHidePolicyTests.MustStayVisible_OnlyTheDefaultStateIsEligibleToHide
#[test]
fn must_stay_visible_only_the_default_state_is_eligible_to_hide() {
    assert!(!InfoOverlayAutoHidePolicy::must_stay_visible(
        true, true, false, false, true
    ));
}

// C#: ToolbarAutoHidePolicyTests.MustStayVisible_AutoHideDisabled_IsTrueRegardlessOfOtherState
#[test]
fn toolbar_must_stay_visible_auto_hide_disabled_is_true_regardless_of_other_state() {
    assert!(ToolbarAutoHidePolicy::must_stay_visible(false, true, false, false));
}

// C#: ToolbarAutoHidePolicyTests.MustStayVisible_NoFolderOpen_IsTrue
#[test]
fn toolbar_must_stay_visible_no_folder_open_is_true() {
    assert!(ToolbarAutoHidePolicy::must_stay_visible(true, false, false, false));
}

// C#: ToolbarAutoHidePolicyTests.MustStayVisible_ToolsPopupOpen_IsTrue
#[test]
fn toolbar_must_stay_visible_tools_popup_open_is_true() {
    assert!(ToolbarAutoHidePolicy::must_stay_visible(true, true, true, false));
}

// C#: ToolbarAutoHidePolicyTests.MustStayVisible_KeyboardFocusInsideToolbar_IsTrue
#[test]
fn toolbar_must_stay_visible_keyboard_focus_inside_toolbar_is_true() {
    assert!(ToolbarAutoHidePolicy::must_stay_visible(true, true, false, true));
}

// C#: ToolbarAutoHidePolicyTests.MustStayVisible_AutoHideEnabledFolderOpenPopupClosedNoFocus_IsFalse
#[test]
fn toolbar_must_stay_visible_auto_hide_enabled_folder_open_popup_closed_no_focus_is_false() {
    assert!(!ToolbarAutoHidePolicy::must_stay_visible(true, true, false, false));
}

// C#: ToolbarAutoHidePolicyTests.IsShown_HiddenState_OnlyTheHotZoneBringsItBack
// Q-R34: no key-press, image-change or mouse-elsewhere input can show a hidden toolbar.
#[test]
fn is_shown_hidden_state_only_the_hot_zone_brings_it_back() {
    assert!(!ToolbarAutoHidePolicy::is_shown(true, true, false, false, false));
    assert!(ToolbarAutoHidePolicy::is_shown(true, true, false, false, true));
}

// C#: ToolbarAutoHidePolicyTests.IsShown_MustStayVisibleRules_ShowItWithoutTheMouse (4 InlineData rows)
#[test]
fn is_shown_must_stay_visible_rules_show_it_without_the_mouse() {
    let rows = [
        (false, true, false, false), // auto-hide off
        (true, false, false, false), // no folder
        (true, true, true, false),   // Tools popup open
        (true, true, false, true),   // keyboard focus in the toolbar
    ];
    for (enabled, folder, popup, focus) in rows {
        assert!(
            ToolbarAutoHidePolicy::is_shown(enabled, folder, popup, focus, false),
            "enabled={enabled} folder={folder} popup={popup} focus={focus}"
        );
    }
}

// C#: ToolbarAutoHidePolicyTests.TargetOpacity_IsTheClampedPercentAsFraction (7 InlineData rows)
#[test]
fn target_opacity_is_the_clamped_percent_as_fraction() {
    let rows = [
        (100, 1.0),
        (60, 0.6),
        (20, 0.2),
        (5, 0.2),
        (0, 0.2),
        (-30, 0.2),
        (500, 1.0),
    ];
    for (percent, expected) in rows {
        let actual = ToolbarAutoHidePolicy::target_opacity(percent);
        assert!(
            (actual - expected).abs() < 1e-6,
            "percent {percent}: {actual} != {expected}"
        );
    }
}

// C#: ToolbarAutoHidePolicyTests.DelayMs_UsesOnlyTheToolbarDelay_IndependentOfTheInfoDelay
#[test]
fn delay_ms_uses_only_the_toolbar_delay_independent_of_the_info_delay() {
    let mut settings = AppSettings {
        toolbar_auto_hide_delay_ms: 700,
        info_overlay_auto_hide_delay_ms: 9000,
        ..AppSettings::default()
    };
    assert_eq!(ToolbarAutoHidePolicy::delay_ms(&settings), 700);

    settings.info_overlay_auto_hide_delay_ms = 1;
    assert_eq!(ToolbarAutoHidePolicy::delay_ms(&settings), 700);

    settings.toolbar_auto_hide_delay_ms = 2500;
    assert_eq!(ToolbarAutoHidePolicy::delay_ms(&settings), 2500);
}

// C#: ToolbarAutoHidePolicyTests.IsInsideHotZone_MarginInflatesBoundsOnEverySide (8 InlineData rows)
// The toolbar occupies (0,0)-(100,30); a 10 px margin inflates it to (-10,-10)-(110,40).
#[test]
fn is_inside_hot_zone_margin_inflates_bounds_on_every_side() {
    let rows = [
        (0.0, 0.0, true),     // top-left corner of the toolbar itself
        (50.0, 20.0, true),   // inside the toolbar
        (-10.0, -10.0, true), // inside the margin, just outside the toolbar's own bounds
        (110.0, 40.0, true),  // inside the margin, past the toolbar's right/bottom edge
        (-11.0, 0.0, false),  // just past the margin, left
        (0.0, -11.0, false),  // just past the margin, top
        (111.0, 20.0, false), // just past the margin, right
        (50.0, 41.0, false),  // just past the margin, bottom
    ];
    for (x, y, expected) in rows {
        let actual = ToolbarAutoHidePolicy::is_inside_hot_zone(x, y, 0.0, 0.0, 100.0, 30.0, 10.0);
        assert_eq!(actual, expected, "mouse ({x}, {y})");
    }
}

// New test: a NaN operand makes every comparison false (contract trap T4), on each axis and in the bounds. A zero-size
// box at the origin with zero margin is a single inside point.
#[test]
fn new_test_is_inside_hot_zone_nan_operand_is_never_inside() {
    assert!(!ToolbarAutoHidePolicy::is_inside_hot_zone(
        f64::NAN,
        0.0,
        0.0,
        0.0,
        100.0,
        30.0,
        10.0
    ));
    assert!(!ToolbarAutoHidePolicy::is_inside_hot_zone(
        0.0,
        f64::NAN,
        0.0,
        0.0,
        100.0,
        30.0,
        10.0
    ));
    assert!(!ToolbarAutoHidePolicy::is_inside_hot_zone(
        0.0,
        0.0,
        f64::NAN,
        0.0,
        100.0,
        30.0,
        10.0
    ));
    assert!(ToolbarAutoHidePolicy::is_inside_hot_zone(
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
    ));
    assert!(!ToolbarAutoHidePolicy::is_inside_hot_zone(
        0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0
    ));
}
