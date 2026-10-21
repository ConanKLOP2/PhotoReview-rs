//! Tests of W8-13b: `host::icons` (C# `ShellIconsTests`, baseline `d92d6aae`). The OS tests are `#[ignore]`d (a hidden `STATIC` popup window and
//! `LoadImage`). The C# tests find the icon in the test assembly's module; a Rust test executable has no icon resource, so the twins load the stock
//! application icon through the seam `load_from(0, ..)` and apply it through `apply_with`.
#![cfg(windows)]

use std::cell::RefCell;

use pr_platform_win::host::icons::{
    RESOURCE_ID, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, apply, apply_with, load, load_big, load_from,
    load_small, metric,
};
use pr_platform_win::host::native::{ICON_BIG, ICON_SMALL};
use pr_platform_win::win32::api::user32::get_system_metrics;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_EX_STYLE, WM_GETICON, WS_POPUP,
};
use windows::core::w;

/// A hidden `STATIC` popup (what the C# test creates), destroyed on drop.
struct TestWindow(isize);

impl TestWindow {
    fn new() -> Self {
        // SAFETY: the predefined `STATIC` class with no parent, menu, instance or creation data; the window is created and destroyed on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("icons"),
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
        .unwrap();
        Self(hwnd.0 as isize)
    }

    fn icon(&self, kind: usize) -> isize {
        // SAFETY: WM_GETICON has no pointer parameters; the window is alive.
        unsafe { SendMessageW(HWND(self.0 as *mut _), WM_GETICON, Some(WPARAM(kind)), Some(LPARAM(0))) }.0
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by `new` on this thread.
        let _ = unsafe { DestroyWindow(HWND(self.0 as *mut _)) };
    }
}

// C#: ShellIconsTests.Load_ReturnsZeroForAnInvalidSize (T7: the OS is never reached, `(0, 16)` would load the default-size icon)
#[test]
fn load_returns_zero_for_an_invalid_size() {
    for (cx, cy) in [(0, 0), (0, 16), (16, 0), (16, -1), (-1, 16), (i32::MIN, i32::MIN)] {
        assert_eq!(load(cx, cy), 0, "load({cx}, {cy})");
        assert_eq!(load_from(0, cx, cy), 0, "load_from(0, {cx}, {cy})");
    }
}

// C#: ShellIconsTests.Apply_ToNoWindow_ReturnsFalse
#[test]
fn apply_to_no_window_returns_false() {
    assert!(!apply(0, 96));
    assert!(!apply_with(0, 96, &|_, _| 1));
}

// C#: ShellIconsTests.Load_ReturnsAnIconHandle_ForBigAndSmallSizes
#[test]
#[ignore = "native: needs desktop"]
fn load_from_system_returns_an_icon_for_big_and_small_sizes() {
    for dpi in [96, 192] {
        let big = load_from(0, metric(SM_CXICON, dpi), metric(SM_CYICON, dpi));
        let small = load_from(0, metric(SM_CXSMICON, dpi), metric(SM_CYSMICON, dpi));
        assert_ne!(
            big, 0,
            "big icon at {dpi} dpi (resource id {RESOURCE_ID} of the system module)"
        );
        assert_ne!(small, 0, "small icon at {dpi} dpi");
    }
}

// C#: ShellIconsTests.Apply_SetsTheBigAndSmallIconOfTheWindow (theory 96 and 144)
#[test]
#[ignore = "native: needs desktop"]
fn apply_with_sets_the_big_and_small_icon_of_the_window() {
    for dpi in [96, 144] {
        let window = TestWindow::new();
        assert_eq!(window.icon(ICON_BIG), 0, "a fresh STATIC window has no big icon");

        let applied = apply_with(window.0, dpi, &|cx, cy| load_from(0, cx, cy));

        assert!(applied, "dpi {dpi}");
        assert_ne!(window.icon(ICON_BIG), 0, "ICON_BIG at {dpi} dpi");
        assert_ne!(window.icon(ICON_SMALL), 0, "ICON_SMALL at {dpi} dpi");
    }
}

// new (T6): the loader is asked for the big pair first, then the small pair; a zero handle is skipped individually
#[test]
#[ignore = "native: needs desktop"]
fn apply_with_a_loader_that_finds_only_the_small_icon_returns_true() {
    let window = TestWindow::new();
    let asked = RefCell::new(Vec::new());
    let applied = apply_with(window.0, 144, &|cx, cy| {
        asked.borrow_mut().push((cx, cy));
        if cx == metric(SM_CXSMICON, 144) && cy == metric(SM_CYSMICON, 144) {
            load_from(0, cx, cy)
        } else {
            0
        }
    });

    assert!(applied);
    assert_eq!(
        *asked.borrow(),
        vec![
            (metric(SM_CXICON, 144), metric(SM_CYICON, 144)),
            (metric(SM_CXSMICON, 144), metric(SM_CYSMICON, 144))
        ]
    );
    assert_eq!(window.icon(ICON_BIG), 0);
    assert_ne!(window.icon(ICON_SMALL), 0);

    // Nothing found: nothing set, `false`.
    let other = TestWindow::new();
    assert!(!apply_with(other.0, 96, &|_, _| 0));
    assert_eq!(other.icon(ICON_BIG), 0);
    assert_eq!(other.icon(ICON_SMALL), 0);
}

// new: `dpi == 0` is the system metric (the window class asks that way), otherwise the per-dpi one
#[test]
#[ignore = "native: needs desktop"]
fn metric_uses_the_system_value_for_dpi_zero_and_the_scaled_one_otherwise() {
    for index in [SM_CXICON, SM_CYICON, SM_CXSMICON, SM_CYSMICON] {
        assert_eq!(metric(index, 0), get_system_metrics(index));
        assert!(metric(index, 192) >= metric(index, 96));
        assert!(metric(index, 96) > 0);
    }
}

// new: without an exe icon resource the test executable finds nothing and nothing panics (the contract's "never fails")
#[test]
#[ignore = "native: needs desktop"]
fn load_big_and_small_never_fail_without_a_resource() {
    // The value is 0 in a test executable (no RT_GROUP_ICON 32512); a real exe with the resource gives non-zero. Both are fine here.
    let _ = load_big(0);
    let _ = load_small(96);
    let _ = load_big(144);
}
