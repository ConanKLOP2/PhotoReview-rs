//! Port of C# PreviewCacheFileTests / PreviewCacheCrossVersionTests (baseline 7eaee2a2).
#![cfg(windows)]
use pr_core::{model::enums::DecoderBackend, sync::CancelToken};
use pr_imaging::{
    cache::preview_file::{read, write_atomically},
    exif_summary::{ExifSummary, RawExifTags},
    pixels::{PixelBuffer, PixelLayout},
    tiff::ExifRational,
};
use pr_platform_win::wic::{cache_adapters::WicPreviewPayloadCodec, cache_image};
use pr_test_support::{TempRoot, golden};
use std::fs;
fn fixture_exif() -> ExifSummary {
    ExifSummary::create(&RawExifTags {
        date_taken: Some("2024:05:06 07:08:09"),
        make: Some("Canon"),
        model: Some("EOS R5"),
        lens: Some("RF24-70mm F2.8 L IS USM"),
        iso: Some(400),
        focal_length: Some(ExifRational {
            numerator: 50,
            denominator: 1,
        }),
        f_number: Some(ExifRational {
            numerator: 28,
            denominator: 10,
        }),
        exposure_time: Some(ExifRational {
            numerator: 1,
            denominator: 250,
        }),
    })
    .expect("fixture EXIF summary is not empty")
}
fn legacy_pattern_pixels(width: i32, height: i32, layout: PixelLayout) -> PixelBuffer {
    let mut px = PixelBuffer::allocate(width, height, layout).expect("valid geometry");
    for y in 0..height {
        let row = px.get_row_mut(y).expect("not disposed");
        for x in 0..width {
            let i = x as usize * 4;
            row[i] = (x * 255 / (width - 1).max(1)) as u8;
            row[i + 1] = (y * 255 / (height - 1).max(1)) as u8;
            row[i + 2] = ((((x / 8) + (y / 8)) % 2) * 200 + 28) as u8;
            row[i + 3] = 255;
        }
    }
    px
}
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
#[test]
#[ignore = "native: needs WIC"]
fn encoded_entry_is_smaller_than_equivalent_png() {
    // C#: PreviewCacheFileTests.EncodedEntryIsSmallerThanEquivalentPng: actual files, including the .pv4 header.
    // Fixed LCG noise replaces System.Random(2024), preserving the 512x384 source class and exact size inequality.
    let root = TempRoot::new("twins-size");
    let source = gradient(512, 384);
    let path = root.join("entry.pv4");
    write_atomically(
        &source,
        DecoderBackend::Wpf,
        1,
        512,
        384,
        &path,
        false,
        None,
        &WicPreviewPayloadCodec,
        &CancelToken::new(),
    )
    .expect("write preview");
    let png = root.join("reference.png");
    cache_image::encode_png(&source, &mut fs::File::create(&png).expect("create PNG")).expect("encode PNG");
    assert!(fs::metadata(&path).expect("preview stat").len() < fs::metadata(png).expect("PNG stat").len());
    // Also retain the original pr-imaging twin's 2190x1460 / quarter-of-12.8MB bound.
    let large = gradient(2190, 1460);
    let large_path = root.join("large.pv4");
    write_atomically(
        &large,
        DecoderBackend::Wpf,
        1,
        2190,
        1460,
        &large_path,
        false,
        None,
        &WicPreviewPayloadCodec,
        &CancelToken::new(),
    )
    .expect("write large preview");
    assert!(fs::metadata(large_path).expect("large preview stat").len() < 12_800_000 / 4);
}

#[test]
#[ignore = "native: needs WIC"]
fn new_writer_reproduces_committed_old_fixture_jpeg_tail() {
    // C#: PreviewCacheCrossVersionTests.NewWriter_ReproducesCommittedOldFixtureBytes (both rows, entire file).
    let root = TempRoot::new("twins-old-writer");
    for (name, layout, backend, orientation, ow, oh, exif) in [
        (
            "v7-exif-bgr32-96x64.pv4",
            PixelLayout::Bgr32,
            DecoderBackend::WicDirect,
            6,
            6000,
            4000,
            Some(fixture_exif()),
        ),
        (
            "v7-noexif-pbgra32-96x64.pv4",
            PixelLayout::Pbgra32,
            DecoderBackend::TurboJpeg,
            1,
            0,
            0,
            None,
        ),
    ] {
        let pixels = legacy_pattern_pixels(96, 64, layout);
        let path = root.join(name);
        write_atomically(
            &pixels,
            backend,
            orientation,
            ow,
            oh,
            &path,
            false,
            exif.as_ref(),
            &WicPreviewPayloadCodec,
            &CancelToken::new(),
        )
        .expect("write preview");
        let old = golden::repo_root().join("tests/fixtures/PreviewCache").join(name);
        assert_eq!(
            fs::read(path).expect("new bytes"),
            fs::read(old).expect("committed old bytes"),
            "{name}"
        );
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn write_atomically_opaque_pbgra32_round_trips_colours() {
    // C#: PreviewCacheFileTests.WriteAtomically_OpaquePbgra32_RoundTripsColours (AlphaBitmap: B=G=R=77, A=255).
    let root = TempRoot::new("twins-opaque");
    let mut source = PixelBuffer::allocate(32, 32, PixelLayout::Pbgra32).expect("geometry");
    for pixel in source.bytes_mut().expect("pixels").chunks_exact_mut(4) {
        pixel.copy_from_slice(&[77, 77, 77, 255]);
    }
    let path = root.join("entry.pv4");
    write_atomically(
        &source,
        DecoderBackend::Wpf,
        1,
        32,
        32,
        &path,
        false,
        None,
        &WicPreviewPayloadCodec,
        &CancelToken::new(),
    )
    .expect("write preview");
    let result = read(&path, &WicPreviewPayloadCodec).expect("read preview");
    assert_eq!((result.pixels.width(), result.pixels.height()), (32, 32));
    for (want, got) in source
        .bytes()
        .expect("source")
        .chunks_exact(4)
        .zip(result.pixels.bytes().expect("decoded").chunks_exact(4))
    {
        for c in 0..3 {
            assert!(want[c].abs_diff(got[c]) <= 3, "channel {c}: {} vs {}", want[c], got[c]);
        }
    }
}
