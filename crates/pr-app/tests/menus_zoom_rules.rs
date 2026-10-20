//! Red tests (W8-01d, test porter) for `pr_app::menus::zoom_rules` (C# `Menus/ZoomMenuRules.cs`, baseline `d92d6aae`).
//!
//! Ported from `tests/PhotoReview.Shell.Tests/Menus/ContextMenuModelBuilderTests.cs` (the theory and the NaN case) and
//! `tests/PhotoReview.App.Tests/ViewModels/ViewerStateTests.cs` (the four click-level tests, which reach the same rule through
//! `MainWindowHelpers`). The rule is ported once. Tests marked "new" have no C# twin (contract "Test plan").
//!
//! Every test fails through `todo!()` on the stub. The zoom rule needs only `pr-core`, so these tests turn green as soon as
//! the port lands (no W8-04e or W8-01a dependency).

use pr_app::menus::zoom_rules::{PRESET_PERCENTS, calculate_click_level_from_effective_zoom as click_level};
use pr_core::settings::app_settings::{MAX_CLICK_ZOOM_PERCENT, MIN_CLICK_ZOOM_PERCENT};

// C#: ContextMenuModelBuilderTests.CalculateClickLevelFromEffectiveZoom_RoundsAndClamps
// Theory rows: (null -> null), (1.5 -> 150), (0.004 -> MIN), (100.0 -> MAX), (0.125 -> 13).
#[test]
fn click_level_rounds_and_clamps() {
    assert_eq!(click_level(None), None);
    assert_eq!(click_level(Some(1.5)), Some(150));
    assert_eq!(click_level(Some(0.004)), Some(MIN_CLICK_ZOOM_PERCENT));
    assert_eq!(click_level(Some(100.0)), Some(MAX_CLICK_ZOOM_PERCENT));
    assert_eq!(click_level(Some(0.125)), Some(13), "12,5 % rounds away from zero");
}

// C#: ContextMenuModelBuilderTests.Build_NonFiniteEffectiveZoom_IsTreatedAsFit (the rule part: NaN -> None)
#[test]
fn non_finite_effective_zoom_is_treated_as_fit() {
    assert_eq!(click_level(Some(f64::NAN)), None);
}

// C#: ViewerStateTests.ClickLevelFromEffectiveZoom_Null_ReturnsNull_Fit
#[test]
fn click_level_null_returns_null_fit() {
    assert_eq!(click_level(None), None);
}

// C#: ViewerStateTests.ClickLevelFromEffectiveZoom_RoundsToNearestPercent
// Rows: 1.0 -> 100, 1.499 -> 150, 1.501 -> 150, 2.505 -> 251 (AwayFromZero on the .5 boundary, not banker's rounding).
#[test]
fn click_level_rounds_to_nearest_percent() {
    assert_eq!(click_level(Some(1.0)), Some(100));
    assert_eq!(click_level(Some(1.499)), Some(150));
    assert_eq!(click_level(Some(1.501)), Some(150));
    assert_eq!(click_level(Some(2.505)), Some(251));
}

// C#: ViewerStateTests.ClickLevelFromEffectiveZoom_BelowMinimum_ClampsToMinimum
#[test]
fn click_level_below_minimum_clamps_to_minimum() {
    assert_eq!(click_level(Some(0.01)), Some(MIN_CLICK_ZOOM_PERCENT));
}

// C#: ViewerStateTests.ClickLevelFromEffectiveZoom_AboveMaximum_ClampsToMaximum
#[test]
fn click_level_above_maximum_clamps_to_maximum() {
    assert_eq!(click_level(Some(50.0)), Some(MAX_CLICK_ZOOM_PERCENT));
}

// new: +inf and -inf are non-finite too: they are treated as Fit (None), like NaN.
#[test]
fn non_finite_infinities_are_treated_as_fit() {
    assert_eq!(click_level(Some(f64::INFINITY)), None);
    assert_eq!(click_level(Some(f64::NEG_INFINITY)), None);
}

// new: the Zoom submenu presets, in menu order.
#[test]
fn preset_percents_are_the_zoom_submenu_in_order() {
    assert_eq!(PRESET_PERCENTS, &[50, 70, 100, 150, 200, 300, 400]);
}

// new: a finite zoom whose percent overflows i32 (f64::MAX * 100 is +inf) saturates to MAX; a negative zoom clamps to MIN.
#[test]
fn huge_and_negative_zoom_saturate_to_the_clamp_bounds() {
    assert_eq!(click_level(Some(f64::MAX)), Some(MAX_CLICK_ZOOM_PERCENT));
    assert_eq!(click_level(Some(-5.0)), Some(MIN_CLICK_ZOOM_PERCENT));
}

// new: .5 boundaries round away from zero (0.125 -> 12.5 -> 13, 2.505 -> 250.5 -> 251), never to even.
#[test]
fn half_percent_rounds_away_from_zero() {
    assert_eq!(click_level(Some(0.125)), Some(13));
    assert_eq!(click_level(Some(2.505)), Some(251));
}
