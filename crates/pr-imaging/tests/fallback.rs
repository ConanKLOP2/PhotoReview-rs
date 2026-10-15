//! Port tests for `pr_imaging::fallback` (C# baseline d92d6aae, `PhotoReview.Imaging/Decoding/FallbackImageDecoder.cs`):
//! the INV-12 whitelist (`IsFallbackable`), the `Decode` / `ReadInfo` fallback flow, backend relabelling, the warning
//! text, the decoder-fallback metric and the source-bytes hand-over. Contract: `contracts/W4-12b.md`.
//! Goldens: `tests/golden/fallback-is-fallbackable.json` and `tests/golden/fallback-decoder.json` (inputs in
//! `tests/golden-inputs/`, oracle `tools/oracle-cs/Commands/W4-12b.cs`).
//!
//! Documented skips (not ported here; owned by the units named):
//! - `FallbackBackendLabelTests.Constructor_NullDecoders_Throw`: an `Arc` cannot be null, so there is no Rust twin.
//! - `FactoryTests.FactoryWrapsRegisteredBackendInFallbackDecoder`, `FactoryReturnsWpfDecoderDirectly`: W4-12a.
//! - `DecoderBusyFallbackTests.DecoderBusyException_IsNotAFallbackableExceptionType_...`: W4-01c (`decoder_traits` test).
//! - `DecoderAdmissionFallbackTests` (the two real `WicDirectDecoder` / `TurboJpegDecoder` admission tests): W4-11c / W5.
//!   The scripted twin is ported below.
//! - `DecoderContractRegressionTests.TurboSuccessReportsActualBackendAndOrientation`,
//!   `WpfSuccessReportsOrientationMetadata`: real decoders, W5.
//! - `FallbackSourceBytesTests.IccJpeg_FallbackDecodesFromBytesTurboJpegAlreadyRead`: real TurboJpeg, W5.
//! - `DecoderExifTests.FallbackDecoderKeepsExif`: real WPF/WIC decode (W4-04b / W5). The exif part is asserted by the
//!   scripted relabel tests (`exif()` is the inner image's object).
//! - `Wp06WpfFreeExceptionMappingTests`: only the two `IsFallbackable` lines are ported here (the rest is W4-10).
//! - `PreviewBackendIdentityTests`, `Robustness/*`, `Quality/*`: real decoders, W4-14 / W4-11c / W5.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PlatformImage, SourceBytes};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::fallback::{FALLBACK_BACKEND, FallbackImageDecoder};
use pr_test_support::golden::{self, bool_of, i64_of, repo_root, str_of};
use pr_test_support::{LogLevel, RecordingLog};
use serde_json::Value;

/// `0x88982F50` as the signed HRESULT the C# `COMException` default carries.
const COM_HRESULT: i32 = -2_003_292_336;

/// Golden cases whose `bytes_len` disagrees with the C# source (`DecodeFailureSourceBytes.TryGet` hands the attached
/// buffer over, so the fallback request must carry it). The committed golden says 0 for these two. Their hand-over is
/// asserted through the attached length here until the goldens are regenerated; see the open report item.
const GOLDEN_BYTES_LEN_CONTRADICTS_CSHARP: &[&str] = &[
    "source-bytes-handed-to-fallback-when-request-has-none",
    "fallback-request-keeps-box-orientation-and-source-orientation",
];

// ---------------------------------------------------------------------------------------------------------------------
// Fixtures: stub images, scripted decoders, error construction.
// ---------------------------------------------------------------------------------------------------------------------

/// C# `static readonly SomeExif`: every stub reports this one object, so `exif()` identity is checkable.
fn shared_exif() -> &'static ExifSummary {
    static EXIF: OnceLock<ExifSummary> = OnceLock::new();
    EXIF.get_or_init(|| ExifSummary {
        camera_model: Some("TestCam".to_owned()),
        iso: Some(400),
        ..ExifSummary::default()
    })
}

/// C# `StubImage` / `W412bImage`: 640 + tag wide, 480 high, downscaled, orientation 6, 1 234 567 bytes, original
/// 6000x4000, degraded, shared exif, one platform object per stub (compared by `Arc` identity).
struct StubImage {
    backend: DecoderBackend,
    tag: i32,
    platform: PlatformImage,
}

impl StubImage {
    fn new(backend: DecoderBackend, tag: i32) -> Self {
        Self {
            backend,
            tag,
            platform: PlatformImage::Opaque(Arc::new(())),
        }
    }
}

impl DecodedImageView for StubImage {
    fn pixel_width(&self) -> i32 {
        640 + self.tag
    }
    fn pixel_height(&self) -> i32 {
        480
    }
    fn downscaled(&self) -> bool {
        true
    }
    fn orientation(&self) -> i32 {
        6
    }
    fn estimated_bytes(&self) -> i64 {
        1_234_567
    }
    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
    fn actual_backend(&self) -> DecoderBackend {
        self.backend
    }
    fn original_width(&self) -> i32 {
        6000
    }
    fn original_height(&self) -> i32 {
        4000
    }
    fn exif(&self) -> Option<&ExifSummary> {
        Some(shared_exif())
    }
    fn is_degraded_fallback(&self) -> bool {
        true
    }
}

fn stub(backend: DecoderBackend, tag: i32) -> Arc<dyn DecodedImageView> {
    Arc::new(StubImage::new(backend, tag))
}

/// Same platform object (`Arc` identity for the opaque form, the buffer identity for the pixel form).
fn same_platform(a: &PlatformImage, b: &PlatformImage) -> bool {
    match (a, b) {
        (PlatformImage::Opaque(x), PlatformImage::Opaque(y)) => Arc::ptr_eq(x, y),
        (PlatformImage::Pixels(x), PlatformImage::Pixels(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

type DecodeScript = Box<dyn Fn(&DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> + Send + Sync>;
type InfoScript = Box<dyn Fn(&str) -> Result<ImageInfo, DecodeError> + Send + Sync>;

/// C# scripted decoder: behaviour is a closure; counts calls, keeps the last request and path, and the image it last
/// returned (identity checks: `Arc::ptr_eq`).
struct ScriptedDecoder {
    decode_script: DecodeScript,
    info_script: InfoScript,
    decode_calls: AtomicUsize,
    info_calls: AtomicUsize,
    last_request: Mutex<Option<DecodeRequest>>,
    last_info_path: Mutex<Option<String>>,
    returned: Mutex<Option<Arc<dyn DecodedImageView>>>,
}

impl ScriptedDecoder {
    fn new(
        decode_script: impl Fn(&DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> + Send + Sync + 'static,
        info_script: impl Fn(&str) -> Result<ImageInfo, DecodeError> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            decode_script: Box::new(decode_script),
            info_script: Box::new(info_script),
            decode_calls: AtomicUsize::new(0),
            info_calls: AtomicUsize::new(0),
            last_request: Mutex::new(None),
            last_info_path: Mutex::new(None),
            returned: Mutex::new(None),
        })
    }

    fn decode_calls(&self) -> usize {
        self.decode_calls.load(Ordering::SeqCst)
    }

    fn info_calls(&self) -> usize {
        self.info_calls.load(Ordering::SeqCst)
    }

    fn last_request(&self) -> Option<DecodeRequest> {
        self.last_request.lock().unwrap().clone()
    }

    fn last_info_path(&self) -> Option<String> {
        self.last_info_path.lock().unwrap().clone()
    }

    fn returned(&self) -> Option<Arc<dyn DecodedImageView>> {
        self.returned.lock().unwrap().clone()
    }
}

impl ImageDecoder for ScriptedDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.decode_calls.fetch_add(1, Ordering::SeqCst);
        *self.last_request.lock().unwrap() = Some(request.clone());
        let result = (self.decode_script)(request);
        if let Ok(image) = &result {
            *self.returned.lock().unwrap() = Some(Arc::clone(image));
        }
        result
    }

    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        self.info_calls.fetch_add(1, Ordering::SeqCst);
        *self.last_info_path.lock().unwrap() = Some(path.to_owned());
        (self.info_script)(path)
    }
}

/// A read-info script for decoders whose read-info path the test does not exercise.
fn unscripted_info(_path: &str) -> Result<ImageInfo, DecodeError> {
    Err(DecodeError::InvalidOperation {
        message: "read_info is not scripted".to_owned(),
    })
}

/// A decoder that always succeeds with `image` (identity preserved: the same `Arc` every call).
fn returning(image: Arc<dyn DecodedImageView>) -> Arc<ScriptedDecoder> {
    ScriptedDecoder::new(move |_| Ok(Arc::clone(&image)), unscripted_info)
}

/// A decoder that always succeeds with a fresh `StubImage` for `backend`, and reads `ImageInfo(321 + tag, 123, 3)`.
fn always_ok(backend: DecoderBackend, tag: i32) -> Arc<ScriptedDecoder> {
    ScriptedDecoder::new(
        move |_| Ok(stub(backend, tag)),
        move |_| Ok(ImageInfo::with_orientation(321 + tag, 123, 3)),
    )
}

/// A decoder that always fails with a fresh error from `make`, on both paths.
fn always_fail<F>(make: F) -> Arc<ScriptedDecoder>
where
    F: Fn() -> DecodeError + Clone + Send + Sync + 'static,
{
    let for_info = make.clone();
    ScriptedDecoder::new(move |_| Err(make()), move |_| Err(for_info()))
}

/// Builds the error of a golden exception kind (the C# exception type name is mapped in [`variant_for_clr`]).
/// `Canceled` ignores `message` (C# `OperationCanceledException` has the fixed .NET text).
fn make_error(kind: &str, message: &str, hresult: i32) -> DecodeError {
    let message = message.to_owned();
    match kind {
        "NotSupported" | "PlatformNotSupported" => DecodeError::NotSupported {
            message,
            user_text: None,
        },
        "MissingCodec" => DecodeError::missing_codec_with(message),
        "InvalidCast" => DecodeError::InvalidCast { message },
        "Overflow" => DecodeError::Overflow { message },
        "Arithmetic" | "Generic" => DecodeError::Other { message },
        "Format" | "FileFormat" => DecodeError::Format { message },
        "InvalidData" => DecodeError::InvalidData { message },
        "COM" => DecodeError::Com { hresult, message },
        "DllNotFound" => DecodeError::DllNotFound { message },
        "BadImageFormat" => DecodeError::BadImageFormat { message },
        "EntryPointNotFound" => DecodeError::EntryPointNotFound { message },
        "FileNotFound" => DecodeError::FileNotFound { message },
        "DirectoryNotFound" => DecodeError::DirectoryNotFound { message },
        "OperationCanceled" | "TaskCanceled" => DecodeError::Canceled,
        "OutOfMemory" => DecodeError::OutOfMemory { message },
        "MemoryAdmission" => DecodeError::memory_admission_with(message),
        "Busy" => DecodeError::busy_with(message),
        "InvalidOperation" => DecodeError::InvalidOperation { message },
        "Argument" => DecodeError::InvalidArgument { message },
        // C# `new ObjectDisposedException(objectName)`: the argument is the object name, the .NET message wraps it.
        "ObjectDisposed" => DecodeError::Disposed {
            message: format!("Cannot access a disposed object.{}Object name: '{message}'.", "\r\n"),
        },
        "IO" | "FileLoad" | "UnauthorizedAccess" => DecodeError::Io(io::Error::other(message)),
        other => panic!("unknown exception kind {other}"),
    }
}

/// The `DecodeError` variant name (the Rust side of the golden's CLR type name).
fn variant_name(error: &DecodeError) -> &'static str {
    match error.kind() {
        DecodeError::Busy { .. } => "Busy",
        DecodeError::MemoryAdmission { .. } => "MemoryAdmission",
        DecodeError::MissingCodec { .. } => "MissingCodec",
        DecodeError::FileNotFound { .. } => "FileNotFound",
        DecodeError::DirectoryNotFound { .. } => "DirectoryNotFound",
        DecodeError::Canceled => "Canceled",
        DecodeError::OutOfMemory { .. } => "OutOfMemory",
        DecodeError::NotSupported { .. } => "NotSupported",
        DecodeError::InvalidCast { .. } => "InvalidCast",
        DecodeError::Overflow { .. } => "Overflow",
        DecodeError::Format { .. } => "Format",
        DecodeError::InvalidData { .. } => "InvalidData",
        DecodeError::Com { .. } => "Com",
        DecodeError::DllNotFound { .. } => "DllNotFound",
        DecodeError::BadImageFormat { .. } => "BadImageFormat",
        DecodeError::EntryPointNotFound { .. } => "EntryPointNotFound",
        DecodeError::InvalidOperation { .. } => "InvalidOperation",
        DecodeError::InvalidArgument { .. } => "InvalidArgument",
        DecodeError::Disposed { .. } => "Disposed",
        DecodeError::Io(_) => "Io",
        DecodeError::Other { .. } => "Other",
        DecodeError::WithSourceBytes { .. } => unreachable!("kind() peels the source-bytes wrapper"),
    }
}

/// C# CLR exception type name (golden `type`) to the `DecodeError` variant name.
fn variant_for_clr(clr: &str) -> &'static str {
    match clr {
        "NotSupportedException" | "PlatformNotSupportedException" => "NotSupported",
        "MissingImageCodecException" => "MissingCodec",
        "InvalidCastException" => "InvalidCast",
        "OverflowException" => "Overflow",
        "ArithmeticException" | "Exception" => "Other",
        "FormatException" => "Format",
        "InvalidDataException" => "InvalidData",
        "COMException" => "Com",
        "DllNotFoundException" => "DllNotFound",
        "BadImageFormatException" => "BadImageFormat",
        "EntryPointNotFoundException" => "EntryPointNotFound",
        "FileNotFoundException" => "FileNotFound",
        "DirectoryNotFoundException" => "DirectoryNotFound",
        "OperationCanceledException" | "TaskCanceledException" => "Canceled",
        "OutOfMemoryException" => "OutOfMemory",
        "DecoderMemoryAdmissionException" => "MemoryAdmission",
        "DecoderBusyException" => "Busy",
        "InvalidOperationException" => "InvalidOperation",
        "ArgumentException" => "InvalidArgument",
        "ObjectDisposedException" => "Disposed",
        "IOException" | "FileLoadException" | "UnauthorizedAccessException" => "Io",
        other => panic!("unknown CLR exception type {other}"),
    }
}

/// Golden backend name (C# `DecoderBackend.ToString()`) to the enum. The Rust variants carry the C# names.
fn backend_of(name: &str) -> DecoderBackend {
    match name {
        "Wpf" => DecoderBackend::Wpf,
        "WicDirect" => DecoderBackend::WicDirect,
        "TurboJpeg" => DecoderBackend::TurboJpeg,
        "LibRaw" => DecoderBackend::LibRaw,
        other => panic!("unknown backend {other}"),
    }
}

/// `DecoderBackend` name as the golden spells it (the Rust variants are named like the C# enum members).
fn backend_name(backend: DecoderBackend) -> String {
    format!("{backend:?}")
}

fn request(path: &str) -> DecodeRequest {
    DecodeRequest::new(path, 0)
}

/// The error of a result whose success side is not `Debug` (`Arc<dyn DecodedImageView>`); panics on success.
fn err_of<T>(result: Result<T, DecodeError>, why: &str) -> DecodeError {
    match result {
        Ok(_) => panic!("{why}: expected an error"),
        Err(error) => error,
    }
}

/// Decoder under test with its recording log and a real `ReviewMetrics`.
struct Harness {
    decoder: FallbackImageDecoder,
    log: Arc<RecordingLog>,
    metrics: Arc<ReviewMetrics>,
}

fn harness(primary: Arc<ScriptedDecoder>, primary_backend: DecoderBackend, fallback: Arc<ScriptedDecoder>) -> Harness {
    let log = Arc::new(RecordingLog::new());
    let metrics = Arc::new(ReviewMetrics::new());
    let decoder = FallbackImageDecoder::new(primary, primary_backend, fallback, log.clone(), Some(metrics.clone()));
    Harness { decoder, log, metrics }
}

fn warnings_of(log: &RecordingLog) -> Vec<String> {
    log.entries()
        .into_iter()
        .filter(|(level, _)| *level == LogLevel::Warn)
        .map(|(_, message)| message)
        .collect()
}

fn no_entries_at_all(log: &RecordingLog) -> bool {
    log.entries().is_empty()
}

/// Asserts `actual` is `expected` apart from its backend label: every property the C# `DecodedImageWithBackend` forwards.
fn assert_same_apart_from_backend(expected: &dyn DecodedImageView, actual: &dyn DecodedImageView) {
    assert_eq!(actual.pixel_width(), expected.pixel_width(), "pixel width");
    assert_eq!(actual.pixel_height(), expected.pixel_height(), "pixel height");
    assert_eq!(actual.downscaled(), expected.downscaled(), "downscaled");
    assert_eq!(actual.orientation(), expected.orientation(), "orientation");
    assert_eq!(actual.estimated_bytes(), expected.estimated_bytes(), "estimated bytes");
    assert!(
        same_platform(actual.platform_image(), expected.platform_image()),
        "platform image must be the inner image's object"
    );
    assert_eq!(actual.original_width(), expected.original_width(), "original width");
    assert_eq!(actual.original_height(), expected.original_height(), "original height");
    assert_eq!(
        actual.is_degraded_fallback(),
        expected.is_degraded_fallback(),
        "degraded fallback"
    );
    assert!(
        actual.exif().is_some_and(|e| std::ptr::eq(e, shared_exif())),
        "exif must be the inner image's object"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven tests.
// ---------------------------------------------------------------------------------------------------------------------

fn load_inputs(name: &str) -> Vec<Value> {
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

fn tag_of(behaviour: &Value) -> i32 {
    behaviour
        .get("tag")
        .and_then(Value::as_i64)
        .map_or(0, |t| i32::try_from(t).expect("tag fits i32"))
}

/// The error a golden `throw` behaviour describes.
fn failure_of(behaviour: &Value) -> DecodeError {
    let hresult = behaviour
        .get("hresult")
        .and_then(Value::as_i64)
        .map_or(COM_HRESULT, |h| i32::try_from(h).expect("hresult fits i32"));
    make_error(str_of(behaviour, "exception"), str_of(behaviour, "message"), hresult)
}

/// The decode half of a golden scripted behaviour (`attach_bytes` attaches a source buffer of that length).
fn decode_behaviour(behaviour: &Value) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
    if behaviour["kind"].as_str() == Some("ok") {
        let backend = backend_of(str_of(behaviour, "backend"));
        return Ok(stub(backend, tag_of(behaviour)));
    }
    let mut error = failure_of(behaviour);
    if let Some(len) = behaviour.get("attach_bytes").and_then(Value::as_u64) {
        let bytes: SourceBytes = Arc::from(vec![0u8; usize::try_from(len).expect("len fits usize")]);
        error = error.attach_source_bytes(bytes);
    }
    Err(error)
}

/// The read-info half of a golden scripted behaviour (read-info has no source-bytes concept).
fn info_behaviour(behaviour: &Value) -> Result<ImageInfo, DecodeError> {
    if behaviour["kind"].as_str() == Some("ok") {
        return Ok(ImageInfo::with_orientation(321 + tag_of(behaviour), 123, 3));
    }
    Err(failure_of(behaviour))
}

fn scripted_from(behaviour: &Value) -> Arc<ScriptedDecoder> {
    let for_decode = behaviour.clone();
    let for_info = behaviour.clone();
    ScriptedDecoder::new(
        move |_request| decode_behaviour(&for_decode),
        move |_path| info_behaviour(&for_info),
    )
}

fn build_request(r: &Value) -> DecodeRequest {
    let target_width = r.get("target_width").and_then(Value::as_i64).unwrap_or(0);
    let target_height = r.get("target_height").and_then(Value::as_i64).unwrap_or(0);
    let mut built = DecodeRequest::new(str_of(r, "path"), i32::try_from(target_width).expect("width fits i32"))
        .with_target_height(i32::try_from(target_height).expect("height fits i32"))
        .with_apply_orientation(r.get("apply_orientation").and_then(Value::as_bool).unwrap_or(true));
    if let Some(len) = r.get("bytes_len").and_then(Value::as_u64) {
        built = built.with_bytes(Arc::from(vec![0u8; usize::try_from(len).expect("len fits usize")]));
    }
    if let Some(orientation) = r.get("source_orientation").and_then(Value::as_i64) {
        built = built.with_source_orientation(i32::try_from(orientation).expect("orientation fits i32"));
    }
    built
}

fn count_of(case: &Value, key: &str) -> usize {
    usize::try_from(i64_of(case, key)).expect("count fits usize")
}

fn check_decode_outcome(
    name: &str,
    input: &Value,
    outcome: &Value,
    result: Result<Arc<dyn DecodedImageView>, DecodeError>,
    primary: &ScriptedDecoder,
    fallback: &ScriptedDecoder,
) {
    match result {
        Ok(image) => {
            assert_eq!(outcome["kind"].as_str(), Some("ok"), "{name}: outcome kind");
            assert_eq!(
                backend_name(image.actual_backend()),
                str_of(outcome, "backend"),
                "{name}: actual backend"
            );
            let primary_image = primary.returned();
            let fallback_image = fallback.returned();
            assert_eq!(
                primary_image.as_ref().is_some_and(|p| Arc::ptr_eq(&image, p)),
                bool_of(outcome, "same_as_primary_image"),
                "{name}: identity with the primary image"
            );
            assert_eq!(
                fallback_image.as_ref().is_some_and(|f| Arc::ptr_eq(&image, f)),
                bool_of(outcome, "same_as_fallback_image"),
                "{name}: identity with the fallback image"
            );
            assert_eq!(
                i64::from(image.pixel_width()),
                i64_of(outcome, "pixel_width"),
                "{name}: pixel width"
            );
            assert_eq!(
                i64::from(image.pixel_height()),
                i64_of(outcome, "pixel_height"),
                "{name}: pixel height"
            );
            assert_eq!(image.downscaled(), bool_of(outcome, "downscaled"), "{name}: downscaled");
            assert_eq!(
                i64::from(image.orientation()),
                i64_of(outcome, "orientation"),
                "{name}: orientation"
            );
            assert_eq!(
                image.estimated_bytes(),
                i64_of(outcome, "estimated_bytes"),
                "{name}: estimated bytes"
            );
            assert_eq!(
                i64::from(image.original_width()),
                i64_of(outcome, "original_width"),
                "{name}: original width"
            );
            assert_eq!(
                i64::from(image.original_height()),
                i64_of(outcome, "original_height"),
                "{name}: original height"
            );
            assert_eq!(
                image.is_degraded_fallback(),
                bool_of(outcome, "is_degraded_fallback"),
                "{name}: degraded fallback"
            );
            assert_eq!(
                image.exif().is_some_and(|e| std::ptr::eq(e, shared_exif())),
                bool_of(outcome, "exif_is_same_object"),
                "{name}: exif identity"
            );
            let source = primary_image
                .or(fallback_image)
                .expect("an ok outcome has a returned image");
            assert_eq!(
                same_platform(image.platform_image(), source.platform_image()),
                bool_of(outcome, "platform_image_same"),
                "{name}: platform image identity"
            );
        }
        Err(error) => {
            assert_eq!(outcome["kind"].as_str(), Some("throw"), "{name}: outcome kind");
            let clr = str_of(outcome, "type");
            assert_eq!(variant_name(&error), variant_for_clr(clr), "{name}: error variant");
            if variant_name(&error) != "Canceled" {
                assert_eq!(error.message(), str_of(outcome, "message"), "{name}: error message");
            }
            // The golden's identity flags, translated: the error came from the decoder whose behaviour is a `throw`.
            if bool_of(outcome, "same_as_primary_exception") {
                assert_eq!(
                    input["primary"]["kind"].as_str(),
                    Some("throw"),
                    "{name}: primary threw"
                );
            }
            if bool_of(outcome, "same_as_fallback_exception") {
                assert_eq!(
                    input["fallback"]["kind"].as_str(),
                    Some("throw"),
                    "{name}: fallback threw"
                );
            }
        }
    }
}

fn check_info_outcome(name: &str, outcome: &Value, result: Result<ImageInfo, DecodeError>) {
    match result {
        Ok(info) => {
            assert_eq!(outcome["kind"].as_str(), Some("ok"), "{name}: outcome kind");
            let want: Vec<i64> = outcome["info"]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: info"))
                .iter()
                .map(|v| v.as_i64().expect("info value"))
                .collect();
            assert_eq!(
                vec![
                    i64::from(info.pixel_width),
                    i64::from(info.pixel_height),
                    i64::from(info.orientation),
                ],
                want,
                "{name}: image info"
            );
        }
        Err(error) => {
            assert_eq!(outcome["kind"].as_str(), Some("throw"), "{name}: outcome kind");
            assert_eq!(
                variant_name(&error),
                variant_for_clr(str_of(outcome, "type")),
                "{name}: error variant"
            );
            if variant_name(&error) != "Canceled" {
                assert_eq!(error.message(), str_of(outcome, "message"), "{name}: error message");
            }
        }
    }
}

fn check_fallback_request(name: &str, input: &Value, expected: &Value, fallback: &ScriptedDecoder) {
    let Some(want) = expected.get("fallback_request").filter(|v| !v.is_null()) else {
        assert!(
            fallback.last_request().is_none(),
            "{name}: the fallback must not be called"
        );
        return;
    };
    let got = fallback
        .last_request()
        .unwrap_or_else(|| panic!("{name}: the fallback was not called"));
    assert_eq!(got.path, PathBuf::from(str_of(want, "path")), "{name}: fallback path");
    assert_eq!(
        i64::from(got.target_width),
        i64_of(want, "target_width"),
        "{name}: fallback target width"
    );
    assert_eq!(
        i64::from(got.target_height),
        i64_of(want, "target_height"),
        "{name}: fallback target height"
    );
    assert_eq!(
        got.apply_orientation,
        bool_of(want, "apply_orientation"),
        "{name}: fallback apply orientation"
    );
    assert_eq!(
        got.source_orientation.map(i64::from),
        want["source_orientation"].as_i64(),
        "{name}: fallback source orientation"
    );
    let got_len = got
        .bytes
        .as_ref()
        .map_or(0, |b| i64::try_from(b.len()).expect("len fits i64"));
    let want_len = if GOLDEN_BYTES_LEN_CONTRADICTS_CSHARP.contains(&name) {
        input["primary"]["attach_bytes"]
            .as_i64()
            .unwrap_or_else(|| panic!("{name}: attached length"))
    } else {
        i64_of(want, "bytes_len")
    };
    assert_eq!(got_len, want_len, "{name}: fallback request bytes length");
}

/// One case of `tests/golden/fallback-decoder.json`, driven by its input in `tests/golden-inputs/fallback-decoder.json`.
fn check_decoder_case(expected: &Value, input: &Value) {
    let name = str_of(expected, "name");
    assert_eq!(
        name,
        str_of(input, "name"),
        "golden and input cases are in the same order"
    );
    let op = str_of(input, "op");
    let outcome = &expected["outcome"];

    let primary = scripted_from(&input["primary"]);
    let fallback = scripted_from(&input["fallback"]);
    let h = harness(
        primary.clone(),
        backend_of(str_of(input, "primary_backend")),
        fallback.clone(),
    );
    let req = build_request(&input["request"]);

    match op {
        "decode" => {
            let result = h.decoder.decode(&req);
            check_decode_outcome(name, input, outcome, result, &primary, &fallback);
        }
        "read-info" => {
            let result = h.decoder.read_info(str_of(&input["request"], "path"));
            check_info_outcome(name, outcome, result);
        }
        other => panic!("{name}: unknown op {other}"),
    }

    assert_eq!(
        primary.decode_calls(),
        count_of(expected, "primary_decode_calls"),
        "{name}: primary decode calls"
    );
    assert_eq!(
        primary.info_calls(),
        count_of(expected, "primary_read_info_calls"),
        "{name}: primary read-info calls"
    );
    assert_eq!(
        fallback.decode_calls(),
        count_of(expected, "fallback_decode_calls"),
        "{name}: fallback decode calls"
    );
    assert_eq!(
        fallback.info_calls(),
        count_of(expected, "fallback_read_info_calls"),
        "{name}: fallback read-info calls"
    );
    assert_eq!(
        fallback.last_info_path().as_deref(),
        expected["fallback_info_path"].as_str(),
        "{name}: fallback read-info path"
    );
    check_fallback_request(name, input, expected, &fallback);

    let entries = h.log.entries();
    assert_eq!(
        entries.iter().filter(|(level, _)| *level != LogLevel::Warn).count(),
        0,
        "{name}: only warnings are logged"
    );
    let want_warnings: Vec<String> = expected["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: warnings"))
        .iter()
        .map(|w| w.as_str().expect("warning text").to_owned())
        .collect();
    assert_eq!(warnings_of(&h.log), want_warnings, "{name}: warnings");

    let snapshot = h.metrics.snapshot();
    let mut got: Vec<(String, i64)> = snapshot
        .decoder_fallbacks
        .iter()
        .map(|(backend, n)| (backend_name(*backend), *n))
        .collect();
    got.sort();
    let mut want: Vec<(String, i64)> = expected["decoder_fallbacks"]
        .as_object()
        .unwrap_or_else(|| panic!("{name}: decoder_fallbacks"))
        .iter()
        .map(|(k, v)| (k.clone(), v.as_i64().expect("count")))
        .collect();
    want.sort();
    assert_eq!(got, want, "{name}: decoder fallbacks");
    assert_eq!(
        snapshot.decoder_fallback_count(),
        i64_of(expected, "decoder_fallback_count"),
        "{name}: decoder fallback count"
    );
}

// C#: FallbackExceptionCoverageTests / DecoderContractRegressionTests / FallbackImageDecoder golden `fallback-decoder`
#[test]
fn golden_fallback_decoder() {
    let expected = golden::load("fallback-decoder").cases;
    let inputs = load_inputs("fallback-decoder");
    assert_eq!(expected.len(), inputs.len(), "golden and input case counts");
    assert_eq!(expected.len(), 55, "the fallback-decoder golden has 55 cases");
    for (case, input) in expected.iter().zip(&inputs) {
        check_decoder_case(case, input);
    }
}

// Golden `fallback-is-fallbackable`: one exception per kind, the CLR type name and `IsFallbackable`.
#[test]
fn golden_fallback_is_fallbackable() {
    let expected = golden::load("fallback-is-fallbackable").cases;
    let inputs = load_inputs("fallback-is-fallbackable");
    assert_eq!(expected.len(), inputs.len(), "golden and input case counts");
    assert_eq!(expected.len(), 26, "the fallback-is-fallbackable golden has 26 cases");
    for (case, input) in expected.iter().zip(&inputs) {
        let name = str_of(case, "name");
        assert_eq!(
            name,
            str_of(input, "name"),
            "golden and input cases are in the same order"
        );
        let message = input
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("scripted failure");
        let error = make_error(str_of(input, "exception"), message, COM_HRESULT);
        assert_eq!(
            variant_name(&error),
            variant_for_clr(str_of(case, "type")),
            "{name}: variant of the CLR type"
        );
        assert_eq!(
            FallbackImageDecoder::is_fallbackable(&error),
            bool_of(case, "fallbackable"),
            "{name}: IsFallbackable"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// FallbackBackendLabelTests.
// ---------------------------------------------------------------------------------------------------------------------

// C#: FallbackBackendLabelTests.Decode_PrimaryReportsAnotherBackend_IsRelabelledAndKeepsEveryProperty
#[test]
fn decode_primary_reports_another_backend_is_relabelled_and_keeps_every_property() {
    let inner = stub(DecoderBackend::Wpf, 0);
    let h = harness(
        returning(Arc::clone(&inner)),
        DecoderBackend::TurboJpeg,
        always_fail(|| make_error("InvalidOperation", "must not run", 0)),
    );

    let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("primary succeeds");

    assert_eq!(decoded.actual_backend(), DecoderBackend::TurboJpeg);
    assert!(!Arc::ptr_eq(&decoded, &inner), "a relabelled image is a new object");
    assert_same_apart_from_backend(&*inner, &*decoded);
}

// C#: FallbackBackendLabelTests.Decode_PrimaryReportsTheRequestedBackend_ReturnsTheSameInstance
#[test]
fn decode_primary_reports_the_requested_backend_returns_the_same_instance() {
    let inner = stub(DecoderBackend::TurboJpeg, 0);
    let h = harness(
        returning(Arc::clone(&inner)),
        DecoderBackend::TurboJpeg,
        always_fail(|| make_error("InvalidOperation", "must not run", 0)),
    );

    let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("primary succeeds");

    assert!(Arc::ptr_eq(&decoded, &inner), "the primary's image is returned as is");
}

// C#: FallbackBackendLabelTests.Decode_FallbackReportsAnotherBackend_IsRelabelledAsTheFallbackBackend
#[test]
fn decode_fallback_reports_another_backend_is_relabelled_as_the_fallback_backend() {
    let inner = stub(DecoderBackend::LibRaw, 0);
    let h = harness(
        always_fail(|| make_error("NotSupported", "primary cannot", 0)),
        DecoderBackend::TurboJpeg,
        returning(Arc::clone(&inner)),
    );

    let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("fallback succeeds");

    assert_eq!(decoded.actual_backend(), FALLBACK_BACKEND);
    assert!(!Arc::ptr_eq(&decoded, &inner), "a relabelled image is a new object");
    assert_same_apart_from_backend(&*inner, &*decoded);
}

// C#: FallbackBackendLabelTests.Decode_FallbackReportsTheFallbackBackend_ReturnsTheSameInstance
#[test]
fn decode_fallback_reports_the_fallback_backend_returns_the_same_instance() {
    let inner = stub(FALLBACK_BACKEND, 0);
    let h = harness(
        always_fail(|| make_error("NotSupported", "primary cannot", 0)),
        DecoderBackend::TurboJpeg,
        returning(Arc::clone(&inner)),
    );

    let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("fallback succeeds");

    assert!(
        Arc::ptr_eq(&decoded, &inner),
        "the fallback's Wpf image is returned as is"
    );
}

// C#: FallbackBackendLabelTests.ReadInfo_PrimaryFailsFallbackable_ReturnsTheFallbackInfo
#[test]
fn read_info_primary_fails_fallbackable_returns_the_fallback_info() {
    let primary = ScriptedDecoder::new(
        |_| Err(make_error("InvalidOperation", "must not run", 0)),
        |_| Err(make_error("NotSupported", "primary cannot", 0)),
    );
    let fallback = ScriptedDecoder::new(
        |_| Err(make_error("InvalidOperation", "must not run", 0)),
        |_| Ok(ImageInfo::with_orientation(321, 123, 3)),
    );
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback);

    let info = h.decoder.read_info(r"C:\a.jpg").expect("fallback answers");

    assert_eq!(info, ImageInfo::with_orientation(321, 123, 3));
}

// C#: FallbackBackendLabelTests.Constructor_NullDecoders_Throw has NO twin: `Arc<dyn ImageDecoder>` cannot be null.

// The fallback is a plain decoder: C# `FallbackImageDecoder` is not an `ICancellableImageDecoder` (contract open question 1).
#[test]
fn fallback_decoder_is_not_cancellable_like_the_csharp_wrapper() {
    let h = harness(
        always_ok(DecoderBackend::TurboJpeg, 0),
        DecoderBackend::TurboJpeg,
        always_ok(FALLBACK_BACKEND, 0),
    );

    assert!(h.decoder.as_cancellable().is_none());
}

// ---------------------------------------------------------------------------------------------------------------------
// FactoryTests (the decoder-level half; the factory itself is W4-12a).
// ---------------------------------------------------------------------------------------------------------------------

// C#: FactoryTests.PrimarySucceedsReturnsPrimaryBackend
#[test]
fn primary_succeeds_returns_primary_backend() {
    let scripted = always_ok(DecoderBackend::WicDirect, 0);
    let h = harness(scripted.clone(), DecoderBackend::WicDirect, scripted.clone());

    let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("primary succeeds");

    assert_eq!(decoded.actual_backend(), DecoderBackend::WicDirect);
    assert!(
        h.metrics
            .snapshot()
            .decoder_fallbacks
            .iter()
            .all(|(backend, _)| *backend != DecoderBackend::WicDirect),
        "no fallback is counted for WicDirect"
    );
}

// C#: FactoryTests.FallbackOccursOnSupportedExceptions (5 theory cases of the C# test)
#[test]
fn fallback_occurs_on_supported_exceptions() {
    let cases: [(&str, &str); 5] = [
        ("NotSupported", "not supported"),
        ("FileFormat", "bad file format"),
        ("InvalidData", "bad data"),
        ("COM", "com failure"),
        ("DllNotFound", "no dll"),
    ];
    for (kind, message) in cases {
        let primary = always_fail(move || make_error(kind, message, COM_HRESULT));
        let fallback = always_ok(FALLBACK_BACKEND, 0);
        let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

        let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("fallback succeeds");

        assert_eq!(decoded.actual_backend(), FALLBACK_BACKEND, "{kind}: backend");
        assert_eq!(fallback.decode_calls(), 1, "{kind}: fallback decodes once");
        assert_eq!(
            h.metrics
                .snapshot()
                .decoder_fallbacks
                .iter()
                .find(|(backend, _)| *backend == DecoderBackend::TurboJpeg)
                .map(|(_, n)| *n),
            Some(1),
            "{kind}: TurboJpeg fallback counted once"
        );
    }
}

// C#: FactoryTests.MissingFileDoesNotFallback
#[test]
fn missing_file_does_not_fallback() {
    let primary = always_fail(|| make_error("FileNotFound", "gone", 0));
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let error = err_of(
        h.decoder.decode(&request(r"C:\missing.jpg")),
        "a missing file is not repaired by the fallback",
    );

    assert_eq!(variant_name(&error), "FileNotFound");
    assert_eq!(fallback.decode_calls(), 0, "the fallback is never called");
    assert!(no_entries_at_all(&h.log), "no warning for a missing file");
    assert_eq!(h.metrics.snapshot().decoder_fallback_count(), 0);
}

// C#: FactoryTests.ReadInfoFallsBack
#[test]
fn read_info_falls_back() {
    let primary = always_fail(|| make_error("NotSupported", "primary cannot", 0));
    let fallback = always_ok(FALLBACK_BACKEND, 2);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let info = h.decoder.read_info(r"C:\a.jpg").expect("fallback answers");

    assert_eq!(info, ImageInfo::with_orientation(323, 123, 3));
    assert_eq!(fallback.info_calls(), 1);
    assert_eq!(fallback.last_info_path().as_deref(), Some(r"C:\a.jpg"));
    assert_eq!(h.metrics.snapshot().decoder_fallback_count(), 1);
}

// ---------------------------------------------------------------------------------------------------------------------
// DecoderBusyFallbackTests (INV-12: a busy decoder never falls back and is never counted).
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecoderBusyFallbackTests.Decode_PrimaryThrowsDecoderBusy_PropagatesWithoutUsingTheFallbackOrCountingAFallback
#[test]
fn decode_primary_throws_decoder_busy_propagates_without_using_the_fallback_or_counting_a_fallback() {
    let primary = always_fail(|| make_error("Busy", "queue full", 0));
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let error = err_of(h.decoder.decode(&request(r"C:\a.jpg")), "busy propagates");

    assert_eq!(variant_name(&error), "Busy");
    assert_eq!(error.message(), "queue full");
    assert_eq!(fallback.decode_calls(), 0, "the fallback is never called");
    assert!(warnings_of(&h.log).is_empty(), "no warning for a busy decoder");
    assert_eq!(h.metrics.snapshot().decoder_fallback_count(), 0);
}

// C#: DecoderBusyFallbackTests.ReadInfo_PrimaryThrowsDecoderBusy_PropagatesWithoutUsingTheFallback
#[test]
fn read_info_primary_throws_decoder_busy_propagates_without_using_the_fallback() {
    let primary = always_fail(|| make_error("Busy", "queue full", 0));
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let error = err_of(h.decoder.read_info(r"C:\a.jpg"), "busy propagates");

    assert_eq!(variant_name(&error), "Busy");
    assert_eq!(fallback.info_calls(), 0, "the fallback is never called");
    assert!(warnings_of(&h.log).is_empty());
    assert_eq!(h.metrics.snapshot().decoder_fallback_count(), 0);
}

// C#: DecoderBusyFallbackTests.IsFallbackable_DecoderBusy_IsFalse
#[test]
fn is_fallbackable_decoder_busy_is_false() {
    assert!(!FallbackImageDecoder::is_fallbackable(&DecodeError::busy()));
    assert!(!FallbackImageDecoder::is_fallbackable(&DecodeError::busy_with(
        "queue full"
    )));
}

// ---------------------------------------------------------------------------------------------------------------------
// DecoderAdmissionFallbackTests (the admission refusal never falls back; the corrupt-image fallback still does).
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecoderAdmissionFallbackTests.IsFallbackable_AdmissionRefusal_IsFalse
#[test]
fn is_fallbackable_admission_refusal_is_false() {
    assert!(!FallbackImageDecoder::is_fallbackable(&DecodeError::memory_admission()));
    assert!(!FallbackImageDecoder::is_fallbackable(
        &DecodeError::memory_admission_with("too large for the available memory")
    ));
}

// C#: DecoderAdmissionFallbackTests.Decode_CorruptImageInvalidData_StillFallsBack
#[test]
fn decode_corrupt_image_invalid_data_still_falls_back() {
    let primary = always_fail(|| make_error("InvalidData", "corrupt", 0));
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("fallback succeeds");

    assert_eq!(decoded.actual_backend(), FALLBACK_BACKEND);
    assert_eq!(fallback.decode_calls(), 1, "the fallback is reached exactly once");
}

// Scripted twin of the two C# `Decode_*OutputAdmissionRefusal_PropagatesWithoutUsingTheFallback` tests (the real
// `EnsureOutputFits` / TurboJpeg versions are W4-11c / W5).
#[test]
fn decode_memory_admission_refusal_propagates_without_fallback_or_metric() {
    let primary = always_fail(|| make_error("MemoryAdmission", "too large for the available memory", 0));
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::WicDirect, fallback.clone());

    let error = err_of(
        h.decoder.decode(&request(r"C:\big.jpg")),
        "an admission refusal propagates",
    );

    assert_eq!(variant_name(&error), "MemoryAdmission");
    assert_eq!(error.message(), "too large for the available memory");
    assert_eq!(fallback.decode_calls(), 0, "the fallback is never called");
    assert_eq!(h.metrics.snapshot().decoder_fallback_count(), 0);
}

// ---------------------------------------------------------------------------------------------------------------------
// DecoderContractRegressionTests.
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecoderContractRegressionTests.TurboHeaderFailureAttemptsWpfFallbackExactlyOnceAndPreservesFinalException
// (`Assert.Same` becomes message + variant equality: `DecodeError` is not `Clone`.)
#[test]
fn turbo_header_failure_attempts_wpf_fallback_exactly_once_and_preserves_final_exception() {
    let primary = always_fail(|| make_error("Format", "header is corrupt", 0));
    let fallback = always_fail(|| make_error("Format", "sentinel: fallback failed", 0));
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let error = err_of(
        h.decoder.decode(&request(r"C:\a.jpg")),
        "the fallback's failure is returned",
    );

    assert_eq!(variant_name(&error), "Format");
    assert_eq!(
        error.message(),
        "sentinel: fallback failed",
        "the final exception is the fallback's"
    );
    assert_eq!(fallback.decode_calls(), 1, "the fallback runs exactly once");
}

// C#: DecoderContractRegressionTests.TurboScanFailureAttemptsWpfFallbackExactlyOnce
#[test]
fn turbo_scan_failure_attempts_wpf_fallback_exactly_once() {
    let primary = always_fail(|| make_error("InvalidData", "scan data is corrupt", 0));
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("fallback succeeds");

    assert_eq!(decoded.actual_backend(), FALLBACK_BACKEND);
    assert_eq!(fallback.decode_calls(), 1, "the fallback runs exactly once");
}

// C#: DecoderContractRegressionTests.NonFallbackableFailuresAreNotRetried (4 theory cases of the C# test)
#[test]
fn non_fallbackable_failures_are_not_retried() {
    let cases: [(&str, &str); 4] = [
        ("FileNotFound", "FileNotFound"),
        ("OperationCanceled", "Canceled"),
        ("OutOfMemory", "OutOfMemory"),
        ("InvalidOperation", "InvalidOperation"),
    ];
    for (kind, variant) in cases {
        let primary = always_fail(move || make_error(kind, "refused", 0));
        let fallback = always_ok(FALLBACK_BACKEND, 0);
        let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

        let error = err_of(
            h.decoder.decode(&request(r"C:\a.jpg")),
            "a non-fallbackable failure propagates",
        );

        assert_eq!(variant_name(&error), variant, "{kind}: variant");
        assert_eq!(fallback.decode_calls(), 0, "{kind}: the fallback is never called");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// DecodingHelpersMutationTests: FallbackDecoder identity and logging (lines 400-485).
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecodingHelpersMutationTests.FallbackDecoder_FallbackImageAlreadyOnTheFallbackBackend_IsReturnedAsIs
#[test]
fn fallback_decoder_fallback_image_already_on_the_fallback_backend_is_returned_as_is() {
    let wpf_image = stub(FALLBACK_BACKEND, 0);
    let h = harness(
        always_fail(|| make_error("NotSupported", "primary cannot", 0)),
        DecoderBackend::TurboJpeg,
        returning(Arc::clone(&wpf_image)),
    );

    let result = h.decoder.decode(&request("x.jpg")).expect("fallback succeeds");

    assert!(Arc::ptr_eq(&result, &wpf_image));
}

// C#: DecodingHelpersMutationTests.FallbackDecoder_FallbackImageReportingAnotherBackend_IsRelabelledAsTheFallbackBackend
#[test]
fn fallback_decoder_fallback_image_reporting_another_backend_is_relabelled_as_the_fallback_backend() {
    let h = harness(
        always_fail(|| make_error("NotSupported", "primary cannot", 0)),
        DecoderBackend::TurboJpeg,
        returning(stub(DecoderBackend::WicDirect, 0)),
    );

    let result = h.decoder.decode(&request("x.jpg")).expect("fallback succeeds");

    assert_eq!(result.actual_backend(), FALLBACK_BACKEND);
}

// C#: DecodingHelpersMutationTests.FallbackDecoder_PrimaryImageOnAnotherBackend_IsRelabelledAsThePrimaryBackend
#[test]
fn fallback_decoder_primary_image_on_another_backend_is_relabelled_as_the_primary_backend() {
    let h = harness(
        returning(stub(FALLBACK_BACKEND, 0)),
        DecoderBackend::TurboJpeg,
        always_fail(|| make_error("InvalidOperation", "must not be used", 0)),
    );

    let result = h.decoder.decode(&request("x.jpg")).expect("primary succeeds");

    assert_eq!(result.actual_backend(), DecoderBackend::TurboJpeg);
}

// C#: DecodingHelpersMutationTests.FallbackDecoder_WarnsThroughTheInjectedLogWhenItFallsBack
#[test]
fn fallback_decoder_warns_through_the_injected_log_when_it_falls_back() {
    let h = harness(
        always_fail(|| make_error("NotSupported", "primary cannot", 0)),
        DecoderBackend::TurboJpeg,
        returning(stub(FALLBACK_BACKEND, 0)),
    );

    let _ = h.decoder.decode(&request("x.jpg")).expect("fallback succeeds");

    let warnings = warnings_of(&h.log);
    assert_eq!(warnings.len(), 1, "exactly one warning");
    assert!(
        warnings[0].contains("x.jpg"),
        "the warning names the file: {}",
        warnings[0]
    );
    assert_eq!(h.log.entries().len(), 1, "no other log entry");
}

// ---------------------------------------------------------------------------------------------------------------------
// Wp06WpfFreeExceptionMappingTests: the two `IsFallbackable` lines only (the cache-entry half is W4-10).
// ---------------------------------------------------------------------------------------------------------------------

// C#: Wp06WpfFreeExceptionMappingTests.FormatException_IsFallbackable_AndACacheEntryFailure (IsFallbackable part)
#[test]
fn format_exception_is_fallbackable() {
    // `FormatException` and WPF's `FileFormatException` both map to `DecodeError::Format`.
    assert!(FallbackImageDecoder::is_fallbackable(&make_error("Format", "bad", 0)));
    assert!(FallbackImageDecoder::is_fallbackable(&make_error(
        "FileFormat",
        "bad",
        0
    )));
}

// ---------------------------------------------------------------------------------------------------------------------
// FallbackExceptionCoverageTests (the scripted fallback replaces the real WPF decoder and the generated ICC JPEG).
// ---------------------------------------------------------------------------------------------------------------------

// C#: FallbackExceptionCoverageTests.FallbackableTypes_UseTheFallback (3 cases, decode and read_info)
#[test]
fn fallbackable_types_use_the_fallback() {
    let cases: [(&str, &str); 3] = [
        ("InvalidCast", "bad cast"),
        ("Overflow", "too big"),
        ("NotSupported", "unsupported"),
    ];
    for (kind, message) in cases {
        let primary = always_fail(move || make_error(kind, message, 0));
        let fallback = always_ok(FALLBACK_BACKEND, 0);
        let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

        let decoded = h.decoder.decode(&request(r"C:\a.jpg")).expect("decode falls back");
        assert_eq!(decoded.actual_backend(), FALLBACK_BACKEND, "{kind}: decode backend");

        let info = h.decoder.read_info(r"C:\a.jpg").expect("read_info falls back");
        assert_eq!(
            info,
            ImageInfo::with_orientation(321, 123, 3),
            "{kind}: read_info answer"
        );
        assert_eq!(fallback.decode_calls(), 1, "{kind}: fallback decode once");
        assert_eq!(fallback.info_calls(), 1, "{kind}: fallback read_info once");
    }
}

// C#: FallbackExceptionCoverageTests.NonFallbackableTypes_Propagate (4 cases, decode and read_info)
#[test]
fn non_fallbackable_types_propagate() {
    let cases: [(&str, &str); 4] = [
        ("FileNotFound", "FileNotFound"),
        ("DirectoryNotFound", "DirectoryNotFound"),
        ("OperationCanceled", "Canceled"),
        ("OutOfMemory", "OutOfMemory"),
    ];
    for (kind, variant) in cases {
        let primary = always_fail(move || make_error(kind, "refused", 0));
        let fallback = always_ok(FALLBACK_BACKEND, 0);
        let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

        let decode_error = err_of(h.decoder.decode(&request(r"C:\a.jpg")), "decode propagates");
        assert_eq!(variant_name(&decode_error), variant, "{kind}: decode variant");

        let info_error = err_of(h.decoder.read_info(r"C:\a.jpg"), "read_info propagates");
        assert_eq!(variant_name(&info_error), variant, "{kind}: read_info variant");

        assert_eq!(fallback.decode_calls(), 0, "{kind}: no fallback decode");
        assert_eq!(fallback.info_calls(), 0, "{kind}: no fallback read_info");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// FallbackSourceBytesTests (`fallbackable_failure_bytes_are_handed_to_the_fallback` is the scripted twin of the C#
// `IccJpeg_FallbackDecodesFromBytesTurboJpegAlreadyRead`; the real TurboJpeg part is W5).
// ---------------------------------------------------------------------------------------------------------------------

// C#: FallbackSourceBytesTests.CallerBytes_ReachFallbackUnchanged
#[test]
fn caller_bytes_reach_fallback_unchanged() {
    let caller: SourceBytes = Arc::from(&b"caller bytes"[..]);
    let primary = ScriptedDecoder::new(
        |_| Err(make_error("NotSupported", "primary cannot", 0).attach_source_bytes(Arc::from(vec![9u8; 4]))),
        unscripted_info,
    );
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let _ = h
        .decoder
        .decode(&request(r"C:\a.jpg").with_bytes(Arc::clone(&caller)))
        .expect("fallback succeeds");

    let seen = fallback.last_request().expect("fallback called");
    let seen_bytes = seen.bytes.expect("the caller's bytes are kept");
    assert!(
        Arc::ptr_eq(&seen_bytes, &caller),
        "the fallback sees the caller's buffer, not the attached copy"
    );
}

// C#: FallbackSourceBytesTests.IccJpeg_FallbackDecodesFromBytesTurboJpegAlreadyRead (scripted twin, named by the contract)
#[test]
fn fallbackable_failure_bytes_are_handed_to_the_fallback() {
    let attached: SourceBytes = Arc::from(vec![1u8, 2, 3, 4, 5, 6, 7, 8]);
    let to_attach = Arc::clone(&attached);
    let primary = ScriptedDecoder::new(
        move |_| Err(make_error("InvalidData", "bad scan", 0).attach_source_bytes(Arc::clone(&to_attach))),
        unscripted_info,
    );
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let _ = h.decoder.decode(&request(r"C:\a.jpg")).expect("fallback succeeds");

    assert_eq!(fallback.decode_calls(), 1, "a single fallback request");
    let seen = fallback.last_request().expect("fallback called");
    let seen_bytes = seen.bytes.expect("the attached source bytes are handed over");
    assert!(
        Arc::ptr_eq(&seen_bytes, &attached),
        "the very buffer the primary attached"
    );
}

// Golden `request-bytes-empty-buffer-counts-as-given`, asserted with `Option` identity (the golden only has the length):
// a request that already carries an EMPTY buffer keeps it (C# `Bytes.HasValue`).
#[test]
fn request_with_empty_bytes_is_kept_not_replaced() {
    let primary = ScriptedDecoder::new(
        |_| Err(make_error("InvalidData", "bad scan", 0).attach_source_bytes(Arc::from(vec![0u8; 8]))),
        unscripted_info,
    );
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let _ = h
        .decoder
        .decode(&request(r"C:\a.jpg").with_bytes(Arc::from(Vec::<u8>::new())))
        .expect("fallback succeeds");

    let seen = fallback.last_request().expect("fallback called");
    let seen_bytes = seen.bytes.expect("the empty request buffer is kept");
    assert!(
        seen_bytes.is_empty(),
        "the request's own (empty) buffer, not the 8 attached bytes"
    );
}

// Golden `source-bytes-empty-buffer-is-still-handed-over`: an attached EMPTY buffer is still handed over.
#[test]
fn attached_empty_buffer_is_still_handed_over() {
    let primary = ScriptedDecoder::new(
        |_| Err(make_error("InvalidData", "bad scan", 0).attach_source_bytes(Arc::from(Vec::<u8>::new()))),
        unscripted_info,
    );
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let _ = h.decoder.decode(&request(r"C:\a.jpg")).expect("fallback succeeds");

    let seen = fallback.last_request().expect("fallback called");
    assert!(seen.bytes.is_some(), "an attached empty buffer counts as attached");
    assert!(seen.bytes.is_some_and(|b| b.is_empty()));
}

// Golden `source-bytes-not-used-for-non-fallbackable`: a non-fallbackable failure is returned as is, its bytes never read.
#[test]
fn source_bytes_are_not_used_for_non_fallbackable_failures() {
    let primary = ScriptedDecoder::new(
        |_| Err(make_error("Busy", "queue full", 0).attach_source_bytes(Arc::from(vec![0u8; 8]))),
        unscripted_info,
    );
    let fallback = always_ok(FALLBACK_BACKEND, 0);
    let h = harness(primary, DecoderBackend::TurboJpeg, fallback.clone());

    let error = err_of(h.decoder.decode(&request(r"C:\a.jpg")), "busy propagates");

    assert_eq!(variant_name(&error), "Busy");
    assert_eq!(fallback.decode_calls(), 0);
}

// ---------------------------------------------------------------------------------------------------------------------
// Relabel forwarding of boolean properties with BOTH values (kills "always true" wrapper mutants).
// ---------------------------------------------------------------------------------------------------------------------

/// Image whose `downscaled` / `is_degraded_fallback` are configurable, so a wrapper that hard-codes `true` is caught.
struct FlagImage {
    downscaled: bool,
    degraded: bool,
    platform: PlatformImage,
}

impl DecodedImageView for FlagImage {
    fn pixel_width(&self) -> i32 {
        10
    }
    fn pixel_height(&self) -> i32 {
        10
    }
    fn downscaled(&self) -> bool {
        self.downscaled
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn estimated_bytes(&self) -> i64 {
        400
    }
    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
    fn actual_backend(&self) -> DecoderBackend {
        DecoderBackend::Wpf
    }
    fn original_width(&self) -> i32 {
        10
    }
    fn original_height(&self) -> i32 {
        10
    }
    fn exif(&self) -> Option<&ExifSummary> {
        None
    }
    fn is_degraded_fallback(&self) -> bool {
        self.degraded
    }
}

#[test]
fn relabel_forwards_false_and_true_boolean_properties() {
    for (downscaled, degraded) in [(false, false), (true, false), (false, true), (true, true)] {
        let inner: Arc<dyn DecodedImageView> = Arc::new(FlagImage {
            downscaled,
            degraded,
            platform: PlatformImage::Opaque(Arc::new(())),
        });
        let relabeled = pr_imaging::fallback::BackendRelabeledImage::new(inner, DecoderBackend::TurboJpeg);
        assert_eq!(relabeled.downscaled(), downscaled, "downscaled forwarded");
        assert_eq!(
            relabeled.is_degraded_fallback(),
            degraded,
            "is_degraded_fallback forwarded"
        );
        assert_eq!(relabeled.actual_backend(), DecoderBackend::TurboJpeg);
    }
}
