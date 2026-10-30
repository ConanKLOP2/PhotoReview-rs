//! Port of `tests/PhotoReview.Imaging.Tests/AdaptivePreviewPolicyTests.cs` (C# baseline d92d6aae, PARITY I-05)
//! plus golden checks against `tests/golden/adaptive-preview-box.json` (38 cases) and
//! `tests/golden/adaptive-preview-quantize.json` (24 cases), generated from .NET by `tools/oracle-cs`.
//!
//! Port of `PhotoReview.Imaging/AdaptivePreviewPolicy.cs` (C# baseline d92d6aae).
//!
//! Red tests (W4-05a): they compile against the stub and fail through `todo!()` until the bodies are implemented.

use std::fs;

use pr_imaging::adaptive_preview::{
    DEFAULT_DPI_SCALE, DEFAULT_QUALITY_MULTIPLIER, InvalidPreviewArgument, MAXIMUM_BOX_SIDE, MINIMUM_BOX_LONG_SIDE,
    calculate_target_decode_box, quantize,
};
use pr_imaging::decode_types::DecodeBox;
use pr_test_support::golden::{self, i64_of, repo_root, str_of};
use serde_json::Value;

/// Reads `tests/golden-inputs/<name>.json`: the oracle's inputs for a golden. The golden keeps only the expected values.
fn golden_inputs(name: &str) -> Vec<Value> {
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

/// A JSON input as a double. JSON has no NaN, Infinity or -0, so the oracle writes those three as strings.
fn number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a double: {value}")),
        Value::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => panic!("unknown special double {other:?}"),
        },
        other => panic!("not a number: {other}"),
    }
}

/// An optional double input: the C# default when the key is absent.
fn number_or(case: &Value, key: &str, default: f64) -> f64 {
    case.get(key).map_or(default, number)
}

/// The outcome compared by name, so the golden's `param` string and the error's `param_name` meet in one type.
fn actual_outcome(result: Result<DecodeBox, InvalidPreviewArgument>) -> Result<DecodeBox, String> {
    result.map_err(|e| e.param_name.to_owned())
}

/// The golden's outcome: a box, or the rejected parameter name (`error` names the C# exception kind).
fn expected_outcome(case: &Value) -> Result<DecodeBox, String> {
    if case.get("error").is_some() {
        assert_eq!(str_of(case, "error"), "ArgumentOutOfRangeException", "case {case}");
        Err(str_of(case, "param").to_owned())
    } else {
        let width = i32::try_from(i64_of(case, "width")).expect("golden width fits i32");
        let height = i32::try_from(i64_of(case, "height")).expect("golden height fits i32");
        Ok(DecodeBox::new(width, height))
    }
}

// C#: AdaptivePreviewPolicyTests.CalculateTargetDecodeBox_FullHd_QuantizedUp
#[test]
fn calculate_target_decode_box_full_hd_quantized_up() {
    // 1920 * 1.15 = 2208 -> 2304; 1080 * 1.15 = 1242 -> 1280.
    let decode_box = calculate_target_decode_box(1920.0, 1080.0, 1.0, 1.15).expect("valid viewport");

    assert_eq!(decode_box, DecodeBox::new(2304, 1280));
}

// C#: AdaptivePreviewPolicyTests.CalculateTargetDecodeBox_DpiScale_AppliesToBothSides
#[test]
fn calculate_target_decode_box_dpi_scale_applies_to_both_sides() {
    // 1536x864 DIPs at 125% is the same 1920x1080 device-pixel viewport.
    assert_eq!(
        calculate_target_decode_box(1920.0, 1080.0, 1.0, 1.15),
        calculate_target_decode_box(1536.0, 864.0, 1.25, 1.15)
    );
}

// C#: AdaptivePreviewPolicyTests.CalculateTargetDecodeBox_SmallResize_SameBox
#[test]
fn calculate_target_decode_box_small_resize_same_box() {
    let before = calculate_target_decode_box(1920.0, 1080.0, 1.0, 1.15);

    assert_eq!(before, calculate_target_decode_box(1930.0, 1090.0, 1.0, 1.15));
    assert_eq!(before, calculate_target_decode_box(1900.0, 1050.0, 1.0, 1.15));
}

// C#: AdaptivePreviewPolicyTests.CalculateTargetDecodeBox_BiggerWindow_BiggerBox
#[test]
fn calculate_target_decode_box_bigger_window_bigger_box() {
    let full_hd = calculate_target_decode_box(1920.0, 1080.0, 1.0, 1.15).expect("valid viewport");
    let qhd = calculate_target_decode_box(2560.0, 1440.0, 1.0, 1.15).expect("valid viewport");

    // 2560 * 1.15 = 2944 (exactly 23 quanta, no extra step); 1440 * 1.15 = 1656 -> 1664.
    assert_eq!(qhd, DecodeBox::new(2944, 1664));
    assert!(qhd.width > full_hd.width && qhd.height > full_hd.height);
}

// C#: AdaptivePreviewPolicyTests.CalculateTargetDecodeBox_TinyWindow_LongSideRaisedToMinimum
#[test]
fn calculate_target_decode_box_tiny_window_long_side_raised_to_minimum() {
    // 500x300 -> x2.4 -> 1200x720 -> quantized 1280x768.
    let decode_box = calculate_target_decode_box(500.0, 300.0, 1.0, 1.0).expect("valid viewport");

    assert_eq!(decode_box, DecodeBox::new(1280, 768));
    assert!(decode_box.width.max(decode_box.height) >= MINIMUM_BOX_LONG_SIDE);
}

// C#: AdaptivePreviewPolicyTests.CalculateTargetDecodeBox_Huge_Capped
#[test]
fn calculate_target_decode_box_huge_capped() {
    let decode_box = calculate_target_decode_box(5000.0, 3000.0, 2.0, 1.5).expect("valid viewport");

    assert_eq!(decode_box, DecodeBox::new(MAXIMUM_BOX_SIDE, MAXIMUM_BOX_SIDE));
}

// C#: AdaptivePreviewPolicyTests.Quantize_RoundsUpAndCaps (theory, 7 rows, one assert per row)
#[test]
fn quantize_rounds_up_and_caps() {
    let rows: [(f64, i32); 7] = [
        (1.0, 128),
        (128.0, 128),
        (128.5, 256),
        (1242.0, 1280),
        (1280.0000000001, 1280),
        (3999.0, 4000),
        (99999.0, 4000),
    ];
    for (side, expected) in rows {
        assert_eq!(quantize(side), expected, "quantize({side})");
    }
}

// C#: AdaptivePreviewPolicyTests.CalculateTargetDecodeBox_ThrowsOnInvalid
// The C# test asserts only the exception type; the Rust port also pins the parameter name (golden `param`).
#[test]
fn calculate_target_decode_box_throws_on_invalid() {
    assert_eq!(
        calculate_target_decode_box(0.0, 1080.0, DEFAULT_DPI_SCALE, DEFAULT_QUALITY_MULTIPLIER),
        Err(InvalidPreviewArgument {
            param_name: "viewportWidth"
        })
    );
    assert_eq!(
        calculate_target_decode_box(1920.0, 0.0, DEFAULT_DPI_SCALE, DEFAULT_QUALITY_MULTIPLIER),
        Err(InvalidPreviewArgument {
            param_name: "viewportHeight"
        })
    );
    assert_eq!(
        calculate_target_decode_box(1920.0, f64::NAN, DEFAULT_DPI_SCALE, DEFAULT_QUALITY_MULTIPLIER),
        Err(InvalidPreviewArgument {
            param_name: "viewportHeight"
        })
    );
    assert_eq!(
        calculate_target_decode_box(1920.0, 1080.0, -1.0, DEFAULT_QUALITY_MULTIPLIER),
        Err(InvalidPreviewArgument { param_name: "dpiScale" })
    );
    assert_eq!(
        calculate_target_decode_box(1920.0, 1080.0, 1.0, 0.0),
        Err(InvalidPreviewArgument {
            param_name: "qualityMultiplier"
        })
    );
}

// C#: AdaptivePreviewPolicyTests.BoxDecode_24MpInFullHdWindow_PixelCounts (theory, row 1: landscape 6000x4000)
#[test]
fn box_decode_24mp_in_full_hd_window_pixel_counts_landscape() {
    assert_box_decode_pixel_counts(6000, 4000, 2208 * 1472, 1920 * 1280);
}

// C#: AdaptivePreviewPolicyTests.BoxDecode_24MpInFullHdWindow_PixelCounts (theory, row 2: portrait 4000x6000)
#[test]
fn box_decode_24mp_in_full_hd_window_pixel_counts_portrait() {
    assert_box_decode_pixel_counts(4000, 6000, 2208 * 3312, 853 * 1280);
}

/// Shared body of the two pixel-count rows. The former width-only target for the 1920 x 1.15 window is 2208 px wide.
fn assert_box_decode_pixel_counts(src_width: i32, src_height: i32, width_only_pixels: i32, box_pixels: i32) {
    let decode_box = calculate_target_decode_box(1920.0, 1080.0, 1.0, 1.15).expect("valid viewport");

    let (ow, oh) = DecodeBox::new(2208, 0).fit(src_width, src_height);
    let (bw, bh) = decode_box.fit(src_width, src_height);

    assert_eq!(ow * oh, width_only_pixels);
    assert_eq!(bw * bh, box_pixels);
    assert!(bh <= decode_box.height && bw <= decode_box.width);
}

// Golden: tests/golden/adaptive-preview-box.json (38 cases, oracle-cs adaptive-preview-box).
// Compares the whole outcome of every case: width, height, or the error and its parameter name.
#[test]
fn box_golden_matches_csharp_oracle() {
    let expected_doc = golden::load("adaptive-preview-box");
    let inputs = golden_inputs("adaptive-preview-box");
    assert_eq!(expected_doc.cases.len(), 38, "golden case count");
    assert_eq!(
        inputs.len(),
        expected_doc.cases.len(),
        "golden and inputs disagree on the case count"
    );

    for (expected, input) in expected_doc.cases.iter().zip(&inputs) {
        let name = str_of(expected, "name");
        assert_eq!(
            str_of(input, "name"),
            name,
            "golden and inputs are in a different order"
        );

        let actual = calculate_target_decode_box(
            number(&input["viewport_width"]),
            number(&input["viewport_height"]),
            number_or(input, "dpi_scale", DEFAULT_DPI_SCALE),
            number_or(input, "quality_multiplier", DEFAULT_QUALITY_MULTIPLIER),
        );

        assert_eq!(actual_outcome(actual), expected_outcome(expected), "case {name}");
    }
}

// Golden: tests/golden/adaptive-preview-quantize.json (24 cases, oracle-cs adaptive-preview-quantize).
#[test]
fn quantize_golden_matches_csharp_oracle() {
    let expected_doc = golden::load("adaptive-preview-quantize");
    let inputs = golden_inputs("adaptive-preview-quantize");
    assert_eq!(expected_doc.cases.len(), 24, "golden case count");
    assert_eq!(
        inputs.len(),
        expected_doc.cases.len(),
        "golden and inputs disagree on the case count"
    );

    for (expected, input) in expected_doc.cases.iter().zip(&inputs) {
        let name = str_of(expected, "name");
        assert_eq!(
            str_of(input, "name"),
            name,
            "golden and inputs are in a different order"
        );

        let expected_result = i32::try_from(i64_of(expected, "result")).expect("golden result fits i32");

        assert_eq!(quantize(number(&input["side"])), expected_result, "case {name}");
    }
}
