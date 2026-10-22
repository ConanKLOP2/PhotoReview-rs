//! Port of `PhotoReview.Shell.Win32/Startup/ForwardedOpenHandler.cs` (73 lines, baseline `d92d6aae`) plus the UI-thread glue of `ShellStartup.OnForwardedOpen`
//! (`ShellStartup.cs` 597-615) and the 750 ms coalescer window (`ShellStartup.cs` line 112). Unit W8-17a. Contract: `contracts/W8-17a.md`.
//!
//! A second process forwards its paths to the running one (pipe, `pr_platform_win::forward_server`); `pr_core::instance::coalescer::ForwardedOpenCoalescer`
//! merges a burst (Explorer starts N processes for N selected files) into ONE request after 750 ms; the request is posted to the UI thread, where the
//! [`ForwardedOpenHandler`] brings the window to the front and opens the first path.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_core::abstractions::{Log, UiScheduler};
use pr_core::instance::coalescer::{ForwardedOpenCoalescer, OpenCallback};

/// C# `ShellStartup.ForwardCoalesceWindow`.
pub const FORWARD_COALESCE_WINDOW: Duration = Duration::from_millis(750);

/// The C# `InvalidOperationException` of a window that disappeared between the check and the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowGone {
    /// C# `ex.Message`.
    pub message: String,
}

impl fmt::Display for WindowGone {
    /// The message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Error for WindowGone {}

/// C# `IForwardedOpenTarget`: what a forwarded open needs of the main window (implemented by W8-17e over the main window, D-W8B4X-23). UI thread only.
pub trait ForwardedOpenTarget {
    /// The window decided to close or is closed: a late request must not touch it.
    fn is_closing_or_closed(&self) -> bool;
    /// C# `IsMinimized` (can throw `InvalidOperationException` when the window is gone).
    fn is_minimized(&self) -> Result<bool, WindowGone>;
    /// C# `Restore()` (SC_RESTORE: back to the state before minimising, R7-5).
    fn restore(&self) -> Result<(), WindowGone>;
    /// C# `Activate()`.
    fn activate(&self) -> Result<(), WindowGone>;
    /// C# `OpenPathAsync(path)`: starts the open and returns when it has been STARTED (no async in Rust, D-W8-2); a failure after that is reported by the window.
    fn open_path(&self, path: &str) -> Result<(), Box<dyn Error>>;
}

/// C# `ForwardedOpenHandler`. UI thread only.
pub struct ForwardedOpenHandler {
    target: Rc<dyn ForwardedOpenTarget>,
    log: Arc<dyn Log>,
    ignored: Cell<usize>,
}

impl fmt::Debug for ForwardedOpenHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwardedOpenHandler").finish_non_exhaustive()
    }
}

impl ForwardedOpenHandler {
    /// C# constructor.
    pub fn new(target: Rc<dyn ForwardedOpenTarget>, log: Arc<dyn Log>) -> Self {
        todo!()
    }

    /// C# `IgnoredCount`: requests skipped because the window is closing or gone.
    pub fn ignored_count(&self) -> usize {
        todo!()
    }

    /// C# `HandleAsync(path)`: a path-less request only brings the window to the front.
    pub fn handle(&self, path: Option<&str>) {
        todo!()
    }
}

/// C# `ShellStartup.OnForwardedOpen` + its three fields (`_forwardHandler`, `_hasPendingForward`, `_pendingForwardPath`): before the window exists the first request
/// is remembered (the first path wins), after it the request goes to the handler. UI thread only.
pub struct ForwardedOpenRouter {
    handler: RefCell<Option<Rc<ForwardedOpenHandler>>>,
    /// `None` = nothing pending; `Some(path)` = a request arrived (its path may be `None`).
    pending: RefCell<Option<Option<String>>>,
}

impl fmt::Debug for ForwardedOpenRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwardedOpenRouter").finish_non_exhaustive()
    }
}

impl ForwardedOpenRouter {
    /// A router with no handler and nothing pending.
    pub fn new() -> Rc<Self> {
        todo!()
    }

    /// C# `_forwardHandler = new ForwardedOpenHandler(_main, log)`.
    pub fn set_handler(&self, handler: Rc<ForwardedOpenHandler>) {
        todo!()
    }

    /// C# `OnForwardedOpen(path)`.
    pub fn on_forwarded_open(&self, path: Option<String>) {
        todo!()
    }

    /// C# `if (_hasPendingForward) { _hasPendingForward = false; path = _pendingForwardPath; _pendingForwardPath = null; ... }`: takes the remembered request.
    pub fn take_pending(&self) -> Option<Option<String>> {
        todo!()
    }

    /// Makes this router the one that [`ui_open_callback`] delivers to on the CALLING (UI) thread. Holds a `Weak`.
    pub fn attach_to_current_thread(self: &Rc<Self>) {
        todo!()
    }

    /// Delivers to the router attached to the current thread; nothing when there is none or it was dropped.
    fn deliver_on_current_thread(path: Option<String>) {
        todo!()
    }
}

/// The `OpenCallback` of the coalescer (C# `path => dispatcher.Post(() => OnForwardedOpen(path))`): runs on the coalescer's worker thread, posts a `Send` job to
/// `ui` that calls `ForwardedOpenRouter::deliver_on_current_thread(path)` on the UI thread.
pub fn ui_open_callback(ui: Arc<dyn UiScheduler>) -> OpenCallback {
    todo!()
}

/// C# `new ForwardedOpenCoalescer(path => dispatcher.Post(...), ForwardCoalesceWindow, log)`: the coalescer with the production delay and thread.
pub fn new_forward_coalescer(ui: Arc<dyn UiScheduler>, log: Arc<dyn Log>) -> Arc<ForwardedOpenCoalescer> {
    todo!()
}
