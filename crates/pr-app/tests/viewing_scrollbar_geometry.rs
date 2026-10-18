//! Red tests (W8-16e, test porter) for `pr_app::viewing::scrollbar_geometry`: the port of
//! `PhotoReview.Shell.Win32/Viewing/ScrollBarGeometry.cs` (baseline `d92d6aae`).
//!
//! Three kinds of test live here:
//! - F tests, one per C# method of `tests/PhotoReview.Shell.Tests/Viewing/ScrollBarGeometryTests.cs` (same name in snake_case,
//!   `// C#: ScrollBarGeometryTests.<Method>`), with the C# expectations. They use the literal layouts of the W8-16e contract
//!   table (`layout(id)`), not the C# `ViewportLayoutEngine` (W8-01b is not needed by Rust); one test checks the table against
//!   the engine echoes in the golden.
//! - Extra F tests (`// new:`), one per trap of the contract (.NET `Math.Min`/`Math.Max` NaN and `-0` rules, `sanitize`, the
//!   half-open `inside`, corner priority, non-zero `Default`).
//! - Golden drivers over `tests/golden/scrollbar-geometry.json` (133 cases).
//!
//! Number convention: a JSON number, or the strings "NaN", "Infinity", "-Infinity" and "-0". Doubles are compared bit for bit
//! (`same`): NaN matches NaN and the sign of zero counts. `assert_close` mirrors xunit `Assert.Equal(expected, actual, digits)`.
//!
//! Every test here fails through `todo!()` until the bodies land (the `ScrollBarMetrics::default()` and `compute` calls panic).

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::viewing::scrollbar_geometry::{
    DEFAULT_MIN_THUMB_LENGTH, ScrollAxis, ScrollBarBox, ScrollBarLayout, ScrollBarMetrics, ScrollBarPart, axis_at,
    compute, hit_test, inside, offset_for_thumb_start,
};
use pr_app::viewport::types::ViewportLayout;
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Literal layouts (contract table "Literal layouts"): the engine output for the C# `LayoutFor` cases
// ---------------------------------------------------------------------------------------------------------------------

/// The layouts of the contract table, by id. Fields the geometry never reads stay zero / false / `RectD::default()`.
fn layout(id: &str) -> ViewportLayout {
    let (viewport_width, viewport_height, extent_width, extent_height, horizontal, vertical, max_h, max_v) = match id {
        // LayoutFor(1000, 600, 3000, 2000)
        "both" => (990.0, 590.0, 3000.0, 2000.0, true, true, 2010.0, 1410.0),
        // LayoutFor(1000, 600, 800, 500)
        "fit" => (1000.0, 600.0, 800.0, 500.0, false, false, 0.0, 0.0),
        // LayoutFor(1000, 600, 900, 2000)
        "vertical_only" => (990.0, 600.0, 900.0, 2000.0, false, true, 0.0, 1400.0),
        // LayoutFor(1000, 600, 3000, 500)
        "horizontal_only" => (1000.0, 590.0, 3000.0, 500.0, true, false, 2000.0, 0.0),
        // LayoutFor(1000, 600, 995, 2000)
        "second_bar" => (990.0, 590.0, 995.0, 2000.0, true, true, 5.0, 1410.0),
        // Policy Overlay, LayoutFor(1000, 600, 3000, 2000) with the overlay policy
        "overlay" => (1000.0, 600.0, 3000.0, 2000.0, true, true, 2000.0, 1400.0),
        // Policy Hidden, same extent
        "hidden" => (1000.0, 600.0, 3000.0, 2000.0, false, false, 0.0, 0.0),
        // LayoutFor(14, 14, 100, 100)
        "tiny" => (4.0, 4.0, 100.0, 100.0, true, true, 96.0, 96.0),
        // LayoutFor(1000, 600, 3000, 1_000_000)
        "big_extent" => (990.0, 590.0, 3000.0, 1_000_000.0, true, true, 2010.0, 999_410.0),
        other => panic!("unknown literal layout {other:?}"),
    };
    ViewportLayout {
        viewport_width,
        viewport_height,
        extent_width,
        extent_height,
        horizontal_bar_visible: horizontal,
        vertical_bar_visible: vertical,
        image_rect: RectD::default(),
        max_horizontal_offset: max_h,
        max_vertical_offset: max_v,
    }
}

/// The C# `Compute(clientWidth, clientHeight, ...)` helper with the literal layout of `id` and the default metrics.
fn compute_for(id: &str, width: f64, height: f64, horizontal_offset: f64, vertical_offset: f64) -> ScrollBarLayout {
    compute(
        SizeD { width, height },
        &layout(id),
        horizontal_offset,
        vertical_offset,
        &ScrollBarMetrics::default(),
    )
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

// ---------------------------------------------------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------------------------------------------------

/// Bit-exact double equality: NaN matches NaN, and `0.0` differs from `-0.0` (contract traps T1 and T3).
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// xunit `Assert.Equal(expected, actual, precision)`: both values rounded to `digits` decimals (half away from zero), then equal.
fn assert_close(actual: f64, expected: f64, digits: i32) {
    let scale = 10f64.powi(digits);
    let a = (actual * scale).round() / scale;
    let e = (expected * scale).round() / scale;
    assert!(a == e, "actual {actual} != expected {expected} at {digits} digits");
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden readers (the number convention of `tests/golden`)
// ---------------------------------------------------------------------------------------------------------------------

/// A golden number: a JSON number, or one of the four strings of the oracle convention.
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

fn field<'a>(object: &'a Value, key: &str) -> &'a Value {
    object.get(key).unwrap_or_else(|| panic!("missing '{key}' in {object}"))
}

fn num_of(object: &Value, key: &str) -> f64 {
    num(field(object, key))
}

/// A number that may be absent (the direct-form layouts and boxes of the golden leave keys out).
fn opt_num(object: &Value, key: &str) -> f64 {
    object.get(key).map_or(0.0, num)
}

fn opt_bool(object: &Value, key: &str) -> bool {
    object.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn opt_rect(object: &Value, key: &str) -> RectD {
    object.get(key).map_or_else(RectD::default, rect_of)
}

fn rect_of(value: &Value) -> RectD {
    RectD {
        x: num_of(value, "x"),
        y: num_of(value, "y"),
        width: num_of(value, "width"),
        height: num_of(value, "height"),
    }
}

fn point_of(value: &Value) -> PointD {
    PointD {
        x: num_of(value, "x"),
        y: num_of(value, "y"),
    }
}

fn layout_from(value: &Value) -> ViewportLayout {
    ViewportLayout {
        viewport_width: opt_num(value, "viewport_width"),
        viewport_height: opt_num(value, "viewport_height"),
        extent_width: opt_num(value, "extent_width"),
        extent_height: opt_num(value, "extent_height"),
        horizontal_bar_visible: opt_bool(value, "horizontal_bar_visible"),
        vertical_bar_visible: opt_bool(value, "vertical_bar_visible"),
        image_rect: opt_rect(value, "image_rect"),
        max_horizontal_offset: opt_num(value, "max_horizontal_offset"),
        max_vertical_offset: opt_num(value, "max_vertical_offset"),
    }
}

/// The client of a case: `client_size`, else the engine's `client` (engine form without `client_size`).
fn client_of(case: &Value) -> SizeD {
    let pair = case
        .get("client_size")
        .or_else(|| case.get("engine").and_then(|engine| engine.get("client")))
        .unwrap_or_else(|| panic!("no client in {case}"));
    let items = pair
        .as_array()
        .unwrap_or_else(|| panic!("client is not an array: {pair}"));
    SizeD {
        width: num(&items[0]),
        height: num(&items[1]),
    }
}

/// The metrics of a case: the defaults with the optional overrides applied.
fn metrics_of(case: &Value) -> ScrollBarMetrics {
    let mut metrics = ScrollBarMetrics::default();
    if let Some(overrides) = case.get("metrics") {
        if let Some(v) = overrides.get("thickness") {
            metrics.thickness = num(v);
        }
        if let Some(v) = overrides.get("track_margin_along") {
            metrics.track_margin_along = num(v);
        }
        if let Some(v) = overrides.get("track_margin_across") {
            metrics.track_margin_across = num(v);
        }
        if let Some(v) = overrides.get("min_thumb_length") {
            metrics.min_thumb_length = num(v);
        }
        if let Some(v) = overrides.get("thumb_radius") {
            metrics.thumb_radius = num(v);
        }
    }
    metrics
}

fn axis_of(name: &str) -> ScrollAxis {
    match name {
        "Vertical" => ScrollAxis::Vertical,
        "Horizontal" => ScrollAxis::Horizontal,
        other => panic!("unknown axis {other:?}"),
    }
}

fn axis_name(axis: ScrollAxis) -> &'static str {
    match axis {
        ScrollAxis::Vertical => "Vertical",
        ScrollAxis::Horizontal => "Horizontal",
    }
}

fn part_name(part: ScrollBarPart) -> &'static str {
    match part {
        ScrollBarPart::None => "None",
        ScrollBarPart::Thumb => "Thumb",
        ScrollBarPart::TrackBefore => "TrackBefore",
        ScrollBarPart::TrackAfter => "TrackAfter",
        ScrollBarPart::Background => "Background",
        ScrollBarPart::Corner => "Corner",
    }
}

/// A golden box. A key that is absent gives the zero value (the hit-test golden writes only some boxes out in full).
fn box_from(value: &Value, default_axis: ScrollAxis) -> ScrollBarBox {
    let axis = value.get("axis").and_then(Value::as_str).map_or(default_axis, axis_of);
    ScrollBarBox {
        axis,
        visible: opt_bool(value, "visible"),
        bar: opt_rect(value, "bar"),
        track: opt_rect(value, "track"),
        thumb: opt_rect(value, "thumb"),
        thumb_visible: opt_bool(value, "thumb_visible"),
        max_offset: opt_num(value, "max_offset"),
        offset: opt_num(value, "offset"),
        viewport_length: opt_num(value, "viewport_length"),
    }
}

fn case_by_id<'a>(cases: &'a [Value], id: &str) -> &'a Value {
    cases
        .iter()
        .find(|case| case.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("no golden case with id {id:?}"))
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden comparisons (every double bit for bit)
// ---------------------------------------------------------------------------------------------------------------------

fn check_num(actual: f64, expected: &Value, ctx: &str) {
    let want = num(expected);
    assert!(same(actual, want), "{ctx}: actual {actual:?} expected {want:?}");
}

fn check_key(actual: f64, object: &Value, key: &str, ctx: &str) {
    check_num(actual, field(object, key), &format!("{ctx}.{key}"));
}

fn check_rect(actual: RectD, expected: &Value, ctx: &str) {
    check_key(actual.x, expected, "x", ctx);
    check_key(actual.y, expected, "y", ctx);
    check_key(actual.width, expected, "width", ctx);
    check_key(actual.height, expected, "height", ctx);
}

/// Every field of a golden box, including the derived ones (`track_start`, `track_length`, `thumb_start`, `thumb_length`, `travel`).
fn check_box(actual: &ScrollBarBox, expected: &Value, ctx: &str) {
    assert_eq!(axis_name(actual.axis), str_of(expected, "axis"), "{ctx}: axis");
    assert_eq!(actual.visible, bool_of(expected, "visible"), "{ctx}: visible");
    check_rect(actual.bar, field(expected, "bar"), &format!("{ctx}.bar"));
    check_rect(actual.track, field(expected, "track"), &format!("{ctx}.track"));
    check_rect(actual.thumb, field(expected, "thumb"), &format!("{ctx}.thumb"));
    assert_eq!(
        actual.thumb_visible,
        bool_of(expected, "thumb_visible"),
        "{ctx}: thumb_visible"
    );
    check_key(actual.max_offset, expected, "max_offset", ctx);
    check_key(actual.offset, expected, "offset", ctx);
    check_key(actual.viewport_length, expected, "viewport_length", ctx);
    check_key(actual.track_start(), expected, "track_start", ctx);
    check_key(actual.track_length(), expected, "track_length", ctx);
    check_key(actual.thumb_start(), expected, "thumb_start", ctx);
    check_key(actual.thumb_length(), expected, "thumb_length", ctx);
    check_key(actual.travel(), expected, "travel", ctx);
}

/// A whole layout: both boxes, the corner square, `has_corner` and `any_visible`.
fn check_layout(actual: &ScrollBarLayout, expected: &Value, ctx: &str) {
    check_box(
        &actual.vertical,
        field(expected, "vertical"),
        &format!("{ctx}: vertical"),
    );
    check_box(
        &actual.horizontal,
        field(expected, "horizontal"),
        &format!("{ctx}: horizontal"),
    );
    check_rect(actual.corner, field(expected, "corner"), &format!("{ctx}: corner"));
    assert_eq!(actual.has_corner, bool_of(expected, "has_corner"), "{ctx}: has_corner");
    assert_eq!(
        actual.any_visible(),
        bool_of(expected, "any_visible"),
        "{ctx}: any_visible"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// F tests of the C# file (ScrollBarGeometryTests)
// ---------------------------------------------------------------------------------------------------------------------

// C#: ScrollBarGeometryTests.Defaults_AreTheValuesOfDarkScrollBarsXaml
#[test]
fn defaults_are_the_values_of_dark_scrollbars_xaml() {
    let metrics = ScrollBarMetrics::default();
    assert_eq!(metrics.thickness, 10.0);
    assert_eq!(metrics.track_margin_along, 2.0);
    assert_eq!(metrics.track_margin_across, 1.0);
    assert_eq!(metrics.min_thumb_length, 8.0);
    assert_eq!(metrics.thumb_radius, 4.0);
    assert_eq!(DEFAULT_MIN_THUMB_LENGTH, 8.0);
    // C#: Assert.Equal(new ViewportControllerOptions().ScrollBarThickness, Metrics.Thickness);
    // W8-16h: assert thickness == ViewportControllerOptions::default().scroll_bar_thickness
}

// C#: ScrollBarGeometryTests.BothBars_BarsOccupyTheEdges_TracksHaveTheirMargins_AndTheCornerIsTheSquareLeft
#[test]
fn both_bars_bars_occupy_the_edges_tracks_have_their_margins_and_the_corner_is_the_square_left() {
    let layout = compute_for("both", 1000.0, 600.0, 0.0, 0.0);

    assert!(layout.vertical.visible);
    assert!(layout.horizontal.visible);
    assert_eq!(layout.vertical.bar, rect(990.0, 0.0, 10.0, 590.0));
    assert_eq!(layout.horizontal.bar, rect(0.0, 590.0, 990.0, 10.0));
    assert_eq!(layout.vertical.track, rect(991.0, 2.0, 8.0, 586.0)); // Margin="1,2"
    assert_eq!(layout.horizontal.track, rect(2.0, 591.0, 986.0, 8.0)); // Margin="2,1"
    assert!(layout.has_corner);
    assert_eq!(layout.corner, rect(990.0, 590.0, 10.0, 10.0));
}

// C#: ScrollBarGeometryTests.ThumbLength_IsTrackTimesViewportOverExtent_AndPositionIsProportionalToTheOffset
#[test]
fn thumb_length_is_track_times_viewport_over_extent_and_position_is_proportional_to_the_offset() {
    let at_top = compute_for("both", 1000.0, 600.0, 0.0, 0.0).vertical;
    // viewport 590, extent 2000, rãnh 586 -> 586 * 590 / 2000 = 172,87.
    assert_close(at_top.thumb.height, 172.87, 6);
    assert_close(at_top.thumb.y, 2.0, 9);
    assert_close(at_top.thumb.x, 991.0, 9);
    assert_close(at_top.thumb.width, 8.0, 9);

    let middle = compute_for("both", 1000.0, 600.0, 0.0, 705.0).vertical; // max 1410: đúng nửa
    assert_close(middle.thumb.y, 2.0 + ((586.0 - 172.87) / 2.0), 6);

    let bottom = compute_for("both", 1000.0, 600.0, 0.0, 1410.0).vertical;
    assert_close(bottom.thumb.y, 2.0 + 586.0 - 172.87, 6);
    assert_close(bottom.thumb.bottom(), 588.0, 6); // rãnh kết thúc cách đáy thanh (590) 2 DIP

    let horizontal = compute_for("both", 1000.0, 600.0, 2010.0, 0.0).horizontal;
    assert_close(horizontal.thumb.width, 986.0 * 990.0 / 3000.0, 6);
    assert_close(horizontal.thumb.x, 2.0 + 986.0 - (986.0 * 990.0 / 3000.0), 6);
}

// C#: ScrollBarGeometryTests.ThumbHasAMinimumLength_ThatTheProportionalSizeWouldGoBelow
#[test]
fn thumb_has_a_minimum_length_that_the_proportional_size_would_go_below() {
    let box_ = compute_for("big_extent", 1000.0, 600.0, 0.0, 0.0).vertical;
    assert_close(box_.thumb.height, 8.0, 9);
    assert!(box_.thumb_visible);
    // Con trượt tối thiểu vẫn đi hết rãnh: offset tối đa -> sát đáy rãnh.
    let bottom = compute_for("big_extent", 1000.0, 600.0, 0.0, 1e9).vertical;
    assert_close(bottom.track.bottom(), bottom.thumb.bottom(), 9);
}

// C#: ScrollBarGeometryTests.NoBarsAreProduced_WhenTheImageFitsTheViewport
#[test]
fn no_bars_are_produced_when_the_image_fits_the_viewport() {
    let layout = compute_for("fit", 1000.0, 600.0, 0.0, 0.0);
    assert!(!layout.any_visible());
    assert!(!layout.has_corner);
    assert_eq!(hit_test(&layout, PointD { x: 995.0, y: 300.0 }), ScrollBarPart::None);
}

// C#: ScrollBarGeometryTests.OnlyOneBar_NoCorner_AndTheBarSpansTheWholeEdge
#[test]
fn only_one_bar_no_corner_and_the_bar_spans_the_whole_edge() {
    let vertical_only = compute_for("vertical_only", 1000.0, 600.0, 0.0, 0.0);
    assert!(vertical_only.vertical.visible);
    assert!(!vertical_only.horizontal.visible);
    assert!(!vertical_only.has_corner);
    assert_eq!(vertical_only.vertical.bar, rect(990.0, 0.0, 10.0, 600.0));

    let horizontal_only = compute_for("horizontal_only", 1000.0, 600.0, 0.0, 0.0);
    assert!(!horizontal_only.vertical.visible);
    assert_eq!(horizontal_only.horizontal.bar, rect(0.0, 590.0, 1000.0, 10.0));
}

// C#: ScrollBarGeometryTests.TheSecondBarAppearsBecauseTheFirstOneTookRoom_LikeWpfsThirdMeasure
#[test]
fn the_second_bar_appears_because_the_first_one_took_room_like_wpfs_third_measure() {
    // Ảnh rộng 995 trong client 1000: chỉ thanh dọc (cao 2000), nhưng thanh dọc làm viewport còn 990 -> thanh ngang hiện.
    let layout = compute_for("second_bar", 1000.0, 600.0, 0.0, 0.0);
    assert!(layout.vertical.visible);
    assert!(layout.horizontal.visible);
    assert!(layout.has_corner);
}

// C#: ScrollBarGeometryTests.OverlayPolicy_DrawsBarsOverTheImage_WithoutShrinkingTheViewport_AndHiddenPolicyDrawsNone
#[test]
fn overlay_policy_draws_bars_over_the_image_without_shrinking_the_viewport_and_hidden_policy_draws_none() {
    // NE-3 (b)/(c) chưa chọn (mặc định (a)), nhưng chính sách của engine vẫn phải cho hình học đúng nếu đổi sau này.
    let overlay_layout = layout("overlay");
    assert_close(overlay_layout.viewport_width, 1000.0, 9);
    let overlay = compute_for("overlay", 1000.0, 600.0, 0.0, 0.0);
    assert_eq!(overlay.vertical.bar, rect(990.0, 0.0, 10.0, 590.0)); // cùng chỗ với chế độ chiếm chỗ: cạnh phải/dưới
    assert_eq!(overlay.horizontal.bar, rect(0.0, 590.0, 990.0, 10.0));
    // Viewport rộng hơn (1000): con trượt tính theo viewport 1000 / extent 3000.
    assert_close(overlay.horizontal.thumb.width, 986.0 * 1000.0 / 3000.0, 6);

    let hidden = compute_for("hidden", 1000.0, 600.0, 0.0, 0.0);
    assert!(!hidden.any_visible());
}

// C#: ScrollBarGeometryTests.ThumbIsHidden_WhenTheTrackHasNoRoomForIt_ButTheBarStillShows
#[test]
fn thumb_is_hidden_when_the_track_has_no_room_for_it_but_the_bar_still_shows() {
    // Thanh dọc cao 14 - 10 = 4 DIP: rãnh 0.
    let layout = compute_for("tiny", 14.0, 14.0, 0.0, 0.0);
    assert!(layout.vertical.visible);
    assert!(!layout.vertical.thumb_visible);
    assert_eq!(hit_test(&layout, PointD { x: 12.0, y: 2.0 }), ScrollBarPart::Background);
}

// C#: ScrollBarGeometryTests.OffsetForThumbStart_IsLinear_ClampedAndZeroWithoutTravel
#[test]
fn offset_for_thumb_start_is_linear_clamped_and_zero_without_travel() {
    let box_ = compute_for("both", 1000.0, 600.0, 0.0, 0.0).vertical;
    let travel = box_.travel();

    assert_close(offset_for_thumb_start(&box_, box_.track_start()), 0.0, 9);
    assert_close(offset_for_thumb_start(&box_, box_.track_start() - 50.0), 0.0, 9);
    assert_close(offset_for_thumb_start(&box_, box_.track_start() + travel), 1410.0, 9);
    assert_close(
        offset_for_thumb_start(&box_, box_.track_start() + travel + 999.0),
        1410.0,
        9,
    );
    assert_close(
        offset_for_thumb_start(&box_, box_.track_start() + 100.0),
        1410.0 * 100.0 / travel,
        9,
    );

    let hidden = compute_for("tiny", 14.0, 14.0, 0.0, 0.0).vertical;
    assert_close(offset_for_thumb_start(&hidden, 3.0), 0.0, 9);
}

// C#: ScrollBarGeometryTests.ThumbPositionAndOffsetAreInverse_AcrossTheRange
#[test]
fn thumb_position_and_offset_are_inverse_across_the_range() {
    for offset in [0.0, 1.0, 333.3, 705.0, 1409.0, 1410.0] {
        let box_ = compute_for("both", 1000.0, 600.0, 0.0, offset).vertical;
        assert_close(offset_for_thumb_start(&box_, box_.thumb_start()), offset, 6);
    }
}

// C#: ScrollBarGeometryTests.HitTest_ReportsThePartUnderThePoint (theory, 9 rows)
#[test]
fn hit_test_reports_the_part_under_the_point() {
    let rows: [(f64, f64, ScrollBarPart); 9] = [
        (995.0, 100.0, ScrollBarPart::Thumb), // trong con trượt (2..174,87)
        (995.0, 300.0, ScrollBarPart::TrackAfter),
        (990.5, 300.0, ScrollBarPart::Background), // lề trái của Track (Margin 1)
        (995.0, 1.0, ScrollBarPart::Background),   // lề trên của Track (Margin 2)
        (995.0, 588.5, ScrollBarPart::Background), // lề dưới
        (500.0, 595.0, ScrollBarPart::TrackAfter), // thanh ngang
        (100.0, 595.0, ScrollBarPart::Thumb),
        (995.0, 595.0, ScrollBarPart::Corner),
        (500.0, 300.0, ScrollBarPart::None),
    ];
    let layout = compute_for("both", 1000.0, 600.0, 0.0, 0.0);
    for (x, y, expected) in rows {
        assert_eq!(hit_test(&layout, PointD { x, y }), expected, "point ({x}, {y})");
    }
}

// C#: ScrollBarGeometryTests.HitTest_TrackBeforeAndAfter_AreSeparatedByTheThumb_WithHalfOpenEdges
#[test]
fn hit_test_track_before_and_after_are_separated_by_the_thumb_with_half_open_edges() {
    let layout = compute_for("both", 1000.0, 600.0, 0.0, 705.0);
    let thumb = layout.vertical.thumb;
    assert_eq!(
        hit_test(
            &layout,
            PointD {
                x: 995.0,
                y: thumb.y - 0.001
            }
        ),
        ScrollBarPart::TrackBefore
    );
    assert_eq!(hit_test(&layout, PointD { x: 995.0, y: thumb.y }), ScrollBarPart::Thumb);
    assert_eq!(
        hit_test(
            &layout,
            PointD {
                x: 995.0,
                y: thumb.bottom() - 0.001
            }
        ),
        ScrollBarPart::Thumb
    );
    assert_eq!(
        hit_test(
            &layout,
            PointD {
                x: 995.0,
                y: thumb.bottom()
            }
        ),
        ScrollBarPart::TrackAfter
    );
    assert_eq!(hit_test(&layout, PointD { x: 1000.0, y: 100.0 }), ScrollBarPart::None); // phải của thanh: ngoài
}

// C#: ScrollBarGeometryTests.NonFiniteOrNegativeInputs_DoNotProduceNaNBoxes
// The C# test feeds the engine a NaN client. The Rust side uses the echoed engine layout of the golden case `nan_client`
// (contract: do not invent a layout for it).
#[test]
fn non_finite_or_negative_inputs_do_not_produce_nan_boxes() {
    let golden = golden::load("scrollbar-geometry");
    let case = case_by_id(&golden.cases, "nan_client");
    let echoed = layout_from(field(case, "layout"));
    let geometry = compute(
        SizeD {
            width: f64::NAN,
            height: 600.0,
        },
        &echoed,
        f64::NAN,
        -5.0,
        &ScrollBarMetrics::default(),
    );
    for b in [&geometry.vertical, &geometry.horizontal] {
        let t = b.thumb;
        assert!(
            !t.x.is_nan() && !t.y.is_nan() && !t.width.is_nan() && !t.height.is_nan(),
            "NaN thumb in {:?}: {t:?}",
            b.axis
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Extra F tests (new): one per trap of the contract
// ---------------------------------------------------------------------------------------------------------------------

// new: T1 `cs_min`/`cs_max` propagate NaN (a `f64::max` would return the other operand)
#[test]
fn cs_min_and_cs_max_propagate_nan_through_compute() {
    let mut nan_layout = layout("both");
    nan_layout.max_vertical_offset = f64::NAN;
    let layout = compute(
        SizeD {
            width: 1000.0,
            height: 600.0,
        },
        &nan_layout,
        0.0,
        0.0,
        &ScrollBarMetrics::default(),
    );
    assert!(layout.vertical.max_offset.is_nan());
    assert!(layout.vertical.offset.is_nan());
}

// new: T1 `Math.Min(0.0, -0.0)` is `-0.0`, `Math.Max(0.0, -0.0)` is `+0.0`; the clamp of an offset of `-0.0` on a zero range is `+0.0`
#[test]
fn negative_zero_offsets_keep_the_c_sharp_sign() {
    let zero_range = ViewportLayout {
        viewport_width: 1000.0,
        viewport_height: 600.0,
        vertical_bar_visible: true,
        ..Default::default()
    };
    let layout = compute(
        SizeD {
            width: 1000.0,
            height: 600.0,
        },
        &zero_range,
        0.0,
        -0.0,
        &ScrollBarMetrics::default(),
    );
    assert_eq!(
        layout.vertical.offset.to_bits(),
        0.0_f64.to_bits(),
        "offset must be +0.0"
    );
}

// new: T2 `sanitize` maps NaN, negatives and infinities to `+0.0` (client, thickness and viewport length; not the margins)
#[test]
fn sanitize_maps_nan_negative_and_infinite_to_positive_zero() {
    // thickness NaN -> 0: the bar is 0 wide
    let metrics = ScrollBarMetrics {
        thickness: f64::NAN,
        ..ScrollBarMetrics::default()
    };
    let by_thickness = compute(
        SizeD {
            width: 1000.0,
            height: 600.0,
        },
        &layout("both"),
        0.0,
        0.0,
        &metrics,
    );
    assert_eq!(
        by_thickness.vertical.bar.width.to_bits(),
        0.0_f64.to_bits(),
        "NaN thickness must sanitise to +0.0"
    );

    // viewport length infinite, then NaN, -> 0
    for bad in [f64::INFINITY, f64::NAN, -5.0] {
        let mut infinite_viewport = layout("vertical_only");
        infinite_viewport.viewport_height = bad;
        let box_ = compute(
            SizeD {
                width: 1000.0,
                height: 600.0,
            },
            &infinite_viewport,
            0.0,
            0.0,
            &ScrollBarMetrics::default(),
        )
        .vertical;
        assert_eq!(
            box_.viewport_length.to_bits(),
            0.0_f64.to_bits(),
            "viewport {bad:?} must sanitise to +0.0"
        );
    }
}

// new: T6 `ScrollBarMetrics::default()` is the WPF set, not all zeros
#[test]
fn defaults_are_not_zero() {
    let metrics = ScrollBarMetrics::default();
    assert_ne!(metrics.thickness, 0.0);
    assert_ne!(metrics.min_thumb_length, 0.0);
    assert_eq!(DEFAULT_MIN_THUMB_LENGTH, metrics.min_thumb_length);
}

// new: T6 `ScrollBarLayout::none()` and `ScrollBarBox::hidden` are all-zero boxes with `visible = false`
#[test]
fn none_and_hidden_are_all_zero() {
    let none = ScrollBarLayout::none();
    assert!(!none.any_visible());
    assert!(!none.has_corner);
    assert!(!none.vertical.visible && !none.horizontal.visible);
    assert!(!none.vertical.thumb_visible && !none.horizontal.thumb_visible);
    assert_eq!(none.corner, RectD::default());

    let hidden = ScrollBarBox::hidden(ScrollAxis::Horizontal);
    assert_eq!(hidden.axis, ScrollAxis::Horizontal);
    assert!(!hidden.visible);
    assert_eq!(hidden.track_length(), 0.0);
    assert_eq!(hidden.thumb_length(), 0.0);
    assert_eq!(hidden.travel(), 0.0);
}

// new: `Inside` is half-open on the right and bottom edges, not `RectD::contains` (closed)
#[test]
fn inside_is_half_open_and_not_rect_contains() {
    let r = rect(10.0, 20.0, 30.0, 40.0);
    let right_edge = PointD { x: 40.0, y: 30.0 };
    assert!(!inside(r, right_edge));
    assert!(
        r.contains(right_edge),
        "RectD::contains is closed, the geometry must not use it"
    );
}

// new: the corner has priority over a bar rectangle that covers it
#[test]
fn hit_test_corner_has_priority() {
    let covering = ScrollBarBox {
        axis: ScrollAxis::Vertical,
        visible: true,
        bar: rect(0.0, 0.0, 100.0, 100.0),
        track: rect(1.0, 1.0, 98.0, 98.0),
        thumb: RectD::default(),
        thumb_visible: false,
        max_offset: 0.0,
        offset: 0.0,
        viewport_length: 0.0,
    };
    let layout = ScrollBarLayout {
        vertical: covering,
        horizontal: ScrollBarBox::hidden(ScrollAxis::Horizontal),
        corner: rect(0.0, 0.0, 100.0, 100.0),
        has_corner: true,
    };
    assert_eq!(hit_test(&layout, PointD { x: 50.0, y: 50.0 }), ScrollBarPart::Corner);
}

// new: the corner square lies in neither bar rectangle, so `axis_at` is `None` there
#[test]
fn axis_at_corner_is_none_when_bars_do_not_cover_it() {
    let layout = compute_for("both", 1000.0, 600.0, 0.0, 0.0);
    assert_eq!(axis_at(&layout, PointD { x: 995.0, y: 595.0 }), None);
}

// new: a NaN thumb start is not guarded in C#, so the offset is NaN
#[test]
fn offset_for_thumb_start_nan_thumb_start_gives_nan() {
    let box_ = compute_for("both", 1000.0, 600.0, 0.0, 0.0).vertical;
    assert!(offset_for_thumb_start(&box_, f64::NAN).is_nan());
}

// new: `box_for` returns the box of the axis
#[test]
fn box_for_returns_the_axis_box() {
    let layout = compute_for("both", 1000.0, 600.0, 0.0, 0.0);
    assert_eq!(layout.box_for(ScrollAxis::Vertical).axis, ScrollAxis::Vertical);
    assert_eq!(layout.box_for(ScrollAxis::Horizontal).axis, ScrollAxis::Horizontal);
    assert_eq!(*layout.box_for(ScrollAxis::Horizontal), layout.horizontal);
}

// new: the table above equals the engine layouts echoed in the golden (the golden wins when they differ), and the
// geometry shows a bar exactly where the engine layout says one is.
#[test]
fn literal_layouts_equal_the_engine_layouts_in_the_golden() {
    const IDS: [&str; 9] = [
        "both",
        "fit",
        "vertical_only",
        "horizontal_only",
        "second_bar",
        "overlay",
        "hidden",
        "tiny",
        "big_extent",
    ];
    let golden = golden::load("scrollbar-geometry");
    for id in IDS {
        let case = case_by_id(&golden.cases, id);
        let echo = field(case, "layout");
        let table = layout(id);
        let ctx = format!("literal layout {id}");
        assert!(
            same(table.viewport_width, num_of(echo, "viewport_width")),
            "{ctx}: viewport_width"
        );
        assert!(
            same(table.viewport_height, num_of(echo, "viewport_height")),
            "{ctx}: viewport_height"
        );
        assert_eq!(
            table.horizontal_bar_visible,
            bool_of(echo, "horizontal_bar_visible"),
            "{ctx}: horizontal flag"
        );
        assert_eq!(
            table.vertical_bar_visible,
            bool_of(echo, "vertical_bar_visible"),
            "{ctx}: vertical flag"
        );
        assert!(
            same(table.max_horizontal_offset, num_of(echo, "max_horizontal_offset")),
            "{ctx}: max_horizontal_offset"
        );
        assert!(
            same(table.max_vertical_offset, num_of(echo, "max_vertical_offset")),
            "{ctx}: max_vertical_offset"
        );

        let result = compute(client_of(case), &table, 0.0, 0.0, &ScrollBarMetrics::default());
        assert_eq!(
            result.vertical.visible, table.vertical_bar_visible,
            "{ctx}: vertical bar shown"
        );
        assert_eq!(
            result.horizontal.visible, table.horizontal_bar_visible,
            "{ctx}: horizontal bar shown"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden driver: tests/golden/scrollbar-geometry.json
// ---------------------------------------------------------------------------------------------------------------------

fn check_defaults(case: &Value, ctx: &str) {
    let metrics = ScrollBarMetrics::default();
    let expected = field(case, "metrics");
    check_key(metrics.thickness, expected, "thickness", ctx);
    check_key(metrics.track_margin_along, expected, "track_margin_along", ctx);
    check_key(metrics.track_margin_across, expected, "track_margin_across", ctx);
    check_key(metrics.min_thumb_length, expected, "min_thumb_length", ctx);
    check_key(metrics.thumb_radius, expected, "thumb_radius", ctx);
    check_num(DEFAULT_MIN_THUMB_LENGTH, field(case, "default_min_thumb_length"), ctx);
    check_layout(&ScrollBarLayout::none(), field(case, "none"), &format!("{ctx}: none"));
    check_box(
        &ScrollBarBox::hidden(ScrollAxis::Vertical),
        field(case, "hidden_vertical"),
        &format!("{ctx}: hidden_vertical"),
    );
    check_box(
        &ScrollBarBox::hidden(ScrollAxis::Horizontal),
        field(case, "hidden_horizontal"),
        &format!("{ctx}: hidden_horizontal"),
    );
}

fn check_compute(case: &Value, ctx: &str) {
    let echoed = layout_from(field(case, "layout"));
    let h_offset = case.get("h_offset").map_or(0.0, num);
    let v_offset = case.get("v_offset").map_or(0.0, num);
    let result = compute(client_of(case), &echoed, h_offset, v_offset, &metrics_of(case));

    check_layout(&result, field(case, "result"), ctx);

    for hit in field(case, "hits").as_array().unwrap_or_else(|| panic!("{ctx}: hits")) {
        let point = point_of(hit);
        let part = part_name(hit_test(&result, point));
        assert_eq!(
            part,
            str_of(hit, "part"),
            "{ctx}: hit at ({:?}, {:?})",
            point.x,
            point.y
        );
        let axis = axis_at(&result, point).map(axis_name);
        assert_eq!(
            axis,
            hit.get("axis").and_then(Value::as_str),
            "{ctx}: axis at ({:?}, {:?})",
            point.x,
            point.y
        );
    }

    for entry in field(case, "offsets")
        .as_array()
        .unwrap_or_else(|| panic!("{ctx}: offsets"))
    {
        let thumb_start = num_of(entry, "thumb_start");
        check_num(
            offset_for_thumb_start(&result.vertical, thumb_start),
            field(entry, "vertical"),
            &format!("{ctx}: vertical offset for thumb start {thumb_start:?}"),
        );
        check_num(
            offset_for_thumb_start(&result.horizontal, thumb_start),
            field(entry, "horizontal"),
            &format!("{ctx}: horizontal offset for thumb start {thumb_start:?}"),
        );
    }

    let round = field(case, "roundtrip");
    check_num(
        offset_for_thumb_start(&result.vertical, result.vertical.thumb_start()),
        field(round, "vertical"),
        &format!("{ctx}: vertical roundtrip"),
    );
    check_num(
        offset_for_thumb_start(&result.horizontal, result.horizontal.thumb_start()),
        field(round, "horizontal"),
        &format!("{ctx}: horizontal roundtrip"),
    );
}

fn check_box_helpers(case: &Value, ctx: &str) {
    let b = box_from(field(case, "box"), ScrollAxis::Vertical);
    let derived = field(case, "derived");
    check_key(b.track_start(), derived, "track_start", ctx);
    check_key(b.track_length(), derived, "track_length", ctx);
    check_key(b.thumb_start(), derived, "thumb_start", ctx);
    check_key(b.thumb_length(), derived, "thumb_length", ctx);
    check_key(b.travel(), derived, "travel", ctx);
    for entry in field(case, "offsets")
        .as_array()
        .unwrap_or_else(|| panic!("{ctx}: offsets"))
    {
        let thumb_start = num_of(entry, "thumb_start");
        check_num(
            offset_for_thumb_start(&b, thumb_start),
            field(entry, "offset"),
            &format!("{ctx}: offset for thumb start {thumb_start:?}"),
        );
    }
    for entry in field(case, "along")
        .as_array()
        .unwrap_or_else(|| panic!("{ctx}: along"))
    {
        check_num(
            b.along(point_of(entry)),
            field(entry, "value"),
            &format!("{ctx}: along"),
        );
    }
}

fn check_hit_test(case: &Value, ctx: &str) {
    let boxes = field(case, "boxes");
    let layout = ScrollBarLayout {
        vertical: box_from(field(boxes, "vertical"), ScrollAxis::Vertical),
        horizontal: box_from(field(boxes, "horizontal"), ScrollAxis::Horizontal),
        corner: opt_rect(boxes, "corner"),
        has_corner: opt_bool(boxes, "has_corner"),
    };
    for hit in field(case, "hits").as_array().unwrap_or_else(|| panic!("{ctx}: hits")) {
        let point = point_of(hit);
        assert_eq!(
            part_name(hit_test(&layout, point)),
            str_of(hit, "part"),
            "{ctx}: part at ({:?}, {:?})",
            point.x,
            point.y
        );
        assert_eq!(
            axis_at(&layout, point).map(axis_name),
            hit.get("axis").and_then(Value::as_str),
            "{ctx}: axis at ({:?}, {:?})",
            point.x,
            point.y
        );
    }
}

fn check_inside(case: &Value, ctx: &str) {
    let r = rect_of(field(case, "rect"));
    for result in field(case, "results")
        .as_array()
        .unwrap_or_else(|| panic!("{ctx}: results"))
    {
        let point = point_of(result);
        assert_eq!(
            inside(r, point),
            bool_of(result, "inside"),
            "{ctx}: inside at ({:?}, {:?})",
            point.x,
            point.y
        );
    }
}

// new: golden driver for every case of `tests/golden/scrollbar-geometry.json` (133 cases)
#[test]
fn golden_scrollbar_geometry() {
    let golden = golden::load("scrollbar-geometry");
    assert_eq!(golden.cases.len(), 133, "scrollbar-geometry golden case count");
    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "kind") {
            "defaults" => check_defaults(case, name),
            "compute" => check_compute(case, name),
            "box_helpers" => check_box_helpers(case, name),
            "hit_test" => check_hit_test(case, name),
            "inside" => check_inside(case, name),
            other => panic!("{name}: unknown kind {other:?}"),
        }
    }
}
