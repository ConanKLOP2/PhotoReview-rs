//! Repair round 1 tests for W4-11e: .NET exception classification of raw WIC failures, failing output streams, and the
//! boundaries/constants the unit tests left unpinned. Pure tests need W4-11a's `exception_kind_for_hresult`; native
//! tests are `#[ignore]` and only touch memory.
#![cfg(windows)]

use pr_imaging::decode_types::DecodeBox;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::wic::cache_image::{
    BAND_BYTES, CacheImageError, EncodeError, INVALID_DATA_TOO_LARGE, WINCODEC_ERR_STREAMWRITE, com_is_entry_failure,
    decode, encode_jpeg, encode_png, finish_output, plan_decode, rows_per_band,
};
use pr_platform_win::wic::interop::DotnetExceptionKind;
use pr_platform_win::wic::scaler::{ResizePlan, ScaleError, plan_resize};
use std::io::{self, Write};

const ARITHMETIC: i32 = 0x8007_0216_u32 as i32;
const E_NOTIMPL: i32 = 0x8000_4001_u32 as i32;
const E_POINTER: i32 = 0x8000_4003_u32 as i32;
const E_FAIL: i32 = 0x8000_4005_u32 as i32;
const WINCODEC_ERR_BADIMAGE: i32 = 0x8898_2F60_u32 as i32;
const E_INVALIDARG: i32 = 0x8007_0057_u32 as i32;

struct FailingWriter;
impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("disk full"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn com_hresult_mapped_to_non_whitelisted_dotnet_type_is_not_an_entry_failure() {
    for hr in [ARITHMETIC, E_NOTIMPL, E_POINTER] {
        assert!(!CacheImageError::Com { hresult: hr }.is_entry_failure(), "{hr:#x}");
        assert!(!com_is_entry_failure(hr));
    }
}

#[test]
fn com_hresult_mapped_to_whitelisted_dotnet_type_is_an_entry_failure() {
    for hr in [WINCODEC_ERR_BADIMAGE, E_FAIL, E_INVALIDARG] {
        assert!(CacheImageError::Com { hresult: hr }.is_entry_failure(), "{hr:#x}");
    }
}

#[test]
fn scale_error_com_reports_the_dotnet_type() {
    assert_eq!(
        ScaleError::Com { hresult: ARITHMETIC }.dotnet_kind(),
        Some(DotnetExceptionKind::Arithmetic)
    );
    assert_eq!(
        ScaleError::Com {
            hresult: WINCODEC_ERR_BADIMAGE
        }
        .dotnet_kind(),
        Some(DotnetExceptionKind::Com)
    );
    assert_eq!(
        ScaleError::InsufficientMemory {
            hresult: 0x8007_000E_u32 as i32
        }
        .dotnet_kind(),
        Some(DotnetExceptionKind::OutOfMemory)
    );
    assert_eq!(ScaleError::Disposed.dotnet_kind(), None);
}

#[test]
fn finish_output_matches_csharp_for_a_failing_stream() {
    let failure = || Err(io::Error::other("x"));
    assert!(matches!(finish_output(true, failure()), Ok(())));
    assert!(matches!(
        finish_output(false, failure()),
        Err(EncodeError::Com { hresult }) if hresult == WINCODEC_ERR_STREAMWRITE
    ));
    assert_eq!(WINCODEC_ERR_STREAMWRITE as u32, 0x8898_2F71);
    assert!(matches!(finish_output(false, Ok(())), Ok(())));
}

#[test]
#[ignore = "native: needs WIC"]
fn jpeg_encode_into_a_failing_stream_returns_ok_like_csharp() {
    let px = PixelBuffer::allocate(16, 16, PixelLayout::Bgr32).expect("alloc");
    assert!(encode_jpeg(&px, &mut FailingWriter, 80).is_ok());
}

#[test]
#[ignore = "native: needs WIC"]
fn png_encode_into_a_failing_stream_is_a_com_error_like_csharp() {
    let px = PixelBuffer::allocate(16, 16, PixelLayout::Bgr32).expect("alloc");
    assert!(matches!(
        encode_png(&px, &mut FailingWriter),
        Err(EncodeError::Com { hresult }) if hresult == WINCODEC_ERR_STREAMWRITE
    ));
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    let mut tail = kind.to_vec();
    tail.extend_from_slice(body);
    out.extend_from_slice(&tail);
    out.extend_from_slice(&crc32(&tail).to_be_bytes());
    out
}

/// Palette PNG with a 65536x65536 header (CRC-correct, no real pixel data).
fn huge_palette_png() -> Vec<u8> {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&65536u32.to_be_bytes());
    ihdr.extend_from_slice(&65536u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"PLTE", &[0, 0, 0]));
    png.extend(chunk(b"IDAT", &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]));
    png.extend(chunk(b"IEND", &[]));
    png
}

#[test]
#[ignore = "native: needs WIC"]
fn decode_of_65536_square_palette_png_with_box_is_not_an_entry_failure() {
    let png = huge_palette_png();
    for decode_box in [DecodeBox::new(10, 0), DecodeBox::new(0, 10), DecodeBox::new(100, 100)] {
        let error = decode(&png, None, decode_box, 0, 0).expect_err("65536 square palette png fails in WIC");
        assert!(!error.is_entry_failure(), "{error:?}");
    }
}

#[test]
fn band_size_and_rows_per_band_pin_the_constant() {
    assert_eq!(BAND_BYTES, 262_144);
    assert_eq!(rows_per_band(1, 1_000_000), 87_381);
    assert_eq!(rows_per_band(87_381, 10), 1);
}

#[test]
fn plan_decode_size_limit_boundary() {
    // 536_870_911 * 4 = 2_147_483_644 <= i32::MAX; 536_870_912 * 4 = 2^31 > i32::MAX.
    assert!(plan_decode(536_870_911, 1, DecodeBox::UNBOUNDED, 0, 0).is_ok());
    assert_eq!(
        plan_decode(536_870_912, 1, DecodeBox::UNBOUNDED, 0, 0),
        Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
    );
    // The product is u64: i32::MAX squared must not panic or wrap into a pass.
    assert_eq!(
        plan_decode(i32::MAX as u32, i32::MAX as u32, DecodeBox::UNBOUNDED, 0, 0),
        Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
    );
}

#[test]
fn plan_decode_height_only_shrink_is_downscaled() {
    // 1 x 400 into a box of 100 high: the width stays 1, only the height shrinks.
    let plan = plan_decode(1, 400, DecodeBox::new(0, 100), 0, 0).expect("valid");
    assert_eq!(plan.width, 1);
    assert!(plan.height < 400);
    assert!(plan.downscaled);
}

#[test]
fn plan_resize_u32_max_boundary() {
    let at = i64::from(u32::MAX);
    assert_eq!(plan_resize(false, 1, 1, at, 1, 1), Ok(ResizePlan::Copy));
    assert_eq!(plan_resize(false, 1, 1, at, 2, 2), Ok(ResizePlan::Wic));
    assert_eq!(
        plan_resize(false, 1, 1, at + 1, 1, 1),
        Err(ScaleError::TooLarge { byte_count: at + 1 })
    );
}

#[test]
fn scale_error_display_texts() {
    assert_eq!(
        ScaleError::Com { hresult: E_FAIL }.to_string(),
        "WIC call failed (0x80004005)."
    );
    assert_eq!(
        ScaleError::WidthOutOfRange { width: 0 }.to_string(),
        "Width must be at least 1 (was 0)."
    );
    assert_eq!(
        ScaleError::HeightOutOfRange { height: -1 }.to_string(),
        "Height must be at least 1 (was -1)."
    );
    assert!(ScaleError::TooLarge { byte_count: 5 }.to_string().contains("5 bytes"));
}
