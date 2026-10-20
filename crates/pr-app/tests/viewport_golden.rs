//! Golden-driven red tests (W8-01a, test porter) for the four goldens of the viewport contracts:
//! `viewport-state` (scripted layout engine, every step checked), `arrow-pan`, `viewport-snapshot-text` and
//! `viewport-values`. Oracle: `tools/oracle-cs` (`W8-01a.cs`) at baseline `d92d6aae`. Every case is run; no case is skipped.
//!
//! Number convention (`oracle/W8-01a.cs`): a JSON number, or the strings "NaN", "Infinity", "-Infinity" and "-0".
//! Comparisons are bit-exact for doubles: NaN matches NaN, and the sign of zero counts (trap T3, T5).
//!
//! Input note: the `arrow-pan` golden stores its OUTPUT in the `horizontal` and `vertical` keys of each `step` case (the
//! input offsets are only in `tests/golden-inputs/arrow-pan.json`), so the `step` cases read their inputs from that file.
//! The other three goldens echo their inputs, so they are read from `tests/golden` alone.
//!
//! Every test here fails through `todo!()` until the bodies land.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use pr_app::input::primitives::{PointD, RectD};
use pr_app::viewport::arrow_pan::{ArrowPanOutcome, DEFAULT_STEP_FRACTION, step, step_length};
use pr_app::viewport::state::{LayoutEngine, ViewportState};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout, ViewportSnapshot};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
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

fn rect_of(value: &Value) -> RectD {
    RectD {
        x: num_of(value, "x"),
        y: num_of(value, "y"),
        width: num_of(value, "width"),
        height: num_of(value, "height"),
    }
}

fn point_of(value: &Value) -> PointD {
    PointD {
        x: num_of(value, "x"),
        y: num_of(value, "y"),
    }
}

fn layout_of(value: &Value) -> ViewportLayout {
    ViewportLayout {
        viewport_width: num_of(value, "viewport_width"),
        viewport_height: num_of(value, "viewport_height"),
        extent_width: num_of(value, "extent_width"),
        extent_height: num_of(value, "extent_height"),
        horizontal_bar_visible: bool_of(value, "horizontal_bar_visible"),
        vertical_bar_visible: bool_of(value, "vertical_bar_visible"),
        image_rect: rect_of(field(value, "image_rect")),
        max_horizontal_offset: num_of(value, "max_horizontal_offset"),
        max_vertical_offset: num_of(value, "max_vertical_offset"),
    }
}

fn scroll_bars_of(name: &str) -> ScrollBarPolicy {
    match name {
        "Auto" => ScrollBarPolicy::Auto,
        "Overlay" => ScrollBarPolicy::Overlay,
        "Hidden" => ScrollBarPolicy::Hidden,
        other => panic!("unknown scroll bar policy {other:?}"),
    }
}

fn stretch_of(name: &str) -> ViewerStretchMode {
    match name {
        "None" => ViewerStretchMode::None,
        "Uniform" => ViewerStretchMode::Uniform,
        other => panic!("unknown stretch mode {other:?}"),
    }
}

fn input_of(value: &Value) -> ViewportInput {
    ViewportInput {
        client_width: num_of(value, "client_width"),
        client_height: num_of(value, "client_height"),
        scroll_bar_thickness: num_of(value, "scroll_bar_thickness"),
        scroll_bars: scroll_bars_of(str_of(value, "scroll_bars")),
        stretch: stretch_of(str_of(value, "stretch")),
        image_width: num_of(value, "image_width"),
        image_height: num_of(value, "image_height"),
        max_image_width: num_of(value, "max_image_width"),
        max_image_height: num_of(value, "max_image_height"),
        bitmap_width: num_of(value, "bitmap_width"),
        bitmap_height: num_of(value, "bitmap_height"),
    }
}

fn outcome_name(outcome: ArrowPanOutcome) -> &'static str {
    match outcome {
        ArrowPanOutcome::NotScrollable => "NotScrollable",
        ArrowPanOutcome::AtEdge => "AtEdge",
        ArrowPanOutcome::Panned => "Panned",
    }
}

/// `tests/golden-inputs/<name>.json` `cases` (the inputs of a golden that stores its outputs over them).
fn golden_inputs(name: &str) -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

// ---------------------------------------------------------------------------------------------------------------------
// Assertions (bit-exact doubles: NaN equals NaN, the sign of zero matters)
// ---------------------------------------------------------------------------------------------------------------------

fn check_f64(actual: f64, expected: f64, context: &str) {
    let same = if expected.is_nan() {
        actual.is_nan()
    } else {
        actual.to_bits() == expected.to_bits()
    };
    assert!(same, "{context}: expected {expected:?}, got {actual:?}");
}

fn check_bool(actual: bool, expected: bool, context: &str) {
    assert_eq!(actual, expected, "{context}");
}

fn check_point(actual: PointD, expected: &Value, context: &str) {
    check_f64(actual.x, num_of(expected, "x"), &format!("{context} x"));
    check_f64(actual.y, num_of(expected, "y"), &format!("{context} y"));
}

fn check_rect(actual: &RectD, expected: &Value, context: &str) {
    check_f64(actual.x, num_of(expected, "x"), &format!("{context} rect.x"));
    check_f64(actual.y, num_of(expected, "y"), &format!("{context} rect.y"));
    check_f64(
        actual.width,
        num_of(expected, "width"),
        &format!("{context} rect.width"),
    );
    check_f64(
        actual.height,
        num_of(expected, "height"),
        &format!("{context} rect.height"),
    );
}

fn check_layout(actual: &ViewportLayout, expected: &Value, context: &str) {
    check_f64(
        actual.viewport_width,
        num_of(expected, "viewport_width"),
        &format!("{context} viewport_width"),
    );
    check_f64(
        actual.viewport_height,
        num_of(expected, "viewport_height"),
        &format!("{context} viewport_height"),
    );
    check_f64(
        actual.extent_width,
        num_of(expected, "extent_width"),
        &format!("{context} extent_width"),
    );
    check_f64(
        actual.extent_height,
        num_of(expected, "extent_height"),
        &format!("{context} extent_height"),
    );
    check_bool(
        actual.horizontal_bar_visible,
        bool_of(expected, "horizontal_bar_visible"),
        &format!("{context} horizontal_bar_visible"),
    );
    check_bool(
        actual.vertical_bar_visible,
        bool_of(expected, "vertical_bar_visible"),
        &format!("{context} vertical_bar_visible"),
    );
    check_rect(
        &actual.image_rect,
        field(expected, "image_rect"),
        &format!("{context} image_rect"),
    );
    check_f64(
        actual.max_horizontal_offset,
        num_of(expected, "max_horizontal_offset"),
        &format!("{context} max_horizontal_offset"),
    );
    check_f64(
        actual.max_vertical_offset,
        num_of(expected, "max_vertical_offset"),
        &format!("{context} max_vertical_offset"),
    );
}

/// Checks the `expect.state` object of a step against the state after that step.
fn check_state<E: LayoutEngine>(state: &ViewportState<E>, expected: &Value, context: &str) {
    check_bool(
        state.is_layout_pending(),
        bool_of(expected, "is_layout_pending"),
        &format!("{context} is_layout_pending"),
    );
    check_layout(&state.layout(), field(expected, "layout"), &format!("{context} layout"));
    check_f64(
        state.horizontal_offset(),
        num_of(expected, "horizontal_offset"),
        &format!("{context} horizontal_offset"),
    );
    check_f64(
        state.vertical_offset(),
        num_of(expected, "vertical_offset"),
        &format!("{context} vertical_offset"),
    );
    check_point(
        state.image_origin(),
        field(expected, "image_origin"),
        &format!("{context} image_origin"),
    );
    check_f64(
        state.image_actual_width(),
        num_of(expected, "image_actual_width"),
        &format!("{context} image_actual_width"),
    );
    check_f64(
        state.image_actual_height(),
        num_of(expected, "image_actual_height"),
        &format!("{context} image_actual_height"),
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Scripted engine: the layouts and clamps come from the golden, in call order
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Script {
    computes: VecDeque<ViewportLayout>,
    clamps: VecDeque<(f64, f64)>,
    compute_calls: usize,
    clamp_calls: usize,
}

/// A `LayoutEngine` that returns what the golden step expects. A call the golden does not expect panics.
#[derive(Debug, Clone)]
struct ScriptedEngine(Rc<RefCell<Script>>);

impl LayoutEngine for ScriptedEngine {
    fn compute(&self, _input: &ViewportInput) -> ViewportLayout {
        let mut script = self.0.borrow_mut();
        script.compute_calls += 1;
        script
            .computes
            .pop_front()
            .expect("unexpected engine call: compute (the golden does not expect it)")
    }

    fn clamp_offset(&self, _layout: &ViewportLayout, _horizontal: f64, _vertical: f64) -> (f64, f64) {
        let mut script = self.0.borrow_mut();
        script.clamp_calls += 1;
        script
            .clamps
            .pop_front()
            .expect("unexpected engine call: clamp_offset (the golden does not expect it)")
    }
}

fn engine_calls(script: &Rc<RefCell<Script>>) -> (usize, usize) {
    let script = script.borrow();
    (script.compute_calls, script.clamp_calls)
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden tests
// ---------------------------------------------------------------------------------------------------------------------

// Golden `viewport-state`: 11 scripts over `ViewportState` (oracle: `ViewportState` by reflection, D38 rule).
// Each `update_layout` expects `returned`, `compute_called`, `clamp_called`, `compute`, `clamp`; each step expects `state`;
// a `query` with `probe` also expects `state.to_image`.
// New test: this file is the golden of W8-01a (contract "Tests", `golden_viewport_state`).
#[test]
fn golden_viewport_state() {
    let golden = golden::load("viewport-state");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let script = Rc::new(RefCell::new(Script::default()));
        let mut state = ViewportState::new(ScriptedEngine(Rc::clone(&script)));

        let steps = case["steps"].as_array().unwrap_or_else(|| panic!("{name}: no steps"));
        for (index, step_value) in steps.iter().enumerate() {
            let context = format!("{name} / step {index}");
            let op = str_of(step_value, "op");
            let expect = &step_value["expect"];
            let calls_before_step = engine_calls(&script);

            match op {
                "set_input" => state.set_input(input_of(field(step_value, "input"))),
                "scroll_to" => state.scroll_to(num_of(step_value, "horizontal"), num_of(step_value, "vertical")),
                "scroll_home" => state.scroll_home(),
                "update_layout" => {
                    let before = engine_calls(&script);
                    let compute_called = bool_of(expect, "compute_called");
                    let clamp_called = bool_of(expect, "clamp_called");
                    if compute_called {
                        script
                            .borrow_mut()
                            .computes
                            .push_back(layout_of(field(expect, "compute")));
                    }
                    if clamp_called {
                        let pair = field(expect, "clamp")
                            .as_array()
                            .unwrap_or_else(|| panic!("{context}: clamp is not a pair"));
                        script.borrow_mut().clamps.push_back((num(&pair[0]), num(&pair[1])));
                    }

                    let returned = state.update_layout();
                    check_bool(returned, bool_of(expect, "returned"), &format!("{context} returned"));

                    let after = engine_calls(&script);
                    assert_eq!(
                        after.0 - before.0,
                        usize::from(compute_called),
                        "{context}: compute calls"
                    );
                    assert_eq!(after.1 - before.1, usize::from(clamp_called), "{context}: clamp calls");
                    assert!(
                        script.borrow().computes.is_empty() && script.borrow().clamps.is_empty(),
                        "{context}: the golden expected an engine call that did not happen"
                    );
                }
                "query" => {
                    if let Some(probe) = step_value.get("probe") {
                        check_point(
                            state.to_image_element(point_of(probe)),
                            field(&expect["state"], "to_image"),
                            &format!("{context} to_image"),
                        );
                    }
                }
                other => panic!("{context}: unknown op {other:?}"),
            }

            if op != "update_layout" {
                assert_eq!(
                    engine_calls(&script),
                    calls_before_step,
                    "{context}: {op} must not call the engine"
                );
            }
            check_state(&state, &expect["state"], &context);
        }
    }
}

// Golden `arrow-pan`: 26 cases (`constant`, `step_length`, `step`). The `step` cases read their inputs from
// `tests/golden-inputs/arrow-pan.json` (see the module note). New test: the golden is W8-01a's (contract "Tests").
#[test]
fn golden_arrow_pan() {
    let golden = golden::load("arrow-pan");
    let inputs = golden_inputs("arrow-pan");
    assert_eq!(
        inputs.len(),
        golden.cases.len(),
        "inputs and golden outputs have the same number of cases"
    );

    for (case, input) in golden.cases.iter().zip(&inputs) {
        let name = str_of(case, "name");
        assert_eq!(name, str_of(input, "name"), "inputs and outputs are in the same order");

        match str_of(case, "kind") {
            "constant" => check_f64(DEFAULT_STEP_FRACTION, num_of(case, "value"), name),
            "step_length" => check_f64(
                step_length(
                    num_of(case, "viewport_width"),
                    num_of(case, "viewport_height"),
                    num_of(case, "step_fraction"),
                ),
                num_of(case, "value"),
                name,
            ),
            "step" => {
                let dx = i32::try_from(i64_of(input, "dx")).expect("dx fits i32");
                let dy = i32::try_from(i64_of(input, "dy")).expect("dy fits i32");
                let layout = layout_of(field(input, "layout"));
                let (outcome, horizontal, vertical) = step(
                    dx,
                    dy,
                    num_of(input, "horizontal"),
                    num_of(input, "vertical"),
                    &layout,
                    num_of(input, "step_fraction"),
                );
                assert_eq!(outcome_name(outcome), str_of(case, "outcome"), "{name}: outcome");
                check_f64(horizontal, num_of(case, "horizontal"), &format!("{name} horizontal"));
                check_f64(vertical, num_of(case, "vertical"), &format!("{name} vertical"));
            }
            other => panic!("{name}: unknown arrow-pan kind {other:?}"),
        }
    }
}

// Golden `viewport-snapshot-text`: 6 cases of `ViewportSnapshot.ToString()` (invariant culture, F0/F1/F2 rounding).
// New test: the golden is W8-01a's (contract "Tests", `golden_snapshot_text`).
#[test]
fn golden_snapshot_text() {
    let golden = golden::load("viewport-snapshot-text");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let snapshot = ViewportSnapshot {
            zoom: num_of(case, "zoom"),
            stretch: stretch_of(str_of(case, "stretch")),
            max_image_width: num_of(case, "max_image_width"),
            max_image_height: num_of(case, "max_image_height"),
            actual_image_width: num_of(case, "actual_image_width"),
            actual_image_height: num_of(case, "actual_image_height"),
            extent_width: num_of(case, "extent_width"),
            extent_height: num_of(case, "extent_height"),
            viewport_width: num_of(case, "viewport_width"),
            viewport_height: num_of(case, "viewport_height"),
            horizontal_offset: num_of(case, "horizontal_offset"),
            vertical_offset: num_of(case, "vertical_offset"),
            horizontal_scroll_bar_visible: bool_of(case, "horizontal_scroll_bar_visible"),
            vertical_scroll_bar_visible: bool_of(case, "vertical_scroll_bar_visible"),
        };
        assert_eq!(snapshot.to_string(), str_of(case, "text"), "{name}");
    }
}

// Golden `viewport-values`: 17 cases (`input_equals`, `layout_equals`, `rect_contains`, `rect_edges`).
// New test: the golden is W8-01a's (contract "Tests", `golden_viewport_values`).
#[test]
fn golden_viewport_values() {
    let golden = golden::load("viewport-values");
    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "kind") {
            "input_equals" => {
                let a = input_of(field(case, "a"));
                let b = input_of(field(case, "b"));
                assert_eq!(a == b, bool_of(case, "equal"), "{name}");
            }
            "layout_equals" => {
                let a = layout_of(field(case, "a"));
                let b = layout_of(field(case, "b"));
                assert_eq!(a == b, bool_of(case, "equal"), "{name}");
            }
            "rect_contains" => {
                let rect = rect_of(field(case, "rect"));
                let point = point_of(field(case, "point"));
                assert_eq!(rect.contains(point), bool_of(case, "contains"), "{name}");
            }
            "rect_edges" => {
                let rect = rect_of(field(case, "rect"));
                check_f64(rect.right(), num_of(case, "right"), &format!("{name} right"));
                check_f64(rect.bottom(), num_of(case, "bottom"), &format!("{name} bottom"));
            }
            other => panic!("{name}: unknown viewport-values kind {other:?}"),
        }
    }
}
