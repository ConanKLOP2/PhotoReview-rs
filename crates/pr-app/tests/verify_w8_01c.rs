use pr_app::viewport::render_geometry::Matrix3x2;

// Kills "replace + with - in compose m11" (and siblings): general matrices, hand-computed (row-vector, a then b).
#[test]
fn compose_general_matrices() {
    let a = Matrix3x2::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Matrix3x2::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let c = a.compose(b);
    assert_eq!(
        (c.m11, c.m12, c.m21, c.m22, c.m31, c.m32),
        (25.0, 28.0, 57.0, 64.0, 100.0, 112.0)
    );
}

// Kills "delete -" in rotation: 90 degrees clockwise (y down): (1,0) -> (0,1), (0,1) -> (-1,0) as System.Numerics.
#[test]
fn rotation_sign() {
    let m = Matrix3x2::rotation(std::f32::consts::FRAC_PI_2);
    let (x, y) = m.transform_point(0.0, 1.0);
    assert!((x + 1.0).abs() < 1e-6 && y.abs() < 1e-6, "{x} {y}");
}

use pr_app::input::primitives::RectD;
use pr_app::viewport::render_geometry::intersect;

fn rr(x: f64, y: f64, w: f64, h: f64) -> RectD {
    RectD {
        x,
        y,
        width: w,
        height: h,
    }
}

// C# Math.Max/Min propagate NaN from either operand, so a NaN origin or extent gives the empty default rect.
#[test]
fn intersect_one_sided_nan_gives_default() {
    let n = f64::NAN;
    let ok = rr(0.0, 0.0, 10.0, 10.0);
    for bad in [
        rr(n, 0.0, 10.0, 10.0),
        rr(0.0, n, 10.0, 10.0),
        rr(0.0, 0.0, n, 10.0),
        rr(0.0, 0.0, 10.0, n),
    ] {
        assert_eq!(intersect(bad, ok), RectD::default(), "a={bad:?}");
        assert_eq!(intersect(ok, bad), RectD::default(), "b={bad:?}");
    }
    // NaN only on the losing side of a plain comparison (a.x < b.x with NaN width on the other rect).
    assert_eq!(
        intersect(rr(5.0, 5.0, 10.0, 10.0), rr(0.0, 0.0, n, n)),
        RectD::default()
    );
    assert_eq!(
        intersect(rr(0.0, 0.0, n, n), rr(5.0, 5.0, 10.0, 10.0)),
        RectD::default()
    );
}

// Kills `&& -> ||` at the early-exit of try_snap_pixel_exact: only dy needs snapping (dx == 0).
#[test]
fn snap_only_y_offset_is_snapped() {
    use pr_app::viewport::render_geometry::try_snap_pixel_exact;
    let s = try_snap_pixel_exact(
        Matrix3x2::IDENTITY,
        rr(10.0, 4.6, 100.0, 50.0),
        rr(0.0, 0.0, 100.0, 50.0),
    );
    assert!(s.exact);
    assert!(
        (s.snapped.y - 5.0).abs() < 1e-9 && (s.snapped.x - 10.0).abs() < 1e-9,
        "{:?}",
        s.snapped
    );
    let s = try_snap_pixel_exact(
        Matrix3x2::IDENTITY,
        rr(10.3, 5.0, 100.0, 50.0),
        rr(0.0, 0.0, 100.0, 50.0),
    );
    assert!(
        (s.snapped.x - 10.0).abs() < 1e-9 && (s.snapped.y - 5.0).abs() < 1e-9,
        "{:?}",
        s.snapped
    );
}

// Kills `> -> >=` in math_max: Math.Max(-0.0, +0.0) = +0.0 in .NET (the other order is in the _zero file).
#[test]
fn intersect_max_negzero_first_gives_poszero() {
    let out = intersect(rr(-0.0, -0.0, 10.0, 10.0), rr(0.0, 0.0, 10.0, 10.0));
    assert_eq!(out.x.to_bits(), 0.0f64.to_bits());
    assert_eq!(out.y.to_bits(), 0.0f64.to_bits());
}

// Kills `- -> /` in split_axis count: exact multiple of the core must not create an empty extra tile.
#[test]
fn tiles_exact_multiple_of_core_has_no_empty_tile() {
    use pr_app::viewport::render_geometry::compute_tiles;
    // max 256, gutter 32 -> core 192; 384 = 2 * 192.
    let t = compute_tiles(384, 1, 256, 32).unwrap();
    assert_eq!(t.len(), 2);
    assert!(t.iter().all(|x| x.core.width > 0));
    let t = compute_tiles(1, 576, 256, 32).unwrap();
    assert_eq!(t.len(), 3);
}

// Kills `< -> ==` on the singular-determinant guard: a near-singular (tiny, nonzero) linear part is "not exact".
#[test]
fn snap_near_singular_determinant_is_not_exact() {
    use pr_app::viewport::render_geometry::try_snap_pixel_exact;
    let m = Matrix3x2::new(1e-7, 0.0, 0.0, 1e-7, 0.4, 0.4);
    let d = rr(0.3, 0.3, 1e7, 1e7);
    let s = try_snap_pixel_exact(m, d, rr(0.0, 0.0, 1.0, 1.0));
    assert!(!s.exact);
    assert_eq!(s.snapped, d);
}
