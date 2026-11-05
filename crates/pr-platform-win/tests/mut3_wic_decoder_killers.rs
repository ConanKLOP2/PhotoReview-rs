//! MUT3-HOST (2026-11-06): killers for `wic/decoder.rs` mutants that the golden/fixture tests miss. Pictures are generated in memory (see
//! `common/mut3_png.rs`) so a one-pixel-high or one-pixel-wide image can be decoded through the real WIC; all tests need WIC.
#![cfg(windows)]

#[path = "common/mut3_png.rs"]
mod mut3_png;

use std::sync::Arc;

use pr_core::abstractions::SourceReader;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_platform_win::wic::decoder::WicDirectDecoder;

fn memory() -> Arc<dyn MemoryInfoSource> {
    Arc::new(|| MemoryInfo {
        total_available: 64 * 1024 * 1024 * 1024,
        load: 0,
    })
}

fn decoder() -> WicDirectDecoder {
    WicDirectDecoder::new(PixelBufferImageCodec::shared(), None::<Arc<dyn SourceReader>>, memory())
}

fn decode_in_box(bytes: Vec<u8>, decode_box: DecodeBox) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
    let request = DecodeRequest::with_box("mut3-memory.png", decode_box).with_bytes(Arc::from(bytes));
    decoder().decode(&request)
}

fn gradient(width: u32, height: u32) -> Vec<u8> {
    mut3_png::png(width, height, false, |x, y| {
        [(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255]
    })
}

// decoder.rs:172:34 `target_w < orig_w || target_h < orig_h` -> `&&`: a box that shrinks only ONE axis (the other stays at the stored size because the
// fitted size is floored to at least 1 / the aspect keeps it equal) must still downscale.
#[test]
#[ignore = "native: needs WIC"]
fn a_box_that_shrinks_only_one_axis_still_downscales() {
    // 100 x 1 into a width-only box of 50: fit is (50, 1), the height is unchanged.
    let wide = decode_in_box(gradient(100, 1), DecodeBox::new(50, 0)).expect("decode wide strip");
    assert_eq!(
        (wide.pixel_width(), wide.pixel_height()),
        (50, 1),
        "width shrinks, height stays 1"
    );
    assert!(wide.downscaled(), "flag");
    assert_eq!((wide.original_width(), wide.original_height()), (100, 1));
    // 1 x 100 into a height-only box of 40: fit is (1, 40), the width is unchanged.
    let tall = decode_in_box(gradient(1, 100), DecodeBox::new(0, 40)).expect("decode tall strip");
    assert_eq!(
        (tall.pixel_width(), tall.pixel_height()),
        (1, 40),
        "height shrinks, width stays 1"
    );
    assert!(tall.downscaled(), "flag");
    assert_eq!((tall.original_width(), tall.original_height()), (1, 100));
}

// decoder.rs:172:25 `target_w < orig_w` -> `<=`: a box that does not shrink the image on any axis (equal to the stored size, or larger, or equal on
// the constrained axis only) must NOT scale and must report `downscaled() == false`; one pixel smaller must scale.
#[test]
#[ignore = "native: needs WIC"]
fn a_box_that_does_not_shrink_the_image_does_not_downscale() {
    for (name, decode_box) in [
        ("equal", DecodeBox::new(64, 48)),
        ("width only, equal", DecodeBox::new(64, 0)),
        ("height only, equal", DecodeBox::new(0, 48)),
        ("larger", DecodeBox::new(200, 200)),
        ("equal width, larger height", DecodeBox::new(64, 200)),
    ] {
        let view = decode_in_box(gradient(64, 48), decode_box).expect(name);
        assert_eq!((view.pixel_width(), view.pixel_height()), (64, 48), "{name}: size");
        assert!(!view.downscaled(), "{name}: not downscaled");
        assert_eq!(
            (view.original_width(), view.original_height()),
            (64, 48),
            "{name}: original"
        );
    }
    let one_less = decode_in_box(gradient(64, 48), DecodeBox::new(63, 0)).expect("one pixel narrower");
    assert_eq!((one_less.pixel_width(), one_less.pixel_height()), (63, 47));
    assert!(one_less.downscaled());
    let one_shorter = decode_in_box(gradient(64, 48), DecodeBox::new(0, 47)).expect("one pixel shorter");
    assert_eq!((one_shorter.pixel_width(), one_shorter.pixel_height()), (62, 47));
    assert!(one_shorter.downscaled());
}
