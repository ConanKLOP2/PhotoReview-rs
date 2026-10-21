//! Port of `PhotoReview.Shell.Win32/Input/ClickCounter.cs` (58 lines, baseline `d92d6aae`). Unit W8-14a. Contract: `contracts/W8-14a.md`.
//!
//! WP-22: counts consecutive clicks (WPF `MouseButtonEventArgs.ClickCount`). The window class has no `CS_DBLCLKS`, so there is no
//! `WM_LBUTTONDBLCLK` from the OS; a click continues the chain when it has the same button, comes within `GetDoubleClickTime` of the previous one and
//! lies inside the `SM_CXDOUBLECLK x SM_CYDOUBLECLK` rectangle centred on the previous click. The third click counts on (Fit when `>= 2`). Pure: the
//! caller passes the time stamp and the thresholds.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::input::primitives::PointerButton;

/// C# `ClickCounter` (`internal sealed class`): the state of the current click chain.
#[derive(Debug, Clone)]
pub struct ClickCounter {
    has: bool,
    button: PointerButton,
    x: i32,
    y: i32,
    time: i32,
    count: i32,
}

impl ClickCounter {
    /// A counter with no chain (C# field defaults: `_has = false`).
    pub fn new() -> Self {
        Self {
            has: false,
            button: PointerButton::Left,
            x: 0,
            y: 0,
            time: 0,
            count: 0,
        }
    }

    /// C# `Register(button, x, y, time, doubleClickTimeMs, slopWidth, slopHeight)`: records one click and returns the consecutive count including it
    /// (`>= 1`). `x`, `y` are client pixels; `time` is `GetMessageTime` in ms (wrapping: `elapsed = time.wrapping_sub(previous)`); the chain continues
    /// when `has && same button && 0 <= elapsed <= double_click_time_ms && |x - x0| <= slop_width / 2 && |y - y0| <= slop_height / 2`; any click then
    /// becomes the new reference point (button, x, y, time).
    #[allow(clippy::too_many_arguments)] // the C# signature has these seven parameters
    pub fn register(
        &mut self,
        button: PointerButton,
        x: i32,
        y: i32,
        time: i32,
        double_click_time_ms: i32,
        slop_width: i32,
        slop_height: i32,
    ) -> i32 {
        todo!()
    }

    /// C# `Reset()`: forget the chain (the next click counts 1).
    pub fn reset(&mut self) {
        todo!()
    }

    /// C# `internal Seed(...)` (tests): as if `count` consecutive clicks had happened, the last at `time`.
    pub fn seed(&mut self, button: PointerButton, x: i32, y: i32, time: i32, count: i32) {
        todo!()
    }
}

impl Default for ClickCounter {
    fn default() -> Self {
        Self::new()
    }
}
