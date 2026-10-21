//! Amendments A3 and A6 of the Wave 8 B4 reconciliation (`contracts/README.md`): `MainViewModel::overlay_model` (C#
//! `OverlayModel.From(MainViewModel)`, `Shell.Win32/Overlay/OverlayModel.cs`) and `MainViewModel::settings_source`. Every test is
//! marked `// New test` (the C# `OverlayModel.From` has no direct test; the Rust shell windows consume it).

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::sync::Arc;

use pr_core::model::RawPairMode;
use support::main_env::{EnvOptions, MainEnv};

#[test]
fn overlay_model_without_a_folder_mirrors_the_closed_view_model() {
    // New test (amendment A3)
    let env = MainEnv::new();

    let model = env.vm.overlay_model();

    assert!(!model.has_images);
    assert!(!model.compare_visible);
    assert!(!model.capture_pair_visible);
    assert!(model.capture_pair_text.is_empty() && model.capture_pair_tool_tip.is_empty());
    assert!(!model.zoom_indicator_visible, "no image, no zoom indicator");
    assert_eq!(model.status_text, env.vm.status_text());
    assert_eq!(model.status_needs_attention, env.vm.status_needs_attention());
    assert_eq!(model.status_panel_visible, env.vm.is_status_panel_visible());
    let settings = env.vm.settings();
    assert_eq!(model.font_size, settings.info_overlay_font_size);
    assert_eq!(model.exif_font_size, settings.info_overlay_font_size - 1.0);
}

#[test]
fn overlay_model_with_an_open_folder_carries_the_view_model_getters() {
    // New test (amendment A3)
    let mut env = MainEnv::new();
    env.update_settings(|settings| {
        settings.show_zoom_indicator = true;
        settings.show_folder_info = true;
    });
    let (folder, _) = env.images("overlay_model_open", &["a.png", "b.png"]);
    env.open(&folder);

    let model = env.vm.overlay_model();

    assert!(model.has_images);
    assert!(model.zoom_indicator_visible);
    assert_eq!(
        model.zoom_indicator_text,
        format!("{}%", env.vm.viewer().borrow().display_zoom_percent())
    );
    assert_eq!(
        model.folder_info_visible,
        env.vm.info_overlay().is_folder_info_visible(&env.vm.settings())
    );
    assert_eq!(model.folder_info_text, env.vm.info_overlay().folder_info_text());
    assert_eq!(model.exif_text, env.vm.exif_text());
    assert_eq!(model.exif_visible, env.vm.is_exif_line_visible());
    assert_eq!(model.has_skipped_entries, env.vm.has_skipped_entries());
    assert_eq!(model.skipped_warning_text, env.vm.skipped_warning_text());
    assert_eq!(model.status_panel_visible, env.vm.is_status_panel_visible());
}

#[test]
fn overlay_model_follows_the_settings_and_compare_toggles() {
    // New test (amendment A3)
    // Compare needs a capture pair to stay visible.
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| {
            settings.raw_support_enabled = true;
            settings.raw_pair_mode = RawPairMode::PreferJpeg;
        }),
        ..EnvOptions::default()
    });
    let (folder, _) = env.images("overlay_model_toggle", &["a.jpg", "a.cr2"]);
    env.open(&folder);
    assert!(env.vm.overlay_model().file_info_visible);

    env.vm.toggle_info_overlay();
    assert!(!env.vm.overlay_model().file_info_visible);

    env.vm.toggle_compare();
    env.settle();
    assert!(env.vm.overlay_model().compare_visible);
}

#[test]
fn overlay_model_reports_the_capture_pair_badge_and_tool_tip_only_for_a_pair() {
    // New test (amendment A3; C# `hasPair ? CapturePairBadge : string.Empty`)
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| {
            settings.raw_support_enabled = true;
            settings.raw_pair_mode = RawPairMode::PreferJpeg;
        }),
        ..EnvOptions::default()
    });
    let (folder, _) = env.images("overlay_model_pair", &["a.jpg", "a.cr2"]);
    env.open(&folder);

    let model = env.vm.overlay_model();

    assert!(model.capture_pair_visible);
    assert_eq!(model.capture_pair_text, env.text("main.capturePair.badge.jpeg"));
    assert_eq!(model.capture_pair_tool_tip, env.vm.capture_pair_badge_tool_tip());
    assert!(!model.capture_pair_tool_tip.is_empty());
}

#[test]
fn settings_source_shares_the_override_with_the_view_model() {
    // New test (amendment A6)
    let mut env = MainEnv::new();
    let source = env.vm.settings_source();
    let mut changed = (*env.vm.settings()).clone();
    changed.info_overlay_font_size += 3.0;
    let changed = Arc::new(changed);

    env.vm.set_settings(Arc::clone(&changed));

    assert!(
        Arc::ptr_eq(&source.current(), &changed),
        "the view model's override is visible through the clone"
    );
    assert!(source.has_override());

    let other = Arc::new((*changed).clone());
    source.set_override(Some(Arc::clone(&other)));

    assert!(
        Arc::ptr_eq(&env.vm.settings(), &other),
        "an override set through the clone is visible to the view model"
    );
    assert!(Arc::ptr_eq(env.vm.settings_source().store(), source.store()));
}
