#![cfg(windows)]
//! Red ports of the WIC-free half of `WicDirectDecoder` (C# baseline d92d6aae, `PhotoReview.Imaging/Decoding/Wic/WicDirectDecoder.cs`).
//!
//! Every test here calls a pure `pub fn` of `pr_platform_win::wic::decoder` (or only reads committed files), so the file runs on any
//! Windows machine without WIC. Until the porter fills the bodies, the stub's `todo!()` makes the calling tests fail: that is the
//! red state of W4-11c. Tests that need real WIC decodes live in `wic_decoder_native.rs` and `wic_decoder_buffers.rs`.
//!
//! Golden inputs: `tests/golden/wic-direct-internals.json` (pure, not ignored) and `tests/golden/wic-direct-fixtures.json`
//! (manifest: every committed fixture's SHA-256 must match; this one passes in the normal run).
//!
//! The .NET exception type of every golden error row is mapped to a `DecodeError` variant by ONE table here
//! ([`variant_matches`]); the fallback verdict is compared with `FallbackImageDecoder::is_fallbackable` (contract trap T3).
//!
//! DIVERGENCE: `Decode_ArgumentNullException_PropagatesUnchanged` has no twin (Rust has no null; see the native file).
//! DIVERGENCE: the `ReadColorContexts_*` and `ColorTransformChain_*` tests drive the COM paths of private items; their filters
//! are ported here through `is_color_metadata_fault` and `color_chain_failure` (contract OQ4).

use std::cell::Cell;
use std::fs;
use std::io::SeekFrom;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_core::session::store::sha256;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::fallback::FallbackImageDecoder;
use pr_imaging::memory_headroom::{GUARD_THRESHOLD_BYTES, MemoryInfo, MemoryInfoSource};
use pr_platform_win::wic::decoder::{
    DECODER_LABEL, ICC_TRANSFORM_FAILED_PREFIX, OPAQUE_PIXEL_FORMATS, SRGB_EXIF_COLOR_SPACE, SourceStreamBackend,
    as_invalid_data, color_chain_failure, copy_pixels_guarded, decode_error_from_wic, ensure_output_fits,
    icc_transform_failed, is_color_metadata_fault, is_opaque_format, map_to_transform_options,
    native_reduction_is_useful, output_byte_length, output_too_large, relabel_invalid_argument,
    select_source_color_context, to_int_size,
};
use pr_platform_win::wic::interop::{
    COR_E_IO, COR_E_NOTSUPPORTED, E_INVALIDARG, E_NOINTERFACE, StreamBackend, WicError, hresult_from_win32,
};
use pr_test_support::golden;
use serde_json::Value;
use windows::Win32::Graphics::Imaging::{
    WICColorContextExifColorSpace, WICColorContextProfile, WICColorContextType, WICColorContextUninitialized,
};
use windows::core::GUID;

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

/// Memory double that counts its reads (C# `Func<(long, long)>` lambdas with a call counter).
#[derive(Debug)]
struct CountingMemory {
    total: i64,
    load: i64,
    reads: AtomicUsize,
}

impl CountingMemory {
    fn new(total: i64, load: i64) -> Self {
        Self {
            total,
            load,
            reads: AtomicUsize::new(0),
        }
    }

    fn consulted(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl MemoryInfoSource for CountingMemory {
    fn read(&self) -> MemoryInfo {
        self.reads.fetch_add(1, Ordering::SeqCst);
        MemoryInfo {
            total_available: self.total,
            load: self.load,
        }
    }
}

/// `E_OUTOFMEMORY` (0x8007000E): not a named constant of `wic::interop`, so it is written here.
const E_OUTOFMEMORY: i32 = 0x8007_000E_u32 as i32;

const GIB: i64 = 1024 * 1024 * 1024;
const MIB: i64 = 1024 * 1024;

/// `GUID` as the lowercase hyphenated text the golden files use.
fn guid_text(g: &GUID) -> String {
    let d4 = g.data4;
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.data1, g.data2, g.data3, d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7]
    )
}

fn guid_of(text: &str) -> GUID {
    GUID::from_u128(u128::from_str_radix(&text.replace('-', ""), 16).expect("hex GUID"))
}

fn fallbackable(error: &DecodeError) -> bool {
    FallbackImageDecoder::is_fallbackable(error)
}

/// The one table that maps a golden row's .NET exception type to the expected `DecodeError` variant.
/// `DirectoryNotFoundException` is T5: Rust cannot tell a missing directory from a missing file (`io::ErrorKind::NotFound`),
/// so only the behaviour (not fallbackable, an IO exception) is compared.
fn variant_matches(dotnet_type: &str, error: &DecodeError) -> bool {
    let kind = error.kind();
    match dotnet_type {
        "PhotoReview.Imaging.Decoding.DecoderMemoryAdmissionException" => {
            matches!(kind, DecodeError::MemoryAdmission { .. })
        }
        "System.IO.InvalidDataException" => matches!(kind, DecodeError::InvalidData { .. }),
        "System.NotSupportedException" => matches!(kind, DecodeError::NotSupported { .. }),
        "System.Runtime.InteropServices.COMException" => matches!(kind, DecodeError::Com { .. }),
        "System.ArgumentException" => matches!(kind, DecodeError::InvalidArgument { .. }),
        "System.InvalidOperationException" => matches!(kind, DecodeError::InvalidOperation { .. }),
        "System.OutOfMemoryException" => matches!(kind, DecodeError::OutOfMemory { .. }),
        "System.InvalidCastException" => matches!(kind, DecodeError::InvalidCast { .. }),
        "System.IO.FileNotFoundException" => matches!(kind, DecodeError::FileNotFound { .. }),
        "System.IO.DirectoryNotFoundException" => !fallbackable(error) && error.is_io_exception(),
        other => panic!("golden exception type {other} has no DecodeError variant in the table"),
    }
}

/// Checks one golden error description against the error this port produced: the variant (through the table), the
/// INV-12 verdict, the HRESULT for COM rows, and the exact text only for the messages this unit builds.
fn check_error(name: &str, expected: &Value, actual: &DecodeError) {
    let dotnet = expected["type"].as_str().expect("golden error type");
    assert!(
        variant_matches(dotnet, actual),
        "{name}: expected a {dotnet} equivalent, got {actual} ({actual:?})"
    );
    let verdict = expected["fallbackable"].as_bool().expect("golden fallbackable");
    assert_eq!(fallbackable(actual), verdict, "{name}: INV-12 fallback verdict");
    if let DecodeError::Com { hresult, .. } = actual.kind() {
        assert_eq!(
            i64::from(*hresult),
            expected["hresult"].as_i64().expect("golden hresult"),
            "{name}: HRESULT"
        );
    }
    let built_text = matches!(
        dotnet,
        "PhotoReview.Imaging.Decoding.DecoderMemoryAdmissionException"
            | "System.IO.InvalidDataException"
            | "System.NotSupportedException"
    );
    if built_text {
        assert_eq!(
            actual.message(),
            expected["message"].as_str().expect("golden message"),
            "{name}: message built by this unit"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: wic-direct-internals (96 pure cases; W4-11a owns `throw-if-factory-failed`)
// ---------------------------------------------------------------------------------------------------------------

fn check_constants(case: &Value) {
    assert_eq!(
        golden::i64_of(case, "srgb_exif_color_space"),
        i64::from(SRGB_EXIF_COLOR_SPACE)
    );
    assert_eq!(golden::i64_of(case, "guard_threshold_bytes"), GUARD_THRESHOLD_BYTES);
    let expected: Vec<&str> = case["opaque_formats"]
        .as_array()
        .expect("opaque_formats")
        .iter()
        .map(|v| v.as_str().expect("guid text"))
        .collect();
    let actual: Vec<String> = OPAQUE_PIXEL_FORMATS.iter().map(guid_text).collect();
    assert_eq!(actual, expected, "OPAQUE_PIXEL_FORMATS in C# order");
}

fn check_to_int_size(name: &str, case: &Value) {
    let w = golden::i64_of(case, "width") as u32;
    let h = golden::i64_of(case, "height") as u32;
    match (case.get("ok"), case.get("error")) {
        (Some(ok), _) => {
            let expected = (
                ok["width"].as_i64().expect("width") as i32,
                ok["height"].as_i64().expect("height") as i32,
            );
            assert_eq!(to_int_size(w, h).ok(), Some(expected), "{name}");
        }
        (None, Some(error)) => match to_int_size(w, h) {
            Ok(size) => panic!("{name}: expected a refusal, got {size:?}"),
            Err(e) => check_error(name, error, &e),
        },
        _ => panic!("{name}: row has neither ok nor error"),
    }
}

fn check_ensure_output_fits(name: &str, case: &Value) {
    let w = golden::i64_of(case, "width") as i32;
    let h = golden::i64_of(case, "height") as i32;
    let buffer = golden::i64_of(case, "buffer_length");
    let memory = CountingMemory::new(golden::i64_of(case, "total"), golden::i64_of(case, "load"));
    let result = ensure_output_fits(w, h, buffer, &memory);
    assert_eq!(
        memory.consulted() as i64,
        golden::i64_of(case, "consulted"),
        "{name}: the memory budget is read only at or above the guard threshold"
    );
    match case.get("error") {
        None => {
            if let Err(e) = result {
                panic!("{name}: expected Ok, got {e}");
            }
        }
        Some(error) => match result {
            Ok(()) => panic!("{name}: expected a refusal"),
            Err(e) => check_error(name, error, &e),
        },
    }
}

fn check_copy_pixels_guarded(name: &str, case: &Value) {
    let active = golden::bool_of(case, "active");
    let kind = golden::str_of(case, "throw");
    let hresult = golden::i64_of(case, "hresult") as i32;
    let message = golden::str_of(case, "message").to_owned();
    let result = copy_pixels_guarded(
        || match kind {
            "com" => Err(DecodeError::Com {
                hresult,
                message: message.clone(),
            }),
            "invalid-op" => Err(DecodeError::InvalidOperation {
                message: message.clone(),
            }),
            "argument" => Err(DecodeError::InvalidArgument {
                message: message.clone(),
            }),
            "none" => Ok(()),
            other => panic!("unknown throw kind {other}"),
        },
        active,
    );
    match case.get("error") {
        None => {
            if let Err(e) = result {
                panic!("{name}: expected Ok, got {e}");
            }
        }
        Some(error) => match result {
            Ok(()) => panic!("{name}: expected the error to pass through or become ICC"),
            Err(e) => check_error(name, error, &e),
        },
    }
}

/// Runs every `wic-direct-internals` row. The W4-11a rows (`throw-if-factory-failed`) are checked by W4-11a's test.
#[test]
fn golden_wic_direct_internals_matches_csharp() {
    let golden = golden::load("wic-direct-internals");
    let mut checked = 0usize;
    let mut skipped = 0usize;
    for case in &golden.cases {
        let name = golden::str_of(case, "name");
        match case.get("fn").and_then(Value::as_str) {
            None if name == "constants" => check_constants(case),
            None => panic!("{name}: row without fn"),
            Some("to-int-size") => check_to_int_size(name, case),
            Some("output-byte-length") => {
                let w = golden::i64_of(case, "width") as i32;
                let h = golden::i64_of(case, "height") as i32;
                assert_eq!(output_byte_length(w, h), golden::i64_of(case, "ok"), "{name}");
            }
            Some("ensure-output-fits") => check_ensure_output_fits(name, case),
            Some("is-opaque-format") => {
                let guid = guid_of(golden::str_of(case, "guid"));
                assert_eq!(is_opaque_format(&guid), golden::bool_of(case, "ok"), "{name}");
            }
            Some("map-transform-options") => {
                let orientation = golden::i64_of(case, "orientation") as i32;
                assert_eq!(
                    map_to_transform_options(orientation).0,
                    golden::i64_of(case, "ok") as i32,
                    "{name}"
                );
            }
            Some("as-invalid-data") => {
                let decoder = case["decoder"].as_str().unwrap_or(DECODER_LABEL);
                let error = as_invalid_data(golden::str_of(case, "detail"), decoder);
                check_error(name, &case["ok"], &error);
            }
            Some("copy-pixels-guarded") => check_copy_pixels_guarded(name, case),
            Some("throw-if-factory-failed") => {
                skipped += 1;
                continue;
            }
            Some(other) => panic!("{name}: unknown fn {other}"),
        }
        checked += 1;
    }
    assert_eq!(checked + skipped, golden.cases.len());
    assert_eq!(skipped, 9, "the 9 `throw-if-factory-failed` rows belong to W4-11a");
}

// ---------------------------------------------------------------------------------------------------------------
// Golden fixtures manifest (normal run: must pass once the fixtures are committed)
// ---------------------------------------------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Every committed `tests/fixtures/wic-direct/*` file has the length and SHA-256 that the oracle recorded (fixtures are
/// write-once, never hand-edited, T11 and the fixture rule of contracts/README.md).
#[test]
fn wic_direct_fixtures_manifest_matches_files() {
    let manifest = golden::load("wic-direct-fixtures");
    let dir = golden::repo_root().join("tests").join("fixtures").join("wic-direct");
    for case in &manifest.cases {
        let file = golden::str_of(case, "file");
        let bytes = fs::read(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(bytes.len() as i64, golden::i64_of(case, "length"), "{file}: length");
        assert_eq!(hex(&sha256(&bytes)), golden::str_of(case, "sha256"), "{file}: sha256");
    }
}

/// C# `WicColorProfileTests.BundledDisplayP3ProfileHasPinnedHashAndJpegContainsIccApp2`: the bundled profile is the pinned one and
/// the profile-bearing fixture carries an `ICC_PROFILE` APP2 segment.
#[test]
fn bundled_display_p3_profile_has_pinned_hash_and_jpeg_contains_icc_app2() {
    let fixtures = golden::repo_root().join("tests").join("fixtures");
    let profile = fs::read(fixtures.join("ColorProfiles").join("DisplayP3-v4.icc")).expect("bundled profile");
    assert_eq!(
        hex(&sha256(&profile)),
        "cb51de38e482ee974c0c76b9689e16aad04bad16e226fed2f30c842d15ff3a3d",
        "pinned SHA-256 of DisplayP3-v4.icc (tests/fixtures/README.md)"
    );
    let jpeg = fs::read(fixtures.join("wic-direct").join("icc-p3-64x48.jpg")).expect("profile-bearing fixture");
    let app2 = [0xFF, 0xE2];
    let tag = b"ICC_PROFILE\0";
    let has_icc = jpeg
        .windows(app2.len() + 2 + tag.len())
        .any(|w| w[..2] == app2 && w[4..] == tag[..]);
    assert!(has_icc, "icc-p3-64x48.jpg holds an ICC_PROFILE APP2 segment");
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectInternalsMutationTests
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectInternalsMutationTests.ToIntSize_WithinTheIntRange_IsReturnedUnchanged
#[test]
fn to_int_size_within_the_int_range_is_returned_unchanged() {
    let max = i32::MAX as u32;
    for (w, h) in [(1u32, 1u32), (max, 1), (1, max), (max, max)] {
        assert_eq!(to_int_size(w, h).ok(), Some((w as i32, h as i32)), "{w}x{h}");
    }
}

// C#: WicDirectInternalsMutationTests.ToIntSize_BeyondTheIntRangeOnEitherAxis_IsRefusedAsInvalidData
#[test]
fn to_int_size_beyond_the_int_range_on_either_axis_is_refused_as_invalid_data() {
    let max = i32::MAX as u32;
    for (w, h) in [
        (max + 1, 1),
        (1, max + 1),
        (max + 1, max + 1),
        (u32::MAX, 5),
        (5, u32::MAX),
    ] {
        match to_int_size(w, h) {
            Ok(size) => panic!("{w}x{h}: must be refused, got {size:?}"),
            Err(e) => assert!(matches!(e.kind(), DecodeError::InvalidData { .. }), "{w}x{h}: {e}"),
        }
    }
}

// C#: WicDirectInternalsMutationTests.EnsureOutputFits_BufferOfExactlyTheGuardThreshold_ConsultsTheMemoryBudget
#[test]
fn ensure_output_fits_buffer_of_exactly_the_guard_threshold_consults_the_memory_budget() {
    let memory = CountingMemory::new(64 * MIB, 0);
    let result = ensure_output_fits(8192, 4096, GUARD_THRESHOLD_BYTES, &memory);
    assert_eq!(memory.consulted(), 1, "the threshold itself is guarded");
    assert!(matches!(
        result.err().as_ref().map(DecodeError::kind),
        Some(DecodeError::MemoryAdmission { .. })
    ));
}

// C#: WicDirectInternalsMutationTests.EnsureOutputFits_NegativeWrappedLength_IsRefused
#[test]
fn ensure_output_fits_negative_wrapped_length_is_refused() {
    let memory = CountingMemory::new(64 * GIB, 0);
    let result = ensure_output_fits(i32::MAX, i32::MAX, -17_179_869_180, &memory);
    assert_eq!(
        memory.consulted(),
        0,
        "a negative length is refused before the budget is read"
    );
    assert!(matches!(
        result.err().as_ref().map(DecodeError::kind),
        Some(DecodeError::MemoryAdmission { .. })
    ));
}

// C#: WicDirectInternalsMutationTests.OutputByteLength_SaturatesInsteadOfWrapping
#[test]
fn output_byte_length_saturates_instead_of_wrapping() {
    for (w, h, expected) in [(1, 1, 4i64), (6000, 6000, 144_000_000), (i32::MAX, i32::MAX, i64::MAX)] {
        assert_eq!(output_byte_length(w, h), expected, "{w}x{h}");
    }
}

// C#: WicDirectInternalsMutationTests.EnsureOutputFits_BufferJustBelowTheGuardThreshold_NeverConsultsTheMemoryBudget
#[test]
fn ensure_output_fits_buffer_just_below_the_guard_threshold_never_consults_the_memory_budget() {
    let memory = CountingMemory::new(0, 0);
    let result = ensure_output_fits(8192, 4095, GUARD_THRESHOLD_BYTES - 1, &memory);
    assert!(result.is_ok(), "below the threshold everything fits");
    assert_eq!(memory.consulted(), 0);
}

// C#: WicDirectInternalsMutationTests.TryGetNativeReducedSize_CodecOffer_IsUsefulOnlyWhenItReducesWithoutUndershooting
/// The decision half of `TryGetNativeReducedSize` (13 rows, target 100x80, original 400x320). The frame and COM-throw rows
/// are native, see `wic_decoder_native.rs`.
#[test]
fn native_reduction_codec_offer_is_useful_only_when_it_reduces_without_undershooting() {
    let rows = [
        (200u32, 160u32, true),
        (100, 80, true),
        (100, 160, true),
        (200, 80, true),
        (400, 160, true),
        (200, 320, true),
        (400, 320, false),
        (99, 160, false),
        (200, 79, false),
        (99, 79, false),
        (500, 400, false),
        (401, 160, true),
        (200, 321, true),
    ];
    for (closest_w, closest_h, expected) in rows {
        assert_eq!(
            native_reduction_is_useful((100, 80), (400, 320), (closest_w, closest_h)),
            expected,
            "closest {closest_w}x{closest_h}"
        );
    }
}

// C#: WicDirectInternalsMutationTests.EnsureOutputFits_BeyondIntRange_IsAdmissionError (WicDirectDecoderWp03ReviewTests, 4 rows)
#[test]
fn ensure_output_fits_beyond_int_range_is_admission_error() {
    for (w, h, buffer) in [
        (40_000, 20_000, 2_147_483_648i64),
        (40_000, 20_000, 4_294_967_296),
        (40_000, 20_000, i64::MAX),
        (40_000, 20_000, -1),
    ] {
        let memory = CountingMemory::new(i64::MAX / 2, 0);
        match ensure_output_fits(w, h, buffer, &memory) {
            Ok(()) => panic!("{w}x{h} buffer {buffer}: must be refused"),
            Err(e) => assert!(matches!(e.kind(), DecodeError::MemoryAdmission { .. }), "{buffer}: {e}"),
        }
        assert_eq!(
            memory.consulted(),
            0,
            "buffer {buffer} is refused without reading the budget"
        );
    }
}

// C#: WicDirectDecoderWp03ReviewTests.EnsureOutputFits_ExactlyIntMax_FitsWhenThereIsRoom
#[test]
fn ensure_output_fits_exactly_int_max_fits_when_there_is_room() {
    let memory = CountingMemory::new(64 * GIB, 0);
    assert!(ensure_output_fits(32_768, 16_384, i64::from(i32::MAX), &memory).is_ok());
    assert_eq!(memory.consulted(), 1);
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectFactoryFailureTests (the decoder's view; the ThrowIfFactoryFailed_* rows are W4-11a's)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectFactoryFailureTests (decoder view of a failed factory: WINCODEC_ERR_NOTINITIALIZED)
#[test]
fn decode_error_from_wic_not_initialized_factory_is_com_with_that_hresult() {
    let error = decode_error_from_wic(&WicError::Com {
        hresult: pr_platform_win::wic::interop::WINCODEC_ERR_NOTINITIALIZED,
    });
    match error.kind() {
        DecodeError::Com { hresult, .. } => {
            assert_eq!(*hresult, pr_platform_win::wic::interop::WINCODEC_ERR_NOTINITIALIZED);
        }
        other => panic!("expected Com, got {other:?}"),
    }
}

/// T1/T3: each WIC failure is mapped by its .NET exception kind (`Marshal.GetExceptionForHR`), and the fallback verdict equals C#
/// `IsFallbackable` for that type. Ported beyond the golden rows, which only cover the decode paths.
#[test]
fn decode_error_from_wic_keeps_the_dotnet_kind_and_its_fallback_verdict() {
    let rows: [(WicError, bool); 6] = [
        (WicError::Com { hresult: E_INVALIDARG }, false),
        (WicError::Com { hresult: E_OUTOFMEMORY }, false),
        (WicError::Com { hresult: E_NOINTERFACE }, true),
        (
            WicError::Com {
                hresult: COR_E_NOTSUPPORTED,
            },
            true,
        ),
        (WicError::Com { hresult: COR_E_IO }, false),
        (
            WicError::Com {
                hresult: hresult_from_win32(2),
            },
            false,
        ),
    ];
    for (wic, verdict) in rows {
        let error = decode_error_from_wic(&wic);
        assert_eq!(fallbackable(&error), verdict, "{error}: INV-12 verdict");
    }
    assert!(matches!(
        decode_error_from_wic(&WicError::Com { hresult: E_OUTOFMEMORY }).kind(),
        DecodeError::OutOfMemory { .. }
    ));
    assert!(matches!(
        decode_error_from_wic(&WicError::NoInterface {
            interface: "IWICStream"
        })
        .kind(),
        DecodeError::InvalidCast { .. }
    ));
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicIccDiagnosticsTests
// ---------------------------------------------------------------------------------------------------------------

// C#: WicIccDiagnosticsTests.CopyPixelsGuarded_TransformActive_ReportsIccFailureAsFallbackable
#[test]
fn copy_pixels_guarded_transform_active_reports_icc_failure_as_fallbackable() {
    let result = copy_pixels_guarded(
        || {
            Err(DecodeError::Com {
                hresult: -2_003_292_336,
                message: "converter exploded".to_owned(),
            })
        },
        true,
    );
    let error = result.expect_err("a COM failure of the transform is an error");
    assert!(matches!(error.kind(), DecodeError::NotSupported { .. }), "{error}");
    assert!(
        error.message().starts_with(ICC_TRANSFORM_FAILED_PREFIX),
        "{}",
        error.message()
    );
    assert!(fallbackable(&error));
}

// C#: WicIccDiagnosticsTests.CopyPixelsGuarded_TransformInactive_LeavesComFailureUntouched
#[test]
fn copy_pixels_guarded_transform_inactive_leaves_com_failure_untouched() {
    let result = copy_pixels_guarded(
        || {
            Err(DecodeError::Com {
                hresult: -2_003_292_336,
                message: "converter exploded".to_owned(),
            })
        },
        false,
    );
    match result.expect_err("error passes through").kind() {
        DecodeError::Com { hresult, .. } => assert_eq!(*hresult, -2_003_292_336),
        other => panic!("expected Com, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectDecoderWp03/FaultInjection: the colour-context filters
// ---------------------------------------------------------------------------------------------------------------

/// C# `WicDirectFaultInjectionTests.ReadColorContexts_CountQueryFails_IsUntaggedSrgb`: the count query may fail with
/// `COMException` or `ArgumentException`; both are "untagged sRGB". Other faults propagate.
#[test]
fn read_color_contexts_count_query_fails_is_untagged_srgb() {
    let com = DecodeError::Com {
        hresult: -2_003_292_336,
        message: "x".to_owned(),
    };
    let arg = DecodeError::InvalidArgument {
        message: "x".to_owned(),
    };
    assert!(is_color_metadata_fault(&com));
    assert!(is_color_metadata_fault(&arg));
}

/// C# `WicDirectFaultInjectionTests.ReadColorContexts_SecondQueryOrContextCreationFails_IsUntaggedSrgb`.
#[test]
fn read_color_contexts_second_query_or_context_creation_fails_is_untagged_srgb() {
    for error in [
        DecodeError::Com {
            hresult: -2_147_467_259,
            message: "x".to_owned(),
        },
        DecodeError::InvalidArgument {
            message: "x".to_owned(),
        },
    ] {
        assert!(is_color_metadata_fault(&error), "{error}");
    }
}

/// C# `WicDirectFaultInjectionTests.ReadColorContexts_UnexpectedFault_Propagates`.
#[test]
fn read_color_contexts_unexpected_fault_propagates() {
    for error in [
        DecodeError::OutOfMemory {
            message: "x".to_owned(),
        },
        DecodeError::InvalidOperation {
            message: "x".to_owned(),
        },
    ] {
        assert!(
            !is_color_metadata_fault(&error),
            "{error} must propagate, not become sRGB"
        );
    }
}

/// C# `WicDirectFaultInjectionTests.ColorTransformChain_WicFault_IsReportedAsIccTransformFailure` (3 rows, plus OutOfMemory
/// which the outer catch does not name).
#[test]
fn color_transform_chain_wic_fault_is_reported_as_icc_transform_failure() {
    let rows = [
        DecodeError::Com {
            hresult: -2_003_292_336,
            message: "x".to_owned(),
        },
        DecodeError::InvalidCast {
            message: "x".to_owned(),
        },
        DecodeError::InvalidArgument {
            message: "x".to_owned(),
        },
    ];
    for error in rows {
        let expected_hresult = match error.kind() {
            DecodeError::Com { hresult, .. } => Some(*hresult),
            _ => None,
        };
        let mapped = color_chain_failure(error);
        assert!(matches!(mapped.kind(), DecodeError::NotSupported { .. }), "{mapped}");
        assert_eq!(mapped.inner_hresult(), expected_hresult);
        assert!(mapped.message().starts_with(ICC_TRANSFORM_FAILED_PREFIX));
    }
    let untouched = color_chain_failure(DecodeError::OutOfMemory {
        message: "x".to_owned(),
    });
    assert!(matches!(untouched.kind(), DecodeError::OutOfMemory { .. }));
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_* (fakes = a local enum and the two closures)
#[derive(Debug, Clone, Copy)]
struct Fake {
    kind: WICColorContextType,
    exif: u32,
}

fn ctx(kind: WICColorContextType, exif: u32) -> Option<Fake> {
    Some(Fake { kind, exif })
}

fn select(contexts: &[Option<Fake>]) -> Option<usize> {
    select_source_color_context(contexts, |c| Ok(c.kind), |c| Ok(c.exif)).expect("no fault injected")
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_NoContexts_IsNull
#[test]
fn select_source_color_context_no_contexts_is_null() {
    assert_eq!(select(&[]), None);
    assert_eq!(select(&[None]), None);
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_SrgbExifColorSpaceOnly_NeedsNoTransform
#[test]
fn select_source_color_context_srgb_exif_color_space_only_needs_no_transform() {
    assert_eq!(
        select(&[ctx(WICColorContextExifColorSpace, SRGB_EXIF_COLOR_SPACE)]),
        None
    );
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_NonSrgbExifColorSpace_IsChosen
#[test]
fn select_source_color_context_non_srgb_exif_color_space_is_chosen() {
    assert_eq!(select(&[ctx(WICColorContextExifColorSpace, 2)]), Some(0));
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_UninitializedContext_IsIgnored
#[test]
fn select_source_color_context_uninitialized_context_is_ignored() {
    assert_eq!(select(&[ctx(WICColorContextUninitialized, 2)]), None);
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_EmbeddedProfile_BeatsAnyExifColorSpace
/// The entry after the profile is never queried (the closures count their calls).
#[test]
fn select_source_color_context_embedded_profile_beats_any_exif_color_space() {
    assert_eq!(
        select(&[ctx(WICColorContextExifColorSpace, 2), ctx(WICColorContextProfile, 0)]),
        Some(1)
    );
    assert_eq!(
        select(&[ctx(WICColorContextProfile, 0), ctx(WICColorContextExifColorSpace, 2)]),
        Some(0)
    );

    let exif_queries = Cell::new(0usize);
    let contexts = [ctx(WICColorContextProfile, 0), ctx(WICColorContextExifColorSpace, 2)];
    let chosen = select_source_color_context(
        &contexts,
        |c| Ok(c.kind),
        |c| {
            exif_queries.set(exif_queries.get() + 1);
            Ok(c.exif)
        },
    )
    .expect("no fault injected");
    assert_eq!(chosen, Some(0));
    assert_eq!(exif_queries.get(), 0, "the entry after the profile is not queried");
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_FirstNonSrgbExifColorSpaceWins_AndAnSrgbOneDoesNotBlockALaterOne
#[test]
fn select_source_color_context_first_non_srgb_exif_color_space_wins_and_an_srgb_one_does_not_block_a_later_one() {
    assert_eq!(
        select(&[
            ctx(WICColorContextExifColorSpace, 2),
            ctx(WICColorContextExifColorSpace, 3)
        ]),
        Some(0)
    );
    assert_eq!(
        select(&[
            ctx(WICColorContextExifColorSpace, SRGB_EXIF_COLOR_SPACE),
            ctx(WICColorContextExifColorSpace, 3)
        ]),
        Some(1)
    );
}

// C#: WicDirectInternalsMutationTests.SelectSourceColorContext_NullEntries_AreSkipped
#[test]
fn select_source_color_context_null_entries_are_skipped() {
    assert_eq!(select(&[None, ctx(WICColorContextExifColorSpace, 2)]), Some(1));
}

// ---------------------------------------------------------------------------------------------------------------
// C# ErrorHandlingReviewDecodingTests, DecoderAdmissionFallbackTests, DecoderErrorLocalizationTests
// ---------------------------------------------------------------------------------------------------------------

// C#: ErrorHandlingReviewDecodingTests.WicOutputGuard_RefusesOnlyWhenItDoesNotFit
#[test]
fn wic_output_guard_refuses_only_when_it_does_not_fit() {
    let tight = CountingMemory::new(64 * MIB, 0);
    assert!(ensure_output_fits(8192, 4096, GUARD_THRESHOLD_BYTES, &tight).is_err());
    let roomy = CountingMemory::new(64 * GIB, 0);
    assert!(ensure_output_fits(8192, 4096, GUARD_THRESHOLD_BYTES, &roomy).is_ok());
    let below = CountingMemory::new(0, 0);
    assert!(ensure_output_fits(8192, 4095, GUARD_THRESHOLD_BYTES - 1, &below).is_ok());
}

// C#: ErrorHandlingReviewDecodingTests.WicInvalidArg_BecomesFallbackable
#[test]
fn wic_invalid_arg_becomes_fallbackable() {
    let relabelled = relabel_invalid_argument(
        DecodeError::InvalidArgument {
            message: "E_INVALIDARG".to_owned(),
        },
        DECODER_LABEL,
    );
    assert!(
        matches!(relabelled.kind(), DecodeError::InvalidData { .. }),
        "{relabelled}"
    );
    assert!(fallbackable(&relabelled));
}

// C#: DecoderAdmissionFallbackTests (message part; the refusal-propagation part is W4-12b's)
#[test]
fn output_admission_refusal_message_says_too_large_for_the_available_memory() {
    let memory = CountingMemory::new(512 * MIB, 0);
    let result = ensure_output_fits(16_384, 16_384, GIB, &memory);
    match result.err().as_ref().map(DecodeError::kind) {
        Some(DecodeError::MemoryAdmission { message }) => {
            assert!(message.contains("too large for the available memory"), "{message}");
        }
        other => panic!("expected MemoryAdmission, got {other:?}"),
    }
    let direct = output_too_large(16_384, 16_384, GIB);
    assert!(matches!(direct.kind(), DecodeError::MemoryAdmission { .. }));
}

// C#: DecoderErrorLocalizationTests.WicIccTransformFailure_IsLocalizedAndFallbackable (English parts)
#[test]
fn wic_icc_transform_failure_is_not_supported_with_english_text_and_fallbackable() {
    let error = icc_transform_failed("boom", Some(-2_003_292_336));
    assert!(matches!(error.kind(), DecodeError::NotSupported { .. }), "{error}");
    assert_eq!(error.inner_hresult(), Some(-2_003_292_336));
    assert!(
        error.message().starts_with("WicDirect could not transform"),
        "{}",
        error.message()
    );
    assert!(fallbackable(&error));
}

// C#: DecoderErrorLocalizationTests.WicIccTransformFailure_IsLocalizedAndFallbackable (localized sentence, contract OQ1)
#[test]
#[ignore = "blocked: W4-11c OQ1 localization text seam"]
fn wic_icc_transform_failure_carries_the_localized_sentence() {
    let error = icc_transform_failed("boom", Some(-2_003_292_336));
    match error.kind() {
        DecodeError::NotSupported {
            user_text,
            inner_hresult,
            ..
        } => {
            assert!(user_text.is_some(), "localized sentence attached");
            assert_eq!(*inner_hresult, Some(-2_003_292_336));
        }
        other => panic!("expected NotSupported, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Repair round 1 (verifier findings): every decode_error_from_wic arm, and the SourceStreamBackend contract
// ---------------------------------------------------------------------------------------------------------------

/// Oracle: .NET 10 `Marshal.GetExceptionForHR(hr, -1)` types (verifier sweep); each row pins one arm of
/// `decode_error_from_wic` and its INV-12 fallback verdict (C# `FallbackImageDecoder.IsFallbackable`).
#[test]
fn decode_error_from_wic_every_arm_matches_the_dotnet_type() {
    let rows: [(u32, &str, bool); 13] = [
        (0x8013_1516, "Overflow", true),
        (0x8013_1537, "Format", true),
        (0x8007_000B, "BadImageFormat", true),
        (0x8013_1524, "DllNotFound", true),
        (0x8013_1523, "EntryPointNotFound", true),
        (0x8013_1509, "InvalidOperation", false),
        (0x8013_1622, "Disposed", false),
        (0x8013_153B, "Canceled", false),
        (0x8007_0002, "FileNotFound", false),
        (0x8007_0003, "DirectoryNotFound", false),
        (0x8013_1620, "Io", false),
        (0x8007_0026, "Io", false),
        (0x8007_00CE, "Io", false),
    ];
    for (hr, variant, fallbackable) in rows {
        let error = decode_error_from_wic(&WicError::Com { hresult: hr as i32 });
        let got = match &error {
            DecodeError::Overflow { .. } => "Overflow",
            DecodeError::Format { .. } => "Format",
            DecodeError::BadImageFormat { .. } => "BadImageFormat",
            DecodeError::DllNotFound { .. } => "DllNotFound",
            DecodeError::EntryPointNotFound { .. } => "EntryPointNotFound",
            DecodeError::InvalidOperation { .. } => "InvalidOperation",
            DecodeError::Disposed { .. } => "Disposed",
            DecodeError::Canceled => "Canceled",
            DecodeError::FileNotFound { .. } => "FileNotFound",
            DecodeError::DirectoryNotFound { .. } => "DirectoryNotFound",
            DecodeError::Io(_) => "Io",
            _ => "Other",
        };
        assert_eq!(got, variant, "{hr:08X}");
        assert_eq!(FallbackImageDecoder::is_fallbackable(&error), fallbackable, "{hr:08X}");
    }
}

/// `Read` that counts the bytes the source hands out (the disk reads of a `FileStream`).
struct CountingSource {
    inner: std::io::Cursor<Vec<u8>>,
    bytes: Arc<AtomicUsize>,
    reads: Arc<AtomicUsize>,
}

impl std::io::Read for CountingSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = std::io::Read::read(&mut self.inner, buf)?;
        self.bytes.fetch_add(read, Ordering::SeqCst);
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(read)
    }
}

impl std::io::Seek for CountingSource {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.inner, pos)
    }
}

/// A backend over `data` read through a `BufReader` of `capacity` (the `PhysicalSourceReader` shape), plus the
/// (bytes, reads) counters of the underlying source.
fn counted_backend(
    data: Vec<u8>,
    capacity: usize,
    window: usize,
) -> (SourceStreamBackend, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let bytes = Arc::new(AtomicUsize::new(0));
    let reads = Arc::new(AtomicUsize::new(0));
    let source = CountingSource {
        inner: std::io::Cursor::new(data),
        bytes: Arc::clone(&bytes),
        reads: Arc::clone(&reads),
    };
    let reader = std::io::BufReader::with_capacity(capacity, source);
    (
        SourceStreamBackend::with_buffer_size(Box::new(reader), window),
        bytes,
        reads,
    )
}

fn read_exact_at(backend: &mut SourceStreamBackend, position: u64, len: usize) -> Vec<u8> {
    backend.seek(SeekFrom::Start(position)).expect("seek");
    let mut out = vec![0u8; len];
    let read = backend.read(&mut out).expect("read");
    out.truncate(read);
    out
}

// Verifier finding (AGENTS priority 1, read each file once): C# `FileStream` keeps its buffer on `Length` and on seeks
// that stay inside it, so a source below the buffer size is read from disk exactly once whatever WIC seeks.
#[test]
fn source_stream_backend_reads_a_small_source_once_whatever_the_seeks() {
    let data: Vec<u8> = (0..200u32).map(|i| i as u8).collect();
    let (mut backend, bytes, reads) = counted_backend(data.clone(), 256, 256);
    assert_eq!(backend.len().expect("len"), 200);
    assert_eq!(read_exact_at(&mut backend, 0, 10), data[0..10].to_vec());
    assert_eq!(backend.len().expect("len"), 200);
    assert_eq!(read_exact_at(&mut backend, 150, 20), data[150..170].to_vec());
    assert_eq!(read_exact_at(&mut backend, 4, 4), data[4..8].to_vec());
    backend.seek(SeekFrom::End(-5)).expect("seek end");
    let mut tail = [0u8; 16];
    assert_eq!(backend.read(&mut tail).expect("tail"), 5);
    assert_eq!(&tail[..5], &data[195..200]);
    assert_eq!(backend.read(&mut tail).expect("eof"), 0);
    backend.seek(SeekFrom::Current(-100)).expect("seek back");
    assert_eq!(read_exact_at(&mut backend, 100, 3), data[100..103].to_vec());
    assert_eq!(
        bytes.load(Ordering::SeqCst),
        200,
        "every byte read from the source once"
    );
    // One fill; the other source calls are the two end-of-stream probes (0 bytes each).
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[test]
fn source_stream_backend_larger_source_refills_and_large_reads_go_direct() {
    let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let (mut backend, bytes, _) = counted_backend(data.clone(), 64, 64);
    assert_eq!(read_exact_at(&mut backend, 0, 10), data[0..10].to_vec());
    assert_eq!(
        read_exact_at(&mut backend, 60, 10),
        data[60..70].to_vec(),
        "straddles the window"
    );
    assert_eq!(
        read_exact_at(&mut backend, 500, 300),
        data[500..800].to_vec(),
        "direct read"
    );
    assert_eq!(
        read_exact_at(&mut backend, 990, 50),
        data[990..1000].to_vec(),
        "short at the end"
    );
    assert!(
        bytes.load(Ordering::SeqCst) <= 64 + 64 + 300 + 10,
        "no source byte read twice here: {}",
        bytes.load(Ordering::SeqCst)
    );
}

#[test]
fn source_stream_backend_seek_before_begin_fails_and_keeps_the_position() {
    let (mut backend, _, _) = counted_backend(vec![1, 2, 3, 4], 16, 16);
    backend.seek(SeekFrom::Start(2)).expect("seek");
    let error = backend.seek(SeekFrom::Current(-3)).expect_err("before begin");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let error = backend.seek(SeekFrom::End(-5)).expect_err("before begin");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let mut one = [0u8; 1];
    assert_eq!(backend.read(&mut one).expect("read"), 1);
    assert_eq!(one[0], 3, "position unchanged by the failed seeks");
    assert_eq!(backend.seek(SeekFrom::Start(10)).expect("past the end"), 10);
    assert_eq!(backend.read(&mut one).expect("eof"), 0);
}

// Verifier finding: C# `NotSupportedException` of a read-only stream; `Length` keeps the position.
#[test]
fn source_stream_backend_len_keeps_position_and_is_read_only() {
    let mut backend = SourceStreamBackend::new(Box::new(std::io::Cursor::new(vec![1u8, 2, 3, 4, 5, 6, 7])));
    backend.seek(SeekFrom::Start(3)).expect("seek");
    assert_eq!(backend.len().expect("len"), 7);
    let mut one = [0u8; 1];
    assert_eq!(backend.read(&mut one).expect("read"), 1);
    assert_eq!(one[0], 4, "position kept after len");
    let error = backend.write_all(&[1]).expect_err("read-only");
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(error.to_string(), "Stream does not support writing.");
    let error = backend.set_len(1).expect_err("read-only");
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(error.to_string(), "Stream does not support writing.");
    assert!(backend.flush().is_ok());
}

#[test]
fn source_stream_backend_starts_at_the_source_position() {
    let mut cursor = std::io::Cursor::new(vec![9u8, 8, 7, 6]);
    cursor.set_position(2);
    let mut backend = SourceStreamBackend::new(Box::new(cursor));
    assert_eq!(
        backend.len().expect("len"),
        4,
        "len first must not move the logical position"
    );
    let mut two = [0u8; 2];
    assert_eq!(backend.read(&mut two).expect("read"), 2);
    assert_eq!(two, [7, 6]);
}

// ---------------------------------------------------------------------------------------------------------------
// Repair round 2 (verifier mutants on SourceStreamBackend: read_inner_at position, partial-copy fault, i64 seek cap)
// ---------------------------------------------------------------------------------------------------------------

fn ramp() -> Vec<u8> {
    (0u8..64).collect()
}

// Kills `position + read` -> `position * read` in `read_inner_at`: the remembered source position after a direct
// read must be its end, otherwise a later direct read at `position * read` skips its seek and returns wrong bytes.
#[test]
fn source_stream_backend_direct_read_after_direct_read_returns_the_target_bytes() {
    let mut backend = SourceStreamBackend::with_buffer_size(Box::new(std::io::Cursor::new(ramp())), 4);
    assert_eq!(read_exact_at(&mut backend, 2, 8), (2u8..10).collect::<Vec<_>>());
    assert_eq!(
        read_exact_at(&mut backend, 16, 8),
        (16u8..24).collect::<Vec<_>>(),
        "bytes at 16"
    );
    assert_eq!(
        read_exact_at(&mut backend, 24, 8),
        (24u8..32).collect::<Vec<_>>(),
        "sequential direct read"
    );
}

/// A source whose first read succeeds and every later read fails.
struct FailsAfterFirstRead {
    inner: std::io::Cursor<Vec<u8>>,
    reads: usize,
}

impl std::io::Read for FailsAfterFirstRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads += 1;
        if self.reads > 1 {
            return Err(std::io::Error::other("disk fault"));
        }
        std::io::Read::read(&mut self.inner, buf)
    }
}

impl std::io::Seek for FailsAfterFirstRead {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.inner, pos)
    }
}

// Kills the `copied > 0` guard mutants in `read`: a read that copies the rest of the window and then meets a source
// fault reports the copied bytes; with nothing copied the fault surfaces.
#[test]
fn source_stream_backend_fault_after_a_partial_window_copy_reports_the_copied_bytes() {
    let source = FailsAfterFirstRead {
        inner: std::io::Cursor::new(ramp()),
        reads: 0,
    };
    let mut backend = SourceStreamBackend::with_buffer_size(Box::new(source), 4);
    let mut head = [0u8; 2];
    assert_eq!(backend.read(&mut head).expect("first read fills the window"), 2);
    assert_eq!(head, [0, 1]);
    let mut rest = [0u8; 3];
    assert_eq!(backend.read(&mut rest).expect("the two window bytes are reported"), 2);
    assert_eq!(&rest[..2], &[2, 3]);
    let mut more = [0u8; 1];
    let error = backend.read(&mut more).expect_err("nothing copied: the fault surfaces");
    assert_eq!(error.to_string(), "disk fault");
}

// Kills the `i64::try_from(target).is_ok()` guard -> `true` mutant in `seek`: a target past `i64::MAX` is refused
// with `InvalidInput` and the position is unchanged; `i64::MAX` itself is accepted.
#[test]
fn source_stream_backend_seek_past_i64_max_is_refused() {
    let mut backend = SourceStreamBackend::with_buffer_size(Box::new(std::io::Cursor::new(ramp())), 4);
    backend.seek(SeekFrom::Start(10)).expect("seek 10");
    let error = backend.seek(SeekFrom::Current(i64::MAX)).expect_err("past i64::MAX");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let error = backend.seek(SeekFrom::Start(u64::MAX)).expect_err("u64::MAX");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let error = backend
        .seek(SeekFrom::Start(i64::MAX as u64 + 1))
        .expect_err("i64::MAX + 1");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(backend.seek(SeekFrom::Current(0)).expect("position"), 10);
    assert_eq!(
        backend.seek(SeekFrom::Start(i64::MAX as u64)).expect("i64::MAX"),
        i64::MAX as u64
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Repair round 3 (verifier finding: SourceStreamBackend seek-before-begin HRESULT)
// ---------------------------------------------------------------------------------------------------------------

// Oracle (.NET 10, `new FileStream(path, Open, Read, ReadWrite | Delete, 1 MiB, SequentialScan)`, the C#
// `PhysicalSourceReader` stream): `Seek(-1, Begin)`, `Seek(-5, Current)`, `Seek(-101, End)` on 100 bytes and
// `Seek(long.MaxValue, End)` all throw `IOException` HResult 0x80070057 and keep the position; the COM caller (WIC) sees
// that HRESULT. The memory default (`COR_E_IO`) was wrong for the file source.
#[test]
fn source_stream_backend_seek_before_begin_is_the_filestream_hresult() {
    use pr_platform_win::wic::interop::ComStream;
    let backend = SourceStreamBackend::new(Box::new(std::io::Cursor::new(vec![0u8; 100])));
    assert_eq!(backend.seek_before_begin_fault().hresult(), E_INVALIDARG);
    assert_eq!(E_INVALIDARG, hresult_from_win32(87));
    let stream = ComStream::new(Box::new(backend));
    assert_eq!(stream.seek(2, 0).expect("seek 2"), 2);
    for (offset, origin) in [(-1i64, 0u32), (-5, 1), (-101, 2), (i64::MAX, 2)] {
        let fault = stream.seek(offset, origin).expect_err("before begin");
        assert_eq!(fault.hresult(), E_INVALIDARG, "Seek({offset}, {origin})");
        assert_ne!(fault.hresult(), COR_E_IO);
    }
    assert_eq!(stream.seek(0, 1).expect("position"), 2, "position kept");
    assert_eq!(stream.seek(i64::MAX, 0).expect("i64::MAX"), i64::MAX as u64);
}
