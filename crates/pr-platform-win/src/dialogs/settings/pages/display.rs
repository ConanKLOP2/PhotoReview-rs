//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/DisplaySettingsPage.cs` (247 lines, baseline `d92d6aae`): the "Display" page (WPF `DisplayPage`):
//! info overlay, toolbar (auto hide, opacity), auto-hiding info, loading/scaling, EXIF line fields, title bar fields. Unit W8-19e. Contract: `contracts/W8-19e.md`.
//!
//! Properties owned: `ShowInfoOverlay`, `ShowFileInfo`, `ShowFolderInfo`, `InfoOverlayFontSize`, `ShowZoomIndicator`, `ToolbarAutoHide(+DelayMs)`,
//! `InfoOverlayAutoHide(+DelayMs)`, `ToolbarOpacityPercent`, `LoadingMode`, `ScalingQuality`, `ShowExifInfo`, `ExifInfoFields`, `TitleBarFields`.
use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_core::settings::app_settings::{MAX_TOOLBAR_OPACITY_PERCENT, MIN_TOOLBAR_OPACITY_PERCENT};
use pr_l10n::tr;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::{SettingsPageContext, with_field_context};
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView, text_fn};
use crate::win32::dialog_native::BN_CLICKED;

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
        Self {
            context: PageContextSlot::new(),
            ids: DisplayIds {
                show_info_overlay: 0,
                show_file_info: 0,
                show_folder_info: 0,
                font_size: 0,
                show_zoom_indicator: 0,
                toolbar_auto_hide: 0,
                toolbar_delay: 0,
                opacity: 0,
                opacity_value: 0,
                info_auto_hide: 0,
                info_delay: 0,
                loading_mode: 0,
                scaling_quality: 0,
                show_exif: 0,
            },
            exif_checks: Vec::new(),
            title_checks: Vec::new(),
        }
    }

    /// C# `Ids`.
    pub fn ids(&self) -> DisplayIds {
        self.ids
    }

    /// C# `ExifChecks`: 10 ids after `declare`, in [`EXIF_ORDER`] order.
    pub fn exif_checks(&self) -> &[i32] {
        &self.exif_checks
    }

    /// C# `TitleChecks`: 14 ids after `declare`, in [`TITLE_ORDER`] order.
    pub fn title_checks(&self) -> &[i32] {
        &self.title_checks
    }
}

impl SettingsPageView for DisplaySettingsPage {
    /// `"Display"`.
    fn key(&self) -> &str {
        "Display"
    }

    /// Six groups, exactly the C# order and texts (see the contract table).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        let mut ids = self.ids;
        let mut exif_checks = Vec::new();
        let mut title_checks = Vec::new();

        page.group(text_fn(tr::settings_nav_display), |group| {
            ids.show_info_overlay = group.check(text_fn(tr::settings_show_info_overlay_label), 0);
            ids.show_file_info = group.check(text_fn(tr::settings_show_file_info_label), 12);
            ids.show_folder_info = group.check(text_fn(tr::settings_show_folder_info_label), 12);
            ids.font_size = group
                .edit(text_fn(tr::settings_info_overlay_font_size_label), 40)
                .control;
            ids.show_zoom_indicator = group.check(text_fn(tr::settings_show_zoom_indicator_label), 0);
        });
        page.group(text_fn(tr::settings_group_toolbar), |group| {
            ids.toolbar_auto_hide = group.check(text_fn(tr::settings_toolbar_auto_hide_label), 0);
            ids.toolbar_delay = group
                .edit(text_fn(tr::settings_toolbar_auto_hide_delay_label), 45)
                .control;
            group.note(Some(text_fn(tr::settings_toolbar_auto_hide_hint)), true);
            let slider = group.slider(
                text_fn(tr::settings_toolbar_opacity_label),
                MIN_TOOLBAR_OPACITY_PERCENT,
                MAX_TOOLBAR_OPACITY_PERCENT,
            );
            ids.opacity = slider.slider;
            ids.opacity_value = slider.value;
            group.note(
                Some(text_fn(|| with_field_context(SettingsPresenter::toolbar_opacity_hint))),
                true,
            );
        });
        page.group(text_fn(tr::settings_group_info_overlay), |group| {
            ids.info_auto_hide = group.check(text_fn(tr::settings_info_overlay_auto_hide_label), 0);
            ids.info_delay = group
                .edit(text_fn(tr::settings_info_overlay_auto_hide_delay_label), 45)
                .control;
            group.note(Some(text_fn(tr::settings_info_overlay_auto_hide_hint)), true);
        });
        page.group(text_fn(tr::settings_group_display_performance), |group| {
            ids.loading_mode = group
                .combo(
                    text_fn(tr::settings_loading_mode_label),
                    Some(vec![
                        text_fn(tr::enum_loading_mode_fast),
                        text_fn(tr::enum_loading_mode_preview),
                        text_fn(tr::settings_loading_mode_original_with_hint),
                    ]),
                    0,
                )
                .control;
            ids.scaling_quality = group
                .combo(
                    text_fn(tr::settings_scaling_quality_label),
                    Some(vec![
                        text_fn(tr::enum_scaling_quality_high_quality),
                        text_fn(tr::enum_scaling_quality_linear),
                    ]),
                    0,
                )
                .control;
        });
        page.group(text_fn(tr::settings_group_exif), |group| {
            ids.show_exif = group.check(text_fn(tr::settings_show_exif_info_label), 0);
            group.note(Some(text_fn(tr::settings_group_exif_hint)), false);
            exif_checks = group.check_flow(
                [
                    tr::settings_exif_field_file_name,
                    tr::settings_exif_field_date_taken,
                    tr::settings_exif_field_modified_date,
                    tr::settings_exif_field_dimensions,
                    tr::settings_exif_field_camera,
                    tr::settings_exif_field_lens,
                    tr::settings_exif_field_iso,
                    tr::settings_exif_field_focal_length,
                    tr::settings_exif_field_aperture,
                    tr::settings_exif_field_shutter_speed,
                ]
                .into_iter()
                .map(text_fn)
                .collect::<Vec<_>>()
                .as_slice(),
                12,
            );
        });
        page.group(text_fn(tr::settings_group_title_bar), |group| {
            group.note(Some(text_fn(tr::settings_group_title_bar_hint)), false);
            title_checks = group.check_flow(
                [
                    tr::settings_title_bar_field_folder_name,
                    tr::settings_title_bar_field_folder_path,
                    tr::settings_title_bar_field_index_count,
                    tr::settings_exif_field_file_name,
                    tr::settings_title_bar_field_file_size,
                    tr::settings_exif_field_dimensions,
                    tr::settings_exif_field_modified_date,
                    tr::settings_exif_field_date_taken,
                    tr::settings_exif_field_camera,
                    tr::settings_exif_field_lens,
                    tr::settings_exif_field_iso,
                    tr::settings_exif_field_focal_length,
                    tr::settings_exif_field_aperture,
                    tr::settings_exif_field_shutter_speed,
                ]
                .into_iter()
                .map(text_fn)
                .collect::<Vec<_>>()
                .as_slice(),
                0,
            );
        });

        self.ids = ids;
        self.exif_checks = exif_checks;
        self.title_checks = title_checks;
    }

    fn attach(&mut self, context: SettingsPageContext) {
        self.context.attach(context);
    }

    /// The C# statement order, then `update_dependents()` and `update_opacity_value()`.
    fn load(&mut self, state: &SettingsFormState) {
        let context = self.context.context();
        context.set_checked(self.ids.show_info_overlay, state.show_info_overlay);
        context.set_checked(self.ids.show_file_info, state.show_file_info);
        context.set_checked(self.ids.show_folder_info, state.show_folder_info);
        context.set_text(self.ids.font_size, &state.info_overlay_font_size_text);
        context.set_checked(self.ids.show_zoom_indicator, state.show_zoom_indicator);
        context.set_checked(self.ids.toolbar_auto_hide, state.toolbar_auto_hide);
        context.set_text(self.ids.toolbar_delay, &state.toolbar_auto_hide_delay_text);
        context.set_checked(self.ids.info_auto_hide, state.info_overlay_auto_hide);
        context.set_text(self.ids.info_delay, &state.info_overlay_auto_hide_delay_text);
        context.set_slider_value(self.ids.opacity, state.toolbar_opacity_value.round_ties_even() as i32);
        context.set_combo_index(self.ids.loading_mode, state.loading_mode_index);
        context.set_combo_index(self.ids.scaling_quality, state.scaling_quality_index);
        context.set_checked(self.ids.show_exif, state.show_exif_info);
        for (id, flag) in self.exif_checks.iter().zip(EXIF_ORDER) {
            context.set_checked(*id, state.exif_info_fields.contains(flag));
        }
        for (id, flag) in self.title_checks.iter().zip(TITLE_ORDER) {
            context.set_checked(*id, state.title_bar_fields.contains(flag));
        }
        self.update_dependents();
        self.update_opacity_value();
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        let context = self.context.context();
        state.show_info_overlay = context.is_checked(self.ids.show_info_overlay);
        state.show_file_info = context.is_checked(self.ids.show_file_info);
        state.show_folder_info = context.is_checked(self.ids.show_folder_info);
        state.info_overlay_font_size_text = context.text(self.ids.font_size);
        state.show_zoom_indicator = context.is_checked(self.ids.show_zoom_indicator);
        state.toolbar_auto_hide = context.is_checked(self.ids.toolbar_auto_hide);
        state.toolbar_auto_hide_delay_text = context.text(self.ids.toolbar_delay);
        state.info_overlay_auto_hide = context.is_checked(self.ids.info_auto_hide);
        state.info_overlay_auto_hide_delay_text = context.text(self.ids.info_delay);
        state.toolbar_opacity_value = f64::from(context.slider_value(self.ids.opacity));
        state.loading_mode_index = context.combo_index(self.ids.loading_mode);
        state.scaling_quality_index = context.combo_index(self.ids.scaling_quality);
        state.show_exif_info = context.is_checked(self.ids.show_exif);
        let mut exif = ExifInfoFields::NONE;
        for (id, flag) in self.exif_checks.iter().zip(EXIF_ORDER) {
            if context.is_checked(*id) {
                exif = exif.union(flag);
            }
        }
        state.exif_info_fields = exif;
        let mut title = TitleBarFields::NONE;
        for (id, flag) in self.title_checks.iter().zip(TITLE_ORDER) {
            if context.is_checked(*id) {
                title = title.union(flag);
            }
        }
        state.title_bar_fields = title;
    }

    /// `BN_CLICKED` of `show_info_overlay`, `toolbar_auto_hide`, `info_auto_hide`, `show_exif` -> `update_dependents()`, handled; else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        if notification != BN_CLICKED {
            return false;
        }
        if id == self.ids.show_info_overlay
            || id == self.ids.toolbar_auto_hide
            || id == self.ids.info_auto_hide
            || id == self.ids.show_exif
        {
            self.update_dependents();
            true
        } else {
            false
        }
    }

    /// `opacity` -> `update_opacity_value()`.
    fn on_scroll(&mut self, id: i32) {
        if id == self.ids.opacity {
            self.update_opacity_value();
        }
    }

    fn on_language_changed(&mut self) {
        self.update_opacity_value();
    }

    /// `InfoOverlayAutoHideDelay` -> `info_delay`, else `None`.
    fn focus_target(&self, field: SettingsFocusField) -> Option<i32> {
        match field {
            SettingsFocusField::InfoOverlayAutoHideDelay => Some(self.ids.info_delay),
            _ => None,
        }
    }
}

impl DisplaySettingsPage {
    fn update_opacity_value(&self) {
        let context = self.context.context();
        let value = with_field_context(|field_context| {
            SettingsPresenter::toolbar_opacity_value_text(
                f64::from(context.slider_value(self.ids.opacity)),
                field_context,
            )
        });
        context.set_text(self.ids.opacity_value, &value);
    }

    fn update_dependents(&self) {
        let context = self.context.context();
        let overlay = context.is_checked(self.ids.show_info_overlay);
        context.set_enabled(self.ids.show_file_info, overlay);
        context.set_enabled(self.ids.show_folder_info, overlay);
        context.set_enabled(self.ids.toolbar_delay, context.is_checked(self.ids.toolbar_auto_hide));
        context.set_enabled(self.ids.info_delay, context.is_checked(self.ids.info_auto_hide));
        let exif = context.is_checked(self.ids.show_exif);
        for id in &self.exif_checks {
            context.set_enabled(*id, exif);
        }
    }
}
