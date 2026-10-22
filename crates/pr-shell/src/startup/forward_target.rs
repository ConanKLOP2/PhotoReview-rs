//! Port of the `IForwardedOpenTarget` implementation of `ShellMainWindow` (C# members `IsClosingOrClosed`, `IsMinimized`, `Restore`, `Activate`, `OpenPathAsync`; baseline `d92d6aae`, the window
//! side of `Startup/ForwardedOpenHandler.cs`, D-W8B4X-23). Unit W8-17e. Contract: `contracts/W8-17e.md`.
//!
//! `ForwardedOpenTarget` is `pr_app::startup::forwarded_open` (W8-17a), `MainWindow` is `pr_platform_win::host::main_window` (13g..13k): neither is local to `pr-shell`, so the impl is on this
//! newtype (orphan rule, D-W8B6-28). It holds a `Weak` of the window SLOT, so the handler (which `ForwardedOpenRouter` keeps) never keeps the window alive.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::RefCell;
use std::error::Error;
use std::rc::{Rc, Weak};

use pr_app::startup::forwarded_open::{ForwardedOpenTarget, WindowGone};
use pr_platform_win::host::main_window::MainWindow;

/// The forwarded-open target over the main window slot.
pub struct MainWindowForwardTarget {
    slot: Weak<RefCell<Option<Rc<MainWindow>>>>,
}

impl std::fmt::Debug for MainWindowForwardTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainWindowForwardTarget").finish_non_exhaustive()
    }
}

impl MainWindowForwardTarget {
    /// A target over `slot`.
    pub fn new(slot: Weak<RefCell<Option<Rc<MainWindow>>>>) -> Self {
        Self { slot }
    }

    /// A clone of the window `Rc` (a short borrow of the slot), `None` when the slot is gone or empty.
    fn window(&self) -> Option<Rc<MainWindow>> {
        todo!()
    }
}

impl ForwardedOpenTarget for MainWindowForwardTarget {
    /// No window (slot gone or empty) is `true` (a late request must not touch it); else `MainWindow::is_closing_or_closed()`.
    fn is_closing_or_closed(&self) -> bool {
        todo!()
    }

    /// `Ok(window.is_minimized())`; no window: `Err(WindowGone { message: "The main window is already destroyed." })`.
    fn is_minimized(&self) -> Result<bool, WindowGone> {
        todo!()
    }

    /// `window.restore()` (SC_RESTORE: back to the state before minimising, R7-5); no window: `Err(WindowGone)`.
    fn restore(&self) -> Result<(), WindowGone> {
        todo!()
    }

    /// `window.activate()`; `Err(MainWindowError::WindowDestroyed)` or no window: `Err(WindowGone { message })` with the text of the `MainWindowError`.
    fn activate(&self) -> Result<(), WindowGone> {
        todo!()
    }

    /// `window.open_path(path)` and `Ok(())` (the open is STARTED; the view model's step machine finishes it, failures are the window's to report). No window: `Err(WindowGone)` boxed.
    fn open_path(&self, path: &str) -> Result<(), Box<dyn Error>> {
        todo!()
    }
}
