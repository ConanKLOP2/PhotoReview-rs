//! Slice S2 of unit W8-20b (`contracts/W8-20b.md`): the four whole-shell tests of C# `ShellStartupTests` that `W8-17e` leaves to this unit because they exercise the render path
//! (`contracts/W8-17e.md` D-W8B6-37 and "NOT here"). Baseline `d92d6aae`, file `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellStartupTests.cs`.
//!
//! Native (`#[ignore = "native: needs desktop"]`), WARP, one `UiSmokeRun` each. STUB: the bodies are `todo!()`.
#![cfg(windows)]
#![allow(dead_code, unused_imports)] // stub

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{OpenTarget, SmokeConfig, UiSmokeRun, vk};

/// C# `ShellStartupTests.DeviceLost_DuringPresent_RecreatesTheDeviceAndRedrawsTheImage` (line 626): a device-removed fault injected through the surface's fault seam (the one the 13i twin
/// uses) during a present; the device generation rises, the GPU cache refills, the image is drawn again.
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_during_present_recreates_the_device_and_redraws_the_image() {
    todo!("W8-20b S2: contracts/W8-20b.md")
}

/// C# `ShellStartupTests.DeviceLost_RequestsItsOwnRedraw_EvenWhenTheOsSendsNoPaint` (line 688): after the fault no `WM_PAINT` is sent; the frame counter still rises.
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_requests_its_own_redraw_even_when_the_os_sends_no_paint() {
    todo!("W8-20b S2: contracts/W8-20b.md")
}

/// C# `ShellStartupTests.FirstImage_IsDrawnInTheSameTurnItIsAssigned_NotAtTheNextFrameTick` (line 728): the frame that has the first image is rendered inside the UI turn that assigned it.
#[test]
#[ignore = "native: needs desktop"]
fn first_image_is_drawn_in_the_same_turn_it_is_assigned_not_at_the_next_frame_tick() {
    todo!("W8-20b S2: contracts/W8-20b.md")
}

/// C# `ShellStartupTests.Closing_ReleasesTheRenderSurface_WhileTheWindowIsStillAlive` (line 812): on `Closing` (not cancelled) the surface is released before the window is destroyed
/// (`MainWindow::has_surface()` is false at `Closed`).
#[test]
#[ignore = "native: needs desktop"]
fn closing_releases_the_render_surface_while_the_window_is_still_alive() {
    todo!("W8-20b S2: contracts/W8-20b.md")
}
