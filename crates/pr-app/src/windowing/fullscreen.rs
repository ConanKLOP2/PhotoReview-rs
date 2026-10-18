//! F11 fullscreen without ever changing the OS window state.
//!
//! Port of `PhotoReview.App.Shared/Windowing/FullscreenController.cs` (baseline d92d6aae, C-14, K-21). The window keeps
//! its state (Maximized stays Maximized, Normal stays Normal); only its style bits change and ONE `SetWindowPos`
//! moves it onto the whole monitor (`rcMonitor`, so it covers the taskbar). Exit restores the style and moves the window
//! straight back. All coordinates are physical pixels. UI thread only.
//!
//! The Win32 calls the C# class made through `LibraryImport` go through the [`WindowOps`] trait (real implementation in
//! `pr_platform_win` / `host::main_window::windowing`, W8-13k); this module has no `unsafe` and no `windows` crate.
//! The monitor queries used on exit are the `MonitorLayout` seam of W8-03a (C# `IMonitorLayout`).

#![allow(unused_variables, dead_code)] // stub: removed by the porter

use super::contracts::WindowShowState;
use super::monitor_layout::{MonitorLayout, ScreenRect};

/// `SWP_NOSIZE`.
pub const SWP_NOSIZE: u32 = 0x0001;
/// `SWP_NOMOVE`.
pub const SWP_NOMOVE: u32 = 0x0002;
/// `SWP_NOZORDER`.
pub const SWP_NOZORDER: u32 = 0x0004;
/// `SWP_NOACTIVATE`.
pub const SWP_NOACTIVATE: u32 = 0x0010;
/// `SWP_FRAMECHANGED`.
pub const SWP_FRAMECHANGED: u32 = 0x0020;

/// Caption, thick frame, system menu, minimize box and maximize box (`WsChrome` in C#): `0x00CF0000`.
pub const WS_CHROME: isize = 0x00C0_0000 | 0x0004_0000 | 0x0008_0000 | 0x0002_0000 | 0x0001_0000;

/// Caption band height used to decide whether a saved Normal rect is still reachable (`CaptionBand`).
pub const CAPTION_BAND: i32 = 32;
/// Minimum visible width of the caption band (`MinVisibleWidth`).
pub const MIN_VISIBLE_WIDTH: i32 = 120;
/// Minimum visible height of the caption band (`MinVisibleHeight`).
pub const MIN_VISIBLE_HEIGHT: i32 = 16;

/// What `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfo` report for a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorSnapshot {
    /// The `HMONITOR` value; never 0 (a zero handle is reported as `None` by [`WindowOps::nearest_monitor`]).
    pub handle: isize,
    /// `rcMonitor`: the whole monitor including the taskbar.
    pub monitor: ScreenRect,
    /// `rcWork`: the monitor minus the taskbar.
    pub work: ScreenRect,
}

/// The user32 calls `FullscreenController` makes on a window handle (an `HWND` as `isize`). Stateless: `&self` everywhere;
/// fakes keep a `RefCell` call log. Failures are reported as `None`, never as a panic.
pub trait WindowOps {
    /// `IsIconic`.
    fn is_iconic(&self, hwnd: isize) -> bool;
    /// `ShowWindow(hwnd, SW_SHOWNORMAL)` (1).
    fn show_normal(&self, hwnd: isize);
    /// `ShowWindow(hwnd, SW_MAXIMIZE)` (3).
    fn show_maximized(&self, hwnd: isize);
    /// `GetWindowPlacement(..).rcNormalPosition`; `None` when the call fails.
    fn normal_position(&self, hwnd: isize) -> Option<ScreenRect>;
    /// `GetWindowRect`; `None` when the call fails (the controller then uses an all-zero rect, like the C# `out` default).
    fn window_rect(&self, hwnd: isize) -> Option<ScreenRect>;
    /// `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)` then `GetMonitorInfoW`; `None` when the handle is 0 or the call fails.
    fn nearest_monitor(&self, hwnd: isize) -> Option<MonitorSnapshot>;
    /// `GetWindowLongPtrW(hwnd, GWL_STYLE)`.
    fn window_style(&self, hwnd: isize) -> isize;
    /// `SetWindowLongPtrW(hwnd, GWL_STYLE, style)`.
    fn set_window_style(&self, hwnd: isize, style: isize);
    /// `SetWindowPos(hwnd, 0, rect.left, rect.top, rect.width(), rect.height(), flags)` with `width`/`height` computed with
    /// wrapping `i32` subtraction. `flags` is a combination of the `SWP_*` constants above.
    fn set_window_pos(&self, hwnd: isize, rect: ScreenRect, flags: u32);
}

/// Where a Normal window's saved rect goes when the monitor layout changed (C# `ResolveNormalExitRect`). Pure.
///
/// Kept as is when the caption band (top [`CAPTION_BAND`] px) still shows at least [`MIN_VISIBLE_WIDTH`] x
/// [`MIN_VISIBLE_HEIGHT`] px inside some work area. Otherwise moved onto the nearest work area (squared distance from the
/// rect centre, first area wins a tie), keeping its size (shrunk only when larger than that area) and the closest
/// position. An empty `works` list returns `saved` unchanged. `i32` arithmetic wraps like unchecked C# `int`, and the final
/// clamp never panics.
pub fn resolve_normal_exit_rect(saved: ScreenRect, works: &[ScreenRect]) -> ScreenRect {
    todo!("W8-03b")
}

/// F11 state machine over a window handle. Owns the saved restore data; the OS calls are made through the [`WindowOps`]
/// given to `enter`/`exit`; the monitor queries of `exit` go through the injected [`MonitorLayout`].
///
/// Deliberately has NO "already fullscreen" guard (C# has none; `ShellFullscreen` / W8-13k makes it idempotent): a second
/// `enter` without `exit` overwrites the saved data with the fullscreen state.
pub struct FullscreenController {
    layout: Box<dyn MonitorLayout>,
    state_before: WindowShowState,
    normal_bounds: Option<ScreenRect>,
    window_rect_before: ScreenRect,
    work_before: ScreenRect,
    style_before: isize,
    monitor_before: isize,
    enter_needs_maximized_state: bool,
    exit_needs_state_restore: bool,
}

impl std::fmt::Debug for FullscreenController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `layout` is a trait object without `Debug`.
        f.debug_struct("FullscreenController")
            .field("state_before", &self.state_before)
            .field("normal_bounds", &self.normal_bounds)
            .field("monitor_before", &self.monitor_before)
            .finish_non_exhaustive()
    }
}

impl FullscreenController {
    /// C# `new FullscreenController(IMonitorLayout)`. (The production Win32 layout is owned by W8-13k, lead decision D-W8B1-5.)
    pub fn new(layout: Box<dyn MonitorLayout>) -> Self {
        todo!("W8-03b")
    }

    /// The state the window had before fullscreen (a minimized window counts as Normal). Initially Normal.
    pub fn state_before(&self) -> WindowShowState {
        todo!("W8-03b")
    }

    /// Restore bounds (`rcNormalPosition`, physical px) saved when fullscreen was entered, as `(left, top, right, bottom)`;
    /// `None` while not fullscreen or when none could be saved.
    pub fn normal_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        todo!("W8-03b")
    }

    /// After `enter`: true when the OS window was NOT moved onto the monitor (no handle yet, or no monitor info), so the
    /// caller must maximize the window itself to get a fullscreen-sized window.
    pub fn enter_needs_maximized_state(&self) -> bool {
        todo!("W8-03b")
    }

    /// After `exit`: true when the controller did not restore the OS window itself (no handle, no saved bounds, or no
    /// monitor info), so the caller must put the window back into [`state_before`](Self::state_before).
    pub fn exit_needs_state_restore(&self) -> bool {
        todo!("W8-03b")
    }

    /// `current`: the window's state now; `Minimized` is remembered as `Normal`. `hwnd == 0`: not shown yet, nothing to flip.
    pub fn enter(&mut self, ops: &dyn WindowOps, hwnd: isize, current: WindowShowState) {
        todo!("W8-03b")
    }

    /// Leaves fullscreen: restores the style and moves the window straight back (see the contract for the three branches).
    pub fn exit(&mut self, ops: &dyn WindowOps, hwnd: isize) {
        todo!("W8-03b")
    }
}
