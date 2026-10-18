//! Verifier kill-tests for W8-16e (kept out of the commit). Expected values follow the C# `ScrollBarGeometry` source at d92d6aae.
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::viewing::scrollbar_geometry::{ScrollBarMetrics, ScrollBarPart, compute, hit_test};
use pr_app::viewport::types::ViewportLayout;

fn vlayout(vw: f64, vh: f64, max_v: f64) -> ViewportLayout {
    ViewportLayout {
        viewport_width: vw,
        viewport_height: vh,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: false,
        vertical_bar_visible: true,
        image_rect: RectD::default(),
        max_horizontal_offset: 0.0,
        max_vertical_offset: max_v,
    }
}

// C# Sanitize(-0.0) = +0.0 (-0.0 > 0 is false), so Math.Min(10, 0) = +0.0 and the bar width is +0.0.
#[test]
fn negative_zero_client_width_gives_positive_zero_bar_width() {
    let l = compute(
        SizeD {
            width: -0.0,
            height: 100.0,
        },
        &vlayout(0.0, 100.0, 0.0),
        0.0,
        0.0,
        &ScrollBarMetrics::default(),
    );
    assert_eq!(l.vertical.bar.width.to_bits(), 0.0f64.to_bits());
}

// C# Math.Max(-0.0, 0.0) = +0.0: a thumb of length 0 with MinThumbLength -0.0 must stay +0.0.
#[test]
fn max_with_negative_zero_min_thumb_gives_positive_zero_thumb() {
    let m = ScrollBarMetrics {
        min_thumb_length: -0.0,
        ..ScrollBarMetrics::default()
    };
    let l = compute(
        SizeD {
            width: 100.0,
            height: 100.0,
        },
        &vlayout(0.0, 0.0, 1000.0),
        0.0,
        0.0,
        &m,
    );
    assert!(l.vertical.thumb_visible);
    assert_eq!(l.vertical.thumb.height.to_bits(), 0.0f64.to_bits());
}

// C#: trackLength 0, MinThumbLength 0 -> `0 > 0` false, `0 >= thumb && 0 > 0` false -> no thumb.
#[test]
fn zero_track_with_zero_min_thumb_has_no_thumb() {
    let m = ScrollBarMetrics {
        min_thumb_length: 0.0,
        ..ScrollBarMetrics::default()
    };
    let l = compute(
        SizeD {
            width: 100.0,
            height: 4.0,
        },
        &vlayout(0.0, 4.0, 10.0),
        0.0,
        0.0,
        &m,
    );
    assert_eq!(l.vertical.track.height, 0.0);
    assert!(!l.vertical.thumb_visible);
    assert_eq!(l.vertical.thumb, RectD::default());
}

// C#: trackLength 0 > MinThumbLength -1 -> thumb visible (zero length).
#[test]
fn zero_track_with_negative_min_thumb_has_a_thumb() {
    let m = ScrollBarMetrics {
        min_thumb_length: -1.0,
        ..ScrollBarMetrics::default()
    };
    let l = compute(
        SizeD {
            width: 100.0,
            height: 4.0,
        },
        &vlayout(0.0, 4.0, 10.0),
        0.0,
        0.0,
        &m,
    );
    assert!(l.vertical.thumb_visible);
}

// C#: a zero-length thumb never contains the point (Inside needs Height > 0); along == ThumbStart is not `<` -> TrackAfter.
#[test]
fn point_at_the_start_of_a_zero_length_thumb_is_track_after() {
    let m = ScrollBarMetrics {
        min_thumb_length: 0.0,
        ..ScrollBarMetrics::default()
    };
    let l = compute(
        SizeD {
            width: 100.0,
            height: 100.0,
        },
        &vlayout(0.0, 0.0, 1000.0),
        0.0,
        0.0,
        &m,
    );
    assert!(l.vertical.thumb_visible);
    assert_eq!(l.vertical.thumb.height, 0.0);
    assert_eq!(hit_test(&l, PointD { x: 95.0, y: 2.0 }), ScrollBarPart::TrackAfter);
}
