//! M4 mutation killers (hand-written, no golden data) for the presenter files of the PARITY close-out batch.
//! One section per production file; each test names the mutant it was written for.
use pr_app::presenters::settings_field_map::index_to_anchor;
use pr_app::viewmodels::compare::{CompareError, CompareLoadOptions};
use pr_core::model::enums::FitWidthAnchor;

// --- settings_field_map.rs -------------------------------------------------------------------------------------------

// C# `IndexToAnchor` (trap T6): index 0 is Centre whatever the fallback is.
#[test]
fn index_to_anchor_zero_is_centre_even_with_another_fallback() {
    assert_eq!(index_to_anchor(0, FitWidthAnchor::TopThird), FitWidthAnchor::Centre);
    assert_eq!(index_to_anchor(0, FitWidthAnchor::BottomThird), FitWidthAnchor::Centre);
}

// --- compare.rs --------------------------------------------------------------------------------------------------------

#[test]
fn compare_error_display_is_the_cancel_text_or_the_failure_message() {
    assert_eq!(CompareError::Cancelled.to_string(), "operation cancelled");
    assert_eq!(
        CompareError::Failed("decode broke".to_string()).to_string(),
        "decode broke"
    );
}

#[test]
fn compare_load_options_debug_lists_every_field() {
    let options = CompareLoadOptions {
        compare_size_enabled: true,
        current_index: 3,
        total_files: 9,
        initial_selected_path: Some("b.jpg".to_string()),
        ..CompareLoadOptions::default()
    };
    let text = format!("{options:?}");
    assert!(text.starts_with("CompareLoadOptions"), "{text}");
    assert!(text.contains("compare_size_enabled: true"), "{text}");
    assert!(text.contains("get_file_size: false"), "{text}");
    assert!(text.contains("current_index: 3"), "{text}");
    assert!(text.contains("total_files: 9"), "{text}");
    assert!(text.contains("b.jpg"), "{text}");
}

// C# `IndexToViewMode` (trap T17): 1..=5 map to the five modes, every other index (0, negative, 6) gives Fit.
#[test]
fn index_to_view_mode_maps_each_index_and_falls_back_to_fit() {
    use pr_app::presenters::settings_field_map::index_to_view_mode;
    use pr_core::model::enums::InitialViewMode as M;
    assert_eq!(index_to_view_mode(0), M::Fit);
    assert_eq!(index_to_view_mode(1), M::FitWidth);
    assert_eq!(index_to_view_mode(2), M::FitHeight);
    assert_eq!(index_to_view_mode(3), M::ClickZoomLevel);
    assert_eq!(index_to_view_mode(4), M::Percent100);
    assert_eq!(index_to_view_mode(5), M::Percent200);
    assert_eq!(index_to_view_mode(6), M::Fit);
    assert_eq!(index_to_view_mode(-1), M::Fit);
}

// C# `ViewModeToIndex` (trap T17) and `AnchorToIndex`.
#[test]
fn view_mode_and_anchor_indexes_follow_the_combo_order() {
    use pr_app::presenters::settings_field_map::{anchor_to_index, view_mode_to_index};
    use pr_core::model::enums::InitialViewMode as M;
    assert_eq!(view_mode_to_index(M::Fit), 0);
    assert_eq!(view_mode_to_index(M::FitWidth), 1);
    assert_eq!(view_mode_to_index(M::FitHeight), 2);
    assert_eq!(view_mode_to_index(M::ClickZoomLevel), 3);
    assert_eq!(view_mode_to_index(M::Percent100), 4);
    assert_eq!(view_mode_to_index(M::Percent200), 5);
    assert_eq!(view_mode_to_index(M::Percent400), 5);
    assert_eq!(anchor_to_index(FitWidthAnchor::Centre), 0);
    assert_eq!(anchor_to_index(FitWidthAnchor::TopThird), 1);
    assert_eq!(anchor_to_index(FitWidthAnchor::BottomThird), 2);
    assert_eq!(index_to_anchor(1, FitWidthAnchor::Centre), FitWidthAnchor::TopThird);
    assert_eq!(index_to_anchor(2, FitWidthAnchor::Centre), FitWidthAnchor::BottomThird);
    assert_eq!(index_to_anchor(3, FitWidthAnchor::TopThird), FitWidthAnchor::TopThird);
    assert_eq!(
        index_to_anchor(-1, FitWidthAnchor::BottomThird),
        FitWidthAnchor::BottomThird
    );
}

// C# `double.TryParse(Float, Invariant)` symbols (trap T4): NaN and Infinity in any case, `inf` and a comma are refused.
#[test]
fn try_double_reads_the_invariant_symbols_and_refuses_inf_and_commas() {
    use pr_app::presenters::settings_field_map::try_double;
    for text in ["NaN", "+NaN", "-NaN", " NaN\t"] {
        assert!(try_double(text).is_some_and(f64::is_nan), "{text:?}");
    }
    for text in [
        "Infinity",
        "+Infinity",
        "infinity",
        "INFINITY",
        "+infinity",
        "+INFINITY",
        " Infinity ",
    ] {
        assert_eq!(try_double(text), Some(f64::INFINITY), "{text:?}");
    }
    for text in ["-Infinity", "-infinity", "-INFINITY"] {
        assert_eq!(try_double(text), Some(f64::NEG_INFINITY), "{text:?}");
    }
    for text in ["inf", "INF", "Inf", "1,5", "1,000", ",", ""] {
        assert_eq!(try_double(text), None, "{text:?}");
    }
    assert_eq!(try_double("12.5"), Some(12.5));
    assert_eq!(try_double("-3e2"), Some(-300.0));
}

// C# `double.ToString()` thresholds (trap T13): 1E-04 is the last fixed value below 1, 1E+17 is the first exponent value.
#[test]
fn dotnet_double_text_switches_to_exponent_form_exactly_at_the_thresholds() {
    use pr_app::presenters::settings_field_map::dotnet_double_text;
    assert_eq!(dotnet_double_text(0.0001), "0.0001");
    assert_eq!(dotnet_double_text(0.00012), "0.00012");
    assert_eq!(dotnet_double_text(0.00001), "1E-05");
    assert_eq!(dotnet_double_text(1.0e16), "10000000000000000");
    assert_eq!(dotnet_double_text(1.0e17), "1E+17");
    assert_eq!(dotnet_double_text(f64::NAN), "NaN");
    assert_eq!(dotnet_double_text(f64::INFINITY), "Infinity");
    assert_eq!(dotnet_double_text(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(dotnet_double_text(0.0), "0");
    assert_eq!(dotnet_double_text(-0.0), "0");
    assert_eq!(dotnet_double_text(12.5), "12.5");
}

// C# `Double.ToString()` of the InfoOverlayFontSize load row: the exponent form starts at 1E+17 (the private
// `dotnet_load_double_text`, reached through the binding of the "InfoOverlayFontSize" property).
#[test]
fn info_overlay_font_size_load_row_uses_the_exponent_form_from_1e17() {
    use pr_app::presenters::settings_field_map::bindings;
    use pr_app::presenters::settings_form_state::SettingsFormState;
    use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
    let row = bindings()
        .iter()
        .find(|b| b.properties.contains(&"InfoOverlayFontSize"))
        .expect("InfoOverlayFontSize row");
    let load = |size: f64| {
        let mut settings = AppSettings::new(&EnglishSettingsTexts);
        settings.info_overlay_font_size = size;
        let mut form = SettingsFormState::default();
        (row.load)(&settings, &mut form);
        form.info_overlay_font_size_text
    };
    assert_eq!(load(12.0), "12");
    assert_eq!(load(12.5), "12.5");
    assert_eq!(load(1.0e16), "10000000000000000");
    assert_eq!(load(1.0e17), "1E+17");
    assert_eq!(load(1.0e20), "1E+20");
    assert_eq!(load(1.0e5), "100000");
}
