//! Port tests for `pr_imaging::decoder_traits` (C# baseline d92d6aae, `PhotoReview.Imaging/Decoding/`): the three
//! refusal exceptions (`DecoderBusyException`, `DecoderMemoryAdmissionException`, `MissingImageCodecException`), the
//! `DecodeError` mapping the fallback whitelist relies on, the decoder seams, and the golden `decoder-exceptions`
//! (oracle-cs `decoder-exceptions`, contracts/W4-01c.md).
//!
//! Documented skips (not ported here, owned by the units named):
//! - `IsSealed` in the golden: reflection fact with no Rust equivalent (the enum is closed by construction).
//! - `DecoderBusyFallbackTests.IsFallbackable_*` and the `IsFallbackable(...)` half of `DecoderAdmissionFallbackTests`
//!   line 54: W4-12b (`FallbackImageDecoder`). Here only the `IOException` fact of the admission refusal is ported.
//! - `WebpHeicRoutingDecoderTests.cs`: W4-12c. `FallbackExceptionCoverageTests` / `FactoryTests`: W4-12a/b.
//! - `PreviewImageServiceBusyJoinTests.cs`: W4-14. `PreloadScheduler` busy back-off: W4-15.
//! - `PixelBufferImageCodec` tests: W4-16b. `ImagePresenterTests.*` busy tests: App, W8.
//! - The trait bodies (`ImageDecoder`, `ImageDecoderFactory`, `PlatformImageCodec`) have no implementation in this unit;
//!   only their object safety and `Send + Sync` bounds are checked here.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_imaging::decode_types::DecodeFailureSourceBytes;
use pr_imaging::decoder_traits::{
    CancellableImageDecoder, DECODER_BUSY_MESSAGE, DECODER_MEMORY_ADMISSION_MESSAGE, DecodeError, ImageDecoder,
    ImageDecoderFactory, MISSING_IMAGE_CODEC_MESSAGE, PlatformImageCodec, UserText,
};
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

/// The fallback whitelist of C# `FallbackImageDecoder.IsFallbackable` (W4-12b implements it as a `match`). Reproduced
/// here so the refusals can be checked against it without depending on W4-12b. `MissingCodec` is in the whitelist
/// because C# `MissingImageCodecException` is a `NotSupportedException` (decoder_traits.rs, semantics note 3).
fn whitelisted_fallbackable(error: &DecodeError) -> bool {
    matches!(
        error,
        DecodeError::NotSupported { .. }
            | DecodeError::MissingCodec { .. }
            | DecodeError::InvalidCast { .. }
            | DecodeError::Overflow { .. }
            | DecodeError::Format { .. }
            | DecodeError::InvalidData { .. }
            | DecodeError::Com { .. }
            | DecodeError::DllNotFound { .. }
            | DecodeError::BadImageFormat { .. }
            | DecodeError::EntryPointNotFound { .. }
    )
}

// C#: DecoderBusyFallbackTests.DecoderBusyException_IsNotAFallbackableExceptionType_SoTheWhitelistNeverSeesItAsOne
#[test]
fn decoder_busy_exception_is_not_a_fallbackable_exception_type_so_the_whitelist_never_sees_it_as_one() {
    for error in [DecodeError::busy(), DecodeError::busy_with("queue full")] {
        assert!(!error.is_not_supported(), "Busy must not be a NotSupportedException");
        assert!(!error.is_io_exception(), "Busy must not be an IOException");
        assert!(
            !whitelisted_fallbackable(&error),
            "Busy must never be fallbackable (INV-12)"
        );
    }
}

// C#: DecoderAdmissionFallbackTests line 54, the IOException fact only (`IsFallbackable` half is W4-12b).
// Admission refusals derive IOException in C# so "cannot show this image" handlers keep working.
#[test]
fn decoder_memory_admission_exception_derives_io_exception() {
    let error = DecodeError::memory_admission_with("no room");
    assert!(error.is_io_exception(), "MemoryAdmission derives IOException");
    assert!(
        !error.is_not_supported(),
        "MemoryAdmission is not a NotSupportedException"
    );
    assert!(
        !whitelisted_fallbackable(&error),
        "MemoryAdmission must never be fallbackable (INV-12)"
    );
    assert_eq!(error.message(), "no room");
}

// C#: MissingImageCodecException (no own test class; type fact from its base class NotSupportedException).
#[test]
fn missing_image_codec_exception_is_not_supported_and_not_io() {
    let error = DecodeError::missing_codec();
    assert!(
        error.is_not_supported(),
        "MissingImageCodecException derives NotSupportedException"
    );
    assert!(
        !error.is_io_exception(),
        "MissingImageCodecException is not an IOException"
    );
    assert_eq!(error.message(), MISSING_IMAGE_CODEC_MESSAGE);
}

/// Builds the error the golden case describes. `inner` cases pass the outer message only: Rust folds the cause into
/// the message text at the throw site (semantics note 6), so only `message` is compared for them.
fn construct(case: &Value) -> DecodeError {
    let ctor = str_of(case, "ctor");
    let arg = match ctor {
        "message" => Some(str_of(case, "message_in")),
        "inner" => Some(str_of(case, "message")),
        _ => None,
    };
    match (str_of(case, "type"), arg) {
        ("busy", None) => DecodeError::busy(),
        ("busy", Some(m)) => DecodeError::busy_with(m),
        ("memory-admission", None) => DecodeError::memory_admission(),
        ("memory-admission", Some(m)) => DecodeError::memory_admission_with(m),
        ("missing-codec", None) => DecodeError::missing_codec(),
        ("missing-codec", Some(m)) => DecodeError::missing_codec_with(m),
        (other, _) => panic!("unknown golden type: {other}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/decoder-exceptions.json (oracle-cs decoder-exceptions, 15 cases)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn decoder_exceptions_golden_matches_dotnet() {
    let g = golden::load("decoder-exceptions");
    assert_eq!(g.generator, "oracle-cs decoder-exceptions");
    assert_eq!(g.cases.len(), 15, "golden has 5 cases per exception type");
    for case in &g.cases {
        let name = str_of(case, "name");
        let error = construct(case);

        assert_eq!(error.message(), str_of(case, "message"), "{name}: message");

        let probes = &case["assignable_to"];
        assert_eq!(
            error.is_io_exception(),
            bool_of(probes, "IOException"),
            "{name}: IOException"
        );
        assert_eq!(
            error.is_not_supported(),
            bool_of(probes, "NotSupportedException"),
            "{name}: NotSupportedException"
        );
        // The other five probes are false for all three types; no Rust predicate exists for them, so check that the
        // refusal is not one of the variants that map to those C# types.
        for other in [
            "InvalidDataException",
            "COMException",
            "InvalidCastException",
            "OverflowException",
            "FormatException",
        ] {
            assert!(
                !bool_of(probes, other),
                "{name}: golden says {other} is false for this type"
            );
        }
        assert!(
            !matches!(
                error,
                DecodeError::InvalidData { .. }
                    | DecodeError::Com { .. }
                    | DecodeError::InvalidCast { .. }
                    | DecodeError::Overflow { .. }
                    | DecodeError::Format { .. }
            ),
            "{name}: refusal must not map to a C# type it does not derive from"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Rust-only tests for the mapping and message rules (contracts/W4-01c.md semantics notes 4, 5, 6).
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn default_messages_are_the_three_csharp_constants() {
    assert_eq!(DecodeError::busy().message(), DECODER_BUSY_MESSAGE);
    assert_eq!(
        DecodeError::memory_admission().message(),
        DECODER_MEMORY_ADMISSION_MESSAGE
    );
    assert_eq!(DecodeError::missing_codec().message(), MISSING_IMAGE_CODEC_MESSAGE);
}

#[test]
fn display_is_the_exception_message_including_empty_and_unicode() {
    assert_eq!(
        DecodeError::busy_with("").to_string(),
        "",
        "an empty message stays empty"
    );
    assert_eq!(
        DecodeError::missing_codec_with("Thiếu codec: é中").to_string(),
        "Thiếu codec: é中",
        "no re-encoding"
    );
    assert_eq!(DecodeError::memory_admission_with("no room").to_string(), "no room");
}

#[test]
fn canceled_message_is_the_dotnet_operation_canceled_text() {
    assert_eq!(DecodeError::Canceled.message(), "The operation was canceled.");
    assert!(!DecodeError::Canceled.is_io_exception());
    assert!(!DecodeError::Canceled.is_not_supported());
}

#[test]
fn from_io_not_found_maps_to_file_not_found_and_is_io() {
    let error = DecodeError::from(io::Error::new(io::ErrorKind::NotFound, "gone"));
    assert!(
        matches!(error, DecodeError::FileNotFound { .. }),
        "NotFound must map to FileNotFound"
    );
    assert!(error.is_io_exception(), "FileNotFoundException is an IOException");
    assert!(!whitelisted_fallbackable(&error), "FileNotFound is never fallbackable");
}

#[test]
fn from_io_other_kinds_map_to_io_and_never_to_memory_admission() {
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::Other,
    ] {
        let error = DecodeError::from(io::Error::new(kind, "denied"));
        assert!(matches!(error, DecodeError::Io(_)), "{kind:?} must map to Io");
        assert!(error.is_io_exception());
        assert!(
            !matches!(error, DecodeError::MemoryAdmission { .. }),
            "only decoders produce MemoryAdmission"
        );
    }
}

#[test]
fn decode_error_is_a_std_error_whose_source_is_the_io_cause_only() {
    let io_error = DecodeError::from(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
    let source = std::error::Error::source(&io_error);
    assert!(source.is_some(), "Io exposes its cause");
    assert!(std::error::Error::source(&DecodeError::busy()).is_none());
}

// ---------------------------------------------------------------------------------------------------------------
// Compile-time seam checks: the traits are object safe and Send + Sync (semantics note 9).
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn decoder_seams_are_object_safe_and_send_sync() {
    fn assert_send_sync<T: ?Sized + Send + Sync>() {}
    assert_send_sync::<dyn ImageDecoder>();
    assert_send_sync::<dyn CancellableImageDecoder>();
    assert_send_sync::<dyn ImageDecoderFactory>();
    assert_send_sync::<dyn PlatformImageCodec>();
    assert_send_sync::<DecodeError>();
}

// ---------------------------------------------------------------------------------------------------------------
// Amendment A1 (localized sentence, UI text at display time) and the WithSourceBytes wrapper of A2. Rust-only.
// The source-bytes round trip (`attach_source_bytes` -> `source_bytes`) needs the W4-01b bodies of
// `DecodeFailureSourceBytes::attach/try_get` and is tested with W4-01b / W4-12b, not here.
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn localized_not_supported_describes_with_the_closure_and_keeps_the_english_message() {
    let text: UserText = Arc::new(|| "Bản địa hóa".to_owned());
    let error = DecodeError::not_supported_localized("english", text);
    assert_eq!(error.message(), "english");
    assert_eq!(error.describe(), "Bản địa hóa");
    assert!(error.is_localized());
    assert!(error.is_not_supported());
    assert!(!error.is_io_exception());
}

#[test]
fn localized_missing_codec_describes_with_the_closure() {
    let text: UserText = Arc::new(|| "Thiếu codec".to_owned());
    let error = DecodeError::missing_codec_localized("english", text);
    assert_eq!(error.message(), "english");
    assert_eq!(error.describe(), "Thiếu codec");
    assert!(error.is_localized());
    assert!(error.is_not_supported(), "MissingCodec stays a NotSupportedException");
}

#[test]
fn text_is_evaluated_at_display_time_not_at_construction() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let text: UserText = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        "t".to_owned()
    });
    let error = DecodeError::missing_codec_localized("m", text);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "the closure must not run while the error is built"
    );
    assert_eq!(error.describe(), "t");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn describe_without_text_is_the_message_and_not_localized() {
    let plain = DecodeError::NotSupported {
        message: "m".to_owned(),
        user_text: None,
        inner_hresult: None,
    };
    assert!(!plain.is_localized());
    assert_eq!(plain.describe(), "m");
    assert_eq!(DecodeError::missing_codec().describe(), MISSING_IMAGE_CODEC_MESSAGE);
}

#[test]
fn not_supported_inner_hresult_survives_localization_and_wrappers() {
    let error = DecodeError::not_supported_with_inner_hresult("icc failed", -2_003_292_336);
    assert_eq!(error.inner_hresult(), Some(-2_003_292_336));
    let localized = error.with_user_text(text("localized"));
    assert_eq!(localized.inner_hresult(), Some(-2_003_292_336));
    let wrapped = localized.attach_source_bytes(Arc::from([1_u8, 2]));
    assert_eq!(wrapped.inner_hresult(), Some(-2_003_292_336));
    assert_eq!(wrapped.describe(), "localized");
}

#[test]
fn describe_falls_back_to_the_message_when_the_closure_panics() {
    let text: UserText = Arc::new(|| panic!("closure failed"));
    let error = DecodeError::missing_codec_localized("english", text);
    assert_eq!(
        error.describe(),
        "english",
        "a panicking sentence must not unwind the caller"
    );
}

#[test]
fn refusals_and_plain_errors_are_not_localized_until_text_is_attached() {
    assert!(!DecodeError::busy().is_localized());
    assert!(!DecodeError::memory_admission().is_localized());
    assert!(!DecodeError::Canceled.is_localized());
    assert!(DecodeError::busy().user_text().is_none());
    assert_eq!(DecodeError::busy().describe(), DECODER_BUSY_MESSAGE);
}

fn text(s: &'static str) -> UserText {
    Arc::new(move || s.to_owned())
}

// C#: UserFacingError.Localized (any exception type: TurboJpegDecoder InvalidDataException/FileNotFoundException/
// ArgumentException/InvalidOperationException, RawDecoder IOException, SourceBytesCache EndOfStreamException)
#[test]
fn with_user_text_localizes_every_variant_and_keeps_type_predicates_and_message() {
    let cases: Vec<(DecodeError, bool, bool)> = vec![
        (DecodeError::Busy { message: "m".into() }, false, false),
        (DecodeError::MemoryAdmission { message: "m".into() }, true, false),
        (DecodeError::FileNotFound { message: "m".into() }, true, false),
        (DecodeError::DirectoryNotFound { message: "m".into() }, true, false),
        (DecodeError::Canceled, false, false),
        (DecodeError::OutOfMemory { message: "m".into() }, false, false),
        (DecodeError::InvalidCast { message: "m".into() }, false, false),
        (DecodeError::Overflow { message: "m".into() }, false, false),
        (DecodeError::Format { message: "m".into() }, false, false),
        (DecodeError::InvalidData { message: "m".into() }, false, false),
        (
            DecodeError::Com {
                hresult: -1,
                message: "m".into(),
            },
            false,
            false,
        ),
        (DecodeError::DllNotFound { message: "m".into() }, false, false),
        (DecodeError::BadImageFormat { message: "m".into() }, false, false),
        (DecodeError::EntryPointNotFound { message: "m".into() }, false, false),
        (DecodeError::InvalidOperation { message: "m".into() }, false, false),
        (DecodeError::InvalidArgument { message: "m".into() }, false, false),
        (DecodeError::Disposed { message: "m".into() }, false, false),
        (DecodeError::Io(io::Error::other("m")), true, false),
        (DecodeError::Other { message: "m".into() }, false, false),
        (
            DecodeError::NotSupported {
                message: "m".into(),
                user_text: None,
                inner_hresult: None,
            },
            false,
            true,
        ),
        (DecodeError::missing_codec_with("m"), false, true),
    ];
    for (error, io_exception, not_supported) in cases {
        let name = format!("{error:?}");
        let before = error.message();
        let kind_name = format!("{:?}", error.kind());
        let localized = error.with_user_text(text("Địa phương"));
        assert!(localized.is_localized(), "{name}");
        assert_eq!(localized.describe(), "Địa phương", "{name}");
        assert_eq!(localized.message(), before, "{name}: English message unchanged");
        assert_eq!(localized.to_string(), before, "{name}");
        assert_eq!(localized.is_io_exception(), io_exception, "{name}");
        assert_eq!(localized.is_not_supported(), not_supported, "{name}");
        assert_eq!(
            format!("{:?}", localized.kind()),
            kind_name,
            "{name}: kind() peels the wrapper"
        );
    }
}

#[test]
fn localized_wrapper_second_call_replaces_the_text() {
    let error = DecodeError::InvalidData { message: "m".into() }
        .with_user_text(text("one"))
        .with_user_text(text("two"));
    assert_eq!(error.describe(), "two");
    assert!(
        matches!(
            &error,
            DecodeError::Localized { error, .. } if matches!(**error, DecodeError::InvalidData { .. })
        ),
        "replaced in place, not nested"
    );
    let codec = DecodeError::missing_codec_localized("m", text("one")).with_user_text(text("two"));
    assert!(matches!(codec, DecodeError::MissingCodec { .. }), "stays unwrapped");
    assert_eq!(codec.describe(), "two");
}

#[test]
fn localized_closure_is_lazy_and_a_panic_falls_back_to_the_message_for_wrapped_variants() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let lazy: UserText = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        "t".to_owned()
    });
    let error = DecodeError::FileNotFound {
        message: "english".into(),
    }
    .with_user_text(lazy);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(error.describe(), "t");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let boom: UserText = Arc::new(|| panic!("closure failed"));
    let error = DecodeError::InvalidData {
        message: "english".into(),
    }
    .with_user_text(boom);
    assert_eq!(error.describe(), "english");
}

#[test]
fn text_and_source_bytes_wrappers_compose_in_either_order() {
    let bytes: Arc<[u8]> = Arc::from(vec![1u8, 2, 3]);
    let a = DecodeError::InvalidData { message: "m".into() }
        .with_user_text(text("loc"))
        .attach_source_bytes(bytes.clone());
    let b = DecodeError::InvalidData { message: "m".into() }
        .attach_source_bytes(bytes.clone())
        .with_user_text(text("loc"));
    for error in [a, b] {
        assert_eq!(error.describe(), "loc");
        assert_eq!(error.message(), "m");
        assert!(error.source_bytes().is_some());
        assert!(matches!(error.kind(), DecodeError::InvalidData { .. }));
        assert!(error.is_localized());
    }
}

#[test]
fn debug_and_source_delegate_through_the_text_wrapper() {
    use std::error::Error;
    let error = DecodeError::Io(io::Error::new(io::ErrorKind::PermissionDenied, "denied")).with_user_text(text("loc"));
    assert!(error.source().is_some());
    let shown = format!("{error:?}");
    assert!(shown.contains("Localized") && shown.contains("denied"), "got {shown}");
}

#[test]
fn with_source_bytes_wrapper_is_transparent_for_message_and_predicates() {
    let inner = DecodeError::missing_codec_localized("english", Arc::new(|| "local".to_owned()));
    let wrapped = DecodeError::WithSourceBytes {
        error: Box::new(inner),
        bytes: DecodeFailureSourceBytes::none(),
    };
    assert_eq!(wrapped.message(), "english");
    assert_eq!(wrapped.to_string(), "english");
    assert!(wrapped.is_not_supported());
    assert!(!wrapped.is_io_exception());
    assert!(wrapped.is_localized());
    assert_eq!(wrapped.describe(), "local");
    assert!(matches!(wrapped.kind(), DecodeError::MissingCodec { .. }));
}

#[test]
fn kind_peels_the_wrapper_for_io_and_memory_admission() {
    let admission = DecodeError::WithSourceBytes {
        error: Box::new(DecodeError::memory_admission_with("no room")),
        bytes: DecodeFailureSourceBytes::none(),
    };
    assert!(
        admission.is_io_exception(),
        "MemoryAdmission stays an IOException under the wrapper"
    );
    assert!(matches!(admission.kind(), DecodeError::MemoryAdmission { .. }));
    assert!(matches!(DecodeError::busy().kind(), DecodeError::Busy { .. }));
}

#[test]
fn debug_names_the_variant_and_the_message_without_the_closure() {
    let text = format!(
        "{:?}",
        DecodeError::missing_codec_localized("english", Arc::new(|| "local".to_owned()))
    );
    assert!(text.contains("MissingCodec"), "got {text}");
    assert!(text.contains("english"), "got {text}");
}

#[test]
fn error_source_is_delegated_through_the_wrapper() {
    use std::error::Error;
    let wrapped = DecodeError::WithSourceBytes {
        error: Box::new(DecodeError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "denied",
        ))),
        bytes: DecodeFailureSourceBytes::none(),
    };
    assert!(wrapped.source().is_some());
}

#[test]
fn plain_error_has_no_source_bytes() {
    assert!(DecodeError::busy().source_bytes().is_none());
}

/// A2 round trip (attach replaces, empty buffer is `Some`).
#[test]
fn attach_then_source_bytes_round_trip() {
    let bytes: Arc<[u8]> = Arc::from(vec![1u8, 2, 3]);
    let e = DecodeError::missing_codec().attach_source_bytes(bytes);
    assert_eq!(e.source_bytes().map(|b| b.to_vec()), Some(vec![1, 2, 3]));
    let empty: Arc<[u8]> = Arc::from(Vec::<u8>::new());
    let e = e.attach_source_bytes(empty);
    assert_eq!(e.source_bytes().map(|b| b.len()), Some(0));
    assert!(matches!(e.kind(), DecodeError::MissingCodec { .. }));
}

#[test]
fn attach_replaces_earlier_bytes_without_nesting() {
    let first: Arc<[u8]> = Arc::from(vec![9u8]);
    let second: Arc<[u8]> = Arc::from(vec![4u8, 5]);
    let e = DecodeError::busy()
        .attach_source_bytes(first)
        .attach_source_bytes(second);
    assert_eq!(e.source_bytes().map(|b| b.to_vec()), Some(vec![4, 5]));
    match &e {
        DecodeError::WithSourceBytes { error, .. } => {
            assert!(matches!(**error, DecodeError::Busy { .. }), "no double wrapping");
        }
        other => panic!("expected wrapper, got {other:?}"),
    }
}

#[test]
fn empty_buffer_is_some_and_unattached_wrapper_is_none() {
    let empty: Arc<[u8]> = Arc::from(Vec::<u8>::new());
    let e = DecodeError::busy().attach_source_bytes(empty);
    assert_eq!(e.source_bytes().map(|b| b.len()), Some(0));
    let bare = DecodeError::WithSourceBytes {
        error: Box::new(DecodeError::busy()),
        bytes: DecodeFailureSourceBytes::none(),
    };
    assert!(bare.source_bytes().is_none());
}

#[test]
fn nested_wrapper_falls_back_to_inner_bytes() {
    let inner: Arc<[u8]> = Arc::from(vec![7u8, 8]);
    let outer = DecodeError::WithSourceBytes {
        error: Box::new(DecodeError::busy().attach_source_bytes(inner)),
        bytes: DecodeFailureSourceBytes::none(),
    };
    assert_eq!(outer.source_bytes().map(|b| b.to_vec()), Some(vec![7, 8]));
}

#[test]
fn localized_wrapper_over_source_bytes_still_exposes_the_bytes() {
    let inner: Arc<[u8]> = Arc::from(vec![5u8, 6]);
    let error = DecodeError::Localized {
        error: Box::new(DecodeError::InvalidData { message: "m".into() }.attach_source_bytes(inner)),
        text: text("loc"),
    };
    assert_eq!(error.source_bytes().map(|b| b.to_vec()), Some(vec![5, 6]));
    assert_eq!(error.describe(), "loc");
}
