//! G2 / b4 H3 evidence (docs/reviews/G2-2026-11-03.md): the pointer half of `ShellInput` tells the host once per zoom change (wheel, click zoom,
//! double click back to Fit), so the main view model can forward `presenter.set_viewer_zoom`. The keyboard half is pinned by
//! `shell_input_dispatch_keys.rs::zoom_commands_tell_the_host_that_the_zoom_mode_changed_so_the_presenter_can_load_the_original`.
//!
//! Test kind F (the fixture of `shell_input_dispatch_*`: real pure controllers and real `ViewportController`, fake window and host).
//! No C# twin: the C# viewer raises `ZoomModeChanged` itself (`MainViewModel.cs` line 136), the Rust shell input forwards it.
#![cfg(windows)]

#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use fixture::Rig;
use pr_core::model::enums::MouseWheelAction;
use pr_platform_win::win32::messages::{WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEWHEEL};

#[test]
fn each_wheel_zoom_notch_tells_the_host_exactly_once() {
    let rig = Rig::new();
    rig.fit();
    let before = rig.host.zoom_mode_changes.get();

    assert!(rig.wheel(WM_MOUSEWHEEL, 120, 300, 200, false).0);
    rig.drain();
    assert!(!rig.viewer.borrow().is_fit());
    assert_eq!(before + 1, rig.host.zoom_mode_changes.get(), "first notch");

    assert!(rig.wheel(WM_MOUSEWHEEL, 120, 300, 200, false).0);
    rig.drain();
    assert_eq!(before + 2, rig.host.zoom_mode_changes.get(), "second notch");
}

#[test]
fn a_wheel_over_the_compare_view_does_not_zoom_and_does_not_tell_the_host() {
    let rig = Rig::new();
    rig.fit();
    rig.host.is_compare_visible.set(true);
    let before = rig.host.zoom_mode_changes.get();

    assert!(!rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false).0);
    rig.drain();

    assert!(rig.viewer.borrow().is_fit());
    assert_eq!(before, rig.host.zoom_mode_changes.get());
}

#[test]
fn a_navigate_wheel_does_not_zoom_and_does_not_tell_the_host() {
    let rig = Rig::configured(|s| s.mouse_wheel_action = MouseWheelAction::Navigate);
    rig.fit();
    let before = rig.host.zoom_mode_changes.get();

    rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false);
    rig.drain();

    assert!(rig.viewer.borrow().is_fit());
    assert_eq!(before, rig.host.zoom_mode_changes.get());
}

#[test]
fn a_click_zoom_tells_the_host_and_the_double_click_back_to_fit_tells_it_again() {
    let rig = Rig::configured(|s| s.click_to_zoom_enabled = true);
    rig.fit();
    let before = rig.host.zoom_mode_changes.get();

    rig.message_time.set(5000);
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.mouse(WM_LBUTTONUP, 500, 300, 0);
    rig.drain();
    assert!(!rig.viewer.borrow().is_fit(), "click zoom leaves Fit");
    let after_click = rig.host.zoom_mode_changes.get();
    assert!(after_click > before, "click zoom must tell the host");

    rig.message_time.set(9000);
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.mouse(WM_LBUTTONUP, 500, 300, 0);
    rig.message_time.set(9100);
    rig.mouse(WM_LBUTTONDOWN, 501, 300, 0);
    rig.mouse(WM_LBUTTONUP, 501, 300, 0);
    rig.drain();
    assert!(rig.viewer.borrow().is_fit(), "the double click returns to Fit");
    assert!(
        rig.host.zoom_mode_changes.get() > after_click,
        "returning to Fit is a zoom mode change (the loader drops the original)"
    );
}
