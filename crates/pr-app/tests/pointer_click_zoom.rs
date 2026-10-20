//! W8-05h, part 1: click-to-zoom (mouse), the ClickZoom shortcut and the context-menu click-zoom level.
//! Twins of the `PointerInputControllerTests` methods named in `contracts/W8-05h.md`; the fixture settings are the C# ones
//! (`ClickToZoomEnabled`, `ClickZoomPercent = 200`, `KineticPanEnabled`, `KeyboardZoomAnchor.Pointer`).

// Each integration binary uses a different subset of the shared fixture.
#[allow(dead_code)]
#[path = "common/pointer_fixture.rs"]
mod pointer_fixture;

use pointer_fixture::{Fixture, point};
use pr_app::input::pointer::{PointerRelease, PointerStep};
use pr_app::input::primitives::PointerButton;

/// The C# test class constructor: the settings every test starts from.
fn fixture() -> Fixture {
    let mut f = Fixture::new();
    f.settings.click_to_zoom_enabled = true;
    f.settings.click_zoom_percent = 200;
    f.settings.kinetic_pan_enabled = true;
    f
}

/// C# `ZoomInSoTheImageCanPan()`.
fn zoom_in_so_the_image_can_pan(f: &mut Fixture) {
    f.viewer.set_zoom(2.0);
    f.surface.extent_width = 4000.0;
    f.surface.extent_height = 3000.0;
    f.surface.horizontal = 500.0;
    f.surface.vertical = 400.0;
}

fn press(f: &mut Fixture, click_count: i32, x: f64, y: f64, timestamp: i32) -> bool {
    let mut taken = false;
    f.run(|c, ctx| {
        taken = c.on_image_press(ctx, PointerButton::Left, click_count, point(x, y), timestamp);
        PointerStep::Done
    });
    taken
}

fn release(f: &mut Fixture, x: f64, y: f64, timestamp: i32) -> PointerRelease {
    let mut result = None;
    f.run(|c, ctx| {
        result = Some(c.on_image_release(ctx, point(x, y), timestamp));
        PointerStep::Done
    });
    result.expect("the closure ran")
}

// C#: PointerInputControllerTests.Click_InFit_WithClickToZoom_ZoomsToTheClickZoomAtTheCursor
#[test]
fn click_in_fit_with_click_to_zoom_zooms_to_the_click_zoom_at_the_cursor() {
    let mut f = fixture();
    assert!(press(&mut f, 1, 50.0, 50.0, 0));
    assert!(!f.surface.pan_cursor); // Fit: nothing to pan, the press is tracked only as a click

    let result = release(&mut f, 50.0, 50.0, 10);
    assert!(result.handled);
    assert!(matches!(result.step, PointerStep::AwaitRender(_)));
    f.drive(result.step);

    assert!(!f.viewer.is_fit());
    assert!((f.viewer.zoom() - 2.0).abs() < 1e-6);
    assert_eq!(f.surface.scrolls.len(), 1);
    assert_eq!(
        f.surface.to_image_element_calls.borrow().as_slice(),
        &[point(50.0, 50.0)]
    );
    assert_eq!(f.yields, 1);
}

// C#: PointerInputControllerTests.Click_InFit_WithClickToZoomDisabled_IsNotTracked
#[test]
fn click_in_fit_with_click_to_zoom_disabled_is_not_tracked() {
    let mut f = fixture();
    f.settings.click_to_zoom_enabled = false;
    assert!(!press(&mut f, 1, 50.0, 50.0, 0));
    assert!(!release(&mut f, 50.0, 50.0, 10).handled);
    assert!(f.viewer.is_fit());
    assert_eq!(f.surface.captures, 0);
    assert_eq!(f.surface.render_yields + f.yields, 0);
}

// C#: PointerInputControllerTests.DoubleClick_AlwaysFits_AndItsReleaseIsNotAClick
#[test]
fn double_click_always_fits_and_its_release_is_not_a_click() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(press(&mut f, 2, 50.0, 50.0, 0));
    assert_eq!(f.commands.fits, 1);
    assert_eq!(f.surface.captures, 0);
    assert!(!release(&mut f, 50.0, 50.0, 10).handled);
    assert_eq!(f.commands.fits, 1);
}

// C#: PointerInputControllerTests.ToggleClickZoomAsync_InFit_ZoomsToClickZoomPercentAtTheViewportCentre
#[test]
fn toggle_click_zoom_in_fit_zooms_to_click_zoom_percent_at_the_viewport_centre() {
    let mut f = fixture();
    f.settings.click_to_zoom_enabled = false; // the shortcut must work even when the mouse click feature is off
    f.run(|c, ctx| c.toggle_click_zoom(ctx));
    assert!(!f.viewer.is_fit());
    assert!((f.viewer.zoom() - 2.0).abs() < 1e-6);
    assert_eq!(f.surface.scrolls.len(), 1);
    assert_eq!(f.commands.fits, 0);
    // No pointer over the viewport: the keyboard anchor is the viewport centre.
    assert_eq!(
        f.surface.to_image_element_calls.borrow().as_slice(),
        &[point(400.0, 300.0)]
    );
}

// C#: PointerInputControllerTests.ToggleClickZoomAsync_AtTheClickZoomLevel_WithTogglesFit_ReturnsToFit
#[test]
fn toggle_click_zoom_at_the_click_zoom_level_with_toggles_fit_returns_to_fit() {
    let mut f = fixture();
    f.settings.click_zoom_key_toggles_fit = true;
    f.run(|c, ctx| c.toggle_click_zoom(ctx)); // Fit -> click zoom
    f.run(|c, ctx| c.toggle_click_zoom(ctx)); // click zoom -> Fit
    assert_eq!(f.commands.fits, 1);
}

// C#: PointerInputControllerTests.ToggleClickZoomAsync_AtTheClickZoomLevel_ByDefault_DoesNothing
#[test]
fn toggle_click_zoom_at_the_click_zoom_level_by_default_does_nothing() {
    let mut f = fixture();
    assert!(!pr_core::settings::app_settings::AppSettings::default().click_zoom_key_toggles_fit);
    f.viewer.set_zoom(2.0);
    let scrolls = f.surface.scrolls.len();
    f.run(|c, ctx| c.toggle_click_zoom(ctx));
    assert_eq!(f.commands.fits, 0);
    assert!(!f.viewer.is_fit());
    assert!((f.viewer.zoom() - 2.0).abs() < 1e-6);
    assert_eq!(f.surface.scrolls.len(), scrolls); // no re-anchoring scroll either: nothing happened
}

// C#: PointerInputControllerTests.ToggleClickZoomAsync_AtAnotherZoom_GoesToTheClickZoomLevel_WhateverTheSetting
#[test]
fn toggle_click_zoom_at_another_zoom_goes_to_the_click_zoom_level_whatever_the_setting() {
    for toggles_fit in [false, true] {
        let mut f = fixture();
        f.settings.click_zoom_key_toggles_fit = toggles_fit;
        f.viewer.set_zoom(0.5); // e.g. Fit width
        f.run(|c, ctx| c.toggle_click_zoom(ctx));
        assert_eq!(f.commands.fits, 0, "toggles_fit={toggles_fit}");
        assert!((f.viewer.zoom() - 2.0).abs() < 1e-6, "toggles_fit={toggles_fit}");
    }
}

// C#: PointerInputControllerTests.ToggleClickZoomAsync_WithoutImages_DoesNothing
#[test]
fn toggle_click_zoom_without_images_does_nothing() {
    let mut f = fixture();
    f.commands.has_images = false;
    f.run(|c, ctx| c.toggle_click_zoom(ctx));
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
    // Nothing at all: not even the cancel_pan / stop_kinetic side effects.
    assert_eq!(f.surface.releases, 0);
}

// C#: PointerInputControllerTests.ToggleClickZoomAsync_PointerOverTheViewport_ZoomsAtThePointer_NotTheCentre
#[test]
fn toggle_click_zoom_pointer_over_the_viewport_zooms_at_the_pointer_not_the_centre() {
    let mut f = fixture();
    f.surface.pointer = Some(point(120.0, 80.0));
    f.run(|c, ctx| c.toggle_click_zoom(ctx));
    assert_eq!(
        f.surface.to_image_element_calls.borrow().last(),
        Some(&point(120.0, 80.0))
    );
}

// C#: PointerInputControllerTests.SetClickZoomLevelAsync_ZoomsStraightToTheGivenPercent_NoFitToggle
#[test]
fn set_click_zoom_level_zooms_straight_to_the_given_percent_no_fit_toggle() {
    let mut f = fixture();
    f.run(|c, ctx| c.set_click_zoom_level(ctx, 150));
    assert!(!f.viewer.is_fit());
    assert!((f.viewer.zoom() - 1.5).abs() < 1e-6);

    // Calling it again at the SAME percent must zoom again (not toggle back to Fit, unlike the shortcut).
    f.run(|c, ctx| c.set_click_zoom_level(ctx, 150));
    assert!(!f.viewer.is_fit());
    assert!((f.viewer.zoom() - 1.5).abs() < 1e-6);
    assert_eq!(f.commands.fits, 0);
}

// C#: PointerInputControllerTests.SetClickZoomLevelAsync_WithoutImages_DoesNothing
#[test]
fn set_click_zoom_level_without_images_does_nothing() {
    let mut f = fixture();
    f.commands.has_images = false;
    f.run(|c, ctx| c.set_click_zoom_level(ctx, 150));
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
    assert_eq!(f.surface.releases, 0);
}

// C#: PointerInputControllerTests.MutationGaps.SetClickZoomLevelAsync_AnchorsAtTheViewportCentre
#[test]
fn set_click_zoom_level_anchors_at_the_viewport_centre() {
    let mut f = fixture();
    f.surface.viewport_width = 1000.0;
    f.surface.viewport_height = 400.0;
    f.surface.pointer = Some(point(11.0, 12.0)); // neither the cursor nor the keyboard anchor
    f.run(|c, ctx| c.set_click_zoom_level(ctx, 200));
    assert_eq!(
        f.surface.to_image_element_calls.borrow().as_slice(),
        &[point(500.0, 200.0)]
    );
}

// New test: the percent is clamped to 10..800 in the integer domain (ClickZoomFactor).
#[test]
fn set_click_zoom_level_clamps_the_percent() {
    let mut f = fixture();
    f.run(|c, ctx| c.set_click_zoom_level(ctx, 5000));
    assert!((f.viewer.zoom() - 8.0).abs() < 1e-6);
    f.run(|c, ctx| c.set_click_zoom_level(ctx, 1));
    assert!((f.viewer.zoom() - 0.1).abs() < 1e-6);
}

// New test: the shortcut stops a running pan (cancel_pan) before zooming.
#[test]
fn toggle_click_zoom_cancels_a_tracked_press() {
    let mut f = fixture();
    assert!(press(&mut f, 1, 50.0, 50.0, 0));
    let releases = f.surface.releases;
    f.run(|c, ctx| c.toggle_click_zoom(ctx));
    assert_eq!(f.surface.releases, releases + 1);
    // The press is no longer tracked: its release is not a click.
    assert!(!release(&mut f, 50.0, 50.0, 10).handled);
}

/// Starts a glide with an arrow impulse (kinetic pan is on in the fixture).
fn start_arrow_glide(f: &mut Fixture) {
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 100.0;
    f.run(|c, ctx| {
        assert!(c.try_pan_by_arrow(ctx, pr_app::input::key_id::KeyId::RIGHT, false));
        PointerStep::Done
    });
    assert!(f.controller.is_kinetic_hooked());
}

// New test: the shortcut stops a running glide, also when it then does nothing (already at the click zoom).
#[test]
fn toggle_click_zoom_stops_a_running_glide_even_when_it_does_nothing_more() {
    let mut f = fixture();
    f.viewer.set_zoom(2.0); // already at the click zoom, key does not toggle Fit
    start_arrow_glide(&mut f);
    f.run(|c, ctx| c.toggle_click_zoom(ctx));
    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 1);
}

// New test: the context-menu level stops a running glide.
#[test]
fn set_click_zoom_level_stops_a_running_glide() {
    let mut f = fixture();
    start_arrow_glide(&mut f);
    f.run(|c, ctx| c.set_click_zoom_level(ctx, 150));
    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 1);
}
