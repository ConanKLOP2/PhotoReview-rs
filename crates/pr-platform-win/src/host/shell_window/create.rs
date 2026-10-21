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

use std::cell::{Cell, RefCell};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::UI::WindowsAndMessaging::{HICON, WNDCLASSEXW};
use windows::core::PCWSTR;

use crate::host::contracts::{
    ClosingHandler, EventHandlerId, PointD, ShellCursor, ShellWindowApi, ShellWindowEvent, SizeD, WindowEventHandler,
    WindowMessageHandler, new_rect, rect_height, rect_width,
};
use crate::host::error::HostError;
use crate::host::frame_clock::base::{FrameHandlerId, FrameTick, HostFrameClock};
use crate::host::pending_panic::rethrow_pending;
use crate::host::wndproc::{WindowTarget, free_cookie, make_cookie, shell_window_proc};
use crate::host::{icons, native};
use crate::win32::api::{kernel32, user32};
use crate::win32::comctl::user32 as comctl_user32;
use crate::win32::dialog_native::user32 as dialog_user32;
use crate::win32::messages::{
    SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, WM_CLOSE, WS_OVERLAPPEDWINDOW,
};
use crate::win32::structs::{Hwnd, Lparam, Lresult, Point, Wparam, to_utf16z};

/// C# `ClassName`, with the Rust prefix.
pub const CLASS_NAME: &str = "PhotoReviewRs.Shell.Window";
/// C# `DefaultDpi`.
pub const DEFAULT_DPI: u32 = 96;

/// The `Closing` and `Render` lists have no event of their own in [`ShellWindowEvent`]; every handler of those lists is filed under this value.
pub(super) const LIST_EVENT: ShellWindowEvent = ShellWindowEvent::Closed;
/// First id of the `Closing` list (the three lists of a window use disjoint id ranges so `unsubscribe` can find the owner of an id).
const CLOSING_FIRST_ID: EventHandlerId = 1 << 40;
/// First id of the `Render` list.
const RENDER_FIRST_ID: EventHandlerId = 2 << 40;

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
        Self {
            title: "PhotoReview".to_owned(),
            width_dip: 1024.0,
            height_dip: 720.0,
            min_width_dip: 320.0,
            min_height_dip: 240.0,
            size_is_outer: false,
        }
    }
}

/// A list of event handlers (the Rust form of a C# multicast event). Handlers run in subscription order, with NO borrow held (each is taken out of its
/// slot for the call), so a handler may call `subscribe`/`unsubscribe` on the same list: a handler removed while running is dropped when it returns;
/// one added while running is NOT called by the current `raise` (C# delegate snapshot). A handler that is already running is skipped by a nested
/// `raise` of the same event (an `FnMut` cannot be re-entered).
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
    /// An empty list; ids start at 1.
    pub fn new() -> Self {
        Self::with_first_id(1)
    }

    /// An empty list whose first id is `first_id` (the lists of one window use disjoint id ranges).
    pub fn with_first_id(first_id: EventHandlerId) -> Self {
        Self {
            slots: RefCell::new(Vec::new()),
            next_id: Cell::new(first_id),
        }
    }

    /// Subscribes `handler` to `event`; returns the id for [`EventList::remove`].
    pub fn add(&self, event: ShellWindowEvent, handler: H) -> EventHandlerId {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.slots.borrow_mut().push(EventSlot {
            id,
            event,
            handler: Some(handler),
            removed: false,
        });
        id
    }

    /// Removes the subscription; `false` for an unknown (or already removed) id. A handler that is running is dropped when it returns.
    pub fn remove(&self, id: EventHandlerId) -> bool {
        let dropped = {
            let mut slots = self.slots.borrow_mut();
            let Some(index) = slots.iter().position(|slot| slot.id == id && !slot.removed) else {
                return false;
            };
            if slots[index].handler.is_some() {
                Some(slots.remove(index))
            } else {
                slots[index].removed = true;
                None
            }
        };
        // The handler's `Drop` may run arbitrary code: outside the borrow.
        drop(dropped);
        true
    }

    /// Calls `call(&mut handler)` for each handler of `event`, in order. A panic in a handler propagates after the handler is put back (the C# exception
    /// leaves the subscription in place).
    pub fn raise(&self, event: ShellWindowEvent, call: &mut dyn FnMut(&mut H)) {
        let ids: Vec<EventHandlerId> = self
            .slots
            .borrow()
            .iter()
            .filter(|slot| slot.event == event && !slot.removed)
            .map(|slot| slot.id)
            .collect();
        for id in ids {
            let taken = {
                let mut slots = self.slots.borrow_mut();
                slots
                    .iter_mut()
                    .find(|slot| slot.id == id && !slot.removed)
                    .and_then(|slot| slot.handler.take())
            };
            let Some(mut handler) = taken else {
                continue; // removed meanwhile, or already running further up the stack
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| call(&mut handler)));
            let discarded = {
                let mut slots = self.slots.borrow_mut();
                match slots.iter().position(|slot| slot.id == id) {
                    Some(index) if slots[index].removed => {
                        slots.remove(index);
                        Some(handler)
                    }
                    Some(index) => {
                        slots[index].handler = Some(handler);
                        None
                    }
                    None => Some(handler),
                }
            };
            drop(discarded);
            if let Err(payload) = outcome {
                resume_unwind(payload);
            }
        }
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
    /// The `Rc` that owns this value; the frame handler holds it weakly (a strong `Rc` in the clock would keep the window alive).
    pub(super) weak_self: Weak<ShellWindowInner>,
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
    /// Forwards to `ShellWindowInner::process_message` (`messages.rs`).
    fn process_message(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        ShellWindowInner::process_message(self, hwnd, message, wparam, lparam)
    }

    /// C# `OnNativeDestroyed` = `FreeSelf`.
    fn on_native_destroyed(&self) {
        self.free_self();
    }
}

impl ShellWindowInner {
    fn new(
        options: ShellWindowOptions,
        frame_clock: Rc<dyn HostFrameClock>,
        weak_self: Weak<ShellWindowInner>,
    ) -> Self {
        Self {
            frame_clock,
            options,
            weak_self,
            handlers: RefCell::new(Vec::new()),
            events: EventList::new(),
            closing: EventList::with_first_id(CLOSING_FIRST_ID),
            render: EventList::with_first_id(RENDER_FIRST_ID),
            cookie: Cell::new(0),
            hwnd: Cell::new(0),
            dpi: Cell::new(DEFAULT_DPI),
            client_width_px: Cell::new(0),
            client_height_px: Cell::new(0),
            last_raised_client_size: Cell::new(SizeD::default()),
            cursor: Cell::new(ShellCursor::Arrow),
            created: Cell::new(false),
            destroyed: Cell::new(false),
            surface_ready: Cell::new(true),
            render_requested: Cell::new(false),
            is_active: Cell::new(false),
            render_count: Cell::new(0),
            frame_subscription: Cell::new(None),
        }
    }

    /// C# `WM_NCCREATE` arm: `hwnd = hwnd`; `dpi = GetDpiForWindow(hwnd)` (0 -> `DEFAULT_DPI`); returns `DefWindowProc(..)`.
    pub(super) fn on_nc_create(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        self.hwnd.set(hwnd);
        let dpi = user32::get_dpi_for_window(hwnd);
        self.dpi.set(if dpi == 0 { DEFAULT_DPI } else { dpi });
        // SAFETY: the parameters are the ones the window procedure received for this message.
        unsafe { user32::def_window_proc(hwnd, message, wparam, lparam) }
    }

    /// C# `WM_CREATE` arm: `created = true`; `icons::apply(hwnd, dpi)`; returns 0.
    pub(super) fn on_create(&self, hwnd: Hwnd) -> Lresult {
        self.created.set(true);
        icons::apply(hwnd, self.dpi.get());
        0
    }

    /// C# `OnDestroy()` (`WM_DESTROY`): `destroyed = true`, `is_active = false`; release the pointer capture if this window has it; if a render is requested,
    /// clear the flag and unsubscribe from the frame clock; raise `Closed`.
    pub(super) fn on_destroy(&self) {
        self.destroyed.set(true);
        self.is_active.set(false);
        if native::get_capture() == self.hwnd.get() {
            user32::release_capture();
        }
        if self.render_requested.get() {
            self.render_requested.set(false);
            self.release_frame_subscription();
        }
        self.events
            .raise(ShellWindowEvent::Closed, &mut |handler: &mut WindowEventHandler| {
                handler()
            });
    }

    /// C# `OnFrame(sender, e)`: unsubscribe from the clock, clear `render_requested`; destroyed -> return; `render_count += 1`; raise `Render` with the tick.
    pub(super) fn on_frame(&self, tick: &FrameTick) {
        self.release_frame_subscription();
        self.render_requested.set(false);
        if self.destroyed.get() {
            return;
        }
        self.render_count.set(self.render_count.get() + 1);
        self.render
            .raise(LIST_EVENT, &mut |handler: &mut RenderHandler| handler(tick));
    }

    /// C# `_frameClock.Frame -= _onFrame`.
    fn release_frame_subscription(&self) {
        if let Some(id) = self.frame_subscription.take() {
            self.frame_clock.unsubscribe(id);
        }
    }

    /// C# `Invalidate()`: no-op when a render is already requested or the window is destroyed; else ONE frame-clock subscription.
    pub(super) fn invalidate(&self) {
        if self.render_requested.get() || self.destroyed.get() {
            return;
        }
        let weak = self.weak_self.clone();
        let id = self.frame_clock.subscribe(Box::new(move |tick: &FrameTick| {
            if let Some(inner) = weak.upgrade() {
                inner.on_frame(tick);
            }
        }));
        self.frame_subscription.set(Some(id));
        self.render_requested.set(true);
    }

    /// C# `ApplyInitialSize()` (lines 319-336): `rect = (0, 0, to_pixels(width_dip), to_pixels(height_dip))`; unless `size_is_outer`, grow it with
    /// `AdjustWindowRectExForDpi(WS_OVERLAPPEDWINDOW, no menu, ex_style 0, dpi)` (a failure there skips the resize, like the C# `if`);
    /// `SetWindowPos(hwnd, 0, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE)`, then `rethrow_pending()`; finally read the client rect into
    /// `client_width_px`/`client_height_px` (only when `GetClientRect` succeeds).
    pub(super) fn apply_initial_size(&self) {
        let dpi = self.dpi.get();
        let scale = dpi_scale_of(dpi);
        let hwnd = self.hwnd.get();
        let mut rect = new_rect(
            0,
            0,
            to_pixels(self.options.width_dip, scale),
            to_pixels(self.options.height_dip, scale),
        );
        if self.options.size_is_outer
            || user32::adjust_window_rect_ex_for_dpi(&mut rect, WS_OVERLAPPEDWINDOW, false, 0, dpi).is_ok()
        {
            let _ = user32::set_window_pos(
                hwnd,
                0,
                0,
                0,
                rect_width(&rect),
                rect_height(&rect),
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            rethrow_pending();
        }
        if let Ok(client) = user32::get_client_rect(hwnd) {
            self.client_width_px.set(rect_width(&client));
            self.client_height_px.set(rect_height(&client));
        }
    }

    /// C# `IsPointerOverClient()`: `GetCursorPos` and `WindowFromPoint(pt) == hwnd`.
    pub(super) fn is_pointer_over_client(&self) -> bool {
        comctl_user32::get_cursor_pos().is_some_and(|point| native::window_from_point(point) == self.hwnd.get())
    }

    /// C# `HasPointerCapture`.
    fn has_pointer_capture(&self) -> bool {
        let hwnd = self.hwnd.get();
        hwnd != 0 && native::get_capture() == hwnd
    }

    /// C# `FreeSelf()`: idempotent `wndproc::free_cookie`.
    pub(super) fn free_self(&self) {
        let cookie = self.cookie.replace(0);
        if cookie != 0 {
            // SAFETY: the cookie came from `make_cookie` in `create` and `replace(0)` makes this the only free; the trampoline clones the `Rc` before it
            // calls the target, so a call that is still running keeps its own reference.
            unsafe { free_cookie(cookie) };
        }
    }

    /// C# `ClientSizeDip` = `(client_width_px / scale, client_height_px / scale)`.
    pub(super) fn client_size_dip(&self) -> SizeD {
        let scale = dpi_scale_of(self.dpi.get());
        SizeD {
            width: f64::from(self.client_width_px.get()) / scale,
            height: f64::from(self.client_height_px.get()) / scale,
        }
    }

    /// C# `SetCursor`: the stored value; the OS cursor changes at once when the window has the capture or the pointer is over the client area.
    fn set_cursor(&self, cursor: ShellCursor) {
        self.cursor.set(cursor);
        if !self.destroyed.get() && (self.has_pointer_capture() || self.is_pointer_over_client()) {
            apply_cursor_handle(cursor_handle(cursor));
        }
    }

    /// C# `Dispose()`: destroy the window if it exists, free the cookie, then resume (`rethrow`) a panic captured during `DestroyWindow`.
    fn dispose(&self, rethrow: bool) {
        let hwnd = self.hwnd.get();
        let destroying = hwnd != 0 && !self.destroyed.get();
        if destroying {
            let _ = user32::destroy_window(hwnd);
        }
        self.free_self();
        if destroying && rethrow {
            rethrow_pending();
        }
    }
}

/// `SetCursor(handle)` unless the handle could not be loaded (`0` would hide the cursor).
pub(super) fn apply_cursor_handle(handle: isize) {
    if handle != 0 {
        user32::set_cursor(handle);
    }
}

/// C# `DpiScale` = `dpi / 96.0`.
pub(super) fn dpi_scale_of(dpi: u32) -> f64 {
    f64::from(dpi) / f64::from(DEFAULT_DPI)
}

/// C# `ToPixels(dip)` = `(int)Math.Round(dip * scale)`: ties-to-even (`round_ties_even`), then a saturating cast (NaN gives 0).
pub fn to_pixels(dip: f64, scale: f64) -> i32 {
    (dip * scale).round_ties_even() as i32
}

/// C# `CursorHandle(cursor)`: the shared system cursor (`IDC_ARROW`, `IDC_SIZEALL`, `IDC_WAIT`), loaded once per process.
pub(super) fn cursor_handle(cursor: ShellCursor) -> isize {
    static ARROW: OnceLock<isize> = OnceLock::new();
    static SIZE_ALL: OnceLock<isize> = OnceLock::new();
    static WAIT: OnceLock<isize> = OnceLock::new();
    let (slot, id) = match cursor {
        ShellCursor::SizeAll => (&SIZE_ALL, native::IDC_SIZEALL),
        ShellCursor::Wait => (&WAIT, native::IDC_WAIT),
        ShellCursor::Arrow => (&ARROW, user32::IDC_ARROW),
    };
    // A failed load is remembered as 0 (no cursor change), like the C# static readonly fields.
    *slot.get_or_init(|| user32::load_system_cursor(id).unwrap_or(0))
}

/// C# `EnsureClassRegistered()`: once per process, only a success is remembered. Class: `shell_window_proc`, `hInstance = GetModuleHandle(0)`,
/// big/small icon `icons::load_big(0)`/`load_small(0)`, no background brush (the renderer paints everything; `WM_ERASEBKGND` returns 1).
/// `Err(Win32 "RegisterClassEx (shell window) failed")`.
pub(super) fn ensure_class_registered() -> Result<(), HostError> {
    static REGISTERED: Mutex<bool> = Mutex::new(false);
    let mut registered = REGISTERED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if *registered {
        return Ok(());
    }
    let class_name = to_utf16z(CLASS_NAME);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(shell_window_proc),
        hInstance: HINSTANCE(kernel32::get_module_handle().unwrap_or(0) as *mut core::ffi::c_void),
        hIcon: HICON(icons::load_big(0) as *mut core::ffi::c_void),
        hIconSm: HICON(icons::load_small(0) as *mut core::ffi::c_void),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `class_name` is a NUL-terminated UTF-16 buffer that outlives the call (the OS copies the name); `shell_window_proc` is a valid window
    // procedure for the life of the process; the other pointer members are null.
    unsafe { user32::register_class_ex(&class) }.map_err(|error| HostError::Win32 {
        operation: "RegisterClassEx (shell window) failed",
        error,
    })?;
    *registered = true;
    Ok(())
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
        ensure_class_registered()?;
        let title = options.title.clone();
        let inner = Rc::new_cyclic(|weak| ShellWindowInner::new(options, frame_clock, weak.clone()));
        let cookie = make_cookie(Rc::clone(&inner) as Rc<dyn WindowTarget>);
        inner.cookie.set(cookie);
        let instance = kernel32::get_module_handle().unwrap_or(0);
        // SAFETY: `CLASS_NAME` is registered with `shell_window_proc`, which expects exactly a cookie from `make_cookie` in `lpCreateParams`.
        let created = unsafe {
            user32::create_window_ex(
                0,
                CLASS_NAME,
                &title,
                WS_OVERLAPPEDWINDOW,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                0,
                0,
                instance,
                cookie,
            )
        };
        // The wrapper reads the error straight after the call (the `Err` value), before the cookie is freed.
        if let Err(error) = created {
            inner.free_self();
            rethrow_pending(); // a captured panic wins over the Win32 error
            return Err(HostError::Win32 {
                operation: "CreateWindowEx (shell window) failed",
                error,
            });
        }
        // From here the `Drop` of `window` destroys the window if a later step panics.
        let window = ShellWindow { inner };
        rethrow_pending();
        window.inner.apply_initial_size();
        window.inner.last_raised_client_size.set(window.inner.client_size_dip());
        Ok(window)
    }

    /// Client size in device pixels (C# `ClientSizePixels`), for the renderer.
    pub fn client_size_pixels(&self) -> (i32, i32) {
        (self.inner.client_width_px.get(), self.inner.client_height_px.get())
    }

    /// C# `IsDestroyed`.
    pub fn is_destroyed(&self) -> bool {
        self.inner.destroyed.get()
    }

    /// C# `Cursor`.
    pub fn cursor(&self) -> ShellCursor {
        self.inner.cursor.get()
    }

    /// C# `RenderCount`.
    pub fn render_count(&self) -> i64 {
        self.inner.render_count.get()
    }

    /// C# `IsRenderRequested`.
    pub fn is_render_requested(&self) -> bool {
        self.inner.render_requested.get()
    }

    /// C# internal `Render` event: raised once per `Invalidate` burst, at the next frame tick.
    pub fn subscribe_render(&self, handler: RenderHandler) -> EventHandlerId {
        self.inner.render.add(LIST_EVENT, handler)
    }

    /// C# internal `InsertMessageHandlerFirst` (overlay and the keep-warm listener).
    pub fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.inner.handlers.borrow_mut().insert(0, handler);
    }

    /// C# internal `Show(bool activate)`: `SW_SHOWNORMAL` or `SW_SHOWNOACTIVATE`.
    pub fn show(&self, activate: bool) {
        self.show_command(if activate { SW_SHOWNORMAL } else { SW_SHOWNOACTIVATE });
    }

    /// C# internal `Show(int showCommand)`: `ShowWindow`, then `rethrow_pending()`.
    pub fn show_command(&self, show_command: i32) {
        user32::show_window(self.inner.hwnd.get(), show_command);
        rethrow_pending();
    }

    /// C# internal `SetSurfaceReady(bool)`.
    pub fn set_surface_ready(&self, ready: bool) {
        self.inner.surface_ready.set(ready);
    }

    /// C# `Dispose()`: if the window exists and is not destroyed, `DestroyWindow` then `rethrow_pending()`; always `free_self()`.
    pub fn dispose(&self) {
        self.inner.dispose(true);
    }
}

impl Drop for ShellWindow {
    fn drop(&mut self) {
        // A second panic while unwinding would abort: a captured panic stays in the slot then.
        self.inner.dispose(!std::thread::panicking());
    }
}

impl ShellWindowApi for ShellWindow {
    fn hwnd(&self) -> Hwnd {
        self.inner.hwnd.get()
    }
    fn dpi_scale(&self) -> f64 {
        dpi_scale_of(self.inner.dpi.get())
    }
    fn client_size_dip(&self) -> SizeD {
        self.inner.client_size_dip()
    }
    fn is_active(&self) -> bool {
        self.inner.is_active.get()
    }
    /// `created && !destroyed && surface_ready`.
    fn is_loaded(&self) -> bool {
        self.inner.created.get() && !self.inner.destroyed.get() && self.inner.surface_ready.get()
    }
    fn subscribe(&self, event: ShellWindowEvent, handler: WindowEventHandler) -> EventHandlerId {
        self.inner.events.add(event, handler)
    }
    fn subscribe_closing(&self, handler: ClosingHandler) -> EventHandlerId {
        self.inner.closing.add(LIST_EVENT, handler)
    }
    fn unsubscribe(&self, id: EventHandlerId) {
        let _ = self.inner.events.remove(id) || self.inner.closing.remove(id) || self.inner.render.remove(id);
    }
    fn add_message_handler(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.inner.handlers.borrow_mut().push(handler);
    }
    /// Delegates to the inherent [`ShellWindow::insert_message_handler_first`] (reconciliation amendment A7).
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        ShellWindow::insert_message_handler_first(self, handler);
    }
    /// Delegates to the inherent [`ShellWindow::is_destroyed`] (reconciliation amendment A7).
    fn is_destroyed(&self) -> bool {
        ShellWindow::is_destroyed(self)
    }
    /// C# `Invalidate()`: no-op when a render is already requested or the window is destroyed; else `render_requested = true` and ONE frame-clock subscription.
    fn invalidate(&self) {
        self.inner.invalidate();
    }
    /// C# `SetTitle`: destroyed -> `Ok`; `SetWindowText` failure is `Err(Win32 "SetWindowText failed")`.
    fn set_title(&self, title: &str) -> Result<(), HostError> {
        if self.inner.destroyed.get() {
            return Ok(());
        }
        dialog_user32::set_window_text(self.inner.hwnd.get(), title).map_err(|error| HostError::Win32 {
            operation: "SetWindowText failed",
            error,
        })
    }
    /// C# `SetCursor`: stores the cursor; applies it now if the window has the capture or the pointer is over the client area.
    fn set_cursor(&self, cursor: ShellCursor) {
        self.inner.set_cursor(cursor);
    }
    fn capture_pointer(&self) {
        if !self.inner.destroyed.get() {
            user32::set_capture(self.inner.hwnd.get());
        }
    }
    fn release_pointer(&self) {
        if self.inner.has_pointer_capture() {
            user32::release_capture();
        }
    }
    fn has_pointer_capture(&self) -> bool {
        self.inner.has_pointer_capture()
    }
    /// C# `ScreenToClientDip`: round both coordinates (ties-to-even) to a `POINT`, `ScreenToClient` unless destroyed, divide by `dpi_scale`.
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        let mut point = Point {
            x: screen_pixel.x.round_ties_even() as i32,
            y: screen_pixel.y.round_ties_even() as i32,
        };
        if !self.inner.destroyed.get() {
            dialog_user32::screen_to_client(self.inner.hwnd.get(), &mut point);
        }
        let scale = self.dpi_scale();
        PointD {
            x: f64::from(point.x) / scale,
            y: f64::from(point.y) / scale,
        }
    }
    /// C# `Close()`: destroyed -> nothing; else `SendMessage(hwnd, WM_CLOSE, 0, 0)` then `rethrow_pending()`.
    fn close(&self) {
        if self.inner.destroyed.get() {
            return;
        }
        // SAFETY: WM_CLOSE carries no pointers; the window procedure runs synchronously on this thread.
        let _result = unsafe { dialog_user32::send_message(self.inner.hwnd.get(), WM_CLOSE, 0, 0) };
        rethrow_pending();
    }
}
