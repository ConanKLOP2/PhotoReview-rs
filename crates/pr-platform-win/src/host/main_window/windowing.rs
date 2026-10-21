//! Port of `PhotoReview.Shell.Win32/Windowing/{ShellFullscreen.cs (163), TitleBar.cs (52), ShellMainWindow.Windowing.cs (127)}` (baseline `d92d6aae`) and of
//! `PhotoReview.App.Shared/Windowing/MonitorLayout.cs` lines 25-80 (`Win32MonitorLayout`, owner decided by W8-03a note 21). Unit W8-13k.
//! Contract: `contracts/W8-13k.md`.
//!
//! The pure state machine of F11 is `pr_app::windowing::fullscreen::FullscreenController` (W8-03b); this file adds what C# put around it: the real
//! `WindowOps` ([`Win32WindowOps`], the `LibraryImport` block of `FullscreenController`), the `IFullscreenNative` seam ([`FullscreenNative`] and
//! [`Win32FullscreenNative`]), the idempotent wrapper [`ShellFullscreen`] (active flag, maximise fallback, `Refit` onto the monitor after a display or dpi
//! change), the dark title bar ([`apply_title_bar`] over the [`DwmWindowAttributes`] seam), the real [`Win32MonitorLayout`], and the window glue
//! (`register_windowing`, `apply_fullscreen`, `on_windowing_dpi_changed`, `on_display_changed`, [`DisplayChangeHandler`]).
//!
//! The WPF `FullscreenWindowPlacer` (63 lines) is retired (the WPF window is gone). `dark_title_bar::WindowsDarkTitleBar` (Platform.Windows) is left as is:
//! dialogs keep using it; the shell title bar goes through the seam so that its six C# tests port (D-W8B4M-9).

#![allow(unused_variables, dead_code)] // W8-13k stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::windowing::contracts::WindowShowState;
use pr_app::windowing::fullscreen::{FullscreenController, MonitorSnapshot, WindowOps};
use pr_app::windowing::monitor_layout::{MonitorLayout, ScreenRect};

use super::core::{MainWindow, MainWindowInner};
use crate::host::contracts::{WindowMessage, WindowMessageHandler};
use crate::win32::structs::{Hwnd, Lresult};

/// C# `TitleBar.CaptionColorBgr` = `Dark.Window` #171717 as a `COLORREF` (`0x00BBGGRR`).
pub const CAPTION_COLOR_BGR: u32 = 0x0017_1717;
/// C# `TitleBar.DwmwaUseImmersiveDarkModeLegacy` = 19 (Windows 10 before build 18985). The current attribute (20) is `win32::api::dwmapi::DWMWA_USE_IMMERSIVE_DARK_MODE`
/// and the caption colour attribute (35) `DWMWA_CAPTION_COLOR`.
pub const DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY: u32 = 19;

// ---- the dark title bar --------------------------------------------------------------------------------------------------------------------------

/// C# `IDwmWindowAttributes`: the seam of [`apply_title_bar`] on `DwmSetWindowAttribute` (tests record the calls; no DWM needed).
pub trait DwmWindowAttributes {
    /// `true` when DWM accepted the attribute (`S_OK`); `false` on a Windows that does not know it (not an error).
    fn set(&self, hwnd: Hwnd, attribute: u32, value: u32) -> bool;
}

/// C# `DwmWindowAttributes.Instance`: `dwmapi::dwm_set_window_attribute(hwnd, attribute, &value)` with `Result::is_ok`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32DwmWindowAttributes;

impl DwmWindowAttributes for Win32DwmWindowAttributes {
    fn set(&self, hwnd: Hwnd, attribute: u32, value: u32) -> bool {
        todo!("W8-13k")
    }
}

/// C# `TitleBar.Apply(hwnd, dwm)`: `hwnd == 0` -> `false`, no call. Else `dark = dwm.set(hwnd, 20, 1) || dwm.set(hwnd, 19, 1)` (19 only when 20 failed),
/// then ALWAYS `dwm.set(hwnd, 35, CAPTION_COLOR_BGR)` (result ignored); returns `dark`. Never panics for an OS refusal. `dwm = None` uses
/// [`Win32DwmWindowAttributes`]. (The C# `catch (DllNotFoundException or EntryPointNotFoundException)` -> `false` is inside the real seam: `dwmapi` is
/// bound at run time, a missing library makes `set` return `false`.)
pub fn apply_title_bar(hwnd: Hwnd, dwm: Option<&dyn DwmWindowAttributes>) -> bool {
    todo!("W8-13k")
}

// ---- the real WindowOps, FullscreenNative and MonitorLayout ---------------------------------------------------------------------------------------

/// The user32 calls of `FullscreenController` (C# `LibraryImport` block), on the `windows` crate through `win32::api` / `win32::startup`.
/// Stateless: `Copy`, no `unsafe` of its own except the one `SetWindowLongPtr` wrapper call.
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32WindowOps;

impl WindowOps for Win32WindowOps {
    /// `win32::startup::is_iconic`.
    fn is_iconic(&self, hwnd: isize) -> bool {
        todo!("W8-13k")
    }
    /// `show_window(hwnd, SW_SHOWNORMAL)`.
    fn show_normal(&self, hwnd: isize) {
        todo!("W8-13k")
    }
    /// `show_window(hwnd, SW_MAXIMIZE)`.
    fn show_maximized(&self, hwnd: isize) {
        todo!("W8-13k")
    }
    /// `get_window_placement` -> `rcNormalPosition`; `None` when the call fails.
    fn normal_position(&self, hwnd: isize) -> Option<ScreenRect> {
        todo!("W8-13k")
    }
    /// `get_window_rect`; `None` when the call fails.
    fn window_rect(&self, hwnd: isize) -> Option<ScreenRect> {
        todo!("W8-13k")
    }
    /// `monitor_from_window(hwnd, MONITOR_DEFAULTTONEAREST)` (0 -> `None`) then `get_monitor_info` (failure -> `None`): handle, `rcMonitor`, `rcWork`.
    fn nearest_monitor(&self, hwnd: isize) -> Option<MonitorSnapshot> {
        todo!("W8-13k")
    }
    /// `get_window_long_ptr(hwnd, GWL_STYLE)`.
    fn window_style(&self, hwnd: isize) -> isize {
        todo!("W8-13k")
    }
    /// `set_window_long_ptr(hwnd, GWL_STYLE, style)` (the one `unsafe` call; `// SAFETY:` = a style bit set, no pointer).
    fn set_window_style(&self, hwnd: isize, style: isize) {
        todo!("W8-13k")
    }
    /// `set_window_pos(hwnd, 0, left, top, width, height, flags)`, width/height by wrapping subtraction (`ScreenRect::width/height`); an `Err` is ignored (C# ignored
    /// the `BOOL`).
    fn set_window_pos(&self, hwnd: isize, rect: ScreenRect, flags: u32) {
        todo!("W8-13k")
    }
}

/// C# `IFullscreenNative` (mutable seam of [`ShellFullscreen`]; all coordinates physical pixels).
pub trait FullscreenNative {
    /// Minimised -> `Minimized`, maximised -> `Maximized`, else `Normal`.
    fn current_state(&self, hwnd: Hwnd) -> WindowShowState;
    /// The whole monitor (`rcMonitor`, taskbar included) nearest to the window; `None` when unknown.
    fn try_get_monitor_rect(&self, hwnd: Hwnd) -> Option<ScreenRect>;
    /// `GetWindowRect`; `None` on failure.
    fn try_get_window_rect(&self, hwnd: Hwnd) -> Option<ScreenRect>;
    /// ONE `SetWindowPos(hwnd, 0, rect, SWP_NOZORDER | SWP_NOACTIVATE)`.
    fn move_to(&self, hwnd: Hwnd, rect: ScreenRect);
    /// The fallback when there is no monitor information: `ShowWindow(Maximized ? SW_SHOWMAXIMIZED : SW_SHOWNORMAL)`.
    fn show(&self, hwnd: Hwnd, state: WindowShowState);
}

/// C# `Win32FullscreenNative.Instance`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32FullscreenNative;

impl FullscreenNative for Win32FullscreenNative {
    /// `is_iconic` -> Minimized; `IsZoomed` (the `windows` crate, one `unsafe` call with `// SAFETY:`; `win32::api` has no wrapper, D-W8B4M-9) -> Maximized; else Normal.
    fn current_state(&self, hwnd: Hwnd) -> WindowShowState {
        todo!("W8-13k")
    }
    /// `MonitorFromWindow(DEFAULTTONEAREST)`; a null handle or a failing `GetMonitorInfo` -> `None`.
    fn try_get_monitor_rect(&self, hwnd: Hwnd) -> Option<ScreenRect> {
        todo!("W8-13k")
    }
    fn try_get_window_rect(&self, hwnd: Hwnd) -> Option<ScreenRect> {
        todo!("W8-13k")
    }
    fn move_to(&self, hwnd: Hwnd, rect: ScreenRect) {
        todo!("W8-13k")
    }
    fn show(&self, hwnd: Hwnd, state: WindowShowState) {
        todo!("W8-13k")
    }
}

/// C# `Win32MonitorLayout` (`MonitorLayout.cs` 25-80): `monitor_of` = `monitor_from_window(hwnd, MONITOR_DEFAULTTONEAREST)`; `try_get_work(m)` = `m != 0 &&
/// get_monitor_info(m)` -> `rcWork`; `work_areas()` = `enum_display_monitors(None)` mapped through `try_get_work` (a monitor whose info call fails is skipped,
/// enumeration order kept; a failing enumeration gives an empty list). `Send + Sync` as the trait demands (zero-sized).
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32MonitorLayout;

impl MonitorLayout for Win32MonitorLayout {
    fn monitor_of(&self, hwnd: isize) -> isize {
        todo!("W8-13k")
    }
    fn try_get_work(&self, monitor: isize) -> Option<ScreenRect> {
        todo!("W8-13k")
    }
    fn work_areas(&self) -> Vec<ScreenRect> {
        todo!("W8-13k")
    }
}

// ---- ShellFullscreen -----------------------------------------------------------------------------------------------------------------------------

/// C# `ShellFullscreen`: F11 on the shell's HWND. All methods take `&self` (the window procedure re-enters while `apply` runs: `SetWindowPos` raises
/// `WM_SIZE`, possibly `WM_DPICHANGED`), the controller sits in a `RefCell` that is borrowed ONLY around `enter`/`exit`, and `refit` uses `try_borrow`
/// (D-W8B4M-10): a re-entrant `refit` during `enter`/`exit` returns `false`.
pub struct ShellFullscreen {
    controller: RefCell<FullscreenController>,
    ops: Rc<dyn WindowOps>,
    native: Rc<dyn FullscreenNative>,
    active: Cell<bool>,
}

impl std::fmt::Debug for ShellFullscreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellFullscreen")
            .field("active", &self.active.get())
            .finish_non_exhaustive()
    }
}

impl ShellFullscreen {
    /// C# constructor: `controller` over the monitor layout, `ops` for the controller's own user32 calls, `native` for the fallback and `refit`.
    pub fn new(controller: FullscreenController, ops: Rc<dyn WindowOps>, native: Rc<dyn FullscreenNative>) -> Self {
        todo!("W8-13k")
    }

    /// C# `IsActive`.
    pub fn is_active(&self) -> bool {
        todo!("W8-13k")
    }

    /// C# `RestoreState`: `controller.state_before()` while active, else `None` (R7-10).
    pub fn restore_state(&self) -> Option<WindowShowState> {
        todo!("W8-13k")
    }

    /// C# `RestoreBounds`: the Normal rect (physical pixels) at the moment F11 was entered while active, else `None`.
    pub fn restore_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        todo!("W8-13k")
    }

    /// C# `Apply(hwnd, fullscreen)`: `hwnd == 0` or `fullscreen == active` -> `false` and NOTHING touched (idempotent; a second enter must not overwrite the
    /// state from before the first). Enter: `controller.enter(ops, hwnd, native.current_state(hwnd))`, `active = true`, and when
    /// `controller.enter_needs_maximized_state()` (no monitor information) `native.show(hwnd, Maximized)`. Exit: `controller.exit(ops, hwnd)`, `active = false`,
    /// and when `controller.exit_needs_state_restore()` `native.show(hwnd, controller.state_before())`. `true` = the state really changed.
    pub fn apply(&self, hwnd: Hwnd, fullscreen: bool) -> bool {
        todo!("W8-13k")
    }

    /// C# `Refit(hwnd)`: only while active, with a handle, and not in the maximise fallback; needs both `try_get_monitor_rect` and `try_get_window_rect` and a window
    /// that differs from the monitor; then `native.move_to(hwnd, monitor)` and `true`. Re-entrant during `apply` -> `false`.
    pub fn refit(&self, hwnd: Hwnd) -> bool {
        todo!("W8-13k")
    }

    /// C# `Reset()`: the window was destroyed in F11; drop the state.
    pub fn reset(&self) {
        todo!("W8-13k")
    }
}

// ---- seams and window glue ---------------------------------------------------------------------------------------------------------------------------

/// C# `ShellWindowingSeams(Native, MonitorLayout, Dwm)` plus the `WindowOps` of the controller (C# let the controller call user32 directly). `None` = real Win32.
#[derive(Default)]
pub struct ShellWindowingSeams {
    /// The F11 fallback and `refit` operations.
    pub native: Option<Rc<dyn FullscreenNative>>,
    /// The controller's user32 calls.
    pub ops: Option<Rc<dyn WindowOps>>,
    /// The monitor layout of `FullscreenController::exit` (work areas for the saved rect).
    pub monitor_layout: Option<Box<dyn MonitorLayout>>,
    /// `DwmSetWindowAttribute`.
    pub dwm: Option<Rc<dyn DwmWindowAttributes>>,
}

impl std::fmt::Debug for ShellWindowingSeams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellWindowingSeams").finish_non_exhaustive()
    }
}

/// The fields that `ShellMainWindow.Windowing.cs` added: `_fullscreen`, `_displayChangeHandler`, `_windowingDisposed`.
pub struct WindowingState {
    pub(super) fullscreen: RefCell<Option<Rc<ShellFullscreen>>>,
    pub(super) display_change_handler: RefCell<Option<Rc<DisplayChangeHandler>>>,
    pub(super) windowing_disposed: Cell<bool>,
    /// Id of the `DpiChanged` subscription (`OnWindowingDpiChanged`).
    pub(super) dpi_subscription: Cell<Option<u64>>,
}

impl std::fmt::Debug for WindowingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowingState").finish_non_exhaustive()
    }
}

impl Default for WindowingState {
    /// Nothing registered.
    fn default() -> Self {
        todo!("W8-13k")
    }
}

/// C# `DisplayChangeHandler(owner)`: observes `WM_DISPLAYCHANGE` and ALWAYS returns `None` (default processing still runs). Holds a `Weak`.
pub struct DisplayChangeHandler {
    owner: std::rc::Weak<MainWindowInner>,
}

impl std::fmt::Debug for DisplayChangeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayChangeHandler").finish_non_exhaustive()
    }
}

impl WindowMessageHandler for DisplayChangeHandler {
    /// `WM_DISPLAYCHANGE` -> `owner.on_display_changed()`; every message -> `None`.
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        todo!("W8-13k")
    }
}

impl MainWindowInner {
    /// C# `RegisterWindowing()` (34-45): seams = options or real; `apply_title_bar(hwnd, seams.dwm)` BEFORE the window is revealed (the cloak is still on);
    /// build `ShellFullscreen`; `window.add_message_handler(DisplayChangeHandler)`; subscribe `DpiChanged` -> `on_windowing_dpi_changed` (BEFORE the core's
    /// `DpiChanged` handler, because `register_features` runs first). The viewer `IsFullscreen` subscription is the `ChangeSet` step of `apply_changes`.
    pub(super) fn register_windowing(&self) {
        todo!("W8-13k")
    }

    /// C# `DisposeWindowing()` (47-58), once: unsubscribe `DpiChanged`; `fullscreen.reset()`.
    pub(super) fn dispose_windowing(&self) {
        todo!("W8-13k")
    }

    /// C# `ApplyFullscreen(bool)` (72-82): `fullscreen.apply(hwnd, value)`; on a real change `viewport.invalidate_layout()` and `window.invalidate()`; returns it.
    pub(super) fn apply_fullscreen(&self, fullscreen: bool) -> bool {
        todo!("W8-13k")
    }

    /// C# `OnWindowingViewerChanged` for `IsFullscreen` (61-69): not disposed, window not destroyed -> `apply_fullscreen(viewer.is_fullscreen())`.
    pub(super) fn on_viewer_fullscreen_changed(&self) {
        todo!("W8-13k")
    }

    /// C# `OnWindowingDpiChanged` (88-94): unless disposed, `fullscreen.refit(hwnd)`.
    pub(super) fn on_windowing_dpi_changed(&self) {
        todo!("W8-13k")
    }

    /// C# `OnDisplayChanged()` (97-112): when not disposed and F11 is active, post (`UiPriority::Render`, through `post_to_window`) a job that, if the window is alive,
    /// not destroyed and `fullscreen.refit(hwnd)` moved it, calls `viewport.invalidate_layout()` and `window.invalidate()`. Not in F11: nothing.
    pub(super) fn on_display_changed(&self) {
        todo!("W8-13k")
    }
}

impl MainWindow {
    /// C# `ApplyFullscreen(bool)` (`internal`; the startup code, the menu and tests call it).
    pub fn apply_fullscreen(&self, fullscreen: bool) -> bool {
        todo!("W8-13k")
    }

    /// C# `Fullscreen` (diagnostics and tests).
    pub fn fullscreen(&self) -> Option<Rc<ShellFullscreen>> {
        todo!("W8-13k")
    }
}
