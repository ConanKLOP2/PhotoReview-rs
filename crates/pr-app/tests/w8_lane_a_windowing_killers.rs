//! Killer tests for `windowing::fullscreen` (`resolve_normal_exit_rect` centre arithmetic, `Debug`).
//!
//! Equivalent survivors (no test possible): `WS_CHROME` and the `SWP_*` combinations use `|` between constants with
//! disjoint bits, so `^` yields the same value; `visibility::is_visible` early return is a pure shortcut.

use pr_app::windowing::contracts::{MonitorHandle, WindowHandle};
use pr_app::windowing::fullscreen::{FullscreenController, resolve_normal_exit_rect};
use pr_app::windowing::monitor_layout::{MonitorLayout, ScreenRect};

fn r(l: i32, t: i32, rt: i32, b: i32) -> ScreenRect {
    ScreenRect::new(l, t, rt, b)
}

/// Centre x = (1600+1800)/2 = 1700: closer to B (dx 300) than A (dx 700). `% 2` would give centre 0 (A).
#[test]
fn centre_uses_division_picks_right_area() {
    let works = [r(0, 0, 1000, 1000), r(2000, 0, 3000, 1000)];
    let out = resolve_normal_exit_rect(r(1600, 100, 1800, 300), &works);
    assert_eq!(out, r(2000, 100, 2200, 300));
}

/// Centre (600, 1100): A is nearer (d = 600^2 = 360000) than B (500^2 + 600^2 = 610000). Doubling the centre
/// (1200, 2200) would make B nearer (360000 vs 400000).
#[test]
fn centre_is_not_doubled() {
    let works = [r(0, 0, 1000, 500), r(1100, 0, 2100, 500)];
    let out = resolve_normal_exit_rect(r(500, 1000, 700, 1200), &works);
    assert_eq!(out, r(500, 300, 700, 500));
}

/// Vertical twin: centre y = (1600+1800)/2 = 1700 is nearer B (dy 300) than A (dy 700). `% 2` would give 0 (A).
#[test]
fn centre_y_uses_division_picks_lower_area() {
    let works = [r(0, 0, 1000, 1000), r(0, 2000, 1000, 3000)];
    let out = resolve_normal_exit_rect(r(100, 1600, 300, 1800), &works);
    assert_eq!(out, r(100, 2000, 300, 2200));
}

/// Centre (1100, 600): A nearer (600^2 = 360000) than B (600^2 + 500^2 = 610000). Doubled y (1200) flips to B.
#[test]
fn centre_y_is_not_doubled() {
    let works = [r(0, 0, 500, 1000), r(0, 1100, 500, 2100)];
    let out = resolve_normal_exit_rect(r(1000, 500, 1200, 700), &works);
    assert_eq!(out, r(300, 500, 500, 700));
}

struct NoLayout;
impl MonitorLayout for NoLayout {
    fn monitor_of(&self, _hwnd: WindowHandle) -> MonitorHandle {
        0
    }
    fn try_get_work(&self, _monitor: MonitorHandle) -> Option<ScreenRect> {
        None
    }
    fn work_areas(&self) -> Vec<ScreenRect> {
        Vec::new()
    }
}

#[test]
fn debug_output_lists_fields() {
    let c = FullscreenController::new(Box::new(NoLayout));
    assert_eq!(
        format!("{c:?}"),
        "FullscreenController { state_before: Normal, normal_bounds: None, monitor_before: 0, .. }"
    );
}
