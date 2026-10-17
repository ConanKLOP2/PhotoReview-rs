//! Port tests for `pr_libraw::decoder` (C# `PhotoReview.Imaging.LibRaw.LibRawDecoder`, baseline d92d6aae): the pure logic and the
//! fake-backed decode pipeline. NO `libraw.dll` is loaded here; every native call goes through the scripted fakes in
//! `support` (contract semantics 1: seams instead of P/Invoke).
//!
//! Covers `LibRawSeamGapTests`, `LibRawAdmissionSeamTests`, `LibRawDecodeRequestGuardTests`,
//! `LibRawGateAndGuardMutationGapTests` (the non-native rows), `LibRawManagedLogicTests`,
//! `LibRawThumbnailTrimMutationGapTests`, `LibRawOpenOrderTests` / `LibRawGateSlotNativeTests` (fake twins),
//! `Pixels/LibRawPixelBufferHandOffTests`, and the Rust-only property tests of contract trap T11.
//!
//! Red tests: the stub bodies are `todo!()`, so every test here fails until the W5-16c porter fills them in.

mod support;

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use pr_core::abstractions::SourceReadPriority;
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView};
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::fallback::FallbackImageDecoder;
use pr_imaging::memory_headroom::{MemoryInfoSource, has_headroom};
use pr_imaging::pixel_memory::NativePixelMemory;
use pr_imaging::pixels::PixelBufferError;
use pr_libraw::decoder::{
    CoreOverride, EMPTY_OR_WHITE_SPACE_MESSAGE, ERRNO_NO_MEMORY, INCOMPLETE_JPEG_MESSAGE, INIT_FAILED_MESSAGE,
    INVALID_DIMENSIONS_MESSAGE, LIBRAW_IMAGE_BITMAP, LIBRAW_IMAGE_JPEG, LIBRAW_IO_ERROR, LIBRAW_UNSUFFICIENT_MEMORY,
    LONG_PATH_THRESHOLD, LibRawDecoder, MAX_QUEUED_PRELOAD_DECODES, MAX_THUMBNAIL_BYTES, NOT_ENOUGH_MEMORY_MESSAGE,
    ORIENTATION_REQUIRED_MESSAGE, ProcessedImageHeader, STAGE_CONFIGURED, STAGE_HANDLE_CLOSED, STAGE_PROCESSED,
    STAGE_UNPACKED, TRAILING_EOI_SEARCH_WINDOW, check_result, create_failure, create_thumbnail_failure,
    find_end_of_image, is_downscaled, is_supported_bitmap, map_native_error, map_resample_error, open_file_with,
    read_jpeg_thumbnail, to_extended_length_path, trim_to_end_of_image, validate_image_dimensions,
    validate_thumbnail_header,
};
use pr_libraw::full_decode_gate::{FullDecodeGate, FullDecodeLease, GateError};
use pr_libraw::memory_guard::{RawStructure, classify, estimate_peak_bytes};
use pr_libraw::native::{LoadError, NativeError};
use pr_libraw::resampler::ResampleError;

use support::{
    ExpectError, FakeApi, FakeCodec, FakeProbe, FakeStaged, Log, Script, WAIT_LIMIT, global_gate_lock, kind_name,
    memory_source, memory_source_never_read, pixel_memory_exclusive, pixel_memory_shared, request, select_corpus_files,
    stage_logger, trim_case_bytes, xorshift32_bytes,
};

// ------------------------------------------------------------------------------------------------ helpers

const GIB: i64 = 1 << 30;
/// Full 11600 x 8700 frame: 100 920 000 pixels (see the admission arithmetic in `admit_*` below).
const FULL_W: i32 = 11_600;
const FULL_H: i32 = 8_700;
const MOSAIC: u32 = 0x9494_9494;

/// A decoder over fakes with a PRIVATE gate (contract trap T9: parallel tests never share slots).
fn private_decoder(
    api: Arc<dyn pr_libraw::decoder::LibRawApi>,
    memory: Arc<dyn MemoryInfoSource>,
    log: &Log,
) -> LibRawDecoder {
    let codec = Arc::new(FakeCodec {
        log: log.clone(),
        fail: false,
    });
    LibRawDecoder::new(codec, api, memory).with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
}

/// `open_wfile` / `open_buffer` double: records every path and returns `codes` in order (then `0`).
fn scripted_open(codes: Vec<i32>) -> (Arc<Mutex<Vec<String>>>, impl FnMut(&OsStr) -> i32) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&calls);
    let mut codes = VecDeque::from(codes);
    let open = move |path: &OsStr| {
        seen.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(path.to_string_lossy().into_owned());
        codes.pop_front().unwrap_or(0)
    };
    (calls, open)
}

/// The `FormatError` double used by the pure failure tests: `format_error(-100009)` appears verbatim in messages.
fn fake_format(code: i32) -> String {
    format!("format_error({code})")
}

/// A `DecodeRequest` path that is valid (not blank) and long enough to hit the extended-length retry.
fn long_path(prefix: &str) -> String {
    format!("{prefix}{}\\photo.cr2", "a".repeat(260))
}

/// A `CoreOverride` that returns `error` without touching LibRaw.
fn failing_core(error: fn() -> DecodeError) -> CoreOverride {
    Arc::new(
        move |_req: &DecodeRequest,
              _lease: &FullDecodeLease,
              _cancel: &CancelToken|
              -> Result<Arc<dyn DecodedImageView>, DecodeError> { Err(error()) },
    )
}

/// The `Script` of a full, bounded-size Bayer decode (what a real 11600 x 8700 mosaic reports).
fn full_bayer_script() -> Script {
    Script {
        width: FULL_W,
        height: FULL_H,
        raw_width: FULL_W,
        raw_height: FULL_H,
        decoder_name: Some("unpacked_load_raw()".to_owned()),
        structure: Some(RawStructure::new(3, MOSAIC)),
        ..Script::default()
    }
}

/// Lease of a free slot of a private gate (the admission tests need one, never the process-wide one).
fn free_lease(gate: &FullDecodeGate) -> FullDecodeLease {
    gate.enter(SourceReadPriority::Viewer, &CancelToken::none())
        .expect("a free slot of a private gate")
}

// ------------------------------------------------------------------------------------------------ LibRawSeamGapTests

// C#: LibRawSeamGapTests.ValidateThumbnailHeader_AcceptsOnlyAJpegOfBoundedSize (6 rows)
#[test]
fn validate_thumbnail_header_accepts_only_a_jpeg_of_bounded_size() {
    let rows: [(i32, u32, bool); 6] = [
        (LIBRAW_IMAGE_JPEG, 4, true),
        (LIBRAW_IMAGE_JPEG, 3, false),
        (LIBRAW_IMAGE_JPEG, MAX_THUMBNAIL_BYTES, true),
        (LIBRAW_IMAGE_JPEG, MAX_THUMBNAIL_BYTES + 1, false),
        (LIBRAW_IMAGE_BITMAP, 100, false),
        (0, 100, false),
    ];
    for (image_type, data_size, accepted) in rows {
        let header = ProcessedImageHeader {
            image_type,
            data_size,
            ..ProcessedImageHeader::default()
        };
        assert_eq!(
            validate_thumbnail_header(&header).is_ok(),
            accepted,
            "type {image_type}, {data_size} bytes"
        );
    }
}

// C#: LibRawSeamGapTests.ValidateImageDimensions_RejectsANonPositiveSide (7 rows)
#[test]
fn validate_image_dimensions_rejects_a_non_positive_side() {
    let rows = [(0, 1), (1, 0), (-1, 1), (1, -1), (0, 0), (i32::MIN, 5), (5, i32::MIN)];
    for (width, height) in rows {
        let error = validate_image_dimensions(width, height).expect_error("a non-positive side is refused");
        assert_eq!(kind_name(&error), "InvalidData");
        assert_eq!(error.message(), INVALID_DIMENSIONS_MESSAGE);
    }
}

// C#: LibRawSeamGapTests.IsSupportedBitmap_RequiresEveryPartOfTheHeaderToBeValid
#[test]
fn is_supported_bitmap_requires_every_part_of_the_header_to_be_valid() {
    let valid = ProcessedImageHeader {
        image_type: LIBRAW_IMAGE_BITMAP,
        width: 10,
        height: 10,
        colors: 3,
        bits: 8,
        data_size: 300,
    };
    assert!(is_supported_bitmap(&valid), "the valid RGB bitmap is supported");
    let broken = [
        ProcessedImageHeader {
            image_type: LIBRAW_IMAGE_JPEG,
            ..valid
        },
        ProcessedImageHeader { width: 0, ..valid },
        ProcessedImageHeader { height: 0, ..valid },
        ProcessedImageHeader { colors: 2, ..valid },
        ProcessedImageHeader { colors: 0, ..valid },
        ProcessedImageHeader { bits: 16, ..valid },
        ProcessedImageHeader { bits: 0, ..valid },
    ];
    for header in broken {
        assert!(!is_supported_bitmap(&header), "{header:?} is not a supported bitmap");
    }
}

// C#: LibRawSeamGapTests.IsDownscaled_IsTrueWhenEitherSideIsSmallerThanTheSource (6 rows)
#[test]
fn is_downscaled_is_true_when_either_side_is_smaller_than_the_source() {
    assert!(!is_downscaled(100, 100, 100, 100));
    assert!(is_downscaled(99, 100, 100, 100));
    assert!(is_downscaled(100, 99, 100, 100));
    assert!(is_downscaled(50, 50, 100, 100));
    assert!(!is_downscaled(101, 100, 100, 100));
    assert!(!is_downscaled(100, 101, 100, 100));
}

// C#: LibRawSeamGapTests.OpenFile_Success_OpensOnce
#[test]
fn open_file_success_opens_once() {
    let (calls, mut open) = scripted_open(vec![0]);
    open_file_with(
        &mut open,
        OsStr::new("C:\\short.cr3"),
        &CancelToken::none(),
        &fake_format,
    )
    .expect("open succeeds");
    assert_eq!(calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len(), 1);
}

// C#: LibRawSeamGapTests.OpenFile_IoErrorForAShortPath_FailsWithoutRetry
#[test]
fn open_file_io_error_for_a_short_path_fails_without_retry() {
    let (calls, mut open) = scripted_open(vec![LIBRAW_IO_ERROR]);
    let error = open_file_with(
        &mut open,
        OsStr::new("C:\\short.cr3"),
        &CancelToken::none(),
        &fake_format,
    )
    .expect_error("the I/O error surfaces");
    assert_eq!(kind_name(&error), "InvalidData");
    assert_eq!(
        calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len(),
        1,
        "no retry for a short path"
    );
}

// C#: LibRawSeamGapTests.OpenFile_IoErrorForALongPath_RetriesInExtendedLengthForm
#[test]
fn open_file_io_error_for_a_long_path_retries_in_extended_length_form() {
    let path = long_path("C:\\");
    let (calls, mut open) = scripted_open(vec![LIBRAW_IO_ERROR, 0]);
    open_file_with(&mut open, OsStr::new(&path), &CancelToken::none(), &fake_format).expect("the retry succeeds");
    let calls = calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    assert_eq!(calls.len(), 2, "exactly one retry");
    assert!(
        calls[1].starts_with(r"\\?\"),
        "retry uses the extended-length form: {}",
        calls[1]
    );
}

// C#: LibRawSeamGapTests.OpenFile_RetryAlsoFails_ReportsTheOriginalError (message carries format_error(-100009))
#[test]
fn open_file_retry_also_fails_reports_the_original_error() {
    let path = long_path("C:\\");
    let (calls, mut open) = scripted_open(vec![LIBRAW_IO_ERROR, LIBRAW_IO_ERROR]);
    let error = open_file_with(&mut open, OsStr::new(&path), &CancelToken::none(), &fake_format)
        .expect_error("both attempts fail");
    assert_eq!(calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len(), 2);
    assert_eq!(kind_name(&error), "InvalidData");
    assert!(
        error.message().contains("format_error(-100009)"),
        "original code reported: {}",
        error.message()
    );
}

// C#: LibRawSeamGapTests.OpenFile_NonIoErrorForALongPath_IsNotRetried
#[test]
fn open_file_non_io_error_for_a_long_path_is_not_retried() {
    let path = long_path("C:\\");
    let (calls, mut open) = scripted_open(vec![-2]);
    open_file_with(&mut open, OsStr::new(&path), &CancelToken::none(), &fake_format).expect_error("not a file error");
    assert_eq!(calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len(), 1);
}

// C#: LibRawSeamGapTests.CheckResult_ZeroIsSuccess_AnythingElseFails
#[test]
fn check_result_zero_is_success_anything_else_fails() {
    check_result(0, "unpack RAW data", &CancelToken::none(), &fake_format).expect("zero is success");
    for code in [1, -1, -100_002] {
        let error = check_result(code, "unpack RAW data", &CancelToken::none(), &fake_format)
            .expect_error("a non-zero code fails");
        assert_eq!(kind_name(&error), "InvalidData", "code {code}");
    }
}

// C#: LibRawSeamGapTests.CheckResult_FailureWhileCancelled_IsACancellation
#[test]
fn check_result_failure_while_cancelled_is_a_cancellation() {
    let token = CancelToken::new();
    token.cancel();
    let error = check_result(1, "process RAW data", &token, &fake_format).expect_error("canceled failure");
    assert_eq!(kind_name(&error), "Canceled");
}

// C#: LibRawSeamGapTests.Decode_OutOfMemoryInTheDecodeBody_BecomesAnInvalidOperation (no inner error in Rust)
#[test]
fn decode_out_of_memory_in_the_decode_body_becomes_an_invalid_operation() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let core = failing_core(|| DecodeError::OutOfMemory {
        message: "body ran out of memory".to_owned(),
    });
    let decoder = private_decoder(FakeApi::arc(Script::default(), &log), memory_source_never_read(), &log)
        .with_core_override(core);
    let error = decoder
        .decode_with_cancel(&request("C:\\photos\\oom.cr3"), &CancelToken::none())
        .expect_error("out of memory is refused");
    assert_eq!(kind_name(&error), "InvalidOperation");
    assert_eq!(error.message(), NOT_ENOUGH_MEMORY_MESSAGE);
}

// Rust-only: the fake session records the CancelToken given to set_progress_handler; canceling the caller's token makes the
// recorded one canceled, i.e. it is a clone sharing the flag, not a fresh token.
#[test]
fn decode_and_thumbnail_install_the_callers_token() {
    let log = Log::default();
    let api = FakeApi::arc(Script::default(), &log);
    let caller = CancelToken::new();
    let thumbnail = read_jpeg_thumbnail(&*api, "C:\\photos\\x.orf", &caller).expect("the fake thumbnail is a JPEG");
    assert_eq!(thumbnail, vec![0xFF, 0xD8, 0xFF, 0xD9]);
    let recorded = log.tokens();
    assert_eq!(recorded.len(), 1, "one progress handler installed");
    caller.cancel();
    assert!(
        recorded[0].is_canceled(),
        "the handler sees the caller's cancellation (shared flag)"
    );
}

// Rust-only: the 3 `NativeError` shapes map to the DecodeError kinds; only the DLL failures stay fallbackable (INV-12).
#[test]
fn map_native_error_keeps_dll_failures_fallbackable() {
    let dll = NativeError::Unavailable(LoadError::LoadFailed {
        path: PathBuf::from("libraw.dll"),
        os_error: 126,
    });
    let entry = NativeError::Unavailable(LoadError::MissingExport { name: "libraw_init" });
    let init = NativeError::InitReturnedNull;

    let dll_error = map_native_error(&dll);
    let entry_error = map_native_error(&entry);
    let init_error = map_native_error(&init);

    assert_eq!(kind_name(&dll_error), "DllNotFound");
    assert_eq!(kind_name(&entry_error), "EntryPointNotFound");
    assert_eq!(kind_name(&init_error), "InvalidOperation");
    assert_eq!(init_error.message(), INIT_FAILED_MESSAGE);
    assert!(FallbackImageDecoder::is_fallbackable(&dll_error));
    assert!(FallbackImageDecoder::is_fallbackable(&entry_error));
    assert!(!FallbackImageDecoder::is_fallbackable(&init_error));
}

// C#: LibRawSeamGapTests / RgbBgraResamplerTests error mapping (`map_resample_error_*`, every ResampleError variant)
#[test]
fn map_resample_error_maps_every_variant() {
    assert_eq!(
        kind_name(&map_resample_error(ResampleError::InvalidData("bad".to_owned()))),
        "InvalidData"
    );
    assert_eq!(kind_name(&map_resample_error(ResampleError::Canceled)), "Canceled");
    assert_eq!(kind_name(&map_resample_error(ResampleError::Overflow)), "Overflow");
    assert_eq!(kind_name(&map_resample_error(ResampleError::Disposed)), "Disposed");
    let oom = map_resample_error(ResampleError::PixelBuffer(PixelBufferError::OutOfMemory {
        byte_count: 1 << 40,
    }));
    assert_eq!(
        kind_name(&oom),
        "OutOfMemory",
        "a PixelBuffer OOM is the managed OutOfMemory"
    );
    let other = map_resample_error(ResampleError::PixelBuffer(PixelBufferError::Disposed));
    assert_eq!(kind_name(&other), "InvalidArgument");
}

// ------------------------------------------------------------------------------------------------ LibRawAdmissionSeamTests

// C#: LibRawAdmissionSeamTests.AdmitDecode_RefusedByTheMemoryGuard_DoesNotMarkTheLeaseWorked
#[test]
fn admit_decode_refused_by_the_memory_guard_does_not_mark_the_lease_worked() {
    let log = Log::default();
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder =
        private_decoder(FakeApi::arc(Script::default(), &log), memory_source(GIB, 0), &log).with_gate(gate.clone());
    let probe = FakeProbe::bayer(FULL_W, FULL_H, &log);
    let lease = free_lease(&gate);
    let error = decoder
        .admit_decode(&probe, &request("C:\\photos\\big.cr3"), &lease)
        .expect_error("1 GiB cannot hold a 100 MP mosaic decode");
    assert_eq!(kind_name(&error), "InvalidOperation");
    assert_eq!(error.message(), NOT_ENOUGH_MEMORY_MESSAGE);
    assert!(!lease.is_worked(), "a refusal does not age the queued preloads");
}

// C#: LibRawAdmissionSeamTests.AdmitDecode_PassingTheMemoryGuard_MarksTheLeaseWorked
#[test]
fn admit_decode_passing_the_memory_guard_marks_the_lease_worked() {
    let log = Log::default();
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder = private_decoder(FakeApi::arc(Script::default(), &log), memory_source(64 * GIB, 0), &log)
        .with_gate(gate.clone());
    let probe = FakeProbe::bayer(FULL_W, FULL_H, &log);
    let lease = free_lease(&gate);
    decoder
        .admit_decode(&probe, &request("C:\\photos\\big.cr3"), &lease)
        .expect("64 GiB holds the decode");
    assert!(lease.is_worked());
}

// C#: LibRawAdmissionSeamTests.AdmitDecode_UnknownSize_MarksTheLeaseWorkedWithoutEstimating
#[test]
fn admit_decode_unknown_size_marks_the_lease_worked_without_estimating() {
    let log = Log::default();
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder = private_decoder(FakeApi::arc(Script::default(), &log), memory_source_never_read(), &log)
        .with_gate(gate.clone());
    let probe = FakeProbe::bayer(0, FULL_H, &log);
    let lease = free_lease(&gate);
    decoder
        .admit_decode(&probe, &request("C:\\photos\\unknown.cr3"), &lease)
        .expect("an unknown size is admitted");
    assert!(lease.is_worked());
    assert!(
        !log.lines()
            .iter()
            .any(|line| line == "probe:decoder_name" || line == "probe:structure"),
        "nothing else is read when the size is unknown"
    );
}

// C#: LibRawAdmissionSeamTests.AdmitDecode_ClassifiesByTheHandlesDecoderNameAndStructure (3 rows). The expected refusal is the
// pure guard's own decision for the family classify() picks, so the test follows the W5-16b rules exactly.
#[test]
fn admit_decode_classifies_by_the_handles_decoder_name_and_structure() {
    let rows: [(Option<&str>, Option<RawStructure>); 3] = [
        (Some("unpacked_load_raw()"), Some(RawStructure::new(3, MOSAIC))),
        (Some("lossless_dng_load_raw()"), Some(RawStructure::new(3, 0))),
        (None, None),
    ];
    for (name, structure) in rows {
        let log = Log::default();
        let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
        let decoder =
            private_decoder(FakeApi::arc(Script::default(), &log), memory_source(GIB, 0), &log).with_gate(gate.clone());
        let probe = FakeProbe {
            width: FULL_W,
            height: FULL_H,
            raw_width: FULL_W,
            raw_height: FULL_H,
            decoder_name: name.map(str::to_owned),
            structure,
            log: log.clone(),
        };
        let family = classify(name, structure);
        let estimate = estimate_peak_bytes(FULL_W, FULL_H, FULL_W, FULL_H, FULL_W, FULL_H, family);
        let lease = free_lease(&gate);
        let admitted = decoder
            .admit_decode(&probe, &request("C:\\photos\\row.cr3"), &lease)
            .is_ok();
        assert_eq!(
            admitted,
            has_headroom(estimate, GIB, 0),
            "decision for {name:?} / {structure:?}"
        );
        let reads = log.lines();
        assert!(reads.iter().any(|l| l == "probe:decoder_name"), "decoder name is read");
        assert!(reads.iter().any(|l| l == "probe:structure"), "structure is read");
    }
}

// C#: LibRawAdmissionSeamTests.AdmitDecode_DngWithANonMosaicStructure_IsRefusedButAnUnrelatedDecoderIsNot
// Budget 2.0e9 bytes: a DNG without a mosaic is Linear (23 bytes/px = 2.32e9, refused), an unrelated decoder without a mosaic
// stays Bayer (17 bytes/px = 1.72e9, admitted).
#[test]
fn admit_decode_dng_with_a_non_mosaic_structure_is_refused_but_an_unrelated_decoder_is_not() {
    let budget = 2_000_000_000;
    let log = Log::default();
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder =
        private_decoder(FakeApi::arc(Script::default(), &log), memory_source(budget, 0), &log).with_gate(gate.clone());
    let no_mosaic = RawStructure::new(3, 0);

    let dng = FakeProbe {
        width: FULL_W,
        height: FULL_H,
        raw_width: FULL_W,
        raw_height: FULL_H,
        decoder_name: Some("lossless_dng_load_raw()".to_owned()),
        structure: Some(no_mosaic),
        log: log.clone(),
    };
    let dng_lease = free_lease(&gate);
    assert!(
        decoder
            .admit_decode(&dng, &request("C:\\photos\\a.dng"), &dng_lease)
            .is_err(),
        "DNG refused"
    );

    let unrelated = FakeProbe {
        decoder_name: Some("kodak_load_raw()".to_owned()),
        ..dng
    };
    // The private gate has one slot: release the first lease before the second one is requested.
    drop(dng_lease);
    let other_lease = free_lease(&gate);
    assert!(
        decoder
            .admit_decode(&unrelated, &request("C:\\photos\\b.cr3"), &other_lease)
            .is_ok(),
        "an unrelated decoder without a mosaic is admitted"
    );
}

// ------------------------------------------------------------------------------------------------ LibRawDecodeRequestGuardTests

// C#: LibRawDecodeRequestGuardTests.Decode_ApplyOrientationFalse_IsRefusedBeforeTheGateAndTheBody
#[test]
fn decode_apply_orientation_false_is_refused_before_the_gate_and_the_body() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let body_ran = Arc::new(Mutex::new(false));
    let probe = Arc::clone(&body_ran);
    let core: CoreOverride = Arc::new(
        move |_req: &DecodeRequest,
              _lease: &FullDecodeLease,
              _cancel: &CancelToken|
              -> Result<Arc<dyn DecodedImageView>, DecodeError> {
            *probe.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
            Err(DecodeError::InvalidData {
                message: "stop here".to_owned(),
            })
        },
    );
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder = private_decoder(
        FakeApi::arc(
            Script {
                init_panics: true,
                ..Script::default()
            },
            &log,
        ),
        memory_source_never_read(),
        &log,
    )
    .with_gate(gate.clone())
    .with_core_override(core);
    let req = request("C:\\photos\\x.cr3").with_apply_orientation(false);
    let error = decoder
        .decode_with_cancel(&req, &CancelToken::none())
        .expect_error("un-oriented decode refused");
    assert_eq!(kind_name(&error), "NotSupported");
    assert_eq!(error.message(), ORIENTATION_REQUIRED_MESSAGE);
    assert!(
        !*body_ran.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        "the body never runs"
    );
    assert_eq!(gate.slots_available(), 1, "the gate was never entered");
}

// C#: LibRawDecodeRequestGuardTests.Decode_BlankPath_IsRejectedBeforeTheBody (2 rows)
#[test]
fn decode_blank_path_is_rejected_before_the_body() {
    let _pixels = pixel_memory_shared();
    for blank in ["", "   "] {
        let log = Log::default();
        let decoder = private_decoder(
            FakeApi::arc(
                Script {
                    init_panics: true,
                    ..Script::default()
                },
                &log,
            ),
            memory_source_never_read(),
            &log,
        );
        let error = decoder
            .decode_with_cancel(&request(blank), &CancelToken::none())
            .expect_error("a blank path is refused");
        assert_eq!(kind_name(&error), "InvalidArgument");
        assert_eq!(
            error.message(),
            format!("{EMPTY_OR_WHITE_SPACE_MESSAGE} (Parameter 'request.Path')")
        );
    }
}

// C#: LibRawDecodeRequestGuardTests.Decode_ValidRequest_RunsTheBodyUnderTheGateAndReleasesTheSlot (process-wide gate)
#[test]
fn decode_valid_request_runs_the_body_under_the_gate_and_releases_the_slot() {
    let _pixels = pixel_memory_shared();
    let _lock = global_gate_lock();
    let log = Log::default();
    let seen = Arc::new(Mutex::new(None::<i32>));
    let probe = Arc::clone(&seen);
    let core: CoreOverride = Arc::new(
        move |_req: &DecodeRequest,
              _lease: &FullDecodeLease,
              _cancel: &CancelToken|
              -> Result<Arc<dyn DecodedImageView>, DecodeError> {
            *probe.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(LibRawDecoder::full_decode_slots_available());
            Err(DecodeError::InvalidData {
                message: "stop here".to_owned(),
            })
        },
    );
    let codec = Arc::new(FakeCodec {
        log: log.clone(),
        fail: false,
    });
    let decoder = LibRawDecoder::new(codec, FakeApi::arc(Script::default(), &log), memory_source_never_read())
        .with_core_override(core);
    let _ = decoder.decode_with_cancel(&request("C:\\photos\\valid.cr3"), &CancelToken::none());
    assert_eq!(
        *seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        Some(0),
        "the body held the slot"
    );
    assert_eq!(
        LibRawDecoder::full_decode_slots_available(),
        1,
        "the slot is released afterwards"
    );
}

// C#: LibRawDecodeRequestGuardTests / Rust-only: an un-oriented request does not reach the gate (contract semantics 2, step 2).
#[test]
fn decode_rejects_request_that_would_leave_orientation_unapplied() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let decoder = private_decoder(
        FakeApi::arc(
            Script {
                init_panics: true,
                ..Script::default()
            },
            &log,
        ),
        memory_source_never_read(),
        &log,
    );
    let error = decoder
        .decode_with_cancel(
            &request("C:\\photos\\raw.orf").with_apply_orientation(false),
            &CancelToken::none(),
        )
        .expect_error("refused");
    assert_eq!(kind_name(&error), "NotSupported");
    assert!(log.lines().is_empty(), "libraw_init is never called");
}

// ------------------------------------------------------------------------------------------------ gate and guard mutation gap

// C#: LibRawGateAndGuardMutationGapTests.AdmitDecode_AnUnknownImageSizeUnderMemoryPressure_IsAdmittedWithoutAnEstimate (3 rows)
#[test]
fn admit_decode_an_unknown_image_size_under_memory_pressure_is_admitted_without_an_estimate() {
    let rows = [(0, FULL_H), (FULL_W, 0), (0, 0)];
    for (width, height) in rows {
        let log = Log::default();
        let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
        let decoder = private_decoder(FakeApi::arc(Script::default(), &log), memory_source_never_read(), &log)
            .with_gate(gate.clone());
        let probe = FakeProbe::bayer(width, height, &log);
        let lease = free_lease(&gate);
        decoder
            .admit_decode(&probe, &request("C:\\photos\\unknown.cr3"), &lease)
            .expect("nothing to estimate from: admitted");
        assert!(lease.is_worked(), "{width}x{height}: the lease is marked worked");
    }
}

// C#: LibRawGateAndGuardMutationGapTests.FullDecodeQueuedWaiters_ViewerAndPreloadWaiting_CountsBothLanes (private gate twin).
// The hook sends on a channel when a waiter queues (no polling); the holder releases through the channel-free drop.
#[test]
fn full_decode_queued_waiters_viewer_and_preload_waiting_counts_both_lanes() {
    let (tx, rx) = mpsc::channel::<SourceReadPriority>();
    let gate = FullDecodeGate::with_waiter_queued(
        MAX_QUEUED_PRELOAD_DECODES,
        Arc::new(move |priority: SourceReadPriority| tx.send(priority).map_err(|e| e.to_string())),
    );
    let holder = gate
        .enter(SourceReadPriority::Viewer, &CancelToken::none())
        .expect("the holder takes the only slot");

    let viewer_gate = gate.clone();
    let viewer = thread::spawn(move || {
        viewer_gate
            .enter(SourceReadPriority::Viewer, &CancelToken::none())
            .map(drop)
    });
    let preload_gate = gate.clone();
    let preload = thread::spawn(move || {
        preload_gate
            .enter(SourceReadPriority::Preload, &CancelToken::none())
            .map(drop)
    });

    let mut queued = vec![
        rx.recv_timeout(WAIT_LIMIT).expect("first waiter queued"),
        rx.recv_timeout(WAIT_LIMIT).expect("second waiter queued"),
    ];
    queued.sort_by_key(|p| matches!(p, SourceReadPriority::Preload));
    assert_eq!(queued, vec![SourceReadPriority::Viewer, SourceReadPriority::Preload]);
    assert_eq!(gate.queued_viewers(), 1, "viewer lane");
    assert_eq!(gate.queued_preloads(), 1, "preload lane");

    drop(holder);
    viewer.join().expect("viewer thread").expect("viewer granted");
    preload.join().expect("preload thread").expect("preload granted");
    assert_eq!(gate.queued_viewers() + gate.queued_preloads(), 0);
}

// C#: LibRawGateAndGuardMutationGapTests (global-gate check after the three decodes): the process-wide counters are idle.
#[test]
fn full_decode_global_gate_counters_are_idle_after_the_decodes() {
    let _lock = global_gate_lock();
    assert_eq!(LibRawDecoder::full_decode_queued_waiters(), 0);
    assert_eq!(LibRawDecoder::full_decode_slots_available(), 1);
}

// ------------------------------------------------------------------------------------------------ LibRawManagedLogicTests

// C#: LibRawManagedLogicTests.CreateThumbnailFailure_InsufficientMemory_IsResourceErrorNotCorruptFile (2 rows)
#[test]
fn create_thumbnail_failure_insufficient_memory_is_resource_error_not_corrupt_file() {
    for code in [LIBRAW_UNSUFFICIENT_MEMORY, ERRNO_NO_MEMORY] {
        let error = create_thumbnail_failure(code, &fake_format);
        assert_eq!(kind_name(&error), "InvalidOperation", "code {code}");
        assert_eq!(error.message(), NOT_ENOUGH_MEMORY_MESSAGE);
    }
}

// C#: LibRawManagedLogicTests.CreateThumbnailFailure_DataError_StaysInvalidData
#[test]
fn create_thumbnail_failure_data_error_stays_invalid_data() {
    let error = create_thumbnail_failure(-100_008, &fake_format);
    assert_eq!(kind_name(&error), "InvalidData");
    assert_eq!(
        error.message(),
        "LibRaw could not create embedded JPEG thumbnail: format_error(-100008)"
    );
}

// C#: LibRawManagedLogicTests.CreateFailure_InsufficientMemory_IsInvalidOperationLikeManagedOutOfMemory (2 rows)
#[test]
fn create_failure_insufficient_memory_is_invalid_operation_like_managed_out_of_memory() {
    for code in [LIBRAW_UNSUFFICIENT_MEMORY, ERRNO_NO_MEMORY] {
        let error = create_failure(code, "unpack RAW data", &fake_format);
        assert_eq!(kind_name(&error), "InvalidOperation", "code {code}");
        assert_eq!(error.message(), NOT_ENOUGH_MEMORY_MESSAGE);
    }
}

// C#: LibRawManagedLogicTests.CreateFailure_OtherCodes_StayInvalidData (3 rows)
#[test]
fn create_failure_other_codes_stay_invalid_data() {
    for code in [-2, 1, LIBRAW_IO_ERROR] {
        let error = create_failure(code, "process RAW data", &fake_format);
        assert_eq!(kind_name(&error), "InvalidData", "code {code}");
        assert_eq!(
            error.message(),
            format!("LibRaw could not process RAW data: format_error({code})")
        );
    }
}

// C#: LibRawManagedLogicTests.ReadJpegThumbnail_AndDecode_WithCancelledToken_ThrowOperationCanceledBeforeOpeningTheFile
#[test]
fn read_jpeg_thumbnail_and_decode_with_cancelled_token_throw_before_opening_the_file() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let api = FakeApi::new(
        Script {
            init_panics: true,
            ..Script::default()
        },
        &log,
    );
    let token = CancelToken::new();
    token.cancel();
    let error = read_jpeg_thumbnail(&api, "C:\\photos\\x.orf", &token).expect_error("canceled before init");
    assert_eq!(kind_name(&error), "Canceled");
    assert!(log.lines().is_empty(), "no libraw_init and no open");

    let decoder = private_decoder(
        FakeApi::arc(
            Script {
                init_panics: true,
                ..Script::default()
            },
            &log,
        ),
        memory_source_never_read(),
        &log,
    );
    let error = decoder
        .decode_with_cancel(&request("C:\\photos\\x.cr3"), &token)
        .expect_error("canceled before the gate");
    assert_eq!(kind_name(&error), "Canceled");
}

// C#: LibRawManagedLogicTests.ToExtendedLengthPath_LongDrivePath_GetsTheExtendedPrefix
#[test]
fn to_extended_length_path_long_drive_path_gets_the_extended_prefix() {
    let path = long_path("C:\\");
    let extended = to_extended_length_path(&path).expect("a long drive path is converted");
    assert!(extended.starts_with(r"\\?\C:\"), "{extended}");
}

// C#: LibRawManagedLogicTests.ToExtendedLengthPath_LongUncPath_BecomesTheUncExtendedForm
#[test]
fn to_extended_length_path_long_unc_path_becomes_the_unc_extended_form() {
    let path = format!("\\\\server\\share\\{}\\photo.cr2", "a".repeat(260));
    let extended = to_extended_length_path(&path).expect("a long UNC path is converted");
    assert!(extended.starts_with(r"\\?\UNC\server\share\"), "{extended}");
}

// C#: LibRawManagedLogicTests.ToExtendedLengthPath_DotSegmentsAndForwardSlashes_AreNormalised
#[test]
fn to_extended_length_path_dot_segments_and_forward_slashes_are_normalised() {
    let path = format!("C:/photos/./sub/../{}/photo.cr2", "a".repeat(260));
    let extended = to_extended_length_path(&path).expect("a long path with dot segments is converted");
    assert!(extended.starts_with(r"\\?\C:\photos\"), "{extended}");
    assert!(!extended.contains("\\.\\") && !extended.contains("/"), "{extended}");
}

// C#: LibRawManagedLogicTests.ToExtendedLengthPath_ShortOrRelativePath_IsLeftAlone (2 rows)
#[test]
fn to_extended_length_path_short_or_relative_path_is_left_alone() {
    assert_eq!(to_extended_length_path("C:\\photos\\short.cr2"), None);
    assert_eq!(
        to_extended_length_path(&format!("photos\\{}\\photo.cr2", "a".repeat(260))),
        None
    );
}

// C#: LibRawManagedLogicTests.ToExtendedLengthPath_ShortPathJustBelowTheThreshold_IsLeftAlone
#[test]
fn to_extended_length_path_short_path_just_below_the_threshold_is_left_alone() {
    let path = format!("C:\\{}", "a".repeat(LONG_PATH_THRESHOLD - 1 - 3));
    assert_eq!(path.encode_utf16().count(), LONG_PATH_THRESHOLD - 1);
    assert_eq!(to_extended_length_path(&path), None);
}

// C#: LibRawManagedLogicTests.ToExtendedLengthPath_AlreadyExtendedOrDevicePath_IsLeftAlone
#[test]
fn to_extended_length_path_already_extended_or_device_path_is_left_alone() {
    let tail = "a".repeat(260);
    assert_eq!(to_extended_length_path(&format!("\\\\?\\C:\\{tail}\\x.cr2")), None);
    assert_eq!(to_extended_length_path(&format!("\\\\.\\C:\\{tail}\\x.cr2")), None);
}

// C#: LibRawThumbnailTrimMutationGapTests / LibRawManagedLogicTests.TrimToEndOfImage_* (golden-backed inputs, the named rows)
#[test]
fn trim_to_end_of_image_arbitrary_non_fill_tail_after_the_eoi_is_trimmed() {
    for name in [
        "trim-non-fill-tail-1",
        "trim-non-fill-tail-37",
        "trim-non-fill-tail-4096",
    ] {
        let trimmed = trim_to_end_of_image(trim_case_bytes(name)).expect("the JPEG is complete");
        assert_eq!(trimmed.len(), 163, "{name}");
    }
}

// C#: LibRawManagedLogicTests.TrimToEndOfImage_TailWithFfBytesButNoEoi_IsTrimmed
#[test]
fn trim_to_end_of_image_tail_with_ff_bytes_but_no_eoi_is_trimmed() {
    let trimmed = trim_to_end_of_image(trim_case_bytes("trim-tail-ff-no-eoi")).expect("trims");
    assert_eq!(trimmed.len(), 163);
}

// C#: LibRawManagedLogicTests.TrimToEndOfImage_EmbeddedThumbnailWithCompleteMainImage_KeepsTheWholeMainImage
#[test]
fn trim_to_end_of_image_embedded_thumbnail_with_complete_main_image_keeps_the_whole_main_image() {
    let trimmed = trim_to_end_of_image(trim_case_bytes("trim-embedded-thumbnail-complete")).expect("trims");
    assert_eq!(trimmed.len(), 336);
}

// C#: LibRawManagedLogicTests.TrimToEndOfImage_MainImageTruncatedAfterAnEmbeddedThumbnailEoi_IsNotCutAtTheThumbnail
#[test]
fn trim_to_end_of_image_main_image_truncated_after_an_embedded_thumbnail_eoi_is_refused() {
    let error = trim_to_end_of_image(trim_case_bytes("trim-embedded-thumbnail-main-truncated"))
        .expect_error("a truncated main image is not cut at the thumbnail EOI");
    assert_eq!(kind_name(&error), "InvalidData");
    assert_eq!(error.message(), INCOMPLETE_JPEG_MESSAGE);
}

// C#: LibRawManagedLogicTests.TrimToEndOfImage_TruncatedStreamWithGarbageTail_Throws
#[test]
fn trim_to_end_of_image_truncated_stream_with_garbage_tail_throws() {
    let error = trim_to_end_of_image(trim_case_bytes("trim-truncated-garbage-tail")).expect_error("throws");
    assert_eq!(kind_name(&error), "InvalidData");
}

// C#: LibRawManagedLogicTests.TrimToEndOfImage_ProgressiveStyleSecondScanBeforeTheEoi_IsKept
#[test]
fn trim_to_end_of_image_progressive_style_second_scan_before_the_eoi_is_kept() {
    let trimmed = trim_to_end_of_image(trim_case_bytes("trim-two-scans")).expect("trims");
    assert_eq!(trimmed.len(), 183);
}

// C#: LibRawManagedLogicTests.TrimToEndOfImage_UnparsableSegmentLengthButLastEoiWithCleanTail_FallsBackToTheLastEoi
#[test]
fn trim_to_end_of_image_unparsable_segment_length_falls_back_to_the_last_eoi() {
    let trimmed = trim_to_end_of_image(trim_case_bytes("trim-unparsable-app0-clean-tail")).expect("trims");
    assert_eq!(trimmed.len(), 163);
}

// C#: LibRawThumbnailTrimMutationGapTests.ToExtendedLengthPath_PathOfExactlyTheThresholdLength_GetsTheExtendedPrefix
#[test]
fn to_extended_length_path_path_of_exactly_the_threshold_length_gets_the_extended_prefix() {
    let head = "C:\\";
    let path = format!("{head}{}", "a".repeat(LONG_PATH_THRESHOLD - head.len()));
    assert_eq!(path.encode_utf16().count(), LONG_PATH_THRESHOLD);
    assert!(
        to_extended_length_path(&path)
            .expect("threshold length is converted")
            .starts_with(r"\\?\")
    );
}

// C#: LibRawThumbnailTrimMutationGapTests.ToExtendedLengthPath_LongPathThatCannotBeNormalised_IsLeftAlone
#[test]
fn to_extended_length_path_long_path_that_cannot_be_normalised_is_left_alone() {
    let path = format!("C:\\{}\u{0}tail", "a".repeat(260));
    assert_eq!(
        to_extended_length_path(&path),
        None,
        "a NUL inside the path cannot be normalised"
    );
}

// C#: LibRawThumbnailTrimMutationGapTests.TailContainingJpegStructure_IsRejectedAsATruncatedStream (11 rows)
#[test]
fn trim_tail_containing_jpeg_structure_is_rejected_as_a_truncated_stream() {
    for name in [
        "trim-tail-structure-d8",
        "trim-tail-structure-da",
        "trim-tail-structure-db",
        "trim-tail-structure-dd",
        "trim-tail-structure-fe",
        "trim-tail-structure-c0",
        "trim-tail-structure-c8",
        "trim-tail-structure-cf",
        "trim-tail-structure-e0",
        "trim-tail-structure-e8",
        "trim-tail-structure-ef",
    ] {
        let error = trim_to_end_of_image(trim_case_bytes(name)).expect_error(name);
        assert_eq!(kind_name(&error), "InvalidData", "{name}");
    }
}

// C#: LibRawThumbnailTrimMutationGapTests.TailWithAMarkerLikePairThatIsNotStructure_IsAccepted (4 rows)
#[test]
fn trim_tail_with_a_marker_like_pair_that_is_not_structure_is_accepted() {
    for name in [
        "trim-tail-marker-like-12",
        "trim-tail-marker-like-bf",
        "trim-tail-marker-like-d1",
        "trim-tail-marker-like-f0",
    ] {
        let trimmed = trim_to_end_of_image(trim_case_bytes(name)).expect(name);
        assert_eq!(trimmed.len(), 6, "{name}");
    }
}

// C#: LibRawThumbnailTrimMutationGapTests.SegmentLongerThan255Bytes_IsSkippedByItsFullLength and
// EoiAtTheStartOfTheLast64KbWindow_IsFound (golden-backed window rows, straddling the 64 KiB search window)
#[test]
fn trim_eoi_at_the_start_of_the_last_64kb_window_is_found() {
    assert_eq!(TRAILING_EOI_SEARCH_WINDOW, 64 * 1024);
    let trimmed = trim_to_end_of_image(trim_case_bytes("trim-window-at-window-start")).expect("found");
    assert_eq!(trimmed.len(), 4466);
    let before = trim_to_end_of_image(trim_case_bytes("trim-window-before-window"));
    assert_eq!(kind_name(&before.expect_error("outside the window")), "InvalidData");
}

// C#: LibRawCorpusSelectionTests.SelectCorpusFiles_DefaultIsRepresentative_AndStrictIncludesEveryRawFile (test-support helper
// `select_corpus_files`, not ported yet: red until it lands)
#[test]
fn select_corpus_files_default_is_representative_and_strict_includes_every_raw_file() {
    let files: Vec<PathBuf> = ["a.cr2", "b.nef", "c.orf", "d.txt", "e.dng", "f.rw2"]
        .iter()
        .map(PathBuf::from)
        .collect();
    let default = select_corpus_files(&files, false);
    let strict = select_corpus_files(&files, true);
    assert!(
        default.len() <= strict.len(),
        "the default selection is never larger than strict"
    );
    for raw in ["a.cr2", "b.nef", "c.orf", "e.dng", "f.rw2"] {
        assert!(strict.iter().any(|f| f.ends_with(raw)), "strict includes {raw}");
    }
    assert!(
        !strict.iter().any(|f| f.ends_with("d.txt")),
        "a non-RAW file is never selected"
    );
}

// ------------------------------------------------------------------------------------------------ parsers never panic (T11)

/// All byte strings of length `0..=max_len` over `alphabet` (Rust-only property test, contract trap T11).
fn all_streams(alphabet: &[u8], max_len: usize) -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new()];
    let mut frontier = vec![Vec::new()];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for stream in &frontier {
            for &byte in alphabet {
                let mut longer = stream.clone();
                longer.push(byte);
                next.push(longer);
            }
        }
        out.extend(next.iter().cloned());
        frontier = next;
    }
    out
}

// Rust-only: `find_end_of_image` never panics and never returns an index past the input (all streams of length 0..=6 over
// {00, 12, D8, D9, DA, FF}, plus 2 000 xorshift mutants of the minimal JPEG).
#[test]
fn find_end_of_image_never_panics_and_never_reads_past_the_input() {
    let alphabet: [u8; 6] = [0x00, 0x12, 0xD8, 0xD9, 0xDA, 0xFF];
    for stream in all_streams(&alphabet, 6) {
        if let Ok(end) = find_end_of_image(&stream) {
            assert!(end <= stream.len(), "{stream:02x?} -> {end}");
        }
    }
    let seed = trim_case_bytes("trim-minimal-unpadded-same-array");
    for mutant in 0..2_000u32 {
        let mut bytes = seed.clone();
        let noise = xorshift32_bytes(mutant + 1, 4);
        let at = usize::from(noise[0]) % bytes.len();
        bytes[at] = noise[1];
        if let Ok(end) = find_end_of_image(&bytes) {
            assert!(end <= bytes.len());
        }
    }
}

// Rust-only: `to_extended_length_path` never panics on odd long paths (the same alphabet over a long prefix).
#[test]
fn to_extended_length_path_never_panics_on_odd_long_paths() {
    let alphabet = ['C', ':', '\\', '/', '.', 'a', '\u{1F600}'];
    let tail = "a".repeat(250);
    let mut suffixes = vec![String::new()];
    for _ in 0..3 {
        let mut next = Vec::new();
        for s in &suffixes {
            for c in alphabet {
                let mut longer = s.clone();
                longer.push(c);
                next.push(longer);
            }
        }
        suffixes.extend(next);
    }
    for suffix in suffixes {
        let _ = to_extended_length_path(&format!("C:\\{tail}{suffix}"));
    }
}

// ------------------------------------------------------------------------------------------------ open order and gate slot (fake twins)

// C#: LibRawOpenOrderTests.Decode_MissingFile_ConfiguresOutputBeforeAttemptingToOpen (fake twin)
#[test]
fn decode_missing_file_configures_output_before_attempting_to_open() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let api = FakeApi::arc(
        Script {
            open_codes: VecDeque::from(vec![LIBRAW_IO_ERROR]),
            ..full_bayer_script()
        },
        &log,
    );
    let decoder = private_decoder(api, memory_source(64 * GIB, 0), &log).with_stage_observer(stage_logger(&log));
    let error = decoder
        .decode_with_cancel(&request("Z:\\definitely-missing\\file.orf"), &CancelToken::none())
        .expect_error("the file does not open");
    assert_eq!(kind_name(&error), "InvalidData");
    assert_eq!(
        log.stages(),
        vec![STAGE_CONFIGURED.to_owned()],
        "only the configured stage"
    );
    let lines = log.lines();
    let open_at = lines
        .iter()
        .position(|l| l.starts_with("open_wfile:"))
        .expect("the open ran");
    let colour_at = lines
        .iter()
        .position(|l| l == "set_output_color(1)")
        .expect("output configured");
    assert!(colour_at < open_at, "output parameters are set before libraw_open_*");
}

// C#: LibRawOpenOrderTests.Decode_CorpusFile_ConfiguresOutputBeforeOpenAndNotAfter (fake twin)
#[test]
fn decode_configures_output_before_open_and_not_after() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let api = FakeApi::arc(full_bayer_script(), &log);
    let decoder = private_decoder(api, memory_source(64 * GIB, 0), &log).with_stage_observer(stage_logger(&log));
    let _ = decoder.decode_with_cancel(&request("C:\\photos\\ok.cr3"), &CancelToken::none());
    let lines = log.lines();
    let open_at = lines.iter().position(|l| l.starts_with("open_wfile:")).expect("opened");
    for setting in [
        "set_output_color(1)",
        "set_output_bps(8)",
        "set_no_auto_bright(1)",
        "try_set_use_camera_wb(1,8,1)",
    ] {
        let at = lines
            .iter()
            .position(|l| l == setting)
            .unwrap_or_else(|| panic!("{setting} was called"));
        assert!(at < open_at, "{setting} before open");
    }
    let after = lines.iter().skip(open_at).filter(|l| l.starts_with("set_")).count();
    assert_eq!(after, 0, "no output setting after the open");
}

// C#: LibRawOpenOrderTests.Decode_NotEnoughHeadroom_IsRefusedBeforeUnpackAsInvalidOperation (fake twin)
#[test]
fn decode_not_enough_headroom_is_refused_before_unpack_as_invalid_operation() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let api = FakeApi::arc(full_bayer_script(), &log);
    let decoder = private_decoder(api, memory_source(GIB, 0), &log).with_stage_observer(stage_logger(&log));
    let error = decoder
        .decode_with_cancel(&request("C:\\photos\\big.cr3"), &CancelToken::none())
        .expect_error("1 GiB cannot hold the 100 MP mosaic decode");
    assert_eq!(kind_name(&error), "InvalidOperation");
    assert_eq!(error.message(), NOT_ENOUGH_MEMORY_MESSAGE);
    assert!(!log.lines().iter().any(|l| l == "unpack"), "refused before unpack");
}

// C#: LibRawOpenOrderTests.Decode_HeadroomCoversOnlyTheWorkingImageAndBitmap_IsRefusedBecauseTheUnpackedRawBufferCounts
// Budget 1.6e9: working image + bitmap (15 bytes/px = 1.51e9) passes, the full Bayer estimate (17 bytes/px = 1.72e9) does not.
#[test]
fn decode_headroom_covers_only_the_working_image_and_bitmap_is_refused_because_the_raw_buffer_counts() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let api = FakeApi::arc(full_bayer_script(), &log);
    let decoder = private_decoder(api, memory_source(1_600_000_000, 0), &log);
    let error = decoder
        .decode_with_cancel(&request("C:\\photos\\big.cr3"), &CancelToken::none())
        .expect_error("the unpacked RAW buffer does not fit");
    assert_eq!(kind_name(&error), "InvalidOperation");
    assert!(!log.lines().iter().any(|l| l == "unpack"));
}

// C#: LibRawOpenOrderTests.Decode_HeadroomCoversTheRawBufferToo_Decodes (fake twin of the full pipeline, stage order pinned)
#[test]
fn decode_headroom_covers_the_raw_buffer_too_and_decodes_through_the_pipeline() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let api = FakeApi::arc(Script::default(), &log);
    let decoder = private_decoder(api, memory_source(64 * GIB, 0), &log).with_stage_observer(stage_logger(&log));
    let image = decoder
        .decode_with_cancel(&request("C:\\photos\\small.cr3"), &CancelToken::none())
        .expect("the decode succeeds");
    assert_eq!(image.pixel_width(), 2);
    assert_eq!(
        log.stages(),
        vec![
            STAGE_CONFIGURED,
            "opened",
            STAGE_UNPACKED,
            STAGE_PROCESSED,
            STAGE_HANDLE_CLOSED,
            "source-released",
            "bitmap-created",
        ]
    );
}

// C#: LibRawGateSlotNativeTests.ReadJpegThumbnailAndDecode_WithCancelledToken_LeaveTheFullDecodeSlotFree (fake api)
#[test]
fn read_jpeg_thumbnail_and_decode_with_cancelled_token_leave_the_full_decode_slot_free() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder = private_decoder(FakeApi::arc(Script::default(), &log), memory_source_never_read(), &log)
        .with_gate(gate.clone());
    let token = CancelToken::new();
    token.cancel();
    let _ = read_jpeg_thumbnail(&*FakeApi::arc(Script::default(), &log), "C:\\photos\\x.orf", &token);
    let _ = decoder.decode_with_cancel(&request("C:\\photos\\x.cr3"), &token);
    assert_eq!(gate.slots_available(), 1, "a canceled decode never holds the slot");
}

// C#: LibRawGateSlotNativeTests.Decode_CancelledAfterTheGateWasEntered_ReleasesTheSlotOnUnwind (fake twin)
#[test]
fn decode_canceled_after_the_gate_was_entered_releases_the_slot() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let core = failing_core(|| DecodeError::Canceled);
    let decoder = private_decoder(FakeApi::arc(Script::default(), &log), memory_source_never_read(), &log)
        .with_gate(gate.clone())
        .with_core_override(core);
    let error = decoder
        .decode_with_cancel(&request("C:\\photos\\x.cr3"), &CancelToken::none())
        .expect_error("canceled in the body");
    assert_eq!(kind_name(&error), "Canceled");
    assert_eq!(gate.slots_available(), 1, "the slot is released on every exit");
}

// C#: LibRawGateSlotNativeTests.Decode_QueuedWaiterCancelled_LeavesTheHolderInChargeAndFreesTheSlotAtTheEnd (fake twin with
// channels: the queued signal is the private gate's waiter hook; no sleep)
#[test]
fn decode_queued_waiter_cancelled_leaves_the_holder_in_charge_and_frees_the_slot_at_the_end() {
    let (tx, rx) = mpsc::channel::<SourceReadPriority>();
    let gate = FullDecodeGate::with_waiter_queued(
        MAX_QUEUED_PRELOAD_DECODES,
        Arc::new(move |priority: SourceReadPriority| tx.send(priority).map_err(|e| e.to_string())),
    );
    let holder = gate
        .enter(SourceReadPriority::Viewer, &CancelToken::none())
        .expect("the holder is in charge");

    let waiter_token = CancelToken::new();
    let waiter_gate = gate.clone();
    let token_for_thread = waiter_token.clone();
    let waiter = thread::spawn(move || {
        waiter_gate
            .enter(SourceReadPriority::Viewer, &token_for_thread)
            .map(drop)
    });
    assert_eq!(
        rx.recv_timeout(WAIT_LIMIT).expect("the waiter queued"),
        SourceReadPriority::Viewer
    );

    waiter_token.cancel();
    let result = waiter.join().expect("waiter thread");
    assert!(
        matches!(result, Err(GateError::Canceled)),
        "the cancelled waiter gives up: {result:?}"
    );
    assert_eq!(gate.slots_available(), 0, "the holder still owns the slot");

    drop(holder);
    assert_eq!(gate.slots_available(), 1, "the slot is free at the end");
}

// ------------------------------------------------------------------------------------------------ pixels: hand-off (fake staged)

fn handoff_decoder(log: &Log, codec_fails: bool) -> LibRawDecoder {
    LibRawDecoder::new(
        Arc::new(FakeCodec {
            log: log.clone(),
            fail: codec_fails,
        }),
        FakeApi::arc(Script::default(), log),
        memory_source(64 * GIB, 0),
    )
    .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
    .with_stage_observer(stage_logger(log))
}

// C#: Pixels/LibRawPixelBufferHandOffTests.HandOff_WithCodec_GivesTheSamePixelsAsTheWpfPath (4 rows)
#[test]
fn hand_off_with_codec_gives_the_same_pixels_as_the_wpf_path() {
    let _pixels = pixel_memory_shared();
    for (downscaled, original_width, original_height) in [(false, 4, 4), (true, 8, 8), (true, 4, 8), (false, 3, 5)] {
        let log = Log::default();
        let decoder = handoff_decoder(&log, false);
        let mut staged = FakeStaged::new(4, 4);
        let address = staged.address();
        let view = decoder
            .hand_off(&mut staged, downscaled, original_width, original_height, &|| true)
            .expect("the codec takes the buffer");
        assert_eq!(view.pixel_width(), 4);
        assert_eq!(view.pixel_height(), 4);
        assert_eq!(view.downscaled(), downscaled);
        assert_eq!(view.orientation(), 1);
        assert_eq!(view.original_width(), original_width);
        assert_eq!(view.original_height(), original_height);
        assert_eq!(view.actual_backend(), DecoderBackend::LibRaw);
        assert_eq!(view.estimated_bytes(), 4 * 4 * 4, "BGRA bytes");
        let pixels = view.platform_image().as_pixels().expect("pixels").clone();
        assert_eq!(
            pixels.address().map(|p| p as usize).unwrap_or(0),
            address,
            "the codec gets the staged buffer"
        );
        assert_eq!(
            log.stages(),
            vec!["source-released".to_owned(), "bitmap-created".to_owned()]
        );
    }
}

// C#: Pixels/LibRawPixelBufferHandOffTests.HandOff_WithCodec_MovesOwnershipWithoutCopy_AndTheEmptiedBufferFreesNothing
#[test]
fn hand_off_with_codec_moves_ownership_without_copy_and_the_emptied_buffer_frees_nothing() {
    let _pixels = pixel_memory_exclusive();
    let log = Log::default();
    let decoder = handoff_decoder(&log, false);
    let mut staged = FakeStaged::new(4, 4);
    let live_before = NativePixelMemory::live_count();
    let _view = decoder
        .hand_off(&mut staged, false, 4, 4, &|| true)
        .expect("handed off");
    assert_eq!(
        NativePixelMemory::live_count(),
        live_before,
        "moved, not copied: no new native allocation"
    );
    assert!(!staged.owned(), "the staging buffer is emptied");
}

// C#: Pixels/LibRawPixelBufferHandOffTests.HandOff_CodecThrows_BufferIsFreed_AndTheErrorPropagates
#[test]
fn hand_off_codec_throws_buffer_is_freed_and_the_error_propagates() {
    let _pixels = pixel_memory_exclusive();
    let log = Log::default();
    let decoder = handoff_decoder(&log, true);
    let mut staged = FakeStaged::new(4, 4);
    let live_before = NativePixelMemory::live_count();
    let error = decoder
        .hand_off(&mut staged, false, 4, 4, &|| true)
        .expect_error("the codec refuses the pixels");
    assert_eq!(kind_name(&error), "InvalidOperation");
    assert_eq!(
        NativePixelMemory::live_count(),
        live_before - 1,
        "the buffer the codec dropped is freed"
    );
}

// C#: Pixels/LibRawPixelBufferHandOffTests.HandOff_WithoutCodec_StillReportsSourceHeldWhenTheRgbBufferIsOpen
#[test]
fn hand_off_still_reports_source_held_when_the_rgb_buffer_is_open() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let decoder = handoff_decoder(&log, false);
    let mut staged = FakeStaged::new(4, 4);
    decoder
        .hand_off(&mut staged, false, 4, 4, &|| false)
        .expect("handed off");
    assert_eq!(
        log.stages(),
        vec!["source-held".to_owned(), "bitmap-created".to_owned()]
    );
}

// C#: Pixels/LibRawPixelBufferHandOffTests.HandOff_ReportsSourceReleased_WhileTheStagingBufferIsStillOwned
#[test]
fn hand_off_reports_source_released_while_the_staging_buffer_is_still_owned() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let mut staged = FakeStaged::new(4, 4);
    let owned = staged.owned_flag();
    let seen = Arc::new(Mutex::new(Vec::<bool>::new()));
    let seen_in_observer = Arc::clone(&seen);
    let observer = Arc::new(move |stage: &str| {
        if stage == "source-released" {
            seen_in_observer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(owned.load(Ordering::SeqCst));
        }
        Ok(())
    });
    let decoder = LibRawDecoder::new(
        Arc::new(FakeCodec {
            log: log.clone(),
            fail: false,
        }),
        FakeApi::arc(Script::default(), &log),
        memory_source(64 * GIB, 0),
    )
    .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
    .with_stage_observer(observer);
    decoder
        .hand_off(&mut staged, false, 4, 4, &|| true)
        .expect("handed off");
    assert_eq!(
        *seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![true]
    );
}

// ------------------------------------------------------------------------------------------------ golden inputs (named)
