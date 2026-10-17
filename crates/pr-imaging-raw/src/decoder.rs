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

mod fallback;

pub use fallback::{
    MAX_FALLBACK_THUMBNAIL_BYTES, MAX_PREVIEW_ATTEMPTS, MIN_USEFUL_FALLBACK_LONG_SIDE, is_corrupt_data_failure,
    is_corrupt_hresult, is_recoverable_preview_failure, is_resource_exhaustion, next_preview_candidates,
};

use std::fmt;
use std::sync::Arc;

use crate::RawHeaderSource;
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
        "Corrupt or invalid RAW file.".to_owned()
    }

    fn raw_unsupported(&self) -> String {
        "Unsupported RAW container format.".to_owned()
    }

    fn raw_no_preview(&self) -> String {
        "This RAW file has no embedded preview, so it cannot be shown.".to_owned()
    }

    fn file_changed_while_reading(&self, path: &str) -> String {
        format!("The file changed while it was being read: {path}")
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
        f.debug_struct("RawDecoderOptions")
            .field("source_reader", &self.source_reader.is_some())
            .field("registry", &self.registry.is_some())
            .field("source_bytes_cache", &self.source_bytes_cache.is_some())
            .field("preview_fallback", &self.preview_fallback.is_some())
            .field("no_preview_decoder", &self.no_preview_decoder.is_some())
            .field("texts", &self.texts.is_some())
            .finish()
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
        self.inner.pixel_width()
    }

    fn pixel_height(&self) -> i32 {
        self.inner.pixel_height()
    }

    fn downscaled(&self) -> bool {
        self.downscaled
    }

    fn orientation(&self) -> i32 {
        self.orientation
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
        self.original_width
    }

    fn original_height(&self) -> i32 {
        self.original_height
    }

    fn exif(&self) -> Option<&ExifSummary> {
        self.exif.as_ref()
    }

    fn is_degraded_fallback(&self) -> bool {
        self.is_degraded_fallback
    }

    fn source_bytes_read(&self) -> Option<i64> {
        Some(self.source_bytes_read)
    }

    fn embedded_preview_size(&self) -> Option<(i32, i32)> {
        Some((self.embedded_preview_width, self.embedded_preview_height))
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
        f.debug_struct("RawDecoder")
            .field("container_info_cache", &self.container_info_cache.count())
            .finish_non_exhaustive()
    }
}

impl RawDecoder {
    /// `new RawDecoder(innerDecoder)` with every other argument at its C# default.
    pub fn new(inner_decoder: Arc<dyn ImageDecoder>) -> Self {
        Self::with_options(inner_decoder, RawDecoderOptions::default())
    }

    /// The full C# constructor. The cache is built with [`CONTAINER_INFO_CACHE_CAPACITY`] and a size of 1 per entry
    /// (`BoundedLruCache::new` cannot fail for a positive constant; see the contract for the allowed `expect`).
    pub fn with_options(inner_decoder: Arc<dyn ImageDecoder>, options: RawDecoderOptions) -> Self {
        let source_reader = options
            .source_reader
            .unwrap_or_else(|| Arc::new(pr_core::abstractions::PhysicalSourceReader));
        let registry = options
            .registry
            .unwrap_or_else(|| Arc::new(RawContainerReaderRegistry::new()));
        let texts = options.texts.unwrap_or_else(|| Arc::new(EnglishRawDecoderTexts));
        Self {
            inner_decoder,
            source_reader,
            registry,
            source_bytes_cache: options.source_bytes_cache,
            preview_fallback: options.preview_fallback,
            no_preview_decoder: options.no_preview_decoder,
            texts,
            container_info_cache: BoundedLruCache::new(CONTAINER_INFO_CACHE_CAPACITY, |_| 1)
                .expect("positive constant capacity"),
        }
    }

    /// C# `ReadInfo`. The sensor size of the container, else the size of the best embedded preview (selected with an
    /// unbounded box; the resolved previews are remembered). Neither known => `InvalidData`
    /// `"RAW file declares no image size (no sensor size and no sized preview): {path}"` localized `raw_corrupt`.
    /// Returns `ImageInfo::with_orientation(w, h, info.orientation)` (stored size; `ImageInfo::width` swaps for 5..=8).
    fn read_info_impl(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        let mut lookup = self.get_container_info(path, SourceReadPriority::Viewer)?;
        let mut info = lookup.cached.info.clone();
        let (mut width, mut height) = (info.sensor_width, info.sensor_height);
        if width <= 0 || height <= 0 {
            let selection = crate::preview_selector::select_preview_resolved(
                &mut lookup.header_source,
                &info.previews,
                pr_imaging::decode_types::DecodeBox::UNBOUNDED,
                info.orientation,
            )
            .map_err(header_error_to_decode_error)?;
            lookup.cached = self.remember_resolved_previews(&lookup.key, lookup.cached, selection.resolved_previews);
            info = lookup.cached.info.clone();
            if let Some(preview) = selection.chosen.filter(|p| p.width > 0 && p.height > 0) {
                width = preview.width;
                height = preview.height;
            }
        }
        if width <= 0 || height <= 0 {
            return Err(DecodeError::InvalidData {
                message: format!("RAW file declares no image size (no sensor size and no sized preview): {path}"),
            }
            .with_user_text(self.user_text(|t| t.raw_corrupt())));
        }
        Ok(ImageInfo::with_orientation(width, height, info.orientation))
    }

    /// C# `Decode` (see the contract, "Decode composition"): EXIF (cached once complete), preview selection, the
    /// no-preview branch (`decode_without_preview`, W5-12b2) or the preview chain (`decode_preview_with_fallbacks`,
    /// W5-12b2), then the sensor size / downscaled / byte-count rules and the [`RawDecodedImage`] wrapper.
    fn decode_impl(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let mut lookup = self.get_container_info(request.path.to_string_lossy().as_ref(), request.priority)?;
        let mut info = lookup.cached.info.clone();
        let exif = if lookup.cached.exif_read {
            lookup.cached.exif.clone()
        } else {
            let read = crate::raw_exif::try_read_exif(&mut lookup.header_source, &info);
            if read.complete {
                let cached = Arc::new(CachedRaw {
                    info: info.clone(),
                    exif: read.summary.clone(),
                    exif_read: true,
                });
                self.container_info_cache.set(lookup.key.clone(), cached.clone());
                lookup.cached = cached;
            }
            read.summary
        };
        let selection = crate::preview_selector::select_preview_resolved(
            &mut lookup.header_source,
            &info.previews,
            request.decode_box(),
            info.orientation,
        )
        .map_err(header_error_to_decode_error)?;
        lookup.cached = self.remember_resolved_previews(&lookup.key, lookup.cached, selection.resolved_previews);
        info = lookup.cached.info.clone();
        let outcome = if let Some(preview) = selection.chosen.filter(|p| p.length > 0) {
            self.decode_preview_with_fallbacks(request, &info, &lookup.key, &mut lookup.header_source, preview)?
        } else {
            if self.preview_fallback.is_none() && self.no_preview_decoder.is_none() {
                return Err(DecodeError::InvalidData {
                    message: format!("No embedded preview found in RAW file: {}", request.path.display()),
                }
                .with_user_text(self.user_text(|t| t.raw_no_preview())));
            }
            self.decode_without_preview(request, &info)?
        };
        let mut sw = info.sensor_width;
        let mut sh = info.sensor_height;
        if sw <= 0 || sh <= 0 {
            sw = outcome.image.original_width();
            sh = outcome.image.original_height();
        } else if request.apply_orientation && matches!(info.orientation, 5..=8) {
            std::mem::swap(&mut sw, &mut sh);
        }
        let tiny = outcome.from_jpeg_preview
            && outcome.image.original_width().max(outcome.image.original_height()) <= MAX_THUMBNAIL_LONG_SIDE;
        let downscaled =
            outcome.image.downscaled() || tiny || outcome.image.pixel_width() < sw || outcome.image.pixel_height() < sh;
        let source_bytes_read = if outcome.from_jpeg_preview || outcome.full_decode_bytes_read <= 0 {
            lookup
                .header_source
                .total_bytes_read()
                .checked_add(outcome.preview_bytes_read)
                .and_then(|n| n.checked_add(outcome.fallback_thumbnail_bytes_read))
        } else {
            outcome.full_decode_bytes_read.checked_add(outcome.preview_bytes_read)
        }
        .ok_or_else(|| DecodeError::Overflow {
            message: "Arithmetic operation resulted in an overflow.".to_owned(),
        })?;
        let image = Arc::new(RawDecodedImage {
            inner: outcome.image.clone(),
            original_width: sw,
            original_height: sh,
            downscaled,
            orientation: info.orientation,
            exif: exif.or_else(|| outcome.image.exif().cloned()),
            actual_backend: outcome.image.actual_backend(),
            source_bytes_read,
            embedded_preview_width: if outcome.from_jpeg_preview {
                outcome.image.original_width()
            } else {
                0
            },
            embedded_preview_height: if outcome.from_jpeg_preview {
                outcome.image.original_height()
            } else {
                0
            },
            is_degraded_fallback: outcome.degraded_fallback || outcome.image.is_degraded_fallback(),
        });
        Ok(image)
    }

    /// C# `GetContainerInfo` (TOCTOU rules in the contract): stat BEFORE the open, open the header source (open errors
    /// propagate as they are), compare lengths, build the key, cache hit (re-stat) or probe + `find_reader` + `read` +
    /// re-stat + `set`. A header-source I/O failure (`RawHeaderError::InvalidData { io: Some(_) }`) surfaces as that
    /// I/O error (`DecodeError::from(io)`); a reader's malformed-container error is `InvalidData` localized `raw_corrupt`.
    fn get_container_info(&self, path: &str, priority: SourceReadPriority) -> Result<ContainerLookup, DecodeError> {
        let stat_before = stat_file(path);
        let mut header_source =
            SourceRawHeaderSource::open(std::path::Path::new(path), self.source_reader.as_ref(), priority)
                .map_err(DecodeError::Io)?;
        let length = header_source.length();
        if let Some((stat_len, _)) = stat_before {
            if stat_len != length {
                return Err(self.changed_while_reading(path));
            }
        }
        let key = RawInfoKey {
            path: pr_imaging::cache_key::normalize_key_path(path).map_err(|error| DecodeError::InvalidArgument {
                message: error.to_string(),
            })?,
            length,
            last_write_utc_ticks: stat_before.map_or(0, |(_, ticks)| ticks),
        };
        if let Some(cached) = self.container_info_cache.try_get(&key) {
            self.throw_if_stat_changed(path, stat_before.is_some(), length, key.last_write_utc_ticks)?;
            return Ok(ContainerLookup {
                cached,
                header_source,
                key,
            });
        }
        let probe = header_source
            .read(0, crate::formats::limits::initial_probe_length(length))
            .map_err(|e| match e {
                RawHeaderError::InvalidData { io: Some(io), .. } => DecodeError::Io(io),
                other => header_error_to_decode_error(other),
            })?
            .to_vec();
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| format!(".{s}"))
            .unwrap_or_default();
        let reader = self
            .registry
            .find_reader(&probe, &extension)
            .ok_or_else(|| DecodeError::NotSupported {
                message: format!("{UNSUPPORTED_FORMAT_PREFIX}{extension}"),
                user_text: Some(self.user_text(|t| t.raw_unsupported())),
            })?;
        let info = reader
            .read(&mut header_source, &pr_core::sync::CancelToken::new())
            .map_err(|e| match e {
                RawHeaderError::InvalidData { io: Some(io), .. } => DecodeError::Io(io),
                other => self.localize_corrupt(header_error_to_decode_error(other)),
            })?;
        self.throw_if_stat_changed(path, stat_before.is_some(), length, key.last_write_utc_ticks)?;
        let cached = Arc::new(CachedRaw {
            info,
            exif: None,
            exif_read: false,
        });
        self.container_info_cache.set(key.clone(), cached.clone());
        Ok(ContainerLookup {
            cached,
            header_source,
            key,
        })
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
        if cached.info.previews == resolved {
            return cached;
        }
        let next = Arc::new(CachedRaw {
            info: RawContainerInfo {
                previews: resolved,
                ..cached.info.clone()
            },
            exif: cached.exif.clone(),
            exif_read: cached.exif_read,
        });
        self.container_info_cache.set(key.clone(), next.clone());
        next
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
        const MAX_BYTES: i64 = crate::formats::limits::MAX_PREVIEW_BYTES as i64;
        if preview.length > MAX_BYTES {
            return Err(self.localize_corrupt(DecodeError::InvalidData {
                message: format!(
                    "Embedded preview is too large: {} bytes (limit {MAX_BYTES}).",
                    preview.length
                ),
            }));
        }
        let file_stat = stat_file(request.path.to_string_lossy().as_ref());
        if file_stat.is_some_and(|(len, ticks)| len != key.length || ticks != key.last_write_utc_ticks) {
            self.container_info_cache.remove(key);
            return Err(self.changed_while_reading(request.path.to_string_lossy().as_ref()));
        }
        let count = i32::try_from(preview.length).map_err(|_| {
            self.localize_corrupt(DecodeError::InvalidData {
                message: "Embedded preview size is invalid.".to_owned(),
            })
        })?;
        let path = request.path.to_string_lossy().to_string();
        let (bytes, read) = if let (Some(cache), Some((file_len, file_ticks))) = (&self.source_bytes_cache, file_stat)
            && cache.can_cache_range(i64::from(count))
        {
            match cache
                .try_get_range(std::path::Path::new(&path), file_len, file_ticks, preview.offset, count)
                .map_err(source_bytes_error_to_decode_error)?
            {
                Some(bytes) => (bytes, 0),
                _ => (
                    cache
                        .get_or_read_range(
                            std::path::Path::new(&path),
                            file_len,
                            file_ticks,
                            preview.offset,
                            count,
                            request.priority,
                        )
                        .map_err(source_bytes_error_to_decode_error)?,
                    i64::from(count),
                ),
            }
        } else {
            (
                Arc::<[u8]>::from(self.read_preview_range(
                    &path,
                    preview.offset,
                    count,
                    key.length,
                    request.priority,
                )?),
                i64::from(count),
            )
        };
        let bytes: Arc<[u8]> = if preview.color_space == crate::PreviewColorSpace::AdobeRgb {
            Arc::from(
                crate::registry::ensure_adobe_rgb_profile(bytes.to_vec())
                    .map_err(|e| self.localize_corrupt(header_error_to_decode_error(e)))?,
            )
        } else {
            bytes
        };
        let inner_request = DecodeRequest {
            path: request.path.clone(),
            target_width: request.target_width,
            target_height: request.target_height,
            apply_orientation: request.apply_orientation,
            bytes: Some(bytes),
            priority: request.priority,
            source_orientation: Some(info.orientation),
        };
        self.inner_decoder.decode(&inner_request).map(|img| (img, read))
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
        if count < 0 || offset < 0 {
            return Err(DecodeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("Embedded preview range is outside {path}"),
            )));
        }
        let mut stream = self
            .source_reader
            .open_source(std::path::Path::new(path), priority, 1024 * 1024)
            .map_err(DecodeError::Io)?;
        let length = stream.seek(std::io::SeekFrom::End(0)).map_err(DecodeError::Io)?;
        if i64::try_from(length).ok() != Some(expected_length) {
            return Err(self.changed_while_reading(path));
        }
        let count64 = i64::from(count);
        if count64 > expected_length || offset > expected_length - count64 {
            return Err(DecodeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("Embedded preview range is outside {path}"),
            )));
        }
        let size = usize::try_from(count).map_err(|_| DecodeError::Overflow {
            message: "preview size overflow".into(),
        })?;
        stream
            .seek(std::io::SeekFrom::Start(offset as u64))
            .map_err(DecodeError::Io)?;
        let mut bytes = vec![0; size];
        let mut at = 0;
        while at < size {
            match std::io::Read::read(&mut stream, &mut bytes[at..]) {
                Ok(0) => break,
                Ok(n) => at += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(DecodeError::Io(e)),
            }
        }
        if at != size {
            return Err(DecodeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("Truncated embedded preview in {path}"),
            )));
        }
        Ok(bytes)
    }

    /// C# `ChangedWhileReading(path)`: `DecodeError::Io(io::Error::other("RAW source changed while reading: {path}"))`
    /// with the `file_changed_while_reading(path)` sentence. Exactly `Io` (never `FileNotFound`, see the identity tests).
    fn changed_while_reading(&self, path: &str) -> DecodeError {
        let text = Arc::clone(&self.texts);
        let path = path.to_owned();
        DecodeError::Io(std::io::Error::other(format!(
            "RAW source changed while reading: {path}"
        )))
        .with_user_text(Arc::new(move || text.file_changed_while_reading(&path)))
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
        if !existed_before {
            return Ok(());
        }
        match stat_file(path) {
            Some((length, ticks)) if length == expected_length && ticks == expected_last_write_ticks => Ok(()),
            _ => Err(self.changed_while_reading(path)),
        }
    }

    /// `error.with_user_text(raw_corrupt)` unless `error.is_localized()` (C# `catch (InvalidDataException ex) when
    /// (!UserFacingError.IsLocalized(ex))`). Callers pass only `InvalidData` errors.
    fn localize_corrupt(&self, error: DecodeError) -> DecodeError {
        if error.is_localized() {
            error
        } else {
            error.with_user_text(self.user_text(|t| t.raw_corrupt()))
        }
    }

    /// A [`UserText`] closure over the injected texts (used for `raw_corrupt`, `raw_unsupported`, `raw_no_preview`).
    fn user_text(&self, text: fn(&dyn RawDecoderTexts) -> String) -> UserText {
        let texts = Arc::clone(&self.texts);
        Arc::new(move || text(texts.as_ref()))
    }
}

impl ImageDecoder for RawDecoder {
    /// C# `Decode(DecodeRequest)`: blank path (`ArgumentException.ThrowIfNullOrWhiteSpace(request.Path)`) =>
    /// `InvalidArgument`; else [`RawDecoder::decode_impl`].
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if is_blank(&request.path.to_string_lossy()) {
            return Err(blank_path_error("path"));
        }
        self.decode_impl(request)
    }

    /// C# `ReadInfo(string)`: blank path => `InvalidArgument`; else [`RawDecoder::read_info_impl`].
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        if is_blank(path) {
            return Err(blank_path_error("path"));
        }
        self.read_info_impl(path)
    }
}

/// `new FileInfo(path)`: `Some((length, last_write_utc_ticks))` when a FILE exists at `path` (relative paths resolve
/// against the current directory like .NET), `None` for a missing path, a directory or a virtual/test path. Uses
/// `pr_core::io::physical::PhysicalFileSystem::get_file_stat` (the same stat W4-10a uses); `length` saturates at `i64::MAX`.
fn stat_file(path: &str) -> Option<(i64, i64)> {
    use pr_core::abstractions::FileSystem;
    let stat = pr_core::io::physical::PhysicalFileSystem.get_file_stat(std::path::Path::new(path))?;
    Some((
        i64::try_from(stat.length).unwrap_or(i64::MAX),
        stat.last_write_utc.ticks(),
    ))
}

/// Maps a header-source / reader / ICC error to the decoder's error: `InvalidData { io: None }` => `DecodeError::InvalidData`
/// with the same message; `InvalidData { io: Some(e) }` => `DecodeError::InvalidData` too (C# keeps the
/// `InvalidDataException` type outside `GetContainerInfo`; only `get_container_info` unwraps the inner I/O error itself);
/// `ArgumentOutOfRange` => `InvalidArgument` with the `Display` text; `Canceled` => `Canceled`.
fn header_error_to_decode_error(error: RawHeaderError) -> DecodeError {
    match error {
        RawHeaderError::InvalidData { message, .. } => DecodeError::InvalidData { message },
        RawHeaderError::ArgumentOutOfRange { param, message } => DecodeError::InvalidArgument {
            message: format!("{message} (Parameter '{param}')"),
        },
        RawHeaderError::Canceled => DecodeError::Canceled,
    }
}

/// Maps a `SourceBytesCache` error: `Io` => `DecodeError::Io` with the same kind and message (the `Arc<io::Error>` is not
/// unwrappable) and the cache's own UI sentence when it has one (`with_user_text`); `Argument`/`ArgumentOutOfRange` =>
/// `InvalidArgument` naming the parameter.
fn source_bytes_error_to_decode_error(error: SourceBytesError) -> DecodeError {
    let user_text = error.user_text();
    match error {
        SourceBytesError::Io(e) => {
            let mut decoded = DecodeError::Io(std::io::Error::new(e.kind(), e.to_string()));
            if let Some(text) = user_text {
                decoded = decoded.with_user_text(text);
            }
            decoded
        }
        other => DecodeError::InvalidArgument {
            message: other.to_string(),
        },
    }
}

/// `ArgumentException.ThrowIfNullOrWhiteSpace(value, paramName)`: `true` for "" and all-whitespace (`char::is_whitespace`).
fn is_blank(value: &str) -> bool {
    value.is_empty() || value.chars().all(char::is_whitespace)
}

/// The `InvalidArgument` error of [`is_blank`]: .NET's "The value cannot be an empty string or composed entirely of
/// whitespace. (Parameter '{param}')".
fn blank_path_error(param: &str) -> DecodeError {
    DecodeError::InvalidArgument {
        message: format!(
            "The value cannot be an empty string or composed entirely of whitespace. (Parameter '{param}')"
        ),
    }
}
