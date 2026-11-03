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

// new: the C# `static` formula is a `const fn`; the const item below is evaluated at compile time (the body is filled now).
#[test]
fn repeat_timing_for_is_usable_in_const_context() {
    const AT_DEFAULT_SPEED: (i32, i32) = repeat_timing_for(0, 31);
    assert_eq!(AT_DEFAULT_SPEED, (250, 33));
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

/// A hidden `STATIC` popup on this thread, destroyed on drop (the same shape the other native tests use).
struct ProbeWindow(isize);

impl ProbeWindow {
    fn new() -> Self {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, WINDOW_EX_STYLE, WS_POPUP};
        use windows::core::w;
        // SAFETY: the predefined `STATIC` class with no parent, menu, instance or creation data; created and destroyed on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("scrollbar-native"),
                WS_POPUP,
                0,
                0,
                16,
                16,
                None,
                None,
                None,
                None,
            )
        }
        .expect("create the probe window");
        Self(hwnd.0 as isize)
    }
}

impl Drop for ProbeWindow {
    fn drop(&mut self) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        // SAFETY: the handle is the live window created by `new` on this thread; a failed destroy only leaks a hidden window.
        let _ = unsafe { DestroyWindow(HWND(self.0 as *mut core::ffi::c_void)) };
    }
}

/// Whether `TrackMouseEvent(TME_QUERY)` reports a pending `TME_LEAVE` request for `hwnd`.
fn leave_tracking_is_pending(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TME_QUERY, TRACKMOUSEEVENT, TrackMouseEvent};
    let mut data = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_QUERY,
        hwndTrack: HWND(hwnd as *mut core::ffi::c_void),
        dwHoverTime: 0,
    };
    // SAFETY: `data` is a local, fully initialised `TRACKMOUSEEVENT` with its own size; the call fills it and keeps no pointer.
    unsafe { TrackMouseEvent(&mut data) }.expect("TME_QUERY on a live window");
    (data.dwFlags.0 & TME_LEAVE.0) != 0
}

/// Takes a posted `WM_MOUSELEAVE` for `hwnd` off this thread's queue; `true` when one was there.
fn take_posted_mouse_leave(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};
    let mut msg = MSG::default();
    // SAFETY: `msg` is a local `MSG`; the filter names a window of this thread and one message id; the call keeps no pointer.
    unsafe {
        PeekMessageW(
            &mut msg,
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            WM_MOUSE_LEAVE,
            WM_MOUSE_LEAVE,
            PM_REMOVE,
        )
    }
    .as_bool()
}

// new (native): C# `ScrollBarNative.TrackMouseLeave(hwnd)` asks Windows for ONE `WM_MOUSELEAVE`. The OS either keeps the request pending
// (`TME_QUERY` reports `TME_LEAVE`, pointer over the window) or, when the pointer is already outside, posts `WM_MOUSELEAVE` at once; both are
// independent witnesses that the request reached the OS (the invalid-handle test above only proves "no panic").
#[test]
#[ignore = "native: creates a hidden window and observes its mouse-leave request"]
fn track_mouse_leave_requests_a_mouse_leave_for_a_real_window() {
    let window = ProbeWindow::new();
    assert!(
        !leave_tracking_is_pending(window.0),
        "precondition: no tracking before the call"
    );
    assert!(
        !take_posted_mouse_leave(window.0),
        "precondition: no WM_MOUSELEAVE queued before the call"
    );
    track_mouse_leave(window.0);
    let pending = leave_tracking_is_pending(window.0);
    let posted = take_posted_mouse_leave(window.0);
    assert!(
        pending || posted,
        "track_mouse_leave must leave a pending TME_LEAVE or a posted WM_MOUSELEAVE"
    );
}

/// `HKCU\Control Panel\Keyboard` value `name` as an integer, read with `reg.exe` (a path independent of `SystemParametersInfoW`).
fn keyboard_registry_value(name: &str) -> Option<i32> {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Control Panel\Keyboard", "/v", name])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let line = text.lines().find(|line| line.trim_start().starts_with(name))?;
    line.split_whitespace().last()?.parse().ok()
}

// new (native): the C# `RepeatTiming_IsTheDefaultOfAWpfRepeatButton_OnThisMachine` compares with the WPF default of this machine; the
// stable witness here is the persisted keyboard setting (the SPI values equal it unless the session changed them live).
#[test]
#[ignore = "native: reads the system keyboard settings and the HKCU keyboard registry values"]
fn repeat_timing_equals_the_formula_of_the_persisted_keyboard_settings() {
    let delay = keyboard_registry_value("KeyboardDelay").expect("KeyboardDelay in HKCU");
    let speed = keyboard_registry_value("KeyboardSpeed").expect("KeyboardSpeed in HKCU");
    assert!(
        (0..=3).contains(&delay) && (0..=31).contains(&speed),
        "registry values in range: {delay}/{speed}"
    );
    assert_eq!(repeat_timing(), repeat_timing_for(delay, speed));
}
