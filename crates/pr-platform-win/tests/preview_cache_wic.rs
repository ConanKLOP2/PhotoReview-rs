//! Red native tests for W4-11e that other drafts assign to this unit (contract "Native tests that OTHER drafts assign to this
//! unit", open question 8). All `#[ignore = "native: needs WIC"]`; run with `-- --ignored` on Windows.
//!
//! The C# tests go through `PreviewCacheFile`, `DiskCacheStore` and `ThumbnailCache`, which belong to W4-08b, W4-09 and W4-10b.
//! Their Rust twins here call the WIC seam of this unit directly (`encode_png`, `encode_jpeg`, `decode`), which is what the
//! `PreviewPayloadCodec` / `PngEncoder` / `CacheImageReader` adapters of contract note 10 forward to. Tests that need the
//! file-format layer are documented skips:
//! * `PreviewCacheCrossVersionTests.CommittedOldFixture_ReadsWithNewCode_IdenticalToOldReader` (pixel half),
//!   `NewWriter_ReproducesCommittedOldFixtureBytes` (JPEG tail) and `NewWrite_ByteIdenticalToOldWpfWrite`: need the v7 entry
//!   format of W4-08b (`PreviewCacheFile`) and the committed `tests/fixtures` entries.
//! * `PreviewCacheFileTests.WriteAtomically_OpaquePbgra32_RoundTripsColours` is ported at the payload level (below), not at the
//!   `PreviewCacheFile` level.
//! * `WebpHeicNativeTests.Webp_TransparentPreview_IsRefusedByTheJpegDiskCache_OpaqueOneRoundTrips` (lines 90-94): needs the
//!   W4-11c WIC Direct decoder and a WebP fixture writer; it is W4-11c's to port (skip here).
//! * `PreviewCacheCrossVersionTests.CommittedOldPngThumbnail_ReadsWithNewCode`: stays in W4-10b (it says so itself).
#![cfg(windows)]

use pr_imaging::decode_types::DecodeBox;
use pr_imaging::pixels::{PixelBuffer, PixelLayout, is_fully_opaque};
use pr_platform_win::wic::cache_image::{decode, encode_jpeg, encode_png};
use pr_test_support::TempRoot;

/// Byte `i` (row-major, no padding) = `(i * 131 + seed * 17 + i / 251) mod 256`, 64-bit arithmetic (golden pattern).
fn pattern_byte(index: u64, seed: u64) -> u8 {
    ((index * 131 + seed * 17 + index / 251) % 256) as u8
}

/// The golden pattern. With `alpha` (Pbgra32 only) byte 3 of pixel (x, y) is `255 - x * 255 / max(width - 1, 1)` and the colour
/// bytes are clamped to it (valid premultiplied data).
fn pattern(width: i32, height: i32, layout: PixelLayout, seed: u64, alpha: bool) -> PixelBuffer {
    let mut px = PixelBuffer::allocate(width, height, layout).expect("valid geometry");
    for (index, byte) in px.bytes_mut().expect("not disposed").iter_mut().enumerate() {
        *byte = pattern_byte(index as u64, seed);
    }
    if alpha && layout == PixelLayout::Pbgra32 {
        for y in 0..height {
            let row = px.get_row_mut(y).expect("not disposed");
            for x in 0..width as usize {
                let a = (255 - (x as i64 * 255 / i64::from((width - 1).max(1)))) as u8;
                row[x * 4 + 3] = a;
                for c in 0..3 {
                    row[x * 4 + c] = row[x * 4 + c].min(a);
                }
            }
        }
    }
    px
}

/// A photo-like source (Bgr32): the C# `CreatePhotoLikeBitmap` is a gradient checkerboard with +-16 of per-channel noise, so a
/// PNG cannot predict it away and the JPEG is smaller. The noise here is a fixed LCG (deterministic, no RNG dependency).
fn gradient(width: i32, height: i32) -> PixelBuffer {
    let mut px = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).expect("valid geometry");
    let mut state: u32 = 2024;
    let mut noise = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 24) % 32) as i32 - 16
    };
    for y in 0..height {
        let row = px.get_row_mut(y).expect("not disposed");
        for x in 0..width {
            let i = x as usize * 4;
            let base = [x * 255 / width.max(1), y * 255 / height.max(1), (x + y) & 0xFF];
            for (c, value) in base.into_iter().enumerate() {
                row[i + c] = (value + noise()).clamp(0, 255) as u8;
            }
            row[i + 3] = 255;
        }
    }
    px
}

/// Mean absolute error over the four channels of every pixel (C# `PixelAssert.MeanAbsoluteError`). Both buffers Pbgra32.
fn mean_absolute_error(expected: &PixelBuffer, actual: &PixelBuffer) -> f64 {
    assert_eq!(
        (expected.width(), expected.height()),
        (actual.width(), actual.height()),
        "geometry differs"
    );
    let a = expected.bytes().expect("not disposed");
    let b = actual.bytes().expect("not disposed");
    let total: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    total as f64 / a.len() as f64
}

fn encode_png_bytes(px: &PixelBuffer) -> Vec<u8> {
    let mut out = Vec::new();
    encode_png(px, &mut out).expect("encode_png");
    out
}

fn encode_jpeg_bytes(px: &PixelBuffer, quality: i32) -> Vec<u8> {
    let mut out = Vec::new();
    encode_jpeg(px, &mut out, quality).expect("encode_jpeg");
    out
}

#[test]
#[ignore = "native: needs WIC"]
fn translucent_png_round_trips_through_wic() {
    // C#: PreviewCacheCrossVersionTests.TranslucentPng_RoundTripsThroughWic (W4-10b): 40x24 pattern with alpha, written to a file,
    // read back with the layout None: Pbgra32, not fully opaque, mean absolute error of the four channels <= 1.0.
    let root = TempRoot::new("translucent-png");
    let source = pattern(40, 24, PixelLayout::Pbgra32, 0, true);
    let path = root.join("alpha.png");
    std::fs::write(&path, encode_png_bytes(&source)).expect("write png");
    let bytes = std::fs::read(&path).expect("read png");

    let (decoded, _) = decode(&bytes, None, DecodeBox::UNBOUNDED, 0, 0).expect("decode");

    assert_eq!(decoded.layout(), PixelLayout::Pbgra32);
    assert!(!is_fully_opaque(&decoded).expect("not disposed"));
    assert!(mean_absolute_error(&source, &decoded) <= 1.0);
}

#[test]
#[ignore = "native: needs WIC"]
fn encoded_jpeg_entry_is_smaller_than_equivalent_png() {
    // C#: PreviewCacheFileTests.EncodedEntryIsSmallerThanEquivalentPng (W4-08b), adapted to the WIC seam: the JPEG payload of a
    // photo-like 512x384 source is smaller than the 32-bit PNG of the same pixels (the measurement behind the format decision).
    let source = gradient(512, 384);

    let jpeg = encode_jpeg_bytes(&source, 90);
    let png = encode_png_bytes(&source);

    assert!(
        jpeg.len() < png.len(),
        "jpeg {} bytes vs png {} bytes",
        jpeg.len(),
        png.len()
    );
}

#[test]
#[ignore = "native: needs WIC"]
fn opaque_pbgra32_payload_round_trips_colours() {
    // C#: PreviewCacheFileTests.WriteAtomically_OpaquePbgra32_RoundTripsColours (W4-08b), at the payload level: a fully opaque
    // Pbgra32 buffer encodes and decodes back with every colour channel within 3 (premultiplied == straight at alpha 255).
    let mut source = pattern(32, 32, PixelLayout::Pbgra32, 0, false);
    for y in 0..32 {
        let row = source.get_row_mut(y).expect("not disposed");
        for x in 0..32 {
            row[x * 4 + 3] = 255;
        }
    }
    let png = encode_png_bytes(&source);

    let (decoded, _) = decode(&png, Some(PixelLayout::Pbgra32), DecodeBox::UNBOUNDED, 0, 0).expect("decode");

    assert_eq!(decoded.layout(), PixelLayout::Pbgra32);
    for y in 0..32 {
        let want = source.get_row(y).expect("not disposed");
        let got = decoded.get_row(y).expect("not disposed");
        for i in (0..32 * 4).step_by(4) {
            for c in 0..3 {
                assert!(want[i + c].abs_diff(got[i + c]) <= 3, "row {y} byte {} differs", i + c);
            }
        }
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn png_write_leaves_only_the_final_file() {
    // C#: DiskCacheStoreTests.WriteAtomicallyAsyncProducesOnlyTheFinalFile (W4-09), the real-WIC half through the PNG encoder:
    // a decodable PNG, and only the final file in the directory. The temp-file and rename part is W4-09's AtomicCacheFile.
    let root = TempRoot::new("png-only-final");
    let dir = root.join("write-atomic");
    std::fs::create_dir_all(&dir).expect("create dir");
    let final_path = dir.join("out.png");
    let image = pattern(16, 12, PixelLayout::Bgr32, 3, false);
    std::fs::write(&final_path, encode_png_bytes(&image)).expect("write png");

    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("read dir")
        .map(|e| e.expect("entry").path())
        .collect();
    assert!(
        entries.len() == 1 && entries[0] == final_path,
        "only the final file: {entries:?}"
    );
    let bytes = std::fs::read(&final_path).expect("read png");
    assert!(!bytes.is_empty());
    let (decoded, _) = decode(&bytes, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, 0, 0).expect("decode");
    assert_eq!((decoded.width(), decoded.height()), (16, 12));
}
