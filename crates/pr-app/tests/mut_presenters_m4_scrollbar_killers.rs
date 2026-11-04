//! M4 mutation killers for `viewing/scrollbar_geometry.rs` (hand-written, no golden data).
//! Port of the intent of C# `ScrollBarLayout.AnyVisible` and `ScrollBarGeometry.AxisAt` / `Inside`.
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::viewing::scrollbar_geometry::{
    ScrollAxis, ScrollBarBox, ScrollBarLayout, ScrollBarMetrics, axis_at, compute, inside,
};
use pr_app::viewport::types::ViewportLayout;

fn layout(vertical: bool, horizontal: bool) -> ScrollBarLayout {
    let mut l = ScrollBarLayout::none();
    l.vertical = ScrollBarBox {
        visible: vertical,
        ..ScrollBarBox::hidden(ScrollAxis::Vertical)
    };
    l.horizontal = ScrollBarBox {
        visible: horizontal,
        ..ScrollBarBox::hidden(ScrollAxis::Horizontal)
    };
    l
}

#[test]
fn any_visible_is_the_or_of_both_boxes() {
    assert!(!layout(false, false).any_visible());
    assert!(layout(true, false).any_visible());
    assert!(layout(false, true).any_visible());
    assert!(layout(true, true).any_visible());
}

fn both_bars(client: f64) -> ScrollBarLayout {
    let vl = ViewportLayout {
        viewport_width: client,
        viewport_height: client,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: true,
        image_rect: RectD::default(),
        max_horizontal_offset: 50.0,
        max_vertical_offset: 50.0,
    };
    compute(
        SizeD {
            width: client,
            height: client,
        },
        &vl,
        0.0,
        0.0,
        &ScrollBarMetrics::default(),
    )
}

fn centre(r: RectD) -> PointD {
    PointD {
        x: r.x + r.width / 2.0,
        y: r.y + r.height / 2.0,
    }
}

// C# `AxisAt`: vertical inside the vertical bar, horizontal inside the horizontal bar, None elsewhere (also in the corner).
#[test]
fn axis_at_reports_the_bar_under_the_point() {
    let l = both_bars(200.0);
    assert!(l.vertical.visible && l.horizontal.visible);
    assert_eq!(axis_at(&l, centre(l.vertical.bar)), Some(ScrollAxis::Vertical));
    assert_eq!(axis_at(&l, centre(l.horizontal.bar)), Some(ScrollAxis::Horizontal));
    assert_eq!(axis_at(&l, PointD { x: 10.0, y: 10.0 }), None);
    assert!(l.has_corner);
    assert_eq!(axis_at(&l, centre(l.corner)), None);
    // A hidden box never answers, even if its rectangle contains the point.
    let mut hidden = l;
    hidden.vertical.visible = false;
    assert_eq!(axis_at(&hidden, centre(l.vertical.bar)), None);
    hidden = l;
    hidden.horizontal.visible = false;
    assert_eq!(axis_at(&hidden, centre(l.horizontal.bar)), None);
}

// C# `Inside` is half-open: the left/top edge is in, the right/bottom edge is out.
#[test]
fn inside_is_half_open_on_all_four_sides() {
    let r = RectD {
        x: 10.0,
        y: 20.0,
        width: 5.0,
        height: 4.0,
    };
    let at = |x: f64, y: f64| inside(r, PointD { x, y });
    assert!(at(10.0, 20.0));
    assert!(at(14.9, 23.9));
    assert!(!at(15.0, 22.0));
    assert!(!at(12.0, 24.0));
    assert!(!at(9.9, 22.0));
    assert!(!at(12.0, 19.9));
    let empty_w = RectD { width: 0.0, ..r };
    assert!(!inside(empty_w, PointD { x: 10.0, y: 21.0 }));
    let empty_h = RectD { height: 0.0, ..r };
    assert!(!inside(empty_h, PointD { x: 11.0, y: 20.0 }));
}

// C# `BuildBox` track rectangle: the margins are applied on both sides. The default margins (2, 1) hide `2 * m` vs `2 / m`
// for the across margin 1, so use margins whose product and quotient differ.
#[test]
fn track_margins_shrink_the_bar_on_both_sides_for_both_axes() {
    let metrics = ScrollBarMetrics {
        thickness: 12.0,
        track_margin_along: 3.0,
        track_margin_across: 2.0,
        ..ScrollBarMetrics::default()
    };
    let vl = ViewportLayout {
        viewport_width: 200.0,
        viewport_height: 100.0,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: true,
        image_rect: RectD::default(),
        max_horizontal_offset: 50.0,
        max_vertical_offset: 50.0,
    };
    let l = compute(
        SizeD {
            width: 200.0,
            height: 100.0,
        },
        &vl,
        0.0,
        0.0,
        &metrics,
    );
    // Vertical bar: x 188..200, y 0..88 (the corner takes the last 12).
    assert_eq!(
        l.vertical.bar,
        RectD {
            x: 188.0,
            y: 0.0,
            width: 12.0,
            height: 88.0
        }
    );
    assert_eq!(
        l.vertical.track,
        RectD {
            x: 190.0,
            y: 3.0,
            width: 8.0,
            height: 82.0
        }
    );
    // Horizontal bar: x 0..188, y 88..100.
    assert_eq!(
        l.horizontal.bar,
        RectD {
            x: 0.0,
            y: 88.0,
            width: 188.0,
            height: 12.0
        }
    );
    assert_eq!(
        l.horizontal.track,
        RectD {
            x: 3.0,
            y: 90.0,
            width: 182.0,
            height: 8.0
        }
    );
}

// C# `BuildBox`: `total = range + viewport`; `total > 0` is false for an empty range and an empty viewport, and the thumb then
// fills the whole track (the division 0/0 must not happen).
#[test]
fn zero_range_and_zero_viewport_give_a_full_length_thumb() {
    let vl = ViewportLayout {
        viewport_width: 100.0,
        viewport_height: 0.0,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: false,
        vertical_bar_visible: true,
        image_rect: RectD::default(),
        max_horizontal_offset: 0.0,
        max_vertical_offset: 0.0,
    };
    let l = compute(
        SizeD {
            width: 100.0,
            height: 100.0,
        },
        &vl,
        0.0,
        0.0,
        &ScrollBarMetrics::default(),
    );
    assert_eq!(l.vertical.track.height, 96.0);
    assert!(l.vertical.thumb_visible);
    assert_eq!(l.vertical.thumb.height, 96.0);
    assert_eq!(l.vertical.thumb.y, l.vertical.track.y);
}

fn vertical_only(client: f64, metrics: &ScrollBarMetrics) -> ScrollBarLayout {
    let vl = ViewportLayout {
        viewport_width: client,
        viewport_height: client,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: false,
        vertical_bar_visible: true,
        image_rect: RectD::default(),
        max_horizontal_offset: 0.0,
        max_vertical_offset: 50.0,
    };
    compute(
        SizeD {
            width: client,
            height: client,
        },
        &vl,
        0.0,
        0.0,
        metrics,
    )
}

// C# `BuildBox` `thumbVisible = trackLength > min || (trackLength >= thumb && trackLength > 0)`: a track exactly as long as the
// minimum thumb still shows the thumb (second clause), a track shorter than the minimum thumb does not.
#[test]
fn track_exactly_as_long_as_the_minimum_thumb_still_shows_the_thumb() {
    let exact = ScrollBarMetrics {
        min_thumb_length: 96.0,
        ..ScrollBarMetrics::default()
    };
    let l = vertical_only(100.0, &exact);
    assert_eq!(l.vertical.track.height, 96.0);
    assert!(l.vertical.thumb_visible);
    assert_eq!(l.vertical.thumb.height, 96.0);

    let longer_min = ScrollBarMetrics {
        min_thumb_length: 97.0,
        ..ScrollBarMetrics::default()
    };
    assert!(!vertical_only(100.0, &longer_min).vertical.thumb_visible);
}

// C# `Math.Max(minThumb, thumb)` is NaN when the minimum thumb length is NaN (either operand), so the thumb is NaN and neither
// visibility clause holds: no thumb is shown. Rust `f64::max` (or a one-sided NaN check) would return the other operand.
#[test]
fn nan_minimum_thumb_length_hides_the_thumb() {
    let nan_min = ScrollBarMetrics {
        min_thumb_length: f64::NAN,
        ..ScrollBarMetrics::default()
    };
    let l = vertical_only(100.0, &nan_min);
    assert!(l.vertical.visible);
    assert!(!l.vertical.thumb_visible);
    assert_eq!(l.vertical.thumb, RectD::default());
}
