//! Q6 (X-composition-factories) mutation killers for `pr_shell::composition::decoders`: the production wrapper `create_decoder_providers`, the
//! `AppLogCompositionLog` glue and three `Debug` impls that no test reached (cargo-mutants, `composition_decoders` + `composition_factories`).
//! New tests, no C# twin (C# `DecoderProvidersTests` always call the internal overload with fakes). Nothing here opens `libraw.dll`; the real
//! probes only look the DLLs up. `AppLog::global()` is process-wide, so every test takes `LOCK` and redirects it to a log file in a temp root.

use std::error::Error;
use std::sync::{Arc, Mutex, PoisonError};

use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_core::diagnostics::file_log::{DEFAULT_MAX_LOG_BYTES, FileLog};
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::factory::DecoderProvider;
use pr_imaging::memory_headroom::MemoryInfo;
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_libraw::decoder::{LibRawApi, LibRawSession};
use pr_shell::composition::decoders::{
    AppLogCompositionLog, CompositionLog, DecoderDeps, DecoderProbes, LibRawPreviewFallback, ProbeFailure,
    create_decoder_providers,
};
use pr_test_support::TempRoot;
use pr_turbojpeg::decoder::{EnglishTurboJpegTexts, FineScaleError};

static LOCK: Mutex<()> = Mutex::new(());

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
        source_reader: Arc::new(pr_core::abstractions::PhysicalSourceReader),
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

/// Holds `LOCK`, points the global `AppLog` at a file in a temp root and restores the default target on drop.
struct RedirectedLog {
    _guard: std::sync::MutexGuard<'static, ()>,
    root: TempRoot,
}

impl RedirectedLog {
    fn new() -> Self {
        let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let root = TempRoot::new("q6-composition-log");
        let target: Arc<dyn AppLogTarget> =
            Arc::new(FileLog::new(root.join("app.log"), DEFAULT_MAX_LOG_BYTES).expect("absolute temp path"));
        AppLog::global().set_instance(Some(target));
        Self { _guard: guard, root }
    }

    fn text(&self) -> String {
        AppLog::global().flush();
        std::fs::read_to_string(self.root.join("app.log")).unwrap_or_default()
    }
}

impl Drop for RedirectedLog {
    fn drop(&mut self) {
        AppLog::global().set_instance(None);
    }
}

#[test]
fn the_production_overload_registers_wpf_then_wic_direct_with_the_real_probes() {
    // New: `create_decoder_providers` (real probes + real log) was never called; `vec![]` passed every test.
    let log = RedirectedLog::new();
    let wpf: DecoderProvider = Arc::new(|| -> Arc<dyn ImageDecoder> { panic!("the Wpf provider must stay lazy") });
    let providers = create_decoder_providers(&deps(), Some(wpf), None);
    let backends: Vec<DecoderBackend> = providers.iter().map(|(backend, _)| *backend).collect();
    assert!(backends.len() >= 2, "{backends:?}");
    assert_eq!(backends[..2], [DecoderBackend::Wpf, DecoderBackend::WicDirect]);
    // A probe that fails is logged through the global AppLog even while logging is off; one that succeeds registers its backend.
    let text = log.text();
    for (backend, name) in [
        (DecoderBackend::TurboJpeg, "TurboJPEG decoder backend is not available"),
        (DecoderBackend::LibRaw, "LibRaw decoder backend is not available"),
    ] {
        assert_ne!(
            backends.contains(&backend),
            text.contains(name),
            "{backend:?}: registered XOR logged as unavailable; log: {text}"
        );
    }
    // Without a Wpf provider (the Win32 host) the list starts with WicDirect.
    let without_wpf = create_decoder_providers(&deps(), None, None);
    assert_eq!(without_wpf[0].0, DecoderBackend::WicDirect);
    assert_eq!(without_wpf.len(), providers.len() - 1);
}

#[test]
fn app_log_composition_log_forwards_info_and_forced_errors_to_the_global_log() {
    // New: `AppLogCompositionLog::{info, error_forced}` -> `AppLog::global()` (mutants: body replaced by `()`).
    let log = RedirectedLog::new();
    AppLog::global().set_enabled(false);
    let sink = AppLogCompositionLog;
    let failure = ProbeFailure("probe-reason-q6".to_owned());
    let error: &(dyn Error + 'static) = &failure;
    sink.error_forced("forced-message-q6", error);
    sink.info("info-while-off-q6");
    let text = log.text();
    assert!(text.contains("forced-message-q6"), "{text}");
    assert!(text.contains("probe-reason-q6"), "{text}");
    assert!(
        !text.contains("info-while-off-q6"),
        "info is invisible while logging is off: {text}"
    );
    assert!(!AppLog::global().enabled(), "the forced write restores the flag");
    AppLog::global().set_enabled(true);
    sink.info("info-while-on-q6");
    assert!(log.text().contains("info-while-on-q6"));
}

#[test]
fn composition_debug_impls_name_their_types() {
    // New: the three `Debug` impls of decoders.rs (mutants: `Ok(Default::default())` prints nothing).
    assert!(format!("{:?}", DecoderProbes::production()).contains("DecoderProbes"));
    assert!(format!("{:?}", deps()).contains("DecoderDeps"));
    assert!(format!("{:?}", LibRawPreviewFallback::new(Arc::new(UnreachableLibRaw))).contains("LibRawPreviewFallback"));
}
