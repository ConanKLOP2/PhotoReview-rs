//! Golden tests (W8-05f) for `pr_app::input::kinetic_pan`.
//!
//! Reads `tests/golden/<command>.json` (written by `oracle-cs` at baseline `d92d6aae`, `KineticPan.cs`) and the
//! matching inputs in `tests/golden-inputs/<command>.json` (joined by case `name`). Every case is checked; mismatches
//! are collected and reported together. Doubles are compared bit for bit (`to_bits`), so `-0` must be `-0` and NaN
//! must be NaN (the golden writes `"-0"`, `"NaN"`, `"Infinity"`, `"-Infinity"` as strings).
//! C#: KineticPanTests / KineticPanMutationGapTests / KineticScrollerGapTests / GlideFrameClockTests (golden twin).
//! The bodies are `todo!()`, so every test fails until the port lands.

use std::collections::HashMap;

use pr_app::input::kinetic_pan::{
    GlideFrameClock, KeyboardPanResult, KineticScroller, PanVelocityTracker, ScrollBounds, keyboard_pan_consumes_key,
    keyboard_pan_impulse_velocity, keyboard_pan_is_zoomed, keyboard_pan_step, keyboard_pan_step_pixels,
};
use pr_core::abstractions::DisplayTiming;
use pr_core::model::enums::KineticGlideSmoothing;
use pr_test_support::golden::{load, repo_root, str_of};
use serde_json::Value;

/// `tests/golden-inputs/<command>.json`, keyed by case name.
fn inputs(command: &str) -> HashMap<String, Value> {
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{command}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: no cases", path.display()))
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

fn input<'a>(inputs: &'a HashMap<String, Value>, name: &str) -> &'a Value {
    inputs
        .get(name)
        .unwrap_or_else(|| panic!("no golden input named '{name}'"))
}

/// A golden number: a JSON number, or one of the special spellings the oracle writes as strings.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().expect("golden number fits f64"),
        Value::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => other
                .parse()
                .unwrap_or_else(|_| panic!("golden: '{other}' is not a number spelling")),
        },
        other => panic!("golden: expected a number, got {other}"),
    }
}

fn int(value: &Value) -> i64 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("golden: expected an integer, got {value}"))
}

fn bounds(value: &Value) -> ScrollBounds {
    let a = value
        .as_array()
        .unwrap_or_else(|| panic!("golden: bounds must be an array, got {value}"));
    ScrollBounds::new(num(&a[0]), num(&a[1]), num(&a[2]), num(&a[3]))
}

fn result(value: &Value) -> KeyboardPanResult {
    match value.as_str() {
        Some("Panned") => KeyboardPanResult::Panned,
        Some("AtEdge") => KeyboardPanResult::AtEdge,
        Some("NotScrollable") => KeyboardPanResult::NotScrollable,
        other => panic!("golden: unknown keyboard pan result {other:?}"),
    }
}

/// Collects every mismatch, then fails once with all of them.
#[derive(Default)]
struct Mismatches(Vec<String>);

impl Mismatches {
    fn number(&mut self, case: &str, what: &str, actual: f64, expected: &Value) {
        let want = num(expected);
        let same = if want.is_nan() {
            actual.is_nan()
        } else {
            actual.to_bits() == want.to_bits()
        };
        if !same {
            self.0
                .push(format!("{case}: {what}: expected {expected}, got {actual:?}"));
        }
    }

    fn flag(&mut self, case: &str, what: &str, actual: bool, expected: &Value) {
        if expected.as_bool() != Some(actual) {
            self.0
                .push(format!("{case}: {what}: expected {expected}, got {actual}"));
        }
    }

    fn finish(self, command: &str) {
        assert!(
            self.0.is_empty(),
            "{command}: {} mismatch(es) against the C# oracle:\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

fn feed(tracker: &mut PanVelocityTracker, samples: &Value) {
    for sample in samples.as_array().expect("golden: samples is an array") {
        let s = sample.as_array().expect("golden: a sample is [time, x, y]");
        tracker.add(num(&s[0]), num(&s[1]), num(&s[2]));
    }
}

// C#: KineticPanTests.Velocity_* and KineticPanMutationGapTests.GetVelocity_* (golden twin, command kinetic-velocity)
#[test]
fn golden_kinetic_velocity() {
    let golden = load("kinetic-velocity");
    let inputs = inputs("kinetic-velocity");
    let mut m = Mismatches::default();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = input(&inputs, name);
        let mut tracker = PanVelocityTracker::new();
        feed(&mut tracker, &input["samples"]);
        match str_of(input, "kind") {
            "reset" => tracker.reset(),
            "reset_then_velocity" => {
                tracker.reset();
                feed(&mut tracker, &input["after"]);
            }
            "velocity" => {}
            other => panic!("golden: unknown kinetic-velocity kind '{other}'"),
        }

        m.number(name, "count", tracker.count() as f64, &case["count"]);
        let (vx, vy) = tracker.velocity();
        m.number(name, "vx", vx, &case["vx"]);
        m.number(name, "vy", vy, &case["vy"]);
    }
    m.finish("kinetic-velocity");
}

// C#: KineticPanTests.Start_*, AddImpulse_*, Step_*, Stop_* and KineticScrollerGapTests (golden twin, command kinetic-scroller)
#[test]
fn golden_kinetic_scroller() {
    let golden = load("kinetic-scroller");
    let inputs = inputs("kinetic-scroller");
    let mut m = Mismatches::default();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = input(&inputs, name);
        let mut scroller = KineticScroller::default();

        match str_of(case, "kind") {
            "start" => {
                let ret = scroller.start(num(&input["vx"]), num(&input["vy"]));
                m.flag(name, "ret", ret, &case["ret"]);
                m.flag(name, "active", scroller.is_active(), &case["active"]);
                m.number(name, "vx", scroller.velocity_x(), &case["vx"]);
                m.number(name, "vy", scroller.velocity_y(), &case["vy"]);
            }
            "script" => {
                let ops = input["ops"].as_array().expect("golden: ops is an array");
                let results = case["results"].as_array().expect("golden: results is an array");
                assert_eq!(ops.len(), results.len(), "{name}: one result per op");
                for (op, res) in ops.iter().zip(results) {
                    match str_of(op, "op") {
                        "start" => {
                            let ret = scroller.start(num(&op["vx"]), num(&op["vy"]));
                            if res.get("ret").is_some() {
                                m.flag(name, "ret", ret, &res["ret"]);
                            }
                        }
                        "stop" => scroller.stop(),
                        "impulse" => scroller.add_impulse(num(&op["vx"]), num(&op["vy"])),
                        "step" => {
                            let (h, v) =
                                scroller.step(num(&op["elapsed"]), num(&op["h"]), num(&op["v"]), bounds(&op["bounds"]));
                            if res.get("h").is_some() {
                                m.number(name, "h", h, &res["h"]);
                            }
                            if res.get("v").is_some() {
                                m.number(name, "v", v, &res["v"]);
                            }
                        }
                        other => panic!("golden: unknown kinetic-scroller op '{other}'"),
                    }
                    if res.get("vx").is_some() {
                        m.number(name, "vx", scroller.velocity_x(), &res["vx"]);
                    }
                    if res.get("vy").is_some() {
                        m.number(name, "vy", scroller.velocity_y(), &res["vy"]);
                    }
                    if res.get("active").is_some() {
                        m.flag(name, "active", scroller.is_active(), &res["active"]);
                    }
                }
            }
            other => panic!("golden: unknown kinetic-scroller kind '{other}'"),
        }
    }
    m.finish("kinetic-scroller");
}

// C#: KeyboardPan.* tests in KineticPanTests.cs and KineticPanMutationGapTests.cs (golden twin, command keyboard-pan)
#[test]
fn golden_keyboard_pan() {
    let golden = load("keyboard-pan");
    let inputs = inputs("keyboard-pan");
    let mut m = Mismatches::default();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = input(&inputs, name);
        match str_of(case, "kind") {
            "step_pixels" => {
                let value = keyboard_pan_step_pixels(
                    num(&input["viewport_width"]),
                    num(&input["viewport_height"]),
                    num(&input["step_fraction"]),
                );
                m.number(name, "value", value, &case["value"]);
            }
            "step" => {
                let dx = int(&input["dx"]) as i32;
                let dy = int(&input["dy"]) as i32;
                let (actual, h, v) = keyboard_pan_step(
                    dx,
                    dy,
                    num(&input["h"]),
                    num(&input["v"]),
                    bounds(&input["bounds"]),
                    num(&input["step_fraction"]),
                );
                if result(&case["result"]) != actual {
                    m.0.push(format!("{name}: result: expected {}, got {actual:?}", case["result"]));
                }
                m.number(name, "h", h, &case["h"]);
                m.number(name, "v", v, &case["v"]);
            }
            "is_zoomed" => {
                m.flag(
                    name,
                    "value",
                    keyboard_pan_is_zoomed(bounds(&input["bounds"])),
                    &case["value"],
                );
            }
            "impulse" => {
                let (vx, vy) = keyboard_pan_impulse_velocity(num(&input["step_x"]), num(&input["step_y"]));
                m.number(name, "vx", vx, &case["vx"]);
                m.number(name, "vy", vy, &case["vy"]);
            }
            "consumes" => {
                let consumed = keyboard_pan_consumes_key(
                    result(&input["result"]),
                    input["is_repeat"].as_bool().expect("golden: is_repeat is a bool"),
                    input["navigates_at_edge"]
                        .as_bool()
                        .expect("golden: navigates_at_edge is a bool"),
                    bounds(&input["bounds"]),
                );
                m.flag(name, "value", consumed, &case["value"]);
            }
            other => panic!("golden: unknown keyboard-pan kind '{other}'"),
        }
    }
    m.finish("keyboard-pan");
}

// C#: GlideFrameClockTests.* and KineticPanMutationGapTests.GlideFrameClock tests (golden twin, command glide-clock)
#[test]
fn golden_glide_clock() {
    let golden = load("glide-clock");
    let inputs = inputs("glide-clock");
    let mut m = Mismatches::default();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = input(&inputs, name);
        let ops = input["ops"].as_array().expect("golden: ops is an array");
        let results = case["results"].as_array().expect("golden: results is an array");
        assert_eq!(ops.len(), results.len(), "{name}: one result per op");

        let mut clock = GlideFrameClock::new();
        for (op, res) in ops.iter().zip(results) {
            match str_of(op, "op") {
                "start" => {
                    let mode = match str_of(op, "mode") {
                        "Off" => KineticGlideSmoothing::Off,
                        "Predict" => KineticGlideSmoothing::Predict,
                        other => panic!("golden: unknown glide mode '{other}'"),
                    };
                    clock.start(mode, num(&op["ticks_per_ms"]));
                }
                "advance" => {
                    let timing = if op["timing"].is_null() {
                        None
                    } else {
                        Some(DisplayTiming {
                            last_vblank: int(&op["timing"]["last_vblank"]),
                            refresh_period: int(&op["timing"]["refresh_period"]),
                        })
                    };
                    let ret = clock.advance(num(&op["rendering_ms"]), int(&op["now"]), timing);
                    if res.get("ret").is_some() {
                        m.number(name, "ret", ret, &res["ret"]);
                    }
                }
                other => panic!("golden: unknown glide-clock op '{other}'"),
            }
            if res.get("needs").is_some() {
                m.flag(name, "needs", clock.needs_display_timing(), &res["needs"]);
            }
        }
    }
    m.finish("glide-clock");
}
