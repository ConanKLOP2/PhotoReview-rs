use pr_app::viewport::render_geometry::{Matrix3x2, PixelRect};

fn bits(m: Matrix3x2) -> [u32; 4] {
    [m.m11.to_bits(), m.m12.to_bits(), m.m21.to_bits(), m.m22.to_bits()]
}

// Oracle: .NET Matrix3x2.CreateRotation(float) snaps near 0, +-pi/2, pi (epsilon 0.001 deg) and reduces with IEEERemainder.
#[test]
fn rotation_matches_dotnet_snapping() {
    let pi = std::f32::consts::PI;
    let rows: [(f32, [u32; 4]); 9] = [
        (0.0, [0x3f800000, 0, 0x80000000, 0x3f800000]),
        (pi / 2.0, [0, 0x3f800000, 0xbf800000, 0]),
        (pi, [0xbf800000, 0, 0x80000000, 0xbf800000]),
        (-pi / 2.0, [0, 0xbf800000, 0x3f800000, 0]),
        (6.2831855, [0x3f800000, 0, 0x80000000, 0x3f800000]),
        (0.00001, [0x3f800000, 0, 0x80000000, 0x3f800000]),
        (1.0, [0x3f0a5140, 0x3f576aa4, 0xbf576aa4, 0x3f0a5140]),
        (3.0, [0xbf7d7026, 0x3e1081c3, 0xbe1081c3, 0xbf7d7026]),
        (10.0, [0xbf56cd67, 0xbf0b44f3, 0x3f0b44f3, 0xbf56cd67]),
    ];
    for (r, want) in rows {
        assert_eq!(bits(Matrix3x2::rotation(r)), want, "radians={r}");
    }
}

// C# PixelRect.Right is unchecked int addition (wraps); Rust debug build panics.
#[test]
fn pixel_rect_right_wraps_like_unchecked_csharp() {
    let r = PixelRect::new(i32::MAX, i32::MAX, 1, 1);
    assert_eq!(r.right(), i32::MIN);
    assert_eq!(r.bottom(), i32::MIN);
}

// Mutation-gap probe: math_min with both right/bottom edges negative (kills `&& -> ||` and `== -> !=` in math_min).
#[test]
fn intersect_negative_edges_take_the_smaller_right_and_bottom() {
    use pr_app::input::primitives::RectD;
    use pr_app::viewport::render_geometry::intersect;
    let r = |x, y, w, h| RectD {
        x,
        y,
        width: w,
        height: h,
    };
    // a.right = -1, b.right = -5; a.bottom = -2, b.bottom = -6; x = -10, y = -20.
    let out = intersect(r(-10.0, -20.0, 9.0, 18.0), r(-20.0, -30.0, 15.0, 24.0));
    assert_eq!(out, r(-10.0, -20.0, 5.0, 14.0));
    let out = intersect(r(-20.0, -30.0, 15.0, 24.0), r(-10.0, -20.0, 9.0, 18.0));
    assert_eq!(out, r(-10.0, -20.0, 5.0, 14.0));
}

// Mutation-gap probe: is_zero boundary (`< -> <=`): c = sy * m21 is exactly 1e-6, which C# `Math.Abs(c) < 1e-6` rejects.
#[test]
fn snap_is_zero_boundary_is_strict() {
    use pr_app::input::primitives::RectD;
    use pr_app::viewport::render_geometry::try_snap_pixel_exact;
    let r = |x, y, w, h| RectD {
        x,
        y,
        width: w,
        height: h,
    };
    let m = Matrix3x2::new(1.0, 0.0, 1.0, 1.0e6, 0.0, 0.0);
    let dest = r(0.0, 0.0, 100.0, 1e-6);
    let src = r(0.0, 0.0, 100.0, 1.0);
    let s = try_snap_pixel_exact(m, dest, src);
    assert!(!s.exact);
    assert_eq!(s.snapped, dest);
}

// Mutation-gap probe: swapped branch needs BOTH b and c unit (kills `&& -> ||` between is_unit(b) and is_unit(c)).
#[test]
fn snap_swapped_requires_both_off_diagonals_unit() {
    use pr_app::input::primitives::RectD;
    use pr_app::viewport::render_geometry::try_snap_pixel_exact;
    let r = |x, y, w, h| RectD {
        x,
        y,
        width: w,
        height: h,
    };
    let dest = r(0.0, 0.0, 100.0, 50.0);
    for m in [
        Matrix3x2::new(0.0, 1.0, 2.0, 0.0, 0.0, 0.0),
        Matrix3x2::new(0.0, 2.0, 1.0, 0.0, 0.0, 0.0),
    ] {
        let s = try_snap_pixel_exact(m, dest, r(0.0, 0.0, 100.0, 50.0));
        assert!(!s.exact, "{m:?}");
    }
}
