//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/MouseSettingsPage.cs` (268 lines, baseline `d92d6aae`): the "Mouse" page (WPF `MousePage`): the Zoom
//! group, the mouse and navigation group, and the right-click menu group (one tick box per `ContextMenuItemId`, built in code). Unit W8-19h. Contract:
//! `contracts/W8-19h.md`.
//!
//! Properties owned: `ClickZoomPercent`, `ClickToZoomEnabled`, `ArrowPanStepPercent`, `KeyboardZoomAnchor`, `FitWidthAnchor`, `FitWidthAnchor2`, `KeepZoomAcrossImages`,
//! `KeyboardZoomStepPercent`, `MiddleClickAction`, `ClickZoomKeyTogglesFit`, `MouseWheelAction`, `KineticPanEnabled`, `KineticGlideSmoothing`,
//! `ArrowKeyNavigatesAtZoomEdge`, `ImageTransition(+Ms)`, `TouchpadSwipeEnabled(+DistancePerImage)`, `HiddenContextMenuItems`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_core::settings::context_menu::ContextMenuItemId;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView};

/// C# `Ids` (internal, for tests): the 19 single control ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseIds {
    pub click_zoom_percent: i32,
    pub click_to_zoom: i32,
    pub arrow_pan_step: i32,
    pub keyboard_zoom_anchor: i32,
    pub zoom_shortcuts_summary: i32,
    pub fit_width_anchor: i32,
    pub keep_zoom: i32,
    pub keyboard_zoom_step: i32,
    pub fit_width_anchor2: i32,
    pub middle_click: i32,
    pub click_zoom_key_toggles_fit: i32,
    pub mouse_wheel: i32,
    pub kinetic_pan: i32,
    pub kinetic_smoothing: i32,
    pub arrow_key_at_edge: i32,
    pub image_transition: i32,
    pub image_transition_ms: i32,
    pub touchpad_swipe: i32,
    pub touchpad_distance: i32,
}

/// C# `MouseSettingsPage`.
#[derive(Debug)]
pub struct MouseSettingsPage {
    context: PageContextSlot,
    ids: MouseIds,
    /// C# `_menuChecks` (a `Dictionary<ContextMenuItemId, int>` in insertion order): `(item, check id)` in `ITEMS` order.
    menu_checks: Vec<(ContextMenuItemId, i32)>,
}

impl Default for MouseSettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseSettingsPage {
    /// All ids 0, no menu checks, detached context.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Ids`.
    pub fn ids(&self) -> MouseIds {
        todo!()
    }

    /// C# `MenuChecks`: one entry per `pr_core::settings::context_menu::ITEMS` row after `declare`, in table order.
    pub fn menu_checks(&self) -> &[(ContextMenuItemId, i32)] {
        todo!()
    }
}

impl SettingsPageView for MouseSettingsPage {
    /// `"Mouse"`.
    fn key(&self) -> &str {
        todo!()
    }

    /// Three groups, exactly the C# order and texts (see the contract table).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        todo!()
    }

    /// Stores the context, then a tooltip (`tr::settings_context_menu_settings_locked_tooltip`) on the check of every LOCKED row.
    fn attach(&mut self, context: SettingsPageContext) {
        todo!()
    }

    fn load(&mut self, state: &SettingsFormState) {
        todo!()
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        todo!()
    }

    /// Image transition `CBN_SELCHANGE` -> `update_dependents()`; `BN_CLICKED` of the `Settings` menu check -> re-tick it; else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        todo!()
    }

    /// `set_summary(&context.current_shortcut_texts())`.
    fn on_activated(&mut self) {
        todo!()
    }

    /// `set_summary(&context.current_shortcut_texts())`.
    fn on_language_changed(&mut self) {
        todo!()
    }

    /// `ArrowPanStep`, `KeyboardZoomStep`, `ImageTransitionMs`, `TouchpadSwipeDistance` -> their boxes, else `None`.
    fn focus_target(&self, field: SettingsFocusField) -> Option<i32> {
        todo!()
    }
}
