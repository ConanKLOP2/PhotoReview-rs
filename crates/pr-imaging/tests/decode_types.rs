//! Tests for `pr_imaging::decode_types` (port of `DecodeBox.cs`, `Decoding/DecodeRequest.cs`, `Decoding/ImageInfo.cs`,
//! `Decoding/DecodedImage.cs`, `Decoding/IDecodedImage.cs`, `Decoding/PixelLease.cs` and
//! `Decoding/DecodeFailureSourceBytes.cs`, C# baseline d92d6aae). Each test names the C# test it ports in a `// C#:` comment.
//!
//! The golden files `tests/golden/decode-box.json` and `tests/golden/decode-types.json` hold the C# results; their inputs are in
//! `tests/golden-inputs/` and are matched to the outputs by case name (same order, checked per case).
//!
//! Red phase (W4-01b): every test here fails through `todo!()` in the stub until the implementation lands.

use std::sync::Arc;

use pr_core::abstractions::SourceReadPriority;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{
    DecodeBox, DecodeFailureSourceBytes, DecodeRequest, DecodedImage, DecodedImageError, DecodedImageOptions,
    DecodedImageView, ImageInfo, PixelLease, PlatformImage, has_known_original,
};
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Deterministic xorshift64 RNG (no `rand` crate). The C# property tests use `System.Random` with a seed; the exact values are
/// not reproduced, only determinism and the same size/side bucket distributions.
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// `rng.Next(lo, hi)`: a value in `[lo, hi)`.
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + i32::try_from(self.below(u64::try_from(hi - lo).unwrap())).unwrap()
    }
}

/// `RandomSize` of the C# property test (7 buckets, includes `int.MaxValue - n`).
fn random_size(rng: &mut XorShift) -> i32 {
    match rng.below(7) {
        0 => rng.range(1, 4),
        1 => rng.range(1, 300),
        2 => rng.range(1, 8_000),
        3 => rng.range(30_000, 70_000),
        4 => i32::MAX - i32::try_from(rng.below(3)).unwrap(),
        5 => rng.range(1, 200_000),
        _ => rng.range(1, 100),
    }
}

/// `RandomBoxSide` of the C# property test (6 buckets, includes zero, `int.MaxValue - n` and negative sides).
fn random_box_side(rng: &mut XorShift) -> i32 {
    match rng.below(6) {
        0 => 0,
        1 => rng.range(1, 10),
        2 => rng.range(1, 200),
        3 => rng.range(100, 4_100),
        4 => i32::MAX - i32::try_from(rng.below(3)).unwrap(),
        _ => -i32::try_from(rng.below(5)).unwrap(),
    }
}

/// `(width, height)` of a golden size object.
fn dims(v: &Value) -> (i32, i32) {
    (i32_of(v, "width"), i32_of(v, "height"))
}

fn i32_of(v: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(v, key)).unwrap_or_else(|_| panic!("'{key}' out of i32 range: {v}"))
}

/// The input cases of `tests/golden-inputs/<name>.json`, in file order.
fn inputs(name: &str) -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

fn backend_of(name: &str) -> DecoderBackend {
    match name {
        "Wpf" => DecoderBackend::Wpf,
        "WicDirect" => DecoderBackend::WicDirect,
        "TurboJpeg" => DecoderBackend::TurboJpeg,
        "LibRaw" => DecoderBackend::LibRaw,
        other => panic!("unknown backend '{other}' in golden"),
    }
}

fn backend_name(backend: DecoderBackend) -> &'static str {
    match backend {
        DecoderBackend::Wpf => "Wpf",
        DecoderBackend::WicDirect => "WicDirect",
        DecoderBackend::TurboJpeg => "TurboJpeg",
        DecoderBackend::LibRaw => "LibRaw",
    }
}

fn priority_name(priority: SourceReadPriority) -> &'static str {
    match priority {
        SourceReadPriority::Viewer => "Viewer",
        SourceReadPriority::Preload => "Preload",
    }
}

/// A platform image that is not a pixel buffer (C# `new object()`).
fn opaque() -> PlatformImage {
    PlatformImage::Opaque(Arc::new(()))
}

/// A platform image holding a freshly allocated `width` x `height` pixel buffer.
fn pixels(width: i32, height: i32) -> PlatformImage {
    let buffer = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).expect("allocate test pixels");
    PlatformImage::Pixels(Arc::new(buffer))
}

/// A decoded-image view that overrides only what the C# fake overrides.
struct SizedImage {
    downscaled: bool,
    pixel_w: i32,
    pixel_h: i32,
    orig_w: i32,
    orig_h: i32,
    platform: PlatformImage,
}

impl DecodedImageView for SizedImage {
    fn pixel_width(&self) -> i32 {
        self.pixel_w
    }
    fn pixel_height(&self) -> i32 {
        self.pixel_h
    }
    fn downscaled(&self) -> bool {
        self.downscaled
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn estimated_bytes(&self) -> i64 {
        0
    }
    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
    fn original_width(&self) -> i32 {
        self.orig_w
    }
    fn original_height(&self) -> i32 {
        self.orig_h
    }
}

/// The interface defaults only: a view that overrides the required members and nothing else.
struct BareImage {
    platform: PlatformImage,
}

impl DecodedImageView for BareImage {
    fn pixel_width(&self) -> i32 {
        10
    }
    fn pixel_height(&self) -> i32 {
        7
    }
    fn downscaled(&self) -> bool {
        true
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn estimated_bytes(&self) -> i64 {
        0
    }
    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// DecodeBoxTests
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecodeBoxTests.Fit_LandscapePortraitPanorama_FitsInsideBox (4 inline cases)
#[test]
fn fit_landscape_portrait_panorama_fits_inside_box() {
    let cases = [
        (6000, 4000, 2304, 1280, 1920, 1280),
        (4000, 6000, 2304, 1280, 853, 1280),
        (9000, 2000, 2304, 1280, 2304, 512),
        (3600, 2000, 1800, 1000, 1800, 1000),
    ];
    for (src_w, src_h, box_w, box_h, expected_w, expected_h) in cases {
        let (w, h) = DecodeBox::new(box_w, box_h).fit(src_w, src_h);
        assert_eq!(
            (w, h),
            (expected_w, expected_h),
            "source {src_w}x{src_h} in box {box_w}x{box_h}"
        );
        assert!(w <= box_w && h <= box_h, "result {w}x{h} exceeds box {box_w}x{box_h}");
    }
}

// C#: DecodeBoxTests.Fit_SourceInsideBox_ReturnsSourceUnchanged (3 inline cases)
#[test]
fn fit_source_inside_box_returns_source_unchanged() {
    for (src_w, src_h) in [(800, 600), (2304, 1280), (100, 1280)] {
        assert_eq!(
            DecodeBox::new(2304, 1280).fit(src_w, src_h),
            (src_w, src_h),
            "source {src_w}x{src_h}"
        );
    }
}

// C#: DecodeBoxTests.Fit_Unbounded_ReturnsSource
#[test]
fn fit_unbounded_returns_source() {
    assert!(DecodeBox::UNBOUNDED.is_unbounded());
    assert_eq!(DecodeBox::UNBOUNDED.fit(6000, 4000), (6000, 4000));
}

// C#: DecodeBoxTests.Fit_WidthOnly_MatchesLegacyFloor
#[test]
fn fit_width_only_matches_legacy_floor() {
    // Legacy decoders: targetH = origH * TargetWidth / origW (integer floor).
    assert_eq!(DecodeBox::new(2190, 0).fit(6000, 4000), (2190, 1460));
    assert_eq!(DecodeBox::new(2190, 0).fit(4000, 6000), (2190, 3285));
}

// C#: DecodeBoxTests.Fit_HeightOnly_ConstrainsHeight
#[test]
fn fit_height_only_constrains_height() {
    assert_eq!(DecodeBox::new(0, 1000).fit(6000, 4000), (1500, 1000));
}

// C#: DecodeBoxTests.FitStored_Transposed_UsesRotatedDimensions (2 inline cases)
#[test]
fn fit_stored_transposed_uses_rotated_dimensions() {
    // Stored 6000x4000 with EXIF 6/8 displays as 4000x6000: the stored-grid result is 1280x853 when transposed, 1920x1280 when not.
    let cases = [(true, 1280, 853), (false, 1920, 1280)];
    for (transposed, expected_w, expected_h) in cases {
        let (w, h) = DecodeBox::new(2304, 1280).fit_stored(6000, 4000, transposed);
        assert_eq!((w, h), (expected_w, expected_h), "transposed={transposed}");
    }
}

// C#: DecodeBoxTests.Fit_DegenerateSource_ReturnsSource
#[test]
fn fit_degenerate_source_returns_source() {
    assert_eq!(DecodeBox::new(100, 100).fit(0, 10), (0, 10));
}

// ---------------------------------------------------------------------------------------------------------------------
// DecodeBoxPropertyTests
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecodeBoxPropertyTests.Fit_SatisfiesGeometricInvariants (200 000 iterations, seed 2026; the decimal reference is i128
// floor division with the constraining side decided by cross-multiplication, which is exact for positive operands)
#[test]
fn fit_satisfies_geometric_invariants() {
    let mut rng = XorShift::new(2026);
    for _ in 0..200_000 {
        let src_w = random_size(&mut rng);
        let src_h = random_size(&mut rng);
        let decode_box = DecodeBox::new(random_box_side(&mut rng), random_box_side(&mut rng));
        let context = format!("src={src_w}x{src_h} box={}x{}", decode_box.width, decode_box.height);

        let (w, h) = decode_box.fit(src_w, src_h);

        assert!(
            w >= 1 && h >= 1 && w <= src_w && h <= src_h,
            "out of [1, source]: {context} -> {w}x{h}"
        );
        if decode_box.width > 0 {
            assert!(
                w <= decode_box.width || w == src_w,
                "exceeds box width: {context} -> {w}x{h}"
            );
        }
        if decode_box.height > 0 {
            assert!(
                h <= decode_box.height || h == src_h,
                "exceeds box height: {context} -> {w}x{h}"
            );
        }

        let width_exceeds = decode_box.width > 0 && src_w > decode_box.width;
        let height_exceeds = decode_box.height > 0 && src_h > decode_box.height;
        if !width_exceeds && !height_exceeds {
            assert_eq!(
                (w, h),
                (src_w, src_h),
                "source already fits so it must be unchanged: {context}"
            );
            continue;
        }

        // Reference: the tighter ratio wins. An unbounded side has an infinite ratio (C# decimal.MaxValue).
        let (bw, bh) = (i128::from(decode_box.width), i128::from(decode_box.height));
        let (sw, sh) = (i128::from(src_w), i128::from(src_h));
        let width_is_tighter = if decode_box.width <= 0 {
            false
        } else if decode_box.height <= 0 {
            true
        } else {
            bw * sh <= bh * sw
        };
        if width_is_tighter {
            assert_eq!(
                w, decode_box.width,
                "width should be the constraining side: {context} -> {w}x{h}"
            );
            assert_eq!(
                i128::from(h),
                (sh * bw / sw).max(1),
                "wrong derived height: {context} -> {w}x{h}"
            );
        } else {
            assert_eq!(
                h, decode_box.height,
                "height should be the constraining side: {context} -> {w}x{h}"
            );
            assert_eq!(
                i128::from(w),
                (sw * bh / sh).max(1),
                "wrong derived width: {context} -> {w}x{h}"
            );
        }
    }
}

// C#: DecodeBoxPropertyTests.FitStored_IsFitOnTheDisplayedSize (50 000 iterations, seed 77)
#[test]
fn fit_stored_is_fit_on_the_displayed_size() {
    let mut rng = XorShift::new(77);
    for _ in 0..50_000 {
        let src_w = random_size(&mut rng);
        let src_h = random_size(&mut rng);
        let decode_box = DecodeBox::new(random_box_side(&mut rng), random_box_side(&mut rng));

        assert_eq!(decode_box.fit(src_w, src_h), decode_box.fit_stored(src_w, src_h, false));
        let (dw, dh) = decode_box.fit(src_h, src_w);
        assert_eq!((dh, dw), decode_box.fit_stored(src_w, src_h, true));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// PixelBufferImageCodecTests: the DecodedImage and PixelLease part (the codec tests are W4-16b)
// ---------------------------------------------------------------------------------------------------------------------

// C#: PixelBufferImageCodecTests.DecodedImage_CarriesEveryArgument
#[test]
fn decoded_image_carries_every_argument() {
    let exif = ExifSummary {
        camera_model: Some("Cam".to_owned()),
        ..ExifSummary::default()
    };
    let options = DecodedImageOptions {
        downscaled: true,
        orientation: 6,
        actual_backend: DecoderBackend::TurboJpeg,
        original_width: 3000,
        original_height: 2000,
        exif: Some(exif),
        is_degraded_fallback: true,
    };

    let image = DecodedImage::new(pixels(3, 2), 3, 2, 24, options).expect("valid arguments");

    assert!(image.downscaled());
    assert_eq!(image.orientation(), 6);
    assert_eq!(image.actual_backend(), DecoderBackend::TurboJpeg);
    assert_eq!(image.original_width(), 3000);
    assert_eq!(image.original_height(), 2000);
    assert!(image.exif().is_some());
    assert_eq!(image.exif().and_then(|e| e.camera_model.as_deref()), Some("Cam"));
    assert!(image.is_degraded_fallback());
}

// C#: PixelBufferImageCodecTests.DecodedImage_NonPositiveOriginal_FallsBackToPixelSize (4 inline cases)
#[test]
fn decoded_image_non_positive_original_falls_back_to_pixel_size() {
    let cases = [(0, 0), (-5, -1), (0, 900), (1200, 0)];
    for (original_width, original_height) in cases {
        let options = DecodedImageOptions {
            original_width,
            original_height,
            ..DecodedImageOptions::default()
        };
        let image = DecodedImage::new(opaque(), 40, 30, 4800, options).expect("valid arguments");

        assert_eq!(
            image.original_width(),
            if original_width > 0 { original_width } else { 40 },
            "original_width"
        );
        assert_eq!(
            image.original_height(),
            if original_height > 0 { original_height } else { 30 },
            "original_height"
        );
    }
}

// C#: PixelBufferImageCodecTests.DecodedImage_RejectsInvalidArguments
// The C# null-argument line has no Rust twin (a Rust platform image cannot be null) and is dropped.
#[test]
fn decoded_image_rejects_invalid_arguments() {
    let width_too_small = DecodedImage::new(opaque(), 0, 3, 48, DecodedImageOptions::default()).expect_err("width 0");
    assert!(
        matches!(width_too_small, DecodedImageError::PixelWidthTooSmall(0)),
        "{width_too_small:?}"
    );

    let height_too_small = DecodedImage::new(opaque(), 4, 0, 48, DecodedImageOptions::default()).expect_err("height 0");
    assert!(
        matches!(height_too_small, DecodedImageError::PixelHeightTooSmall(0)),
        "{height_too_small:?}"
    );

    let negative_bytes = DecodedImage::new(opaque(), 4, 3, -1, DecodedImageOptions::default()).expect_err("bytes -1");
    assert!(
        matches!(negative_bytes, DecodedImageError::NegativeEstimatedBytes(-1)),
        "{negative_bytes:?}"
    );

    // The declared size must match the buffer (for example a size given before a 90-degree turn).
    for (declared_w, declared_h) in [(3, 4), (4, 4), (5, 3)] {
        let mismatch = DecodedImage::new(pixels(4, 3), declared_w, declared_h, 48, DecodedImageOptions::default())
            .expect_err("buffer size mismatch");
        assert!(
            matches!(mismatch, DecodedImageError::PixelBufferSizeMismatch { .. }),
            "{declared_w}x{declared_h}: {mismatch:?}"
        );
    }
}

// Rust-only: the guard order of the constructor (width, then height, then bytes, then buffer size) is part of the contract;
// the C# message text is not in the golden, so the order is pinned here.
#[test]
fn decoded_image_guard_order_is_width_then_height_then_bytes() {
    let width_first =
        DecodedImage::new(opaque(), 0, 0, -1, DecodedImageOptions::default()).expect_err("all three invalid");
    assert!(
        matches!(width_first, DecodedImageError::PixelWidthTooSmall(0)),
        "{width_first:?}"
    );

    let height_before_bytes =
        DecodedImage::new(opaque(), 4, 0, -1, DecodedImageOptions::default()).expect_err("height and bytes");
    assert!(
        matches!(height_before_bytes, DecodedImageError::PixelHeightTooSmall(0)),
        "{height_before_bytes:?}"
    );
}

// C#: PixelBufferImageCodecTests.DecodedImage_AcceptsZeroEstimateAndOneByOne
#[test]
fn decoded_image_accepts_zero_estimate_and_one_by_one() {
    let image = DecodedImage::new(opaque(), 1, 1, 0, DecodedImageOptions::default()).expect("valid arguments");

    assert_eq!(image.estimated_bytes(), 0);
    assert_eq!(image.pixel_width(), 1);
    assert_eq!(image.pixel_height(), 1);
}

// C#: PixelBufferImageCodecTests.DecodedImage_Defaults (not in the contract list; ported for the WicDirect default of the class)
#[test]
fn decoded_image_defaults() {
    let image = DecodedImage::new(pixels(6, 4), 6, 4, 96, DecodedImageOptions::default()).expect("valid arguments");

    assert_eq!(
        (image.pixel_width(), image.pixel_height(), image.estimated_bytes()),
        (6, 4, 96)
    );
    assert!(!image.downscaled());
    assert_eq!(image.orientation(), 1);
    assert_eq!(image.actual_backend(), DecoderBackend::WicDirect);
    assert_eq!((image.original_width(), image.original_height()), (6, 4));
    assert!(image.exif().is_none());
    assert!(!image.is_degraded_fallback());
}

// C#: PixelBufferImageCodecTests.Lease_Owned_DisposesItsPixels
// Partial port: the owned lease, its pixels and `is_owned` are asserted. The C# `pixels.IsDisposed` after `Dispose` has no Rust
// twin: the buffer is moved into the lease, and its free happens in `PixelBuffer`'s own `Drop` (W4-16a) with nothing left to
// observe. Documented skip for that single assertion.
#[test]
fn lease_owned_is_owned_and_holds_its_pixels() {
    let lease = PixelLease::owned_buffer(PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("allocate"));

    assert!(lease.is_owned());
    assert!(lease.pixels().is_some());
    drop(lease);
}

// C#: PixelBufferImageCodecTests.Lease_Borrowed_LeavesPixelsAlive
#[test]
fn lease_borrowed_leaves_pixels_alive() {
    let pixels = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("allocate");

    {
        let lease = PixelLease::borrowed(&pixels);
        assert!(!lease.is_owned());
        assert!(lease.pixels().is_some());
    }

    assert!(!pixels.is_disposed());
}

// C#: PixelBufferImageCodecTests.Lease_DefaultIsEmpty_AndNullIsRejected
// The `new PixelLease(null, owned: true)` assertion has no Rust twin (no null in Rust) and is dropped.
#[test]
fn lease_default_is_empty() {
    let empty = PixelLease::empty();

    assert!(empty.pixels().is_none());
    assert!(!empty.is_owned());
    drop(empty);
}

// ---------------------------------------------------------------------------------------------------------------------
// DecodingHelpersMutationTests: HasKnownOriginal
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecodingHelpersMutationTests.HasKnownOriginal_FullDecodeOrOriginalLargerThanThePixels_IsTrue (11 inline cases)
#[test]
fn has_known_original_full_decode_or_original_larger_than_the_pixels_is_true() {
    let cases = [
        (false, 10, 10, 10, 10, true),
        (false, 10, 10, 5, 5, true),
        (true, 10, 10, 20, 20, true),
        (true, 10, 10, 20, 10, true),
        (true, 10, 10, 10, 20, true),
        (true, 10, 10, 10, 10, false),
        (true, 10, 10, 5, 5, false),
        (true, 10, 10, 5, 10, false),
        (true, 10, 10, 10, 5, false),
        (true, 10, 10, 5, 20, true),
        (true, 10, 10, 20, 5, true),
    ];
    for (downscaled, pixel_w, pixel_h, orig_w, orig_h, expected) in cases {
        let image = SizedImage {
            downscaled,
            pixel_w,
            pixel_h,
            orig_w,
            orig_h,
            platform: opaque(),
        };
        assert_eq!(
            has_known_original(&image),
            expected,
            "downscaled={downscaled} pixels {pixel_w}x{pixel_h} original {orig_w}x{orig_h}"
        );
    }
}

// C#: ErrorHandlingReviewDecodingTests.UnknownOriginal_IsNotReportedAsKnown (ported with DecodedImage and an opaque platform image)
#[test]
fn unknown_original_is_not_reported_as_known() {
    let unknown = DecodedImage::new(
        opaque(),
        32,
        24,
        4800,
        DecodedImageOptions {
            downscaled: true,
            ..DecodedImageOptions::default()
        },
    )
    .expect("valid arguments");
    let known = DecodedImage::new(
        opaque(),
        32,
        24,
        4800,
        DecodedImageOptions {
            downscaled: true,
            original_width: 640,
            original_height: 480,
            ..DecodedImageOptions::default()
        },
    )
    .expect("valid arguments");
    let full = DecodedImage::new(
        opaque(),
        32,
        24,
        4800,
        DecodedImageOptions {
            downscaled: false,
            ..DecodedImageOptions::default()
        },
    )
    .expect("valid arguments");

    assert!(!has_known_original(&unknown));
    assert!(has_known_original(&known));
    assert!(has_known_original(&full));
}

// ---------------------------------------------------------------------------------------------------------------------
// DecodeSizeDecisionMutationTests: IsDownscaleRequested
// ---------------------------------------------------------------------------------------------------------------------

// C#: DecodeSizeDecisionMutationTests.IsDownscaleRequested_EitherAxisPositive_IsTrue (9 inline cases)
#[test]
fn is_downscale_requested_either_axis_positive_is_true() {
    let cases = [
        (0, 0, false),
        (1, 0, true),
        (0, 1, true),
        (5, 7, true),
        (-1, 0, false),
        (0, -1, false),
        (-1, -1, false),
        (-3, 5, true),
        (5, -3, true),
    ];
    for (width, height, expected) in cases {
        let request = DecodeRequest::new("x.jpg", width).with_target_height(height);
        assert_eq!(
            request.is_downscale_requested(),
            expected,
            "width {width} height {height}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// OrientationTests: ImageInfo dimension swap
// ---------------------------------------------------------------------------------------------------------------------

// C#: OrientationTests.ImageInfoDimensionSwap (2 inline cases; orientations 0..9 and 255 are in the golden test below)
#[test]
fn image_info_dimension_swap() {
    let cases = [(1, 64, 48, 64, 48), (6, 64, 48, 48, 64)];
    for (orientation, pixel_width, pixel_height, expected_width, expected_height) in cases {
        let info = ImageInfo::with_orientation(pixel_width, pixel_height, orientation);

        assert_eq!((info.pixel_width, info.pixel_height), (pixel_width, pixel_height));
        assert_eq!(
            (info.width(), info.height()),
            (expected_width, expected_height),
            "orientation {orientation}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven tests
// ---------------------------------------------------------------------------------------------------------------------

// Golden: tests/golden/decode-box.json (oracle command `decode-box`, 152 cases). Inputs: tests/golden-inputs/decode-box.json.
#[test]
fn golden_decode_box_cases_match_csharp() {
    let golden = golden::load("decode-box");
    let inputs = inputs("decode-box");
    assert_eq!(golden.cases.len(), inputs.len(), "case count");

    for (case, input) in golden.cases.iter().zip(&inputs) {
        let name = str_of(case, "name");
        assert_eq!(name, str_of(input, "name"), "case order");
        let decode_box = DecodeBox::new(i32_of(input, "box_width"), i32_of(input, "box_height"));
        let (src_w, src_h) = (i32_of(input, "source_width"), i32_of(input, "source_height"));

        assert_eq!(
            decode_box.is_unbounded(),
            bool_of(case, "is_unbounded"),
            "{name}: is_unbounded"
        );
        assert_eq!(decode_box.fit(src_w, src_h), dims(&case["fit"]), "{name}: fit");
        assert_eq!(
            decode_box.fit_stored(src_w, src_h, false),
            dims(&case["fit_stored_false"]),
            "{name}: fit_stored_false"
        );
        assert_eq!(
            decode_box.fit_stored(src_w, src_h, true),
            dims(&case["fit_stored_true"]),
            "{name}: fit_stored_true"
        );
    }
}

fn check_image_info(name: &str, case: &Value, input: &Value) {
    let info = ImageInfo::with_orientation(i32_of(input, "w"), i32_of(input, "h"), i32_of(input, "orientation"));
    assert_eq!((info.width(), info.height()), dims(case), "{name}: displayed size");
}

fn check_request(name: &str, case: &Value, input: &Value) {
    let path = str_of(input, "path");
    let width = i32_of(input, "width");
    let height = i32_of(input, "height");
    let mut request = if bool_of(input, "from_box") {
        DecodeRequest::with_box(path, DecodeBox::new(width, height))
    } else {
        DecodeRequest::new(path, width).with_target_height(height)
    };
    request = request.with_apply_orientation(bool_of(input, "apply"));
    if !input["source_orientation"].is_null() {
        request = request.with_source_orientation(i32_of(input, "source_orientation"));
    }
    if str_of(input, "priority") == "Preload" {
        request = request.with_priority(SourceReadPriority::Preload);
    }
    if !input["bytes_length"].is_null() {
        let length = usize::try_from(i64_of(input, "bytes_length")).expect("non-negative length");
        request = request.with_bytes(Arc::from(vec![0_u8; length]));
    }

    assert_eq!(
        request.decode_box(),
        DecodeBox::new(i32_of(&case["box"], "width"), i32_of(&case["box"], "height")),
        "{name}: box"
    );
    assert_eq!(
        request.target_width,
        i32_of(case, "target_width"),
        "{name}: target_width"
    );
    assert_eq!(
        request.target_height,
        i32_of(case, "target_height"),
        "{name}: target_height"
    );
    assert_eq!(
        request.is_downscale_requested(),
        bool_of(case, "is_downscale_requested"),
        "{name}: is_downscale_requested"
    );
    assert_eq!(
        request.apply_orientation,
        bool_of(case, "apply_orientation"),
        "{name}: apply_orientation"
    );
    assert_eq!(
        priority_name(request.priority),
        str_of(case, "priority"),
        "{name}: priority"
    );
    assert_eq!(
        request.source_orientation.map(i64::from),
        case["source_orientation"].as_i64(),
        "{name}: source_orientation"
    );
    // None and Some(empty) are different: the golden keeps only the length, so the input decides Some/None.
    let got_length = request.bytes.as_ref().map(|b| b.len());
    let want_length = input["bytes_length"]
        .as_u64()
        .map(|n| usize::try_from(n).expect("length fits usize"));
    assert_eq!(got_length, want_length, "{name}: bytes presence and length");
    assert_eq!(
        i64_of(case, "bytes_length"),
        input["bytes_length"].as_i64().unwrap_or(0),
        "{name}: golden bytes_length"
    );
}

/// `"object"` -> an opaque platform image; `"pixels:WxH"` -> a pixel buffer of that size.
fn platform_of(spec: &str) -> PlatformImage {
    match spec.strip_prefix("pixels:") {
        Some(size) => {
            let (w, h) = size.split_once('x').expect("pixels:WxH");
            pixels(w.parse().expect("width"), h.parse().expect("height"))
        }
        None => {
            assert_eq!(spec, "object", "unknown platform spec");
            opaque()
        }
    }
}

fn check_decoded(name: &str, case: &Value, input: &Value) {
    let options = DecodedImageOptions {
        downscaled: bool_of(input, "downscaled"),
        orientation: i32_of(input, "orientation"),
        actual_backend: backend_of(str_of(input, "backend")),
        original_width: i32_of(input, "ow"),
        original_height: i32_of(input, "oh"),
        exif: None,
        is_degraded_fallback: bool_of(input, "degraded"),
    };
    let result = DecodedImage::new(
        platform_of(str_of(input, "platform")),
        i32_of(input, "w"),
        i32_of(input, "h"),
        i64_of(input, "estimated_bytes"),
        options,
    );

    let expected = &case["result"];
    if let Some(thrown) = expected["throws"].as_str() {
        let error = match result {
            Ok(_) => panic!("{name}: expected {thrown}, got an image"),
            Err(error) => error,
        };
        let matched = match thrown {
            "ArgumentOutOfRangeException" => matches!(
                error,
                DecodedImageError::PixelWidthTooSmall(_)
                    | DecodedImageError::PixelHeightTooSmall(_)
                    | DecodedImageError::NegativeEstimatedBytes(_)
            ),
            "ArgumentException" => matches!(error, DecodedImageError::PixelBufferSizeMismatch { .. }),
            other => panic!("{name}: unknown golden exception '{other}'"),
        };
        assert!(matched, "{name}: {error:?} is not {thrown}");
        return;
    }

    let image = result.unwrap_or_else(|e| panic!("{name}: unexpected error {e:?}"));
    assert_eq!(
        image.pixel_width(),
        i32_of(expected, "pixel_width"),
        "{name}: pixel_width"
    );
    assert_eq!(
        image.pixel_height(),
        i32_of(expected, "pixel_height"),
        "{name}: pixel_height"
    );
    assert_eq!(
        image.estimated_bytes(),
        i64_of(expected, "estimated_bytes"),
        "{name}: estimated_bytes"
    );
    assert_eq!(
        image.downscaled(),
        bool_of(expected, "downscaled"),
        "{name}: downscaled"
    );
    assert_eq!(
        image.orientation(),
        i32_of(expected, "orientation"),
        "{name}: orientation"
    );
    assert_eq!(
        backend_name(image.actual_backend()),
        str_of(expected, "actual_backend"),
        "{name}: actual_backend"
    );
    assert_eq!(
        image.original_width(),
        i32_of(expected, "original_width"),
        "{name}: original_width"
    );
    assert_eq!(
        image.original_height(),
        i32_of(expected, "original_height"),
        "{name}: original_height"
    );
    assert_eq!(
        image.is_degraded_fallback(),
        bool_of(expected, "is_degraded_fallback"),
        "{name}: is_degraded_fallback"
    );
    assert_eq!(
        image.exif().is_some(),
        bool_of(expected, "has_exif"),
        "{name}: has_exif"
    );
    assert_eq!(
        has_known_original(&image),
        bool_of(expected, "has_known_original"),
        "{name}: has_known_original"
    );
}

fn check_interface_defaults(name: &str, case: &Value, _input: &Value) {
    let image = BareImage { platform: opaque() };

    assert_eq!(
        backend_name(image.actual_backend()),
        str_of(case, "actual_backend"),
        "{name}: actual_backend"
    );
    assert_eq!(
        image.original_width(),
        i32_of(case, "original_width"),
        "{name}: original_width"
    );
    assert_eq!(
        image.original_height(),
        i32_of(case, "original_height"),
        "{name}: original_height"
    );
    assert_eq!(image.exif().is_some(), bool_of(case, "has_exif"), "{name}: has_exif");
    assert_eq!(
        image.is_degraded_fallback(),
        bool_of(case, "is_degraded_fallback"),
        "{name}: is_degraded_fallback"
    );
    assert_eq!(
        image.source_bytes_read().is_some(),
        bool_of(case, "is_source_read_metrics"),
        "{name}: is_source_read_metrics"
    );
    assert_eq!(
        image.embedded_preview_size().is_some(),
        bool_of(case, "is_raw_preview_info"),
        "{name}: is_raw_preview_info"
    );
    assert_eq!(
        has_known_original(&image),
        bool_of(case, "has_known_original"),
        "{name}: has_known_original"
    );
}

fn check_lease(name: &str, case: &Value, input: &Value) {
    let buffer = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("allocate");
    let lease = match str_of(input, "mode") {
        "default" => PixelLease::empty(),
        "borrowed" => PixelLease::borrowed(&buffer),
        "owned" => PixelLease::owned_buffer(PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("allocate")),
        other => panic!("{name}: unknown lease mode '{other}'"),
    };

    assert_eq!(
        lease.pixels().is_none(),
        bool_of(case, "pixels_null"),
        "{name}: pixels_null"
    );
    assert_eq!(lease.is_owned(), bool_of(case, "owned"), "{name}: owned");
}

fn bytes_of(values: &[Value]) -> Arc<[u8]> {
    values
        .iter()
        .map(|v| u8::try_from(v.as_u64().expect("byte")).expect("0..=255"))
        .collect::<Vec<u8>>()
        .into()
}

fn check_failure_bytes(name: &str, case: &Value, input: &Value) {
    let mut failure = DecodeFailureSourceBytes::none();
    assert_eq!(
        failure.try_get().is_some(),
        bool_of(case, "try_get_before"),
        "{name}: try_get before attach"
    );

    for key in ["first", "second"] {
        if let Some(values) = input[key].as_array() {
            failure.attach(bytes_of(values));
        }
    }

    let got: Option<Vec<u64>> = failure
        .try_get()
        .map(|bytes| bytes.iter().map(|b| u64::from(*b)).collect());
    assert_eq!(got.is_some(), bool_of(case, "try_get"), "{name}: try_get");
    let want: Option<Vec<u64>> = case["bytes"]
        .as_array()
        .map(|values| values.iter().map(|v| v.as_u64().expect("byte")).collect());
    assert_eq!(got, want, "{name}: bytes");
}

// Golden: tests/golden/decode-types.json (oracle command `decode-types`, 72 cases). Inputs: tests/golden-inputs/decode-types.json.
#[test]
fn golden_decode_types_cases_match_csharp() {
    let golden = golden::load("decode-types");
    let inputs = inputs("decode-types");
    assert_eq!(golden.cases.len(), inputs.len(), "case count");

    for (case, input) in golden.cases.iter().zip(&inputs) {
        let name = str_of(case, "name");
        assert_eq!(name, str_of(input, "name"), "case order");
        let kind = str_of(input, "kind");
        assert_eq!(str_of(case, "kind"), kind, "{name}: kind");
        match kind {
            "image-info" => check_image_info(name, case, input),
            "request" => check_request(name, case, input),
            "decoded" => check_decoded(name, case, input),
            "interface-defaults" => check_interface_defaults(name, case, input),
            "lease" => check_lease(name, case, input),
            "failure-bytes" => check_failure_bytes(name, case, input),
            other => panic!("{name}: unknown kind '{other}'"),
        }
    }
}
