//! Tests for `pr_shell::composition::decoders` (contract W8-09a). Port of `DecoderProvidersTests` and
//! `LibRawPreviewFallbackTests` (C# baseline d92d6aae, `tests/PhotoReview.App.Tests/Composition/`).
//!
//! Nothing here loads `libraw.dll` / `turbojpeg.dll`: the probes are closures and the `LibRawApi` is a fake whose `init`
//! panics (a test that reaches it has opened the library when it must not). The two native ORF tests are `#[ignore]`d.

use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::PhysicalSourceReader;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::factory::DecoderProvider;
use pr_imaging::memory_headroom::MemoryInfo;
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging_raw::{RawFormat, RawPreviewFallback};
use pr_libraw::decoder::{LibRawApi, LibRawSession};
use pr_shell::composition::decoders::{
    CompositionLog, DecoderDeps, DecoderProbes, LibRawPreviewFallback, ProbeFn, ProbeResult,
    create_decoder_providers_with,
};
use pr_turbojpeg::decoder::{EnglishTurboJpegTexts, FineScaleError};

/// A `LibRawApi` that must never be reached.
struct UnreachableLibRaw;

impl LibRawApi for UnreachableLibRaw {
    fn init(&self) -> Result<Box<dyn LibRawSession>, DecodeError> {
        panic!("the LibRaw library must not be opened by this test");
    }

    fn format_error(&self, error_code: i32) -> String {
        format!("LibRaw error {error_code}")
    }
}

fn deps() -> DecoderDeps {
    DecoderDeps {
        source_reader: Arc::new(PhysicalSourceReader),
        codec: PixelBufferImageCodec::shared(),
        memory_info: Arc::new(|| MemoryInfo {
            total_available: 0,
            load: 0,
        }),
        libraw_api: Arc::new(UnreachableLibRaw),
        fine_scaler: Arc::new(|_: &_, _, _| Err(FineScaleError::OutOfMemory)),
        turbojpeg_texts: Arc::new(EnglishTurboJpegTexts),
    }
}

#[derive(Default)]
struct RecordingLogs {
    errors: Mutex<Vec<(String, String)>>,
    infos: Mutex<Vec<String>>,
}

impl RecordingLogs {
    fn errors(&self) -> Vec<(String, String)> {
        self.errors.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn infos(&self) -> Vec<String> {
        self.infos.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl CompositionLog for RecordingLogs {
    fn error_forced(&self, message: &str, error: &(dyn Error + 'static)) {
        self.errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((message.to_owned(), error.to_string()));
    }

    fn info(&self, message: &str) {
        self.infos
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message.to_owned());
    }
}

fn probe(result: ProbeResult) -> ProbeFn {
    Arc::new(move || result.clone())
}

fn counting_probe(result: ProbeResult, calls: &Arc<AtomicUsize>) -> ProbeFn {
    let calls = Arc::clone(calls);
    Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        result.clone()
    })
}

fn missing_libraw() -> ProbeFn {
    probe(ProbeResult::unavailable("libraw.dll not found"))
}

fn available() -> ProbeFn {
    probe(ProbeResult::ok())
}

/// A Wpf provider that must stay lazy: running it fails the test.
fn wpf_provider() -> DecoderProvider {
    Arc::new(|| -> Arc<dyn ImageDecoder> { panic!("the Wpf provider must stay lazy") })
}

fn needed(value: bool) -> Option<Arc<dyn Fn() -> bool + Send + Sync>> {
    Some(Arc::new(move || value))
}

fn backends(providers: &[(DecoderBackend, DecoderProvider)]) -> Vec<DecoderBackend> {
    providers.iter().map(|(backend, _)| *backend).collect()
}

fn create(
    needed: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    libraw: ProbeFn,
    turbojpeg: ProbeFn,
    logs: &RecordingLogs,
) -> Vec<(DecoderBackend, DecoderProvider)> {
    create_decoder_providers_with(
        &deps(),
        Some(wpf_provider()),
        needed,
        &DecoderProbes { libraw, turbojpeg },
        logs,
    )
}

// C#: DecoderProvidersTests.Create_TurboJpegMissing_LogsThroughTheInjectedLoggerAndDoesNotRegisterIt
#[test]
fn create_turbo_jpeg_missing_logs_through_the_injected_logger_and_does_not_register_it() {
    let logs = RecordingLogs::default();

    let providers = create(
        needed(false),
        available(),
        probe(ProbeResult::unavailable("turbojpeg.dll not found")),
        &logs,
    );

    let errors = logs.errors();
    assert!(
        errors.iter().any(|(message, _)| message.contains("TurboJPEG")),
        "{errors:?}"
    );
    // The exact text and the exception message (support reads the log).
    assert_eq!(
        errors,
        vec![(
            "TurboJPEG decoder backend is not available and will not be registered: turbojpeg.dll not found".to_owned(),
            "turbojpeg.dll not found".to_owned()
        )]
    );
    assert!(!backends(&providers).contains(&DecoderBackend::TurboJpeg));
}

// New test: a probe without a reason prints an empty reason and the C# default exception text.
#[test]
fn turbo_jpeg_missing_without_a_reason_uses_the_default_exception_text() {
    let logs = RecordingLogs::default();
    let no_reason = ProbeResult {
        available: false,
        reason: None,
    };

    create(needed(false), available(), probe(no_reason), &logs);

    assert_eq!(
        logs.errors(),
        vec![(
            "TurboJPEG decoder backend is not available and will not be registered: ".to_owned(),
            "TurboJPEG probe failed.".to_owned()
        )]
    );
}

// C#: DecoderProvidersTests.Create_LibRawMissingWhileRawSupportIsOff_LogsInfoNotAnError
#[test]
fn create_lib_raw_missing_while_raw_support_is_off_logs_info_not_an_error() {
    let logs = RecordingLogs::default();

    let providers = create(needed(false), missing_libraw(), available(), &logs);

    assert!(logs.errors().iter().all(|(message, _)| !message.contains("LibRaw")));
    assert_eq!(
        logs.infos(),
        vec!["LibRaw decoder backend is not available (RAW support is off, so it is not needed): libraw.dll not found"]
    );
    assert!(!backends(&providers).contains(&DecoderBackend::LibRaw));
}

// C#: DecoderProvidersTests.Create_LibRawMissingWhileNeeded_LogsAnError
#[test]
fn create_lib_raw_missing_while_needed_logs_an_error() {
    for is_needed in [needed(true), None] {
        let logs = RecordingLogs::default();

        create(is_needed, missing_libraw(), available(), &logs);

        assert_eq!(
            logs.errors(),
            vec![(
                "LibRaw decoder backend is not available and will not be registered: libraw.dll not found".to_owned(),
                "libraw.dll not found".to_owned()
            )]
        );
        assert!(logs.infos().iter().all(|message| !message.contains("LibRaw")));
    }
}

// C#: DecoderProvidersTests.Create_LibRawAvailable_RegistersTheBackendWithoutLogging
#[test]
fn create_lib_raw_available_registers_the_backend_without_logging() {
    let logs = RecordingLogs::default();

    let providers = create(needed(false), available(), available(), &logs);

    assert!(backends(&providers).contains(&DecoderBackend::LibRaw));
    assert!(logs.errors().is_empty());
    assert!(logs.infos().is_empty());
}

// New test
#[test]
fn providers_are_listed_in_registration_order_with_the_wpf_slot_first() {
    let logs = RecordingLogs::default();

    let with_wpf = create(needed(true), available(), available(), &logs);
    let without_wpf = create_decoder_providers_with(
        &deps(),
        None,
        needed(true),
        &DecoderProbes {
            libraw: available(),
            turbojpeg: available(),
        },
        &logs,
    );
    let wic_only = create_decoder_providers_with(
        &deps(),
        None,
        needed(true),
        &DecoderProbes {
            libraw: missing_libraw(),
            turbojpeg: probe(ProbeResult::unavailable("x")),
        },
        &logs,
    );

    assert_eq!(
        backends(&with_wpf),
        [
            DecoderBackend::Wpf,
            DecoderBackend::WicDirect,
            DecoderBackend::TurboJpeg,
            DecoderBackend::LibRaw
        ]
    );
    assert_eq!(
        backends(&without_wpf),
        [
            DecoderBackend::WicDirect,
            DecoderBackend::TurboJpeg,
            DecoderBackend::LibRaw
        ]
    );
    assert_eq!(backends(&wic_only), [DecoderBackend::WicDirect]);
}

// New test: T1, the providers are lazy (a counting Wpf closure must not run while building the list).
#[test]
fn building_the_list_constructs_no_decoder() {
    let built = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&built);
    let wpf: DecoderProvider = Arc::new(move || -> Arc<dyn ImageDecoder> {
        counter.fetch_add(1, Ordering::SeqCst);
        panic!("not under test")
    });
    let logs = RecordingLogs::default();

    let providers = create_decoder_providers_with(
        &deps(),
        Some(wpf),
        needed(true),
        &DecoderProbes {
            libraw: available(),
            turbojpeg: available(),
        },
        &logs,
    );

    assert_eq!(providers.len(), 4);
    assert_eq!(built.load(Ordering::SeqCst), 0);
}

// New test: T2.
#[test]
fn each_probe_is_called_once() {
    let libraw_calls = Arc::new(AtomicUsize::new(0));
    let turbo_calls = Arc::new(AtomicUsize::new(0));
    let logs = RecordingLogs::default();

    create(
        needed(true),
        counting_probe(ProbeResult::unavailable("x"), &libraw_calls),
        counting_probe(ProbeResult::unavailable("y"), &turbo_calls),
        &logs,
    );
    assert_eq!(libraw_calls.load(Ordering::SeqCst), 1);
    assert_eq!(turbo_calls.load(Ordering::SeqCst), 1);

    create(
        needed(true),
        counting_probe(ProbeResult::ok(), &libraw_calls),
        counting_probe(ProbeResult::ok(), &turbo_calls),
        &logs,
    );
    assert_eq!(libraw_calls.load(Ordering::SeqCst), 2, "once per Create call");
    assert_eq!(turbo_calls.load(Ordering::SeqCst), 2, "once per Create call");
}

// New test: `is_libraw_needed` is evaluated only on the unavailable path (C# `isLibRawNeeded?.Invoke()` sits in the else branch).
#[test]
fn is_libraw_needed_is_not_called_when_libraw_is_available() {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&asked);
    let is_needed: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        true
    });
    let logs = RecordingLogs::default();

    create(Some(Arc::clone(&is_needed)), available(), available(), &logs);
    assert_eq!(asked.load(Ordering::SeqCst), 0);

    create(Some(is_needed), missing_libraw(), available(), &logs);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

fn fallback() -> LibRawPreviewFallback {
    LibRawPreviewFallback::new(Arc::new(UnreachableLibRaw))
}

// C#: LibRawPreviewFallbackTests.ReadJpegThumbnail_NonOrfFormat_ThrowsNotSupportedWithoutOpeningTheFile
#[test]
fn read_jpeg_thumbnail_non_orf_format_throws_not_supported_without_opening_the_file() {
    let formats = [
        RawFormat::Cr2,
        RawFormat::Cr3,
        RawFormat::Nef,
        RawFormat::Nrw,
        RawFormat::Arw,
        RawFormat::Dng,
        RawFormat::Raf,
        RawFormat::Rw2,
        RawFormat::Pef,
        RawFormat::Unknown,
    ];
    for format in formats {
        let error = fallback()
            .read_jpeg_thumbnail(r"Z:\does\not\exist.raw", format)
            .expect_err("only ORF is enabled");

        match error {
            DecodeError::NotSupported {
                message,
                user_text,
                inner_hresult,
            } => {
                assert_eq!(
                    message,
                    format!("LibRaw thumbnail fallback is not enabled for {format:?}.")
                );
                assert!(user_text.is_none());
                assert!(inner_hresult.is_none());
            }
            other => panic!("{format:?}: expected NotSupported, got {other:?}"),
        }
    }
}

// C#: LibRawPreviewFallbackTests.ReadJpegThumbnail_BlankPathForOrf_ThrowsArgumentException
#[test]
fn read_jpeg_thumbnail_blank_path_for_orf_throws_argument_exception() {
    // `UnreachableLibRaw::init` panics: the blank check must come first.
    let error = fallback()
        .read_jpeg_thumbnail(" ", RawFormat::Orf)
        .expect_err("a blank path is an argument error");

    assert!(matches!(error, DecodeError::InvalidArgument { .. }), "{error:?}");
}

// C#: LibRawPreviewFallbackTests.ReadJpegThumbnail_OrfCorpusFile_ReturnsCompleteJpeg
#[test]
#[ignore = "native: needs libraw.dll and the RAW corpus"]
fn read_jpeg_thumbnail_orf_corpus_file_returns_complete_jpeg() {
    let Some(path) = std::env::var_os("PHOTOREVIEW_RS_RAW_CORPUS_ORF") else {
        return; // corpus absent
    };
    let fallback = LibRawPreviewFallback::new(Arc::new(pr_libraw::decoder::NativeLibRaw));

    let thumbnail = fallback
        .read_jpeg_thumbnail(&path.to_string_lossy(), RawFormat::Orf)
        .expect("the corpus ORF has a thumbnail");

    assert!(thumbnail.len() > 4);
    assert_eq!(thumbnail[0], 0xFF);
    assert_eq!(thumbnail[1], 0xD8);
    assert_eq!(thumbnail[thumbnail.len() - 1], 0xD9);
}

// C#: LibRawPreviewFallbackTests.ReadJpegThumbnail_MissingOrfFile_ThrowsInvalidDataFromLibRaw
#[test]
#[ignore = "native: needs libraw.dll"]
fn read_jpeg_thumbnail_missing_orf_file_throws_invalid_data_from_lib_raw() {
    let temp = pr_test_support::TempRoot::new("libraw-fallback-missing");
    let missing = temp.join("absent.orf");
    let fallback = LibRawPreviewFallback::new(Arc::new(pr_libraw::decoder::NativeLibRaw));

    let error = fallback
        .read_jpeg_thumbnail(&missing.to_string_lossy(), RawFormat::Orf)
        .expect_err("the file does not exist");

    assert!(matches!(error, DecodeError::InvalidData { .. }), "{error:?}");
}
