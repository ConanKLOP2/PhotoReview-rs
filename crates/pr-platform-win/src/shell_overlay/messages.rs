//! Port of `PhotoReview.Shell.Win32/Overlay/OverlayMessageHandler.cs` (111 lines, baseline `d92d6aae`). Unit W8-15f.
//!
//! Connects the mouse and key messages of the shell window to [`ShellOverlay`]. It must be FIRST in the window's handler chain
//! (`ShellWindow::insert_message_handler_first`): the overlay sees a message before the image does, like the WPF buttons that sit above the
//! `ScrollViewer`. Mouse move and keys are only OBSERVED (`None`: the image still gets them); a left press or release that belongs to the overlay, the
//! wheel over an overlay panel and Tab/Space/Enter of the toolbar are SWALLOWED (`Some(0)`).
//!
//! `lParam` of the mouse messages is client PIXELS (low and high word, signed 16 bit): the handler divides by the window's DPI scale to get DIP. The
//! wheel messages carry SCREEN pixels instead: they go through `ShellWindowApi::screen_to_client_dip`.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use pr_app::input::primitives::PointD;

use crate::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use crate::shell_overlay::native::{OverlayOs, VK_SHIFT, WM_MOUSELEAVE};
use crate::shell_overlay::tooltips::OverlayTooltips;
use crate::win32::messages::{
    WM_CAPTURECHANGED, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_SYSKEYDOWN,
};
use crate::win32::structs::{Lparam, Lresult};

/// The part of `ShellOverlay` the message handler drives (D-W8B4O-12: a trait, so 15f is testable with a recording fake and does not wait for 15h/15i;
/// `ShellOverlay` implements it in `overlay/input.rs`). Coordinates are client DIP.
pub trait OverlayInput {
    /// C# `PointerMoved`.
    fn pointer_moved(&self, point: PointD);
    /// C# `PointerLeft`.
    fn pointer_left(&self);
    /// C# `PointerDown`: true = swallowed.
    fn pointer_down(&self, point: PointD) -> bool;
    /// C# `PointerUp`: true = the release belonged to the overlay.
    fn pointer_up(&self, point: PointD) -> bool;
    /// C# `PointerCancel`.
    fn pointer_cancel(&self);
    /// C# `IsPointerDown`.
    fn is_pointer_down(&self) -> bool;
    /// C# `Wheel`: true = an overlay panel takes the wheel.
    fn wheel(&self, point: PointD) -> bool;
    /// C# `KeyDown(virtualKey, shift, isAutoRepeat)`: true = used.
    fn key_down(&self, virtual_key: i32, shift: bool, is_auto_repeat: bool) -> bool;
}

/// C# `OverlayMessageHandler(ShellOverlay overlay, IShellWindow window, IOverlayTooltips? tooltips)` plus the [`OverlayOs`] seam.
///
/// All state is in `Cell`s: the handler is re-entered (`ReleaseCapture` sends `WM_CAPTURECHANGED` to the same window synchronously, while the
/// `WM_LBUTTONUP` arm is still running).
pub struct OverlayMessageHandler {
    overlay: Rc<dyn OverlayInput>,
    window: Rc<dyn ShellWindowApi>,
    tooltips: Option<Rc<dyn OverlayTooltips>>,
    os: Rc<dyn OverlayOs>,
    /// C# `_trackingLeave`: a `WM_MOUSELEAVE` is already requested.
    tracking_leave: Cell<bool>,
    /// C# `_releasing`: our own `release_pointer` is running; the `WM_CAPTURECHANGED` it causes is not a stolen capture.
    releasing: Cell<bool>,
}

impl fmt::Debug for OverlayMessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayMessageHandler")
            .field("tracking_leave", &self.tracking_leave.get())
            .field("releasing", &self.releasing.get())
            .finish_non_exhaustive()
    }
}

impl OverlayMessageHandler {
    /// C# primary constructor; `os` is [`crate::shell_overlay::native::Win32OverlayOs`] in production.
    pub fn new(
        overlay: Rc<dyn OverlayInput>,
        window: Rc<dyn ShellWindowApi>,
        tooltips: Option<Rc<dyn OverlayTooltips>>,
        os: Rc<dyn OverlayOs>,
    ) -> Self {
        Self {
            overlay,
            window,
            tooltips,
            os,
            tracking_leave: Cell::new(false),
            releasing: Cell::new(false),
        }
    }

    /// C# `ClientPoint(lParam)`: `(low_word / dpi, high_word / dpi)` with `dpi = window.dpi_scale()`, or `1` when that is `<= 0`.
    fn client_point(&self, lparam: Lparam) -> PointD {
        let scale = self.window.dpi_scale();
        let scale = if scale <= 0.0 { 1.0 } else { scale };
        PointD {
            x: f64::from(low_word(lparam)) / scale,
            y: f64::from(high_word(lparam)) / scale,
        }
    }
}

impl WindowMessageHandler for OverlayMessageHandler {
    /// C# `TryHandle` (the whole `switch`):
    /// - `WM_MOUSEMOVE`: first one requests `WM_MOUSELEAVE` once (`tracking_leave`); `overlay.pointer_moved(client_point)`; relay to the tooltips; `None`.
    /// - `WM_MOUSELEAVE`: `tracking_leave = false`; `overlay.pointer_left()`; `None`.
    /// - `WM_LBUTTONDOWN`, `WM_LBUTTONDBLCLK`: relay to the tooltips FIRST; `overlay.pointer_down(client_point)`: when true and `overlay.is_pointer_down()`,
    ///   `window.capture_pointer()`; `Some(0)`; when false `None`.
    /// - `WM_LBUTTONUP`: `None` unless `overlay.is_pointer_down()`; relay; when `window.has_pointer_capture()` release it with `releasing` set (cleared on every
    ///   path); the result is `overlay.pointer_up(client_point)` as `Some(0)` / `None`.
    /// - `WM_CAPTURECHANGED`: when `!releasing && overlay.is_pointer_down() && lparam != hwnd`: `overlay.pointer_cancel()`; `None`.
    /// - `WM_MOUSEWHEEL`, `WM_MOUSEHWHEEL`: `overlay.wheel(window.screen_to_client_dip(low_word, high_word))` as `Some(0)` / `None`.
    /// - `WM_KEYDOWN`, `WM_SYSKEYDOWN`: `repeat = (lparam >> 30) & 1 != 0`, `shift = os.is_key_down(VK_SHIFT)`, key = `wparam & 0xFFFF`;
    ///   `overlay.key_down(key, shift, repeat)` as `Some(0)` / `None`.
    /// - anything else: `None`.
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        match message.msg {
            WM_MOUSEMOVE => {
                if !self.tracking_leave.replace(true) {
                    self.os.request_mouse_leave(message.hwnd);
                }
                self.overlay.pointer_moved(self.client_point(message.lparam));
                if let Some(tooltips) = &self.tooltips {
                    tooltips.relay_mouse(message.msg, message.wparam, message.lparam);
                }
                None
            }
            WM_MOUSELEAVE => {
                self.tracking_leave.set(false);
                self.overlay.pointer_left();
                None
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                if let Some(tooltips) = &self.tooltips {
                    tooltips.relay_mouse(message.msg, message.wparam, message.lparam);
                }
                if self.overlay.pointer_down(self.client_point(message.lparam)) {
                    if self.overlay.is_pointer_down() {
                        self.window.capture_pointer();
                    }
                    Some(0)
                } else {
                    None
                }
            }
            WM_LBUTTONUP => {
                if !self.overlay.is_pointer_down() {
                    return None;
                }
                if let Some(tooltips) = &self.tooltips {
                    tooltips.relay_mouse(message.msg, message.wparam, message.lparam);
                }
                if self.window.has_pointer_capture() {
                    self.releasing.set(true);
                    let _reset = ReleaseReset(&self.releasing);
                    self.window.release_pointer();
                }
                self.overlay.pointer_up(self.client_point(message.lparam)).then_some(0)
            }
            WM_CAPTURECHANGED => {
                if !self.releasing.get() && self.overlay.is_pointer_down() && message.lparam != message.hwnd {
                    self.overlay.pointer_cancel();
                }
                None
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let screen = PointD {
                    x: f64::from(low_word(message.lparam)),
                    y: f64::from(high_word(message.lparam)),
                };
                self.overlay
                    .wheel(self.window.screen_to_client_dip(screen))
                    .then_some(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let repeat = ((message.lparam >> 30) & 1) != 0;
                let shift = self.os.is_key_down(VK_SHIFT);
                let key = (message.wparam & 0xFFFF) as i32;
                self.overlay.key_down(key, shift, repeat).then_some(0)
            }
            _ => None,
        }
    }
}

/// C# `LowWord(nint)`: `(short)(value & 0xFFFF)` as `i32` (signed: a point left of the primary monitor is negative).
pub fn low_word(value: isize) -> i32 {
    (value as u16 as i16).into()
}

/// C# `HighWord(nint)`: `(short)((value >> 16) & 0xFFFF)` as `i32`.
pub fn high_word(value: isize) -> i32 {
    ((value >> 16) as u16 as i16).into()
}

/// Clears the release marker even if a re-entrant window call unwinds.
struct ReleaseReset<'a>(&'a Cell<bool>);

impl Drop for ReleaseReset<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
