//! Red tests (W8-10c) for `presenters::settings_form_state`: the Settings form data and the save result.
//!
//! Port of the contract `contracts/W8-10c.md` (test kind F). No C# test exists for these types; the tests are written from
//! the doc comments of `SettingsFormState.cs` (baseline `d92d6aae`) and the stub. Every test here calls a stub that is
//! still `todo!()`, so each one fails until the porter implements the bodies.

use std::collections::BTreeSet;

use pr_app::presenters::settings_form_state::{
    SettingsFieldError, SettingsFocusField, SettingsFormState, SettingsSaveOutcome, SettingsSaveResult,
};
use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_core::settings::context_menu::ContextMenuItemId;

#[test]
fn settings_form_state_default_matches_csharp_initialisers() {
    // new: from semantics 1 of contracts/W8-10c.md (C# property initialisers of SettingsFormState.cs)
    let s = SettingsFormState::default();

    assert!(s.shortcut_texts.is_empty());
    assert_eq!(s.actions_json, "");

    assert_eq!(s.view_mode_index, 0);
    assert_eq!(s.fit_width_anchor_index, 0);
    assert_eq!(s.fit_width_anchor2_index, 0);
    assert_eq!(s.middle_click_action_index, 0);
    assert!(!s.keep_zoom_across_images);
    assert_eq!(s.loading_mode_index, 0);
    assert_eq!(s.sort_mode_tag, None);
    assert_eq!(s.scaling_quality_index, 0);
    assert_eq!(s.decoder_backend_index, 0);
    assert_eq!(s.instance_mode_index, 0);
    assert!(!s.compare_hash_enabled);
    assert!(!s.compare_size_enabled);
    assert!(!s.raw_support_enabled);
    assert!(!s.webp_heic_support_enabled);
    assert_eq!(s.raw_full_decode_index, 0);
    assert_eq!(s.raw_pair_mode_index, 0);

    assert!(!s.logging_enabled);
    assert!(!s.allow_permanent_delete);
    assert!(!s.confirm_before_delete);
    assert_eq!(s.external_editor_path, "");
    assert!(!s.journal_safe);
    assert!(!s.move_copy_reuse_last_folder);

    assert!(!s.show_info_overlay);
    assert!(!s.show_file_info);
    assert!(!s.show_folder_info);
    assert_eq!(s.info_overlay_font_size_text, "");
    assert!(!s.show_zoom_indicator);
    assert!(!s.toolbar_auto_hide);
    assert_eq!(s.toolbar_auto_hide_delay_text, "");
    assert!(!s.info_overlay_auto_hide);
    assert_eq!(s.info_overlay_auto_hide_delay_text, "");
    assert_eq!(s.toolbar_opacity_value, 0.0);
    assert!(!s.show_exif_info);
    // The two flag structs are default(struct) = bits 0 = NONE, not their DEFAULT sets.
    assert_eq!(s.exif_info_fields, ExifInfoFields::NONE);
    assert_eq!(s.title_bar_fields, TitleBarFields::NONE);

    assert_eq!(s.mouse_wheel_action_index, 0);
    assert!(!s.click_to_zoom_enabled);
    assert_eq!(s.click_zoom_percent_text, "");
    assert!(!s.kinetic_pan_enabled);
    assert_eq!(s.kinetic_glide_smoothing_index, 0);
    assert!(!s.arrow_key_navigates_at_zoom_edge);
    assert!(!s.click_zoom_key_toggles_fit);
    assert_eq!(s.arrow_pan_step_text, "");
    assert_eq!(s.keyboard_zoom_step_percent_text, "");
    assert_eq!(s.keyboard_zoom_anchor_index, 0);
    assert_eq!(s.image_transition_index, 0);
    assert_eq!(s.image_transition_ms_text, "");
    assert!(!s.touchpad_swipe_enabled);
    assert_eq!(s.touchpad_swipe_distance_text, "");

    assert_eq!(s.preload_forward_text, "");
    assert_eq!(s.preload_backward_text, "");
    assert_eq!(s.ram_cache_percent, 0.0);

    assert!(s.context_menu_shown.is_empty());
    assert_eq!(s.selected_language_code, None);
}

#[test]
fn settings_form_state_shortcut_texts_are_ordinal_and_sorted() {
    // new: from the `shortcut_texts` doc comment (ordinal keys). "F6" and "f6" are two different keys and iteration
    // follows byte order, so "F6" (0x46) comes before "f6" (0x66).
    let mut s = SettingsFormState::default();
    s.shortcut_texts.insert("f6".to_string(), "b".to_string());
    s.shortcut_texts.insert("F6".to_string(), "a".to_string());

    assert_eq!(s.shortcut_texts.len(), 2);
    let keys: Vec<&str> = s.shortcut_texts.keys().map(String::as_str).collect();
    assert_eq!(keys, ["F6", "f6"]);
    assert_eq!(s.shortcut_texts.get("F6").map(String::as_str), Some("a"));
    assert_eq!(s.shortcut_texts.get("f6").map(String::as_str), Some("b"));
}

#[test]
fn settings_save_result_saved_has_no_message_and_no_focus() {
    // new: from semantics 3 (`saved()` is `{ Saved, None, None }`, C# `SettingsSaveResult.Saved`)
    let saved = SettingsSaveResult::saved();

    assert_eq!(saved.outcome, SettingsSaveOutcome::Saved);
    assert_eq!(saved.message, None);
    assert_eq!(saved.focus, SettingsFocusField::None);
}

#[test]
fn settings_field_error_new_keeps_message_and_focus() {
    // new: from semantics 3 (`SettingsFieldError::new(m, f)` keeps both values)
    let error = SettingsFieldError::new("Preload must be a number", SettingsFocusField::PreloadForward);

    assert_eq!(error.message, "Preload must be a number");
    assert_eq!(error.focus, SettingsFocusField::PreloadForward);
}

#[test]
fn settings_focus_field_default_is_none_variant() {
    // new: T15 (`SettingsFocusField::None` is written out in every focus position, never `Option::None`).
    // The derived `Default` alone would already pass, so the check also needs the stub: a saved result has focus None.
    assert_eq!(SettingsFocusField::default(), SettingsFocusField::None);
    assert_eq!(SettingsSaveResult::saved().focus, SettingsFocusField::None);

    // The context-menu set type in the form is ordered (BTreeSet), so the same key set always iterates the same way.
    let shown: BTreeSet<ContextMenuItemId> = SettingsFormState::default().context_menu_shown;
    assert!(shown.is_empty());
}
