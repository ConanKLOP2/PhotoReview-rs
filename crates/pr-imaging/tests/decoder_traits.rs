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

use pr_imaging::decoder_traits::{
    CancellableImageDecoder, DECODER_BUSY_MESSAGE, DECODER_MEMORY_ADMISSION_MESSAGE, DecodeError, ImageDecoder,
    ImageDecoderFactory, MISSING_IMAGE_CODEC_MESSAGE, PlatformImageCodec,
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
