//! Port of `PhotoReview.Imaging.Raw/RawDecoder.cs` (C# baseline d92d6aae), W5-12b2 half: what happens when the chosen
//! embedded preview cannot be decoded, or the container has none.
//!
//! * [`RawDecoder::decode_preview_with_fallbacks`] (C# `DecodePreviewWithFallbacks`): the chosen preview, then up to
//!   [`MAX_PREVIEW_ATTEMPTS`] next-best previews (no native code needed), then the ORF thumbnail fallback (ORF + an
//!   unsupported preview only), then the full decode; the FIRST failure propagates when no step remains.
//! * [`RawDecoder::decode_without_preview`] (C# `DecodeWithoutPreview`): thumbnail fallback, then the full decode.
//! * the failure classification ([`is_recoverable_preview_failure`], [`is_corrupt_data_failure`]) and the pure half of
//!   the next-best rule ([`next_preview_candidates`]).
//!
//! Child module of `decoder` (W5-12b1): it reads the private fields of `RawDecoder` and calls `decode_preview`,
//! `stat_file`, `localize_*`; W5-12b1 calls the two `pub(super)` entry points.

#![allow(unused_variables, dead_code)] // D11: removed together with the last `todo!()`.

use std::sync::Arc;

use pr_imaging::decode_types::{DecodeRequest, DecodedImageView};
use pr_imaging::decoder_traits::{DecodeError, UserText};

use super::{PreviewOutcome, RawDecoder, RawInfoKey};
use crate::{EmbeddedPreview, RawContainerInfo, RawFormat, RawHeaderSource};

/// C# `MaxFallbackThumbnailBytes` (`32 << 20`): a LibRaw thumbnail longer than this (or empty) is refused as invalid.
pub const MAX_FALLBACK_THUMBNAIL_BYTES: usize = 32 << 20;

/// C# `MaxPreviewAttempts`: most previews one decode tries (the chosen one plus next-best candidates).
pub const MAX_PREVIEW_ATTEMPTS: usize = 4;

/// C# `MinUsefulFallbackLongSide`: a preview whose long side is below this is a thumbnail, not the photo.
pub const MIN_USEFUL_FALLBACK_LONG_SIDE: i32 = 1000;

/// `E_OUTOFMEMORY`.
pub const HRESULT_E_OUTOFMEMORY: u32 = 0x8007_000E;
/// `ERROR_NOT_ENOUGH_MEMORY` as an HRESULT.
pub const HRESULT_NOT_ENOUGH_MEMORY: u32 = 0x8007_0008;
/// `ERROR_NO_SYSTEM_RESOURCES` as an HRESULT.
pub const HRESULT_NO_SYSTEM_RESOURCES: u32 = 0x8007_05AA;
/// `ERROR_COMMITMENT_LIMIT` as an HRESULT.
pub const HRESULT_COMMITMENT_LIMIT: u32 = 0x8007_05AF;
/// `WINCODEC_ERR_BADIMAGE`.
pub const WINCODEC_ERR_BADIMAGE: u32 = 0x8898_2F60;
/// `WINCODEC_ERR_BADHEADER`.
pub const WINCODEC_ERR_BADHEADER: u32 = 0x8898_2F61;
/// `WINCODEC_ERR_UNKNOWNIMAGEFORMAT`.
pub const WINCODEC_ERR_UNKNOWNIMAGEFORMAT: u32 = 0x8898_2F07;

/// C# `IsResourceExhaustion(hresult)`: `(uint)hresult` is one of the four resource-exhaustion HRESULTs above.
pub fn is_resource_exhaustion(hresult: i32) -> bool {
    todo!()
}

/// C# `IsCorruptHResult(hresult)`: `(uint)hresult` is `BADIMAGE`, `BADHEADER` or `UNKNOWNIMAGEFORMAT`.
pub fn is_corrupt_hresult(hresult: i32) -> bool {
    todo!()
}

/// C# `IsRecoverablePreviewFailure(ex)`: corrupt, truncated or unsupported preview data. Order (C#):
/// 1. a directly wrapped COM failure ([`inner_com_hresult`]) that is resource exhaustion => `false`;
/// 2. `Com { hresult }` => `!is_resource_exhaustion(hresult)`;
/// 3. else `InvalidData`, `Format` (C# `FormatException` and WPF `FileFormatException`), `NotSupported` and
///    `MissingCodec` (a `NotSupportedException` subclass) => `true`;
/// 4. everything else (`Canceled`, `OutOfMemory`, `Io`, `FileNotFound`, `InvalidCast`, `Overflow`, `Busy`, ...) => `false`.
///
/// Classify on `error.kind()` (the `WithSourceBytes`/`Localized` wrappers are transparent, as C# `Exception.Data` is).
pub fn is_recoverable_preview_failure(error: &DecodeError) -> bool {
    todo!()
}

/// C# `IsCorruptDataFailure(ex)`: `InvalidData` or `Format`, or `Com` with a corrupt HRESULT, or a directly wrapped COM
/// failure ([`inner_com_hresult`]) with a corrupt HRESULT. `NotSupported` alone is NOT corrupt data.
pub fn is_corrupt_data_failure(error: &DecodeError) -> bool {
    todo!()
}

/// C# `ex.InnerException is COMException inner ? inner.HResult : (none)`. `DecodeError` has no inner-exception slot at the
/// baseline (W4-11c's `icc_transform_failed` keeps only the message): Open question 1 of the contract proposes one. Until
/// it lands this returns `None` and the three "wrapped in NotSupported" golden cases stay red (they are tagged
/// `requires: ["inner_com_hresult"]`).
fn inner_com_hresult(error: &DecodeError) -> Option<i32> {
    todo!()
}

/// C# `LongSide(p)`: `max(width, height)`.
fn long_side(preview: &EmbeddedPreview) -> i32 {
    todo!()
}

/// The pure part of C# `SelectNextPreview` (everything before the `PreviewSelector.SelectPreview` call):
/// `remaining` = JPEG previews with `length > 0` whose `offset` is not in `tried_offsets` (input order kept);
/// empty => `None`. When `largest_failed_long_side > 0`: `usable` = remaining previews that are not thumbnails
/// (`0 < long_side < MIN_USEFUL_FALLBACK_LONG_SIDE`; an unknown size 0 is never a thumbnail) OR whose long side is at
/// least `largest_failed_long_side`; `usable` non-empty => `Some(usable)`; else `failure_is_corrupt_data` =>
/// `Some(remaining)`; else `None`. When `largest_failed_long_side <= 0` => `Some(remaining)`.
/// Offsets identify previews (C# `triedOffsets.Contains(p.Offset)`): two previews at one offset count as one.
pub fn next_preview_candidates(
    previews: &[EmbeddedPreview],
    tried_offsets: &[i64],
    largest_failed_long_side: i32,
    failure_is_corrupt_data: bool,
) -> Option<Vec<EmbeddedPreview>> {
    todo!()
}

impl RawDecoder {
    /// C# `DecodePreviewWithFallbacks` (contract, "Fallback chain"). Loop while a candidate exists and fewer than
    /// [`MAX_PREVIEW_ATTEMPTS`] were tried: record its offset, `decode_preview`; success => `preview_bytes_read = read +
    /// failed bytes` (checked), `degraded_fallback = tried > 1`; a recoverable failure => keep the FIRST failure (by
    /// value) and whether the LAST one `is_not_supported()`, add `min(length, MAX_PREVIEW_BYTES)` to the failed bytes
    /// (checked), raise the largest failed long side, pick the next candidate ([`Self::select_next_preview`]) - also after
    /// the 4th failure, before the loop condition stops it (C# order: its header reads count in `total_bytes_read` and its
    /// I/O error propagates); a non-recoverable failure propagates at once. After the loop: ORF thumbnail (format ORF, fallback present, last
    /// failure unsupported; its recoverable failure is swallowed), then the full decode (`no_preview_decoder`; bytes =
    /// file length or 0), else `Err(first failure)`.
    pub(super) fn decode_preview_with_fallbacks(
        &self,
        request: &DecodeRequest,
        info: &RawContainerInfo,
        key: &RawInfoKey,
        header_source: &mut dyn RawHeaderSource,
        chosen: EmbeddedPreview,
    ) -> Result<PreviewOutcome, DecodeError> {
        todo!()
    }

    /// C# `SelectNextPreview`: [`next_preview_candidates`] then `preview_selector::select_preview(header_source,
    /// &candidates, request.decode_box(), info.orientation)` (no `resolved` list is kept: C# discards it here). A
    /// header-source I/O failure of that walk propagates (C# lets it escape the `catch` block).
    fn select_next_preview(
        &self,
        header_source: &mut dyn RawHeaderSource,
        info: &RawContainerInfo,
        request: &DecodeRequest,
        tried_offsets: &[i64],
        largest_failed_long_side: i32,
        failure_is_corrupt_data: bool,
    ) -> Result<Option<EmbeddedPreview>, DecodeError> {
        todo!()
    }

    /// C# `DecodeWithoutPreview` (called only when `preview_fallback` or `no_preview_decoder` exists): the thumbnail
    /// fallback (invalid size => `raw_no_preview`); a recoverable failure falls through to the full decode ONLY when a
    /// full decoder exists, else it propagates; then the full decode of the ORIGINAL request. The returned outcome has the
    /// counters in their final places (full decode: `full_decode_bytes_read` = file length or 0, thumbnail 0).
    pub(super) fn decode_without_preview(
        &self,
        request: &DecodeRequest,
        info: &RawContainerInfo,
    ) -> Result<PreviewOutcome, DecodeError> {
        todo!()
    }

    /// C# `DecodeFallbackThumbnail`: `preview_fallback.read_jpeg_thumbnail(path, format)`; length 0 or above
    /// [`MAX_FALLBACK_THUMBNAIL_BYTES`] => `InvalidData` "RAW fallback thumbnail size is invalid: {n} bytes." localized
    /// with `invalid_thumbnail_text`; else the inner decode of the request with `bytes = thumbnail` and
    /// `source_orientation = info.orientation` (box, orientation flag, priority from the caller's request). Returns the
    /// image and the thumbnail length.
    fn decode_fallback_thumbnail(
        &self,
        path: &str,
        format: RawFormat,
        request: &DecodeRequest,
        source_orientation: i32,
        invalid_thumbnail_text: UserText,
    ) -> Result<(Arc<dyn DecodedImageView>, i64), DecodeError> {
        todo!()
    }

    /// The full decode: `no_preview_decoder.decode(request)` (the caller's request unchanged) and the whole-file length
    /// (`stat_file`, 0 when there is no physical file). Callers guarantee the decoder exists; a missing one is
    /// `InvalidOperation` (never a panic).
    fn full_decode(&self, request: &DecodeRequest) -> Result<(Arc<dyn DecodedImageView>, i64), DecodeError> {
        todo!()
    }
}
