//! Port of `PhotoReview.Shell.Win32/Hosting/ShellWindow.cs` lines 1-250 (baseline `d92d6aae`) plus the helpers its code calls. Unit W8-13e.
//!
//! Options, the window class, `CreateWindowEx` with the trampoline cookie, the public API of `IShellWindow` (events, handlers, `Invalidate`, title, cursor,
//! capture, `ScreenToClientDip`, `Close`, `Show`, `Dispose`) and the lifecycle messages (`WM_NCCREATE`, `WM_CREATE`, `WM_DESTROY`, `WM_NCDESTROY`).
//!
//! Cut moved from the plan (D-W8B3H-13): 13e also owns the pieces of C# lines 250-516 that its own code calls, so 13e never calls 13f code:
//! `apply_initial_size` (319-336), `to_pixels` (450), `free_self` (452-458), `verify_access` (460-466), `cursor_handle`/`SystemCursors` (472-477, 510-515),
//! `ensure_class_registered` (479-507, called from the constructor), `is_pointer_over_client` (444-448), `on_destroy` (413-429), `on_frame` (431-442),
//! `on_native_destroyed` (317), and the four lifecycle arms of `ProcessMessage` (`on_nc_create`, `on_create`). 13f owns the rest (see `messages.rs`, `dpi.rs`).
//!
//! Rust shape: `ShellWindow` (owner, `Drop` = C# `Dispose`) over `Rc<ShellWindowInner>`; the inner state is `Cell`/`RefCell` because the window procedure
//! re-enters it. Events are subscribe/unsubscribe lists ([`EventList`]); handlers run with no borrow held, so a handler may subscribe, unsubscribe
//! or call back into the window. `!Send`. The window class is `PhotoReviewRs.Shell.Window` (R0002, D-W8B3H-9).

#![allow(unused_variables, dead_code)] // W8-13e stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::host::contracts::{
    ClosingHandler, EventHandlerId, PointD, ShellCursor, ShellWindowApi, ShellWindowEvent, SizeD, WindowEventHandler,
    WindowMessageHandler,
};
use crate::host::error::HostError;
use crate::host::frame_clock::base::{FrameHandlerId, FrameTick, HostFrameClock};
use crate::host::wndproc::WindowTarget;
use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};

/// C# `ClassName`, with the Rust prefix.
pub const CLASS_NAME: &str = "PhotoReviewRs.Shell.Window";
/// C# `DefaultDpi`.
pub const DEFAULT_DPI: u32 = 96;

/// C# `ShellWindowOptions` (`sealed record`): sizes in DIP, multiplied by the dpi of the monitor the window opens on.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellWindowOptions {
    /// Default `"PhotoReview"` (the C# literal; the composition root sets the real title text right after creation).
    pub title: String,
    /// Default 1024.
    pub width_dip: f64,
    /// Default 720.
    pub height_dip: f64,
    /// Default 320.
    pub min_width_dip: f64,
    /// Default 240.
    pub min_height_dip: f64,
    /// `true`: `width_dip`/`height_dip` are the OUTER size (frame included, WPF `Window.Width` semantics, WP-20); `false`: the client size.
    pub size_is_outer: bool,
}

impl Default for ShellWindowOptions {
    fn default() -> Self {
        todo!("W8-13e")
    }
}

/// A list of event handlers (the Rust form of a C# multicast event). Handlers run in subscription order, with NO borrow held (each is taken out of its
/// slot for the call), so a handler may call `subscribe`/`unsubscribe` on the same list: a handler removed while running is dropped when it returns;
/// one added while running is NOT called by the current `raise` (C# delegate snapshot).
pub struct EventList<H> {
    slots: RefCell<Vec<EventSlot<H>>>,
    next_id: Cell<EventHandlerId>,
}

struct EventSlot<H> {
    id: EventHandlerId,
    event: ShellWindowEvent,
    handler: Option<H>,
    removed: bool,
}

impl<H> fmt::Debug for EventList<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventList").finish_non_exhaustive()
    }
}

impl<H> EventList<H> {
    pub fn new() -> Self {
        todo!("W8-13e")
    }
    pub fn add(&self, event: ShellWindowEvent, handler: H) -> EventHandlerId {
        todo!("W8-13e")
    }
    pub fn remove(&self, id: EventHandlerId) -> bool {
        todo!("W8-13e")
    }
    /// Calls `call(&mut handler)` for each handler of `event`, in order. A panic in a handler propagates after the handler is put back (the C# exception
    /// leaves the subscription in place).
    pub fn raise(&self, event: ShellWindowEvent, call: &mut dyn FnMut(&mut H)) {
        todo!("W8-13e")
    }
}

impl<H> Default for EventList<H> {
    fn default() -> Self {
        Self::new()
    }
}

/// A `Render` subscriber.
pub type RenderHandler = Box<dyn FnMut(&FrameTick)>;

/// All state of a shell window (C# fields, lines 39-54). Shared with the window procedure through the cookie.
pub(super) struct ShellWindowInner {
    pub(super) frame_clock: Rc<dyn HostFrameClock>,
    pub(super) options: ShellWindowOptions,
    /// C# `_handlers` (registration order; `insert_message_handler_first` puts one at the head).
    pub(super) handlers: RefCell<Vec<Rc<dyn WindowMessageHandler>>>,
    /// Handlers of the parameterless events (`ClientSizeChanged`, `DpiChanged`, `Activated`, `Deactivated`, `Closed`).
    pub(super) events: EventList<WindowEventHandler>,
    /// `Closing` handlers (the event argument can cancel).
    pub(super) closing: EventList<ClosingHandler>,
    /// C# `Render` event (an internal event: the renderer subscribes; it receives the frame tick).
    pub(super) render: EventList<RenderHandler>,
    /// Cookie from `wndproc::make_cookie`; `0` once freed (C# `_self` GCHandle).
    pub(super) cookie: Cell<isize>,
    pub(super) hwnd: Cell<Hwnd>,
    pub(super) dpi: Cell<u32>,
    pub(super) client_width_px: Cell<i32>,
    pub(super) client_height_px: Cell<i32>,
    pub(super) last_raised_client_size: Cell<SizeD>,
    pub(super) cursor: Cell<ShellCursor>,
    pub(super) created: Cell<bool>,
    pub(super) destroyed: Cell<bool>,
    /// Default `true` (C# `_surfaceReady = true`).
    pub(super) surface_ready: Cell<bool>,
    pub(super) render_requested: Cell<bool>,
    pub(super) is_active: Cell<bool>,
    pub(super) render_count: Cell<i64>,
    /// The id of the one frame-clock subscription while `render_requested` (C# `_onFrame` delegate identity).
    pub(super) frame_subscription: Cell<Option<FrameHandlerId>>,
}

impl fmt::Debug for ShellWindowInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellWindowInner")
            .field("hwnd", &self.hwnd.get())
            .finish_non_exhaustive()
    }
}

impl WindowTarget for ShellWindowInner {
    /// Forwards to `ShellWindowInner::process_message` (13f; `messages.rs`).
    fn process_message(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        todo!("W8-13e")
    }

    /// C# `OnNativeDestroyed` = `FreeSelf`.
    fn on_native_destroyed(&self) {
        todo!("W8-13e")
    }
}

impl ShellWindowInner {
    /// C# `WM_NCCREATE` arm: `hwnd = hwnd`; `dpi = GetDpiForWindow(hwnd)` (0 -> `DEFAULT_DPI`); returns `DefWindowProc(..)`.
    pub(super) fn on_nc_create(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        todo!("W8-13e")
    }

    /// C# `WM_CREATE` arm: `created = true`; `icons::apply(hwnd, dpi)`; returns 0.
    pub(super) fn on_create(&self, hwnd: Hwnd) -> Lresult {
        todo!("W8-13e")
    }

    /// C# `OnDestroy()` (`WM_DESTROY`): `destroyed = true`, `is_active = false`; release the pointer capture if this window has it; if a render is requested,
    /// clear the flag and unsubscribe from the frame clock; raise `Closed`.
    pub(super) fn on_destroy(&self) {
        todo!("W8-13e")
    }

    /// C# `OnFrame(sender, e)`: unsubscribe from the clock, clear `render_requested`; destroyed -> return; `render_count += 1`; raise `Render` with the tick.
    pub(super) fn on_frame(&self, tick: &FrameTick) {
        todo!("W8-13e")
    }

    /// C# `ApplyInitialSize()` (lines 319-336): `rect = (0, 0, to_pixels(width_dip), to_pixels(height_dip))`; unless `size_is_outer`, grow it with
    /// `AdjustWindowRectExForDpi(WS_OVERLAPPEDWINDOW, no menu, ex_style 0, dpi)` (a failure there skips the resize, like the C# `if`);
    /// `SetWindowPos(hwnd, 0, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE)`, then `rethrow_pending()`; finally read the client rect into
    /// `client_width_px`/`client_height_px` (only when `GetClientRect` succeeds).
    pub(super) fn apply_initial_size(&self) {
        todo!("W8-13e")
    }

    /// C# `IsPointerOverClient()`: `GetCursorPos` and `WindowFromPoint(pt) == hwnd`.
    pub(super) fn is_pointer_over_client(&self) -> bool {
        todo!("W8-13e")
    }

    /// C# `FreeSelf()`: idempotent `wndproc::free_cookie`.
    pub(super) fn free_self(&self) {
        todo!("W8-13e")
    }

    /// C# `ClientSizeDip` = `(client_width_px / scale, client_height_px / scale)`.
    pub(super) fn client_size_dip(&self) -> SizeD {
        todo!("W8-13e")
    }
}

/// C# `DpiScale` = `dpi / 96.0`.
pub(super) fn dpi_scale_of(dpi: u32) -> f64 {
    todo!("W8-13e")
}

/// C# `ToPixels(dip)` = `(int)Math.Round(dip * scale)`: ties-to-even (`round_ties_even`), then a saturating cast (NaN gives 0).
pub fn to_pixels(dip: f64, scale: f64) -> i32 {
    todo!("W8-13e")
}

/// C# `CursorHandle(cursor)`: the shared system cursor (`IDC_ARROW`, `IDC_SIZEALL`, `IDC_WAIT`), loaded once per process.
pub(super) fn cursor_handle(cursor: ShellCursor) -> isize {
    todo!("W8-13e")
}

/// C# `EnsureClassRegistered()`: once per process, only a success is remembered. Class: `shell_window_proc`, `hInstance = GetModuleHandle(0)`,
/// big/small icon `icons::load_big(0)`/`load_small(0)`, no background brush (the renderer paints everything; `WM_ERASEBKGND` returns 1).
/// `Err(Win32 "RegisterClassEx (shell window) failed")`.
pub(super) fn ensure_class_registered() -> Result<(), HostError> {
    todo!("W8-13e")
}

/// C# `ShellWindow` (the owner handle). `Drop` is C# `Dispose()` (destroys the window if it still exists; does not resume a captured panic while the
/// thread is already panicking).
pub struct ShellWindow {
    pub(super) inner: Rc<ShellWindowInner>,
}

impl fmt::Debug for ShellWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellWindow")
            .field("hwnd", &self.inner.hwnd.get())
            .finish_non_exhaustive()
    }
}

impl ShellWindow {
    /// C# constructor (lines 56-79): `ensure_class_registered`; allocate the cookie; `CreateWindowEx(0, class, title, WS_OVERLAPPEDWINDOW, CW_USEDEFAULT x4,
    /// no parent, no menu, exe module, cookie)`; on failure read the error FIRST, free the cookie, `rethrow_pending()` (a panic wins), then
    /// `Err(Win32 "CreateWindowEx (shell window) failed")`; on success `rethrow_pending()`, `apply_initial_size()`, `last_raised_client_size = client_size_dip()`.
    pub fn create(options: ShellWindowOptions, frame_clock: Rc<dyn HostFrameClock>) -> Result<ShellWindow, HostError> {
        todo!("W8-13e")
    }

    /// Client size in device pixels (C# `ClientSizePixels`), for the renderer.
    pub fn client_size_pixels(&self) -> (i32, i32) {
        todo!("W8-13e")
    }

    /// C# `IsDestroyed`.
    pub fn is_destroyed(&self) -> bool {
        todo!("W8-13e")
    }

    /// C# `Cursor`.
    pub fn cursor(&self) -> ShellCursor {
        todo!("W8-13e")
    }

    /// C# `RenderCount`.
    pub fn render_count(&self) -> i64 {
        todo!("W8-13e")
    }

    /// C# `IsRenderRequested`.
    pub fn is_render_requested(&self) -> bool {
        todo!("W8-13e")
    }

    /// C# internal `Render` event: raised once per `Invalidate` burst, at the next frame tick.
    pub fn subscribe_render(&self, handler: RenderHandler) -> EventHandlerId {
        todo!("W8-13e")
    }

    /// C# internal `InsertMessageHandlerFirst` (overlay and the keep-warm listener).
    pub fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        todo!("W8-13e")
    }

    /// C# internal `Show(bool activate)`: `SW_SHOWNORMAL` or `SW_SHOWNOACTIVATE`.
    pub fn show(&self, activate: bool) {
        todo!("W8-13e")
    }

    /// C# internal `Show(int showCommand)`: `ShowWindow`, then `rethrow_pending()`.
    pub fn show_command(&self, show_command: i32) {
        todo!("W8-13e")
    }

    /// C# internal `SetSurfaceReady(bool)`.
    pub fn set_surface_ready(&self, ready: bool) {
        todo!("W8-13e")
    }

    /// C# `Dispose()`: if the window exists and is not destroyed, `DestroyWindow` then `rethrow_pending()`; always `free_self()`.
    pub fn dispose(&self) {
        todo!("W8-13e")
    }
}

impl Drop for ShellWindow {
    fn drop(&mut self) {
        todo!("W8-13e")
    }
}

impl ShellWindowApi for ShellWindow {
    fn hwnd(&self) -> Hwnd {
        todo!("W8-13e")
    }
    fn dpi_scale(&self) -> f64 {
        todo!("W8-13e")
    }
    fn client_size_dip(&self) -> SizeD {
        todo!("W8-13e")
    }
    fn is_active(&self) -> bool {
        todo!("W8-13e")
    }
    /// `created && !destroyed && surface_ready`.
    fn is_loaded(&self) -> bool {
        todo!("W8-13e")
    }
    fn subscribe(&self, event: ShellWindowEvent, handler: WindowEventHandler) -> EventHandlerId {
        todo!("W8-13e")
    }
    fn subscribe_closing(&self, handler: ClosingHandler) -> EventHandlerId {
        todo!("W8-13e")
    }
    fn unsubscribe(&self, id: EventHandlerId) {
        todo!("W8-13e")
    }
    fn add_message_handler(&self, handler: Rc<dyn WindowMessageHandler>) {
        todo!("W8-13e")
    }
    /// C# `Invalidate()`: no-op when a render is already requested or the window is destroyed; else `render_requested = true` and ONE frame-clock subscription.
    fn invalidate(&self) {
        todo!("W8-13e")
    }
    /// C# `SetTitle`: destroyed -> `Ok`; `SetWindowText` failure is `Err(Win32 "SetWindowText failed")`.
    fn set_title(&self, title: &str) -> Result<(), HostError> {
        todo!("W8-13e")
    }
    /// C# `SetCursor`: stores the cursor; applies it now if the window has the capture or the pointer is over the client area.
    fn set_cursor(&self, cursor: ShellCursor) {
        todo!("W8-13e")
    }
    fn capture_pointer(&self) {
        todo!("W8-13e")
    }
    fn release_pointer(&self) {
        todo!("W8-13e")
    }
    fn has_pointer_capture(&self) -> bool {
        todo!("W8-13e")
    }
    /// C# `ScreenToClientDip`: round both coordinates (ties-to-even) to a `POINT`, `ScreenToClient` unless destroyed, divide by `dpi_scale`.
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        todo!("W8-13e")
    }
    /// C# `Close()`: destroyed -> nothing; else `SendMessage(hwnd, WM_CLOSE, 0, 0)` then `rethrow_pending()`.
    fn close(&self) {
        todo!("W8-13e")
    }
}
