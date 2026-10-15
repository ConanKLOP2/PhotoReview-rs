//! Port of `PhotoReview.Imaging/Decoding/FallbackImageDecoder.cs` (C# baseline d92d6aae, 123 lines, 5.2 KB).
//! Contract: `contracts/W4-12b.md`.
//!
//! Wraps a primary decoder with a fallback decoder (always the WPF slot). When the primary fails with a *fallbackable*
//! error (INV-12) it logs a warning, counts a decoder fallback in the metrics and delegates to the fallback decoder, handing
//! it the source bytes the primary already read (when the failure carries them). Missing-file errors, cancellation,
//! out-of-memory, memory-admission refusals and "decoder busy" are returned immediately without a fallback.
//!
//! The C# class throws; here every failure is a [`DecodeError`] (W4-01c) and `IsFallbackable` is a `match` on its variants.
//! A "null" primary/fallback cannot be expressed (`Arc`), so the C# constructor guards have no Rust twin.
//!
use std::sync::Arc;

use pr_core::abstractions::Log;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_core::model::serde_ext::LenientEnum;

use crate::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PlatformImage, SourceBytes}; // W4-01b
use crate::decoder_traits::{DecodeError, ImageDecoder}; // W4-01c

/// The fallback is always WPF (the only caller, the decoder factory, never used another backend). C# `FallbackBackend`.
pub const FALLBACK_BACKEND: DecoderBackend = DecoderBackend::Wpf;

/// `FallbackImageDecoder`. Immutable and shareable (`Send + Sync` through its fields), like every decoder.
pub struct FallbackImageDecoder {
    primary: Arc<dyn ImageDecoder>,
    primary_backend: DecoderBackend,
    fallback: Arc<dyn ImageDecoder>,
    /// C# `ILog? log = null` -> `NullLog.Instance`: the caller passes `Arc::new(NullLog)` for "no log".
    log: Arc<dyn Log>,
    metrics: Option<Arc<ReviewMetrics>>,
}

impl std::fmt::Debug for FallbackImageDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FallbackImageDecoder").finish_non_exhaustive()
    }
}

impl FallbackImageDecoder {
    /// C# constructor. `log` is `Arc::new(pr_core::abstractions::NullLog)` when the caller has none.
    #[must_use]
    pub fn new(
        primary: Arc<dyn ImageDecoder>,
        primary_backend: DecoderBackend,
        fallback: Arc<dyn ImageDecoder>,
        log: Arc<dyn Log>,
        metrics: Option<Arc<ReviewMetrics>>,
    ) -> Self {
        Self {
            primary,
            primary_backend,
            fallback,
            log,
            metrics,
        }
    }

    /// `IsFallbackable(Exception)`: true exactly for `NotSupported`, `MissingCodec` (a `NotSupportedException` subtype in C#),
    /// `InvalidCast`, `Overflow`, `Format`, `InvalidData`, `Com`, `DllNotFound`, `BadImageFormat`, `EntryPointNotFound`.
    /// Everything else is false: `FileNotFound`, `DirectoryNotFound`, `Canceled`, `OutOfMemory`, `MemoryAdmission`, `Busy`
    /// (the explicit INV-12 exclusions) and `InvalidOperation`, `InvalidArgument`, `Disposed`, `Io`, `Other` (not whitelisted).
    #[must_use]
    pub fn is_fallbackable(error: &DecodeError) -> bool {
        // Exhaustive on purpose: a new `DecodeError` variant must be classified here.
        match error {
            DecodeError::WithSourceBytes { error, .. } => Self::is_fallbackable(error),
            DecodeError::NotSupported { .. }
            | DecodeError::MissingCodec { .. }
            | DecodeError::InvalidCast { .. }
            | DecodeError::Overflow { .. }
            | DecodeError::Format { .. }
            | DecodeError::InvalidData { .. }
            | DecodeError::Com { .. }
            | DecodeError::DllNotFound { .. }
            | DecodeError::BadImageFormat { .. }
            | DecodeError::EntryPointNotFound { .. } => true,
            DecodeError::FileNotFound { .. }
            | DecodeError::DirectoryNotFound { .. }
            | DecodeError::Canceled
            | DecodeError::OutOfMemory { .. }
            | DecodeError::MemoryAdmission { .. }
            | DecodeError::Busy { .. }
            | DecodeError::InvalidOperation { .. }
            | DecodeError::InvalidArgument { .. }
            | DecodeError::Disposed { .. }
            | DecodeError::Io(_)
            | DecodeError::Other { .. } => false,
        }
    }

    fn record_fallback(&self) {
        if let Some(metrics) = &self.metrics {
            metrics.record_decoder_fallback(self.primary_backend);
        }
    }
}

impl ImageDecoder for FallbackImageDecoder {
    /// `Decode`: primary first; the result is returned as is when `actual_backend() == primary_backend`, otherwise wrapped
    /// ([`BackendRelabeledImage`]) so it reports `primary_backend`. On a fallbackable failure: `log.warn(
    /// "Decoder {primary_backend} failed for '{path}', falling back to {FALLBACK_BACKEND}: {message}")`, then
    /// `metrics.record_decoder_fallback(primary_backend)`, then the fallback decodes (request carries the failure's source
    /// bytes when the request had none); its result is returned as is when it already reports [`FALLBACK_BACKEND`], else
    /// wrapped. A failure of the fallback propagates unchanged (the primary's error is dropped).
    ///
    /// # Errors
    /// The primary's non-fallbackable error, or the fallback's error.
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        match self.primary.decode(request) {
            Ok(image) => Ok(relabel_if_needed(image, self.primary_backend)),
            Err(error) if !Self::is_fallbackable(&error) => Err(error),
            Err(error) => {
                self.log.warn(&format!(
                    "Decoder {} failed for '{}', falling back to {}: {}",
                    self.primary_backend.name(),
                    request.path.display(),
                    FALLBACK_BACKEND.name(),
                    error.message()
                ));
                self.record_fallback();
                let fallback_request = match (&request.bytes, failure_source_bytes(&error)) {
                    (None, Some(bytes)) => request.clone().with_bytes(bytes),
                    _ => request.clone(),
                };
                let image = self.fallback.decode(&fallback_request)?;
                Ok(relabel_if_needed(image, FALLBACK_BACKEND))
            }
        }
    }

    /// `ReadInfo`: same fallback rule; warning text `"Decoder {primary_backend}.ReadInfo failed for '{path}', falling back
    /// to {FALLBACK_BACKEND}: {message}"`; the fallback is called with the same path (no source bytes involved).
    ///
    /// # Errors
    /// As [`Self::decode`].
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        match self.primary.read_info(path) {
            Ok(info) => Ok(info),
            Err(error) if !Self::is_fallbackable(&error) => Err(error),
            Err(error) => {
                self.log.warn(&format!(
                    "Decoder {}.ReadInfo failed for '{}', falling back to {}: {}",
                    self.primary_backend.name(),
                    path,
                    FALLBACK_BACKEND.name(),
                    error.message()
                ));
                self.record_fallback();
                self.fallback.read_info(path)
            }
        }
    }

    // `as_cancellable` is NOT overridden: C# `FallbackImageDecoder` does not implement `ICancellableImageDecoder`
    // (default `None`). See contract open question 1.
}

/// Returns `image` itself (same `Arc`) when it already reports `backend`, else a relabelled wrapper.
fn relabel_if_needed(image: Arc<dyn DecodedImageView>, backend: DecoderBackend) -> Arc<dyn DecodedImageView> {
    if image.actual_backend() == backend {
        image
    } else {
        Arc::new(BackendRelabeledImage::new(image, backend))
    }
}

/// C# `DecodedImageWithBackend`: every property of `inner` except `actual_backend`, which is `actual_backend`.
/// It deliberately does NOT forward `source_bytes_read()` / `embedded_preview_size()` (C# wrapper does not implement
/// `ISourceReadMetrics` / `IRawPreviewInfo`): they keep the trait defaults (`None`).
pub struct BackendRelabeledImage {
    inner: Arc<dyn DecodedImageView>,
    actual_backend: DecoderBackend,
}

impl std::fmt::Debug for BackendRelabeledImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendRelabeledImage").finish_non_exhaustive()
    }
}

impl BackendRelabeledImage {
    #[must_use]
    pub fn new(inner: Arc<dyn DecodedImageView>, actual_backend: DecoderBackend) -> Self {
        Self { inner, actual_backend }
    }
}

impl DecodedImageView for BackendRelabeledImage {
    fn pixel_width(&self) -> i32 {
        self.inner.pixel_width()
    }
    fn pixel_height(&self) -> i32 {
        self.inner.pixel_height()
    }
    fn downscaled(&self) -> bool {
        self.inner.downscaled()
    }
    fn orientation(&self) -> i32 {
        self.inner.orientation()
    }
    fn estimated_bytes(&self) -> i64 {
        self.inner.estimated_bytes()
    }
    fn platform_image(&self) -> &PlatformImage {
        self.inner.platform_image()
    }
    fn actual_backend(&self) -> DecoderBackend {
        self.actual_backend
    }
    fn original_width(&self) -> i32 {
        self.inner.original_width()
    }
    fn original_height(&self) -> i32 {
        self.inner.original_height()
    }
    fn exif(&self) -> Option<&crate::exif_summary::ExifSummary> {
        self.inner.exif()
    }
    fn is_degraded_fallback(&self) -> bool {
        self.inner.is_degraded_fallback()
    }
}

/// `DecodeFailureSourceBytes.TryGet(ex, out bytes)` for a [`DecodeError`]: the whole source file the failing decoder had
/// already read, if it attached it.
fn failure_source_bytes(error: &DecodeError) -> Option<SourceBytes> {
    error.source_bytes().cloned()
}
