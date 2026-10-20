//! Golden-driven red tests (W8-06a, test porter) for `pr_app::viewmodels::viewer_state`, section D of `contracts/W8-06a.md`.
//! Oracle: `tools/oracle-cs` (`W8-06a.cs`) at baseline `d92d6aae`. Every case is run; no case is skipped.
//!
//! - `golden_viewer_state_trace`: `tests/golden/viewer-state-trace.json`, 31 scripts. The golden stores, per step, the expected
//!   `changes` (sorted C# names), `zoom_mode_changed`, `swap_events` (seen BEFORE the change), `applied` (apply op only) and every
//!   observable `state` field. The step inputs are read from `tests/golden-inputs/viewer-state-trace.json` (the golden echoes only
//!   `op` and `expect`), matched by case and step index.
//! - `golden_viewer_step_zoom`: `tests/golden/viewer-step-zoom.json`, 33 cases of `CalculateStepZoom` (inputs echoed).
//! - `golden_viewer_display_size`: `tests/golden/viewer-display-size.json`, 17 cases of `CalculateDisplaySize` (inputs echoed).
//!
//! Number convention (`oracle/W8-06a.cs`): a JSON number, or the strings "NaN", "Infinity", "-Infinity" and "-0"; `null` is
//! `None` (`effective_zoom`, an expected step result of "no change"). Doubles are compared with `==`, NaN equal to NaN, no
//! tolerance: the port performs the same IEEE operations in the same order.
//!
//! Every test here fails through `todo!()` until the bodies land.

use pr_app::viewmodels::viewer_state::{
    ViewerChanges, ViewerState, ViewerUpdate, calculate_display_size, calculate_step_zoom,
};
use pr_app::viewport::types::ViewerStretchMode;
use pr_core::model::enums::{InitialViewMode, ScalingQuality};
use pr_test_support::golden;
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// JSON readers
// ---------------------------------------------------------------------------------------------------------------------

/// A golden number: a JSON number, or one of the four strings of the oracle convention.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => panic!("not a golden number: {other:?}"),
        },
        other => panic!("not a number: {other}"),
    }
}

fn field<'a>(object: &'a Value, key: &str) -> &'a Value {
    object.get(key).unwrap_or_else(|| panic!("missing '{key}' in {object}"))
}

fn num_of(object: &Value, key: &str) -> f64 {
    num(field(object, key))
}

fn int_of(object: &Value, key: &str) -> i32 {
    let value = field(object, key);
    value
        .as_i64()
        .and_then(|raw| i32::try_from(raw).ok())
        .unwrap_or_else(|| panic!("'{key}' is not an i32: {value}"))
}

fn bool_of(object: &Value, key: &str) -> bool {
    field(object, key)
        .as_bool()
        .unwrap_or_else(|| panic!("'{key}' is not a bool: {object}"))
}

fn str_of<'a>(object: &'a Value, key: &str) -> &'a str {
    field(object, key)
        .as_str()
        .unwrap_or_else(|| panic!("'{key}' is not a string: {object}"))
}

/// Bit-level equality for the golden doubles: `==`, and NaN equals NaN.
fn same(actual: f64, expected: f64) -> bool {
    actual == expected || (actual.is_nan() && expected.is_nan())
}

fn assert_same(actual: f64, expected: f64, context: &str) {
    assert!(
        same(actual, expected),
        "{context}: actual {actual}, expected {expected}"
    );
}

fn assert_same_opt(actual: Option<f64>, expected: &Value, context: &str) {
    match expected {
        Value::Null => assert!(actual.is_none(), "{context}: actual {actual:?}, expected null"),
        other => {
            let want = num(other);
            let got = actual.unwrap_or_else(|| panic!("{context}: actual None, expected {want}"));
            assert_same(got, want, context);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Enum readers (C# names, as the goldens write them)
// ---------------------------------------------------------------------------------------------------------------------

fn stretch_of(name: &str) -> ViewerStretchMode {
    match name {
        "None" => ViewerStretchMode::None,
        "Uniform" => ViewerStretchMode::Uniform,
        other => panic!("unknown ViewerStretchMode {other:?}"),
    }
}

fn scaling_of(name: &str) -> ScalingQuality {
    match name {
        "Linear" => ScalingQuality::Linear,
        "HighQuality" => ScalingQuality::HighQuality,
        other => panic!("unknown ScalingQuality {other:?}"),
    }
}

fn mode_of(name: &str) -> InitialViewMode {
    match name {
        "Fit" => InitialViewMode::Fit,
        "Percent100" => InitialViewMode::Percent100,
        "Percent200" => InitialViewMode::Percent200,
        "Percent400" => InitialViewMode::Percent400,
        "FitWidth" => InitialViewMode::FitWidth,
        "FitHeight" => InitialViewMode::FitHeight,
        "ClickZoomLevel" => InitialViewMode::ClickZoomLevel,
        other => panic!("unknown InitialViewMode {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// golden-viewer-state-trace
// ---------------------------------------------------------------------------------------------------------------------

/// `tests/golden-inputs/viewer-state-trace.json` `cases` (the step inputs; the golden echoes only `op` and `expect`).
fn trace_input_cases() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("viewer-state-trace.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

/// The observable outcome of one scripted step.
struct Outcome {
    update: ViewerUpdate,
    applied: Option<bool>,
    swap_events: Vec<(i32, i32)>,
}

/// Runs one input step against `state`, exactly as the oracle script does.
fn run_step(state: &mut ViewerState, step: &Value) -> Outcome {
    let op = str_of(step, "op");
    let plain = |update: ViewerUpdate| Outcome {
        update,
        applied: None,
        swap_events: Vec::new(),
    };
    let raw = |changes: ViewerChanges| {
        plain(ViewerUpdate {
            changes,
            zoom_mode_changed: false,
        })
    };
    match op {
        "query" => plain(ViewerUpdate::default()),
        "set_source_size" => plain(state.set_source_size(
            int_of(step, "width"),
            int_of(step, "height"),
            bool_of(step, "new_image"),
        )),
        "swap_source_size" => {
            let mut events = Vec::new();
            let update = state.swap_source_size(
                int_of(step, "width"),
                int_of(step, "height"),
                &mut |seen: &ViewerState| {
                    events.push((seen.source_pixel_width(), seen.source_pixel_height()));
                },
            );
            Outcome {
                update,
                applied: None,
                swap_events: events,
            }
        }
        "set_zoom" => plain(state.set_zoom(num_of(step, "value"))),
        "set_zoom_raw" => raw(state.set_zoom_raw(num_of(step, "value"))),
        "zoom_to_actual_size" => plain(state.zoom_to_actual_size()),
        "zoom_to_fit_width" => plain(state.zoom_to_fit_width()),
        "zoom_to_fit_height" => plain(state.zoom_to_fit_height()),
        "zoom_in" => plain(state.zoom_in()),
        "zoom_out" => plain(state.zoom_out()),
        "wheel_zoom" => plain(state.wheel_zoom(int_of(step, "delta"))),
        "reset_fit" => plain(state.reset_fit(num_of(step, "viewport_width"), num_of(step, "viewport_height"))),
        "update_viewport" => plain(state.update_viewport(
            num_of(step, "viewport_width"),
            num_of(step, "viewport_height"),
            bool_of(step, "force"),
        )),
        "apply_initial_view_mode" => {
            let result = state.apply_initial_view_mode(
                mode_of(str_of(step, "mode")),
                num_of(step, "viewport_width"),
                num_of(step, "viewport_height"),
                int_of(step, "click_zoom_percent"),
                bool_of(step, "keep_zoom_across_images"),
            );
            Outcome {
                update: result.unwrap_or_default(),
                applied: Some(result.is_some()),
                swap_events: Vec::new(),
            }
        }
        "toggle_fullscreen" => plain(state.toggle_fullscreen()),
        "exit_fullscreen" => plain(state.exit_fullscreen()),
        "set_stretch" => raw(state.set_stretch(stretch_of(str_of(step, "value")))),
        "set_dpi_scale" => raw(state.set_dpi_scale(num_of(step, "value"))),
        "set_max_image_width" => raw(state.set_max_image_width(num_of(step, "value"))),
        "set_max_image_height" => raw(state.set_max_image_height(num_of(step, "value"))),
        "set_scaling_quality" => raw(state.set_scaling_quality(scaling_of(str_of(step, "value")))),
        "set_force_high_quality" => raw(state.set_force_high_quality_scaling(bool_of(step, "value"))),
        "set_zoom_step" => {
            state.set_zoom_step(num_of(step, "value"));
            plain(ViewerUpdate::default())
        }
        other => panic!("unknown trace op {other:?}"),
    }
}

/// Compares the `state` object of one expected step with the live state.
fn check_state(state: &ViewerState, expected: &Value, context: &str) {
    let check = |key: &str| format!("{context}: state.{key}");

    assert_same(state.zoom(), num_of(expected, "zoom"), &check("zoom"));
    assert_eq!(
        format!("{:?}", state.stretch()),
        str_of(expected, "stretch"),
        "{}",
        check("stretch")
    );
    assert_eq!(state.is_fit(), bool_of(expected, "is_fit"), "{}", check("is_fit"));
    assert_same(
        state.image_width(),
        num_of(expected, "image_width"),
        &check("image_width"),
    );
    assert_same(
        state.image_height(),
        num_of(expected, "image_height"),
        &check("image_height"),
    );
    assert_same(
        state.max_image_width(),
        num_of(expected, "max_image_width"),
        &check("max_image_width"),
    );
    assert_same(
        state.max_image_height(),
        num_of(expected, "max_image_height"),
        &check("max_image_height"),
    );
    assert_eq!(
        state.source_pixel_width(),
        int_of(expected, "source_pixel_width"),
        "{}",
        check("source_pixel_width")
    );
    assert_eq!(
        state.source_pixel_height(),
        int_of(expected, "source_pixel_height"),
        "{}",
        check("source_pixel_height")
    );
    assert_same(state.dpi_scale(), num_of(expected, "dpi_scale"), &check("dpi_scale"));
    assert_eq!(
        state.is_fullscreen(),
        bool_of(expected, "is_fullscreen"),
        "{}",
        check("is_fullscreen")
    );
    assert_eq!(
        format!("{:?}", state.scaling_quality()),
        str_of(expected, "scaling_quality"),
        "{}",
        check("scaling_quality")
    );
    assert_eq!(
        state.force_high_quality_scaling(),
        bool_of(expected, "force_high_quality_scaling"),
        "{}",
        check("force_high_quality_scaling")
    );
    assert_eq!(
        format!("{:?}", state.effective_scaling_quality()),
        str_of(expected, "effective_scaling_quality"),
        "{}",
        check("effective_scaling_quality")
    );
    assert_same_opt(
        state.effective_zoom(),
        field(expected, "effective_zoom"),
        &check("effective_zoom"),
    );
    assert_same(state.fit_zoom(), num_of(expected, "fit_zoom"), &check("fit_zoom"));
    assert_same(
        state.fit_width_zoom(),
        num_of(expected, "fit_width_zoom"),
        &check("fit_width_zoom"),
    );
    assert_same(
        state.fit_height_zoom(),
        num_of(expected, "fit_height_zoom"),
        &check("fit_height_zoom"),
    );
    assert_eq!(
        state.display_zoom_percent(),
        int_of(expected, "display_zoom_percent"),
        "{}",
        check("display_zoom_percent")
    );
    assert_same(state.zoom_step(), num_of(expected, "zoom_step"), &check("zoom_step"));
}

/// Compares one step's `expect` object with its outcome and the state after the step.
fn check_step(state: &ViewerState, outcome: &Outcome, expect: &Value, context: &str) {
    let mut want_changes: Vec<String> = field(expect, "changes")
        .as_array()
        .unwrap_or_else(|| panic!("{context}: 'changes' is not an array"))
        .iter()
        .map(|name| {
            name.as_str()
                .unwrap_or_else(|| panic!("{context}: change name"))
                .to_owned()
        })
        .collect();
    want_changes.sort();
    let mut got_changes: Vec<String> = outcome
        .update
        .changes
        .names()
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    got_changes.sort();
    assert_eq!(got_changes, want_changes, "{context}: changes");

    assert_eq!(
        outcome.update.zoom_mode_changed,
        bool_of(expect, "zoom_mode_changed"),
        "{context}: zoom_mode_changed"
    );

    let want_swaps: Vec<(i32, i32)> = field(expect, "swap_events")
        .as_array()
        .unwrap_or_else(|| panic!("{context}: 'swap_events' is not an array"))
        .iter()
        .map(|event| (int_of(event, "width"), int_of(event, "height")))
        .collect();
    assert_eq!(
        outcome.swap_events, want_swaps,
        "{context}: swap_events (seen before the change)"
    );

    if let Some(applied) = expect.get("applied") {
        assert_eq!(
            outcome.applied,
            Some(
                applied
                    .as_bool()
                    .unwrap_or_else(|| panic!("{context}: 'applied' is not a bool"))
            ),
            "{context}: applied"
        );
    }

    check_state(state, field(expect, "state"), context);
}

// C#: ViewerState (ViewerStateTests, ViewerStateMutationGapTests, ViewerStateRefreshScalingTests) observed through the
// scripted trace of the oracle; the trace drives every mutating command and checks the change set, the mode-change flag,
// the swap announcements and every observable field after each step.
#[test]
fn golden_viewer_state_trace() {
    let golden = golden::load("viewer-state-trace");
    let inputs = trace_input_cases();
    assert_eq!(
        golden.cases.len(),
        inputs.len(),
        "case count of the trace golden and its inputs"
    );

    for (case, input) in golden.cases.iter().zip(&inputs) {
        let name = str_of(case, "name");
        assert_eq!(name, str_of(input, "name"), "case order of the golden and its inputs");

        let expected_steps = field(case, "steps")
            .as_array()
            .unwrap_or_else(|| panic!("{name}: 'steps'"));
        let input_steps = field(input, "steps")
            .as_array()
            .unwrap_or_else(|| panic!("{name}: input 'steps'"));
        assert_eq!(expected_steps.len(), input_steps.len(), "{name}: step count");

        let mut state = ViewerState::new();
        for (index, (expected_step, input_step)) in expected_steps.iter().zip(input_steps).enumerate() {
            let op = str_of(input_step, "op");
            assert_eq!(str_of(expected_step, "op"), op, "{name}: step {index} op");

            let outcome = run_step(&mut state, input_step);
            let context = format!("{name}: step {index} ({op})");
            check_step(&state, &outcome, field(expected_step, "expect"), &context);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// golden-viewer-step-zoom
// ---------------------------------------------------------------------------------------------------------------------

// C#: ViewerState.CalculateStepZoom (internal static), 33 cases; `expected` null means "no change" (None).
#[test]
fn golden_viewer_step_zoom() {
    let golden = golden::load("viewer-step-zoom");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let current = num_of(case, "current");
        let step = num_of(case, "step");

        let actual = calculate_step_zoom(current, step);

        assert_same_opt(
            actual,
            field(case, "expected"),
            &format!("{name}: CalculateStepZoom({current}, {step})"),
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// golden-viewer-display-size
// ---------------------------------------------------------------------------------------------------------------------

// C#: ViewerState.CalculateDisplaySize (public static), 17 cases.
#[test]
fn golden_viewer_display_size() {
    let golden = golden::load("viewer-display-size");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let width = int_of(case, "width");
        let height = int_of(case, "height");
        let zoom = num_of(case, "zoom");
        let dpi_scale = num_of(case, "dpi_scale");

        let (actual_width, actual_height) = calculate_display_size(width, height, zoom, dpi_scale);

        assert_same(actual_width, num_of(case, "expected_width"), &format!("{name}: width"));
        assert_same(
            actual_height,
            num_of(case, "expected_height"),
            &format!("{name}: height"),
        );
    }
}
