//! Red tests (W8-01b, test porter) for `pr_app::viewport::layout`: the 33 facts of the C# `ViewportLayoutEngineTests`
//! (`tests/PhotoReview.Shell.Tests/Viewport/ViewportLayoutEngineTests.cs` at baseline `d92d6aae`) plus the golden test over
//! `tests/golden/viewport-layout-engine.json` (361 cases, exact IEEE bits). Property tests live in
//! `viewport_layout_props.rs`.
//!
//! Every test fails through `todo!()` until the bodies land. Each test names the C# test it ports in a comment.
//! Assertions use an absolute tolerance of 1e-9 where the C# test uses `Assert.Equal(expected, actual, 9)`.
//! The golden test compares `f64::to_bits`, so the sign of zero and the last bit both count (trap 11).

use pr_app::viewport::layout::{clamp_offset, compute, wpf_double};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout};
use pr_test_support::golden::{self, str_of};
use serde_json::Value;

const BAR: f64 = 10.0;
const INF: f64 = f64::INFINITY;
const NAN: f64 = f64::NAN;

/// `Fit(cw, ch, bitmapW, bitmapH)` (C# 13-14): Uniform, MaxImage = client, Width/Height auto.
fn fit(cw: f64, ch: f64, bitmap_w: f64, bitmap_h: f64) -> ViewportInput {
    ViewportInput {
        client_width: cw,
        client_height: ch,
        scroll_bar_thickness: BAR,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::Uniform,
        image_width: NAN,
        image_height: NAN,
        max_image_width: cw,
        max_image_height: ch,
        bitmap_width: bitmap_w,
        bitmap_height: bitmap_h,
    }
}

/// `Zoomed(cw, ch, w, h, policy)` (C# 16-17): Stretch None, explicit Width/Height, no MaxImage, bitmap = max(1, w/4) x max(1, h/4).
fn zoomed_with(cw: f64, ch: f64, w: f64, h: f64, policy: ScrollBarPolicy) -> ViewportInput {
    ViewportInput {
        client_width: cw,
        client_height: ch,
        scroll_bar_thickness: BAR,
        scroll_bars: policy,
        stretch: ViewerStretchMode::None,
        image_width: w,
        image_height: h,
        max_image_width: INF,
        max_image_height: INF,
        bitmap_width: (w / 4.0).max(1.0),
        bitmap_height: (h / 4.0).max(1.0),
    }
}

fn zoomed(cw: f64, ch: f64, w: f64, h: f64) -> ViewportInput {
    zoomed_with(cw, ch, w, h, ScrollBarPolicy::Auto)
}

fn assert_close(actual: f64, expected: f64, context: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{context}: expected {expected}, got {actual}"
    );
}

/// C# `AssertLayout` (ViewportLayoutEngineTests 35-50). Same argument order as the C# helper.
#[allow(clippy::too_many_arguments)] // mirrors the C# AssertLayout signature so each fact reads like its source
fn assert_layout(
    layout: &ViewportLayout,
    vw: f64,
    vh: f64,
    ew: f64,
    eh: f64,
    h_bar: bool,
    v_bar: bool,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    max_h: f64,
    max_v: f64,
) {
    assert_close(layout.viewport_width, vw, "viewport_width");
    assert_close(layout.viewport_height, vh, "viewport_height");
    assert_close(layout.extent_width, ew, "extent_width");
    assert_close(layout.extent_height, eh, "extent_height");
    assert_eq!(layout.horizontal_bar_visible, h_bar, "horizontal_bar_visible");
    assert_eq!(layout.vertical_bar_visible, v_bar, "vertical_bar_visible");
    assert_close(layout.image_rect.x, x, "image_rect.x");
    assert_close(layout.image_rect.y, y, "image_rect.y");
    assert_close(layout.image_rect.width, w, "image_rect.width");
    assert_close(layout.image_rect.height, h, "image_rect.height");
    assert_close(layout.max_horizontal_offset, max_h, "max_horizontal_offset");
    assert_close(layout.max_vertical_offset, max_v, "max_vertical_offset");
}

// ---- Fit (Stretch=Uniform, MaxWidth/MaxHeight = vùng client) ----

/// C# `Fit_Landscape_FillsTheHeightAndIsCentredHorizontally`
#[test]
fn fit_landscape_fills_the_height_and_is_centred_horizontally() {
    // 1500x1000 vào 1280x720: tỉ lệ min(0,8533; 0,72) = 0,72 -> 1080x720, lề trái (1280-1080)/2.
    assert_layout(
        &compute(&fit(1280.0, 720.0, 1500.0, 1000.0)),
        1280.0,
        720.0,
        1080.0,
        720.0,
        false,
        false,
        100.0,
        0.0,
        1080.0,
        720.0,
        0.0,
        0.0,
    );
}

/// C# `Fit_Portrait_FillsTheHeightAndIsCentredHorizontally`
#[test]
fn fit_portrait_fills_the_height_and_is_centred_horizontally() {
    assert_layout(
        &compute(&fit(1280.0, 720.0, 1000.0, 1500.0)),
        1280.0,
        720.0,
        480.0,
        720.0,
        false,
        false,
        400.0,
        0.0,
        480.0,
        720.0,
        0.0,
        0.0,
    );
}

/// C# `Fit_Panorama_FillsTheWidthAndIsCentredVertically`
#[test]
fn fit_panorama_fills_the_width_and_is_centred_vertically() {
    let layout = compute(&fit(800.0, 600.0, 3000.0, 250.0));
    let h = 250.0 * (800.0 / 3000.0);
    assert_layout(
        &layout,
        800.0,
        600.0,
        800.0,
        h,
        false,
        false,
        0.0,
        (600.0 - h) / 2.0,
        800.0,
        h,
        0.0,
        0.0,
    );
}

/// C# `Fit_SmallBitmap_IsEnlargedToFill_StretchDirectionBoth`
#[test]
fn fit_small_bitmap_is_enlarged_to_fill_stretch_direction_both() {
    assert_layout(
        &compute(&fit(800.0, 600.0, 100.0, 75.0)),
        800.0,
        600.0,
        800.0,
        600.0,
        false,
        false,
        0.0,
        0.0,
        800.0,
        600.0,
        0.0,
        0.0,
    );
}

/// C# `Fit_NeverShowsScrollBars_EvenWhenTheImageExactlyFillsTheClient`
#[test]
fn fit_never_shows_scroll_bars_even_when_the_image_exactly_fills_the_client() {
    let layout = compute(&fit(1600.0, 900.0, 1600.0, 900.0));
    assert!(!layout.horizontal_bar_visible);
    assert!(!layout.vertical_bar_visible);
    assert_close(layout.viewport_width, 1600.0, "viewport_width");
}

/// C# `Uniform_WithoutABound_MeasuresToTheBitmapsNaturalSize_AndScrolls`
#[test]
fn uniform_without_a_bound_measures_to_the_natural_size_and_scrolls() {
    // ViewerState trước lần UpdateViewport đầu: Uniform, MaxImage = vô hạn -> ảnh đo bằng kích thước tự nhiên.
    let input = ViewportInput {
        max_image_width: INF,
        max_image_height: INF,
        ..fit(1000.0, 600.0, 1500.0, 900.0)
    };
    assert_layout(
        &compute(&input),
        990.0,
        590.0,
        1500.0,
        900.0,
        true,
        true,
        0.0,
        0.0,
        1500.0,
        900.0,
        510.0,
        310.0,
    );
}

/// C# `Uniform_BoundedInWidthOnly_ScalesBothAxesByTheWidth`
#[test]
fn uniform_bounded_in_width_only_scales_both_axes_by_the_width() {
    let input = ViewportInput {
        max_image_width: 800.0,
        max_image_height: INF,
        ..fit(1000.0, 600.0, 1500.0, 900.0)
    };
    // 800/1500 cho cả hai trục: 800x480; ô = max(extent, viewport) = 1000x600 -> căn giữa.
    assert_layout(
        &compute(&input),
        1000.0,
        600.0,
        800.0,
        480.0,
        false,
        false,
        100.0,
        60.0,
        800.0,
        480.0,
        0.0,
        0.0,
    );
}

/// C# `Uniform_BoundedInHeightOnly_ScalesBothAxesByTheHeight`
#[test]
fn uniform_bounded_in_height_only_scales_both_axes_by_the_height() {
    let input = ViewportInput {
        max_image_width: INF,
        max_image_height: 300.0,
        ..fit(1000.0, 600.0, 1500.0, 900.0)
    };
    assert_layout(
        &compute(&input),
        1000.0,
        600.0,
        500.0,
        300.0,
        false,
        false,
        250.0,
        150.0,
        500.0,
        300.0,
        0.0,
        0.0,
    );
}

/// C# `Fit_WithoutABitmap_IsAnEmptyElementAtTheCentre`
#[test]
fn fit_without_a_bitmap_is_an_empty_element_at_the_centre() {
    assert_layout(
        &compute(&fit(1000.0, 600.0, 0.0, 0.0)),
        1000.0,
        600.0,
        0.0,
        0.0,
        false,
        false,
        500.0,
        300.0,
        0.0,
        0.0,
        0.0,
        0.0,
    );
}

// ---- Zoom (Stretch=None -> Fill, Width/Height tường minh) ----

/// C# `Zoomed_LargerThanTheClient_ShowsBothBars_WhichTakeTheirThicknessFromTheViewport`
#[test]
fn zoomed_larger_than_the_client_shows_both_bars_taking_their_thickness() {
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 3000.0, 2000.0)),
        990.0,
        590.0,
        3000.0,
        2000.0,
        true,
        true,
        0.0,
        0.0,
        3000.0,
        2000.0,
        2010.0,
        1410.0,
    );
}

/// C# `Zoomed_WiderOnly_ShowsTheHorizontalBar_AndCentresVerticallyInTheShorterViewport`
#[test]
fn zoomed_wider_only_shows_the_horizontal_bar_and_centres_vertically() {
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 2000.0, 300.0)),
        1000.0,
        590.0,
        2000.0,
        300.0,
        true,
        false,
        0.0,
        145.0,
        2000.0,
        300.0,
        1000.0,
        0.0,
    );
}

/// C# `Zoomed_TallerOnly_ShowsTheVerticalBar_AndCentresHorizontallyInTheNarrowerViewport`
#[test]
fn zoomed_taller_only_shows_the_vertical_bar_and_centres_horizontally() {
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 500.0, 2000.0)),
        990.0,
        600.0,
        500.0,
        2000.0,
        false,
        true,
        245.0,
        0.0,
        500.0,
        2000.0,
        0.0,
        1400.0,
    );
}

/// C# `Zoomed_SmallerThanTheClient_IsCentred_WithoutBars`
#[test]
fn zoomed_smaller_than_the_client_is_centred_without_bars() {
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 400.0, 300.0)),
        1000.0,
        600.0,
        400.0,
        300.0,
        false,
        false,
        300.0,
        150.0,
        400.0,
        300.0,
        0.0,
        0.0,
    );
}

// ---- Lượt đo 3 (chỉ thanh bị lệch được tính lại) ----

/// C# `ThirdMeasurePass_TheVerticalBarMakesTheWidthOverflow_SoTheHorizontalBarAppearsToo`
#[test]
fn third_pass_vertical_bar_makes_width_overflow_so_horizontal_appears() {
    // Lượt 1: 995 <= 1000 (không thanh ngang), 2000 > 600 (thanh dọc). Lượt 3: 995 > 1000 - 10 -> thanh ngang.
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 995.0, 2000.0)),
        990.0,
        590.0,
        995.0,
        2000.0,
        true,
        true,
        0.0,
        0.0,
        995.0,
        2000.0,
        5.0,
        1410.0,
    );
}

/// C# `ThirdMeasurePass_TheHorizontalBarMakesTheHeightOverflow_SoTheVerticalBarAppearsToo`
#[test]
fn third_pass_horizontal_bar_makes_height_overflow_so_vertical_appears() {
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 2000.0, 595.0)),
        990.0,
        590.0,
        2000.0,
        595.0,
        true,
        true,
        0.0,
        0.0,
        2000.0,
        595.0,
        1010.0,
        5.0,
    );
}

/// C# `ThirdMeasurePass_ExactlyFillingTheNarrowedViewport_DoesNotOverflow`
#[test]
fn third_pass_exactly_filling_the_narrowed_viewport_does_not_overflow() {
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 990.0, 2000.0)),
        990.0,
        600.0,
        990.0,
        2000.0,
        false,
        true,
        0.0,
        0.0,
        990.0,
        2000.0,
        0.0,
        1400.0,
    );
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 2000.0, 590.0)),
        1000.0,
        590.0,
        2000.0,
        590.0,
        true,
        false,
        0.0,
        0.0,
        2000.0,
        590.0,
        1000.0,
        0.0,
    );
}

/// C# `Overflow_UsesWpfDoubleUtil_AFloatingPointHairOverTheClientIsNotAnOverflow`
#[test]
fn overflow_uses_wpf_double_util_a_float_hair_is_not_an_overflow() {
    // FitWidth: ảnh = px * (cw * dpi / px) / dpi có thể lệch cw ~1e-13; WPF (AreClose) không hiện thanh.
    let hair = compute(&zoomed(1000.0, 600.0, 1000.0 + 1e-13, 600.0));
    assert!(!hair.horizontal_bar_visible);
    assert!(!hair.vertical_bar_visible);

    let real = compute(&zoomed(1000.0, 600.0, 1000.001, 600.0));
    assert!(real.horizontal_bar_visible);
    assert_close(real.viewport_height, 590.0, "viewport_height");
}

/// C# `ClientThinnerThanABar_LeavesAnEmptyViewport_NotANegativeOne`
#[test]
fn client_thinner_than_a_bar_leaves_an_empty_viewport_not_negative() {
    assert_layout(
        &compute(&zoomed(6.0, 6.0, 50.0, 50.0)),
        0.0,
        0.0,
        50.0,
        50.0,
        true,
        true,
        0.0,
        0.0,
        50.0,
        50.0,
        50.0,
        50.0,
    );
}

/// C# `ExplicitSize_WithoutABitmap_KeepsTheExtent_ButDrawsNothing`
#[test]
fn explicit_size_without_a_bitmap_keeps_the_extent_but_draws_nothing() {
    let input = ViewportInput {
        bitmap_width: 0.0,
        bitmap_height: 0.0,
        ..zoomed(1000.0, 600.0, 2000.0, 1500.0)
    };
    // Extent = Width/Height (MinMax), ActualWidth = 0, căn giữa trong ô 2000x1500.
    assert_layout(
        &compute(&input),
        990.0,
        590.0,
        2000.0,
        1500.0,
        true,
        true,
        1000.0,
        750.0,
        0.0,
        0.0,
        1010.0,
        910.0,
    );
}

/// C# `Fill_WithAnAutoSize_MeasuresNaturally_ThenStretchesToTheViewport`
#[test]
fn fill_with_an_auto_size_measures_naturally_then_stretches() {
    // Stretch=None khi kích thước nguồn chưa biết (ImageWidth = NaN): Fill đo theo bitmap rồi giãn ra theo ô.
    let input = ViewportInput {
        client_width: 1000.0,
        client_height: 600.0,
        scroll_bar_thickness: BAR,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::None,
        image_width: NAN,
        image_height: NAN,
        max_image_width: INF,
        max_image_height: INF,
        bitmap_width: 400.0,
        bitmap_height: 300.0,
    };
    assert_layout(
        &compute(&input),
        1000.0,
        600.0,
        400.0,
        300.0,
        false,
        false,
        0.0,
        0.0,
        1000.0,
        600.0,
        0.0,
        0.0,
    );
}

/// C# `MaxSmallerThanAnExplicitSize_ClampsTheElement`
#[test]
fn max_smaller_than_an_explicit_size_clamps_the_element() {
    let input = ViewportInput {
        max_image_width: 500.0,
        max_image_height: 400.0,
        ..zoomed(1000.0, 600.0, 2000.0, 1500.0)
    };
    assert_layout(
        &compute(&input),
        1000.0,
        600.0,
        500.0,
        400.0,
        false,
        false,
        250.0,
        100.0,
        500.0,
        400.0,
        0.0,
        0.0,
    );
}

// ---- chính sách thanh cuộn (NE-3, chỉ bản Win32) ----

/// C# `OverlayBars_AreShownOnOverflow_ButTakeNoSpace`
#[test]
fn overlay_bars_are_shown_on_overflow_but_take_no_space() {
    assert_layout(
        &compute(&zoomed_with(1000.0, 600.0, 995.0, 2000.0, ScrollBarPolicy::Overlay)),
        1000.0,
        600.0,
        995.0,
        2000.0,
        false,
        true,
        2.5,
        0.0,
        995.0,
        2000.0,
        0.0,
        1400.0,
    );
}

/// C# `HiddenBars_AreNeverShown_TheContentStillScrolls`
#[test]
fn hidden_bars_are_never_shown_the_content_still_scrolls() {
    assert_layout(
        &compute(&zoomed_with(1000.0, 600.0, 3000.0, 2000.0, ScrollBarPolicy::Hidden)),
        1000.0,
        600.0,
        3000.0,
        2000.0,
        false,
        false,
        0.0,
        0.0,
        3000.0,
        2000.0,
        2000.0,
        1400.0,
    );
}

// ---- input không hợp lệ ----

/// C# `InvalidClientSize_ReadsAsZero` (`[Theory]`, rows NaN, -5, +inf).
#[test]
fn invalid_client_size_reads_as_zero() {
    for bad in [NAN, -5.0, INF] {
        let input = ViewportInput {
            scroll_bar_thickness: bad,
            ..zoomed(bad, bad, 400.0, 300.0)
        };
        let layout = compute(&input);
        assert_close(layout.viewport_width, 0.0, &format!("viewport_width for client {bad}"));
        assert_close(
            layout.viewport_height,
            0.0,
            &format!("viewport_height for client {bad}"),
        );
        assert!(layout.horizontal_bar_visible, "horizontal bar for client {bad}");
        assert_close(
            layout.max_horizontal_offset,
            400.0,
            &format!("max_horizontal_offset for client {bad}"),
        );
    }
}

/// C# `InvalidElementSizes_FallBackToWpfsValidatedMeaning`
#[test]
fn invalid_element_sizes_fall_back_to_wpf_validated_meaning() {
    // Width vô hạn/âm -> auto (NaN); MaxWidth NaN/âm -> vô hạn: cùng kết quả với ca Fill auto-size.
    let input = ViewportInput {
        client_width: 1000.0,
        client_height: 600.0,
        scroll_bar_thickness: BAR,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::None,
        image_width: INF,
        image_height: -1.0,
        max_image_width: NAN,
        max_image_height: -3.0,
        bitmap_width: 400.0,
        bitmap_height: 300.0,
    };
    assert_layout(
        &compute(&input),
        1000.0,
        600.0,
        400.0,
        300.0,
        false,
        false,
        0.0,
        0.0,
        1000.0,
        600.0,
        0.0,
        0.0,
    );
    let no_bitmap = ViewportInput {
        bitmap_width: NAN,
        bitmap_height: -1.0,
        ..input
    };
    assert_close(
        compute(&no_bitmap).extent_width,
        0.0,
        "extent_width without a valid bitmap",
    );
}

// ---- ClampOffset (ScrollContentPresenter.CoerceOffset) ----

/// C# `ClampOffset_KeepsInRangeOffsets_AndClampsBothEnds`
#[test]
fn clamp_offset_keeps_in_range_offsets_and_clamps_both_ends() {
    let layout = compute(&zoomed(1000.0, 600.0, 3000.0, 2000.0));
    assert_eq!(clamp_offset(&layout, 123.5, 77.25), (123.5, 77.25));
    assert_eq!(clamp_offset(&layout, 1e9, 1e9), (2010.0, 1410.0));
    assert_eq!(clamp_offset(&layout, -1.0, -1e9), (0.0, 0.0));
    assert_eq!(clamp_offset(&layout, 2010.0, 0.0), (2010.0, 0.0));
}

/// C# `ClampOffset_ContentSmallerThanTheViewport_IsAlwaysZero`
#[test]
fn clamp_offset_content_smaller_than_the_viewport_is_always_zero() {
    let layout = compute(&zoomed(1000.0, 600.0, 400.0, 300.0));
    assert_eq!(clamp_offset(&layout, 50.0, 50.0), (0.0, 0.0));
}

/// C# `ClampOffset_NaN_ReadsAsZero`
#[test]
fn clamp_offset_nan_reads_as_zero() {
    let layout = compute(&zoomed(1000.0, 600.0, 3000.0, 2000.0));
    assert_eq!(clamp_offset(&layout, NAN, 15.0), (0.0, 15.0));
    assert_eq!(clamp_offset(&layout, 15.0, NAN), (15.0, 0.0));
}

// ---- DoubleUtil ----

/// C# `DoubleUtil_MatchesWpf`
#[test]
fn wpf_double_util_matches_wpf() {
    assert!(wpf_double::are_close(1000.0, 1000.0 + 1e-13));
    assert!(!wpf_double::are_close(1000.0, 1000.000001));
    assert!(wpf_double::are_close(INF, INF));
    // eps = (|a| + |b| + 10) * DBL_EPSILON: biên của khoảng mở.
    let eps = (1.0 + 1.0 + 10.0) * wpf_double::WPF_DOUBLE_EPSILON;
    assert!(!wpf_double::are_close(1.0 + (2.0 * eps), 1.0));
    assert!(!wpf_double::are_close(1.0, 1.0 + (2.0 * eps)));
    assert!(wpf_double::greater_than(2.0, 1.0));
    assert!(!wpf_double::greater_than(1.0, 2.0));
    assert!(!wpf_double::greater_than(1.0 + 1e-15, 1.0));
    assert!(wpf_double::is_zero(1e-15));
    assert!(wpf_double::is_zero(-1e-15));
    assert!(!wpf_double::is_zero(1e-14));
}

/// C# `DoubleUtil_IsZero_IsAStrictLessThan_AtTenEpsilonItIsNotZero`
#[test]
fn wpf_double_is_zero_is_strict_less_than_at_ten_epsilon() {
    // WPF: Math.Abs(value) < 10.0 * DBL_EPSILON.
    let ten = 10.0 * wpf_double::WPF_DOUBLE_EPSILON;
    assert!(!wpf_double::is_zero(ten));
    assert!(!wpf_double::is_zero(-ten));
}

/// C# `DoubleUtil_AreClose_BothEdgesOfTheOpenIntervalAreNotClose`
#[test]
fn wpf_double_are_close_both_edges_of_the_open_interval_are_not_close() {
    // 2^52 - 4 và 2^52 - 6 đều biểu diễn chính xác: eps = 2 và delta = +/-2 đúng bằng eps -> bất đẳng thức chặt, false.
    const A: f64 = 4503599627370492.0;
    const B: f64 = 4503599627370490.0;
    assert!(!wpf_double::are_close(A, B));
    assert!(!wpf_double::are_close(B, A));
    assert!(wpf_double::greater_than(A, B));
}

// ---- kích thước tường minh bằng 0 là hợp lệ (WPF: Width/MaxWidth >= 0) ----

/// C# `ExplicitWidthZero_IsAValidZeroWidth_NotAutoSize`
#[test]
fn explicit_width_zero_is_a_valid_zero_width_not_auto_size() {
    assert_layout(
        &compute(&zoomed(1000.0, 600.0, 0.0, 1500.0)),
        990.0,
        600.0,
        0.0,
        1500.0,
        false,
        true,
        495.0,
        0.0,
        0.0,
        1500.0,
        0.0,
        900.0,
    );
}

/// C# `MaxWidthZero_IsAValidBound_NotUnbounded`
#[test]
fn max_width_zero_is_a_valid_bound_not_unbounded() {
    let input = ViewportInput {
        max_image_width: 0.0,
        ..zoomed(1000.0, 600.0, 2000.0, 1500.0)
    };
    assert_layout(
        &compute(&input),
        990.0,
        600.0,
        0.0,
        1500.0,
        false,
        true,
        495.0,
        0.0,
        0.0,
        1500.0,
        0.0,
        900.0,
    );
}

// ---- Golden: tests/golden/viewport-layout-engine.json (oracle `viewport-layout-engine`, oracle/W8-01b.cs) ----

/// The five `clamp` probes of every golden case (oracle header: `[[0,0],[1e9,1e9],[-1,-1e9],["NaN",15],[123.5,77.25]]`).
const PROBES: [(f64, f64); 5] = [(0.0, 0.0), (1e9, 1e9), (-1.0, -1e9), (NAN, 15.0), (123.5, 77.25)];

/// Golden number: a JSON number, or one of the strings of the oracle convention ("NaN", "Infinity", "-Infinity", "-0").
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => NAN,
            "Infinity" => INF,
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

fn bool_field(object: &Value, key: &str) -> bool {
    field(object, key)
        .as_bool()
        .unwrap_or_else(|| panic!("'{key}' is not a bool in {object}"))
}

fn scroll_bars_of(name: &str) -> ScrollBarPolicy {
    match name {
        "Auto" => ScrollBarPolicy::Auto,
        "Overlay" => ScrollBarPolicy::Overlay,
        "Hidden" => ScrollBarPolicy::Hidden,
        other => panic!("unknown scroll bar policy {other:?}"),
    }
}

fn stretch_of(name: &str) -> ViewerStretchMode {
    match name {
        "None" => ViewerStretchMode::None,
        "Uniform" => ViewerStretchMode::Uniform,
        other => panic!("unknown stretch mode {other:?}"),
    }
}

fn input_of(value: &Value) -> ViewportInput {
    ViewportInput {
        client_width: num_of(value, "client_width"),
        client_height: num_of(value, "client_height"),
        scroll_bar_thickness: num_of(value, "scroll_bar_thickness"),
        scroll_bars: scroll_bars_of(str_of(value, "scroll_bars")),
        stretch: stretch_of(str_of(value, "stretch")),
        image_width: num_of(value, "image_width"),
        image_height: num_of(value, "image_height"),
        max_image_width: num_of(value, "max_image_width"),
        max_image_height: num_of(value, "max_image_height"),
        bitmap_width: num_of(value, "bitmap_width"),
        bitmap_height: num_of(value, "bitmap_height"),
    }
}

/// `{ "bits": "<16 hex>", "text": "<R>" }` -> the exact IEEE bit pattern.
fn expected_bits(value: &Value) -> u64 {
    let text = value["bits"].as_str().unwrap_or_else(|| panic!("no 'bits' in {value}"));
    u64::from_str_radix(text, 16).unwrap_or_else(|e| panic!("bad bits {text:?}: {e}"))
}

fn check_bits(actual: f64, expected: &Value, context: &str) {
    let want = expected_bits(expected);
    assert_eq!(
        actual.to_bits(),
        want,
        "{context}: expected bits {want:016x} ({:?}), got {actual:?} ({:016x})",
        f64::from_bits(want),
        actual.to_bits()
    );
}

/// Every golden case, bit-exact on each layout field and on each of the five clamp probes.
/// The golden stores `image_x/y/width/height`, which are the fields of `ViewportLayout::image_rect`.
#[test]
fn engine_reproduces_the_oracle_golden_bit_for_bit() {
    let golden = golden::load("viewport-layout-engine");
    assert_eq!(golden.cases.len(), 361, "viewport-layout-engine golden case count");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let layout = compute(&input_of(field(case, "input")));
        let expected = field(case, "layout");

        check_bits(
            layout.viewport_width,
            field(expected, "viewport_width"),
            &format!("{name} viewport_width"),
        );
        check_bits(
            layout.viewport_height,
            field(expected, "viewport_height"),
            &format!("{name} viewport_height"),
        );
        check_bits(
            layout.extent_width,
            field(expected, "extent_width"),
            &format!("{name} extent_width"),
        );
        check_bits(
            layout.extent_height,
            field(expected, "extent_height"),
            &format!("{name} extent_height"),
        );
        assert_eq!(
            layout.horizontal_bar_visible,
            bool_field(expected, "horizontal_bar_visible"),
            "{name} horizontal_bar_visible"
        );
        assert_eq!(
            layout.vertical_bar_visible,
            bool_field(expected, "vertical_bar_visible"),
            "{name} vertical_bar_visible"
        );
        check_bits(
            layout.image_rect.x,
            field(expected, "image_x"),
            &format!("{name} image_x"),
        );
        check_bits(
            layout.image_rect.y,
            field(expected, "image_y"),
            &format!("{name} image_y"),
        );
        check_bits(
            layout.image_rect.width,
            field(expected, "image_width"),
            &format!("{name} image_width"),
        );
        check_bits(
            layout.image_rect.height,
            field(expected, "image_height"),
            &format!("{name} image_height"),
        );
        check_bits(
            layout.max_horizontal_offset,
            field(expected, "max_horizontal_offset"),
            &format!("{name} max_horizontal_offset"),
        );
        check_bits(
            layout.max_vertical_offset,
            field(expected, "max_vertical_offset"),
            &format!("{name} max_vertical_offset"),
        );

        let probes = field(case, "clamp")
            .as_array()
            .unwrap_or_else(|| panic!("{name}: 'clamp' is not an array"));
        assert_eq!(probes.len(), PROBES.len(), "{name}: clamp probe count");
        for (index, (probe, expected_probe)) in PROBES.iter().zip(probes).enumerate() {
            let (h, v) = clamp_offset(&layout, probe.0, probe.1);
            check_bits(
                h,
                field(expected_probe, "horizontal"),
                &format!("{name} clamp[{index}] horizontal"),
            );
            check_bits(
                v,
                field(expected_probe, "vertical"),
                &format!("{name} clamp[{index}] vertical"),
            );
        }
    }
}
