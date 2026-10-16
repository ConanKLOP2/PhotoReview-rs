//! Port of `PhotoReview.Imaging.Raw/RawDecoder.cs` (C# baseline d92d6aae), W5-12b1 half: construction, the
//! container-info cache keyed by file identity (TOCTOU checks), `read_info`, the `decode` composition (EXIF summary,
//! preview selection, sensor size, downscaled flag, source-bytes accounting, the `RawDecodedImage` wrapper) and the
//! single-preview read (`decode_preview` / `read_preview_range`, never a whole-file read).
//!
//! The fallback chain (next-best preview, ORF thumbnail fallback, no-preview path, full-decode last resort, failure
//! classification) is W5-12b2 and lives in the child module [`fallback`] (`decoder/fallback.rs`), which sees the private
//! fields of [`RawDecoder`] because it is a descendant module.
//!
//! RAW-21/22 steps (C# class summary):
//! 1. read the RAW container header through a `RawContainerReader` over a `SourceRawHeaderSource` (bounded reads);
//! 2. select the JPEG preview with `PreviewSelector`;
//! 3. read ONLY that preview's byte range (through the `SourceReader`, optionally the `SourceBytesCache` range cache);
//! 4. decode it with the inner decoder, passing the container orientation as `source_orientation`;
//! 5. wrap the result with the sensor size, the container EXIF and the inner decoder's backend.
//!
//! Localized sentences are injected ([`RawDecoderTexts`], D41: no `pr-l10n` dependency). No `unsafe`, no Windows-only API.

#![allow(unused_variables, dead_code)] // D11: removed together with the last `todo!()`.

mod fallback;

pub use fallback::{
    MAX_FALLBACK_THUMBNAIL_BYTES, MAX_PREVIEW_ATTEMPTS, MIN_USEFUL_FALLBACK_LONG_SIDE, is_corrupt_data_failure,
    is_corrupt_hresult, is_recoverable_preview_failure, is_resource_exhaustion, next_preview_candidates,
};

use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::{SourceReadPriority, SourceReader};
use pr_core::caching::lru::BoundedLruCache;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::cache::source_bytes::{SourceBytesCache, SourceBytesError};
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, UserText};
use pr_imaging::exif_summary::ExifSummary;

// W5-01a / W5-01b (crate-root re-exports, see mod-lines.txt), W5-11a (registry), W5-11b (`preview_selector`),
// W5-11c (`raw_exif`), W5-12a (`header_source`).
use crate::header_source::SourceRawHeaderSource;
use crate::registry::RawContainerReaderRegistry;
use crate::{EmbeddedPreview, RawContainerInfo, RawHeaderError, RawPreviewFallback};

/// C# `MaxThumbnailLongSide`: a JPEG whose long side is at most this many pixels is a thumbnail, never a viewable
/// photo (marked downscaled even when the container declares no sensor size).
pub const MAX_THUMBNAIL_LONG_SIDE: i32 = 256;

/// C# `new BoundedLruCache<RawInfoKey, CachedRaw>(256, _ => 1)`: at most this many file identities are remembered.
pub const CONTAINER_INFO_CACHE_CAPACITY: i64 = 256;

/// C# `RawDecoder.GetContainerInfo` error text when no reader accepts the file: `"Unsupported RAW format: {ext}"`.
pub const UNSUPPORTED_FORMAT_PREFIX: &str = "Unsupported RAW format: ";

/// The four UI sentences of the decoder (C# `Tr.ImageErrorRawCorrupt`, `Tr.ImageErrorRawUnsupported`,
/// `Tr.ImageErrorRawNoPreview`, `Tr.ErrIoFileChangedWhileReading(path)`; catalog keys `image.error.rawCorrupt`,
/// `image.error.rawUnsupported`, `image.error.rawNoPreview`, `err.io.fileChangedWhileReading`). Injected (D41/P-24, the
/// D13 pattern): the decoder stores `Arc<dyn RawDecoderTexts>` (plus the path) in each error's [`UserText`] closure, so a
/// language switch applies at display time.
pub trait RawDecoderTexts: Send + Sync {
    /// `image.error.rawCorrupt`: "Corrupt or invalid RAW file."
    fn raw_corrupt(&self) -> String;
    /// `image.error.rawUnsupported`: "Unsupported RAW container format."
    fn raw_unsupported(&self) -> String;
    /// `image.error.rawNoPreview`: "This RAW file has no embedded preview, so it cannot be shown."
    fn raw_no_preview(&self) -> String;
    /// `err.io.fileChangedWhileReading`: "The file changed while it was being read: {path}"
    fn file_changed_while_reading(&self, path: &str) -> String;
}

/// The English catalog (`assets/languages/en.json` at the baseline): what C# `Tr` renders while the UI is English. The
/// default texts of [`RawDecoder::new`]; tests compare `DecodeError::describe` with these strings.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnglishRawDecoderTexts;

impl RawDecoderTexts for EnglishRawDecoderTexts {
    fn raw_corrupt(&self) -> String {
        todo!()
    }

    fn raw_unsupported(&self) -> String {
        todo!()
    }

    fn raw_no_preview(&self) -> String {
        todo!()
    }

    fn file_changed_while_reading(&self, path: &str) -> String {
        todo!()
    }
}

/// The optional constructor arguments of C# `RawDecoder(innerDecoder, sourceReader = null, registry = null,
/// sourceBytesCache = null, previewFallback = null, noPreviewDecoder = null)`, plus the injected texts (D41).
/// `Default` = every C# default: `PhysicalSourceReader`, `new RawContainerReaderRegistry()`, no cache, no fallbacks,
/// [`EnglishRawDecoderTexts`].
#[derive(Default)]
pub struct RawDecoderOptions {
    /// `sourceReader ?? PhysicalSourceReader.Instance`: every header and preview open goes through it (INV-8).
    pub source_reader: Option<Arc<dyn SourceReader>>,
    /// `registry ?? new RawContainerReaderRegistry()`.
    pub registry: Option<Arc<RawContainerReaderRegistry>>,
    /// Range cache for the preview bytes (never the whole RAW file).
    pub source_bytes_cache: Option<Arc<SourceBytesCache>>,
    /// LibRaw thumbnail provider (ORF unsupported preview; RAW without a usable JPEG).
    pub preview_fallback: Option<Arc<dyn RawPreviewFallback>>,
    /// Full-decode last resort (LibRaw) for a RAW with no usable JPEG.
    pub no_preview_decoder: Option<Arc<dyn ImageDecoder>>,
    /// UI sentences; `None` = [`EnglishRawDecoderTexts`].
    pub texts: Option<Arc<dyn RawDecoderTexts>>,
}

impl fmt::Debug for RawDecoderOptions {
    /// The trait objects have no `Debug`: report which options are set.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// C# `RawInfoKey(string Path, long Length, long LastWriteUtcTicks)`: `path` is `Path.GetFullPath(path).ToUpperInvariant()`
/// (`pr_imaging::cache_key::normalize_key_path`), `length` the opened source's length, `last_write_utc_ticks` the stat
/// taken BEFORE the open (0 for a path with no physical file).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RawInfoKey {
    path: String,
    length: i64,
    last_write_utc_ticks: i64,
}

/// C# `CachedRaw(RawContainerInfo Info, ExifSummary? Exif, bool ExifRead)`: what is remembered per file identity. `info`
/// carries the previews with the JPEG headers already resolved; `exif_read` is true once a COMPLETE EXIF read happened
/// (`exif` may then be `None` = "this file has no EXIF"). Shared behind an `Arc` (the LRU clones its values).
#[derive(Debug, Clone)]
struct CachedRaw {
    info: RawContainerInfo,
    exif: Option<ExifSummary>,
    exif_read: bool,
}

/// What `GetContainerInfo` hands back through its `out` parameters: the cache entry, the OPEN header source (dropped at
/// the end of the caller's `using` block = the end of `read_info`/`decode`) and the identity key.
struct ContainerLookup {
    cached: Arc<CachedRaw>,
    header_source: SourceRawHeaderSource,
    key: RawInfoKey,
}

/// The `out` parameters of C# `DecodePreviewWithFallbacks` / `DecodeWithoutPreview`, as one value (W5-12b2 produces it,
/// W5-12b1's `decode` consumes it). The byte counters are already in their FINAL places: when the no-preview path ends in
/// the full decode, the whole-file length is in `full_decode_bytes_read` and `fallback_thumbnail_bytes_read` is 0 (C#
/// does that move in `Decode`; here `decode_without_preview` returns it moved).
struct PreviewOutcome {
    /// The inner (or thumbnail, or full) decode result.
    image: Arc<dyn DecodedImageView>,
    /// Bytes of the decoded preview range plus every failed attempt's range (each capped at `MAX_PREVIEW_BYTES`); 0 for a
    /// range-cache hit.
    preview_bytes_read: i64,
    /// Length of the accepted fallback thumbnail; 0 when none was used or it was discarded.
    fallback_thumbnail_bytes_read: i64,
    /// Whole-file length of the full decode (0: no full decode, or a path with no physical file).
    full_decode_bytes_read: i64,
    /// False only for the full decode: its pixels are the sensor's, not a JPEG preview.
    from_jpeg_preview: bool,
    /// A next-best preview or the ORF thumbnail was accepted after a better source failed (never cache it as the photo).
    degraded_fallback: bool,
}

/// C# `RawDecodedImage : IDecodedImage, ISourceReadMetrics, IRawPreviewInfo`. Pixel size, estimated bytes and the
/// platform image delegate to the inner image; everything else is the RAW view of it. `source_bytes_read` and
/// `embedded_preview_size` are ALWAYS `Some` (C# implements both interfaces unconditionally; a full decode reports
/// `(0, 0)` as its embedded preview size).
struct RawDecodedImage {
    inner: Arc<dyn DecodedImageView>,
    /// C# `OriginalWidth`/`OriginalHeight`: the sensor size (orientation-applied when the request applied it).
    original_width: i32,
    original_height: i32,
    downscaled: bool,
    /// The CONTAINER orientation (not the inner image's).
    orientation: i32,
    /// `exif ?? decoded.Exif`.
    exif: Option<ExifSummary>,
    actual_backend: DecoderBackend,
    source_bytes_read: i64,
    embedded_preview_width: i32,
    embedded_preview_height: i32,
    is_degraded_fallback: bool,
}

impl DecodedImageView for RawDecodedImage {
    fn pixel_width(&self) -> i32 {
        todo!()
    }

    fn pixel_height(&self) -> i32 {
        todo!()
    }

    fn downscaled(&self) -> bool {
        todo!()
    }

    fn orientation(&self) -> i32 {
        todo!()
    }

    fn estimated_bytes(&self) -> i64 {
        todo!()
    }

    fn platform_image(&self) -> &PlatformImage {
        todo!()
    }

    fn actual_backend(&self) -> DecoderBackend {
        todo!()
    }

    fn original_width(&self) -> i32 {
        todo!()
    }

    fn original_height(&self) -> i32 {
        todo!()
    }

    fn exif(&self) -> Option<&ExifSummary> {
        todo!()
    }

    fn is_degraded_fallback(&self) -> bool {
        todo!()
    }

    fn source_bytes_read(&self) -> Option<i64> {
        todo!()
    }

    fn embedded_preview_size(&self) -> Option<(i32, i32)> {
        todo!()
    }
}

/// `RawDecoder : IImageDecoder`. Shared across threads (the factory builds one); the container-info cache is the only
/// mutable state and is internally locked (`BoundedLruCache`). Not cancellable (C# does not implement
/// `ICancellableImageDecoder`): `as_cancellable` keeps the trait default `None`.
pub struct RawDecoder {
    inner_decoder: Arc<dyn ImageDecoder>,
    source_reader: Arc<dyn SourceReader>,
    registry: Arc<RawContainerReaderRegistry>,
    source_bytes_cache: Option<Arc<SourceBytesCache>>,
    preview_fallback: Option<Arc<dyn RawPreviewFallback>>,
    no_preview_decoder: Option<Arc<dyn ImageDecoder>>,
    texts: Arc<dyn RawDecoderTexts>,
    container_info_cache: BoundedLruCache<RawInfoKey, Arc<CachedRaw>>,
}

impl fmt::Debug for RawDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl RawDecoder {
    /// `new RawDecoder(innerDecoder)` with every other argument at its C# default.
    pub fn new(inner_decoder: Arc<dyn ImageDecoder>) -> Self {
        todo!()
    }

    /// The full C# constructor. The cache is built with [`CONTAINER_INFO_CACHE_CAPACITY`] and a size of 1 per entry
    /// (`BoundedLruCache::new` cannot fail for a positive constant; see the contract for the allowed `expect`).
    pub fn with_options(inner_decoder: Arc<dyn ImageDecoder>, options: RawDecoderOptions) -> Self {
        todo!()
    }

    /// C# `ReadInfo`. The sensor size of the container, else the size of the best embedded preview (selected with an
    /// unbounded box; the resolved previews are remembered). Neither known => `InvalidData`
    /// `"RAW file declares no image size (no sensor size and no sized preview): {path}"` localized `raw_corrupt`.
    /// Returns `ImageInfo::with_orientation(w, h, info.orientation)` (stored size; `ImageInfo::width` swaps for 5..=8).
    fn read_info_impl(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        todo!()
    }

    /// C# `Decode` (see the contract, "Decode composition"): EXIF (cached once complete), preview selection, the
    /// no-preview branch (`decode_without_preview`, W5-12b2) or the preview chain (`decode_preview_with_fallbacks`,
    /// W5-12b2), then the sensor size / downscaled / byte-count rules and the [`RawDecodedImage`] wrapper.
    fn decode_impl(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!()
    }

    /// C# `GetContainerInfo` (TOCTOU rules in the contract): stat BEFORE the open, open the header source (open errors
    /// propagate as they are), compare lengths, build the key, cache hit (re-stat) or probe + `find_reader` + `read` +
    /// re-stat + `set`. A header-source I/O failure (`RawHeaderError::InvalidData { io: Some(_) }`) surfaces as that
    /// I/O error (`DecodeError::from(io)`); a reader's malformed-container error is `InvalidData` localized `raw_corrupt`.
    fn get_container_info(&self, path: &str, priority: SourceReadPriority) -> Result<ContainerLookup, DecodeError> {
        todo!()
    }

    /// C# `RememberResolvedPreviews`: stores `resolved` in the entry of `key` when it differs from the cached previews
    /// and returns the entry now in effect. C# compares by reference; a resolved copy always has `header_resolved == true`
    /// where its original had `false`, so value equality (`!=` on the two `Vec`s) is the same test.
    fn remember_resolved_previews(
        &self,
        key: &RawInfoKey,
        cached: Arc<CachedRaw>,
        resolved: Vec<EmbeddedPreview>,
    ) -> Arc<CachedRaw> {
        todo!()
    }

    /// C# `DecodePreview`: cap check (`length > MAX_PREVIEW_BYTES` => `InvalidData` "Embedded preview is too large: {n}
    /// bytes (limit {max})."), identity re-check against `key` (mismatch => remove `key` from the cache, changed-while-
    /// reading), the range cache or [`Self::read_preview_range`], the Adobe RGB ICC injection, then the inner decode with
    /// `bytes = preview`, the request's box/orientation flag/priority and `source_orientation = info.orientation`.
    /// Every `InvalidData` raised before the inner decode that is not localized yet gets `raw_corrupt`. Returns the image
    /// and the bytes read from the SOURCE (0 for a range-cache hit). Called by the W5-12b2 chain.
    fn decode_preview(
        &self,
        request: &DecodeRequest,
        info: &RawContainerInfo,
        key: &RawInfoKey,
        preview: &EmbeddedPreview,
    ) -> Result<(Arc<dyn DecodedImageView>, i64), DecodeError> {
        todo!()
    }

    /// C# `ReadPreviewRange`: opens `path` through the source reader, length != `expected_length` => changed-while-reading;
    /// `count < 0 || offset < 0 || offset > length - count` => `Io(UnexpectedEof)` "Embedded preview range is outside
    /// {path}" (checked BEFORE allocating and BEFORE seeking to `offset`: a negative offset must give this error, not a
    /// seek `InvalidInput`); then seek to `offset` and read until full or a 0-byte read; short => `Io(UnexpectedEof)`
    /// "Truncated embedded preview in {path}". The stream is dropped before returning (INV-8: closed early).
    fn read_preview_range(
        &self,
        path: &str,
        offset: i64,
        count: i32,
        expected_length: i64,
        priority: SourceReadPriority,
    ) -> Result<Vec<u8>, DecodeError> {
        todo!()
    }

    /// C# `ChangedWhileReading(path)`: `DecodeError::Io(io::Error::other("RAW source changed while reading: {path}"))`
    /// with the `file_changed_while_reading(path)` sentence. Exactly `Io` (never `FileNotFound`, see the identity tests).
    fn changed_while_reading(&self, path: &str) -> DecodeError {
        todo!()
    }

    /// C# `ThrowIfStatChanged`: nothing for a path that had no physical file; else a fresh stat must exist with the same
    /// length and write ticks, otherwise changed-while-reading.
    fn throw_if_stat_changed(
        &self,
        path: &str,
        existed_before: bool,
        expected_length: i64,
        expected_last_write_ticks: i64,
    ) -> Result<(), DecodeError> {
        todo!()
    }

    /// `error.with_user_text(raw_corrupt)` unless `error.is_localized()` (C# `catch (InvalidDataException ex) when
    /// (!UserFacingError.IsLocalized(ex))`). Callers pass only `InvalidData` errors.
    fn localize_corrupt(&self, error: DecodeError) -> DecodeError {
        todo!()
    }

    /// A [`UserText`] closure over the injected texts (used for `raw_corrupt`, `raw_unsupported`, `raw_no_preview`).
    fn user_text(&self, text: fn(&dyn RawDecoderTexts) -> String) -> UserText {
        todo!()
    }
}

impl ImageDecoder for RawDecoder {
    /// C# `Decode(DecodeRequest)`: blank path (`ArgumentException.ThrowIfNullOrWhiteSpace(request.Path)`) =>
    /// `InvalidArgument`; else [`RawDecoder::decode_impl`].
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!()
    }

    /// C# `ReadInfo(string)`: blank path => `InvalidArgument`; else [`RawDecoder::read_info_impl`].
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        todo!()
    }
}

/// `new FileInfo(path)`: `Some((length, last_write_utc_ticks))` when a FILE exists at `path` (relative paths resolve
/// against the current directory like .NET), `None` for a missing path, a directory or a virtual/test path. Uses
/// `pr_core::io::physical::PhysicalFileSystem::get_file_stat` (the same stat W4-10a uses); `length` saturates at `i64::MAX`.
fn stat_file(path: &str) -> Option<(i64, i64)> {
    todo!()
}

/// Maps a header-source / reader / ICC error to the decoder's error: `InvalidData { io: None }` => `DecodeError::InvalidData`
/// with the same message; `InvalidData { io: Some(e) }` => `DecodeError::InvalidData` too (C# keeps the
/// `InvalidDataException` type outside `GetContainerInfo`; only `get_container_info` unwraps the inner I/O error itself);
/// `ArgumentOutOfRange` => `InvalidArgument` with the `Display` text; `Canceled` => `Canceled`.
fn header_error_to_decode_error(error: RawHeaderError) -> DecodeError {
    todo!()
}

/// Maps a `SourceBytesCache` error: `Io` => `DecodeError::Io` with the same kind and message (the `Arc<io::Error>` is not
/// unwrappable) and the cache's own UI sentence when it has one (`with_user_text`); `Argument`/`ArgumentOutOfRange` =>
/// `InvalidArgument` naming the parameter.
fn source_bytes_error_to_decode_error(error: SourceBytesError) -> DecodeError {
    todo!()
}

/// `ArgumentException.ThrowIfNullOrWhiteSpace(value, paramName)`: `true` for "" and all-whitespace (`char::is_whitespace`).
fn is_blank(value: &str) -> bool {
    todo!()
}

/// The `InvalidArgument` error of [`is_blank`]: .NET's "The value cannot be an empty string or composed entirely of
/// whitespace. (Parameter '{param}')".
fn blank_path_error(param: &str) -> DecodeError {
    todo!()
}
