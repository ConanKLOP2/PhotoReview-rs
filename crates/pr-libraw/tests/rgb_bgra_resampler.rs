//! Red tests for `pr_libraw::resampler` (W5-17, contract `contracts/W5-17.md`). Ports of the C# `RgbBgraResampler` tests of
//! `RgbBgraResamplerTests` and `RgbBgraResamplerEdgeShapeTests`, the resampler parts of `LibRawManagedLogicTests`,
//! `RgbBgraResamplerMutationGapTests`, `LibRawDecodeMemoryTests`, `LibRawPixelBufferHandOffTests` and `RawErrorHandlingReviewTests`
//! (C# baseline d92d6aae), plus the golden-driven tests over `tests/golden/rgb-bgra-resample.json` and
//! `tests/golden/rgb-bgra-validate.json` (inputs in `tests/golden-inputs/`).
//!
//! Every test fails through the stub's `todo!()` until the porter writes the bodies. Exceptions, which pass on the stub
//! because the stub already has the body: `is_supported_channel_count_only_gray_and_rgb`, the `source_bytes` / `fnv1a64_hex`
//! unit tests (test helpers, not the unit under test), `max_array_length_is_dotnet_array_max`, and the `BgraBuffer` accessors.
//!
//! Documented skips (non-portable): `LibRawCorruptFileHandleTests` (native, W5-16); the managed allocation budget of
//! `ResizeToBuffer_FourMegapixelFullSize_DoesNotAllocateAManagedBgraCopy` (no managed GC in Rust, only the functional half is
//! ported); the WPF `BitmapSource` reads (replaced by `take_pixels` and `PixelBuffer::bytes`).
//!
//! The C# tests build inputs with `new Random(seed)`. They are replaced here by the golden `xorshift` source kind
//! ([`xorshift_bytes`]), so the expected values come from the reference functions below, not from .NET's generator.
//!
//! `NativePixelMemory::live_count` is process-wide and the test harness runs tests on several threads: every test holds
//! [`guard`], so the live-count deltas are exact.

use std::sync::{Mutex, MutexGuard, PoisonError};

use pr_core::sync::CancelToken;
use pr_imaging::pixel_memory::NativePixelMemory;
use pr_imaging::pixels::PixelLayout;
use pr_libraw::resampler::{
    BOX_AVERAGE_RATIO_THRESHOLD, BgraBuffer, MAX_ARRAY_LENGTH, ResampleError, ResamplePath, is_supported_channel_count,
    resize, resize_to_buffer, select_path, validate_source_length, validate_target_length,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::{Value, json};

static GUARD: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    GUARD.lock().unwrap_or_else(PoisonError::into_inner)
}

fn cancelled_token() -> CancelToken {
    let token = CancelToken::new();
    token.cancel();
    token
}

/// The golden `xorshift` source kind and the replacement of C# `new Random(seed).NextBytes`: `u32` state, shifts 13 / 17 / 5,
/// byte = state >> 8. The seed must be non-zero.
fn xorshift_bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 8) as u8
        })
        .collect()
}

/// Source bytes of one golden `source` spec (`tests/golden-inputs/rgb-bgra-resample.json`), length `sw * sh * channels`.
fn source_bytes(spec: &Value, sw: usize, sh: usize, channels: usize) -> Vec<u8> {
    let len = sw * sh * channels;
    match str_of(spec, "kind") {
        "mod" => {
            let (mul, add) = (i64_of(spec, "mul"), i64_of(spec, "add"));
            (0..len).map(|i| ((i as i64 * mul + add) % 251) as u8).collect()
        }
        "xorshift" => xorshift_bytes(len, u32::try_from(i64_of(spec, "seed")).expect("seed fits u32")),
        "solid" => vec![i64_of(spec, "value") as u8; len],
        "solid_rgb" => {
            let rgb: Vec<u8> = spec["rgb"]
                .as_array()
                .expect("solid_rgb needs rgb")
                .iter()
                .map(|v| v.as_i64().expect("rgb byte") as u8)
                .collect();
            let mut data = vec![0u8; len];
            for i in 0..sw * sh {
                for c in 0..3 {
                    data[i * 3 + c] = rgb[c];
                }
            }
            data
        }
        "checker" => {
            let mut data = vec![0u8; len];
            for y in 0..sh {
                for x in 0..sw {
                    let value = if (x + y) % 2 == 0 { 255 } else { 0 };
                    for c in 0..channels {
                        data[(y * sw + x) * channels + c] = value;
                    }
                }
            }
            data
        }
        "gradient" => {
            let mut data = vec![0u8; len];
            for y in 0..sh {
                for x in 0..sw {
                    for c in 0..channels {
                        data[(y * sw + x) * channels + c] = ((x * 7 + y * 13 + c * 50) % 256) as u8;
                    }
                }
            }
            data
        }
        "explicit" => {
            let bytes: Vec<u8> = spec["bytes"]
                .as_array()
                .expect("explicit needs bytes")
                .iter()
                .map(|v| v.as_i64().expect("byte") as u8)
                .collect();
            assert_eq!(bytes.len(), len, "explicit source length mismatch");
            bytes
        }
        other => panic!("unknown source kind {other}"),
    }
}

/// FNV-1a 64 over `bytes`, lower-case 16 hex digits (the golden `fnv1a64` field).
fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

/// Lower-case hex of `bytes` (the golden `head_hex` / `all_hex` fields).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Runs [`resize`] into a fresh zeroed BGRA array and panics on any error.
fn run(source: &[u8], sw: i32, sh: i32, tw: i32, th: i32, channels: i32) -> Vec<u8> {
    let mut bgra = vec![0u8; (tw * th * 4) as usize];
    resize(source, sw, sh, &mut bgra, tw, th, &CancelToken::none(), channels)
        .unwrap_or_else(|error| panic!("resize {sw}x{sh} -> {tw}x{th} failed: {error}"));
    bgra
}

/// Reference: every target pixel is the rounded-half-up mean of the source pixels its footprint covers (at least one per axis).
/// Port of the C# `ReferenceBox` (integer arithmetic, `i64`). The source is RGB or gray; the output is B, G, R, A.
fn reference_box(source: &[u8], sw: i64, sh: i64, tw: i64, th: i64, channels: usize) -> Vec<u8> {
    let mut result = vec![0u8; (tw * th * 4) as usize];
    for y in 0..th {
        for x in 0..tw {
            let y_start = y * sh / th;
            let y_end = (y_start + 1).max((y + 1) * sh / th);
            let x_start = x * sw / tw;
            let x_end = (x_start + 1).max((x + 1) * sw / tw);
            let mut sums = [0i64; 3];
            let mut count = 0i64;
            for sy in y_start..y_end {
                for sx in x_start..x_end {
                    let at = ((sy * sw + sx) as usize) * channels;
                    for (c, sum) in sums.iter_mut().enumerate() {
                        let ch = if channels == 3 { c } else { 0 };
                        *sum += i64::from(source[at + ch]);
                    }
                    count += 1;
                }
            }
            let at = ((y * tw + x) * 4) as usize;
            result[at] = ((sums[2] + count / 2) / count) as u8; // B
            result[at + 1] = ((sums[1] + count / 2) / count) as u8; // G
            result[at + 2] = ((sums[0] + count / 2) / count) as u8; // R
            result[at + 3] = 255;
        }
    }
    result
}

/// Reference: standard bilinear sampling at pixel centres, edges clamped, rounded to the nearest even (`Math.Round`).
/// Port of the C# `ReferenceBilinear`, `f64` arithmetic as in C#.
fn reference_bilinear(source: &[u8], sw: i64, sh: i64, tw: i64, th: i64, channels: usize) -> Vec<u8> {
    let mut result = vec![0u8; (tw * th * 4) as usize];
    for y in 0..th {
        for x in 0..tw {
            let fy = (((y as f64 + 0.5) * sh as f64) / th as f64 - 0.5).clamp(0.0, sh as f64 - 1.0);
            let fx = (((x as f64 + 0.5) * sw as f64) / tw as f64 - 0.5).clamp(0.0, sw as f64 - 1.0);
            let (y0, x0) = (fy.floor() as i64, fx.floor() as i64);
            let (y1, x1) = ((y0 + 1).min(sh - 1), (x0 + 1).min(sw - 1));
            let (wy, wx) = (fy - y0 as f64, fx - x0 as f64);
            let at = ((y * tw + x) * 4) as usize;
            for c in 0..3usize {
                let ch = if channels == 3 { c } else { 0 };
                let pixel = |px: i64, py: i64| f64::from(source[((py * sw + px) as usize) * channels + ch]);
                let top = pixel(x0, y0) + (pixel(x1, y0) - pixel(x0, y0)) * wx;
                let bottom = pixel(x0, y1) + (pixel(x1, y1) - pixel(x0, y1)) * wx;
                let value = (top + (bottom - top) * wy).round_ties_even() as i64;
                result[at + (2 - c)] = value.clamp(0, 255) as u8; // output is B, G, R
            }
            result[at + 3] = 255;
        }
    }
    result
}

/// Opaque BGRA of gray values (C# `Gray` test helper).
fn gray_bgra(values: &[u8]) -> Vec<u8> {
    values.iter().flat_map(|&v| [v, v, v, 255]).collect()
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

// ---- C#: RgbBgraResamplerTests ------------------------------------------------------------------------------------------------

#[test]
fn resize_one_pixel_checkerboard_reduced_threefold_averages_instead_of_aliasing() {
    // C#: RgbBgraResamplerTests.Resize_OnePixelCheckerboardReducedThreefold_AveragesInsteadOfAliasing
    let _guard = guard();
    let (source, target) = (60, 20);
    let rgb = source_bytes(&json!({"kind": "checker"}), 60, 60, 3);
    let bgra = run(&rgb, source, source, target, target, 3);

    // A 3x3 box of a checkerboard is 4 or 5 white pixels of 9 -> 113 or 142; pure 0/255 would be aliasing.
    for pixel in bgra.chunks_exact(4) {
        for &channel in &pixel[..3] {
            assert!((100..=155).contains(&channel), "channel {channel} is aliased");
        }
        assert_eq!(pixel[3], 255);
    }
}

#[test]
fn resize_solid_color_box_average_keeps_channel_order_bgr() {
    // C#: RgbBgraResamplerTests.Resize_SolidColorBoxAverage_KeepsChannelOrderBgr
    let _guard = guard();
    let rgb = [200u8, 100, 10].repeat(40 * 40);
    let bgra = run(&rgb, 40, 40, 8, 8, 3);

    assert_eq!(bgra[..4].to_vec(), vec![10, 100, 200, 255]);
    assert_eq!(bgra[bgra.len() - 4..].to_vec(), vec![10, 100, 200, 255]);
}

#[test]
fn resize_moderate_reduction_uses_bilinear() {
    // C#: RgbBgraResamplerTests.Resize_ModerateReduction_UsesBilinear
    let _guard = guard();
    let rgb: [u8; 12] = [0, 0, 0, 90, 90, 90, 180, 180, 180, 255, 255, 255];
    let bgra = run(&rgb, 4, 1, 3, 1, 3);

    assert_eq!(bgra[0], 15); // sample at 1/6 of the way between source pixels 0 and 1 (a box average would give 0)
    assert_eq!(bgra[8], 242);
    assert_eq!(bgra[4], 135);
}

#[test]
fn resize_cancelled_token_throws() {
    // C#: RgbBgraResamplerTests.Resize_CancelledToken_Throws
    let _guard = guard();
    let rgb = vec![0u8; 60 * 60 * 3];
    let mut bgra = vec![0u8; 20 * 20 * 4];
    let result = resize(&rgb, 60, 60, &mut bgra, 20, 20, &cancelled_token(), 3);

    assert_eq!(result, Err(ResampleError::Canceled));
}

#[test]
fn validate_source_length_absurd_or_inconsistent_header_throws_invalid_data_not_overflow() {
    // C#: RgbBgraResamplerTests.ValidateSourceLength_AbsurdOrInconsistentHeader_ThrowsInvalidDataNotOverflow (Theory x4)
    // The C# `uint dataSize` is `i64` here: uint.MaxValue is 4294967295.
    let _guard = guard();
    let cases: [(i32, i32, i64); 4] = [
        (65535, 65535, 4_294_967_295),
        (46341, 46341, 4_294_967_295),
        (1000, 1000, 100),
        (0, 10, 1000),
    ];
    for (width, height, data_size) in cases {
        let result = validate_source_length(width, height, data_size, 3);
        assert!(
            matches!(&result, Err(error) if error.dotnet_type_name() == "InvalidDataException"),
            "{width}x{height} data {data_size}: expected InvalidData, got {result:?}"
        );
    }
}

#[test]
fn validate_source_length_consistent_header_returns_rgb_length() {
    // C#: RgbBgraResamplerTests.ValidateSourceLength_ConsistentHeader_ReturnsRgbLength
    let _guard = guard();
    assert_eq!(
        validate_source_length(6000, 4000, 6000 * 4000 * 3, 3),
        Ok(6000 * 4000 * 3)
    );
}

#[test]
fn validate_target_length_array_too_large_throws_invalid_data_not_overflow() {
    // C#: RgbBgraResamplerTests.ValidateTargetLength_ArrayTooLarge_ThrowsInvalidDataNotOverflow (Theory x2)
    let _guard = guard();
    for (width, height) in [(65535, 65535), (30000, 20000)] {
        let result = validate_target_length(width, height);
        assert!(
            matches!(&result, Err(error) if error.dotnet_type_name() == "InvalidDataException"),
            "{width}x{height}: expected InvalidData, got {result:?}"
        );
    }
}

#[test]
fn validate_target_length_normal_size_returns_bgra_length() {
    // C#: RgbBgraResamplerTests.ValidateTargetLength_NormalSize_ReturnsBgraLength
    let _guard = guard();
    assert_eq!(validate_target_length(300, 200), Ok(300 * 200 * 4));
}

// ---- C#: LibRawManagedLogicTests (resampler part) -------------------------------------------------------------------------

#[test]
fn resize_monochrome_same_size_expands_gray_to_opaque_bgra() {
    // C#: LibRawManagedLogicTests.Resize_MonochromeSameSize_ExpandsGrayToOpaqueBgra
    let _guard = guard();
    let bgra = run(&[0, 90, 180, 255], 2, 2, 2, 2, 1);

    assert_eq!(
        bgra,
        vec![0, 0, 0, 255, 90, 90, 90, 255, 180, 180, 180, 255, 255, 255, 255, 255]
    );
}

#[test]
fn resize_monochrome_box_average_averages_each_target_pixel() {
    // C#: LibRawManagedLogicTests.Resize_MonochromeBoxAverage_AveragesEachTargetPixel
    // 6x2 gray reduced to 2x1 (ratio 3 > 2): left block (0+30)*3/6 = 15, right block (100+200)*3/6 = 150.
    let _guard = guard();
    let gray: [u8; 12] = [0, 0, 0, 100, 100, 100, 30, 30, 30, 200, 200, 200];
    let bgra = run(&gray, 6, 2, 2, 1, 1);

    assert_eq!(bgra, vec![15, 15, 15, 255, 150, 150, 150, 255]);
}

#[test]
fn resize_monochrome_bilinear_interpolates_like_the_rgb_path() {
    // C#: LibRawManagedLogicTests.Resize_MonochromeBilinear_InterpolatesLikeTheRgbPath
    let _guard = guard();
    let bgra = run(&[0, 90, 180, 255], 4, 1, 3, 1, 1);

    assert_eq!(bgra, vec![15, 15, 15, 255, 135, 135, 135, 255, 242, 242, 242, 255]);
}

#[test]
fn resize_monochrome_and_rgb_with_equal_channels_produce_identical_output() {
    // C#: LibRawManagedLogicTests.Resize_MonochromeAndRgbWithEqualChannels_ProduceIdenticalOutput
    let _guard = guard();
    let (source, target) = (30, 7);
    let gray: Vec<u8> = (0..source * source).map(|i| (i * 37 % 251) as u8).collect();
    let rgb: Vec<u8> = gray.iter().flat_map(|&v| [v, v, v]).collect();

    let from_gray = run(&gray, source, source, target, target, 1);
    let from_rgb = run(&rgb, source, source, target, target, 3);

    assert_eq!(from_rgb, from_gray);
}

#[test]
fn validate_source_length_monochrome_uses_one_byte_per_pixel() {
    // C#: LibRawManagedLogicTests.ValidateSourceLength_Monochrome_UsesOneBytePerPixel
    let _guard = guard();
    assert_eq!(validate_source_length(6000, 4000, 6000 * 4000, 1), Ok(6000 * 4000));
    assert!(matches!(
        validate_source_length(6000, 4000, 6000 * 4000 - 1, 1),
        Err(ResampleError::InvalidData(_))
    ));
}

#[test]
fn is_supported_channel_count_only_gray_and_rgb() {
    // C#: LibRawManagedLogicTests.IsSupportedChannelCount_OnlyGrayAndRgb (Theory x5)
    let _guard = guard();
    for (channels, supported) in [(1, true), (3, true), (0, false), (2, false), (4, false)] {
        assert_eq!(is_supported_channel_count(channels), supported, "channels {channels}");
    }
}

#[test]
fn validate_source_length_unsupported_channel_count_throws_invalid_data() {
    // C#: LibRawManagedLogicTests.ValidateSourceLength_UnsupportedChannelCount_ThrowsInvalidData
    let _guard = guard();
    let result = validate_source_length(10, 10, 1000, 4);

    assert_eq!(
        result,
        Err(ResampleError::InvalidData(
            "LibRaw returned an unsupported processed image format.".to_owned()
        ))
    );
}

// ---- C#: RgbBgraResamplerEdgeShapeTests ---------------------------------------------------------------------------------

#[test]
fn resize_monochrome_one_pixel_wide_column_box_averages_each_group_of_rows() {
    // C#: RgbBgraResamplerEdgeShapeTests.Resize_MonochromeOnePixelWideColumn_BoxAveragesEachGroupOfRows
    // 1x8 -> 1x2: ratio 4 > 2 selects the box average; rows 0-3 average 25, rows 4-7 average 65.
    let _guard = guard();
    let actual = run(&[10, 20, 30, 40, 50, 60, 70, 80], 1, 8, 1, 2, 1);

    assert_eq!(actual, gray_bgra(&[25, 65]));
}

#[test]
fn resize_monochrome_one_pixel_high_row_box_averages_each_group_of_columns() {
    // C#: RgbBgraResamplerEdgeShapeTests.Resize_MonochromeOnePixelHighRow_BoxAveragesEachGroupOfColumns
    let _guard = guard();
    let actual = run(&[10, 20, 30, 40, 50, 60, 70, 80], 8, 1, 2, 1, 1);

    assert_eq!(actual, gray_bgra(&[25, 65]));
}

#[test]
fn resize_monochrome_one_pixel_wide_column_bilinear_interpolates_along_the_column_only() {
    // C#: RgbBgraResamplerEdgeShapeTests.Resize_MonochromeOnePixelWideColumn_BilinearInterpolatesAlongTheColumnOnly
    // 1x4 -> 1x3 (ratio 1.33 < 2): source rows 0.1667, 1.5 and 2.8333 of [0,30,60,90] -> 5, 45, 85.
    let _guard = guard();
    let actual = run(&[0, 30, 60, 90], 1, 4, 1, 3, 1);

    assert_eq!(actual, gray_bgra(&[5, 45, 85]));
}

#[test]
fn resize_monochrome_one_pixel_high_row_bilinear_interpolates_along_the_row_only() {
    // C#: RgbBgraResamplerEdgeShapeTests.Resize_MonochromeOnePixelHighRow_BilinearInterpolatesAlongTheRowOnly
    let _guard = guard();
    let actual = run(&[0, 30, 60, 90], 4, 1, 3, 1, 1);

    assert_eq!(actual, gray_bgra(&[5, 45, 85]));
}

#[test]
fn resize_rgb_one_pixel_wide_column_box_averages_without_reading_the_next_column() {
    // C#: RgbBgraResamplerEdgeShapeTests.Resize_RgbOnePixelWideColumn_BoxAveragesWithoutReadingTheNextColumn
    // 1x4 RGB -> 1x1: mean of R, G, B over the four rows; BGRA = (B, G, R, A).
    let _guard = guard();
    let rgb: [u8; 12] = [10, 100, 200, 20, 110, 190, 30, 120, 180, 40, 130, 170];

    let actual = run(&rgb, 1, 4, 1, 1, 3);

    assert_eq!(actual, vec![185, 115, 25, 255]);
}

#[test]
fn resize_rgb_one_pixel_high_row_upscaled_bilinear_keeps_each_pixels_own_colour() {
    // C#: RgbBgraResamplerEdgeShapeTests.Resize_RgbOnePixelHighRowUpscaledBilinear_KeepsEachPixelsOwnColour
    // 2x1 -> 4x1 (ratio 0.5): the outer target pixels sample the source ends exactly.
    let _guard = guard();
    let rgb: [u8; 6] = [0, 0, 0, 200, 100, 50];

    let actual = run(&rgb, 2, 1, 4, 1, 3);

    assert_eq!(actual[..4].to_vec(), vec![0, 0, 0, 255]);
    assert_eq!(actual[12..].to_vec(), vec![50, 100, 200, 255]);
}

#[test]
fn resize_uniform_source_of_degenerate_shape_yields_a_uniform_image_for_every_channel_count() {
    // C#: RgbBgraResamplerEdgeShapeTests.Resize_UniformSourceOfDegenerateShape_YieldsAUniformImageForEveryChannelCount (Theory x6)
    let _guard = guard();
    let shapes: [(i32, i32, i32, i32, i32); 6] = [
        (1, 1, 3, 2, 1),
        (1, 1, 3, 2, 3),
        (1, 5, 4, 5, 1),
        (5, 1, 5, 4, 3),
        (1, 9, 1, 3, 3),
        (9, 1, 3, 1, 1),
    ];
    for (sw, sh, tw, th, channels) in shapes {
        let source = vec![77u8; (sw * sh * channels) as usize];
        let actual = run(&source, sw, sh, tw, th, channels);

        for pixel in actual.chunks_exact(4) {
            assert_eq!(
                pixel.to_vec(),
                vec![77, 77, 77, 255],
                "shape {sw}x{sh} -> {tw}x{th} channels {channels}"
            );
        }
    }
}

// ---- C#: RgbBgraResamplerMutationGapTests ---------------------------------------------------------------------------------

/// Deterministic noise of the C# `Noise(width, height, channels, seed)` helper (xorshift instead of `Random`).
fn noise(width: usize, height: usize, channels: usize, seed: u32) -> Vec<u8> {
    xorshift_bytes(width * height * channels, seed)
}

fn assert_bilinear_matches_reference(channels: usize, sw: i32, sh: i32, tw: i32, th: i32) {
    let source = noise(sw as usize, sh as usize, channels, 11 + channels as u32);
    let expected = reference_bilinear(
        &source,
        i64::from(sw),
        i64::from(sh),
        i64::from(tw),
        i64::from(th),
        channels,
    );

    assert_eq!(
        run(&source, sw, sh, tw, th, channels as i32),
        expected,
        "{sw}x{sh} -> {tw}x{th} channels {channels}"
    );
}

#[test]
fn resize_upscale_or_mild_downscale_matches_the_bilinear_reference_rgb_upscale() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_UpscaleOrMildDownscale_MatchesTheBilinearReference (channels 3, 3x2 -> 5x4)
    let _guard = guard();
    assert_bilinear_matches_reference(3, 3, 2, 5, 4);
}

#[test]
fn resize_upscale_or_mild_downscale_matches_the_bilinear_reference_gray_upscale() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_UpscaleOrMildDownscale_MatchesTheBilinearReference (channels 1, 3x2 -> 5x4)
    let _guard = guard();
    assert_bilinear_matches_reference(1, 3, 2, 5, 4);
}

#[test]
fn resize_upscale_or_mild_downscale_matches_the_bilinear_reference_rgb_mild_downscale() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_UpscaleOrMildDownscale_MatchesTheBilinearReference (channels 3, 7x5 -> 4x3)
    let _guard = guard();
    assert_bilinear_matches_reference(3, 7, 5, 4, 3);
}

#[test]
fn resize_upscale_or_mild_downscale_matches_the_bilinear_reference_gray_mild_downscale() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_UpscaleOrMildDownscale_MatchesTheBilinearReference (channels 1, 7x5 -> 4x3)
    let _guard = guard();
    assert_bilinear_matches_reference(1, 7, 5, 4, 3);
}

fn assert_width_shrink_uses_box(channels: usize) {
    // 40x6 -> 10x12: the width ratio (4) selects the area average even though the height is upscaled.
    let source = noise(40, 6, channels, 21);
    let expected = reference_box(&source, 40, 6, 10, 12, channels);

    assert_eq!(run(&source, 40, 6, 10, 12, channels as i32), expected);
}

fn assert_height_shrink_uses_box(channels: usize) {
    // 6x40 -> 12x10: the height ratio (4) selects the area average even though the width is upscaled.
    let source = noise(6, 40, channels, 22);
    let expected = reference_box(&source, 6, 40, 12, 10, channels);

    assert_eq!(run(&source, 6, 40, 12, 10, channels as i32), expected);
}

#[test]
fn resize_width_shrinks_more_than_twice_while_height_grows_uses_area_averaging_for_the_whole_image_rgb() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_WidthShrinksMoreThanTwiceWhileHeightGrows_UsesAreaAveragingForTheWholeImage (channels 3)
    let _guard = guard();
    assert_width_shrink_uses_box(3);
}

#[test]
fn resize_width_shrinks_more_than_twice_while_height_grows_uses_area_averaging_for_the_whole_image_gray() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_WidthShrinksMoreThanTwiceWhileHeightGrows_UsesAreaAveragingForTheWholeImage (channels 1)
    let _guard = guard();
    assert_width_shrink_uses_box(1);
}

#[test]
fn resize_height_shrinks_more_than_twice_while_width_grows_uses_area_averaging_for_the_whole_image_rgb() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_HeightShrinksMoreThanTwiceWhileWidthGrows_UsesAreaAveragingForTheWholeImage (channels 3)
    let _guard = guard();
    assert_height_shrink_uses_box(3);
}

#[test]
fn resize_height_shrinks_more_than_twice_while_width_grows_uses_area_averaging_for_the_whole_image_gray() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_HeightShrinksMoreThanTwiceWhileWidthGrows_UsesAreaAveragingForTheWholeImage (channels 1)
    let _guard = guard();
    assert_height_shrink_uses_box(1);
}

#[test]
fn resize_reduction_of_exactly_two_on_one_axis_stays_bilinear() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_ReductionOfExactlyTwoOnOneAxis_StaysBilinear
    // 8x4 -> 4x4: the width ratio is exactly the threshold (2.0), which is NOT above it. (2 + 3) / 2 = 2.5 rounds to 2 (even)
    // in bilinear, while the area average would give 3 (half up).
    let _guard = guard();
    let source: Vec<u8> = (0..32).map(|i| if i % 2 == 0 { 2 } else { 3 }).collect();
    let gray = run(&source, 8, 4, 4, 4, 1);

    assert_eq!(gray, reference_bilinear(&source, 8, 4, 4, 4, 1));
    assert_eq!(gray[0], 2);
}

#[test]
fn resize_reduction_of_exactly_two_on_the_height_axis_stays_bilinear() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_ReductionOfExactlyTwoOnTheHeightAxis_StaysBilinear
    let _guard = guard();
    let source: [u8; 16] = [2, 2, 2, 2, 3, 3, 3, 3, 2, 2, 2, 2, 3, 3, 3, 3];
    let gray = run(&source, 4, 4, 4, 2, 1);

    assert_eq!(gray, reference_bilinear(&source, 4, 4, 4, 2, 1));
    assert_eq!(gray[0], 2);
}

#[test]
fn resize_same_size_with_a_cancelled_token_throws_even_for_a_single_pixel() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_SameSizeWithACancelledToken_ThrowsEvenForASinglePixel
    let _guard = guard();
    let mut bgra = [0u8; 4];
    let result = resize(&[0u8; 3], 1, 1, &mut bgra, 1, 1, &cancelled_token(), 3);

    assert_eq!(result, Err(ResampleError::Canceled));
}

#[test]
fn resize_same_size_with_a_cancelled_token_throws_for_a_larger_image() {
    // C#: RgbBgraResamplerMutationGapTests.Resize_SameSizeWithACancelledToken_ThrowsForALargerImage
    let _guard = guard();
    let mut bgra = [0u8; 4 * 16];
    let result = resize(&[0u8; 3 * 16], 4, 4, &mut bgra, 4, 4, &cancelled_token(), 3);

    assert_eq!(result, Err(ResampleError::Canceled));
}

#[test]
fn validate_source_length_zero_height_is_rejected() {
    // C#: RgbBgraResamplerMutationGapTests.ValidateSourceLength_ZeroHeight_IsRejected
    let _guard = guard();
    assert!(matches!(
        validate_source_length(100, 0, 0, 3),
        Err(ResampleError::InvalidData(_))
    ));
}

#[test]
fn validate_source_length_byte_count_of_exactly_int_max_value_is_accepted() {
    // C#: RgbBgraResamplerMutationGapTests.ValidateSourceLength_ByteCountOfExactlyIntMaxValue_IsAccepted
    // The validator only: no 2 GiB array is allocated.
    let _guard = guard();
    assert_eq!(
        validate_source_length(i32::MAX, 1, i64::from(i32::MAX), 1),
        Ok(i32::MAX)
    );
}

#[test]
fn validate_target_length_non_positive_side_is_rejected() {
    // C#: RgbBgraResamplerMutationGapTests.ValidateTargetLength_NonPositiveSide_IsRejected (Theory x4)
    let _guard = guard();
    for (width, height) in [(0, 5), (5, 0), (-1, 5), (5, -1)] {
        assert!(
            matches!(
                validate_target_length(width, height),
                Err(ResampleError::InvalidData(_))
            ),
            "{width}x{height} must be InvalidData"
        );
    }
}

#[test]
fn resize_to_buffer_a_zero_sized_side_is_rejected_as_invalid_data_before_any_work() {
    // C#: RgbBgraResamplerMutationGapTests.ResizeToBuffer_AZeroSizedSide_IsRejectedAsInvalidDataBeforeAnyWork (Theory x4)
    let _guard = guard();
    let cases: [(i32, i32, i32, i32); 4] = [(0, 4, 2, 2), (4, 0, 2, 2), (4, 4, 0, 2), (4, 4, 2, 0)];
    for (sw, sh, tw, th) in cases {
        let before = NativePixelMemory::live_count();
        let result = resize_to_buffer(&[0u8; 48], sw, sh, tw, th, 3, &CancelToken::none());

        assert!(
            matches!(result, Err(ResampleError::InvalidData(_))),
            "{sw}x{sh} -> {tw}x{th} must be InvalidData"
        );
        assert_eq!(
            NativePixelMemory::live_count(),
            before,
            "no native allocation before the guard"
        );
    }
}

// ---- C#: LibRawDecodeMemoryTests (resampler part) -----------------------------------------------------------------------

#[test]
fn resize_to_buffer_four_megapixel_full_size_does_not_allocate_a_managed_bgra_copy() {
    // C#: LibRawDecodeMemoryTests.ResizeToBuffer_FourMegapixelFullSize_DoesNotAllocateAManagedBgraCopy
    // Functional half only: the allocation budget is dropped (no managed GC in Rust).
    let _guard = guard();
    let (width, height) = (2300i32, 1800i32);
    let count = (width * height) as usize;
    let mut gray = vec![0u8; count];
    for i in (0..count).step_by(4099) {
        gray[i] = 200;
    }

    let buffer = resize_to_buffer(&gray, width, height, width, height, 1, &CancelToken::none())
        .unwrap_or_else(|error| panic!("full-size resize failed: {error}"));

    assert_eq!((buffer.width(), buffer.height()), (width, height));
    assert_eq!(buffer.length(), width * height * 4);
    let bytes = buffer.as_slice().expect("buffer is owned");
    let mismatches = (0..count)
        .filter(|&i| {
            let expected: u8 = if i % 4099 == 0 { 200 } else { 0 };
            bytes[i * 4..i * 4 + 4] != [expected, expected, expected, 255]
        })
        .count();
    assert_eq!(mismatches, 0, "pixels that differ from the source");
}

#[test]
fn resize_to_buffer_buffer_disposed_before_the_bitmap_is_read_bitmap_keeps_its_own_pixels() {
    // C#: LibRawDecodeMemoryTests.ResizeToBuffer_BufferDisposedBeforeTheBitmapIsRead_BitmapKeepsItsOwnPixels
    // WPF BitmapSource replaced by take_pixels(): the staging buffer is dropped and the PixelBuffer keeps the pixels.
    let _guard = guard();
    let rgb: [u8; 12] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 10, 20, 30]; // 2x2: red, green / blue, (10,20,30)
    let pixels = {
        let mut staging = resize_to_buffer(&rgb, 2, 2, 2, 2, 3, &CancelToken::none())
            .unwrap_or_else(|error| panic!("resize failed: {error}"));
        staging
            .take_pixels()
            .unwrap_or_else(|error| panic!("take_pixels failed: {error}"))
    };

    assert_eq!(
        pixels.bytes().expect("pixels are alive").to_vec(),
        vec![0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 30, 20, 10, 255]
    );
    assert_eq!(pixels.layout(), PixelLayout::Bgr32);
}

#[test]
fn resize_to_buffer_downscaled_monochrome_matches_resize_into_an_array() {
    // C#: LibRawDecodeMemoryTests.ResizeToBuffer_DownscaledMonochrome_MatchesResizeIntoAnArray
    let _guard = guard();
    let (source, target) = (90, 30);
    let gray: Vec<u8> = (0..source * source).map(|i| (i * 31 % 251) as u8).collect();
    let expected = run(&gray, source, source, target, target, 1);

    let buffer = resize_to_buffer(&gray, source, source, target, target, 1, &CancelToken::none())
        .unwrap_or_else(|error| panic!("resize failed: {error}"));

    assert_eq!(buffer.as_slice().expect("buffer is owned").to_vec(), expected);
}

#[test]
fn resize_to_buffer_cancelled_token_throws_operation_canceled_and_frees_the_buffer() {
    // C#: LibRawDecodeMemoryTests.ResizeToBuffer_CancelledToken_ThrowsOperationCanceledAndFreesTheBuffer
    let _guard = guard();
    let before = NativePixelMemory::live_count();
    let result = resize_to_buffer(&vec![0u8; 60 * 60 * 3], 60, 60, 20, 20, 3, &cancelled_token());

    assert!(matches!(result, Err(ResampleError::Canceled)));
    assert_eq!(
        NativePixelMemory::live_count(),
        before,
        "the staging buffer must be freed on error"
    );
}

#[test]
fn bgra_buffer_disposed_twice_frees_once() {
    // C#: LibRawDecodeMemoryTests.BgraBuffer_DisposedTwice_FreesOnce
    let _guard = guard();
    let before = NativePixelMemory::live_count();
    let mut buffer = BgraBuffer::new(4, 4).unwrap_or_else(|error| panic!("new failed: {error}"));
    assert_eq!(NativePixelMemory::live_count(), before + 1);

    buffer.dispose();
    buffer.dispose();

    assert_eq!(NativePixelMemory::live_count(), before);
    assert!(buffer.pointer().is_null());
}

// ---- C#: LibRawPixelBufferHandOffTests (BgraBuffer part) ---------------------------------------------------------------

#[test]
fn bgra_buffer_dispose_frees_the_native_memory_exactly_once() {
    // C#: LibRawPixelBufferHandOffTests.BgraBuffer_Dispose_FreesTheNativeMemoryExactlyOnce
    let _guard = guard();
    let before = NativePixelMemory::live_count();
    let mut staging = BgraBuffer::new(16, 8).unwrap_or_else(|error| panic!("new failed: {error}"));
    assert_eq!(NativePixelMemory::live_count(), before + 1);

    staging.dispose();
    staging.dispose();

    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
fn bgra_buffer_take_pixels_transfers_ownership_once() {
    // C#: LibRawPixelBufferHandOffTests.BgraBuffer_TakePixels_TransfersOwnershipOnce
    let _guard = guard();
    let before = NativePixelMemory::live_count();
    let mut staging = BgraBuffer::new(3, 2).unwrap_or_else(|error| panic!("new failed: {error}"));

    let mut pixels = staging
        .take_pixels()
        .unwrap_or_else(|error| panic!("take_pixels failed: {error}"));

    assert_eq!((pixels.width(), pixels.height(), pixels.stride()), (3, 2, 12));
    assert_eq!(pixels.layout(), PixelLayout::Bgr32);
    assert!(staging.pointer().is_null());
    assert_eq!(staging.take_pixels().err(), Some(ResampleError::Disposed));
    assert_eq!(staging.as_mut_slice().err(), Some(ResampleError::Disposed));
    staging.dispose();
    assert_eq!(
        NativePixelMemory::live_count(),
        before + 1,
        "the taken PixelBuffer stays alive after dispose"
    );
    assert!(!pixels.is_disposed());
    pixels.dispose();
    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
fn bgra_buffer_alignment_is_the_pixel_buffer_contract() {
    // C#: LibRawPixelBufferHandOffTests.BgraBuffer_Alignment_IsThePixelBufferContract
    let _guard = guard();
    let mut staging = BgraBuffer::new(7, 3).unwrap_or_else(|error| panic!("new failed: {error}"));

    assert_eq!(staging.pointer() as usize % 64, 0);
    assert_eq!(staging.as_mut_slice().expect("buffer is owned").len(), 7 * 4 * 3);
}

// ---- C#: RawErrorHandlingReviewTests -----------------------------------------------------------------------------------

#[test]
fn resize_to_buffer_non_positive_target_throws_invalid_data() {
    // C#: RawErrorHandlingReviewTests.ResizeToBuffer_NonPositiveTarget_ThrowsInvalidData (Theory x3)
    let _guard = guard();
    for (tw, th) in [(0, 10), (10, 0), (-1, 10)] {
        let result = resize_to_buffer(&[0u8; 10 * 10 * 3], 10, 10, tw, th, 3, &CancelToken::none());

        assert!(
            matches!(result, Err(ResampleError::InvalidData(_))),
            "target {tw}x{th} must be InvalidData"
        );
    }
}

#[test]
fn resize_to_buffer_non_positive_source_throws_invalid_data() {
    // C#: RawErrorHandlingReviewTests.ResizeToBuffer_NonPositiveSource_ThrowsInvalidData
    let _guard = guard();
    let result = resize_to_buffer(&[0u8; 3], 0, 1, 1, 1, 3, &CancelToken::none());

    assert!(matches!(result, Err(ResampleError::InvalidData(_))));
}

// ---- Rust-only contract points (contracts/W5-17.md, semantics 1, 5, 6, 9 and "Different") ----------------------------

#[test]
fn select_path_is_the_resize_dispatch_chain() {
    // Contract semantics 1: strict `> 2.0` on either axis, BoxAverage for the whole image; equal sizes are Convert.
    let _guard = guard();
    assert_eq!(BOX_AVERAGE_RATIO_THRESHOLD, 2.0);
    assert_eq!(select_path(4, 4, 4, 4), ResamplePath::Convert);
    assert_eq!(
        select_path(8, 4, 4, 4),
        ResamplePath::Bilinear,
        "ratio exactly 2.0 is bilinear"
    );
    assert_eq!(
        select_path(6, 6, 3, 12),
        ResamplePath::Bilinear,
        "exactly 2.0 on width, height grows"
    );
    assert_eq!(select_path(9, 4, 4, 4), ResamplePath::BoxAverage, "2.25 > 2");
    assert_eq!(
        select_path(40, 6, 10, 12),
        ResamplePath::BoxAverage,
        "width above threshold while height grows"
    );
    assert_eq!(
        select_path(6, 40, 12, 10),
        ResamplePath::BoxAverage,
        "height above threshold while width grows"
    );
    assert_eq!(select_path(2001, 1, 1000, 1), ResamplePath::BoxAverage, "ratio 2.001");
    assert_eq!(select_path(4, 4, 8, 8), ResamplePath::Bilinear, "upscale");
    assert_eq!(
        select_path(4, 4, 0, 4),
        ResamplePath::BoxAverage,
        "zero target gives an infinite ratio"
    );
}

#[test]
fn resize_rejects_non_positive_sizes_and_short_slices_as_invalid_data_without_writing() {
    // Contract "Different": argument errors are InvalidData (no panic) and a failing call leaves bgra untouched.
    let _guard = guard();
    let mut bgra = [7u8; 4];
    assert!(matches!(
        resize(&[0u8; 3], 0, 1, &mut bgra, 1, 1, &CancelToken::none(), 3),
        Err(ResampleError::InvalidData(_))
    ));
    assert!(matches!(
        resize(&[0u8; 2], 1, 1, &mut bgra, 1, 1, &CancelToken::none(), 3),
        Err(ResampleError::InvalidData(_))
    ));
    let mut short = [7u8; 2];
    assert!(matches!(
        resize(&[0u8; 3], 1, 1, &mut short, 1, 1, &CancelToken::none(), 3),
        Err(ResampleError::InvalidData(_))
    ));
    assert_eq!(short, [7, 7], "a failing call must not write");
    assert_eq!(bgra, [7, 7, 7, 7]);
}

#[test]
fn resize_unsupported_channels_is_invalid_data_before_any_size_check() {
    // Contract semantic 5 and the `source-unsupported-channels-before-bad-size` golden case.
    let _guard = guard();
    let mut bgra = [0u8; 4];
    let result = resize(&[0u8; 4], 0, 0, &mut bgra, 1, 1, &CancelToken::none(), 2);

    assert_eq!(
        result,
        Err(ResampleError::InvalidData(
            "LibRaw returned an unsupported processed image format.".to_owned()
        ))
    );
}

#[test]
fn validate_source_length_bad_buffer_length_message_is_the_contract_text() {
    // Contract semantic 6: every failure after the channel check carries this text.
    let _guard = guard();
    let result = validate_source_length(10, 10, 299, 3);

    assert_eq!(
        result,
        Err(ResampleError::InvalidData(
            "LibRaw returned an invalid processed image buffer length.".to_owned()
        ))
    );
}

#[test]
fn max_array_length_is_dotnet_array_max() {
    // Contract semantic 7: `Array.MaxLength` is 0x7FFFFFC7; 536870897 x 1 (2147483588 bytes) is accepted, 536870898 x 1 is not.
    let _guard = guard();
    assert_eq!(MAX_ARRAY_LENGTH, 0x7FFF_FFC7);
    assert_eq!(validate_target_length(536_870_897, 1), Ok(2_147_483_588));
    assert!(matches!(
        validate_target_length(536_870_898, 1),
        Err(ResampleError::InvalidData(_))
    ));
}

#[test]
fn bgra_buffer_new_checks_the_stride_and_length_overflow_before_the_pixel_buffer() {
    // Contract semantic 9: stride and length overflow come first (`Overflow`), then the PixelBuffer range errors.
    let _guard = guard();
    assert_eq!(BgraBuffer::new(-536_870_912, -1).err(), Some(ResampleError::Overflow));
    let error = BgraBuffer::new(-1, -1).expect_err("negative geometry is refused");
    assert_eq!(error.dotnet_type_name(), "ArgumentOutOfRangeException");
}

#[test]
fn bgra_buffer_pointer_is_null_after_dispose() {
    // Contract semantic 10: pointer() is null after dispose and after take_pixels.
    let _guard = guard();
    let mut buffer = BgraBuffer::new(2, 2).unwrap_or_else(|error| panic!("new failed: {error}"));
    assert!(!buffer.pointer().is_null());
    buffer.dispose();
    assert!(buffer.pointer().is_null());
    assert_eq!(buffer.as_slice().err(), Some(ResampleError::Disposed));
}

// ---- Golden helpers and unit tests of the golden spec -----------------------------------------------------------------

/// `tests/golden-inputs/<name>.json`: the inputs of a golden file (same case order as `tests/golden/<name>.json`).
fn golden_input(name: &str) -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[test]
fn golden_source_kinds_match_hand_computed_bytes() {
    // Two hand-computed bytes per kind of the golden source spec (the unit under test is the helper itself).
    assert_eq!(
        source_bytes(&json!({"kind": "mod", "mul": 3, "add": 5}), 101, 1, 1)[..3].to_vec(),
        vec![5, 8, 11]
    );
    assert_eq!(
        source_bytes(&json!({"kind": "mod", "mul": 3, "add": 5}), 101, 1, 1)[100],
        54
    );
    assert_eq!(
        source_bytes(&json!({"kind": "xorshift", "seed": 1}), 4, 1, 1),
        vec![32, 6, 168, 153]
    );
    assert_eq!(source_bytes(&json!({"kind": "solid", "value": 7}), 2, 2, 1), vec![7; 4]);
    assert_eq!(
        source_bytes(&json!({"kind": "solid_rgb", "rgb": [10, 20, 30]}), 2, 1, 3),
        vec![10, 20, 30, 10, 20, 30]
    );
    assert_eq!(
        source_bytes(&json!({"kind": "checker"}), 2, 2, 3),
        vec![255, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255, 255]
    );
    assert_eq!(
        source_bytes(&json!({"kind": "gradient"}), 2, 2, 3),
        vec![0, 50, 100, 7, 57, 107, 13, 63, 113, 20, 70, 120]
    );
    assert_eq!(
        source_bytes(&json!({"kind": "explicit", "bytes": [1, 2, 3]}), 3, 1, 1),
        vec![1, 2, 3]
    );
}

#[test]
fn golden_fnv1a64_matches_the_reference_vectors() {
    assert_eq!(fnv1a64_hex(b""), "cbf29ce484222325");
    assert_eq!(fnv1a64_hex(b"a"), "af63dc4c8601ec8c");
}

// ---- Golden-driven tests ----------------------------------------------------------------------------------------------

/// Compares one `rgb-bgra-resample` case with its C# result. `None` when every field agrees.
fn check_resample_case(input: &Value, want: &Value) -> Option<String> {
    let name = str_of(input, "name");
    if str_of(want, "name") != name {
        return Some(format!("{name}: golden case {} is out of order", str_of(want, "name")));
    }
    let (sw, sh, tw, th) = (
        i64_of(input, "sw") as i32,
        i64_of(input, "sh") as i32,
        i64_of(input, "tw") as i32,
        i64_of(input, "th") as i32,
    );
    let channels = i64_of(input, "channels") as i32;
    let source = source_bytes(&input["source"], sw as usize, sh as usize, channels as usize);
    let token = if input.get("cancelled").and_then(Value::as_bool).unwrap_or(false) {
        cancelled_token()
    } else {
        CancelToken::none()
    };

    let length = (tw * th * 4) as usize;
    let mut out = vec![0u8; length];
    let resize_result = resize(&source, sw, sh, &mut out, tw, th, &token, channels);
    let resize_error = resize_result.as_ref().err().map(ResampleError::dotnet_type_name);
    let buffer_result = resize_to_buffer(&source, sw, sh, tw, th, channels, &token);
    let buffer_error = buffer_result.as_ref().err().map(ResampleError::dotnet_type_name);

    let buffer_length = buffer_result.as_ref().map_or(0, |b| i64::from(b.length()));
    let buffer_aligned = buffer_result.as_ref().is_ok_and(|b| b.pointer() as usize % 64 == 0);
    let buffer_equal = buffer_result
        .as_ref()
        .is_ok_and(|b| resize_error.is_none() && b.as_slice().is_ok_and(|bytes| bytes == out.as_slice()));

    let ours_fnv = resize_error.is_none().then(|| fnv1a64_hex(&out));
    let ours_head = resize_error.is_none().then(|| hex(&out[..32.min(out.len())]));
    let ours_all = (resize_error.is_none() && length <= 96).then(|| hex(&out));

    let mut problems = Vec::new();
    let mut compare = |field: &str, ours: String, theirs: String| {
        if ours != theirs {
            problems.push(format!("{field}: C# {theirs}, Rust {ours}"));
        }
    };
    compare(
        "resize_error",
        format!("{resize_error:?}"),
        format!("{:?}", want["resize_error"].as_str()),
    );
    compare(
        "buffer_error",
        format!("{buffer_error:?}"),
        format!("{:?}", want["buffer_error"].as_str()),
    );
    compare("length", length.to_string(), i64_of(want, "length").to_string());
    compare(
        "buffer_length",
        buffer_length.to_string(),
        i64_of(want, "buffer_length").to_string(),
    );
    compare(
        "buffer_aligned",
        buffer_aligned.to_string(),
        bool_of(want, "buffer_aligned").to_string(),
    );
    compare(
        "buffer_equal",
        buffer_equal.to_string(),
        bool_of(want, "buffer_equal").to_string(),
    );
    compare(
        "fnv1a64",
        format!("{ours_fnv:?}"),
        format!("{:?}", want["fnv1a64"].as_str()),
    );
    compare(
        "head_hex",
        format!("{ours_head:?}"),
        format!("{:?}", want["head_hex"].as_str()),
    );
    compare(
        "all_hex",
        format!("{ours_all:?}"),
        format!("{:?}", want["all_hex"].as_str()),
    );

    (!problems.is_empty()).then(|| format!("{name}: {}", problems.join("; ")))
}

#[test]
fn rgb_bgra_resample_golden_matches_csharp() {
    // C#: RgbBgraResampler.Resize / ResizeToBuffer (oracle `rgb-bgra-resample`, 176 cases).
    let _guard = guard();
    let expected = golden::load("rgb-bgra-resample");
    let inputs = golden_input("rgb-bgra-resample");
    let inputs = inputs["cases"].as_array().expect("golden input has cases");
    assert_eq!(
        inputs.len(),
        expected.cases.len(),
        "input and golden case counts differ"
    );

    let mismatches: Vec<String> = inputs
        .iter()
        .zip(&expected.cases)
        .filter_map(|(input, want)| check_resample_case(input, want))
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} rgb-bgra-resample cases differ from C#:\n{}",
        mismatches.len(),
        inputs.len(),
        mismatches.iter().take(30).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// Compares one `rgb-bgra-validate` case with its C# result. `None` when it agrees.
fn check_validate_case(input: &Value, want: &Value) -> Option<String> {
    let name = str_of(input, "name");
    if str_of(want, "name") != name {
        return Some(format!("{name}: golden case {} is out of order", str_of(want, "name")));
    }
    let op = str_of(input, "op");
    let outcome: Result<Value, ResampleError> = match op {
        "source" => validate_source_length(
            i64_of(input, "width") as i32,
            i64_of(input, "height") as i32,
            i64_of(input, "data_size"),
            i64_of(input, "channels") as i32,
        )
        .map(|v| json!(v)),
        "target" => {
            validate_target_length(i64_of(input, "width") as i32, i64_of(input, "height") as i32).map(|v| json!(v))
        }
        "supported" => Ok(json!(is_supported_channel_count(i64_of(input, "channels") as i32))),
        "buffer-new" => BgraBuffer::new(i64_of(input, "width") as i32, i64_of(input, "height") as i32).map(|b| {
            json!({
                "width": b.width(),
                "height": b.height(),
                "stride": b.stride(),
                "length": b.length(),
                "aligned": b.pointer() as usize % 64 == 0,
            })
        }),
        "resize-to-buffer" => {
            let source = vec![0u8; i64_of(input, "source_len") as usize];
            resize_to_buffer(
                &source,
                i64_of(input, "sw") as i32,
                i64_of(input, "sh") as i32,
                i64_of(input, "tw") as i32,
                i64_of(input, "th") as i32,
                i64_of(input, "channels") as i32,
                &CancelToken::none(),
            )
            .map(|b| json!({"length": b.length(), "stride": b.stride()}))
        }
        other => panic!("unknown validate op {other}"),
    };

    match outcome {
        Ok(value) => {
            if !want["error"].is_null() {
                Some(format!("{name}: C# error {}, Rust ok {value}", want["error"]))
            } else if want["ok"] != value {
                Some(format!("{name}: C# ok {}, Rust ok {value}", want["ok"]))
            } else {
                None
            }
        }
        Err(error) => {
            let ours_message = normalize_newlines(&error.to_string());
            let theirs_type = want["error"]["type"].as_str();
            let theirs_message = want["error"]["message"].as_str().map(normalize_newlines);
            if theirs_type == Some(error.dotnet_type_name()) && theirs_message.as_deref() == Some(ours_message.as_str())
            {
                None
            } else {
                Some(format!(
                    "{name}: C# {} {:?}, Rust {} {ours_message:?}",
                    theirs_type.unwrap_or("ok"),
                    theirs_message,
                    error.dotnet_type_name()
                ))
            }
        }
    }
}

#[test]
fn rgb_bgra_validate_golden_matches_csharp() {
    // C#: RgbBgraResampler.ValidateSourceLength / ValidateTargetLength / IsSupportedChannelCount / BgraBuffer
    // (oracle `rgb-bgra-validate`, 113 cases).
    let _guard = guard();
    let expected = golden::load("rgb-bgra-validate");
    let inputs = golden_input("rgb-bgra-validate");
    let inputs = inputs["cases"].as_array().expect("golden input has cases");
    assert_eq!(
        inputs.len(),
        expected.cases.len(),
        "input and golden case counts differ"
    );

    let mismatches: Vec<String> = inputs
        .iter()
        .zip(&expected.cases)
        .filter_map(|(input, want)| check_validate_case(input, want))
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} rgb-bgra-validate cases differ from C#:\n{}",
        mismatches.len(),
        inputs.len(),
        mismatches.iter().take(30).cloned().collect::<Vec<_>>().join("\n")
    );
}
