//! Red tests (W8-01c, test porter) for `viewport::render_geometry`: the pure draw geometry of `RenderGeometry.cs`
//! (baseline `d92d6aae`). Contract: `contracts/W8-01c.md`.
//!
//! Every test calls a stub that still ends in `todo!()`, so every test fails until the bodies land. No GPU, no files
//! other than the golden read through `pr_test_support::golden`. Test names mirror the C# test methods; each ported
//! test carries a `// C#: RenderGeometryTests.<Method>` comment. Extra tests are marked `// new:`.
//!
//! Golden convention (`tests/golden/render-geometry.json`, written by `oracle-cs render-geometry`): doubles are
//! 16 hex digits of `f64::to_bits`, floats are 8 hex digits of `f32::to_bits`. Comparison is bit-exact, except that
//! any NaN equals any NaN (the sign bit of a NaN is not part of the contract). Input numbers may be the strings
//! "NaN", "Infinity", "-Infinity" or "-0".
//!
//! DIVERGENCE: none in these tests. The C# `OverflowException` path of `TileLayout.Split` (contract D-W8c-6) has no
//! golden case and no test here.
//!
//! Documented skips (non-portable C# tests):
//! - `PixelOps.ApplyOrientation` has no Rust counterpart. The EXIF test uses the explicit pixel table of the contract
//!   (note 5) instead of the pixel buffer.
//! - `Matrix3x2.Identity` / `CreateScale` / `CreateRotation` are `IDENTITY`, `scale`, `rotation` on our own type.
//! - The `out RectD snapped` parameter is the `PixelSnap` return value (D-W8c-5).
//! - `Assert.Equal(double, double, precision)` is `assert_close` with an absolute tolerance of half a unit in the
//!   given decimal place (the C# rounding rule, expressed as a tolerance).
//! - `TileLayoutError` `Display` text is not asserted: the porter adds `Display` in the implementation change, and the
//!   stub has none. Variant identity is asserted instead (`tile_layout_error_variants_match_c_param_names`).

use pr_app::input::primitives::RectD;
use pr_app::viewport::render_geometry::{
    ColorF, ImageInterpolation, Matrix3x2, PixelRect, TileLayoutError, TileSpec, color_from_rgb, compute_tiles,
    dark_canvas, for_exif_orientation, image_interpolation_from_scaling_quality, intersect, is_transposed,
    map_to_destination, pixel_rect_to_rect_d, rect_is_empty, try_snap_pixel_exact, unoriented_rect,
};
use pr_core::model::enums::ScalingQuality;
use pr_test_support::golden;
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// `Assert.Equal(expected, actual, precision)` of xUnit: equal once rounded to `decimals` places.
/// Expressed as an absolute tolerance of half a unit in that place.
fn assert_close(actual: f64, expected: f64, decimals: i32, what: &str) {
    let tolerance = 0.5 * 10f64.powi(-decimals);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected} (to {decimals} places), got {actual}"
    );
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

/// A golden number: a JSON number, or one of the oracle's string spellings.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => panic!("not a golden number: {other:?}"),
        },
        other => panic!("not a number: {other}"),
    }
}

/// A golden `[x, y, width, height]` array of numbers.
fn rect_array(value: &Value) -> RectD {
    let items = value.as_array().unwrap_or_else(|| panic!("not an array: {value}"));
    assert_eq!(items.len(), 4, "rect array length in {value}");
    rect(num(&items[0]), num(&items[1]), num(&items[2]), num(&items[3]))
}

/// A golden `[x, y, width, height]` array of integers.
fn pixel_array(value: &Value) -> PixelRect {
    let items = value.as_array().unwrap_or_else(|| panic!("not an array: {value}"));
    assert_eq!(items.len(), 4, "pixel array length in {value}");
    let at = |i: usize| {
        items[i]
            .as_i64()
            .unwrap_or_else(|| panic!("not an integer: {}", items[i])) as i32
    };
    PixelRect::new(at(0), at(1), at(2), at(3))
}

/// The 4 hex strings of a golden rect (`x, y, width, height`, f64 bits).
fn expected_rect_bits(value: &Value) -> [f64; 4] {
    let items = value.as_array().unwrap_or_else(|| panic!("not an array: {value}"));
    assert_eq!(items.len(), 4, "rect bits length in {value}");
    let bits = |i: usize| {
        let text = items[i]
            .as_str()
            .unwrap_or_else(|| panic!("not a hex string: {}", items[i]));
        f64::from_bits(u64::from_str_radix(text, 16).unwrap_or_else(|e| panic!("bad f64 hex {text:?}: {e}")))
    };
    [bits(0), bits(1), bits(2), bits(3)]
}

/// The f32 value of a golden hex string (8 digits).
fn expected_f32(text: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(text, 16).unwrap_or_else(|e| panic!("bad f32 hex {text:?}: {e}")))
}

fn same_f64(actual: f64, expected: f64) -> bool {
    (actual.is_nan() && expected.is_nan()) || actual.to_bits() == expected.to_bits()
}

fn same_f32(actual: f32, expected: f32) -> bool {
    (actual.is_nan() && expected.is_nan()) || actual.to_bits() == expected.to_bits()
}

/// Bit-exact comparison of a result rect with the golden rect bits (NaN equals NaN).
fn assert_rect_bits(actual: RectD, expected: &Value, ctx: &str) {
    let want = expected_rect_bits(expected);
    let got = [actual.x, actual.y, actual.width, actual.height];
    for (i, name) in ["x", "y", "width", "height"].iter().enumerate() {
        assert!(
            same_f64(got[i], want[i]),
            "{ctx}: {name} expected bits {:016X} ({}), got {:016X} ({})",
            want[i].to_bits(),
            want[i],
            got[i].to_bits(),
            got[i]
        );
    }
}

fn kind_name(case: &Value) -> &str {
    case["kind"]
        .as_str()
        .unwrap_or_else(|| panic!("case has no kind: {case}"))
}

// ---------------------------------------------------------------------------------------------------------------------
// Tile layout
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn tile_layout_cores_partition_image_textures_within_limit_and_gutter() {
    // C#: RenderGeometryTests.TileLayout_CoresPartitionImage_TexturesWithinLimitAndGutter (6 Theory rows)
    const GUTTER: i32 = 32;
    let rows: [(i32, i32, i32, i32, i32); 6] = [
        (100, 50, 16384, 1, 1),
        (16384, 16384, 16384, 1, 1),
        (30000, 2000, 16384, 2, 1),
        (2000, 40000, 16384, 1, 3),
        (1000, 1000, 256, 6, 6),
        (1000, 300, 256, 6, 2),
    ];
    for (width, height, max, columns, rows_expected) in rows {
        let ctx = format!("{width}x{height} max {max} columns {columns} rows {rows_expected}");
        let tiles = compute_tiles(width, height, max, GUTTER).unwrap_or_else(|e| panic!("{ctx}: {e:?}"));

        assert_eq!((columns * rows_expected) as usize, tiles.len(), "{ctx}: tile count");
        let mut area: i64 = 0;
        for tile in &tiles {
            let t = tile.texture;
            let c = tile.core;
            assert!((1..=max).contains(&t.width), "{ctx}: texture width {tile:?}");
            assert!((1..=max).contains(&t.height), "{ctx}: texture height {tile:?}");
            assert!(t.x <= c.x && t.right() >= c.right(), "{ctx}: x {tile:?}");
            assert!(t.y <= c.y && t.bottom() >= c.bottom(), "{ctx}: y {tile:?}");
            assert!(
                t.x >= 0 && t.right() <= width && t.y >= 0 && t.bottom() <= height,
                "{ctx}: inside image {tile:?}"
            );

            // The overlap border: the texture reaches the core by exactly GUTTER, clamped to the image.
            assert_eq!((c.x - GUTTER).max(0), t.x, "{ctx}: left gutter {tile:?}");
            assert_eq!(
                (c.right() + GUTTER).min(width),
                t.right(),
                "{ctx}: right gutter {tile:?}"
            );
            assert_eq!((c.y - GUTTER).max(0), t.y, "{ctx}: top gutter {tile:?}");
            assert_eq!(
                (c.bottom() + GUTTER).min(height),
                t.bottom(),
                "{ctx}: bottom gutter {tile:?}"
            );
            area += i64::from(c.width) * i64::from(c.height);
        }

        // The cores cover the image exactly once, and the core columns are consecutive.
        assert_eq!(
            i64::from(width) * i64::from(height),
            area,
            "{ctx}: cores cover the image"
        );
        let mut starts: Vec<i32> = tiles.iter().map(|t| t.core.x).collect();
        let mut ends: Vec<i32> = tiles.iter().map(|t| t.core.right()).collect();
        starts.sort_unstable();
        starts.dedup();
        ends.sort_unstable();
        ends.dedup();
        assert_eq!(0, starts[0], "{ctx}: first column starts at 0");
        assert_eq!(width, *ends.last().unwrap(), "{ctx}: last column ends at the width");
        assert_eq!(&starts[1..], &ends[..ends.len() - 1], "{ctx}: columns are consecutive");
    }
}

#[test]
fn tile_layout_single_tile_when_image_fits_has_no_gutter() {
    // C#: RenderGeometryTests.TileLayout_SingleTile_WhenImageFits_HasNoGutter
    let tiles = compute_tiles(6000, 4000, 16384, 64).expect("a 6000x4000 image fits one 16384 texture");

    assert_eq!(1, tiles.len(), "one tile");
    let tile = tiles[0];
    assert_eq!(PixelRect::new(0, 0, 6000, 4000), tile.texture);
    assert_eq!(tile.texture, tile.core);
}

#[test]
fn tile_layout_rejects_invalid_sizes() {
    // C#: RenderGeometryTests.TileLayout_RejectsInvalidSizes (ArgumentOutOfRangeException -> TileLayoutError)
    assert_eq!(
        Err(TileLayoutError::Width(0)),
        compute_tiles(0, 1, 16384, 64).map(|_| ())
    );
    assert_eq!(
        Err(TileLayoutError::Height(0)),
        compute_tiles(1, 0, 16384, 64).map(|_| ())
    );
    assert_eq!(
        Err(TileLayoutError::MaxTextureSize(2)),
        compute_tiles(1, 1, 2, 64).map(|_| ())
    );
}

#[test]
fn tile_layout_error_variants_match_c_param_names() {
    // new: each invalid parameter maps to the variant named after the C# ParamName (width, height, maxTextureSize,
    // gutter). Checked through the variant itself, so it compiles without the `Display` the porter adds later.
    assert!(matches!(compute_tiles(0, 1, 16384, 64), Err(TileLayoutError::Width(0))));
    assert!(matches!(
        compute_tiles(1, 0, 16384, 64),
        Err(TileLayoutError::Height(0))
    ));
    assert!(matches!(
        compute_tiles(1, 1, 3, 64),
        Err(TileLayoutError::MaxTextureSize(3))
    ));
    assert!(matches!(
        compute_tiles(1, 1, 16384, -1),
        Err(TileLayoutError::Gutter(-1))
    ));
}

#[test]
fn split_axis_clamps_gutter_before_splitting() {
    // new: trap T6. gutter 1000 with max 256 is clamped to 256 / 4 = 64 BEFORE the split (core = 256 - 128 = 128),
    // so a 500 px axis gives ceil(500 / 128) = 4 columns and 4 rows, and the texture of tile 0 is 192 px wide.
    let tiles = compute_tiles(500, 500, 256, 1000).expect("gutter clamps, the call is valid");

    assert_eq!(16, tiles.len(), "4 columns x 4 rows");
    assert_eq!(
        TileSpec {
            texture: PixelRect::new(0, 0, 192, 192),
            core: PixelRect::new(0, 0, 128, 128)
        },
        tiles[0]
    );
    assert_eq!(PixelRect::new(128, 0, 128, 128), tiles[1].core);
    assert_eq!(PixelRect::new(64, 0, 256, 192), tiles[1].texture);
    assert!(tiles.iter().all(|t| t.texture.width <= 256 && t.texture.height <= 256));
}

// ---------------------------------------------------------------------------------------------------------------------
// Draw geometry: rect helpers
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn map_to_destination_scales_and_offsets_part() {
    // C#: RenderGeometryTests.MapToDestination_ScalesAndOffsetsPart
    let mapped = map_to_destination(
        rect(10.0, 20.0, 100.0, 50.0),
        rect(0.0, 0.0, 50.0, 100.0),
        rect(30.0, 30.0, 20.0, 10.0),
    );

    assert_eq!(rect(10.0, 20.0, 10.0, 20.0), mapped);
}

#[test]
fn intersect_returns_overlap_or_empty() {
    // C#: RenderGeometryTests.Intersect_ReturnsOverlapOrEmpty
    assert_eq!(
        rect(5.0, 5.0, 5.0, 5.0),
        intersect(rect(0.0, 0.0, 10.0, 10.0), rect(5.0, 5.0, 10.0, 10.0))
    );
    assert!(rect_is_empty(intersect(
        rect(0.0, 0.0, 10.0, 10.0),
        rect(10.0, 0.0, 5.0, 5.0)
    )));
    assert!(!rect_is_empty(rect(0.0, 0.0, 1.0, 1.0)));
}

#[test]
fn pixel_rect_helpers_match_integer_rules() {
    // new: PixelRect right/bottom/is_empty (contract note 6) and the f64 widening used by DrawGeometry.ToRectD.
    let p = PixelRect::new(3, 4, 5, 6);
    assert_eq!(8, p.right());
    assert_eq!(10, p.bottom());
    assert!(!p.is_empty());
    assert!(PixelRect::new(0, 0, 0, 5).is_empty());
    assert!(PixelRect::new(0, 0, 5, -1).is_empty());
    assert_eq!(rect(3.0, 4.0, 5.0, 6.0), pixel_rect_to_rect_d(p));
}

// ---------------------------------------------------------------------------------------------------------------------
// Draw geometry: pixel snap
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn try_snap_pixel_exact_one_to_one_snaps_corner_to_device_pixel() {
    // C#: RenderGeometryTests.TrySnapPixelExact_OneToOne_SnapsCornerToDevicePixel
    let snap = try_snap_pixel_exact(
        Matrix3x2::IDENTITY,
        rect(10.3, 4.6, 100.0, 50.0),
        rect(0.0, 0.0, 100.0, 50.0),
    );

    assert!(snap.exact, "identity is exact");
    assert_close(snap.snapped.x, 10.0, 9, "snapped x");
    assert_close(snap.snapped.y, 5.0, 9, "snapped y");
    assert_eq!((100.0, 50.0), (snap.snapped.width, snap.snapped.height));
}

#[test]
fn try_snap_pixel_exact_high_dpi_uses_device_pixels() {
    // C#: RenderGeometryTests.TrySnapPixelExact_HighDpi_UsesDevicePixels
    // DPI 1.5: 100 source px = 66.67 DIP; the corner at 10.1 DIP = 15.15 px moves to 15 px = 10 DIP.
    let device = Matrix3x2::scale(1.5);
    let snap = try_snap_pixel_exact(
        device,
        rect(10.1, 0.0, 100.0 / 1.5, 50.0 / 1.5),
        rect(0.0, 0.0, 100.0, 50.0),
    );

    assert!(snap.exact, "scale 1.5 maps 100 source px onto 100 device px");
    assert_close(snap.snapped.x, 10.0, 4, "snapped x");
    assert_close(snap.snapped.y, 0.0, 4, "snapped y");
}

#[test]
fn try_snap_pixel_exact_rotated90_is_exact_and_snaps_through_inverse() {
    // C#: RenderGeometryTests.TrySnapPixelExact_Rotated90_IsExactAndSnapsThroughInverse
    let rotate = Matrix3x2::new(0.0, 1.0, -1.0, 0.0, 200.4, 0.0); // 90 degree rotation plus an odd translation
    let snap = try_snap_pixel_exact(rotate, rect(0.0, 0.0, 40.0, 30.0), rect(0.0, 0.0, 40.0, 30.0));

    assert!(snap.exact, "a 90 degree rotation of a 1:1 draw is exact");
    let (cx, cy) = rotate.transform_point(snap.snapped.x as f32, snap.snapped.y as f32);
    assert_close(
        f64::from(cx),
        f64::from(cx.round()),
        3,
        "corner x lands on a device pixel",
    );
    assert_close(
        f64::from(cy),
        f64::from(cy.round()),
        3,
        "corner y lands on a device pixel",
    );
}

#[test]
fn try_snap_pixel_exact_scaled_draw_is_not_exact() {
    // C#: RenderGeometryTests.TrySnapPixelExact_ScaledDraw_IsNotExact (Theory: 101x50 and 100x49)
    for (width, height) in [(101.0, 50.0), (100.0, 49.0)] {
        let snap = try_snap_pixel_exact(
            Matrix3x2::IDENTITY,
            rect(0.0, 0.0, width, height),
            rect(0.0, 0.0, 100.0, 50.0),
        );

        assert!(!snap.exact, "scaled {width}x{height} is not exact");
        assert_eq!(
            rect(0.0, 0.0, width, height),
            snap.snapped,
            "snapped equals the destination"
        );
    }
}

#[test]
fn try_snap_pixel_exact_rotated45_is_not_exact() {
    // C#: RenderGeometryTests.TrySnapPixelExact_Rotated45_IsNotExact
    let rotate = Matrix3x2::rotation(std::f32::consts::FRAC_PI_4);

    let snap = try_snap_pixel_exact(rotate, rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 0.0, 10.0, 10.0));
    assert!(!snap.exact, "a 45 degree rotation is never exact");
}

#[test]
fn snap_negative_destination_is_not_exact() {
    // new: trap T8. A flipped (negative width) destination is not drawn exactly and is not "fixed" with abs.
    let snap = try_snap_pixel_exact(
        Matrix3x2::IDENTITY,
        rect(10.0, 4.0, -100.0, 50.0),
        rect(0.0, 0.0, 100.0, 50.0),
    );

    assert!(!snap.exact);
    assert_eq!(rect(10.0, 4.0, -100.0, 50.0), snap.snapped);
}

// ---------------------------------------------------------------------------------------------------------------------
// Colours, interpolation, EXIF orientation
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn render_colors_dark_canvas_is_101010() {
    // C#: RenderGeometryTests.RenderColors_DarkCanvas_Is101010
    let expected = ColorF {
        r: 16.0 / 255.0,
        g: 16.0 / 255.0,
        b: 16.0 / 255.0,
        a: 1.0,
    };

    assert_eq!(expected, dark_canvas());
}

#[test]
fn dark_canvas_is_opaque_and_matches_color_from_rgb() {
    // new: DarkCanvas is color_from_rgb(0x10, 0x10, 0x10, 0xFF), and the alpha is exactly 1.
    let canvas = dark_canvas();

    assert_eq!(color_from_rgb(0x10, 0x10, 0x10, 0xFF), canvas);
    assert_eq!(1.0_f32, canvas.a);
}

#[test]
fn image_interpolations_from_scaling_quality_matches_wpf_converter() {
    // C#: RenderGeometryTests.ImageInterpolations_FromScalingQuality_MatchesWpfConverter
    assert_eq!(
        ImageInterpolation::Linear,
        image_interpolation_from_scaling_quality(ScalingQuality::Linear)
    );
    assert_eq!(
        ImageInterpolation::HighQualityCubic,
        image_interpolation_from_scaling_quality(ScalingQuality::HighQuality)
    );
}

#[test]
fn exif_orientation_matrix_maps_pixel_centers_like_pixel_ops() {
    // C#: RenderGeometryTests.ExifOrientationMatrix_MapsPixelCentersLikePixelOps (Theory: orientations 1..8)
    // PixelOps.ApplyOrientation has no Rust counterpart (documented skip). The explicit pixel table of contract
    // note 5 replaces it: raw pixel (u, v) of a W x H raw image lands at the position `expected_position` in the
    // oriented image. The destination sits at (100, 200) to check the translation too.
    const W: i32 = 5;
    const H: i32 = 3;

    fn expected_position(orientation: i32, u: i32, v: i32, w: i32, h: i32) -> (i32, i32) {
        match orientation {
            2 => (w - 1 - u, v),
            3 => (w - 1 - u, h - 1 - v),
            4 => (u, h - 1 - v),
            5 => (v, u),
            6 => (h - 1 - v, u),
            7 => (h - 1 - v, w - 1 - u),
            8 => (v, w - 1 - u),
            _ => (u, v), // 1
        }
    }

    for orientation in 1..=8 {
        let ctx = format!("orientation {orientation}");
        let (oriented_w, oriented_h) = if is_transposed(orientation) { (H, W) } else { (W, H) };
        let destination = rect(100.0, 200.0, f64::from(oriented_w), f64::from(oriented_h));
        let m = for_exif_orientation(orientation, destination);
        let raw = unoriented_rect(orientation, destination);
        assert_eq!(
            (f64::from(W), f64::from(H)),
            (raw.width, raw.height),
            "{ctx}: unoriented rect is the raw size"
        );

        let mut seen = vec![false; (oriented_w * oriented_h) as usize];
        for v in 0..H {
            for u in 0..W {
                let (px, py) = m.transform_point(u as f32 + 0.5, v as f32 + 0.5);
                let x = (px - 100.0).floor() as i32;
                let y = (py - 200.0).floor() as i32;
                assert_eq!(
                    expected_position(orientation, u, v, W, H),
                    (x, y),
                    "{ctx}: raw pixel ({u}, {v}) lands on the wrong oriented pixel"
                );
                seen[(y * oriented_w + x) as usize] = true;
            }
        }
        assert!(
            seen.iter().all(|&s| s),
            "{ctx}: the mapping covers every oriented pixel once"
        );
    }
}

#[test]
fn compose_applies_left_then_right() {
    // new: trap T9. `a.compose(b)` transforms the point by `a` first, then by `b` (C# `a * b`).
    let scale_then_move = Matrix3x2::scale(2.0).compose(Matrix3x2::translation(10.0, 5.0));
    assert_eq!((12.0, 7.0), scale_then_move.transform_point(1.0, 1.0));

    let move_then_scale = Matrix3x2::translation(10.0, 5.0).compose(Matrix3x2::scale(2.0));
    assert_eq!((22.0, 12.0), move_then_scale.transform_point(1.0, 1.0));
}

#[test]
fn transform_point_matches_row_vector_convention() {
    // new: x' = x*m11 + y*m21 + m31, y' = x*m12 + y*m22 + m32 (contract "Public API", T10).
    let m = Matrix3x2::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);

    assert_eq!((36.0, 52.0), m.transform_point(7.0, 8.0));
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/render-geometry.json (133 cases, all kinds)
// ---------------------------------------------------------------------------------------------------------------------

fn parse_quality(text: &str) -> ScalingQuality {
    match text {
        "Linear" => ScalingQuality::Linear,
        "HighQuality" => ScalingQuality::HighQuality,
        other => panic!("unknown ScalingQuality {other:?}"),
    }
}

#[test]
fn golden_render_geometry() {
    // C#: tools/oracle-cs/Commands/W8-01c.cs `render-geometry`. One dispatch per `kind`.
    let golden = golden::load("render-geometry");
    let mut kinds_seen = std::collections::BTreeSet::new();

    for (index, case) in golden.cases.iter().enumerate() {
        let kind = kind_name(case).to_owned();
        let ctx = format!("case {index} ({kind}): {case}");
        kinds_seen.insert(kind.clone());

        match kind.as_str() {
            "dark_canvas" => {
                let c = dark_canvas();
                let want = case["color"].as_array().expect("color");
                let got = [c.r, c.g, c.b, c.a];
                for i in 0..4 {
                    assert!(
                        same_f32(got[i], expected_f32(want[i].as_str().unwrap())),
                        "{ctx}: channel {i}"
                    );
                }
            }
            "color_from_rgb" => {
                let byte = |key: &str| case[key].as_u64().expect(key) as u8;
                let c = color_from_rgb(byte("r"), byte("g"), byte("b"), byte("a"));
                let want = case["color"].as_array().expect("color");
                let got = [c.r, c.g, c.b, c.a];
                for i in 0..4 {
                    assert!(
                        same_f32(got[i], expected_f32(want[i].as_str().unwrap())),
                        "{ctx}: channel {i}"
                    );
                }
            }
            "interpolation" => {
                let quality = parse_quality(case["quality"].as_str().expect("quality"));
                let got = image_interpolation_from_scaling_quality(quality);
                assert_eq!(
                    case["interpolation"].as_str().expect("interpolation"),
                    format!("{got:?}"),
                    "{ctx}"
                );
            }
            "transposed" => {
                let orientation = case["orientation"].as_i64().expect("orientation") as i32;
                assert_eq!(
                    case["transposed"].as_bool().expect("transposed"),
                    is_transposed(orientation),
                    "{ctx}"
                );
            }
            "unoriented_rect" => {
                let orientation = case["orientation"].as_i64().expect("orientation") as i32;
                let got = unoriented_rect(orientation, rect_array(&case["destination"]));
                assert_rect_bits(got, &case["rect"], &ctx);
            }
            "exif_matrix" => {
                let orientation = case["orientation"].as_i64().expect("orientation") as i32;
                let m = for_exif_orientation(orientation, rect_array(&case["destination"]));
                let got = [m.m11, m.m12, m.m21, m.m22, m.m31, m.m32];
                let want = case["matrix"].as_array().expect("matrix");
                for i in 0..6 {
                    assert!(
                        same_f32(got[i], expected_f32(want[i].as_str().unwrap())),
                        "{ctx}: component {i} got {}",
                        got[i]
                    );
                }
            }
            "tile_layout" => {
                let width = case["width"].as_i64().expect("width") as i32;
                let height = case["height"].as_i64().expect("height") as i32;
                let max = case["max"].as_i64().expect("max") as i32;
                let gutter = case["gutter"].as_i64().expect("gutter") as i32;
                let result = compute_tiles(width, height, max, gutter);

                match case["threw"].as_str() {
                    Some(_) => {
                        let error = result.expect_err(&ctx);
                        let param = case["param"].as_str().expect("param");
                        let named = matches!(
                            (param, error),
                            ("width", TileLayoutError::Width(_))
                                | ("height", TileLayoutError::Height(_))
                                | ("maxTextureSize", TileLayoutError::MaxTextureSize(_))
                                | ("gutter", TileLayoutError::Gutter(_))
                        );
                        assert!(named, "{ctx}: wrong error variant {error:?}");
                    }
                    None => {
                        let tiles = result.unwrap_or_else(|e| panic!("{ctx}: unexpected {e:?}"));
                        let want: Vec<TileSpec> = case["tiles"]
                            .as_array()
                            .expect("tiles")
                            .iter()
                            .map(|t| TileSpec {
                                texture: pixel_array(&t["texture"]),
                                core: pixel_array(&t["core"]),
                            })
                            .collect();
                        assert_eq!(want, tiles, "{ctx}");
                    }
                }
            }
            "intersect" => {
                let got = intersect(rect_array(&case["a"]), rect_array(&case["b"]));
                assert_rect_bits(got, &case["rect"], &ctx);
            }
            "is_empty" => {
                let empty = rect_is_empty(rect_array(&case["rect"]));
                assert_eq!(case["empty"].as_bool().expect("empty"), empty, "{ctx}");
            }
            "pixel_to_rect" => {
                let got = pixel_rect_to_rect_d(pixel_array(&case["pixel"]));
                assert_rect_bits(got, &case["rect"], &ctx);
            }
            "map_to_destination" => {
                let got = map_to_destination(
                    rect_array(&case["source"]),
                    rect_array(&case["destination"]),
                    rect_array(&case["part"]),
                );
                assert_rect_bits(got, &case["rect"], &ctx);
            }
            "snap" => {
                let device_items = case["device"].as_array().expect("device");
                let d: Vec<f32> = device_items.iter().map(|v| num(v) as f32).collect();
                let device = Matrix3x2::new(d[0], d[1], d[2], d[3], d[4], d[5]);
                let snap = try_snap_pixel_exact(device, rect_array(&case["destination"]), rect_array(&case["source"]));
                assert_eq!(case["exact"].as_bool().expect("exact"), snap.exact, "{ctx}: exact");
                assert_rect_bits(snap.snapped, &case["snapped"], &ctx);
            }
            other => panic!("unknown golden kind {other:?} in case {index}"),
        }
    }

    // Every kind of the oracle is exercised by at least one case.
    let expected_kinds: std::collections::BTreeSet<String> = [
        "dark_canvas",
        "color_from_rgb",
        "interpolation",
        "transposed",
        "unoriented_rect",
        "exif_matrix",
        "tile_layout",
        "intersect",
        "is_empty",
        "pixel_to_rect",
        "map_to_destination",
        "snap",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(expected_kinds, kinds_seen, "golden kinds exercised");
}
