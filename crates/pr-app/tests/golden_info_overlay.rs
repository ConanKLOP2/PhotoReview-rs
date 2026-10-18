//! Golden-driven red tests (W8-06b, test porter) for the five goldens of the info overlay contract:
//! `info-overlay-autohide` (107 cases), `toolbar-autohide` (91), `fade-target-gate` (8), `image-transition` (108) and
//! `info-overlay-view-model` (23 scripts, every step checked). Oracle: `tools/oracle-cs` (`W8-06b.cs`) at baseline `d92d6aae`.
//! Every case of every file is run; no case is skipped. The case counts are asserted, so a short golden fails loudly.
//!
//! Number convention (`oracle/W8-06b.cs`): a JSON number, or the strings "NaN", "Infinity", "-Infinity" and "-0". Other
//! strings are decimal text (for example "0.30000000000000004"). Doubles are compared bit for bit: NaN matches nothing in
//! `==`, so the hot-zone and fade-gate cases check that NaN really takes the "false" and "always starts" paths (traps T1, T4).
//!
//! Every test here fails through `todo!()` until the bodies land.

use pr_app::coordinators::autohide::{InfoOverlayAutoHidePolicy, ToolbarAutoHidePolicy};
use pr_app::coordinators::fade_gate::FadeTargetGate;
use pr_app::coordinators::transition::ImageTransitionDecision;
use pr_app::viewmodels::info_overlay::{
    FolderInfoLabels, InfoOverlayProperty, InfoOverlayRefresh, InfoOverlayViewModel, SearchOutcome,
    SiblingImageFolders, ViewInputs,
};
use pr_core::settings::app_settings::AppSettings;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// JSON readers
// ---------------------------------------------------------------------------------------------------------------------

/// A golden number: a JSON number, one of the four oracle strings, or other decimal text.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => other
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("not a golden number {other:?}: {e}")),
        },
        other => panic!("not a number: {other}"),
    }
}

fn num_of(case: &Value, key: &str) -> f64 {
    num(&case[key])
}

/// `case[key]` as an i32 (the golden stores every C# `int` as a JSON integer).
fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit in i32: {case}"))
}

/// A golden string that may be JSON `null` (C# `string?`).
fn opt_str(value: &Value) -> Option<&str> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.as_str()),
        other => panic!("not a string or null: {other}"),
    }
}

/// Bit-exact double comparison (the sign of zero counts, NaN matches only its own bit pattern).
fn assert_bits(actual: f64, expected: f64, message: &str) {
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "{message}: {actual} != {expected}"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// info-overlay-autohide: InfoOverlayAutoHidePolicy (must_stay, evaluate, delay)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn info_overlay_autohide_golden_every_case() {
    let golden = golden::load("info-overlay-autohide");
    assert_eq!(golden.cases.len(), 107, "case count of info-overlay-autohide");

    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "kind") {
            "must_stay" => {
                let actual = InfoOverlayAutoHidePolicy::must_stay_visible(
                    bool_of(case, "enabled"),
                    bool_of(case, "folder"),
                    bool_of(case, "attention"),
                    bool_of(case, "compare"),
                    bool_of(case, "active"),
                );
                assert_eq!(actual, bool_of(case, "must_stay_visible"), "{name}");
            }
            "evaluate" => {
                let (enabled, folder, attention) = (
                    bool_of(case, "enabled"),
                    bool_of(case, "folder"),
                    bool_of(case, "attention"),
                );
                let (compare, active, idle) =
                    (bool_of(case, "compare"), bool_of(case, "active"), bool_of(case, "idle"));
                let must_stay =
                    InfoOverlayAutoHidePolicy::must_stay_visible(enabled, folder, attention, compare, active);
                assert_eq!(
                    must_stay,
                    bool_of(case, "must_stay_visible"),
                    "{name}: must_stay_visible"
                );

                let outcome = InfoOverlayAutoHidePolicy::evaluate(enabled, folder, attention, compare, active, idle);
                assert_bits(outcome.opacity, num_of(case, "opacity"), &format!("{name}: opacity"));
                assert_eq!(
                    outcome.is_hit_test_visible,
                    bool_of(case, "hit_test_visible"),
                    "{name}: hit test"
                );
            }
            "delay" => {
                let mut settings = AppSettings::default();
                settings.info_overlay_auto_hide_delay_ms = i32_of(case, "info_delay_ms");
                settings.toolbar_auto_hide_delay_ms = i32_of(case, "toolbar_delay_ms");
                assert_eq!(
                    InfoOverlayAutoHidePolicy::delay_ms(&settings),
                    i32_of(case, "delay_ms"),
                    "{name}"
                );
            }
            other => panic!("{name}: unknown kind {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// toolbar-autohide: ToolbarAutoHidePolicy (must_stay, is_shown, delay, target_opacity, hot_zone)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn toolbar_autohide_golden_every_case() {
    let golden = golden::load("toolbar-autohide");
    assert_eq!(golden.cases.len(), 91, "case count of toolbar-autohide");

    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "kind") {
            "must_stay" => {
                let actual = ToolbarAutoHidePolicy::must_stay_visible(
                    bool_of(case, "enabled"),
                    bool_of(case, "folder"),
                    bool_of(case, "popup"),
                    bool_of(case, "focus"),
                );
                assert_eq!(actual, bool_of(case, "must_stay_visible"), "{name}");
            }
            "is_shown" => {
                let actual = ToolbarAutoHidePolicy::is_shown(
                    bool_of(case, "enabled"),
                    bool_of(case, "folder"),
                    bool_of(case, "popup"),
                    bool_of(case, "focus"),
                    bool_of(case, "mouse"),
                );
                assert_eq!(actual, bool_of(case, "shown"), "{name}");
            }
            "delay" => {
                let mut settings = AppSettings::default();
                settings.info_overlay_auto_hide_delay_ms = i32_of(case, "info_delay_ms");
                settings.toolbar_auto_hide_delay_ms = i32_of(case, "toolbar_delay_ms");
                assert_eq!(
                    ToolbarAutoHidePolicy::delay_ms(&settings),
                    i32_of(case, "delay_ms"),
                    "{name}"
                );
            }
            "target_opacity" => {
                let actual = ToolbarAutoHidePolicy::target_opacity(i32_of(case, "percent"));
                assert_bits(actual, num_of(case, "opacity"), &format!("{name}: opacity"));
            }
            "hot_zone" => {
                let inside = ToolbarAutoHidePolicy::is_inside_hot_zone(
                    num_of(case, "mouse_x"),
                    num_of(case, "mouse_y"),
                    num_of(case, "left"),
                    num_of(case, "top"),
                    num_of(case, "width"),
                    num_of(case, "height"),
                    num_of(case, "margin"),
                );
                assert_eq!(inside, bool_of(case, "inside"), "{name}");
            }
            other => panic!("{name}: unknown kind {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// fade-target-gate: FadeTargetGate (sequences of targets, IEEE == on the stored target)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn fade_target_gate_golden_every_case() {
    let golden = golden::load("fade-target-gate");
    assert_eq!(golden.cases.len(), 8, "case count of fade-target-gate");

    for case in &golden.cases {
        let name = str_of(case, "name");
        assert_eq!(str_of(case, "kind"), "sequence", "{name}");
        let targets = case["targets"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: no targets"));
        let results = case["results"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: no results"));
        assert_eq!(
            targets.len(),
            results.len(),
            "{name}: targets and results differ in length"
        );

        let mut gate = FadeTargetGate::new();
        for (index, (target, expected)) in targets.iter().zip(results).enumerate() {
            let expected = expected
                .as_bool()
                .unwrap_or_else(|| panic!("{name}: result {index} not a bool"));
            assert_eq!(
                gate.try_change(num(target)),
                expected,
                "{name}: call {index} with target {target}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// image-transition: ImageTransitionDecision::should_transition (previous x new x compare)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn image_transition_golden_every_case() {
    let golden = golden::load("image-transition");
    assert_eq!(golden.cases.len(), 108, "case count of image-transition");

    for case in &golden.cases {
        let name = str_of(case, "name");
        assert_eq!(str_of(case, "kind"), "should", "{name}");
        let actual = ImageTransitionDecision::should_transition(
            opt_str(&case["previous"]),
            opt_str(&case["new"]),
            bool_of(case, "compare"),
        );
        assert_eq!(actual, bool_of(case, "should"), "{name}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// info-overlay-view-model: scripts over InfoOverlayViewModel (every step: raised names in order, searches, live state)
// ---------------------------------------------------------------------------------------------------------------------

/// English fallbacks of the folder-info catalog keys (what the C# golden sees).
#[derive(Debug)]
struct EnglishLabels;

impl FolderInfoLabels for EnglishLabels {
    fn current(&self, folder_name: &str) -> String {
        format!("📁 {folder_name}")
    }

    fn previous(&self, key: &str, folder_name: &str) -> String {
        format!("◀ {key}: {folder_name}")
    }

    fn next(&self, key: &str, folder_name: &str) -> String {
        format!("{key}: {folder_name} ▶")
    }

    fn pending(&self) -> String {
        "…".to_owned()
    }
}

type Overlay = InfoOverlayViewModel<EnglishLabels>;

/// Name of a notification, as the golden spells it (the C# `PropertyChanged` name).
fn property_name(property: InfoOverlayProperty) -> &'static str {
    match property {
        InfoOverlayProperty::IsFileInfoVisible => "IsFileInfoVisible",
        InfoOverlayProperty::IsFolderInfoVisible => "IsFolderInfoVisible",
        InfoOverlayProperty::FontSize => "FontSize",
        InfoOverlayProperty::ExifFontSize => "ExifFontSize",
        InfoOverlayProperty::IsZoomIndicatorVisible => "IsZoomIndicatorVisible",
        InfoOverlayProperty::ZoomIndicatorText => "ZoomIndicatorText",
        InfoOverlayProperty::FolderInfoText => "FolderInfoText",
    }
}

/// Applies the golden settings keys to `settings`. Every key of the golden `settings` object (or of a `set_settings`
/// step) is a C# `AppSettings` property; an unknown key fails the test instead of being ignored.
fn apply_settings(settings: &mut AppSettings, object: &Value, label: &str) {
    let entries = object
        .as_object()
        .unwrap_or_else(|| panic!("{label}: settings is not an object"));
    for (key, value) in entries {
        match key.as_str() {
            "op" | "expect" => {}
            "show_info_overlay" => settings.show_info_overlay = value.as_bool().expect("bool"),
            "show_file_info" => settings.show_file_info = value.as_bool().expect("bool"),
            "show_folder_info" => settings.show_folder_info = value.as_bool().expect("bool"),
            "show_zoom_indicator" => settings.show_zoom_indicator = value.as_bool().expect("bool"),
            "info_overlay_font_size" => settings.info_overlay_font_size = num(value),
            "next_folder" => settings.shortcuts.next_folder = value.as_str().expect("string").to_owned(),
            "previous_folder" => settings.shortcuts.previous_folder = value.as_str().expect("string").to_owned(),
            other => panic!("{label}: unknown settings key {other:?}"),
        }
    }
}

/// The finder answer for `folder`, as the golden script scripts it. A `fault` entry raises a finder error (IOException or
/// UnauthorizedAccessException), which the C# `SearchAsync` maps to the folder alone (`SearchOutcome::Failed`).
fn scripted_outcome(finder: &Value, folder: &str, label: &str) -> SearchOutcome {
    let entry = finder
        .get(folder)
        .unwrap_or_else(|| panic!("{label}: the script has no finder entry for {folder:?}"));
    if let Some(fault) = entry.get("fault") {
        assert!(
            matches!(fault.as_str(), Some("IOException" | "UnauthorizedAccessException")),
            "{label}: unexpected fault {fault}"
        );
        return SearchOutcome::Failed;
    }
    SearchOutcome::Found(SiblingImageFolders {
        previous: opt_str(&entry["previous"]).map(str::to_owned),
        next: opt_str(&entry["next"]).map(str::to_owned),
    })
}

/// Appends the notifications of `refresh` to `raised`, then runs its search (if any) the way the C# `SearchAsync` does
/// and appends the continuation's notifications. Returns the number of finder calls.
fn collect(
    overlay: &mut Overlay,
    settings: &AppSettings,
    inputs: ViewInputs,
    finder: &Value,
    first: InfoOverlayRefresh,
    raised: &mut Vec<String>,
    label: &str,
) -> usize {
    raised.extend(first.changes.iter().map(|p| property_name(p).to_owned()));
    let Some(search) = first.search else {
        return 0;
    };
    let outcome = scripted_outcome(finder, &search.folder, label);
    let second = overlay.complete_search(settings, inputs, &search, outcome);
    raised.extend(second.changes.iter().map(|p| property_name(p).to_owned()));
    assert!(
        second.search.is_none(),
        "{label}: the continuation must not start a second search"
    );
    1
}

/// Checks the seven live accessors against the golden `state` object.
fn assert_state(overlay: &Overlay, settings: &AppSettings, inputs: ViewInputs, state: &Value, label: &str) {
    assert_eq!(
        Overlay::is_file_info_visible(settings),
        bool_of(state, "file_info_visible"),
        "{label}: file_info_visible"
    );
    assert_eq!(
        overlay.is_folder_info_visible(settings),
        bool_of(state, "folder_info_visible"),
        "{label}: folder_info_visible"
    );
    assert_bits(
        Overlay::font_size(settings),
        num_of(state, "font_size"),
        &format!("{label}: font_size"),
    );
    assert_bits(
        Overlay::exif_font_size(settings),
        num_of(state, "exif_font_size"),
        &format!("{label}: exif_font_size"),
    );
    assert_eq!(
        Overlay::is_zoom_indicator_visible(settings, inputs),
        bool_of(state, "zoom_visible"),
        "{label}: zoom_visible"
    );
    assert_eq!(
        Overlay::zoom_indicator_text(inputs),
        str_of(state, "zoom_text"),
        "{label}: zoom_text"
    );
    assert_eq!(
        overlay.folder_info_text(),
        str_of(state, "folder_text"),
        "{label}: folder_text"
    );
}

fn run_script(case: &Value) {
    let name = str_of(case, "name");
    let mut settings = AppSettings::default();
    apply_settings(&mut settings, &case["settings"], name);
    let mut inputs = ViewInputs {
        has_image: bool_of(&case["view"], "has_image"),
        zoom_percent: i32_of(&case["view"], "zoom_percent"),
    };
    let finder = &case["finder"];
    let steps = case["steps"].as_array().unwrap_or_else(|| panic!("{name}: no steps"));
    let mut overlay = InfoOverlayViewModel::new(EnglishLabels);

    for (index, step) in steps.iter().enumerate() {
        let op = str_of(step, "op");
        let label = format!("{name} / step {index} ({op})");
        let mut raised: Vec<String> = Vec::new();
        let mut searches = 0;

        match op {
            "set_folder" => {
                let first = overlay.set_folder(&settings, inputs, Some(str_of(step, "folder")));
                searches += collect(&mut overlay, &settings, inputs, finder, first, &mut raised, &label);
            }
            "refresh" => {
                let first = overlay.refresh(&settings, inputs);
                searches += collect(&mut overlay, &settings, inputs, finder, first, &mut raised, &label);
            }
            "set_settings" => apply_settings(&mut settings, step, &label),
            "set_view" => {
                inputs = ViewInputs {
                    has_image: bool_of(step, "has_image"),
                    zoom_percent: i32_of(step, "zoom_percent"),
                };
            }
            other => panic!("{label}: unknown op {other:?}"),
        }

        let expect = &step["expect"];
        let expected_raised: Vec<String> = expect["raised"]
            .as_array()
            .unwrap_or_else(|| panic!("{label}: no expected raised list"))
            .iter()
            .map(|v| {
                v.as_str()
                    .unwrap_or_else(|| panic!("{label}: raised entry {v} is not a name"))
                    .to_owned()
            })
            .collect();
        assert_eq!(raised, expected_raised, "{label}: raised");
        let expected_searches = usize::try_from(i64_of(expect, "searches")).expect("searches is not negative");
        assert_eq!(searches, expected_searches, "{label}: searches");
        assert_state(&overlay, &settings, inputs, &expect["state"], &label);
    }
}

#[test]
fn info_overlay_view_model_golden_every_script() {
    let golden = golden::load("info-overlay-view-model");
    assert_eq!(golden.cases.len(), 23, "script count of info-overlay-view-model");

    for case in &golden.cases {
        assert_eq!(str_of(case, "kind"), "script", "{}", str_of(case, "name"));
        run_script(case);
    }
}
