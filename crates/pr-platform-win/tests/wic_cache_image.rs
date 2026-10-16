//! Red tests for W4-11e: `wic::cache_image` (C# `Caching/WicImageEncoder.cs` and `Caching/WicCacheImageReader.cs`, baseline
//! d92d6aae). Port of `tests/PhotoReview.Imaging.Tests/Caching/WicCacheImageIoTests.cs` (12 of its 15 methods; the rest belong to
//! W4-08a `AtomicCacheFile` and W4-10b `ThumbnailCache`) plus the golden-driven tests over `tests/golden/wic-bgrx-to-bgr.json`,
//! `wic-cache-classify.json` and `wic-cache-codec.json`, plus the pure tests the contract lists as new.
//!
//! Every test here fails through `todo!()` until the porter implements the bodies. Native tests (they call WIC) are
//! `#[ignore = "native: needs WIC"]`; run them with `-- --ignored` on Windows.
//!
//! Documented skips (not portable here):
//! * `Encode_BadArguments_Throw`, the `null` stream half: a `&mut dyn Write` cannot be null (contract note 1).
//! * Adapter mapping tables of contract note 11 (`WicCacheCodec` as `PngEncoder` / `PreviewPayloadCodec` /
//!   `CacheImageReader`): the traits are not in the tree yet; added with the wiring step.
//! * `WicCacheImageIoTests.DiskThumbnail_*` (W4-10b), `AtomicWrite_*` (W4-08a).
//! * `Wp06BridgeSplitTests` `Resize_EqualsWpfScaleTransform`: WPF `TransformedBitmap` has no Rust twin; the golden `fnv` column of
//!   `wic-scale-resize` replaces it (see `wic_scaler.rs`).
#![cfg(windows)]

use pr_core::catalog::explorer_types::Guid;
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::wic::cache_image::{
    BAND_BYTES, CONTAINER_FORMAT_JPEG, CONTAINER_FORMAT_PNG, CacheImageError, DPI, DestinationTooShort, E_FAIL,
    E_OUTOFMEMORY, EncodeError, EncodePath, IMAGE_QUALITY_PROPERTY, INVALID_DATA_DIMENSIONS, INVALID_DATA_EMPTY,
    INVALID_DATA_HEADER_MISMATCH, INVALID_DATA_TOO_LARGE, MAX_DECODED_BYTES, OPAQUE_FORMATS, PIXEL_FORMAT_24BPP_BGR,
    PIXEL_FORMAT_32BPP_BGR, PIXEL_FORMAT_32BPP_BGRA, PIXEL_FORMAT_32BPP_PBGRA, PIXEL_FORMAT_48BPP_BGR,
    PIXEL_FORMAT_BLACK_WHITE, VENDOR_MICROSOFT, WIC_BITMAP_ENCODER_NO_CACHE, bgrx32_to_bgr24, decode, encode_jpeg,
    encode_png, is_opaque_format, is_out_of_memory, plan_decode, plan_encode_path, rows_per_band, source_format,
    target_layout,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers (twins of C# `LegacyWpfPreviewCache.PatternPixels` and `PixelAssert`; the golden pattern is the one of the
// oracle file `tools/oracle-cs/Commands/W4-11e.cs`, `W411ePattern`)
// ---------------------------------------------------------------------------------------------------------------------

/// Byte `i` (row-major, no padding) = `(i * 131 + seed * 17 + i / 251) mod 256`, 64-bit arithmetic.
fn pattern_byte(index: u64, seed: u64) -> u8 {
    ((index * 131 + seed * 17 + index / 251) % 256) as u8
}

/// The golden pattern. With `alpha` (Pbgra32 only) byte 3 of pixel (x, y) is `255 - x * 255 / max(width - 1, 1)` and the colour
/// bytes are clamped to it, so the buffer is valid premultiplied data.
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

/// C# `LegacyWpfPreviewCache.Pattern(width, height, alpha: false)` as a `PixelBuffer`: the smooth gradient checkerboard
/// (the C# `PatternPixels` of the quality and round-trip tests, unlike the noisy golden pattern).
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

/// Colour channels that are part of the contract: B, G, R of every pixel for Bgr32 (byte 3 is WIC-defined), all four for Pbgra32.
fn meaningful_bytes(px: &PixelBuffer) -> Vec<u8> {
    let channels = if px.layout() == PixelLayout::Pbgra32 { 4 } else { 3 };
    let mut out = Vec::new();
    for y in 0..px.height() {
        let row = px.get_row(y).expect("not disposed");
        for x in 0..px.width() as usize {
            out.extend_from_slice(&row[x * 4..x * 4 + channels]);
        }
    }
    out
}

/// FNV-1a 64 over `bytes`, 16 lowercase hex digits (golden `fnv` / `encodedFnv` columns).
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

/// Mean squared error over the meaningful channels, PSNR in dB (255 peak). Infinite for identical pixels.
fn psnr(expected: &PixelBuffer, actual: &PixelBuffer) -> f64 {
    let a = meaningful_bytes(expected);
    let b = meaningful_bytes(actual);
    assert_eq!(a.len(), b.len(), "pixel counts differ");
    let mse: f64 = a
        .iter()
        .zip(&b)
        .map(|(x, y)| {
            let d = f64::from(*x) - f64::from(*y);
            d * d
        })
        .sum::<f64>()
        / a.len() as f64;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0_f64 * 255.0 / mse).log10()
    }
}

/// C# `PixelAssert.Equal(expected, actual, compareUnusedByte: false)`: geometry, layout and meaningful bytes.
fn assert_same_pixels(expected: &PixelBuffer, actual: &PixelBuffer) {
    assert_eq!(
        (expected.width(), expected.height(), expected.layout()),
        (actual.width(), actual.height(), actual.layout()),
        "geometry differs"
    );
    assert_eq!(meaningful_bytes(expected), meaningful_bytes(actual), "pixels differ");
}

/// Parses `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` in .NET `Guid.ToByteArray()` order (first three fields little-endian).
fn guid_from_text(text: &str) -> Guid {
    let hex: Vec<u8> = text
        .replace('-', "")
        .as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex"))
        .collect();
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&[hex[3], hex[2], hex[1], hex[0]]);
    bytes[4..6].copy_from_slice(&[hex[5], hex[4]]);
    bytes[6..8].copy_from_slice(&[hex[7], hex[6]]);
    bytes[8..16].copy_from_slice(&hex[8..16]);
    Guid(bytes)
}

/// The input rows of a golden (`tests/golden-inputs/<name>.json`): the golden outputs do not repeat the inputs.
fn golden_input_cases(name: &str) -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"].as_array().expect("cases array").clone()
}

/// The input row named `name` of [`golden_input_cases`].
fn golden_input(inputs: &[Value], name: &str) -> Value {
    inputs
        .iter()
        .find(|c| c["name"].as_str() == Some(name))
        .cloned()
        .unwrap_or_else(|| panic!("golden input {name} missing"))
}

/// Textual `Guid` as the wincodec.h `u128` (data1 as the most significant 32 bits), comparable with `windows_core::GUID::to_u128`.
fn textual_u128(guid: &Guid) -> u128 {
    let b = guid.0;
    let data1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let data2 = u16::from_le_bytes([b[4], b[5]]);
    let data3 = u16::from_le_bytes([b[6], b[7]]);
    let data4 = u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]);
    (u128::from(data1) << 96) | (u128::from(data2) << 80) | (u128::from(data3) << 64) | u128::from(data4)
}

fn layout_of(text: &str) -> PixelLayout {
    if text == "pbgra32" {
        PixelLayout::Pbgra32
    } else {
        PixelLayout::Bgr32
    }
}

fn layout_name(layout: PixelLayout) -> &'static str {
    if layout == PixelLayout::Pbgra32 {
        "pbgra32"
    } else {
        "bgr32"
    }
}

fn hex_decode(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex"))
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// PNG of `pixels` through the port (only used by tests that need a real payload).
fn png_of(pixels: &PixelBuffer) -> Vec<u8> {
    let mut out = Vec::new();
    encode_png(pixels, &mut out).expect("encode_png");
    out
}

fn jpeg_of(pixels: &PixelBuffer, quality: i32) -> Vec<u8> {
    let mut out = Vec::new();
    encode_jpeg(pixels, &mut out, quality).expect("encode_jpeg");
    out
}

/// `NativePixelMemory::live_count` is process-global: every test of this file runs under one lock so the live-count
/// assertions are not disturbed by buffers other tests hold (the default test runner is multi-threaded).
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------------------------------------------------------------
// Pure tests
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn bgrx32_to_bgr24_drops_fourth_byte() {
    let _serial = serial();
    // C#: WicImageEncoder.Bgrx32ToBgr24_DropsFourthByte (WicCacheImageIoTests). Theory rows 1, 3, 4, 5, 17, 64.
    for pixel_count in [1usize, 3, 4, 5, 17, 64] {
        let source: Vec<u8> = (0..pixel_count * 4)
            .map(|i| if i % 4 == 3 { 0xEE } else { ((i * 7) + 1) as u8 })
            .collect();
        // C# allocates 16 bytes of slack for its vector stores; the Rust port must not need it (contract note 5).
        let mut destination = vec![0u8; pixel_count * 3];

        bgrx32_to_bgr24(&source, &mut destination).expect("destination is 3 bytes per pixel");

        for p in 0..pixel_count {
            assert_eq!(destination[p * 3], source[p * 4], "pixel {p} of {pixel_count}: B");
            assert_eq!(
                destination[p * 3 + 1],
                source[p * 4 + 1],
                "pixel {p} of {pixel_count}: G"
            );
            assert_eq!(
                destination[p * 3 + 2],
                source[p * 4 + 2],
                "pixel {p} of {pixel_count}: R"
            );
        }
    }
}

#[test]
fn bgrx32_to_bgr24_too_short_destination_throws() {
    let _serial = serial();
    // C#: WicImageEncoder.Bgrx32ToBgr24_TooShortDestination_Throws (ArgumentException; here DestinationTooShort).
    let mut destination = vec![0xAAu8; 11];
    let result = bgrx32_to_bgr24(&[0u8; 16], &mut destination);

    assert_eq!(
        result,
        Err(DestinationTooShort {
            required: 12,
            actual: 11
        })
    );
    assert!(
        destination.iter().all(|&b| b == 0xAA),
        "nothing is written when the destination is too short"
    );
}

#[test]
fn bgrx32_to_bgr24_golden_matches_csharp() {
    let _serial = serial();
    // Golden: tests/golden/wic-bgrx-to-bgr.json (13 cases). Source byte i is 0xEE for the 4th byte of a pixel, else (i * 7 + 1).
    let golden = golden::load("wic-bgrx-to-bgr");
    let inputs = golden_input_cases("wic-bgrx-to-bgr");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = golden_input(&inputs, name);
        let source_len = i64_of(&input, "sourceLen") as usize;
        let dest_len = i64_of(&input, "destLen") as usize;
        let source: Vec<u8> = (0..source_len)
            .map(|i| if i % 4 == 3 { 0xEE } else { ((i * 7) + 1) as u8 })
            .collect();
        let mut destination = vec![0u8; dest_len];

        let result = bgrx32_to_bgr24(&source, &mut destination);

        match case["throws"].as_str() {
            Some("ArgumentException") => {
                assert!(result.is_err(), "{name}: expected DestinationTooShort");
            }
            Some(other) => panic!("{name}: unexpected golden throw {other}"),
            None => {
                assert!(result.is_ok(), "{name}: unexpected error {result:?}");
                let compared = (source_len / 4 * 3).min(dest_len);
                let expected = str_of(case, "hex");
                assert_eq!(hex_encode(&destination[..compared]), expected, "{name}");
            }
        }
    }
}

#[test]
fn encode_jpeg_quality_out_of_range_throws() {
    let _serial = serial();
    // C#: WicImageEncoder.EncodeJpeg_QualityOutOfRange_Throws (rows 0, 101, -5): checked before anything else is touched.
    let pixels = pattern(16, 16, PixelLayout::Bgr32, 0, false);
    for quality in [0, 101, -5] {
        let mut out = Vec::new();
        match encode_jpeg(&pixels, &mut out, quality) {
            Err(EncodeError::QualityOutOfRange { quality: q }) => assert_eq!(q, quality),
            other => panic!("quality {quality}: expected QualityOutOfRange, got {other:?}"),
        }
        assert!(out.is_empty(), "quality {quality}: nothing is written");
    }
}

#[test]
fn encode_bad_arguments_throw() {
    let _serial = serial();
    // C#: WicImageEncoder.Encode_BadArguments_Throw, the disposed-buffer half (ObjectDisposedException => Disposed).
    // The `null` stream half has no Rust twin (contract note 1, documented in the file header).
    let mut pixels = pattern(8, 8, PixelLayout::Bgr32, 0, false);
    pixels.dispose();
    let mut out = Vec::new();

    assert!(matches!(encode_png(&pixels, &mut out), Err(EncodeError::Disposed)));
    assert!(out.is_empty(), "a disposed buffer writes nothing");
}

#[test]
fn is_out_of_memory_only_for_e_outofmemory() {
    let _serial = serial();
    // C#: WicCacheImageReader.IsOutOfMemory_OnlyForEOutOfMemory. The COM exception itself is replaced by its HRESULT.
    assert!(is_out_of_memory(E_OUTOFMEMORY));
    assert!(is_out_of_memory(0x8007_000E_u32 as i32));
    assert!(
        !is_out_of_memory(0x8898_2F60_u32 as i32),
        "WINCODEC_ERR_* is not out-of-memory"
    );
    assert!(!is_out_of_memory(E_FAIL));
    assert!(!is_out_of_memory(0));
}

#[test]
fn is_out_of_memory_golden_matches_csharp() {
    let _serial = serial();
    // Golden: tests/golden/wic-cache-classify.json (24 cases): HRESULT classification and the opaque-format set.
    let golden = golden::load("wic-cache-classify");
    let inputs = golden_input_cases("wic-cache-classify");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = golden_input(&inputs, name);
        let hresult = i64_of(&input, "hresult") as i32;
        let guid = guid_from_text(str_of(&input, "guid"));
        assert_eq!(
            is_out_of_memory(hresult),
            bool_of(case, "isOutOfMemory"),
            "{name}: isOutOfMemory"
        );
        assert_eq!(is_opaque_format(&guid), bool_of(case, "isOpaque"), "{name}: isOpaque");
    }
}

#[test]
fn opaque_formats_are_exactly_the_fourteen_listed_guids() {
    let _serial = serial();
    // C#: WicCacheImageReader.OpaqueFormats (same list as WicDirectDecoder). Exact 16-byte membership, no prefix matching.
    assert_eq!(OPAQUE_FORMATS.len(), 14);
    assert!(is_opaque_format(&PIXEL_FORMAT_BLACK_WHITE));
    assert!(is_opaque_format(&PIXEL_FORMAT_48BPP_BGR));
    assert!(
        !is_opaque_format(&PIXEL_FORMAT_32BPP_BGRA),
        "alpha format is not opaque"
    );
    assert!(
        !is_opaque_format(&PIXEL_FORMAT_32BPP_PBGRA),
        "alpha format is not opaque"
    );
    let mut near_miss = PIXEL_FORMAT_32BPP_BGR;
    near_miss.0[15] ^= 0x01;
    assert!(
        !is_opaque_format(&near_miss),
        "same prefix, different GUID is not opaque"
    );
    assert!(!is_opaque_format(&Guid([0u8; 16])), "empty GUID is not opaque");
}

#[test]
fn empty_payload_is_invalid_data_entry_failure() {
    let _serial = serial();
    // C#: WicCacheImageReader.Decode of an empty payload, first half of EmptyOrGarbagePayload_IsEntryFailure
    // (InvalidDataException, a cache-entry failure). WIC must not be touched.
    let result = decode(&[], Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, 0, 0);

    match &result {
        Err(CacheImageError::InvalidData(text)) => assert_eq!(*text, INVALID_DATA_EMPTY),
        other => panic!("expected InvalidData(EMPTY), got {:?}", other.as_ref().err()),
    }
    assert!(result.err().is_some_and(|e| e.is_entry_failure()));
}

#[test]
fn is_entry_failure_table() {
    let _serial = serial();
    // Contract "New Rust tests": InvalidData and Com are entry failures; InsufficientMemory and Pixels(OutOfMemory) are not.
    use pr_imaging::pixels::PixelBufferError;
    assert!(CacheImageError::InvalidData(INVALID_DATA_EMPTY).is_entry_failure());
    assert!(CacheImageError::Com { hresult: E_FAIL }.is_entry_failure());
    assert!(!CacheImageError::InsufficientMemory { hresult: E_OUTOFMEMORY }.is_entry_failure());
    assert!(!CacheImageError::Pixels(PixelBufferError::OutOfMemory { byte_count: 4 }).is_entry_failure());
}

#[test]
fn error_display_texts_are_byte_identical_to_csharp() {
    let _serial = serial();
    // Contract "New Rust tests": Display of every error variant; the four InvalidData texts are fixed (callers log them).
    assert_eq!(INVALID_DATA_EMPTY, "Cache entry payload is empty.");
    assert_eq!(INVALID_DATA_DIMENSIONS, "Cache entry payload has invalid dimensions.");
    assert_eq!(
        INVALID_DATA_HEADER_MISMATCH,
        "Cache entry payload dimensions do not match its header."
    );
    assert_eq!(INVALID_DATA_TOO_LARGE, "Cache entry payload is implausibly large.");
    assert_eq!(
        CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE).to_string(),
        INVALID_DATA_TOO_LARGE
    );
    assert_eq!(
        CacheImageError::InsufficientMemory { hresult: E_OUTOFMEMORY }.to_string(),
        "Not enough memory to decode the cache entry."
    );
    assert!(
        CacheImageError::Com { hresult: E_FAIL }
            .to_string()
            .contains("80004005")
    );
    assert_eq!(
        DestinationTooShort {
            required: 12,
            actual: 11
        }
        .to_string(),
        "Destination is too short."
    );
    assert!(!EncodeError::Disposed.to_string().is_empty());
    assert!(EncodeError::QualityOutOfRange { quality: 0 }.to_string().contains('0'));
}

#[test]
fn plan_decode_rejects_zero_and_oversized_dimensions() {
    let _serial = serial();
    // Contract "New Rust tests": zero width/height, 0x8000_0000 and u32::MAX on each axis => INVALID_DATA_DIMENSIONS.
    let cases: [(u32, u32); 8] = [
        (0, 10),
        (10, 0),
        (0x8000_0000, 10),
        (10, 0x8000_0000),
        (u32::MAX, 10),
        (10, u32::MAX),
        (0, 0),
        (u32::MAX, u32::MAX),
    ];
    for (w, h) in cases {
        match plan_decode(w, h, DecodeBox::UNBOUNDED, 0, 0) {
            Err(CacheImageError::InvalidData(text)) => assert_eq!(text, INVALID_DATA_DIMENSIONS, "{w}x{h}"),
            other => panic!("{w}x{h}: expected InvalidData(DIMENSIONS), got {other:?}"),
        }
    }
}

#[test]
fn plan_decode_header_mismatch_in_either_axis_and_skipped_without_expected_width() {
    let _serial = serial();
    // C#: WicCacheImageReader.Decode header check. Both dimensions compared; expected_height ignored when expected_width == 0.
    for (ew, eh) in [(31, 20), (32, 21), (31, 21)] {
        match plan_decode(32, 20, DecodeBox::UNBOUNDED, ew, eh) {
            Err(CacheImageError::InvalidData(text)) => assert_eq!(text, INVALID_DATA_HEADER_MISMATCH),
            other => panic!("expected {ew}x{eh} to mismatch, got {other:?}"),
        }
    }
    let plan = plan_decode(32, 20, DecodeBox::UNBOUNDED, 32, 20).expect("matching header");
    assert_eq!((plan.width, plan.height, plan.downscaled), (32, 20, false));
    let skipped = plan_decode(32, 20, DecodeBox::UNBOUNDED, 0, 999).expect("expected_width 0 skips the check");
    assert_eq!((skipped.source_width, skipped.source_height), (32, 20));
}

#[test]
fn plan_decode_box_shrinks_400x200_to_100x50_downscaled() {
    let _serial = serial();
    // Contract "New Rust tests": box (100, 0) on a 400x200 frame => 100x50, downscaled, source size reported.
    let plan = plan_decode(400, 200, DecodeBox::new(100, 0), 0, 0).expect("valid");
    assert_eq!(
        plan,
        pr_platform_win::wic::cache_image::DecodePlan {
            source_width: 400,
            source_height: 200,
            width: 100,
            height: 50,
            downscaled: true,
        }
    );
}

#[test]
fn plan_decode_box_larger_than_frame_keeps_frame_size() {
    let _serial = serial();
    // C# Box_ShrinksButNeverUpscales (the "same" half): a box larger than the frame never upscales.
    let plan = plan_decode(400, 200, DecodeBox::new(4000, 0), 0, 0).expect("valid");
    assert_eq!((plan.width, plan.height, plan.downscaled), (400, 200, false));
}

#[test]
fn plan_decode_i32_max_square_is_accepted_when_shrunk_and_too_large_unbounded() {
    let _serial = serial();
    // Contract "New Rust tests" (trap 3): i32::MAX square with a (1, 1) box is accepted; unbounded => TOO_LARGE, no panic.
    let max = i32::MAX as u32;
    assert!(plan_decode(max, max, DecodeBox::new(1, 1), 0, 0).is_ok());
    match plan_decode(max, max, DecodeBox::UNBOUNDED, 0, 0) {
        Err(CacheImageError::InvalidData(text)) => assert_eq!(text, INVALID_DATA_TOO_LARGE),
        other => panic!("expected TOO_LARGE, got {other:?}"),
    }
}

#[test]
fn plan_decode_max_square_unbounded_is_too_large_without_panic() {
    let _serial = serial();
    // Contract trap 3: i32::MAX x i32::MAX unbounded => TOO_LARGE; the product is u64, so it must not overflow or panic.
    let result = plan_decode(i32::MAX as u32, i32::MAX as u32, DecodeBox::UNBOUNDED, 0, 0);
    assert!(matches!(
        result,
        Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
    ));
    assert_eq!(MAX_DECODED_BYTES, i64::from(i32::MAX));
}

#[test]
fn plan_encode_path_table() {
    let _serial = serial();
    // Contract "New Rust tests": accepted == source => Direct; accepted == 24bppBGR => Bgr24Bands; other => WicConversion.
    assert_eq!(
        plan_encode_path(&PIXEL_FORMAT_32BPP_BGR, &PIXEL_FORMAT_32BPP_BGR),
        EncodePath::Direct
    );
    assert_eq!(
        plan_encode_path(&PIXEL_FORMAT_32BPP_PBGRA, &PIXEL_FORMAT_24BPP_BGR),
        EncodePath::Bgr24Bands
    );
    assert_eq!(
        plan_encode_path(&PIXEL_FORMAT_32BPP_PBGRA, &PIXEL_FORMAT_32BPP_BGRA),
        EncodePath::WicConversion
    );
    assert_eq!(
        plan_encode_path(&PIXEL_FORMAT_32BPP_BGR, &PIXEL_FORMAT_48BPP_BGR),
        EncodePath::WicConversion
    );
}

#[test]
fn rows_per_band_table() {
    let _serial = serial();
    // Contract "New Rust tests": 1x1; width 87 382 => 1 row (quotient 0 clamped); height smaller than the quotient;
    // width BAND_BYTES / 3 => 1 row.
    assert_eq!(rows_per_band(1, 1), 1);
    assert_eq!(rows_per_band(87_382, 10), 1);
    assert_eq!(rows_per_band(1, 5), 5, "clamped to the height");
    assert_eq!(rows_per_band((BAND_BYTES / 3) as i32, 10), 1);
    assert_eq!(
        rows_per_band(1, 1_000_000),
        (BAND_BYTES / 3) as i32,
        "BAND_BYTES / (width * 3) for a narrow image"
    );
}

#[test]
fn source_format_and_target_layout() {
    let _serial = serial();
    // C# WicImageEncoder.SourceFormat and WicCacheImageReader target layout (contract note 3 (d)).
    assert_eq!(source_format(PixelLayout::Pbgra32), PIXEL_FORMAT_32BPP_PBGRA);
    assert_eq!(source_format(PixelLayout::Bgr32), PIXEL_FORMAT_32BPP_BGR);
    // requested wins over the frame format
    assert_eq!(
        target_layout(Some(PixelLayout::Pbgra32), &PIXEL_FORMAT_BLACK_WHITE),
        PixelLayout::Pbgra32
    );
    // every opaque GUID => Bgr32 when nothing is requested
    for guid in OPAQUE_FORMATS {
        assert_eq!(target_layout(None, &guid), PixelLayout::Bgr32);
    }
    assert_eq!(
        target_layout(None, &PIXEL_FORMAT_32BPP_BGRA),
        PixelLayout::Pbgra32,
        "alpha GUID"
    );
    assert_eq!(
        target_layout(None, &Guid([0u8; 16])),
        PixelLayout::Pbgra32,
        "empty GUID"
    );
}

#[test]
fn container_and_pixel_format_guids_match_wincodec_h() {
    let _serial = serial();
    // Contract "New Rust tests": each constant against its textual GUID (wincodec.h) and against wic::interop::guids.
    assert_eq!(
        CONTAINER_FORMAT_JPEG,
        guid_from_text("19e4a5aa-5662-4fc5-a0c0-1758028e1057")
    );
    assert_eq!(
        CONTAINER_FORMAT_PNG,
        guid_from_text("1b7cfaf4-713f-473c-bbcd-6137425faeaf")
    );
    assert_eq!(VENDOR_MICROSOFT, guid_from_text("f0e749ca-edef-4589-a73a-ee0e626a2a2b"));
    assert_eq!(
        PIXEL_FORMAT_32BPP_PBGRA,
        guid_from_text("6fddc324-4e03-4bfe-b185-3d77768dc910")
    );
    assert_eq!(
        PIXEL_FORMAT_48BPP_BGR,
        guid_from_text("e605a384-b468-46ce-bb2e-36f180e64313")
    );

    use pr_platform_win::wic::interop::guids as wic;
    assert_eq!(
        textual_u128(&PIXEL_FORMAT_32BPP_BGR),
        wic::PIXEL_FORMAT_32BPP_BGR.to_u128()
    );
    assert_eq!(
        textual_u128(&CONTAINER_FORMAT_JPEG),
        wic::CONTAINER_FORMAT_JPEG.to_u128()
    );
    assert_eq!(WIC_BITMAP_ENCODER_NO_CACHE, 2);
    assert!((DPI - 96.0).abs() < f64::EPSILON);
    assert_eq!(IMAGE_QUALITY_PROPERTY, "ImageQuality");
}

#[test]
fn golden_cache_codec_quality_errors_are_pure() {
    let _serial = serial();
    // Golden wic-cache-codec rows jpeg-quality-{zero,101,negative}-throws: encodeThrows ArgumentOutOfRangeException (pure).
    let golden = golden::load("wic-cache-codec");
    let pixels = pattern(16, 16, PixelLayout::Bgr32, 0, false);
    // The golden result rows do not carry the quality; it is a fixed input of the oracle case list (tests/golden-inputs).
    let rows = [
        ("jpeg-quality-zero-throws", 0),
        ("jpeg-quality-101-throws", 101),
        ("jpeg-quality-negative-throws", -5),
    ];
    for (name, quality) in rows {
        let case = golden
            .cases
            .iter()
            .find(|c| str_of(c, "name") == name)
            .unwrap_or_else(|| panic!("golden case {name} missing"));
        assert_eq!(str_of(case, "encodeThrows"), "ArgumentOutOfRangeException", "{name}");
        let mut out = Vec::new();
        assert!(
            matches!(
                encode_jpeg(&pixels, &mut out, quality),
                Err(EncodeError::QualityOutOfRange { .. })
            ),
            "{name}: encodeThrows ArgumentOutOfRangeException"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Native tests (WIC). Each one is ignored until run on Windows with `-- --ignored`.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs WIC"]
fn encode_jpeg_quality_changes_payload() {
    let _serial = serial();
    // C#: WicImageEncoder.EncodeJpeg_QualityChangesPayload: q95 bigger than q30 and PSNR higher by more than 1 dB.
    // C# uses `LegacyWpfPreviewCache.PatternPixels` (the smooth checkerboard): on the noisy golden pattern both
    // qualities are equally bad and the PSNR gap vanishes.
    let pixels = legacy_pattern_pixels(200, 120, PixelLayout::Bgr32);
    let low = jpeg_of(&pixels, 30);
    let high = jpeg_of(&pixels, 95);
    assert!(
        high.len() > low.len(),
        "q95 {} bytes vs q30 {} bytes",
        high.len(),
        low.len()
    );

    let (from_low, _) = decode(&low, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, 200, 120).expect("decode q30");
    let (from_high, _) = decode(&high, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, 200, 120).expect("decode q95");
    let psnr_low = psnr(&pixels, &from_low);
    let psnr_high = psnr(&pixels, &from_high);
    assert!(
        psnr_high > psnr_low + 1.0,
        "PSNR q95 {psnr_high:.1} dB vs q30 {psnr_low:.1} dB"
    );
}

#[test]
#[ignore = "native: needs WIC"]
fn opaque_png_round_trips_as_bgr32() {
    let _serial = serial();
    // C#: WicCacheImageIoTests.OpaquePng_RoundTrips_AsBgr32: pixel for pixel, Bgr32 when no layout is requested.
    let pixels = pattern(37, 23, PixelLayout::Bgr32, 0, false);
    let png = png_of(&pixels);

    let (decoded, info) = decode(&png, None, DecodeBox::UNBOUNDED, 0, 0).expect("decode");

    assert_eq!(decoded.layout(), PixelLayout::Bgr32);
    assert_eq!((decoded.width(), decoded.height()), (37, 23));
    assert_eq!(
        (info.source_width, info.source_height, info.downscaled),
        (37, 23, false),
        "WicCacheImageInfo(37, 23, Downscaled: false)"
    );
    assert_same_pixels(&pixels, &decoded);
}

#[test]
#[ignore = "native: needs WIC"]
fn alpha_png_decodes_as_pbgra32_with_alpha_kept() {
    let _serial = serial();
    // C#: WicCacheImageIoTests.AlphaPng_DecodesAsPbgra32_WithAlphaKept.
    let pixels = pattern(40, 8, PixelLayout::Pbgra32, 0, true);
    let png = png_of(&pixels);

    let (decoded, _) = decode(&png, None, DecodeBox::UNBOUNDED, 0, 0).expect("decode");

    assert_eq!(decoded.layout(), PixelLayout::Pbgra32);
    assert!(!pr_imaging::pixels::is_fully_opaque(&decoded).expect("not disposed"));
}

#[test]
#[ignore = "native: needs WIC"]
fn box_shrinks_but_never_upscales() {
    let _serial = serial();
    // C#: WicCacheImageIoTests.Box_ShrinksButNeverUpscales.
    let pixels = pattern(400, 200, PixelLayout::Bgr32, 0, false);
    let png = png_of(&pixels);

    let (small, small_info) = decode(&png, Some(PixelLayout::Bgr32), DecodeBox::new(100, 0), 0, 0).expect("box 100");
    assert_eq!((small.width(), small.height()), (100, 50));
    assert_eq!(
        (small_info.source_width, small_info.source_height, small_info.downscaled),
        (400, 200, true)
    );

    let (same, same_info) = decode(&png, Some(PixelLayout::Bgr32), DecodeBox::new(4000, 0), 0, 0).expect("box 4000");
    assert_eq!((same.width(), same.height()), (400, 200));
    assert!(!same_info.downscaled);
}

#[test]
#[ignore = "native: needs WIC"]
fn expected_size_mismatch_throws() {
    let _serial = serial();
    // C#: WicCacheImageIoTests.ExpectedSizeMismatch_Throws (31x20 and 32x21 against a 32x20 frame), real PNG header.
    let png = png_of(&pattern(32, 20, PixelLayout::Bgr32, 0, false));
    for (ew, eh) in [(31, 20), (32, 21)] {
        match decode(&png, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, ew, eh) {
            Err(CacheImageError::InvalidData(text)) => assert_eq!(text, INVALID_DATA_HEADER_MISMATCH),
            other => panic!("expected {ew}x{eh}: InvalidData(HEADER_MISMATCH), got {other:?}"),
        }
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn box_one_pixel_strip_is_shrunk_by_width() {
    let _serial = serial();
    // C#: WicCacheImageIoTests.Box_OnePixelStrip_IsShrunkByWidth: a 1000x1 strip with a 500-wide box is 500x1, downscaled.
    let png = png_of(&pattern(1000, 1, PixelLayout::Bgr32, 0, false));

    let (decoded, info) = decode(&png, Some(PixelLayout::Bgr32), DecodeBox::new(500, 0), 0, 0).expect("decode");

    assert_eq!((decoded.width(), decoded.height()), (500, 1));
    assert!(info.downscaled);
}

#[test]
#[ignore = "native: needs WIC"]
fn garbage_payload_is_com_entry_failure() {
    let _serial = serial();
    // C#: EmptyOrGarbagePayload_IsEntryFailure, garbage half through WIC: Com (entry failure), never InvalidData.
    for payload in [
        vec![0u8; 64],
        vec![1u8, 2, 3, 4],
        vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
    ] {
        let result = decode(&payload, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, 0, 0);
        assert!(
            matches!(result, Err(CacheImageError::Com { .. })),
            "{payload:?}: {result:?}"
        );
        assert!(result.err().is_some_and(|e| e.is_entry_failure()));
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn encode_png_error_path_leaks_nothing() {
    let _serial = serial();
    // Contract "Done when": leak test after an error path in encode_png (live pixel count and COM counter balanced).
    use pr_imaging::pixel_memory::NativePixelMemory;
    use pr_platform_win::wic::interop::outstanding_on_this_thread;
    let mut pixels = pattern(8, 8, PixelLayout::Bgr32, 0, false);
    // The buffer is freed first: the baseline is taken after that, so the encode call itself must change nothing.
    pixels.dispose();
    let live_before = NativePixelMemory::live_count();
    let com_before = outstanding_on_this_thread();

    let mut out = Vec::new();
    let _ = encode_png(&pixels, &mut out);

    assert_eq!(NativePixelMemory::live_count(), live_before);
    assert_eq!(outstanding_on_this_thread(), com_before);
}

#[test]
#[ignore = "native: needs WIC"]
fn cache_encode_decode_balanced() {
    let _serial = serial();
    // C#: WicLifetimeBalanceTests.CacheEncodeDecodeAndScale_Balanced (encode JPEG, encode PNG opaque and with alpha, decode
    // unbounded / box / garbage). Every COM wrapper created is released: the outstanding counter returns to its start.
    use pr_platform_win::wic::interop::outstanding_on_this_thread;
    let opaque = pattern(96, 64, PixelLayout::Bgr32, 0, false);
    let translucent = pattern(96, 64, PixelLayout::Pbgra32, 0, true);
    let before = outstanding_on_this_thread();

    let jpeg = jpeg_of(&opaque, 90);
    let _ = png_of(&opaque);
    let png = png_of(&translucent);
    assert_eq!(outstanding_on_this_thread(), before);

    let _ = decode(&jpeg, None, DecodeBox::UNBOUNDED, 96, 64);
    let _ = decode(&png, None, DecodeBox::new(48, 32), 0, 0);
    let _ = decode(&[1, 2, 3, 4], None, DecodeBox::UNBOUNDED, 0, 0);
    assert_eq!(outstanding_on_this_thread(), before);
}

#[test]
#[ignore = "native: needs WIC"]
fn encode_jpeg_and_decode_are_pinned_by_golden_hash() {
    let _serial = serial();
    // Golden: tests/golden/wic-cache-codec.json (31 cases). Replays each case: encode (length and FNV of the payload), decode
    // (size, layout, FNV of the meaningful bytes, source size, downscaled) or the expected exception family.
    // The golden outputs do not repeat the inputs: the pattern, quality, payload, layout, box and expected size come from
    // tests/golden-inputs/wic-cache-codec.json (by case name); the expected results come from the golden.
    let golden = golden::load("wic-cache-codec");
    let inputs = golden_input_cases("wic-cache-codec");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = golden_input(&inputs, name);
        let payload: Vec<u8> = if let Some(hex) = input["payloadHex"].as_str() {
            hex_decode(hex)
        } else {
            let layout = layout_of(str_of(&input, "layout"));
            let seed = input["seed"].as_u64().unwrap_or(0);
            let alpha = input["alpha"].as_bool().unwrap_or(false);
            let pixels = pattern(
                i64_of(&input, "width") as i32,
                i64_of(&input, "height") as i32,
                layout,
                seed,
                alpha,
            );
            let encoded = match str_of(&input, "encode") {
                "jpeg" => {
                    let quality = input["quality"].as_i64().unwrap_or(90) as i32;
                    let mut out = Vec::new();
                    encode_jpeg(&pixels, &mut out, quality).map(|()| out)
                }
                _ => {
                    let mut out = Vec::new();
                    encode_png(&pixels, &mut out).map(|()| out)
                }
            };
            match (case["encodeThrows"].as_str(), encoded) {
                (None, Ok(bytes)) => {
                    assert_eq!(
                        bytes.len() as i64,
                        i64_of(case, "encodedLength"),
                        "{name}: encodedLength"
                    );
                    assert_eq!(fnv1a_hex(&bytes), str_of(case, "encodedFnv"), "{name}: encodedFnv");
                    bytes
                }
                (Some(_), Err(EncodeError::QualityOutOfRange { .. })) => continue,
                (expected, actual) => panic!("{name}: encode expected {expected:?}, got {actual:?}"),
            }
        };

        let decode_layout = input["decodeLayout"].as_str().map(layout_of);
        let box_ = match input["box"].as_array() {
            Some(b) => DecodeBox::new(b[0].as_i64().unwrap_or(0) as i32, b[1].as_i64().unwrap_or(0) as i32),
            None => DecodeBox::UNBOUNDED,
        };
        let (ew, eh) = match input["expected"].as_array() {
            Some(e) => (e[0].as_i64().unwrap_or(0) as i32, e[1].as_i64().unwrap_or(0) as i32),
            None => (0, 0),
        };

        let result = decode(&payload, decode_layout, box_, ew, eh);
        match case["decodeThrows"].as_str() {
            Some("InvalidDataException") => {
                assert!(
                    matches!(result, Err(CacheImageError::InvalidData(_))),
                    "{name}: {result:?}"
                )
            }
            Some("COMException") => assert!(matches!(result, Err(CacheImageError::Com { .. })), "{name}: {result:?}"),
            Some(other) => panic!("{name}: unexpected golden decodeThrows {other}"),
            None => {
                let (decoded, info) = result.unwrap_or_else(|e| panic!("{name}: decode failed: {e}"));
                assert_eq!(decoded.width() as i64, i64_of(case, "width"), "{name}: width");
                assert_eq!(decoded.height() as i64, i64_of(case, "height"), "{name}: height");
                assert_eq!(layout_name(decoded.layout()), str_of(case, "layout"), "{name}: layout");
                assert_eq!(
                    fnv1a_hex(&meaningful_bytes(&decoded)),
                    str_of(case, "fnv"),
                    "{name}: fnv"
                );
                assert_eq!(
                    info.source_width as i64,
                    i64_of(case, "sourceWidth"),
                    "{name}: sourceWidth"
                );
                assert_eq!(
                    info.source_height as i64,
                    i64_of(case, "sourceHeight"),
                    "{name}: sourceHeight"
                );
                assert_eq!(info.downscaled, bool_of(case, "downscaled"), "{name}: downscaled");
            }
        }
    }
}

#[test]
fn plan_decode_golden_row_shapes_are_pure() {
    let _serial = serial();
    // The box rows of the codec golden (box (100,0), (4000,0), (0,50), (100,100), (500,0), (64,32), (20,0)) are pure decisions of
    // plan_decode: the source size, box and expected size come from tests/golden-inputs, the result from tests/golden.
    let golden = golden::load("wic-cache-codec");
    let inputs = golden_input_cases("wic-cache-codec");
    let mut checked = 0;
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = golden_input(&inputs, name);
        let Some(b) = input["box"].as_array() else { continue };
        if !case["decodeThrows"].is_null() {
            continue;
        }
        checked += 1;
        let (bw, bh) = (b[0].as_i64().unwrap_or(0) as i32, b[1].as_i64().unwrap_or(0) as i32);
        let plan = plan_decode(
            i64_of(case, "sourceWidth") as u32,
            i64_of(case, "sourceHeight") as u32,
            DecodeBox::new(bw, bh),
            0,
            0,
        )
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(
            (plan.width as i64, plan.height as i64),
            (i64_of(case, "width"), i64_of(case, "height")),
            "{name}"
        );
        assert_eq!(plan.downscaled, bool_of(case, "downscaled"), "{name}");
    }
    assert_eq!(checked, 7, "the seven box rows of the golden");
}

#[test]
fn decode_empty_returns_invalid_data_without_wic() {
    let _serial = serial();
    // Contract "New Rust tests": decode(&[]) is InvalidData(EMPTY) without WIC (the pure check runs first).
    match decode(&[], None, DecodeBox::UNBOUNDED, 0, 0) {
        Err(CacheImageError::InvalidData(text)) => assert_eq!(text, INVALID_DATA_EMPTY),
        other => panic!("expected InvalidData(EMPTY), got {other:?}"),
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn decode_and_encode_jpeg_error_paths_leak_nothing() {
    // Contract "Done when": the live pixel count and the COM counter are back at their start after an error path of `decode`
    // (garbage payload, header mismatch after the frame was opened, box shrink of a truncated entry) and of `encode_jpeg`.
    let _serial = serial();
    use pr_imaging::pixel_memory::NativePixelMemory;
    use pr_platform_win::wic::interop::outstanding_on_this_thread;
    let source = pattern(32, 20, PixelLayout::Bgr32, 0, false);
    let png = png_of(&source);
    let live_before = NativePixelMemory::live_count();
    let com_before = outstanding_on_this_thread();

    assert!(decode(&[9, 9, 9, 9], None, DecodeBox::UNBOUNDED, 0, 0).is_err());
    assert!(matches!(
        decode(&png, None, DecodeBox::UNBOUNDED, 31, 20),
        Err(CacheImageError::InvalidData(INVALID_DATA_HEADER_MISMATCH))
    ));
    // A truncated PNG may decode (WIC reads lazily) or fail: either way nothing may leak.
    let _ = decode(&png[..png.len() / 2], None, DecodeBox::new(16, 0), 0, 0);
    let mut out = Vec::new();
    assert!(matches!(
        encode_jpeg(&source, &mut out, 0),
        Err(EncodeError::QualityOutOfRange { .. })
    ));

    assert_eq!(NativePixelMemory::live_count(), live_before);
    assert_eq!(outstanding_on_this_thread(), com_before);
}

/// Repair round 2: multi-band round trips, and the size/disposed/length guards that run before WIC.
mod repair_round_2 {

    use pr_imaging::decode_types::DecodeBox;
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_platform_win::wic::cache_image::{decode, encode_jpeg, encode_png};

    fn noisy(width: i32, height: i32, layout: PixelLayout) -> PixelBuffer {
        let mut px = PixelBuffer::allocate(width, height, layout).unwrap();
        for (i, b) in px.bytes_mut().unwrap().iter_mut().enumerate() {
            *b = ((i as u64 * 131 + (i as u64 / 251)) % 256) as u8;
        }
        px
    }

    fn meaningful(px: &PixelBuffer) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..px.height() {
            let row = px.get_row(y).unwrap();
            for x in 0..px.width() as usize {
                v.extend_from_slice(&row[x * 4..x * 4 + 3]);
            }
        }
        v
    }

    #[test]
    #[ignore = "native: needs WIC"]
    fn png_multi_band_opaque_round_trips_exactly() {
        // 700 x 400 x 3 = 840 000 bytes > BandBytes: 4 bands (rows_per_band = 124), the last one partial.
        for (w, h) in [
            (700, 400),
            (1, 20_000),
            (3, 30_000),
            (40_000, 5),
            (43_691, 2),
            (43_690, 3),
            (60_000, 3),
        ] {
            let src = noisy(w, h, PixelLayout::Bgr32);
            let mut out = Vec::new();
            if let Err(e) = encode_png(&src, &mut out) {
                panic!("{w}x{h}: {e:?}");
            }
            let (dec, info) = decode(&out, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, w, h).unwrap();
            assert_eq!((info.source_width, info.source_height, info.downscaled), (w, h, false));
            assert_eq!(meaningful(&dec), meaningful(&src), "{w}x{h}");
        }
    }

    #[test]
    #[ignore = "native: needs WIC"]
    fn jpeg_multi_band_is_same_as_single_band_image() {
        // Same image content encoded as one band is impossible for 700x400, so compare JPEG decode against PSNR of a smooth source.
        let (w, h) = (700, 400);
        let mut src = PixelBuffer::allocate(w, h, PixelLayout::Bgr32).unwrap();
        for y in 0..h {
            let row = src.get_row_mut(y).unwrap();
            for x in 0..w as usize {
                row[x * 4] = (x * 255 / 699) as u8;
                row[x * 4 + 1] = (y * 255 / 399) as u8;
                row[x * 4 + 2] = 100;
                row[x * 4 + 3] = 0;
            }
        }
        let mut out = Vec::new();
        encode_jpeg(&src, &mut out, 95).unwrap();
        let (dec, _) = decode(&out, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, w, h).unwrap();
        let (a, b) = (meaningful(&src), meaningful(&dec));
        let mut worst = 0i32;
        for (x, y) in a.iter().zip(b.iter()) {
            worst = worst.max((*x as i32 - *y as i32).abs());
        }
        assert!(worst < 16, "worst channel error {worst}");
    }

    use pr_platform_win::wic::cache_image::EncodeError;

    #[test]
    fn encode_too_large_buffer_is_rejected_before_wic() {
        // 2^15 x 2^15 x 4 = 2^32 bytes = u32::MAX + 1: C# `ByteCount > uint.MaxValue` => ArgumentException.
        let big = PixelBuffer::unallocated_for_tests(32768, 32768, PixelLayout::Bgr32);
        assert_eq!(big.byte_count(), 1i64 << 32);
        let mut out = Vec::new();
        match encode_png(&big, &mut out) {
            Err(EncodeError::TooLarge { byte_count }) => assert_eq!(byte_count, 1i64 << 32),
            other => panic!("png: {other:?}"),
        }
        match encode_jpeg(&big, &mut out, 90) {
            Err(EncodeError::TooLarge { byte_count }) => assert_eq!(byte_count, 1i64 << 32),
            other => panic!("jpeg: {other:?}"),
        }
        assert!(out.is_empty());
        // quality is checked before size
        assert!(matches!(
            encode_jpeg(&big, &mut out, 0),
            Err(EncodeError::QualityOutOfRange { quality: 0 })
        ));
        // disposed is checked before size
        let mut gone = PixelBuffer::unallocated_for_tests(32768, 32768, PixelLayout::Bgr32);
        gone.dispose();
        assert!(matches!(encode_png(&gone, &mut out), Err(EncodeError::Disposed)));
        assert!(matches!(encode_jpeg(&gone, &mut out, 90), Err(EncodeError::Disposed)));
    }

    use pr_platform_win::wic::cache_image::{CacheImageError, INVALID_DATA_TOO_LARGE};

    #[test]
    fn decode_payload_longer_than_u32_is_invalid_data_without_touching_memory() {
        // Never dereferenced: the length guard runs before WIC (UB-free in practice: only the length is read).
        // SAFETY: none needed for reads, because the slice is never dereferenced: `decode` rejects it on its length alone, before
        // touching WIC or the memory (a `dangling` pointer with a length below `isize::MAX` only has to be non-null and aligned).
        let huge: &[u8] =
            unsafe { std::slice::from_raw_parts(std::ptr::NonNull::<u8>::dangling().as_ptr(), 5usize << 30) };
        match decode(huge, None, DecodeBox::UNBOUNDED, 0, 0) {
            Err(CacheImageError::InvalidData(t)) => assert_eq!(t, INVALID_DATA_TOO_LARGE),
            other => panic!("{other:?}"),
        }
    }
}
