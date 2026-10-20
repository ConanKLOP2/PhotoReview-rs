// Each integration binary uses a different subset of the shared fixture.
#[allow(dead_code)]
#[path = "common/pointer_fixture.rs"]
mod pointer_fixture;

use pointer_fixture::{Fixture, point};
use pr_app::input::pointer::PointerStep;
use pr_core::model::enums::{FitWidthAnchor, InitialViewMode, KeyboardZoomAnchor};

fn fit_width(f: &mut Fixture, anchor: FitWidthAnchor) {
    f.run(|c, ctx| c.fit_width(ctx, anchor));
}
fn fit_height(f: &mut Fixture) {
    f.run(|c, ctx| c.fit_height(ctx));
}
fn initial(f: &mut Fixture, mode: InitialViewMode, percent: i32) {
    f.run(|c, ctx| c.apply_initial_view(ctx, mode, percent));
}
fn source(f: &mut Fixture, width: i32, height: i32) {
    f.viewer.set_source_size(width, height, false);
}
fn pending_swap(f: &mut Fixture, width: i32, height: i32) {
    let controller = &mut f.controller;
    let surface = &f.surface;
    let versions = &f.versions;
    f.viewer.swap_source_size(width, height, &mut |viewer| {
        controller.on_source_size_swapping(viewer, surface, versions);
    });
}
fn drive_swap(f: &mut Fixture) {
    if let Some(next) = f.controller.take_pending_continuation() {
        f.yields += 1;
        let step = f.call(|controller, ctx| controller.resume(ctx, next));
        f.drive(step);
    }
}

// C#: PointerInputControllerTests.ZoomAtPoint_SupersededByALaterOperation_DoesNotScrollAfterIt
#[test]
fn zoom_at_point_superseded_by_a_later_operation_does_not_scroll_after_it() {
    let mut f = Fixture::new();
    let step = f.call(|c, ctx| c.on_wheel_vertical(ctx, 120, false, point(400.0, 300.0)));
    assert_eq!(f.surface.scrolls.len(), 1);
    f.versions.next_version();
    f.surface.viewport_width = 700.0;
    f.drive(step);
    assert_eq!(f.surface.scrolls.len(), 1);
}

// C#: PointerInputControllerTests.ZoomAtPoint_PlacesTheScrollBeforeTheRenderPass_AndOnlyAgainIfTheLayoutChanged
#[test]
fn zoom_at_point_places_the_scroll_before_the_render_pass_and_only_again_if_the_layout_changed() {
    let mut f = Fixture::new();
    let step = f.call(|c, ctx| c.zoom_in(ctx));
    assert!(matches!(step, PointerStep::AwaitRender(_)));
    assert_eq!(f.surface.scrolls.len(), 1);
    f.drive(step);
    assert_eq!(f.surface.scrolls.len(), 1);
    let step = f.call(|c, ctx| c.zoom_in(ctx));
    assert_eq!(f.surface.scrolls.len(), 2);
    f.surface.extent_width += 40.0;
    f.drive(step);
    assert_eq!(f.surface.scrolls.len(), 3);
}

// C#: PointerInputControllerTests.FitWidthAsync_UsesTheConfiguredAnchor_DefaultCentre
#[test]
fn fit_width_uses_the_configured_anchor_default_centre() {
    let mut f = Fixture::new();
    source(&mut f, 2000, 4000);
    let anchor = f.settings.fit_width_anchor;
    fit_width(&mut f, anchor);
    assert!(!f.viewer.is_fit());
    assert!((f.viewer.zoom() - 0.4).abs() < 1e-6);
    assert_eq!(f.surface.scrolls.len(), 1);
}

fn fit_vertical(anchor: FitWidthAnchor) -> f64 {
    let mut f = Fixture::new();
    f.surface.extent_height = 1200.0;
    source(&mut f, 2000, 4000);
    fit_width(&mut f, anchor);
    f.surface.scrolls.last().map_or(f64::NAN, |s| s.1)
}

// C#: PointerInputControllerTests.FitWidthAsync_TopThirdAnchor_ScrollsLessFarThanCentre
#[test]
fn fit_width_top_third_anchor_scrolls_less_far_than_centre() {
    assert!(fit_vertical(FitWidthAnchor::TopThird) < fit_vertical(FitWidthAnchor::Centre));
}

// C#: PointerInputControllerTests.FitWidthAsync_BottomThirdAnchor_ScrollsFurtherThanCentre_AndTheAnchorIsTheParameterNotTheSetting
#[test]
fn fit_width_bottom_third_anchor_scrolls_further_than_centre_and_the_anchor_is_the_parameter_not_the_setting() {
    let mut f = Fixture::new();
    f.settings.fit_width_anchor = FitWidthAnchor::Centre;
    f.surface.extent_height = 1200.0;
    source(&mut f, 2000, 4000);
    fit_width(&mut f, FitWidthAnchor::BottomThird);
    assert!(
        f.surface
            .scrolls
            .last()
            .is_some_and(|s| s.1 > fit_vertical(FitWidthAnchor::Centre))
    );
}

// C#: PointerInputControllerTests.FitWidthAsync_IgnoresTheKeyboardZoomAnchorSetting
#[test]
fn fit_width_ignores_the_keyboard_zoom_anchor_setting() {
    let mut a = Fixture::new();
    a.surface.extent_height = 1200.0;
    source(&mut a, 2000, 4000);
    a.surface.pointer = Some(point(100.0, 500.0));
    a.settings.keyboard_zoom_anchor = Some(KeyboardZoomAnchor::ViewportCentre);
    fit_width(&mut a, FitWidthAnchor::Centre);
    let mut b = Fixture::new();
    b.surface.extent_height = 1200.0;
    source(&mut b, 2000, 4000);
    b.surface.pointer = Some(point(100.0, 500.0));
    b.settings.keyboard_zoom_anchor = Some(KeyboardZoomAnchor::Pointer);
    fit_width(&mut b, FitWidthAnchor::Centre);
    assert_eq!(a.surface.scrolls.last(), b.surface.scrolls.last());
}

// C#: PointerInputControllerTests.FitHeightAsync_AlwaysAnchorsAtImageCentre
#[test]
fn fit_height_always_anchors_at_image_centre() {
    let mut f = Fixture::new();
    source(&mut f, 4000, 2000);
    fit_height(&mut f);
    assert!(!f.viewer.is_fit());
    assert!((f.viewer.zoom() - 0.3).abs() < 1e-6);
    assert_eq!(f.surface.scrolls.len(), 1);
}

// C#: PointerInputControllerTests.FitWidthAsync_VerticalScrollbarAppearsAsSideEffect_ReappliesOnceMore
#[test]
fn fit_width_vertical_scrollbar_appears_as_side_effect_reapplies_once_more() {
    let mut f = Fixture::new();
    source(&mut f, 2000, 4000);
    f.surface.extent_width = 800.0;
    let anchor = f.settings.fit_width_anchor;
    let step = f.call(|c, ctx| c.fit_width(ctx, anchor));
    f.surface.viewport_width = 780.0;
    f.drive(step);
    assert_eq!(f.yields, 2);
    assert_eq!(f.surface.scrolls.len(), 3);
}

// C#: PointerInputControllerTests.FitWidthAsync_NoScrollbarSideEffect_DoesNotReapply
#[test]
fn fit_width_no_scrollbar_side_effect_does_not_reapply() {
    let mut f = Fixture::new();
    source(&mut f, 2000, 4000);
    f.surface.extent_width = 800.0;
    fit_width(&mut f, FitWidthAnchor::Centre);
    assert_eq!(f.yields, 1);
    assert_eq!(f.surface.scrolls.len(), 1);
}

// C#: PointerInputControllerTests.FitHeightAsync_HorizontalScrollbarAppearsAsSideEffect_ReappliesOnceMore
#[test]
fn fit_height_horizontal_scrollbar_appears_as_side_effect_reapplies_once_more() {
    let mut f = Fixture::new();
    source(&mut f, 4000, 2000);
    f.surface.extent_height = 600.0;
    let step = f.call(|c, ctx| c.fit_height(ctx));
    f.surface.viewport_height = 580.0;
    f.drive(step);
    assert_eq!(f.yields, 2);
    assert_eq!(f.surface.scrolls.len(), 3);
}

// C#: PointerInputControllerTests.ApplyInitialViewAsync_FitWidth_VerticalScrollbarAppearsAsSideEffect_ReappliesOnceMore
#[test]
fn apply_initial_view_fit_width_vertical_scrollbar_appears_as_side_effect_reapplies_once_more() {
    let mut f = Fixture::new();
    source(&mut f, 2000, 4000);
    f.surface.extent_width = 800.0;
    let step = f.call(|c, ctx| c.apply_initial_view(ctx, InitialViewMode::FitWidth, 100));
    f.surface.viewport_width = 780.0;
    f.drive(step);
    assert_eq!(f.yields, 2);
    assert_eq!(f.surface.scrolls.len(), 3);
}

// C#: PointerInputControllerTests.FitWidthAsync_WithoutImages_DoesNothing
#[test]
fn fit_width_without_images_does_nothing() {
    let mut f = Fixture::new();
    f.commands.has_images = false;
    fit_width(&mut f, FitWidthAnchor::Centre);
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.ApplyInitialViewAsync_Fit_ResetsFitWithNoScrollPlacementPass
#[test]
fn apply_initial_view_fit_resets_fit_with_no_scroll_placement_pass() {
    let mut f = Fixture::new();
    f.viewer.set_zoom(3.0);
    initial(&mut f, InitialViewMode::Fit, 100);
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.ApplyInitialViewAsync_FitWidth_AppliesZoomThenPlacesScrollAtTheConfiguredAnchor
#[test]
fn apply_initial_view_fit_width_applies_zoom_then_places_scroll_at_the_configured_anchor() {
    let mut f = Fixture::new();
    source(&mut f, 2000, 4000);
    initial(&mut f, InitialViewMode::FitWidth, 100);
    assert!((f.viewer.zoom() - 0.4).abs() < 1e-6);
    assert_eq!(f.surface.scrolls.len(), 1);
}

// C#: PointerInputControllerTests.ApplyInitialViewAsync_ClickZoomLevel_ZoomsButDoesNotScroll
#[test]
fn apply_initial_view_click_zoom_level_zooms_but_does_not_scroll() {
    let mut f = Fixture::new();
    initial(&mut f, InitialViewMode::ClickZoomLevel, 150);
    assert!((f.viewer.zoom() - 1.5).abs() < 1e-6);
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.ApplyInitialViewAsync_KeepZoomAcrossImages_IsANoOp
#[test]
fn apply_initial_view_keep_zoom_across_images_is_a_no_op() {
    let mut f = Fixture::new();
    f.viewer.set_zoom(3.0);
    f.settings.keep_zoom_across_images = true;
    initial(&mut f, InitialViewMode::Fit, 100);
    assert_eq!(f.viewer.zoom(), 3.0);
    assert!(!f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.MutationGaps.FitWidthAsync_OverflowOfExactlyHalfADip_DoesNotReapply
#[test]
fn fit_width_overflow_of_exactly_half_a_dip_does_not_reapply() {
    let mut f = Fixture::new();
    source(&mut f, 2000, 4000);
    f.surface.extent_width = 800.5;
    fit_width(&mut f, FitWidthAnchor::Centre);
    assert_eq!(f.yields, 1);
}

// C#: PointerInputControllerTests.MutationGaps.FitHeightAsync_OverflowOfExactlyHalfADip_DoesNotReapply
#[test]
fn fit_height_overflow_of_exactly_half_a_dip_does_not_reapply() {
    let mut f = Fixture::new();
    source(&mut f, 4000, 2000);
    f.surface.extent_height = 600.5;
    fit_height(&mut f);
    assert_eq!(f.yields, 1);
}

fn superseded_fit(
    action: impl FnOnce(
        &mut pr_app::input::pointer::PointerInputController,
        &mut pr_app::input::pointer::PointerContext<'_>,
    ) -> PointerStep,
) {
    let mut f = Fixture::new();
    source(&mut f, 2000, 4000);
    f.surface.extent_width = 1200.0;
    f.surface.extent_height = 900.0;
    let step = f.call(action);
    assert_eq!(f.surface.scrolls.len(), 1);
    f.versions.next_version();
    f.viewer.set_zoom(2.0);
    f.drive(step);
    assert_eq!(f.viewer.zoom(), 2.0);
    assert_eq!(f.surface.scrolls.len(), 1);
    assert_eq!(f.yields, 1);
}

// C#: PointerInputControllerTests.FitWidthAsync_SupersededDuringFirstPass_CorrectionDoesNotReapplyFit
#[test]
fn fit_width_superseded_during_first_pass_correction_does_not_reapply_fit() {
    superseded_fit(|c, ctx| c.fit_width(ctx, FitWidthAnchor::Centre));
}
// C#: PointerInputControllerTests.FitHeightAsync_SupersededDuringFirstPass_CorrectionDoesNotReapplyFit
#[test]
fn fit_height_superseded_during_first_pass_correction_does_not_reapply_fit() {
    superseded_fit(|c, ctx| c.fit_height(ctx));
}
// C#: PointerInputControllerTests.ApplyInitialViewAsync_FitWidth_SupersededDuringFirstPass_CorrectionDoesNotReapplyFit
#[test]
fn apply_initial_view_fit_width_superseded_during_first_pass_correction_does_not_reapply_fit() {
    superseded_fit(|c, ctx| c.apply_initial_view(ctx, InitialViewMode::FitWidth, 100));
}

// C#: PointerInputControllerTests.SourceSizeSwap_SupersededByALaterViewportOperation_DoesNotScroll
#[test]
fn source_size_swap_superseded_by_a_later_viewport_operation_does_not_scroll() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    pending_swap(&mut f, 2002, 1001);
    f.versions.next_version();
    drive_swap(&mut f);
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.SourceSizeSwap_WhileAZoomAwaitsItsLayout_LeavesTheScrollToThatZoom
#[test]
fn source_size_swap_while_a_zoom_awaits_its_layout_leaves_the_scroll_to_that_zoom() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    let step = f.call(|c, ctx| c.zoom_in(ctx));
    pending_swap(&mut f, 2002, 1001);
    assert!(f.controller.take_pending_continuation().is_none());
    f.drive(step);
    assert_eq!(f.yields, 1);
    assert_eq!(f.surface.scrolls.len(), 1);
}

// C#: PointerInputControllerTests.SourceSizeSwap_InFit_DoesNotTouchTheScroll
#[test]
fn source_size_swap_in_fit_does_not_touch_the_scroll() {
    let mut f = Fixture::new();
    pending_swap(&mut f, 2002, 1001);
    drive_swap(&mut f);
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.MutationGaps.SourceSizeSwap_WhileZoomed_ReanchorsOnceAfterTheRenderPass
#[test]
fn source_size_swap_while_zoomed_reanchors_once_after_the_render_pass() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    pending_swap(&mut f, 2002, 1001);
    drive_swap(&mut f);
    assert_eq!(f.yields, 1);
    assert_eq!(f.surface.scrolls.len(), 1);
}

// C#: PointerInputControllerTests.MutationGaps.SourceSizeSwap_AfterACompletedZoomGesture_StillReanchors
#[test]
fn source_size_swap_after_a_completed_zoom_gesture_still_reanchors() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    f.run(|c, ctx| c.zoom_in(ctx));
    let scrolls = f.surface.scrolls.len();
    let yields = f.yields;
    pending_swap(&mut f, 2002, 1001);
    drive_swap(&mut f);
    assert_eq!(f.yields, yields + 1);
    assert_eq!(f.surface.scrolls.len(), scrolls + 1);
}

// C#: PointerInputControllerTests.MutationGaps.SourceSizeSwap_WhileAZoomGestureAwaitsItsLayout_LeavesTheAnchoringToTheGesture
#[test]
fn source_size_swap_while_a_zoom_gesture_awaits_its_layout_leaves_the_anchoring_to_the_gesture() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    let step = f.call(|c, ctx| c.on_wheel_vertical(ctx, 120, false, point(400.0, 300.0)));
    pending_swap(&mut f, 2002, 1001);
    f.drive(step);
    assert_eq!(f.yields, 1);
    assert_eq!(f.surface.scrolls.len(), 1);
}

// C#: PointerInputControllerTests.MutationGaps.SourceSizeSwap_ImageNotLoaded_DoesNothing
#[test]
fn source_size_swap_image_not_loaded_does_nothing() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    f.surface.loaded = false;
    pending_swap(&mut f, 2002, 1001);
    assert!(f.controller.take_pending_continuation().is_none());
}

// C#: PointerInputControllerTests.MutationGaps.SourceSizeSwap_ImageElementHasNoWidth_DoesNothing
#[test]
fn source_size_swap_image_element_has_no_width_does_nothing() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    f.surface.extent_width = 0.0;
    pending_swap(&mut f, 2002, 1001);
    assert!(f.controller.take_pending_continuation().is_none());
}

// C#: PointerInputControllerTests.MutationGaps.SourceSizeSwap_ImageElementHasNoHeight_DoesNothing
#[test]
fn source_size_swap_image_element_has_no_height_does_nothing() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    f.surface.extent_height = 0.0;
    pending_swap(&mut f, 2002, 1001);
    assert!(f.controller.take_pending_continuation().is_none());
}

// C#: PointerInputControllerTests.MutationGaps.SourceSizeSwap_ASeparateOperationStartsBeforeTheRenderPass_DropsTheReanchor
#[test]
fn source_size_swap_a_separate_operation_starts_before_the_render_pass_drops_the_reanchor() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    pending_swap(&mut f, 2002, 1001);
    f.versions.next_version();
    drive_swap(&mut f);
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.SourceSizeSwap_KeepsTheViewportCentreOnTheSameImagePoint (seven theory rows)
#[test]
fn source_size_swap_keeps_the_viewport_centre_on_the_same_image_point() {
    for (zoom, offset_x, offset_y, new_w, new_h) in [
        (1.0, 3000.0, 2000.0, 6744, 4502),
        (1.0, 5900.0, 3600.0, 6744, 4502),
        (2.0, 9000.0, 6000.0, 6744, 4502),
        (4.0, 20000.0, 10000.0, 6744, 4502),
        (0.5, 1000.0, 800.0, 6744, 4502),
        (1.0, 3000.0, 2000.0, 6719, 4479),
        (2.0, 9000.0, 6000.0, 6719, 4479),
    ] {
        let mut f = Fixture::new();
        f.zoomed_with_source(6720, 4480);
        f.viewer.set_zoom(zoom);
        f.surface.extent_width = 6720.0 * zoom;
        f.surface.extent_height = 4480.0 * zoom;
        f.surface.horizontal = offset_x;
        f.surface.vertical = offset_y;
        let before_x = (offset_x + 400.0) / f.surface.extent_width;
        let before_y = (offset_y + 300.0) / f.surface.extent_height;
        pending_swap(&mut f, new_w, new_h);
        f.surface.extent_width = f64::from(new_w) * zoom;
        f.surface.extent_height = f64::from(new_h) * zoom;
        drive_swap(&mut f);
        assert_eq!(f.yields, 1);
        assert_eq!(f.surface.scrolls.len(), 1);
        let after_x = (f.surface.horizontal + 400.0) / f.surface.extent_width;
        let after_y = (f.surface.vertical + 300.0) / f.surface.extent_height;
        assert!((after_x - before_x).abs() * f.surface.extent_width <= 1.0);
        assert!((after_y - before_y).abs() * f.surface.extent_height <= 1.0);
    }
}

// C#: no direct twin; W8-05g T2 counter balance after a superseded zoom.
#[test]
fn resume_balances_the_awaiting_layout_counter_on_every_exit() {
    let mut f = Fixture::new();
    f.zoomed_with_source(2000, 1000);
    let step = f.call(|c, ctx| c.zoom_in(ctx));
    f.versions.next_version();
    f.drive(step);
    pending_swap(&mut f, 2002, 1001);
    assert!(f.controller.take_pending_continuation().is_some());
}

// C#: no direct twin; W8-05g T11 viewer update accumulation.
#[test]
fn take_viewer_update_unions_and_resets() {
    let mut f = Fixture::new();
    f.run(|c, ctx| c.zoom_in(ctx));
    let first = f.controller.take_viewer_update();
    assert!(!first.changes.is_empty());
    let second = f.controller.take_viewer_update();
    assert!(second.changes.is_empty());
}

// New test: kills LayoutMetrics::eq -> true.
#[test]
fn changed_viewport_after_render_requires_second_scroll_placement() {
    let mut f = Fixture::new();
    let step = f.call(|c, ctx| c.on_wheel_vertical(ctx, 120, false, point(100.0, 450.0)));
    assert!(matches!(step, PointerStep::AwaitRender(_)));
    assert_eq!(f.surface.scrolls.len(), 1);
    f.surface.viewport_width = 700.0;
    f.drive(step);
    assert_eq!(f.surface.scrolls.len(), 2);
}

// New test: kills PointerContinuation::version -> 0, 1, or -1.
#[test]
fn continuation_version_matches_each_operation_and_swap() {
    let mut f = Fixture::new();
    for expected in [1, 2, 3] {
        let step = f.call(|c, ctx| c.zoom_in(ctx));
        let PointerStep::AwaitRender(continuation) = step else {
            panic!("zoom must await render")
        };
        assert_eq!(f.versions.current(), expected);
        assert_eq!(continuation.version(), expected);
        f.drive(PointerStep::AwaitRender(continuation));
    }
    f.zoomed_with_source(2000, 1000);
    pending_swap(&mut f, 2002, 1001);
    let swap = f.controller.take_pending_continuation().expect("swap must re-anchor");
    assert_eq!(swap.version(), 3);
    f.drive(PointerStep::AwaitRender(swap));
}

fn wheel_scroll_with_source(source_width: f64, source_height: f64) -> (f64, f64) {
    let mut f = Fixture::new();
    f.surface.extent_width = 1200.0;
    f.surface.extent_height = 900.0;
    f.surface.source = Some(pr_app::input::primitives::SizeD {
        width: source_width,
        height: source_height,
    });
    f.run(|c, ctx| c.on_wheel_vertical(ctx, 120, false, point(100.0, 500.0)));
    f.surface.scrolls[0]
}

// New test: kills > with == or < in capture_zoom_anchor for source width and height.
#[test]
fn fitted_zoom_uses_positive_source_dimensions_for_uniform_anchor() {
    let valid = wheel_scroll_with_source(1600.0, 800.0);
    assert!(valid.0.abs() < 1e-9, "{valid:?}");
    assert!((valid.1 - 25.0).abs() < 1e-9, "{valid:?}");
}

// New test: kills > with >= in capture_zoom_anchor for source width and height.
#[test]
fn zero_or_negative_source_dimension_uses_element_anchor() {
    for (width, height) in [(0.0, 800.0), (-1.0, 800.0), (1600.0, 0.0), (1600.0, -1.0)] {
        assert_eq!(
            wheel_scroll_with_source(width, height),
            (0.0, 0.0),
            "source=({width}, {height})"
        );
    }
}
