//! Port of `PhotoReview.Shell.Win32/Menus/ContextMenuMessageHandler.cs` (57 lines, baseline `d92d6aae`). Unit W8-14e. Contract: `contracts/W8-14e.md`.
//!
//! WP-24: `WM_CONTEXTMENU` to `ContextMenuController::show`. `DefWindowProc` sends `WM_CONTEXTMENU` when the right button is released (`lparam` = SCREEN
//! pixels, signed) and for the Menu key or Shift+F10 (`lparam` = -1: WPF opened it at the centre of the element). Only a point inside the client area
//! is taken: a right click on the title bar or the frame (the system also sends `WM_CONTEXTMENU` with non-client coordinates) is left to
//! `DefWindowProc` for the system menu. The point is converted to DIP with the window's DPI; the host scales it back when it shows the menu.
use std::fmt;
use std::rc::Rc;

use pr_app::input::primitives::PointD;

use super::controller::ContextMenuController;
use crate::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use crate::shell_input::native::{signed_high_word, signed_low_word};
use crate::win32::messages::WM_CONTEXTMENU;
use crate::win32::structs::Lresult;

/// C# `ContextMenuMessageHandler(window, controller)`.
pub struct ContextMenuMessageHandler {
    window: Rc<dyn ShellWindowApi>,
    controller: Rc<ContextMenuController>,
}

impl ContextMenuMessageHandler {
    /// C# primary constructor.
    pub fn new(window: Rc<dyn ShellWindowApi>, controller: Rc<ContextMenuController>) -> Self {
        Self { window, controller }
    }

    /// C# `ClientCenterScreenPixel()`: the centre of the client area in screen pixels, derived from `screen_to_client_dip((0, 0))` (= minus the client
    /// origin over the scale): `origin + client_size * scale / 2`.
    fn client_center_screen_pixel(&self) -> PointD {
        let scale = self.scale();
        let size = self.window.client_size_dip();
        // The client DIP of the screen point (0, 0) is minus the client origin over the scale.
        let origin = self.window.screen_to_client_dip(PointD { x: 0.0, y: 0.0 });
        let origin_x = -origin.x * scale;
        let origin_y = -origin.y * scale;
        PointD {
            x: origin_x + size.width * scale / 2.0,
            y: origin_y + size.height * scale / 2.0,
        }
    }

    /// `dpi_scale`, or 1.0 when it is not positive.
    fn scale(&self) -> f64 {
        let scale = self.window.dpi_scale();
        if scale > 0.0 { scale } else { 1.0 }
    }
}

impl fmt::Debug for ContextMenuMessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextMenuMessageHandler").finish_non_exhaustive()
    }
}

impl WindowMessageHandler for ContextMenuMessageHandler {
    /// `WM_CONTEXTMENU` only. `(x, y) = (signed low word, signed high word)` of `lparam`; `(-1, -1)` opens at the client centre; otherwise the point must be
    /// inside the client area (top-left inclusive, bottom-right exclusive) or the result is `None`. Then `controller.show(screen / scale)` with
    /// `scale = dpi_scale` (1.0 when not positive) and the result is `Some(0)`.
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if message.msg != WM_CONTEXTMENU {
            return None;
        }
        let x = signed_low_word(message.lparam);
        let y = signed_high_word(message.lparam);
        let screen_pixel = if x == -1 && y == -1 {
            self.client_center_screen_pixel()
        } else {
            let screen_pixel = PointD {
                x: f64::from(x),
                y: f64::from(y),
            };
            let client = self.window.screen_to_client_dip(screen_pixel);
            let size = self.window.client_size_dip();
            if client.x < 0.0 || client.y < 0.0 || client.x >= size.width || client.y >= size.height {
                return None; // non-client area: the system menu of the window
            }
            screen_pixel
        };
        let scale = self.scale();
        self.controller.show(PointD {
            x: screen_pixel.x / scale,
            y: screen_pixel.y / scale,
        });
        Some(0)
    }
}
