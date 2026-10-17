//! Golden twin of `LibRawDecoder` (W5-16c): every case of `tests/golden/libraw-decoder.json` (155, pure, NOT ignored) plus the
//! non-native rows of `tests/golden/libraw-decoder-native.json` that only need the `FormatError` table (`failure` 12 rows,
//! `check-result` 5 rows). Inputs are read from `tests/golden-inputs/<same file>.json` by case name; each trim input is checked
//! against the golden `input_length` / `input_sha256` first, so a drifted recipe fails loudly.
//!
//! Red tests: the stub bodies are `todo!()`, so every test here fails until the W5-16c porter fills them in.
//! The fake-backed guard rows (`decode-guard`, `read-info-guard`, `thumbnail-guard`) run over `support::FakeApi`, never over
//! `libraw.dll`.

mod support;

use std::sync::{Arc, Mutex};

use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_libraw::decoder::{
    CoreOverride, ERRNO_NO_MEMORY, LIBRAW_IMAGE_BITMAP, LIBRAW_IMAGE_JPEG, LIBRAW_IO_ERROR, LIBRAW_UNSUFFICIENT_MEMORY,
    LONG_PATH_THRESHOLD, LibRawDecoder, MAX_QUEUED_PRELOAD_DECODES, MAX_THUMBNAIL_BYTES, ProcessedImageHeader,
    TRAILING_EOI_SEARCH_WINDOW, check_result, create_failure, create_thumbnail_failure, find_end_of_image,
    is_downscaled, is_supported_bitmap, open_file_with, read_jpeg_thumbnail, to_extended_length_path,
    trim_to_end_of_image, validate_image_dimensions, validate_thumbnail_header,
};
use pr_libraw::full_decode_gate::FullDecodeGate;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

use support::{
    FakeApi, FakeCodec, Log, Script, assert_golden_throws, canceled_token, golden_format_error, golden_input,
    memory_source_never_read, priority_of, request, sha256_hex, trim_input_bytes,
};

const FILE: &str = "libraw-decoder";
const NATIVE_FILE: &str = "libraw-decoder-native";

fn cases(file: &str) -> Vec<Value> {
    golden::load(file).cases
}

fn header(image_type: i32, width: i64, height: i64, colors: i64, bits: i64, data_size: i64) -> ProcessedImageHeader {
    ProcessedImageHeader {
        image_type,
        width: u16::try_from(width).expect("golden width fits u16"),
        height: u16::try_from(height).expect("golden height fits u16"),
        colors: u16::try_from(colors).expect("golden colors fits u16"),
        bits: u16::try_from(bits).expect("golden bits fits u16"),
        data_size: u32::try_from(data_size).expect("golden data_size fits u32"),
    }
}

/// A decoder over fakes (codec, api, memory) with a PRIVATE gate: parallel tests never share slots (contract trap T9).
fn fake_decoder(api: Arc<dyn pr_libraw::decoder::LibRawApi>, core: Option<CoreOverride>) -> LibRawDecoder {
    let log = Log::default();
    let codec = Arc::new(FakeCodec { log, fail: false });
    let decoder = LibRawDecoder::new(codec, api, memory_source_never_read())
        .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES));
    match core {
        Some(core) => decoder.with_core_override(core),
        None => decoder,
    }
}

// C#: LibRawSeamGapTests (constants pinned by golden `constants`)
#[test]
fn golden_constants_match_the_csharp_values() {
    let golden = cases(FILE);
    let row = golden.iter().find(|c| c["op"] == "constants").expect("constants case");
    assert_eq!(i64::from(MAX_THUMBNAIL_BYTES), i64_of(row, "max_thumbnail_bytes"));
    assert_eq!(
        i64::from(MAX_QUEUED_PRELOAD_DECODES),
        i64_of(row, "max_queued_preload_decodes")
    );
    assert_eq!(LONG_PATH_THRESHOLD as i64, i64_of(row, "long_path_threshold"));
    assert_eq!(
        TRAILING_EOI_SEARCH_WINDOW as i64,
        i64_of(row, "trailing_eoi_search_window")
    );
    assert_eq!(
        ProcessedImageHeader::DATA_OFFSET as i64,
        i64_of(row, "header_data_offset")
    );
    assert_eq!(
        std::mem::size_of::<ProcessedImageHeader>() as i64,
        i64_of(row, "header_size")
    );
    assert_eq!(LIBRAW_IO_ERROR, -100_009);
    assert_eq!(LIBRAW_UNSUFFICIENT_MEMORY, -100_007);
    assert_eq!(ERRNO_NO_MEMORY, 12);
    assert_eq!(LIBRAW_IMAGE_JPEG, 1);
    assert_eq!(LIBRAW_IMAGE_BITMAP, 2);
}

// C#: LibRawThumbnailTrimMutationGapTests / LibRawManagedLogicTests (golden `trim-*`, 67 rows: input recipe from
// golden-inputs, length + sha256 checked first, then the exact trimmed length, same-allocation flag or the InvalidData kind).
#[test]
fn golden_trim_to_end_of_image_matches_every_case() {
    let mut checked = 0;
    for case in cases(FILE).iter().filter(|c| c["op"] == "trim") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        let bytes = trim_input_bytes(&input);
        assert_eq!(bytes.len() as i64, i64_of(case, "input_length"), "{name}: input length");
        if let Some(digest) = case["input_sha256"].as_str() {
            assert_eq!(sha256_hex(&bytes), digest, "{name}: input recipe drifted");
        }
        let before = bytes.as_ptr();
        match trim_to_end_of_image(bytes) {
            Ok(out) => {
                let ok = &case["ok"];
                assert_eq!(out.len() as i64, i64_of(ok, "length"), "{name}: trimmed length");
                assert_eq!(
                    std::ptr::eq(out.as_ptr(), before),
                    bool_of(ok, "same"),
                    "{name}: same-allocation flag"
                );
            }
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
        checked += 1;
    }
    assert_eq!(checked, 67, "golden trim row count");
}

// C#: LibRawSeamGapTests / LibRawDecoderRegressionTests (golden `trim-*`, pure `find_end_of_image` view of the same rows)
#[test]
fn golden_find_end_of_image_matches_every_trim_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "trim") {
        let name = str_of(case, "name");
        let bytes = trim_input_bytes(&golden_input(FILE, name));
        match find_end_of_image(&bytes) {
            Ok(length) => assert_eq!(length as i64, i64_of(&case["ok"], "length"), "{name}: end of image"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
    }
}

// C#: LibRawSeamGapTests / LibRawGateAndGuardMutationGapTests (golden `extended-path-*`, 25 rows). `null` = not converted.
#[test]
fn golden_to_extended_length_path_matches_every_case() {
    let mut checked = 0;
    for case in cases(FILE).iter().filter(|c| c["op"] == "extended-path") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        let path = input["path"].as_str().expect("extended-path input");
        let expected = case["value"].as_str().map(str::to_owned);
        assert_eq!(
            to_extended_length_path(path),
            expected,
            "{name}: extended form of {path:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 25, "golden extended-path row count");
}

// C#: LibRawSeamGapTests.ValidateThumbnailHeader_AcceptsOnlyAJpegOfBoundedSize (golden `thumbnail-header-*`, 11 rows)
#[test]
fn golden_validate_thumbnail_header_matches_every_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "thumbnail-header") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        let h = header(
            input["type"].as_i64().expect("type") as i32,
            input["width"].as_i64().unwrap_or(0),
            input["height"].as_i64().unwrap_or(0),
            input["colors"].as_i64().unwrap_or(0),
            input["bits"].as_i64().unwrap_or(0),
            input["data_size"].as_i64().expect("data_size"),
        );
        match validate_thumbnail_header(&h) {
            Ok(()) => assert!(case["ok"].as_bool() == Some(true), "{name}: expected a rejection"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
    }
}

// C#: LibRawSeamGapTests.ValidateImageDimensions_RejectsANonPositiveSide (golden `image-dimensions-*`, 9 rows)
#[test]
fn golden_validate_image_dimensions_matches_every_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "image-dimensions") {
        let name = str_of(case, "name");
        let w = i64_of(case, "w") as i32;
        let h = i64_of(case, "h") as i32;
        match validate_image_dimensions(w, h) {
            Ok(()) => assert!(case["ok"].as_bool() == Some(true), "{name}: expected a rejection"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
    }
}

// C#: LibRawSeamGapTests.IsSupportedBitmap_RequiresEveryPartOfTheHeaderToBeValid (golden `supported-bitmap-*`, 12 rows)
#[test]
fn golden_is_supported_bitmap_matches_every_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "supported-bitmap") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        let h = header(
            input["type"].as_i64().expect("type") as i32,
            input["width"].as_i64().expect("width"),
            input["height"].as_i64().expect("height"),
            input["colors"].as_i64().expect("colors"),
            input["bits"].as_i64().expect("bits"),
            input["data_size"].as_i64().expect("data_size"),
        );
        assert_eq!(is_supported_bitmap(&h), bool_of(case, "value"), "{name}");
    }
}

// C#: LibRawSeamGapTests.IsDownscaled_IsTrueWhenEitherSideIsSmallerThanTheSource (golden `downscaled-*`, 8 rows)
#[test]
fn golden_is_downscaled_matches_every_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "downscaled") {
        let name = str_of(case, "name");
        let got = is_downscaled(
            i64_of(case, "tw") as i32,
            i64_of(case, "th") as i32,
            i64_of(case, "sw") as i32,
            i64_of(case, "sh") as i32,
        );
        assert_eq!(got, bool_of(case, "value"), "{name}");
    }
}

// C#: LibRawManagedLogicTests.CreateFailure_* / CreateThumbnailFailure_* (golden `failure-memory-*`, 4 rows)
#[test]
fn golden_failure_memory_maps_resource_codes_to_invalid_operation() {
    let format = golden_format_error();
    for case in cases(FILE).iter().filter(|c| c["op"] == "failure-memory") {
        let name = str_of(case, "name");
        let code = i64_of(case, "code") as i32;
        let error = if case["thumbnail"].as_bool() == Some(true) {
            create_thumbnail_failure(code, &format)
        } else {
            create_failure(code, str_of(case, "operation"), &format)
        };
        assert_golden_throws(&error, &case["value"], name);
    }
}

// C#: LibRawSeamGapTests.CheckResult_* (golden `check-result-canceled-*`, 4 rows): a canceled token turns a failure into a
// cancellation; a zero result is success whatever the token state.
#[test]
fn golden_check_result_while_canceled_matches_every_case() {
    let format = golden_format_error();
    for case in cases(FILE).iter().filter(|c| c["op"] == "check-result-canceled") {
        let name = str_of(case, "name");
        let code = i64_of(case, "code") as i32;
        let token = canceled_token();
        match check_result(code, "process RAW data", &token, &format) {
            Ok(()) => assert!(case["ok"].as_bool() == Some(true), "{name}: expected a failure"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
    }
}

// C#: LibRawDecodeRequestGuardTests / LibRawGateAndGuardMutationGapTests (golden `decode-guard`, 9 rows). Fake api whose
// `libraw_init` must never run (the guards come first), core override as the body probe, private gate.
#[test]
fn golden_decode_guard_matches_every_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "decode-guard") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        let log = Log::default();
        let api = FakeApi::arc(
            Script {
                init_panics: true,
                ..Script::default()
            },
            &log,
        );
        let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
        let body = Arc::new(Mutex::new((false, None::<i32>)));
        let body_probe = Arc::clone(&body);
        let gate_probe = gate.clone();
        let core: CoreOverride = Arc::new(
            move |_req: &DecodeRequest,
                  _lease: &pr_libraw::full_decode_gate::FullDecodeLease,
                  _cancel: &CancelToken|
                  -> Result<Arc<dyn DecodedImageView>, DecodeError> {
                let mut state = body_probe.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.0 = true;
                state.1 = Some(gate_probe.slots_available());
                Err(DecodeError::InvalidData {
                    message: "stop here".to_owned(),
                })
            },
        );
        let decoder = LibRawDecoder::new(
            Arc::new(FakeCodec { log, fail: false }),
            api,
            memory_source_never_read(),
        )
        .with_gate(gate.clone())
        .with_core_override(core);

        let slots_before = gate.slots_available() as i64;
        assert_eq!(slots_before, i64_of(case, "slots_before"), "{name}: slots before");

        let mut req = request(str_of(&input, "path"));
        if input["apply_orientation"].as_bool() == Some(false) {
            req = req.with_apply_orientation(false);
        }
        req = req.with_priority(priority_of(input["priority"].as_str()));
        let token = if input["canceled"].as_bool() == Some(true) {
            canceled_token()
        } else {
            CancelToken::none()
        };

        let result = decoder.decode_with_cancel(&req, &token);
        let (ran, slots_in_body) = *body.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(ran, case["body_ran"].as_bool().expect("body_ran"), "{name}: body ran");
        if let Some(expected) = case["slots_in_body"].as_i64() {
            assert_eq!(
                slots_in_body.map(i64::from),
                Some(expected),
                "{name}: slots inside the body"
            );
        }
        assert_eq!(
            gate.slots_available() as i64,
            i64_of(case, "slots_after"),
            "{name}: slots after"
        );
        assert_eq!(
            i64::from(gate.queued_viewers() + gate.queued_preloads()),
            i64_of(case, "queued_after"),
            "{name}: queued waiters after"
        );
        match result {
            Ok(_) => panic!("{name}: the decode must not succeed"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
    }
}

// C#: LibRawSeamGapTests / LibRawManagedLogicTests guard (golden `read-info-guard`, 2 rows): blank path refused first.
#[test]
fn golden_read_info_blank_path_matches_every_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "read-info-guard") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        let log = Log::default();
        let decoder = fake_decoder(
            FakeApi::arc(
                Script {
                    init_panics: true,
                    ..Script::default()
                },
                &log,
            ),
            None,
        );
        match decoder.read_info(str_of(&input, "path")) {
            Ok(info) => panic!("{name}: expected a refusal, got {info:?}"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
    }
}

// C#: LibRawManagedLogicTests.ReadJpegThumbnail_AndDecode_WithCancelledToken_ThrowOperationCanceledBeforeOpeningTheFile
// (golden `thumbnail-guard`, 3 rows): the cancel check comes before `libraw_init`; the fake init panics if it runs.
#[test]
fn golden_read_jpeg_thumbnail_guard_matches_every_case() {
    for case in cases(FILE).iter().filter(|c| c["op"] == "thumbnail-guard") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        let log = Log::default();
        let api = FakeApi::new(
            Script {
                init_panics: true,
                ..Script::default()
            },
            &log,
        );
        let token = if input["canceled"].as_bool() == Some(true) {
            canceled_token()
        } else {
            CancelToken::none()
        };
        match read_jpeg_thumbnail(&api, str_of(&input, "path"), &token) {
            Ok(bytes) => panic!("{name}: expected a refusal, got {} bytes", bytes.len()),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
        assert!(log.lines().is_empty(), "{name}: no libraw call before the guard");
        let _ = case["slots_after"].as_i64();
    }
}

// C#: LibRawSeamGapTests.OpenFile_* (golden `libraw-decoder-native` `open-retry` rows, 9): the scripted open sequence of
// `open_file_with`, its calls and its result. Pure: the codes come from the golden, no libraw.dll.
#[test]
fn golden_open_retry_rows_match_every_case() {
    let format = golden_format_error();
    let mut checked = 0;
    for case in cases(NATIVE_FILE).iter().filter(|c| c["op"] == "open-retry") {
        let name = str_of(case, "name");
        let path = str_of(case, "path");
        let mut codes: std::collections::VecDeque<i32> = case["codes"]
            .as_array()
            .expect("codes")
            .iter()
            .map(|c| c.as_i64().expect("code") as i32)
            .collect();
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = Arc::clone(&calls);
        let mut open = move |p: &std::ffi::OsStr| {
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(p.to_string_lossy().into_owned());
            codes.pop_front().unwrap_or(0)
        };
        let result = open_file_with(&mut open, std::ffi::OsStr::new(path), &CancelToken::none(), &format);
        let expected_calls: Vec<String> = case["calls"]
            .as_array()
            .map(|a| a.iter().map(|c| c.as_str().expect("call").to_owned()).collect())
            .unwrap_or_default();
        assert_eq!(
            *calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
            expected_calls,
            "{name}: calls"
        );
        match result {
            Ok(()) => assert!(case["ok"].as_bool() == Some(true), "{name}: expected a failure"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
        checked += 1;
    }
    assert_eq!(checked, 9, "golden open-retry row count");
}

// C#: LibRawNativeFailureMapTests (golden `libraw-decoder-native` `failure` rows, 12): the FormatError table of the golden is the
// closure of the pure failure map. (`failure` rows need no libraw.dll: the table is the golden.)
#[test]
fn golden_native_failure_rows_map_through_the_format_error_table() {
    let format = golden_format_error();
    let mut checked = 0;
    for case in cases(NATIVE_FILE).iter().filter(|c| c["op"] == "failure") {
        let name = str_of(case, "name");
        let code = i64_of(case, "code") as i32;
        let error = if case["thumbnail"].as_bool() == Some(true) {
            create_thumbnail_failure(code, &format)
        } else {
            create_failure(code, str_of(case, "operation"), &format)
        };
        assert_golden_throws(&error, &case["value"], name);
        checked += 1;
    }
    assert_eq!(checked, 12, "native failure row count");
}

// C#: LibRawNativeFailureMapTests (golden `libraw-decoder-native` `check-result` rows, 5)
#[test]
fn golden_native_check_result_rows_match() {
    let format = golden_format_error();
    for case in cases(NATIVE_FILE).iter().filter(|c| c["op"] == "check-result") {
        let name = str_of(case, "name");
        let code = i64_of(case, "code") as i32;
        let operation = str_of(case, "operation");
        match check_result(code, operation, &CancelToken::none(), &format) {
            Ok(()) => assert!(case["ok"].as_bool() == Some(true), "{name}: expected a failure"),
            Err(error) => assert_golden_throws(&error, &case["throws"], name),
        }
    }
}

// C#: LibRawThumbnailTrimTests.UnpaddedJpeg_ReturnsSameArray (non-native): the golden `trim-minimal-unpadded-same-array`
// input comes back as the very same allocation. The hex in golden-inputs is the 163-byte minimal JPEG.
#[test]
fn unpadded_jpeg_comes_back_as_the_same_allocation() {
    let bytes = trim_input_bytes(&golden_input(FILE, "trim-minimal-unpadded-same-array"));
    let before_ptr = bytes.as_ptr();
    let before_cap = bytes.capacity();
    let out = trim_to_end_of_image(bytes).expect("an unpadded JPEG trims to itself");
    assert_eq!(out.as_ptr(), before_ptr, "same allocation");
    assert_eq!(out.capacity(), before_cap, "same capacity");
}

// C#: LibRawThumbnailTrimTests.ZeroPaddedJpeg_TrimsToEoiAndStaysDecodable (non-native; the WPF decode is dropped, only the
// byte equality with the unpadded stream is kept, see W5-16c notes).
#[test]
fn zero_padded_jpeg_trims_to_the_unpadded_stream() {
    let padded = trim_input_bytes(&golden_input(FILE, "trim-zero-padded-37"));
    let unpadded = trim_input_bytes(&golden_input(FILE, "trim-minimal-unpadded-same-array"));
    let trimmed = trim_to_end_of_image(padded).expect("zero padded JPEG trims");
    assert_eq!(trimmed, unpadded, "trim keeps exactly the JPEG bytes");
}

// The golden inputs and the golden cases describe the same 155 rows, row for row.
#[test]
fn golden_inputs_match_the_golden_cases_by_name() {
    let golden = cases(FILE);
    for case in golden.iter().filter(|c| c["op"] == "trim") {
        let name = str_of(case, "name");
        let input = golden_input(FILE, name);
        assert_eq!(str_of(&input, "op"), "trim", "{name}: op");
    }
    assert_eq!(golden.len(), 155, "pure golden case count");
}

// C#: LibRawSeamGapTests.ValidateThumbnailHeader_AcceptsOnlyAJpegOfBoundedSize (the accepted row, named for the JPEG type).
#[test]
fn jpeg_thumbnail_header_uses_the_jpeg_image_type() {
    let jpeg = header(LIBRAW_IMAGE_JPEG, 0, 0, 0, 0, 4);
    validate_thumbnail_header(&jpeg).expect("a 4-byte JPEG is accepted");
}
