//! Red tests (W8-16a, test porter) for `pr_app::viewing::scaling_policy`: the port of
//! `PhotoReview.Shell.Win32/Viewing/ScalingPolicy.cs` (baseline `d92d6aae`).
//!
//! Ported from `tests/PhotoReview.Shell.Tests/Viewing/ScalingPolicyTests.cs` (6 tests, names in snake_case, each with a
//! `// C#:` comment; the Theories are one test with a loop over the rows). `ScalingPolicy` lives in `pr_app::viewing`
//! (lead decision D-W8B1-1), so its tests and its golden driver are here, not in `pr-platform-win`.
//!
//! Golden: `golden_scaling_policy` drives every case of `tests/golden/scaling-policy.json` (17 cases, 12 `choose`, 5
//! `is_heavy`). The inputs are in `tests/golden-inputs/scaling-policy.json`, joined by case name. Non-finite destination
//! sizes are the strings "NaN" / "Infinity".
//!
//! Every test here fails through `todo!()` until the bodies land.

use std::collections::HashMap;

use pr_app::viewing::scaling_policy::{HEAVY_DOWNSCALE_AREA_RATIO, SETTLE_MS, choose, is_heavy_downscale};
use pr_app::viewport::render_geometry::ImageInterpolation;
use pr_test_support::golden::{self, bool_of, repo_root, str_of};
use serde_json::Value;

const HQ: ImageInterpolation = ImageInterpolation::HighQualityCubic;

// C#: ScalingPolicyTests.Resting_KeepsTheRequestedFilter_EvenForAHeavyDownscale
#[test]
fn resting_keeps_requested_filter_for_heavy_downscale() {
    assert_eq!(choose(HQ, 6000, 4000, 1000.0, 667.0, false), HQ);
}

// C#: ScalingPolicyTests.InMotion_HeavyDownscale_FallsBackToLinear
#[test]
fn in_motion_heavy_downscale_falls_back_to_linear() {
    assert_eq!(choose(HQ, 6000, 4000, 1000.0, 667.0, true), ImageInterpolation::Linear);
}

// C#: ScalingPolicyTests.InMotion_LightDownscaleOrUpscale_KeepsHq
#[test]
fn in_motion_light_downscale_and_upscale_keep_hq() {
    assert_eq!(choose(HQ, 2000, 1500, 1000.0, 750.0, true), HQ); // exactly 4x the area: at the threshold, not heavy
    assert_eq!(choose(HQ, 1000, 750, 3000.0, 2250.0, true), HQ); // upscale
}

// C#: ScalingPolicyTests.ThresholdIsStrictlyGreaterThanFourTimesTheArea
#[test]
fn threshold_is_strictly_greater_than_four_times_the_area() {
    assert!(!is_heavy_downscale(2000, 1000, 1000.0, 500.0)); // exactly 4x
    assert!(is_heavy_downscale(2001, 1000, 1000.0, 500.0));
}

// C#: ScalingPolicyTests.NonCubicRequests_AreNeverChanged (Theory, 2 rows)
#[test]
fn non_cubic_requests_are_never_changed() {
    for requested in [ImageInterpolation::Linear, ImageInterpolation::NearestNeighbor] {
        assert_eq!(
            choose(requested, 6000, 4000, 100.0, 66.0, true),
            requested,
            "{requested:?}"
        );
    }
}

// C#: ScalingPolicyTests.DegenerateSizes_AreNotHeavy (Theory, 5 rows)
#[test]
fn degenerate_sizes_are_not_heavy() {
    let rows: [(i32, i32, f64, f64); 5] = [
        (0, 100, 10.0, 10.0),
        (100, 0, 10.0, 10.0),
        (100, 100, 0.0, 10.0),
        (100, 100, 10.0, 0.0),
        (100, 100, f64::NAN, 10.0),
    ];
    for (sw, sh, dw, dh) in rows {
        assert!(
            !is_heavy_downscale(sw, sh, dw, dh),
            "IsHeavyDownscale({sw}, {sh}, {dw}, {dh})"
        );
        assert_eq!(choose(HQ, sw, sh, dw, dh, true), HQ, "Choose({sw}, {sh}, {dw}, {dh})");
    }
}

// new: the two constants are the C# values (4.0 and 120 ms); the policy is useless with other numbers.
#[test]
fn constants_are_the_csharp_values() {
    assert_eq!(HEAVY_DOWNSCALE_AREA_RATIO, 4.0);
    assert_eq!(SETTLE_MS, 120);
}

// new: zero and negative sizes never panic in `choose` (the C# call sites pass whatever the viewport reports).
#[test]
fn choose_never_panics_on_zero_and_negative_sizes() {
    for (sw, sh) in [(0, 0), (-1, -1), (i32::MAX, i32::MAX), (-5, 7)] {
        let _ = choose(HQ, sw, sh, 0.0, -3.0, true);
        let _ = choose(HQ, sw, sh, -1.0, -1.0, false);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden driver: tests/golden/scaling-policy.json
// ---------------------------------------------------------------------------------------------------------------------

/// `tests/golden-inputs/scaling-policy.json`, keyed by case name.
fn inputs() -> HashMap<String, Value> {
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("scaling-policy.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: no cases", path.display()))
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

/// A golden number: a JSON number or one of the oracle spellings for non-finite values.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => other
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("not a golden number {other:?}: {e}")),
        },
        other => panic!("not a number: {other}"),
    }
}

/// The C# enum name of an interpolation (the golden writes `ToString()` of the enum).
fn interpolation_named(name: &str) -> ImageInterpolation {
    match name {
        "NearestNeighbor" => ImageInterpolation::NearestNeighbor,
        "Linear" => ImageInterpolation::Linear,
        "HighQualityCubic" => ImageInterpolation::HighQualityCubic,
        other => panic!("unknown ImageInterpolation {other:?}"),
    }
}

fn interpolation_name(value: ImageInterpolation) -> &'static str {
    match value {
        ImageInterpolation::NearestNeighbor => "NearestNeighbor",
        ImageInterpolation::Linear => "Linear",
        ImageInterpolation::HighQualityCubic => "HighQualityCubic",
    }
}

#[test]
fn golden_scaling_policy() {
    let golden = golden::load("scaling-policy");
    assert_eq!(golden.cases.len(), 17, "scaling-policy golden case count");
    let inputs = inputs();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let spec = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input named '{name}'"));
        let sw = i32::try_from(spec["sw"].as_i64().unwrap_or_else(|| panic!("{name}: sw"))).expect("sw fits i32");
        let sh = i32::try_from(spec["sh"].as_i64().unwrap_or_else(|| panic!("{name}: sh"))).expect("sh fits i32");
        let dw = num(&spec["dw"]);
        let dh = num(&spec["dh"]);

        match str_of(case, "kind") {
            "choose" => {
                let requested = interpolation_named(str_of(spec, "requested"));
                let in_motion = bool_of(spec, "in_motion");
                let got = choose(requested, sw, sh, dw, dh, in_motion);
                assert_eq!(
                    interpolation_name(got),
                    str_of(case, "interpolation"),
                    "{name}: interpolation"
                );
            }
            "is_heavy" => {
                assert_eq!(
                    is_heavy_downscale(sw, sh, dw, dh),
                    bool_of(case, "heavy"),
                    "{name}: heavy"
                );
            }
            other => panic!("{name}: unknown scaling-policy kind {other:?}"),
        }
    }
}
