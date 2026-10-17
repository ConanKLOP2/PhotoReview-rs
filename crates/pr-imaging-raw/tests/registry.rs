//! Registry routing and Adobe-compatible JPEG ICC profile tests.

use pr_core::sync::CancelToken;
use pr_imaging_raw::RawHeaderError;
use pr_imaging_raw::contracts::{RawContainerReader, RawHeaderSource};
use pr_imaging_raw::formats::RawContainerInfo;
use pr_imaging_raw::readers::{
    arw::ArwContainerReader, cr2::Cr2ContainerReader, cr3::Cr3ContainerReader, dng::DngContainerReader,
    nef::NefContainerReader, orf::OrfContainerReader, raf::RafContainerReader, rw2::Rw2ContainerReader,
};
use pr_imaging_raw::registry::{
    DEFAULT_READER_ORDER, ICC_IDENTIFIER, RawContainerReaderRegistry, adobe_rgb_profile, ensure_adobe_rgb_profile,
    has_embedded_icc, insert_icc_app2,
};
use serde_json::Value;

const REGISTRY_INPUTS: &str = include_str!("../../../tests/golden-inputs/raw-registry-find.json");
const ICC_INPUTS: &str = include_str!("../../../tests/golden-inputs/raw-icc-ensure.json");

#[derive(Debug)]
struct TestRawReader {
    extension: &'static str,
    marker: u8,
}

impl RawContainerReader for TestRawReader {
    fn can_read(&self, _first_64_bytes: &[u8], extension: &str) -> bool {
        extension == self.extension
    }

    fn read(&self, _source: &mut dyn RawHeaderSource, _ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        Err(RawHeaderError::invalid_data(format!("test reader {}", self.marker)))
    }
}

fn boxed_reader(name: &str) -> Box<dyn RawContainerReader> {
    match name {
        "Cr2" => Box::new(Cr2ContainerReader),
        "Nef" => Box::new(NefContainerReader),
        "Arw" => Box::new(ArwContainerReader),
        "Dng" => Box::new(DngContainerReader),
        "Orf" => Box::new(OrfContainerReader),
        "Rw2" => Box::new(Rw2ContainerReader),
        "Cr3" => Box::new(Cr3ContainerReader),
        "Raf" => Box::new(RafContainerReader),
        other => panic!("unknown reader in golden: {other}"),
    }
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "odd-length hex string");
    hex.as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            let pair = std::str::from_utf8(digits).expect("golden hex is ASCII");
            u8::from_str_radix(pair, 16).expect("golden contains valid hex")
        })
        .collect()
}

fn hex_string(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn registry_golden_inputs() -> Value {
    serde_json::from_str(REGISTRY_INPUTS).expect("raw-registry-find inputs are valid JSON")
}

fn icc_golden_inputs() -> Value {
    serde_json::from_str(ICC_INPUTS).expect("raw-icc-ensure inputs are valid JSON")
}

fn named_case<'a>(cases: &'a [Value], name: &str) -> &'a Value {
    cases
        .iter()
        .find(|case| case["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("missing golden input case {name}"))
}

fn icc_golden_case(name: &str) -> (Vec<u8>, Value) {
    let inputs = icc_golden_inputs();
    let input_case = named_case(inputs["cases"].as_array().expect("cases array"), name);
    let expected = pr_test_support::golden::load("raw-icc-ensure");
    let output_case = expected
        .cases
        .iter()
        .find(|case| case["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("missing raw-icc-ensure golden case {name}"));
    (
        hex_bytes(input_case["jpeg_hex"].as_str().expect("jpeg_hex")),
        output_case.clone(),
    )
}

fn assert_invalid_data(result: Result<Vec<u8>, RawHeaderError>, expected_message: &str) {
    match result {
        Err(error @ RawHeaderError::InvalidData { .. }) => {
            assert_eq!(error.to_string(), expected_message);
            assert!(!error.is_io_failure());
        }
        other => panic!("expected InvalidData({expected_message}), got {other:?}"),
    }
}

fn assert_icc_case(name: &str) {
    let (input, expected) = icc_golden_case(name);
    let input_snapshot = input.clone();
    let input_pointer = input.as_ptr();
    let result = ensure_adobe_rgb_profile(input);

    if expected["ok"] == false {
        assert_eq!(expected["error"], "InvalidDataException", "case {name}");
        assert_invalid_data(result, expected["message"].as_str().expect("golden error message"));
        return;
    }

    let output = result.unwrap_or_else(|error| panic!("case {name}: {error}"));
    assert_eq!(
        output.len(),
        expected["len"].as_u64().expect("golden len") as usize,
        "case {name}"
    );
    let head_len = output.len().min(20);
    assert_eq!(hex_string(&output[..head_len]), expected["head_hex"], "case {name}");
    let expected_same = expected["same"].as_bool().expect("golden same");
    assert_eq!(
        output.as_ptr() == input_pointer,
        expected_same,
        "case {name} allocation identity"
    );
    if expected["profile_equal"] == true {
        let profile = adobe_rgb_profile();
        assert_eq!(&output[20..20 + profile.len()], profile, "case {name} profile bytes");
    }
    if expected["tail_equals_input_after_soi"] == true {
        let tail_start = 20 + adobe_rgb_profile().len();
        assert_eq!(&output[tail_start..], &input_snapshot[2..], "case {name} tail");
    }
}

// C#: RawContractsAndHeaderSourceTests.RawContainerReaderRegistry_RoutesCorrectly
#[test]
fn raw_container_reader_registry_routes_correctly_and_returns_the_registered_instance() {
    let reader: Box<dyn RawContainerReader> = Box::new(TestRawReader {
        extension: ".cr2",
        marker: 1,
    });
    let registered_data = (&*reader as *const dyn RawContainerReader).cast::<()>();
    let registry = RawContainerReaderRegistry::with_readers([reader]);
    let found = registry.find_reader(b"header", ".cr2").expect("reader routes");
    assert_eq!((found as *const dyn RawContainerReader).cast::<()>(), registered_data);
    assert!(registry.find_reader(b"header", ".unknown").is_none());
}

// C#: RawContractsAndHeaderSourceTests.RawContainerReaderRegistry_DoesNotAcceptReservedFormats
#[test]
fn raw_container_reader_registry_does_not_accept_reserved_formats() {
    let registry = RawContainerReaderRegistry::new();
    let tiff_header = [0x49, 0x49, 0x2A, 0x00];
    assert!(registry.find_reader(&tiff_header, ".nrw").is_none());
    assert!(registry.find_reader(&tiff_header, ".pef").is_none());
}

#[test]
fn raw_container_reader_registry_new_has_eight_readers_in_pinned_order() {
    let registry = RawContainerReaderRegistry::new();
    assert_eq!(registry.len(), 8);
    assert!(!registry.is_empty());
    assert_eq!(
        DEFAULT_READER_ORDER,
        ["Cr2", "Nef", "Arw", "Dng", "Orf", "Rw2", "Cr3", "Raf"]
    );
}

#[test]
fn raw_container_reader_registry_with_empty_list_is_empty() {
    let registry = RawContainerReaderRegistry::with_readers([]);
    assert_eq!(registry.len(), 0);
    assert!(registry.is_empty());
    assert!(registry.find_reader(b"", ".cr2").is_none());
}

#[test]
fn raw_container_reader_registry_first_registered_matching_reader_wins() {
    let registry = RawContainerReaderRegistry::with_readers([
        Box::new(TestRawReader {
            extension: ".raw",
            marker: 1,
        }) as Box<dyn RawContainerReader>,
        Box::new(TestRawReader {
            extension: ".raw",
            marker: 2,
        }),
    ]);
    assert_eq!(registry.find_reader_index(b"", ".raw"), Some(0));
    assert!(registry.find_reader(b"", ".raw").is_some());
}

#[test]
fn raw_container_reader_registry_index_and_reader_agree() {
    let registry = RawContainerReaderRegistry::with_readers([
        Box::new(Cr2ContainerReader) as Box<dyn RawContainerReader>,
        Box::new(NefContainerReader),
    ]);
    for (extension, header) in [
        (".cr2", vec![0x49, 0x49, 0x2A, 0, 0, 0, 0, 0, b'C', b'R']),
        (".nef", vec![0x49, 0x49, 0x2A, 0]),
    ] {
        assert_eq!(
            registry.find_reader(&header, extension).is_some(),
            registry.find_reader_index(&header, extension).is_some()
        );
    }
}

#[test]
fn raw_container_reader_registry_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RawContainerReaderRegistry>();
}

#[test]
fn raw_container_reader_registry_matches_raw_registry_find_golden() {
    let inputs = registry_golden_inputs();
    let input_cases = inputs["cases"].as_array().expect("input cases");
    let golden = pr_test_support::golden::load("raw-registry-find");
    assert_eq!(input_cases.len(), golden.cases.len());

    for (input, expected) in input_cases.iter().zip(&golden.cases) {
        let name = input["name"].as_str().expect("case name");
        assert_eq!(expected["name"], name, "golden/input case order");
        if input["op"] == "default-order" {
            let order = expected["order"].as_array().expect("golden default order");
            let actual = DEFAULT_READER_ORDER.to_vec();
            let expected_order: Vec<_> = order.iter().map(|item| item.as_str().expect("order item")).collect();
            assert_eq!(actual, expected_order, "case {name}");
            continue;
        }

        let custom_order: Vec<String> = input["registry"]
            .as_array()
            .map(|formats| {
                formats
                    .iter()
                    .map(|value| value.as_str().expect("reader name").to_owned())
                    .collect()
            })
            .unwrap_or_else(|| DEFAULT_READER_ORDER.iter().map(|name| (*name).to_owned()).collect());
        let registry = if input["registry"].is_array() {
            RawContainerReaderRegistry::with_readers(custom_order.iter().map(|name| boxed_reader(name)))
        } else {
            RawContainerReaderRegistry::new()
        };
        let header = hex_bytes(input["header_hex"].as_str().expect("header_hex"));
        let extension = input["ext"].as_str().expect("extension");
        let actual_index = registry.find_reader_index(&header, extension);
        let expected_index = expected["index"].as_u64().map(|index| index as usize);
        assert_eq!(actual_index, expected_index, "case {name} index");
        assert_eq!(
            registry.find_reader(&header, extension).is_some(),
            actual_index.is_some(),
            "case {name} find/index"
        );
        let actual_reader = actual_index.map(|index| custom_order[index].as_str());
        assert_eq!(actual_reader, expected["reader"].as_str(), "case {name} reader");
    }
}

#[test]
fn adobe_rgb_profile_has_bundled_header_and_identity() {
    let profile = adobe_rgb_profile();
    assert_eq!(profile.len(), 374);
    assert_eq!(&profile[..4], &[0, 0, 1, 0x76]);
    let hash = profile.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    assert_eq!(format!("{hash:016x}"), "1d3e7e4f40c58953");
}

#[test]
fn raw_jpeg_icc_profile_matches_raw_icc_ensure_golden() {
    let inputs = icc_golden_inputs();
    let input_cases = inputs["cases"].as_array().expect("input cases");
    let golden = pr_test_support::golden::load("raw-icc-ensure");
    assert_eq!(input_cases.len(), golden.cases.len());

    for (input, expected) in input_cases.iter().zip(&golden.cases) {
        let name = input["name"].as_str().expect("case name");
        assert_eq!(expected["name"], name, "golden/input case order");
        if input["op"] == "profile" {
            assert_eq!(expected["profile_len"], adobe_rgb_profile().len());
            let profile = adobe_rgb_profile();
            let hash = profile.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(format!("{hash:016x}"), expected["profile_fnv1a64"]);
            continue;
        }
        assert_icc_case(name);
    }
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_UntaggedJpeg_InsertsOneExactApp2SegmentRightAfterSoi
#[test]
fn ensure_adobe_rgb_profile_untagged_jpeg_inserts_one_exact_app2_segment_right_after_soi() {
    let jpeg = [0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22, 0xFF, 0xD9];
    let output = ensure_adobe_rgb_profile(jpeg.to_vec()).expect("valid SOI gets the profile");
    let profile = adobe_rgb_profile();
    assert_eq!(&output[..6], &[0xFF, 0xD8, 0xFF, 0xE2, 0x01, 0x86]);
    assert_eq!(&output[6..18], &ICC_IDENTIFIER);
    assert_eq!(&output[18..20], &[1, 1]);
    assert_eq!(&output[20..20 + profile.len()], profile);
    assert_eq!(&output[20 + profile.len()..], &jpeg[2..]);
    assert_eq!(output.len(), jpeg.len() + 18 + profile.len());
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_UntaggedJpeg_DoesNotMutateTheInput
#[test]
fn ensure_adobe_rgb_profile_untagged_jpeg_does_not_mutate_the_input() {
    let input = hex_bytes("ffd8ffe000074a46494600ffda00021122ffd9");
    let snapshot = input.clone();
    let _output = insert_icc_app2(&input, adobe_rgb_profile()).expect("valid insertion");
    // The Rust helper borrows the source, so this snapshot comparison preserves the C# no-mutation guarantee.
    assert_eq!(input, snapshot);
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_IccAfterOtherSegments_ReturnsTheSameInstance
#[test]
fn ensure_adobe_rgb_profile_icc_after_other_segments_returns_the_same_instance() {
    assert_icc_case("icc-after-other-segments");
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_App2ThatIsNotIcc_StillGetsTheProfile
#[test]
fn ensure_adobe_rgb_profile_app2_that_is_not_icc_still_gets_the_profile() {
    assert_icc_case("app2-mpf-is-not-icc");
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_IccIdentifierInAnApp1Segment_IsNotCountedAsTagged
#[test]
fn ensure_adobe_rgb_profile_icc_identifier_in_app1_is_not_counted_as_tagged() {
    assert_icc_case("icc-identifier-in-app1-is-not-tagged");
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_IccAfterTheScanHeader_IsNotCountedAsTagged
#[test]
fn ensure_adobe_rgb_profile_icc_after_the_scan_header_is_not_counted_as_tagged() {
    assert_icc_case("icc-after-scan-header-is-not-tagged");
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_VeryShortInput_NeverThrowsAnythingButInvalidData
#[test]
fn ensure_adobe_rgb_profile_very_short_input_never_throws_anything_but_invalid_data() {
    for name in ["empty-input", "one-byte", "soi-only"] {
        assert_icc_case(name);
    }
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_NotAJpeg_ThrowsInvalidData
#[test]
fn ensure_adobe_rgb_profile_not_a_jpeg_throws_invalid_data() {
    for name in ["png", "swapped-soi", "zeros", "ff-then-eoi-is-not-soi"] {
        assert_icc_case(name);
    }
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_Null_ThrowsArgumentNull
// Rust has no null Vec value; the owned Vec API makes this C# null-argument case unrepresentable.

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_TruncatedOrImpossibleSegmentLength_DoesNotThrowAndIsNotTagged
#[test]
fn ensure_adobe_rgb_profile_truncated_or_impossible_segment_length_is_not_tagged() {
    for name in [
        "app2-length-0x4000-past-end",
        "app2-length-1-below-minimum",
        "app2-length-0",
        "app1-length-0xffff",
        "app2-length-one-past-the-end",
    ] {
        assert_icc_case(name);
    }
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_SegmentHeaderCutAfterTheMarker_DoesNotThrowAndIsNotTagged
#[test]
fn ensure_adobe_rgb_profile_segment_header_cut_after_marker_is_not_tagged() {
    for name in ["segment-header-cut-after-marker", "stream-ends-after-marker-byte"] {
        assert_icc_case(name);
    }
}

// C#: RawJpegIccProfileTests.EnsureAdobeRgbProfile_StandaloneMarkersAndFillBytesBeforeTheIccSegment_AreSkipped
#[test]
fn ensure_adobe_rgb_profile_standalone_markers_and_fill_bytes_before_icc_are_skipped() {
    assert_icc_case("standalone-markers-and-fill-before-icc");
}

// C#: RawJpegIccProfileMutationGapTests.EnsureAdobeRgbProfile_IccAfterASegmentLongerThan255Bytes_IsRecognizedAndLeftAlone
#[test]
fn ensure_adobe_rgb_profile_icc_after_a_segment_longer_than_255_bytes_is_recognized_and_left_alone() {
    assert_icc_case("icc-after-segment-longer-than-255");
}

// C#: RawJpegIccProfileMutationGapTests.EnsureAdobeRgbProfile_IccAfterARestart7Marker_IsRecognizedAndLeftAlone
#[test]
fn ensure_adobe_rgb_profile_icc_after_restart7_marker_is_recognized_and_left_alone() {
    assert_icc_case("icc-after-rst7");
}

// C#: RawJpegIccProfileMutationGapTests.EnsureAdobeRgbProfile_IccAfterAnEmptySegment_IsRecognizedAndLeftAlone
#[test]
fn ensure_adobe_rgb_profile_icc_after_empty_segment_is_recognized_and_left_alone() {
    assert_icc_case("icc-after-empty-segment");
}

// C#: RawJpegIccProfileMutationGapTests.EnsureAdobeRgbProfile_IccSegmentEndingExactlyAtTheEndOfTheStream_IsRecognizedAndLeftAlone
#[test]
fn ensure_adobe_rgb_profile_icc_segment_ending_exactly_at_end_is_recognized_and_left_alone() {
    assert_icc_case("icc-segment-ends-exactly-at-end");
}

// C#: RawJpegIccProfileMutationGapTests.EnsureAdobeRgbProfile_IccSegmentWithOnlyTheIdentifier_CountsAsPresent
#[test]
fn ensure_adobe_rgb_profile_icc_segment_with_only_identifier_counts_as_present() {
    assert_icc_case("icc-segment-only-identifier");
}

// C#: RawJpegIccProfileMutationGapTests.EnsureAdobeRgbProfile_StreamEndingInsideFillBytesOrAMarker_AddsTheProfileWithoutReadingPastTheEnd
#[test]
fn ensure_adobe_rgb_profile_stream_ending_inside_fill_bytes_or_marker_adds_profile_safely() {
    for name in ["stream-ends-inside-fill-then-marker", "fill-bytes-run-to-end"] {
        assert_icc_case(name);
    }
}

#[test]
fn insert_icc_app2_rejects_profile_larger_than_app2_limit() {
    let error = insert_icc_app2(&[0xFF, 0xD8], &vec![0; 70_000]).expect_err("oversized profile rejected");
    assert_eq!(error.to_string(), "Adobe RGB ICC profile exceeds JPEG APP2 limits.");
    assert!(!error.is_io_failure());
}

#[test]
fn insert_icc_app2_accepts_maximum_app2_segment_length() {
    let output = insert_icc_app2(&[0xFF, 0xD8], &vec![0; 65_519]).expect("65535-byte segment accepted");
    assert_eq!(&output[4..6], &[0xFF, 0xFF]);
    assert_eq!(output.len(), 2 + 18 + 65_519);
}

#[test]
fn has_embedded_icc_never_panics_on_prefixes_of_golden_jpegs() {
    let inputs = icc_golden_inputs();
    let cases = inputs["cases"].as_array().expect("input cases");
    for case in cases {
        let Some(hex) = case["jpeg_hex"].as_str() else {
            continue;
        };
        let jpeg = hex_bytes(hex);
        for end in 0..=jpeg.len() {
            let _ = has_embedded_icc(&jpeg[..end]);
        }
    }
}
