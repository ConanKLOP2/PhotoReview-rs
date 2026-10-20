//! Port of `PhotoReview.App.Shared/Coordinators/InfoOverlayAutoHidePolicy.cs` (33 lines, Q-R34) and
//! `PhotoReview.App.Shared/Coordinators/ToolbarAutoHidePolicy.cs` (38 lines, feat/ui-dark-chrome-toolbar).
//!
//! Pure decision functions: no window, no clock, no WPF. The shell evaluates them on every tick and animates
//! one container's opacity / hit-test flag from the result.

use pr_core::settings::app_settings::{
    AppSettings, MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, MAX_TOOLBAR_AUTO_HIDE_DELAY_MS, MAX_TOOLBAR_OPACITY_PERCENT,
    MIN_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, MIN_TOOLBAR_AUTO_HIDE_DELAY_MS, MIN_TOOLBAR_OPACITY_PERCENT,
};

/// C# `InfoOverlayAutoHidePolicy.Outcome` (`readonly record struct Outcome(double Opacity, bool IsHitTestVisible)`).
/// Opacity is always exactly `0.0` or `1.0`, so the derived `PartialEq` is safe (no NaN reaches it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InfoOverlayOutcome {
    /// `0.0` (hidden) or `1.0` (visible).
    pub opacity: f64,
    /// `false` when hidden: a faded overlay is click-through.
    pub is_hit_test_visible: bool,
}

/// C# `public static class InfoOverlayAutoHidePolicy`. Unit struct, associated functions only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InfoOverlayAutoHidePolicy;

impl InfoOverlayAutoHidePolicy {
    /// C# `MustStayVisible`: `!autoHideEnabled || !hasFolderOpen || statusNeedsAttention || isCompareOpen || !isWindowActive`.
    pub fn must_stay_visible(
        auto_hide_enabled: bool,
        has_folder_open: bool,
        status_needs_attention: bool,
        is_compare_open: bool,
        is_window_active: bool,
    ) -> bool {
        !auto_hide_enabled || !has_folder_open || status_needs_attention || is_compare_open || !is_window_active
    }

    /// C# `DelayMs(AppSettings)`: `settings.InfoOverlayAutoHideDelayMs` clamped to
    /// `[MIN_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS]`. Never reads the toolbar delay.
    pub fn delay_ms(settings: &AppSettings) -> i32 {
        settings
            .info_overlay_auto_hide_delay_ms
            .clamp(MIN_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS)
    }

    /// C# `Evaluate`: `hidden = !MustStayVisible(...) && idleElapsed`; opacity `0.0` / `1.0`, hit-test `!hidden`.
    pub fn evaluate(
        auto_hide_enabled: bool,
        has_folder_open: bool,
        status_needs_attention: bool,
        is_compare_open: bool,
        is_window_active: bool,
        idle_elapsed: bool,
    ) -> InfoOverlayOutcome {
        let must_stay = Self::must_stay_visible(
            auto_hide_enabled,
            has_folder_open,
            status_needs_attention,
            is_compare_open,
            is_window_active,
        );
        let hidden = !must_stay && idle_elapsed;
        InfoOverlayOutcome {
            opacity: if hidden { 0.0 } else { 1.0 },
            is_hit_test_visible: !hidden,
        }
    }
}

/// C# `public static class ToolbarAutoHidePolicy`. Unit struct, associated functions only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarAutoHidePolicy;

impl ToolbarAutoHidePolicy {
    /// C# `MustStayVisible`: `!autoHideEnabled || !hasFolderOpen || isToolsPopupOpen || isKeyboardFocusInsideToolbar`.
    pub fn must_stay_visible(
        auto_hide_enabled: bool,
        has_folder_open: bool,
        is_tools_popup_open: bool,
        is_keyboard_focus_inside_toolbar: bool,
    ) -> bool {
        !auto_hide_enabled || !has_folder_open || is_tools_popup_open || is_keyboard_focus_inside_toolbar
    }

    /// C# `IsShown`: `MustStayVisible(...) || isMouseInsideHotZone`. No other input can show a hidden toolbar.
    pub fn is_shown(
        auto_hide_enabled: bool,
        has_folder_open: bool,
        is_tools_popup_open: bool,
        is_keyboard_focus_inside_toolbar: bool,
        is_mouse_inside_hot_zone: bool,
    ) -> bool {
        Self::must_stay_visible(
            auto_hide_enabled,
            has_folder_open,
            is_tools_popup_open,
            is_keyboard_focus_inside_toolbar,
        ) || is_mouse_inside_hot_zone
    }

    /// C# `DelayMs(AppSettings)`: `settings.ToolbarAutoHideDelayMs` clamped to
    /// `[MIN_TOOLBAR_AUTO_HIDE_DELAY_MS, MAX_TOOLBAR_AUTO_HIDE_DELAY_MS]`. Never reads the info-overlay delay.
    pub fn delay_ms(settings: &AppSettings) -> i32 {
        settings
            .toolbar_auto_hide_delay_ms
            .clamp(MIN_TOOLBAR_AUTO_HIDE_DELAY_MS, MAX_TOOLBAR_AUTO_HIDE_DELAY_MS)
    }

    /// C# `TargetOpacity(int percent)`: `Math.Clamp(percent, 20, 100) / 100.0`. The integer is clamped first, then
    /// divided in `f64` (contract trap T3). `60` gives `0.6`, `5` gives `0.2`.
    pub fn target_opacity(percent: i32) -> f64 {
        let clamped = percent.clamp(MIN_TOOLBAR_OPACITY_PERCENT, MAX_TOOLBAR_OPACITY_PERCENT);
        f64::from(clamped) / 100.0
    }

    /// C# `IsInsideHotZone`: the toolbar rectangle inflated by `margin` on every side, closed box.
    /// Evaluation order must match C# exactly: `left - margin`, `left + width + margin` (left + width first).
    /// Any NaN operand makes every comparison false, so the result is `false`.
    pub fn is_inside_hot_zone(
        mouse_x: f64,
        mouse_y: f64,
        toolbar_left: f64,
        toolbar_top: f64,
        toolbar_width: f64,
        toolbar_height: f64,
        margin: f64,
    ) -> bool {
        mouse_x >= toolbar_left - margin
            && mouse_x <= toolbar_left + toolbar_width + margin
            && mouse_y >= toolbar_top - margin
            && mouse_y <= toolbar_top + toolbar_height + margin
    }
}
