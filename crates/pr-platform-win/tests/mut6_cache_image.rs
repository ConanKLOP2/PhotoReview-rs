//! P6 mutation evidence for `wic/cache_image.rs`: the survivors of the first full run (no production change).
//!
//! * `cache_image.rs:293:14` (`bytes > MAX` -> `>=` in `plan_decode`) and `:466:28` (`byte_count > u32::MAX` -> `>=` in
//!   `validate_pixels`): the two operands can never be equal. The left side is `4 * w * h` (a literal factor 4, also
//!   modulo 2^64), the right side is odd. The tests execute that argument over every residue class of `w` and `h`
//!   modulo 4 and pin its premises on the real code.
//! * `cache_image.rs:679:13` / `:680:13` (delete `dwType` / `vt` of the `PROPBAG2` in `write_image_quality`): the real
//!   WIC JPEG encoder reads the option by NAME and takes the type from the VARIANT. The test re-implements the encode with
//!   the four field variants and compares the bytes for EVERY quality 1..=100 and several pixel-content classes.
#![cfg(windows)]

use std::io::Cursor;

use pr_imaging::decode_types::DecodeBox;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::wic::cache_image::{
    CONTAINER_FORMAT_JPEG, CacheImageError, DPI, EncodeError, INVALID_DATA_TOO_LARGE, MAX_DECODED_BYTES,
    PIXEL_FORMAT_24BPP_BGR, VENDOR_MICROSOFT, bgrx32_to_bgr24, encode_jpeg, encode_png, image_quality_value,
    plan_decode, source_format,
};

#[test]
fn plan_decode_size_limit_operands_can_never_be_equal() {
    // Premise 1: the limit is odd (== 3 mod 4).
    assert_eq!(MAX_DECODED_BYTES % 4, 3);
    // Premise 2: `w * h * 4` cannot overflow `u64` for any accepted dimension, so there is no wrap that could change
    // the residue.
    let max_side = i32::MAX as u64;
    assert!(max_side.checked_mul(max_side).and_then(|p| p.checked_mul(4)).is_some());
    // The residue of `w * h * 4` modulo 4 depends only on the residues of `w` and `h`: all 16 classes are 0.
    for rw in 0u64..4 {
        for rh in 0u64..4 {
            assert_eq!(rw * rh * 4 % 4, 0);
            assert_ne!(rw * rh * 4 % 4, (MAX_DECODED_BYTES as u64) % 4);
        }
    }
    // The real function at the neighbours of the limit: 2^29 pixels = 2^31 bytes is refused, one pixel fewer is accepted.
    assert_eq!(
        plan_decode(1 << 29, 1, DecodeBox::UNBOUNDED, 0, 0),
        Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
    );
    assert!(plan_decode((1 << 29) - 1, 1, DecodeBox::UNBOUNDED, 0, 0).is_ok());
    assert_eq!(
        plan_decode(1, 1 << 29, DecodeBox::UNBOUNDED, 0, 0),
        Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
    );
}

#[test]
fn pixel_buffer_byte_count_is_never_u32_max() {
    // Premise: u32::MAX is odd; a buffer's byte count is `width * 4 * height` (also when it wraps modulo 2^64, which
    // keeps the residue modulo 4).
    assert_eq!(u64::from(u32::MAX) % 4, 3);
    for rw in 0i64..4 {
        for rh in 0i64..4 {
            assert_eq!(rw.wrapping_mul(4).wrapping_mul(rh).rem_euclid(4), 0);
        }
    }
    // The real constructors over the corner geometry: stride 4 per pixel, byte count a multiple of 4, never u32::MAX.
    let cases = [
        (1, 1),
        (3, 5),
        (1, i32::MAX),
        (7, 614_891_469),
        (65_535, 16_385),
        (32_768, 32_768),
        (536_870_911, 8),
    ];
    for (w, h) in cases {
        let px = PixelBuffer::unallocated_for_tests(w, h, PixelLayout::Bgr32);
        assert_eq!(px.byte_count() % 4, 0, "{w}x{h}");
        assert_ne!(px.byte_count(), i64::from(u32::MAX), "{w}x{h}");
    }
    let one = PixelBuffer::allocate(1, 1, PixelLayout::Pbgra32).unwrap();
    assert_eq!((one.stride(), one.byte_count()), (4, 4));
    // The refusal side of the limit is already pinned (wic_cache_image repair_round_2): 2^32 bytes => TooLarge.
    let big = PixelBuffer::unallocated_for_tests(32768, 32768, PixelLayout::Bgr32);
    let mut out = Vec::new();
    assert!(matches!(encode_png(&big, &mut out), Err(EncodeError::TooLarge { .. })));
}

fn to_win(guid: &pr_core::catalog::explorer_types::Guid) -> windows_core::GUID {
    let b = guid.0;
    windows_core::GUID {
        data1: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        data2: u16::from_le_bytes([b[4], b[5]]),
        data3: u16::from_le_bytes([b[6], b[7]]),
        data4: [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
    }
}

fn from_win(guid: &windows_core::GUID) -> pr_core::catalog::explorer_types::Guid {
    let (a, b, c, d) = (
        guid.data1.to_le_bytes(),
        guid.data2.to_le_bytes(),
        guid.data3.to_le_bytes(),
        guid.data4,
    );
    pr_core::catalog::explorer_types::Guid([
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7],
    ])
}

// ---- PROPBAG2 fields -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bag {
    Full,
    NoDwType,
    NoVt,
    NoBoth,
}

/// The production `encode` for a `Bgr32` buffer through a JPEG encoder, with the `PROPBAG2` built per `bag`.
#[cfg(windows)]
fn encode_with_bag(pixels: &PixelBuffer, quality: i32, bag_kind: Bag) -> Vec<u8> {
    use pr_platform_win::wic::interop::{self, Com, ComStream};
    use windows::Win32::Graphics::Imaging::{IWICBitmapEncoder, IWICBitmapFrameEncode, WICBitmapEncoderCacheOption};
    use windows::Win32::System::Com::StructuredStorage::{IPropertyBag2, PROPBAG2};
    use windows::Win32::System::Variant::{VARENUM, VARIANT};
    use windows_core::PWSTR;

    let stream = ComStream::writable_memory();
    let factory = interop::create_factory().expect("factory");
    let container = to_win(&CONTAINER_FORMAT_JPEG);
    let vendor = to_win(&VENDOR_MICROSOFT);
    // SAFETY: `factory` is live; both GUID pointers are valid for the call.
    let encoder: Com<IWICBitmapEncoder> =
        Com::adopt(unsafe { factory.CreateEncoder(&container, &vendor) }).expect("encoder");
    let istream = stream.istream();
    // SAFETY: `encoder` is live; `istream` outlives the function.
    unsafe { encoder.Initialize(&istream, WICBitmapEncoderCacheOption(2)) }.expect("initialize");
    let mut frame_out: Option<IWICBitmapFrameEncode> = None;
    let mut options_out: Option<IPropertyBag2> = None;
    // SAFETY: both out pointers are valid `Option` locals.
    unsafe { encoder.CreateNewFrame(&mut frame_out, &mut options_out) }.expect("frame");
    let frame = Com::adopt(frame_out.ok_or_else(windows_core::Error::empty)).expect("frame");
    let options = Com::adopt(options_out.ok_or_else(windows_core::Error::empty)).expect("options");

    let mut name: Vec<u16> = "ImageQuality".encode_utf16().chain(std::iter::once(0)).collect();
    let bag = PROPBAG2 {
        dwType: if matches!(bag_kind, Bag::NoDwType | Bag::NoBoth) {
            0
        } else {
            1
        },
        vt: if matches!(bag_kind, Bag::NoVt | Bag::NoBoth) {
            VARENUM(0)
        } else {
            VARENUM(4)
        },
        pstrName: PWSTR(name.as_mut_ptr()),
        ..PROPBAG2::default()
    };
    let mut variant = VARIANT::default();
    // SAFETY: a zeroed, owned VARIANT; writing the tag and `fltVal` is sound and `VT_R4` needs no cleanup.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VARENUM(4);
        inner.Anonymous.fltVal = image_quality_value(quality);
    }
    // SAFETY: `options` is live; `bag` and `variant` are valid for one property and outlive the call.
    let written = unsafe { options.Write(1, &bag, &variant) };
    std::mem::forget(variant);
    written.unwrap_or_else(|e| panic!("{bag_kind:?} quality {quality}: Write failed: {e}"));

    let width = pixels.width();
    let height = pixels.height();
    let mut accepted = to_win(&source_format(pixels.layout()));
    // SAFETY: `frame` is live, `options` is its bag, `accepted` a valid in/out GUID.
    unsafe {
        frame.Initialize(&*options).expect("frame init");
        frame.SetSize(width as u32, height as u32).expect("size");
        frame.SetResolution(DPI, DPI).expect("dpi");
        frame.SetPixelFormat(&mut accepted).expect("format");
    }
    assert_eq!(
        from_win(&accepted),
        PIXEL_FORMAT_24BPP_BGR,
        "the JPEG encoder wants 24bppBGR"
    );
    let row_bytes = width as usize * 3;
    let mut band = vec![0u8; row_bytes * height as usize];
    for y in 0..height {
        let source = pixels.get_row(y).unwrap();
        let start = y as usize * row_bytes;
        bgrx32_to_bgr24(source, &mut band[start..start + row_bytes]).unwrap();
    }
    // SAFETY: `frame` is configured; `band` is `height * row_bytes` initialised bytes.
    unsafe {
        frame
            .WritePixels(height as u32, row_bytes as u32, &band)
            .expect("pixels");
        frame.Commit().expect("frame commit");
        encoder.Commit().expect("commit");
    }
    let backend = stream.dispose().expect("stream");
    let payload = backend
        .into_any()
        .downcast::<Cursor<Vec<u8>>>()
        .expect("memory backend");
    payload.into_inner()
}

fn content(width: i32, height: i32, class: usize) -> PixelBuffer {
    let mut pixels = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).unwrap();
    for (index, byte) in pixels.bytes_mut().unwrap().iter_mut().enumerate() {
        *byte = match class {
            0 => 0x80,
            1 => ((index * 131 + index / 13) % 256) as u8,
            2 => ((index / 4) * 7 % 256) as u8,
            _ => (index.wrapping_mul(2_654_435_761) >> 7) as u8,
        };
    }
    pixels
}

#[test]
#[ignore = "native: needs WIC"]
fn jpeg_image_quality_bag_ignores_dwtype_and_vt_for_every_quality() {
    for (w, h) in [(16, 8), (33, 17)] {
        for class in 0..4 {
            let pixels = content(w, h, class);
            for quality in 1..=100 {
                let full = encode_with_bag(&pixels, quality, Bag::Full);
                // The reference is the production path.
                let mut production = Vec::new();
                encode_jpeg(&pixels, &mut production, quality).unwrap();
                assert_eq!(
                    production, full,
                    "{w}x{h} class {class} q{quality}: reference differs from production"
                );
                for kind in [Bag::NoDwType, Bag::NoVt, Bag::NoBoth] {
                    assert_eq!(
                        encode_with_bag(&pixels, quality, kind),
                        full,
                        "{w}x{h} class {class} q{quality} {kind:?}"
                    );
                }
            }
        }
    }
}
