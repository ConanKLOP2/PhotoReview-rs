//! M4 mutation killers for the numeric rows of `presenters/settings_field_map.rs` (C# `SettingsFieldMap` Load/Apply of the
//! text boxes). Hand-written, no golden data.
use pr_app::presenters::settings_field_map::{FieldContext, bindings};
use pr_app::presenters::settings_form_state::{SettingsFieldError, SettingsFocusField, SettingsFormState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::NumberCulture;

fn settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

/// Loads every row from `settings()`, lets `edit` change the form, applies the row of `property` to a fresh copy.
fn apply(property: &str, edit: impl FnOnce(&mut SettingsFormState)) -> (Option<SettingsFieldError>, AppSettings) {
    let source = settings();
    let mut form = SettingsFormState::default();
    for b in bindings() {
        (b.load)(&source, &mut form);
    }
    edit(&mut form);
    let texts = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    let ctx = FieldContext {
        texts: &texts,
        numbers: &numbers,
    };
    let row = bindings()
        .iter()
        .find(|b| b.properties.contains(&property))
        .unwrap_or_else(|| panic!("no row for {property}"));
    let mut target = settings();
    let error = (row.apply)(&form, &mut target, &ctx);
    (error, target)
}

struct IntRow {
    property: &'static str,
    set: fn(&mut SettingsFormState, String),
    get: fn(&AppSettings) -> i32,
    min: i32,
    max: i32,
    focus: SettingsFocusField,
    message: &'static str,
}

fn int_rows() -> Vec<IntRow> {
    vec![
        IntRow {
            property: "ImageTransitionMs",
            set: |f, t| f.image_transition_ms_text = t,
            get: |s| s.image_transition_ms,
            min: 40,
            max: 400,
            focus: SettingsFocusField::ImageTransitionMs,
            message: "Fade duration must be a whole number between 40 and 400 milliseconds.",
        },
        IntRow {
            property: "TouchpadSwipeDistancePerImage",
            set: |f, t| f.touchpad_swipe_distance_text = t,
            get: |s| s.touchpad_swipe_distance_per_image,
            min: 30,
            max: 2400,
            focus: SettingsFocusField::TouchpadSwipeDistance,
            message: "Touchpad swipe per photo must be a number between 30 and 2400.",
        },
        IntRow {
            property: "PreloadForwardCount",
            set: |f, t| f.preload_forward_text = t,
            get: |s| s.preload_forward_count,
            min: 1,
            max: 500,
            focus: SettingsFocusField::PreloadForward,
            message: "Preload ahead must be a whole number from 1 to 500.",
        },
        IntRow {
            property: "PreloadBackwardCount",
            set: |f, t| f.preload_backward_text = t,
            get: |s| s.preload_backward_count,
            min: 0,
            max: 500,
            focus: SettingsFocusField::PreloadBackward,
            message: "Preload behind must be a whole number from 0 to 500.",
        },
        IntRow {
            property: "ToolbarAutoHideDelayMs",
            set: |f, t| f.toolbar_auto_hide_delay_text = t,
            get: |s| s.toolbar_auto_hide_delay_ms,
            min: 0,
            max: 10000,
            focus: SettingsFocusField::None,
            message: "Toolbar auto-hide delay must be a whole number between 0 and 10000 milliseconds.",
        },
        IntRow {
            property: "InfoOverlayAutoHideDelayMs",
            set: |f, t| f.info_overlay_auto_hide_delay_text = t,
            get: |s| s.info_overlay_auto_hide_delay_ms,
            min: 0,
            max: 10000,
            focus: SettingsFocusField::InfoOverlayAutoHideDelay,
            message: "Info auto-hide delay must be a whole number between 0 and 10000 milliseconds.",
        },
        IntRow {
            property: "ClickZoomPercent",
            set: |f, t| f.click_zoom_percent_text = t,
            get: |s| s.click_zoom_percent,
            min: 10,
            max: 800,
            focus: SettingsFocusField::None,
            message: "Click zoom level must be a number between 10 and 800.",
        },
        IntRow {
            property: "ArrowPanStepPercent",
            set: |f, t| f.arrow_pan_step_text = t,
            get: |s| s.arrow_pan_step_percent,
            min: 1,
            max: 100,
            focus: SettingsFocusField::ArrowPanStep,
            message: "Arrow key step must be a number between 1 and 100.",
        },
        IntRow {
            property: "KeyboardZoomStepPercent",
            set: |f, t| f.keyboard_zoom_step_percent_text = t,
            get: |s| s.keyboard_zoom_step_percent,
            min: 5,
            max: 100,
            focus: SettingsFocusField::KeyboardZoomStep,
            message: "Keyboard zoom step must be a whole number between 5 and 100.",
        },
    ]
}

#[test]
fn every_integer_row_accepts_its_range_and_refuses_outside_it_with_its_message_and_focus() {
    for row in int_rows() {
        let name = row.property;
        let original = (row.get)(&settings());
        for ok in [row.min, row.max, (row.min + row.max) / 2] {
            let (error, target) = apply(name, |f| (row.set)(f, format!(" {ok} ")));
            assert_eq!(error, None, "{name}: {ok}");
            assert_eq!((row.get)(&target), ok, "{name}: {ok}");
        }
        for bad in [
            (row.min - 1).to_string(),
            (row.max + 1).to_string(),
            "abc".to_string(),
            String::new(),
            "1.5".to_string(),
        ] {
            let (error, target) = apply(name, |f| (row.set)(f, bad.clone()));
            let error = error.unwrap_or_else(|| panic!("{name}: {bad:?} must be refused"));
            assert_eq!(error.message, row.message, "{name}: {bad:?}");
            assert_eq!(error.focus, row.focus, "{name}: {bad:?}");
            assert_eq!((row.get)(&target), original, "{name}: {bad:?} must not be written");
        }
    }
}

#[test]
fn preload_row_checks_forward_then_backward_and_writes_both_or_neither() {
    let (error, target) = apply("PreloadForwardCount", |f| {
        f.preload_forward_text = "0".into();
        f.preload_backward_text = "7".into();
    });
    assert_eq!(error.map(|e| e.focus), Some(SettingsFocusField::PreloadForward));
    assert_eq!(target.preload_backward_count, settings().preload_backward_count);
    let (error, target) = apply("PreloadForwardCount", |f| {
        f.preload_forward_text = "7".into();
        f.preload_backward_text = "501".into();
    });
    assert_eq!(error.map(|e| e.focus), Some(SettingsFocusField::PreloadBackward));
    assert_eq!(target.preload_forward_count, settings().preload_forward_count);
    let (error, target) = apply("PreloadForwardCount", |f| {
        f.preload_forward_text = "7".into();
        f.preload_backward_text = "9".into();
    });
    assert_eq!(error, None);
    assert_eq!((target.preload_forward_count, target.preload_backward_count), (7, 9));
}

#[test]
fn transition_and_touchpad_rows_apply_their_flags_even_when_the_number_is_refused() {
    use pr_core::model::enums::ImageTransition;
    let (_, target) = apply("ImageTransition", |f| f.image_transition_index = 1);
    assert_eq!(target.image_transition, ImageTransition::Fade);
    let (error, target) = apply("ImageTransition", |f| {
        f.image_transition_index = 1;
        f.image_transition_ms_text = "x".into();
    });
    assert!(error.is_some());
    assert_eq!(target.image_transition, ImageTransition::Fade);
    let (_, target) = apply("ImageTransition", |f| f.image_transition_index = 0);
    assert_eq!(target.image_transition, ImageTransition::None);
    let (_, target) = apply("TouchpadSwipeEnabled", |f| f.touchpad_swipe_enabled = true);
    assert!(target.touchpad_swipe_enabled);
    let (error, target) = apply("TouchpadSwipeEnabled", |f| {
        f.touchpad_swipe_enabled = true;
        f.touchpad_swipe_distance_text = "5".into();
    });
    assert!(error.is_some());
    assert!(target.touchpad_swipe_enabled);
}

#[test]
fn overlay_row_checks_the_delays_then_the_font_size_and_writes_all_or_nothing() {
    let message = "Info overlay font size must be a number between 8 and 24.";
    for ok in ["8", "24", "12.5", " 16 "] {
        let (error, target) = apply("InfoOverlayFontSize", |f| f.info_overlay_font_size_text = ok.into());
        assert_eq!(error, None, "{ok}");
        assert_eq!(target.info_overlay_font_size, ok.trim().parse::<f64>().unwrap(), "{ok}");
    }
    for bad in ["7.9", "24.1", "30", "NaN", "Infinity", "-Infinity", "x", ""] {
        let (error, target) = apply("InfoOverlayFontSize", |f| f.info_overlay_font_size_text = bad.into());
        let error = error.unwrap_or_else(|| panic!("{bad:?} must be refused"));
        assert_eq!(error.message, message, "{bad:?}");
        assert_eq!(error.focus, SettingsFocusField::None, "{bad:?}");
        assert_eq!(
            target.info_overlay_font_size,
            settings().info_overlay_font_size,
            "{bad:?}"
        );
    }
    // All fields are written together, the opacity rounds half to even and is clamped to 20..=100.
    let (error, target) = apply("ToolbarOpacityPercent", |f| {
        f.toolbar_auto_hide = true;
        f.info_overlay_auto_hide = true;
        f.toolbar_auto_hide_delay_text = "1234".into();
        f.info_overlay_auto_hide_delay_text = "4321".into();
        f.toolbar_opacity_value = 62.5;
        f.info_overlay_font_size_text = "13".into();
    });
    assert_eq!(error, None);
    assert!(target.toolbar_auto_hide && target.info_overlay_auto_hide);
    assert_eq!(target.toolbar_auto_hide_delay_ms, 1234);
    assert_eq!(target.info_overlay_auto_hide_delay_ms, 4321);
    assert_eq!(target.toolbar_opacity_percent, 62);
    assert_eq!(target.info_overlay_font_size, 13.0);
    for (value, expected) in [(63.5, 64), (5.0, 20), (250.0, 100), (20.0, 20), (100.0, 100)] {
        let (_, target) = apply("ToolbarOpacityPercent", |f| f.toolbar_opacity_value = value);
        assert_eq!(target.toolbar_opacity_percent, expected, "{value}");
    }
    // A refused delay writes nothing of the row.
    let (error, target) = apply("ToolbarOpacityPercent", |f| {
        f.toolbar_auto_hide = true;
        f.toolbar_auto_hide_delay_text = "-1".into();
    });
    assert!(error.is_some());
    assert_eq!(target.toolbar_auto_hide, settings().toolbar_auto_hide);
}

#[test]
fn ram_row_rounds_half_to_even() {
    for (value, expected) in [(12.5, 12), (13.5, 14), (50.0, 50), (49.4, 49)] {
        let (error, target) = apply("ImageCacheRamPercent", |f| f.ram_cache_percent = value);
        assert_eq!(error, None);
        assert_eq!(target.image_cache_ram_percent, expected, "{value}");
    }
}
