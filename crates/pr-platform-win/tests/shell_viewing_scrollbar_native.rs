#![cfg(windows)]
//! Red tests (W8-16e, test porter) for `pr_platform_win::shell_viewing::scrollbar_native`: the port of
//! `PhotoReview.Shell.Win32/Viewing/ScrollBarNative.cs` (baseline `d92d6aae`).
//!
//! - `repeat_timing_follows_wpf_repeat_button_formulas` ports the theory `RepeatTiming_FollowsWpfRepeatButtonFormulas`
//!   (`tests/PhotoReview.Shell.Tests/Viewing/ShellScrollBarsTests.cs`, 4 rows).
//! - `golden_scrollbar_repeat_timing` drives every case of `tests/golden/scrollbar-repeat-timing.json` (145 cases).
//! - Native tests (`#[ignore]`, run on a desktop with `--include-ignored`) call the OS. They replace the WPF-only
//!   `RepeatTiming_IsTheDefaultOfAWpfRepeatButton_OnThisMachine` and the mouse-leave call.
//!
//! Every test here fails through `todo!()` until the bodies land.

use pr_platform_win::shell_viewing::scrollbar_native::{
    WM_CANCEL_MODE, WM_MOUSE_LEAVE, repeat_timing, repeat_timing_for, track_mouse_leave,
};
use pr_test_support::golden::{self, i64_of, str_of};

// C#: ScrollBarNativeTests (ShellScrollBarsTests.RepeatTiming_FollowsWpfRepeatButtonFormulas)
#[test]
fn repeat_timing_follows_wpf_repeat_button_formulas() {
    // (keyboardDelay, keyboardSpeed, delayMs, intervalMs)
    let rows: [(i32, i32, i32, i32); 4] = [(0, 31, 250, 33), (1, 31, 500, 33), (3, 0, 1000, 400), (2, 15, 750, 222)];
    for (keyboard_delay, keyboard_speed, delay_ms, interval_ms) in rows {
        assert_eq!(
            repeat_timing_for(keyboard_delay, keyboard_speed),
            (delay_ms, interval_ms),
            "RepeatTimingFor({keyboard_delay}, {keyboard_speed})"
        );
    }
}

// new: the C# `static` formula is a `const fn`; the const-context check (`const X: (i32, i32) = repeat_timing_for(0, 31);`)
// is NOT written here: a const item is evaluated at compile time, and the stub's `todo!()` would then break the whole test
// binary with a compile error instead of a test failure. The porter adds that const item when the body lands.
#[test]
fn repeat_timing_for_is_usable_in_const_context() {
    assert_eq!(repeat_timing_for(0, 31), (250, 33));
}

// new: golden driver for every case of `tests/golden/scrollbar-repeat-timing.json` (145 cases)
#[test]
fn golden_scrollbar_repeat_timing() {
    let golden = golden::load("scrollbar-repeat-timing");
    assert_eq!(golden.cases.len(), 145, "scrollbar-repeat-timing golden case count");
    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "kind") {
            "timing" => {
                let keyboard_delay = i64_of(case, "keyboard_delay") as i32;
                let keyboard_speed = i64_of(case, "keyboard_speed") as i32;
                let expected = (i64_of(case, "delay_ms") as i32, i64_of(case, "interval_ms") as i32);
                assert_eq!(
                    repeat_timing_for(keyboard_delay, keyboard_speed),
                    expected,
                    "{name}: RepeatTimingFor({keyboard_delay}, {keyboard_speed})"
                );
            }
            "constants" => {
                assert_eq!(
                    i64_of(case, "wm_mouse_leave") as u32,
                    WM_MOUSE_LEAVE,
                    "{name}: WM_MOUSELEAVE"
                );
                assert_eq!(
                    i64_of(case, "wm_cancel_mode") as u32,
                    WM_CANCEL_MODE,
                    "{name}: WM_CANCELMODE"
                );
            }
            other => panic!("{name}: unknown kind {other:?}"),
        }
    }
}

// C#: ScrollBarNative.RepeatTiming() (native, reads the system keyboard settings)
#[test]
#[ignore = "native: reads the system keyboard settings"]
fn repeat_timing_is_within_the_documented_ranges() {
    let (delay_ms, interval_ms) = repeat_timing();
    assert!([250, 500, 750, 1000].contains(&delay_ms), "delay {delay_ms}");
    assert!((33..=400).contains(&interval_ms), "interval {interval_ms}");
}

// C#: ScrollBarNative.TrackMouseLeave(hwnd) (native, an invalid handle is a silent no-op)
#[test]
#[ignore = "native: calls TrackMouseEvent with invalid window handles"]
fn track_mouse_leave_with_an_invalid_window_is_a_silent_no_op() {
    track_mouse_leave(0);
    track_mouse_leave(0x7fff_ffff);
}
