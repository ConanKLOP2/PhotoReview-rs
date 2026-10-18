//! The monitor queries window placement and fullscreen need, as a seam so tests can inject a layout.
//!
//! Port of `PhotoReview.App.Shared/Windowing/MonitorLayout.cs` (baseline d92d6aae): `ScreenRect`, `IMonitorLayout`. The C#
//! `Win32MonitorLayout` (`MonitorFromWindow`, `GetMonitorInfo`, `EnumDisplayMonitors`) is native code and is NOT in this crate
//! section: it lives in `pr_platform_win` (contract `W8-03a.md`, "Native implementation"). Contract: `contracts/W8-03a.md`.

#![allow(unused_variables, dead_code)] // D11: remove with the last `todo!()`.

use super::contracts::{MonitorHandle, WindowHandle};

/// A screen rectangle in physical pixels, right and bottom exclusive (a Win32 `RECT`). Field order as in C#.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// `Right - Left` in C# `unchecked` `int` arithmetic: wraps instead of panicking (`i32::MAX - i32::MIN` is `-1`).
    pub fn width(&self) -> i32 {
        todo!()
    }

    /// `Bottom - Top`, wrapping like [`width`](ScreenRect::width).
    pub fn height(&self) -> i32 {
        todo!()
    }
}

/// `IMonitorLayout`. `Send + Sync` because a placement store holds it behind an `Arc` and may be used from several threads.
pub trait MonitorLayout: Send + Sync {
    /// The monitor nearest to the window (`MonitorFromWindow(MONITOR_DEFAULTTONEAREST)`); zero when unknown.
    fn monitor_of(&self, hwnd: WindowHandle) -> MonitorHandle;

    /// The work area (monitor minus taskbar) of `monitor`; `None` when it no longer exists (C# `bool` + `out`).
    fn try_get_work(&self, monitor: MonitorHandle) -> Option<ScreenRect>;

    /// The work areas of every monitor currently attached, in enumeration order (not necessarily primary first).
    fn work_areas(&self) -> Vec<ScreenRect>;
}

/// The test double the C# tests call `FixedLayout`: a fixed list of work areas. `monitor_of` is always 1, `try_get_work` is the first
/// area (`None` when the list is empty; C# would throw `IndexOutOfRangeException`, no test relies on that), `work_areas` is the list.
/// It is public because W8-03b, W8-17b and W8-13k tests need it too.
#[derive(Debug, Clone, Default)]
pub struct FixedMonitorLayout {
    works: Vec<ScreenRect>,
}

impl FixedMonitorLayout {
    pub fn new(works: Vec<ScreenRect>) -> Self {
        todo!()
    }
}

impl MonitorLayout for FixedMonitorLayout {
    fn monitor_of(&self, hwnd: WindowHandle) -> MonitorHandle {
        todo!()
    }

    fn try_get_work(&self, monitor: MonitorHandle) -> Option<ScreenRect> {
        todo!()
    }

    fn work_areas(&self) -> Vec<ScreenRect> {
        todo!()
    }
}
