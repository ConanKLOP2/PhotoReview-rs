//! Red tests for W4-16a (`pr_imaging::pixels`): port of `PhotoReview.Imaging/Pixels/PixelBuffer.cs` and `PixelOps.cs`
//! (C# baseline d92d6aae), tests from `tests/PhotoReview.Imaging.Tests/Pixels/` (`PixelBufferTests`, `PixelOpsAlphaTests`,
//! `PixelOpsOrientationTests`), plus the golden-driven tests over `tests/golden/{pixel-buffer,pixel-ops,orientation-pixels}.json`
//! (inputs in `tests/golden-inputs/`).
//!
//! Every test here compiles against the stub and fails through `todo!()` until the porter fills the bodies.
//! The lifetime tests (`NativePixelMemory::live_count`, a process-wide counter) live in `pixels_lifetime.rs`, a separate
//! process, so they never race with the allocating tests of this file.
//!
//! Documented skips (not portable to Rust, see the contract W4-16a): `ApplyOrientation_MatchesWpfTransformedBitmap` compares
//! with `WpfExifOrientation.Apply` (no WPF in Rust) and is replaced by a per-pixel reference built from
//! `pr_imaging::orientation::map_pixel`; `AlphaChecks_MatchPreviewCacheFile` compares with `WpfBitmapSourceCodec` and is
//! replaced by the closed expectation the C# test states; the `NullOrDisposed_Throws` null halves have no Rust twin.

use std::collections::HashMap;

use pr_imaging::orientation::{map_pixel, oriented_size};
use pr_imaging::pixel_memory::ALIGNMENT;
use pr_imaging::pixels::{
    ApplyOrientationError, BYTES_PER_PIXEL, PixelBuffer, PixelBufferError, PixelLayout, apply_orientation,
    fits_in_span, has_alpha, is_fully_opaque,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Local helpers (twins of C# `PixelAssert`, `tests/PhotoReview.TestSupport.Windows/PixelAssert.cs`)
// ---------------------------------------------------------------------------------------------------------------------

/// Golden pattern: byte `i` (row-major, no padding) is `(i * 131 + seed * 17 + i / 251) mod 256`, 64-bit arithmetic.
fn pattern_byte(index: u64, seed: u64) -> u8 {
    ((index * 131 + seed * 17 + index / 251) % 256) as u8
}

/// A freshly allocated buffer filled with the golden pattern.
fn create_pattern(width: i32, height: i32, layout: PixelLayout, seed: u64) -> PixelBuffer {
    let mut buffer = PixelBuffer::allocate(width, height, layout).expect("valid geometry");
    for (index, byte) in buffer.bytes_mut().expect("not disposed").iter_mut().enumerate() {
        *byte = pattern_byte(index as u64, seed);
    }
    buffer
}

/// An independent buffer with the same geometry and bytes as `src` (C# `PixelAssert.Clone`).
fn clone_buffer(src: &PixelBuffer) -> PixelBuffer {
    let mut copy = PixelBuffer::allocate(src.width(), src.height(), src.layout()).expect("valid geometry");
    copy.bytes_mut()
        .expect("not disposed")
        .copy_from_slice(src.bytes().expect("not disposed"));
    copy
}

/// Panics with the first differing pixel and channel (C# `PixelAssert.Equal`).
fn assert_equal(expected: &PixelBuffer, actual: &PixelBuffer) {
    assert_eq!(
        (expected.width(), expected.height(), expected.layout()),
        (actual.width(), actual.height(), actual.layout()),
        "geometry differs"
    );
    let want = expected.bytes().expect("not disposed");
    let got = actual.bytes().expect("not disposed");
    assert_eq!(want.len(), got.len(), "byte count differs");
    if let Some(index) = want.iter().zip(got).position(|(a, b)| a != b) {
        let pixel = index / BYTES_PER_PIXEL as usize;
        let width = expected.width() as usize;
        panic!(
            "first difference at pixel ({}, {}) channel {}: expected {}, got {}",
            pixel % width,
            pixel / width,
            index % BYTES_PER_PIXEL as usize,
            want[index],
            got[index]
        );
    }
}

/// Copies one 4-byte pixel from `src` (sx, sy) into `dst` (dx, dy).
fn copy_pixel(src: &PixelBuffer, sx: usize, sy: usize, dst: &mut PixelBuffer, dx: usize, dy: usize) {
    let pixel: [u8; 4] = src.get_row(sy as i32).expect("row in range")[sx * 4..sx * 4 + 4]
        .try_into()
        .expect("four bytes");
    dst.get_row_mut(dy as i32).expect("row in range")[dx * 4..dx * 4 + 4].copy_from_slice(&pixel);
}

/// FNV-1a 64 over `bytes`, 16 lowercase hex digits (the golden digest).
fn fnv1a64(bytes: &[u8]) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `Ok(())` or the .NET exception type name of the error (the golden `throws` value).
fn err_name<T>(result: Result<T, PixelBufferError>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(error) => error.dotnet_type_name().to_owned(),
    }
}

/// Applies the orientation and hands back only the buffer or the error (the source is dropped on error paths here).
fn orient(src: PixelBuffer, orientation: i32) -> Result<PixelBuffer, PixelBufferError> {
    apply_orientation(src, orientation).map_err(|failure: ApplyOrientationError| failure.error)
}

/// Applies the orientations in order; the first error stops the chain.
fn orient_chain(mut current: PixelBuffer, steps: &[i32]) -> Result<PixelBuffer, PixelBufferError> {
    for &step in steps {
        current = orient(current, step)?;
    }
    Ok(current)
}

fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit i32: {case}"))
}

fn layout_of(case: &Value) -> PixelLayout {
    PixelLayout::from_i32(i32_of(case, "layout")).expect("golden layouts are 0 or 1")
}

/// Input cases of `tests/golden-inputs/<name>.json`, keyed by case name.
fn input_cases(name: &str) -> HashMap<String, Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

fn input_for<'a>(inputs: &'a HashMap<String, Value>, name: &str) -> &'a Value {
    inputs
        .get(name)
        .unwrap_or_else(|| panic!("no golden input named '{name}'"))
}

/// Fails once, listing every mismatching golden case.
fn check_all(label: &str, failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{label}: {} mismatching case(s)\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// PixelBufferTests (C# `PhotoReview.Imaging.Tests/Pixels/PixelBufferTests.cs`)
// ---------------------------------------------------------------------------------------------------------------------

// C#: PixelBufferTests.Allocate_ReportsGeometryAndLayout
#[test]
fn allocate_reports_geometry_and_layout() {
    let cases = [
        (1, 1, PixelLayout::Bgr32),
        (1, 9, PixelLayout::Pbgra32),
        (9, 1, PixelLayout::Bgr32),
        (37, 23, PixelLayout::Pbgra32),
    ];
    for (width, height, layout) in cases {
        let buffer = PixelBuffer::allocate(width, height, layout).expect("valid size");

        assert_eq!(buffer.width(), width);
        assert_eq!(buffer.height(), height);
        assert_eq!(buffer.layout(), layout);
        assert_eq!(buffer.stride(), width * 4);
        assert_eq!(buffer.byte_count(), i64::from(width) * 4 * i64::from(height));
        assert!(!buffer.is_disposed());
        let address = buffer.address().expect("not disposed") as usize;
        assert_ne!(address, 0);
        assert_eq!(address % ALIGNMENT, 0, "first byte is 64-byte aligned");
    }
}

// C#: PixelBufferTests.Allocate_IdsAreUniqueAndIncreasing
#[test]
fn allocate_ids_are_unique_and_increasing() {
    let first = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("valid size");
    let second = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("valid size");
    let third = PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).expect("valid size");

    assert!(first.id() > 0);
    assert!(second.id() > first.id());
    assert!(third.id() > second.id());
}

// C#: PixelBufferTests.Allocate_RejectsInvalidSize
#[test]
fn allocate_rejects_invalid_size() {
    // (width, height) => the failing dimension decides the variant; the width cases must fail before any allocation.
    let cases: [(i32, i32); 7] = [
        (0, 1),
        (1, 0),
        (-1, 5),
        (5, -1),
        (i32::MIN, 1),
        (i32::MAX / 4 + 1, 1),
        (i32::MAX, 1),
    ];
    for (width, height) in cases {
        let result = PixelBuffer::allocate(width, height, PixelLayout::Bgr32);
        let expected_width_error = !(1..=i32::MAX / 4).contains(&width);
        match result {
            Err(PixelBufferError::WidthOutOfRange { width: reported }) => {
                assert!(expected_width_error, "width {width} reported as WidthOutOfRange");
                assert_eq!(reported, width);
            }
            Err(PixelBufferError::HeightOutOfRange { height: reported }) => {
                assert!(
                    !expected_width_error && height < 1,
                    "height {height} reported as HeightOutOfRange"
                );
                assert_eq!(reported, height);
            }
            other => panic!("({width}, {height}) must be rejected, got {other:?}"),
        }
    }
}

// C#: PixelBufferTests.Allocate_RejectsUnknownLayout
// The enum cannot hold another value in Rust: the C# `(PixelLayout)n` cast is `PixelLayout::from_i32`.
#[test]
fn allocate_rejects_unknown_layout() {
    for value in [-1, 2, 255] {
        assert_eq!(
            PixelLayout::from_i32(value),
            Err(PixelBufferError::UnknownLayout(value)),
            "layout {value}"
        );
    }
    assert_eq!(PixelLayout::from_i32(0), Ok(PixelLayout::Bgr32));
    assert_eq!(PixelLayout::from_i32(1), Ok(PixelLayout::Pbgra32));
    assert_eq!(PixelLayout::Bgr32.as_i32(), 0);
    assert_eq!(PixelLayout::Pbgra32.as_i32(), 1);
}

// C#: PixelBufferTests.GetRow_RowsAreContiguousAndCoverTheBuffer
#[test]
fn get_row_rows_are_contiguous_and_cover_the_buffer() {
    let mut buffer = PixelBuffer::allocate(5, 4, PixelLayout::Pbgra32).expect("valid size");
    let stride = buffer.stride() as usize;
    for y in 0..buffer.height() {
        let row = buffer.get_row_mut(y).expect("row in range");
        assert_eq!(row.len(), stride);
        row.fill((y + 1) as u8);
    }

    let all = buffer.bytes().expect("not disposed");
    assert_eq!(all.len() as i64, buffer.byte_count());
    for (index, byte) in all.iter().enumerate() {
        assert_eq!(*byte, (index / stride + 1) as u8, "byte {index}");
    }
}

// C#: PixelBufferTests.GetRow_OutOfRange_Throws
#[test]
fn get_row_out_of_range_throws() {
    let buffer = PixelBuffer::allocate(2, 3, PixelLayout::Bgr32).expect("valid size");
    for y in [-1, 3, i32::MAX, i32::MIN] {
        assert_eq!(
            buffer.get_row(y).err(),
            Some(PixelBufferError::RowOutOfRange { y, height: 3 }),
            "row {y}"
        );
    }
}

// C#: PixelBufferTests.GetRow_LastRow_IsReadable
#[test]
fn get_row_last_row_is_readable() {
    let mut buffer = PixelBuffer::allocate(3, 3, PixelLayout::Bgr32).expect("valid size");
    buffer.get_row_mut(2).expect("last row").fill(7);

    let all = buffer.bytes().expect("not disposed");
    assert_eq!(all[all.len() - 1], 7);
    assert_eq!(all[2 * buffer.stride() as usize], 7);
}

// C#: PixelBufferTests.TryGetSpan_FalseOnlyWhenByteCountExceedsIntMax
// The fake block of the C# test is `unallocated_for_tests`: no memory behind it, so the span is empty when it is `Some`.
#[test]
fn try_get_span_false_only_when_byte_count_exceeds_int_max() {
    let cases = [
        (23171, 23171, false), // 2 147 580 964 byte
        (23170, 23170, true),  // 2 147 395 600 byte
        (536_870_911, 1, true),
        (536_870_911, 2, false),
        (65536, 8193, false),
        (65536, 8192, false), // exactly 2^31
        (65536, 8191, true),
    ];
    for (width, height, expected) in cases {
        let buffer = PixelBuffer::unallocated_for_tests(width, height, PixelLayout::Bgr32);

        assert_eq!(fits_in_span(buffer.byte_count()), expected, "{width}x{height}");
        match buffer.try_get_span().expect("not disposed") {
            None => assert!(!expected, "{width}x{height} must be a span"),
            Some(span) => {
                assert!(expected, "{width}x{height} must not be a span");
                assert!(span.is_empty(), "unallocated block has no bytes");
            }
        }
    }
}

// C#: PixelBufferTests.TryGetSpan_FalseAt2GiB_TrueJustBelow
#[test]
fn try_get_span_false_at_2_gib_true_just_below() {
    let over = PixelBuffer::unallocated_for_tests(65536, 8192, PixelLayout::Pbgra32);
    let under = PixelBuffer::unallocated_for_tests(65536, 8191, PixelLayout::Pbgra32);

    assert_eq!(over.byte_count(), i64::from(i32::MAX) + 1);
    assert!(over.try_get_span().expect("not disposed").is_none());
    assert!(under.try_get_span().expect("not disposed").is_some());
}

// C#: PixelBufferTests.Dispose_Twice_IsSafe_AndMarksDisposed
#[test]
fn dispose_twice_is_safe_and_marks_disposed() {
    let mut buffer = PixelBuffer::allocate(3, 2, PixelLayout::Bgr32).expect("valid size");

    buffer.dispose();
    buffer.dispose();

    assert!(buffer.is_disposed());
    // Geometry stays readable after dispose (log and metrics).
    assert_eq!(buffer.width(), 3);
    assert_eq!(buffer.height(), 2);
    assert_eq!(buffer.byte_count(), 24);
}

// C#: PixelBufferTests.UseAfterDispose_ThrowsObjectDisposed
#[test]
fn use_after_dispose_throws_object_disposed() {
    let mut buffer = PixelBuffer::allocate(3, 2, PixelLayout::Pbgra32).expect("valid size");
    buffer.dispose();

    assert_eq!(buffer.address().err(), Some(PixelBufferError::Disposed));
    assert_eq!(buffer.get_row(0).err(), Some(PixelBufferError::Disposed));
    // The disposed check comes before the row range check (C# order).
    assert_eq!(buffer.get_row(-5).err(), Some(PixelBufferError::Disposed));
    assert_eq!(buffer.try_get_span().err(), Some(PixelBufferError::Disposed));
    assert_eq!(buffer.bytes().err(), Some(PixelBufferError::Disposed));
    assert_eq!(buffer.get_row_mut(0).err(), Some(PixelBufferError::Disposed));
    assert_eq!(buffer.bytes_mut().err(), Some(PixelBufferError::Disposed));
    assert_eq!(buffer.try_get_span_mut().err(), Some(PixelBufferError::Disposed));
}

// ---------------------------------------------------------------------------------------------------------------------
// PixelOpsAlphaTests (C# `PhotoReview.Imaging.Tests/Pixels/PixelOpsAlphaTests.cs`)
// ---------------------------------------------------------------------------------------------------------------------

// C#: PixelOpsAlphaTests.AlphaChecks_MatchPreviewCacheFile
// Replaced (no WPF): the C# `legacyOpaque` is the closed expectation `!with_transparent_pixel`, and the six source format
// names collapse to the two layouts (a source that can carry alpha is Pbgra32, the rest Bgr32).
#[test]
fn alpha_checks_match_preview_cache_file() {
    let cases = [
        ("Bgr24", PixelLayout::Bgr32, false),
        ("Bgr32", PixelLayout::Bgr32, false),
        ("Gray8", PixelLayout::Bgr32, false),
        ("Bgra32", PixelLayout::Pbgra32, false),
        ("Bgra32", PixelLayout::Pbgra32, true),
        ("Pbgra32", PixelLayout::Pbgra32, false),
        ("Pbgra32", PixelLayout::Pbgra32, true),
        ("Indexed8", PixelLayout::Pbgra32, false),
        ("Indexed8", PixelLayout::Pbgra32, true),
    ];
    for (format, layout, with_transparent_pixel) in cases {
        // 21 x 5 pattern (vector tail); one pixel at the middle of the last row is translucent when requested.
        let mut pixels = create_pattern(21, 5, layout, format.len() as u64);
        if layout == PixelLayout::Pbgra32 {
            for pixel in pixels.bytes_mut().expect("not disposed").chunks_exact_mut(4) {
                pixel[3] = 255;
            }
            if with_transparent_pixel {
                let at = (4 * 21 + 10) * 4;
                let bytes = pixels.bytes_mut().expect("not disposed");
                bytes[at] = 40;
                bytes[at + 1] = 40;
                bytes[at + 2] = 40;
                bytes[at + 3] = 100;
            }
        }

        let opaque = is_fully_opaque(&pixels).expect("not disposed");
        assert_eq!(
            opaque, !with_transparent_pixel,
            "{format} transparent={with_transparent_pixel}"
        );
        assert_eq!(
            has_alpha(&pixels).expect("not disposed"),
            with_transparent_pixel,
            "{format}"
        );
    }
}

// C#: PixelOpsAlphaTests.IsFullyOpaque_FindsOneTransparentPixelAnywhere
#[test]
fn is_fully_opaque_finds_one_transparent_pixel_anywhere() {
    for (width, height) in [(1, 1), (17, 3), (64, 2), (3, 9)] {
        let mut pixels = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).expect("valid size");
        for y in 0..height {
            pixels.get_row_mut(y).expect("row").fill(255);
        }
        assert!(is_fully_opaque(&pixels).expect("not disposed"));
        assert!(!has_alpha(&pixels).expect("not disposed"));

        for y in 0..height {
            for x in 0..width as usize {
                pixels.get_row_mut(y).expect("row")[x * 4 + 3] = 254;
                assert!(!is_fully_opaque(&pixels).expect("not disposed"), "pixel ({x}, {y})");
                assert!(has_alpha(&pixels).expect("not disposed"), "pixel ({x}, {y})");
                pixels.get_row_mut(y).expect("row")[x * 4 + 3] = 255;
            }
        }
    }
}

// C#: PixelOpsAlphaTests.IsFullyOpaque_IgnoresColourBytes
#[test]
fn is_fully_opaque_ignores_colour_bytes() {
    let mut pixels = PixelBuffer::allocate(20, 2, PixelLayout::Pbgra32).expect("valid size");
    for y in 0..2 {
        let row = pixels.get_row_mut(y).expect("row");
        for (index, byte) in row.iter_mut().enumerate() {
            *byte = if index % 4 == 3 { 255 } else { 0 };
        }
    }

    assert!(is_fully_opaque(&pixels).expect("not disposed"));
}

// C#: PixelOpsAlphaTests.Bgr32_IsAlwaysOpaque_WhateverTheUnusedByte
#[test]
fn bgr32_is_always_opaque_whatever_the_unused_byte() {
    let mut pixels = PixelBuffer::allocate(9, 2, PixelLayout::Bgr32).expect("valid size");
    for fill in [0u8, 255] {
        pixels.bytes_mut().expect("not disposed").fill(fill);

        assert!(is_fully_opaque(&pixels).expect("not disposed"), "unused byte {fill}");
        assert!(!has_alpha(&pixels).expect("not disposed"), "unused byte {fill}");
    }
}

// C#: PixelOpsAlphaTests.NullOrDisposed_Throws (the null half has no Rust twin: a `&PixelBuffer` cannot be null)
#[test]
fn null_or_disposed_throws() {
    let mut disposed = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("valid size");
    disposed.dispose();

    assert_eq!(is_fully_opaque(&disposed), Err(PixelBufferError::Disposed));
    assert_eq!(has_alpha(&disposed), Err(PixelBufferError::Disposed));
}

// ---------------------------------------------------------------------------------------------------------------------
// PixelOpsOrientationTests (C# `PhotoReview.Imaging.Tests/Pixels/PixelOpsOrientationTests.cs`)
// ---------------------------------------------------------------------------------------------------------------------

// C#: PixelOpsOrientationTests.ApplyOrientation_MatchesWpfTransformedBitmap_ByteForByte
// The WPF reference (`WpfExifOrientation.Apply`) has no Rust twin: the reference is built pixel by pixel from the
// independent W4-02a table `orientation::map_pixel` (same 6 sizes, both layouts, orientations 1..=8: 96 cases).
#[test]
fn apply_orientation_matches_wpf_transformed_bitmap_byte_for_byte() {
    let sizes = [(1, 1), (1, 7), (7, 1), (2, 3), (33, 31), (70, 45)];
    for layout in [PixelLayout::Bgr32, PixelLayout::Pbgra32] {
        for (width, height) in sizes {
            for orientation in 1..=8 {
                let seed = (orientation * 1000 + width + height) as u64;
                let original = create_pattern(width, height, layout, seed);

                let (out_width, out_height) = oriented_size(orientation, width as u32, height as u32);
                let mut reference =
                    PixelBuffer::allocate(out_width as i32, out_height as i32, layout).expect("valid size");
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let (dx, dy) = map_pixel(orientation, x as u32, y as u32, width as u32, height as u32);
                        copy_pixel(&original, x, y, &mut reference, dx as usize, dy as usize);
                    }
                }

                let actual = orient(clone_buffer(&original), orientation).expect("valid orientation");

                let transposed = orientation >= 5;
                assert_eq!(actual.width(), if transposed { height } else { width });
                assert_eq!(actual.height(), if transposed { width } else { height });
                assert_eq!(actual.layout(), layout);
                assert_equal(&reference, &actual);
            }
        }
    }
}

// C#: PixelOpsOrientationTests.ApplyOrientation_MovesKnownPixel
#[test]
fn apply_orientation_moves_known_pixel() {
    // (orientation, srcX, srcY, dstX, dstY) on a 3 x 2 source; each pixel's first byte is 10 + y * 3 + x.
    let cases = [
        (2, 0, 0, 2, 0),
        (3, 0, 0, 2, 1),
        (4, 0, 0, 0, 1),
        (5, 2, 0, 0, 2),
        (6, 0, 0, 1, 0),
        (7, 0, 0, 1, 2),
        (8, 0, 0, 0, 2),
    ];
    for (orientation, src_x, src_y, dst_x, dst_y) in cases {
        let mut src = PixelBuffer::allocate(3, 2, PixelLayout::Bgr32).expect("valid size");
        for y in 0..2usize {
            for x in 0..3usize {
                src.get_row_mut(y as i32).expect("row")[x * 4..x * 4 + 4].fill((10 + y * 3 + x) as u8);
            }
        }

        let dst = orient(src, orientation).expect("valid orientation");

        let expected = (10 + src_y * 3 + src_x) as u8;
        assert_eq!(
            dst.get_row(dst_y).expect("row")[dst_x * 4],
            expected,
            "orientation {orientation}: source ({src_x}, {src_y}) must land on ({dst_x}, {dst_y})"
        );
    }
}

// C#: PixelOpsOrientationTests.ApplyOrientation_NonIdentity_ReturnsNewBuffer_AndDisposesSource
// The "source is disposed" half is observed through the live counter in `pixels_lifetime.rs` (the source is moved).
#[test]
fn apply_orientation_non_identity_returns_new_buffer() {
    for orientation in [2, 3, 5, 6, 8] {
        let src = create_pattern(4, 3, PixelLayout::Pbgra32, orientation as u64);
        let src_id = src.id();

        let dst = orient(src, orientation).expect("valid orientation");

        assert_ne!(
            dst.id(),
            src_id,
            "orientation {orientation} must not reuse the source id"
        );
        assert!(!dst.is_disposed());
        let transposed = orientation >= 5;
        assert_eq!(dst.width(), if transposed { 3 } else { 4 });
        assert_eq!(dst.height(), if transposed { 4 } else { 3 });
    }
}

// C#: PixelOpsOrientationTests.ApplyOrientation_IdentityOrInvalid_ReturnsSourceUntouched
#[test]
fn apply_orientation_identity_or_invalid_returns_source_untouched() {
    for orientation in [1, 0, -1, 9, i32::MIN, i32::MAX] {
        let src = create_pattern(4, 3, PixelLayout::Bgr32, 5);
        let src_id = src.id();
        let copy = clone_buffer(&src);

        let result = apply_orientation(src, orientation).expect("identity never fails");

        assert_eq!(result.id(), src_id, "orientation {orientation} returns the same buffer");
        assert!(!result.is_disposed());
        assert_equal(&copy, &result);
    }
}

// C#: PixelOpsOrientationTests.ApplyOrientation_NullOrDisposed_Throws (the null half has no Rust twin)
#[test]
fn apply_orientation_null_or_disposed_throws() {
    for orientation in [6, 1] {
        let mut disposed = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("valid size");
        disposed.dispose();
        let id = disposed.id();

        // Checked before the orientation value, so orientation 1 fails too (C# throws first).
        let failure = apply_orientation(disposed, orientation).expect_err("disposed source is an error");
        assert_eq!(failure.error, PixelBufferError::Disposed, "orientation {orientation}");
        assert_eq!(failure.source.id(), id, "the error hands the source back");
        assert!(failure.source.is_disposed());
    }
}

/// xorshift64 (no `rand` crate, as W2-05): deterministic and seeded per test.
struct XorShift64(u64);

impl XorShift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish value in `lo..hi` (`hi` exclusive).
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next_u64() % (hi - lo) as u64) as i32
    }
}

// C#: PixelOpsOrientationTests.ApplyOrientation_ThenInverse_IsIdentity_Property (40 iterations)
#[test]
fn apply_orientation_then_inverse_is_identity_property() {
    // Inverse: 6 <-> 8, every other value is its own inverse (mirror, 180, transpose).
    let inverse = |orientation: i32| match orientation {
        6 => 8,
        8 => 6,
        other => other,
    };
    let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15);
    for iteration in 0..40 {
        let width = rng.range(1, 80);
        let height = rng.range(1, 80);
        let layout = if rng.range(0, 2) == 0 {
            PixelLayout::Bgr32
        } else {
            PixelLayout::Pbgra32
        };
        let orientation = rng.range(1, 9);
        let seed = rng.next_u64() % 100_000;
        let original = create_pattern(width, height, layout, seed);

        let once = orient(clone_buffer(&original), orientation).expect("valid orientation");
        let round_trip = orient(once, inverse(orientation)).expect("valid orientation");

        assert_ne!(round_trip.id(), original.id(), "iteration {iteration}");
        assert_equal(&original, &round_trip);
    }
}

// C#: PixelOpsOrientationTests.ApplyOrientation_ComposesLikeTheDihedralGroup_Property (10 iterations)
#[test]
fn apply_orientation_composes_like_the_dihedral_group_property() {
    let mut rng = XorShift64(0xD1B5_4A32_D192_ED03);
    for iteration in 0..10 {
        let width = rng.range(1, 50);
        let height = rng.range(1, 50);
        let seed = rng.next_u64() % 100_000;
        let original = create_pattern(width, height, PixelLayout::Pbgra32, seed);

        // 6 (90 CW) twice is 3 (180); 6 four times is the identity.
        let twice = orient(orient(clone_buffer(&original), 6).expect("valid"), 6).expect("valid");
        let half = orient(clone_buffer(&original), 3).expect("valid");
        assert_eq!(
            (half.width(), half.height()),
            (twice.width(), twice.height()),
            "iteration {iteration}"
        );
        assert_equal(&half, &twice);

        let full = orient(orient(clone_buffer(&twice), 6).expect("valid"), 6).expect("valid");
        assert_equal(&original, &full);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven tests
// ---------------------------------------------------------------------------------------------------------------------

// Golden `pixel-buffer` (21 cases, inputs in `golden-inputs/pixel-buffer.json`): geometry, rows, span, dispose.
#[test]
fn golden_pixel_buffer_geometry_rows_span_and_dispose() {
    let inputs = input_cases("pixel-buffer");
    let golden = golden::load("pixel-buffer");
    let mut failures = Vec::new();
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = input_for(&inputs, name);
        let result = &case["result"];
        let layout_value = i32_of(input, "layout");
        let outcome = PixelLayout::from_i32(layout_value)
            .and_then(|layout| PixelBuffer::allocate(i32_of(input, "width"), i32_of(input, "height"), layout));

        if let Some(expected) = result.get("throws").and_then(Value::as_str) {
            let actual = err_name(outcome.map(|_| ()));
            if actual != expected {
                failures.push(format!("{name}: expected {expected}, got {actual}"));
            }
            continue;
        }

        let mut buffer = match outcome {
            Ok(buffer) => buffer,
            Err(error) => {
                failures.push(format!("{name}: allocation failed: {error}"));
                continue;
            }
        };
        let mut check = |label: &str, ok: bool| {
            if !ok {
                failures.push(format!("{name}: {label}"));
            }
        };
        check("width", i64::from(buffer.width()) == i64_of(result, "width"));
        check("height", i64::from(buffer.height()) == i64_of(result, "height"));
        check(
            "layout",
            i64::from(buffer.layout().as_i32()) == i64_of(result, "layout"),
        );
        check("stride", i64::from(buffer.stride()) == i64_of(result, "stride"));
        check("byte_count", buffer.byte_count() == i64_of(result, "byte_count"));
        check("is_disposed", buffer.is_disposed() == bool_of(result, "is_disposed"));
        let aligned = buffer.address().is_ok_and(|p| p as usize % ALIGNMENT == 0);
        check("address_aligned_64", aligned == bool_of(result, "address_aligned_64"));
        check("id_positive", (buffer.id() > 0) == bool_of(result, "id_positive"));

        for row in result["rows"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let y = i32_of(row, "y");
            let outcome = buffer.get_row(y).map(<[u8]>::len);
            if let Some(expected) = row.get("throws").and_then(Value::as_str) {
                let actual = match outcome {
                    Ok(len) => format!("ok({len})"),
                    Err(error) => error.dotnet_type_name().to_owned(),
                };
                check(
                    &format!("get_row({y}) throws {expected}, got {actual}"),
                    actual == expected,
                );
            } else {
                let expected_len = i64_of(row, "length");
                let actual_len = outcome.map_or(-1, |len| len as i64);
                check(
                    &format!("get_row({y}) length {expected_len}, got {actual_len}"),
                    actual_len == expected_len,
                );
            }
        }

        if bool_of(result, "span_ok") {
            let span_length = match buffer.try_get_span() {
                Ok(Some(span)) => span.len() as i64,
                _ => -1,
            };
            check("span length", span_length == i64_of(result, "span_length"));
        }

        let after = &result["after_dispose"];
        buffer.dispose();
        check(
            "after_dispose.is_disposed",
            buffer.is_disposed() == bool_of(after, "is_disposed"),
        );
        check(
            "after_dispose.width",
            i64::from(buffer.width()) == i64_of(after, "width"),
        );
        check(
            "after_dispose.height",
            i64::from(buffer.height()) == i64_of(after, "height"),
        );
        check(
            "after_dispose.byte_count",
            buffer.byte_count() == i64_of(after, "byte_count"),
        );
        let disposed_names = [
            ("address", err_name(buffer.address().map(|_| ()))),
            ("get_row", err_name(buffer.get_row(0).map(|_| ()))),
            ("try_get_span", err_name(buffer.try_get_span().map(|_| ()))),
        ];
        for (field, actual) in disposed_names {
            let expected = str_of(after, field);
            check(
                &format!("after_dispose.{field} expected {expected}, got {actual}"),
                actual == expected,
            );
        }
        buffer.dispose(); // second dispose is a no-op: must not panic
    }
    check_all("pixel-buffer", &failures);
}

// Golden `pixel-ops`, kind `orient` (364 cases): destination geometry, identity vs new buffer, FNV-1a and hex.
#[test]
fn golden_pixel_ops_orient_matches_pixels() {
    let inputs = input_cases("pixel-ops");
    let golden = golden::load("pixel-ops");
    let mut failures = Vec::new();
    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "orient") {
        let name = str_of(case, "name");
        let input = input_for(&inputs, name);
        let result = &case["result"];
        let layout = layout_of(input);
        let src = create_pattern(
            i32_of(input, "width"),
            i32_of(input, "height"),
            layout,
            i64_of(input, "seed") as u64,
        );
        let src_id = src.id();

        let out = match orient(src, i32_of(input, "orientation")) {
            Ok(out) => out,
            Err(error) => {
                failures.push(format!("{name}: unexpected error {error}"));
                continue;
            }
        };
        let same_instance = out.id() == src_id;
        let mut check = |label: &str, ok: bool| {
            if !ok {
                failures.push(format!("{name}: {label}"));
            }
        };
        check("same_instance", same_instance == bool_of(result, "same_instance"));
        check("id_changed", (out.id() != src_id) == bool_of(result, "id_changed"));
        check("width", i64::from(out.width()) == i64_of(result, "width"));
        check("height", i64::from(out.height()) == i64_of(result, "height"));
        check("layout", i64::from(out.layout().as_i32()) == i64_of(result, "layout"));
        check("stride", i64::from(out.stride()) == i64_of(result, "stride"));
        check("byte_count", out.byte_count() == i64_of(result, "byte_count"));
        let bytes = out.bytes().expect("not disposed");
        check("fnv1a64", fnv1a64(bytes) == str_of(result, "fnv1a64"));
        if let Some(expected_hex) = result.get("hex").and_then(Value::as_str) {
            check("hex", hex(bytes) == expected_hex);
        }
    }
    check_all("pixel-ops orient", &failures);
}

// Golden `pixel-ops`, kind `compose` (20 cases): chains of orientations return the original pixels when they should.
#[test]
fn golden_pixel_ops_compose_matches_pixels() {
    let inputs = input_cases("pixel-ops");
    let golden = golden::load("pixel-ops");
    let mut failures = Vec::new();
    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "compose") {
        let name = str_of(case, "name");
        let input = input_for(&inputs, name);
        let result = &case["result"];
        let (width, height) = (i32_of(input, "width"), i32_of(input, "height"));
        let layout = layout_of(input);
        let seed = i64_of(input, "seed") as u64;
        let original = create_pattern(width, height, layout, seed);

        let steps: Vec<i32> = input["steps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|step| i32::try_from(step.as_i64().expect("step is an integer")).expect("fits"))
            .collect();
        let current = match orient_chain(create_pattern(width, height, layout, seed), &steps) {
            Ok(current) => current,
            Err(error) => {
                failures.push(format!("{name}: unexpected error {error}"));
                continue;
            }
        };

        let equals_original = current.width() == original.width()
            && current.height() == original.height()
            && current.bytes().expect("not disposed") == original.bytes().expect("not disposed");
        if equals_original != bool_of(result, "equals_original") {
            failures.push(format!("{name}: equals_original is {equals_original}"));
        }
        let bytes = current.bytes().expect("not disposed");
        if i64::try_from(bytes.len()).expect("fits") != i64_of(result, "byte_count") {
            failures.push(format!("{name}: byte_count differs"));
        }
        if fnv1a64(bytes) != str_of(result, "fnv1a64") {
            failures.push(format!("{name}: fnv1a64 differs"));
        }
        if let Some(expected_hex) = result.get("hex").and_then(Value::as_str) {
            if hex(bytes) != expected_hex {
                failures.push(format!("{name}: hex differs"));
            }
        }
    }
    check_all("pixel-ops compose", &failures);
}

// Compose note of the contract: two 90-degree turns and one 180-degree turn of the same pattern are the same pixels.
#[test]
fn compose_rotate_90_twice_equals_rotate_180_once() {
    for (width, height) in [(1, 1), (5, 3), (33, 31), (70, 45)] {
        let twice = orient(
            orient(create_pattern(width, height, PixelLayout::Pbgra32, 12), 6).expect("valid"),
            6,
        )
        .expect("valid");
        let once = orient(create_pattern(width, height, PixelLayout::Pbgra32, 12), 3).expect("valid");
        assert_eq!(
            fnv1a64(twice.bytes().expect("not disposed")),
            fnv1a64(once.bytes().expect("not disposed"))
        );
        assert_equal(&once, &twice);
    }
}

// Golden `pixel-ops`, kind `alpha` (84 cases): `is_fully_opaque` and `has_alpha` on patterns with chosen alpha bytes.
#[test]
fn golden_pixel_ops_alpha_matches_pixels() {
    let inputs = input_cases("pixel-ops");
    let golden = golden::load("pixel-ops");
    let mut failures = Vec::new();
    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "alpha") {
        let name = str_of(case, "name");
        let input = input_for(&inputs, name);
        let result = &case["result"];
        let (width, height) = (i32_of(input, "width"), i32_of(input, "height"));
        let mut pixels = create_pattern(width, height, layout_of(input), i64_of(input, "seed") as u64);
        let stride = pixels.stride() as usize;
        let alpha_fill = u8::try_from(i64_of(input, "alpha_fill")).expect("alpha byte");
        let colour_fill = input.get("colour_fill").and_then(Value::as_i64).map(|c| c as u8);
        {
            let bytes = pixels.bytes_mut().expect("not disposed");
            for pixel in bytes.chunks_exact_mut(4) {
                pixel[3] = alpha_fill;
                if let Some(colour) = colour_fill {
                    pixel[..3].fill(colour);
                }
            }
            for set in input["alpha_set"].as_array().map(Vec::as_slice).unwrap_or_default() {
                let x = i32_of(set, "x") as usize;
                let y = i32_of(set, "y") as usize;
                bytes[y * stride + x * 4 + 3] = u8::try_from(i64_of(set, "a")).expect("alpha byte");
            }
        }

        let opaque = is_fully_opaque(&pixels).expect("not disposed");
        let alpha = has_alpha(&pixels).expect("not disposed");
        if opaque != bool_of(result, "is_fully_opaque") {
            failures.push(format!("{name}: is_fully_opaque is {opaque}"));
        }
        if alpha != bool_of(result, "has_alpha") {
            failures.push(format!("{name}: has_alpha is {alpha}"));
        }
    }
    check_all("pixel-ops alpha", &failures);
}

// Golden `pixel-ops`, kind `disposed` (8 cases): every operation on a disposed buffer throws ObjectDisposedException.
#[test]
fn golden_pixel_ops_disposed_matches_pixels() {
    let inputs = input_cases("pixel-ops");
    let golden = golden::load("pixel-ops");
    let mut failures = Vec::new();
    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "disposed") {
        let name = str_of(case, "name");
        let input = input_for(&inputs, name);
        let expected = str_of(&case["result"], "throws");
        let mut buffer = PixelBuffer::allocate(i32_of(input, "width"), i32_of(input, "height"), layout_of(input))
            .expect("valid size");
        buffer.dispose();
        let actual = match str_of(input, "op") {
            op @ ("orient-6" | "orient-1") => {
                let orientation: i32 = op.trim_start_matches("orient-").parse().expect("orientation");
                err_name(orient(buffer, orientation).map(|_| ()))
            }
            "is_fully_opaque" => err_name(is_fully_opaque(&buffer).map(|_| ())),
            "has_alpha" => err_name(has_alpha(&buffer).map(|_| ())),
            other => panic!("unknown op {other} in {name}"),
        };
        if actual != expected {
            failures.push(format!("{name}: expected {expected}, got {actual}"));
        }
    }
    check_all("pixel-ops disposed", &failures);
}

// Golden `orientation-pixels` (W4-02a golden, run here against `apply_orientation`): every destination pixel names its
// source index. Pixel `i` of the source holds the unique id `i` (little-endian u32 in its 4 bytes).
#[test]
fn golden_orientation_pixels_placement_through_apply_orientation() {
    let inputs = input_cases("orientation-pixels");
    let golden = golden::load("orientation-pixels");
    let mut failures = Vec::new();
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = input_for(&inputs, name);
        let result = case;
        let (width, height) = (i32_of(input, "width"), i32_of(input, "height"));
        let mut src = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).expect("valid size");
        for (index, pixel) in src.bytes_mut().expect("not disposed").chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&(index as u32).to_le_bytes());
        }
        let src_id = src.id();

        let out = match orient(src, i32_of(input, "orientation")) {
            Ok(out) => out,
            Err(error) => {
                failures.push(format!("{name}: unexpected error {error}"));
                continue;
            }
        };
        if (out.id() == src_id) != bool_of(result, "same_instance") {
            failures.push(format!("{name}: same_instance differs"));
        }
        if i64::from(out.width()) != i64_of(result, "out_width")
            || i64::from(out.height()) != i64_of(result, "out_height")
        {
            failures.push(format!("{name}: destination size differs"));
            continue;
        }
        let bytes = out.bytes().expect("not disposed");
        let expected_ids = result["source_ids"].as_array().map(Vec::as_slice).unwrap_or_default();
        for (index, expected) in expected_ids.iter().enumerate() {
            let offset = index * 4;
            let actual = u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"));
            if i64::from(actual) != expected.as_i64().expect("id") {
                failures.push(format!("{name}: destination pixel {index} holds source {actual}"));
            }
        }
    }
    check_all("orientation-pixels", &failures);
}

// ---------------------------------------------------------------------------------------------------------------------
// Repair round 1: validation order, ids, mutable span, unallocated-block accessors, error texts
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn allocate_validation_order_matches_csharp() {
    // C# order: width < 1, height < 1, then width > i32::MAX / 4.
    assert_eq!(
        PixelBuffer::allocate(i32::MAX, 0, PixelLayout::Bgr32).unwrap_err(),
        PixelBufferError::HeightOutOfRange { height: 0 }
    );
    assert_eq!(
        PixelBuffer::allocate(i32::MAX / 4 + 1, -5, PixelLayout::Pbgra32).unwrap_err(),
        PixelBufferError::HeightOutOfRange { height: -5 }
    );
    assert_eq!(
        PixelBuffer::allocate(0, 0, PixelLayout::Bgr32).unwrap_err(),
        PixelBufferError::WidthOutOfRange { width: 0 }
    );
    assert_eq!(
        PixelBuffer::allocate(i32::MAX, 1, PixelLayout::Bgr32).unwrap_err(),
        PixelBufferError::WidthOutOfRange { width: i32::MAX }
    );
}

#[test]
fn ids_are_positive_unique_and_increasing() {
    let first = PixelBuffer::allocate(1, 1, PixelLayout::Bgr32).expect("alloc");
    assert!(first.id() >= 1, "ids start at 1, got {}", first.id());
    let hook = PixelBuffer::unallocated_for_tests(2, 2, PixelLayout::Pbgra32);
    assert!(hook.id() > first.id());
    let hook2 = PixelBuffer::unallocated_for_tests(2, 2, PixelLayout::Pbgra32);
    assert!(hook2.id() > hook.id());
    let last = PixelBuffer::allocate(1, 1, PixelLayout::Bgr32).expect("alloc");
    assert!(last.id() > hook2.id());
    assert!(hook.id() >= 1 && hook2.id() >= 1);
}

#[test]
fn try_get_span_mut_on_small_buffer_is_some_and_writable() {
    let mut buffer = PixelBuffer::allocate(3, 2, PixelLayout::Pbgra32).expect("alloc");
    assert!(buffer.id() >= 1);
    {
        let span = buffer.try_get_span_mut().expect("not disposed").expect("fits");
        assert_eq!(span.len(), 24);
        span[23] = 0xAB;
    }
    assert_eq!(buffer.bytes().expect("bytes")[23], 0xAB);
    buffer.dispose();
    assert_eq!(buffer.try_get_span_mut().unwrap_err(), PixelBufferError::Disposed);
}

#[test]
fn try_get_span_mut_over_limit_is_none_on_unallocated() {
    let mut huge = PixelBuffer::unallocated_for_tests(i32::MAX / 4, 3, PixelLayout::Bgr32);
    assert!(!fits_in_span(huge.byte_count()));
    assert!(huge.try_get_span_mut().expect("not disposed").is_none());
    let mut small = PixelBuffer::unallocated_for_tests(4, 4, PixelLayout::Bgr32);
    assert!(
        small
            .try_get_span_mut()
            .expect("not disposed")
            .expect("fits")
            .is_empty()
    );
}

#[test]
fn unallocated_block_accessors_are_empty_not_crashing() {
    let mut buffer = PixelBuffer::unallocated_for_tests(8, 8, PixelLayout::Pbgra32);
    assert!(!buffer.is_disposed());
    assert!(buffer.bytes().expect("bytes").is_empty());
    assert!(buffer.bytes_mut().expect("bytes_mut").is_empty());
    assert!(buffer.get_row(0).expect("row").is_empty());
    assert!(buffer.get_row_mut(7).expect("row_mut").is_empty());
    assert!(buffer.try_get_span().expect("span").expect("fits").is_empty());
}

#[test]
fn error_texts_are_stable_and_distinct() {
    let width = PixelBufferError::WidthOutOfRange { width: 0 }.to_string();
    assert!(width.contains("Width") && width.contains("was 0") && width.contains(&(i32::MAX / 4).to_string()));
    let height = PixelBufferError::HeightOutOfRange { height: -3 }.to_string();
    assert!(height.contains("Height") && height.contains("-3"));
    assert!(PixelBufferError::UnknownLayout(7).to_string().contains("layout 7"));
    let oom = PixelBufferError::OutOfMemory { byte_count: 123 }.to_string();
    assert!(oom.contains("123"));
    assert!(PixelBufferError::Disposed.to_string().contains("disposed"));
    let row = PixelBufferError::RowOutOfRange { y: 9, height: 4 }.to_string();
    assert!(row.contains("Row 9") && row.contains("height 4"));
}
