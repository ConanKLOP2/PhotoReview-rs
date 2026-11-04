//! M4 mutation killers for the combo-box, toggle and editor rows of `presenters/settings_field_map.rs` (C# `SettingsFieldMap`
//! Load/Apply pairs). Every row is checked in both directions with values that differ from the defaults and from the
//! fallback of the row. Hand-written, no golden data.
use pr_app::presenters::settings_field_map::{FieldContext, bindings};
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_core::model::enums::*;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::external_editors::ExternalEditor;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::NumberCulture;

fn settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

/// Loads ONLY the row of `property` from `source` into a form that `preset` has filled with sentinels first.
fn load(property: &str, source: &AppSettings, preset: impl FnOnce(&mut SettingsFormState)) -> SettingsFormState {
    let mut form = SettingsFormState::default();
    preset(&mut form);
    let row = bindings()
        .iter()
        .find(|b| b.properties.contains(&property))
        .unwrap_or_else(|| panic!("no row for {property}"));
    (row.load)(source, &mut form);
    form
}

/// Applies ONLY the row of `property` from the form `edit` made (starting from the default form) onto `target`.
fn apply(property: &str, target: &mut AppSettings, edit: impl FnOnce(&mut SettingsFormState)) {
    let mut form = SettingsFormState::default();
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
    assert_eq!((row.apply)(&form, target, &ctx), None, "{property} never refuses");
}

#[test]
fn view_mode_and_anchor_rows_round_trip_each_value() {
    for (index, mode) in [
        (1, InitialViewMode::FitWidth),
        (2, InitialViewMode::FitHeight),
        (3, InitialViewMode::ClickZoomLevel),
        (4, InitialViewMode::Percent100),
        (5, InitialViewMode::Percent200),
    ] {
        let mut s = settings();
        s.initial_view_mode = mode;
        assert_eq!(
            load("InitialViewMode", &s, |f| f.view_mode_index = 9).view_mode_index,
            index
        );
        let mut target = settings();
        apply("InitialViewMode", &mut target, |f| f.view_mode_index = index);
        assert_eq!(target.initial_view_mode, mode);
    }
    let mut target = settings();
    target.initial_view_mode = InitialViewMode::FitWidth;
    apply("InitialViewMode", &mut target, |f| f.view_mode_index = 0);
    assert_eq!(target.initial_view_mode, InitialViewMode::Fit);

    for (index, anchor) in [(1, FitWidthAnchor::TopThird), (2, FitWidthAnchor::BottomThird)] {
        let mut s = settings();
        s.fit_width_anchor = anchor;
        assert_eq!(
            load("FitWidthAnchor", &s, |f| f.fit_width_anchor_index = 9).fit_width_anchor_index,
            index
        );
        let mut target = settings();
        apply("FitWidthAnchor", &mut target, |f| f.fit_width_anchor_index = index);
        assert_eq!(target.fit_width_anchor, anchor);
    }
    let mut target = settings();
    target.fit_width_anchor = FitWidthAnchor::TopThird;
    apply("FitWidthAnchor", &mut target, |f| f.fit_width_anchor_index = -1);
    assert_eq!(
        target.fit_width_anchor,
        FitWidthAnchor::Centre,
        "an unknown index falls back to Centre"
    );

    let mut s = settings();
    s.fit_width_anchor2 = Some(FitWidthAnchor::TopThird);
    assert_eq!(
        load("FitWidthAnchor2", &s, |f| f.fit_width_anchor2_index = 9).fit_width_anchor2_index,
        1
    );
    s.fit_width_anchor2 = None;
    assert_eq!(
        load("FitWidthAnchor2", &s, |f| f.fit_width_anchor2_index = 9).fit_width_anchor2_index,
        2
    );
    for (index, anchor) in [
        (0, FitWidthAnchor::Centre),
        (1, FitWidthAnchor::TopThird),
        (7, FitWidthAnchor::BottomThird),
    ] {
        let mut target = settings();
        target.fit_width_anchor2 = None;
        apply("FitWidthAnchor2", &mut target, |f| f.fit_width_anchor2_index = index);
        assert_eq!(target.fit_width_anchor2, Some(anchor));
    }
}

#[test]
fn middle_click_row_maps_every_index_and_defaults_to_actual_size() {
    for (index, action) in [
        (0, MiddleClickAction::None),
        (1, MiddleClickAction::ClickZoom),
        (3, MiddleClickAction::Fit),
        (4, MiddleClickAction::FitWidth),
        (5, MiddleClickAction::FitWidth2),
    ] {
        let mut target = settings();
        apply("MiddleClickAction", &mut target, |f| {
            f.middle_click_action_index = index
        });
        assert_eq!(target.middle_click_action, Some(action));
        let mut s = settings();
        s.middle_click_action = Some(action);
        assert_eq!(
            load("MiddleClickAction", &s, |f| f.middle_click_action_index = 9).middle_click_action_index,
            index
        );
    }
    for index in [2, 6, 99, -1] {
        let mut target = settings();
        target.middle_click_action = Some(MiddleClickAction::Fit);
        apply("MiddleClickAction", &mut target, |f| {
            f.middle_click_action_index = index
        });
        assert_eq!(
            target.middle_click_action,
            Some(MiddleClickAction::ActualSize),
            "index {index}"
        );
    }
    let mut s = settings();
    s.middle_click_action = None;
    assert_eq!(
        load("MiddleClickAction", &s, |f| f.middle_click_action_index = 9).middle_click_action_index,
        2
    );
}

#[test]
fn sort_row_reads_names_and_numbers_and_loads_the_name() {
    for (tag, mode) in [
        ("Name", ImageSortMode::Name),
        ("SizeDescending", ImageSortMode::SizeDescending),
        ("SizeAscending", ImageSortMode::SizeAscending),
        ("Default", ImageSortMode::Default),
        ("NameAscending", ImageSortMode::NameAscending),
        ("NameDescending", ImageSortMode::NameDescending),
        ("0", ImageSortMode::Name),
        ("1", ImageSortMode::SizeDescending),
        ("2", ImageSortMode::SizeAscending),
        ("3", ImageSortMode::Default),
        ("4", ImageSortMode::NameAscending),
        ("5", ImageSortMode::NameDescending),
        (" SizeAscending ", ImageSortMode::SizeAscending),
    ] {
        let mut target = settings();
        target.image_sort_mode = Some(ImageSortMode::Default);
        apply("ImageSortMode", &mut target, |f| f.sort_mode_tag = Some(tag.to_owned()));
        assert_eq!(target.image_sort_mode, Some(mode), "tag {tag:?}");
    }
    for tag in [None, Some("bogus".to_owned())] {
        let mut target = settings();
        target.image_sort_mode = None;
        apply("ImageSortMode", &mut target, |f| f.sort_mode_tag = tag.clone());
        assert_eq!(target.image_sort_mode, Some(ImageSortMode::Name), "unknown tag {tag:?}");
    }
    let mut s = settings();
    s.image_sort_mode = Some(ImageSortMode::SizeAscending);
    assert_eq!(
        load("ImageSortMode", &s, |f| f.sort_mode_tag = Some("x".to_owned())).sort_mode_tag,
        Some("SizeAscending".to_owned())
    );
}

#[test]
fn scaling_decoder_loading_instance_rows_round_trip() {
    let mut s = settings();
    s.scaling_quality = Some(ScalingQuality::Linear);
    assert_eq!(
        load("ScalingQuality", &s, |f| f.scaling_quality_index = 9).scaling_quality_index,
        1
    );
    s.scaling_quality = Some(ScalingQuality::HighQuality);
    assert_eq!(
        load("ScalingQuality", &s, |f| f.scaling_quality_index = 9).scaling_quality_index,
        0
    );
    for (index, quality) in [(1, ScalingQuality::Linear), (0, ScalingQuality::HighQuality)] {
        let mut target = settings();
        target.scaling_quality = None;
        apply("ScalingQuality", &mut target, |f| f.scaling_quality_index = index);
        assert_eq!(target.scaling_quality, Some(quality));
    }

    for (index, backend) in [
        (0, DecoderBackend::Wpf),
        (1, DecoderBackend::WicDirect),
        (2, DecoderBackend::TurboJpeg),
    ] {
        let mut s = settings();
        s.decoder_backend = Some(backend);
        assert_eq!(
            load("DecoderBackend", &s, |f| f.decoder_backend_index = 9).decoder_backend_index,
            index
        );
        let mut target = settings();
        target.decoder_backend = None;
        apply("DecoderBackend", &mut target, |f| f.decoder_backend_index = index);
        assert_eq!(target.decoder_backend, Some(backend));
    }

    for (index, mode) in [
        (0, LoadingMode::Fast),
        (1, LoadingMode::Preview),
        (2, LoadingMode::Original),
    ] {
        let mut s = settings();
        s.loading_mode = Some(mode);
        assert_eq!(
            load("LoadingMode", &s, |f| f.loading_mode_index = 9).loading_mode_index,
            index
        );
        let mut target = settings();
        target.loading_mode = None;
        apply("LoadingMode", &mut target, |f| f.loading_mode_index = index);
        assert_eq!(target.loading_mode, Some(mode));
    }

    for (index, mode) in [(0, InstanceMode::SingleWindow), (1, InstanceMode::PerFolder)] {
        let mut s = settings();
        s.instance_mode = mode;
        assert_eq!(
            load("InstanceMode", &s, |f| f.instance_mode_index = 9).instance_mode_index,
            index
        );
        let mut target = settings();
        target.instance_mode = if index == 0 {
            InstanceMode::PerFolder
        } else {
            InstanceMode::SingleWindow
        };
        apply("InstanceMode", &mut target, |f| f.instance_mode_index = index);
        assert_eq!(target.instance_mode, mode);
    }
}

#[test]
fn raw_wheel_glide_anchor_journal_and_delete_rows_round_trip() {
    for (index, mode) in [(0, RawFullDecode::Never), (1, RawFullDecode::OnZoom)] {
        let mut s = settings();
        s.raw_full_decode = mode;
        assert_eq!(
            load("RawFullDecode", &s, |f| f.raw_full_decode_index = 9).raw_full_decode_index,
            index
        );
        let mut target = settings();
        target.raw_full_decode = if index == 0 {
            RawFullDecode::OnZoom
        } else {
            RawFullDecode::Never
        };
        apply("RawFullDecode", &mut target, |f| f.raw_full_decode_index = index);
        assert_eq!(target.raw_full_decode, mode);
    }
    for (index, mode) in [
        (0, RawPairMode::Separate),
        (1, RawPairMode::PreferJpeg),
        (2, RawPairMode::PreferRaw),
    ] {
        let mut s = settings();
        s.raw_pair_mode = mode;
        assert_eq!(
            load("RawPairMode", &s, |f| f.raw_pair_mode_index = 9).raw_pair_mode_index,
            index
        );
        for start in [RawPairMode::PreferRaw, RawPairMode::PreferJpeg] {
            let mut target = settings();
            target.raw_pair_mode = start;
            apply("RawPairMode", &mut target, |f| f.raw_pair_mode_index = index);
            assert_eq!(target.raw_pair_mode, mode);
        }
    }

    for (index, action) in [(0, MouseWheelAction::Zoom), (1, MouseWheelAction::Navigate)] {
        let mut s = settings();
        s.mouse_wheel_action = action;
        assert_eq!(
            load("MouseWheelAction", &s, |f| f.mouse_wheel_action_index = 9).mouse_wheel_action_index,
            index
        );
        let mut target = settings();
        target.mouse_wheel_action = if index == 0 {
            MouseWheelAction::Navigate
        } else {
            MouseWheelAction::Zoom
        };
        apply("MouseWheelAction", &mut target, |f| f.mouse_wheel_action_index = index);
        assert_eq!(target.mouse_wheel_action, action);
    }
    for (index, smoothing) in [(0, KineticGlideSmoothing::Off), (1, KineticGlideSmoothing::Predict)] {
        let mut s = settings();
        s.kinetic_glide_smoothing = Some(smoothing);
        assert_eq!(
            load("KineticGlideSmoothing", &s, |f| f.kinetic_glide_smoothing_index = 9).kinetic_glide_smoothing_index,
            index
        );
        let mut target = settings();
        target.kinetic_glide_smoothing = None;
        apply("KineticGlideSmoothing", &mut target, |f| {
            f.kinetic_glide_smoothing_index = index
        });
        assert_eq!(target.kinetic_glide_smoothing, Some(smoothing));
    }
    for (index, anchor) in [
        (0, KeyboardZoomAnchor::Pointer),
        (1, KeyboardZoomAnchor::ViewportCentre),
    ] {
        let mut s = settings();
        s.keyboard_zoom_anchor = Some(anchor);
        assert_eq!(
            load("KeyboardZoomAnchor", &s, |f| f.keyboard_zoom_anchor_index = 9).keyboard_zoom_anchor_index,
            index
        );
        let mut target = settings();
        target.keyboard_zoom_anchor = None;
        apply("KeyboardZoomAnchor", &mut target, |f| {
            f.keyboard_zoom_anchor_index = index
        });
        assert_eq!(target.keyboard_zoom_anchor, Some(anchor));
    }

    for (safe, durability) in [
        (true, JournalDurability::PowerLossSafe),
        (false, JournalDurability::Fast),
    ] {
        let mut s = settings();
        s.journal_durability = durability;
        assert_eq!(
            load("JournalDurability", &s, |f| f.journal_safe = !safe).journal_safe,
            safe
        );
        let mut target = settings();
        target.journal_durability = if safe {
            JournalDurability::Fast
        } else {
            JournalDurability::PowerLossSafe
        };
        apply("JournalDurability", &mut target, |f| f.journal_safe = safe);
        assert_eq!(target.journal_durability, durability);
    }

    for value in [true, false] {
        let mut s = settings();
        s.allow_permanent_delete_without_recycle_bin = value;
        assert_eq!(
            load("AllowPermanentDeleteWithoutRecycleBin", &s, |f| f
                .allow_permanent_delete =
                !value)
            .allow_permanent_delete,
            value
        );
        let mut target = settings();
        target.allow_permanent_delete_without_recycle_bin = !value;
        apply("AllowPermanentDeleteWithoutRecycleBin", &mut target, |f| {
            f.allow_permanent_delete = value
        });
        assert_eq!(target.allow_permanent_delete_without_recycle_bin, value);
    }
}

#[test]
fn editor_row_copies_the_list_the_default_the_raw_flag_and_the_default_path() {
    let editor = |id: &str, path: &str| ExternalEditor {
        id: id.to_owned(),
        name: id.to_owned(),
        path: path.to_owned(),
        arguments: ExternalEditor::DEFAULT_ARGUMENTS.to_owned(),
    };
    let mut s = settings();
    s.external_editors = vec![editor("a", r"C:\ed\a.exe"), editor("b", r"C:\ed\b.exe")];
    s.default_external_editor_id = "b".to_owned();
    s.external_editor_sends_raw_original = true;
    let form = load("ExternalEditors", &s, |f| f.external_editor_sends_raw_original = false);
    assert_eq!(form.external_editors, s.external_editors);
    assert_eq!(form.default_external_editor_id, "b");
    assert!(form.external_editor_sends_raw_original);
    assert_eq!(
        form.external_editor_path, r"C:\ed\b.exe",
        "the legacy box shows the default editor path"
    );

    let mut target = settings();
    apply("ExternalEditors", &mut target, |f| {
        f.external_editors = vec![editor("a", r"C:\ed\a.exe"), editor("b", r"C:\ed\b.exe")];
        f.default_external_editor_id = "b".to_owned();
        f.external_editor_sends_raw_original = true;
        f.external_editor_path = r"C:\ed\b.exe".to_owned();
    });
    assert_eq!(target.external_editors.len(), 2);
    assert_eq!(target.default_external_editor_id, "b");
    assert!(target.external_editor_sends_raw_original);
    assert_eq!(target.external_editor_path, r"C:\ed\b.exe");
}
