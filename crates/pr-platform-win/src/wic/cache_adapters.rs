//! Trait adapters over the free functions of `wic::cache_image` (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151).
//!
//! `cache_image` (W4-11e) ports `WicImageEncoder` and `WicCacheImageReader` as free functions. The cache code of `pr-imaging` takes them as traits so that it needs no WIC:
//! `pr_imaging::cache::preview_file::PreviewPayloadCodec` (the `.pv4` payload of `PreviewImageService`, C# static `WicImageEncoder.EncodeJpeg` / `WicCacheImageReader.Decode`) and
//! `pr_imaging::cache::thumbnails::CacheImageReader` (the PNG disk entries of `ThumbnailCache`). Nothing implemented them in production (the tests of those crates use fakes), so the
//! shared service graph cannot build the preview service or the thumbnail cache. Both structs are stateless and `Send + Sync`; every method forwards, no new behaviour.
use std::io::Write;

use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::cache::thumbnails::{CacheImageInfo, CacheImageReader};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};

use super::cache_image::{self, CacheImageError, EncodeError};

/// C# `WicCacheImageReader.Decode` wraps a WIC COM failure of the `.pv4` payload in this message (the trait doc of `PreviewPayloadCodec::decode_jpeg`).
pub const PREVIEW_PAYLOAD_NOT_DECODED: &str = "Preview cache entry payload could not be decoded.";

/// `EncodeError` to the `io::Error` the payload writer expects: an `io::Error` the output produced keeps its kind, everything else is `Other` with the `Display` text.
pub fn encode_error_to_io(error: EncodeError) -> std::io::Error {
    match error {
        EncodeError::Io(io_error) => io_error,
        other => std::io::Error::other(other.to_string()),
    }
}

/// `CacheImageError` to the `.pv4` reader's error: WIC out-of-memory (and a pixel buffer the system refused) is [`PreviewCacheError::OutOfMemory`] (the entry is kept); a COM
/// failure is `InvalidData` with the C# wrapper message; the fixed `InvalidDataException` texts and the other pixel buffer errors are `InvalidData` with their own text.
pub fn preview_decode_error(error: CacheImageError) -> PreviewCacheError {
    match error {
        CacheImageError::InsufficientMemory { .. } | CacheImageError::Pixels(PixelBufferError::OutOfMemory { .. }) => {
            PreviewCacheError::OutOfMemory(error.to_string())
        }
        CacheImageError::InvalidData(text) => PreviewCacheError::InvalidData(text.to_owned()),
        CacheImageError::Com { .. } => PreviewCacheError::InvalidData(PREVIEW_PAYLOAD_NOT_DECODED.to_owned()),
        CacheImageError::Pixels(_) => PreviewCacheError::InvalidData(error.to_string()),
    }
}

/// `CacheImageError` to the thumbnail cache's `DecodeError`: `InvalidData` / `OutOfMemory` / `Com`, as the doc of `CacheImageReader::decode` lists.
pub fn cache_reader_error(error: CacheImageError) -> DecodeError {
    match error {
        CacheImageError::InvalidData(text) => DecodeError::InvalidData {
            message: text.to_owned(),
        },
        CacheImageError::Com { hresult } => DecodeError::Com {
            hresult,
            message: error.to_string(),
        },
        CacheImageError::InsufficientMemory { .. } | CacheImageError::Pixels(PixelBufferError::OutOfMemory { .. }) => {
            DecodeError::OutOfMemory {
                message: error.to_string(),
            }
        }
        CacheImageError::Pixels(_) => DecodeError::InvalidData {
            message: error.to_string(),
        },
    }
}

/// `PreviewPayloadCodec` over `cache_image::{encode_jpeg, decode}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WicPreviewPayloadCodec;

impl PreviewPayloadCodec for WicPreviewPayloadCodec {
    /// `cache_image::encode_jpeg(pixels, out, quality)`; `EncodeError` -> `io::Error` (kind `Other`, the `Display` text; an `io::Error` the writer produced keeps its kind).
    fn encode_jpeg(&self, pixels: &PixelBuffer, quality: i32, out: &mut dyn Write) -> std::io::Result<()> {
        cache_image::encode_jpeg(pixels, out, quality).map_err(encode_error_to_io)
    }

    /// `cache_image::decode(payload, Some(layout), DecodeBox::UNBOUNDED, expected_width, expected_height)`, pixels only. `CacheImageError::InsufficientMemory` ->
    /// `PreviewCacheError::OutOfMemory`; every other error -> `PreviewCacheError::InvalidData(<the C# message the doc of the trait lists>)`.
    fn decode_jpeg(
        &self,
        payload: &[u8],
        layout: PixelLayout,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<PixelBuffer, PreviewCacheError> {
        cache_image::decode(
            payload,
            Some(layout),
            DecodeBox::UNBOUNDED,
            expected_width,
            expected_height,
        )
        .map(|(pixels, _info)| pixels)
        .map_err(preview_decode_error)
    }
}

/// `CacheImageReader` over `cache_image::decode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WicCacheImageReader;

impl CacheImageReader for WicCacheImageReader {
    /// `cache_image::decode(encoded, layout, decode_box, expected_width, expected_height)`; the `WicCacheImageInfo` becomes `CacheImageInfo { source_width, source_height, downscaled }`;
    /// the error mapping is the one the doc of `CacheImageReader::decode` lists (`InvalidData` / `OutOfMemory` / `Com`).
    fn decode(
        &self,
        encoded: &[u8],
        layout: Option<PixelLayout>,
        decode_box: DecodeBox,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<(PixelBuffer, CacheImageInfo), DecodeError> {
        cache_image::decode(encoded, layout, decode_box, expected_width, expected_height)
            .map(|(pixels, info)| {
                (
                    pixels,
                    CacheImageInfo {
                        source_width: info.source_width,
                        source_height: info.source_height,
                        downscaled: info.downscaled,
                    },
                )
            })
            .map_err(cache_reader_error)
    }
}
