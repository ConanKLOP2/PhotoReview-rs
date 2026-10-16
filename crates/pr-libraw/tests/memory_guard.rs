//! Port tests for `pr_libraw::memory_guard` (C# `PhotoReview.Imaging.LibRaw.DecodeMemoryGuard`, baseline d92d6aae).
//!
//! Covers the pure rows of `tests/PhotoReview.Imaging.Tests/Raw/LibRawDecodeMemoryTests.cs` and
//! `LibRawGateAndGuardMutationGapTests.cs` (the 4 `DecodeMemoryGuard` tests of the latter), plus the golden twin over
//! `tests/golden/libraw-memory-guard.json` (inputs: `tests/golden-inputs/libraw-memory-guard.json`).
//!
//! Red tests: the stub bodies are `todo!()`, so every test here fails until the W5-16b porter fills them in.
//! Not here (other owners): `HasHeadroom_ComparesTheEstimateWithTotalMinusLoad` (W4-07b), `TryReadRawStructure_*` and
//! `LibRawRawStructureNativeTests` (W5-16a), `AdmitDecode_*` and the `FullDecodeGate` rows (W5-16a / W5-16c).

use pr_imaging::memory_headroom::has_headroom;
use pr_libraw::memory_guard::{
    BAYER_RAW_BYTES_PER_PIXEL, BITMAP_BYTES_PER_PIXEL, FLOAT_RAW_BYTES_PER_PIXEL, LINEAR_RAW_BYTES_PER_PIXEL,
    RGB_OUTPUT_BYTES_PER_PIXEL, RawBufferFamily, RawStructure, WORKING_IMAGE_BYTES_PER_PIXEL, classify,
    estimate_peak_bytes, estimate_peak_bytes_output_sized,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

const WIDTH: i32 = 11_600;
const HEIGHT: i32 = 8_700;

/// C# `Mosaic` field of `LibRawGateAndGuardMutationGapTests` / `LibRawDecodeMemoryTests`: 3 colours, CFA pattern.
fn mosaic() -> RawStructure {
    RawStructure::new(3, 0x9494_9494)
}

/// C# `NoMosaic` field: 3 colours, no CFA (filters 0).
fn no_mosaic() -> RawStructure {
    RawStructure::new(3, 0)
}

/// The four DNG decoder names of the C# DNG family (ordinal substring test).
const DNG_NAMES: [&str; 4] = [
    "lossless_dng_load_raw()",
    "packed_dng_load_raw()",
    "deflate_dng_load_raw()",
    "lossy_dng_load_raw()",
];

// ---- LibRawDecodeMemoryTests (pure rows) ----------------------------------------------------------------------------------

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_HundredMegapixelFullSize_CountsWorkingImageRgbRawBufferAndBitmap
#[test]
fn estimate_peak_bytes_hundred_megapixel_full_size_counts_working_image_rgb_raw_buffer_and_bitmap() {
    let estimate = estimate_peak_bytes_output_sized(WIDTH, HEIGHT, WIDTH, HEIGHT);

    assert_eq!(estimate, i64::from(WIDTH) * i64::from(HEIGHT) * (8 + 3 + 2 + 4));
    assert!(
        (1_650_000_000..=1_750_000_000).contains(&estimate),
        "estimate {estimate} outside 1.65e9..=1.75e9"
    );
}

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_BayerHundredMegapixelBoundedTarget_IsThirteenBytesPerPixelPlusTheTarget
#[test]
fn estimate_peak_bytes_bayer_hundred_megapixel_bounded_target_is_thirteen_bytes_per_pixel_plus_the_target() {
    let estimate = estimate_peak_bytes(WIDTH, HEIGHT, 1920, 1280, WIDTH, HEIGHT, RawBufferFamily::Bayer);

    assert_eq!(estimate, i64::from(WIDTH) * i64::from(HEIGHT) * 13 + 1920 * 1280 * 4);
}

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_LinearHundredMegapixelBoundedTarget_IsNineteenBytesPerPixelPlusTheTarget
#[test]
fn estimate_peak_bytes_linear_hundred_megapixel_bounded_target_is_nineteen_bytes_per_pixel_plus_the_target() {
    let estimate = estimate_peak_bytes(WIDTH, HEIGHT, 1920, 1280, WIDTH, HEIGHT, RawBufferFamily::Linear);

    assert_eq!(estimate, i64::from(WIDTH) * i64::from(HEIGHT) * 19 + 1920 * 1280 * 4);
}

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_FloatHundredMegapixelBoundedTarget_CountsTheFloatBufferAndTheSixteenBitCopyTogether
#[test]
fn estimate_peak_bytes_float_hundred_megapixel_bounded_target_counts_the_float_buffer_and_the_sixteen_bit_copy_together()
 {
    let estimate = estimate_peak_bytes(WIDTH, HEIGHT, 1920, 1280, WIDTH, HEIGHT, RawBufferFamily::Float);

    // 35 B/px: above the 19 of an integer linear file
    assert_eq!(
        estimate,
        i64::from(WIDTH) * i64::from(HEIGHT) * (8 + 3 + 24) + 1920 * 1280 * 4
    );
}

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_RawBufferUsesTheSensorSizeNotTheOutputSize
#[test]
fn estimate_peak_bytes_raw_buffer_uses_the_sensor_size_not_the_output_size() {
    // A 10 MP output cropped from a 12 MP sensor: the unpacked buffer is sensor sized.
    let estimate = estimate_peak_bytes(4000, 2500, 100, 100, 4200, 2860, RawBufferFamily::Bayer);

    assert_eq!(estimate, 4000 * 2500 * 11 + 4200 * 2860 * 2 + 100 * 100 * 4);
}

// C#: LibRawDecodeMemoryTests.HasHeadroom_BayerHundredMegapixel_IsRefusedWithTwelveHundredMegabytesAndAllowedWithFifteenHundred
// The Rust twin pins that the estimate and the W4-07b guard compose; the guard itself is W4-07b's test.
#[test]
fn has_headroom_bayer_hundred_megapixel_is_refused_with_twelve_hundred_megabytes_and_allowed_with_fifteen_hundred() {
    let estimate = estimate_peak_bytes(WIDTH, HEIGHT, 1920, 1280, WIDTH, HEIGHT, RawBufferFamily::Bayer);

    assert!(!has_headroom(estimate, 1_200_000_000, 0));
    assert!(has_headroom(estimate, 1_500_000_000, 0));
}

// C#: LibRawDecodeMemoryTests.Classify_DngDecoderOnAMosaicFile_IsBayer
#[test]
fn classify_dng_decoder_on_a_mosaic_file_is_bayer() {
    for name in DNG_NAMES {
        assert_eq!(
            classify(Some(name), Some(mosaic())),
            RawBufferFamily::Bayer,
            "row {name}: a mosaic DNG is Bayer (2 B/px)"
        );
    }
}

// C#: LibRawDecodeMemoryTests.Classify_DngDecoderOnANonMosaicFile_IsLinear
#[test]
fn classify_dng_decoder_on_a_non_mosaic_file_is_linear() {
    for name in DNG_NAMES {
        assert_eq!(
            classify(Some(name), Some(no_mosaic())),
            RawBufferFamily::Linear,
            "row {name}: no-mosaic DNG (filters 0) is Linear"
        );
        assert_eq!(
            classify(Some(name), Some(RawStructure::new(4, 0))),
            RawBufferFamily::Linear,
            "row {name}: 4 colours without a mosaic is Linear"
        );
    }
}

// C#: LibRawDecodeMemoryTests.Classify_DngDecoderWithUnreadableStructure_FallsBackToTheConservativeLinearEstimate
#[test]
fn classify_dng_decoder_with_unreadable_structure_falls_back_to_the_conservative_linear_estimate() {
    for name in DNG_NAMES {
        assert_eq!(
            classify(Some(name), None),
            RawBufferFamily::Linear,
            "row {name}: unreadable structure is the conservative Linear (8 B/px)"
        );
    }
}

// C#: LibRawDecodeMemoryTests.Classify_FloatDngDecoder_IsFloatWhateverTheStructure
#[test]
fn classify_float_dng_decoder_is_float_whatever_the_structure() {
    for name in ["uncompressed_fp_dng_load_raw()", "deflate_fp_dng_load_raw()"] {
        assert_eq!(
            classify(Some(name), Some(mosaic())),
            RawBufferFamily::Float,
            "row {name} with mosaic"
        );
        assert_eq!(
            classify(Some(name), Some(no_mosaic())),
            RawBufferFamily::Float,
            "row {name} without mosaic"
        );
        assert_eq!(
            classify(Some(name), None),
            RawBufferFamily::Float,
            "row {name} with unknown structure"
        );
    }
}

// C#: LibRawDecodeMemoryTests.Classify_NonDngOrUnknownDecoder_IsBayerEvenWithoutAMosaic
#[test]
fn classify_non_dng_or_unknown_decoder_is_bayer_even_without_a_mosaic() {
    for name in [Some("canon_load_raw()"), Some("nikon_load_raw()"), None] {
        assert_eq!(
            classify(name, Some(no_mosaic())),
            RawBufferFamily::Bayer,
            "row {name:?} with no mosaic"
        );
        assert_eq!(
            classify(name, None),
            RawBufferFamily::Bayer,
            "row {name:?} with unknown structure"
        );
    }
}

// C#: LibRawDecodeMemoryTests.Estimate_LinearFortyEightMegapixelLosslessDng_IsNotChargedAsBayer
#[test]
fn estimate_linear_forty_eight_megapixel_lossless_dng_is_not_charged_as_bayer() {
    let (width, height) = (8_000, 6_000); // 48 MP
    let linear = estimate_peak_bytes(
        width,
        height,
        1920,
        1280,
        width,
        height,
        classify(Some("lossless_dng_load_raw()"), Some(no_mosaic())),
    );
    let bayer = estimate_peak_bytes(
        width,
        height,
        1920,
        1280,
        width,
        height,
        classify(Some("lossless_dng_load_raw()"), Some(mosaic())),
    );

    // ~288 MB of under-estimate fixed: 8 - 2 bytes per sensor pixel
    assert_eq!(linear - bayer, i64::from(width) * i64::from(height) * 6);
}

// C#: LibRawDecodeMemoryTests.Estimate_BayerHundredMegapixelDeflateDng_IsTheThirteenBytesPerPixelBayerResult
#[test]
fn estimate_bayer_hundred_megapixel_deflate_dng_is_the_thirteen_bytes_per_pixel_bayer_result() {
    let family = classify(Some("deflate_dng_load_raw()"), Some(mosaic()));

    let estimate = estimate_peak_bytes(WIDTH, HEIGHT, 1920, 1280, WIDTH, HEIGHT, family);

    assert_eq!(estimate, i64::from(WIDTH) * i64::from(HEIGHT) * 13 + 1920 * 1280 * 4);
}

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_BoundedTarget_CountsOnlyTheTargetBitmap
#[test]
fn estimate_peak_bytes_bounded_target_counts_only_the_target_bitmap() {
    assert_eq!(
        estimate_peak_bytes_output_sized(6000, 4000, 1920, 1280),
        6000 * 4000 * 13 + 1920 * 1280 * 4
    );
}

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_InvalidSource_IsZero
#[test]
fn estimate_peak_bytes_invalid_source_is_zero() {
    for (width, height) in [(0, 10), (10, 0), (-1, 10)] {
        assert_eq!(
            estimate_peak_bytes_output_sized(width, height, 10, 10),
            0,
            "row {width}x{height}: a non-positive source is 0 before any other term"
        );
    }
}

// C#: LibRawDecodeMemoryTests.EstimatePeakBytes_AbsurdDimensions_SaturatesInsteadOfOverflowing
// Strengthened: the C# value is long.MaxValue (saturation), not just "> 0".
#[test]
fn estimate_peak_bytes_absurd_dimensions_saturates_instead_of_overflowing() {
    assert_eq!(
        estimate_peak_bytes_output_sized(i32::MAX, i32::MAX, i32::MAX, i32::MAX),
        i64::MAX
    );
}

// ---- LibRawGateAndGuardMutationGapTests (the 4 pure DecodeMemoryGuard tests) ----------------------------------------------

// C#: LibRawGateAndGuardMutationGapTests.Classify_DngMosaicWithOneToFourColours_IsBayer
#[test]
fn classify_dng_mosaic_with_one_to_four_colours_is_bayer() {
    for colors in [1, 2, 3, 4] {
        assert_eq!(
            classify(
                Some("lossless_dng_load_raw()"),
                Some(RawStructure::new(colors, 0x9494_9494))
            ),
            RawBufferFamily::Bayer,
            "colours {colors}: 1 to 4 colours with a mosaic is Bayer"
        );
    }
}

// C#: LibRawGateAndGuardMutationGapTests.Classify_DngMosaicWithAnImplausibleColourCount_IsLinear
#[test]
fn classify_dng_mosaic_with_an_implausible_colour_count_is_linear() {
    for colors in [0, 5] {
        assert_eq!(
            classify(
                Some("lossless_dng_load_raw()"),
                Some(RawStructure::new(colors, 0x9494_9494))
            ),
            RawBufferFamily::Linear,
            "colours {colors}: outside 1..=4 is not Bayer, so Linear"
        );
    }
}

// C#: LibRawGateAndGuardMutationGapTests.EstimatePeakBytes_OnlyOneRawSideKnown_UsesTheOutputSizeForTheRawBuffer
#[test]
fn estimate_peak_bytes_only_one_raw_side_known_uses_the_output_size_for_the_raw_buffer() {
    let expected = estimate_peak_bytes_output_sized(1000, 800, 500, 400);

    for (raw_width, raw_height) in [(0, 100), (100, 0)] {
        assert_eq!(
            estimate_peak_bytes(1000, 800, 500, 400, raw_width, raw_height, RawBufferFamily::Bayer),
            expected,
            "raw sides ({raw_width}, {raw_height}): a lone known side is ignored"
        );
    }
}

// C#: LibRawGateAndGuardMutationGapTests.EstimatePeakBytes_BothRawSidesKnown_UsesTheSensorSizeForTheRawBuffer
#[test]
fn estimate_peak_bytes_both_raw_sides_known_uses_the_sensor_size_for_the_raw_buffer() {
    let estimate = estimate_peak_bytes(1000, 800, 500, 400, 1100, 900, RawBufferFamily::Bayer);

    // 1000x800 x (8+3) working/output + 1100x900 x 2 raw + 500x400 x 4 bitmap
    assert_eq!(estimate, 1000 * 800 * 11 + 1100 * 900 * 2 + 500 * 400 * 4);
}

// ---- Constants (golden `constants`) ---------------------------------------------------------------------------------------

#[test]
fn constants_are_the_csharp_values() {
    assert_eq!(WORKING_IMAGE_BYTES_PER_PIXEL, 8);
    assert_eq!(RGB_OUTPUT_BYTES_PER_PIXEL, 3);
    assert_eq!(BITMAP_BYTES_PER_PIXEL, 4);
    assert_eq!(BAYER_RAW_BYTES_PER_PIXEL, 2);
    assert_eq!(LINEAR_RAW_BYTES_PER_PIXEL, 8);
    assert_eq!(FLOAT_RAW_BYTES_PER_PIXEL, 24);
}

// ---- Golden twin over tests/golden/libraw-memory-guard.json ---------------------------------------------------------------

/// The input cases of `tests/golden-inputs/libraw-memory-guard.json` (written by `gen-libraw-memory-guard.js`).
fn input_cases() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("libraw-memory-guard.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

/// Every input case of `op`, paired with its oracle result (matched by `name`). Asserts both sides have the same count.
fn paired(op: &str) -> Vec<(Value, Value)> {
    let oracle = golden::load("libraw-memory-guard").cases;
    let pairs: Vec<(Value, Value)> = input_cases()
        .into_iter()
        .filter(|input| str_of(input, "op") == op)
        .map(|input| {
            let name = str_of(&input, "name").to_owned();
            let result = oracle
                .iter()
                .find(|r| str_of(r, "name") == name)
                .unwrap_or_else(|| panic!("no oracle result for {name}"))
                .clone();
            assert_eq!(str_of(&result, "op"), op, "{name}: oracle op differs");
            (input, result)
        })
        .collect();
    let oracle_count = oracle.iter().filter(|r| str_of(r, "op") == op).count();
    assert!(!pairs.is_empty(), "no input cases for op {op}");
    assert_eq!(
        pairs.len(),
        oracle_count,
        "op {op}: input and oracle case counts differ"
    );
    pairs
}

fn i32_of(value: &Value, key: &str) -> i32 {
    let raw = i64_of(value, key);
    i32::try_from(raw).unwrap_or_else(|_| panic!("'{key}' = {raw} does not fit i32"))
}

fn u32_of(value: &Value, key: &str) -> u32 {
    let raw = value[key]
        .as_u64()
        .unwrap_or_else(|| panic!("case has no unsigned integer '{key}': {value}"));
    u32::try_from(raw).unwrap_or_else(|_| panic!("'{key}' = {raw} does not fit u32"))
}

/// The `structure` field of a case: JSON `null` is `None`, an object is `Some(RawStructure)`.
fn structure_of(value: &Value) -> Option<RawStructure> {
    if value.is_null() {
        None
    } else {
        Some(RawStructure::new(i32_of(value, "colors"), u32_of(value, "filters")))
    }
}

fn family_named(name: &str) -> RawBufferFamily {
    match name {
        "Bayer" => RawBufferFamily::Bayer,
        "Linear" => RawBufferFamily::Linear,
        "Float" => RawBufferFamily::Float,
        other => panic!("unknown family '{other}' in golden"),
    }
}

#[test]
fn golden_constants_match_the_oracle() {
    for (_input, result) in paired("constants") {
        let values = &result["values"];
        assert_eq!(
            i64_of(values, "WorkingImageBytesPerPixel"),
            WORKING_IMAGE_BYTES_PER_PIXEL
        );
        assert_eq!(i64_of(values, "RgbOutputBytesPerPixel"), RGB_OUTPUT_BYTES_PER_PIXEL);
        assert_eq!(i64_of(values, "BitmapBytesPerPixel"), BITMAP_BYTES_PER_PIXEL);
        assert_eq!(i64_of(values, "BayerRawBytesPerPixel"), BAYER_RAW_BYTES_PER_PIXEL);
        assert_eq!(i64_of(values, "LinearRawBytesPerPixel"), LINEAR_RAW_BYTES_PER_PIXEL);
        assert_eq!(i64_of(values, "FloatRawBytesPerPixel"), FLOAT_RAW_BYTES_PER_PIXEL);
    }
}

#[test]
fn golden_is_bayer_matches_the_oracle() {
    let cases = paired("is-bayer");
    assert_eq!(cases.len(), 40, "is-bayer golden case count");
    for (input, result) in cases {
        let name = str_of(&input, "name");
        let structure = RawStructure::new(i32_of(&input, "colors"), u32_of(&input, "filters"));
        assert_eq!(structure.is_bayer(), bool_of(&result, "value"), "{name}");
    }
}

#[test]
fn golden_classify_matches_the_oracle() {
    let cases = paired("classify");
    assert_eq!(cases.len(), 200, "classify golden case count");
    for (input, result) in cases {
        let name = str_of(&input, "name");
        let decoder = input["decoder"].as_str();
        let family = classify(decoder, structure_of(&input["structure"]));
        assert_eq!(
            format!("{family:?}"),
            str_of(&result, "family"),
            "{name}: decoder {decoder:?}"
        );
    }
}

#[test]
fn golden_estimate_matches_the_oracle() {
    let cases = paired("estimate");
    assert_eq!(cases.len(), 111, "estimate golden case count");
    for (input, result) in cases {
        let name = str_of(&input, "name");
        let estimate = estimate_peak_bytes(
            i32_of(&input, "width"),
            i32_of(&input, "height"),
            i32_of(&input, "target_width"),
            i32_of(&input, "target_height"),
            i32_of(&input, "raw_width"),
            i32_of(&input, "raw_height"),
            family_named(str_of(&input, "family")),
        );
        assert_eq!(estimate, i64_of(&result, "bytes"), "{name}");
    }
}

#[test]
fn golden_estimate_defaults_match_the_oracle() {
    let cases = paired("estimate-defaults");
    assert_eq!(cases.len(), 7, "estimate-defaults golden case count");
    for (input, result) in cases {
        let name = str_of(&input, "name");
        let estimate = estimate_peak_bytes_output_sized(
            i32_of(&input, "width"),
            i32_of(&input, "height"),
            i32_of(&input, "target_width"),
            i32_of(&input, "target_height"),
        );
        assert_eq!(estimate, i64_of(&result, "bytes"), "{name}");
    }
}
