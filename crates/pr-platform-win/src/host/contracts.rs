//! Port of `PhotoReview.Shell.Win32/Hosting/ShellWindowContracts.cs` (67 lines, C-15) and `NativeRect.cs` (13 lines), baseline `d92d6aae`. Unit W8-13b.
//!
//! The shell window contract (`IShellWindow`, `IWindowMessageHandler`, `WindowMessage`, `ShellCursor`) and the small `Rect` helpers. The concrete
//! window is `shell_window::ShellWindow` (13e/13f). `SizeD`/`PointD` are interim twins of `pr_app::input::primitives::{SizeD, PointD}` (same fields and
//! record equality); the layering PR replaces them by imports (D-W8B3H-1).
//!
//! Naming: C# `IShellWindow` becomes the trait [`ShellWindowApi`] because the concrete type keeps the C# name `ShellWindow` (D-W8B3H-3).
//! C# events become `subscribe`/`unsubscribe` pairs with a `u64` id (precedent: `FrameClock`, `RenderSurface::subscribe_device_recreated`).

#![allow(unused_variables, dead_code)] // W8-13b stub: the porter removes this line when the last `todo!()` is gone.

use std::rc::Rc;

use crate::host::error::HostError;
use crate::win32::structs::{Hwnd, Lparam, Lresult, Rect, Wparam};

/// C# `ShellCursor` (values 0, 1, 2 are asserted by a test).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellCursor {
    Arrow = 0,
    SizeAll = 1,
    Wait = 2,
}

/// C# `WindowMessage(nint Hwnd, uint Msg, nint WParam, nint LParam)` (`readonly record struct`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowMessage {
    pub hwnd: Hwnd,
    pub msg: u32,
    pub wparam: Wparam,
    pub lparam: Lparam,
}

/// C# `IWindowMessageHandler`: handlers run in registration order; the first that returns `Some(result)` wins.
/// `&self`: a handler can be re-entered (a nested `SendMessage` from inside a handler dispatches the next message to the same chain), so it keeps its
/// own state in `Cell`/`RefCell` (D-W8B3H-6). Registered as `Rc<dyn WindowMessageHandler>`.
pub trait WindowMessageHandler {
    /// C# `TryHandle(in message, out result)`: `Some(result)` = handled (`true` + the `out` value), `None` = not handled.
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult>;
}

/// Interim twin of `pr_app::input::primitives::SizeD` (DIP).
#[derive(Debug, Clone, Copy, Default)]
pub struct SizeD {
    pub width: f64,
    pub height: f64,
}

impl PartialEq for SizeD {
    /// Record equality of two `double`s: NaN equals NaN, `0.0` equals `-0.0`.
    fn eq(&self, other: &Self) -> bool {
        todo!("W8-13b")
    }
}

/// Interim twin of `pr_app::input::primitives::PointD` (DIP or pixel).
#[derive(Debug, Clone, Copy, Default)]
pub struct PointD {
    pub x: f64,
    pub y: f64,
}

impl PartialEq for PointD {
    fn eq(&self, other: &Self) -> bool {
        todo!("W8-13b")
    }
}

/// C# `CancelEventArgs`: a `Closing` handler sets `cancel = true` to keep the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CancelEventArgs {
    pub cancel: bool,
}

/// The parameterless events of `IShellWindow` (`ClientSizeChanged`, `DpiChanged`, `Activated`, `Deactivated`, `Closed`). `Closing` has its own
/// subscribe method because its handler can cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellWindowEvent {
    ClientSizeChanged,
    /// `WM_DPICHANGED`, raised after the suggested rectangle was applied.
    DpiChanged,
    Activated,
    Deactivated,
    Closed,
}

/// Identifies one event subscription.
pub type EventHandlerId = u64;

/// A parameterless event handler (UI thread, no `Send` bound).
pub type WindowEventHandler = Box<dyn FnMut()>;

/// A `Closing` handler.
pub type ClosingHandler = Box<dyn FnMut(&mut CancelEventArgs)>;

/// C# `IShellWindow` (C-15). UI thread only; all methods `&self` (the window is re-entered from its own window procedure).
pub trait ShellWindowApi {
    /// C# `Hwnd`; `0` before `WM_NCCREATE`.
    fn hwnd(&self) -> Hwnd;
    /// C# `DpiScale` = `dpi / 96.0`.
    fn dpi_scale(&self) -> f64;
    /// C# `ClientSizeDip` = client pixels / `dpi_scale`.
    fn client_size_dip(&self) -> SizeD;
    /// C# `IsActive`.
    fn is_active(&self) -> bool;
    /// C# `IsLoaded`: after `WM_CREATE` and with the surface ready, before `WM_DESTROY`.
    fn is_loaded(&self) -> bool;
    /// C# `ClientSizeChanged += ` etc. Handlers run in subscription order.
    fn subscribe(&self, event: ShellWindowEvent, handler: WindowEventHandler) -> EventHandlerId;
    /// C# `Closing +=`.
    fn subscribe_closing(&self, handler: ClosingHandler) -> EventHandlerId;
    /// Removes a subscription made with either subscribe method; an unknown id is a no-op.
    fn unsubscribe(&self, id: EventHandlerId);
    /// C# `AddMessageHandler`.
    fn add_message_handler(&self, handler: Rc<dyn WindowMessageHandler>);
    /// C# `Invalidate()`: request the next frame (coalesced).
    fn invalidate(&self);
    /// C# `SetTitle`; `Err(Win32 "SetWindowText failed")` when the OS refuses. A destroyed window is `Ok(())`.
    fn set_title(&self, title: &str) -> Result<(), HostError>;
    /// C# `SetCursor`.
    fn set_cursor(&self, cursor: ShellCursor);
    /// C# `CapturePointer`.
    fn capture_pointer(&self);
    /// C# `ReleasePointer`.
    fn release_pointer(&self);
    /// C# `HasPointerCapture`.
    fn has_pointer_capture(&self) -> bool;
    /// C# `ScreenToClientDip(PointD screenPixel)`.
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD;
    /// C# `Close()`: synchronous `WM_CLOSE`; a `Closing` handler may cancel.
    fn close(&self);
}

/// C# `NativeRect.Create`.
pub fn new_rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    todo!("W8-13b")
}

/// C# `NativeRect.Width`: `right - left` with wrapping `i32` subtraction (unchecked C# `int`).
pub fn rect_width(rect: &Rect) -> i32 {
    todo!("W8-13b")
}

/// C# `NativeRect.Height`: `bottom - top`, wrapping.
pub fn rect_height(rect: &Rect) -> i32 {
    todo!("W8-13b")
}
