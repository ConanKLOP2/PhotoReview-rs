//! Trait adapters over the free functions of `wic::cache_image` (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151).
//!
//! `cache_image` (W4-11e) ports `WicImageEncoder` and `WicCacheImageReader` as free functions. The cache code of `pr-imaging` takes them as traits so that it needs no WIC:
//! `pr_imaging::cache::preview_file::PreviewPayloadCodec` (the `.pv4` payload of `PreviewImageService`, C# static `WicImageEncoder.EncodeJpeg` / `WicCacheImageReader.Decode`) and
//! `pr_imaging::cache::thumbnails::CacheImageReader` (the PNG disk entries of `ThumbnailCache`). Nothing implemented them in production (the tests of those crates use fakes), so the
//! shared service graph cannot build the preview service or the thumbnail cache. Both structs are stateless and `Send + Sync`; every method forwards, no new behaviour.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::io::Write;

use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::cache::thumbnails::{CacheImageInfo, CacheImageReader};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};

/// `PreviewPayloadCodec` over `cache_image::{encode_jpeg, decode}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WicPreviewPayloadCodec;

impl PreviewPayloadCodec for WicPreviewPayloadCodec {
    /// `cache_image::encode_jpeg(pixels, out, quality)`; `EncodeError` -> `io::Error` (kind `Other`, the `Display` text; an `io::Error` the writer produced keeps its kind).
    fn encode_jpeg(&self, pixels: &PixelBuffer, quality: i32, out: &mut dyn Write) -> std::io::Result<()> {
        todo!()
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
        todo!()
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
        todo!()
    }
}
