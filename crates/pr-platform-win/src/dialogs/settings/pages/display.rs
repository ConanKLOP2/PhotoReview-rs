//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/DisplaySettingsPage.cs` (247 lines, baseline `d92d6aae`): the "Display" page (WPF `DisplayPage`):
//! info overlay, toolbar (auto hide, opacity), auto-hiding info, loading/scaling, EXIF line fields, title bar fields. Unit W8-19e. Contract: `contracts/W8-19e.md`.
//!
//! Properties owned: `ShowInfoOverlay`, `ShowFileInfo`, `ShowFolderInfo`, `InfoOverlayFontSize`, `ShowZoomIndicator`, `ToolbarAutoHide(+DelayMs)`,
//! `InfoOverlayAutoHide(+DelayMs)`, `ToolbarOpacityPercent`, `LoadingMode`, `ScalingQuality`, `ShowExifInfo`, `ExifInfoFields`, `TitleBarFields`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_core::model::fields::{ExifInfoFields, TitleBarFields};

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView};

/// C# `ExifOrder` (private static): the flag of each EXIF check box, in declaration order. Real code (data).
pub const EXIF_ORDER: [ExifInfoFields; 10] = [
    ExifInfoFields::FILE_NAME,
    ExifInfoFields::DATE_TAKEN,
    ExifInfoFields::MODIFIED_DATE,
    ExifInfoFields::DIMENSIONS,
    ExifInfoFields::CAMERA,
    ExifInfoFields::LENS,
    ExifInfoFields::ISO,
    ExifInfoFields::FOCAL_LENGTH,
    ExifInfoFields::APERTURE,
    ExifInfoFields::SHUTTER_SPEED,
];

/// C# `TitleOrder` (private static): the flag of each title bar check box, in declaration order. Real code (data).
pub const TITLE_ORDER: [TitleBarFields; 14] = [
    TitleBarFields::FOLDER_NAME,
    TitleBarFields::FOLDER_PATH,
    TitleBarFields::INDEX_COUNT,
    TitleBarFields::FILE_NAME,
    TitleBarFields::FILE_SIZE,
    TitleBarFields::DIMENSIONS,
    TitleBarFields::MODIFIED_DATE,
    TitleBarFields::DATE_TAKEN,
    TitleBarFields::CAMERA,
    TitleBarFields::LENS,
    TitleBarFields::ISO,
    TitleBarFields::FOCAL_LENGTH,
    TitleBarFields::APERTURE,
    TitleBarFields::SHUTTER_SPEED,
];

/// C# `Ids` (internal, for tests): the 14 single control ids (the two check lists are [`DisplaySettingsPage::exif_checks`] / [`DisplaySettingsPage::title_checks`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayIds {
    pub show_info_overlay: i32,
    pub show_file_info: i32,
    pub show_folder_info: i32,
    pub font_size: i32,
    pub show_zoom_indicator: i32,
    pub toolbar_auto_hide: i32,
    pub toolbar_delay: i32,
    pub opacity: i32,
    pub opacity_value: i32,
    pub info_auto_hide: i32,
    pub info_delay: i32,
    pub loading_mode: i32,
    pub scaling_quality: i32,
    pub show_exif: i32,
}

/// C# `DisplaySettingsPage`.
#[derive(Debug)]
pub struct DisplaySettingsPage {
    context: PageContextSlot,
    ids: DisplayIds,
    exif_checks: Vec<i32>,
    title_checks: Vec<i32>,
}

impl Default for DisplaySettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplaySettingsPage {
    /// All ids 0, empty check lists, detached context.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Ids`.
    pub fn ids(&self) -> DisplayIds {
        todo!()
    }

    /// C# `ExifChecks`: 10 ids after `declare`, in [`EXIF_ORDER`] order.
    pub fn exif_checks(&self) -> &[i32] {
        todo!()
    }

    /// C# `TitleChecks`: 14 ids after `declare`, in [`TITLE_ORDER`] order.
    pub fn title_checks(&self) -> &[i32] {
        todo!()
    }
}

impl SettingsPageView for DisplaySettingsPage {
    /// `"Display"`.
    fn key(&self) -> &str {
        todo!()
    }

    /// Six groups, exactly the C# order and texts (see the contract table).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        todo!()
    }

    fn attach(&mut self, context: SettingsPageContext) {
        todo!()
    }

    /// The C# statement order, then `update_dependents()` and `update_opacity_value()`.
    fn load(&mut self, state: &SettingsFormState) {
        todo!()
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        todo!()
    }

    /// `BN_CLICKED` of `show_info_overlay`, `toolbar_auto_hide`, `info_auto_hide`, `show_exif` -> `update_dependents()`, handled; else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        todo!()
    }

    /// `opacity` -> `update_opacity_value()`.
    fn on_scroll(&mut self, id: i32) {
        todo!()
    }

    fn on_language_changed(&mut self) {
        todo!()
    }

    /// `InfoOverlayAutoHideDelay` -> `info_delay`, else `None`.
    fn focus_target(&self, field: SettingsFocusField) -> Option<i32> {
        todo!()
    }
}
