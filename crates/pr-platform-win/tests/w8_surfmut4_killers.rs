//! Mutant killers for `shell_input/surface.rs` (CL-MUT-SURF4): the production `ViewportControllerApi` forwarders of the real `ViewportController`.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/input_feature_fixture.rs"]
mod input_feature_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use input_feature_fixture::FeatureRig;
use pr_platform_win::shell_input::surface::ViewportControllerApi;

// C#: ViewportController.ViewportSize forwarded by the production ViewportControllerApi impl (surface.rs:241-243):
// constant (0|1, 0|-1|1) tuples are killed by two distinct non-trivial client sizes.
#[test]
fn controller_api_viewport_size_forwards_the_live_client_size() {
    let rig = FeatureRig::new();
    let api: &dyn ViewportControllerApi = &*rig.viewport;
    assert_eq!(api.viewport_size(), (1000.0, 600.0));
    rig.window.resize(812.5, 377.25);
    assert_eq!(api.viewport_size(), (812.5, 377.25));
}

// C#: ViewportController.IsLoaded = window.IsLoaded && !disposed, forwarded by surface.rs:244-246 (`-> true` mutant).
#[test]
fn controller_api_is_loaded_follows_the_window_and_dispose() {
    let rig = FeatureRig::new();
    let api: &dyn ViewportControllerApi = &*rig.viewport;
    assert!(api.is_loaded());
    rig.window.loaded.set(false);
    assert!(!api.is_loaded());
    rig.window.loaded.set(true);
    assert!(api.is_loaded());
    rig.viewport.dispose();
    assert!(!api.is_loaded());
}

use std::cell::Cell;
use std::rc::Rc;

use pr_app::input::primitives::PointD;
use pr_app::viewport::types::ViewerStretchMode;
use pr_platform_win::host::contracts::ShellCursor;

/// A rig at 100% zoom (3000 x 2000 image in a 1000 x 600 window, so both bars scroll) with the layout settled.
fn scrollable_rig() -> FeatureRig {
    let rig = FeatureRig::new();
    let _ = rig.viewer.borrow_mut().set_zoom(100.0);
    rig.viewport.invalidate_layout();
    rig.drain();
    rig
}

// C#: ViewportController.InvalidateLayout / ScrollHome / ScrollTo / HorizontalOffset / VerticalOffset through the production forwarders
// (surface.rs:250-258, 262-269): `-> ()` mutants leave the posted flag, the offsets and the scroll untouched.
#[test]
fn controller_api_scroll_and_invalidate_forwarders_reach_the_controller() {
    let rig = scrollable_rig();
    let api: &dyn ViewportControllerApi = &*rig.viewport;
    assert!(!rig.viewport.is_layout_posted());
    api.invalidate_layout();
    assert!(rig.viewport.is_layout_posted());
    rig.drain();
    assert!(!rig.viewport.is_layout_posted());

    api.scroll_to(120.0, 80.0);
    assert!(rig.viewport.is_layout_posted(), "scroll_to must queue a layout pass");
    rig.drain();
    assert_eq!((api.horizontal_offset(), api.vertical_offset()), (120.0, 80.0));

    api.scroll_home();
    assert!(rig.viewport.is_layout_posted(), "scroll_home must queue a layout pass");
    rig.drain();
    assert_eq!((api.horizontal_offset(), api.vertical_offset()), (0.0, 0.0));
}

// C#: ViewportController.UpdateFitSize (surface.rs:247-249): the callback pushes the client size into the viewer's max image size.
#[test]
fn controller_api_update_fit_size_pushes_the_client_size_to_the_viewer() {
    let rig = FeatureRig::new();
    let api: &dyn ViewportControllerApi = &*rig.viewport;
    let _ = rig.viewer.borrow_mut().set_stretch(ViewerStretchMode::Uniform);
    rig.window.client_size.set(pr_app::input::primitives::SizeD {
        width: 733.0,
        height: 411.0,
    });
    api.update_fit_size();
    assert_eq!(rig.viewer.borrow().max_image_width(), 733.0);
    assert_eq!(rig.viewer.borrow().max_image_height(), 411.0);
}

// C#: ViewportController geometry members, DragThreshold and SourceSize through the production forwarders (surface.rs:270-290).
#[test]
fn controller_api_geometry_members_match_the_controller_and_are_not_trivial() {
    let rig = scrollable_rig();
    let api: &dyn ViewportControllerApi = &*rig.viewport;
    rig.viewport.scroll_to(120.0, 80.0);
    rig.drain();
    assert_eq!(api.drag_threshold(), (4.0, 4.0));
    rig.window.dpi_scale.set(2.0);
    assert_eq!(api.drag_threshold(), (2.0, 2.0));
    let size = api.source_size().expect("3000 x 2000 bitmap");
    assert_eq!((size.width, size.height), (3000.0, 2000.0));
    assert_eq!(api.image_actual_width(), rig.viewport.image_actual_width());
    assert_eq!(api.image_actual_height(), rig.viewport.image_actual_height());
    assert!(api.image_actual_width() > 1.0 && api.image_actual_height() > 1.0);
    assert_ne!(api.image_actual_width(), api.image_actual_height());
    let origin = api.image_origin();
    let expected = rig.viewport.image_origin();
    assert_eq!((origin.x, origin.y), (expected.x, expected.y));
    assert_eq!((origin.x, origin.y), (-120.0, -80.0));
    let probe = PointD { x: 321.5, y: 77.25 };
    let mapped = api.to_image_element(probe);
    let want = rig.viewport.to_image_element(probe);
    assert_eq!((mapped.x, mapped.y), (want.x, want.y));
    assert_ne!((mapped.x, mapped.y), (probe.x, probe.y), "the origin offsets the point");
}

// C#: CaptureMouse / ReleaseMouseCapture / SetPanCursor (surface.rs:291-299).
#[test]
fn controller_api_capture_and_cursor_forwarders_reach_the_window() {
    let rig = FeatureRig::new();
    let api: &dyn ViewportControllerApi = &*rig.viewport;
    api.capture_mouse();
    assert!(rig.window.captured.get());
    api.release_mouse_capture();
    assert!(!rig.window.captured.get());
    api.set_pan_cursor(true);
    assert_eq!(rig.window.cursor.get(), Some(ShellCursor::SizeAll));
    api.set_pan_cursor(false);
    assert_eq!(rig.window.cursor.get(), Some(ShellCursor::Arrow));
}

// C#: Hook/UnhookRenderFrame, Timestamp, DisplayTiming, PointerPosition, YieldToRender (surface.rs:300-320).
#[test]
fn controller_api_frame_hooks_timing_and_yield_forwarders_reach_the_controller() {
    let rig = FeatureRig::new();
    let api: &dyn ViewportControllerApi = &*rig.viewport;
    let calls = Rc::new(Cell::new(0_u32));
    let counter = Rc::clone(&calls);
    let first = api.hook_render_frame(Box::new(move |_| counter.set(counter.get() + 1)));
    let second = api.hook_render_frame(Box::new(|_| {}));
    assert_ne!(first, second);
    rig.clock.tick(16.0);
    assert_eq!(calls.get(), 1);
    api.unhook_render_frame(first);
    rig.clock.tick(32.0);
    assert_eq!(calls.get(), 1, "an unhooked handler is not called again");

    assert!(api.timestamp() > 1);
    assert!(api.display_timing().is_none());
    assert!(api.pointer_position().is_none());

    let ran = Rc::new(Cell::new(false));
    let flag = Rc::clone(&ran);
    api.yield_to_render(Box::new(move || flag.set(true)));
    assert!(!ran.get(), "the continuation never runs inline");
    rig.drain();
    assert!(ran.get());
}
