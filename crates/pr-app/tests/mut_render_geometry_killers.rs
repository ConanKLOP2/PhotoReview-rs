//! Mutation killers for `viewport::render_geometry` (task M2, evidence `docs/reviews/M2-MUT-2026-11-04.md`).
//!
//! Hand-written, hand-computed values; no golden file, no production change. Each test names the mutants it is
//! meant to kill. Expected numbers are worked out by hand in the comments, not copied from the implementation.

use pr_app::input::primitives::RectD;
use pr_app::viewport::render_geometry::{
    ColorF, Matrix3x2, PixelRect, TileLayoutError, TileSpec, color_from_rgb, compute_tiles, for_exif_orientation,
    intersect, map_to_destination, pixel_rect_to_rect_d, rect_is_empty, try_snap_pixel_exact, unoriented_rect,
};

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

fn tuple(m: Matrix3x2) -> (f32, f32, f32, f32, f32, f32) {
    (m.m11, m.m12, m.m21, m.m22, m.m31, m.m32)
}

// ---------------------------------------------------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn color_from_rgb_divides_each_channel_by_255_in_its_own_slot() {
    // Four distinct channel values: a swapped slot, `/ -> %` and `/ -> *` all change at least one channel.
    let c = color_from_rgb(51, 102, 153, 204);
    assert_eq!(
        ColorF {
            r: 51.0_f32 / 255.0_f32,
            g: 102.0_f32 / 255.0_f32,
            b: 153.0_f32 / 255.0_f32,
            a: 204.0_f32 / 255.0_f32,
        },
        c
    );
    assert!(
        (c.r - 0.2).abs() < 1e-6 && (c.g - 0.4).abs() < 1e-6 && (c.b - 0.6).abs() < 1e-6 && (c.a - 0.8).abs() < 1e-6
    );
    assert_eq!(
        ColorF {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0
        },
        color_from_rgb(255, 0, 0, 255)
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Matrix3x2 constructors, rotation, transform
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn matrix_constructors_fill_the_documented_slots() {
    assert_eq!(
        (1.0, 2.0, 3.0, 4.0, 5.0, 6.0),
        tuple(Matrix3x2::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0))
    );
    assert_eq!(
        (1.0, 0.0, 0.0, 1.0, 3.0, -4.0),
        tuple(Matrix3x2::translation(3.0, -4.0))
    );
    assert_eq!((2.5, 0.0, 0.0, 2.5, 0.0, 0.0), tuple(Matrix3x2::scale(2.5)));
    assert_eq!((1.0, 0.0, 0.0, 1.0, 0.0, 0.0), tuple(Matrix3x2::IDENTITY));
}

#[test]
fn rotation_quarter_turns_are_exact() {
    use std::f32::consts::{FRAC_PI_2, PI};
    assert_eq!((1.0, 0.0, 0.0, 1.0, 0.0, 0.0), tuple(Matrix3x2::rotation(0.0)));
    assert_eq!((0.0, 1.0, -1.0, 0.0, 0.0, 0.0), tuple(Matrix3x2::rotation(FRAC_PI_2)));
    assert_eq!((-1.0, 0.0, 0.0, -1.0, 0.0, 0.0), tuple(Matrix3x2::rotation(PI)));
    assert_eq!((-1.0, 0.0, 0.0, -1.0, 0.0, 0.0), tuple(Matrix3x2::rotation(-PI)));
    assert_eq!((0.0, -1.0, 1.0, 0.0, 0.0, 0.0), tuple(Matrix3x2::rotation(-FRAC_PI_2)));
    // Three quarter turns and a full turn: the IEEERemainder reduction brings them back to the snapped values.
    assert_eq!(
        (0.0, -1.0, 1.0, 0.0, 0.0, 0.0),
        tuple(Matrix3x2::rotation(3.0 * FRAC_PI_2))
    );
    assert_eq!((1.0, 0.0, 0.0, 1.0, 0.0, 0.0), tuple(Matrix3x2::rotation(2.0 * PI)));
    assert_eq!(
        (0.0, 1.0, -1.0, 0.0, 0.0, 0.0),
        tuple(Matrix3x2::rotation(FRAC_PI_2 + 2.0 * PI))
    );
}

#[test]
fn rotation_snaps_within_a_thousandth_of_a_degree_and_not_beyond() {
    use std::f32::consts::{FRAC_PI_2, PI};
    // epsilon = 0.001 degree = 1.745e-5 rad. 1e-5 is inside, 3e-5 is outside, on both sides of each quarter turn.
    let inside = 1.0e-5_f32;
    let outside = 3.0e-5_f32;
    for sign in [-1.0_f32, 1.0] {
        assert_eq!(
            (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            tuple(Matrix3x2::rotation(sign * inside)),
            "0 inside"
        );
        let m = Matrix3x2::rotation(sign * outside);
        assert!(m.m12 != 0.0, "0 outside, sign {sign}: sin must not be snapped");
        assert!(m.m12.signum() == sign, "sin has the sign of the angle");

        assert_eq!(
            (0.0, 1.0, -1.0, 0.0, 0.0, 0.0),
            tuple(Matrix3x2::rotation(FRAC_PI_2 + sign * inside)),
            "pi/2 inside"
        );
        assert!(
            Matrix3x2::rotation(FRAC_PI_2 + sign * outside).m11 != 0.0,
            "pi/2 outside"
        );

        assert_eq!(
            (-1.0, 0.0, 0.0, -1.0, 0.0, 0.0),
            tuple(Matrix3x2::rotation(PI - inside)),
            "pi inside (below)"
        );
        assert_eq!(
            (-1.0, 0.0, 0.0, -1.0, 0.0, 0.0),
            tuple(Matrix3x2::rotation(-PI + inside)),
            "-pi inside (above)"
        );
        assert!(Matrix3x2::rotation(PI - outside).m12 != 0.0, "pi outside (below)");
        assert!(Matrix3x2::rotation(-PI + outside).m12 != 0.0, "-pi outside (above)");

        assert_eq!(
            (0.0, -1.0, 1.0, 0.0, 0.0, 0.0),
            tuple(Matrix3x2::rotation(-FRAC_PI_2 + sign * inside)),
            "-pi/2 inside"
        );
        assert!(
            Matrix3x2::rotation(-FRAC_PI_2 + sign * outside).m11 != 0.0,
            "-pi/2 outside"
        );
    }
    // The reduction is part of the snap: a full turn plus 1e-5 still snaps to the identity.
    assert_eq!(
        (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        tuple(Matrix3x2::rotation(2.0 * PI + inside))
    );
}

#[test]
fn rotation_general_angle_is_cos_sin_minus_sin_cos() {
    // 0.5 rad is far from every snap; row-vector layout (cos, sin, -sin, cos).
    let (sin, cos) = 0.5_f32.sin_cos();
    assert_eq!((cos, sin, -sin, cos, 0.0, 0.0), tuple(Matrix3x2::rotation(0.5)));
    // 7.0 rad reduces by one turn (round(7 / 2pi) = 1) to 0.7168...; 4.0 rad reduces to -2.2832.
    let (s7, c7) = 7.0_f32.sin_cos();
    let m7 = Matrix3x2::rotation(7.0);
    assert!((m7.m11 - c7).abs() < 1e-5 && (m7.m12 - s7).abs() < 1e-5, "{m7:?}");
    let (s4, c4) = 4.0_f32.sin_cos();
    let m4 = Matrix3x2::rotation(4.0);
    assert!((m4.m11 - c4).abs() < 1e-5 && (m4.m12 - s4).abs() < 1e-5, "{m4:?}");
    // 20 rad needs 3 turns: n = round(3.18) = 3.
    let (s20, c20) = 20.0_f32.sin_cos();
    let m20 = Matrix3x2::rotation(20.0);
    assert!((m20.m11 - c20).abs() < 1e-4 && (m20.m12 - s20).abs() < 1e-4, "{m20:?}");
}

#[test]
fn compose_translation_adds_its_offset_and_keeps_the_linear_part() {
    let a = Matrix3x2::new(2.0, 3.0, 5.0, 7.0, 11.0, 13.0);
    let c = a.compose(Matrix3x2::translation(100.0, 200.0));
    assert_eq!((2.0, 3.0, 5.0, 7.0, 111.0, 213.0), tuple(c));
    // And with a pure scale on the right the offset is scaled too: (11, 13) * 2 = (22, 26).
    let c = a.compose(Matrix3x2::scale(2.0));
    assert_eq!((4.0, 6.0, 10.0, 14.0, 22.0, 26.0), tuple(c));
}

// ---------------------------------------------------------------------------------------------------------------------
// EXIF orientation
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn unoriented_rect_swaps_only_for_transposed_orientations() {
    let destination = rect(7.0, 9.0, 30.0, 20.0);
    for orientation in [0, 1, 2, 3, 4, 9, -1] {
        assert_eq!(
            rect(0.0, 0.0, 30.0, 20.0),
            unoriented_rect(orientation, destination),
            "{orientation}"
        );
    }
    for orientation in 5..=8 {
        assert_eq!(
            rect(0.0, 0.0, 20.0, 30.0),
            unoriented_rect(orientation, destination),
            "{orientation}"
        );
    }
}

#[test]
fn for_exif_orientation_exact_matrices_for_a_non_square_destination() {
    // destination (10, 20, 30, 40). Orientations 1..4: raw is 30 x 40 (w = 30, h = 40). 5..8: raw is 40 x 30 (w = 40, h = 30).
    // The matrix is orient.compose(translation(10, 20)): the linear part is unchanged, the offset gains (10, 20).
    let d = rect(10.0, 20.0, 30.0, 40.0);
    let expect = [
        (1, (1.0, 0.0, 0.0, 1.0, 10.0, 20.0)),
        (2, (-1.0, 0.0, 0.0, 1.0, 40.0, 20.0)),  // w + 10, 0 + 20
        (3, (-1.0, 0.0, 0.0, -1.0, 40.0, 60.0)), // w + 10, h + 20
        (4, (1.0, 0.0, 0.0, -1.0, 10.0, 60.0)),  // 0 + 10, h + 20
        (5, (0.0, 1.0, 1.0, 0.0, 10.0, 20.0)),
        (6, (0.0, 1.0, -1.0, 0.0, 40.0, 20.0)),  // h(30) + 10, 0 + 20
        (7, (0.0, -1.0, -1.0, 0.0, 40.0, 60.0)), // h(30) + 10, w(40) + 20
        (8, (0.0, -1.0, 1.0, 0.0, 10.0, 60.0)),  // 0 + 10, w(40) + 20
        (0, (1.0, 0.0, 0.0, 1.0, 10.0, 20.0)),
        (9, (1.0, 0.0, 0.0, 1.0, 10.0, 20.0)),
        (-3, (1.0, 0.0, 0.0, 1.0, 10.0, 20.0)),
    ];
    for (orientation, want) in expect {
        assert_eq!(
            want,
            tuple(for_exif_orientation(orientation, d)),
            "orientation {orientation}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// PixelRect
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn pixel_rect_new_keeps_field_order_and_edges_wrap() {
    let p = PixelRect::new(1, 2, 3, 4);
    assert_eq!((1, 2, 3, 4), (p.x, p.y, p.width, p.height));
    assert_eq!(4, p.right());
    assert_eq!(6, p.bottom());
    assert_eq!(-2, PixelRect::new(-5, -7, 3, 4).right());
    assert_eq!(-3, PixelRect::new(-5, -7, 3, 4).bottom());
    // `X + Width` is C# unchecked int addition: it wraps.
    assert_eq!(i32::MIN, PixelRect::new(i32::MAX, i32::MAX, 1, 1).right());
    assert_eq!(i32::MIN, PixelRect::new(i32::MAX, i32::MAX, 1, 1).bottom());
}

#[test]
fn pixel_rect_is_empty_when_either_side_is_not_positive() {
    assert!(!PixelRect::new(0, 0, 1, 1).is_empty());
    assert!(PixelRect::new(0, 0, 0, 1).is_empty());
    assert!(PixelRect::new(0, 0, 1, 0).is_empty());
    assert!(PixelRect::new(0, 0, -1, 1).is_empty());
    assert!(PixelRect::new(0, 0, 1, -1).is_empty());
    assert!(PixelRect::new(0, 0, 0, 0).is_empty());
    assert!(PixelRect::new(0, 0, -4, -4).is_empty());
}

#[test]
fn pixel_rect_to_rect_d_maps_each_field() {
    assert_eq!(
        rect(-1.0, 2.0, 3.0, 4.0),
        pixel_rect_to_rect_d(PixelRect::new(-1, 2, 3, 4))
    );
    assert_eq!(
        rect(f64::from(i32::MAX), f64::from(i32::MIN), 1.0, 2.0),
        pixel_rect_to_rect_d(PixelRect::new(i32::MAX, i32::MIN, 1, 2))
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Tile layout
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn compute_tiles_validation_boundaries_and_order() {
    // Smallest valid values pass.
    assert!(compute_tiles(1, 1, 4, 0).is_ok());
    assert_eq!(
        Err(TileLayoutError::Width(-5)),
        compute_tiles(-5, 1, 16384, 64).map(|_| ())
    );
    assert_eq!(
        Err(TileLayoutError::Height(-5)),
        compute_tiles(1, -5, 16384, 64).map(|_| ())
    );
    assert_eq!(
        Err(TileLayoutError::MaxTextureSize(3)),
        compute_tiles(1, 1, 3, 0).map(|_| ())
    );
    assert_eq!(
        Err(TileLayoutError::MaxTextureSize(-8)),
        compute_tiles(1, 1, -8, 0).map(|_| ())
    );
    assert_eq!(Err(TileLayoutError::Gutter(-1)), compute_tiles(1, 1, 4, -1).map(|_| ()));
    // The first failing parameter wins, in C# order: width, height, maxTextureSize, gutter.
    assert_eq!(Err(TileLayoutError::Width(0)), compute_tiles(0, 0, 0, -1).map(|_| ()));
    assert_eq!(Err(TileLayoutError::Height(0)), compute_tiles(1, 0, 0, -1).map(|_| ()));
    assert_eq!(
        Err(TileLayoutError::MaxTextureSize(0)),
        compute_tiles(1, 1, 0, -1).map(|_| ())
    );
}

#[test]
fn tile_layout_error_display_texts() {
    assert_eq!(
        "width must be at least 1 (got 0)",
        TileLayoutError::Width(0).to_string()
    );
    assert_eq!(
        "height must be at least 1 (got -2)",
        TileLayoutError::Height(-2).to_string()
    );
    assert_eq!(
        "maxTextureSize must be at least 4 (got 3)",
        TileLayoutError::MaxTextureSize(3).to_string()
    );
    assert_eq!(
        "gutter must not be negative (got -1)",
        TileLayoutError::Gutter(-1).to_string()
    );
    let boxed: Box<dyn std::error::Error> = Box::new(TileLayoutError::Width(0));
    assert_eq!("width must be at least 1 (got 0)", boxed.to_string());
}

#[test]
fn compute_tiles_one_pixel_over_the_limit_splits_in_two() {
    // max 256, gutter 32 -> core 192. 257 px: count = ceil(257 / 192) = 2.
    // tile 0: core 0..192, texture 0..224. tile 1: core 192..257 (65), texture 160..257 (97).
    let tiles = compute_tiles(257, 1, 256, 32).unwrap();
    assert_eq!(
        vec![
            TileSpec {
                texture: PixelRect::new(0, 0, 224, 1),
                core: PixelRect::new(0, 0, 192, 1)
            },
            TileSpec {
                texture: PixelRect::new(160, 0, 97, 1),
                core: PixelRect::new(192, 0, 65, 1)
            },
        ],
        tiles
    );
    // Exactly at the limit: one tile without gutter.
    let tiles = compute_tiles(256, 256, 256, 32).unwrap();
    assert_eq!(1, tiles.len());
    assert_eq!(PixelRect::new(0, 0, 256, 256), tiles[0].texture);
    assert_eq!(tiles[0].texture, tiles[0].core);
}

#[test]
fn compute_tiles_four_columns_with_gutters_on_both_sides() {
    // max 256, gutter 32, core 192, 600 px: count = ceil(600 / 192) = 4.
    // i=0 core 0..192 tex 0..224 | i=1 core 192..384 tex 160..416 | i=2 core 384..576 tex 352..600 (clamped) | i=3 core 576..600 tex 544..600.
    let tiles = compute_tiles(600, 1, 256, 32).unwrap();
    let got: Vec<((i32, i32), (i32, i32))> = tiles
        .iter()
        .map(|t| ((t.core.x, t.core.width), (t.texture.x, t.texture.width)))
        .collect();
    assert_eq!(
        vec![
            ((0, 192), (0, 224)),
            ((192, 192), (160, 256)),
            ((384, 192), (352, 248)),
            ((576, 24), (544, 56)),
        ],
        got
    );
}

#[test]
fn compute_tiles_rows_are_outer_and_axes_are_not_mixed() {
    // 257 wide (2 columns) x 600 tall (4 rows), max 256, gutter 32. Rows outer, columns inner.
    let tiles = compute_tiles(257, 600, 256, 32).unwrap();
    assert_eq!(8, tiles.len());
    // tile 1 = column 1, row 0.
    assert_eq!(
        TileSpec {
            texture: PixelRect::new(160, 0, 97, 224),
            core: PixelRect::new(192, 0, 65, 192)
        },
        tiles[1]
    );
    // tile 2 = column 0, row 1.
    assert_eq!(
        TileSpec {
            texture: PixelRect::new(0, 160, 224, 256),
            core: PixelRect::new(0, 192, 192, 192)
        },
        tiles[2]
    );
    // tile 7 = last column, last row.
    assert_eq!(
        TileSpec {
            texture: PixelRect::new(160, 544, 97, 56),
            core: PixelRect::new(192, 576, 65, 24)
        },
        tiles[7]
    );
}

#[test]
fn compute_tiles_gutter_below_the_clamp_is_used_as_given() {
    // gutter 10 < 256 / 4: kept. core = 256 - 20 = 236. 300 px: 2 tiles. tile 1: core 236..300, texture 226..300.
    let tiles = compute_tiles(300, 1, 256, 10).unwrap();
    assert_eq!(2, tiles.len());
    assert_eq!(PixelRect::new(0, 0, 246, 1), tiles[0].texture);
    assert_eq!(PixelRect::new(0, 0, 236, 1), tiles[0].core);
    assert_eq!(PixelRect::new(226, 0, 74, 1), tiles[1].texture);
    assert_eq!(PixelRect::new(236, 0, 64, 1), tiles[1].core);
    // gutter 0: texture == core everywhere, core = 256.
    let tiles = compute_tiles(600, 1, 256, 0).unwrap();
    assert_eq!(3, tiles.len());
    assert!(tiles.iter().all(|t| t.texture == t.core));
    assert_eq!(PixelRect::new(512, 0, 88, 1), tiles[2].core);
}

#[test]
fn compute_tiles_allocates_exactly_the_tile_count() {
    // 3 columns x 2 rows = 6 tiles; `columns.len() * rows.len()` is the reserved capacity ("allocates only the result vector").
    let tiles = compute_tiles(500, 300, 256, 32).unwrap();
    assert_eq!(3 * 2, tiles.len());
    assert_eq!(tiles.len(), tiles.capacity());
}

#[test]
fn compute_tiles_near_i32_max_does_not_overflow() {
    // D-W8c-6: the ceiling is computed in i64. max 16384, gutter 64 -> core 16256; ceil(2147483647 / 16256) = 132105.
    let tiles = compute_tiles(i32::MAX, 1, 16384, 64).unwrap();
    assert_eq!(132_105, tiles.len());
    let last = tiles.last().unwrap();
    assert_eq!(i32::MAX, last.core.right());
    assert_eq!(i32::MAX, last.texture.right());
    assert_eq!(132_104 * 16_256, last.core.x);
    // Only the last core is short; all others are 16256 wide and the textures never exceed the limit.
    assert!(tiles[..tiles.len() - 1].iter().all(|t| t.core.width == 16_256));
    assert!(tiles.iter().all(|t| t.texture.width <= 16_384));
}

// ---------------------------------------------------------------------------------------------------------------------
// Rect helpers
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn intersect_overlap_uses_each_edge_of_the_right_rect() {
    // a = (1,2)-(11,22); b = (4,8)-(7,13): b is inside a, so the result is b.
    assert_eq!(
        rect(4.0, 8.0, 3.0, 5.0),
        intersect(rect(1.0, 2.0, 10.0, 20.0), rect(4.0, 8.0, 3.0, 5.0))
    );
    assert_eq!(
        rect(4.0, 8.0, 3.0, 5.0),
        intersect(rect(4.0, 8.0, 3.0, 5.0), rect(1.0, 2.0, 10.0, 20.0))
    );
    // Partial overlaps that pick the left/top edge of one rect and the right/bottom edge of the other, with negatives.
    assert_eq!(
        rect(-2.0, -3.0, 3.0, 6.0),
        intersect(rect(-5.0, -9.0, 6.0, 12.0), rect(-2.0, -3.0, 9.0, 9.0))
    );
    assert_eq!(
        rect(-2.0, -3.0, 3.0, 6.0),
        intersect(rect(-2.0, -3.0, 9.0, 9.0), rect(-5.0, -9.0, 6.0, 12.0))
    );
    // The right/bottom edge comes from `b` in x and from `a` in y.
    assert_eq!(
        rect(2.0, 1.0, 3.0, 4.0),
        intersect(rect(0.0, 1.0, 10.0, 4.0), rect(2.0, 0.0, 3.0, 20.0))
    );
}

#[test]
fn intersect_needs_overlap_on_both_axes_and_a_positive_area() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    // x overlaps, y does not (gap, then touching).
    assert_eq!(RectD::default(), intersect(a, rect(2.0, 12.0, 5.0, 5.0)));
    assert_eq!(RectD::default(), intersect(a, rect(2.0, 10.0, 5.0, 5.0)));
    assert_eq!(RectD::default(), intersect(rect(2.0, 10.0, 5.0, 5.0), a));
    // y overlaps, x does not (gap, then touching).
    assert_eq!(RectD::default(), intersect(a, rect(12.0, 2.0, 5.0, 5.0)));
    assert_eq!(RectD::default(), intersect(a, rect(10.0, 2.0, 5.0, 5.0)));
    assert_eq!(RectD::default(), intersect(rect(10.0, 2.0, 5.0, 5.0), a));
    // Left/top side separated too.
    assert_eq!(RectD::default(), intersect(a, rect(-5.0, 2.0, 5.0, 5.0)));
    assert_eq!(RectD::default(), intersect(a, rect(2.0, -5.0, 5.0, 5.0)));
    // Zero-sized rect inside a: area 0, so the default.
    assert_eq!(RectD::default(), intersect(a, rect(3.0, 3.0, 0.0, 4.0)));
    assert_eq!(RectD::default(), intersect(a, rect(3.0, 3.0, 4.0, 0.0)));
    // A sliver of 1 is a real overlap.
    assert_eq!(rect(9.0, 9.0, 1.0, 1.0), intersect(a, rect(9.0, 9.0, 5.0, 5.0)));
}

#[test]
fn intersect_signed_zero_origin_follows_dotnet_max() {
    // Math.Max(+0, -0) = +0 whatever the order (x and y independently).
    for (a, b) in [(0.0_f64, -0.0_f64), (-0.0, 0.0), (-0.0, -0.0)] {
        let out = intersect(rect(a, a, 4.0, 4.0), rect(b, b, 4.0, 4.0));
        let want_positive = a.is_sign_positive() || b.is_sign_positive();
        assert_eq!(out.x.is_sign_positive(), want_positive, "x for {a:?} {b:?}");
        assert_eq!(out.y.is_sign_positive(), want_positive, "y for {a:?} {b:?}");
        assert_eq!((4.0, 4.0), (out.width, out.height));
    }
}

#[test]
fn rect_is_empty_requires_both_extents_positive() {
    assert!(!rect_is_empty(rect(0.0, 0.0, 1.0, 1.0)));
    assert!(!rect_is_empty(rect(-5.0, -5.0, 1e-300, 1e-300)));
    assert!(rect_is_empty(rect(0.0, 0.0, 0.0, 0.0)));
    assert!(rect_is_empty(rect(0.0, 0.0, 0.0, 5.0)));
    assert!(rect_is_empty(rect(0.0, 0.0, 5.0, 0.0)));
    assert!(rect_is_empty(rect(0.0, 0.0, -1.0, 5.0)));
    assert!(rect_is_empty(rect(0.0, 0.0, 5.0, -1.0)));
    assert!(rect_is_empty(rect(0.0, 0.0, f64::NAN, 5.0)));
    assert!(rect_is_empty(rect(0.0, 0.0, 5.0, f64::NAN)));
    assert!(rect_is_empty(rect(0.0, 0.0, -0.0, 5.0)));
}

#[test]
fn map_to_destination_uses_each_origin_and_scale_separately() {
    // source (10, 20, 100, 50) -> destination (1, 2, 50, 100): sx = 0.5, sy = 2.
    // part (30, 45, 20, 10): x = 1 + (30 - 10) * 0.5 = 11; y = 2 + (45 - 20) * 2 = 52; w = 20 * 0.5 = 10; h = 10 * 2 = 20.
    assert_eq!(
        rect(11.0, 52.0, 10.0, 20.0),
        map_to_destination(
            rect(10.0, 20.0, 100.0, 50.0),
            rect(1.0, 2.0, 50.0, 100.0),
            rect(30.0, 45.0, 20.0, 10.0)
        )
    );
    // Part left/above the source origin gives a position left/above the destination origin.
    assert_eq!(
        rect(-4.0, -3.0, 4.0, 3.0),
        map_to_destination(
            rect(2.0, 3.0, 8.0, 6.0),
            rect(0.0, 0.0, 16.0, 12.0),
            rect(0.0, 1.5, 2.0, 1.5)
        )
    );
    // Identity mapping returns the part.
    let source = rect(3.0, 4.0, 7.0, 9.0);
    assert_eq!(
        rect(5.0, 6.0, 1.0, 2.0),
        map_to_destination(source, source, rect(5.0, 6.0, 1.0, 2.0))
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// try_snap_pixel_exact
// ---------------------------------------------------------------------------------------------------------------------

/// Device position of a local point under `m`, all in f64 (each f32 entry widened first).
fn device_of(m: Matrix3x2, x: f64, y: f64) -> (f64, f64) {
    (
        x * f64::from(m.m11) + y * f64::from(m.m21) + f64::from(m.m31),
        x * f64::from(m.m12) + y * f64::from(m.m22) + f64::from(m.m32),
    )
}

/// Property of an exact snap: the snapped corner sits on the device pixel grid, at most 0.5 device px from the
/// original corner, and the size is unchanged.
fn assert_snap_on_grid(m: Matrix3x2, destination: RectD, source: RectD, ctx: &str) {
    let snap = try_snap_pixel_exact(m, destination, source);
    assert!(snap.exact, "{ctx}: expected exact");
    assert_eq!(
        (destination.width, destination.height),
        (snap.snapped.width, snap.snapped.height),
        "{ctx}: size"
    );
    let (ox, oy) = device_of(m, destination.x, destination.y);
    let (sx, sy) = device_of(m, snap.snapped.x, snap.snapped.y);
    assert!(
        (sx - sx.round()).abs() < 1e-9,
        "{ctx}: device x {sx} is not on the grid"
    );
    assert!(
        (sy - sy.round()).abs() < 1e-9,
        "{ctx}: device y {sy} is not on the grid"
    );
    assert!(
        (sx - ox).abs() <= 0.5 + 1e-9 && (sy - oy).abs() <= 0.5 + 1e-9,
        "{ctx}: moved too far"
    );
}

#[test]
fn snap_hand_computed_axis_aligned_mirror() {
    // device (-1,0,0,1, 10.25, 20.75), destination corner (3, 4), 1:1 draw.
    // corner = (-3 + 10.25, 4 + 20.75) = (7.25, 24.75); dx = 7 - 7.25 = -0.25, dy = 25 - 24.75 = 0.25; det = -1.
    // lx = (dx*m22 - dy*m21)/det = (-0.25)/-1 = 0.25; ly = (dy*m11 - dx*m12)/det = (-0.25)/-1 = 0.25 -> (3.25, 4.25).
    let m = Matrix3x2::new(-1.0, 0.0, 0.0, 1.0, 10.25, 20.75);
    let snap = try_snap_pixel_exact(m, rect(3.0, 4.0, 8.0, 6.0), rect(0.0, 0.0, 8.0, 6.0));
    assert!(snap.exact);
    assert_eq!(rect(3.25, 4.25, 8.0, 6.0), snap.snapped);
}

#[test]
fn snap_hand_computed_swapped_transpose() {
    // device (0,1,1,0, 0.3f, 0.6f), destination corner (2, 3): device x = y + m31, device y = x + m32.
    // The corner lands on (3 + m31, 2 + m32); it moves so that device x = round(.) and device y = round(.):
    // x' = round(2 + m32) - m32 = 3 - m32 ... worked through the 2x2 inverse in the comment of the module: x' = 3 - m32, y' = 3 - m31.
    let m = Matrix3x2::new(0.0, 1.0, 1.0, 0.0, 0.3, 0.6);
    let snap = try_snap_pixel_exact(m, rect(2.0, 3.0, 10.0, 10.0), rect(0.0, 0.0, 10.0, 10.0));
    assert!(snap.exact);
    let m31 = f64::from(0.3_f32);
    let m32 = f64::from(0.6_f32);
    assert!((snap.snapped.x - (3.0 - m32)).abs() < 1e-9, "{:?}", snap.snapped);
    assert!((snap.snapped.y - (3.0 - m31)).abs() < 1e-9, "{:?}", snap.snapped);
}

#[test]
fn snap_hand_computed_rotation_quarter_turn() {
    // device (0,1,-1,0, 200.4f, 0): corner (5, 7) -> (7*-1 + 200.4f, 5) = (193.4 - ..., 5). dx = 193 - cx, dy = 0.
    // det = 1; lx = 0; ly = -dx*m12 = cx - 193. So (5, 7 + cx - 193) with cx = 200.4f - 7.
    let m = Matrix3x2::new(0.0, 1.0, -1.0, 0.0, 200.4, 0.0);
    let snap = try_snap_pixel_exact(m, rect(5.0, 7.0, 40.0, 30.0), rect(0.0, 0.0, 40.0, 30.0));
    assert!(snap.exact);
    let cx = f64::from(200.4_f32) - 7.0;
    assert!((snap.snapped.x - 5.0).abs() < 1e-9, "{:?}", snap.snapped);
    assert!((snap.snapped.y - (7.0 + cx - 193.0)).abs() < 1e-9, "{:?}", snap.snapped);
}

#[test]
fn snap_uses_ties_to_even() {
    // Identity device: corner 10.5 -> 10, 11.5 -> 12, 0.5 -> 0, 2.5 -> 2, -0.5 -> -0, -1.5 -> -2.
    for (input, want) in [
        (10.5, 10.0),
        (11.5, 12.0),
        (0.5, 0.0),
        (2.5, 2.0),
        (-1.5, -2.0),
        (3.5, 4.0),
    ] {
        let snap = try_snap_pixel_exact(
            Matrix3x2::IDENTITY,
            rect(input, input, 4.0, 4.0),
            rect(0.0, 0.0, 4.0, 4.0),
        );
        assert!(snap.exact);
        assert_eq!(want, snap.snapped.x, "x {input}");
        assert_eq!(want, snap.snapped.y, "y {input}");
    }
}

#[test]
fn snap_offsets_below_epsilon_leave_the_destination_untouched() {
    // |dx| = 1e-7 < 1e-6: counts as already on the grid, destination returned as is (exact).
    let near = rect(10.0000001, 5.0000001, 4.0, 4.0);
    let snap = try_snap_pixel_exact(Matrix3x2::IDENTITY, near, rect(0.0, 0.0, 4.0, 4.0));
    assert!(snap.exact);
    assert_eq!(near, snap.snapped);
    // 1e-5 > epsilon: snapped to the grid.
    let snap = try_snap_pixel_exact(
        Matrix3x2::IDENTITY,
        rect(10.00001, 5.0, 4.0, 4.0),
        rect(0.0, 0.0, 4.0, 4.0),
    );
    assert!(snap.exact);
    assert_eq!(10.0, snap.snapped.x);
    assert_eq!(5.0, snap.snapped.y);
    let snap = try_snap_pixel_exact(
        Matrix3x2::IDENTITY,
        rect(10.0, 4.99999, 4.0, 4.0),
        rect(0.0, 0.0, 4.0, 4.0),
    );
    assert!(snap.exact);
    assert_eq!(10.0, snap.snapped.x);
    assert_eq!(5.0, snap.snapped.y);
}

#[test]
fn snap_empty_source_or_destination_is_not_exact() {
    let good = rect(0.0, 0.0, 10.0, 10.0);
    for bad in [
        rect(0.0, 0.0, 0.0, 10.0),
        rect(0.0, 0.0, 10.0, 0.0),
        rect(0.0, 0.0, -10.0, 10.0),
        rect(0.0, 0.0, 10.0, -10.0),
    ] {
        let s = try_snap_pixel_exact(Matrix3x2::IDENTITY, good, bad);
        assert!(!s.exact, "empty source {bad:?}");
        assert_eq!(good, s.snapped);
        let dest = rect(0.3, 0.3, bad.width, bad.height);
        let s = try_snap_pixel_exact(Matrix3x2::IDENTITY, dest, good);
        assert!(!s.exact, "empty destination {bad:?}");
        assert_eq!(dest, s.snapped);
    }
}

#[test]
fn snap_each_linear_entry_must_be_right_for_exactness() {
    let r = rect(0.3, 0.3, 10.0, 10.0);
    let s = rect(0.0, 0.0, 10.0, 10.0);
    let not_exact = [
        // axis-aligned family with exactly one entry wrong
        (1.0, 0.5, 0.0, 1.0),
        (1.0, 0.0, 0.5, 1.0),
        (2.0, 0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0, 2.0),
        (0.5, 0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0, 0.5),
        (1.0, 0.00001, 0.0, 1.0),
        (1.0, 0.0, 0.00001, 1.0),
        (1.00001, 0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0, 1.00001),
        // swapped family with exactly one entry wrong
        (0.5, 1.0, 1.0, 0.0),
        (0.0, 1.0, 1.0, 0.5),
        (0.0, 2.0, 1.0, 0.0),
        (0.0, 1.0, 2.0, 0.0),
        (0.0, 0.5, 1.0, 0.0),
        (0.0, 1.0, 0.5, 0.0),
        (0.00001, 1.0, 1.0, 0.0),
        (0.0, 1.0, 1.0, 0.00001),
        (0.0, 1.00001, 1.0, 0.0),
        (0.0, 1.0, 1.00001, 0.0),
        // neither family
        (0.0, 0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0, 1.0),
    ];
    for (m11, m12, m21, m22) in not_exact {
        let m = Matrix3x2::new(m11, m12, m21, m22, 0.0, 0.0);
        let snap = try_snap_pixel_exact(m, r, s);
        assert!(!snap.exact, "({m11}, {m12}, {m21}, {m22}) must not be exact");
        assert_eq!(r, snap.snapped);
    }
}

#[test]
fn snap_entries_within_epsilon_still_count_as_exact() {
    // 1e-7 is below the 1e-6 tolerance for "zero" and for "one": still exact, and the snap lands on the grid.
    let r = rect(0.3, 0.7, 10.0, 10.0);
    let s = rect(0.0, 0.0, 10.0, 10.0);
    let m = Matrix3x2::new(1.0000001, 1e-7, -1e-7, 0.9999999, 0.25, 0.25);
    assert_snap_on_grid(m, r, s, "axis within epsilon");
    let m = Matrix3x2::new(1e-7, 1.0000001, 0.9999999, -1e-7, 0.25, 0.25);
    assert_snap_on_grid(m, r, s, "swapped within epsilon");
}

#[test]
fn snap_zero_test_is_strictly_below_epsilon() {
    // b = sx * m12 = 1e-6 * 1.0 = 1e-6 exactly (f64): NOT below epsilon, so not zero, so not exact.
    // a = sx * m11 = 1e-6 * 1e6 = 1; d = 1.
    let m = Matrix3x2::new(1.0e6, 1.0, 0.0, 1.0, 0.0, 0.0);
    let dest = rect(0.5, 0.5, 1.0e-6, 10.0);
    let snap = try_snap_pixel_exact(m, dest, rect(0.0, 0.0, 1.0, 10.0));
    assert!(!snap.exact);
    assert_eq!(dest, snap.snapped);
}

#[test]
fn snap_device_scale_compensated_by_destination_size_is_exact() {
    // destination twice the source width, device scale 0.5 in x: a = 2 * 0.5 = 1. sy: height 1/2, device m22 = 2: d = 0.5 * 2 = 1.
    let m = Matrix3x2::new(0.5, 0.0, 0.0, 2.0, 0.25, 0.25);
    assert_snap_on_grid(m, rect(3.3, 4.4, 20.0, 5.0), rect(0.0, 0.0, 10.0, 10.0), "scaled axis");
    // swapped: sx = 2, m12 = 0.5 -> b = 1; sy = 0.5, m21 = 2 -> c = 1.
    let m = Matrix3x2::new(0.0, 0.5, 2.0, 0.0, 0.25, 0.25);
    assert_snap_on_grid(
        m,
        rect(3.3, 4.4, 20.0, 5.0),
        rect(0.0, 0.0, 10.0, 10.0),
        "scaled swapped",
    );
    // The wrong pairing (sx with m21, sy with m12) must not be accepted: sx = 2, sy = 0.5, device (0, 2, 0.5, 0) -> b = 4, c = 0.25.
    let m = Matrix3x2::new(0.0, 2.0, 0.5, 0.0, 0.0, 0.0);
    assert!(!try_snap_pixel_exact(m, rect(0.3, 0.3, 20.0, 5.0), rect(0.0, 0.0, 10.0, 10.0)).exact);
    // And scaling mismatch the other way for the axis family: device (2, 0, 0, 0.5) with sx = 2, sy = 0.5 -> a = 4.
    let m = Matrix3x2::new(2.0, 0.0, 0.0, 0.5, 0.0, 0.0);
    assert!(!try_snap_pixel_exact(m, rect(0.3, 0.3, 20.0, 5.0), rect(0.0, 0.0, 10.0, 10.0)).exact);
}

#[test]
fn snap_moves_the_corner_onto_the_device_grid_for_every_orientation_and_offset() {
    let s = rect(0.0, 0.0, 16.0, 9.0);
    let d = rect(1234.37, 5678.62, 16.0, 9.0);
    let linear = [
        (1.0, 0.0, 0.0, 1.0),
        (-1.0, 0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0, -1.0),
        (-1.0, 0.0, 0.0, -1.0),
        (0.0, 1.0, 1.0, 0.0),
        (0.0, 1.0, -1.0, 0.0),
        (0.0, -1.0, 1.0, 0.0),
        (0.0, -1.0, -1.0, 0.0),
    ];
    for (i, (m11, m12, m21, m22)) in linear.into_iter().enumerate() {
        for (tx, ty) in [(0.0, 0.0), (0.4, 0.6), (-3.25, 7.75), (100.5, -0.5)] {
            let m = Matrix3x2::new(m11, m12, m21, m22, tx, ty);
            assert_snap_on_grid(m, d, s, &format!("linear #{i} offset ({tx}, {ty})"));
        }
    }
}

#[test]
fn snap_accounts_for_cross_terms_inside_the_epsilon_band() {
    // Cross entries of 4e-7 are "zero" (below 1e-6), but at a destination near 5000 they move the device corner by
    // ~2e-3, far above epsilon: the inverse must use the full 2x2 matrix, and the corner must use all four entries.
    let s = rect(0.0, 0.0, 10.0, 10.0);
    let d = rect(5000.3, 4000.7, 10.0, 10.0);
    let m = Matrix3x2::new(1.0, 4.0e-7, 4.0e-7, 1.0, 0.31, 0.62);
    assert_snap_on_grid(m, d, s, "axis with cross terms");
    let m = Matrix3x2::new(4.0e-7, 1.0, 1.0, 4.0e-7, 0.31, 0.62);
    assert_snap_on_grid(m, d, s, "swapped with cross terms");
    let m = Matrix3x2::new(-4.0e-7, -1.0, 1.0, -4.0e-7, 0.31, 0.62);
    assert_snap_on_grid(m, d, s, "swapped negative with cross terms");
}

#[test]
fn snap_small_device_scale_with_a_compensating_destination_snaps_through_the_inverse() {
    // device scale 0.01 (det = 1e-4, above epsilon), destination 100x the source: a = d = 1. Local units are 100x device units.
    let m = Matrix3x2::new(0.01, 0.0, 0.0, 0.01, 0.4, 0.4);
    assert_snap_on_grid(m, rect(0.3, 0.3, 100.0, 100.0), rect(0.0, 0.0, 1.0, 1.0), "small scale");
}

#[test]
fn snap_singular_linear_part_just_below_epsilon_is_not_exact() {
    // det = 1e-14 (below epsilon) although a = d = 1 through the destination size: not exact, destination returned.
    let m = Matrix3x2::new(1e-7, 0.0, 0.0, 1e-7, 0.4, 0.4);
    let d = rect(0.3, 0.3, 1e7, 1e7);
    let snap = try_snap_pixel_exact(m, d, rect(0.0, 0.0, 1.0, 1.0));
    assert!(!snap.exact);
    assert_eq!(d, snap.snapped);
}

#[test]
fn snap_already_on_grid_returns_the_destination_exactly() {
    // Both offsets zero: the early return gives the destination back (exact). Either offset alone is not enough for it.
    let d = rect(10.0, 5.0, 4.0, 4.0);
    let s = rect(0.0, 0.0, 4.0, 4.0);
    let snap = try_snap_pixel_exact(Matrix3x2::IDENTITY, d, s);
    assert!(snap.exact);
    assert_eq!(d, snap.snapped);
    let snap = try_snap_pixel_exact(Matrix3x2::IDENTITY, rect(10.0, 5.4, 4.0, 4.0), s);
    assert_eq!(rect(10.0, 5.0, 4.0, 4.0), snap.snapped);
    let snap = try_snap_pixel_exact(Matrix3x2::IDENTITY, rect(10.4, 5.0, 4.0, 4.0), s);
    assert_eq!(rect(10.0, 5.0, 4.0, 4.0), snap.snapped);
}

#[test]
fn rotation_snap_window_is_open_at_exactly_epsilon() {
    use std::f32::consts::{FRAC_PI_2, PI};
    // The comparisons are strict: an angle exactly epsilon away from a quarter turn is NOT snapped, so the result
    // keeps the tiny sin/cos term. epsilon is computed with the same f32 expression as the implementation.
    let eps: f32 = 0.001_f32 * PI / 180.0_f32;
    let near_zero_sin = |m: Matrix3x2| m.m12 != 0.0 && m.m21 != 0.0;
    let near_zero_cos = |m: Matrix3x2| m.m11 != 0.0 && m.m22 != 0.0;
    assert!(near_zero_sin(Matrix3x2::rotation(eps)), "+eps around 0");
    assert!(near_zero_sin(Matrix3x2::rotation(-eps)), "-eps around 0");
    assert!(near_zero_cos(Matrix3x2::rotation(FRAC_PI_2 + eps)), "pi/2 + eps");
    assert!(near_zero_cos(Matrix3x2::rotation(FRAC_PI_2 - eps)), "pi/2 - eps");
    assert!(near_zero_sin(Matrix3x2::rotation(PI - eps)), "pi - eps");
    assert!(near_zero_sin(Matrix3x2::rotation(-PI + eps)), "-pi + eps");
    assert!(near_zero_cos(Matrix3x2::rotation(-FRAC_PI_2 + eps)), "-pi/2 + eps");
    assert!(near_zero_cos(Matrix3x2::rotation(-FRAC_PI_2 - eps)), "-pi/2 - eps");
}
