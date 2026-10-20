//! Golden runner (W8-05e, test porter) for the two gesture goldens written by the oracle at baseline `d92d6aae`:
//! `tests/golden/mouse-gestures.json` (235 cases) and `tests/golden/touchpad-gestures.json` (158 cases). Every case of both
//! files is run; no case is skipped. The per-kind case counts are asserted, so a short golden fails loudly.
//!
//! Number convention: a JSON number, or the strings "NaN", "Infinity", "-Infinity" and "-0". Doubles are compared bit for
//! bit (NaN matches any NaN, since a NaN payload is not part of the contract). Enum outputs are compared by their C# name.
//!
//! Known golden shape notes: the `pan_offset` cases do not store the input's horizontal flag as a field, only in the case
//! `name` (`horizontal=True` / `horizontal=False`); the runner reads it from there and panics when the name has neither.
//! Unknown `kind` values panic with the case name, so a missing kind cannot be skipped silently.
//!
//! Every test here fails through `todo!()` until the bodies land. A failure message names the case.

use std::panic::{self, AssertUnwindSafe};

use pr_app::input::mouse_gestures::{
    ClickZoomTarget, PointerReleaseAction, WheelGestureInterpreter, WheelOutcomeKind, classify_release,
    click_zoom_factor, decide_click_zoom, is_fit_double_click, resolve_keyboard_zoom_anchor, resolve_middle_click,
};
use pr_app::input::primitives::PointD;
use pr_app::input::touchpad_gestures::{
    TouchpadSwipeNavigator, TouchpadWheelAction, TouchpadWheelClassifier, WheelDeviceHint, WheelInput, is_within_gap,
    pan_offset_delta, route,
};
use pr_core::model::enums::{KeyboardZoomAnchor, MiddleClickAction, MouseWheelAction};
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

/// `case[key]` as an i32 (the golden stores every C# `int` as a JSON integer).
fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit in i32: {case}"))
}

/// `case[key]` as a JSON array; panics with the case printed when absent.
fn array_of<'a>(case: &'a Value, key: &str) -> &'a Vec<Value> {
    case[key]
        .as_array()
        .unwrap_or_else(|| panic!("case has no array '{key}': {case}"))
}

/// A golden string array (outcome names), compared by name.
fn names_of(case: &Value, key: &str) -> Vec<String> {
    array_of(case, key)
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("not a string in '{key}': {v}"))
                .to_owned()
        })
        .collect()
}

/// A golden integer array (per-step values, compared by value).
fn ints_of(case: &Value, key: &str) -> Vec<i32> {
    array_of(case, key)
        .iter()
        .map(|v| {
            let n = v.as_i64().unwrap_or_else(|| panic!("not an integer in '{key}': {v}"));
            i32::try_from(n).unwrap_or_else(|_| panic!("'{key}' does not fit in i32: {v}"))
        })
        .collect()
}

/// A golden string that may be JSON `null` (C# `string?`).
fn opt_str(value: &Value) -> Option<&str> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.as_str()),
        other => panic!("not a string or null: {other}"),
    }
}

/// Bit-exact double comparison; any NaN matches any NaN (the payload is not part of the contract). Keeps the sign of zero.
fn assert_same_double(actual: f64, expected: f64, what: &str) {
    if expected.is_nan() {
        assert!(actual.is_nan(), "{what}: expected NaN, got {actual}");
    } else {
        assert_eq!(actual.to_bits(), expected.to_bits(), "{what}: {actual} != {expected}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// C# names of the Rust enums (the golden compares by name)
// ---------------------------------------------------------------------------------------------------------------------

fn outcome_name(kind: WheelOutcomeKind) -> &'static str {
    match kind {
        WheelOutcomeKind::None => "None",
        WheelOutcomeKind::Zoom => "Zoom",
        WheelOutcomeKind::Next => "Next",
        WheelOutcomeKind::Previous => "Previous",
    }
}

fn release_name(action: PointerReleaseAction) -> &'static str {
    match action {
        PointerReleaseAction::None => "None",
        PointerReleaseAction::ClickZoom => "ClickZoom",
        PointerReleaseAction::StartKinetic => "StartKinetic",
    }
}

fn target_name(target: ClickZoomTarget) -> &'static str {
    match target {
        ClickZoomTarget::ClickZoom => "ClickZoom",
        ClickZoomTarget::Fit => "Fit",
    }
}

fn touchpad_action_name(action: TouchpadWheelAction) -> &'static str {
    match action {
        TouchpadWheelAction::MouseWheel => "MouseWheel",
        TouchpadWheelAction::Ignore => "Ignore",
        TouchpadWheelAction::Pan => "Pan",
        TouchpadWheelAction::Navigate => "Navigate",
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden name -> Rust input
// ---------------------------------------------------------------------------------------------------------------------

fn mouse_wheel_mode(name: &str) -> MouseWheelAction {
    match name {
        "Zoom" => MouseWheelAction::Zoom,
        "Navigate" => MouseWheelAction::Navigate,
        other => panic!("unknown MouseWheelAction {other:?}"),
    }
}

fn keyboard_anchor_mode(name: &str) -> KeyboardZoomAnchor {
    match name {
        "Pointer" => KeyboardZoomAnchor::Pointer,
        "ViewportCentre" => KeyboardZoomAnchor::ViewportCentre,
        other => panic!("unknown KeyboardZoomAnchor {other:?}"),
    }
}

fn middle_click_action(name: &str) -> MiddleClickAction {
    match name {
        "None" => MiddleClickAction::None,
        "ClickZoom" => MiddleClickAction::ClickZoom,
        "ActualSize" => MiddleClickAction::ActualSize,
        "Fit" => MiddleClickAction::Fit,
        "FitWidth" => MiddleClickAction::FitWidth,
        "FitWidth2" => MiddleClickAction::FitWidth2,
        "PreviousImage" => MiddleClickAction::PreviousImage,
        "NextImage" => MiddleClickAction::NextImage,
        "PreviousFolder" => MiddleClickAction::PreviousFolder,
        "NextFolder" => MiddleClickAction::NextFolder,
        "OpenFolder" => MiddleClickAction::OpenFolder,
        other => panic!("unknown MiddleClickAction {other:?}"),
    }
}

fn hint_of(step: &Value) -> WheelDeviceHint {
    match step.get("hint").and_then(Value::as_str) {
        None | Some("Unknown") => WheelDeviceHint::Unknown,
        Some("Touchpad") => WheelDeviceHint::Touchpad,
        Some(other) => panic!("unknown WheelDeviceHint {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// mouse-gestures.json
// ---------------------------------------------------------------------------------------------------------------------

fn check_mouse_case(case: &Value) {
    match str_of(case, "kind") {
        "wheel_sequence" => {
            let mode = mouse_wheel_mode(str_of(case, "mode"));
            let steps = array_of(case, "steps");
            let expected = names_of(case, "outcomes");
            assert_eq!(steps.len(), expected.len(), "step count");
            let mut wheel = WheelGestureInterpreter::new();
            for (i, (step, want)) in steps.iter().zip(&expected).enumerate() {
                let got = wheel.handle(i32_of(step, "delta"), bool_of(step, "ctrl"), mode);
                assert_eq!(outcome_name(got), want, "step {i}");
            }
        }
        "classify_release" => {
            let got = classify_release(
                bool_of(case, "dragged"),
                bool_of(case, "panned"),
                bool_of(case, "press_was_consumed"),
                bool_of(case, "click_to_zoom_enabled"),
                bool_of(case, "kinetic_pan_enabled"),
            );
            assert_eq!(release_name(got), str_of(case, "action"));
        }
        "fit_double_click" => {
            let got = is_fit_double_click(i32_of(case, "click_count"));
            assert_eq!(got, bool_of(case, "result"));
        }
        "click_zoom_target" => {
            let got = decide_click_zoom(
                bool_of(case, "is_fit"),
                num(&case["current_zoom"]),
                num(&case["click_zoom"]),
            );
            assert_eq!(target_name(got), str_of(case, "target"));
        }
        "click_zoom_factor" => {
            let got = click_zoom_factor(i32_of(case, "percent"));
            assert_same_double(got, num(&case["factor"]), "factor");
        }
        "keyboard_zoom_anchor" => {
            let pointer = match &case["pointer"] {
                Value::Null => None,
                p => Some(PointD {
                    x: num(&p["x"]),
                    y: num(&p["y"]),
                }),
            };
            let got = resolve_keyboard_zoom_anchor(
                keyboard_anchor_mode(str_of(case, "mode")),
                pointer,
                num(&case["viewport_width"]),
                num(&case["viewport_height"]),
            );
            assert_same_double(got.x, num(&case["anchor"]["x"]), "anchor.x");
            assert_same_double(got.y, num(&case["anchor"]["y"]), "anchor.y");
        }
        "middle_click" => {
            let got = resolve_middle_click(middle_click_action(str_of(case, "action")))
                .map(|command| format!("{:?}", command.kind));
            assert_eq!(got.as_deref(), opt_str(&case["command"]), "command");
        }
        other => panic!("unknown mouse-gestures kind {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// touchpad-gestures.json
// ---------------------------------------------------------------------------------------------------------------------

fn check_touchpad_case(case: &Value) {
    match str_of(case, "kind") {
        "classifier_sequence" => {
            let mut classifier = TouchpadWheelClassifier::new();
            let expected = array_of(case, "touchpad");
            let mut expected = expected.iter();
            for (i, step) in array_of(case, "steps").iter().enumerate() {
                match str_of(step, "op") {
                    "handle" => {
                        let got =
                            classifier.is_touchpad(i32_of(step, "delta"), i32_of(step, "timestamp"), hint_of(step));
                        let want = expected
                            .next()
                            .unwrap_or_else(|| panic!("step {i}: no expected touchpad flag"))
                            .as_bool()
                            .unwrap_or_else(|| panic!("step {i}: touchpad flag is not a bool"));
                        assert_eq!(got, want, "step {i}");
                    }
                    "reset" => classifier.reset(),
                    other => panic!("unknown classifier op {other:?}"),
                }
            }
            assert!(
                expected.next().is_none(),
                "more expected touchpad flags than handle steps"
            );
        }
        "within_gap" => {
            let got = is_within_gap(i32_of(case, "last"), i32_of(case, "now"));
            assert_eq!(got, bool_of(case, "within"));
        }
        "navigator_sequence" => {
            let mut navigator = TouchpadSwipeNavigator::new();
            let expected = names_of(case, "outcomes");
            let totals = ints_of(case, "gesture_total");
            let images = ints_of(case, "gesture_images");
            let mut expected = expected.iter();
            let mut handled = 0usize;
            for (i, step) in array_of(case, "steps").iter().enumerate() {
                match str_of(step, "op") {
                    "handle" => {
                        let got = navigator.handle(
                            i32_of(step, "delta"),
                            i32_of(step, "timestamp"),
                            i32_of(step, "distance"),
                        );
                        let want = expected
                            .next()
                            .unwrap_or_else(|| panic!("step {i}: no expected outcome"));
                        assert_eq!(outcome_name(got), want, "step {i}");
                        // The oracle records the swipe totals after every handle step (per-step arrays, same length as outcomes).
                        assert_eq!(navigator.gesture_total(), totals[handled], "step {i}: gesture_total");
                        assert_eq!(navigator.gesture_images(), images[handled], "step {i}: gesture_images");
                        handled += 1;
                    }
                    "reset" => navigator.reset(),
                    other => panic!("unknown navigator op {other:?}"),
                }
            }
            assert!(expected.next().is_none(), "more expected outcomes than handle steps");
            assert_eq!(handled, totals.len(), "gesture_total entries per handle step");
            assert_eq!(handled, images.len(), "gesture_images entries per handle step");
        }
        "route" => {
            let input = WheelInput::new(
                i32_of(case, "delta"),
                bool_of(case, "horizontal"),
                bool_of(case, "ctrl"),
                0,
            );
            let got = route(
                input,
                bool_of(case, "enabled"),
                bool_of(case, "is_touchpad"),
                bool_of(case, "can_pan"),
            );
            assert_eq!(touchpad_action_name(got), str_of(case, "action"));
        }
        "pan_offset" => {
            // The input orientation is only in the case name (see the module doc).
            let name = str_of(case, "name");
            let horizontal_input = if name.contains("horizontal=True") {
                true
            } else if name.contains("horizontal=False") {
                false
            } else {
                panic!("pan_offset case name does not state its orientation: {name:?}")
            };
            let input = WheelInput::new(i32_of(case, "delta"), horizontal_input, false, 0);
            let (dx, dy) = pan_offset_delta(input);
            assert_same_double(dx, num(&case["horizontal"]), "dx");
            assert_same_double(dy, num(&case["vertical"]), "dy");
        }
        other => panic!("unknown touchpad-gestures kind {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Runner: every case, each failure names the case
// ---------------------------------------------------------------------------------------------------------------------

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "non-string panic".to_owned())
}

fn run_cases(cases: &[Value], check: fn(&Value)) -> usize {
    let mut failures = Vec::new();
    for case in cases {
        let name = str_of(case, "name").to_owned();
        if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| check(case))) {
            failures.push(format!("case {name:?}: {}", panic_text(payload.as_ref())));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} golden cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    cases.len()
}

#[test]
fn golden_mouse_gestures_all_cases_match_the_oracle() {
    // C#: oracle-cs mouse-gestures (235 cases: wheel 19, classify_release 32, fit_double_click 7, click_zoom_target 120,
    // click_zoom_factor 14, keyboard_zoom_anchor 32, middle_click 11)
    let golden = golden::load("mouse-gestures");
    assert_eq!(golden.cases.len(), 235, "case count of mouse-gestures");
    let count = |kind: &str| golden.cases.iter().filter(|c| c["kind"] == kind).count();
    assert_eq!(count("wheel_sequence"), 19, "wheel_sequence cases");
    assert_eq!(count("classify_release"), 32, "classify_release cases");
    assert_eq!(count("fit_double_click"), 7, "fit_double_click cases");
    assert_eq!(count("click_zoom_target"), 120, "click_zoom_target cases");
    assert_eq!(count("click_zoom_factor"), 14, "click_zoom_factor cases");
    assert_eq!(count("keyboard_zoom_anchor"), 32, "keyboard_zoom_anchor cases");
    assert_eq!(count("middle_click"), 11, "middle_click cases");

    let ran = run_cases(&golden.cases, check_mouse_case);
    assert_eq!(ran, 235);
}

#[test]
fn golden_touchpad_gestures_all_cases_match_the_oracle() {
    // C#: oracle-cs touchpad-gestures (158 cases: classifier_sequence 12, within_gap 14, navigator_sequence 84,
    // route 32, pan_offset 16)
    let golden = golden::load("touchpad-gestures");
    assert_eq!(golden.cases.len(), 158, "case count of touchpad-gestures");
    let count = |kind: &str| golden.cases.iter().filter(|c| c["kind"] == kind).count();
    assert_eq!(count("classifier_sequence"), 12, "classifier_sequence cases");
    assert_eq!(count("within_gap"), 14, "within_gap cases");
    assert_eq!(count("navigator_sequence"), 84, "navigator_sequence cases");
    assert_eq!(count("route"), 32, "route cases");
    assert_eq!(count("pan_offset"), 16, "pan_offset cases");

    let ran = run_cases(&golden.cases, check_touchpad_case);
    assert_eq!(ran, 158);
}
