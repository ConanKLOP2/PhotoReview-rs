use pr_app::input::primitives::RectD;
use pr_app::viewport::render_geometry::intersect;

fn r(x: f64, y: f64, w: f64, h: f64) -> RectD {
    RectD {
        x,
        y,
        width: w,
        height: h,
    }
}

// .NET Math.Max(+0.0, -0.0) = +0.0 (verified with dotnet fsi); Rust math_max returns -0.0.
#[test]
fn intersect_max_signed_zero_matches_dotnet() {
    let out = intersect(r(0.0, 0.0, 10.0, 10.0), r(-0.0, -0.0, 10.0, 10.0));
    assert_eq!(out.x.to_bits(), 0.0f64.to_bits());
    assert_eq!(out.y.to_bits(), 0.0f64.to_bits());
}
