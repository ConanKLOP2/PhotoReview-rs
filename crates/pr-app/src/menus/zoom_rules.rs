//! Port of `PhotoReview.App.Shared/Menus/ZoomMenuRules.cs` (15 lines).
//! `calculate_click_level_from_effective_zoom` is the same rule as C# `MainWindowHelpers.CalculateClickLevelFromEffectiveZoom`
//! (the WPF App copy, retired with WPF): one Rust function serves the context menu (W8-02a, W8-14e) and the click-level setter.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::settings::app_settings::{MAX_CLICK_ZOOM_PERCENT, MIN_CLICK_ZOOM_PERCENT};

/// C# `ZoomMenuRules.PresetPercents`: the presets of the Zoom submenu, in menu order (50, 70, 100, 150, 200, 300, 400 %).
pub const PRESET_PERCENTS: &[i32] = &[50, 70, 100, 150, 200, 300, 400];

/// C# `ZoomMenuRules.CalculateClickLevelFromEffectiveZoom(double?)`.
///
/// `effective_zoom` is the viewer's zoom factor (1.0 = 100 %), or `None` when there is no zoom (Fit without a value).
/// Returns `None` for `None` and for non-finite input (NaN, +inf, -inf). Otherwise returns
/// `clamp(round_half_away_from_zero(effective_zoom * 100.0), MIN_CLICK_ZOOM_PERCENT, MAX_CLICK_ZOOM_PERCENT)`.
pub fn calculate_click_level_from_effective_zoom(effective_zoom: Option<f64>) -> Option<i32> {
    todo!()
}
