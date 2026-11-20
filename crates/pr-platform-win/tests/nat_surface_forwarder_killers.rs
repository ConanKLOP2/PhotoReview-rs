//! Killers for `shell_input/surface.rs` 305:9 (`display_timing -> None`) and 308:9 (`pointer_position -> None`): the
//! `ViewportControllerApi` forwarders of the real `ViewportController`. `w8_surfmut4_killers` only asserted `is_none()` (a controller
//! without a display clock and without a cursor), which a constant `None` also satisfies. Test kind F: fake window, poster and clock.
#![cfg(windows)]

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::viewport::types::ViewerStretchMode;
use pr_platform_win::host::contracts::PointD as HostPointD;
use pr_platform_win::shell_input::surface::ViewportControllerApi;
use pr_platform_win::shell_viewing::viewport_controller::{
    ViewportContent, ViewportController, ViewportControllerOptions,
};
use viewing_fakes::{FakeDisplayClock, FakeHostClock, FakeRenderPoster, FakeWindow};

fn controller(
    window: &Rc<FakeWindow>,
    poster: &Rc<FakeRenderPoster>,
    options: ViewportControllerOptions,
) -> Rc<ViewportController> {
    let content = Rc::new(Cell::new(ViewportContent {
        stretch: ViewerStretchMode::None,
        image_width: 3000.0,
        image_height: 2000.0,
        max_image_width: f64::INFINITY,
        max_image_height: f64::INFINITY,
        bitmap_width: 750.0,
        bitmap_height: 500.0,
        is_fit: false,
    }));
    ViewportController::new(
        window.clone(),
        poster.clone(),
        FakeHostClock::new(),
        Rc::new(move || content.get()),
        Rc::new(|| {}),
        options,
    )
}

// C#: ViewportController.DisplayTiming forwarded by the production ViewportControllerApi impl (surface.rs:304-306).
#[test]
fn controller_api_display_timing_forwards_the_display_clock_of_the_window() {
    let window = FakeWindow::new();
    let poster = FakeRenderPoster::new();
    let display = Arc::new(FakeDisplayClock::default());
    let viewport = controller(
        &window,
        &poster,
        ViewportControllerOptions {
            display_clock: Some(display.clone()),
            ..ViewportControllerOptions::default()
        },
    );
    let api: &dyn ViewportControllerApi = &*viewport;
    let timing = api.display_timing().expect("a display clock answers through the API");
    assert_eq!((timing.last_vblank, timing.refresh_period), (5, 6));
    assert_eq!(display.last_window(), Some(window.hwnd));
}

// C#: ViewportController.PointerPosition forwarded by the production ViewportControllerApi impl (surface.rs:307-309).
#[test]
fn controller_api_pointer_position_forwards_the_cursor_in_viewport_dips() {
    let window = FakeWindow::new();
    let poster = FakeRenderPoster::new();
    window.dpi_scale.set(2.0);
    window.client_origin_pixel.set(HostPointD { x: 100.0, y: 40.0 });
    let viewport = controller(
        &window,
        &poster,
        ViewportControllerOptions {
            cursor_screen_pixel: Box::new(|| {
                Some(PointD {
                    x: 100.0 + 2.0 * 10.0,
                    y: 40.0 + 2.0 * 20.0,
                })
            }),
            ..ViewportControllerOptions::default()
        },
    );
    poster.drain();
    let api: &dyn ViewportControllerApi = &*viewport;
    assert_eq!(api.pointer_position(), Some(PointD { x: 10.0, y: 20.0 }));
}

// ---- shell_input/native.rs `current_wheel_device` (P-430 seam) ----

// C#: WheelMessageSource.Current(): IMDT_TOUCHPAD (0x10) is the only device type that says "touchpad"; an unavailable source or any other type is Unknown.
#[test]
fn wheel_device_from_device_type_is_touchpad_only_for_the_touchpad_type() {
    use pr_app::input::touchpad_gestures::WheelDeviceHint;
    use pr_platform_win::shell_input::native::wheel_device_from_device_type;
    assert_eq!(WheelDeviceHint::Touchpad, wheel_device_from_device_type(Some(0x10)));
    assert_eq!(WheelDeviceHint::Unknown, wheel_device_from_device_type(None));
    for other in [0x00, 0x01, 0x02, 0x04, 0x08, 0x20, 0x11, -1] {
        assert_eq!(
            WheelDeviceHint::Unknown,
            wheel_device_from_device_type(Some(other)),
            "device type {other:#x}"
        );
    }
}
