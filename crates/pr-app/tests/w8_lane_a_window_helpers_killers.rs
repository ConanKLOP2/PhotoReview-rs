//! Mutation killers for `pr_app::misc::window_helpers` (lane A, W8-04e). Expected values are derived by hand from the C#
//! `MainWindowHelpers` (`CalculateUniformImagePoint`, `NormalizeImagePoint`) and the record-struct equality contract.
//!
//! Equivalent (no test): `normalize_image_point` closure `v > 0.0` -> `v >= 0.0` (window_helpers.rs:79:31). A zero size makes
//! `point.x / 0.0` (or `y / 0.0`) infinite or NaN, so the `is_finite` guard on line 83 returns `IMAGE_CENTRE` exactly like the
//! `positive` guard would.

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::misc::window_helpers::{
    IMAGE_CENTRE, ZoomImagePoint, ZoomViewportOffsets, calculate_uniform_image_point, normalize_image_point,
};

fn sz(width: f64, height: f64) -> SizeD {
    SizeD { width, height }
}
fn pt(x: f64, y: f64) -> PointD {
    PointD { x, y }
}
fn zip(x: f64, y: f64) -> ZoomImagePoint {
    ZoomImagePoint { x, y }
}

#[test]
fn zoom_viewport_offsets_eq_is_field_wise() {
    let a = ZoomViewportOffsets {
        horizontal: 1.0,
        vertical: 2.0,
    };
    assert!(
        a == ZoomViewportOffsets {
            horizontal: 1.0,
            vertical: 2.0
        }
    );
    assert!(
        a != ZoomViewportOffsets {
            horizontal: 1.0,
            vertical: 3.0
        }
    );
    assert!(
        a != ZoomViewportOffsets {
            horizontal: 4.0,
            vertical: 2.0
        }
    );
    let n = ZoomViewportOffsets {
        horizontal: f64::NAN,
        vertical: 2.0,
    };
    assert!(n == n);
}

#[test]
fn zoom_image_point_eq_needs_both_axes() {
    let a = zip(1.0, 2.0);
    assert!(a == zip(1.0, 2.0));
    assert!(a != zip(1.0, 3.0));
    assert!(a != zip(4.0, 2.0));
    assert!(a != zip(4.0, 3.0));
}

#[test]
fn uniform_point_invalid_element_height_alone_gives_origin() {
    // element 200 x 0 is unusable: C# early return (0, 0).
    let p = calculate_uniform_image_point(sz(200.0, 0.0), sz(100.0, 100.0), pt(150.0, 40.0));
    assert_eq!(p, zip(0.0, 0.0));
}

#[test]
fn uniform_point_invalid_source_width_alone_gives_origin() {
    let p = calculate_uniform_image_point(sz(200.0, 100.0), sz(0.0, 100.0), pt(150.0, 40.0));
    assert_eq!(p, zip(0.0, 0.0));
}

#[test]
fn uniform_point_invalid_source_height_alone_gives_origin() {
    let p = calculate_uniform_image_point(sz(200.0, 100.0), sz(100.0, 0.0), pt(150.0, 40.0));
    assert_eq!(p, zip(0.0, 0.0));
}

#[test]
fn uniform_point_height_ratio_is_the_limiting_scale() {
    // scale = min(200/100, 100/100) = 1; left = (200 - 100) / 2 = 50; top = 0.
    // x = (120 - 50) / 1 = 70; y = (30 - 0) / 1 = 30.
    let p = calculate_uniform_image_point(sz(200.0, 100.0), sz(100.0, 100.0), pt(120.0, 30.0));
    assert_eq!(p, zip(70.0, 30.0));
}

#[test]
fn normalize_point_negative_width_with_positive_height_gives_centre() {
    // width -100 is not positive: C# returns (0.5, 0.5) even though x / width and y / height are finite.
    let p = normalize_image_point(pt(50.0, 50.0), sz(-100.0, 200.0));
    assert_eq!(p, IMAGE_CENTRE);
    let q = normalize_image_point(pt(50.0, 50.0), sz(100.0, -200.0));
    assert_eq!(q, IMAGE_CENTRE);
}
