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
    if works.is_empty() {
        return saved;
    }

    let band_bottom = saved.bottom.min(saved.top.wrapping_add(CAPTION_BAND));
    for w in works {
        let iw = saved.right.min(w.right).wrapping_sub(saved.left.max(w.left));
        let ih = band_bottom.min(w.bottom).wrapping_sub(saved.top.max(w.top));
        if iw >= MIN_VISIBLE_WIDTH && ih >= MIN_VISIBLE_HEIGHT {
            return saved;
        }
    }

    let cx = saved.left.wrapping_add(saved.right) / 2;
    let cy = saved.top.wrapping_add(saved.bottom) / 2;
    let mut best = works[0];
    let mut best_dist = i64::MAX;
    for w in works {
        let dx = w.left.wrapping_sub(cx).max(cx.wrapping_sub(w.right)).max(0) as i64;
        let dy = w.top.wrapping_sub(cy).max(cy.wrapping_sub(w.bottom)).max(0) as i64;
        let d = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
        if d < best_dist {
            best_dist = d;
            best = *w;
        }
    }

    let width = saved.width().min(best.width());
    let height = saved.height().min(best.height());
    // `value.max(lo).min(hi)`, never `i32::clamp` (panics when lo > hi).
    let left = saved.left.max(best.left).min(best.right.wrapping_sub(width));
    let top = saved.top.max(best.top).min(best.bottom.wrapping_sub(height));
    ScreenRect::new(left, top, left.wrapping_add(width), top.wrapping_add(height))
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
        let zero = ScreenRect::new(0, 0, 0, 0);
        Self {
            layout,
            state_before: WindowShowState::Normal,
            normal_bounds: None,
            window_rect_before: zero,
            work_before: zero,
            style_before: 0,
            monitor_before: 0,
            enter_needs_maximized_state: false,
            exit_needs_state_restore: false,
        }
    }

    /// The state the window had before fullscreen (a minimized window counts as Normal). Initially Normal.
    pub fn state_before(&self) -> WindowShowState {
        self.state_before
    }

    /// Restore bounds (`rcNormalPosition`, physical px) saved when fullscreen was entered, as `(left, top, right, bottom)`;
    /// `None` while not fullscreen or when none could be saved.
    pub fn normal_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        self.normal_bounds.map(|r| (r.left, r.top, r.right, r.bottom))
    }

    /// After `enter`: true when the OS window was NOT moved onto the monitor (no handle yet, or no monitor info), so the
    /// caller must maximize the window itself to get a fullscreen-sized window.
    pub fn enter_needs_maximized_state(&self) -> bool {
        self.enter_needs_maximized_state
    }

    /// After `exit`: true when the controller did not restore the OS window itself (no handle, no saved bounds, or no
    /// monitor info), so the caller must put the window back into [`state_before`](Self::state_before).
    pub fn exit_needs_state_restore(&self) -> bool {
        self.exit_needs_state_restore
    }

    /// `current`: the window's state now; `Minimized` is remembered as `Normal`. `hwnd == 0`: not shown yet, nothing to flip.
    pub fn enter(&mut self, ops: &dyn WindowOps, hwnd: isize, current: WindowShowState) {
        self.state_before = if current == WindowShowState::Minimized {
            WindowShowState::Normal
        } else {
            current
        };
        self.enter_needs_maximized_state = true;
        if hwnd == 0 {
            self.normal_bounds = None;
            return;
        }

        if ops.is_iconic(hwnd) {
            ops.show_normal(hwnd);
        }
        self.normal_bounds = ops.normal_position(hwnd);
        self.window_rect_before = ops.window_rect(hwnd).unwrap_or(ScreenRect::new(0, 0, 0, 0));

        match ops.nearest_monitor(hwnd) {
            Some(m) => {
                self.monitor_before = m.handle;
                self.work_before = m.work;
                self.style_before = ops.window_style(hwnd);
                ops.set_window_style(hwnd, self.style_before & !WS_CHROME);
                ops.set_window_pos(hwnd, m.monitor, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
                self.enter_needs_maximized_state = false;
            }
            None => {
                self.monitor_before = 0;
            }
        }
    }

    /// Leaves fullscreen: restores the style and moves the window straight back (see the contract for the three branches).
    pub fn exit(&mut self, ops: &dyn WindowOps, hwnd: isize) {
        if hwnd == 0 || self.normal_bounds.is_none() {
            self.exit_needs_state_restore = true;
            return;
        }
        if self.monitor_before == 0 {
            self.exit_needs_state_restore = true;
            self.normal_bounds = None;
            return;
        }

        self.exit_needs_state_restore = false;
        let same_work = self.layout.monitor_of(hwnd) == self.monitor_before
            && self.layout.try_get_work(self.monitor_before) == Some(self.work_before);

        ops.set_window_style(hwnd, self.style_before);

        if same_work || self.state_before != WindowShowState::Maximized {
            let mut b = self.window_rect_before;
            if !same_work {
                b = resolve_normal_exit_rect(b, &self.layout.work_areas());
            }
            ops.set_window_pos(hwnd, b, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        } else {
            ops.set_window_pos(
                hwnd,
                ScreenRect::new(0, 0, 0, 0),
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            ops.show_maximized(hwnd);
        }
        self.normal_bounds = None;
    }
}
