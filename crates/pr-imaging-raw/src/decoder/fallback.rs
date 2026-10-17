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
    matches!(
        hresult as u32,
        HRESULT_E_OUTOFMEMORY | HRESULT_NOT_ENOUGH_MEMORY | HRESULT_NO_SYSTEM_RESOURCES | HRESULT_COMMITMENT_LIMIT
    )
}

/// C# `IsCorruptHResult(hresult)`: `(uint)hresult` is `BADIMAGE`, `BADHEADER` or `UNKNOWNIMAGEFORMAT`.
pub fn is_corrupt_hresult(hresult: i32) -> bool {
    matches!(
        hresult as u32,
        WINCODEC_ERR_BADIMAGE | WINCODEC_ERR_BADHEADER | WINCODEC_ERR_UNKNOWNIMAGEFORMAT
    )
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
    if inner_com_hresult(error).is_some_and(is_resource_exhaustion) {
        return false;
    }
    match error.kind() {
        DecodeError::Com { hresult, .. } => !is_resource_exhaustion(*hresult),
        DecodeError::InvalidData { .. }
        | DecodeError::Format { .. }
        | DecodeError::NotSupported { .. }
        | DecodeError::MissingCodec { .. } => true,
        _ => false,
    }
}

/// C# `IsCorruptDataFailure(ex)`: `InvalidData` or `Format`, or `Com` with a corrupt HRESULT, or a directly wrapped COM
/// failure ([`inner_com_hresult`]) with a corrupt HRESULT. `NotSupported` alone is NOT corrupt data.
pub fn is_corrupt_data_failure(error: &DecodeError) -> bool {
    let nested = inner_com_hresult(error).is_some_and(is_corrupt_hresult);
    nested
        || match error.kind() {
            DecodeError::InvalidData { .. } | DecodeError::Format { .. } => true,
            DecodeError::Com { hresult, .. } => is_corrupt_hresult(*hresult),
            _ => false,
        }
}

/// C# `ex.InnerException is COMException inner ? inner.HResult : (none)`. W5 A8 / D59 stores the direct COM HRESULT on
/// `DecodeError::NotSupported`; wrappers are transparent through `DecodeError::inner_hresult()`.
fn inner_com_hresult(error: &DecodeError) -> Option<i32> {
    error.inner_hresult()
}

/// C# `LongSide(p)`: `max(width, height)`.
fn long_side(preview: &EmbeddedPreview) -> i32 {
    preview.width.max(preview.height)
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
    let remaining: Vec<_> = previews
        .iter()
        .filter(|preview| {
            preview.kind == crate::EmbeddedPreviewKind::Jpeg
                && preview.length > 0
                && !tried_offsets.contains(&preview.offset)
        })
        .cloned()
        .collect();
    if remaining.is_empty() {
        return None;
    }
    if largest_failed_long_side <= 0 {
        return Some(remaining);
    }
    let usable: Vec<_> = remaining
        .iter()
        .filter(|preview| {
            let long = long_side(preview);
            !(long > 0 && long < MIN_USEFUL_FALLBACK_LONG_SIDE) || long >= largest_failed_long_side
        })
        .cloned()
        .collect();
    if !usable.is_empty() {
        Some(usable)
    } else if failure_is_corrupt_data {
        Some(remaining)
    } else {
        None
    }
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
        let mut tried_offsets = Vec::new();
        let mut first_failure = None;
        let mut last_is_not_supported = false;
        let mut failed_bytes = 0i64;
        let mut largest_failed_long_side = 0;
        let mut current = Some(chosen);

        while let Some(preview) = current.take() {
            if tried_offsets.len() >= MAX_PREVIEW_ATTEMPTS {
                break;
            }
            tried_offsets.push(preview.offset);
            match self.decode_preview(request, info, key, &preview) {
                Ok((image, read)) => {
                    let preview_bytes_read = read.checked_add(failed_bytes).ok_or_else(byte_count_overflow)?;
                    return Ok(PreviewOutcome {
                        image,
                        preview_bytes_read,
                        fallback_thumbnail_bytes_read: 0,
                        full_decode_bytes_read: 0,
                        from_jpeg_preview: true,
                        degraded_fallback: tried_offsets.len() > 1,
                    });
                }
                Err(error) if is_recoverable_preview_failure(&error) => {
                    let is_not_supported = error.is_not_supported();
                    let is_corrupt = is_corrupt_data_failure(&error);
                    let failed_long_side = info
                        .previews
                        .iter()
                        .find(|candidate| candidate.offset == preview.offset)
                        .map_or_else(|| long_side(&preview), long_side);
                    if first_failure.is_none() {
                        first_failure = Some(error);
                    }
                    last_is_not_supported = is_not_supported;
                    failed_bytes = failed_bytes
                        .checked_add(preview.length.min(i64::from(crate::formats::limits::MAX_PREVIEW_BYTES)))
                        .ok_or_else(byte_count_overflow)?;
                    largest_failed_long_side = largest_failed_long_side.max(failed_long_side);
                    // C# selects the next candidate after every recoverable failure, including the fourth.
                    current = self.select_next_preview(
                        header_source,
                        info,
                        request,
                        &tried_offsets,
                        largest_failed_long_side,
                        is_corrupt,
                    )?;
                }
                Err(error) => return Err(error),
            }
        }

        if info.format == RawFormat::Orf && self.preview_fallback.is_some() && last_is_not_supported {
            match self.decode_fallback_thumbnail(
                &request.path.to_string_lossy(),
                info.format,
                request,
                info.orientation,
                self.user_text(|texts| texts.raw_corrupt()),
            ) {
                Ok((image, bytes)) => {
                    return Ok(PreviewOutcome {
                        image,
                        preview_bytes_read: failed_bytes,
                        fallback_thumbnail_bytes_read: bytes,
                        full_decode_bytes_read: 0,
                        from_jpeg_preview: true,
                        degraded_fallback: true,
                    });
                }
                Err(error) if is_recoverable_preview_failure(&error) => {}
                Err(error) => return Err(error),
            }
        }

        if self.no_preview_decoder.is_some() {
            let (image, full_decode_bytes_read) = self.full_decode(request)?;
            return Ok(PreviewOutcome {
                image,
                preview_bytes_read: failed_bytes,
                fallback_thumbnail_bytes_read: 0,
                full_decode_bytes_read,
                from_jpeg_preview: false,
                degraded_fallback: false,
            });
        }
        match first_failure {
            Some(error) => Err(error),
            None => Err(DecodeError::InvalidOperation {
                message: "RAW preview fallback chain had no candidate or failure.".to_owned(),
            }),
        }
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
        let Some(candidates) = next_preview_candidates(
            &info.previews,
            tried_offsets,
            largest_failed_long_side,
            failure_is_corrupt_data,
        ) else {
            return Ok(None);
        };
        crate::preview_selector::select_preview(header_source, &candidates, request.decode_box(), info.orientation)
            .map_err(super::header_error_to_decode_error)
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
        if self.preview_fallback.is_some() {
            match self.decode_fallback_thumbnail(
                &request.path.to_string_lossy(),
                info.format,
                request,
                info.orientation,
                self.user_text(|texts| texts.raw_no_preview()),
            ) {
                Ok((image, bytes)) => {
                    return Ok(PreviewOutcome {
                        image,
                        preview_bytes_read: 0,
                        fallback_thumbnail_bytes_read: bytes,
                        full_decode_bytes_read: 0,
                        from_jpeg_preview: true,
                        degraded_fallback: false,
                    });
                }
                Err(error) if self.no_preview_decoder.is_some() && is_recoverable_preview_failure(&error) => {}
                Err(error) => return Err(error),
            }
        }
        let (image, full_decode_bytes_read) = self.full_decode(request)?;
        Ok(PreviewOutcome {
            image,
            preview_bytes_read: 0,
            fallback_thumbnail_bytes_read: 0,
            full_decode_bytes_read,
            from_jpeg_preview: false,
            degraded_fallback: false,
        })
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
        let fallback = self
            .preview_fallback
            .as_ref()
            .ok_or_else(|| DecodeError::InvalidOperation {
                message: "RAW thumbnail fallback was requested but is unavailable.".to_owned(),
            })?;
        let bytes = fallback.read_jpeg_thumbnail(path, format)?;
        if bytes.is_empty() || bytes.len() > MAX_FALLBACK_THUMBNAIL_BYTES {
            return Err(DecodeError::InvalidData {
                message: format!("RAW fallback thumbnail size is invalid: {} bytes.", bytes.len()),
            }
            .with_user_text(invalid_thumbnail_text));
        }
        let count = i64::try_from(bytes.len()).map_err(|_| byte_count_overflow())?;
        let inner_request = DecodeRequest {
            bytes: Some(Arc::from(bytes)),
            source_orientation: Some(source_orientation),
            ..request.clone()
        };
        self.inner_decoder.decode(&inner_request).map(|image| (image, count))
    }

    /// The full decode: `no_preview_decoder.decode(request)` (the caller's request unchanged) and the whole-file length
    /// (`stat_file`, 0 when there is no physical file). Callers guarantee the decoder exists; a missing one is
    /// `InvalidOperation` (never a panic).
    fn full_decode(&self, request: &DecodeRequest) -> Result<(Arc<dyn DecodedImageView>, i64), DecodeError> {
        let decoder = self
            .no_preview_decoder
            .as_ref()
            .ok_or_else(|| DecodeError::InvalidOperation {
                message: "RAW full decoder was requested but is unavailable.".to_owned(),
            })?;
        let image = decoder.decode(request)?;
        let bytes = super::stat_file(&request.path.to_string_lossy()).map_or(0, |(length, _)| length);
        Ok((image, bytes))
    }
}

fn byte_count_overflow() -> DecodeError {
    DecodeError::Overflow {
        message: "Arithmetic operation resulted in an overflow.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EmbeddedPreviewKind, PreviewColorSpace};

    fn preview(offset: i64, width: i32, height: i32) -> EmbeddedPreview {
        EmbeddedPreview::new(
            0,
            offset,
            10,
            EmbeddedPreviewKind::Jpeg,
            width,
            height,
            PreviewColorSpace::Srgb,
        )
    }

    #[test]
    fn recoverable_preview_failure_matches_raw_decoder_whitelist() {
        assert!(is_recoverable_preview_failure(&DecodeError::InvalidData {
            message: "bad".into()
        }));
        assert!(is_recoverable_preview_failure(&DecodeError::missing_codec()));
        assert!(!is_recoverable_preview_failure(&DecodeError::Canceled));
        assert!(!is_recoverable_preview_failure(&DecodeError::OutOfMemory {
            message: "oom".into()
        }));
        assert!(!is_recoverable_preview_failure(&DecodeError::Io(
            std::io::Error::other("io")
        )));
        assert!(!is_recoverable_preview_failure(&DecodeError::Com {
            hresult: HRESULT_E_OUTOFMEMORY as i32,
            message: "oom".into(),
        }));
    }

    #[test]
    fn next_preview_candidates_filters_tried_and_tiny_unless_corrupt() {
        let previews = [preview(1, 2400, 1600), preview(2, 320, 240), preview(3, 2000, 1000)];
        let usable = next_preview_candidates(&previews, &[1], 2400, false).expect("remaining usable");
        assert_eq!(usable.iter().map(|p| p.offset).collect::<Vec<_>>(), [3]);
        let corrupt = next_preview_candidates(&previews, &[1], 2400, true).expect("corrupt permits thumbnail");
        assert_eq!(corrupt.iter().map(|p| p.offset).collect::<Vec<_>>(), [3]);
        let tiny_only = next_preview_candidates(&previews[..2], &[1], 2400, true).expect("corrupt permits thumbnail");
        assert_eq!(tiny_only.iter().map(|p| p.offset).collect::<Vec<_>>(), [2]);
        assert!(next_preview_candidates(&previews[..1], &[1], 2400, true).is_none());
    }
}
