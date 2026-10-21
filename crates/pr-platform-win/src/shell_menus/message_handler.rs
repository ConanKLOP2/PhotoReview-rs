//! Port of `PhotoReview.Shell.Win32/Menus/ContextMenuMessageHandler.cs` (57 lines, baseline `d92d6aae`). Unit W8-14e. Contract: `contracts/W8-14e.md`.
//!
//! WP-24: `WM_CONTEXTMENU` to `ContextMenuController::show`. `DefWindowProc` sends `WM_CONTEXTMENU` when the right button is released (`lparam` = SCREEN
//! pixels, signed) and for the Menu key or Shift+F10 (`lparam` = -1: WPF opened it at the centre of the element). Only a point inside the client area
//! is taken: a right click on the title bar or the frame (the system also sends `WM_CONTEXTMENU` with non-client coordinates) is left to
//! `DefWindowProc` for the system menu. The point is converted to DIP with the window's DPI; the host scales it back when it shows the menu.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;
use std::rc::Rc;

use super::controller::ContextMenuController;
use crate::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
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
    fn client_center_screen_pixel(&self) -> (f64, f64) {
        todo!()
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
        todo!()
    }
}
