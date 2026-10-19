//! Port of `PhotoReview.App.Shared/Composition/DecoderProviders.cs` (84 lines) and `LibRawPreviewFallback.cs` (16),
//! C# baseline `d92d6aae`. Contract: `contracts/W8-09a.md`.
//!
//! The explicit list of decoder providers (AR01: no reflection). The list order is the registration order of the C#:
//! optional Wpf slot, `WicDirect`, `TurboJpeg` (only if its probe says available), `LibRaw` (same). Providers are LAZY: building
//! the list constructs no decoder; each closure builds a new decoder when `DefaultImageDecoderFactory` calls it.
//!
//! C# probes (`TurboJpegAvailability.Probe`, `LibRawAvailability.Probe`) are `out string? reason` delegates; here a [`ProbeFn`]
//! returns a [`ProbeResult`]. The loggers `AppLog.ErrorForced` / `AppLog.Info` are the [`CompositionLog`] seam.
//!
//! Layering: this pure module may name `pr_platform_win::wic::decoder::WicDirectDecoder` (`pr-app` depends on `pr-platform-win`,
//! D-W8B1-1); it never uses the `windows` crate or `unsafe`.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::SourceReader;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::factory::DecoderProvider;
use pr_imaging::memory_headroom::MemoryInfoSource;
use pr_imaging_raw::{RawFormat, RawPreviewFallback};
use pr_libraw::decoder::LibRawApi;
use pr_turbojpeg::decoder::{FineScaler, TurboJpegTexts};

/// C# `(bool Available, string? Reason)` of a probe. Invariant (both real probes): `available == reason.is_none()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub available: bool,
    pub reason: Option<String>,
}

impl ProbeResult {
    /// `(true, null)`.
    pub fn ok() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    /// `(false, reason)`.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

/// C# `TurboJpegProbe` / `LibRawProbe` delegates. Tests pass closures; production uses [`libraw_probe`] / [`turbojpeg_probe`].
pub type ProbeFn = Arc<dyn Fn() -> ProbeResult + Send + Sync>;

/// `LibRawAvailability.Probe` (run once per process, cached): `pr_libraw::availability::probe()` mapped to a [`ProbeResult`].
pub fn libraw_probe() -> ProbeFn {
    todo!()
}

/// `TurboJpegAvailability.Probe`: `pr_turbojpeg::availability::probe()` mapped to a [`ProbeResult`].
pub fn turbojpeg_probe() -> ProbeFn {
    todo!()
}

/// The two probes of `Create`.
#[derive(Clone)]
pub struct DecoderProbes {
    pub libraw: ProbeFn,
    pub turbojpeg: ProbeFn,
}

impl fmt::Debug for DecoderProbes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecoderProbes").finish_non_exhaustive()
    }
}

impl DecoderProbes {
    /// The real probes ([`libraw_probe`], [`turbojpeg_probe`]).
    pub fn production() -> Self {
        todo!()
    }
}

/// The C# `new InvalidOperationException(the reason, or "<Name> probe failed." when absent)` that `logForcedError` receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeFailure(pub String);

impl fmt::Display for ProbeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ProbeFailure {}

/// C# `Action<string, Exception> logForcedError` + `Action<string> logInfo` (production: `AppLog.ErrorForced`, `AppLog.Info`).
pub trait CompositionLog: Send + Sync {
    /// Records an error even while logging is off (`AppLog.ErrorForced`).
    fn error_forced(&self, message: &str, error: &(dyn Error + 'static));
    /// Info level: visible only with diagnostics on (`AppLog.Info`).
    fn info(&self, message: &str);
}

/// Production [`CompositionLog`] over `crate::misc::app_log::AppLog::global()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct AppLogCompositionLog;

impl CompositionLog for AppLogCompositionLog {
    fn error_forced(&self, message: &str, error: &(dyn Error + 'static)) {
        todo!()
    }

    fn info(&self, message: &str) {
        todo!()
    }
}

/// Everything the decoder constructors need. C# passes `sourceReader` + `codec` and the decoders reach the OS themselves; the
/// Rust decoders take their native seams by constructor, so the host (W8-09b / `ShellCompositionRoot`) supplies them once.
#[derive(Clone)]
pub struct DecoderDeps {
    /// Q-R29 C-2 seam: shared with every decoder built here (INV-8: shared-read opens).
    pub source_reader: Arc<dyn SourceReader>,
    /// C-02 platform codec every decoder hands its pixels to.
    pub codec: Arc<dyn PlatformImageCodec>,
    /// Memory probe of the decoder output guards (C# `MemoryInfo` default `MemoryHeadroom.ReadGcMemoryInfo`).
    pub memory_info: Arc<dyn MemoryInfoSource>,
    /// LibRaw library: production `Arc::new(pr_libraw::decoder::NativeLibRaw)`.
    pub libraw_api: Arc<dyn LibRawApi>,
    /// TurboJPEG fine-scale step (adapter over `pr_platform_win::wic::scaler::resize`).
    pub fine_scaler: Arc<dyn FineScaler>,
    /// TurboJPEG UI sentences (D41: `pr-turbojpeg` has no `pr-l10n` dependency).
    pub turbojpeg_texts: Arc<dyn TurboJpegTexts>,
}

impl fmt::Debug for DecoderDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecoderDeps")
            .field("codec", &self.codec.name())
            .finish_non_exhaustive()
    }
}

/// C# `DecoderProviders.Create(sourceReader, codec, wpfDecoder, isLibRawNeeded)` (public overload): real probes, real log.
/// `wpf_decoder` = the host's Wpf provider (`None` on the Win32 host: no Wpf entry in the list). `is_libraw_needed` = "RAW
/// support is on or the LibRaw backend is selected" (`None` = needed).
pub fn create_decoder_providers(
    deps: &DecoderDeps,
    wpf_decoder: Option<DecoderProvider>,
    is_libraw_needed: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
) -> Vec<(DecoderBackend, DecoderProvider)> {
    todo!()
}

/// C# internal `Create(..., libRawProbe, turboJpegProbe, logForcedError, logInfo)`. In this order:
/// 1. `wpf_decoder` is `Some` => push `(Wpf, wpf_decoder)`;
/// 2. push `(WicDirect, || WicDirectDecoder::new(codec, Some(source_reader), memory_info))`;
/// 3. call `probes.turbojpeg` ONCE: available => push `(TurboJpeg, || TurboJpegDecoder::native(codec, fine_scaler, memory_info,
///    turbojpeg_texts))`; else `log.error_forced("TurboJPEG decoder backend is not available and will not be registered: {reason}",
///    ProbeFailure(reason or "TurboJPEG probe failed."))` (a `None` reason prints as an empty string in the message, like C#);
/// 4. call `probes.libraw` ONCE: available => push `(LibRaw, || LibRawDecoder::new(codec, libraw_api, memory_info))`; else if
///    `is_libraw_needed` is `None` or returns true => `log.error_forced("LibRaw decoder backend is not available and will not be
///    registered: {reason}", ProbeFailure(reason or "LibRaw probe failed."))`; else `log.info("LibRaw decoder backend is not
///    available (RAW support is off, so it is not needed): {reason}")`. `is_libraw_needed` is called only on the unavailable path.
pub fn create_decoder_providers_with(
    deps: &DecoderDeps,
    wpf_decoder: Option<DecoderProvider>,
    is_libraw_needed: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    probes: &DecoderProbes,
    log: &dyn CompositionLog,
) -> Vec<(DecoderBackend, DecoderProvider)> {
    todo!()
}

/// C# `LibRawPreviewFallback : IRawPreviewFallback`: adapts LibRaw's thumbnail extractor to the RAW container preview contract.
/// Holds the LibRaw library (C# calls the static `LibRawDecoder.ReadJpegThumbnail`).
#[derive(Clone)]
pub struct LibRawPreviewFallback {
    api: Arc<dyn LibRawApi>,
}

impl fmt::Debug for LibRawPreviewFallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibRawPreviewFallback").finish_non_exhaustive()
    }
}

impl LibRawPreviewFallback {
    pub fn new(api: Arc<dyn LibRawApi>) -> Self {
        Self { api }
    }
}

impl RawPreviewFallback for LibRawPreviewFallback {
    /// `format != Orf` => `DecodeError::NotSupported { message: "LibRaw thumbnail fallback is not enabled for {format:?}.",
    /// user_text: None, inner_hresult: None }` BEFORE the path is looked at or opened (the message carries the C# enum name:
    /// `Cr2`, `Unknown`, ...; `RawFormat`'s derived `Debug` prints exactly these). `Orf` =>
    /// `pr_libraw::decoder::read_jpeg_thumbnail(&*self.api, path, &CancelToken::none())` unchanged (a blank path is its
    /// `InvalidArgument`, a missing file its LibRaw `InvalidData`).
    fn read_jpeg_thumbnail(&self, path: &str, format: RawFormat) -> Result<Vec<u8>, DecodeError> {
        todo!()
    }
}
