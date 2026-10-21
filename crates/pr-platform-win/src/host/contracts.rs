//! Port of `PhotoReview.Shell.Win32/Hosting/ShellWindowContracts.cs` (67 lines, C-15) and `NativeRect.cs` (13 lines), baseline `d92d6aae`. Unit W8-13b.
//!
//! The shell window contract (`IShellWindow`, `IWindowMessageHandler`, `WindowMessage`, `ShellCursor`) and the small `Rect` helpers. The concrete
//! window is `shell_window::ShellWindow` (13e/13f). `SizeD`/`PointD` are interim twins of `pr_app::input::primitives::{SizeD, PointD}` (same fields and
//! record equality); the layering PR replaces them by imports (D-W8B3H-1).
//!
//! Naming: C# `IShellWindow` becomes the trait [`ShellWindowApi`] because the concrete type keeps the C# name `ShellWindow` (D-W8B3H-3).
//! C# events become `subscribe`/`unsubscribe` pairs with a `u64` id (precedent: `FrameClock`, `RenderSurface::subscribe_device_recreated`).

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
        double_equals(self.width, other.width) && double_equals(self.height, other.height)
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
        double_equals(self.x, other.x) && double_equals(self.y, other.y)
    }
}

/// C# `double.Equals(double)`: NaN equals NaN, `0.0` equals `-0.0`.
fn double_equals(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
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
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

/// C# `NativeRect.Width`: `right - left` with wrapping `i32` subtraction (unchecked C# `int`).
pub fn rect_width(rect: &Rect) -> i32 {
    rect.right.wrapping_sub(rect.left)
}

/// C# `NativeRect.Height`: `bottom - top`, wrapping.
pub fn rect_height(rect: &Rect) -> i32 {
    rect.bottom.wrapping_sub(rect.top)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    // new: values are public API (C#: ShellCursor 0/1/2) and the event order is part of the contract (T5)
    #[test]
    fn shell_cursor_values_match_csharp() {
        assert_eq!(ShellCursor::Arrow as i32, 0);
        assert_eq!(ShellCursor::SizeAll as i32, 1);
        assert_eq!(ShellCursor::Wait as i32, 2);
        assert_eq!(ShellWindowEvent::ClientSizeChanged as i32, 0);
        assert_eq!(ShellWindowEvent::DpiChanged as i32, 1);
        assert_eq!(ShellWindowEvent::Activated as i32, 2);
        assert_eq!(ShellWindowEvent::Deactivated as i32, 3);
        assert_eq!(ShellWindowEvent::Closed as i32, 4);
    }

    // new: C# record equality over `double.Equals`
    #[test]
    fn size_and_point_equality_is_double_equals() {
        let nan = f64::NAN;
        assert_eq!(
            SizeD {
                width: nan,
                height: 1.0
            },
            SizeD {
                width: nan,
                height: 1.0
            }
        );
        assert_eq!(
            SizeD {
                width: 0.0,
                height: -0.0
            },
            SizeD {
                width: -0.0,
                height: 0.0
            }
        );
        assert_ne!(
            SizeD {
                width: 1.0,
                height: 2.0
            },
            SizeD {
                width: 1.0,
                height: 3.0
            }
        );
        assert_ne!(
            SizeD {
                width: 1.0,
                height: 2.0
            },
            SizeD {
                width: 2.0,
                height: 2.0
            }
        );
        assert_eq!(PointD { x: nan, y: nan }, PointD { x: nan, y: nan });
        assert_eq!(PointD { x: 0.0, y: 0.0 }, PointD { x: -0.0, y: -0.0 });
        assert_ne!(PointD { x: 1.0, y: 2.0 }, PointD { x: 1.0, y: 2.5 });
        assert_ne!(PointD { x: 1.0, y: 2.0 }, PointD { x: 1.5, y: 2.0 });
        assert_eq!(
            SizeD::default(),
            SizeD {
                width: 0.0,
                height: 0.0
            }
        );
        assert_eq!(PointD::default(), PointD { x: 0.0, y: 0.0 });
    }

    // new (T1): C# `NativeRect` is unchecked `int`
    #[test]
    fn new_rect_and_width_height_wrap_instead_of_panicking() {
        let rect = new_rect(1, 2, 11, 32);
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (1, 2, 11, 32));
        assert_eq!(rect_width(&rect), 10);
        assert_eq!(rect_height(&rect), 30);
        assert_eq!(rect_width(&new_rect(i32::MIN, 0, i32::MAX, 0)), -1);
        assert_eq!(rect_height(&new_rect(0, i32::MAX, 0, i32::MIN)), 1);
        assert_eq!(rect_width(&new_rect(10, 0, 4, 0)), -6);
    }

    // new
    #[test]
    fn window_message_is_copy_and_eq() {
        let message = WindowMessage {
            hwnd: 7,
            msg: 0x0112,
            wparam: 0xF060,
            lparam: -1,
        };
        let copy = message;
        assert_eq!(message, copy);
        assert_ne!(message, WindowMessage { lparam: 0, ..message });
    }

    // new
    #[test]
    fn cancel_event_args_default_is_false() {
        assert!(!CancelEventArgs::default().cancel);
    }

    struct Pass;
    impl WindowMessageHandler for Pass {
        fn try_handle(&self, _message: &WindowMessage) -> Option<Lresult> {
            None
        }
    }

    /// Answers `msg + 1000`, and asks `next` first (a nested `SendMessage` that dispatches to the next handler of the chain while this one runs).
    struct Reentrant {
        next: Rc<dyn WindowMessageHandler>,
        calls: Cell<u32>,
    }
    impl WindowMessageHandler for Reentrant {
        fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
            self.calls.set(self.calls.get() + 1);
            self.next.try_handle(message).or(Some(message.msg as Lresult + 1000))
        }
    }

    /// Handles only `WM_USER` (0x400) and returns the `lparam`.
    struct Echo;
    impl WindowMessageHandler for Echo {
        fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
            (message.msg == 0x400).then_some(message.lparam)
        }
    }

    /// Minimal implementation of every member.
    #[derive(Default)]
    struct FakeWindow {
        title: RefCell<String>,
        handlers: RefCell<Vec<Rc<dyn WindowMessageHandler>>>,
        subscriptions: RefCell<Vec<EventHandlerId>>,
        next_id: Cell<u64>,
        closed: Cell<bool>,
        captured: Cell<bool>,
        cursor: Cell<Option<ShellCursor>>,
        invalidated: Cell<u32>,
    }

    impl FakeWindow {
        fn next(&self) -> u64 {
            self.next_id.set(self.next_id.get() + 1);
            let id = self.next_id.get();
            self.subscriptions.borrow_mut().push(id);
            id
        }
    }

    impl ShellWindowApi for FakeWindow {
        fn hwnd(&self) -> Hwnd {
            99
        }
        fn dpi_scale(&self) -> f64 {
            1.5
        }
        fn client_size_dip(&self) -> SizeD {
            SizeD {
                width: 800.0,
                height: 600.0,
            }
        }
        fn is_active(&self) -> bool {
            true
        }
        fn is_loaded(&self) -> bool {
            true
        }
        fn subscribe(&self, _event: ShellWindowEvent, _handler: WindowEventHandler) -> EventHandlerId {
            self.next()
        }
        fn subscribe_closing(&self, mut handler: ClosingHandler) -> EventHandlerId {
            let mut args = CancelEventArgs::default();
            handler(&mut args);
            self.next()
        }
        fn unsubscribe(&self, id: EventHandlerId) {
            self.subscriptions.borrow_mut().retain(|known| *known != id);
        }
        fn add_message_handler(&self, handler: Rc<dyn WindowMessageHandler>) {
            self.handlers.borrow_mut().push(handler);
        }
        fn invalidate(&self) {
            self.invalidated.set(self.invalidated.get() + 1);
        }
        fn set_title(&self, title: &str) -> Result<(), HostError> {
            *self.title.borrow_mut() = title.to_owned();
            Ok(())
        }
        fn set_cursor(&self, cursor: ShellCursor) {
            self.cursor.set(Some(cursor));
        }
        fn capture_pointer(&self) {
            self.captured.set(true);
        }
        fn release_pointer(&self) {
            self.captured.set(false);
        }
        fn has_pointer_capture(&self) -> bool {
            self.captured.get()
        }
        fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
            PointD {
                x: screen_pixel.x / 1.5,
                y: screen_pixel.y / 1.5,
            }
        }
        fn close(&self) {
            self.closed.set(true);
        }
    }

    // new: the contract surface of C-15 is usable through trait objects (the Rust twin of ContractSurfaceTests)
    #[test]
    fn shell_window_api_is_object_safe() {
        let fake = FakeWindow::default();
        let window: &dyn ShellWindowApi = &fake;
        assert_eq!(window.hwnd(), 99);
        assert_eq!(window.dpi_scale(), 1.5);
        assert_eq!(
            window.client_size_dip(),
            SizeD {
                width: 800.0,
                height: 600.0
            }
        );
        assert!(window.is_active() && window.is_loaded());

        let first = window.subscribe(ShellWindowEvent::Activated, Box::new(|| {}));
        let closing = window.subscribe_closing(Box::new(|args| args.cancel = true));
        assert_ne!(first, closing);
        window.unsubscribe(first);
        window.unsubscribe(12345); // unknown id: no-op
        assert_eq!(*fake.subscriptions.borrow(), vec![closing]);

        window.invalidate();
        assert_eq!(fake.invalidated.get(), 1);
        assert_eq!(window.set_title("PhotoReview"), Ok(()));
        assert_eq!(*fake.title.borrow(), "PhotoReview");
        window.set_cursor(ShellCursor::Wait);
        assert_eq!(fake.cursor.get(), Some(ShellCursor::Wait));
        window.capture_pointer();
        assert!(window.has_pointer_capture());
        window.release_pointer();
        assert!(!window.has_pointer_capture());
        assert_eq!(
            window.screen_to_client_dip(PointD { x: 3.0, y: 6.0 }),
            PointD { x: 2.0, y: 4.0 }
        );
        window.close();
        assert!(fake.closed.get());

        // Handlers as `Rc<dyn WindowMessageHandler>`, one of them re-entrant.
        let inner: Rc<dyn WindowMessageHandler> = Rc::new(Echo);
        let outer = Rc::new(Reentrant {
            next: inner,
            calls: Cell::new(0),
        });
        window.add_message_handler(Rc::new(Pass));
        window.add_message_handler(Rc::clone(&outer) as Rc<dyn WindowMessageHandler>);
        let message = WindowMessage {
            hwnd: 99,
            msg: 0x400,
            wparam: 0,
            lparam: 55,
        };
        let results: Vec<Option<Lresult>> = fake
            .handlers
            .borrow()
            .iter()
            .map(|handler| handler.try_handle(&message))
            .collect();
        assert_eq!(results, vec![None, Some(55)]);
        // A message the inner handler does not know falls through to the outer one.
        let other = WindowMessage { msg: 0x401, ..message };
        assert_eq!(outer.try_handle(&other), Some(0x401 + 1000));
        assert_eq!(outer.calls.get(), 2);
    }
}
