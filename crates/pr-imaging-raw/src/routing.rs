//! Port of `PhotoReview.Imaging.Raw/FormatRoutingDecoder.cs` (C# baseline d92d6aae).
//!
//! RAW router: a path whose extension is one of the 8 RAW extensions goes to the RAW decoder while RAW support is on;
//! with RAW support off it is REFUSED (never handed to the standard chain, which would decode a DNG's tiny IFD0
//! thumbnail as if it were the photo). Every other path goes to the standard decoder chain unchanged. Container
//! errors of the RAW decoder propagate as they are: the router never falls back to the standard chain for RAW.
//! PARITY: I-13, I-02/I-03/INV-12 (routing). See contracts/drafts-w5/W5-12c/contract.md.
//!
//! Target path: `crates/pr-imaging-raw/src/routing.rs` (lead adds `pub mod routing;` to `lib.rs`, see mod-lines.txt).
//!
//! No unsafe, no I/O, no Windows dependency. Depends on W5-01a (`crate::formats::is_raw_extension`) and on the Wave 4
//! seams `pr_imaging::{decode_types, decoder_traits}` (amendment A1: `DecodeError::not_supported_localized`).
#![allow(unused_variables, unused_imports, dead_code)] // D11: stub bodies are todo!(); the porter removes this line.

use std::fmt;
use std::sync::Arc;

use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, UserText};

use crate::formats::is_raw_extension;

/// The UI sentence the router attaches to its refusal (C# `Tr.ImageErrorRawDisabled`, key `image.error.rawDisabled`).
/// Injected instead of a static `Tr` (same idea as `WebpHeicTexts` in W4-12c, decision D13). Each call renders in the
/// language current NOW: the router stores `Arc<dyn RawRoutingTexts>` + the file name in the error's [`UserText`]
/// closure, so a language switch applies at display time.
pub trait RawRoutingTexts: Send + Sync {
    /// "RAW support is turned off in Settings, so {fileName} cannot be shown." (`en.json`).
    fn raw_disabled(&self, file_name: &str) -> String;
}

/// The `RawSupportEnabled` setting, read on EVERY call that reaches a RAW path (a Settings change applies at once).
pub type RawEnabledSource = Arc<dyn Fn() -> bool + Send + Sync>;

/// `FormatRoutingDecoder`. Implements [`ImageDecoder`] but NOT `CancellableImageDecoder`: the C# class does not
/// implement `ICancellableImageDecoder` either, so `as_cancellable()` keeps the trait default `None` (parity trap, see
/// the contract, semantics 7).
pub struct FormatRoutingDecoder {
    standard: Arc<dyn ImageDecoder>,
    raw: Arc<dyn ImageDecoder>,
    is_raw_enabled: RawEnabledSource,
    texts: Arc<dyn RawRoutingTexts>,
}

impl fmt::Debug for FormatRoutingDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FormatRoutingDecoder").finish_non_exhaustive()
    }
}

impl FormatRoutingDecoder {
    /// C# constructor. `standard`: the non-RAW decoder chain (WebP/HEIC router over the user's backend). `raw`: the
    /// RAW decoder. `is_raw_enabled`: the live setting. `texts`: the localized refusal sentence (Rust addition, see the
    /// contract's Open questions 1). The three C# `ArgumentNullException` checks have no Rust twin (non-null by type).
    #[must_use]
    pub fn new(
        standard: Arc<dyn ImageDecoder>,
        raw: Arc<dyn ImageDecoder>,
        is_raw_enabled: RawEnabledSource,
        texts: Arc<dyn RawRoutingTexts>,
    ) -> Self {
        Self {
            standard,
            raw,
            is_raw_enabled,
            texts,
        }
    }

    /// C# `Route` (the shared body of `ReadInfo` and `Decode`): `is_raw_extension(Some(path))` false -> the standard
    /// decoder WITHOUT calling `is_raw_enabled`; else `is_raw_enabled()` exactly once; off ->
    /// `Err(self.raw_disabled(path))`; on -> the RAW decoder.
    fn route(&self, path: &str) -> Result<&Arc<dyn ImageDecoder>, DecodeError> {
        todo!("W5-12c: see contract semantics 1-3")
    }

    /// C# `RawDisabled(path)`: `DecodeError::not_supported_localized(english, text)` with
    /// `english = format!("RAW support is disabled; cannot decode '{file_name}'.")`, `file_name = get_file_name(path)`
    /// (`pr_core::text::dotnet_path`), and `text` = closure over `Arc<dyn RawRoutingTexts>` + the name calling
    /// `raw_disabled(&name)` at display time.
    fn raw_disabled(&self, path: &str) -> DecodeError {
        todo!("W5-12c: see contract semantics 4")
    }
}

impl ImageDecoder for FormatRoutingDecoder {
    /// C# `Decode(request)`: `route(request.path.to_string_lossy())` then forward the SAME request unchanged. No
    /// blank-path check (that is `read_info` only).
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!("W5-12c: see contract semantics 1, 5")
    }

    /// C# `ReadInfo(path)`: blank/whitespace path (`chars().all(char::is_whitespace)`, includes `""`) ->
    /// `Err(DecodeError::InvalidArgument)` BEFORE routing (C# `ArgumentException.ThrowIfNullOrWhiteSpace`), even
    /// with RAW support off; else `route(path)` then forward.
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        todo!("W5-12c: see contract semantics 5")
    }
}
