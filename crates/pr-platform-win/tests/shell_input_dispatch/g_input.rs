//! G-INPUT: replays every script of the C# golden `tests/fixtures/golden/input-scripts.v1.json` (recorded from the WPF `MainWindow`) through the three
//! registered message handlers of `ShellInput` and the REAL `ViewportController`, and diffs the checkpoints.
//!
//! Port of `tests/PhotoReview.Shell.Integration.Tests/Input/InputGoldenReplay.cs` and `InputGoldenShellTests.cs` (baseline `7eaee2a2`), on the `Rig`
//! (the twin of `InputRig`): the messages go through `InputWindow::dispatch`, the same handler chain a window procedure runs, with no HWND, so the test needs no
//! desktop and runs in CI. The OS window procedure path is covered by the ignored native tests of `native.rs`.
//! The window reports a DPI scale of 1.0 whatever the viewer's is, and its "screen" coordinates equal its client coordinates (C# `SizedShellWindow`).
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_app::input::primitives::{KeyModifiers, PointD, PointerButton};
use pr_app::input::review_command::ReviewCommand;
use pr_app::input::touchpad_gestures::WheelDeviceHint;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts, json};
use pr_platform_win::win32::messages::{
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};
use pr_test_support::golden;
use serde_json::Value;

use crate::fixture::{Rig, RigOptions};

const MK_LBUTTON: usize = 0x0001;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Checkpoint {
    after_step: i64,
    zoom: f64,
    is_fit: bool,
    horizontal_offset: f64,
    vertical_offset: f64,
    extent_width: f64,
    extent_height: f64,
    viewport_width: f64,
    viewport_height: f64,
    display_zoom_percent: i64,
}

fn number(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .unwrap_or_else(|| panic!("no number '{key}' in {value}"))
}

fn checkpoint_of(value: &Value) -> Checkpoint {
    Checkpoint {
        after_step: value["afterStep"].as_i64().expect("afterStep"),
        zoom: number(value, "zoom"),
        is_fit: value["isFit"].as_bool().expect("isFit"),
        horizontal_offset: number(value, "horizontalOffset"),
        vertical_offset: number(value, "verticalOffset"),
        extent_width: number(value, "extentWidth"),
        extent_height: number(value, "extentHeight"),
        viewport_width: number(value, "viewportWidth"),
        viewport_height: number(value, "viewportHeight"),
        display_zoom_percent: value["displayZoomPercent"].as_i64().expect("displayZoomPercent"),
    }
}

/// C# `Diff`: zoom 0.001, lengths 0.5 DIP, `IsFit` and `DisplayZoomPercent` exact.
fn diff(expected: &Checkpoint, actual: &Checkpoint) -> Vec<String> {
    let mut found = Vec::new();
    if (expected.zoom - actual.zoom).abs() > 0.001 || expected.zoom.is_nan() || actual.zoom.is_nan() {
        found.push(format!("Zoom {} != {}", expected.zoom, actual.zoom));
    }
    if expected.is_fit != actual.is_fit {
        found.push(format!("IsFit {} != {}", expected.is_fit, actual.is_fit));
    }
    if expected.display_zoom_percent != actual.display_zoom_percent {
        found.push(format!(
            "DisplayZoomPercent {} != {}",
            expected.display_zoom_percent, actual.display_zoom_percent
        ));
    }
    for (field, e, a) in [
        ("HorizontalOffset", expected.horizontal_offset, actual.horizontal_offset),
        ("VerticalOffset", expected.vertical_offset, actual.vertical_offset),
        ("ExtentWidth", expected.extent_width, actual.extent_width),
        ("ExtentHeight", expected.extent_height, actual.extent_height),
        ("ViewportWidth", expected.viewport_width, actual.viewport_width),
        ("ViewportHeight", expected.viewport_height, actual.viewport_height),
    ] {
        if (e - a).abs() > 0.5 || e.is_nan() || a.is_nan() {
            found.push(format!("{field} {e} != {a}"));
        }
    }
    found
}

fn has_modifier(modifiers: &Value, name: &str) -> bool {
    modifiers
        .as_str()
        .is_some_and(|text| text.split(',').map(str::trim).any(|part| part == name))
}

/// The golden writes `fitWidthAnchor`; `AppSettings` has `FitWidthAnchor` (System.Text.Json matched case-insensitively).
fn pascal(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// C# `ApplySettings`: defaults for every property, then each override; then the viewer's zoom step and scaling quality.
fn apply_settings(overrides_json: &str, rig: &Rig) {
    let texts = EnglishSettingsTexts;
    let mut settings = AppSettings::default();
    if !overrides_json.trim().is_empty() {
        let root = json::parse(overrides_json).expect("settingsOverridesJson parses");
        let json::JsonValue::Object(entries) = root else {
            panic!("settingsOverridesJson is not an object: {overrides_json}");
        };
        for (name, value) in entries {
            let name = pascal(&name.expect("property name"));
            let known = settings
                .apply_json_property(&name, &value, &texts)
                .unwrap_or_else(|error| panic!("override {name}: {error:?}"));
            assert!(known, "Unknown AppSettings property in golden setup: {name}");
        }
    }
    {
        let mut viewer = rig.viewer.borrow_mut();
        viewer.set_zoom_step(f64::from(settings.keyboard_zoom_step_percent) / 100.0);
        if let Some(quality) = settings.scaling_quality {
            let _ = viewer.set_scaling_quality(quality);
        }
    }
    *rig.settings.borrow_mut() = Arc::new(settings);
}

fn checkpoint(after_step: i64, rig: &Rig) -> Checkpoint {
    let viewer = rig.viewer.borrow();
    Checkpoint {
        after_step,
        zoom: viewer.zoom(),
        is_fit: viewer.is_fit(),
        horizontal_offset: rig.viewport.horizontal_offset(),
        vertical_offset: rig.viewport.vertical_offset(),
        extent_width: rig.viewport.extent_width(),
        extent_height: rig.viewport.extent_height(),
        viewport_width: rig.viewport.viewport_width(),
        viewport_height: rig.viewport.viewport_height(),
        display_zoom_percent: i64::from(viewer.display_zoom_percent()),
    }
}

/// The C# `Viewer.PropertyChanged += InvalidateLayout` hook (`ShellMainWindow.OnViewerChanged`; the `Rig` only invalidates through Render-priority posts): re-reads
/// the viewer into the content cache, invalidates the layout and settles. Harmless when nothing changed.
fn viewer_changed(rig: &Rig) {
    rig.refresh_content();
    rig.viewport.invalidate_layout();
    rig.drain();
}

fn unsupported(index: usize, step: &Value) -> String {
    format!("G-INPUT step {index} is not supported by the replay: {step}")
}

fn command(rig: &Rig, step: &Value, index: usize) -> Result<(), String> {
    let name = step["command"].as_str().unwrap_or_default();
    let x = step["x"].as_f64().unwrap_or(0.0);
    let y = step["y"].as_f64().unwrap_or(0.0);
    let delta = step["delta"].as_i64().unwrap_or(0) as i32;
    let execute = |command: ReviewCommand| {
        rig.input.execute(command);
        rig.drain();
    };
    match name {
        "ToggleFit" | "Fit" => execute(ReviewCommand::TOGGLE_FIT),
        "FitWidth" => execute(ReviewCommand::FIT_WIDTH),
        "FitWidth2" => execute(ReviewCommand::FIT_WIDTH2),
        "FitHeight" => execute(ReviewCommand::FIT_HEIGHT),
        "ZoomIn" => execute(ReviewCommand::ZOOM_IN),
        "ZoomOut" => execute(ReviewCommand::ZOOM_OUT),
        "ZoomActualSize" | "ActualSize" => execute(ReviewCommand::ZOOM_ACTUAL_SIZE),
        "ClickZoom" => execute(ReviewCommand::CLICK_ZOOM),
        "SetClickZoomLevel" => {
            rig.input.set_click_zoom_level(x as i32);
            rig.drain();
        }
        "ClickZoomLevel" => {
            rig.input.set_click_zoom_level(delta);
            rig.drain();
        }
        "ZoomToLevelMenu" => {
            let percent = rig.settings.borrow().click_zoom_percent;
            rig.input.set_click_zoom_level(percent);
            rig.drain();
        }
        "Next" | "Previous" => {} // the recorder only counts navigation; the viewport does not change
        "SetDpi" => {
            let _ = rig.viewer.borrow_mut().set_dpi_scale(x);
            viewer_changed(rig);
            rig.viewport.update_fit_size(); // MainWindow.UpdateFitSize: the DPI scale enters the Fit factor
            viewer_changed(rig);
        }
        "LoadImage" => {
            rig.input.note_current_index_changed(0);
            rig.content.bitmap.set((x.trunc(), y.trunc()));
            let _ = rig.viewer.borrow_mut().set_source_size(x as i32, y as i32, true);
            viewer_changed(rig);
            rig.viewport.update_layout();
            if rig.viewer.borrow().is_fit() {
                rig.viewport.update_fit_size();
                viewer_changed(rig);
            }
            rig.viewport.update_layout();
            rig.input.apply_initial_view();
            rig.drain();
        }
        "SwapSourceSize" => {
            rig.content.bitmap.set((x.trunc(), y.trunc()));
            // A pure call holds the viewer mutably and posts a job: no content refresh meanwhile (see the `Rig` notes).
            rig.content.suspended.set(true);
            let _ = rig
                .viewer
                .borrow_mut()
                .swap_source_size(x as i32, y as i32, &mut |before| {
                    rig.input.on_source_size_swapping(before)
                });
            rig.content.suspended.set(false);
            viewer_changed(rig);
            rig.viewport.update_layout();
        }
        _ => return Err(unsupported(index, step)),
    }
    Ok(())
}

/// C# `ApplyAsync`: returns the new "left button is down" flag.
fn apply(rig: &Rig, step: &Value, index: usize, left_down: bool) -> Result<bool, String> {
    let kind = step["kind"].as_str().unwrap_or_default();
    let x = step["x"].as_f64().unwrap_or(0.0);
    let y = step["y"].as_f64().unwrap_or(0.0);
    let (xi, yi) = (x as i32, y as i32);
    let delta = step["delta"].as_i64().unwrap_or(0) as i32;
    let time = step["timestampMs"].as_i64().unwrap_or(0) as i32;
    if matches!(kind, "wheel" | "hwheel" | "press" | "move" | "release") {
        rig.mouse.set(Some(PointD { x, y }));
        rig.message_time.set(time);
        rig.tick_count.set(time);
    }
    let ctrl = has_modifier(&step["modifiers"], "Control");
    match kind {
        "wheel" | "hwheel" => {
            rig.wheel_device.set(if step["key"].as_str() == Some("Touchpad") {
                WheelDeviceHint::Touchpad
            } else {
                WheelDeviceHint::Unknown
            });
            let message = if kind == "wheel" { WM_MOUSEWHEEL } else { WM_MOUSEHWHEEL };
            let _ = rig.wheel(message, delta, xi, yi, ctrl);
        }
        "press" => {
            // The golden fixes the click count (Delta): seed the chain as if Delta - 1 clicks had just happened.
            rig.input.reset_clicks();
            let middle = step["key"].as_str() == Some("Middle");
            let button = if middle {
                PointerButton::Middle
            } else {
                PointerButton::Left
            };
            if delta > 1 {
                rig.input.seed_clicks(button, xi, yi, time, delta - 1);
            }
            let _ = rig.mouse(if middle { WM_MBUTTONDOWN } else { WM_LBUTTONDOWN }, xi, yi, 0);
            return Ok(!middle || left_down);
        }
        "move" => {
            let _ = rig.mouse(WM_MOUSEMOVE, xi, yi, if left_down { MK_LBUTTON } else { 0 });
        }
        "release" => {
            let _ = rig.mouse(WM_LBUTTONUP, xi, yi, 0);
            return Ok(false);
        }
        "key" => {
            let key = step["key"]
                .as_str()
                .and_then(KeyId::from_name)
                .ok_or_else(|| unsupported(index, step))?;
            let mut modifiers = KeyModifiers::NONE;
            if ctrl {
                modifiers = modifiers | KeyModifiers::CONTROL;
            }
            if has_modifier(&step["modifiers"], "Shift") {
                modifiers = modifiers | KeyModifiers::SHIFT;
            }
            if has_modifier(&step["modifiers"], "Alt") {
                modifiers = modifiers | KeyModifiers::ALT;
            }
            rig.modifiers.set(modifiers);
            rig.message_time.set(time);
            rig.tick_count.set(time);
            let repeat = has_modifier(&step["modifiers"], "Repeat");
            let _ = rig.key_down_vk(key_mapping::to_virtual_key(key), repeat, false, false, 0);
        }
        "command" => command(rig, step, index)?,
        "frame" => {
            rig.timestamp
                .set((f64::from(time) * (pr_core::diagnostics::perf::STOPWATCH_FREQUENCY as f64 / 1000.0)) as i64);
            rig.clock.tick(f64::from(time));
            rig.drain();
        }
        "resize" => {
            rig.window.resize(x, y);
            rig.drain();
        }
        _ => return Err(unsupported(index, step)),
    }
    Ok(left_down)
}

/// C# `RunAsync`: the checkpoints the script asks for, or the reason the script cannot run.
fn run(script: &Value) -> Result<Vec<Checkpoint>, String> {
    let setup = &script["setup"];
    let rig = Rig::with(RigOptions {
        client_width: number(setup, "clientWidth"),
        client_height: number(setup, "clientHeight"),
        image_width: setup["imagePixelWidth"].as_i64().expect("imagePixelWidth") as i32,
        image_height: setup["imagePixelHeight"].as_i64().expect("imagePixelHeight") as i32,
        dpi_scale: number(setup, "dpiScale"),
        window_dpi_scale: Some(1.0),
        configure: None,
    });
    // C# `ContentFromViewer` reads the viewer on every call; the rig caches it, so make every read live (inside a layout pass the Fit size feeds back into it).
    {
        let content = Rc::clone(&rig.content);
        let viewer = rig.viewer.clone();
        *rig.content.before_read.borrow_mut() = Some(Rc::new(move || content.refresh(&viewer)));
    }
    // The WP-10 recorder: the first image is shown with the DEFAULT settings at Fit, THEN SettingsOverridesJson is applied.
    rig.input.apply_initial_view();
    viewer_changed(&rig);
    rig.fit();
    viewer_changed(&rig);
    apply_settings(setup["settingsOverridesJson"].as_str().unwrap_or(""), &rig);
    let settings = Arc::clone(&rig.settings.borrow());
    rig.input.rebuild_shortcuts(&settings);

    let wanted: Vec<i64> = script["expected"]
        .as_array()
        .expect("expected")
        .iter()
        .map(|c| c["afterStep"].as_i64().expect("afterStep"))
        .collect();
    let mut checkpoints = Vec::new();
    if wanted.contains(&-1) {
        checkpoints.push(checkpoint(-1, &rig));
    }
    let mut left_down = false;
    for (index, step) in script["steps"].as_array().expect("steps").iter().enumerate() {
        left_down = apply(&rig, step, index, left_down)?;
        viewer_changed(&rig);
        if wanted.contains(&(index as i64)) {
            checkpoints.push(checkpoint(index as i64, &rig));
        }
    }
    *rig.content.before_read.borrow_mut() = None; // breaks the rc cycle above
    rig.dispose();
    Ok(checkpoints)
}

fn load() -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join("input-scripts.v1.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(text.trim_start_matches('\u{feff}')).expect("input-scripts.v1.json parses")
}

// C# `InputGoldenShellTests.GInput_EveryRecordedScript_ReplaysThroughRealMessages_ToTheSameCheckpoints`.
#[test]
fn g_input_every_recorded_script_replays_through_real_messages() {
    let document = load();
    let scripts = document["items"].as_array().expect("items");
    assert!(
        scripts.len() >= 60,
        "G-INPUT needs >= 60 scripts (NO-WPF-EXEC-PLAN 7.3), the file has {}",
        scripts.len()
    );
    let mut failures = Vec::new();
    let mut compared = 0;
    for script in scripts {
        let name = script["name"].as_str().unwrap_or("?");
        let actual = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(script))) {
            Ok(Ok(actual)) => actual,
            Ok(Err(reason)) => {
                failures.push(format!("{name}: {reason}"));
                continue;
            }
            Err(_) => {
                failures.push(format!("{name}: panic during the replay"));
                continue;
            }
        };
        for expected in script["expected"].as_array().expect("expected") {
            let expected = checkpoint_of(expected);
            compared += 1;
            let problems = match actual.iter().find(|c| c.after_step == expected.after_step) {
                Some(got) => diff(&expected, got),
                None => vec!["missing checkpoint".to_owned()],
            };
            if !problems.is_empty() {
                failures.push(format!(
                    "{name} after step {}: {}",
                    expected.after_step,
                    problems.join(", ")
                ));
            }
        }
    }
    assert!(compared >= 700, "only {compared} checkpoints were compared");
    assert!(
        failures.is_empty(),
        "{} checkpoints differ:\n{}",
        failures.len(),
        failures.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}

// new: the comparer of the replay rejects every field it must (a replay that cannot fail proves nothing).
#[test]
fn g_input_diff_rejects_a_difference_in_every_compared_field() {
    let base = Checkpoint {
        after_step: 0,
        zoom: 0.28,
        is_fit: false,
        horizontal_offset: 10.0,
        vertical_offset: 20.0,
        extent_width: 1680.0,
        extent_height: 1120.0,
        viewport_width: 1270.0,
        viewport_height: 710.0,
        display_zoom_percent: 28,
    };
    assert!(diff(&base, &base).is_empty());
    assert!(
        diff(&base, &Checkpoint { zoom: 0.2805, ..base }).is_empty(),
        "within 0.001"
    );
    assert!(
        diff(
            &base,
            &Checkpoint {
                horizontal_offset: 10.4,
                ..base
            }
        )
        .is_empty(),
        "within 0.5"
    );
    assert_eq!(1, diff(&base, &Checkpoint { zoom: 0.2812, ..base }).len());
    assert_eq!(1, diff(&base, &Checkpoint { is_fit: true, ..base }).len());
    assert_eq!(
        1,
        diff(
            &base,
            &Checkpoint {
                display_zoom_percent: 29,
                ..base
            }
        )
        .len()
    );
    assert_eq!(
        1,
        diff(
            &base,
            &Checkpoint {
                horizontal_offset: 10.51,
                ..base
            }
        )
        .len()
    );
    assert_eq!(
        1,
        diff(
            &base,
            &Checkpoint {
                vertical_offset: 20.51,
                ..base
            }
        )
        .len()
    );
    assert_eq!(
        1,
        diff(
            &base,
            &Checkpoint {
                extent_width: 1681.0,
                ..base
            }
        )
        .len()
    );
    assert_eq!(
        1,
        diff(
            &base,
            &Checkpoint {
                extent_height: 1121.0,
                ..base
            }
        )
        .len()
    );
    assert_eq!(
        1,
        diff(
            &base,
            &Checkpoint {
                viewport_width: 1271.0,
                ..base
            }
        )
        .len()
    );
    assert_eq!(
        1,
        diff(
            &base,
            &Checkpoint {
                viewport_height: 711.0,
                ..base
            }
        )
        .len()
    );
}

// new: the replay computes the hand-derived checkpoint of a wheel from Fit (C# `Runner_WheelFromFit_MatchesTheHandComputedCheckpoint`), so an engine that
// copied the golden could not pass.
#[test]
fn g_input_wheel_from_fit_matches_the_hand_computed_checkpoint() {
    // 6000x4000 in 1280x720: Fit 1080x720 at x = 100, FitZoom 0.18; the default step 10 % gives 0.28 -> 1680x1120 with two scroll bars.
    let script: Value = serde_json::json!({
        "name": "wheel-from-fit",
        "setup": {"clientWidth": 1280, "clientHeight": 720, "dpiScale": 1.0, "imagePixelWidth": 6000, "imagePixelHeight": 4000,
                   "settingsOverridesJson": "{\"MouseWheelAction\":\"Zoom\"}"},
        "steps": [{"kind": "wheel", "x": 400, "y": 300, "delta": 120, "key": null, "modifiers": null, "timestampMs": 0, "command": null}],
        "expected": [{"afterStep": -1}, {"afterStep": 0}]
    });
    let actual = run(&script).expect("runs");
    assert_eq!(2, actual.len());
    assert!(actual[0].is_fit);
    assert_eq!(18, actual[0].display_zoom_percent);
    let after = actual[1];
    assert!(!after.is_fit);
    assert!((after.zoom - 0.28).abs() < 1e-9, "zoom {}", after.zoom);
    assert_eq!(28, after.display_zoom_percent);
    assert!(
        (after.horizontal_offset - (1680.0 * 300.0 / 1080.0 - 400.0)).abs() <= 0.5,
        "h {}",
        after.horizontal_offset
    );
    assert!(
        (after.vertical_offset - (1120.0 * 300.0 / 720.0 - 300.0)).abs() <= 0.5,
        "v {}",
        after.vertical_offset
    );
    assert_eq!(
        (1680.0, 1120.0),
        (after.extent_width.round(), after.extent_height.round())
    );
    assert_eq!((1270.0, 710.0), (after.viewport_width, after.viewport_height));
}
