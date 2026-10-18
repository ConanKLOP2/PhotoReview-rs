//! Port of `PhotoReview.App.Shared/Presenters/SettingsFormState.cs` (C# baseline `d92d6aae`, 128 lines).
//!
//! WP-35 G0: the value of every control of the Settings dialog as plain data (no control type). A view copies its
//! controls into [`SettingsFormState`] before Save and out of it after `SettingsPresenter::load_fields`. All mapping to
//! and from `AppSettings` lives in `presenters::settings_field_map` and `presenters::settings`, so a second view (the
//! native Win32 dialog) reuses this type unchanged.
//!
//! Combo boxes are carried as the zero-based index of the item in the fixed item order the dialog has always had
//! (`-1` is a legal value where C# produces `-1`, for example `LanguageOptions::IndexOf`). Text boxes are carried as
//! the typed text, because Save validates what the user typed, not a parsed number.
//!
//! Contract: `contracts/W8-10c.md`. Stub: `todo!()` bodies only (decision D11 in
//! `contracts/README.md`: the stub-level `allow` below is removed by the porter with the last `todo!()`).

#![allow(unused_variables, dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_core::settings::context_menu::ContextMenuItemId;

/// C# `SettingsFormState`. Every field is public and plain; the struct has no behaviour of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsFormState {
    /// Shortcut boxes: `ShortcutMappings` property name to the text exactly as typed (C# `StringComparer.Ordinal`).
    /// `BTreeMap` (not `HashMap`) so that any iteration is deterministic; the names are ASCII, so byte order is ordinal.
    pub shortcut_texts: BTreeMap<String, String>,
    /// The raw actions JSON text box.
    pub actions_json: String,

    // ---- View / zoom ----
    pub view_mode_index: i32,
    pub fit_width_anchor_index: i32,
    pub fit_width_anchor2_index: i32,
    pub middle_click_action_index: i32,
    pub keep_zoom_across_images: bool,
    pub loading_mode_index: i32,
    /// The `ImageSortMode` member name of the selected sort item (C# `ToString()`, for example `"Name"`); `None` when
    /// nothing is selected. Unknown or missing values are saved as `Name` by `SettingsFieldMap`, not here.
    pub sort_mode_tag: Option<String>,
    pub scaling_quality_index: i32,
    pub decoder_backend_index: i32,
    pub instance_mode_index: i32,
    pub compare_hash_enabled: bool,
    pub compare_size_enabled: bool,
    pub raw_support_enabled: bool,
    pub webp_heic_support_enabled: bool,
    pub raw_full_decode_index: i32,
    pub raw_pair_mode_index: i32,

    // ---- Files / safety ----
    pub logging_enabled: bool,
    pub allow_permanent_delete: bool,
    pub confirm_before_delete: bool,
    pub external_editor_path: String,
    pub journal_safe: bool,
    pub move_copy_reuse_last_folder: bool,

    // ---- Info overlay / toolbar ----
    pub show_info_overlay: bool,
    pub show_file_info: bool,
    pub show_folder_info: bool,
    pub info_overlay_font_size_text: String,
    pub show_zoom_indicator: bool,
    pub toolbar_auto_hide: bool,
    pub toolbar_auto_hide_delay_text: String,
    pub info_overlay_auto_hide: bool,
    pub info_overlay_auto_hide_delay_text: String,
    /// The opacity slider position (a double on the slider; rounded on Save).
    pub toolbar_opacity_value: f64,
    pub show_exif_info: bool,
    pub exif_info_fields: ExifInfoFields,
    pub title_bar_fields: TitleBarFields,

    // ---- Mouse / keyboard ----
    pub mouse_wheel_action_index: i32,
    pub click_to_zoom_enabled: bool,
    pub click_zoom_percent_text: String,
    pub kinetic_pan_enabled: bool,
    pub kinetic_glide_smoothing_index: i32,
    pub arrow_key_navigates_at_zoom_edge: bool,
    pub click_zoom_key_toggles_fit: bool,
    pub arrow_pan_step_text: String,
    pub keyboard_zoom_step_percent_text: String,
    pub keyboard_zoom_anchor_index: i32,
    pub image_transition_index: i32,
    pub image_transition_ms_text: String,
    pub touchpad_swipe_enabled: bool,
    pub touchpad_swipe_distance_text: String,

    // ---- Performance ----
    pub preload_forward_text: String,
    pub preload_backward_text: String,
    /// The RAM cache slider position (rounded on Save).
    pub ram_cache_percent: f64,

    // ---- Right-click menu ----
    /// The tick boxes that are ticked (shown). Items not in this set are saved as hidden. `BTreeSet` for determinism.
    pub context_menu_shown: BTreeSet<ContextMenuItemId>,

    // ---- Language ----
    /// The code of the selected picker entry (`LanguageOption.Code`); `None` when nothing is selected or the picker is
    /// hidden. Decision D-W8-10c-1: the code, not the `LanguageOption` record (see contract "Public API").
    pub selected_language_code: Option<String>,
}

impl Default for SettingsFormState {
    /// C# property initialisers: empty strings, zero integers and doubles, `false`, `ExifInfoFields`/`TitleBarFields`
    /// default(struct) (bits 0, i.e. `NONE`), empty sets, `None` for the optional tag and selection.
    fn default() -> Self {
        todo!()
    }
}

/// Which input a refused Save should put the keyboard focus in (a view may ignore it). C# `SettingsFocusField`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsFocusField {
    #[default]
    None,
    ArrowPanStep,
    KeyboardZoomStep,
    ImageTransitionMs,
    TouchpadSwipeDistance,
    PreloadForward,
    PreloadBackward,
    InfoOverlayAutoHideDelay,
}

/// One refused input: the localized message and the control to focus. C# `SettingsFieldError` (record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFieldError {
    pub message: String,
    pub focus: SettingsFocusField,
}

impl SettingsFieldError {
    /// C# `new SettingsFieldError(message, focus)`. The C# default `focus = None` is written out by the caller.
    pub fn new(message: impl Into<String>, focus: SettingsFocusField) -> Self {
        todo!()
    }
}

/// C# `SettingsSaveOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSaveOutcome {
    /// Everything was valid, written to the store and (if changed) the language switched: close the dialog with OK.
    Saved,
    /// An input was refused (nothing was persisted): show `SettingsSaveResult::message` as a warning, stay open.
    Invalid,
    /// The store could not write: show `SettingsSaveResult::message` with the "save failed" title, stay open.
    SaveFailed,
}

/// C# `SettingsSaveResult` (record). `message` is `None` for `Saved`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSaveResult {
    pub outcome: SettingsSaveOutcome,
    pub message: Option<String>,
    pub focus: SettingsFocusField,
}

impl SettingsSaveResult {
    /// C# `SettingsSaveResult.Saved` (a static shared instance in C#; a plain value here).
    pub fn saved() -> Self {
        todo!()
    }
}
