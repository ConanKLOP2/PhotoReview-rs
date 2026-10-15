//! Port of `PhotoReview.Imaging/Decoding/WebpHeicRoutingDecoder.cs` (C# baseline d92d6aae).
//!
//! Q-FMT-WEBP-HEIC router: a WebP/HEIC/HEIF path goes to the WIC decoder chain whatever backend the user picked, every
//! other path to the inner decoder unchanged. Before any disk read it refuses (a) these formats while the
//! `WebpHeicSupportEnabled` switch is off and (b) a format whose Windows codec is missing (logged ONCE per format per
//! process). PARITY: see contracts/drafts-w4/W4-12c/contract.md.
//!
//! Target path: `crates/pr-imaging/src/webp_heic.rs` (lead adds `pub mod webp_heic;` to `lib.rs`).
//!
//! REQUIRES the W4-01c amendment A1 (see contract, Open questions 1): `DecodeError::not_supported_localized` and
//! `DecodeError::missing_codec_localized` (a `UserText` closure riding on the `NotSupported` / `MissingCodec` variants).
#![allow(unused_variables, dead_code, unused_imports)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pr_core::abstractions::Log;
use pr_core::catalog::types::{WicImageFormat, get_wic_format};
use pr_core::text::dotnet_path::get_file_name;

use crate::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use crate::decoder_traits::{DecodeError, ImageDecoder};

/// C# `WicCodecSupport` (lives in `Decoding/Wic/WicCodecAvailability.cs`, which is W4-11b). It is plain data and the
/// router needs it, so the pure crate owns the type; W4-11b (`pr_platform_win::wic::availability`) produces it and must
/// `use pr_imaging::webp_heic::WicCodecSupport` instead of defining its own (Open questions 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WicCodecSupport {
    /// A WIC decoder for the WebP container is registered.
    pub webp: bool,
    /// A WIC decoder for the HEIF container is registered ("HEIF Image Extensions").
    pub heif_container: bool,
    /// A Media Foundation HEVC video decoder exists ("HEVC Video Extensions" or a device-maker equivalent).
    pub hevc_decoder: bool,
    /// Probe diagnostics; goes into the once-only warning text.
    pub detail: String,
}

impl WicCodecSupport {
    /// C# `Heif`: a HEIC/HEIF photo needs both the container codec and the HEVC decoder.
    #[must_use]
    pub fn heif(&self) -> bool {
        self.heif_container && self.hevc_decoder
    }

    /// C# `WicCodecSupport.None(detail)`: nothing available (the probe itself failed).
    #[must_use]
    pub fn none(detail: impl Into<String>) -> Self {
        Self {
            webp: false,
            heif_container: false,
            hevc_decoder: false,
            detail: detail.into(),
        }
    }

    /// C# `Supports(format)`: `WebP` -> `webp`, `Heif` -> `heif()`, `None` -> `true`.
    #[must_use]
    pub fn supports(&self, format: WicImageFormat) -> bool {
        match format {
            WicImageFormat::WebP => self.webp,
            WicImageFormat::Heif => self.heif(),
            WicImageFormat::None => true,
        }
    }
}

/// The three UI sentences the router attaches (C# `Tr.ImageErrorWebpHeicDisabled` / `ImageErrorWebpCodecMissing` /
/// `ImageErrorHeifCodecMissing`, keys `image.error.webpHeicDisabled`, `image.error.webpCodecMissing`,
/// `image.error.heifCodecMissing`). Injected instead of a static `Tr` (same idea as `SettingsTexts`, D13). Each call
/// renders in the language current NOW: the router stores `Arc<dyn WebpHeicTexts>` + the file name in the error's
/// `UserText` closure, so a language switch applies at display time.
pub trait WebpHeicTexts: Send + Sync {
    /// "WebP/HEIC support is turned off in Settings, so {fileName} cannot be shown."
    fn webp_heic_disabled(&self, file_name: &str) -> String;
    /// "{fileName} is a WebP image, but Windows on this PC has no WebP decoder. ..."
    fn webp_codec_missing(&self, file_name: &str) -> String;
    /// "{fileName} is a HEIC/HEIF photo ... Install \"HEIF Image Extensions\" and \"HEVC Video Extensions\" ..."
    fn heif_codec_missing(&self, file_name: &str) -> String;
}

/// `WebpHeicSupportEnabled`, read on EVERY call (a Settings change applies at once).
pub type EnabledSource = Arc<dyn Fn() -> bool + Send + Sync>;
/// The codec probe; invoked only for a WebP/HEIC path whose switch is on, so JPEG-only sessions never run it.
pub type CodecSource = Arc<dyn Fn() -> WicCodecSupport + Send + Sync>;

/// `WebpHeicRoutingDecoder`. Implements [`ImageDecoder`] but NOT `CancellableImageDecoder`: the C# class does not
/// implement `ICancellableImageDecoder` either, so `as_cancellable()` keeps the trait default `None` (parity trap, see
/// the contract).
pub struct WebpHeicRoutingDecoder {
    inner: Arc<dyn ImageDecoder>,
    wic_decoder: Arc<dyn ImageDecoder>,
    is_enabled: EnabledSource,
    codecs: CodecSource,
    texts: Arc<dyn WebpHeicTexts>,
    log: Arc<dyn Log>,
    /// C# `_webpMissingLogged` (`Interlocked.Exchange`): `swap(true) == false` means "first time, log it".
    webp_missing_logged: AtomicBool,
    /// C# `_heifMissingLogged`.
    heif_missing_logged: AtomicBool,
}

impl fmt::Debug for WebpHeicRoutingDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebpHeicRoutingDecoder").finish_non_exhaustive()
    }
}

impl WebpHeicRoutingDecoder {
    /// C# constructor. `inner`: decoder for every other path (the user's backend chain). `wic_decoder`: WIC chain for
    /// WebP/HEIC. `log`: `None` = `NullLog` (the C# `log ?? NullLog.Instance`). The four C# `ArgumentNullException`
    /// checks have no Rust twin (non-null by type).
    #[must_use]
    pub fn new(
        inner: Arc<dyn ImageDecoder>,
        wic_decoder: Arc<dyn ImageDecoder>,
        is_enabled: EnabledSource,
        codecs: CodecSource,
        texts: Arc<dyn WebpHeicTexts>,
        log: Option<Arc<dyn Log>>,
    ) -> Self {
        todo!()
    }

    /// C# static `WillRefuse(path, isEnabled, codecs)`: true when `decode` would refuse `path` without reading it.
    /// `path == None` (C# `null`) is false. Short-circuits like C#: format `None` -> false WITHOUT calling either
    /// closure; else `!is_enabled() || !codecs().supports(format)` (the probe is not called when the switch is off).
    /// Preload uses it to skip prefetching bytes no decode could use.
    #[must_use]
    pub fn will_refuse(
        path: Option<&str>,
        is_enabled: &dyn Fn() -> bool,
        codecs: &dyn Fn() -> WicCodecSupport,
    ) -> bool {
        todo!()
    }

    /// C# `Route(path)`: `get_wic_format(path)`; `None` -> inner. Else the file name is `get_file_name(path)`; switch off
    /// -> `Err(DecodeError::not_supported_localized(..))` (probe NOT run); else `codecs()` ONCE, missing ->
    /// `Err(self.missing_codec(..))`; else the WIC decoder.
    fn route(&self, path: &str) -> Result<&Arc<dyn ImageDecoder>, DecodeError> {
        todo!()
    }

    /// C# `MissingCodec`: builds the `MissingCodec` refusal and logs ONE warning per format per process.
    fn missing_codec(&self, format: WicImageFormat, file_name: &str, codecs: &WicCodecSupport) -> DecodeError {
        todo!()
    }
}

impl ImageDecoder for WebpHeicRoutingDecoder {
    /// C# `Decode(request)`: `route(request.path)` then forward. Nothing is read before the refusal.
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!()
    }

    /// C# `ReadInfo(path)`: blank/whitespace path -> `Err(DecodeError::InvalidArgument)` (C#
    /// `ArgumentException.ThrowIfNullOrWhiteSpace`), BEFORE routing; else `route(path)` then forward.
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        todo!()
    }
}
